---
title: "Burn and recycle block-step increment"
description: "Local measurement of the additional nonzero burn/recycle paths, preserving the release-487 hook baseline."
---

The PR enables nonzero burn and recycle emission shares. Its weight change is
the existing `release-487` block-step weight plus the separately measured cost
of those two storage paths on 128 subnets. Owner payout and share calculation
already exist on the base branch. The combined-payout fixture remains enabled
for future full reference measurements.

Measured locally on 2026-11-04 with the compiled WASM benchmark runner, steps 3,
repeat 20:

```sh
cargo build --release -p node-subtensor --features runtime-benchmarks --locked
target/release/node-subtensor benchmark pallet \
  --runtime=target/release/wbuild/node-subtensor-runtime/node_subtensor_runtime.compact.compressed.wasm \
  --genesis-builder=runtime --genesis-builder-preset=benchmark \
  --wasm-execution=compiled --pallet=pallet_subtensor \
  --extrinsic=block_step_burn_recycle --steps=3 --repeat=20 \
  --no-storage-info --no-min-squares --no-median-slopes \
  --output=/tmp/subtensor-burn-recycle-increment.rs \
  --template=.maintain/frame-weight-template.hbs
```

Two runs produced 995,000,000 and 1,017,000,000 ps execution time. The higher
generated result is used: 1,017,000,000 ps execution time, 322,753 bytes estimated
proof, 512 reads, and 512 writes. With runtime RocksDB weights it is
65,017,000,000 ps (65.017 ms). Adding it to the base branch's
760,524,016,000 ps yields 825,541,016,000 ps, leaving capacity within the existing
4-second limit. Both weight implementations include the same generated increment.

This is a PR-specific local calibration, not a new reference measurement of
the full hook. The previous full Null measurement (about 4.56 seconds including
DB weights at 2,500 UIDs with collateral) remains a separate base-weight/capacity
finding; preserving the base branch does not resolve or invalidate that finding.
Full reference regeneration must use the combined-payout fixture and replace
the baseline-plus-increment composition with its measured `block_step` result.

The runtime admission regression reserves the actual `on_initialize` return
value and runs FRAME's weight pre-dispatch checks for a Shield key announcement,
the two sudo setup calls seen in CI failures, and a normal transfer. It failed
with `ExhaustsResources` before this change.
