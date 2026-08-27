# Make CI green fast

This is a playbook. Use it when a pull request is red and you want the next
push to pass. It comes from current workflow files and from many CI-fix
sessions in this repo.

**Source of truth for status:** `gh pr checks`, not `gh run list`.
`gh run list` misses required checks that are not GitHub Actions jobs.

Always finish a local pass with:

```bash
git diff --check
git status --short
```

Do not run `scripts/fix_rust.sh` during a normal CI loop. That script
creates commits.

---

## 1. The 10-minute loop

Do this in order. Stop when the remaining red jobs are slow or infra.

1. Confirm GitHub is testing the commit you think it is.
   Local fmt/clippy that you did not push does not count.
2. Run the fast local gates that match the files you changed.
3. Push those fixes as one commit.
4. Watch with fail-fast. Fix the next cheap failure. Repeat.
5. Rerun only when the log is infra (429, GHCR pull/push, warp-proof,
   cancelled sibling).
6. Do not push again just to "unstick" CI. A new push cancels
   in-flight Runtime Checks. Clone-upgrade and try-runtime then
   restart from zero (often 40+ minutes).
7. Leave Clippy workspace runs, full `cargo test`, clone-upgrade, and
   e2e to CI unless you already know they are the failure.

```bash
gh pr view --json number,url,headRefName,headRefOid
gh pr checks --json name,bucket,state,workflow,link
gh pr checks --watch --fail-fast
# After a failure:
gh run view <run-id> --log-failed
```

`--fail-fast` stops on the first failure. Other jobs may still be green
or still running. Re-read the full check set after every push. The set
can change.

`gh pr checks` is tab-separated. Names have spaces. To list real
failures and hide a waived skeptic:

```bash
gh pr checks <N> | awk -F'\t' '$2=="fail" && $1!="skeptic"'
```

---

## 2. What "required" means here

A **required check** is a GitHub branch-protection name. If that name is
missing or red, merge stays blocked.

A **skipped** job usually counts as passing. That is why path filters and
labels exist. Do not add a new required check name without a skip-success
path, or docs-only PRs get stuck on "Expected".

These names matter today:

| Check name | Workflow | Usual local match |
|---|---|---|
| `cargo fmt` | Check Rust | `cargo fmt --check --all` |
| `cargo clippy (default)` | Check Rust | `SKIP_WASM_BUILD=1 cargo clippy --workspace --all-targets -- -D warnings` |
| `cargo clippy (all)` | Check Rust | same + `--all-features` |
| `cargo test` | Check Rust | leave to CI |
| `bittensor-core wasm32 seam` | Check Rust | Linux/wasm; flake on GitHub 429 |
| `zepter feature propagation` | Check Rust | `zepter run check` |
| `SDK offline checks` | Runtime Checks | `cd sdk/python && just check` (almost) |
| `Docs and website build` | Runtime Checks | `generate.py --check` then website build |
| `spec_version newer than mainnet` | Runtime Checks | bump or label `no-spec-version-bump` |
| `Sudo-upgrade mainnet clone and test` | Runtime Checks | clone-upgrade fan-in; see below |
| `cargo test (eco-tests)` | Eco Tests | `cd eco-tests && cargo test` |
| `typescript-formatting` | Typescript E2E | `cd ts-tests && pnpm run fmt` |
| `typescript-e2e-zombienet_evm` | Typescript E2E | gate name; skip when EVM not selected |
| `typescript-e2e-zombienet_shield` | Typescript E2E | gate name; skip when shield not selected |
| `skeptic` / `auditor` | ai-review | required; often not a compile bug |

`cargo fix leaves no diff` and `no cargo check warnings` run on main /
manual, not on pull requests.

---

## 3. Fast local gates (do these before push)

Match the changed files. Do not run the whole matrix.

### Rust source, manifests, fixtures

```bash
cargo fmt --check --all
```

If it fails only on files you own:

```bash
cargo fmt --all
```

Then inspect the diff. Typical hit: import order. rustfmt wants
`use frame_support::storage::{TransactionOutcome, with_transaction}`
on one line, or `use super::*;` above other imports.

`just clippy` is **not** the CI job. CI is:

```bash
SKIP_WASM_BUILD=1 cargo clippy --workspace --all-targets -- -D warnings
SKIP_WASM_BUILD=1 cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Root `just clippy` only denies `todo` and `unimplemented`. CI denies
**all** warnings. Unused imports fail CI and pass `just clippy`.

Do not run workspace Clippy as routine preflight. When CI is already
red on Clippy, run the two commands above.

`cargo clippy -p pallet-subtensor --lib` misses test-only denies.
A new test module usually needs the same crate-level allow as the
other pallet tests:

```rust
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
```

Do not `touch` a `.rs` file to "clear" a missing `use`. If Clippy
wants `WeightInfo` in scope, add the import.

### Python SDK

From `sdk/python`, when the locked env exists:

```bash
uv run --no-sync ruff check .
uv run --no-sync ruff format --check .
```

Use **`uv run ruff`**, not `ruff` on PATH. A system or stale `.venv`
binary will report lints (for example `UP045`) that CI's locked ruff
does not, or the reverse. CI pins ruff through `uv sync --python 3.14`.

Lint the **whole** SDK. Formatting one file (`query.py`) while CI
scans everything will bounce.

Fix only task-owned files:

```bash
uv run --no-sync ruff check . --fix
uv run --no-sync ruff format .
```

CI `SDK offline checks` also runs (from `sdk/python`):

```bash
uv sync --python 3.14 --locked --all-extras --dev
uv run ruff check .
uv run ruff format --check .
uv run ty check --exit-zero-on-warning bittensor
uv run pytest
uv run python -m codegen.check --coverage
uv run python -m codegen.check --names
uv run python scripts/export_beta_baselines_rs.py --check
```

`just check` in `sdk/python` also runs `--units` and `--namespaces`.
CI does not. A local `just check` failure on those two is not always
a CI failure.

### Generated docs

From `sdk/python`:

```bash
uv run --no-sync python ../../website/apps/bittensor-website/scripts/generate.py --check
```

If the drift is from this change:

```bash
uv run --no-sync python ../../website/apps/bittensor-website/scripts/generate.py
```

Repeat `--check`. Inspect the **exit code**. `--check` can print a
long file list and still exit 0. A list is not always a fail.

**Order that avoids the fmt/regen race:**

1. Finish Rust and Python source.
2. `cargo fmt --all`
3. `generate.py` then `generate.py --check`
4. Do not fmt again after regen. Line anchors in MDX and `reads.json`
   will move and the gate goes red again.

**Do not hand-edit** `docs/tx/`, `docs/query/`, `docs/errors/`, generated
hyperparameter pages, or `website/apps/bittensor-website/public/catalog/`.

If `--check` **fails** on tens of unrelated pages, stop. That is leftover
WIP, not your one-line change. Do not dump a 50-file regen into someone
else's PR.

MDX pages you do write by hand need string `title` and `description`
frontmatter. Missing those fail the website build after the drift gate.

### TypeScript tests

From `ts-tests`, when the locked env exists:

```bash
pnpm run fmt
```

Fix: `pnpm run fmt:fix`.

### EVM ABI

Canonical files: `precompiles/src/solidity/*.abi`.
Vendored copies: `sdk/python/bittensor/evm/abi/*.json`.

From `sdk/python`:

```bash
uv run --no-sync pytest tests/unit/test_evm.py::TestVendoredAbiSync -q
```

Update the vendored JSON only when the canonical ABI changed.

---

## 4. Failure cookbook

Each row is a failure we keep hitting. Symptom first. Then the fix.

### `cargo fmt`

**Symptom:** `Diff in path/to/file.rs`. Often import order or a long
iterator chain that rustfmt wants on one line.

**Fix:** `cargo fmt --all`. Commit only the rustfmt diff. Re-run
`cargo fmt --check --all`.

**Traps:**

- Do not edit `.rustfmt.toml`. Almost every option is commented on
  purpose. Turning options on reformats hundreds of pallet files.
- `vendor/frontier` prints `imports_granularity` warnings. Stable
  rustfmt ignores them. They do not fail the job.
- rustfmt can ICE and still exit 0. CI greps the output for
  `panicked at` / `internal compiler error`.
- `cargo fmt -p pallet-subtensor` is not enough. CI is `--all`.

### `cargo clippy (default)` / `cargo clippy (all)`

**Symptom:** unused import, `clippy::expect_used`, indexing, or a
compile error. `-D warnings` turns every warning into a hard fail.

**Fix:** smallest code change. Then run **both** Clippy jobs. Default
green does not mean `--all-features` is green.

**Traps:**

- One compile error in `pallet-subtensor` turns Clippy, `cargo test`,
  Docker, e2e node builds, and clone-upgrade red at once. Fix the
  compile first. Do not chase the cascade.
- `just clippy` will lie to you. Use the CI flags.
- Build-script `cargo:warning` lines (missing benchmarks,
  `freeze_struct`) are not Clippy denials. Ignore them.
- Pallet `--lib` Clippy misses test modules. Add the crate-level
  allow shown above, or rewrite the lint.

### `SDK offline checks`

**Symptom:** `ruff format` would reformat `query.py` (or similar);
`codegen.check --names` / `--coverage` fail; pytest fails.

**Fix:**

```bash
cd sdk/python
uv run ruff format .
uv run python -m codegen.check --coverage
uv run python -m codegen.check --names
# after a read-registry change:
uv run python -m codegen.emit_namespaces
```

**Traps:**

- Do not hand-edit `sdk/python/bittensor/_generated/`. Codegen headers
  have no trailing comma after `Spec version: N`. A hand edit of the
  catalog will fail clone-upgrade even if ruff is clean.
- Do not advertise storage that the runtime does not have. Reads for
  `TotalAlphaStaked` or leftover `DeferredRootAlphaDividends` failed
  when the chain had no such item.
- Do not regen from a local `--dev` / `pow-faucet` node. That node
  adds `SubtensorModule.faucet`. CI's production release binary does
  not. You will then fail `--coverage` (`raw-only entries no longer
  on chain: ['faucet']`) or the reverse.
- Do not use system `ruff`. Use `uv run ruff`.
- Do not let a local venv's Python leak into PyO3 / binding builds.
  See the pyo3 trap under `cargo test`.

### `Docs and website build`

**Symptom:** `Docs drift gate (generated tx/query/errors pages)` and
a list of stale files. Often `docs/tx/claim-root.mdx` or line anchors
in `reads.json` after a Rust edit that shifted source lines.

**Fix:** regenerate with `generate.py`, then `--check`. Commit the
generated pages.

**Traps:**

- Fmt then regen, or regen then fmt, can leave stale line anchors.
  Run generate **after** rustfmt is final. Do not fmt again.
- `--check` can print many paths and still exit 0. Read the header
  and the exit code.
- Broad unexpected drift that **fails**: stop and report. Do not
  "make CI green" by committing unrelated catalog churn.
- The website build (`yarn turbo run build --filter=@raofoundation/bittensor-website`)
  is a second step. Drift can be clean and the Next build still fail
  on missing MDX `title` / `description`.

### `spec_version newer than mainnet`

**Symptom:** local `spec_version` in `runtime/src/lib.rs` is less than
or equal to mainnet.

**Fix (only if the user asked):** bump to mainnet + 1, **or** add the
`no-spec-version-bump` label.

Check mainnet:

```bash
curl -sS -H 'Content-Type: application/json' \
  -d '{"id":1,"jsonrpc":"2.0","method":"state_getRuntimeVersion","params":[]}' \
  https://entrypoint-finney.opentensor.ai \
  | jq -r '.result.specVersion'
```

The file must contain exactly one `spec_version: <n>,` literal.

**Trap:** do not bump or label unless asked. A bump on a non-release
PR fights every other open PR. Two in-flight PRs must not claim the
same number. A missing bump on a release PR means the release train
builds and then no-ops at every deploy guard.

### `clone-upgrade (pristine|remaining|combined)` and
`Sudo-upgrade mainnet clone and test`

**What it is:** CI builds a Linux release node, sudo-upgrades a copy
of real mainnet state with your runtime, then runs clone regressions
and (when relevant) SDK metadata drift against that upgraded chain.

**Phases:**

- Snapshot available: `pristine` and `remaining` run in parallel.
- Live scrape or `fresh-mainnet-clone` label: one `combined` job.
- **`pristine` does not run the SDK drift gate.** It can pass while
  `_generated/` is stale. `remaining` (or the end of `combined`) is
  the metadata check.
- `remaining` / end of `combined` runs
  `uv run python -m codegen.check --drift ws://127.0.0.1:9944`
  when the path classifier sets `sdk_drift`.

The required name is **`Sudo-upgrade mainnet clone and test`**.
That is a fan-in job. If a sibling clone job is `failure`, this
gate dies in about 3 seconds with `CLONE: failure`. That is not a
second product bug. Open the inner `clone-upgrade (remaining)` log.

If clone is skipped because the **build** failed, the gate is red.
If clone is skipped because of `skip-clone-upgrade` or a docs-only
PR, the gate is green. GitHub treats **skipped** as a pass. That
label hides the check; it does not fix drift.

**Common real failures:**

1. **Generated SDK drift.** Committed `_generated/` does not match
   the upgraded **production** runtime. Regen from the CI Linux
   release node, not a Mac `--dev` node:

   ```bash
   gh run download <run-id> -n 'node-subtensor-release-<sha>' -D /tmp/release-node
   docker run -d --name st-meta --platform linux/amd64 -p 9944:9944 \
     -v /tmp/release-node:/node:ro ubuntu:24.04 \
     bash -c 'apt-get update -qq && apt-get install -y -qq ca-certificates >/dev/null && \
       exec /node/node-subtensor --chain local --tmp --alice --validator \
       --rpc-external --rpc-cors all --rpc-methods unsafe --rpc-port 9944 \
       --unsafe-force-node-key-generation'
   # Confirm specVersion == this PR before codegen. A stale
   # scripts/specs/local.json can still serve an OLD genesis.
   curl -sS -H 'Content-Type: application/json' \
     -d '{"id":1,"jsonrpc":"2.0","method":"state_getRuntimeVersion","params":[]}' \
     http://127.0.0.1:9944 | jq -r '.result.specVersion'
   cd sdk/python
   uv run python -m codegen ws://127.0.0.1:9944
   uv run python scripts/record_golden.py ws://127.0.0.1:9944
   uv run python -m codegen.check --drift ws://127.0.0.1:9944
   ```

   `--dev` is safe only if the binary is that CI release artifact
   (no `pow-faucet`). A local Mac `--dev` build is the `faucet` trap.

2. **Hand-edited `_generated/`.** Trailing commas, extra storage
   items, leftover `faucet`, or merge-conflict leftovers. Re-run
   codegen. Do not patch the generated files by eye.

3. **Regression assert.** Behavior on real mainnet state changed
   (locks conviction after `transferStake`, balancer, emission).
   Reproduce with `docs/internals/mainnet-clone.mdx`. Run the named
   script in `clones/js-tests` (`npm run test:locks-conviction`, …).
   A cancelled run is not a pass.

4. **Cancelled sibling / 3-second gate.** `--fail-fast`, a cheap
   job, or a new push cancelled clone-upgrade. Fix the cheap job
   or wait for the new HEAD. Do not debug clone.

5. **Warp-sync infra.** Log says `Bad warp proof` or
   `Downloading finality proofs, 0.00 MiB`. Nightly snapshot is
   stale and live scrape failed. Rerun `--failed` up to three
   times. Do not apply `skip-clone-upgrade` unless asked.

**Traps:**

- Do not regenerate `_generated/` as routine preflight. You need a
  node on the **new** spec. A Mac `SKIP_WASM_BUILD=1` tree cannot
  produce that node.
- Do not regen from `--dev` / `pow-faucet`. That adds `faucet`.
- Do not add `skip-clone-upgrade` to hide a runtime/SDK change.
- Apple clang on this machine often cannot target wasm32. Leave
  full runtime wasm and the release node to CI.

Local clone steps (slow; only when clone is the real failure):
see `docs/internals/mainnet-clone.mdx`.

### `try-runtime` (devnet / testnet / mainnet)

**What it is:** replay `on_runtime_upgrade` against a network state
snapshot. Cached snapshots are the default. Label
`fresh-try-runtime-state` to scrape live.

**When it fails:** a migration panics on real storage, or a new
migration has no `pre_upgrade` / `post_upgrade`. Unit tests will
not catch this. Put the migration in the runtime `Migrations`
tuple and add those hooks.

If **all three** networks go red together, look for a compile break
first (a vendored crate), not three migration bugs.

Local Mac cannot build the try-runtime wasm. Leave it to CI.

### `skeptic` / `auditor`

**What they are:** required AI review jobs. `skeptic` is read-only.
`auditor` may run `scripts/fix_rust.sh` and push `chore: auditor auto-fix`
on non-fork PRs.

**Read the verdict text**, not the red X. Findings live in the PR
comments and the `skeptic-output` artifact. The job log is often
empty.

```bash
gh api repos/RaoFoundation/subtensor/issues/<N>/comments
gh run download <run-id> --repo RaoFoundation/subtensor -n skeptic-output
```

**Recurring verdict:** `VULNERABLE` on weight accounting. The bot
flags dispatchables whose weight does not pay for new loops or
storage growth (`BasketClaimed`, per-hotkey drains, dissolve
cursors, `root_register` walking every subnet).

**When to fix:**

- New unbounded walk or unpaid work **this commit** added.
- Real economic loss (for example recycled credits on eviction).
- Secrets inherited by `npm install`, or unbounded PR archive
  extract. Do not waive those. Auto-merge stays stuck until
  `SAFE`.

**When not to thrash:**

- Same HIGH weight findings already on the PR, unchanged. Docs
  regen and rustfmt do not create them.
- HTTP 406 / “PR exceeds 300 files” / merge-base missing: infra.
  The workflow reads prefetch scripts from the **base** branch.
  A `prefetch.sh` fix on the PR head does nothing until it is
  on `main`. Do not rewrite pallet code for a 406.
- Fork PRs fail on auto-trigger. A nucleus member must
  `gh workflow run ai-review.yml -f pr_number=<N>`.
- User said "green ignoring skeptic": keep fixing every other
  required check. Do not invent weights.
- Auditor 👎 “missing tests” or a docs wording nit is
  informational unless that check itself is required. Skeptic
  can be `SAFE` while Auditor is 👎.

Do not invent `WeightInfo` numbers. CI measures on reference
hardware (threshold cited: 40%) and uploads a `bench-patch`
artifact. Label `apply-benchmark-patch` commits that patch, or
download it and apply it yourself. Label `run-benchmarks` turns
on `validate-benchmarks` (not required today).

### `bittensor-core wasm32 seam`

**Symptom:** checkout download `429`, or a real wasm compile break.

**Fix:** if the log is GitHub rate limit, `gh run rerun <id> --failed`.
If the log is a compile error in `bittensor-core` / `bittensor-core-wasm`,
fix the portable core. This job exists because Clippy on native does
not link the wasm target.

Mac: skip this locally. CI builds it on Linux.

### `typescript-formatting` / TS e2e gates

**Formatting:** `cd ts-tests && pnpm run fmt`. Required even when e2e
shards skip.

**E2E flakes (rerun once):**

- GHCR pull `denied: requested access to the resource is denied`
- GHCR push `unknown blob` after a successful build
- GitHub checkout `429`
- `intent_multisig_cancel` or stake-movement amounts that pass
  on siblings

```bash
gh run rerun <run-id> --failed
```

If the same test fails twice on this commit, it is a real regression.

**Real e2e bugs (fix the test or the step):**

- `NextKey` is `None` ~17–20s after the first block → wait/retry
  in the test. Not a `LOCALNET_IMAGE` rename.
- `find-e2e-tests` expected count drifted after a test was removed
  (for example 113 → 112). Update the count. Do not add a dummy
  test.
- Workflow still calls a deleted Python e2e path
  (`pytest tests/e2e/test_reads.py` file not found). Delete or
  retarget the step. Keep the metadata drift gate.

**Traps:** ignore a `LOCALNET_IMAGE` / `LOCALNET_IMAGE_NAME` hint
when 98/100 tests already hit the chain. Read the real assertion.

### `Bittensor E2E Test` / `find-e2e-tests`

Label `skip-bittensor-e2e-tests` skips the Rust SDK e2e suite.

### `cargo audit`

Label `skip-cargo-audit` skips it. Do not add advisory ignores just
to go green. Do not refresh `Cargo.lock` unless the lockfile change
is the task.

### `cargo test` (workspace)

Leave to CI unless you know the failing test name. Then:

```bash
SKIP_WASM_BUILD=1 cargo test -p pallet-subtensor <test_name>
```

CI uses `cargo nextest run --workspace --all-features` plus
`cargo test --doc --workspace --all-features`.

**py-sp-core / PyO3 traps:**

- `undefined reference` to `Py_*` under `--all-features` is the
  crate feature `extension-module`. Remove that **crate** feature.
  Maturin already sets `features = ["pyo3/extension-module"]`.
  Do not exclude the crate from `--all-features`.
- “Need `python3-dev`” is often a misread. Fix the **first**
  compile error. py-sp-core may never have been reached.
- After `source .venv/bin/activate`, cargo/pyo3 can cache the
  venv’s `python3.10`. `deactivate` in a subshell is not enough:

  ```bash
  unset VIRTUAL_ENV
  # take .venv off PATH, then:
  hash -r
  cargo clean -p pyo3-ffi
  ```

  Rebuild with system Python. This is local. CI Linux is usually
  fine.

---

## 5. Labels that change CI

Use a label only when the user asks, or when the failure is the label's
own gate.

| Label | Effect |
|---|---|
| `no-spec-version-bump` | Skip spec_version vs mainnet |
| `skip-clone-upgrade` | Skip clone-upgrade; required fan-in still passes |
| `fresh-mainnet-clone` | Live scrape; one `combined` job |
| `fresh-try-runtime-state` | Live RPC instead of cached try-runtime snap |
| `skip-cargo-audit` | Skip audit |
| `skip-bittensor-e2e-tests` | Skip Rust SDK e2e |
| `run-benchmarks` | Run `validate-benchmarks` on the PR |
| `apply-benchmark-patch` | Apply the bot's `weights.rs` patch |
| `check-node-compat` | Opt-in node compat harness |
| `mainnet-clone` | Public tunneled clone (non-fork, base `main`) |
| `auditor:run-node` | Auditor may start localnet (heavy) |

`red-team`, `blue-team`, `runtime`, `breaking-change` do not skip checks.

---

## 6. Flake vs real

Rerun once when the log is one of these:

- GitHub `429` on checkout (wasm32 seam, other jobs)
- GHCR pull `denied` / `error pulling image configuration`
- GHCR push `unknown blob` after a green build
- `Bad warp proof` / `Downloading finality proofs, 0.00 MiB`
- Job `cancelled` because a cheaper sibling failed, `--fail-fast`,
  or a newer push
- Skeptic HTTP 406 / “PR exceeds 300 files” / empty verdict JSON

```bash
gh run rerun <run-id> --failed
```

If it fails the same way on the same commit, treat it as real.

---

## 7. Time-wasters (do not do these)

- Run `scripts/fix_rust.sh`. It commits.
- Run full workspace Clippy or `cargo test` as routine preflight.
- Regenerate `_generated/` without a node on this spec.
- Hand-edit generated docs or `_generated/`.
- Bump `spec_version` or apply labels without being asked.
- Invent benchmark weights.
- Refresh a lockfile to "make CI pass".
- Merge latest main into the PR to hide an unrelated red check
  without checking that main is actually green for that check.
- Debug clone-upgrade when `cargo fmt` is the first failure.
- Treat `just clippy` as CI Clippy.
- Push fmt/clippy that you never committed.
- Push again to “unstick” CI (cancels 40+ min of clone/try-runtime).
- Dump a broad `generate.py` regen when `--check` was already stale
  from other WIP.
- Use system `ruff` instead of `uv run ruff`.
- Regen `_generated/` from a `--dev` / faucet node.
- Treat `pristine` green as “SDK metadata is fine.”
- Treat skeptic 406 as a pallet bug. Prefetch is read from **base**.
- Add `python3-dev` for a compile error that never reached py-sp-core.
- Exclude `py-sp-core` from `--all-features` instead of removing
  the `extension-module` crate feature.
- Chase `LOCALNET_IMAGE` when the container already ran tests.
- Assume an admin can force-merge `main`. `enforce_admins: true`
  still needs reviews and required checks.

---

## 8. Command card

```bash
# Where are we?
gh pr view --json number,url,headRefOid,mergeStateStatus
gh pr checks --json name,bucket,state,link

# Watch
gh pr checks --watch --fail-fast

# Logs
gh run view <run-id> --log-failed
gh run download <run-id> -n skeptic-output
gh run download <run-id> -n 'node-subtensor-release-<sha>' -D /tmp/release-node

# Rerun infra (after the run has finished)
gh run rerun <run-id> --failed

# Fast Rust
cargo fmt --check --all
cargo fmt --all   # only after check fails on owned files

# CI Clippy (when that job is red)
SKIP_WASM_BUILD=1 cargo clippy --workspace --all-targets -- -D warnings
SKIP_WASM_BUILD=1 cargo clippy --workspace --all-targets --all-features -- -D warnings

# Fast Python (sdk/python) — use uv run, not system ruff
uv run --no-sync ruff check .
uv run --no-sync ruff format --check .
uv run --no-sync python -m codegen.check --coverage --names
uv run --no-sync python ../../website/apps/bittensor-website/scripts/generate.py --check

# Regen bindings only after a Linux release node on THIS spec is up
# uv run python -m codegen ws://127.0.0.1:9944

# Fast TS (ts-tests)
pnpm run fmt

# Zepter (when that job is red)
zepter run check
# fix: zepter run default   # inspect the diff; do not use fix_rust.sh
```

---

## 9. Path filters (why a job skipped)

Runtime Checks uses `.github/scripts/classify-runtime-changes.sh`
on the **trusted base** SHA. Outputs: `runtime`, `docs`, `python_sdk`,
`sdk_drift`, `snapshot_ci`.

`docs=true` for `docs/*`, `website/*`, `sdk/python/*`, and
`.github/workflows/runtime-checks.yml`. That is why an SDK-only
change still runs the docs drift gate.

Check Rust uses `.github/scripts/classify-rust-changes.sh` and
`.github/rust-ci-paths.txt`. Non-Rust PRs get placeholder Clippy
jobs so the required names still exist.

If the classifier is missing or the PR file list fails, the
workflows **enable every check**. That looks like "why is clone
running on my docs typo?"

---

## 10. Mac vs CI

Local preflight sets `SKIP_WASM_BUILD=1` (root `justfile` does this
too). CI builds real runtime wasm and a Linux `node-subtensor` on
self-hosted Linux.

On this machine, Apple clang often cannot target wasm32. That is
environment, not your diff. Do not spend time on a local wasm
runtime build to "match CI".

Clone-upgrade and try-runtime need that Linux artifact. Reproduce
them only after the cheap gates are green and the log shows a real
migration, regression, or metadata-drift failure.
