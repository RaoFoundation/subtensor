<!-- Generated from the working spec by make_public_spec_v21.py; edit the source, not this file. -->
# Synthetic-mint shorts and pool-lifted longs: build spec v2.1 (rewrite of PR #3268)

Status: build spec for review. Nothing has been posted or deployed. v2.1 is a simplification of v2.0.1: the protocol never trades on anyone's behalf while a subnet is live. Stretched positions are wound down by a convex interest rate that pays the subnet's pool, and a position ends by its own close or by default when its collateral is spent. Evidence for v2.1 is the reference model `model/v21` (section 10). Evidence for everything v2.1 does not change (coverage, dereg settlement and ranking, supply cap, longs economics) is the v2.0.1 record, sections 9–21 of the previous revision of this file, which v2.1 keeps by reference and does not repeat (section 11).

In this repo: the implementation design is `DESIGN-v2.md` next to this file. The v2.0.1 reference model used by the differential tests lives in `pallets/lending/model/`. The v2.1 model (`model/v21`), the result files named in section 10, and `IGOR-REVIEW-v201.md` / `model/results_igor/` cited in section 11 are in the research workspace, not in this repo.

**Changelog**
* **v2.1 (2026-11-27 UTC).** Jacob approved the design in chat: no forced trades on either side, and one convex interest curve for both sides. Igor's v2.0.1 review is folded in where it still applies.
  * **No forced trades.** Forced deleverage (the short buy-back and the long sell-down) is deleted on both sides, with everything that existed only to make it safe: the `Deleveraging` state, the pace limit, both pause bands, the health trigger, `equity_floor`, `exit_hysteresis_bps`, `min_interest_cover_blocks`, the per-block deleverage work, `poke` / `poke_long`, and the deleverage queue, cursor and budgets. A position is either Open or gone (closed, defaulted or settled at dissolution).
  * **Convex interest (section 1 item 2, P3/P5).** Interest is still charged on the current debt value on the existing accrual and charge schedule, and still goes 100% to the subnet's SubnetTAO. The yearly rate is now `rate = min(apr × u^k, cap)`, where u is the position's debt over its holdings, valued at the 3-day TWAP median: short `u = q × TWAP_median / (C + P)`, long `u = D / (A × TWAP_median + C)`. `apr`, `k` and `cap` are shared by both sides and root-settable. The rate depends only on a ratio, so it is the same for a 1 TAO and a 10,000 TAO position.
  * **Default (section 1 item 6, 7.1 item 7).** A position defaults when an interest charge finds C exhausted with interest still due. Short: P goes to SubnetTAO and the unbought q becomes ResidualAlpha. Long: the locked alpha is sold in one swap, lender first; any gap is a lender shortfall. This is Igor's N4 fix, now the only way a position ends without its owner.
  * **Igor fixes adopted:** virtual long with a gross per-block lift limit (2.2), and the short coverage reference `max(spot, SMA30_rank)` (2.4). N3 (`equity_floor` in bps of H) and the separate N4 switch are **obsolete**: the parameter and the paused state they patched no longer exist. 2.1 and N1 (the squeeze through the forced buy-back) are closed by construction: nothing buys for a short. The new interest-side lever this creates is measured in section 10.
  * **Partial close** is allowed when coverage holds afterwards **or** the close does not raise the position's u (pure de-risking), so a stretched position can shrink itself without first topping up.
  * **Open decisions for Jacob (section 8.1):** dissolution payout path and the short proceeds floor (2.7/2.8/2.9), the conviction bar (2.6), griefing near the prune (2.7 residue), the stress slope dial (2.3), and the rate launch values (section 10 recommends).
  * **Parameter count:** 25 → 21. Six shared params removed (`equity_floor`, `min_interest_cover_blocks`, `pace_bps_per_block`, `pause_twap_bps`, `pause_sma_bps`, `exit_hysteresis_bps`); two added (`rate_k`, `rate_cap_bps`).
  * **Removed IDs (not reused):** P7, P17, P22 (as a pause; the SMA30 ring itself stays for ranking and coverage), the forced-close half of P5, L7, L9; invariants I5 (old form), I9, I15 (except the prune sale), I23 (old form), I27; e2e 2, 7, 12, 13, 25, 27–30, 32 and 40 (old forms; replaced in section 6). The full list is in section 0.
* **v2.0.1 and earlier.** The full changelog from v1.0 to v2.0.1 is in the previous revision of this file. Those entries describe the forced-deleverage design and are history; where they conflict with v2.1, v2.1 wins.

## 0. What v2.1 removes (explicit list)
| Removed | Was | Why it can go |
|---|---|---|
| Forced short buy-back (deleverage up-step) | Paced buys from P then C once a short failed health | Igor 2.1/N1: the buy is predictable and the float owner sells into it. Nothing replaces it; interest winds positions down instead |
| Forced long sell-down (deleverage down-step) | Paced sales of locked alpha once a long failed health | Mirror; same squeeze in reverse (bear raid) |
| `Deleveraging{since}` state, `DeleverageQueue`, `DeleverageFilled`, `DeleverageCursor`, `DeleverageBudget` (Up/Down) | Bookkeeping for the paced trades | No paced trades |
| Health trigger (`Equity_TWAP < equity_floor`, whole-book H_pos / V_pos) | Decided when to start forced trades | u drives the rate continuously; there is no line to cross |
| `equity_floor` (P5) | Trigger offset | No trigger. Igor N3 (make it bps of H) is obsolete |
| `min_interest_cover_blocks` (P5) | "C ≥ 1 day of interest" health test | Running out of C is simply the default |
| `exit_hysteresis_bps` (P5, v2.0.1) and the exit rule | Stopped Open ↔ Deleveraging flapping | No states to flap between |
| `pace_bps_per_block` (P7/L7) | Per-block price-impact cap on forced trades | No forced trades |
| `pause_twap_bps`, `pause_sma_bps` (P17/P22/L9) | Paused forced trades during pumps / crashes | No forced trades. The SMA30 ring stays (ranking P13, coverage ref P2) |
| `poke(who, netuid)`, `poke_long(who, netuid)` | Permissionless health check + deleverage step | Nothing to check between charges; defaults are found by the charge itself |
| Per-block deleverage work in `on_initialize` / `on_idle` and its weights | Ran the paced trades | Only interest charging (the existing `Due` queue) remains |
| Events `DeleverageStarted`, `DeleverageEnded`, `DeleverageStep`, `LongDeleverageStarted`, `LongDeleverageEnded`, `LongDeleverageStep` | | No deleverage |
| Error `PositionDeleveraging` | Blocked withdraw / increase while deleveraging | No such state |
| Invariants I5 (old), I9, I15 (except prune sale), I23 (old), I27 | Pace, pause, trigger and exit properties | Replaced by I5 (new), I23 (new) and I28 (section 5) |
| Igor fix switches N3 and N4 as separate fixes | | Obsolete: N3's parameter is gone; N4 is now the base rule |

What stays unchanged: opening coverage (with the 2.4 reference change), the P8/L8 per-block open limits, P10 minimum position, P11 locked proceeds, P12/P16 dereg settlement, P13 ranking, P18 minimum pool depth, P19 deposit, P20 withdraw buffer, P23 21M cap, the TWAP ring, the longs dereg sale (7.4), and every subtensor hook.

## 1. Mechanism, shorts (normative)

1. **Open** `open_short(netuid, mint q, collateral C, min_tao_out)` / `increase_short(netuid, extra_q, extra_C, min_tao_out)`: mint q alpha into the custody account and sell it into the pool, enforcing `min_tao_out`, the P8 per-block net-mint limit, P18 depth, P23 supply cap and the P16 zone tracking. The proceeds P stay locked in custody (P11). The **coverage rule** must hold after the sale:
   `C + P ≥ BuyCost(Q_book | pool pumped to Stress_eff × max(spot, SMA30_rank)) × q_pos / Q_book`
   * Q_book = `SyntheticOutstanding[netuid]` after the open; Stress_eff = Stress × (1 + slope × Q_book / SubnetAlphaIn).
   * **v2.1 (Igor 2.4):** the reference is `max(spot, SMA30_rank)`. The TWAP median is no longer part of the coverage reference; it is used only for interest (item 2) and minimum notional (P10).
   * An infinite whole-book buy-back (book ≥ pumped pool alpha) fails the open (`CoverageNotMet`).
   * An increase accrues and charges the position's interest at its pre-increase u first.
2. **Interest (v2.1).** Accrues per block and is charged every `charge_interval_blocks` (P15) through the existing `Due` queue, from C, 100% to the subnet's SubnetTAO (P4: pool-internal credit, not inflow or volume, I19).
   * **Debt value:** `N = q × TWAP_median` (TAO).
   * **Stretch:** `u = N / (C + P)`. At a launch-coverage open u is about 0.27–0.32 for a small short (C + P ≈ 3.2–3.7 × N). u reaches 1 when the position's holdings equal its debt at the TWAP.
   * **Rate (bps per year):** `rate = min(rate_cap_bps, apr_bps × u^k)`, computed exactly as
     `r ← apr_bps; repeat k times { r ← ⌊r × N / (C + P)⌋; if r ≥ rate_cap_bps { r ← rate_cap_bps; stop } }`
     in u128. If C + P = 0 the rate is the cap; if N = 0 it is 0. The floor at each step and the early stop make it deterministic and overflow-free (r ≤ cap ≤ 10^6, N < 2^64).
   * **Accrual:** `due += ⌊(N × rate × blocks + remainder) / (10,000 × blocks_per_year)⌋`, carrying the remainder, exactly as v2.0.1 but with `rate` in place of the flat APR.
   * **When the rate is read:** at every accrual. An accrual happens at each scheduled charge and at the start of every touch (`increase_short`, `add_collateral`, `withdraw_collateral`, `close_short`), using that block's TWAP_median, C and P **before** the touch changes them. So a top-up lowers the rate from that block on, never retroactively.
   * **Size independence:** u is a ratio of two quantities linear in position size, so scaling q, C and P together leaves the rate unchanged. Splitting a position into pieces funded pro rata leaves every piece's rate and the total interest unchanged (to rounding). Splitting with uneven funding can only raise the total while no piece is at the cap, because `N × (N/H)^k` is jointly convex and degree-1 homogeneous in (N, H).
   * **Why q × TWAP_median and not the whole-book H_pos** (the v1.9 health cost): H_pos priced a forced whole-book buy-back that no longer exists. What a stretched short costs holders is q of unbought supply, valued fairly at the TWAP. H_pos would let any third party raise everyone's rate by opening a large book (its slippage term), would read the live pool every accrual, and is not needed for split invariance because q × TWAP is already linear. The TWAP median needs a pump held for ≥ 36 h to move (P6).
3. **No term.** No expiry, no roll, no liquidation, and (v2.1) no forced trade of any kind while the subnet is live.
4. **No health state.** There is no trigger and no `Deleveraging` state. A position is Open until its owner closes it, it defaults (item 6), or the subnet dissolves (item 8). Nothing is evaluated between charges, so there is no `poke`.
5. **Close** `close_short(netuid, alpha, max_tao_in)`: accrue, then buy `alpha` from the pool and burn it, paying from P then C. Fails with `InsufficientFunds` if the cost exceeds P + C − pending interest.
   * **Full close** refunds the remaining C, P and the deposit.
   * **Partial close** is allowed if, after it, **either** the coverage rule (item 1) holds **or** the position's u is not higher than before (`q₂ × (C₁ + P₁) ≤ q₁ × (C₂ + P₂)`, both at the same TWAP). The second branch lets a stretched short shrink without topping up first; it can only lower its own rate and residual exposure.
6. **Default.** At a scheduled charge, if the charge leaves `C = 0` with interest still due, the position defaults in the same block:
   * P goes to SubnetTAO (pool-internal credit, not inflow or volume).
   * The unbought q moves from `SyntheticOutstanding` to `ResidualAlpha` (permanent supply until dissolution; counted in issuance, all_alpha, the halving schedule and P23).
   * The deposit is burned; unpaid interest is forgiven. Emits `ShortDefaulted{who, netuid, residual_alpha, proceeds_to_pool}`.
   * A default is never triggered by a touch, by spot, or by anyone else's call; only by the owner's own C running out.
   * **Note:** a short whose C runs out while it is in profit (u < 1) also defaults and forfeits P. The convex rate makes this slow (k = 4: at u = 0.5 the rate is 1/16 of apr), the owner can top up at any time, and UIs should warn. Charging P after C is a possible later refinement, not adopted (it delays P reaching the pool).
7. **Accounting:** unchanged from v2.0.1. Minted alpha is real once sold. `SyntheticOutstanding` + `ResidualAlpha` count in `get_alpha_issuance` (halving schedule, `root_proportion`, P23). The emission price is raw spot/EMA with no add-back (P14). The legacy owner-cut deduction subtracts `lending_alpha` only (v1.8).
8. **Dissolution:** unchanged from v2.0.1 (P12, P16, section 7.4): shorts settle at `r = min(T_flag, T_ema, T_A) / all_alpha` (zone-minted alpha at its own proceeds), the minted alpha is burned, the rest is refunded; stakers are paid r per alpha. Two items here are **open** for Jacob (section 8.1: payout path and proceeds floor).
9. **Minimum pool depth (P18):** opens and increases fail with `PoolTooShallow` below 1,000 TAO SubnetTAO (checked before and after). Existing positions keep close, top-up, withdraw within coverage, interest, default and dereg.
10. **21M alpha supply cap (P23):** an open or increase fails with `AlphaSupplyCap` if `get_alpha_issuance + q > 21,000,000 α`. Nothing else mints. Unchanged.
11. **Withdraw** `withdraw_collateral(netuid, tao)`: accrue, then allowed only if afterwards the coverage rule holds with a buffer of `withdraw_buffer_blocks` (P20) of interest **at the post-withdraw rate**: `C₂ + P ≥ need + N × rate(u₂) × buffer / (10,000 × blocks_per_year)`.
12. **Top-up** `add_collateral(netuid, tao)`: accrue at the current u, then add. Always allowed while the position exists and the subnet is not dissolving.

## 2. Parameters (launch values; every one is settable by root)

| # | Parameter | Value | Evidence and sensitivity |
|---|---|---|---|
| P1 | `Enabled` | false at genesis of the upgrade; root turns it on | Same practice as #3268 |
| P2 | Coverage rule | **Stress = 3.0** (`stress_bps` 30000), **slope 2** (`stress_slope_bps` 20000), ref = **max(spot, SMA30_rank)** (v2.1, Igor 2.4), whole book pro rata, proceeds counted | v1.0: 3× survives +200% held 7 days. v2.1: dropping the TWAP median lets an opposer take a 36× day-3 pump down to 17.9× / 14.1× / 12.7× with 30k / 100k / 300k TAO instead of 22.2× / 19.1× / 17.8× (Igor review, 2.4). Coverage is now also the main bound on default dilution, since nothing buys back |
| P3 | Base APR (**shared**) | **20%** (`apr_bps` 2000): the rate at u = 1 | With the curve, apr is the price of a position whose holdings just equal its debt. A launch-coverage short (u ≈ 0.3) pays far less (section 10) |
| P4 | Interest destination | **100% to the subnet's SubnetTAO**: no burn, no reserve. Not inflow or volume (I19). Not a parameter | Unchanged since v1.4 |
| P5 | **Interest curve** (v2.1, **shared**) | `rate = min(rate_cap_bps, apr_bps × u^k)`; launch **k = 2**, **cap = 100%/yr** (10,000 bps; provisional, section 10.3) | Section 10: sweep of k ∈ {1, 2, 4, 8} × cap ∈ {100%, 500%, 730% (2%/day), 2000%} against the squeeze, held pumps, a sustained rally, long crashes, the long drain and splitting |
| P6 | TWAP window | **3 days, median of 72 hourly samples** | A held pump must last ≥ 36 h to move the median, so it is the only price that moves u |
| P8 | Net mint per block (`open_limit_bps` 100) | Net minted alpha per block per subnet ≤ 1% of SubnetAlphaIn | Anti-flash only; unchanged |
| P9 | Supply cap | None. Bounded by P2 and P23 | Unchanged |
| P10 | Minimum position (**shared**) | **10 TAO**: shorts q × TWAP_median, longs D. One position per (coldkey, netuid, side) | Unchanged |
| P11 | Proceeds | Locked in custody | Unchanged. v2.1: P is also what reaches the pool at default |
| P12 | Dereg settlement | q × r, r = min(T_flag, T_ema, T_A)/all_alpha | Unchanged; payout path open (8.1) |
| P13 | Dereg ordering | 30-day SMA of raw spot, ring lifecycle per section 9 | Unchanged |
| P14 | Emission price | Raw (no add-back) | Unchanged |
| P15 | Interest cadence (**shared**) | Accrue per block, charge every 360 blocks, `Due` queue | Unchanged; v2.1 reads the rate at each accrual |
| P16 | Dereg-zone settlement | Zone-minted alpha settles at its own proceeds | Unchanged |
| P18 | Minimum pool depth (**shared**) | 1,000 TAO, before and after | Unchanged |
| P19 | Deposit (**shared**) | 0.1 TAO, refunded on full close, burned on default | Unchanged |
| P20 | Withdraw buffer (**shared**) | Coverage + **7 days** of interest at the post-withdraw rate | v2.1: the rate is the curve's, not the flat APR |
| P23 | 21M alpha supply cap | Protocol constant | Unchanged |
| ~~P5 trigger~~, ~~P7~~, ~~P17~~, ~~P22 pause~~ | *(removed v2.1)* | | Section 0 |

### 2.1 Root-settable parameters (v2.1)
All four groups are root-only. Each setter replaces the whole struct, validates every bound, fails `InvalidParams` on any violation, writes a `Pending…` value applied in the next block's `on_initialize`, and emits `<Group>ParamsSet{old, new}`. No setter re-checks or mutates a position (I26). Coverage is checked only at open, increase, partial close and withdraw. **v2.1:** a change to `apr_bps`, `rate_k` or `rate_cap_bps` applies to every position from its next accrual; this is the only way a setter reaches open positions, and it can only change what they pay, never close them.

| Param | Storage → field | Setter | Bounds | Launch |
|---|---|---|---|---|
| Shorts on (P1) | `Enabled` | `set_enabled(bool)` | — (false→true re-seeds rank rings, I25) | false |
| Longs on (L1) | `LongsEnabled` | `set_longs_enabled(bool)` | — | false |
| Base APR, both sides (P3) | `LendingParams.apr_bps` | `set_lending_params` | 1 ≤ v ≤ 10,000 | 2,000 |
| Curve exponent, both sides (P5) | `LendingParams.rate_k` (u8) | `set_lending_params` | 1 ≤ v ≤ 8 | 2 |
| Rate cap, both sides (P5) | `LendingParams.rate_cap_bps` (u32, per year) | `set_lending_params` | `apr_bps` ≤ v ≤ 1,000,000 (10,000%/yr ≈ 27%/day) | 10,000 (100%/yr; provisional, section 10.3) |
| Charge cadence (P15) | `LendingParams.charge_interval_blocks` | `set_lending_params` | 1 ≤ v ≤ 7,200 | 360 |
| Min pool depth (P18) | `LendingParams.min_pool_tao` | `set_lending_params` | 100 TAO ≤ v ≤ 100,000 TAO | 1,000 TAO |
| Min position (P10) | `LendingParams.min_notional` | `set_lending_params` | 0.1 TAO ≤ v ≤ 10,000 TAO | 10 TAO |
| Deposit (P19) | `LendingParams.deposit` | `set_lending_params` | 0 ≤ v ≤ 10 TAO | 0.1 TAO |
| Withdraw buffer (P20) | `LendingParams.withdraw_buffer_blocks` | `set_lending_params` | 7,200 ≤ v ≤ 216,000 | 50,400 (7 days) |
| TWAP samples (P6) | `LendingParams.twap_samples` | `set_lending_params` | 3 ≤ v ≤ 72 | 72 |
| TWAP interval (P6) | `LendingParams.twap_interval_blocks` | `set_lending_params` | 50 ≤ v ≤ 1,200 | 300 |
| Short stress (P2) | `ShortParams.stress_bps` | `set_short_params` | 10,000 ≤ v ≤ 100,000 | 30,000 |
| Short book slope (P2) | `ShortParams.stress_slope_bps` | `set_short_params` | 0 ≤ v ≤ 100,000 | 20,000 (dial open, 8.1) |
| Net mint per block (P8) | `ShortParams.open_limit_bps` | `set_short_params` | 1 ≤ v ≤ 1,000 | 100 |
| Long stress (L2) | `LongParams.stress_bps` | `set_long_params` | `stress_floor_bps` ≤ v ≤ 100,000 | 20,000 |
| Long stress floor (L2) | `LongParams.stress_floor_bps` | `set_long_params` | 10,000 ≤ v ≤ `stress_bps` | 15,000 |
| Lift cap (L5) | `LongParams.lift_cap_bps` | `set_long_params` | 1 ≤ v ≤ 5,000 | 1,000 |
| Gross lift per block (L8) | `LongParams.open_limit_bps` | `set_long_params` | 1 ≤ v ≤ 1,000 | 100 |
| Dereg zone (P16) | `DeregParams.zone_bps` | `set_dereg_params` | 10,000 ≤ v ≤ 30,000 | 12,500 |
| T_ema window (P12) | `DeregParams.pool_tao_ema_blocks` | `set_dereg_params` | 300 ≤ v ≤ 216,000 | 7,200 |
| Rank SMA window (P13) | `DeregParams.rank_window_days` | `set_dereg_params` | 7 ≤ v ≤ 30 | 30 |
| Longs per subnet (7.4) | `DeregParams.max_long_positions_per_subnet` | `set_dereg_params` | current max count ≤ v ≤ 1,024 | 256 |

**Count:** 21 tunables (`LendingParams` 10, `ShortParams` 3, `LongParams` 4, `DeregParams` 4) plus the two enable flags. v2.0.1 had 25. Re-seeding rules for the TWAP and rank windows are unchanged (resize and fill with the current TWAP median / published SMA30). Not settable: the 21M cap, the interest destination, `MaxLongPositionsCeiling` (1,024).

**Why the bounds:** `rate_k ≤ 8` keeps the iteration bounded and benchmarkable; `rate_cap_bps ≥ apr_bps` keeps a u ≥ 1 position paying at least the base rate; the cap's upper bound (27%/day) means even a hostile setting drains a position over days, not one charge. `apr_bps ≥ 1` is still needed: interest is the only thing that ends a position its owner abandons.

## 3. Branch impact (`lending-v2-synthetic`)
* **Landed and kept:** pure math core, params skeleton, price rings and migration prefill, positions both sides (open / increase / close / add / withdraw), interest `Due` queue, benchmarks and weights for those calls, SDK.
* **Deleted from the plan:** task 5 (forced deleverage, `poke` / `poke_long`, exit rule, pace budgets, pause bands, `DeleverageQueue` and its weights). What survives of it is the default path, which runs inside the interest charge.
* **Changed:**
  * `LendingParams`: remove the six fields in section 0, add `rate_k: u8`, `rate_cap_bps: u32`; bounds per 2.1.
  * Interest: `accrue(pos)` computes `rate_bps` per section 1 item 2 (shorts) / 7.1 item 4 (longs) and passes it where the flat `apr_bps` was. Every touch calls `accrue` before mutating. Pure function `rate_bps(apr, k, cap, num, den) -> u32` in `math/`, with golden vectors from `model/v21`.
  * Charge: after `min(due, C)` is taken, if `C == 0 && due > 0` call `default_short` / `default_long` in the same `Due` step. Weight: one extra branch per charge, plus one swap for a long default; `MaxChargesPerBlock` keeps bounding the per-block work.
  * `close_short` / `close_long` partial: coverage **or** non-increasing u.
  * Coverage reference: `reference_price = max(spot, rank SMA30)`; drop the TWAP median from it.
  * Longs: virtual long (7.1 items 1 and 8), gross L8.
  * Position struct: drop `state` and `Deleveraging{since}`; a stored position is Open.
* **Unchanged:** subtensor hooks (custody guards, dissolution, coldkey swap, issuance), the prune-time long sale, dereg settlement, rank rings.

## 4. Storage, extrinsics, events, errors (both sides)

**Storage (pallet_lending)**
* `Enabled: bool`, `LongsEnabled: bool`
* `LendingParamsStore: LendingParams { apr_bps, rate_k, rate_cap_bps, charge_interval_blocks, min_pool_tao, min_notional, deposit, withdraw_buffer_blocks, twap_samples, twap_interval_blocks }`, `ShortParamsStore: ShortParams { stress_bps, stress_slope_bps, open_limit_bps }`, `LongParamsStore: LongParams { stress_bps, stress_floor_bps, lift_cap_bps, open_limit_bps }`, `DeregParamsStore: DeregParams { zone_bps, pool_tao_ema_blocks, rank_window_days, max_long_positions_per_subnet }`, each with a `Pending…` twin.
* `Positions: Map<(AccountId, (NetUid, Side)) → Position>` (one map, as built), where
  * `ShortPosition { custody, minted, collateral, proceeds, deposit, zone_minted, zone_proceeds, opened_at, last_accrued, interest_due, interest_remainder, terms_version }`
  * `LongPosition { custody, debt, locked_alpha, collateral, deposit, opened_at, last_accrued, interest_due, interest_remainder, terms_version }`
  * v2.1: no `state` field. A stored position is Open.
* `SyntheticOutstanding: Map<NetUid → u64>`, `ResidualAlpha: Map<NetUid → u64>`
* `LongLiftedTao`, `LongLockedAlpha: Map<NetUid → u64>`; `LongFlowTotals: Map<NetUid → { lifted, bought_alpha, sold_alpha, repaid, interest_to_pool, shortfall }>`
* `TwapRing: Map<NetUid → (BoundedVec<u64, 72>, cursor, last_sample_block)>`; `RankRing` (subtensor, section 9)
* `BlockNetMinted: Map<NetUid → (block, i128)>` (P8), `LongBlockGrossLifted: Map<NetUid → (block, u128)>` (L8, gross since v2.1)
* Kept: `Due*`, `InterestRetry*`, `Dissolutions*`, `OpenByNetuid`, `PositionCount`, `TotalPositions`, `LongPositionCount`, `EscrowOwner`, `StrandedCustody`
* **Removed v2.1:** `DeleverageQueue`, `DeleverageFilled`, `DeleverageCursor`, `DeleverageBudget`, the `Deleveraging{since}` state.

**Extrinsics**
* Shorts: `open_short(netuid, mint_alpha, collateral, min_tao_out)`, `increase_short(netuid, extra_mint, extra_collateral, min_tao_out)`, `add_collateral(netuid, tao)`, `withdraw_collateral(netuid, tao)`, `close_short(netuid, alpha, max_tao_in)`
* Longs: `open_long(netuid, lift_tao, collateral, min_alpha_out)`, `increase_long(netuid, extra_lift, extra_collateral, min_alpha_out)`, `add_long_collateral(netuid, tao)`, `withdraw_long_collateral(netuid, tao)`, `close_long(netuid, alpha, min_tao_out)`
* Root: `set_enabled`, `set_longs_enabled`, `set_lending_params`, `set_short_params`, `set_long_params`, `set_dereg_params`
* **Removed v2.1:** `poke`, `poke_long`.

**Events**
* `ShortOpened{who, netuid, minted, proceeds, collateral}`, `ShortIncreased{..}`, `CollateralAdded{..}`, `CollateralWithdrawn{..}`, `ShortClosed{who, netuid, burned, tao_spent, refund}`
* `LongOpened{who, netuid, lifted, alpha, collateral}`, `LongIncreased`, `LongClosed{who, netuid, alpha_sold, tao_out, repaid, refund}`, `LongFlow{kind, tao, alpha}`
* `InterestCharged{who, netuid, side, to_pool, rate_bps}` (v2.1: carries the rate used; one event for both sides, replacing `LongInterestCharged`)
* `ShortDefaulted{who, netuid, residual_alpha, proceeds_to_pool}`, `LongDefaulted{who, netuid, alpha_sold, tao_out, repaid, refund, shortfall}`
* `ShortSettledOnDissolution{who, netuid, price, paid}`, `LongBookSoldOnPrune{netuid, alpha, tao_out}`, `LongSettledOnDissolution{..}`
* `LendingParamsSet`, `ShortParamsSet`, `LongParamsSet`, `DeregParamsSet`, `EnabledSet`, `LongsEnabledSet`
* **Removed v2.1:** `DeleverageStarted`, `DeleverageEnded`, `DeleverageStep`, `LongDeleverageStarted`, `LongDeleverageEnded`, `LongDeleverageStep`.

**Errors**
* `Disabled`, `LongsDisabled`, `SubnetNotEligible`, `BelowMinNotional`, `CoverageNotMet`, `LongCoverageNotMet`, `SlippageExceeded`, `OpenImpactTooHigh`, `LongOpenLimit`, `LiftCapExceeded`, `NoPosition`, `PositionExists`, `OppositeSideOpen`, `TooManyLongPositions`, `InsufficientFunds`, `CloseInsolvent`, `WithdrawBreaksCoverage`, `WithdrawBufferBreached`, `LongWithdrawBreaksCoverage`, `DissolutionInProgress`, `InvalidParams`, `LongStressBelowFloor`, `CustodyUnavailable`, `LockedAlpha`, `Overflow`, `AlphaSupplyCap`, `PoolTooShallow`
* **Removed v2.1:** `PositionDeleveraging`.

## 5. Invariants (assert in try-state and in tests)
* **I1** Σ `minted` over a subnet's short positions == `SyntheticOutstanding[netuid]`.
* **I2** Alpha is created only by short open/increase and destroyed only by short close and dissolution. A default moves q from SyntheticOutstanding to ResidualAlpha and creates or destroys nothing.
* **I3** Custody TAO == Σ (collateral + proceeds + deposits) over both sides. No balance goes negative; the protocol never owes TAO.
* **I4** Every open, increase and withdraw ends with the coverage rule satisfied for the acting position (whole book pro rata, ref max(spot, SMA30)). A partial close ends with coverage satisfied **or** the position's u not higher than before. Other positions are never re-checked.
* **I5** (v2.1) After every scheduled charge, no position has `C == 0` with interest due: it defaulted in that step. Charged interest ≤ C at every charge.
* **I6** `emission_alpha_price == spot`.
* **I7** The halving schedule and `root_proportion` use `get_alpha_issuance`, which includes SyntheticOutstanding and ResidualAlpha.
* **I8** Dissolution settles each short exactly once; pot ≥ Σ staker payouts; each q·r ≤ C + P; zone alpha at zone_proceeds ≤ P.
* **I11** No open or increase on either side succeeds under P18. No close, top-up, withdraw, charge, default or dereg step fails because of P18.
* **I12** Σ D == `LongLiftedTao`, Σ A == `LongLockedAlpha` == long custody stake, per netuid.
* **I13** (v2.1, virtual long) A long open is one on-curve buy against the live pool: ΔSubnetTAO == L − fee, ΔSubnetAlphaIn == −A, and `LongLiftedTao += L`. Real pool TAO is `SubnetTAO − LongLiftedTao` at all times. No lifted TAO reaches a user account.
* **I14** At every long open and increase: `LongLiftedTao ≤ lift_cap × SubnetTAO` (pre-open), gross lifted in the block ≤ L8, and L2 coverage holds for the opener.
* **I16** A long default or prune shortfall writes D off `LongLiftedTao` and SubnetTAO exactly once; Σ write-offs == `LongFlowTotals.shortfall`.
* **I17 / I24** Prune: one-shot long sale before T_A and r, then B → D → E → F; conservation as v2.0.1.
* **I18 / I19** Lending legs never touch `SubnetTaoFlow`, `SubnetProtocolFlow` or `SubnetVolume`. ΔSubnetTAO from interest == Σ interest charged, both sides.
* **I21** `get_alpha_issuance(netuid) ≤ 21,000,000 α` after every block.
* **I23** (v2.1, rate) For every position, the rate charged equals `rate_bps(apr, k, cap, num, den)` on that block's TWAP_median, C, P (and A, D for longs) read before any mutation in the step, and `rate ≤ rate_cap_bps`. Splitting a position into N positions with pro-rata q (or D, A), C and P gives every piece the same rate and the same total interest within N rao per charge.
* **I25** Rank rings full and current, no EMA or partial-mean read (unchanged).
* **I26** Params within bounds; changes only via `on_initialize` from `Pending…`; no setter mutates a position.
* **I28** (v2.1, no forced trades) While a subnet is not dissolving, the pool's reserves change because of the lending pallet only inside an extrinsic signed by the position's owner (open, increase, close) or inside a default step, and the only reserve changes a charge or default makes are: interest credited to SubnetTAO, P credited to SubnetTAO (short default), and one sale of the defaulting long's own locked alpha plus the write-off of its unpaid D (long default). No other buy or sell happens on anyone's behalf.
* **I10** When disabled: no opens or increases; close, top-up, withdraw and charges still work.

## 6. Test plan

**Unit tests (pallet mock)**
* `rate_bps` golden vectors from `model/v21` (`gen_golden.py`): k = 1..8; u from 0 to 100; den = 0; N = 0; the cap reached at each step; agreement with the model to the bps.
* Accrual: rate read before the touch for every touch; a top-up lowers the rate only from that block; a root change to apr / k / cap applies from the next accrual of every position; remainder carry at the minimum position.
* Default at charge: C exactly covering the due (no default), short by 1 rao (default), short default moves q to ResidualAlpha and P to SubnetTAO, deposit burned; long default sells A once, lender first, surplus to owner, shortfall written off; no default from a touch.
* Partial close: coverage branch, de-risk branch (u non-increasing), and a close that raises u fails `CoverageNotMet`.
* Withdraw to exactly the P20 buffer at the post-withdraw rate; 1 rao more fails.
* Coverage reference max(spot, SMA30): golden vectors; TWAP median no longer moves the open check.
* Virtual long: ΔSubnetTAO == L − fee at open; repay retires credit; I13; gross L8 (a close in the same block does not free room).
* Split: 1 vs 10 vs 100 pro-rata pieces pay the same rate and the same total interest within N rao per charge (I23).
* No forced trade: with any price path and any parameter values, no reserve change outside I28's list (property test).

**Mainnet-clone e2e (Finney clone).** Expected values come from `model/v21` at the launch k and cap. All replace the v2.0.1 list; old numbers 2, 7, 12, 13, 25, 27–30, 32 and 40 are retired with deleverage.
1. **Green path** on SN91, SN101, SN112, SN128 and SN51, both sides: open, increase, top-up, withdraw, partial close, full close with refund. Interest-to-pool and issuance reconciliations.
2. **Held pump, honest short:** a 1% short at 1×, 2× and 3× minimum cover; the team pumps 2×, 3× and 6× and holds 3, 10 and 30 days. Expect the rate and interest paid from section 10 to within rounding, no trade by the pallet, and a default only where section 10 predicts one.
3. **Squeeze (Igor 2.1):** SN29, 18% outside book, team at 95% of stake pumps 6× and holds 30 and 60 days, against a control that pumps to the same price with no short. Expect the premium from section 10.
4. **Sustained rally:** +3%/day for 200 days with an 18% book. Expect the rate path, the default day, P to the pool and residual from section 10; residual counted in issuance and the halving schedule.
5. **Defend-and-hold:** the team buys back to the open price and holds. The shorter closes on day 1 and day 7. SubnetTAO up by exactly the interest charged.
6. **De-risking partial close:** pump a short to u > 1, then close half without a top-up. Expect success and a lower rate.
7. **Top-up timing:** a stretched short tops up just before a charge. Expect the elapsed blocks charged at the pre-top-up rate.
8. **Root changes:** raise apr, k and cap mid-flight. Expect new rates from each position's next accrual, no position closed, `LendingParamsSet` once.
9. **Long crash, no prune:** a 10% lift long, −14%/day for 6 days then hold; and −30%/day for 4 days then hold. Expect the long rate path, default (or not) and lender shortfall from section 10, and the default sale as one swap.
10. **Long crash into prune** (v2.0.1 e2e 37 paths): one-shot sale, lender first; shortfall as section 10.
11. **Long drain (Igor 2.2):** cap-size long plus the trader's own spot round trip. Expect a loss for the trader (model −0.025% of pool TAO per round) and no gain from closes freeing L8 room.
12. **Dereg griefing:** pin SN79 at 0.5 × L for 30 days with and without a dip-buyer; then SN29 at 0.3 × L. Settlement per P12/P16.
13. **Dereg snipe:** SN40, coverage-maximum short one block before a registration. Settles at zone_proceeds.
14. **Multi-account open:** 10 coldkeys each mint 0.2% of SubnetAlphaIn in one block; the 6th fails `OpenImpactTooHigh`. Same for longs with gross L8.
15. **Shallow pool (P18):** sell SubnetTAO below 1,000 with an open short and long. Opens fail; close, top-up, withdraw, charges and default work.
16. **21M cap (P23):** S = 21M − 1,000 α; mint 1,001 fails, 1,000 succeeds.
17. **Split:** the same 18% short book as 1, 10 and 100 coldkeys with pro-rata funds under a held 5× pump. Same rate path and same total interest (I23).
18. **Rank rings:** upgrade with `Enabled = false`, prefill, enable re-seed (I25).
19. **Root setters:** every setter's bounds, pending apply, event (I26).
20. **Longs and shorts in one block:** a lift-buy followed by a mint, and the reverse; `OppositeSideOpen`.
21. **Interest-pump grief (new, section 10):** a whale pumps 3× and 6× and holds 3, 10 and 30 days against a book of honest 1×–3×-cover shorts. Expect the interest the shorts pay, the whale's cost of holding, and no default where section 10 predicts none.

## 7. Longs (normative)

### 7.1 Mechanism
1. **Open** `open_long(netuid, lift_tao L, collateral C, min_alpha_out)` (v2.1: **virtual long**, Igor 2.2). In one extrinsic:
   * C moves from the owner's free balance to the position custody account.
   * L is swapped into the pool as an ordinary buy against the live reserves (normal swap math and fee), enforcing `min_alpha_out`: SubnetTAO += L − fee, SubnetAlphaIn −= A, SubnetAlphaOut += A. No account is debited for L; it is a pool-internal credit recorded as the lender claim `D = L` in `LongLiftedTao`. Real pool TAO is `SubnetTAO − LongLiftedTao` (I13).
   * A is credited as **locked** stake of the position custody coldkey on `PalletHotkey` (no unstake, move, transfer or swap except by the pallet; never released to the borrower).
   * Why virtual: v2.0.1 lifted L out of SubnetTAO before the buy, so the buy priced against a smaller pool and a trader's own spot round trip around the open and close drained 0.72% of pool TAO per round at 1.5× (Igor 2.2; 13.4% in 20 rounds). Pricing the buy on the live pool turns the round trip into a loss (−0.025%). Cost: the long's entry impact is a normal buy, about 2× v2.0.1's (+21% at the 10% cap instead of +11%).
2. **Open checks:** `LongsEnabled`; lift cap `LongLiftedTao + L ≤ lift_cap × SubnetTAO` (pre-open); **gross** lift per block ≤ L8 (v2.1: closes in the same block do not free room); P10; P18 before and after; `max_long_positions_per_subnet`; coverage (item 3) after the buy with the whole long book pro rata.
3. **Coverage (L2):** `C ≥ D_pos − SellProceeds(LongLockedAlpha | pool dumped to ref/m) × A_pos / LongLockedAlpha`, ref = min(spot, TWAP_median), m = `LongParams.stress_bps` (2.0, floor 1.5). Unchanged.
4. **Interest (v2.1, mirror of section 1 item 2):** on the debt `D` (TAO), at `rate = min(cap, apr × u^k)` with
   `u = D / (A × TWAP_median + C)`,
   the same shared `apr_bps`, `rate_k`, `rate_cap_bps`, the same iteration (num = D, den = A × TWAP_median + C), the same accrual and charge schedule and touch rule, 100% to SubnetTAO. A long is stretched by a fall in the alpha it holds, as a short is by a rise.
5. **No health state, no forced sale.** There is no trigger, no `Deleveraging`, no paced sell-down and no `poke_long`.
6. **Close** `close_long(netuid, alpha, min_tao_out)`: accrue, sell alpha from A (ordinary sell against the live pool). Proceeds S repay D first (retiring that much of the pool credit: SubnetTAO −= S, of which min(S, D) is retired and the rest paid to the owner). Full close needs `S + C − pending ≥ D` (`CloseInsolvent` otherwise) and refunds C, the surplus and the deposit. Partial close: coverage holds after, **or** u does not rise (`D₂ × (A₁·TWAP + C) ≤ D₁ × (A₂·TWAP + C)`).
7. **Default (v2.1).** At a scheduled charge, if the charge leaves `C = 0` with interest due:
   * the position's locked A is sold into the live pool in **one** swap (normal sell, fee, no `min_tao_out`);
   * proceeds S pay, in order, the interest due (credited to SubnetTAO) and then D (retired);
   * any surplus S goes to the owner; any unpaid D is a **lender shortfall**: written off `LongLiftedTao` and SubnetTAO, added to `LongFlowTotals.shortfall` (I16); unpaid interest beyond S is forgiven; the deposit is burned. Emits `LongDefaulted{..}`.
   * **How this interacts with the virtual long:** the sale removes S from SubnetTAO and the write-off removes D − S, so after an underwater default the reserves are `(T − D, X + A − fee)`: exactly the reversal of the open's reserve change, whatever the price did in between. The "lender" is the pool's own credit, so a shortfall is not a cash payment to anyone; holders bore it through the price while the credit sat in SubnetTAO, and anyone who sold into the inflated pool kept that TAO. The size is bounded by the lift cap (10% of SubnetTAO).
   * **The one remaining market trade on a live subnet** is this default sale of the defaulter's own alpha. It happens at a predictable block (C runs out on schedule), so it can be front-run like the prune sale (Igor N5, accepted). The convex rate makes it rare: a long reaches default only after paying out all of C, and its rate is low while it is solvent.
8. **Accounting:** `LongLiftedTao` = Σ D is the pool's credit; `LongLockedAlpha` = Σ A is ordinary issued alpha (counts in issuance and all_alpha, held on the unregistered `PalletHotkey`, earns nothing). Lending legs are pool-internal: no `SubnetTaoFlow`, no protocol flow, no volume (I18); reconciled through `LongFlow` events and `LongFlowTotals`.
9. **Other calls:** `add_long_collateral` (accrue first), `withdraw_long_collateral` (accrue first; coverage + P20 buffer at the post-withdraw rate). One position per (coldkey, netuid, side); `OppositeSideOpen` for both sides on one netuid.
10. **Dereg:** one-shot lender-first sale of the whole long book inside the prune step (7.4), unchanged except that interest accrued up to the prune uses the curve.

### 7.2 Parameters
| # | Parameter | Value | Note |
|---|---|---|---|
| L1 | `LongsEnabled` | false | Independent of `Enabled` |
| L2 | `stress_bps` / `stress_floor_bps` | 20,000 / 15,000; ref min(spot, TWAP_median) | Unchanged |
| L3/L4 | APR, destination | Shared P3/P5 curve, 100% to SubnetTAO | |
| L5 | `lift_cap_bps` | 1,000 (10% of SubnetTAO) | v2.1: also bounds the default reversal |
| L6 | TWAP | Shared ring | |
| L8 | `open_limit_bps` | **Gross** lifted per block ≤ 1% of SubnetTAO | v2.1 (Igor 2.2): closes do not free room, so open-close loops can't repeat within a block |
| L10–L12 | Min notional, deposit, withdraw buffer | Shared P10, P19, P20 | |
| L14 / L15 | Ranking / emission price | Raw | Accepted (v1.5) |
| ~~L7~~, ~~L9~~, ~~L13~~ | *(removed: pace, pause, unwind length)* | | |

### 7.3 Dissolution (unchanged from v2.0.1 section 7.4)
Inside `start_dissolution`, before T_A and r are read: accrue every long to this block (curve rate), sell the whole long book in one swap into the live pool, split proceeds pro rata, settle each position lender first (interest, then D, from its share then C), write off any gap (I16), refund the rest. Then phase B fixes `r = min(T_flag, T_ema, T_A) / all_alpha`, phase D settles shorts at r (zone alpha at zone proceeds), phase E pays stakers r per alpha, phase F takes the leftover path. At most `max_long_positions_per_subnet` longs; phase A is one swap plus ≤ 256 settlements, charged up front. Two dissolution items are open (8.1).

## 8. Decisions

### 8.1 Open decisions for Jacob
1. **Rate launch values (P5).** Section 10 recommends k and cap with the reasoning and the trade-off against the interest-side squeeze.
2. **Dissolution payout path (Igor 2.8/2.9).** Proposed: stakers keep the v497 distribution of the whole pot (pool TAO + Σ q·r + long repays); r is used only to settle shorts; the leftover never reaches the legacy owner refund. Model: about 713k TAO of leftover across older subnets would otherwise flow to legacy owner refunds (SN29: 3,845 of 7,195). Needs a wording change and a change to the subtensor dissolution path.
3. **Short proceeds floor at dissolution (Igor 2.7 fix).** Proposed: at dissolution a short pays `max(q·r, its unburned mint proceeds)`, extending the P16 zone rule to all mints. Modeled (10k TAO attacker, 18 small subnets): the attacker's result goes from +607 to −10 TAO (median). Not adopted in v2.1 pending this decision.
4. **Griefing near the prune (Igor 2.7 residue).** Even with item 3, 10k TAO of shorts can push a subnet with ≤ 7.2k TAO in the pool into the prune for about 10 TAO of interest (18 of 101 subnets). Options, none modeled: (a) prune ranking on a price that excludes synthetic sell pressure; (b) no new mints in the zone plus a book cap near the bottom of the ranking; (c) accept.
5. **Conviction bar (Igor 2.6).** A borrowed short can be bought back and locked to reach the 18% conviction bar on subnets ≥ 1 year old: feasible on 7 (130-day lock) or 26 (260-day lock) of 88 subnets at q ≤ 0.30 × pool alpha. Proposed subtensor change: bar = 18% × eligible + a slow average of SyntheticOutstanding. Igor's alternative: count only conviction above 9%. Or accept on the exposed subnets.
6. **Stress slope dial (Igor 2.3).** A first mover's book raises later openers' coverage (outsider C per TAO notional 2.1 → 6.4 / 13.9 / 101 behind a 20 / 30 / 40% book). Slope 0 halves it (24.5% book: 8.76 → 4.39) at the cost of coverage depth. Launch keeps slope 2.

### 8.2 Igor's v2.0.1 findings: status in v2.1
| # | Finding | v2.1 status |
|---|---|---|
| 2.1 | Squeeze through the forced buy-back | **Closed by design** (no forced buy-back). Interest-side variant measured in section 10 |
| N1 | Large book lowers others' triggers | **Closed by design** (no trigger). The rate uses each position's own u, so another book does not move it |
| 2.2 | Long + spot round trip drains the pool | **Fixed**: virtual long + gross L8 |
| 2.3 | First-mover lock-out | Open dial (8.1 item 6) |
| 2.4 | Unopposable pump | **Fixed (partly)**: coverage ref max(spot, SMA30) |
| 2.5 | Staked short | Accepted and documented. **Changed by v2.1:** see section 10 (a healthy short pays less than the flat 20%) |
| 2.6 | Lock-conviction ownership | Open (8.1 item 5) |
| 2.7 | Undefended-subnet kill | Open (8.1 items 3 and 4) |
| 2.8/2.9 | Dissolution leftover to owner refund | Open (8.1 item 2) |
| N2 | Halving pulled forward | Accepted and documented |
| N3 | `equity_floor` = 1,000 TAO forces 60% of positions | **Obsolete**: parameter removed; no setter can force-close anything |
| N4 | Paused short with C = 0 never ends | **Obsolete as a fix; now the base rule** (default at C exhaustion) |
| N5 | Front-running the prune sale | Accepted and documented; v2.1 adds the same pattern for a long's default sale (7.1 item 7) |

### 8.3 Decided and closed (record)
Unchanged from v2.0.1 8.2 except where v2.1 replaces it: minimum pool depth 1,000 TAO (both sides); interest 100% to SubnetTAO; dereg window 30-day SMA with prefilled rings; dereg payout r with P16; no supply cap; 21M cap P23; P24 removed; stress 3×; same coldkey on both sides forbidden; slice-built books (old 8.6) is moot for triggers since there are none, and for coverage it stays as recorded. **Replaced by v2.1:** flat APR (now the curve), split-resistant health (no health; rate split invariance is I23), P22 SMA30 buy ceiling (no buys), forced-close exit rule.

## 9. Rank ring lifecycle (normative, unchanged from v1.9.1)
1. **Runtime upgrade:** a migration fills every live non-root subnet's ring with 30 daily means equal to its current `SubnetMovingPrice`, publishes `RankPrice`, and tags it with the current `RankGeneration`, whether or not lending is enabled.
2. **Registration** (new or re-registered netuid): seed the same way.
3. **Enable** (`set_enabled` false→true) and any other `RankGeneration` bump: re-seed every live subnet's ring from its current `SubnetMovingPrice` in the same call.
4. **Disable:** no reset; rings keep sampling; ranking keeps reading them.
5. **No fallback anywhere:** `dereg_rank_price`, `prune_rank_price` and `rank_price` read only the published ring SMA; a missing or wrong-generation ring is a try-state failure (I25).
6. A `rank_window_days` change re-seeds from the published SMA30, never the EMA.


## 10. v2.1 evidence: reference model `model/v21` (provisional, sweeps still finishing)
Model: `model/v21` = v2.0.1 integer-rao oracle with Igor's bundle C pinned (no forced buy-back, C-exhaust default, coverage ref max(spot, SMA30), virtual long, gross L8), the curve of section 1 item 2 / 7.1 item 4, the long default sale of 7.1 item 7 and the partial-close rule. Harness `model/v21_scen.py`, jobs `model/v21_jobs.py`, results `model/results_v21/<config>.json`, table `model/results_v21/table.md`. `k = 0` is the flat-20% reference (bundle C as Igor ran it). APR 20% throughout. Squeeze metric as Igor's: team premium over a control that pumps to the same absolute price with no short; team holds 95% of stake, no outside sellers (β = 0, worst case). Igor 28-subnet subset; 6× pump; 18% outside book. Rate golden vectors: `model/results_v21/rate_golden.json` (1,344 vectors, 0 mismatches vs the model).

### 10.1 Results per k / cap (APR 20%)
| k | cap /yr | Squeeze 6×/30d: median / max / #>0 of 28 | Squeeze 6×/60d: median / max / #>0 of 28 | SN29 grid (4/6/10× × 30/60d × 5/18% book): cells > 0 of 12, max | Honest 1%-short at min cover, 6× pump: paid % of open notional d3 / d10 / d30 | Same, 2× pump, d30 | 3%/day rally, 18% book: default day (median of 6) | Residual at default (% of issuance, median) |
|---|---|---|---|---|---|---|---|---|
| 0 (flat) | — | −735 / −283 / 0 | −622 / −260 / 0 | 0, −54 | 0.6 / 2.9 / 10.6 | 3.3 | 2 of 6 default, ~199 | 0 (4 of 6 not yet defaulted) |
| 1 | 100% | −747 / −291 / 0 | −659 / −230 / 0 | 2, +508 | 0.9 / 5.5 / 23.5 | 2.1 | 166 | 5.6 |
| 1 | 500–2000% | same | same | 2, +1,079 | same | 2.1 | 145–150 | 5.6 |
| 2 | 100% | −757 / −297 / 0 | −684 / −107 / 0 | 4, +831 | 1.7 / 10.9 / 48.8 | 1.4 | 163 | 5.6 |
| 2 | 500% | same | same | 4, +1,361 | 1.7 / 10.9 / 59.7 | 1.4 | 134 | 5.6 |
| 2 | 730% (2%/day) | same | same | 4, +1,373 | same | 1.4 | 131 | 5.6 |
| 2 | 2000% | same | same | 4, +1,381 | same | 1.4 | 128 | 5.6 |
| 4 | 100% | −771 / −302 / 0 | −715 / +84 / 2 | 4, +1,373 | 2.3 / 14.1 / 52.6 | 0.6 | 163 | 5.6 |
| 4 | 500% | same | −715 / +1,006 / 3 | 6, +7,122 | 6.1 / 52.9 / defaults before d30 (3 of 3 subnets) | 0.6 | 126 | 5.6 |
| 4 | 730% | same | −715 / +1,269 / 3 | 6, +7,289 | 6.1 / 55.7 / defaults | 0.6 | 121 | 5.6 |
| 4 | 2000% | same | −715 / +1,865 / 3 | 6, +7,484 | 6.1 / 55.7 / defaults | 0.6 | 113 | 5.6 |
| 8 | 100% | −782 / −306 / 0 | −647 / +170 / 3 | 5, +1,616 | 2.3 / 14.1 / 52.6 | 0.1 | 163 | 5.6 |
| 8 | 500% | −782 / −306 / 0 | −572 / +2,844 / 6 | 7, +7,883 | 11.7 / 70.5 / defaults | 0.1 | 122 | 5.6 |
| 8 | 730%, 2000% | *running* | | | | | | |

Other checks, every config run so far:
* **Long crash, no prune** (10% lift; −14%/day × 6 days or −30%/day × 4 days, then 60 days at the low; 6 subnets): no long defaulted and lender shortfall 0 in every config. The TWAP median lags the crash, so u stays below about 0.9 and the rate stays near or below the base.
* **Long crash into prune** (v2.0.1 `crash_long_*`, 28 subnets): lender shortfall comes from the prune sale, not the curve, and is about the same in every config (−30%/day: total 8.66k–8.71k TAO, all 28 nonzero, max ≈ 3.5k; −14%/day: total 0.10k–0.15k TAO). Interest barely moves it.
* **Long drain (Igor 2.2)**: a round trip loses −0.025% of pool TAO on every subnet in every config (virtual long).
* **Split** (18% short book as 1 vs 10 pro-rata pieces under a held 5× pump; long book 1 vs 10 under a 3× drop; 6 subnets): residual delta 0, shortfall delta 0, pool TAO delta ≤ 1e-8 TAO in every config (I23).
* **Invariant violations** other than the pre-existing I21 on the inflated-stake crash harness: 0 in every config; 0 job errors.
* **At open**, a launch-coverage short has u ≈ 0.27–0.32 (section 1 item 2).

### 10.2 What the numbers say
1. **The squeeze through forced buying is gone** in every config: Igor's 6× 30-day case is negative on 28/28 subnets for every k and cap.
2. **The curve creates a new, smaller squeeze: through interest.** A float owner who holds a pump makes the outside short pay a convex rate into a pool the owner mostly owns, and if the hold outlasts C the short defaults and P goes to the same pool. With the flat rate (k = 0) this never pays (0 of 12 SN29 cells). With any convexity it pays in the extreme cells (10× held 60 days, or a thin 5% book): k = 1 and k = 2 at cap 100% give +0.1k to +0.8k on 2–4 of 12 SN29 cells and 0 of 28 sweep subnets; k ≥ 4 with a cap ≥ 500% gives +1k to +7.9k, positive on 3–6 of 28 subnets at 60 days, and defaults an honest minimum-cover short inside 30 days of a held 6× pump. **The cap is the main lever on this, k is second.**
3. **What the curve buys:** in a sustained 3%/day rally the 18% books default at a median day of 113–166 depending on config (flat: only 2 of 6 by day ~199), handing C + P to the pool months earlier. The residual itself is the same q whenever default comes (5.6% of issuance median for an 18% book): no curve shrinks it, since nothing buys back. Faster drain changes when holders get the TAO, not the dilution.
4. **Healthy positions pay much less than the flat 20%.** At u ≈ 0.3, k = 2 charges about 1.8%/yr and k = 4 about 0.16%/yr (20% × u^k). That cuts pool interest income from honest books and makes Igor's staked short (2.5) cheaper to carry, since staking yield now dwarfs the interest. See 10.4 (floor variant).

### 10.3 Recommendation (provisional; final after 10.4 runs)
**k = 2, cap = 100%/yr (10,000 bps), apr 20%.**
* The cap at 100% is what keeps the interest squeeze small: with k = 2 it holds the SN29 worst cell at +0.8k (vs +1.4k at cap 730%) and keeps 28/28 sweep subnets negative at 30 and 60 days, and no honest short defaults inside 30 days of a held 6× pump at any cover.
* k = 2 rather than 1: same squeeze profile on the 28-subnet sweep, a clearly convex curve (a 2×-stretched position pays 4× the rate), and a lower cost for honest shorts in moderate pumps (2× pump: 1.4% vs 2.1% of notional over 30 days). k = 1 is the safer choice if the SN29 extreme cells matter more than convexity (2 cells, max +0.5k).
* Cost accepted: a 3%/day rally drains an 18% book by about day 163, not day 128 as at cap 2000%.
* Not recommended: k ≥ 4 with a cap ≥ 500%. It reopens a squeeze of the same order as v2.0.1's option B and defaults honest minimum-cover shorts under a held 6× pump.

### 10.4 Still running (results go to `model/results_v21/`, this section is updated when they land)
* **Slow-u variant** (`uprice=1`): u values the debt at min(TWAP_median, SMA30) for shorts (max for longs), so a held pump needs weeks to raise the rate while a real re-rating still does. Aimed at the interest squeeze. Configs k = 2 and k = 4 at cap 730%.
* **Floor variant** (`floor=1`): `rate = min(cap, apr × max(1, u^k))`, so a healthy position pays the flat 20% as in v2.0.1 and only stretched ones pay more. Aimed at 2.5 and pool income. k = 2 at cap 730%, alone and with slow u.
* k = 8 at caps 730% and 2000%.
* Regression for the recommended config: the 13 standard scenarios on the 28-subnet subset, 200 fuzz runs, 200 split-fuzz runs, and the squeeze on all 110 subnets.

### 10.5 New risks the curve creates
1. **Interest squeeze** (10.2 item 2): pumping raises the outside short's rate into a pool the pumper owns, and a long enough hold forces a default that moves P into that pool. Bounded by the cap; measured above.
2. **u manipulation through the TWAP:** u moves only with the 72-sample median, so a pump must be held ≥ 36 h; a one-block spike changes nothing. Holding is the cost the squeeze numbers already include (with no outside sellers, the worst case).
3. **Feedback:** paying interest lowers C, which raises u, which raises the rate. Under a held pump the drain accelerates (this is why k = 4 / cap ≥ 500% defaults the 6× minimum-cover short before day 30 while cap 100% does not). The cap bounds it.
4. **Top-up timing:** a touch accrues at the pre-touch u, so a top-up just before a charge doesn't lower the rate on past blocks (unit test and e2e 7).
5. **Solvent-default edge:** a position whose C runs out while it is in profit still defaults (shorts forfeit P; longs are sold). Rare under the curve because healthy rates are tiny; owners can top up.
6. **Long default sale front-run:** the one remaining protocol sale happens at a predictable block (7.1 item 7); same class as Igor N5.
7. **Pool income and staked shorts** (10.2 item 4): the floor variant is the fix if Jacob wants the v2.0.1 economics for healthy positions.

## 11. Historical evidence (kept by reference)
Sections 9–21 of v2.0.1 (the previous revision of this file) remain the evidence record for everything v2.1 does not change: dereg ranking window and griefing (9, 11), dereg settlement at r and team exit (9.1, 10, 10.1), the v1.2 cap and APR studies (12), the SN29 exit study (13), longs economics (14), interest routing (15), supply and halving (17, 18), and the reference-model findings 3 and 4 (20). Sections that studied forced deleverage, its pause bands, pacing, split-resistant health and the exit rule (16, 19, 21, and the deleverage rows of 11 and 12) are history: the mechanism they measured is deleted in v2.1. Igor's v2.0.1 review and the bundle A/B/C fix runs are in `IGOR-REVIEW-v201.md` and `model/results_igor/`.
