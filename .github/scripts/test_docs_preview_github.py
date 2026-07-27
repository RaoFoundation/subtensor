#!/usr/bin/env python3

import os
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import patch

from docs_preview_github import (
    ApiError,
    PullState,
    associated_pr_number,
    prepare,
    reconcile_mode,
    upsert_preview_comment,
    workflow_pr_number,
)


SHA = "a" * 40
REPOSITORY = "RaoFoundation/subtensor"


class FakeGitHubClient:
    def __init__(self, responses, artifact_entries=None):
        self.repository = REPOSITORY
        self.responses = iter(responses)
        self.artifact_entries = artifact_entries or []
        self.calls = []

    def request_json(self, method, path, query=None, body=None):
        self.calls.append((method, path, query, body))
        return next(self.responses)

    def download(self, path, destination, maximum_bytes):
        self.calls.append(("DOWNLOAD", path, maximum_bytes, None))
        with zipfile.ZipFile(destination, "w", zipfile.ZIP_DEFLATED) as archive:
            for name, content in self.artifact_entries:
                archive.writestr(name, content)


class DocsPreviewGitHubTests(unittest.TestCase):
    def test_resolves_pr_from_workflow_payload_without_commit_lookup(self):
        self.assertEqual(workflow_pr_number('[{"number": 42}]'), "42")
        self.assertIsNone(workflow_pr_number("[]"))
        with self.assertRaises(ApiError):
            workflow_pr_number('[{"number": 42}, {"number": 43}]')
        with self.assertRaises(ApiError):
            workflow_pr_number("{}")

    def test_commit_fallback_requires_one_matching_same_repository_pr(self):
        pulls = [
            {
                "number": 42,
                "head": {"sha": SHA, "repo": {"full_name": REPOSITORY}},
            },
            {
                "number": 43,
                "head": {"sha": SHA, "repo": {"full_name": "fork/repository"}},
            },
        ]
        self.assertEqual(
            associated_pr_number(pulls, SHA, REPOSITORY),
            "42",
        )
        with self.assertRaises(ApiError):
            associated_pr_number([], SHA, REPOSITORY)

    def test_reconciliation_fails_closed_for_stale_or_foreign_heads(self):
        matching = PullState("42", "open", SHA, REPOSITORY)
        stale = PullState("42", "open", "b" * 40, REPOSITORY)
        foreign = PullState("42", "open", SHA, "fork/repository")
        closed = PullState("42", "closed", SHA, REPOSITORY)
        self.assertEqual(reconcile_mode("deploy", matching, SHA, REPOSITORY), "deploy")
        self.assertEqual(reconcile_mode("deploy", stale, SHA, REPOSITORY), "noop")
        self.assertEqual(reconcile_mode("deploy", foreign, SHA, REPOSITORY), "noop")
        self.assertEqual(reconcile_mode("deploy", closed, SHA, REPOSITORY), "cleanup")
        self.assertEqual(
            reconcile_mode("cleanup", matching, SHA, REPOSITORY),
            "cleanup",
        )
        self.assertEqual(reconcile_mode("cleanup", stale, SHA, REPOSITORY), "noop")
        self.assertEqual(reconcile_mode("cleanup", foreign, SHA, REPOSITORY), "noop")

    def test_prepare_validates_artifact_and_emits_bounded_outputs(self):
        responses = [
            {
                "total_count": 1,
                "artifacts": [{"id": 7, "size_in_bytes": 100, "expired": False}],
            },
            {
                "state": "open",
                "head": {"sha": SHA, "repo": {"full_name": REPOSITORY}},
            },
        ]
        client = FakeGitHubClient(
            responses,
            [
                ("docs-preview-action.txt", b"deploy\n"),
                ("docs-preview-pr-number.txt", b"42\n"),
                ("docs-preview-sealed.tgz", b"bundle"),
            ],
        )
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            output = root / "output"
            summary = root / "summary"
            with patch.dict(
                os.environ,
                {
                    "GITHUB_OUTPUT": str(output),
                    "GITHUB_STEP_SUMMARY": str(summary),
                },
            ):
                prepare(
                    client,
                    '[{"number": 42}]',
                    SHA,
                    "99",
                    "bittensor.com",
                    root / "artifact.zip",
                    root / "artifact",
                )
            outputs = output.read_text(encoding="utf-8")
        self.assertIn("pr=42\n", outputs)
        self.assertIn("action=deploy\n", outputs)
        self.assertIn("mode=deploy\n", outputs)
        self.assertIn("alias=pr-42.preview.bittensor.com\n", outputs)
        self.assertFalse(any(call[1].startswith("commits/") for call in client.calls))

    def test_upsert_comment_updates_existing_marker_across_pages(self):
        client = FakeGitHubClient(
            [
                [{"id": index, "body": "other"} for index in range(100)],
                [{"id": 501, "body": "<!-- docs-preview-url -->"}],
                {},
            ]
        )
        upsert_preview_comment(
            client,
            "42",
            "dpl_abc123",
            "https://pr-42.preview.bittensor.com",
        )
        self.assertEqual(client.calls[-1][0:2], ("PATCH", "issues/comments/501"))
        body = client.calls[-1][3]["body"]
        self.assertIn("dpl_abc123", body)
        self.assertIn("https://pr-42.preview.bittensor.com/docs", body)

    def test_upsert_comment_rejects_unbounded_values(self):
        client = FakeGitHubClient([])
        invalid = [
            ("not-a-number", "dpl_abc123", "https://pr-42.preview.bittensor.com"),
            ("42", "bad-id", "https://pr-42.preview.bittensor.com"),
            ("42", "dpl_abc123", "https://example.com\ninjected"),
        ]
        for values in invalid:
            with self.subTest(values=values), self.assertRaises(ValueError):
                upsert_preview_comment(client, *values)


if __name__ == "__main__":
    unittest.main()
