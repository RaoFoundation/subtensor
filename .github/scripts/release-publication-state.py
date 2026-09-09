#!/usr/bin/env python3
"""Check or record per-channel publication receipts for a finalized release.

Each gated publication channel (Docker, Docker localnet, crates.io, website)
leaves one independently named release asset, ``publication-<channel>.json``,
on the exact final release after that channel's publication has been observed
to succeed. The receipt is a completion record, never a release authorization:
callers must already have validated runtime identity, tag, and mirror state.

Usage:
    release-publication-state.py check  --tag TAG --sha SHA --code-hash HASH --channel CHANNEL
    release-publication-state.py record --tag TAG --sha SHA --code-hash HASH --channel CHANNEL

Exit codes:
    0  a valid receipt matching the requested identity exists (check), or the
       receipt now exists and was verified by read-back (record)
    1  the receipt is absent (explicit GitHub 404): safe to publish or retry
    2  malformed or conflicting receipt, invalid arguments, or any API failure:
       fail closed, never treat as absent

Uses only the Python standard library and GitHub's REST API with
``GITHUB_REPOSITORY`` and ``GH_TOKEN``.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
from collections.abc import Callable
from urllib.error import HTTPError, URLError
from urllib.parse import quote, urlencode
from urllib.request import Request, urlopen

API_BASE = "https://api.github.com"
UPLOADS_BASE = "https://uploads.github.com"
USER_AGENT = "subtensor-release-publication/1"
SCHEMA_VERSION = 1
RECEIPT_NAME_TEMPLATE = "publication-{channel}.json"
CHANNELS = ("docker", "docker-localnet", "crates", "website")
RECEIPT_KEYS = (
    "schema",
    "tag",
    "sha",
    "code_hash",
    "channel",
    "run_id",
    "run_attempt",
)
SHA_RE = re.compile(r"^[0-9a-f]{40}$")
CODE_HASH_RE = re.compile(r"^0x[0-9a-f]{64}$")
TAG_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._\-]{0,127}$")
REPOSITORY_RE = re.compile(r"^[^/\s]+/[^/\s]+$")
RUN_ID_RE = re.compile(r"^[0-9]{1,18}$")
MAX_ASSET_SIZE = 1024 * 1024
REQUEST_TIMEOUT = 30


class ApiError(RuntimeError):
    """GitHub could not provide authoritative state."""


class AbsentReceipt(Exception):
    """An explicit GitHub 404: the release or the receipt asset does not exist."""


class ReceiptError(RuntimeError):
    """A receipt asset exists but is malformed or conflicts with the request."""


class GitHubClient:
    """Minimal GitHub REST client; ``transport`` is injectable for tests."""

    def __init__(
        self,
        token: str,
        repository: str,
        transport: Callable[[str, str, dict[str, str], bytes | None], tuple[int, bytes]] | None = None,
    ) -> None:
        self.token = token
        self.repository = repository
        self._transport = transport if transport is not None else self._http_transport

    def _http_transport(
        self, method: str, url: str, headers: dict[str, str], body: bytes | None
    ) -> tuple[int, bytes]:
        request = Request(url, data=body, headers=headers, method=method)
        try:
            with urlopen(request, timeout=REQUEST_TIMEOUT) as response:
                return response.status, response.read(MAX_ASSET_SIZE + 1)
        except HTTPError as error:
            return error.code, error.read(MAX_ASSET_SIZE + 1)
        except (OSError, URLError) as error:
            raise ApiError(f"{method} {url} failed: {error}") from error

    def request(
        self,
        method: str,
        base: str,
        path: str,
        *,
        query: dict[str, str] | None = None,
        accept: str = "application/vnd.github+json",
        content_type: str | None = None,
        body: bytes | None = None,
    ) -> tuple[int, bytes]:
        owner, _, name = self.repository.partition("/")
        url = f"{base}/repos/{quote(owner, safe='')}/{quote(name, safe='')}" + path
        if query:
            url += "?" + urlencode(query)
        headers = {
            "Authorization": f"Bearer {self.token}",
            "Accept": accept,
            "User-Agent": USER_AGENT,
            "X-GitHub-Api-Version": "2022-11-28",
        }
        if content_type is not None:
            headers["Content-Type"] = content_type
        status, data = self._transport(method, url, headers, body)
        if len(data) > MAX_ASSET_SIZE:
            raise ApiError(f"{method} {url} exceeded the response size limit")
        return status, data

    def get_release(self, tag: str) -> dict | None:
        """Return the release for ``tag`` or None on an explicit 404."""
        status, data = self.request(
            "GET", API_BASE, f"/releases/tags/{quote(tag, safe='')}"
        )
        if status == 404:
            return None
        if status != 200:
            raise ApiError(f"release lookup for tag {tag} returned HTTP {status}")
        release = _parse_json_object(data, f"release lookup for tag {tag}")
        if not isinstance(release.get("id"), int) or isinstance(release.get("id"), bool):
            raise ApiError(f"release lookup for tag {tag} has no numeric id")
        if not isinstance(release.get("assets"), list):
            raise ApiError(f"release lookup for tag {tag} has no asset list")
        return release

    def download_asset(self, asset_id: int, name: str) -> bytes:
        status, data = self.request(
            "GET",
            API_BASE,
            f"/releases/assets/{asset_id}",
            accept="application/octet-stream",
        )
        if status == 404:
            raise AbsentReceipt(f"receipt asset {name} no longer exists")
        if status != 200:
            raise ApiError(f"download of asset {name} returned HTTP {status}")
        return data

    def upload_asset(self, release_id: int, name: str, content: bytes) -> None:
        status, data = self.request(
            "POST",
            UPLOADS_BASE,
            f"/releases/{release_id}/assets",
            query={"name": name},
            accept="application/vnd.github+json",
            content_type="application/octet-stream",
            body=content,
        )
        if status != 201:
            raise ApiError(f"upload of asset {name} returned HTTP {status}")
        uploaded = _parse_json_object(data, f"upload of asset {name}")
        if uploaded.get("name") != name:
            raise ApiError(f"upload of asset {name} recorded a different asset name")


def _parse_json_object(data: bytes, context: str) -> dict:
    try:
        value = json.loads(data)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ApiError(f"{context} did not return valid JSON") from error
    if not isinstance(value, dict):
        raise ApiError(f"{context} did not return a JSON object")
    return value


def validate_identity(tag: str, sha: str, code_hash: str, channel: str) -> None:
    if TAG_RE.fullmatch(tag) is None:
        raise ValueError(f"invalid tag: {tag!r}")
    if SHA_RE.fullmatch(sha) is None:
        raise ValueError("sha must be a full lowercase 40-hex commit SHA")
    if CODE_HASH_RE.fullmatch(code_hash) is None:
        raise ValueError("code-hash must be a lowercase 0x-prefixed 64-hex runtime hash")
    if channel not in CHANNELS:
        raise ValueError(f"channel must be one of: {', '.join(CHANNELS)}")


def receipt_name(channel: str) -> str:
    return RECEIPT_NAME_TEMPLATE.format(channel=channel)


def receipt_document(
    tag: str, sha: str, code_hash: str, channel: str, run_id: int, run_attempt: int
) -> bytes:
    receipt = {
        "schema": SCHEMA_VERSION,
        "tag": tag,
        "sha": sha,
        "code_hash": code_hash,
        "channel": channel,
        "run_id": run_id,
        "run_attempt": run_attempt,
    }
    return (json.dumps(receipt, separators=(",", ":")) + "\n").encode()


def parse_receipt(
    raw: bytes, tag: str, sha: str, code_hash: str, channel: str
) -> dict:
    """Parse one receipt asset; raise ReceiptError unless it matches exactly."""
    try:
        value = json.loads(raw)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ReceiptError(f"receipt for {channel} is not valid JSON") from error
    if not isinstance(value, dict):
        raise ReceiptError(f"receipt for {channel} is not a JSON object")
    if sorted(value) != sorted(RECEIPT_KEYS):
        raise ReceiptError(
            f"receipt for {channel} has keys {sorted(value)}; expected exactly {sorted(RECEIPT_KEYS)}"
        )
    if value["schema"] is not SCHEMA_VERSION:
        raise ReceiptError(f"receipt for {channel} has unsupported schema {value['schema']!r}")
    for key in ("tag", "sha", "code_hash", "channel"):
        if not isinstance(value[key], str):
            raise ReceiptError(f"receipt for {channel} field {key!r} is not a string")
    for key in ("run_id", "run_attempt"):
        if not isinstance(value[key], int) or isinstance(value[key], bool) or value[key] <= 0:
            raise ReceiptError(f"receipt for {channel} field {key!r} is not a positive integer")
    if value["tag"] != tag or value["sha"] != sha or value["code_hash"] != code_hash:
        raise ReceiptError(
            f"receipt for {channel} records tag {value['tag']!r} sha {value['sha']!r} "
            f"code_hash {value['code_hash']!r}; expected tag {tag!r} sha {sha!r} code_hash {code_hash!r}"
        )
    if value["channel"] != channel:
        raise ReceiptError(f"receipt asset {receipt_name(channel)} records channel {value['channel']!r}")
    return value


def find_receipt_assets(release: dict, name: str) -> list[dict]:
    assets = []
    for asset in release["assets"]:
        if not isinstance(asset, dict):
            raise ApiError("release returned a malformed asset entry")
        if asset.get("name") == name:
            if not isinstance(asset.get("id"), int) or isinstance(asset.get("id"), bool):
                raise ApiError(f"release asset {name} has no numeric id")
            assets.append(asset)
    return assets


def require_environment(name: str) -> str:
    value = os.environ.get(name, "")
    if not value:
        raise ApiError(f"{name} is not set")
    return value


def read_receipt(client: GitHubClient, tag: str, sha: str, code_hash: str, channel: str) -> dict | None:
    """Return the matching receipt, None when explicitly absent, raise on conflict."""
    release = client.get_release(tag)
    if release is None:
        raise AbsentReceipt(f"release {tag} does not exist")
    name = receipt_name(channel)
    assets = find_receipt_assets(release, name)
    if not assets:
        raise AbsentReceipt(f"release {tag} has no {name} asset")
    receipts = [
        parse_receipt(client.download_asset(asset["id"], name), tag, sha, code_hash, channel)
        for asset in assets
    ]
    first = receipts[0]
    for other in receipts[1:]:
        if other != first:
            raise ReceiptError(f"release {tag} has conflicting {name} assets")
    return first


def run_check(client: GitHubClient, tag: str, sha: str, code_hash: str, channel: str) -> int:
    try:
        receipt = read_receipt(client, tag, sha, code_hash, channel)
    except AbsentReceipt as error:
        print(f"receipt absent: {error}", file=sys.stderr)
        return 1
    except (ReceiptError, ApiError) as error:
        print(f"could not verify publication receipt: {error}", file=sys.stderr)
        return 2
    print(
        f"{receipt_name(channel)} already recorded on release {tag} "
        f"(run {receipt['run_id']}.{receipt['run_attempt']})"
    )
    return 0


def run_identities() -> tuple[int, int]:
    run_id = os.environ.get("GITHUB_RUN_ID", "")
    run_attempt = os.environ.get("GITHUB_RUN_ATTEMPT", "")
    if RUN_ID_RE.fullmatch(run_id) is None or RUN_ID_RE.fullmatch(run_attempt) is None:
        raise ApiError("GITHUB_RUN_ID and GITHUB_RUN_ATTEMPT must be positive integers")
    return int(run_id), int(run_attempt)


def run_record(client: GitHubClient, tag: str, sha: str, code_hash: str, channel: str) -> int:
    name = receipt_name(channel)
    try:
        receipt = read_receipt(client, tag, sha, code_hash, channel)
    except AbsentReceipt as error:
        print(f"recording receipt: {error}")
        try:
            release = client.get_release(tag)
            if release is None:
                print(
                    f"could not record publication receipt: release {tag} does not exist",
                    file=sys.stderr,
                )
                return 2
            run_id, run_attempt = run_identities()
            content = receipt_document(tag, sha, code_hash, channel, run_id, run_attempt)
            client.upload_asset(release["id"], name, content)
            recorded = read_receipt(client, tag, sha, code_hash, channel)
            if recorded is None:
                print(f"could not record publication receipt: {name} missing after upload", file=sys.stderr)
                return 2
        except (AbsentReceipt, ReceiptError, ApiError) as error:
            print(f"could not record publication receipt: {error}", file=sys.stderr)
            return 2
        print(f"recorded {name} on release {tag} (run {run_id}.{run_attempt})")
        return 0
    except (ReceiptError, ApiError) as error:
        print(f"could not record publication receipt: {error}", file=sys.stderr)
        return 2
    print(
        f"{name} already recorded on release {tag} "
        f"(run {receipt['run_id']}.{receipt['run_attempt']}); nothing to do"
    )
    return 0


def main(argv: list[str] | None = None, *, transport=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest="command", required=True)
    for command in ("check", "record"):
        subparser = commands.add_parser(command)
        subparser.add_argument("--tag", required=True)
        subparser.add_argument("--sha", required=True)
        subparser.add_argument("--code-hash", required=True, dest="code_hash")
        subparser.add_argument("--channel", required=True, choices=CHANNELS)
    arguments = parser.parse_args(argv)
    try:
        validate_identity(arguments.tag, arguments.sha, arguments.code_hash, arguments.channel)
    except ValueError as error:
        print(f"invalid arguments: {error}", file=sys.stderr)
        return 2
    try:
        repository = require_environment("GITHUB_REPOSITORY")
        if REPOSITORY_RE.fullmatch(repository) is None:
            raise ApiError(f"GITHUB_REPOSITORY is malformed: {repository!r}")
        token = require_environment("GH_TOKEN")
    except ApiError as error:
        print(f"could not record or check publication receipt: {error}", file=sys.stderr)
        return 2
    client = GitHubClient(token, repository, transport=transport)
    if arguments.command == "check":
        return run_check(client, arguments.tag, arguments.sha, arguments.code_hash, arguments.channel)
    return run_record(client, arguments.tag, arguments.sha, arguments.code_hash, arguments.channel)

if __name__ == "__main__":
    raise SystemExit(main())
