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
support/linting/src/require_extrinsic_benchmarks.rs
```
- proposed by: w1-utility
- status: pending

## maybe_initialize_palswap -> maybe_initialize_swap_balancer
- reason: public helper that lazily creates per-netuid `SwapBalancer` / `PalSwapInitialized`; "palswap" is an internal nickname that does not match glossary/search terms (`balancer`, `swap`). Hits outside w1-swap in subtensor coinbase tests.
- hits:
```
pallets/swap/src/pallet/impls.rs
pallets/swap/src/pallet/migrations/migrate_swapv3_to_balancer.rs
pallets/swap/src/pallet/tests/swap_initialization.rs
pallets/swap/src/pallet/tests/clear_protocol_liquidity.rs
pallets/swap/src/pallet/tests/swap_execution.rs
pallets/subtensor/src/tests/coinbase.rs
```
- proposed by: w1-swap
- status: pending
