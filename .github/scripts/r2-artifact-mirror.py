#!/usr/bin/env python3
"""Publish an immutable Actions artifact archive and its latest manifest to R2."""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import hmac
import http.client
import json
import os
import re
import ssl
import sys
import time
from pathlib import Path
from urllib.parse import quote, urlparse


ACCOUNT_HOST = re.compile(r"^[0-9a-f]{32}\.r2\.cloudflarestorage\.com$")
SHA256_DIGEST = re.compile(r"^sha256:([0-9a-f]{64})$")
COMMIT_SHA = re.compile(r"^[0-9a-f]{40}$")
BUCKET = "subtensor-ci-sccache"
REGION = "auto"
SERVICE = "s3"
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


def validate_endpoint(value: str) -> tuple[str, int]:
    parsed = urlparse(value)
    try:
        port = parsed.port
    except ValueError:
        raise MirrorError("SCCACHE_ENDPOINT is not the expected R2 origin") from None
    if (
        parsed.scheme != "https"
        or not parsed.hostname
        or not ACCOUNT_HOST.fullmatch(parsed.hostname)
        or parsed.path not in ("", "/")
        or parsed.params
        or parsed.query
        or parsed.fragment
        or parsed.username
        or parsed.password
        or port not in (None, 443)
    ):
        raise MirrorError("SCCACHE_ENDPOINT is not the expected R2 origin")
    return parsed.hostname, port or 443


def file_sha256(path: Path) -> tuple[str, int]:
    digest = hashlib.sha256()
    size = 0
    with path.open("rb") as handle:
        while chunk := handle.read(1024 * 1024):
            digest.update(chunk)
            size += len(chunk)
    return digest.hexdigest(), size


def signing_key(secret: str, date: str, region: str = REGION) -> bytes:
    date_key = hmac.new(
        ("AWS4" + secret).encode(), date.encode(), hashlib.sha256
    ).digest()
    region_key = hmac.new(date_key, region.encode(), hashlib.sha256).digest()
    service_key = hmac.new(region_key, SERVICE.encode(), hashlib.sha256).digest()
    return hmac.new(service_key, b"aws4_request", hashlib.sha256).digest()


def authorization_headers(
    *,
    host: str,
    key: str,
    payload_sha256: str,
    access_key: str,
    secret_key: str,
    now: dt.datetime,
) -> tuple[str, dict[str, str]]:
    timestamp = now.strftime("%Y%m%dT%H%M%SZ")
    date = now.strftime("%Y%m%d")
    canonical_uri = quote(f"/{BUCKET}/{key}", safe="/-_.~")
    canonical_headers = (
        f"host:{host}\nx-amz-content-sha256:{payload_sha256}\nx-amz-date:{timestamp}\n"
    )
    signed_headers = "host;x-amz-content-sha256;x-amz-date"
    canonical_request = "\n".join(
        ["PUT", canonical_uri, "", canonical_headers, signed_headers, payload_sha256]
    )
    scope = f"{date}/{REGION}/{SERVICE}/aws4_request"
    string_to_sign = "\n".join(
        [
            "AWS4-HMAC-SHA256",
            timestamp,
            scope,
            hashlib.sha256(canonical_request.encode()).hexdigest(),
        ]
    )
    signature = hmac.new(
        signing_key(secret_key, date), string_to_sign.encode(), hashlib.sha256
    ).hexdigest()
    authorization = (
        f"AWS4-HMAC-SHA256 Credential={access_key}/{scope}, "
        f"SignedHeaders={signed_headers}, Signature={signature}"
    )
    return canonical_uri, {
        "Authorization": authorization,
        "Host": host,
        "x-amz-content-sha256": payload_sha256,
        "x-amz-date": timestamp,
    }


class R2Publisher:
    def __init__(self) -> None:
        bucket = require_environment("SCCACHE_BUCKET")
        if bucket != BUCKET:
            raise MirrorError("SCCACHE_BUCKET is not the trusted cache bucket")
        region = require_environment("SCCACHE_REGION")
        if region != REGION:
            raise MirrorError("SCCACHE_REGION is not the expected R2 region")
        self.host, self.port = validate_endpoint(
            require_environment("SCCACHE_ENDPOINT")
        )
        self.access_key = require_environment("AWS_ACCESS_KEY_ID")
        self.secret_key = require_environment("AWS_SECRET_ACCESS_KEY")

    def put(
        self, key: str, source: Path | bytes, payload_sha256: str, size: int
    ) -> None:
        for attempt in range(1, 4):
            now = dt.datetime.now(dt.timezone.utc)
            uri, headers = authorization_headers(
                host=self.host,
                key=key,
                payload_sha256=payload_sha256,
                access_key=self.access_key,
                secret_key=self.secret_key,
                now=now,
            )
            connection = http.client.HTTPSConnection(
                self.host,
                self.port,
                timeout=1800,
                context=ssl.create_default_context(),
            )
            try:
                connection.putrequest(
                    "PUT", uri, skip_host=True, skip_accept_encoding=True
                )
                for name, value in headers.items():
                    connection.putheader(name, value)
                connection.putheader("Content-Length", str(size))
                connection.putheader("Content-Type", "application/octet-stream")
                connection.endheaders()
                if isinstance(source, bytes):
                    connection.send(source)
                else:
                    with source.open("rb") as handle:
                        while chunk := handle.read(1024 * 1024):
                            connection.send(chunk)
                response = connection.getresponse()
                response.read(4096)
                if response.status in (200, 201):
                    return
                if response.status < 500:
                    raise MirrorError(f"R2 rejected object upload ({response.status})")
            except (OSError, TimeoutError, http.client.HTTPException):
                if attempt == 3:
                    raise MirrorError("R2 object upload failed after retries") from None
            finally:
                connection.close()
            if attempt < 3:
                time.sleep(2**attempt)
        raise MirrorError("R2 object upload failed after retries")


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
    publisher.put(object_key, args.archive, actual_sha, size)
    publisher.put(
        f"{MIRROR_PREFIX}/{args.artifact_name}/latest.json",
        manifest,
        hashlib.sha256(manifest).hexdigest(),
        len(manifest),
    )
    print(f"Published immutable artifact {args.artifact_id} and latest manifest.")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main(sys.argv[1:]))
    except MirrorError as error:
        print(f"artifact mirror failed: {error}", file=sys.stderr)
        raise SystemExit(1) from None
