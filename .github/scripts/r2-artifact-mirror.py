#!/usr/bin/env python3
"""Validate and publish immutable Actions artifacts through the standard S3 CLI."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from urllib.parse import urlparse


ACCOUNT_HOST = "3dc4cebb791314d78848969042fb3382.r2.cloudflarestorage.com"
SHA256_DIGEST = re.compile(r"^sha256:([0-9a-f]{64})$")
COMMIT_SHA = re.compile(r"^[0-9a-f]{40}$")
BUCKET = "subtensor-ci-sccache"
REGION = "auto"
MIRROR_PREFIX = "artifacts/v1"
ALLOWED_ARTIFACT_NAMES = frozenset(
    {
        "mainnet-snapshot",
        "try-runtime-snap-v0.10.1-mainnet",
        "try-runtime-snap-v0.10.1-testnet",
        "try-runtime-snap-v0.10.1-devnet",
    }
)


class MirrorError(Exception):
    """A safe error that never contains credentials."""


def require_environment(name: str) -> str:
    value = os.environ.get(name, "")
    if not value or "\n" in value or "\r" in value:
        raise MirrorError(f"missing or malformed {name}")
    return value


def validate_endpoint(value: str) -> str:
    parsed = urlparse(value)
    try:
        port = parsed.port
    except ValueError:
        raise MirrorError("SCCACHE_ENDPOINT is not the expected R2 origin") from None
    if (
        parsed.scheme != "https"
        or not parsed.hostname
        or parsed.hostname != ACCOUNT_HOST
        or parsed.path not in ("", "/")
        or parsed.params
        or parsed.query
        or parsed.fragment
        or parsed.username
        or parsed.password
        or port not in (None, 443)
    ):
        raise MirrorError("SCCACHE_ENDPOINT is not the expected R2 origin")
    return f"https://{parsed.hostname}"


def file_sha256(path: Path) -> tuple[str, int]:
    digest = hashlib.sha256()
    size = 0
    with path.open("rb") as handle:
        while chunk := handle.read(1024 * 1024):
            digest.update(chunk)
            size += len(chunk)
    return digest.hexdigest(), size


class R2Publisher:
    def __init__(self) -> None:
        if require_environment("SCCACHE_BUCKET") != BUCKET:
            raise MirrorError("SCCACHE_BUCKET is not the trusted cache bucket")
        if require_environment("SCCACHE_REGION") != REGION:
            raise MirrorError("SCCACHE_REGION is not the expected R2 region")
        self.endpoint = validate_endpoint(require_environment("SCCACHE_ENDPOINT"))
        require_environment("AWS_ACCESS_KEY_ID")
        require_environment("AWS_SECRET_ACCESS_KEY")
        self.aws = shutil.which("aws")
        if not self.aws:
            raise MirrorError("the standard AWS CLI is unavailable")

    def put(self, key: str, source: Path) -> None:
        environment = {
            **os.environ,
            "AWS_DEFAULT_REGION": REGION,
            "AWS_REGION": REGION,
            "AWS_RETRY_MODE": "standard",
            "AWS_MAX_ATTEMPTS": "4",
            "AWS_REQUEST_CHECKSUM_CALCULATION": "when_required",
            "AWS_RESPONSE_CHECKSUM_VALIDATION": "when_required",
            "AWS_PAGER": "",
        }
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
                    "application/octet-stream",
                ],
                env=environment,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.PIPE,
                text=True,
                check=False,
                timeout=1800,
            )
        except (OSError, subprocess.TimeoutExpired):
            raise MirrorError("R2 object upload failed through the standard S3 client") from None
        if result.returncode != 0:
            raise MirrorError("R2 object upload failed through the standard S3 client")


def build_manifest(args: argparse.Namespace, object_key: str, size: int) -> bytes:
    repository = require_environment("GITHUB_REPOSITORY")
    repository_id = require_environment("GITHUB_REPOSITORY_ID")
    if not repository_id.isdigit() or int(repository_id) <= 0:
        raise MirrorError("GITHUB_REPOSITORY_ID is malformed")
    return (
        json.dumps(
            {
                "schema_version": 1,
                "repository": repository,
                "repository_id": int(repository_id),
                "workflow_path": args.workflow_path,
                "artifact_id": args.artifact_id,
                "artifact_name": args.artifact_name,
                "digest": args.digest,
                "size_in_bytes": size,
                "object_key": object_key,
                "producer_sha": args.producer_sha,
                "published_at": int(time.time()),
            },
            sort_keys=True,
            separators=(",", ":"),
        )
        + "\n"
    ).encode()


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("archive", type=Path)
    parser.add_argument("artifact_id", type=int)
    parser.add_argument("artifact_name")
    parser.add_argument("digest")
    parser.add_argument("producer_sha")
    parser.add_argument("workflow_path")
    args = parser.parse_args(argv)
    if args.artifact_id <= 0:
        parser.error("artifact_id must be positive")
    if args.artifact_name not in ALLOWED_ARTIFACT_NAMES:
        parser.error("artifact_name is outside the trusted mirror allowlist")
    if not SHA256_DIGEST.fullmatch(args.digest):
        parser.error("digest must be a sha256 digest")
    if not COMMIT_SHA.fullmatch(args.producer_sha):
        parser.error("producer_sha must be a full commit SHA")
    if args.workflow_path != ".github/workflows/refresh-mainnet-snapshot.yml":
        parser.error("workflow_path is outside the trusted producer allowlist")
    if not args.archive.is_file():
        parser.error("archive does not exist")
    return args


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    expected_sha = SHA256_DIGEST.fullmatch(args.digest).group(1)
    actual_sha, size = file_sha256(args.archive)
    if actual_sha != expected_sha or size <= 0:
        raise MirrorError("downloaded artifact archive failed integrity validation")

    object_key = f"{MIRROR_PREFIX}/objects/{args.artifact_id}-{actual_sha}.zip"
    manifest = build_manifest(args, object_key, size)
    publisher = R2Publisher()
    publisher.put(object_key, args.archive)

    manifest_path: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(
            prefix="artifact-manifest-", suffix=".json", delete=False
        ) as output:
            output.write(manifest)
            output.flush()
            os.fsync(output.fileno())
            manifest_path = Path(output.name)
        publisher.put(f"{MIRROR_PREFIX}/{args.artifact_name}/latest.json", manifest_path)
    finally:
        if manifest_path is not None:
            manifest_path.unlink(missing_ok=True)

    print(f"Published immutable artifact {args.artifact_id} and latest manifest.")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main(sys.argv[1:]))
    except MirrorError as error:
        print(f"artifact mirror failed: {error}", file=sys.stderr)
        raise SystemExit(1) from None
