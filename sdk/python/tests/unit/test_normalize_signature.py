"""External MultiSignature inputs retain their scheme and complete signature."""

from __future__ import annotations

import json
from hashlib import blake2b

import pytest
from eth_keys import keys

from bittensor import sp_core
from bittensor._transport import extrinsics as ex
from bittensor._transport.contract import UnsignedExtrinsic
from tests.conftest import golden, golden_codec


@pytest.mark.parametrize("crypto_type, length", [(0, 64), (1, 64), (2, 65)])
@pytest.mark.parametrize("prefixed", [False, True])
@pytest.mark.parametrize("representation", ["bytes", "hex", "0x-hex"])
def test_raw_and_prefixed_signature_forms(crypto_type, length, prefixed, representation):
    raw = b"\x7f" * length
    signature = bytes([crypto_type]) + raw if prefixed else raw
    if representation != "bytes":
        signature = ("0x" if representation == "0x-hex" else "") + signature.hex()

    assert ex._normalize_signature(signature, crypto_type) == (raw, crypto_type)


@pytest.mark.parametrize("first_byte", [0, 1, 2, 255])
def test_raw_ecdsa_leading_byte_is_not_a_variant(first_byte):
    raw = bytes([first_byte]) + bytes(range(64))
    assert ex._normalize_signature(raw, sp_core.CRYPTO_ECDSA) == (raw, sp_core.CRYPTO_ECDSA)


@pytest.mark.parametrize("crypto_type", [0, 1])
def test_classical_signature_prefix_overrides_default(crypto_type):
    raw = b"\x7f" * 64
    assert ex._normalize_signature(bytes([crypto_type]) + raw, 1 - crypto_type) == (
        raw,
        crypto_type,
    )


@pytest.mark.parametrize(
    "signature, crypto_type",
    [(b"", 1), (b"\x01" * 63, 1), (b"\xff" * 65, 1), (b"\x00" * 66, 2)],
)
def test_unrecognized_signature_shape_is_not_reinterpreted(signature, crypto_type):
    assert ex._normalize_signature(signature, crypto_type) == (signature, crypto_type)


# Issue #3306 completely blocked native transfers signed by external ECDSA keys.
# PROTECTED_REGRESSION: changes require explicit human approval.
@pytest.mark.parametrize("prefixed", [False, True])
@pytest.mark.parametrize("as_hex", [False, True])
def test_external_ecdsa_transfer_survives_prepare_sign_attach(prefixed, as_hex):
    codec = golden_codec()
    network = golden()["network"]
    recipient = golden()["ss58"][1]["address"]
    private_key = keys.PrivateKey(b"\x01" * 32)
    compressed_public = private_key.public_key.to_compressed_bytes()
    account = blake2b(compressed_public, digest_size=32).digest()
    address = sp_core.ecdsa_ss58_encode(compressed_public, network["ss58_format"])
    call = codec.compose_call("Balances", "transfer_keep_alive", {"dest": recipient, "value": 123})
    prepared = ex.prepare_extrinsic(
        codec,
        call,
        address=address,
        public_key=account,
        crypto_type=sp_core.CRYPTO_ECDSA,
        era="00",
        nonce=7,
        tip=3,
        genesis_hash=network["genesis_hash"],
        era_block_hash=network["genesis_hash"],
    )
    unsigned = UnsignedExtrinsic.from_dict(json.loads(json.dumps(prepared.to_dict())))
    digest = blake2b(unsigned.payload, digest_size=32).digest()
    raw = private_key.sign_msg_hash(digest).to_bytes()
    assert len(raw) == 65
    signature = b"\x02" + raw if prefixed else raw
    if as_hex:
        signature = "0x" + signature.hex()

    signed = ex.attach_signature(codec, unsigned, signature)
    decoded = codec.decode_extrinsic(signed.data)
    assert decoded["address"] == signed.signer_address == address
    assert decoded["nonce"] == signed.nonce == 7
    assert decoded["tip"] == 3
    assert decoded["signature"] == {"Ecdsa": "0x" + raw.hex()}
    assert decoded["call"]["call_module"] == "Balances"
    assert decoded["call"]["call_function"] == "transfer_keep_alive"
    assert {arg["name"]: arg["value"] for arg in decoded["call"]["call_args"]} == {
        "dest": recipient,
        "value": 123,
    }
    assert signed.data.endswith(unsigned.call_data)
    attached_signature = keys.Signature(bytes.fromhex(decoded["signature"]["Ecdsa"][2:]))
    recovered = attached_signature.recover_public_key_from_msg_hash(digest)
    assert recovered == private_key.public_key
    assert blake2b(recovered.to_compressed_bytes(), digest_size=32).digest() == account
