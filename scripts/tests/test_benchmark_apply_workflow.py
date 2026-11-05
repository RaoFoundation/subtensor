"""Check the apply trust boundary and publish behavior without contacting GitHub."""

import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import textwrap
import unittest


ROOT = Path(__file__).resolve().parents[2]
APPLY = (ROOT / ".github/workflows/apply-benchmark-patch.yml").read_text()
PRODUCER = (ROOT / ".github/workflows/run-benchmarks.yml").read_text()


def mapping_block(document, key, indent):
    """Extract a block from these workflows without a third-party YAML parser."""
    match = re.search(rf"(?m)^{' ' * indent}{re.escape(key)}:[^\n]*\n", document)
    if match is None:
        raise AssertionError(f"Missing workflow key: {key}")
    lines = []
    for line in document[match.end() :].splitlines(keepends=True):
        if line.strip() and not line.lstrip().startswith("#"):
            if len(line) - len(line.lstrip()) <= indent:
                break
        lines.append(line)
    return "".join(lines)


def steps(job):
    return re.split(r"(?m)^      - name: ", job)[1:]


def named_step(job, name):
    matches = [step for step in steps(job) if step.splitlines()[0] == name]
    if len(matches) != 1:
        raise AssertionError(f"Expected one workflow step: {name}")
    return matches[0]


def step_shell(step):
    return textwrap.dedent(mapping_block(step, "run", 8))


WRITER = mapping_block(APPLY, "apply", 2)
TESTER = mapping_block(PRODUCER, "test-tooling", 2)
PUBLISH = step_shell(
    named_step(WRITER, "Commit and publish only to the unchanged PR branch")
)


class WorkflowTrustBoundaryTests(unittest.TestCase):
    def test_privileged_workflow_uses_trusted_event_and_explicit_label_gate(self):
        self.assertRegex(APPLY, r"(?m)^  pull_request_target:\s*$")
        self.assertNotRegex(APPLY, r"(?m)^  (pull_request|workflow_dispatch):")
        self.assertIn("github.event.label.name == 'apply-benchmark-patch'", WRITER)
        self.assertIn(
            "github.event.pull_request.head.repo.full_name == github.repository", WRITER
        )
        self.assertEqual(
            re.findall(r"(?m)^  ([\w-]+):", mapping_block(APPLY, "jobs", 0)), ["apply"]
        )

    def test_default_permission_and_isolated_tests_are_read_only(self):
        self.assertEqual(
            mapping_block(APPLY, "permissions", 0).strip(), "contents: read"
        )
        self.assertNotIn(": write", mapping_block(PRODUCER, "permissions", 0))
        self.assertEqual(
            mapping_block(TESTER, "permissions", 4).strip(), "contents: read"
        )
        self.assertIn("needs: trusted-pr", TESTER)
        self.assertIn("python3 -m unittest discover", TESTER)
        self.assertIn(
            "ref: ${{ github.event.pull_request.head.sha || github.sha }}", TESTER
        )
        self.assertIn("persist-credentials: false", TESTER)
        self.assertRegex(PRODUCER, r"needs: \[trusted-pr, decide, test-tooling\]")

    def test_tests_and_writer_use_fresh_hosted_runners_without_shared_state(self):
        for job in (TESTER, WRITER):
            with self.subTest(job=job.splitlines()[0]):
                self.assertIn("runs-on: ubuntu-latest", job)
                self.assertNotRegex(job, r"(?m)^\s+(outputs|container|services):")
                self.assertNotRegex(job, r"uses: .*cache|uses: .*artifact")
                self.assertNotIn("needs.test-tooling.outputs", job)
        self.assertNotIn("secrets.", TESTER)
        self.assertNotIn("GH_TOKEN:", TESTER)

    def test_writer_checks_out_trusted_workflow_and_exact_source_without_credentials(
        self,
    ):
        checkouts = [
            step for step in steps(WRITER) if "uses: actions/checkout@" in step
        ]
        self.assertEqual(len(checkouts), 2)
        self.assertIn("ref: ${{ github.workflow_sha }}", checkouts[0])
        self.assertIn("path: trusted", checkouts[0])
        self.assertIn("ref: ${{ github.event.pull_request.head.sha }}", checkouts[1])
        self.assertIn("path: source", checkouts[1])
        for checkout in checkouts:
            self.assertIn("persist-credentials: false", checkout)
        self.assertIn(
            'test "$(git -C trusted rev-parse HEAD)" = "$TRUSTED_WORKFLOW_SHA"', WRITER
        )
        self.assertIn('test "$(git -C source rev-parse HEAD)" = "$PR_HEAD_SHA"', WRITER)

    def test_writer_executes_only_isolated_trusted_python(self):
        helper = named_step(
            WRITER, "Download and stage validated weights with trusted tooling"
        )
        self.assertIn("working-directory: trusted", helper)
        self.assertIn("python3 -I scripts/apply_benchmark_patch.py", step_shell(helper))
        self.assertIn('--apply-to "$GITHUB_WORKSPACE/source"', step_shell(helper))
        self.assertNotRegex(
            WRITER, r"unittest|pytest|working-directory: source|uses: \./"
        )
        python_calls = re.findall(r"(?m)^\s*(python\S*[^\n]*)", WRITER)
        self.assertEqual(
            python_calls, ["python3 -I scripts/apply_benchmark_patch.py \\"]
        )
        self.assertEqual(
            re.findall(r"(?m)^\s+uses: ([^\s]+)", WRITER),
            ["actions/checkout@34e114876b0b11c390a56381ad16ebd13914f8d5"] * 2,
        )

    def test_writer_is_limited_to_required_permissions(self):
        permissions = mapping_block(WRITER, "permissions", 4)
        self.assertEqual(
            dict(re.findall(r"(?m)^      ([\w-]+): (\w+)$", permissions)),
            {"contents": "write", "pull-requests": "write", "actions": "read"},
        )

    def test_publish_guards_source_identity_parent_and_exact_branch_lease(self):
        self.assertIn("[.state, .head.repo.full_name, .head.ref, .head.sha]", PUBLISH)
        self.assertLess(
            PUBLISH.index('if [[ "$current" != "$expected" ]]'),
            PUBLISH.index("commit -m"),
        )
        parent_guard = (
            'test "$(git -C "$source" show -s --format=%P HEAD)" = "$PR_HEAD_SHA"'
        )
        self.assertIn(parent_guard, PUBLISH)
        self.assertLess(
            PUBLISH.index(parent_guard), PUBLISH.index("push --force-with-lease=")
        )
        self.assertIn(
            '--force-with-lease="refs/heads/$PR_HEAD_BRANCH:$PR_HEAD_SHA"', PUBLISH
        )
        self.assertIn('"HEAD:refs/heads/$PR_HEAD_BRANCH"', PUBLISH)
        self.assertIn("core.hooksPath=/dev/null", PUBLISH)
        self.assertIn("commit.gpgsign=false", PUBLISH)


@unittest.skipUnless(
    shutil.which("git") and shutil.which("bash"), "git and bash required"
)
class PublishShellTests(unittest.TestCase):
    """Run the real publish shell against a temporary local bare Git repository."""

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="benchmark-publish-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.source = self.root / "source"
        self.source.mkdir()
        self.trusted = self.root / "trusted"
        self.trusted.mkdir()
        self.origin = self.root / "origin.git"
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.log = self.root / "calls.jsonl"
        self.real_git = shutil.which("git")
        self.branch = "codex/targeted-benchmarks"
        self.env = {
            key: value
            for key, value in os.environ.items()
            if not key.startswith(("GIT_", "GH_"))
        }
        self.env.update(
            {
                "GIT_CONFIG_NOSYSTEM": "1",
                "GIT_CONFIG_GLOBAL": "/dev/null",
                "GITHUB_WORKSPACE": str(self.root),
                "GITHUB_REPOSITORY": "example/subtensor",
                "PR_HEAD_BRANCH": self.branch,
                "PR_NUMBER": "12",
                "FIXTURE_REAL_GIT": self.real_git,
                "FIXTURE_ORIGIN": str(self.origin),
                "FIXTURE_CALL_LOG": str(self.log),
            }
        )
        self.git("init", "-b", self.branch)
        self.git("config", "user.name", "Fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        self.weights = self.source / "pallets/subtensor/src/weights.rs"
        self.weights.parent.mkdir(parents=True)
        self.weights.write_text("// Fixture weights\nconst WEIGHT: u64 = 1;\n")
        (self.source / "README.md").write_text("Fixture\n")
        self.git("add", ".")
        self.git("commit", "-m", "fixture base")
        self.sha = self.git("rev-parse", "HEAD")
        self.env["PR_HEAD_SHA"] = self.sha
        self.git("init", "--bare", str(self.origin))
        self.git("push", str(self.origin), f"HEAD:refs/heads/{self.branch}")
        self.git("checkout", "--detach", self.sha)
        self.write_executable(
            "gh",
            """
            import json, os, subprocess, sys
            args = sys.argv[1:]
            with open(os.environ["FIXTURE_CALL_LOG"], "a") as log:
                log.write(json.dumps(["gh", *args]) + "\\n")
            if args[:1] == ["api"]:
                sha = os.environ.get("FIXTURE_PR_SHA") or subprocess.check_output(
                    [os.environ["FIXTURE_REAL_GIT"], "--git-dir", os.environ["FIXTURE_ORIGIN"],
                     "rev-parse", "refs/heads/" + os.environ["PR_HEAD_BRANCH"]], text=True
                ).strip()
                print("\\t".join(["open", os.environ["GITHUB_REPOSITORY"], os.environ["PR_HEAD_BRANCH"], sha]))
            elif args[:2] != ["pr", "edit"]:
                raise SystemExit("Unexpected fixture gh invocation")
            """,
        )
        self.write_executable(
            "git",
            """
            import json, os, subprocess, sys
            args = sys.argv[1:]
            with open(os.environ["FIXTURE_CALL_LOG"], "a") as log:
                log.write(json.dumps(["git", *args]) + "\\n")
            if "push" in args:
                expected = "https://github.com/" + os.environ["GITHUB_REPOSITORY"] + ".git"
                if expected not in args:
                    raise SystemExit("Unexpected fixture push destination")
                args[args.index(expected)] = os.environ["FIXTURE_ORIGIN"]
            if any(arg.startswith(("https://", "http://", "ssh://", "git@")) for arg in args):
                raise SystemExit("Network access forbidden in publish fixture")
            raise SystemExit(subprocess.run([os.environ["FIXTURE_REAL_GIT"], *args]).returncode)
            """,
        )
        self.env["PATH"] = str(self.bin) + os.pathsep + os.environ.get("PATH", "")

    def git(self, *args):
        result = subprocess.run(
            [self.real_git, "-C", str(self.source), *args],
            env=self.env,
            text=True,
            capture_output=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout.strip()

    def write_executable(self, name, body):
        path = self.bin / name
        path.write_text(f"#!{sys.executable}\n" + textwrap.dedent(body))
        path.chmod(0o755)

    def publish(self):
        return subprocess.run(
            ["bash", "-c", PUBLISH],
            cwd=self.trusted,
            env=self.env,
            text=True,
            capture_output=True,
        )

    def calls(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()]

    def stage_weights(self):
        self.weights.write_text("// Fixture weights\nconst WEIGHT: u64 = 2;\n")
        self.git("add", "pallets/subtensor/src/weights.rs")

    def test_no_drift_removes_label_without_commit_or_push(self):
        result = self.publish()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("no weight drift", result.stdout)
        self.assertEqual(self.git("rev-parse", "HEAD"), self.sha)
        self.assertFalse(any("push" in call for call in self.calls()))
        self.assertTrue(any(call[:3] == ["gh", "pr", "edit"] for call in self.calls()))

    def test_changed_pr_head_refuses_commit_push_and_label_removal(self):
        self.stage_weights()
        self.env["FIXTURE_PR_SHA"] = "b" * 40
        result = self.publish()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("PR changed after labeling", result.stderr)
        self.assertEqual(self.git("rev-parse", "HEAD"), self.sha)
        self.assertFalse(
            any("push" in call or "commit" in call for call in self.calls())
        )
        self.assertFalse(any(call[:3] == ["gh", "pr", "edit"] for call in self.calls()))
        self.assertEqual(
            self.git("diff", "--cached", "--name-only"),
            "pallets/subtensor/src/weights.rs",
        )

    def test_weight_commit_publishes_with_exact_lease_and_single_source_parent(self):
        self.stage_weights()
        result = self.publish()
        self.assertEqual(result.returncode, 0, result.stderr)
        new_sha = self.git("rev-parse", "HEAD")
        self.assertNotEqual(new_sha, self.sha)
        self.assertEqual(self.git("show", "-s", "--format=%P", "HEAD"), self.sha)
        self.assertEqual(
            self.git(
                "--git-dir", str(self.origin), "rev-parse", f"refs/heads/{self.branch}"
            ),
            new_sha,
        )
        self.assertEqual(
            self.git("diff", "--name-only", self.sha, "HEAD"),
            "pallets/subtensor/src/weights.rs",
        )
        pushes = [call for call in self.calls() if call[0] == "git" and "push" in call]
        self.assertEqual(len(pushes), 1)
        self.assertIn(
            f"--force-with-lease=refs/heads/{self.branch}:{self.sha}", pushes[0]
        )
        self.assertTrue(any(call[:3] == ["gh", "pr", "edit"] for call in self.calls()))


if __name__ == "__main__":
    unittest.main()
