"""Decode runtime-produced envelopes with the selected installed native binding.

Usage: python ml-dsa-native-client.py FIXTURE [OUTPUT] [--label LABEL] [--sdk-cases FILE]
Select the implementation build with PYTHONPATH, or omit it for an installed
legacy wheel. No client dependency is installed or modified by this script.
"""

import argparse
import hashlib
import importlib.metadata
import json
from pathlib import Path

import bittensor_core


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("fixture", type=Path)
    parser.add_argument("output", type=Path, nargs="?")
    parser.add_argument("--label", default="selected native binding")
    parser.add_argument("--sdk-cases", type=Path)
    args = parser.parse_args()
    fixture_bytes = args.fixture.read_bytes()
    fixture = json.loads(fixture_bytes)
    assert len(fixture["extrinsics"]) == 7
    module_file = Path(bittensor_core.__file__)
    binaries = (
        sorted(module_file.parent.glob("*.so"))
        if module_file.suffix == ".py"
        else [module_file]
    )
    try:
        bittensor_core.Keypair.create_from_seed(bytes([7]) * 32, 7)
        creates_mldsa_keys = True
    except ValueError as error:
        if str(error) != "unsupported signing scheme":
            raise
        creates_mldsa_keys = False
    result = {
        "label": args.label,
        "fixture_sha256": sha256(fixture_bytes),
        "script_sha256": sha256(Path(__file__).read_bytes()),
        "distribution_version": importlib.metadata.version("bittensor-core"),
        "exports_mldsa_crypto_type": hasattr(bittensor_core, "CRYPTO_MLDSA65"),
        "creates_mldsa_keys": creates_mldsa_keys,
        "binding_binaries_sha256": {p.name: sha256(p.read_bytes()) for p in binaries},
        "metadata": {},
    }
    for version in (14, 15, 16):
        try:
            runtime = bittensor_core.Runtime(
                bytes.fromhex(fixture["metadata"][str(version)].removeprefix("0x")),
                fixture["specVersion"],
                fixture["transactionVersion"],
                42,
            )
        except ValueError as error:
            # V16 is an existing unsupported metadata format in these bindings.
            # Any other failure is a regression, not a successful decode result.
            if version != 16 or str(error) != "unsupported metadata version V16":
                raise
            result["metadata"][version] = {"supported": False, "error": str(error)}
            continue
        raw = [bytes.fromhex(tx.removeprefix("0x")) for tx in fixture["extrinsics"]]
        decoded = [runtime.decode_extrinsic(tx, True) for tx in raw]
        for item in decoded:
            assert item["call"]["call_module"] == "System"
            assert item["call"]["call_function"] == "remark"
            assert item["call"]["call_args"][0]["value"] == "hashed wire vector"
        proof = bytes.fromhex(decoded[1]["address"]["Raw"].removeprefix("0x"))
        assert len(proof) == 5367
        assert decoded[1]["signature"]["Ed25519"] == "0x" + "4d" * 64
        assert decoded[1]["nonce"] == 1
        assert decoded[1]["era"] == "00"
        # Block RPC bodies contain separately framed extrinsics. Exercise every
        # ML-DSA position among six unchanged classical/v1/bare transactions.
        others = raw[:1] + raw[2:]
        for position in range(7):
            block = others[:position] + [raw[1]] + others[position:]
            output = [runtime.decode_extrinsic(tx, True) for tx in block]
            assert len(output) == 7
            assert output[position]["address"] == decoded[1]["address"]
            assert all(item["call"] == decoded[0]["call"] for item in output)
        extra = []
        profiles = set()
        if args.sdk_cases:
            cases = json.loads(args.sdk_cases.read_bytes())
            assert cases["fixture_sha256"] == result["fixture_sha256"]
            for case in cases["extrinsics"]:
                wire = bytes.fromhex(case["hex"].removeprefix("0x"))
                item = runtime.decode_extrinsic(wire, True)
                assert item["call"]["call_module"] == case["module"]
                assert item["call"]["call_function"] == case["function"]
                proof = bytes.fromhex(item["address"]["Raw"].removeprefix("0x"))
                assert len(proof) == 5367
                expected = 4 if case["crypto_type"] == 7 else 5
                assert proof[32:34] == bytes([2, expected])
                assert item["signature"]["Ed25519"] == "0x" + "4d" * 64
                profiles.add(expected)
                extra.append(wire)
            # Decode every framed body in a mixed block with both ML-DSA modes.
            mixed = raw + extra
            assert len([runtime.decode_extrinsic(wire, True) for wire in mixed]) == len(mixed)
        result["metadata"][version] = {
            "supported": True,
            "extrinsics_decoded": 7,
            "additional_mode_call_decodes": len(extra),
            "additional_profiles": sorted(profiles),
            "mixed_block_positions": 7,
            "mldsa_raw_length": len(proof),
            "carrier": "Ed25519 4d*64",
            "call": "System.remark",
            "exact_call_preserved": True,
        }
    output = json.dumps(result, indent=2) + "\n"
    if args.output:
        args.output.write_text(output)
    print(output, end="")


if __name__ == "__main__":
    main()
