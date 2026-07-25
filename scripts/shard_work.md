# Shard agent instructions

You are a discoverability shard worker on branch `refactor/discoverability`.

## Before editing

1. Read `AGENTS.md` and `.agents/skills/write-discoverable-code/SKILL.md`.
2. Read `refactor/FREEZE_STRUCT.md`.
3. Load your shard from `refactor/refactor-manifest.json` by id (argument).
4. **Only modify files listed in your shard's `files` array.** Creating new files under a directory you own (e.g. split `foo.rs` → `foo/mod.rs`) is allowed; add them to the mental ownership of your shard.
5. Never edit `pallets/subtensor/src/weights.rs`.

## Work to do

Depending on `task`:

- **discoverability**: Add definition-site doc comments; rename private/`pub(crate)` helpers to 2–3 word domain names when `rg -w OldName` hits are entirely inside your files; split files >~1000 lines via `foo.rs` → `foo/mod.rs` + concept modules; name tests after sources.
- **docs-only**: Only add/improve doc comments. Do not rename anything. Do not reorder enum variants. For `freeze_struct`, follow FREEZE_STRUCT.md (update hash only for doc presence changes).
- **split-and-name**: Split the listed giant test file into `tests/<stem>/mod.rs` + concept modules; update `tests/mod.rs` only if it is in your file list (otherwise note the needed `mod` wiring in rename-proposals).

If a rename would touch files outside your list, append to `refactor/rename-proposals.md` (that file is shared — only append under a `##` heading for your symbol).

## Exit checklist (must pass)

```bash
./scripts/check_metadata_unchanged.sh
cargo fmt --all
# Prefer package-scoped checks when possible:
cargo clippy -p <relevant-packages> --all-targets -- --deny warnings
SKIP_WASM_BUILD=1 cargo nextest run -p <relevant-packages>
```

Commit on a branch named `refactor/shard-<shard-id>` with message:
`refactor(<shard-id>): discoverability improvements`

Do not push to main. Do not change call_index, storage type names, event/error order, RPC strings, or precompile selectors.
