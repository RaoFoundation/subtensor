#!/usr/bin/env python3

import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import Mock

from docs_preview_vercel import (
    ApiError,
    UnsafeEnvironmentVariable,
    VercelClient,
    audit_project,
    deployment_id_for_url,
    deployment_ids_for_pr,
    remove_alias,
    set_alias,
    unsafe_preview_variables,
    validate_configuration,
    write_project_link,
)


class FakeClient(VercelClient):
    def __init__(self, responses):
        self.responses = iter(responses)
        self.calls = []
        super().__init__("token", "team")

    def request(
        self,
        method,
        path,
        query=None,
        body=None,
        allow_missing=False,
    ):
        self.calls.append((method, path, dict(query or {}), body, allow_missing))
        return next(self.responses)


class DocsPreviewVercelTests(unittest.TestCase):
    def test_flags_project_and_shared_preview_variables_without_values(self):
        unsafe = unsafe_preview_variables(
            [
                {
                    "key": "PROJECT_SECRET",
                    "value": "must-not-appear",
                    "target": ["preview"],
                },
                {"key": "PRODUCTION_ONLY", "target": ["production"]},
                {"key": "VERCEL_ENV", "target": ["preview"], "system": True},
            ],
            [
                {"key": "SHARED_SECRET", "target": "preview"},
                {"key": "SHARED_PROD", "target": "production"},
            ],
        )
        self.assertEqual(
            unsafe,
            [
                UnsafeEnvironmentVariable("project", "PROJECT_SECRET"),
                UnsafeEnvironmentVariable("team-shared", "SHARED_SECRET"),
            ],
        )
        self.assertNotIn("must-not-appear", repr(unsafe))

    def test_unknown_environment_shapes_fail_closed(self):
        unsafe = unsafe_preview_variables([{"key": "UNKNOWN"}], [None])
        self.assertEqual(
            unsafe,
            [
                UnsafeEnvironmentVariable("project", "UNKNOWN"),
                UnsafeEnvironmentVariable("team-shared", "<invalid-response>"),
            ],
        )

    def test_project_audit_covers_environment_and_verified_wildcard(self):
        client = Mock()
        client.paginated.side_effect = [[], []]
        client.request.return_value = {
            "name": "*.preview.bittensor.com",
            "verified": True,
        }
        audit_project(client, "prj_preview", "*.preview.bittensor.com")
        self.assertEqual(
            client.paginated.call_args_list[0].args,
            ("/v10/projects/prj_preview/env", "envs", {"decrypt": "false"}),
        )
        self.assertEqual(
            client.paginated.call_args_list[1].args,
            ("/v1/env", "data", {"projectId": "prj_preview"}),
        )
        self.assertEqual(
            client.request.call_args.args,
            (
                "GET",
                "/v9/projects/prj_preview/domains/%2A.preview.bittensor.com",
            ),
        )

    def test_project_audit_rejects_secrets_without_logging_values(self):
        client = Mock()
        client.paginated.side_effect = [
            [{"key": "SECRET_VALUE", "value": "do-not-log", "target": ["preview"]}],
            [],
        ]
        with self.assertRaises(ApiError) as context:
            audit_project(client, "prj_preview", "*.preview.bittensor.com")
        self.assertIn("SECRET_VALUE", str(context.exception))
        self.assertNotIn("do-not-log", str(context.exception))
        client.request.assert_not_called()

    def test_project_audit_rejects_missing_or_unverified_wildcard(self):
        for response in (
            None,
            {"name": "*.preview.bittensor.com", "verified": False},
            {"name": "other.example", "verified": True},
        ):
            with self.subTest(response=response):
                client = Mock()
                client.paginated.side_effect = [[], []]
                client.request.return_value = response
                with self.assertRaises(ApiError):
                    audit_project(
                        client,
                        "prj_preview",
                        "*.preview.bittensor.com",
                    )

    def test_configuration_requires_distinct_valid_projects(self):
        validate_configuration(
            "team_123",
            "prj_preview",
            "prj_production",
            "bittensor.com",
        )
        with self.assertRaises(ValueError):
            validate_configuration(
                "team_123",
                "prj_same",
                "prj_same",
                "bittensor.com",
            )
        with self.assertRaises(ValueError):
            validate_configuration(
                "team_123",
                "prj_preview",
                "prj_production",
                "../bittensor.com",
            )

    def test_project_link_contains_only_expected_identifiers_and_settings(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            write_project_link(root, "team_123", "prj_preview", True)
            payload = json.loads(
                (root / ".vercel/project.json").read_text(encoding="utf-8")
            )
        self.assertEqual(payload["orgId"], "team_123")
        self.assertEqual(payload["projectId"], "prj_preview")
        self.assertEqual(
            payload["settings"]["rootDirectory"],
            "website/apps/bittensor-website",
        )

    def test_pagination_collects_every_page_and_rejects_cursor_loops(self):
        client = FakeClient(
            [
                {
                    "data": [{"id": "one"}],
                    "pagination": {"next": 123},
                },
                {
                    "data": [{"id": "two"}],
                    "pagination": {"next": None},
                },
            ]
        )
        self.assertEqual(
            client.paginated("/v1/env", "data", {"projectId": "project"}),
            [{"id": "one"}, {"id": "two"}],
        )
        self.assertEqual(client.calls[1][2]["until"], "123")

        loop = FakeClient(
            [
                {"data": [], "pagination": {"next": 123}},
                {"data": [], "pagination": {"next": 123}},
            ]
        )
        with self.assertRaises(ApiError):
            loop.paginated("/v1/env", "data")

    def test_deployment_lookup_and_alias_switch_use_bounded_identifiers(self):
        client = FakeClient([{"id": "dpl_abc123", "projectId": "prj_preview"}, {}])
        deployment_id = deployment_id_for_url(
            client,
            "prj_preview",
            "https://preview-abc.vercel.app",
        )
        self.assertEqual(deployment_id, "dpl_abc123")
        set_alias(client, deployment_id, "pr-42.preview.bittensor.com")
        self.assertEqual(
            client.calls[1],
            (
                "POST",
                "/v2/deployments/dpl_abc123/aliases",
                {},
                {"alias": "pr-42.preview.bittensor.com"},
                False,
            ),
        )
        wrong_project = FakeClient(
            [{"id": "dpl_abc123", "projectId": "prj_production"}]
        )
        with self.assertRaises(ApiError):
            deployment_id_for_url(
                wrong_project,
                "prj_preview",
                "https://preview-abc.vercel.app",
            )

    def test_alias_removal_is_confined_to_preview_project(self):
        client = FakeClient(
            [
                {"uid": "alias_123", "projectId": "prj_preview"},
                {},
            ]
        )
        self.assertTrue(
            remove_alias(client, "prj_preview", "pr-42.preview.bittensor.com")
        )
        self.assertEqual(client.calls[1][0:2], ("DELETE", "/now/aliases/alias_123"))

        wrong_project = FakeClient(
            [{"uid": "alias_123", "projectId": "prj_production"}]
        )
        with self.assertRaises(ApiError):
            remove_alias(
                wrong_project,
                "prj_preview",
                "pr-42.preview.bittensor.com",
            )
        self.assertEqual(len(wrong_project.calls), 1)

    def test_selects_only_matching_obsolete_deployments(self):
        deployments = [
            {"uid": "keep", "meta": {"docsPreviewPr": "42"}},
            {"uid": "old", "meta": {"docsPreviewPr": 42}},
            {"uid": "other", "meta": {"docsPreviewPr": "43"}},
            {"uid": "unmarked", "meta": {}},
        ]
        self.assertEqual(
            deployment_ids_for_pr(deployments, "42", keep_deployment_id="keep"),
            ["old"],
        )

    def test_invalid_deployment_response_fails_closed(self):
        with self.assertRaises(ApiError):
            deployment_ids_for_pr([None], "42")


if __name__ == "__main__":
    unittest.main()
