# Reference model

Integer Python models of the lending spec (standard library only). The Rust pure core in `../src/math` must
agree with them to the rao.

| Package | What it models |
| --- | --- |
| `v21/` | `../docs/SPEC.md` v2.1: no forced trades, the convex rate `min(cap, apr × u^k)`, the short coverage reference `max(spot, SMA30_rank)`. A subclass of `v201x`. |
| `v201x/` | v2.0.1 plus the switches `v21` pins (defaults reproduce v2.0.1). Only imported by `v21`. |
| `v201/` | v2.0.1 as the previous spec revision's evidence was run on. |

The generators write the vectors the Rust differential tests in `../src/tests/golden.rs` replay:

    cd pallets/lending/model
    python3 gen_golden_v21.py                              # ../src/tests/golden/v21.json
    python3 gen_golden.py ../src/tests/golden/math.json

`gen_golden_v21.py` (from `v21`) writes three sections and exits nonzero unless every row agrees with
`v21.core`; it prints the row counts and the mismatches:

  * `rate`: `rate(apr, k, cap, floor, num, den)`, 3,264 rows: `apr` 1, 2,000 and 10,000 x `k` 1..8 x `cap`
    2,000 to 1,000,000 x 12 operand pairs x the floor off and on. The floor-on rows are the final rate (floor at
    `apr`, SPEC 2.1); the floor-off rows are the pure curve `min(cap, apr × u^k)`.
  * `rate_edge`: the final rate on edge operands, 1,857 rows: zeros for every exponent, the cap reached at each
    step and missed by one, `u` up to 100 on the launch curve, small and wide random operands, and the top of `u128`.
  * `coverage`: the short coverage need with `ref = max(spot, SMA30_rank)`, 540 rows. Each pool, book and
    SMA appears with three TWAP medians (above, between and below spot and SMA) and one need.
  * `accrual`: due and remainder after `n` blocks at the rate, 400 rows, half shorts
    (`num = ⌊q × TWAP_median⌋`, `den = C + P`) and half longs (`num = D`, `den = ⌊A × TWAP_median⌋ + C`).
    The floor binds in 171 rows.

The rate floor is `apr` itself (SPEC 2.1); there is no separate floor parameter. The model's `rate_floor`
switch applies it: off, a position with no debt pays 0 and the rate is the pure curve; on, it pays at least
`apr`. The rate rows carry the switch; the accrual rows are at the final rate (switch on).

Every row carries its curve parameters. The spec's launch curve is `apr` 2,000, `k` 1, `cap` 20,000 (SPEC 2.1); the model's own defaults (`k` 4, `cap` 200,000) are the values its
evidence runs swept from, not the launch values.

`gen_golden.py` (from `v201`) writes what v2.1 keeps unchanged: the pool curve (520 rows), the long coverage
need (180) and the accrual formula at a given rate (200).

The JSON is committed, so `cargo test` does not need Python. Regenerate it whenever a model changes and commit
both.
