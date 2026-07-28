"""CLI entry: regenerate the wire layer from a node's metadata.

Usage:
    python -m codegen [ws-endpoint]

Defaults to a local node. Writes bittensor/_generated/.
"""

from __future__ import annotations

import sys
from pathlib import Path

from .emit_python import write
from .metadata import dump

DEFAULT_ENDPOINT = "ws://127.0.0.1:9944"
OUT_DIR = Path(__file__).resolve().parent.parent / "bittensor" / "_generated"


def main() -> None:
    endpoint = sys.argv[1] if len(sys.argv) > 1 else DEFAULT_ENDPOINT
    print(f"dumping metadata from {endpoint} ...")
    ir = dump(endpoint)
    written = write(ir, OUT_DIR)
    print(f"spec_version {ir.spec_version}: {len(ir.pallets)} pallets")
    for path in written:
        print(f"  wrote {path.relative_to(OUT_DIR.parent.parent)}")


if __name__ == "__main__":
    main()
