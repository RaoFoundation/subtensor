"""Property-based tests over the codec: SS58, multisig derivation, and
call encode/decode roundtrips against the golden fixture's real metadata.

The golden suite proves byte-exactness on recorded cases; these prove the
roundtrip invariants hold across the whole input space.
"""

from __future__ import annotations

import pytest
from hypothesis import given, settings
from hypothesis import strategies as st

from bittensor._transport import codec as codec_mod
from tests.conftest import GOLDEN_FIXTURE
from tests.conftest import golden_codec as codec

PUBKEY = st.binary(min_size=32, max_size=32)
U64 = st.integers(min_value=0, max_value=2**64 - 1)

requires_golden = pytest.mark.skipif(
    not GOLDEN_FIXTURE.exists(), reason="golden fixture not recorded"
)


@given(public_key=PUBKEY)
def test_ss58_roundtrip(public_key: bytes):
    address = codec_mod.ss58_encode(public_key, 42)
    assert codec_mod.is_valid_ss58_address(address, 42)
    assert bytes.fromhex(codec_mod.ss58_decode(address)) == public_key


@given(keys=st.lists(PUBKEY, min_size=2, max_size=5, unique=True), data=st.data())
def test_multisig_derivation_is_order_independent(keys: list[bytes], data):
    threshold = data.draw(st.integers(min_value=1, max_value=len(keys)))
    addresses = [codec_mod.ss58_encode(k, 42) for k in keys]
    shuffled = data.draw(st.permutations(addresses))
    account = codec_mod.multisig_account(addresses, threshold)
    account2 = codec_mod.multisig_account(list(shuffled), threshold)
    assert account.ss58_address == account2.ss58_address
    assert account.threshold == threshold
    # The derived account is a valid address distinct from every signatory.
    assert codec_mod.is_valid_ss58_address(account.ss58_address, 42)
    assert account.ss58_address not in addresses


@requires_golden
@settings(max_examples=50)  # each example composes against full runtime metadata
@given(public_key=PUBKEY, value=U64)
def test_transfer_call_encode_decode_roundtrip(public_key: bytes, value: int):
    c = codec()
    dest = codec_mod.ss58_encode(public_key, 42)
    call = c.compose_call("Balances", "transfer_keep_alive", {"dest": dest, "value": value})
    decoded = c.decode_call(c.call_data(call))
    assert decoded["call_module"] == "Balances"
    assert decoded["call_function"] == "transfer_keep_alive"
    args = {a["name"]: a["value"] for a in decoded["call_args"]}
    assert args["value"] == value
    assert args["dest"] == dest


@requires_golden
@settings(max_examples=25)
@given(value=U64, remark=st.binary(min_size=0, max_size=64))
def test_nested_call_roundtrip(value: int, remark: bytes):
    """Nesting (Sudo.sudo around System.remark) survives encode/decode."""
    c = codec()
    inner = c.compose_call("System", "remark", {"remark": "0x" + remark.hex()})
    outer = c.compose_call("Sudo", "sudo", {"call": inner})
    decoded = c.decode_call(c.call_data(outer))
    assert decoded["call_module"] == "Sudo"
    inner_decoded = decoded["call_args"][0]["value"]
    assert inner_decoded["call_module"] == "System"
    assert inner_decoded["call_function"] == "remark"


@requires_golden
@settings(max_examples=25)
@given(value=U64)
def test_compact_encoding_roundtrip(value: int):
    c = codec()
    encoded = c.encode_compact(value)
    assert c.decode("Compact<u64>", encoded) == value
