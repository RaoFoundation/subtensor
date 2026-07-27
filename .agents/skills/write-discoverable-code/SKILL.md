---
name: write-discoverable-code
description: Write Rust/Substrate code so grep-first coding agents can find, parse, and trust it — distinctive names, precise types, definition-site docs, concept-named files. Use when adding or refactoring code in subtensor, or when running a discoverability migration shard.
---

# Write discoverable code (subtensor)

Coding agents navigate this repo with `rg` / filename search, not a dependency graph. Names and paths are the reverse index. Types and compiler errors are the feedback loop agents cannot skip.

## Naming

1. **Exported / crate-visible symbols: 2–3 words, one domain word.** Prefer `calculate_subnet_emission` over `calculate`. Package/module path counts as a word only when call sites use the qualified path in source (`swap::NewClient`-style); FRAME dispatchables and free functions need the domain word in the identifier itself.
2. **One spelling per concept.** Follow the glossary in root `AGENTS.md`. Do not introduce `organization` beside `org`, or `network_id` beside `NetUid`.
3. **Spell out common abbreviations in new helpers.** Prefer `subnet` over `sn`, `rate_limit` over `rl`, and full domain words over opaque stubs (`Custom`, `I`, `Impl` as the only distinguisher). Boolean helpers should read as predicates (`subnet_exists`, `should_accumulate_…`), not `if_*`. Extrinsic-body helpers should not share a `do_*` prefix with the dispatchable — use `perform_*` / a concept verb.
4. **Files and modules are search terms.** Name files after the concept they own (`smtp_settings.py` → here: `folder_fallbacks.rs`, `hmac_payload_signer.rs`). Prefer `email/message_rendering.rs` over a 5k-line grab-bag. Precompile `INDEX` / Solidity selectors are path-agnostic in the metadata fingerprint, so `foo.rs` → `foo/mod.rs` splits are safe when those values stay put.
5. **Tests named after source.** `staking/add_stake.rs` → tests that cover it live under a discoverable name (`tests/add_stake.rs` or a module clearly about add-stake). Avoid dumping unrelated cases into one monolith when splitting is cheap.
6. **Mark legacy `@deprecated` / `#[deprecated]`** when keeping a path temporarily. Prefer deletion.

### Do not rename (Tier A–D)

| Tier | Surface | Why |
|------|---------|-----|
| **A** | Storage item **type names**; `construct_runtime!` pallet names and indices | Twox128 storage keys / module prefix |
| **B** | `call_index` numbers; Event/Error **variant order**; SCALE field **order**; `freeze_struct` layouts | Wire / codec compatibility |
| **C** | Extrinsic **fn names**; Event/Error **names**; RPC `#[method(name = "…")]` strings; runtime API trait/method names; precompile `INDEX` + Solidity `public("…")` selectors | Clients, SDKs, EVM, explorers |
| **D** | `WeightInfo` method names (must match calls); applied migration **name strings**; hardcoded `"SubtensorModule"` / pallet prefix strings | Benchmarks, migration idempotency, EVM storage query |

**Never edit** generated [`pallets/subtensor/src/weights.rs`](../../../pallets/subtensor/src/weights.rs).

**Generally safe:** private helpers, `pub(crate)` types not in storage/RPC/metadata, module paths under a pallet, file splits, test helpers, doc comments (see freeze_struct rule).

If a rename would touch files outside your shard, append a line to `refactor/rename-proposals.md` instead of doing it.

## Types

- Annotate inputs/outputs; avoid `any`-equivalent opacity (`impl Trait` only when the trait name is itself a good search term).
- Prefer newtypes / distinct ID types (`NetUid`, account kinds) over raw `u16`/`AccountId` soup at API boundaries so the compiler catches swaps.
- Type names obey the same uniqueness rule as function names (`SubnetEmissionResult`, not `Result` aliases that collide).

## Comments

- **One sentence on the definition** — storage item, dispatchable, important helper — saying what the code cannot say (invariants, units, “deliberately does not sanitize”, migration constraints).
- Agents land on definitions via search; definition-site docs are the highest-leverage docs.
- Do not restate the signature. Do not invent behavior that is not true.

### freeze_struct caveat

`#[freeze_struct]` blanks doc text before hashing but keeps doc attributes. **Adding or removing** docs on a frozen struct/field requires updating the hash; **editing existing** doc text does not. Only update the hash when the non-doc token stream is unchanged.

## File structure

- Target **≤ ~1000 lines** per file for new splits; existing giants should be split by concept when touched.
- Split pattern: `foo.rs` → `foo/mod.rs` + `foo/<concept>.rs`. Update `mod` declarations in the owning tree only.
- Do not create barrel `export *` equivalents that erase names without need; keep re-exports explicit when agents must follow them.

## Deliberate absences

If the codebase intentionally does not do something readers will search for (e.g. “HTML email is not sanitized”), document that at the definition or module the search will hit. Grep cannot prove absence.

## Exit checklist (shard agents)

```bash
cargo fmt --all
cargo clippy -p <owned-crates> --all-targets -- --deny warnings
SKIP_WASM_BUILD=1 cargo nextest run -p <owned-crates>
./scripts/check_metadata_unchanged.sh
```

Metadata fingerprint must match `refactor/metadata-baseline.txt`. If it fails, you changed a frozen surface — revert that part.
