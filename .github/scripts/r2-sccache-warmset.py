#!/usr/bin/env python3
"""Capture exact sccache keys and publish a bounded R2 host-warm manifest."""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Iterable, Mapping
from urllib.parse import urlparse


ACCOUNT_HOST = "3dc4cebb791314d78848969042fb3382.r2.cloudflarestorage.com"
BUCKET = "subtensor-ci-sccache"
REGION = "auto"
COMPILER_PREFIX = "subtensor/v1"
MANIFEST_PREFIX = "sccache-warmsets/v1"
MAX_WARM_BYTES = 4 * 1024 * 1024 * 1024
MAX_OBJECTS = 50_000
MAX_KEY_FILE_BYTES = 8 * 1024 * 1024
MAX_INVENTORY_BYTES = 256 * 1024 * 1024
MAX_MANIFEST_BYTES = 8 * 1024 * 1024
MANIFEST_TTL_SECONDS = 36 * 60 * 60
HASH = re.compile(r"^[0-9a-f]{64}$")
HASH_LOG = re.compile(r"Hash key: ([0-9a-f]{64})(?:\s|$)")
COMMIT_SHA = re.compile(r"^[0-9a-f]{40}$")


class WarmsetError(Exception):
    """A safe publisher error that never contains credential material."""


def require_environment(name: str) -> str:
    value = os.environ.get(name, "")
    if not value or "\n" in value or "\r" in value:
        raise WarmsetError(f"missing or malformed {name}")
    return value


def validate_endpoint(value: str) -> str:
    parsed = urlparse(value)
    try:
        port = parsed.port
    except ValueError:
        raise WarmsetError("SCCACHE_ENDPOINT is not the expected R2 origin") from None
    if (
        parsed.scheme != "https"
        or parsed.hostname != ACCOUNT_HOST
        or parsed.path not in ("", "/")
        or parsed.params
        or parsed.query
        or parsed.fragment
        or parsed.username
        or parsed.password
        or port not in (None, 443)
    ):
        raise WarmsetError("SCCACHE_ENDPOINT is not the expected R2 origin")
    return f"https://{parsed.hostname}"


def normalized_path(cache_hash: str) -> str:
    if not HASH.fullmatch(cache_hash):
        raise WarmsetError("compiler cache key is malformed")
    return f"{cache_hash[0]}/{cache_hash[1]}/{cache_hash[2]}/{cache_hash}"


def extract_hashes(log_path: Path, output_path: Path) -> int:
    try:
        if log_path.stat().st_size > 512 * 1024 * 1024:
            raise WarmsetError("sccache diagnostic log exceeds the size limit")
        ordered: dict[str, None] = {}
        with log_path.open(encoding="utf-8", errors="replace") as source:
            for line in source:
                for cache_hash in HASH_LOG.findall(line):
                    ordered.setdefault(cache_hash, None)
    except OSError:
        raise WarmsetError("sccache diagnostic log is unavailable") from None
    if not ordered:
        raise WarmsetError("sccache diagnostic log contained no compiler cache keys")

    output_path.parent.mkdir(parents=True, exist_ok=True)
    temporary = output_path.with_name(output_path.name + ".tmp")
    try:
        with temporary.open("w", encoding="utf-8") as output:
            for cache_hash in ordered:
                output.write(cache_hash + "\n")
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, output_path)
    except OSError:
        raise WarmsetError("compiler cache key output could not be written") from None
    finally:
        temporary.unlink(missing_ok=True)
    return len(ordered)


def load_hashes(paths: Iterable[Path]) -> list[str]:
    ordered: dict[str, None] = {}
    for path in paths:
        try:
            if not path.is_file() or path.stat().st_size > MAX_KEY_FILE_BYTES:
                raise WarmsetError(
                    "compiler cache key input is unavailable or oversized"
                )
            with path.open(encoding="ascii") as source:
                for line in source:
                    cache_hash = line.rstrip("\n")
                    if not HASH.fullmatch(cache_hash):
                        raise WarmsetError("compiler cache key input is malformed")
                    ordered.setdefault(cache_hash, None)
        except (OSError, UnicodeError):
            raise WarmsetError("compiler cache key input could not be read") from None
    if not ordered:
        raise WarmsetError("compiler cache key inputs were empty")
    if len(ordered) > MAX_OBJECTS:
        raise WarmsetError("compiler cache key inputs exceed the object limit")
    return list(ordered)


class R2Client:
    def __init__(self) -> None:
        if require_environment("SCCACHE_BUCKET") != BUCKET:
            raise WarmsetError("SCCACHE_BUCKET is not the trusted cache bucket")
        if require_environment("SCCACHE_REGION") != REGION:
            raise WarmsetError("SCCACHE_REGION is not the expected R2 region")
        self.endpoint = validate_endpoint(require_environment("SCCACHE_ENDPOINT"))
        require_environment("AWS_ACCESS_KEY_ID")
        require_environment("AWS_SECRET_ACCESS_KEY")
        self.aws = shutil.which("aws")
        if not self.aws:
            raise WarmsetError("the standard AWS CLI is unavailable")

    def environment(self) -> dict[str, str]:
        return {
            **os.environ,
            "AWS_DEFAULT_REGION": REGION,
            "AWS_REGION": REGION,
            "AWS_RETRY_MODE": "standard",
            "AWS_MAX_ATTEMPTS": "4",
            "AWS_REQUEST_CHECKSUM_CALCULATION": "when_required",
            "AWS_RESPONSE_CHECKSUM_VALIDATION": "when_required",
            "AWS_PAGER": "",
        }

    def inventory(self) -> dict[str, int]:
        inventory_path: Path | None = None
        try:
            with tempfile.NamedTemporaryFile(
                prefix="sccache-inventory-", suffix=".json", delete=False
            ) as output:
                inventory_path = Path(output.name)
                try:
                    result = subprocess.run(
                        [
                            self.aws,
                            "s3api",
                            "list-objects-v2",
                            "--endpoint-url",
                            self.endpoint,
                            "--region",
                            REGION,
                            "--bucket",
                            BUCKET,
                            "--prefix",
                            COMPILER_PREFIX + "/",
                            "--page-size",
                            "1000",
                            "--output",
                            "json",
                        ],
                        env=self.environment(),
                        stdin=subprocess.DEVNULL,
                        stdout=output,
                        stderr=subprocess.DEVNULL,
                        check=False,
                        timeout=1800,
                    )
                except (OSError, subprocess.TimeoutExpired):
                    raise WarmsetError("R2 compiler inventory failed") from None
            if result.returncode != 0:
                raise WarmsetError("R2 compiler inventory failed")
            if inventory_path.stat().st_size > MAX_INVENTORY_BYTES:
                raise WarmsetError("R2 compiler inventory exceeds the size limit")
            with inventory_path.open(encoding="utf-8") as source:
                payload = json.load(source)
        except (OSError, UnicodeError, json.JSONDecodeError):
            raise WarmsetError("R2 compiler inventory is invalid") from None
        finally:
            if inventory_path is not None:
                inventory_path.unlink(missing_ok=True)

        contents = payload.get("Contents") if isinstance(payload, dict) else None
        if contents is None:
            contents = []
        if not isinstance(contents, list):
            raise WarmsetError("R2 compiler inventory is invalid")
        objects: dict[str, int] = {}
        for item in contents:
            if not isinstance(item, dict):
                raise WarmsetError("R2 compiler inventory is invalid")
            key = item.get("Key")
            size = item.get("Size")
            # sccache's capability probe may leave a zero-byte check object.
            # Captured compiler objects are required to be non-empty below.
            if not isinstance(key, str) or type(size) is not int or size < 0:
                raise WarmsetError("R2 compiler inventory contains invalid metadata")
            previous = objects.setdefault(key, size)
            if previous != size:
                raise WarmsetError("R2 compiler inventory disagrees about an object")
        return objects

    def put(self, key: str, source: Path) -> None:
        try:
            result = subprocess.run(
                [
                    self.aws,
                    "s3api",
                    "put-object",
                    "--endpoint-url",
                    self.endpoint,
                    "--region",
                    REGION,
                    "--bucket",
                    BUCKET,
                    "--key",
                    key,
                    "--body",
                    str(source),
                    "--content-type",
                    "application/json",
                ],
                env=self.environment(),
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                check=False,
                timeout=1800,
            )
        except (OSError, subprocess.TimeoutExpired):
            raise WarmsetError("R2 warm-set publication failed") from None
        if result.returncode != 0:
            raise WarmsetError("R2 warm-set publication failed")


def build_manifest(
    hashes: list[str],
    inventory: Mapping[str, int],
    producer_sha: str,
    published_at: int,
) -> dict[str, object]:
    if not COMMIT_SHA.fullmatch(producer_sha):
        raise WarmsetError("GITHUB_SHA is malformed")
    captured: list[tuple[str, int]] = []
    missing = 0
    for cache_hash in hashes:
        path = normalized_path(cache_hash)
        size = inventory.get(f"{COMPILER_PREFIX}/{path}")
        if type(size) is not int or size <= 0:
            missing += 1
            continue
        captured.append((path, size))
    if missing:
        raise WarmsetError(
            f"{missing} compiler object(s) were absent from the durable R2 cache"
        )

    selected: list[dict[str, object]] = []
    selected_size = 0
    for path, size in captured:
        if len(selected) >= MAX_OBJECTS:
            break
        if size > MAX_WARM_BYTES or selected_size + size > MAX_WARM_BYTES:
            continue
        selected.append({"path": path, "size": size})
        selected_size += size
    if not selected:
        raise WarmsetError("no compiler objects fit the host warm-set budget")

    generation = f"{producer_sha}-{published_at}"
    return {
        "schema_version": 1,
        "bucket": BUCKET,
        "key_prefix": COMPILER_PREFIX,
        "generation": generation,
        "producer_sha": producer_sha,
        "published_at": published_at,
        "expires_at": published_at + MANIFEST_TTL_SECONDS,
        "max_bytes": MAX_WARM_BYTES,
        "captured_object_count": len(captured),
        "captured_size_bytes": sum(size for _, size in captured),
        "selected_object_count": len(selected),
        "selected_size_bytes": selected_size,
        "objects": selected,
    }


def publish(key_files: list[Path]) -> tuple[str, int, int]:
    hashes = load_hashes(key_files)
    producer_sha = require_environment("GITHUB_SHA")
    client = R2Client()
    manifest = build_manifest(
        hashes, client.inventory(), producer_sha, int(time.time())
    )
    encoded = (
        json.dumps(manifest, sort_keys=True, separators=(",", ":")) + "\n"
    ).encode()
    if len(encoded) > MAX_MANIFEST_BYTES:
        raise WarmsetError("compiler warm-set manifest exceeds the size limit")

    manifest_path: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(
            prefix="sccache-warmset-", suffix=".json", delete=False
        ) as output:
            output.write(encoded)
            output.flush()
            os.fsync(output.fileno())
            manifest_path = Path(output.name)
        client.put(f"{MANIFEST_PREFIX}/latest.json", manifest_path)
    finally:
        if manifest_path is not None:
            manifest_path.unlink(missing_ok=True)
    return (
        str(manifest["generation"]),
        int(manifest["selected_object_count"]),
        int(manifest["selected_size_bytes"]),
    )


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)
    extract_parser = subparsers.add_parser("extract")
    extract_parser.add_argument("log", type=Path)
    extract_parser.add_argument("output", type=Path)
    publish_parser = subparsers.add_parser("publish")
    publish_parser.add_argument("key_files", type=Path, nargs="+")
    return parser.parse_args(argv)


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    if args.command == "extract":
        count = extract_hashes(args.log, args.output)
        print(f"Captured {count} exact compiler cache keys.")
        return 0
    generation, count, size = publish(args.key_files)
    print(
        f"Published compiler warm set {generation}: " f"{count} objects, {size} bytes."
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main(sys.argv[1:]))
    except WarmsetError as error:
        print(f"compiler warm-set publication failed: {error}", file=sys.stderr)
        raise SystemExit(1) from None
