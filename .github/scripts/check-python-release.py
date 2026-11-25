#!/usr/bin/env python3
"""Check whether the complete, attested stable Python release exists on PyPI."""

from __future__ import annotations

import argparse
import base64
import hashlib
import io
import json
import re
import subprocess
import sys
import tarfile
from collections.abc import Callable
from functools import lru_cache
from pathlib import Path
from typing import Any
from urllib.error import HTTPError, URLError
from urllib.parse import quote, urlsplit
from urllib.request import Request, urlopen


VERSION_RE = re.compile(r"^[0-9]+\.[0-9]+\.[0-9]+$")
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
# Stable publishes are attested by one of these workflow files, always on the
# mainnet environment. release-train.yml publishes a runtime's stable packages
# once mainnet runs that runtime, under the approval that proposed it; its rc
# uploads use the testnet environment and never count here. The watcher keeps
# its publisher for manual retries. publish-python-stable.yml is the extra path
# for a release with no runtime upgrade. TestPyPI workflows are absent on
# purpose.
PUBLISHER_WORKFLOWS = (
    "release-train.yml",
    "watch-mainnet-release.yml",
    "publish-python-stable.yml",
)
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
    workflows: tuple[str, ...],
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
                publisher.get("workflow") not in workflows,
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
    workflows: tuple[str, ...],
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
            workflows=workflows,
            environment=environment,
        ):
            raise IncompleteRelease(
                f"{filename} has no matching trusted-publisher attestation"
            )
        verified.append(filename)
    return verified


# Compare shipped inputs to the actual stable sources, not the PR parent or
# the uploader's workflow commit (manual publication can build another ref).
CORE_SOURCES = (
    "sdk/bittensor-core/src/",
    "sdk/bittensor-core-py/src/",
    "primitives/hashed/src/",
    "support/macros/src/",
    "vendor/fips204/src/",
    "vendor/w3f-bls/src/",
)
CORE_MANIFESTS = ("sdk/bittensor-core/Cargo.toml", "sdk/bittensor-core-py/Cargo.toml")


def download_sdist(entry: dict[str, Any]) -> bytes:
    url = entry.get("url")
    if (
        not isinstance(url, str)
        or urlsplit(url).netloc != "files.pythonhosted.org"
        or urlsplit(url).scheme != "https"
    ):
        raise ApiError("invalid PyPI source distribution URL")
    with urlopen(url, timeout=30) as response:
        data = response.read(MAX_RESPONSE_SIZE + 1)
    if (
        len(data) > MAX_RESPONSE_SIZE
        or hashlib.sha256(data).hexdigest() != entry["digests"]["sha256"]
    ):
        raise ApiError("PyPI source distribution size or digest mismatch")
    return data


def source_matches(data: bytes, filename: str, *, sdk: bool) -> bool:
    import tomllib

    prefix = "sdk/python/" if sdk else ""
    roots = ("bittensor/",) if sdk else CORE_SOURCES
    sources = (
        subprocess.check_output(
            ["git", "ls-files", "-z", "--", *(prefix + path for path in roots)]
        )
        .decode()
        .split("\0")
    )
    local = {
        path.removeprefix(prefix): Path(path).read_bytes() for path in sources if path
    }

    def manifest(data: bytes, path: str) -> dict[str, Any]:
        value = tomllib.loads(data.decode())
        if path.endswith("Cargo.toml"):
            # Maturin can add this metadata to the bindings manifest.
            value["package"].pop("readme", None)
        elif sdk:
            value["project"]["version"] = value["project"]["version"].removesuffix(
                ".dev0"
            )
            # The stable SDK build strips only the monorepo source override.
            value["tool"].get("uv", {}).pop("sources", None)
            if not value["tool"].get("uv"):
                value["tool"].pop("uv", None)
        else:
            # Maturin relocates the Python manifest to the sdist root.
            value["tool"]["maturin"].pop("manifest-path", None)
        return value

    manifests = ("pyproject.toml",) if sdk else ("pyproject.toml", *CORE_MANIFESTS)
    root = filename.removesuffix(".tar.gz") + "/"
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
        published = {}
        for member in archive:
            if not member.name.startswith(root):
                continue
            path = member.name.removeprefix(root)
            if not member.isfile() or not (path.startswith(roots) or path in manifests):
                continue
            if path in published or member.size > MAX_RESPONSE_SIZE:
                raise ApiError("invalid member in PyPI source distribution")
            with archive.extractfile(member) as stream:
                published[path] = stream.read()
    for path in manifests:
        current = (
            "sdk/bittensor-core-py/pyproject.toml"
            if not sdk and path == "pyproject.toml"
            else prefix + path
        )
        if path not in published or manifest(published.pop(path), path) != manifest(
            Path(current).read_bytes(), path
        ):
            return False
    return local == published


def check_source(
    *,
    sdk_version: str,
    core_version: str,
    repository: str,
    workflows: tuple[str, ...],
    environment: str,
    base_url: str,
) -> str:
    if not all(VERSION_RE.fullmatch(v) for v in (sdk_version, core_version)):
        raise ValueError("Python versions must be stable X.Y.Z versions")
    get_json = lru_cache(maxsize=None)(fetch_json)
    sdk = get_json(f"{base_url}/pypi/bittensor/{sdk_version}/json")
    core = get_json(f"{base_url}/pypi/bittensor-core/{core_version}/json")
    if sdk is None and core is None:
        return "Both Python versions are available for a new release."
    if sdk is None or core is None:
        raise IncompleteRelease(
            "the paired release needs two unused versions; one is already on PyPI"
        )

    check_release(
        sdk_version=sdk_version,
        core_version=core_version,
        repository=repository,
        workflows=workflows,
        environment=environment,
        base_url=base_url,
        get_json=get_json,
    )
    for metadata, package, filename, is_sdk in (
        (sdk, "bittensor", f"bittensor-{sdk_version}.tar.gz", True),
        (core, "bittensor-core", f"bittensor_core-{core_version}.tar.gz", False),
    ):
        entry = _release_files(metadata, package=package)[filename]
        if not source_matches(download_sdist(entry), filename, sdk=is_sdk):
            raise IncompleteRelease(
                f"shipped {package} sources differ from PyPI; bump both Python package versions and add release notes"
            )
    return (
        "Shipped Python sources match the published pair; no new Python release needed."
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--sdk-version", required=True)
    parser.add_argument("--core-version", required=True)
    parser.add_argument("--repository", required=True)
    parser.add_argument(
        "--workflow",
        action="append",
        default=None,
        help="Trusted publisher workflow filename. Repeat to allow more than one.",
    )
    parser.add_argument("--environment", default="mainnet")
    parser.add_argument("--base-url", default="https://pypi.org")
    parser.add_argument(
        "--check-source",
        action="store_true",
        help="Require a fresh pair or verify the published pair matches this checkout.",
    )
    arguments = parser.parse_args()
    try:
        check = check_source if arguments.check_source else check_release
        verified = check(
            sdk_version=arguments.sdk_version,
            core_version=arguments.core_version,
            repository=arguments.repository,
            workflows=tuple(arguments.workflow or PUBLISHER_WORKFLOWS),
            environment=arguments.environment,
            base_url=arguments.base_url.rstrip("/"),
        )
    except IncompleteRelease as error:
        print(f"Python release incomplete: {error}", file=sys.stderr)
        return 1
    except (
        ApiError,
        ValueError,
        OSError,
        tarfile.TarError,
        subprocess.CalledProcessError,
    ) as error:
        print(f"could not verify Python release: {error}", file=sys.stderr)
        return 2
    print(
        verified
        if arguments.check_source
        else f"verified {len(verified)} attested Python release files"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
