"""Backpressure may repeat submission bytes, never signing or an accepted watch."""

import asyncio
from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest

from bittensor._transport.contract import InclusionReport, SignedExtrinsic
from bittensor._transport.errors import (
    ExtrinsicSubmissionRejected,
    MaxRetriesExceeded,
    SubstrateRequestException,
)
from bittensor._transport.extrinsics import NonceCache
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


@pytest.mark.parametrize(
    "busy",
    [
        BUSY,
        EXECUTION_BUSY,
        {"error": {"code": -32000, "message": "Runtime API error", "data": BUSY}},
    ],
)
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
    assert isinstance(raised.value, ExtrinsicSubmissionRejected)
    assert raised.value.__cause__ is error
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
        SubstrateRequestException(f"subscription disconnected: {BUSY}"),
        MaxRetriesExceeded(f"RPC retries exhausted: {BUSY}"),
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


@pytest.mark.parametrize(
    "code,rejected",
    [
        (1001, True),
        (1002, True),
        (1010, True),
        (1014, True),
        (-32602, True),
        (1013, False),
        (1011, False),
        (-32603, False),
    ],
)
@pytest.mark.parametrize("watch", [False, True])
async def test_only_definite_initial_rpc_rejection_is_classified(transport, code, rejected, watch):
    connection, signed, sleep = transport
    operation = connection._session.subscribe if watch else connection._session.request
    operation.side_effect = SubstrateRequestException({"error": {"code": code, "message": "test"}})
    with pytest.raises(SubstrateRequestException) as raised:
        await connection.submit_extrinsic(signed, wait_for_finalization=watch)
    assert isinstance(raised.value, ExtrinsicSubmissionRejected) is rejected
    operation.assert_awaited_once()
    sleep.assert_not_awaited()


@pytest.mark.parametrize(
    "payload", [BUSY, "RPC session closed", {"error": {"code": 1010, "message": "watch failed"}}]
)
async def test_accepted_watch_is_never_resubmitted_on_error(transport, payload):
    connection, signed, sleep = transport
    error = SubstrateRequestException(payload)
    watch = Watch(error=error)
    connection._session.subscribe.return_value = watch
    with pytest.raises(SubstrateRequestException) as raised:
        await connection.submit_extrinsic(signed, wait_for_finalization=True)
    assert raised.value is error
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


@pytest.mark.parametrize("success", [True, False])
@pytest.mark.parametrize("higher_pending", [False, True])
async def test_inclusion_retires_nonce_without_dropping_concurrent_reservations(
    transport, success, higher_pending
):
    connection, signed, _ = transport
    signed.signer_address, signed.nonce = "account", 3
    connection._nonces = NonceCache(connection._session)
    connection._nonces.pin("account", 3)
    if higher_pending:
        # A concurrent transaction or Shield inner is still pending.
        connection._nonces.pin("account", 4)
    connection._session.subscribe.return_value = Watch({"inBlock": "0xabcd"})
    connection._resolve_inclusion = AsyncMock(
        return_value=InclusionReport(
            extrinsic_hash=signed.extrinsic_hash, block_hash="0xabcd", is_success=success
        )
    )
    await connection.submit_extrinsic(signed, wait_for_inclusion=True)
    # Inclusion consumes the nonce even on dispatch failure. A reaped and
    # refunded account can restart at zero once all reservations are consumed.
    connection._session.request.return_value = 0
    assert await connection._nonces.next_for("account") == (5 if higher_pending else 0)


@pytest.mark.parametrize("mode", ["unwatched", "unknown", "unresolved"])
async def test_unconfirmed_submission_keeps_nonce_reservation(transport, mode):
    connection, signed, _ = transport
    signed.signer_address, signed.nonce = "account", 3
    connection._nonces = NonceCache(connection._session)
    connection._nonces.pin("account", 3)
    if mode == "unwatched":
        connection._session.request.return_value = signed.extrinsic_hash
        await connection.submit_extrinsic(signed)
    else:
        connection._session.subscribe.return_value = Watch(
            {"inBlock": "0xabcd"}, error=TimeoutError() if mode == "unknown" else None
        )
        connection._resolve_inclusion = AsyncMock(side_effect=TimeoutError())
        with pytest.raises(TimeoutError):
            await connection.submit_extrinsic(signed, wait_for_inclusion=True)
    connection._session.request.return_value = 0
    assert await connection._nonces.next_for("account") == 4


async def test_late_completion_cannot_erase_refunded_accounts_new_reservation():
    session = SimpleNamespace(request=AsyncMock(return_value=3))
    cache = NonceCache(session)
    assert await cache.next_for("account") == 3
    previous = cache.token("account")
    await cache.included("account", 3, previous)
    session.request.return_value = 0
    assert await cache.next_for("account") == 0
    await cache.included("account", 3, previous)
    assert await cache.next_for("account") == 1


async def test_inclusion_cannot_clear_reservation_during_an_older_nonce_lookup():
    started, resume = asyncio.Event(), asyncio.Event()

    async def sampled_before_inclusion(*args):
        started.set()
        await resume.wait()
        return 3

    cache = NonceCache(SimpleNamespace(request=sampled_before_inclusion))
    cache.pin("account", 3)
    token = cache.token("account")
    reservation = asyncio.create_task(cache.next_for("account"))
    await started.wait()
    inclusion = asyncio.create_task(cache.included("account", 3, token))
    await asyncio.sleep(0)
    resume.set()
    assert await reservation == 4
    await inclusion
    assert await cache.next_for("account") == 5
