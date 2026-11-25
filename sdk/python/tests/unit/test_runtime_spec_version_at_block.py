"""A rule that depends on the runtime version must read that version at the block of its
other reads."""

from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest

from bittensor._substrate import RpcSubstrate
from bittensor._transport.errors import SubstrateRequestException
from bittensor._transport.interface import SubstrateConnection


@pytest.mark.asyncio
async def test_spec_version_at_a_block_reads_the_runtime_in_the_state_of_that_block():
    connection = SubstrateConnection("ws://unused")
    connection._runtimes = SimpleNamespace(codec_at=AsyncMock())
    connection._session = SimpleNamespace(
        request=AsyncMock(return_value={"specVersion": 494, "transactionVersion": 1})
    )

    assert await connection.runtime_spec_version("0xblock") == 494

    # The state of the block, not the state of its parent: this runtime executes the next
    # block, which is the earliest block that can hold a new transaction.
    connection._session.request.assert_awaited_once_with("state_getRuntimeVersion", ["0xblock"])
    connection._runtimes.codec_at.assert_not_awaited()


@pytest.mark.asyncio
async def test_spec_version_without_a_block_uses_the_head_codec():
    connection = SubstrateConnection("ws://unused")
    connection._runtimes = SimpleNamespace(
        codec_at=AsyncMock(return_value=SimpleNamespace(spec_version=495))
    )
    connection._session = SimpleNamespace(request=AsyncMock())

    assert await connection.runtime_spec_version() == 495

    connection._runtimes.codec_at.assert_awaited_once_with(None)
    connection._session.request.assert_not_awaited()


@pytest.mark.asyncio
async def test_spec_version_at_an_unknown_block_is_an_error():
    connection = SubstrateConnection("ws://unused")
    connection._session = SimpleNamespace(request=AsyncMock(return_value=None))

    with pytest.raises(SubstrateRequestException, match="No runtime information"):
        await connection.runtime_spec_version("0xmissing")


@pytest.mark.asyncio
async def test_spec_version_adapter_preserves_the_requested_block():
    substrate = RpcSubstrate("ws://unused")
    raw = SimpleNamespace(runtime_spec_version=AsyncMock(return_value=494))

    async def read(operation):
        return await operation(raw)

    substrate._read = read
    assert await substrate.spec_version("0xblock") == 494
    raw.runtime_spec_version.assert_awaited_once_with("0xblock")
