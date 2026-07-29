#!/usr/bin/env python3
"""Validate the Rust SDK E2E manifest and split every test into balanced shards."""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

EXPECTED_TESTS = 109
DEFAULT_SHARDS = 32
TEST_NAME = re.compile(r"^(?:intent|test)_[A-Za-z0-9_]+$")


def main() -> int:
    if len(sys.argv) not in (3, 4):
        print(f"usage: {sys.argv[0]} MANIFEST OUTPUT_FILE [SHARDS]", file=sys.stderr)
        return 2

    manifest_path = Path(sys.argv[1])
    output_path = Path(sys.argv[2])
    shard_count = int(sys.argv[3]) if len(sys.argv) == 4 else DEFAULT_SHARDS
    if shard_count < 1 or shard_count > EXPECTED_TESTS:
        raise SystemExit(f"invalid shard count: {shard_count}")

    payload = json.loads(manifest_path.read_text(encoding="utf-8"))
    if not isinstance(payload, list) or len(payload) != EXPECTED_TESTS:
        raise SystemExit(
            f"expected {EXPECTED_TESTS} Rust E2E manifest entries, got "
            f"{len(payload) if isinstance(payload, list) else 'non-list'}"
        )

    names: list[str] = []
    for index, entry in enumerate(payload):
        if not isinstance(entry, dict) or not isinstance(entry.get("test"), str):
            raise SystemExit(f"manifest entry {index} has no string test name")
        name = entry["test"]
        if TEST_NAME.fullmatch(name) is None:
            raise SystemExit(f"unsafe Rust E2E test name: {name!r}")
        names.append(name)

    if len(set(names)) != len(names):
        raise SystemExit("Rust E2E manifest contains duplicate test names")

    shards: list[list[str]] = [[] for _ in range(shard_count)]
    for index, name in enumerate(names):
        shards[index % shard_count].append(name)

    matrix = {
        "include": [
            {"shard": index + 1, "tests": tests}
            for index, tests in enumerate(shards)
            if tests
        ]
    }
    with output_path.open("a", encoding="utf-8") as output:
        output.write(f"test_count={len(names)}\n")
        output.write(f"shard_count={len(matrix['include'])}\n")
        output.write(f"test_matrix={json.dumps(matrix, separators=(',', ':'))}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
