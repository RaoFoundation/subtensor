# Repository Agent Guidance

## Quick CI preflight

Before handing off implementation work, run only the quick, deterministic
checks below that match the changed files. Heavy compilation, tests, audits,
and environment-sensitive validation belong in CI unless the user explicitly
requests them.

This file does not authorize commits, pushes, labels, PR changes, dependency
updates, or unrelated cleanup. Review and diagnostic tasks remain read-only.
If scope or ownership is unclear, do not take action; report what remains.

Before any command that can rewrite files, inspect `git status --short` and
preserve all pre-existing changes as user-owned work. Run check mode first. Use
fix mode only for a failure attributable to files in the current task, then
inspect the resulting diff.

Always finish with:

```bash
git diff --check
git status --short
```

Report the exact checks run and any check skipped because its existing locked
environment was unavailable. Do not install dependencies solely for this quick
preflight.

## Rust formatting

For Rust source, manifest, fixture, or feature changes, run:

```bash
cargo fmt --check --all
```

If it fails only because of task-owned files, run `cargo fmt --all`, inspect the
diff, and repeat the check. Do not run `scripts/fix_rust.sh`; it creates a
commit.

Leave Clippy, Rust builds and tests, and `cargo audit` to CI unless explicitly
requested. If `Cargo.toml` or `Cargo.lock` changed, confirm the lockfile change
is intentional; do not update dependencies or add advisory ignores merely to
make CI pass.

## Python SDK formatting and ABI drift

If the existing `sdk/python` environment is available, run from that directory:

```bash
uv run --no-sync ruff check .
uv run --no-sync ruff format --check .
```

For failures confined to task-owned files, use
`uv run --no-sync ruff check . --fix` and
`uv run --no-sync ruff format .`, inspect the diff, and repeat both checks.
Leave full type checking and tests to CI.

The Solidity ABI files in `precompiles/src/solidity/*.abi` are canonical. When
one of those or a vendored ABI changes, run this narrow consistency test from
`sdk/python`:

```bash
uv run --no-sync pytest tests/unit/test_evm.py::TestVendoredAbiSync -q
```

Update `sdk/python/bittensor/evm/abi/*.json` only when drift is caused by the
current canonical ABI change. Do not edit only the vendored copy.

Runtime metadata bindings under `sdk/python/bittensor/_generated/` require the
correct upgraded node. Do not regenerate them as routine preflight. If CI or
the task indicates they are stale, report the required regeneration unless the
user explicitly requests it and the correct node provenance is known.

## Generated reference docs

Changes to Python registries, calls, queries, errors, hyperparameters, or other
docs-generator inputs require this check from `sdk/python` when its existing
locked environment is available:

```bash
uv run --no-sync python ../../website/apps/bittensor-website/scripts/generate.py --check
```

If drift is attributable to the current task, run
`uv run --no-sync python ../../website/apps/bittensor-website/scripts/generate.py`,
repeat the check, and inspect the generated diff. Unexpected broad drift is a
reason to stop and report it. Do not hand-edit generated files under
`docs/tx/`, `docs/query/`, `docs/errors/`, generated hyperparameter index/meta
files, or `website/apps/bittensor-website/public/catalog/`.

For rendered Markdown and MDX changes, verify that pages have string `title`
and `description` frontmatter and that referenced MDX components exist. Leave
the full website install and build to CI.

## TypeScript formatting

For `ts-tests` changes, run this when its existing locked environment is
available:

```bash
pnpm run fmt
```

If it fails only in task-owned files, run `pnpm run fmt:fix`, inspect the diff,
and repeat the check. Leave lint, type checking, builds, and E2E tests to CI.

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

Do not start clone/regression workflows, full builds, comprehensive test
suites, dependency audits, or benchmark generation as routine preflight.
