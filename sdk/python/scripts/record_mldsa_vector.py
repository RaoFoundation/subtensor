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
from bittensor.sp_core import CRYPTO_MLDSA, Keypair


def record(endpoint: str, output: Path) -> None:
    def rpc(method, params):
        body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
        request = urllib.request.Request(endpoint, body, {"Content-Type": "application/json"})
        with urllib.request.urlopen(request, timeout=30) as response:
            result = json.load(response)
        if "error" in result:
            raise RuntimeError(result["error"])
        return result["result"]

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
    key = Keypair.create_from_seed(bytes([43]) * 32, CRYPTO_MLDSA)
    call = codec.compose_call("System", "remark", {"remark": b"ML-DSA Python SDK vector"})
    genesis = "0x" + "00" * 32
    extrinsics = []
    for generation in (0, 1):
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
    fixture = {
        "generator": "sdk/python/scripts/record_mldsa_vector.py",
        "spec_version": version["specVersion"],
        "transaction_version": version["transactionVersion"],
        "metadata_sha256": hashlib.sha256(metadata).hexdigest(),
        "descriptor": bytes(key.hashed_descriptor).hex(),
        "account": bytes(key.public_key).hex(),
        "extrinsics": extrinsics,
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(fixture, indent=2) + "\n")
    print(f"Recorded {len(extrinsics)} ML-DSA SDK transactions in {output}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("endpoint", nargs="?", default="http://127.0.0.1:9944")
    parser.add_argument(
        "--output",
        type=Path,
        default=Path(__file__).resolve().parents[3] / "runtime/tests/fixtures/mldsa-python-v5.json",
    )
    args = parser.parse_args()
    record(args.endpoint, args.output)
