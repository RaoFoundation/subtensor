#!/usr/bin/env python3
"""Read stable Python release versions and optionally check PyPI availability."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from urllib.error import HTTPError, URLError
from urllib.request import urlopen


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


def check_unpublished(sdk: str, core: str) -> None:
    # Only an explicit 404 proves availability. A registry outage must not
    # turn an unknown publication state into a passing version check.
    for package, version in (("bittensor", sdk), ("bittensor-core", core)):
        url = f"https://pypi.org/pypi/{package}/{version}/json"
        try:
            with urlopen(url, timeout=30) as response:
                status = response.status
        except HTTPError as error:
            status = error.code
        except (OSError, URLError) as error:
            raise ValueError(
                f"could not check PyPI availability for {package} {version}: {error}"
            ) from error
        if status == 404:
            continue
        if status == 200:
            raise ValueError(
                f"{package} {version} is already published; bump the base version."
            )
        raise ValueError(
            f"PyPI availability check for {package} {version} returned HTTP {status}."
        )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("sdk_manifest", type=Path)
    parser.add_argument("core_manifest", type=Path)
    parser.add_argument(
        "--check-unpublished",
        action="store_true",
        help="fail unless both stable versions are available for publication on PyPI",
    )
    arguments = parser.parse_args()
    try:
        sdk, core = release_versions(
            arguments.sdk_manifest.read_text(), arguments.core_manifest.read_text()
        )
        if arguments.check_unpublished:
            check_unpublished(sdk, core)
    except (OSError, ValueError) as error:
        print(f"could not determine release versions: {error}", file=sys.stderr)
        return 1
    print(json.dumps({"sdk": sdk, "core": core}, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
