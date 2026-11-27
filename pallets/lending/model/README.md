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

  * `rate`: `curve_bps(apr, k, cap, num, den)`, 3,201 rows: the 1,344 vectors of the spec's evidence run plus
    edge rows (`k` = 1..8, `num` = 0, `den` = 0, the cap reached at each step, `u` up to 100, small operands,
    operands at the top of `u128`). Each row also has a `floor` and `rate = rate_bps(apr, k, cap, floor, num,
    den)`; the floor binds in 750 rows.
  * `coverage`: the short coverage need with `ref = max(spot, SMA30_rank)`, 540 rows. Each pool, book and
    SMA appears with three TWAP medians (above, between and below spot and SMA) and one need.
  * `accrual`: due and remainder after `n` blocks at the rate, 400 rows, half shorts
    (`num = ⌊q × TWAP_median⌋`, `den = C + P`) and half longs (`num = D`, `den = ⌊A × TWAP_median⌋ + C`).
    The floor binds in 171 rows.

The minimum-rate floor (`rate_floor_bps`, an approved addition beyond SPEC-v2.1) is not in the model:
`rate = max(floor, curve)` for a position with debt and holdings, 0 without debt and the cap without
holdings. The generator checks the `curve` of every rate and accrual row against `v21.core`, and the accrual
at the curve rate; it applies the floor on top by its own formula, so the accrual rows where the floor binds
are not the model's.

Every row carries its curve parameters. The spec's launch curve is `apr` 2,000, `k` 2, `cap` 10,000 (`k` and
`cap` are provisional, spec section 10.3); the model's own defaults (`k` 4, `cap` 200,000) are the values its
evidence runs swept from, not the launch values.

`gen_golden.py` (from `v201`) writes what v2.1 keeps unchanged: the pool curve (520 rows), the long coverage
need (180) and the accrual formula at a given rate (200).

The JSON is committed, so `cargo test` does not need Python. Regenerate it whenever a model changes and commit
both.
