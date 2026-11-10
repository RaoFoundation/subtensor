"""Exact storage presence must not confuse a missing key with a stored default."""

from types import SimpleNamespace
from unittest.mock import AsyncMock, Mock

import pytest

from bittensor._substrate import RpcSubstrate
from bittensor._transport.interface import SubstrateConnection


@pytest.mark.parametrize("value_hash,present", [(None, False), ("0x" + "00" * 32, True)])
@pytest.mark.asyncio
async def test_storage_presence_uses_exact_key_hash_without_decoding_defaults(value_hash, present):
    connection = SubstrateConnection("ws://unused")
    codec = Mock()
    codec.storage_entry.return_value = SimpleNamespace(param_types=["AccountId32"])
    codec.storage_key.return_value = b"owner-key"
    connection._runtimes = SimpleNamespace(codec_at=AsyncMock(return_value=codec))
    connection._session = SimpleNamespace(request=AsyncMock(return_value=value_hash))

    assert (
        await connection.storage_exists("SubtensorModule", "Owner", ["hotkey"], "0xblock")
        is present
    )
    codec.storage_key.assert_called_once_with(codec.storage_entry.return_value, ["hotkey"])
    connection._session.request.assert_awaited_once_with(
        "state_getStorageHash", ["0x6f776e65722d6b6579", "0xblock"]
    )
    codec.decode.assert_not_called()


@pytest.mark.asyncio
async def test_storage_presence_rejects_partial_keys_before_rpc():
    connection = SubstrateConnection("ws://unused")
    codec = Mock()
    codec.storage_entry.return_value = SimpleNamespace(param_types=["AccountId32"])
    connection._runtimes = SimpleNamespace(codec_at=AsyncMock(return_value=codec))
    connection._session = SimpleNamespace(request=AsyncMock())
    with pytest.raises(ValueError, match="requires 1 parameters"):
        await connection.storage_exists("SubtensorModule", "Owner", [], "0xblock")
    connection._session.request.assert_not_awaited()


@pytest.mark.asyncio
async def test_storage_presence_adapter_preserves_the_requested_block():
    substrate = RpcSubstrate("ws://unused")
    raw = SimpleNamespace(storage_exists=AsyncMock(return_value=True))

    async def read(operation):
        return await operation(raw)

    substrate._read = read
    assert await substrate.storage_exists("SubtensorModule", "Owner", ["hotkey"], "0xblock")
    raw.storage_exists.assert_awaited_once_with("SubtensorModule", "Owner", ["hotkey"], "0xblock")
