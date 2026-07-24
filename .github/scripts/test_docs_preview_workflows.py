#!/usr/bin/env python3

import json
import re
import unittest
from pathlib import Path


REPOSITORY = Path(__file__).resolve().parents[2]
DEPLOY_WORKFLOW = REPOSITORY / ".github/workflows/deploy-docs-preview.yml"
REQUEST_WORKFLOW = REPOSITORY / ".github/workflows/request-docs-preview.yml"
BUNDLE_SCRIPT = REPOSITORY / ".github/scripts/docs_preview_bundle.py"
ARTIFACT_SCRIPT = REPOSITORY / ".github/scripts/docs_preview_artifact.py"
GITHUB_SCRIPT = REPOSITORY / ".github/scripts/docs_preview_github.py"
VERCEL_SCRIPT = REPOSITORY / ".github/scripts/docs_preview_vercel.py"
CLI_PACKAGE = REPOSITORY / ".github/docs-preview-vercel/package.json"
CLI_LOCK = REPOSITORY / ".github/docs-preview-vercel/package-lock.json"
ACTION_SHA = re.compile(r"^\s*uses:\s+[^#\s]+@[0-9a-f]{40}(?:\s+#.*)?$")


class DocsPreviewWorkflowPolicyTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.deploy = DEPLOY_WORKFLOW.read_text(encoding="utf-8")
        cls.request = REQUEST_WORKFLOW.read_text(encoding="utf-8")
        cls.bundle = BUNDLE_SCRIPT.read_text(encoding="utf-8")
        cls.artifact = ARTIFACT_SCRIPT.read_text(encoding="utf-8")
        cls.github = GITHUB_SCRIPT.read_text(encoding="utf-8")
        cls.vercel = VERCEL_SCRIPT.read_text(encoding="utf-8")

    def test_every_external_action_is_pinned_to_a_commit(self):
        for workflow in (self.deploy, self.request):
            uses_lines = [
                line
                for line in workflow.splitlines()
                if line.lstrip().startswith("uses:")
            ]
            self.assertTrue(uses_lines)
            for line in uses_lines:
                self.assertRegex(line, ACTION_SHA)

    def test_untrusted_workflow_is_secret_free_and_ephemeral(self):
        self.assertNotIn("secrets.", self.request)
        self.assertNotIn("self-hosted", self.request)
        self.assertGreaterEqual(self.request.count("runs-on: ubuntu-24.04"), 2)
        self.assertIn("persist-credentials: false", self.request)
        self.assertIn("cancel-in-progress: true", self.request)
        self.assertIn("retention-days: 1", self.request)

    def test_trusted_workflow_never_checks_out_pr_head(self):
        self.assertNotIn("pull_request.head.sha", self.deploy)
        self.assertIn("ref: ${{ github.event.repository.default_branch }}", self.deploy)
        self.assertIn("persist-credentials: false", self.deploy)
        self.assertNotIn("self-hosted", self.deploy)
        self.assertIn("runs-on: ubuntu-24.04", self.deploy)

    def test_trusted_workflow_only_uses_vercel_deploy_token(self):
        secret_names = set(re.findall(r"secrets\.([A-Z0-9_]+)", self.deploy))
        self.assertEqual(secret_names, {"VERCEL_TOKEN"})
        self.assertNotIn("GODADDY", self.deploy)
        self.assertNotIn("api.godaddy.com", self.deploy)
        self.assertNotRegex(
            self.deploy,
            r"(?m)^\s{4}env:\s*\n(?:\s{6}.+\n)*\s{6}\w+:\s*\$\{\{\s*secrets\.",
        )

    def test_dedicated_preview_project_is_enforced_in_both_halves(self):
        for workflow in (self.deploy, self.request):
            self.assertIn("VERCEL_DOCS_PREVIEW_PROJECT_ID", workflow)
            self.assertIn("VERCEL_DOCS_PROJECT_ID", workflow)
            self.assertIn("validate-config", workflow)
        self.assertIn(
            "VERCEL_DOCS_PREVIEW_PROJECT_ID must identify a dedicated",
            self.vercel,
        )
        self.assertIn('"non-production project"', self.vercel)
        self.assertNotIn("secrets.VERCEL_DOCS_PROJECT_ID", self.deploy)
        self.assertIn("vars.VERCEL_DOCS_PREVIEW_PROJECT_ID != ''", self.deploy)
        self.assertGreaterEqual(
            self.request.count("vars.VERCEL_DOCS_PREVIEW_PROJECT_ID != ''"),
            2,
        )

    def test_deploy_queue_and_stale_run_controls_are_present(self):
        self.assertIn("cancel-in-progress: false", self.deploy)
        self.assertIn("group: docs-preview-", self.deploy)
        self.assertIn(
            "Resolve, download, and authorize the producing artifact", self.deploy
        )
        self.assertIn("Re-check PR head immediately before deployment", self.deploy)
        self.assertIn("steps.recheck.outputs.authorized == 'true'", self.deploy)
        self.assertIn("steps.meta.outputs.mode == 'cleanup'", self.deploy)
        self.assertIn("head_repository.full_name == github.repository", self.deploy)

    def test_invalid_artifact_actions_and_shapes_fail_closed(self):
        self.assertIn("invalid docs-preview action", self.artifact)
        self.assertIn("artifact contains an unexpected file set", self.artifact)
        self.assertIn("expected one {artifact_name} artifact", self.github)
        self.assertIn("does not match workflow PR", self.artifact)

    def test_secure_extractors_are_used_without_inline_archive_parsing(self):
        self.assertIn("docs_preview_github.py", self.deploy)
        self.assertIn("docs_preview_bundle.py", self.deploy)
        self.assertNotIn("gh run download", self.deploy)
        self.assertNotIn("tarfile", self.deploy)
        self.assertNotIn("extractall", self.artifact)
        self.assertNotIn("extractall", self.bundle)
        self.assertNotIn("getmembers", self.bundle)
        self.assertNotIn('lstrip("./")', self.bundle)
        self.assertIn('mode="r|gz"', self.bundle)

    def test_bundle_is_self_contained_and_validates_every_vercel_file_map(self):
        self.assertIn("seal_bundle", self.bundle)
        self.assertIn('ALLOWED_ROOTS = (".vercel/output",)', self.bundle)
        self.assertIn("filePathMap", self.bundle)
        self.assertIn("filePathMap escapes the deployment root", self.bundle)
        self.assertIn(".docs-preview-files", self.bundle)
        self.assertNotIn("website/node_modules", self.deploy)
        self.assertIn("docs_preview_bundle.py \\", self.request)
        self.assertIn("seal \\", self.request)

    def test_preview_environment_and_deployment_lifecycle_are_guarded(self):
        self.assertIn("audit-project", self.deploy)
        self.assertIn('--preview-domain "*.preview.${DOCS_DOMAIN}"', self.deploy)
        self.assertIn(
            'client.paginated(project_path, "envs", {"decrypt": "false"})', self.vercel
        )
        self.assertIn('"projectId": project_id', self.vercel)
        self.assertIn('--meta "docsPreviewPr=${PR_NUMBER}"', self.deploy)
        self.assertGreaterEqual(self.deploy.count("delete-pr-deployments"), 2)
        self.assertIn("Roll back an unpromoted failed deployment", self.deploy)
        self.assertIn("id: alias", self.deploy)
        self.assertIn("steps.alias.outcome == 'skipped'", self.deploy)
        self.assertNotIn("add-domain", self.deploy)
        self.assertNotIn("remove-domain", self.deploy)

    def test_trusted_workflow_delegates_control_plane_logic_to_tested_modules(self):
        self.assertLessEqual(len(self.deploy.splitlines()), 320)
        for forbidden in ("curl ", "jq ", "gh api", "node -e", "api.vercel.com"):
            self.assertNotIn(forbidden, self.deploy)
        self.assertGreaterEqual(self.deploy.count("docs_preview_github.py"), 3)
        self.assertGreaterEqual(self.deploy.count("docs_preview_vercel.py"), 8)

    def test_cli_graph_is_locked_and_installed_without_scripts(self):
        package = json.loads(CLI_PACKAGE.read_text(encoding="utf-8"))
        lock = json.loads(CLI_LOCK.read_text(encoding="utf-8"))
        self.assertEqual(package["dependencies"], {"vercel": "56.4.1"})
        self.assertEqual(lock["packages"][""]["dependencies"], package["dependencies"])
        self.assertEqual(
            lock["packages"]["node_modules/vercel"]["version"],
            "56.4.1",
        )
        self.assertIn("tar", package["overrides"])
        for workflow in (self.deploy, self.request):
            self.assertIn("npm ci --ignore-scripts", workflow)
            self.assertIn("npm audit --audit-level=high", workflow)
            self.assertNotIn("npm install ", workflow)
            self.assertNotIn("npx ", workflow)


if __name__ == "__main__":
    unittest.main()
