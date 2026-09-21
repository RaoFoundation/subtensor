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
