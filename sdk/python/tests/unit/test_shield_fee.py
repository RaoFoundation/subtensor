"""Carrier sizing must select the same tier as real encrypted transaction bytes."""

from types import SimpleNamespace
from unittest.mock import AsyncMock, Mock

import bittensor_core
import pytest

from bittensor._substrate import RpcSubstrate
from bittensor._transport.extrinsics import SHIELD_ENCRYPTION_OVERHEAD, shielded_ciphertext_size
from bittensor._transport.interface import SubstrateConnection
from bittensor.balance import Balance
from bittensor.client import Client
from bittensor.sp_core import Keypair
from tests.conftest import golden, golden_codec


def connection(key, nonce=63):
    codec = golden_codec()
    conn = SubstrateConnection("ws://unused.invalid")
    conn._runtimes = SimpleNamespace(codec_at=AsyncMock(return_value=codec))
    conn._nonces = SimpleNamespace(next_for=AsyncMock(return_value=nonce))
    genesis = golden()["network"]["genesis_hash"]
    conn.genesis_hash = AsyncMock(return_value=genesis)
    conn._normalize_era = AsyncMock(return_value=({"period": 8, "current": 100}, genesis))
    # A public wallet must never be asked for a real signature while quoting.
    public = SimpleNamespace(
        ss58_address=key.ss58_address,
        public_key=key.public_key,
        crypto_type=key.crypto_type,
        sign=Mock(side_effect=AssertionError("fee quote requested a signature")),
    )
    return conn, codec, public


def inner_bytes(codec, call, key, nonce):
    # Independently produce a real signed envelope, then encrypt it with the native implementation.
    return codec.encode_signed_extrinsic(
        call,
        public_key=bytes(key.public_key),
        signature=key.sign(b"length fixture"),
        signature_version=key.crypto_type,
        era={"period": 8, "current": 100},
        nonce=nonce,
        tip=0,
        tip_asset_id=None,
    )[0]


@pytest.mark.parametrize("crypto", [0, 1])
@pytest.mark.parametrize("carrier_size", [8191, 8192, 8193, 32768, 32769])
async def test_quote_matches_native_encryption_at_tier_and_carrier_boundaries(crypto, carrier_size):
    key = Keypair.create_from_seed(bytes([41]) * 32, crypto)
    conn, codec, public = connection(key)
    size = carrier_size - SHIELD_ENCRYPTION_OVERHEAD - 120
    for _ in range(4):
        call = codec.compose_call("System", "remark", {"remark": bytes([42]) * size})
        signed = inner_bytes(codec, call, key, 64)
        delta = carrier_size - SHIELD_ENCRYPTION_OVERHEAD - len(signed)
        if not delta:
            break
        size += delta
    encrypted = bittensor_core.encrypt_mlkem768(bytes(1184), signed, include_key_hash=True)
    assert len(encrypted) == carrier_size
    composed = []
    compose = conn.compose_call

    async def capture(module, function, params):
        composed.append((module, function, params))
        return await compose(module, function, params)

    conn.compose_call = capture
    conn.get_payment_info = AsyncMock(return_value={"partial_fee": 17})
    if carrier_size > 32768:
        with pytest.raises(ValueError, match="32768-byte carrier limit"):
            await conn.get_shielded_payment_info(call, public, period=8)
        conn.get_payment_info.assert_not_awaited()
        assert not composed
    else:
        assert await conn.get_shielded_payment_info(call, public, period=8) == {"partial_fee": 17}
        module, function, params = composed[0]
        assert (module, function) == ("MevShield", "submit_encrypted")
        assert params["ciphertext"] == bytes(carrier_size)
        assert (len(params["ciphertext"]) <= 8192) == (len(encrypted) <= 8192)
        assert conn.get_payment_info.call_args.kwargs == {"nonce": 63, "era": {"period": 8}}
    public.sign.assert_not_called()
    conn._nonces.next_for.assert_awaited_once_with(key.ss58_address, use_cache=False)
    conn._normalize_era.assert_awaited_once_with({"period": 8})


async def test_rpc_quote_only_sends_dummy_carrier_and_uses_mortal_outer_nonce(monkeypatch):
    key = Keypair.create_from_seed(bytes([41]) * 32, 1)
    conn, codec, public = connection(key)
    call = codec.compose_call("System", "remark", {"remark": b"private shield call"})
    conn._session.request = AsyncMock(return_value="0x")
    monkeypatch.setattr(
        "bittensor._transport.interface.decode_runtime_api_result",
        lambda *args: {"partial_fee": 17},
    )
    expected_ciphertext = bytes(shielded_ciphertext_size(len(inner_bytes(codec, call, key, 64))))
    outer = codec.compose_call("MevShield", "submit_encrypted", {"ciphertext": expected_ciphertext})
    expected = codec.encode_signed_extrinsic(
        outer,
        public_key=bytes(key.public_key),
        signature=bytes(64),
        signature_version=1,
        era={"period": 8, "current": 100},
        nonce=63,
        tip=0,
        tip_asset_id=None,
    )[0]
    assert await conn.get_shielded_payment_info(call, public, period=8) == {"partial_fee": 17}
    conn._session.request.assert_awaited_once_with(
        "state_call",
        [
            "TransactionPaymentApi_query_info",
            (expected + len(expected).to_bytes(4, "little")).hex(),
            None,
        ],
    )
    assert b"private shield call" not in expected
    public.sign.assert_not_called()


async def test_client_and_rpc_adapter_forward_actual_call_and_period():
    key = Keypair.create_from_seed(bytes([41]) * 32, 1)
    conn, codec, _ = connection(key)
    call = codec.compose_call("System", "remark", {"remark": b"quote me"})
    conn.get_shielded_payment_info = AsyncMock(return_value={"partial_fee": 17})
    conn.get_constant = AsyncMock(return_value=None)
    substrate = RpcSubstrate("ws://unused.invalid")
    substrate._substrate = conn
    client = Client("local", substrate=substrate)
    assert await client.estimate_shielded_carrier_fee(
        key.ss58_address, call, period=16
    ) == Balance.from_rao(17)
    args = conn.get_shielded_payment_info.call_args
    assert args.args[0] == call
    assert args.args[1].ss58_address == key.ss58_address
    assert args.kwargs == {"period": 16}
