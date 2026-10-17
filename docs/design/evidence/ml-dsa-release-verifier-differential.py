#!/usr/bin/env python3
"""Exercise the unchanged release Wasm using its release node's cached Rust graph.

Build the release node first. This script does not invoke Cargo, alter dependencies,
patch the Wasm, or export a new runtime API. It links the adjacent evidence harness
against exactly the libraries recorded by the newest release-node fingerprint.
"""

import argparse
import hashlib
import json
import subprocess
import tempfile
from pathlib import Path

CRATES = [
    "node_subtensor_runtime",
    "sc_executor",
    "sc_executor_common",
    "codec",
    "sp_io",
    "frame_support",
    "frame_system",
    "sp_core",
    "sp_runtime",
    "subtensor_runtime_common",
    "subtensor_hashed",
    "pallet_limit_orders",
    "pallet_hashed_accounts",
    "pallet_sudo",
    "pallet_subtensor_utility",
    "pallet_subtensor",
    "serde_json",
    "hex",
]


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--root", type=Path, default=Path(__file__).resolve().parents[3]
    )
    parser.add_argument("--wasm", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    root = args.root.resolve()
    wasm = args.wasm.resolve()
    output = args.output.resolve()
    release = root / "target/release"
    fingerprints = release / ".fingerprint"
    node = max(
        fingerprints.glob("node-subtensor-*/bin-node-subtensor.json"),
        key=lambda path: path.stat().st_mtime_ns,
    )
    index = {}
    for path in fingerprints.glob("*/lib-*"):
        if path.suffix:
            continue
        try:
            digest = int.from_bytes(bytes.fromhex(path.read_text()), "little")
        except ValueError:
            continue
        index[digest] = path.with_suffix(".json")
    selected = {}

    def walk(path):
        for _, name, _, digest in json.loads(path.read_text())["deps"]:
            dependency = index.get(digest)
            if dependency is not None and name not in selected:
                selected[name] = dependency
                walk(dependency)

    walk(node)
    source = Path(__file__).with_suffix(".rs").resolve()
    artifacts = {}
    with tempfile.TemporaryDirectory(prefix="mldsa-release-differential-") as directory:
        binary = Path(directory) / "verifier-differential"
        command = [
            "rustc",
            "--edition=2021",
            "-O",
            str(source),
            "-o",
            str(binary),
            "-L",
            f"dependency={release / 'deps'}",
        ]
        for name in CRATES:
            fingerprint = selected[name]
            crate = fingerprint.stem.removeprefix("lib-")
            tag = fingerprint.parent.name.rsplit("-", 1)[1]
            library = release / "deps" / f"lib{crate}-{tag}.rlib"
            assert library.is_file(), library
            command.extend(["--extern", f"{name}={library}"])
            artifacts[name] = {
                "path": str(library.relative_to(root)),
                "sha256": sha256(library),
                "fingerprint": str(fingerprint.relative_to(root)),
            }
        for path in sorted((release / "build").glob("*/out")):
            command.extend(["-L", f"native={path}"])
        subprocess.run(command, cwd=root, check=True)
        subprocess.run([str(binary), str(wasm), str(output)], cwd=root, check=True)
    result = json.loads(output.read_text())
    assert result["wasmSha256"] == sha256(wasm)
    result.update(
        status="PASS",
        caseCount=len(result["cases"]),
        harnessSha256=sha256(source),
        launcherSha256=sha256(Path(__file__).resolve()),
        nativeNodeFingerprint=str(node.relative_to(root)),
        nativeRuntimeFeatures=json.loads(
            selected["node_subtensor_runtime"].read_text()
        )["features"],
        nativeArtifacts=artifacts,
        rustc=subprocess.check_output(["rustc", "--version", "--verbose"], text=True),
    )
    output.write_text(json.dumps(result, indent=2) + "\n")
    print(f"PASS: {result['caseCount']} native/release-Wasm cases; {output}")


if __name__ == "__main__":
    main()
