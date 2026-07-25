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
