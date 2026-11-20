"""Record Python SDK transaction bytes for the native Executive integration test.

Run against a node built from this tree. The all-43 seed and zero genesis are
public test inputs, never a wallet or network configuration for real funds.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import urllib.request
from pathlib import Path

from bittensor._transport.codec import RuntimeCodec, strip_option_opaque_metadata
from bittensor._transport.extrinsics import create_hashed_extrinsic
from bittensor.sp_core import CRYPTO_HASHED, CRYPTO_HASHED_ED25519, Keypair


def record(
    endpoint: str,
    output: Path,
    metadata_file: Path | None = None,
    spec_version: int | None = None,
    transaction_version: int = 1,
) -> None:
    def rpc(method, params):
        body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
        request = urllib.request.Request(endpoint, body, {"Content-Type": "application/json"})
        with urllib.request.urlopen(request, timeout=30) as response:
            result = json.load(response)
        if "error" in result:
            raise RuntimeError(result["error"])
        return result["result"]

    if metadata_file is not None:
        if spec_version is None:
            raise ValueError("--spec-version is required with --metadata-file")
        version = {"specVersion": spec_version, "transactionVersion": transaction_version}
        metadata = metadata_file.read_bytes()
    else:
        version = rpc("state_getRuntimeVersion", [])
        metadata = strip_option_opaque_metadata(
            bytes.fromhex(rpc("state_call", ["Metadata_metadata_at_version", "0x0f000000"])[2:])
        )
    assert metadata is not None
    codec = RuntimeCodec(
        metadata,
        spec_version=version["specVersion"],
        transaction_version=version["transactionVersion"],
    )
    wallets = []
    for crypto in (CRYPTO_HASHED, CRYPTO_HASHED_ED25519):
        key = Keypair.create_from_seed(bytes([43]) * 32, crypto)
        genesis = "0x" + "00" * 32
        extrinsics = []
        calls = [
            codec.compose_call("System", "remark", {"remark": b"hashed Python SDK v4 vector"}),
            codec.compose_call(
                "Balances",
                "transfer_keep_alive",
                {"dest": "0x" + "63" * 32, "value": 1_000_000_000},
            ),
            codec.compose_call(
                "Balances", "transfer_keep_alive", {"dest": "0x" + "63" * 32, "value": 2**64 - 1}
            ),
        ]
        for generation, call in enumerate(calls):
            signed = create_hashed_extrinsic(
                codec,
                call,
                key.at_generation(generation),
                era="00",
                nonce=generation + 1,
                tip=0,
                tip_asset_id=None,
                genesis_hash=genesis,
                era_block_hash=genesis,
            )
            extrinsics.append(signed.data.hex())
        wallets.append(
            {
                "crypto_type": crypto,
                "descriptor": bytes(key.hashed_descriptor).hex(),
                "account": bytes(key.public_key).hex(),
                "extrinsics": extrinsics,
            }
        )
    fixture = {
        "generator": "sdk/python/scripts/record_hashed_vectors.py",
        "spec_version": version["specVersion"],
        "transaction_version": version["transactionVersion"],
        "metadata_sha256": hashlib.sha256(metadata).hexdigest(),
        "wallets": wallets,
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(fixture, indent=2) + "\n")
    print(f"Recorded {len(wallets)} hashed SDK wallet vectors in {output}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("endpoint", nargs="?", default="http://127.0.0.1:9944")
    parser.add_argument(
        "--output",
        type=Path,
        default=Path(__file__).resolve().parents[3]
        / "runtime/tests/fixtures/hashed-python-v4.json",
    )
    parser.add_argument("--metadata-file", type=Path)
    parser.add_argument("--spec-version", type=int)
    parser.add_argument("--transaction-version", type=int, default=1)
    args = parser.parse_args()
    record(
        args.endpoint, args.output, args.metadata_file, args.spec_version, args.transaction_version
    )
