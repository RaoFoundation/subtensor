#!/usr/bin/env python3
"""Verify that a release-train artifact is the runtime finalized on chain."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
import zipfile
from pathlib import Path
from typing import Any


SHA_RE = re.compile(r"^[0-9a-f]{40}$")
HASH_RE = re.compile(r"^(?:0x)?([0-9a-f]{64})$")
MAX_ARCHIVE_SIZE = 100 * 1024 * 1024
MAX_JSON_SIZE = 2 * 1024 * 1024
MAX_WASM_SIZE = 50 * 1024 * 1024
MAX_CALL_DATA_SIZE = 20 * 1024 * 1024


class ValidationError(ValueError):
    """The artifact does not match the expected immutable release identity."""


def _hash(value: str, name: str) -> str:
    match = HASH_RE.fullmatch(value)
    if match is None:
        raise ValidationError(f"{name} must be a lowercase 32-byte hex digest")
    return match.group(1)


def _object(data: bytes, name: str) -> dict[str, Any]:
    if len(data) > MAX_JSON_SIZE:
        raise ValidationError(f"{name} is unexpectedly large")
    try:
        value = json.loads(data)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ValidationError(f"{name} is not valid JSON") from error
    if not isinstance(value, dict):
        raise ValidationError(f"{name} must contain a JSON object")
    return value


def _require_equal(actual: object, expected: object, name: str) -> None:
    if actual != expected:
        raise ValidationError(f"{name} is {actual!r}, expected {expected!r}")


def verify_archive(
    archive: Path,
    *,
    expected_spec: int,
    expected_commit: str,
    expected_code_hash: str,
) -> dict[str, object]:
    """Validate an Actions artifact zip and return its verified identity."""
    if expected_spec < 0:
        raise ValidationError("expected spec_version must be non-negative")
    if SHA_RE.fullmatch(expected_commit) is None:
        raise ValidationError("expected commit must be a lowercase 40-byte Git SHA")
    code_hash = _hash(expected_code_hash, "expected code hash")

    try:
        artifact = zipfile.ZipFile(archive)
    except (OSError, zipfile.BadZipFile) as error:
        raise ValidationError("artifact is not a readable zip file") from error

    with artifact:
        files: dict[str, zipfile.ZipInfo] = {}
        total_size = 0
        for member in artifact.infolist():
            if member.is_dir():
                continue
            name = member.filename
            parts = Path(name).parts
            if (
                name.startswith(("/", "\\"))
                or "\\" in name
                or ".." in parts
                or len(parts) != 1
            ):
                raise ValidationError(f"artifact contains unsafe member {name!r}")
            if name in files:
                raise ValidationError(f"artifact contains duplicate member {name!r}")
            total_size += member.file_size
            files[name] = member
        if total_size > MAX_ARCHIVE_SIZE:
            raise ValidationError("artifact expands beyond the size limit")

        required = {
            "subtensor.wasm",
            "subtensor-digest.json",
            "proxy_proxy_blob.hex",
            "pending-release.json",
            "upgrade-manifest.json",
        }
        missing = sorted(required - files.keys())
        if missing:
            raise ValidationError(f"artifact is missing: {', '.join(missing)}")

        wasm_info = files["subtensor.wasm"]
        if not 0 < wasm_info.file_size <= MAX_WASM_SIZE:
            raise ValidationError("subtensor.wasm has an invalid size")
        wasm = artifact.read(wasm_info)

        call_data_info = files["proxy_proxy_blob.hex"]
        if not 0 < call_data_info.file_size <= MAX_CALL_DATA_SIZE:
            raise ValidationError("proxy_proxy_blob.hex has an invalid size")

        pending = _object(
            artifact.read(files["pending-release.json"]), "pending-release.json"
        )
        digest = _object(
            artifact.read(files["subtensor-digest.json"]), "subtensor-digest.json"
        )
        manifest = _object(
            artifact.read(files["upgrade-manifest.json"]), "upgrade-manifest.json"
        )

    _require_equal(
        pending.get("expected_spec_version"),
        expected_spec,
        "pending release spec_version",
    )
    _require_equal(pending.get("commit"), expected_commit, "pending release commit")

    wasm_sha256 = hashlib.sha256(wasm).hexdigest()
    wasm_code_hash = hashlib.blake2b(wasm, digest_size=32).hexdigest()
    _require_equal(wasm_code_hash, code_hash, "runtime code hash")
    _require_equal(
        _hash(str(digest.get("sha256", "")), "srtool sha256"),
        wasm_sha256,
        "srtool sha256",
    )

    _require_equal(manifest.get("spec_version"), expected_spec, "manifest spec_version")
    _require_equal(manifest.get("commit"), expected_commit, "manifest commit")
    _require_equal(
        _hash(str(manifest.get("wasm_sha256", "")), "manifest wasm_sha256"),
        wasm_sha256,
        "manifest wasm_sha256",
    )

    return {
        "spec_version": expected_spec,
        "commit": expected_commit,
        "code_hash": f"0x{wasm_code_hash}",
        "wasm_sha256": wasm_sha256,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("archive", type=Path)
    parser.add_argument("--spec", type=int, required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--code-hash", required=True)
    arguments = parser.parse_args()
    try:
        result = verify_archive(
            arguments.archive,
            expected_spec=arguments.spec,
            expected_commit=arguments.commit,
            expected_code_hash=arguments.code_hash,
        )
    except ValidationError as error:
        print(f"invalid release artifact: {error}", file=sys.stderr)
        return 1
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
