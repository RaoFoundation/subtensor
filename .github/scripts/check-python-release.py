#!/usr/bin/env python3
"""Check whether the complete, attested stable Python release exists on PyPI."""

from __future__ import annotations

import argparse
import base64
import json
import re
import sys
from collections.abc import Callable
from typing import Any
from urllib.error import HTTPError, URLError
from urllib.parse import quote
from urllib.request import Request, urlopen


VERSION_RE = re.compile(r"^[0-9]+\.[0-9]+\.[0-9]+$")
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
MAX_RESPONSE_SIZE = 10 * 1024 * 1024
PUBLISH_PREDICATE = "https://docs.pypi.org/attestations/publish/v1"
STATEMENT_TYPE = "https://in-toto.io/Statement/v1"


class ApiError(RuntimeError):
    """PyPI could not provide authoritative state."""


class IncompleteRelease(RuntimeError):
    """One or more required, correctly attested files are not yet on PyPI."""


def fetch_json(url: str, *, timeout: int = 30) -> dict[str, Any] | None:
    request = Request(
        url,
        headers={
            "Accept": "application/json",
            "User-Agent": "subtensor-release-watcher/1",
        },
    )
    try:
        with urlopen(request, timeout=timeout) as response:
            data = response.read(MAX_RESPONSE_SIZE + 1)
    except HTTPError as error:
        if error.code == 404:
            return None
        raise ApiError(f"{url} returned HTTP {error.code}") from error
    except (OSError, URLError) as error:
        raise ApiError(f"could not fetch {url}: {error}") from error
    if len(data) > MAX_RESPONSE_SIZE:
        raise ApiError(f"{url} exceeded the response size limit")
    try:
        value = json.loads(data)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ApiError(f"{url} did not return valid JSON") from error
    if not isinstance(value, dict):
        raise ApiError(f"{url} did not return a JSON object")
    return value


def _valid_file(entry: object, *, package_type: str) -> bool:
    if not isinstance(entry, dict):
        return False
    digest = entry.get("digests")
    return (
        entry.get("packagetype") == package_type
        and entry.get("yanked") is False
        and isinstance(entry.get("filename"), str)
        and isinstance(digest, dict)
        and isinstance(digest.get("sha256"), str)
        and SHA256_RE.fullmatch(digest["sha256"]) is not None
    )


def _core_platform(filename: str, version: str) -> str | None:
    prefix = f"bittensor_core-{version}-"
    if not filename.startswith(prefix) or not filename.endswith(".whl"):
        return None
    wheel_tags = filename[len(prefix) : -4].split("-", 2)
    if len(wheel_tags) != 3 or wheel_tags[0] != "cp310" or wheel_tags[1] != "abi3":
        return None
    platform = wheel_tags[2]
    if "manylinux" in platform and platform.endswith("_x86_64"):
        return "linux-x86_64"
    if "manylinux" in platform and platform.endswith("_aarch64"):
        return "linux-aarch64"
    if platform.startswith("macosx_") and platform.endswith("_x86_64"):
        return "macos-x86_64"
    if platform.startswith("macosx_") and platform.endswith("_arm64"):
        return "macos-arm64"
    return None


def _release_files(
    metadata: dict[str, Any], *, package: str
) -> dict[str, dict[str, Any]]:
    entries = metadata.get("urls")
    if not isinstance(entries, list):
        raise ApiError(f"PyPI metadata for {package} has no file list")

    files: dict[str, dict[str, Any]] = {}
    for entry in entries:
        if not isinstance(entry, dict):
            raise ApiError(f"PyPI metadata for {package} has an invalid file entry")
        if entry.get("yanked") is not False:
            filename = entry.get("filename", "<unknown>")
            raise IncompleteRelease(f"{package} has yanked distribution {filename}")
        package_type = entry.get("packagetype")
        if package_type not in {"bdist_wheel", "sdist"} or not _valid_file(
            entry, package_type=package_type
        ):
            filename = entry.get("filename", "<unknown>")
            raise IncompleteRelease(
                f"{package} has invalid non-yanked distribution metadata for {filename}"
            )
        filename = entry["filename"]
        if filename in files:
            raise IncompleteRelease(
                f"{package} has duplicate non-yanked distribution {filename}"
            )
        files[filename] = entry
    return files


def _required_files(
    sdk_metadata: dict[str, Any], core_metadata: dict[str, Any], sdk: str, core: str
) -> dict[str, dict[str, Any]]:
    required: dict[str, dict[str, Any]] = {}
    expected_sdk = {
        f"bittensor-{sdk}-py3-none-any.whl": "bdist_wheel",
        f"bittensor-{sdk}.tar.gz": "sdist",
    }
    sdk_files = _release_files(sdk_metadata, package="bittensor")
    if set(sdk_files) != set(expected_sdk):
        missing = sorted(set(expected_sdk) - set(sdk_files))
        unexpected = sorted(set(sdk_files) - set(expected_sdk))
        details = []
        if missing:
            details.append(f"missing {', '.join(missing)}")
        if unexpected:
            details.append(f"unexpected {', '.join(unexpected)}")
        raise IncompleteRelease(
            f"bittensor {sdk} distribution set is incomplete or unsafe: "
            + "; ".join(details)
        )
    for filename, package_type in expected_sdk.items():
        entry = sdk_files[filename]
        if entry["packagetype"] != package_type:
            raise IncompleteRelease(
                f"{filename} has package type {entry['packagetype']}, expected {package_type}"
            )
        required[f"sdk:{filename}"] = entry

    core_sdist = f"bittensor_core-{core}.tar.gz"
    core_files = _release_files(core_metadata, package="bittensor-core")
    core_sdist_entry = core_files.pop(core_sdist, None)
    if core_sdist_entry is None:
        raise IncompleteRelease(f"missing non-yanked {core_sdist}")
    if core_sdist_entry["packagetype"] != "sdist":
        raise IncompleteRelease(
            f"{core_sdist} has package type {core_sdist_entry['packagetype']}, expected sdist"
        )
    required[f"core:{core_sdist}"] = core_sdist_entry

    expected_platforms = {
        "linux-x86_64",
        "linux-aarch64",
        "macos-x86_64",
        "macos-arm64",
    }
    platform_files: dict[str, dict[str, Any]] = {}
    unexpected_core: list[str] = []
    for filename, entry in sorted(core_files.items()):
        platform = (
            _core_platform(filename, core)
            if entry["packagetype"] == "bdist_wheel"
            else None
        )
        if platform is None or platform not in expected_platforms:
            unexpected_core.append(filename)
            continue
        if platform in platform_files:
            raise IncompleteRelease(
                f"multiple non-yanked bittensor-core {platform} cp310-abi3 wheels"
            )
        platform_files[platform] = entry
    if unexpected_core:
        raise IncompleteRelease(
            "bittensor-core has unexpected non-yanked distributions: "
            + ", ".join(unexpected_core)
        )
    missing_platforms = sorted(expected_platforms - set(platform_files))
    if missing_platforms:
        raise IncompleteRelease(
            "missing non-yanked bittensor-core cp310-abi3 wheels for "
            + ", ".join(missing_platforms)
        )
    for platform, entry in sorted(platform_files.items()):
        required[f"core:{platform}"] = entry
    return required


def _matching_publish_attestation(
    provenance: dict[str, Any],
    *,
    filename: str,
    sha256: str,
    repository: str,
    workflow: str,
    environment: str,
) -> bool:
    if provenance.get("version") != 1:
        return False
    bundles = provenance.get("attestation_bundles")
    if not isinstance(bundles, list):
        return False
    for bundle in bundles:
        if not isinstance(bundle, dict):
            continue
        publisher = bundle.get("publisher")
        if not isinstance(publisher, dict) or any(
            (
                publisher.get("kind") != "GitHub",
                publisher.get("repository") != repository,
                publisher.get("workflow") != workflow,
                publisher.get("environment") != environment,
            )
        ):
            continue
        attestations = bundle.get("attestations")
        if not isinstance(attestations, list):
            continue
        for attestation in attestations:
            try:
                encoded = attestation["envelope"]["statement"]
                statement = json.loads(base64.b64decode(encoded, validate=True))
            except (
                KeyError,
                TypeError,
                ValueError,
                UnicodeDecodeError,
                json.JSONDecodeError,
            ):
                continue
            if not isinstance(statement, dict):
                continue
            if (
                statement.get("_type") != STATEMENT_TYPE
                or statement.get("predicateType") != PUBLISH_PREDICATE
            ):
                continue
            subjects = statement.get("subject")
            if not isinstance(subjects, list):
                continue
            for subject in subjects:
                if (
                    isinstance(subject, dict)
                    and subject.get("name") == filename
                    and isinstance(subject.get("digest"), dict)
                    and subject["digest"].get("sha256") == sha256
                ):
                    return True
    return False


def check_release(
    *,
    sdk_version: str,
    core_version: str,
    repository: str,
    workflow: str,
    environment: str,
    base_url: str = "https://pypi.org",
    get_json: Callable[[str], dict[str, Any] | None] = fetch_json,
) -> list[str]:
    for version in (sdk_version, core_version):
        if VERSION_RE.fullmatch(version) is None:
            raise ValueError(f"{version!r} is not a stable X.Y.Z version")

    sdk_url = f"{base_url}/pypi/bittensor/{quote(sdk_version, safe='')}/json"
    core_url = f"{base_url}/pypi/bittensor-core/{quote(core_version, safe='')}/json"
    sdk_metadata = get_json(sdk_url)
    core_metadata = get_json(core_url)
    if sdk_metadata is None:
        raise IncompleteRelease(f"bittensor {sdk_version} is not published")
    if core_metadata is None:
        raise IncompleteRelease(f"bittensor-core {core_version} is not published")

    required = _required_files(sdk_metadata, core_metadata, sdk_version, core_version)
    verified: list[str] = []
    for key, entry in sorted(required.items()):
        package = "bittensor" if key.startswith("sdk:") else "bittensor-core"
        filename = entry["filename"]
        provenance_url = (
            f"{base_url}/integrity/{package}/"
            f"{quote(sdk_version if package == 'bittensor' else core_version, safe='')}/"
            f"{quote(filename, safe='')}/provenance"
        )
        provenance = get_json(provenance_url)
        if provenance is None or not _matching_publish_attestation(
            provenance,
            filename=filename,
            sha256=entry["digests"]["sha256"],
            repository=repository,
            workflow=workflow,
            environment=environment,
        ):
            raise IncompleteRelease(
                f"{filename} has no matching trusted-publisher attestation"
            )
        verified.append(filename)
    return verified


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--sdk-version", required=True)
    parser.add_argument("--core-version", required=True)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--workflow", default="watch-mainnet-release.yml")
    parser.add_argument("--environment", default="mainnet")
    parser.add_argument("--base-url", default="https://pypi.org")
    arguments = parser.parse_args()
    try:
        verified = check_release(
            sdk_version=arguments.sdk_version,
            core_version=arguments.core_version,
            repository=arguments.repository,
            workflow=arguments.workflow,
            environment=arguments.environment,
            base_url=arguments.base_url.rstrip("/"),
        )
    except IncompleteRelease as error:
        print(f"Python release incomplete: {error}", file=sys.stderr)
        return 1
    except (ApiError, ValueError) as error:
        print(f"could not verify Python release: {error}", file=sys.stderr)
        return 2
    print(f"verified {len(verified)} attested Python release files")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
