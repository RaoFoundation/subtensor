#!/usr/bin/env python3
"""Check the release node's raw runtime RPC admission and exempt read path."""

import argparse
import hashlib
import json
import time
import urllib.request
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--url", default="http://127.0.0.1:9977")
    parser.add_argument("--wasm", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    def rpc(method, params):
        request = urllib.request.Request(
            args.url,
            data=json.dumps(
                {"jsonrpc": "2.0", "id": 1, "method": method, "params": params}
            ).encode(),
            headers={"Content-Type": "application/json"},
        )
        with urllib.request.urlopen(request, timeout=20) as response:
            return json.load(response)

    version = rpc("state_getRuntimeVersion", [])["result"]
    code = bytes.fromhex(rpc("state_getStorage", ["0x3a636f6465"])["result"][2:])
    artifact = args.wasm.read_bytes()
    assert code == artifact, "node is not serving the supplied Wasm artifact"

    started = time.monotonic()
    responses = [
        rpc("state_call", ["MldsaAdmissionProbe_unexported", "0x"]) for _ in range(64)
    ]
    marker = "Local runtime execution capacity reached; retry later"
    limited = sum(marker in json.dumps(response) for response in responses)
    assert limited > 0, "no admission rejection observed; use an idle local node"
    assert all("error" in response for response in responses), responses
    read = rpc("state_call", ["Core_version", "0x"])
    assert "result" in read and read["result"].startswith("0x"), read

    result = {
        "test": "live raw runtime execution admission and read-only availability",
        "scriptSha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        "specVersion": version["specVersion"],
        "transactionVersion": version["transactionVersion"],
        "wasmSha256": hashlib.sha256(artifact).hexdigest(),
        "wasmMatchesOnchainCode": True,
        "rejectedUnknownExports": len(responses),
        "capacityErrors": limited,
        "probeSeconds": round(time.monotonic() - started, 3),
        "readOnlyCoreVersionAfterSaturation": "PASS",
        "limitations": [
            "ML-DSA remains disabled with uncalibrated weights",
            "This checks the raw-RPC admission route, not production throughput or a finalized ML-DSA transaction",
        ],
    }
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
