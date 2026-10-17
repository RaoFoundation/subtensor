"""Compare deployed legacy call bytes with the consolidated candidate metadata.

Usage: python ml-dsa-api-compatibility.py MAINNET_SNAPSHOT RUNTIME_FIXTURE OUTPUT
MAINNET_SNAPSHOT contains block, version and metadata from one finalized block.
This is codec evidence, not proof verification or an on-chain submission.
"""

import hashlib
import json
import sys
from pathlib import Path

from bittensor_core import Runtime


def digest(data):
    return hashlib.sha256(data).hexdigest()


def main():
    snapshot_path, fixture_path, output_path = map(Path, sys.argv[1:])
    snapshot = json.loads(snapshot_path.read_bytes())
    fixture = json.loads(fixture_path.read_bytes())
    old = Runtime(
        bytes.fromhex(snapshot["metadata"][2:]),
        snapshot["version"]["specVersion"],
        snapshot["version"]["transactionVersion"],
        42,
    )
    results = []
    for metadata_version in (14, 15):
        new = Runtime(
            bytes.fromhex(fixture["metadata"][str(metadata_version)][2:]),
            fixture["specVersion"], fixture["transactionVersion"], 42,
        )
        fields = {
            "signer": "0x" + "11" * 32,
            "hotkey": "0x" + "22" * 32,
            "netuid": 42,
            "order_type": "LimitBuy",
            "amount": 1234567890,
            "limit_price": 2000000000,
            "expiry": 2000000000000,
            "fee_rate": 1000000,
            "fee_recipient": "0x" + "33" * 32,
            "relayer": ["0x" + "55" * 32, "0x" + "66" * 32],
            "max_slippage": 42000000,
            "chain_id": 42,
            "partial_fills_enabled": True,
        }
        orders = [{"V1": fields}, {"V2": {
            **fields, "amount": {"Fixed": fields["amount"]}, "has_linked_order": False,
        }}]
        legacy_checks = 0

        def unchanged(module, function, params):
            nonlocal legacy_checks
            deployed = bytes(old.compose_call(module, function, params))
            candidate = bytes(new.compose_call(module, function, params))
            assert deployed == candidate, (module, function)
            left, right = old.decode_call(deployed), new.decode_call(candidate)
            # The declared Shield bound/type name intentionally changes, while
            # indices, values, hash and vector bytes remain identical.
            for decoded in (left, right):
                for argument in decoded["call_args"]:
                    argument.pop("type", None)
            assert left == right
            legacy_checks += 1

        for order in orders:
            unchanged("LimitOrders", "cancel_order", {"order": order})
            for scheme, size in [("Ed25519", 64), ("Sr25519", 64), ("Ecdsa", 65)]:
                for partial in (None, 123):
                    signed = {
                        "order": order, "signature": {scheme: "0x" + "7a" * size},
                        "partial_fill": partial,
                    }
                    unchanged("LimitOrders", "execute_orders", {
                        "orders": [signed], "should_fail": True,
                    })
                    unchanged("LimitOrders", "execute_batched_orders", {
                        "netuid": 42, "orders": [signed],
                    })
        for size in (0, 64, 8192):
            unchanged("MevShield", "submit_encrypted", {"ciphertext": "0x" + "42" * size})

        # An unmodified installed native binding must decode new authorization
        # variants from fresh metadata without a signature enum override.
        new_checks = 0
        for order in orders:
            authorized = {"Authorized": {"nonce": 17, "terms": order}}
            for signature in ("AccountApproved", {"MlDsa": {
                "epoch": 3, "public_key": "0x" + "01" * 1952,
                "signature": "0x" + "02" * 3309,
            }}):
                calls = [
                    ("authorize_order", {"order": authorized}),
                    ("cancel_order", {"order": authorized}),
                    ("execute_orders", {"orders": [{"order": authorized,
                        "signature": signature, "partial_fill": None}], "should_fail": True}),
                    ("execute_batched_orders", {"netuid": 42, "orders": [{
                        "order": authorized, "signature": signature, "partial_fill": 123}]}),
                ]
                for function, params in calls:
                    encoded = bytes(new.compose_call("LimitOrders", function, params))
                    decoded = new.decode_call(encoded)
                    assert decoded["call_function"] == function
                    assert encoded[:2] == bytes([32, {
                        "execute_orders": 0, "execute_batched_orders": 1,
                        "cancel_order": 2, "authorize_order": 5,
                    }[function]])
                    new_checks += 1
        for module, names in {
            "HashedAccounts": ["register_v2", "check_registered_v2"],
            "MevShield": ["submit_encrypted_v2"],
            "LimitOrders": ["execute_orders_v2", "execute_batched_orders_v2", "cancel_order_v2", "authorize_order_v2"],
        }.items():
            actual = next(p for p in new.metadata_ir()["pallets"] if p["name"] == module)
            assert not set(names).intersection(c["name"] for c in actual["calls"])
        results.append({"metadataVersion": metadata_version, "identicalLegacyCalls": legacy_checks,
                        "newAuthorizationCallsDecoded": new_checks, "duplicateCallsAbsent": True})
    result = {
        "finalizedBaselineBlock": snapshot["block"],
        "baselineSpecVersion": snapshot["version"]["specVersion"],
        "baselineMetadataSha256": digest(bytes.fromhex(snapshot["metadata"][2:])),
        "candidateFixtureSha256": digest(fixture_path.read_bytes()),
        "scriptSha256": digest(Path(__file__).read_bytes()),
        "results": results,
        "limitations": ["Codec-only proofs use dummy signatures", "No transaction was submitted",
                        "Current metadata required for new order variants"],
    }
    output_path.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
