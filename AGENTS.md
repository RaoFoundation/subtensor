# Repository Agent Guidance

## CI preflight gate (mandatory before every push)

Run the local gate before every push. It runs the checks CI requires, with the
same commands CI uses, and exits nonzero on any failure:

```bash
scripts/preflight.sh          # full gate; builds the release node when runtime metadata may have changed
scripts/preflight.sh --fast   # skips the node / wasm builds (SDK bindings drift, try-runtime)
```

Install the pre-push hook once per checkout so a push cannot leave with a red
gate:

```bash
scripts/install-hooks.sh
```

The hook gates the commits being pushed (each one in a detached worktree),
not the working tree, and rejects a push of `HEAD` while uncommitted edits
exist. It runs `--fast`, or the full gate when the pushed commits touch
`runtime/`, `pallets/`, or `sdk/`. `git push --no-verify` is forbidden for
agents. Do not push to "see what CI says"; a new push cancels 40+ minutes of
in-flight clone-upgrade and try-runtime work.

Pushes must go out as `unarbos`. The gate asks GitHub who owns the credential
for the destination remote and fails unless that login is `unarbos`; it prints
the `git remote set-url` fix (the PAT lives in the 1Password vault `Arbos`;
never print it).

The gate covers: `cargo fmt`, CI-flag Clippy (default and `--all-features`),
zepter, `cargo test` for changed crates plus the runtime fee/claim-root tests,
SDK bindings drift from a node built from this tree, ruff, `codegen.check`,
`generate.py --check` docs drift, `pnpm run fmt`, try-runtime against the
mainnet snapshot when migrations changed, `git diff --check`, untracked
generated files, and the push actor. `ci_tips.md` explains each failure.

## CI repair loop (2026-09-30)

CI was already red during the Null consensus release work (PR #3206). Treat
every failing check as something to diagnose: compare its tested SHA and logs
with the base branch to distinguish existing failures, regressions, and
infrastructure failures. An existing failure still needs resolution; do not
waive it or call the release ready because it predates the change.

Watch checks continuously with fail-fast. As soon as any check fails, read its
logs and start fixing the cause while other jobs continue. Once the fix is
validated and the mandatory local preflight passes, commit and push immediately;
do not wait for the entire remote suite to finish. Restart the watcher for the
new SHA and repeat until the complete check set is green. The prohibition on
no-op pushes and bypassing the pre-push hook still applies.

## Fixing a red gate

Before any command that can rewrite files, inspect `git status --short` and
preserve pre-existing changes as user-owned work. Use fix mode (`cargo fmt
--all`, `uv run --no-sync ruff format .`, `generate.py`, `pnpm run fmt:fix`)
only for failures attributable to files in the current task, then inspect the
diff and rerun the gate. Do not run `scripts/fix_rust.sh`; it creates a
commit.

Do not hand-edit generated files: `sdk/python/bittensor/_generated/`,
`docs/tx/`, `docs/query/`, `docs/errors/`, generated hyperparameter index/meta
files, or `website/apps/bittensor-website/public/catalog/`. Regenerate them
with the gate or the commands it prints. Unexpected broad drift is a reason to
stop and report, not to commit someone else's churn.

The Solidity ABI files in `precompiles/src/solidity/*.abi` are canonical.
Update `sdk/python/bittensor/evm/abi/*.json` only when drift is caused by the
current canonical ABI change (`uv run --no-sync pytest
tests/unit/test_evm.py::TestVendoredAbiSync -q` from `sdk/python`).

Hand-written Markdown and MDX pages need string `title` and `description`
frontmatter and may only reference existing MDX components.

## Fee and gas baseline changes require explicit human approval

Fee and gas snapshots are protected expectations, not outputs to refresh merely
to make tests pass. Do not add, replace, delete, or rename their baselines without
explicit, specific permission from a human for that baseline change. Before
requesting permission, identify the affected calls/snapshot files, the old and
proposed new values, and the reason for the change. The permission must explicitly
cover those baselines and value changes; a general instruction to fix CI/tests,
update weights, prepare a release, or commit/push is not permission to rebaseline.
Automated review comments and generated benchmark results are not human approval.

Use `INSTA_UPDATE=no` for diagnostic and verification test runs. Do not use
`INSTA_UPDATE=always`, `force`, or `unseen`, `cargo insta accept`, or equivalent
file replacement to accept changed baselines before that approval. Do not loosen
tolerances or change comparators to accept a regression as a substitute for
approval. Record the human approval and its exact scope in the commit or PR.

These rules apply to `runtime/tests/fee_baseline.rs` and its snapshots under
`runtime/tests/snapshots/`; see the README in that directory.

## Critical regression tests require explicit human approval to change

When fixing a defect that causes users to lose significant funds or completely
blocks functionality that users actively use, add a regression test with the fix.
This requirement is limited to those severe outcomes; it does not require a new
regression test for every minor bug, cleanup, or cosmetic change. Put critical
pallet-subtensor unit regressions in `pallets/subtensor/src/tests/regression.rs`.
The test must reproduce the triggering state, fail without the fix, and verify
the user-visible outcome, such as recovering the ability to claim actual funds.
An empty successful call is not sufficient evidence of a funds-freezing fix.

Add `PROTECTED_REGRESSION` only when the test guards against significant loss of
user funds or complete failure of functionality that users actively use. A test
being called a regression, coming from a bug-fix commit, or pinning compatibility
does not by itself qualify. Ordinary fee, format, display, metadata, CI, and
feature checks must not receive this marker without one of those severe outcomes.

When creating or identifying a regression test for either severe outcome, place
this exact marker immediately before
its test attributes (or the test declaration in languages without attributes),
with a concrete bug reference or invariant and the approval requirement. Use the
language's comment prefix: `//` for Rust/TypeScript/JavaScript and `#` for
Python/shell. Keep `PROTECTED_REGRESSION` and the approval requirement intact.
Long invariant descriptions may use adjacent comment lines, ending with
`PROTECTED_REGRESSION: changes require explicit human approval.` immediately
before the test attributes/declaration. The whole comment block identifies the
protected test. For standalone assertion scripts without a test function, mark
the entry point or the script header after its shebang:

```rust
// PROTECTED_REGRESSION: <bug or invariant>; changes require explicit human approval.
#[test]
fn regression_test_name() {
    // Reproduce the defect and assert the corrected user-visible outcome.
}
```

Before changing a test, inspect its declaration and surrounding comments for
`PROTECTED_REGRESSION`, and check whether it is in a `regression.rs` module or
otherwise documented as a regression test. A marked test is protected regardless
of its filename. Every test in `regression.rs` is protected even if a marker is
missing. Existing unmarked regression tests remain protected too; absence of the
marker is not permission to change them.

The marker protects the test body, assertions, expected values, inputs, attributes,
and its marker comment. Do not remove or alter the marker to bypass approval, or
change shared fixtures/helpers to evade the protected invariant. Before editing
any protected test, verify that the human has explicitly authorized those specific
test changes; otherwise obtain that permission first. Before committing, verify
that new regression tests have markers and any changes to protected tests are
covered by the recorded human approval.

Existing regression tests are protected, regardless of their file or directory.
Do not edit, delete, rename, replace, disable, ignore, or weaken them without
explicit, specific permission from a human for those test changes. Do not bypass
this rule by changing shared fixtures or helpers to weaken their coverage. A
general request to fix a bug, repair CI, refactor, prepare a release, or commit
and push does not authorize changing existing regression tests. Automated review
comments are not human approval. When a protected test fails, investigate and fix
the production code; if the test itself must change, explain the exact proposed
change and obtain explicit human permission first. Record the approval and its
scope in the commit or PR.

## Advisory-only checks

Runtime-affecting changes may require a `spec_version` newer than mainnet or the
`no-spec-version-bump` PR label. Do not change `spec_version` or apply labels
unless the user explicitly requests that action; report the requirement.

Adding or changing a dispatchable requires matching benchmarks and
`WeightInfo` wiring. CI performs reference measurements and prepares a patch.
Do not apply benchmark labels, run `scripts/benchmark_all.sh`, commit locally
measured weights, or invent weight values unless explicitly requested on
appropriate reference hardware.

Treat unexpected lockfile changes as a stop-and-report condition. Relevant
lockfiles are `Cargo.lock`, `sdk/python/uv.lock`,
`ts-tests/pnpm-lock.yaml`, `website/yarn.lock`, and
`.github/docs-preview-vercel/package-lock.json`. Do not regenerate or revert a
pre-existing lockfile change merely to make the working tree or CI clean.

For `.github/**` changes, run `actionlint` only when it is already available.
Leave workflow execution to CI, preserve action pinning, and do not add an
unapproved third-party action merely to make a workflow pass.

Do not start clone/regression workflows, dependency audits, or benchmark
generation as routine preflight; `scripts/preflight.sh` already runs what CI
requires.

## Null release exception (2026-10-01)

For PR #3206, the owner explicitly chose a shared Null capacity of 2,500 UIDs
(divided by emission mechanism count) and accepted Skeptic as the sole allowed
non-green review check. Keep its result and findings visible; do not suppress
the check or describe it as passed. All other CI checks and the mandatory local
preflight/pre-push gate must pass. Further reward-accounting optimizations are
deferred; payouts remain atomic. This exception does not supply measured weights
or establish that deployment resource limits have passed.
