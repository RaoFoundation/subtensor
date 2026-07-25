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

## get_shares -> subnet_emission_shares
- reason: `get_shares` is opaque at search time; name should say these are per-subnet TAO emission weights (price-EMA + miner-burn). Used from coinbase and tests.
- hits:
```
pallets/subtensor/src/coinbase/subnet_emissions.rs
pallets/subtensor/src/tests/subnet_emissions.rs
```
- proposed by: w2-src-coinbase
- status: pending

## inject_and_maybe_swap -> inject_pool_liquidity_and_swap_excess
- reason: clarifies that this materializes tao_in/alpha_in into the pool and swaps excess_tao for protocol alpha; "maybe" hides the always-attempted swap path when excess > 0.
- hits:
```
pallets/subtensor/src/coinbase/run_coinbase/emission_injection.rs
pallets/subtensor/src/tests/coinbase.rs
```
- proposed by: w2-src-coinbase
- status: pending

## get_subnet_terms -> compute_subnet_emission_terms
- reason: "terms" alone is ambiguous; this splits block TAO emission into tao_in/alpha_in/alpha_out/excess_tao per the dTAO injection cap.
- hits:
```
pallets/subtensor/src/coinbase/run_coinbase/emission_injection.rs
pallets/subtensor/src/tests/coinbase.rs
```
- proposed by: w2-src-coinbase
- status: pending

## drain_pending -> drain_pending_subnet_emissions
- reason: `drain_pending` does not say what is drained; this takes pending server/validator/root/owner alpha on epoch fire.
- hits:
```
pallets/subtensor/src/coinbase/run_coinbase/drain_pending_emissions.rs
pallets/subtensor/src/coinbase/run_coinbase/mod.rs
pallets/subtensor/src/tests/coinbase.rs
```
- proposed by: w2-src-coinbase
- status: pending

## get_network_root_sell_flag -> should_accumulate_root_alpha_dividends
- reason: boolean name should read as a predicate; "root sell" is jargon for whether root alpha divs are accumulated vs recycled when total EMA price ≤ 1.
- hits:
```
pallets/subtensor/src/coinbase/run_coinbase/emission_injection.rs
pallets/subtensor/src/coinbase/run_coinbase/mod.rs
pallets/subtensor/src/tests/coinbase.rs
pallets/subtensor/src/tests/claim_root.rs
```
- proposed by: w2-src-coinbase
- status: pending

## fixed -> i32f32_from_f32
- reason: bare `fixed` collides with countless unrelated hits; this helper is specifically `f32` → epoch `I32F32`.
- hits:
```
pallets/subtensor/src/epoch/math/fixed_conversions.rs
pallets/subtensor/src/tests/epoch.rs
pallets/subtensor/src/tests/math.rs
```
- proposed by: w2-src-epoch
- status: pending

## vecdiv -> elementwise_safe_div
- reason: opaque abbreviation; performs element-wise `safe_div` of two `I32F32` vectors (0 divisor → 0).
- hits:
```
pallets/subtensor/src/epoch/math/vector_ops.rs
pallets/subtensor/src/tests/math.rs
```
- proposed by: w2-src-epoch
- status: pending

## is_epoch_input_state_consistent -> epoch_keys_have_unique_hotkeys
- reason: name does not say what is checked (duplicate hotkeys in `Keys`); used from coinbase preflight and tests.
- hits:
```
pallets/subtensor/src/epoch/run_epoch/bonds_ema_liquid_alpha.rs
pallets/subtensor/src/coinbase/run_coinbase.rs
pallets/subtensor/src/tests/epoch.rs
pallets/subtensor/src/tests/coinbase.rs
```
- proposed by: w2-src-epoch
- status: pending

## do_reset_bonds -> reset_bonds_column_for_hotkey
- reason: `do_` prefix mimics dispatchables; this clears one hotkey column in `Bonds` when bonds-reset is enabled. Hits runtime + tests.
- hits:
```
pallets/subtensor/src/epoch/run_epoch/bonds_ema_liquid_alpha.rs
runtime/src/lib.rs
pallets/subtensor/src/tests/epoch.rs
```
- proposed by: w2-src-epoch
- status: pending

## get_weights_sparse -> unnormalized_weights_sparse
- reason: parallel to in-shard `unnormalized_bonds_sparse`; clarifies storage weights are not row-normalized. Hits tests outside shard.
- hits:
```
pallets/subtensor/src/epoch/run_epoch/weights_bonds_loaders.rs
pallets/subtensor/src/tests/epoch.rs
pallets/subtensor/src/tests/mechanism.rs
pallets/subtensor/src/tests/weights.rs
```
- proposed by: w2-src-epoch
## if_subnet_exist -> subnet_exists
- reason: `if_` prefix reads as a statement; boolean helpers elsewhere use `is_`/`*_exists`. Hits span many shards (dispatches, staking, rpc_info, runtime, admin-utils).
- hits:
```
pallets/subtensor/src/subnets/subnet.rs
pallets/subtensor/src/macros/dispatches.rs
pallets/subtensor/src/lib.rs
pallets/subtensor/src/coinbase/root.rs
pallets/subtensor/src/coinbase/tempo_control.rs
pallets/subtensor/src/staking/*.rs
pallets/subtensor/src/rpc_info/*.rs
pallets/subtensor/src/swap/swap_hotkey.rs
pallets/subtensor/src/utils/*.rs
pallets/admin-utils/src/lib.rs
runtime/src/lib.rs
chain-extensions/src/lib.rs
pallets/subtensor/src/tests/*.rs
pallets/subtensor/src/migrations/*.rs
```
- proposed by: w2-src-subnets
- status: pending

## get_netuid -> netuid_from_mechanism_storage_index
- reason: bare `get_netuid` hides that the argument is a packed mechanism [`NetUidStorageIndex`], not a raw netuid lookup.
- hits:
```
pallets/subtensor/src/subnets/mechanism.rs
pallets/subtensor/src/utils/misc/consensus_params.rs
```
- proposed by: w2-src-subnets
- status: pending

## set_element_at -> set_vec_element_at
- reason: generic helper used when clearing/replacing neuron vectors; name should say it mutates a slice/vec slot.
- hits:
```
pallets/subtensor/src/subnets/uids.rs
pallets/subtensor/src/tests/uids.rs
```
- proposed by: w2-src-subnets
- status: pending

## is_uid_exist_on_network -> uid_exists_on_network
- reason: grammar (`is_uid_exist`); weights module (same directory but outside this shard's file list) also calls it.
- hits:
```
pallets/subtensor/src/subnets/uids.rs
pallets/subtensor/src/subnets/weights.rs
```
- proposed by: w2-src-subnets
- status: pending

## is_subnet_account_id -> netuid_for_subnet_account
- reason: returns `Option<NetUid>`, not a bool; name should not start with `is_`.
- hits:
```
pallets/subtensor/src/subnets/subnet.rs
pallets/subtensor/src/utils/misc/subnet_hyperparams.rs
pallets/subtensor/src/swap/swap_hotkey.rs
pallets/subtensor/src/staking/helpers.rs
pallets/subtensor/src/macros/errors.rs
runtime/tests/account_conversion.rs
pallets/subtensor/src/tests/subnet.rs
```
- proposed by: w2-src-subnets
## do_swap_coldkey -> perform_coldkey_swap
- reason: `do_` prefix is opaque next to the `swap_coldkey` extrinsic; name should say it performs the coldkey identity migration body (not a dispatchable).
- hits:
```
pallets/subtensor/src/swap/swap_coldkey.rs
pallets/subtensor/src/swap/mod.rs
pallets/subtensor/src/macros/dispatches.rs
pallets/subtensor/src/tests/swap_coldkey.rs
pallets/subtensor/src/tests/coldkey_lineage.rs
pallets/subtensor/src/tests/claim_root.rs
pallets/subtensor/src/tests/locks.rs
```
- proposed by: w2-src-swap
- status: pending

## do_swap_hotkey -> perform_hotkey_swap
- reason: same as coldkey — extrinsic body helper; `do_swap_hotkey` collides with extrinsic search and does not say "identity rename".
- hits:
```
pallets/subtensor/src/swap/swap_hotkey.rs
pallets/subtensor/src/swap/mod.rs
pallets/subtensor/src/macros/dispatches.rs
pallets/subtensor/src/staking/claim_root.rs
pallets/subtensor/src/tests/swap_hotkey.rs
pallets/subtensor/src/tests/swap_hotkey_with_subnet.rs
pallets/subtensor/src/tests/hotkey_lineage.rs
pallets/subtensor/src/tests/locks.rs
```
- proposed by: w2-src-swap
- status: pending

## charge_swap_cost -> charge_coldkey_swap_cost
- reason: helper only recycles the coldkey-swap fee; name should include `coldkey` so it does not read as a generic/hotkey fee charger.
- hits:
```
pallets/subtensor/src/swap/swap_coldkey.rs
pallets/subtensor/src/macros/dispatches.rs
```
- proposed by: w2-src-swap
- status: pending

## swap_hotkey_v2_dispatch_weight -> hotkey_swap_dispatch_weight
- reason: `v2` is a call-site version tag, not what the helper computes; name should lead with `hotkey_swap` and say it returns pre-dispatch `Weight`.
- hits:
```
pallets/subtensor/src/swap/swap_hotkey.rs
pallets/subtensor/src/macros/dispatches.rs
```
- proposed by: w2-src-swap
- status: pending
