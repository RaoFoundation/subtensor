import argparse
import hashlib
import json
import subprocess
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument("--root", type=Path, default=Path.cwd())
root = parser.parse_args().root.resolve()
work = Path(__file__).resolve().parent
release = root / "target/release"
fingerprints = release / ".fingerprint"
node = max(
    fingerprints.glob("node-subtensor-*/bin-node-subtensor.json"),
    key=lambda p: p.stat().st_mtime_ns,
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
        dep = index.get(digest)
        if dep is not None and name not in selected:
            selected[name] = dep
            walk(dep)


walk(node)
f = selected["subtensor_hashed"]
crate = f.stem.removeprefix("lib-")
tag = f.parent.name.rsplit("-", 1)[1]
library = release / "deps" / f"lib{crate}-{tag}.rlib"
command = [
    "/home/johnreed/.cargo/bin/rustc",
    "--edition=2021",
    "--crate-type=cdylib",
    "-O",
    str(work / "primitive_bridge.rs"),
    "-o",
    str(work / "libprimitive_bridge.so"),
    "-L",
    f"dependency={release / 'deps'}",
    "--extern",
    f"subtensor_hashed={library}",
]
subprocess.run(command, check=True, cwd=root)


def sha(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()


(work / "bridge-artifacts.json").write_text(
    json.dumps(
        {
            "node_fingerprint": str(node),
            "library": str(library),
            "library_sha256": sha(library),
            "bridge_source_sha256": sha(work / "primitive_bridge.rs"),
            "bridge_binary_sha256": sha(work / "libprimitive_bridge.so"),
            "command": command,
        },
        indent=2,
    )
    + "\n"
)
print("Built exact production-graph bridge", library)
