"""Validate exact-run benchmark artifact selection without calling GitHub."""

import importlib.util
import io
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tarfile
import tempfile
import textwrap
import unittest


SCRIPTS = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "apply_benchmark_patch", SCRIPTS / "apply_benchmark_patch.py"
)
apply_patch = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(apply_patch)

REPO = "example/subtensor"
BRANCH = "codex/targeted-benchmarks"
SHA = "a" * 40


def run_fixture(run_id=200, **overrides):
    result = {
        "id": run_id,
        "event": "workflow_dispatch",
        "head_sha": SHA,
        "head_branch": BRANCH,
        "head_repository": {"full_name": REPO},
        "repository": {"full_name": REPO},
        "status": "completed",
        # Benchmark drift makes the workflow fail even with a complete patch.
        "conclusion": "failure",
        "run_started_at": "2026-11-04T12:00:00Z",
        "created_at": "2026-11-04T12:00:00Z",
        "run_attempt": 1,
        "html_url": f"https://github.com/{REPO}/actions/runs/{run_id}",
        "pull_requests": [],
    }
    result.update(overrides)
    return result


def job_fixture(conclusion="failure", **overrides):
    result = {
        "name": "validate-benchmarks",
        "status": "completed",
        "conclusion": conclusion,
    }
    result.update(overrides)
    return result


def artifact_fixture(artifact_id=300, **overrides):
    result = {
        "id": artifact_id,
        "name": "bench-patch",
        "expired": False,
        "created_at": "2026-11-04T13:00:00Z",
    }
    result.update(overrides)
    return result


def summary_fixture(run=None, status="COMPLETE"):
    run = run or run_fixture()
    return (
        f"Head SHA: {SHA}\n"
        f"Run ID: {run['id']}\n"
        f"Run attempt: {run['run_attempt']}\n"
        f"Status: {status}\n"
        "Completed pallets: 1/1\n"
        "Selected scope (completion covers only this selection):\n"
        "  pallet_subtensor: add_stake,remove_stake\n"
    )


def write_archive(path, summary, patch=b"fixture patch\n", extra=()):
    with tarfile.open(path, "w:gz") as archive:
        members = [(".bench_patch/summary.txt", summary.encode())]
        if patch is not None:
            members.append((".bench_patch/benchmark_patch.diff", patch))
        for name, content in members:
            member = tarfile.TarInfo(name)
            member.size = len(content)
            archive.addfile(member, io.BytesIO(content))
        for member, content in extra:
            archive.addfile(member, io.BytesIO(content) if content else None)


class RunSelectionTest(unittest.TestCase):
    def select(self, runs, jobs=None):
        jobs = jobs or (lambda run: [job_fixture()])
        return apply_patch.select_latest_run(runs, REPO, BRANCH, SHA, jobs)

    def test_latest_manual_targeted_run_wins_over_older_pr_full_run(self):
        older = run_fixture(
            100, event="pull_request", run_started_at="2026-11-04T11:00:00Z"
        )
        latest = run_fixture(200)
        self.assertEqual(self.select([older, latest]), latest)

    def test_matching_full_and_targeted_runs_are_equally_eligible(self):
        for event in ("pull_request", "workflow_dispatch", "schedule"):
            with self.subTest(event=event):
                run = run_fixture(event=event)
                self.assertEqual(self.select([run]), run)

    def test_newer_skipped_gate_does_not_hide_latest_benchmark(self):
        latest = run_fixture()
        gate_only = run_fixture(201, run_started_at="2026-11-04T14:00:00Z")
        seen = []

        def jobs(run):
            seen.append(run["id"])
            return [job_fixture("skipped" if run["id"] == 201 else "failure")]

        self.assertEqual(self.select([latest, gate_only], jobs), latest)
        self.assertEqual(seen, [201, 200])

    def test_queued_gate_only_run_does_not_hide_latest_benchmark(self):
        latest = run_fixture()
        gate_only = run_fixture(
            201,
            display_title="Benchmark gate only",
            status="queued",
            conclusion=None,
            run_started_at="2026-11-04T14:00:00Z",
        )

        def jobs(run):
            self.assertEqual(run["id"], latest["id"])
            return [job_fixture()]

        self.assertEqual(self.select([latest, gate_only], jobs), latest)

    def test_rerun_start_time_beats_newer_run_id(self):
        rerun = run_fixture(100, run_attempt=2, run_started_at="2026-11-04T15:00:00Z")
        newer_id = run_fixture(200)
        self.assertEqual(self.select([newer_id, rerun]), rerun)

    def test_equal_start_times_use_run_id(self):
        self.assertEqual(self.select([run_fixture(100), run_fixture(200)])["id"], 200)

    def test_running_latest_run_blocks_older_completed_results(self):
        older = run_fixture(100)
        latest = run_fixture(
            200,
            status="in_progress",
            conclusion=None,
            run_started_at="2026-11-04T14:00:00Z",
        )
        with self.assertRaises(apply_patch.ValidationError):
            self.select([older, latest])

    def test_queued_latest_run_without_jobs_blocks_older_results(self):
        older = run_fixture(100)
        latest = run_fixture(
            200,
            status="queued",
            conclusion=None,
            run_started_at="2026-11-04T14:00:00Z",
        )
        with self.assertRaises(apply_patch.ValidationError):
            self.select([older, latest], lambda run: [])

    def test_missing_benchmark_job_does_not_fall_back(self):
        with self.assertRaises(apply_patch.ValidationError):
            self.select([run_fixture()], lambda run: [{"name": "decide"}])

    def test_wrong_source_sha_branch_repository_and_event_are_excluded(self):
        mismatches = (
            {"head_sha": "b" * 40},
            {"head_branch": "another-branch"},
            {"head_repository": {"full_name": "other/subtensor"}},
            {"event": "workflow_run"},
        )
        for mismatch in mismatches:
            with self.subTest(mismatch=mismatch):
                self.assertEqual(
                    apply_patch.matching_runs(
                        [run_fixture(**mismatch)], REPO, BRANCH, SHA
                    ),
                    [],
                )

    def test_missing_head_repository_is_not_assumed_trusted(self):
        run = run_fixture()
        del run["head_repository"]
        self.assertEqual(apply_patch.matching_runs([run], REPO, BRANCH, SHA), [])

    def test_pr_head_association_can_identify_merge_commit_run(self):
        run = run_fixture(
            event="pull_request",
            head_sha="b" * 40,
            pull_requests=[{"head": {"sha": SHA, "ref": BRANCH}}],
        )
        self.assertEqual(self.select([run]), run)

    def test_no_matching_run_fails(self):
        with self.assertRaises(apply_patch.ValidationError):
            self.select([run_fixture(head_sha="b" * 40)])


class ArtifactSelectionTest(unittest.TestCase):
    def test_bench_patch_selected_among_other_artifacts(self):
        artifact = artifact_fixture()
        self.assertEqual(
            apply_patch.select_artifact(
                [artifact_fixture(100, name="build-output"), artifact]
            ),
            artifact,
        )

    def test_missing_patch_artifact_fails(self):
        with self.assertRaises(apply_patch.ValidationError):
            apply_patch.select_artifact([artifact_fixture(name="build-output")])

    def test_latest_expired_artifact_does_not_fall_back(self):
        with self.assertRaises(apply_patch.ValidationError):
            apply_patch.select_artifact(
                [
                    artifact_fixture(100),
                    artifact_fixture(
                        200, expired=True, created_at="2026-11-04T15:00:00Z"
                    ),
                ]
            )


class ArchiveValidationTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="apply-benchmark-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.archive = self.root / "bench-patch.tgz"
        self.run = run_fixture()

    def read(self):
        return apply_patch.read_patch_archive(self.archive, self.run, SHA)

    def test_complete_drift_results_are_accepted_when_workflow_failed(self):
        summary = summary_fixture(self.run)
        patch = b"selected add_stake and remove_stake patch\n"
        write_archive(self.archive, summary, patch)
        self.assertEqual(self.read(), (summary, patch))

    def test_latest_complete_run_without_diff_is_noop(self):
        summary = summary_fixture(self.run)
        write_archive(self.archive, summary, patch=None)
        self.assertEqual(self.read(), (summary, None))

    def test_empty_patch_is_noop(self):
        summary = summary_fixture(self.run)
        write_archive(self.archive, summary, patch=b"")
        self.assertEqual(self.read(), (summary, None))

    def test_source_commit_run_and_attempt_must_match(self):
        original = summary_fixture(self.run)
        substitutions = (
            (f"Head SHA: {SHA}", f"Head SHA: {'b' * 40}"),
            ("Run ID: 200", "Run ID: 100"),
            ("Run attempt: 1", "Run attempt: 2"),
        )
        for old, new in substitutions:
            with self.subTest(field=old):
                write_archive(self.archive, original.replace(old, new))
                with self.assertRaises(apply_patch.ValidationError):
                    self.read()

    def test_missing_identity_fields_are_rejected(self):
        original = summary_fixture(self.run)
        for prefix in ("Head SHA:", "Run ID:", "Run attempt:", "Status:"):
            with self.subTest(field=prefix):
                summary = "\n".join(
                    line
                    for line in original.splitlines()
                    if not line.startswith(prefix)
                )
                write_archive(self.archive, summary)
                with self.assertRaises(apply_patch.ValidationError):
                    self.read()

    def test_incomplete_and_failed_results_are_rejected(self):
        for status in ("INCOMPLETE", "FAILED"):
            with self.subTest(status=status):
                write_archive(self.archive, summary_fixture(self.run, status=status))
                with self.assertRaises(apply_patch.ValidationError):
                    self.read()

    def test_duplicate_summary_is_rejected(self):
        summary = summary_fixture(self.run)
        member = tarfile.TarInfo(".bench_patch/summary.txt")
        member.size = len(summary.encode())
        write_archive(self.archive, summary, extra=[(member, summary.encode())])
        with self.assertRaises(apply_patch.ValidationError):
            self.read()

    def test_duplicate_identity_line_is_rejected(self):
        write_archive(self.archive, summary_fixture(self.run) + "Run ID: 200\n")
        with self.assertRaises(apply_patch.ValidationError):
            self.read()

    def test_link_members_are_rejected(self):
        for kind in (tarfile.SYMTYPE, tarfile.LNKTYPE):
            with self.subTest(kind=kind):
                member = tarfile.TarInfo(".bench_patch/generated.rs")
                member.type = kind
                member.linkname = ".bench_patch/summary.txt"
                write_archive(
                    self.archive, summary_fixture(self.run), extra=[(member, None)]
                )
                with self.assertRaises(apply_patch.ValidationError):
                    self.read()

    def test_archive_is_read_without_extracting_unrelated_members(self):
        member = tarfile.TarInfo(".bench_patch/generated/weights.rs")
        member.size = len(b"reference weights")
        write_archive(
            self.archive,
            summary_fixture(self.run),
            extra=[(member, b"reference weights")],
        )
        self.read()
        self.assertEqual(list(self.root.iterdir()), [self.archive])


class PatchStagingTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="benchmark-staging-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.checkout = self.root / "checkout"
        self.checkout.mkdir()
        self.relative = "pallets/subtensor/src/weights.rs"
        self.weights = self.checkout / self.relative
        self.weights.parent.mkdir(parents=True)
        self.original = "fn add_stake() { 10 }\nfn remove_stake() { 20 }\n"
        self.weights.write_text(self.original)
        (self.checkout / "README.md").write_text("Fixture documentation\n")
        self.git("init", "--quiet")
        self.git("config", "user.name", "Fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        self.git("add", ".")
        self.git("commit", "--quiet", "-m", "Fixture source")
        self.sha = self.git("rev-parse", "HEAD").decode().strip()
        self.patch_path = self.root / "weights.diff"

    def git(self, *args):
        return subprocess.check_output(
            ["git", "-c", "core.hooksPath=/dev/null", *args],
            cwd=self.checkout,
            stderr=subprocess.PIPE,
        )

    def capture_patch(self):
        self.git("add", "--all")
        self.patch_path.write_bytes(
            self.git("diff", "--cached", "--binary", "--no-ext-diff", "HEAD")
        )
        self.git("reset", "--hard", "--quiet", "HEAD")

    def selected_patch(self):
        self.weights.write_text(self.original.replace("10", "11"))
        self.capture_patch()

    def assert_rejected_without_changes(self):
        before = self.git("status", "--porcelain=v1")
        with self.assertRaises(apply_patch.ValidationError):
            apply_patch.stage_patch(self.checkout, self.patch_path, self.sha)
        self.assertEqual(self.git("status", "--porcelain=v1"), before)
        self.assertEqual(self.weights.read_text(), self.original)

    def test_selected_patch_is_staged_and_other_weights_are_preserved(self):
        self.selected_patch()
        apply_patch.stage_patch(self.checkout, self.patch_path, self.sha)
        self.assertEqual(self.weights.read_text(), self.original.replace("10", "11"))
        self.assertEqual(
            self.git("diff", "--cached", "--name-only").decode().splitlines(),
            [self.relative],
        )
        self.assertEqual(self.git("diff", "--name-only"), b"")
        self.assertEqual(self.git("rev-parse", "HEAD").decode().strip(), self.sha)

    def test_wrong_source_sha_is_rejected(self):
        self.selected_patch()
        with self.assertRaisesRegex(apply_patch.ValidationError, "Checkout HEAD"):
            apply_patch.stage_patch(self.checkout, self.patch_path, "a" * 40)
        self.assertEqual(self.git("status", "--porcelain=v1"), b"")

    def test_dirty_index_and_worktree_are_rejected_without_losing_changes(self):
        self.selected_patch()
        for staged in (False, True):
            with self.subTest(staged=staged):
                (self.checkout / "README.md").write_text("User-owned change\n")
                if staged:
                    self.git("add", "README.md")
                self.assert_rejected_without_changes()
                self.assertEqual(
                    (self.checkout / "README.md").read_text(), "User-owned change\n"
                )

    def test_nonweight_file_is_rejected(self):
        (self.checkout / "README.md").write_text("Updated documentation\n")
        self.capture_patch()
        self.assert_rejected_without_changes()

    def test_binary_weight_change_is_rejected(self):
        self.weights.write_bytes(b"binary\0fixture\n")
        self.capture_patch()
        self.assert_rejected_without_changes()

    def test_executable_mode_change_is_rejected(self):
        self.weights.chmod(0o755)
        self.capture_patch()
        self.assert_rejected_without_changes()
        self.assertEqual(self.weights.stat().st_mode & 0o111, 0)

    def test_symlink_change_is_rejected(self):
        self.weights.unlink()
        self.weights.symlink_to("../../../README.md")
        self.capture_patch()
        self.assert_rejected_without_changes()
        self.assertFalse(self.weights.is_symlink())

    def test_added_weight_file_is_rejected(self):
        added = self.checkout / "pallets/new-pallet/src/weights.rs"
        added.parent.mkdir(parents=True)
        added.write_text("fn new_call() { 10 }\n")
        self.capture_patch()
        self.assert_rejected_without_changes()
        self.assertFalse(added.exists())

    def test_deleted_weight_file_is_rejected(self):
        self.weights.unlink()
        self.capture_patch()
        self.assert_rejected_without_changes()

    def test_renamed_weight_file_is_rejected(self):
        destination = self.checkout / "pallets/renamed/src/weights.rs"
        destination.parent.mkdir(parents=True)
        self.weights.rename(destination)
        self.capture_patch()
        self.assert_rejected_without_changes()
        self.assertFalse(destination.exists())

    def test_mixed_valid_and_invalid_changes_are_rejected_atomically(self):
        self.weights.write_text(self.original.replace("10", "11"))
        added = self.checkout / "pallets/new-pallet/src/weights.rs"
        added.parent.mkdir(parents=True)
        added.write_text("fn new_call() { 10 }\n")
        self.capture_patch()
        self.assert_rejected_without_changes()


class DownloadIntegrationTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="benchmark-download-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.checkout = self.root / "checkout"
        self.checkout.mkdir()
        self.source = self.checkout / "weights.rs"
        self.source.write_text("user-owned weights\n")
        self.output = self.root / "validated"
        self.log = self.root / "gh-calls.jsonl"
        self.older = run_fixture(
            100, event="pull_request", run_started_at="2026-11-04T11:00:00Z"
        )
        self.latest = run_fixture(200)
        self.fixture = {
            "run_pages": [[self.older], [self.latest]],
            "jobs": {
                "100/attempts/1": [job_fixture()],
                "200/attempts/1": [job_fixture()],
            },
            "artifacts": {
                "100": [artifact_fixture(100)],
                "200": [artifact_fixture(200)],
            },
        }
        write_archive(
            self.root / "100.tgz", summary_fixture(self.older), b"full suite patch\n"
        )
        write_archive(
            self.root / "200.tgz", summary_fixture(self.latest), b"selected patch\n"
        )
        bindir = self.root / "bin"
        bindir.mkdir()
        gh = bindir / "gh"
        gh.write_text(
            f"#!{sys.executable}\n"
            + """
import json, os, pathlib, re, shutil, sys
root = pathlib.Path(os.environ['GH_FIXTURE_ROOT'])
args = sys.argv[1:]
with (root / 'gh-calls.jsonl').open('a') as log:
    log.write(json.dumps(args) + '\\n')
fixture = json.loads((root / 'fixture.json').read_text())
if args[0] == 'api':
    endpoint = next(value for value in args if value.startswith('repos/'))
    if endpoint.endswith('/workflows/run-benchmarks.yml/runs'):
        for page in fixture['run_pages']:
            for run in page:
                print(json.dumps(run))
    elif endpoint.endswith('/jobs'):
        key = endpoint.split('/runs/', 1)[1].removesuffix('/jobs')
        for job in fixture['jobs'][key]:
            print(json.dumps(job))
    elif endpoint.endswith('/artifacts'):
        run_id = endpoint.split('/runs/', 1)[1].split('/')[0]
        for artifact in fixture['artifacts'][run_id]:
            print(json.dumps(artifact))
    else:
        sys.exit('Unexpected API endpoint: ' + endpoint)
elif args[:2] == ['run', 'download']:
    destination = pathlib.Path(args[args.index('--dir') + 1])
    shutil.copyfile(root / (args[2] + '.tgz'), destination / 'bench-patch.tgz')
else:
    sys.exit('Unexpected gh command: ' + repr(args))
"""
        )
        gh.chmod(0o755)
        self.env = {
            **os.environ,
            "PATH": f"{bindir}:{os.environ['PATH']}",
            "GH_FIXTURE_ROOT": str(self.root),
        }

    def execute(self, *extra_args, sha=SHA):
        (self.root / "fixture.json").write_text(json.dumps(self.fixture))
        result = subprocess.run(
            [
                sys.executable,
                str(SCRIPTS / "apply_benchmark_patch.py"),
                "--repo",
                REPO,
                "--branch",
                BRANCH,
                "--sha",
                sha,
                "--output-dir",
                str(self.output),
                *extra_args,
            ],
            cwd=self.checkout,
            env=self.env,
            capture_output=True,
            text=True,
            timeout=15,
        )
        if not extra_args:
            self.assertEqual(list(self.checkout.iterdir()), [self.source])
            self.assertEqual(self.source.read_text(), "user-owned weights\n")
        return result

    def calls(self):
        if not self.log.exists():
            return []
        return [json.loads(line) for line in self.log.read_text().splitlines()]

    def downloads(self):
        return [call for call in self.calls() if call[:2] == ["run", "download"]]

    def assert_only_latest_downloaded(self, run_id=200):
        downloads = self.downloads()
        self.assertEqual(len(downloads), 1)
        self.assertEqual(downloads[0][2], str(run_id))
        self.assertEqual(downloads[0][downloads[0].index("--name") + 1], "bench-patch")
        self.assertEqual(downloads[0][downloads[0].index("--repo") + 1], REPO)

    def test_paginated_latest_dispatch_downloads_only_targeted_result(self):
        skipped = run_fixture(300, run_started_at="2026-11-04T14:00:00Z")
        self.fixture["run_pages"].append([skipped])
        self.fixture["jobs"]["300/attempts/1"] = [job_fixture("skipped")]
        result = self.execute()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(
            (self.output / "benchmark_patch.diff").read_bytes(), b"selected patch\n"
        )
        self.assert_only_latest_downloaded()
        self.assertIn(f"branch={BRANCH}", self.calls()[0])
        self.assertFalse(any("/runs/100/" in " ".join(call) for call in self.calls()))

    def test_no_drift_latest_result_does_not_download_older_patch(self):
        write_archive(self.root / "200.tgz", summary_fixture(self.latest), patch=None)
        result = self.execute()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(list(self.output.iterdir()), [self.output / "summary.txt"])
        self.assertIn("no weight changes", result.stdout)
        self.assert_only_latest_downloaded()

    def test_missing_latest_artifact_fails_without_downloading_older_patch(self):
        self.fixture["artifacts"]["200"] = []
        result = self.execute()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("no bench-patch artifact", result.stderr)
        self.assertEqual(self.downloads(), [])
        self.assertFalse(self.output.exists())

    def test_active_latest_run_fails_without_downloading_older_patch(self):
        self.latest["status"] = "in_progress"
        self.fixture["jobs"]["200/attempts/1"] = [
            job_fixture(None, status="in_progress")
        ]
        result = self.execute()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("still running", result.stderr)
        self.assertEqual(self.downloads(), [])
        self.assertFalse(self.output.exists())

    def test_rerun_uses_latest_attempt_jobs_and_matching_summary(self):
        self.older.update(run_attempt=2, run_started_at="2026-11-04T15:00:00Z")
        self.fixture["jobs"]["100/attempts/2"] = [job_fixture()]
        write_archive(
            self.root / "100.tgz", summary_fixture(self.older), b"rerun patch"
        )
        result = self.execute()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assert_only_latest_downloaded(100)
        self.assertTrue(
            any("/runs/100/attempts/2/jobs" in " ".join(call) for call in self.calls())
        )
        self.assertEqual(
            (self.output / "benchmark_patch.diff").read_bytes(), b"rerun patch"
        )

    def test_artifact_from_previous_attempt_is_rejected_without_output(self):
        self.latest["run_attempt"] = 2
        self.fixture["jobs"]["200/attempts/2"] = [job_fixture()]
        result = self.execute()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Run attempt", result.stderr)
        self.assert_only_latest_downloaded()
        self.assertFalse(self.output.exists())

    def test_apply_to_stages_only_the_validated_download(self):
        fixture = PatchStagingTest()
        fixture.setUp()
        self.addCleanup(fixture.doCleanups)
        fixture.selected_patch()
        self.latest["head_sha"] = fixture.sha
        summary = summary_fixture(self.latest).replace(SHA, fixture.sha)
        write_archive(self.root / "200.tgz", summary, fixture.patch_path.read_bytes())
        result = self.execute("--apply-to", str(fixture.checkout), sha=fixture.sha)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("staged for commit", result.stdout)
        self.assertEqual(
            fixture.weights.read_text(), fixture.original.replace("10", "11")
        )
        self.assertEqual(
            fixture.git("diff", "--cached", "--name-only").decode().splitlines(),
            [fixture.relative],
        )

    def test_apply_to_keeps_no_drift_result_a_noop(self):
        fixture = PatchStagingTest()
        fixture.setUp()
        self.addCleanup(fixture.doCleanups)
        self.latest["head_sha"] = fixture.sha
        summary = summary_fixture(self.latest).replace(SHA, fixture.sha)
        write_archive(self.root / "200.tgz", summary, patch=None)
        result = self.execute("--apply-to", str(fixture.checkout), sha=fixture.sha)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(fixture.git("status", "--porcelain=v1"), b"")


class WorkflowGateTest(unittest.TestCase):
    def test_apply_label_never_starts_another_benchmark(self):
        workflow = (SCRIPTS.parent / ".github/workflows/run-benchmarks.yml").read_text()
        gate = re.search(
            r"      - name: Gate on event and label\n"
            r"        id: gate\n"
            r"        run: \|\n((?:          .*\n|\n)+)",
            workflow,
        )
        self.assertIsNotNone(gate)
        script = textwrap.dedent(gate.group(1))
        cases = (
            ("pull_request", "labeled", "apply-benchmark-patch", True, False),
            ("pull_request", "unlabeled", "apply-benchmark-patch", True, False),
            ("pull_request", "labeled", "documentation", True, False),
            ("pull_request", "labeled", "run-benchmarks", True, True),
            ("pull_request", "unlabeled", "run-benchmarks", False, False),
            ("pull_request", "synchronize", None, True, True),
            ("pull_request", "opened", None, False, False),
            ("workflow_dispatch", None, None, False, True),
            ("schedule", None, None, False, True),
        )
        with tempfile.TemporaryDirectory(prefix="benchmark-gate-test-") as directory:
            root = Path(directory)
            event_path, output = root / "event.json", root / "output.txt"
            for event_name, action, label, opted_in, expected in cases:
                with self.subTest(event=event_name, action=action, label=label):
                    event_path.write_text(
                        json.dumps(
                            {
                                "action": action,
                                "label": {"name": label},
                                "pull_request": {
                                    "labels": [{"name": "run-benchmarks"}]
                                    if opted_in
                                    else []
                                },
                            }
                        )
                    )
                    output.write_text("")
                    result = subprocess.run(
                        ["bash", "-c", script],
                        env={
                            **os.environ,
                            "GITHUB_EVENT_NAME": event_name,
                            "GITHUB_EVENT_PATH": str(event_path),
                            "GITHUB_OUTPUT": str(output),
                        },
                        capture_output=True,
                        text=True,
                        timeout=5,
                    )
                    self.assertEqual(
                        result.returncode, 0, result.stdout + result.stderr
                    )
                    self.assertEqual(
                        output.read_text(), f"run={str(expected).lower()}\n"
                    )


class ProducerConsumerRoundTripTest(unittest.TestCase):
    def test_selected_weight_patch_round_trips_without_touching_other_work(self):
        # Load the existing isolated producer harness without importing its
        # TestCase into this module, which would rediscover its entire suite.
        spec = importlib.util.spec_from_file_location(
            "benchmark_producer_fixture", SCRIPTS / "tests/test_benchmark_action.py"
        )
        producer = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(producer)
        fixture = producer.BenchmarkArtifactsTest()
        fixture.setUp()
        self.addCleanup(fixture.doCleanups)

        relative = "pallets/hashed-accounts/src/weights.rs"
        weights = fixture.root / relative
        original = weights.read_text()
        sha = fixture.git("rev-parse", "HEAD").decode().strip()
        result = fixture.run_benchmarks(
            PALLET_DIRS="hashed-accounts",
            EXTRINSICS="authorize",
            GITHUB_RUN_ID="200",
            GITHUB_RUN_ATTEMPT="1",
        )
        self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
        fixture.assert_selection({"pallet_hashed_accounts": {"authorize"}})
        measured = weights.read_text()

        archive_path = fixture.root / "bench-patch.tgz"
        with tarfile.open(archive_path, "w:gz") as archive:
            archive.add(fixture.root / ".bench_patch", arcname=".bench_patch")
        summary, patch = apply_patch.read_patch_archive(
            archive_path, run_fixture(head_sha=sha), sha
        )
        self.assertIn("pallet_hashed_accounts: authorize\n", summary)
        self.assertNotIn("authorize_ed25519", summary)
        self.assertIsNotNone(patch)

        # Recreate the clean source file that the apply workflow checks out,
        # preserving the fixture's independently staged user.txt change.
        weights.write_bytes(fixture.git("show", f"HEAD:{relative}"))
        self.assertEqual(weights.read_text(), original)
        fixture.git("update-index", "--refresh")
        patch_path = fixture.root / "validated.diff"
        patch_path.write_bytes(patch)
        applied = subprocess.run(
            ["git", "apply", "--index", "--3way", str(patch_path)],
            cwd=fixture.root,
            capture_output=True,
            text=True,
            timeout=5,
        )
        self.assertEqual(applied.returncode, 0, applied.stdout + applied.stderr)

        self.assertEqual(weights.read_text(), measured)
        self.assertNotEqual(measured, original)
        self.assertIn(producer.MEASURED_BODY, measured)
        self.assertEqual(
            [line for line in measured.splitlines() if "fn authorize_ed25519(" in line],
            [line for line in original.splitlines() if "fn authorize_ed25519(" in line],
        )
        self.assertEqual(
            fixture.git("diff", "--cached", "--binary", "--", "user.txt"),
            fixture.index_before,
        )
        self.assertEqual(
            fixture.git("diff", "--cached", "--name-only").decode().splitlines(),
            [relative, "user.txt"],
        )


if __name__ == "__main__":
    unittest.main()
