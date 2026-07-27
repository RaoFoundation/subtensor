# Wave 3 gate

- Applied 30 safe cross-shard renames from `rename-proposals.md`
- Deferred: `uid_lookup` (precompile collision), bare `fixed` helper, `extensions/subtensor.rs` module rename (SDK fixtures)
- Precompile fingerprint made path-agnostic; baseline refreshed (`sha256:ee98fab0…`)
- `cargo check -p pallet-subtensor --lib` and `cargo check -p node-subtensor-runtime --lib` OK
- Full workspace nextest / try-runtime / clone-upgrade: run in CI on the PR
