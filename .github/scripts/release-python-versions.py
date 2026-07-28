#!/usr/bin/env python3
"""Read the stable Python versions expected from an immutable release commit."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path


SDK_VERSION_RE = re.compile(
    r'^version = "([0-9]+\.[0-9]+\.[0-9]+)\.dev0"$', re.MULTILINE
)
CORE_VERSION_RE = re.compile(r'^version = "([0-9]+\.[0-9]+\.[0-9]+)"$', re.MULTILINE)


def release_versions(sdk_manifest: str, core_manifest: str) -> tuple[str, str]:
    sdk = SDK_VERSION_RE.search(sdk_manifest)
    if sdk is None:
        raise ValueError("SDK manifest must declare version X.Y.Z.dev0")
    core = CORE_VERSION_RE.search(core_manifest)
    if core is None:
        raise ValueError("core manifest must declare stable version X.Y.Z")
    return sdk.group(1), core.group(1)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("sdk_manifest", type=Path)
    parser.add_argument("core_manifest", type=Path)
    arguments = parser.parse_args()
    try:
        sdk, core = release_versions(
            arguments.sdk_manifest.read_text(), arguments.core_manifest.read_text()
        )
    except (OSError, ValueError) as error:
        print(f"could not determine release versions: {error}", file=sys.stderr)
        return 1
    print(json.dumps({"sdk": sdk, "core": core}, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
