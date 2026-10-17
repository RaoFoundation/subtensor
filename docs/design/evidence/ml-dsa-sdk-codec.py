"""Verify consolidated SDK calls against runtime metadata and a legacy fixture.

Usage: python ml-dsa-sdk-codec.py RUNTIME_ENVELOPE_FIXTURE [OUTPUT]
Run with the branch's native Python extension and SDK on PYTHONPATH. This proves
encoding, proof assembly, and strict decoding; it does not submit transactions.
The test keys use fixed public development seeds, never user wallet material.
"""

import asyncio
import hashlib
import json
import sys
from pathlib import Path
from types import SimpleNamespace

import bittensor_core

from bittensor._transport.codec import RuntimeCodec, strip_option_opaque_metadata
from bittensor.hashed import registration_descriptor_value
from bittensor.intents.limit_orders import (
    ExecuteOrders,
    ExecuteBatchedOrders,
    CancelOrder,
    AuthorizeOrder,
)
from bittensor.limit_orders import prepare_order, SignedOrder
from bittensor.sp_core import Keypair, ROLE_ORDER, ss58_decode, verify_mldsa_authorization


def order(owner):
    return {
        "V1": {
            "signer": owner,
            "hotkey": owner,
            "netuid": 1,
            "order_type": "LimitBuy",
            "amount": 100,
            "limit_price": 1_000_000_000,
            "expiry": 2_000_000_000_000,
            "fee_rate": 0,
            "fee_recipient": owner,
            "relayer": [owner] * 10,
            "max_slippage": 1000,
            "chain_id": 42,
            "partial_fills_enabled": True,
        }
    }


def golden_codec():
    path = Path(__file__).resolve().parents[3] / "sdk/python/tests/fixtures/golden.json"
    golden = json.loads(path.read_text())
    metadata = strip_option_opaque_metadata(
        bytes.fromhex(golden["metadata"]["v15_hex"][2:])
    )
    return RuntimeCodec(
        metadata,
        spec_version=golden["network"]["spec_version"],
        transaction_version=golden["network"]["transaction_version"],
    )


fixture_path = Path(sys.argv[1])
fixture = json.loads(fixture_path.read_text())
codec = RuntimeCodec(
    bytes.fromhex(fixture["metadata"]["15"].removeprefix("0x")),
    spec_version=fixture["specVersion"],
    transaction_version=fixture["transactionVersion"],
)
# Polkadot.js rejects fixed arrays larger than 2048 before block-body decoding.
# Keep this global registry check so another detached-proof field cannot quietly
# reintroduce the failure while ordinary transaction vectors continue to pass.
registry = codec.registry_types()
fixed_array_lengths = [
    item["type"]["def"]["array"]["len"]
    for item in registry
    if "array" in item["type"]["def"]
]
assert max(fixed_array_lengths, default=0) <= 2048
proof_type = next(
    item["id"]
    for item in registry
    if item["type"].get("path", [])[-1:] == ["MlDsaOrderProof"]
)
old = golden_codec()
key = Keypair.create_from_seed(bytes([7]) * 32, 7)
genesis = "0x" + "11" * 32
calls = []
for crypto in [4, 6, 7, 8]:
    signer = Keypair.create_from_seed(bytes([crypto]) * 32, crypto)
    descriptor = registration_descriptor_value(bytes(signer.hashed_descriptor))
    for method, index in [("register", 0), ("check_registered", 1)]:
        call = codec.compose_call("HashedAccounts", method, {"descriptor": descriptor})
        assert call.data[1:3] == bytes([index, int(crypto in (7, 8))])
        assert call.data[3:] == bytes(signer.hashed_descriptor)
        calls.append(("HashedAccounts", method, call))


async def compose(call):
    return codec.compose_call(*call)


async def block_hash(_):
    return genesis


substrate = SimpleNamespace(compose=compose, block_hash=block_hash)


async def main():
    prepared = await prepare_order(substrate, 17, order(key.ss58_address))
    approved = prepared.account_approved().value()
    proof = {
        "epoch": 3,
        "public_key": bytes(key.mldsa_capability_public_key(ROLE_ORDER, 3)),
        "signature": bytes(
            key.sign_mldsa_order(
                3,
                17,
                prepared.genesis_hash,
                prepared.encoded,
                owner=bytes(ss58_decode(key.ss58_address)),
            )
        ),
    }
    # Detached signatures are bounded byte vectors in call metadata. They keep
    # an exact cryptographic size, but include SCALE's compact sequence length.
    encoded_proof = codec.encode(f"scale_info::{proof_type}", proof)
    compact_signature_length = codec.encode_compact(3309)
    assert encoded_proof == (
        proof["epoch"].to_bytes(8, "little")
        + proof["public_key"]
        + compact_signature_length
        + proof["signature"]
    )
    signed = SignedOrder(prepared, {"MlDsa": proof}).value()
    legacy = {
        "order": order(key.ss58_address),
        "signature": {"Sr25519": "0x" + "22" * 64},
        "partial_fill": None,
    }
    legacy_matches = []
    for method, args, index in [
        ("execute_orders", {"orders": [legacy], "should_fail": True}, 0),
        ("execute_batched_orders", {"netuid": 1, "orders": [legacy]}, 1),
        ("cancel_order", {"order": legacy["order"]}, 2),
    ]:
        call = codec.compose_call("LimitOrders", method, args)
        assert call.data == old.compose_call("LimitOrders", method, args).data
        assert call.data[1] == index
        legacy_matches.append(method)
    for intent in [
        AuthorizeOrder(17, prepared.order),
        CancelOrder(17, prepared.order),
        ExecuteOrders([legacy, approved, signed]),
        ExecuteBatchedOrders([legacy, approved, signed], True, 1),
    ]:
        call = await intent.build(substrate, key)
        calls.append(("LimitOrders", intent.op, call))
    for n in [1, 8192, 8193, 32768]:
        call = codec.compose_call(
            "MevShield", "submit_encrypted", {"ciphertext": bytes(n)}
        )
        assert call.data[1] == 1
        if n <= 8192:
            assert (
                call.data
                == old.compose_call(
                    "MevShield", "submit_encrypted", {"ciphertext": bytes(n)}
                ).data
            )
        calls.append(("MevShield", "submit_encrypted", call))
    extrinsics = []
    for crypto in (7, 8):
        signer = Keypair.create_from_seed(bytes([7]) * 32, crypto)
        for module, method, call in calls:
            pieces = codec.signature_payload_parts(
                call,
                era="00",
                nonce=0,
                tip=0,
                tip_asset_id=None,
                genesis_hash=genesis,
                era_block_hash=genesis,
            )
            payload = b"".join(pieces)
            proof = signer.sign_mldsa(payload)
            assert verify_mldsa_authorization(proof, payload, signer.hashed_descriptor, 0,
                                             signer.hashed_current_commitment)
            raw, transaction_hash = codec.encode_signed_extrinsic(
                call,
                public_key=signer.public_key,
                signature=proof,
                signature_version=crypto,
                era="00",
                nonce=0,
                tip=0,
                tip_asset_id=None,
            )
            decoded = codec.decode_extrinsic(raw, strict=True)
            assert (
                decoded["call"]["call_module"] == module
                and decoded["call"]["call_function"] == method
            )
            extrinsics.append(
                {
                    "crypto_type": crypto,
                    "module": module,
                    "function": method,
                    "hex": "0x" + raw.hex(),
                    "hash": transaction_hash,
                }
            )
    result = {
        "fixture_sha256": hashlib.sha256(fixture_path.read_bytes()).hexdigest(),
        "script_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        "native_extension_sha256": hashlib.sha256(
            Path(bittensor_core.__file__).read_bytes()
        ).hexdigest(),
        "legacy_metadata_sha256": hashlib.sha256(old.metadata_bytes).hexdigest(),
        "consolidated_call_mldsa_roundtrips": len(extrinsics),
        "metadata_v15_max_fixed_array": max(fixed_array_lengths, default=0),
        "order_signature_encoding": "bounded byte vector, exactly 3309 signature bytes",
        "order_signature_compact_prefix_hex": compact_signature_length.hex(),
        "legacy_byte_identical_order_calls": legacy_matches,
        "registration_crypto_types": [4, 6, 7, 8],
        "authorization_profiles": [4, 5],
        "valid_authorization_proofs": len(extrinsics),
        "shield_original_index": 1,
        "shield_sizes": [1, 8192, 8193, 32768],
        "extrinsics": extrinsics,
    }
    text = json.dumps(result, indent=2) + "\n"
    if len(sys.argv) > 2:
        Path(sys.argv[2]).write_text(text)
    print(
        json.dumps(
            {key: value for key, value in result.items() if key != "extrinsics"},
            indent=2,
        )
    )


asyncio.run(main())
