#!/usr/bin/env python3

import unittest
from unittest.mock import Mock

from docs_preview_vercel import (
    ApiError,
    UnsafeEnvironmentVariable,
    VercelClient,
    audit_environment,
    deployment_ids_for_pr,
    unsafe_preview_variables,
)


class FakeClient(VercelClient):
    def __init__(self, responses):
        self.responses = iter(responses)
        self.calls = []
        super().__init__("token", "team")

    def request(self, method, path, query=None):
        self.calls.append((method, path, dict(query or {})))
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

    def test_environment_audit_covers_project_and_linked_shared_variables(self):
        client = Mock()
        client.paginated.side_effect = [[], []]
        audit_environment(client, "prj_preview")
        self.assertEqual(
            client.paginated.call_args_list[0].args,
            ("/v10/projects/prj_preview/env", "envs", {"decrypt": "false"}),
        )
        self.assertEqual(
            client.paginated.call_args_list[1].args,
            ("/v1/env", "data", {"projectId": "prj_preview"}),
        )

        client.paginated.side_effect = [
            [{"key": "SECRET_VALUE", "value": "do-not-log", "target": ["preview"]}],
            [],
        ]
        with self.assertRaises(ApiError) as context:
            audit_environment(client, "prj_preview")
        self.assertIn("SECRET_VALUE", str(context.exception))
        self.assertNotIn("do-not-log", str(context.exception))

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
