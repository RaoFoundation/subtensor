"""Backpressure may repeat submission bytes, never signing or an accepted watch."""

import asyncio
from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest

from bittensor._transport.contract import InclusionReport, SignedExtrinsic
from bittensor._transport.errors import SubstrateRequestException
from bittensor._transport.interface import SubstrateConnection

BUSY = "Local transaction admission capacity reached; retry later"
EXECUTION_BUSY = "Local runtime execution capacity reached; retry later"


@pytest.fixture
def transport(monkeypatch):
    connection = object.__new__(SubstrateConnection)
    connection._session = SimpleNamespace(request=AsyncMock(), subscribe=AsyncMock())
    sleep = AsyncMock()
    monkeypatch.setattr("bittensor._transport.interface.asyncio.sleep", sleep)
    return connection, SignedExtrinsic(b"unchanged signed envelope", "0x1234"), sleep


class Watch:
    def __init__(self, update=None, error=None):
        self.update, self.error = update, error
        self.unsubscribe = AsyncMock()

    async def __aiter__(self):
        if self.error is not None:
            raise self.error
        yield {"result": self.update}


@pytest.mark.parametrize("busy", [BUSY, EXECUTION_BUSY])
@pytest.mark.parametrize("mode", ["raw", "included", "finalized"])
async def test_rejected_submission_retries_identical_bytes(transport, busy, mode):
    connection, signed, sleep = transport
    watch = Watch({"finalized" if mode == "finalized" else "inBlock": "0xabcd"})
    operation = connection._session.request if mode == "raw" else connection._session.subscribe
    operation.side_effect = [SubstrateRequestException(busy), watch if mode != "raw" else "0x1234"]
    expected = InclusionReport(extrinsic_hash=signed.extrinsic_hash)
    connection._resolve_inclusion = AsyncMock(return_value=expected)
    result = await connection.submit_extrinsic(
        signed, wait_for_inclusion=mode == "included", wait_for_finalization=mode == "finalized"
    )
    assert result.extrinsic_hash == signed.extrinsic_hash
    assert operation.await_count == 2
    assert operation.await_args_list[0] == operation.await_args_list[1]
    assert operation.await_args_list[0].args[1] == [signed.data_hex]
    sleep.assert_awaited_once_with(0.125)
    if mode != "raw":
        watch.unsubscribe.assert_awaited_once()
        connection._resolve_inclusion.assert_awaited_once_with(
            signed.extrinsic_hash, "0xabcd", mode == "finalized"
        )


@pytest.mark.parametrize("watch", [False, True])
async def test_retries_retain_original_payload_if_caller_mutates_object(transport, watch):
    connection, signed, sleep = transport
    original_hex, original_hash = signed.data_hex, signed.extrinsic_hash
    operation = connection._session.subscribe if watch else connection._session.request
    operation.side_effect = [
        SubstrateRequestException(BUSY),
        Watch({"finalized": "0xabcd"}) if watch else original_hash,
    ]

    async def mutate_during_backoff(_delay):
        signed.data = b"different transaction"
        signed.extrinsic_hash = "0x5678"

    sleep.side_effect = mutate_during_backoff
    connection._resolve_inclusion = AsyncMock(
        return_value=InclusionReport(extrinsic_hash=original_hash)
    )
    result = await connection.submit_extrinsic(signed, wait_for_finalization=watch)
    assert result.extrinsic_hash == original_hash
    assert operation.await_count == 2
    assert all(call.args[1] == [original_hex] for call in operation.await_args_list)
    if watch:
        connection._resolve_inclusion.assert_awaited_once_with(original_hash, "0xabcd", True)


@pytest.mark.parametrize("watch", [False, True])
async def test_persistent_admission_rejection_has_bounded_backoff(transport, watch):
    connection, signed, sleep = transport
    operation = connection._session.subscribe if watch else connection._session.request
    error = SubstrateRequestException(BUSY)
    operation.side_effect = error
    with pytest.raises(SubstrateRequestException) as raised:
        await connection.submit_extrinsic(signed, wait_for_finalization=watch)
    assert raised.value is error
    assert operation.await_count == 5
    assert [call.args[0] for call in sleep.await_args_list] == [0.125, 0.25, 0.5, 1.0]
    assert all(call == operation.await_args_list[0] for call in operation.await_args_list)


@pytest.mark.parametrize("watch", [False, True])
@pytest.mark.parametrize(
    "error",
    [
        SubstrateRequestException("Invalid Transaction: BadProof"),
        TimeoutError("unknown outcome"),
        SubstrateRequestException("connection closed"),
        asyncio.CancelledError(),
    ],
)
async def test_other_errors_and_ambiguous_sends_are_never_retried(transport, watch, error):
    connection, signed, sleep = transport
    operation = connection._session.subscribe if watch else connection._session.request
    operation.side_effect = error
    with pytest.raises(type(error)) as raised:
        await connection.submit_extrinsic(signed, wait_for_finalization=watch)
    assert raised.value is error
    assert operation.await_count == 1
    sleep.assert_not_awaited()


async def test_accepted_watch_is_never_resubmitted_on_error(transport):
    connection, signed, sleep = transport
    watch = Watch(error=SubstrateRequestException(BUSY))
    connection._session.subscribe.return_value = watch
    with pytest.raises(SubstrateRequestException, match=BUSY):
        await connection.submit_extrinsic(signed, wait_for_finalization=True)
    connection._session.subscribe.assert_awaited_once()
    watch.unsubscribe.assert_awaited_once()
    sleep.assert_not_awaited()


async def test_cancellation_during_backoff_stops_submission(transport):
    connection, signed, sleep = transport
    connection._session.request.side_effect = SubstrateRequestException(BUSY)
    sleep.side_effect = asyncio.CancelledError()
    with pytest.raises(asyncio.CancelledError):
        await connection.submit_extrinsic(signed)
    connection._session.request.assert_awaited_once()
