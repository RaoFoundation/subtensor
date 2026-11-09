---
title: "Approximate set_service weight calibration"
description: "Local WASM benchmark measurements scaled against a completed CI reference run."
---

The `set_service` weight is an approximate calibrated local measurement, not a
direct measurement on reference hardware. Replace it with dedicated reference
output when available. A single factor across CPU architectures cannot prove a
worst-case execution bound; the margin below is an engineering allowance.

Source: `4f23d5e1a8` (same benchmark/runtime sources as CI `e04c0881a8`).
CI baseline: [run 41691082689](https://github.com/RaoFoundation/subtensor/actions/runs/41691082689),
`bench-patch` artifact 13272969598. Its generated files contain the completed
admin-utils and commitments measurements, but the run timed out before producing
pallet-subtensor output.

Reference host: AMD EPYC 7763. Local host: Apple M3 Max (Mac15,11), macOS ARM64,
rustc 1.89.0. Both used compiled WASM, benchmark CLI 49.1.0, production profile,
50 steps, 20 repeats, and the repository's frame-weight template. The three local
passes ran sequentially after compilation finished.

Times below are the generated base `ref_time`, in picoseconds, excluding DB
costs. Local control proof sizes and reads/writes matched reference output in
all passes.

| Control | CI | Local passes 1 / 2 / 3 | CI / local median |
| --- | ---: | ---: | ---: |
| `sudo_set_serving_rate_limit` | 28,874,000 | 18,000,000 / 18,000,000 / 18,000,000 | 1.604111 |
| `sudo_set_max_allowed_validators` | 41,368,000 | 26,000,000 / 26,000,000 / 26,000,000 | 1.591077 |
| `sudo_set_activity_cutoff` | 36,338,000 | 23,000,000 / 23,000,000 / 23,000,000 | 1.579913 |
| `set_commitment` | 27,441,000 | 16,000,000 / 16,000,000 / 16,000,000 | 1.7150625 |

The median ratio is 1.597594. Use the largest ratio, 1.7150625, for the estimate.
`set_service` produced base times 37,000,000 / 38,000,000 / 37,000,000 ps,
measured proof 3,839 bytes, estimated proof 7,304 bytes, six reads and one write
in every pass. The existing benchmark exercises a 256-byte URL and a 2,500-entry
validator-permit vector, including the failed owner-coldkey check followed by
validator authorization. Repeated access to `Owner` is cached; benchmark storage
accounting reports one distinct `Owner` read.

Calculation: `ceil_to_1us(38,000,000 * 1.7150625 * 1.25) = 82,000,000 ps`.
The 25% margin is explicit. Proof size and DB reads/writes are not CPU-scaled.
Both `SubstrateWeight<T>` and the `()` fallback use this execution/proof bound.

To reproduce, build with `cargo build --locked --profile production -p
node-subtensor --features runtime-benchmarks`, then run this command three times
for each selection below, using a separate output file for each pass:

```bash
target/production/node-subtensor benchmark pallet \
  --runtime=target/production/wbuild/node-subtensor-runtime/node_subtensor_runtime.compact.compressed.wasm \
  --genesis-builder=runtime --genesis-builder-preset=benchmark \
  --wasm-execution=compiled --pallet="$pallet" --extrinsic="$extrinsics" \
  --steps=50 --repeat=20 --no-storage-info --no-min-squares --no-median-slopes \
  --output="$output" --template=.maintain/frame-weight-template.hbs
```

Selections:

- `pallet_admin_utils`: `sudo_set_serving_rate_limit,sudo_set_max_allowed_validators,sudo_set_activity_cutoff`
- `pallet_commitments`: `set_commitment`
- `pallet_subtensor`: `set_service`
