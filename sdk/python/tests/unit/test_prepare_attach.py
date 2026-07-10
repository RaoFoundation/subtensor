"""The offline signing pipeline: prepare -> export -> sign -> attach.

Everything runs over the golden fixture's recorded metadata — no network. The
critical property: an :class:`UnsignedExtrinsic` that crossed a process
boundary (JSON round-trip, no composed call object at hand) must assemble the
same extrinsic as the in-process path, including for calls that nest other
calls (Proxy.proxy, Utility.batch, Sudo.sudo — where re-encoding a *decoded*
call is impossible and the raw call bytes must be embedded verbatim).
"""

from __future__ import annotations

import json

import pytest

from bittensor._transport import extrinsics as ex
from bittensor._transport.contract import UnsignedExtrinsic
from tests.conftest import GOLDEN_FIXTURE, golden
from tests.conftest import golden_codec as codec

bittensor_core = pytest.importorskip("bittensor_core")
Keypair = bittensor_core.Keypair

pytestmark = pytest.mark.skipif(not GOLDEN_FIXTURE.exists(), reason="golden fixture not recorded")


def _prepare(call, kp, *, metadata_hash=None):
    genesis = golden()["network"]["genesis_hash"]
    return ex.prepare_extrinsic(
        codec(),
        call,
        address=kp.ss58_address,
        public_key=bytes(kp.public_key),
        crypto_type=1,
        era="00",
        nonce=7,
        tip=3,
        genesis_hash=genesis,
        era_block_hash=genesis,
        metadata_hash=metadata_hash,
    )


def _nested_calls():
    g = golden()
    c = codec()
    alice = g["ss58"][0]["address"]
    bob = g["ss58"][1]["address"]
    transfer = c.compose_call("Balances", "transfer_keep_alive", {"dest": bob, "value": 1})
    return {
        "Balances.transfer_keep_alive": transfer,
        "Proxy.proxy": c.compose_call(
            "Proxy", "proxy", {"real": alice, "force_proxy_type": "Transfer", "call": transfer}
        ),
        "Utility.batch": c.compose_call("Utility", "batch", {"calls": [transfer, transfer]}),
        "Sudo.sudo": c.compose_call("Sudo", "sudo", {"call": transfer}),
    }


def test_offline_roundtrip_assembles_nested_calls():
    """prepare -> JSON export -> import -> attach, without the composed call."""
    c = codec()
    kp = Keypair.create_from_uri("//Alice")
    for name, call in _nested_calls().items():
        unsigned = _prepare(call, kp)
        # Cross the machine boundary: JSON text out, JSON text back in.
        imported = UnsignedExtrinsic.from_dict(json.loads(json.dumps(unsigned.to_dict())))
        assert imported == unsigned, f"{name}: JSON round-trip diverged"

        signed = ex.attach_signature(c, imported, kp.sign(imported.payload))
        # The original call bytes must be embedded verbatim (never re-encoded).
        assert signed.data.endswith(imported.call_data), f"{name}: call bytes diverged"
        module, function = name.split(".")
        decoded = c.decode_extrinsic(signed.data)
        assert decoded["call"]["call_module"] == module
        assert decoded["call"]["call_function"] == function


def test_signature_normalization_forms_are_equivalent():
    """64-byte raw, 65-byte version-prefixed, and 0x-hex all assemble the same."""
    c = codec()
    kp = Keypair.create_from_uri("//Alice")
    unsigned = _prepare(_nested_calls()["Balances.transfer_keep_alive"], kp)
    raw = bytes(kp.sign(unsigned.payload))
    variants = [raw, b"\x01" + raw, "0x" + raw.hex()]
    assembled = {ex.attach_signature(c, unsigned, v).data for v in variants}
    assert len(assembled) == 1


def test_payload_json_declares_signed_extensions():
    """Extension signers get the runtime's real extension list, plus the
    Polkadot-JS disabled-mode shape (mode 0, null metadataHash)."""
    kp = Keypair.create_from_uri("//Alice")
    payload = _prepare(_nested_calls()["Balances.transfer_keep_alive"], kp).payload_json
    assert "CheckMetadataHash" in payload["signedExtensions"]
    assert payload["mode"] == 0
    assert payload["metadataHash"] is None


def test_payload_json_pins_polkadot_js_number_shape():
    """Numeric fields are big-endian value hex at the field's SCALE width.

    Polkadot-JS parses hex-string ints as BE numbers; SCALE-compact or LE
    bytes here make the extension sign different bytes than the SDK assembles
    (BadProof on every extension-signed submission). Verified against
    ``registry.createType('ExtrinsicPayload', ...)`` in @polkadot/types: with
    this shape pjs signs bytes identical to ``unsigned.payload``.
    """
    c = codec()
    kp = Keypair.create_from_uri("//Alice")
    call = _nested_calls()["Balances.transfer_keep_alive"]
    payload = _prepare(call, kp).payload_json  # nonce=7, tip=3, immortal
    assert payload["nonce"] == "0x00000007"  # u32 BE, not compact ("0x1c" reads as 28)
    assert payload["tip"] == "0x" + "00" * 15 + "03"  # u128 BE
    assert payload["specVersion"] == "0x" + c.spec_version.to_bytes(4, "big").hex()
    assert payload["transactionVersion"] == "0x" + c.transaction_version.to_bytes(4, "big").hex()
    assert payload["era"] == "0x00"  # era stays raw SCALE hex
    assert payload["method"] == "0x" + c.call_data(call).hex()
    assert payload["version"] == c.extrinsic_version


def test_metadata_hash_mode_survives_roundtrip():
    """The CheckMetadataHash digest and mode byte survive the JSON export."""
    c = codec()
    kp = Keypair.create_from_uri("//Alice")
    call = _nested_calls()["Balances.transfer_keep_alive"]
    digest = bytes(range(32))
    unsigned = _prepare(call, kp, metadata_hash=digest)
    imported = UnsignedExtrinsic.from_dict(json.loads(json.dumps(unsigned.to_dict())))
    assert imported.metadata_hash == digest
    assert imported.payload_json["mode"] == 1
    assert imported.payload_json["metadataHash"] == "0x" + digest.hex()

    signature = kp.sign(imported.payload)
    enabled = ex.attach_signature(c, imported, signature)
    disabled = ex.attach_signature(c, _prepare(call, kp), signature)
    # Only the mode byte differs in the assembled extrinsic; the digest itself
    # is implied data, signed but never transmitted.
    diffs = [i for i, (a, b) in enumerate(zip(enabled.data, disabled.data)) if a != b]
    assert len(diffs) == 1
    assert enabled.data[diffs[0]] == 1 and disabled.data[diffs[0]] == 0
