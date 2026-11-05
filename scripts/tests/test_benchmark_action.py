"""Exercise benchmark selection and artifact survival without measuring weights."""

import json
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
MEASURED_BODY = (
    "Weight::from_parts(1000, 20).saturating_add("
    "Weight::from_parts(123, 0).saturating_mul(n.into()))"
)
MEASURED = WEIGHTS.replace("BODY", MEASURED_BODY)
PALLET_METHODS = {
    "pallet_hashed_accounts": {"authorize", "authorize_ed25519"},
    "pallet_subtensor": {"authorize", "add_stake"},
}


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
            weights = WEIGHTS
            if pallet == "subtensor":
                weights = weights.replace("authorize_ed25519", "add_stake")
            self.write(
                f"pallets/{pallet}/src/weights.rs",
                weights.replace("BODY", "Weight::MAX"),
            )
            self.write(
                f"fixtures/pallet_{pallet.replace('-', '_')}.rs",
                weights.replace("BODY", MEASURED_BODY),
            )
        self.write(
            "target/production/wbuild/node-subtensor-runtime/node_subtensor_runtime.compact.compressed.wasm",
            "fixture",
        )
        self.executable("bin/cargo", "#!/bin/sh\nexit 0\n")
        self.executable(
            "target/production/node-subtensor",
            f"#!{sys.executable}\n"
            + """
import json, os, pathlib, re, sys, time
args = dict(arg[2:].split('=', 1) for arg in sys.argv if arg.startswith('--') and '=' in arg)
with pathlib.Path('calls.jsonl').open('a') as log:
    log.write(json.dumps(sys.argv[1:]) + '\\n')
if 'list' in args:
    for pallet in ('pallet_hashed_accounts', 'pallet_subtensor'):
        source = pathlib.Path(f'fixtures/{pallet}.rs').read_text()
        for method in sorted(set(re.findall(r'fn (\\w+)\\(', source))):
            print(f'{pallet},{method}')
    sys.exit(0)
if args['pallet'] == 'pallet_subtensor':
    pathlib.Path('second-started').touch()
    mode = os.environ.get('SECOND_MODE', 'success')
    if mode == 'fail':
        sys.exit(1)
    if mode == 'interrupt':
        while True:
            time.sleep(1)
source = pathlib.Path(f"fixtures/{args['pallet']}.rs").read_text()
selected = set(args['extrinsic'].split(','))
lines = []
in_unit_impl = False
for line in source.splitlines(keepends=True):
    if line.startswith('impl WeightInfo for ()'):
        in_unit_impl = True
    if in_unit_impl and os.environ.get('GENERATED_NO_UNIT') == '1':
        continue
    method = re.search(r'fn (\\w+)\\(', line)
    if method:
        name = method.group(1)
        if '*' not in selected and name not in selected:
            continue
        if name == os.environ.get('GENERATED_OMIT'):
            continue
        if in_unit_impl and name == os.environ.get('GENERATED_OMIT_UNIT'):
            continue
    lines.append(line)
pathlib.Path(args['output']).write_text(''.join(lines))
""",
        )
        self.executable(
            "target/production/weight-compare",
            f"#!{sys.executable}\n"
            + """
import os, pathlib, re, sys
if os.environ.get('NO_DRIFT') == '1':
    sys.exit(0)
args = dict(zip(sys.argv[1::2], sys.argv[2::2]))
def methods(path):
    found = {}
    for line in pathlib.Path(path).read_text().splitlines():
        match = re.search(r'fn (\\w+)\\(', line)
        if match:
            found.setdefault(match.group(1), []).append(line)
    return found
old, new = methods(args['--old']), methods(args['--new'])
if old.keys() != new.keys():
    print('incomplete weights reached comparison', file=sys.stderr)
    sys.exit(1)
drift = [name for name in old if old[name] != new[name]]
for name in drift:
    print(f' ❌ {name} weight changed')
sys.exit(2 if drift else 0)
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

    def run_benchmarks(self, *args, **env):
        return subprocess.run(
            ["bash", "scripts/benchmark_action.sh", *args],
            cwd=self.root,
            env={**self.env, **env},
            capture_output=True,
            text=True,
            timeout=15,
        )

    def invocations(self):
        path = self.root / "calls.jsonl"
        if not path.exists():
            return []
        return [json.loads(line) for line in path.read_text().splitlines()]

    def measurement_args(self):
        return [
            dict(
                arg[2:].split("=", 1)
                for arg in invocation
                if arg.startswith("--") and "=" in arg
            )
            for invocation in self.invocations()
            if "--list=all" not in invocation
        ]

    def assert_selection(self, expected):
        calls = self.invocations()
        listings = [args for args in calls if "--list=all" in args]
        self.assertEqual(len(listings), 1, calls)
        self.assertIn("--no-csv-header", listings[0])
        self.assertEqual(calls[0], listings[0])
        selections = {}
        for args in self.measurement_args():
            methods = set(args["extrinsic"].split(","))
            selections[args["pallet"]] = (
                PALLET_METHODS[args["pallet"]] if methods == {"*"} else methods
            )
        self.assertEqual(selections, expected)
        self.assertEqual(len(self.measurement_args()), len(expected))

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
        self.assertIn("hashed-account-typo", result.stderr + result.stdout)
        self.assertEqual(self.measurement_args(), [])

    def test_selection_preflight_needs_no_runtime_and_rejects_invalid_syntax(self):
        shutil.rmtree(self.root / "target")
        result = self.run_benchmarks(
            "--check-selection", PALLET_DIRS="hashed-accounts", EXTRINSICS="authorize"
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("pallet_hashed_accounts", result.stdout)
        self.assertNotIn("pallet_subtensor", result.stdout)
        result = self.run_benchmarks("--check-selection", EXTRINSICS="authorize*")
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertEqual(self.invocations(), [])

    def test_default_scope_is_full_for_every_event_even_with_focus_manifest(self):
        self.write(
            ".maintain/benchmark-focus.json",
            '{"pallet_dirs":["hashed-accounts"],"extrinsics":["authorize"]}',
        )
        for event in ("schedule", "pull_request", "workflow_dispatch"):
            with self.subTest(event=event):
                (self.root / "calls.jsonl").unlink(missing_ok=True)
                result = self.run_benchmarks(
                    GITHUB_EVENT_NAME=event,
                    GITHUB_REF_NAME="codex/targeted-benchmarks",
                    EXTRINSICS="",
                    NO_DRIFT="1",
                )
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assert_selection(PALLET_METHODS)

    def test_explicit_wildcards_run_all_methods_and_pallets(self):
        result = self.run_benchmarks(PALLET_DIRS=" * ", EXTRINSICS=" * ")
        self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
        self.assert_selection(PALLET_METHODS)

    def test_directory_and_runtime_aliases_accept_commas_spaces_and_duplicates(self):
        result = self.run_benchmarks(
            PALLET_DIRS="hashed-accounts, pallet_subtensor pallet_hashed_accounts",
            EXTRINSICS="authorize, authorize_ed25519 authorize",
        )
        self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
        self.assert_selection(
            {
                "pallet_hashed_accounts": {"authorize", "authorize_ed25519"},
                "pallet_subtensor": {"authorize"},
            }
        )

    def test_single_extrinsic_merges_only_requested_method_and_keeps_raw_artifact(self):
        result = self.run_benchmarks(
            PALLET_DIRS="pallet_hashed_accounts", EXTRINSICS="authorize"
        )
        self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
        self.assert_selection({"pallet_hashed_accounts": {"authorize"}})
        actual = (self.root / "pallets/hashed-accounts/src/weights.rs").read_text()
        expected = "".join(
            line.replace("BODY", "Weight::MAX")
            if "fn authorize_ed25519(" in line else line
            for line in WEIGHTS.splitlines(keepends=True)
        ).replace("BODY", MEASURED_BODY)
        self.assertEqual(actual, expected)
        self.assertEqual(actual.count("fn authorize_ed25519("), 3)
        generated = (
            self.root / ".bench_patch/generated/pallet_hashed_accounts.rs"
        ).read_text()
        self.assertIn("fn authorize(", generated)
        self.assertNotIn("fn authorize_ed25519(", generated)
        summary = (self.root / ".bench_patch/summary.txt").read_text()
        self.assertIn("pallet_hashed_accounts", summary)
        self.assertIn("authorize", summary)
        self.assertNotIn("authorize_ed25519", summary)
        self.assertEqual(self.index_before, self.git("diff", "--cached", "--binary"))
        self.git(
            "apply", "--reverse", "--check",
            str(self.root / ".bench_patch/benchmark_patch.diff"),
        )

    def test_multiple_requested_methods_run_their_per_pallet_intersections(self):
        result = self.run_benchmarks(EXTRINSICS="authorize_ed25519, add_stake")
        self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
        self.assert_selection(
            {
                "pallet_hashed_accounts": {"authorize_ed25519"},
                "pallet_subtensor": {"add_stake"},
            }
        )

    def test_extrinsic_only_selection_skips_pallets_without_matches(self):
        result = self.run_benchmarks(EXTRINSICS="authorize_ed25519")
        self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
        self.assert_selection({"pallet_hashed_accounts": {"authorize_ed25519"}})
        self.assertFalse((self.root / "second-started").exists())

    def test_unknown_pallet_in_mixed_selection_fails_before_measurement(self):
        result = self.run_benchmarks(PALLET_DIRS="hashed-accounts,hashed-account-typo")
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("hashed-account-typo", result.stdout + result.stderr)
        self.assertEqual(self.measurement_args(), [])

    def test_unknown_extrinsic_in_mixed_selection_fails_before_measurement(self):
        result = self.run_benchmarks(EXTRINSICS="authorize,authorize_typo")
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("authorize_typo", result.stdout + result.stderr)
        self.assertEqual(self.measurement_args(), [])

    def test_invalid_extrinsic_clears_artifacts_from_previous_run(self):
        stale_files = (
            ".bench_patch/generated/stale.rs",
            ".bench_patch/summary.txt",
            ".bench_patch/benchmark_patch.diff",
        )
        for path in stale_files:
            self.write(path, "previous run\n")
        result = self.run_benchmarks(
            PALLET_DIRS="hashed-accounts", EXTRINSICS="authorize_typo"
        )
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertEqual(self.measurement_args(), [])
        for path in stale_files:
            self.assertFalse((self.root / path).exists(), path)

    def test_extrinsic_outside_selected_pallet_is_an_error(self):
        result = self.run_benchmarks(
            PALLET_DIRS="hashed-accounts", EXTRINSICS="add_stake"
        )
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("add_stake", result.stdout + result.stderr)
        self.assertEqual(self.measurement_args(), [])

    def test_globs_and_mixed_wildcards_are_rejected_before_measurement(self):
        for selectors in (
            {"PALLET_DIRS": "hashed-*"},
            {"PALLET_DIRS": "*,hashed-accounts"},
            {"EXTRINSICS": "authorize*"},
            {"EXTRINSICS": "*,authorize"},
        ):
            with self.subTest(selectors=selectors):
                (self.root / "calls.jsonl").unlink(missing_ok=True)
                result = self.run_benchmarks(**selectors)
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertEqual(self.measurement_args(), [])

    def test_unchanged_selected_method_succeeds_without_patch(self):
        self.write("pallets/hashed-accounts/src/weights.rs", MEASURED)
        result = self.run_benchmarks(
            PALLET_DIRS="hashed-accounts", EXTRINSICS="authorize"
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assert_selection({"pallet_hashed_accounts": {"authorize"}})
        self.assertFalse((self.root / ".bench_patch/benchmark_patch.diff").exists())
        self.assertEqual(
            (self.root / "pallets/hashed-accounts/src/weights.rs").read_text(), MEASURED
        )
        self.assertEqual(self.index_before, self.git("diff", "--cached", "--binary"))

    def test_selected_new_method_replaces_fallback_or_adds_all_sections(self):
        base = WEIGHTS.replace("BODY", "Weight::MAX")
        declaration = "    fn authorize_new(n: u32) -> Weight;\n"
        implementation = (
            f"    fn authorize_new(n: u32) -> Weight {{ {MEASURED_BODY} }}\n"
        )
        fallback = (
            "    fn authorize_new(n: u32) -> Weight { Self::authorize(n) }\n"
        )
        trait_anchor = "    fn authorize_ed25519(n: u32) -> Weight;\n"
        impl_anchor = (
            "    fn authorize_ed25519(n: u32) -> Weight { Weight::MAX }\n"
        )
        expected = base.replace(trait_anchor, trait_anchor + "\n" + declaration).replace(
            impl_anchor, impl_anchor + "\n" + implementation
        )
        generated = self.write(
            "selected.rs",
            "pub trait WeightInfo {\n"
            + declaration
            + "}\nimpl<T: Config> WeightInfo for SubstrateWeight<T> {\n"
            + implementation
            + "}\nimpl WeightInfo for () {\n"
            + implementation
            + "}\n",
        )
        for has_fallback in (True, False):
            with self.subTest(has_fallback=has_fallback):
                original = (
                    base.replace(trait_anchor, trait_anchor + fallback)
                    if has_fallback else base
                )
                committed = self.write(
                    "pallets/hashed-accounts/src/weights.rs", original
                )
                merged = self.write("merged.rs", original)
                for command, destination, source in (
                    ("merge-measured", merged, generated),
                    ("selective-patch", committed, merged),
                ):
                    result = subprocess.run(
                        [
                            sys.executable, "scripts/benchmark_action.py", command,
                            str(destination), str(source), "authorize_new",
                        ],
                        cwd=self.root,
                        capture_output=True,
                        text=True,
                        timeout=15,
                    )
                    self.assertEqual(
                        result.returncode, 0, result.stdout + result.stderr
                    )
                    self.assertEqual(destination.read_text(), expected)
                self.assertEqual(
                    self.index_before, self.git("diff", "--cached", "--binary")
                )

    def test_incomplete_generated_weights_are_rejected_without_changing_source(self):
        original = (self.root / "pallets/hashed-accounts/src/weights.rs").read_text()
        for omissions in (
            {"GENERATED_OMIT": "authorize_ed25519"},
            {"GENERATED_OMIT_UNIT": "authorize_ed25519"},
            {"GENERATED_NO_UNIT": "1"},
        ):
            with self.subTest(omissions=omissions):
                result = self.run_benchmarks(
                    PALLET_DIRS="hashed-accounts",
                    EXTRINSICS="authorize,authorize_ed25519",
                    **omissions,
                )
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertEqual(
                    (self.root / "pallets/hashed-accounts/src/weights.rs").read_text(),
                    original,
                )
                self.assertFalse((self.root / ".bench_patch/benchmark_patch.diff").exists())
                self.assertTrue(
                    (self.root / ".bench_patch/generated/pallet_hashed_accounts.rs").exists()
                )
                self.assertEqual(self.index_before, self.git("diff", "--cached", "--binary"))


if __name__ == "__main__":
    unittest.main()
