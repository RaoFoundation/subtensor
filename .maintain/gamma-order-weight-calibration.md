---
title: "Approximate gamma and order approval weights"
description: "Selective local WASM measurements for gamma and current account-approved order storage, scaled using the existing reference-host calibration."
---

Measured on 2026-11-04 from `release-487` source `21908de66`, using the production
WASM runtime, compiled execution, benchmark CLI 49.1.0, 10 steps and 10 repeats.
Only eight gamma/price methods and four account-approved order methods were run.
Two passes ran sequentially after runtime compilation stopped; all proof
coefficients and storage traces matched between passes. For each CPU coefficient,
the larger result from the two passes was selected.

The existing [set-service calibration](set-service-calibration.md) compares this
Apple M3 Max with an AMD EPYC 7763 reference runner, using completed CI
[run 41691082689](https://github.com/RaoFoundation/subtensor/actions/runs/41691082689),
artifact `13272969598`. Reuse its largest reference/local ratio, `1.7150625`,
without the optional 25% engineering margin:

```text
calibrated_cpu_ps = ceil(max(local_pass_1, local_pass_2) * 27441 / 16000)
```

Rounding is only to an integer picosecond; no extra margin is applied.
This applies independently to the base and every nonzero CPU slope. Proof-size
coefficients and DB reads/writes come directly from local benchmark output and
are not CPU-scaled. These are approximate calibrated local weights, not direct
reference-hardware measurements or proof of a worst-case bound across architectures.
Replace the selected methods with dedicated reference output when available.

| Method | Selected local CPU coefficients (ps) | Calibrated CPU coefficients (ps) |
| --- | ---: | ---: |
| `feed_tao_usd_price` | 13,000,000 | 22,295,813 |
| `mint_gamma` | 180,000,000 | 308,711,250 |
| `mint_gamma_with_stake` | 203,000,000 | 348,157,688 |
| `move_gamma` | 33,000,000 | 56,597,063 |
| `close_gamma_deposit` | 23,000,000 | 39,446,438 |
| `set_gamma_deposit_spender` | 34,000,000 | 58,312,125 |
| `set_gamma_deposit_daily_spend_cap` | 15,000,000 | 25,725,938 |
| `spend_gamma(n)` | 15,000,000 + 5,682,789 × n | 25,725,938 + 9,746,339 × n |
| `authorize_order` | 36,000,000 | 61,742,250 |
| `cancel_authenticated_order` | 22,000,000 | 37,731,375 |
| `execute_authenticated_orders(n, p)` | 233,481,167 + 315,431,036 × n | 400,434,794 + 540,983,942 × n |
| `execute_authenticated_batched_orders(n, p)` | 305,865,049 + 255,879,363 × n | 524,577,676 + 438,849,101 × n |

The execution benchmarks fit no positive CPU slope for `p`; its measured proof
and DB terms remain in both formulas. The current approval record has maximum
encoded size 1,034 bytes, replacing the stale 112-byte annotation. Approval proof
is now 5,371 bytes; authenticated cancellation proof is 4,499 bytes. Authenticated
execution proof includes `3,509 * p`, and approval record writes contribute `2 * p`.

Existing fixtures cover the bounded work without production logic changes:

- Gamma minting opens the 64th coldkey deposit, moves/closes use full 64-entry
  indices, and `spend_gamma` ranges from 1 to 512 successful distinct deposits.
- Order approval starts with eight entries, reclaims a terminal slot and retains
  seven live entries. Cancellation and execution also use full approval records.
- Mixed authenticated execution ranges over `n = 8..100` and `p = 1..8`.

To reproduce each pass with an existing compatible runner and a freshly built
runtime:

```sh
cargo build --locked --profile production -p node-subtensor-runtime --features runtime-benchmarks
target/production/node-subtensor benchmark pallet \
  --runtime=target/production/wbuild/node-subtensor-runtime/node_subtensor_runtime.compact.compressed.wasm \
  --genesis-builder=runtime --genesis-builder-preset=benchmark \
  --wasm-execution=compiled --pallet="$pallet" --extrinsic="$methods" \
  --steps=10 --repeat=10 --no-storage-info --no-min-squares --no-median-slopes \
  --output="$output" --template=.maintain/frame-weight-template.hbs
```

Selections:

- `pallet_subtensor`: `feed_tao_usd_price,mint_gamma,mint_gamma_with_stake,move_gamma,close_gamma_deposit,set_gamma_deposit_spender,set_gamma_deposit_daily_spend_cap,spend_gamma`.
- `pallet_limit_orders`: `authorize_order,cancel_authenticated_order,execute_authenticated_orders,execute_authenticated_batched_orders`.

The calibrated methods were applied with `scripts/benchmark_action.py
selective-patch`, updating `SubstrateWeight<T>` and `()` together. All unselected
weight methods, dispatchable logic, fixtures, and block limits are unchanged.
