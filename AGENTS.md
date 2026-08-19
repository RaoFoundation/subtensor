# Repository Agent Guidance

## CI preflight is part of the change

Before handing off a change, run the checks below that correspond to every
area touched. Use the repository's locked dependency installs and the same
commands CI uses. Run formatters in fix mode first, then their check mode.

Always finish with:

```bash
git diff --check
git status --short
```

Inspect all generated and lockfile changes. Commit intentional outputs; do not
leave required regeneration for CI to discover. Report the exact checks run
and call out any check that was skipped or could not run.

## Rust changes

For any Rust source, manifest, fixture, or feature change, run:

```bash
cargo fmt --all
cargo fmt --check --all
SKIP_WASM_BUILD=1 cargo clippy --workspace --all-targets -- -D warnings
SKIP_WASM_BUILD=1 cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Run the relevant package tests as well. Clippy checks both feature modes and
treats warnings as errors; do not assume passing the default mode is enough.
Avoid `scripts/fix_rust.sh` for routine preflight because it creates a commit.

When `Cargo.toml` or `Cargo.lock` changes, verify that `Cargo.lock` reflects the
intended dependency graph and run `cargo audit`. A newly published advisory may
be unrelated to the patch; investigate and report it rather than adding an
ignore or mechanically upgrading dependencies.

## Python SDK changes

The Python SDK uses `uv` and its committed lockfile. From `sdk/python`, run:

```bash
uv sync --locked --all-extras --dev
just fmt
just check
```

`just check` covers Ruff formatting/linting, type checking, unit tests, and
offline code-generation consistency. Re-run it after `just fmt` changes files.
Commit an intentional `uv.lock` update with the dependency change that caused
it.

The Solidity ABI files in `precompiles/src/solidity/*.abi` are canonical. When
one changes, update the matching vendored JSON under
`sdk/python/bittensor/evm/abi/` and run the SDK checks. Do not edit only the
vendored copy.

Runtime metadata bindings in `sdk/python/bittensor/_generated/` are committed
outputs. A metadata-affecting runtime change requires an upgraded local node,
then from `sdk/python`:

```bash
just regen
```

Commit both the regenerated bindings and the recorded golden fixture. Do not
regenerate them against an old or unrelated runtime.

## Generated reference docs and website

Changes to Python registries, calls, queries, errors, hyperparameters, or other
inputs consumed by the docs generator require regeneration. From `sdk/python`:

```bash
uv sync --locked --all-extras --dev
just regen-docs
just check-docs
```

Commit all resulting reference pages and catalog data. Generated surfaces
include `docs/tx/`, `docs/query/`, `docs/errors/`, hyperparameter index/meta
files, and `website/apps/bittensor-website/public/catalog/`. Change the source
registry or generator rather than hand-editing generated output.

For documentation or website changes, also run:

```bash
cd website
yarn install --immutable
yarn turbo run build --filter=@raofoundation/bittensor-website
```

Every rendered Markdown or MDX page must have string `title` and `description`
frontmatter. Verify that referenced MDX components exist and are imported.
Commit an intentional `website/yarn.lock` update with its dependency change.

## TypeScript test changes

From `ts-tests`, use the committed pnpm lockfile:

```bash
pnpm install --frozen-lockfile
pnpm run fmt:fix
pnpm run fmt
pnpm run lint
```

Run the relevant E2E suite when behavior changes. Commit an intentional
`pnpm-lock.yaml` update with its dependency change.

## Runtime version and weights

Runtime-affecting changes trigger the spec-version gate. The literal
`spec_version` in `runtime/src/lib.rs` must be newer than mainnet unless the PR
has the `no-spec-version-bump` label. Treat this as advisory: do not change
`spec_version` unless the user explicitly requests that release action. Never
initiate a bump merely to satisfy CI. Instead, tell the maintainer that the PR
needs the label or a deliberate, separately authorized version bump.

Adding or changing a dispatchable requires matching benchmarks and
`WeightInfo` wiring. Run the pallet's benchmark tests:

```bash
cargo test -p <pallet> --features runtime-benchmarks benchmarks
```

When measured weights must change, follow
`docs/internals/benchmarks-and-weights.mdx` and use
`./scripts/benchmark_all.sh <pallet>` rather than inventing weight values.

## Workflow and dependency-file changes

Treat `.github/**` changes as CI code. Run the relevant repository workflow or
unit scripts and run `actionlint` when it is available. Preserve action pinning
and do not add an unapproved third-party action merely to make a workflow pass.

Use the package manager that owns each lockfile:

- Rust: `Cargo.lock`
- Python SDK: `sdk/python/uv.lock`
- TypeScript tests: `ts-tests/pnpm-lock.yaml`
- Website: `website/yarn.lock`
- Docs preview: `.github/docs-preview-vercel/package-lock.json`

Use locked or frozen install modes in preflight. If a lockfile changes without
an intentional dependency change, stop and resolve the drift before handoff.

The full clone/regression workflow is warranted for runtime behavior,
migrations, and chain-state-sensitive changes. It is not a substitute for the
cheap formatting, generation, lint, lockfile, and docs checks above.
