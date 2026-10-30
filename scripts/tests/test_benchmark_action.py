"""Exercise artifact survival using fake benchmarks; never measure runtime weights."""

import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest


SCRIPTS = Path(__file__).resolve().parents[1]
# Deliberately synthetic values, used only in temporary fixture repositories.
WEIGHTS = """pub trait WeightInfo {
    fn authorize(n: u32) -> Weight;
    fn authorize_ed25519(n: u32) -> Weight;
}
impl<T: Config> WeightInfo for SubstrateWeight<T> {
    fn authorize(n: u32) -> Weight { BODY }
    fn authorize_ed25519(n: u32) -> Weight { BODY }
}
impl WeightInfo for () {
    fn authorize(n: u32) -> Weight { BODY }
    fn authorize_ed25519(n: u32) -> Weight { BODY }
}
"""
MEASURED = WEIGHTS.replace(
    "BODY",
    "Weight::from_parts(1000, 20).saturating_add(Weight::from_parts(123, 0).saturating_mul(n.into()))",
)


class BenchmarkArtifactsTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="benchmark-artifact-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        for name in ("benchmark_action.sh", "benchmark_action.py"):
            dest = self.root / "scripts" / name
            dest.parent.mkdir(exist_ok=True)
            shutil.copyfile(SCRIPTS / name, dest)
        self.executable(
            "scripts/discover_pallets.sh",
            "#!/bin/sh\nprintf '%s\\n' 'pallet_subtensor pallets/subtensor/src/weights.rs' "
            "'pallet_hashed_accounts pallets/hashed-accounts/src/weights.rs'\n",
        )
        for pallet in ("subtensor", "hashed-accounts"):
            self.write(
                f"pallets/{pallet}/src/weights.rs",
                WEIGHTS.replace("BODY", "Weight::MAX"),
            )
        self.write("fixture.rs", MEASURED)
        self.write(
            "target/production/wbuild/node-subtensor-runtime/node_subtensor_runtime.compact.compressed.wasm",
            "fixture",
        )
        self.executable("bin/cargo", "#!/bin/sh\nexit 0\n")
        self.executable(
            "target/production/node-subtensor",
            f"#!{sys.executable}\n"
            + """
import os, pathlib, sys, time
args = dict(arg[2:].split('=', 1) for arg in sys.argv if arg.startswith('--') and '=' in arg)
if args['pallet'] == 'pallet_subtensor':
    pathlib.Path('second-started').touch()
    mode = os.environ.get('SECOND_MODE', 'success')
    if mode == 'fail':
        sys.exit(1)
    if mode == 'interrupt':
        while True:
            time.sleep(1)
pathlib.Path(args['output']).write_text(pathlib.Path('fixture.rs').read_text())
""",
        )
        self.executable(
            "target/production/weight-compare",
            "#!/bin/sh\n"
            + """
if [ "${NO_DRIFT:-}" = 1 ]; then exit 0; fi
printf '%s\n' ' ❌ authorize weight changed' ' ❌ authorize_ed25519 weight changed'
exit 2
""",
        )
        self.git("init", "-q")
        self.write("user.txt", "original\n")
        self.git("add", ".")
        self.git(
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "fixture",
        )
        self.write("user.txt", "user-staged-change\n")
        self.git("add", "user.txt")
        self.index_before = self.git("diff", "--cached", "--binary")
        self.env = {
            **os.environ,
            "PATH": f"{self.root / 'bin'}:{os.environ['PATH']}",
            "GITHUB_EVENT_NAME": "pull_request",
            "PALLET_DIRS": "",
            "EXTRINSICS": "*",
        }
        self.env.pop("GIT_INDEX_FILE", None)

    def write(self, name, content):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)
        return path

    def executable(self, name, content):
        self.write(name, content).chmod(0o755)

    def git(self, *args):
        return subprocess.check_output(
            ["git", *args], cwd=self.root, stderr=subprocess.PIPE
        )

    def run_benchmarks(self, **env):
        return subprocess.run(
            ["bash", "scripts/benchmark_action.sh"],
            cwd=self.root,
            env={**self.env, **env},
            capture_output=True,
            text=True,
            timeout=15,
        )

    def assert_saved(self, status):
        artifact = self.root / ".bench_patch"
        self.assertEqual(
            (artifact / "generated/pallet_hashed_accounts.rs").read_text(), MEASURED
        )
        patch = (artifact / "benchmark_patch.diff").read_text()
        self.assertIn("authorize_ed25519", patch)
        self.assertIn("Weight::from_parts(123, 0)", patch)
        self.assertNotIn("user.txt", patch)
        self.assertIn(f"Status: {status}", (artifact / "summary.txt").read_text())
        self.assertEqual(self.index_before, self.git("diff", "--cached", "--binary"))
        self.git(
            "apply", "--reverse", "--check", str(artifact / "benchmark_patch.diff")
        )

    def test_timeout_preserves_completed_coefficients_and_patch(self):
        with (self.root / "run.log").open("w") as log:
            proc = subprocess.Popen(
                ["bash", "scripts/benchmark_action.sh"],
                cwd=self.root,
                env={**self.env, "SECOND_MODE": "interrupt"},
                stdout=log,
                stderr=log,
                start_new_session=True,
            )
            try:
                deadline = time.monotonic() + 10
                while not (self.root / "second-started").exists():
                    self.assertIsNone(
                        proc.poll(), "Benchmark script exited before the second pallet"
                    )
                    self.assertLess(time.monotonic(), deadline)
                    time.sleep(0.02)
                # An abrupt kill cannot run an EXIT trap: the checkpoint must
                # already exist before the long-running pallet finishes.
                self.assert_saved("INCOMPLETE")
            finally:
                os.killpg(proc.pid, signal.SIGKILL)
                proc.wait(timeout=5)
        self.assert_saved("INCOMPLETE")

    def test_later_failure_keeps_patch_and_reports_failure(self):
        result = self.run_benchmarks(SECOND_MODE="fail")
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assert_saved("FAILED")

    def test_success_emits_patch_and_expected_drift_exit(self):
        result = self.run_benchmarks()
        self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
        self.assert_saved("COMPLETE")

    def test_no_drift_still_retains_full_reference_output(self):
        result = self.run_benchmarks(NO_DRIFT="1")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(
            (
                self.root / ".bench_patch/generated/pallet_hashed_accounts.rs"
            ).read_text(),
            MEASURED,
        )
        self.assertFalse((self.root / ".bench_patch/benchmark_patch.diff").exists())
        self.assertEqual(self.index_before, self.git("diff", "--cached", "--binary"))

    def test_manual_pallet_selection_overrides_branch_manifest(self):
        self.write(
            ".maintain/benchmark-focus.json",
            '{"pallet_dirs":["subtensor"],"extrinsics":["unrelated"]}',
        )
        result = self.run_benchmarks(
            GITHUB_EVENT_NAME="workflow_dispatch",
            GITHUB_REF_NAME="codex/hashed-accounts",
            PALLET_DIRS="hashed-accounts",
        )
        self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
        self.assert_saved("COMPLETE")
        self.assertFalse((self.root / "second-started").exists())

    def test_unknown_pallet_is_an_error(self):
        result = self.run_benchmarks(PALLET_DIRS="hashed-account-typo")
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("no registered benchmarks", result.stderr + result.stdout)


if __name__ == "__main__":
    unittest.main()
