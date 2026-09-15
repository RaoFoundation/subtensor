#!/usr/bin/env python3
"""Offline tests for release-publication-state.py using a fake HTTP transport.

These tests never touch the network and never read real credentials: the
GitHub client is constructed against a routing fake transport and the
environment is patched for the duration of each test.
"""

from __future__ import annotations

import importlib.util
import json
import os
import sys
import unittest
import urllib.parse
from pathlib import Path
from unittest.mock import patch

_SCRIPT_PATH = Path(__file__).resolve().parent / "release-publication-state.py"
_script_spec = importlib.util.spec_from_file_location("release_publication_state", _SCRIPT_PATH)
assert _script_spec is not None and _script_spec.loader is not None
rps = importlib.util.module_from_spec(_script_spec)
sys.modules["release_publication_state"] = rps
_script_spec.loader.exec_module(rps)


REPOSITORY = "RaoFoundation/subtensor"
TAG = "v433"
SHA = "b" * 40
CODE_HASH = "0x" + "c" * 64
RUN_ID = 12345
RUN_ATTEMPT = 2
RECEIPT_NAME = "publication-docker.json"

BASE_ENV = {
    "GITHUB_REPOSITORY": REPOSITORY,
    "GH_TOKEN": "fake-token",
    "GITHUB_RUN_ID": str(RUN_ID),
    "GITHUB_RUN_ATTEMPT": str(RUN_ATTEMPT),
}


def receipt_bytes(
    *,
    tag: str = TAG,
    sha: str = SHA,
    code_hash: str = CODE_HASH,
    channel: str = "docker",
    run_id: int = RUN_ID,
    run_attempt: int = RUN_ATTEMPT,
    extra: dict | None = None,
    drop: str | None = None,
    schema: int = 1,
) -> bytes:
    value = {
        "schema": schema,
        "tag": tag,
        "sha": sha,
        "code_hash": code_hash,
        "channel": channel,
        "run_id": run_id,
        "run_attempt": run_attempt,
    }
    if drop is not None:
        del value[drop]
    if extra:
        value.update(extra)
    return (json.dumps(value) + "\n").encode()


def release_payload(assets: list[dict], release_id: int = 900) -> bytes:
    return json.dumps({"id": release_id, "tag_name": TAG, "assets": assets}).encode()


def asset_payload(asset_id: int, name: str) -> dict:
    return {"id": asset_id, "name": name}


class FakeTransport:
    """Routes expected GitHub API calls; rejects anything unexpected."""

    def __init__(self) -> None:
        self.routes: dict[tuple[str, str], tuple[int, bytes]] = {}
        self.calls: list[tuple[str, str, bytes | None]] = []

    def route(self, method: str, url: str, status: int, body: bytes) -> None:
        self.routes[(method, url)] = (status, body)

    def __call__(self, method: str, url: str, headers: dict, body: bytes | None) -> tuple[int, bytes]:
        self.calls.append((method, url, body))
        key = (method, url)
        if key not in self.routes:
            raise AssertionError(f"unexpected external call: {method} {url}")
        return self.routes[key]

    def upload_calls(self) -> list[tuple[str, bytes | None]]:
        return [(url, body) for method, url, body in self.calls if method == "POST"]


def release_url(tag: str = TAG) -> str:
    return f"{rps.API_BASE}/repos/{REPOSITORY}/releases/tags/{tag}"


def asset_url(asset_id: int) -> str:
    return f"{rps.API_BASE}/repos/{REPOSITORY}/releases/assets/{asset_id}"


def upload_url(name: str, release_id: int = 900) -> str:
    return (
        f"{rps.UPLOADS_BASE}/repos/{REPOSITORY}/releases/{release_id}/assets"
        f"?{urllib.parse.urlencode({'name': name})}"
    )


def check_argv() -> list[str]:
    return ["check", "--tag", TAG, "--sha", SHA, "--code-hash", CODE_HASH, "--channel", "docker"]


def record_argv() -> list[str]:
    return ["record", "--tag", TAG, "--sha", SHA, "--code-hash", CODE_HASH, "--channel", "docker"]


class PublicationStateTestCase(unittest.TestCase):
    def run_main(self, transport: FakeTransport, argv: list[str], env: dict[str, str] | None = None) -> int:
        with patch.dict(os.environ, env if env is not None else BASE_ENV):
            return rps.main(argv, transport=transport)

    @staticmethod
    def sequenced_transport(transport: FakeTransport, release_bodies: list[bytes]):
        """Route repeated release lookups through an ordered response list."""
        responses = iter(release_bodies)
        original_transport = transport.__call__

        def transport_fn(method, url, headers, body):
            if (method, url) == ("GET", release_url()):
                return 200, next(responses)
            return original_transport(method, url, headers, body)

        return transport_fn


class CheckTests(PublicationStateTestCase):
    def test_matching_receipt_exits_zero(self):
        transport = FakeTransport()
        transport.route("GET", release_url(), 200, release_payload([asset_payload(1, RECEIPT_NAME)]))
        transport.route("GET", asset_url(1), 200, receipt_bytes())
        self.assertEqual(self.run_main(transport, check_argv()), 0)
        self.assertEqual(transport.upload_calls(), [])

    def test_missing_release_is_absent_not_failure(self):
        transport = FakeTransport()
        transport.route("GET", release_url(), 404, b'{"message": "Not Found"}')
        self.assertEqual(self.run_main(transport, check_argv()), 1)

    def test_missing_receipt_asset_is_absent_not_failure(self):
        transport = FakeTransport()
        transport.route("GET", release_url(), 200, release_payload([asset_payload(1, "publication-website.json")]))
        self.assertEqual(self.run_main(transport, check_argv()), 1)

    def test_conflicting_receipt_fields_fail_closed(self):
        for kwargs in (
            {"sha": "a" * 40},
            {"tag": "v432"},
            {"code_hash": "0x" + "9" * 64},
            {"channel": "website"},
        ):
            with self.subTest(conflict=kwargs):
                transport = FakeTransport()
                transport.route("GET", release_url(), 200, release_payload([asset_payload(1, RECEIPT_NAME)]))
                transport.route("GET", asset_url(1), 200, receipt_bytes(**kwargs))
                self.assertEqual(self.run_main(transport, check_argv()), 2)

    def test_malformed_receipts_fail_closed(self):
        for raw in (
            b"not json",
            b"[1, 2]",
            receipt_bytes(extra={"surprise": True}),
            receipt_bytes(drop="run_id"),
            receipt_bytes(run_id=0),
            receipt_bytes(run_attempt="2"),
            receipt_bytes(schema=2),
        ):
            with self.subTest(raw=raw[:40]):
                transport = FakeTransport()
                transport.route("GET", release_url(), 200, release_payload([asset_payload(1, RECEIPT_NAME)]))
                transport.route("GET", asset_url(1), 200, raw)
                self.assertEqual(self.run_main(transport, check_argv()), 2)

    def test_api_failure_is_not_treated_as_absent(self):
        transport = FakeTransport()
        transport.route("GET", release_url(), 403, b'{"message": "rate limited"}')
        self.assertEqual(self.run_main(transport, check_argv()), 2)

    def test_duplicate_identical_assets_are_accepted(self):
        transport = FakeTransport()
        transport.route(
            "GET",
            release_url(),
            200,
            release_payload([asset_payload(1, RECEIPT_NAME), asset_payload(2, RECEIPT_NAME)]),
        )
        transport.route("GET", asset_url(1), 200, receipt_bytes())
        transport.route("GET", asset_url(2), 200, receipt_bytes())
        self.assertEqual(self.run_main(transport, check_argv()), 0)

    def test_duplicate_conflicting_assets_fail_closed(self):
        transport = FakeTransport()
        transport.route(
            "GET",
            release_url(),
            200,
            release_payload([asset_payload(1, RECEIPT_NAME), asset_payload(2, RECEIPT_NAME)]),
        )
        transport.route("GET", asset_url(1), 200, receipt_bytes())
        transport.route("GET", asset_url(2), 200, receipt_bytes(run_id=RUN_ID + 1))
        self.assertEqual(self.run_main(transport, check_argv()), 2)


class RecordTests(PublicationStateTestCase):
    def test_record_uploads_receipt_and_verifies_read_back(self):
        transport = FakeTransport()
        transport_fn = self.sequenced_transport(
            transport,
            [
                release_payload([]),                                  # initial lookup
                release_payload([]),                                  # re-fetch before upload
                release_payload([asset_payload(1, RECEIPT_NAME)]),    # read-back
            ],
        )
        transport.route("POST", upload_url(RECEIPT_NAME), 201, json.dumps({"name": RECEIPT_NAME}).encode())
        transport.route("GET", asset_url(1), 200, receipt_bytes())
        client = rps.GitHubClient("fake-token", REPOSITORY, transport=transport_fn)
        with patch.dict(os.environ, BASE_ENV):
            self.assertEqual(rps.run_record(client, TAG, SHA, CODE_HASH, "docker"), 0)
        uploads = transport.upload_calls()
        self.assertEqual(len(uploads), 1)
        url, body = uploads[0]
        self.assertEqual(url, upload_url(RECEIPT_NAME))
        self.assertEqual(
            json.loads(body),
            {
                "schema": 1,
                "tag": TAG,
                "sha": SHA,
                "code_hash": CODE_HASH,
                "channel": "docker",
                "run_id": RUN_ID,
                "run_attempt": RUN_ATTEMPT,
            },
        )

    def test_second_record_is_idempotent_noop(self):
        transport = FakeTransport()
        transport.route("GET", release_url(), 200, release_payload([asset_payload(1, RECEIPT_NAME)]))
        transport.route("GET", asset_url(1), 200, receipt_bytes())
        self.assertEqual(self.run_main(transport, record_argv()), 0)
        self.assertEqual(transport.upload_calls(), [])

    def test_conflicting_receipt_rejects_record_without_upload(self):
        transport = FakeTransport()
        transport.route("GET", release_url(), 200, release_payload([asset_payload(1, RECEIPT_NAME)]))
        transport.route("GET", asset_url(1), 200, receipt_bytes(sha="a" * 40))
        self.assertEqual(self.run_main(transport, record_argv()), 2)
        self.assertEqual(transport.upload_calls(), [])

    def test_record_without_release_fails_closed(self):
        transport = FakeTransport()
        transport.route("GET", release_url(), 404, b'{"message": "Not Found"}')
        self.assertEqual(self.run_main(transport, record_argv()), 2)
        self.assertEqual(transport.upload_calls(), [])

    def test_record_upload_failure_fails_closed(self):
        transport = FakeTransport()
        transport.route("GET", release_url(), 200, release_payload([]))
        transport.route("POST", upload_url(RECEIPT_NAME), 422, b'{"message": "Unprocessable Entity"}')
        self.assertEqual(self.run_main(transport, record_argv()), 2)
        self.assertEqual(len(transport.upload_calls()), 1)

    def test_record_read_back_mismatch_fails_closed(self):
        transport = FakeTransport()
        transport_fn = self.sequenced_transport(
            transport,
            [
                release_payload([]),
                release_payload([]),
                release_payload([asset_payload(1, RECEIPT_NAME)]),
            ],
        )
        transport.route("POST", upload_url(RECEIPT_NAME), 201, json.dumps({"name": RECEIPT_NAME}).encode())
        transport.route("GET", asset_url(1), 200, receipt_bytes(sha="a" * 40))
        client = rps.GitHubClient("fake-token", REPOSITORY, transport=transport_fn)
        with patch.dict(os.environ, BASE_ENV):
            self.assertEqual(rps.run_record(client, TAG, SHA, CODE_HASH, "docker"), 2)


class ArgumentAndEnvironmentTests(PublicationStateTestCase):
    def test_invalid_fields_exit_two_before_any_api_call(self):
        transport = FakeTransport()
        for argv in (
            ["check", "--tag", "v 433", "--sha", SHA, "--code-hash", CODE_HASH, "--channel", "docker"],
            ["check", "--tag", TAG, "--sha", "B" * 40, "--code-hash", CODE_HASH, "--channel", "docker"],
            ["check", "--tag", TAG, "--sha", SHA, "--code-hash", "c" * 64, "--channel", "docker"],
        ):
            with self.subTest(argv=argv):
                self.assertEqual(self.run_main(transport, argv), 2)
        with self.subTest(argv="unknown channel"), self.assertRaises(SystemExit) as exited:
            self.run_main(transport, ["check", "--tag", TAG, "--sha", SHA, "--code-hash", CODE_HASH, "--channel", "pypi"])
        self.assertEqual(exited.exception.code, 2)
        self.assertEqual(transport.calls, [])

    def test_missing_environment_fails_closed_without_api_call(self):
        transport = FakeTransport()
        for env in (
            {k: v for k, v in BASE_ENV.items() if k != "GITHUB_REPOSITORY"},
            {k: v for k, v in BASE_ENV.items() if k != "GH_TOKEN"},
            {**BASE_ENV, "GITHUB_REPOSITORY": "RaoFoundation/subtensor/extra"},
        ):
            with self.subTest(env=env):
                self.assertEqual(self.run_main(transport, check_argv(), env=env), 2)
        self.assertEqual(transport.calls, [])

    def test_record_requires_run_identity_environment(self):
        transport = FakeTransport()
        transport.route("GET", release_url(), 200, release_payload([]))
        env = {k: v for k, v in BASE_ENV.items() if k != "GITHUB_RUN_ID"}
        self.assertEqual(self.run_main(transport, record_argv(), env=env), 2)
        self.assertEqual(transport.upload_calls(), [])


if __name__ == "__main__":
    unittest.main()
