#!/usr/bin/env python3
"""Plan SDK release-candidate publication from stable PyPI base availability."""

from __future__ import annotations

import argparse
import re
import sys
from collections.abc import Callable
from pathlib import Path
from urllib.error import HTTPError, URLError
from urllib.parse import quote
from urllib.request import Request, urlopen


SDK_VERSION_RE = re.compile(
    r'^version = "([0-9]+\.[0-9]+\.[0-9]+)\.dev0"$', re.MULTILINE
)
CORE_VERSION_RE = re.compile(r'^version = "([0-9]+\.[0-9]+\.[0-9]+)"$', re.MULTILINE)
RUN_NUMBER_RE = re.compile(r"^[1-9][0-9]*$")


class ApiError(RuntimeError):
    """PyPI could not provide authoritative package-version state."""


class InconsistentPackageState(RuntimeError):
    """Only one of the SDK and core stable bases is published."""


def base_versions(sdk_manifest: str, core_manifest: str) -> tuple[str, str]:
    """Read the stable SDK and core bases from their project manifests."""
    sdk_match = SDK_VERSION_RE.search(sdk_manifest)
    if sdk_match is None:
        raise ValueError("SDK project.version must be X.Y.Z.dev0")
    core_match = CORE_VERSION_RE.search(core_manifest)
    if core_match is None:
        raise ValueError("core project.version must be X.Y.Z")
    return sdk_match.group(1), core_match.group(1)


def _fetch_status(url: str) -> int:
    request = Request(
        url,
        headers={"User-Agent": "subtensor-release-train/1"},
    )
    try:
        with urlopen(request, timeout=30) as response:
            return response.status
    except HTTPError as error:
        return error.code
    except (OSError, URLError) as error:
        raise ApiError(f"could not fetch {url}: {error}") from error


def version_is_published(
    package: str,
    version: str,
    *,
    base_url: str = "https://pypi.org",
    fetch_status: Callable[[str], int] = _fetch_status,
) -> bool:
    """Return whether PyPI has reserved this exact package version."""
    url = (
        f"{base_url.rstrip('/')}/pypi/"
        f"{quote(package, safe='')}/{quote(version, safe='')}/json"
    )
    status = fetch_status(url)
    if status == 200:
        return True
    if status == 404:
        return False
    raise ApiError(f"{url} returned HTTP {status}")


def publication_outputs(
    sdk_base: str,
    core_base: str,
    run_number: str,
    *,
    is_published: Callable[[str, str], bool] = version_is_published,
) -> list[str]:
    """Return the GitHub job outputs for the consistent package state."""
    if RUN_NUMBER_RE.fullmatch(run_number) is None:
        raise ValueError("run number must be a positive integer")

    sdk_published = is_published("bittensor", sdk_base)
    core_published = is_published("bittensor-core", core_base)
    if sdk_published != core_published:
        raise InconsistentPackageState(
            "one Python stable base is published and the other is available; "
            "bump the published package base before publishing another rc"
        )
    if sdk_published:
        return ["publish=false"]
    return [
        "publish=true",
        f"sdk={sdk_base}rc{run_number}",
        f"core={core_base}rc{run_number}",
    ]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("sdk_manifest", type=Path)
    parser.add_argument("core_manifest", type=Path)
    parser.add_argument("--run-number", required=True)
    parser.add_argument("--base-url", default="https://pypi.org")
    arguments = parser.parse_args()
    try:
        sdk_base, core_base = base_versions(
            arguments.sdk_manifest.read_text(),
            arguments.core_manifest.read_text(),
        )
        outputs = publication_outputs(
            sdk_base,
            core_base,
            arguments.run_number,
            is_published=lambda package, version: version_is_published(
                package,
                version,
                base_url=arguments.base_url,
            ),
        )
    except (OSError, ApiError, InconsistentPackageState, ValueError) as error:
        print(f"could not plan Python rc publication: {error}", file=sys.stderr)
        return 1

    if outputs == ["publish=false"]:
        print(
            f"Stable bittensor {sdk_base} and bittensor-core {core_base} "
            "are already published; skipping SDK rc publication.",
            file=sys.stderr,
        )
    print(*outputs, sep="\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
