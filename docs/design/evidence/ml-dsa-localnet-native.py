"""Decode every extrinsic from captured finalized blocks with a native SDK.

Usage: python SCRIPT CAPTURE.json OUTPUT.json LABEL
Select the current or installed legacy binding with PYTHONPATH.
"""

import hashlib
import json
import sys
from pathlib import Path

import bittensor_core

capture = Path(sys.argv[1]).read_bytes()
data = json.loads(capture)
module = Path(bittensor_core.__file__)
binaries = [module] if module.suffix == ".so" else sorted(module.parent.glob("*.so"))
output = {
    "label": sys.argv[3],
    "capture_sha256": hashlib.sha256(capture).hexdigest(),
    "binding_sha256": {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in binaries},
    "six_wallet_block": data["six_wallet_block"],
    "metadata": {},
}
for version in (14, 15, 16):
    try:
        codec = bittensor_core.Runtime(
            bytes.fromhex(data["metadata"][str(version)][2:]),
            data["runtime"]["specVersion"],
            data["runtime"]["transactionVersion"],
            42,
        )
    except ValueError as error:
        assert version == 16 and str(error) == "unsupported metadata version V16"
        output["metadata"][version] = {"supported": False, "reason": str(error)}
        continue
    count, profiles = 0, set()
    for block in data["blocks"].values():
        for wire in block["block"]["extrinsics"]:
            raw = bytes.fromhex(wire[2:])
            decoded = codec.decode_extrinsic(raw, True)
            tx_hash = "0x" + hashlib.blake2b(raw, digest_size=32).hexdigest()
            if known := data["submitted"].get(tx_hash):
                profiles.add(known["crypto_type"])
                assert decoded["call"]["call_module"] == "System"
                assert decoded["call"]["call_function"] == "remark"
                if known["crypto_type"] in (7, 8):
                    proof = bytes.fromhex(decoded["address"]["Raw"][2:])
                    assert len(proof) == 5367
                    assert decoded["signature"]["Ed25519"] == "0x" + "4d" * 64
            count += 1
    assert profiles == {0, 1, 4, 6, 7, 8}
    output["metadata"][version] = {"extrinsics_decoded": count, "wallet_types": sorted(profiles)}
Path(sys.argv[2]).write_text(json.dumps(output, indent=2) + "\n")
print(json.dumps(output, indent=2))
