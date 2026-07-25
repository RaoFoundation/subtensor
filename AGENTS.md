# Agent guide — subtensor

This repository is a Bittensor Substrate node (Rust / FRAME). Agents navigate primarily by text search (`rg`). Write code so names, paths, and definition-site docs are good search terms.

For the full discoverability conventions, load [`.agents/skills/write-discoverable-code/SKILL.md`](.agents/skills/write-discoverable-code/SKILL.md).

## Repo map (search anchors)

| Path | Role |
|------|------|
| `pallets/subtensor/` | Core: staking, subnets, emissions, epoch/consensus, registration |
| `pallets/swap/` | TAO↔alpha AMM / liquidity |
| `pallets/admin-utils/` | Sudo/admin hyperparameters and toggles |
| `pallets/limit-orders/` | Signed limit / take-profit / stop-loss orders |
| `pallets/{commitments,crowdloan,drand,shield,proxy,utility,transaction-fee,alpha-assets}/` | Supporting pallets |
| `runtime/` | `construct_runtime!`, migrations tuple, `spec_version`, runtime APIs |
| `node/` | Binary, RPC wiring, chain specs |
| `precompiles/` | EVM precompiles (addresses + Solidity selectors are frozen) |
| `common/`, `primitives/`, `support/` | Shared types, math, lints, macros |
| `sdk/` | Client core + language bindings (metadata-driven) |

Canonical layout doc: [`docs/internals/repo-layout.mdx`](docs/internals/repo-layout.mdx).

## Glossary (one spelling per concept)

| Prefer | Avoid |
|--------|--------|
| `netuid` / `NetUid` | `net_uid`, `network_id`, `subnet_id` (for the same type) |
| `hotkey` / `coldkey` | `hot_key`, `cold_key` |
| `tao` / `alpha` | inventing synonyms for the same asset |
| `stake` | mixing `bond` / `delegation` for the same storage amount |
| `subnet` | `network` when you mean a Bittensor subnet (except historical names) |
| `uid` | `neuron_index` for the per-subnet uid |
| `tempo` | `epoch_length` for the subnet tempo parameter |
| `emission` | `reward_mint` for coinbase emission |

When a frozen on-chain name already uses a different spelling, keep the frozen name; do not invent a parallel alias.

## Frozen surface (do not rename)

See the Tier A–D table in the write-discoverable-code skill. Short form:

- **Never rename:** storage item type names, `construct_runtime!` pallet names/indices, `call_index` numbers, Event/Error **variant order**, SCALE field order, RPC method strings, runtime API trait/method names, precompile addresses / Solidity selectors, applied migration name strings, `"SubtensorModule"` hardcodes, `WeightInfo` method names.
- **Do not edit:** [`pallets/subtensor/src/weights.rs`](pallets/subtensor/src/weights.rs) (generated).
- **Safe:** private/crate helpers, internal types not in storage/RPC, file splits (`foo.rs` → `foo/mod.rs`), test layout, doc comments (with `freeze_struct` caveat below).

Safety oracle: `scripts/check_metadata_unchanged.sh` (docs-stripped structural fingerprint must match `refactor/metadata-baseline.txt`).

## freeze_struct and docs

`#[freeze_struct("…")]` hashes the struct after blanking doc *text*, but **keeps** doc attributes. Therefore:

- Changing the text of an existing `///` comment: safe, no hash update.
- Adding or removing a doc comment on a frozen struct/field: changes the hash — update the hash **only** when the non-doc token stream is unchanged.

## Swarm / shard rules

During the discoverability migration (`refactor/discoverability`):

1. Own only the files listed in your shard in [`refactor/refactor-manifest.json`](refactor/refactor-manifest.json).
2. Rename a symbol only if `rg -w OldName` (repo-wide, excl. `target/`/`vendor/`) hits exclusively inside your owned files. Otherwise append to [`refactor/rename-proposals.md`](refactor/rename-proposals.md).
3. File splits must use `foo.rs` → `foo/mod.rs` (+ siblings) so other shards never need to edit your parent.
4. Before finishing: `cargo fmt`, clippy `-D warnings` on owned crates, tests on owned crates, and `scripts/check_metadata_unchanged.sh`.

## Local checks

```bash
just fmt
just clippy
SKIP_WASM_BUILD=1 cargo nextest run -p <crate>
./scripts/check_metadata_unchanged.sh
```

Runtime PRs normally bump `spec_version`; pure discoverability work that leaves metadata (minus docs) identical should use the `no-spec-version-bump` label and say so in the PR.
