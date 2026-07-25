# Cross-shard rename proposals

Symbols whose `rg -w OldName` hits span more than one shard's owned files.
Wave 3 processes this queue serially.

Format:

```
## OldName -> NewName
- reason:
- hits: (paste `rg -w OldName -g '*.rs' -g '!target/**' -g '!vendor/**' -l` output)
- proposed by: <shard-id>
- status: pending
```

---

## do_proxy -> dispatch_filtered_proxy_call
- reason: private helper that dispatches a call as the real account under ProxyType filters; name `do_proxy` is vague and collides with the `proxy` extrinsic in search results
- hits:
  - pallets/proxy/src/impls.rs
  - pallets/proxy/src/lib.rs
  - pallets/subtensor/src/guards/check_coldkey_swap.rs (comment only)
- proposed by: w1-proxy
- status: pending

## weight_and_dispatch_class -> batch_calls_weight_and_pays
- reason: Private batch weight helper; name should include domain word `batch` and clarify it returns `(Weight, Pays)` not a dispatch class. Hits outside w1-utility are string fixtures in the linting crate.
- hits:
```
pallets/utility/src/lib.rs
support/linting/src/require_extrinsic_benchmarks/tests.rs
```
- proposed by: w1-utility
- status: pending
- note (w1-support): fixture path updated after splitting `require_extrinsic_benchmarks.rs` → `require_extrinsic_benchmarks/`

## staking.rs / subnet.rs file splits (precompile path fingerprint)
- reason: `extract_metadata_fingerprint.py` records precompile INDEX/selectors with source file paths. Splitting `staking.rs` → `staking/mod.rs` (+ `legacy_v1.rs`) or `subnet.rs` → `subnet/mod.rs` changes the fingerprint even when INDEX values and Solidity selectors are unchanged. Wave-3 (or a coordinated baseline refresh) should either (a) make precompile fingerprint paths module-name based / path-agnostic, or (b) re-split these files and rewrite `refactor/metadata-baseline.txt` in the same commit. Both files remain >1000 lines after this shard's docs/renames.
- hits:
```
precompiles/src/staking.rs
precompiles/src/subnet.rs
scripts/extract_metadata_fingerprint.py
refactor/metadata-baseline.txt
```
- proposed by: w1-precompiles-b
- status: pending

## CommitmentsI -> CommitmentsPurgeBridge
- reason: runtime adapter that forwards `purge_netuid` into pallet_commitments; `CommitmentsI` is an opaque abbreviation. Same name is re-declared in several test mocks.
- hits:
```
runtime/src/lib.rs
eco-tests/src/mock.rs
chain-extensions/src/mock.rs
precompiles/src/mock.rs
pallets/transaction-fee/src/tests/mock.rs
```
- proposed by: w1-runtime
- status: pending

## GrandpaInterfaceImpl -> GrandpaAuthorityInterface
- reason: mirror the clearer `AuraAuthorityInterface` naming for the admin-utils Grandpa bridge.
- hits:
```
runtime/src/lib.rs
pallets/admin-utils/src/tests/mock.rs
```
- proposed by: w1-runtime
- status: pending

## TempoInterface -> SubtensorTempoBridge
- reason: runtime/commitments tempo lookup via Subtensor epoch index; name collides with trait-shaped helpers in pallet mocks.
- hits:
```
runtime/src/lib.rs
pallets/subtensor/src/tests/mock.rs
pallets/commitments/src/mock.rs
pallets/commitments/src/lib.rs
```
- proposed by: w1-runtime
- status: pending

## applicable_call -> subtensor_call_if
- reason: shared guard helper that yields a Subtensor `Call` when a predicate matches; name should include the domain word `subtensor` and read as a filter, not a boolean check. Used from both guards and the transaction extension.
- hits:
```
pallets/subtensor/src/guards/mod.rs
pallets/subtensor/src/guards/check_delegate_take.rs
pallets/subtensor/src/guards/check_evm_key_association.rs
pallets/subtensor/src/guards/check_rate_limits.rs
pallets/subtensor/src/guards/check_serving_endpoints.rs
pallets/subtensor/src/guards/check_weights.rs
pallets/subtensor/src/extensions/subtensor.rs
```
- proposed by: w2-src-guards
- status: pending

## guards::CallOf -> GuardsRuntimeCallOf
- reason: short `CallOf` alias collides in search with `pallet::CallOf`, extensions' local `CallOf`, and transaction-fee's `CallOf`; guards-specific name would disambiguate. Only the `pub(crate)` alias in `guards/mod.rs` (and its guard call sites) — not the pallet-module or other crates' aliases.
- hits:
```
pallets/subtensor/src/guards/mod.rs
pallets/subtensor/src/guards/check_coldkey_swap.rs
pallets/subtensor/src/guards/check_delegate_take.rs
pallets/subtensor/src/guards/check_evm_key_association.rs
pallets/subtensor/src/guards/check_rate_limits.rs
pallets/subtensor/src/guards/check_serving_endpoints.rs
pallets/subtensor/src/guards/check_weights.rs
```
- note: `rg -w CallOf` also hits unrelated same-named aliases in `lib.rs` (pallet module), `extensions/subtensor.rs`, and `transaction-fee`; do not rename those under this proposal.
- proposed by: w2-src-guards
## ensure_sn_owner_or_root_with_limits -> ensure_subnet_owner_or_root_with_limits
- reason: `sn` abbreviation is opaque next to sibling `ensure_subnet_owner_or_root`; spell out `subnet`.
- hits:
```
pallets/subtensor/src/utils/misc/origin_and_admin.rs
pallets/admin-utils/src/lib.rs
pallets/subtensor/src/tests/ensure.rs
```
- proposed by: w2-src-utils
- status: pending

## record_owner_rl -> record_owner_rate_limits
- reason: `rl` abbreviation hides that this stamps [`TransactionType`] last-block markers after owner admin calls.
- hits:
```
pallets/subtensor/src/utils/misc/origin_and_admin.rs
pallets/admin-utils/src/lib.rs
```
- proposed by: w2-src-utils
- status: pending

## uid_lookup -> associated_uids_for_evm_key
- reason: name does not say EVM reverse-index lookup; collides with the `UidLookup` precompile module name in search results.
- hits:
```
pallets/subtensor/src/utils/evm.rs
pallets/subtensor/src/lib.rs
pallets/subtensor/src/migrations/migrate_associated_evm_address_index.rs
pallets/admin-utils/src/tests/uids_validators.rs
precompiles/src/uid_lookup.rs
precompiles/src/lib.rs
```
- proposed by: w2-src-utils
- status: pending
