<!-- Generated from the working spec by make_public_spec.py; edit the source, not this file. -->
# Synthetic-mint shorts and pool-lifted longs: build spec v2.0.1 (rewrite of PR #3268)

Status: build spec for review. Nothing has been posted or deployed. Evidence comes from `DESIGN.md` (v1 and v2), `sim.py`/`sim2.py`/`dereg2.py`, and finney data read only at block 9,578,833 (2026-11-24 19:54 UTC) plus archive snapshots from 1, 7 and 30 days earlier. v1.3 adds pool-lifted TAO longs (section 7, normative). Longs evidence: live SN29 at block 9,581,283 (2026-11-25 04:04 UTC) and `state.json` for deep subnets, all read only (section 14, `DESIGN.md` longs draft).

In this repo: the implementation design is `DESIGN-v2.md` next to this file (the `DESIGN.md` cited below as evidence is the earlier research draft, not in this repo), and the reference model used by the differential tests will live in `pallets/lending/model/`. The other evidence scripts and result files named below are in the research workspace, not in this repo.

**Changelog**
* **v2.0.1 (2026-11-26 UTC).** Forced-close exit rule (deleverage hysteresis), both sides.
  * A `Deleveraging` position returns to `Open` once its health clears the trigger by a margin `h = exit_hysteresis_bps / 10,000` (launch 500 = 5%). Exact rule in section 1 item 4 (shorts) and 7.1 item 5 (longs).
  * New shared root-settable param `LendingParams.exit_hysteresis_bps` (0 ≤ v ≤ 5,000, launch 500). Parameter count 24 → 25.
  * New events `DeleverageEnded` / `LongDeleverageEnded`, invariant I27, unit tests and e2e 40.
  * Why a margin: with h = 0 a position whose equity sits at the trigger flips Open ↔ Deleveraging on noise every charge, and each entry restarts the paced buy-back. The reference model (`model/v201`) implements the same rule; its comparison against v2.0 on the 110-subnet scenario suite is in section 21.
* **v2.0 (2026-11-26 UTC).** Jacob: every tunable is settable by sudo, plus three simplifications.
  * **Root-settable parameters:** one table in section 2.
    * Every tunable now lives in one of four root-only grouped storage items (`LendingParams`, `ShortParams`, `LongParams`, `DeregParams`), each with its own setter, bounds check (`InvalidParams`) and event.
    * Changes take effect from the next block. Existing positions' coverage is never re-checked retroactively (I4); health and interest cover use current values.
    * New settable items: equity cutoff, minimum interest cover, charge cadence, short deposit, short withdraw buffer, the TWAP pause band (shorts had it only as a constant), the T_ema window, the rank window, and the longs-per-subnet cap (was runtime constant `MaxLongPositionsPerSubnet`). The TWAP and rank-window settings re-seed their rings on change.
    * **Not settable:** the 21M alpha cap (P23, protocol constant) and the interest destination (P4: 100% to SubnetTAO, not a parameter).
    * New I26 and e2e 39.
  * **No SMA-pause lapse:**
    * `sma_pause_max_blocks`, the per-position `sma_paused_blocks` counter and its per-block write are deleted on both sides. I20 is removed. This closes review #10's counter DoS.
    * The pause bands (1.25 × TWAP_median and 1.25 × SMA30) stay. Interest keeps accruing while paused, so a position always ends by C exhaustion if the band never clears; APR > 0 is now a setter bound.
    * Accepted cost: on a permanent re-rating a paused position bleeds interest longer and leaves more residual (v1.5 sim, 6× forever: 102.5k vs 76.4k α).
  * **Price references are spot, TWAP_median and SMA30 only:**
    * Short coverage ref = max(spot, TWAP_median, SMA30_rank).
    * Long coverage ref = min(spot, TWAP_median).
    * Interest and minimum notional use q × TWAP_median for shorts; longs are unchanged (on D).
    * `NotionalEma` is deleted. T_ema (an EMA of pool **TAO** in r) stays, flagged in 8.7.
    * Touched rows: section 1 items 1–2, P2, P10, P13, L2, 7.1 item 3, section 3, unit tests.
  * **One shared parameter set** for APR, deleverage pace, minimum notional, deposit, withdraw buffer and the pause band.
    * L3, L7, L9, L10 and L12 now point to P3, P7, P17/P22, P10 and P20; L11 points to P19.
    * The two deleverage budgets still meter separately but share `pace`.
  * **Parameter count:** 32 distinct tunable values before (23 settable, 9 hard-coded), 24 after, all settable. The breakdown is in section 2.
  * Longs are not dropped. Launch values are unchanged.
* **v1.9.1 (2026-11-25 UTC).** Jacob approved both reference-model fixes. Evidence: `model/FINDINGS.md` Findings 3 and 4, `model/results_v19/`; section 20.
  * **Finding 3, longs at dereg:** paced phase A and every "longs settle at r" path are deleted.
    * Inside the atomic prune step (`start_dissolution`), before T_A and r are read, the whole long book's locked alpha is sold into the live pool in **one** swap. Proceeds are split pro rata to positions.
    * Each position then settles lender-first: D plus accrued interest goes to the pool, C covers any gap, and any surplus goes to the trader.
    * Shorts then settle at r as before.
    * **Removed IDs:** L13 `long_unwind_blocks`, state `Unwinding`, error `UnwindInProgress`, phase C (letter kept, marked removed).
    * New runtime bound `MaxLongPositionsPerSubnet` (256) so the prune's weight is bounded and charged.
    * Rows updated: 7.1 item 11, 7.4, 7.5, I15, I17, new I24, events, weights, e2e 21/32/33, new e2e 37.
  * **Finding 4, SMA30 rank rings:**
    * Every live subnet's ring is prefilled from `SubnetMovingPrice` at the runtime upgrade, regardless of `Enabled`.
    * Rings are re-seeded the same way on every `set_enabled` false→true and every `RankGeneration` reset.
    * No fallback to the emission EMA or a partial mean is allowed anywhere.
    * Rows updated: P13, section 9, new I25, new e2e 38.
  * **New open item 8.6 (not adopted):** marginal-cost opener coverage for slice-built books (model Finding 2 residual).
* **v1.9 (2026-11-25 UTC).** Jacob approved the pass.
  * **Split-resistant health** (review v1.6 High #2):
    * A short's health cost is now its pro-rata share of buying back the **whole open short book** on the subnet: BuyCost(Q_book) × q_pos/Q_book × TWAP/spot.
    * A long's health value is its share of selling the whole long book: SellProceeds(A_book) × a_pos/A_book × TWAP/spot.
    * Splitting a position across coldkeys no longer delays forced close. At SN29 with an 18% book, the trigger stays at 3.92× for 1, 10 or 100 positions (v1.8: 3.92× → 5.41× → 5.59×).
    * A single position alone on its subnet is valued exactly as in v1.8.
    * Literal last-in-line V_dyn is explicitly **not** used for health (section 1 item 4).
    * Rows updated: section 1 item 4, P5, 7.1 item 5, section 3, events, new I23, unit tests, new e2e 36. Evidence is in section 19.
  * **P24 halving guard removed.** Jacob: "the halving is bullish for a subnet I don't see an issue here."
    * **Removed IDs:** P24, error `HalvingCrossing`, `halving_guard` param, I22, the P24 unit tests and e2e 35. They are not reused.
    * P23 (21M `AlphaSupplyCap`) and counting minted/residual alpha in supply stay. The v1.8 `emission_weights.rs` correction also stays.
  * All other params are unchanged. Review findings 3+ are not applied.
* **v1.8 (2026-11-25 UTC).** Jacob's decision (2026-11-25 20:34 UTC): **refuse any short mint that would push alpha supply across the next halving threshold.**
  * **New P24 halving guard** (section 1 item 11). An open or increase of q is refused with `HalvingCrossing` if `get_block_emission_for_issuance(S + q) < get_block_emission_for_issuance(S)`, where S = `get_alpha_issuance` with minted alpha counted. Verified against `block_emission.rs`: the next halving point is T(n+1) = 21M·(1 − 2^−(n+1)) α (10.5M, 15.75M, 18.375M, …).
  * **Emission crossing with books open:** no forced unwind, no collateral change. The guard moves on to the next threshold. Closes, deleverage and dereg burns are never blocked.
  * New I22, boundary unit tests, e2e 35.
  * **Correction found while verifying the code:** two more readers of `alpha_issuance_for_schedule` exist in `emission_weights.rs`.
    * `baseline_root_emission_share` switches to `get_alpha_issuance`.
    * `legacy_owner_cut_accrued` keeps excluding lending alpha, so the lending hook is kept (renamed `lending_alpha`) for that one call.
  * **Flagged (8.5):** the guard stops only a mint that crosses directly. Opening just under T and letting emission cross a block later still pulls the halving forward by up to q/rate days (quantified in section 18).
  * Review findings 2–4 are not applied.
* **v1.7 (2026-11-25 UTC).** Jacob's decision (2026-11-25 20:24 UTC), on review finding #1 (P21 permanent lockout): "go simple: 3x requirement + we count this in the supply of alpha and dont allow it to go above 21M ever."
  * **P21 dilution breaker removed:** params `breaker_on_bps` / `breaker_off_bps`, error `DilutionBreakerActive`, I11 clause, e2e 5/15/31 expectations, 8.3 bound 4.
  * **Minted short alpha now counts in alpha supply,** including residual from defaults. This reverses the v1.0 exclusion. The halving schedule and `root_proportion` use plain `get_alpha_issuance`, and `alpha_issuance_for_schedule` and the `synthetic_and_residual` hook are deleted. Rows updated: section 1 items 6/7, I7, I18, section 3, tests.
  * **New P23, 21M hard cap:** a short open or increase is **refused** (not partially filled) if `get_alpha_issuance + q > 21,000,000 α`. Emission keeps its existing formula and yields nothing; minting yields at the cap (section 1 item 10). New I21, new unit tests and e2e 34.
  * **Collateral unchanged:** 3× with the book/pool term (P2).
  * **Evidence:** section 17 (`pull_supply.py`, `supply_v17.py`, `results_v17_supply.json`, finney block 9,586,188 (2026-11-25 20:25:29 UTC)). No subnet is near 21M; the largest is SN4 at 6.80M α (32.4%). No subnet has reached its first halving at 10.5M.
  * **New consequence to decide (flagged in 8.5):** an open book pulls the next halving forward by book/issuance-rate days, which allows an early-halving grief near 10.5M.
  * Other review findings are not applied.
* **v1.6 (2026-11-25 UTC).** Jacob's decision (2026-11-25 19:20 UTC): **minimum pool depth (P18) = 1,000 TAO for both shorts and longs.**
  * P18 is decided. Below the depth, no opens or increases are allowed. Closes, collateral top-ups, forced close (deleverage), default, interest and dereg all keep working (section 1 item 9).
  * Rows updated: section 1 item 9, P18, SynthParams `min_pool_tao`, L-open checks, I11, unit tests, new e2e 33. Section 8 now has **no open items**.
  * At finney block 9,585,867 (2026-11-25 19:21:15 UTC), 18 of 128 subnets are below 1,000 TAO: SN78 (87), SN16 (118), SN113 (127), SN47 (130), SN109 (135), SN84 (179), SN35 (187), SN57 (217), SN86 (219), SN7 (263), SN26 (282), SN70 (285), SN76 (335), SN103 (335), SN108 (398), SN59 (758), SN36 (825), SN99 (882).
* **v1.5 (2026-11-25 UTC).** Jacob's decisions (2026-11-25 19:13 UTC): **adopt P22, reject L15.**
  * **P22 (normative):** one pause rule per side, with the SMA30 term folded into the TWAP pause.
    * Short buy-backs (deleverage) run only while spot ≤ 1.25 × min(TWAP_median, SMA30_rank).
    * Long forced sells run only while spot ≥ max(TWAP_median, SMA30_rank)/1.25.
    * The SMA30 term lapses after 30 days of SMA-only pause per deleverage episode (liveness bound). Dissolution phase A uses the TWAP term only.
    * Rows updated: P17, P22, L9, L13, I9, I15, new I20, e2e 2/7/19/25, new e2e 27–32. `sma_paused_blocks` added to both position types.
  * **L15 rejected:** long-bought alpha counts in the emission price (raw spot, like shorts). The `emission_addback` field and the add-back touchpoint are removed. The accepted consequence is quantified in 8.4.
  * **Section 8:** only minimum pool depth (P18) stays open. Every other item is recorded as decided or closed.
  * **Evidence:** section 16 (`p22_v15.py`, `results_v15.json`), under v1.4 interest routing.
* **v1.4 (2026-11-25 UTC).** Jacob's decision (2026-11-25 19:00 UTC): **interest is symmetric and simple.**
  * **Interest flow:** all interest on shorts and longs is credited to the position subnet's SubnetTAO. The 50% burn, `ResidualReserve`, `LongDebtReserve`, their caps and the residual-retire sweep are removed.
  * **Parameters:** P4 and L4 are rewritten. Reserve fields are removed from `SynthParams` and `LongParams`.
  * **Short default:** residual alpha is now permanent dilution until dissolution.
  * **Long default:** unpaid debt is written off as a pool shortfall. It is borne by holders through the pool and, at dereg, through a lower T in r (the existing haircut).
  * **Dissolution:** no reserve enters the pot.
  * **Accounting:** the interest credit is **not** a TAO inflow for flow or volume (new rule I19).
  * **Other updates:** storage, events, invariants I2, I3, I8, I9, I15 and I16 (plus new I19), unit and e2e tests, and the P21 resume note.
  * **Re-check:** section 15 (`interest_v14.py`, `results_v14_recheck.json`).
  * **Still open:** P22 and L15.
* **v1.3 (2026-11-25 UTC).** Jacob's decisions: **add pool-lifted TAO longs**, for fairness rather than symmetry.
  * **Mechanism:**
    * A long lifts TAO from the subnet's own pool and buys alpha in the same pool in one atomic step. The alpha is locked as the position and is never released to the borrower.
    * Collateral is 2× (floor 1.5×) on min(EMA, spot, median TWAP), counting the whole long book.
    * The lift cap is 10% of pool TAO per subnet.
    * Interest is a flat 20% APR: half burned, half to a long-debt reserve that repays unpaid long debt.
    * Forced close mirrors shorts: median TWAP, paced on the TAO side, paused while spot < TWAP/1.25.
  * **Dereg is option (c):** the long's locked alpha is sold into the pool first and the lender claim is repaid; whatever is unsold settles at r. The lender claim is settled before stakers are paid.
  * **No open gate.** Jacob: unneeded complication.
  * **Accepted consequences:** third-party longs acting as exit liquidity for a team, and the prune delay caused by long buying (SN29: day 26 → 123 with a 25% book; with the 10% cap it is smaller).
  * **Implementability fixes from code review:**
    * side-aware position keys and types (`Side`, `LongPosition`);
    * explicit dissolution phases;
    * a separate TAO-side pace/pause budget;
    * separate long issuance and flow accounting;
    * new storage names. No `Lifts`/`LongInterest*` names are reused.
  * **Tests:** 11 long attack scenarios added to the e2e list (16–26). All long parameters are root-settable, and longs launch dark behind their own `LongsEnabled`.
  * **New open items:**
    * the v1.2 shorts sustained-pump hole, with the recommended fix P22 (SMA30 buy ceiling) and its sim numbers (section 8.1, `shorts_fix.py`);
    * the long emission add-back (L15).
  * Section 7 ("adding longs later") is replaced. Section 14 adds the evidence.
* **v1.2 addendum (2026-11-25 UTC):** section 13 answers how much a big SN29 short can stop the team extracting. No rule changes.
* **v1.2 (2026-11-24 UTC).** Jacob's decision: **no supply cap and no utilization APR curve; interest is a flat APR.** Evidence is in section 12 (`sim3.py`, `v12.py`, `v12b.py`, `team_exit_v12.py`).
  * **P3:** flat **20%**, chosen over 15% and 30%.
  * **P9:** supply cap removed. Three things replace it:
    * The coverage rule now counts the whole book pro rata and uses ref = max(EMA, spot, SMA30) (P2).
    * Book-aware stress: Stress = 3 × (1 + 2·SyntheticOutstanding/SubnetAlphaIn) (P2).
    * A dilution breaker: no opens or increases while ResidualAlpha > 2% of Issuance_ex (P21).
  * **P8:** the open limit is now a *net minted quantity* per block (≤ 1% of SubnetAlphaIn), not a price test, so other traders' sales can't lock out opens.
  * **P16:** the dereg-zone open freeze is replaced by *zone-minted alpha settling at its own sale proceeds*. The freeze was itself a lock-out: 8 subnets are in the zone today.
  * Storage, errors, invariants and tests are updated. The rewrite is still not on the branch.
* **v1.1 (2026-11-24 UTC).**
  * **Dereg settlement (P12, Jacob's decision):** shorts settle at the dereg rate r = min(T_flag, T_ema)/all_alpha, the same rate stakers get, instead of the EMA price. T is pool TAO, excluding short escrow. all_alpha is total alpha issuance, including synthetic and residual alpha. Short dereg settlement is provably solvent (section 9).
  * **Re-run:** the dereg window and team-exit sims were re-run under r (sections 9 and 10). The dereg window stays at a 30-day SMA (P13).
  * **New guardrails from the red-team (section 11):**
    * P16 dereg-zone open freeze (kills the last-block snipe that r creates).
    * P6 TWAP changed from mean to median (a one-block spike could trip deleveraging).
    * P17 deleverage pauses during a pump.
    * P8 open-impact limit aggregated per block (stops multi-account bypass).
    * Deleveraging is pro rata across positions.
    * P9 cap 20% → 50%, with a steeper APR above the knee (stops cap squatting).
    * P18 minimum pool depth of 100 TAO.
    * P19 per-position deposit.
    * A withdraw buffer of 7 days of interest.
    * ResidualReserve goes to the pot at dereg.
  * Test plan extended.
  * Branch `cursor/lift-lending-escrowed-raise-352f` was checked at 1b1e856 (2026-11-24 18:43 UTC). It still holds the escrowed long/short lending design, so no review against this spec yet.
* **v1.0:** first build spec, dereg window section and team-exit section.

## 1. Mechanism (normative)

1. **Open** `(netuid, mint q, collateral C, min_tao_out)`: mint q alpha into the custody account and sell it into the pool, enforcing min_tao_out and the open-impact limit. The proceeds P stay locked in custody. A position may exist only if the **coverage rule** holds:
   `C + P ≥ BuyCost(Q_pos | pool after a hypothetical pump to Stress × max(spot, TWAP_median, SMA30_rank))` (v2.0: no EMA).
   This is evaluated on the live pool after the sale, with all other open synthetic alpha counted.
   **v1.2:** the test is `C + P ≥ BuyCost(SyntheticOutstanding | pumped pool) × q_pos/SyntheticOutstanding`, i.e. the position's pro-rata share of buying back the whole book.
   * The pump goes to `Stress_eff × max(spot, TWAP_median, SMA30_rank)` (v2.0: EMA removed), with `Stress_eff = Stress × (1 + slope × SyntheticOutstanding/SubnetAlphaIn)`.
   * If the buy-back of the whole book is infinite (the book exceeds what the pumped pool can supply), the open fails. That is the natural bound that replaces the cap.
2. **Interest:** charged in TAO and taken from C, on notional `q × TWAP_median` (v2.0: was q × EMA), at a **flat APR** (v1.2; no utilization term). Charged every `charge_interval_blocks` (P15).
   **v1.4:** all of it is credited to the subnet's SubnetTAO at each charge. This is a pool-internal credit with no alpha out. It is not recorded as TAO inflow for flow or volume (I19). There is no burn and no reserve.
3. **No term.** There is no expiry, no roll, and no liquidation on spot.
4. **Health (v1.9: whole-book pro rata):** a position is healthy while `Equity_TWAP = C + P − H_pos ≥ 0` and C covers at least 1 day of interest.
   * **The health cost** is `H_pos = BuyCost(Q_book | live pool) × q_pos / Q_book × TWAP/spot`.
     * Q_book = `SyntheticOutstanding[netuid]`, the alpha still owed by Open + Deleveraging positions. ResidualAlpha is owed by nobody, so it's excluded.
     * Same pool and TWAP basis as before: live pool, fee-free `buy_cost_on`, scaled by TWAP/spot. If Q_book ≥ pool alpha, BuyCost is infinite and every position on the subnet is unhealthy.
     * Each share is rounded up.
   * **It reuses the opening-coverage calculation.** `book_need` (old branch `cursor/lift-lending-escrowed-raise-352f`, `pallets/lending/src/lib.rs`) already computes `BuyCost(book | pumped pool) × q_pos/book`. Factor its last two steps into `book_share(pool, book, q_pos)`. Coverage calls it on the pumped pool with fee scaling; health calls it on the live pool with no pump and no fee scaling, so a lone position is valued exactly as in v1.8.
   * **Why:** with standalone `BuyCost(q_pos)`, constant-product curvature makes Σ BuyCost(q_i) far smaller than BuyCost(Σq_i). Splitting a book into N coldkeys therefore delays the trigger (review v1.6 #2). The pro-rata share also matches how deleverage actually executes: pro-rata fills against one pool (I9).
   * **Why not literal last-in-line V_dyn** (Karl's lift-lending spec v5.10 §5):
     * V_dyn values a position as if every other position trades first, i.e. its marginal tail cost BuyCost(Q) − BuyCost(Q − q_pos).
     * Summed over positions this overcounts the whole-book unwind. At SN29's 18% book: +18% for 2 positions, +37% for 10, +43% for 100. At 30%: +32% / +77% / +92%.
     * As health, it would force-close books whose combined funds still cover the whole buy-back: the 10-way 18% book would trigger at 3.11× instead of 3.92×.
     * V_dyn fits a one-off opener check, where the newcomer pays for its queue position. It doesn't fit a recurring trigger, where everyone is queued behind everyone else at once.
   * **Side effect (accepted):** a later large open raises every earlier position's health cost to the book's per-alpha average. Earlier positions' coverage isn't re-checked (I4), so their trigger moves earlier. Section 19 has the numbers: a 1% short opened alone triggers at 3.11× instead of 4.18× after a 17% book joins, and at no less than 2.89× even beside a 55% book. It is never pushed into deleverage at the moment of the open (equity stays ≥ 70% of its funds). This is fair: forced close fills pro rata, so its real unwind cost already was the book average, and v1.8 overstated its health. It can top up C.
   * **When either test fails** (v2.0: `Equity_TWAP < equity_floor` or C < `min_interest_cover_blocks` of interest; launch 0 and 7,200), the position enters **Deleveraging**: a paced buy-back limited to `pace` price impact per block, paid from P and then C. **v1.5 (P17 + P22):** buys run only while spot ≤ 1.25 × min(TWAP_median, SMA30_rank). **v2.0:** neither term lapses. Interest keeps accruing while paused, so a position that never clears the band ends by C exhaustion. If the funds run out, the position **defaults**: its unbought q moves to `ResidualAlpha` and is never paid out in TAO.
   * **Exit (v2.0.1).** A `Deleveraging` short returns to `Open` when **both** hold, with `h = exit_hysteresis_bps / 10,000`:
     * `C + P − H_pos ≥ equity_floor + h × H_pos` (with the launch floor 0: `C + P ≥ 1.05 × H_pos`), where H_pos is the same whole-book pro-rata TWAP cost as the trigger, read after any fill in the same step; and
     * `C ≥ (1 + h) × min_interest_cover_blocks` of interest (launch: 1.05 days).
     * It is evaluated at exactly the points where the trigger is evaluated: `poke`, each interest charge, the per-block deleverage step (before that position's fill; an exiting position takes no fill that block) and the end of `add_collateral`.
     * On exit: the state becomes `Open`, the position leaves `DeleverageQueue`, `DeleverageEnded{who, netuid, equity_twap, book_cost}` is emitted, and coverage is **not** re-checked (I4). Partial fills already made stand; the position keeps its smaller q.
     * No exit once the subnet is dissolving. With h = 0 the exit test is exactly "not triggered".
     * Because the exit threshold is ≥ the trigger threshold for every h ≥ 0, one evaluation can never both exit and re-trigger a position.
5. **Close** `(netuid, alpha, max_tao_in)`: buy alpha from the pool and burn it, paying from P and then C. A partial close is allowed if the coverage rule still holds afterwards. Closing the remainder refunds whatever C and P are left.
6. **Residual alpha (v1.4):** a defaulted position's unbought q stays in `ResidualAlpha` as permanent dilution until dissolution. There is no reserve and no retire sweep. **v1.7:** it counts in the subnet's alpha supply (halving schedule, `root_proportion`, the P23 21M cap) and in all_alpha at dereg.
7. **Accounting:**
   * Minted alpha is real once sold. **v1.7:** `SyntheticOutstanding` + `ResidualAlpha` **count** in the issuance used for the halving schedule and `root_proportion`, i.e. plain `get_alpha_issuance` = SubnetAlphaIn + SubnetAlphaOut + protocol reservoir. Sold minted alpha already sits in SubnetAlphaIn, so this means deleting the v1.0 subtraction. An open book pulls halvings forward and lowers `root_proportion`; a close or dereg burn reverses that, but residual doesn't.
   * The emission price uses raw spot/EMA, with **no add-back**.
8. **Dereg (decided by Jacob):** shorts may push a subnet into deregistration. Ranking uses the raw price (no minted-alpha add-back), averaged over a slow window (P13, section 9). **v1.1:** shorts settle at the dereg rate r = min(T_flag, T_ema)/all_alpha, where T is pool TAO (escrow excluded), T_ema is the 7,200-block EMA of pool TAO, and all_alpha is total issuance including synthetic and residual alpha. Each position pays q × r into the pot, from P first and then C, the minted alpha is burned, and the rest is refunded. Stakers are paid r per staked alpha. The pot is pool TAO plus Σ q·r (v1.4: no reserve). The leftover (the pool-alpha share) takes the existing dissolution leftover path. Collateral is 3×, and dilution is the bad-debt policy. Opens are frozen in the dereg zone (P16).
9. **Minimum pool depth (P18, v1.6, both sides).** A pool is *shallow* while SubnetTAO < `min_pool_tao` (1,000 TAO).
   * **On a shallow pool, these fail with `PoolTooShallow`:**
     * a short open or mint increase;
     * a long open, an increase in lift, or an increase in locked alpha.
   * **The check runs on SubnetTAO both before and after the trade,** so an open can't push a pool below the depth. There is no hysteresis.
   * **Existing positions are kept** (no forced exit because of depth). On a shallow pool, these work unchanged:
     * voluntary full or partial close (subject to the usual coverage rule);
     * adding collateral;
     * withdrawing collateral (subject to coverage, health and P20);
     * interest accrual and crediting to SubnetTAO;
     * health checks, deleverage or forced close (P17/P22/L9 pacing and pauses), default to ResidualAlpha or pool shortfall;
     * dereg ranking and settlement (item 8 for shorts, section 7.4 for longs).
   * **When the pool is back at ≥ 1,000 TAO,** opens are allowed again. Depth is read live, not snapshotted.
10. **21M alpha supply cap (P23, v1.7).** Let S = `get_alpha_issuance(netuid)`, which includes SyntheticOutstanding and ResidualAlpha.
    * **The mint rule:** a short open or increase of q fails with `AlphaSupplyCap` if S + q > 21,000,000 α. It is refused outright, not capped to the remaining room. A partial fill would change the position's coverage after the user signed, and the user can retry a smaller q.
    * **Nothing else mints alpha on the lending side.** Close, deleverage and dereg only burn, and longs buy existing alpha. So no other path checks the cap, and none can fail because of it.
    * **Interaction with emission:** emission is unchanged. Each block, `get_block_emission_for_issuance(S)` gives alpha emission e = 1 α × 2^−⌊log2(1/(1 − S/21M))⌋ and 0 once S ≥ 21M. The alpha injection is ≤ `root_proportion` × e ≤ e.
      * With headroom h = 21M − S, e ≤ 2h/21M α. So one block's emission plus injection is ≤ 4h/21M α, far below h, and emission alone can never cross 21M.
      * A mint can take S to exactly 21M at most, after which emission is 0. So S ≤ 21M always holds.
    * **Who yields (the safe choice, flagged):** minting yields at the cap; emission never gets an exception.
      * Because minted alpha counts in S, an open book already slows emission continuously (halvings come earlier) for as long as it is open. That is the price of counting it.
      * The cap only guarantees that supply never exceeds 21M; it reserves nothing for future emission.
      * Coverage bounds the book far below the cap: whole-book buy-back is infinite once Q approaches the pumped pool's alpha, so in practice Q ≲ SubnetAlphaIn. Today max S + SubnetAlphaIn is 10.9M α (SN13), so P23 is a backstop that can't bind today (section 17).
11. *(Removed in v1.9: P24 halving guard.)*

## 2. Parameters (launch values; every one is settable by root)

| # | Parameter | Value | Evidence and sensitivity |
|---|---|---|---|
| P1 | `Enabled` | false at genesis of the upgrade; root turns it on | Same practice as #3268 |
| P2 | Collateral rule | Coverage at **Stress = 3.0** (`short_stress_bps` 30000), ref = **max(spot, TWAP_median, SMA30_rank)** (v2.0: EMA removed), **whole book counted pro rata**, book-aware **Stress_eff = 3 × (1 + 2·Q/SubnetAlphaIn)** (`short_stress_slope_bps` 20000, v1.2), proceeds counted | v1.0 evidence for 3×: it survives +200% held 7 days, while 2× defaults. v1.2: with no cap, this rule is the only size bound. v2.0: the EMA (≈ 7.7 h) is dropped from the max. It only exceeded all three of spot, the 72 h median and SMA30 briefly after a fast up-and-down move, so coverage can be marginally lower right after a spike; not re-simulated |
| P3 | APR (**shared, both sides**, v2.0) | **Flat 20%** (`apr_bps` 2000; v1.2: no ceiling, no knee). Range 10–50%. Must be > 0: since v2.0, interest is what terminates a paused position | The rate barely matters to shorter P&L: a 25% pin over 30 days moves by ≤ 7 TAO between 15% and 30% on SN29/101/128. The pin cost against dip-buyers dominates (−0.5k to −8k). For rival-subnet emission gaming against deep victims (pool ≥ 10k), 28 of 433 pairs break even at 15%, 13 at 20% and 7 at 30%. Those gains go to the *whole* attacker subnet, so an attacker holding ≤ 10% of it needs a break-even ≥ 10× higher, and none is profitable at 20%. Shallow victims (< 1k pool) break even at 30–1,900% at any rate. That is price discovery, not a rate problem. 30% buys little (13 → 7 pairs) for 50% more carry. |
| P4 | Interest destination (v1.4) | **100% to the subnet's SubnetTAO**: no burn, no reserve, no cap. Not counted as inflow or volume (I19) | Jacob's decision: symmetric and simple. The reserve it replaces retired almost nothing against a cascade (about 0 TAO vs 13k–277k α in a week). Re-check (section 15): SN29 team exit changes by 0 to +30 TAO (≤ 0.8%). Rival-gaming break-even SN51←SN64 goes 10.98% → 10.95%, with the same pair counts at 20% (13 deep pairs). Interest now reaches the subnet's holders, including a defending team; on SN29 that is ≤ 6 TAO at β = 0 |
| P5 | Forced-close trigger and exit (**shared**) | Trigger: `Equity_TWAP < equity_floor` (0; v1.9: whole-book pro-rata H_pos, section 1 item 4) **or** C < `min_interest_cover_blocks` of interest (7,200 = 1 day). Exit (v2.0.1): both tests clear by `exit_hysteresis_bps` (500 = 5%) | Spot or EMA triggers give the team a squeeze lever. The exit margin stops Open ↔ Deleveraging flapping at the boundary |
| P6 | TWAP window | **3 days, MEDIAN of 72 hourly samples** (v1.1: was mean) | With a mean, one sampled block at 73–145× spot moves the TWAP 2–3×. That takes 25k–80k TAO of flash capital on SN101/SN128/SN29 and loses only 3–7 TAO in fees on the round trip (section 11.2). With a median, the pump must be held for ≥ 36 h, which the squeeze sims already cover (3× survives +200% for 7 days). A 1-day window still causes defaults at 1.5×; 3 vs 7 days are nearly identical. |
| P7 | Deleverage pace (**shared**, v2.0) | **0.05% price impact per block** (`pace_bps_per_block` 5, ~16%/hour). One value; the short up-budget and long down-budget meter separately | Fast enough to finish within hours; slow enough that outside sellers can meet it. |
| P8 | Open limit (`short_open_limit_bps` 100; was `open_impact_bps`) | **Net minted alpha per block per subnet ≤ 1% of SubnetAlphaIn** (≈ 2% price on 50/50), summed over all accounts. Net = minted − burned in the block (v1.2: was a price test) | A price test can be consumed by anyone: a team sells 2% of real alpha at the top of the block and buys it back at the end. On SN29 that costs 0.074 TAO per block, 533 TAO/day (SN101 247, SN128 294, SN19 2.1k). A net-quantity test can't be consumed by other trades, or by mint-and-burn in the same block. It is an anti-flash limit only; over an hour it allows any size. |
| P9 | Supply cap | **None** (v1.2, Jacob's decision). Size is bounded by P2 (whole-book pro-rata coverage, book-aware stress) and the P23 21M supply cap (v1.7; P21 removed) | With the cap gone, cap squatting and utilization-APR griefing are gone. The book can now reach about 0.6× pool alpha within 30 days of pushing with ref incl. SMA30 (1.4–1.6× with ref = EMA), and 1.2–1.3× over 60 days. The SMA slows the ratchet but does not stop it. |
| P10 | Minimum position (**shared**, v2.0) | **10 TAO** (`min_notional`): short notional q × TWAP_median (v2.0: was q × EMA), long D. One position per (coldkey, netuid, side). Plus P19 | Anti-spam. Splitting across coldkeys can't dodge pacing (P7/P8 are per subnet and per block). It only adds storage, which P19 prices. There is no per-subnet position cap, because a cap can be squatted (256 × 10 TAO ≈ 512 TAO/yr at 20%). |
| P11 | Proceeds | **Locked in custody** | Locked allows 1.7× the size for the same capital and leaves 6× less residual on default (SN112: 7 vs 44 α). Unlocked enables short-rival-buy-own and leverage loops. |
| P12 | Dereg **settlement** (decided v1.1) | **q × r, r = min(T_flag, T_ema)/all_alpha** (T = pool TAO only; all_alpha includes synthetic and residual). Open call: the r_ex variant | Solvent by construction: pot − payouts = T − (out − q)·r ≥ T − I·r ≥ 0, and the coverage rule gives C + P ≥ q·spot ≥ q·r. The sims show a surplus of 0.9–4.9k on every run, and P alone covered q·r on every run. Effect: griefing profit at β = 0 rises from +1.1–1.8k (EMA settle) to +1.9–3.1k. Shorts cut a team's dereg payout by 5–37% (β = 0) and 19–28% (β = 0.5). A last-block snipe nets 15–19% on collateral at stakers' expense unless P16 is in place. Section 9. |
| P13 | Dereg **ordering** (decided: raw price, slow MA) | **30-day SMA of raw spot** (`rank_window_days` 30), sampled hourly (720 samples, or 30 daily means). v2.0: the EMA storage fallback is deleted | Section 9. Where outside dip-buyers are present (β ≥ 0.5), no window lets a griefer with 2× the victim's pool TAO push a healthy mid-ranked subnet to the slot within 200 days. With no dip-buyers (β = 0), the window only adds time: about 3 weeks at 30 days, versus 5 days at 3 days. A 30-day SMA still lets shorts push SN29 out in 15–32 days at β ≤ 1. A 60- or 90-day window blocks the legitimate use at β = 1. **v1.9.1 ring lifecycle:** prefill at upgrade, seed at registration, re-seed on every enable and `RankGeneration` reset, and no EMA or partial-mean fallback (section 9, I25). |
| P14 | Emission price | Raw (no add-back) | Your requirement: shorts push emission down. |
| P15 | Interest cadence (**shared**) | Accrue per block, charge every 360 blocks (`charge_interval_blocks`), reusing the #3268 `Due` queue. A change applies from each position's next scheduled charge; queued `Due` entries aren't moved | Precedent from #3268. Minimum position (10 TAO at 20%) accrues about 760 rao/block, so remainder carry matters. |
| P16 | **Dereg-zone settlement** (v1.2: replaces the freeze) | Alpha minted while the subnet's ranking SMA30 ≤ 1.25 × the lowest non-immune SMA30 is tracked per position (`zone_minted`, `zone_proceeds`). On dereg it settles at its own proceeds (zone_proceeds), not at r. Older alpha settles at r. Opens are allowed in the zone | With no cap, a one-block snipe is bounded only by coverage: 16–30% of issuance. It nets +241 (SN40), +1,536 (SN29), +937 (SN101) or +1,085 (SN128) TAO at r, and stakers lose 0.27k–1.7k. The zone rule makes it −interest. The v1.1 freeze also stopped it, but locked new shorts out of every subnet near the bottom: 8 today (SN40, 72, 66, 98, 27, 123, 55, 106). SN29 sits at 1.36× and would lock itself out by drifting 8% lower. |
| P17 | **Deleverage pause band** (v1.5: merged with P22; v2.0: **shared**, mirrored for longs) | `pause_twap_bps` 12500, `pause_sma_bps` 12500. Short deleverage buys run only while spot ≤ **1.25 × min(TWAP_median, SMA30_rank)**; long deleverage sells only while spot ≥ max(TWAP_median, SMA30_rank)/1.25. No lapse (v2.0). Interest keeps accruing; default happens only when C is exhausted | Stops a team from pumping into a triggered deleverage and selling into the paced buys. The TWAP catches up within 36 h if the pump is held. |
| P18 | **Minimum pool depth for opens and increases** (decided v1.6, both sides) | `min_pool_tao` = **1,000 TAO** (SubnetTAO, checked before and after the trade). Existing positions are unaffected: closes, top-ups, forced close, default and dereg still run (section 1 item 9) | Jacob (2026-11-25 19:20 UTC). Keeps leverage out of the thinnest pools, where a few TAO move price and emission the most (11.9; long emission farming in 8.4). At finney block 9,585,867 (2026-11-25 19:21:15 UTC), 18 of 128 subnets are excluded: SN78 (87), SN16 (118), SN113 (127), SN47 (130), SN109 (135), SN84 (179), SN35 (187), SN57 (217), SN86 (219), SN7 (263), SN26 (282), SN70 (285), SN76 (335), SN103 (335), SN108 (398), SN59 (758), SN36 (825), SN99 (882). Nearest above: SN122 (1,077), SN40 (1,128) |
| P19 | **Position deposit** (**shared**, v2.0) | **0.1 TAO** (`deposit`) refundable on full close, forfeited to burn on default | Prices storage and spam from multi-coldkey splitting. |
| P22 | **SMA30 term of the pause band** (adopted v1.5; v2.0: lapse removed) | `pause_sma_bps` = **12500** (the SMA30 term in P17, shared with longs). **v2.0:** `sma_pause_max_blocks` and the per-position `sma_paused_blocks` counter are deleted. The SMA term pauses for as long as spot is outside the band. For a one-off re-rating to k×, SMA30 catches up in 30·(k/1.25 − 1)/(k − 1) days (< 24 d for any k) | Closes the sustained-pump hole (section 16): on SN29, 1× short, the squeeze premium falls from +7,649 to −1,061 at 6× held 10 days. **Accepted cost of no lapse:** in a sustained rally faster than about +1.5%/day, a forced short stays paused, paying interest, until the rally slows or C runs out. The v1.5 no-lapse runs leave more residual: 6× forever 102.5k α (1.58% of issuance) vs 76.4k with the lapse; 4%/day rally 73.0k vs 57.5k. Longs mirror this in a steady decline (8.3). Removing the counter also removes one storage write per paused position per block (review #10) |
| P23 | **21M alpha supply cap** (v1.7, replaces P21) | A short open or increase is refused if `get_alpha_issuance + q > 21,000,000 α`, with synthetic and residual counted. No buffer. Emission is unchanged and minting yields at the cap (section 1 item 10) | Jacob (2026-11-25 20:24 UTC). P21 could lock shorts out of a subnet for about 2+ years after one default (review #1). Dilution is now priced into the halving schedule instead of blocking opens. Today the largest supply is SN4 at 6.80M α (32.4% of 21M) |
| P20 | **Withdraw buffer** (**shared**, v2.0) | `withdraw_collateral` / `withdraw_long_collateral` must leave C ≥ coverage need + **7 days of interest** (`withdraw_buffer_blocks` 50,400; must be ≥ `min_interest_cover_blocks`) | Otherwise a withdraw to the exact need leaves less than 1 day of interest and triggers deleveraging at the next charge. |


### 2.1 Root-settable parameters (v2.0)
All four parameter groups are root-only (`ensure_root`). Each setter replaces the whole struct, validates every bound, and fails `InvalidParams` on any violation, so a call either fully applies or doesn't apply. Each emits `<Group>ParamsSet{old, new}`.

**When changes take effect:**
* The value is written to `Pending<Group>Params` and copied into the live store in the next block's `on_initialize`.
* Existing positions' coverage is **not** re-checked (I4); coverage applies at the next open, increase, partial close or withdraw.
* Health (`equity_floor`, `min_interest_cover_blocks`), interest (`apr_bps`) and pace/pause use the current values from that block on. Raising APR or the cover can therefore move positions into deleverage at their next check. That is intended.

| Param (P/L) | Storage → field | Setter | Bounds | Launch |
|---|---|---|---|---|
| Shorts on (P1) | `Enabled` | `set_enabled(bool)` | — (false→true re-seeds rank rings, I25) | false |
| Longs on (L1) | `LongsEnabled` | `set_longs_enabled(bool)` | — | false |
| APR, both sides (P3/L3) | `LendingParams.apr_bps` | `set_lending_params` | 1 ≤ v ≤ 10,000 (> 0 needed for termination) | 2,000 (20%) |
| Charge cadence (P15) | `LendingParams.charge_interval_blocks` | `set_lending_params` | 1 ≤ v ≤ 7,200 | 360 |
| Equity cutoff (P5) | `LendingParams.equity_floor` (TAO, u64) | `set_lending_params` | 0 ≤ v ≤ 1,000 TAO (never negative) | 0 |
| Min interest cover (P5) | `LendingParams.min_interest_cover_blocks` | `set_lending_params` | 1 ≤ v ≤ `withdraw_buffer_blocks` | 7,200 (1 day) |
| Deleverage pace, both sides (P7/L7) | `LendingParams.pace_bps_per_block` | `set_lending_params` | 1 ≤ v ≤ 100 | 5 (0.05%) |
| Pause band, TWAP term, both sides (P17/L9) | `LendingParams.pause_twap_bps` | `set_lending_params` | 10,001 ≤ v ≤ 50,000 | 12,500 |
| Pause band, SMA30 term, both sides (P22/L9) | `LendingParams.pause_sma_bps` | `set_lending_params` | 10,001 ≤ v ≤ 50,000 | 12,500 |
| Min pool depth (P18) | `LendingParams.min_pool_tao` | `set_lending_params` | 100 TAO ≤ v ≤ 100,000 TAO | 1,000 TAO |
| Min position, both sides (P10/L10) | `LendingParams.min_notional` | `set_lending_params` | 0.1 TAO ≤ v ≤ 10,000 TAO | 10 TAO |
| Deposit, both sides (P19/L11) | `LendingParams.deposit` | `set_lending_params` | 0 ≤ v ≤ 10 TAO | 0.1 TAO |
| Withdraw buffer, both sides (P20/L12) | `LendingParams.withdraw_buffer_blocks` | `set_lending_params` | `min_interest_cover_blocks` ≤ v ≤ 216,000 | 50,400 (7 days) |
| Deleverage exit margin, both sides (P5, v2.0.1) | `LendingParams.exit_hysteresis_bps` | `set_lending_params` | 0 ≤ v ≤ 5,000 | 500 (5%) |
| TWAP samples (P6/L6) | `LendingParams.twap_samples` | `set_lending_params` | 3 ≤ v ≤ 72 (ring bound) | 72 |
| TWAP sample interval (P6) | `LendingParams.twap_interval_blocks` | `set_lending_params` | 50 ≤ v ≤ 1,200 | 300 |
| Short stress (P2) | `ShortParams.stress_bps` | `set_short_params` | 10,000 ≤ v ≤ 100,000 | 30,000 (3×) |
| Short book slope (P2) | `ShortParams.stress_slope_bps` | `set_short_params` | 0 ≤ v ≤ 100,000 | 20,000 (2) |
| Net mint per block (P8) | `ShortParams.open_limit_bps` | `set_short_params` | 1 ≤ v ≤ 1,000 | 100 (1% of pool α) |
| Long stress (L2) | `LongParams.stress_bps` | `set_long_params` | `stress_floor_bps` ≤ v ≤ 100,000 | 20,000 (2×) |
| Long stress floor (L2) | `LongParams.stress_floor_bps` | `set_long_params` | 10,000 ≤ v ≤ `stress_bps` | 15,000 (1.5×) |
| Lift cap (L5) | `LongParams.lift_cap_bps` | `set_long_params` | 1 ≤ v ≤ 5,000 | 1,000 (10% of SubnetTAO) |
| Net lift per block (L8) | `LongParams.open_limit_bps` | `set_long_params` | 1 ≤ v ≤ 1,000 | 100 (1% of pool TAO) |
| Dereg zone (P16) | `DeregParams.zone_bps` | `set_dereg_params` | 10,000 ≤ v ≤ 30,000 | 12,500 (1.25 × lowest SMA30) |
| T_ema window in r (P12) | `DeregParams.pool_tao_ema_blocks` | `set_dereg_params` | 300 ≤ v ≤ 216,000; the EMA keeps its value and only the smoothing factor changes | 7,200 |
| Rank SMA window (P13) | `DeregParams.rank_window_days` | `set_dereg_params` | 7 ≤ v ≤ 30 (ring bound); a change bumps `RankGeneration` and re-seeds rings | 30 |
| Longs per subnet (7.4) | `DeregParams.max_long_positions_per_subnet` | `set_dereg_params` | max current `LongPositionCount` on any subnet ≤ v ≤ 1,024 (`MaxLongPositionsCeiling`, benchmarked) | 256 |

**Re-seeding on window changes** (ties into the `RankGeneration` / I25 rule):
* **`twap_samples` or `twap_interval_blocks`:** in the `on_initialize` that applies the change, every live subnet's `TwapRing` is resized and filled with that subnet's current TWAP_median. It is not filled from spot, because a one-block spot at the switch would otherwise become the whole window.
* **`rank_window_days`:** bumps `RankGeneration` and re-seeds every ring with its current published SMA30 (`RankPrice`), not `SubnetMovingPrice`, so a window change never imports the EMA. I25 holds from that block.
* **Note on the TWAP sample count:** for an even count (72 at launch), the median is the lower middle sample, as today.

**Not settable (by decision):**
* **21M alpha supply cap (P23):** a protocol constant (`TotalSupply`), removed from Jacob's list. `AlphaSupplyCap` always compares against 21,000,000 α.
* **Interest destination (P4):** not a parameter. Since v1.4, 100% goes to SubnetTAO and no other destination (burn, reserve) exists to share with. Making it a share would need a sink, which is a design change, not a setting.
* **`MaxLongPositionsCeiling` (1,024):** a runtime constant, because the prune weight is benchmarked against it. Only `max_long_positions_per_subnet` under it is settable.

**Parameter count (v2.0 consolidation):**
* **Before:** 32 distinct tunable values. 23 were settable in `SynthParams` (12) and `LongParams` (11). 9 were hard-coded: short deposit, short withdraw buffer, charge cadence, equity cutoff, interest cover, short TWAP pause band, T_ema window, rank window, and the longs cap.
* **After:** 24, all settable (v2.0.1: 25 with `exit_hysteresis_bps`, so `LendingParams` 14): `LendingParams` 13, `ShortParams` 3, `LongParams` 4, `DeregParams` 4 (with `rank_window_days` and the longs cap in Dereg).
* **What went:** 7 duplicate long fields (APR, pace, min notional, deposit, withdraw buffer, and two pause terms) and `sma_pause_max_blocks`. Plus the two enable flags.

## 3. Branch changes (old branch `cursor/lift-lending-escrowed-raise-352f`)

Rough net effect: the lending pallet goes from about 10.5k to about 5.0–5.5k LOC (≈ −6.5k, +1.3k new); tests from 14.7k to about 7k (rewrite); the SDK and CLI from about 4.7k to about 2.5k.

### Keep (light edits)
* `pallets/lending/src/lib.rs`:
  * pallet skeleton, `Enabled`, root param setter pattern, custody/escrow accounts (`EscrowOwner`, custody hotkey)
  * `Positions` / `OpenByNetuid` / `PositionCount` / `TotalPositions` (new struct)
  * interest accrual and retry machinery (`Due*`, `InterestRetry*`)
  * dissolution plumbing (`start_dissolution`, `settle_positions_on_dissolution` cursor/meter, `finish_dissolution`, `Dissolutions*`)
  * *(v2.0: not kept)* EMA-band helpers (`spot_within_settlement_band`, `ensure_settlement_ema_band`). P8 is a net-mint limit and no EMA price reference remains, so delete them
  * paced buy-back helpers (`settle_with_buyback_search_weight`, `slice_bps` logic)
  * coldkey-swap re-key, `on_initialize`/`on_idle` budget walkers
* `pallets/subtensor/src/subnets/dissolution.rs`, `registration.rs`, `subnet.rs`, `staking/set_children.rs`, `staking/remove_stake.rs`, `swap/swap_coldkey.rs`: keep the `T::Lending::*` hook calls (custody guards, dissolution, coldkey swap).
* `common/src/proxy.rs`: keep `ProxyType::Lending` (index 23), with the doc text renamed to shorts. `runtime/src/proxy_filters/*`: update the call group to the new extrinsics.
* v2.0: delete `NotionalEma` and `notional_alpha_price` (the emission-EMA read). Interest and min notional read the TWAP median, and `reference_price` = max(spot, TWAP_median, rank SMA30). Delete the `sma_paused_blocks` increment in the deleverage step. Replace `SynthParams` with `LendingParams` + `ShortParams`, add `DeregParams`, and move the long duplicates into `LendingParams`.
* `benchmarking.rs` / `weights.rs`: keep the framework, rebenchmark.
  * **v1.9.1:** `register_network` (prune path) and root `dissolve_network` must also charge the lending phase A worst case: `long_prune_sale_weight(max_long_positions_per_subnet)` (read in the weight closure; benchmarked up to `MaxLongPositionsCeiling` = 1,024, v2.0) = 1 swap + 256 × (position read and remove, 2 balance transfers, an event). Benchmark it with a full 256-long book.
  * **v1.9.1:** `set_enabled` charges an O(subnet_limit) ring re-seed (P13), and the runtime-upgrade migration charges the same once.

### Delete
*v1.3 note: the #3268 long code below stays deleted. Section 7's new longs are written fresh under new names (`LongPositions`, `LongLiftedTao`, …). No `Lifts`/`LongInterest*` storage is reused.*
* Everything `Side::Long`:
  * `do_open_long_and_sell`, `open_long_and_sell*`, `simulate_long_escrow_raise`, `long_book_tao_cap/used`, `ensure_long_book_pool_tao`, `skip_long_tao_flow`
  * `LongInterestPending`, `LongInterestSince`, `apr_long_bps`, `ltv_long_bps`, `thin_ltv_long_bps`, `Position.escrow_alpha`
* Lift/readd (borrowed pool alpha):
  * `Lifts`, `LiftBook`, `LiftParams` (replaced by `ShortParams` / `LendingParams` / `DeregParams`, v2.0), `lift_*`, `readd_lifts_for_dissolution`, `quote_raise*`, `alpha_to_raise_tao`, `unstake_for_raise`, `side_lift_tao`
  * in `pallets/subtensor/src/staking/lending.rs`: lift/readd and the `SubnetTAO`/`SubnetAlphaIn` debits
  * `pallets/swap/src/pallet/impls.rs` lending readd branch (`~L219–240`)
  * `primitives/swap-interface` lending lift sizing and the `k_neutral` baseline-restore burns
* Term/expiry: `Expiry*`, `ExpiryRetry*`, `ExpiryAttemptsLeft`, `LastDeferral`, `expires_at`, `roll` and its deferral (about 1.7k LOC).
* Pool-share short cap and utilization: `short_book_alpha_cap/used`, `ensure_short_book_pool_alpha`, `short_book_utilization_bps` (replaced by the issuance-based APR).
* Long emission add-back in `staking/stake_utils.rs::emission_alpha_price`: return `spot`. Drop the `emission_reserve_offsets` long fields.
* `subnet_emissions.rs` `LendingSkipTaoFlow` long logic.
* `young_short_settlement_window` (superseded by P12).
* Docs and SDK pages for raise, roll, repay-long and long quotes.

### Rewrite
* `Position` → `ShortPosition` (section 4).
* `open_exact` → `open_short`; `add_collateral` (keep); `close` → `close_short` (partial and full); new `withdraw_collateral`, `poke` and the v2.0 grouped setters (`set_lending_params`, `set_short_params`, `set_dereg_params`; section 2.1).
* `staking/lending.rs` adapter → `mint_and_sell(netuid, q, min_out)` and `buy_and_burn(netuid, a, max_in)`:
  * minting goes through `SubnetAlphaIn` / `SubnetAlphaOut` on the custody hotkey and records `SyntheticOutstanding`;
  * burning goes through the existing burn path.
  * About 250 LOC, down from 697.
* `stake_utils.rs` (v1.7): **delete** `alpha_issuance_for_schedule`. The `Lending::synthetic_and_residual` hook is kept as `lending_alpha` for `legacy_owner_cut_accrued` only (v1.8). `root_proportion` (`block_step.rs`) and `get_subnet_terms` (`run_coinbase.rs`) go back to `get_alpha_issuance`. The short open path adds the P23 check `get_alpha_issuance + q ≤ TotalSupply` (`AlphaSupplyCap`), on the pre-trade S.
* `lending/src/lib.rs` (v1.9): `equity_twap` (currently `buy_cost_on(pool_state, pos.minted)`, lib.rs:2794) uses `book_share(pool_state, SyntheticOutstanding, pos.minted)`. Factor `book_share` out of `book_need` (lib.rs:2830), and mirror it for the long equity with `LongLockedAlpha`. The deleverage trigger, `poke` and withdraw-health all read the new equity. Q_book is one subnet read per evaluation, so evaluation stays O(1) per position.
* `subnets/emission_weights.rs` (v1.8 correction): `baseline_root_emission_share` also reads `alpha_issuance_for_schedule`. Switch it to `get_alpha_issuance`, consistent with `root_proportion`, so root's baseline share falls slightly while a book is open. `legacy_owner_cut_accrued` (the legacy lock-refund deduction) must **not** count lending alpha. Keep a `Lending::lending_alpha(netuid)` hook (SyntheticOutstanding + ResidualAlpha) and subtract it there only. Remove the P21 breaker state and checks. Dereg ordering (P13) adds a `dereg_price(netuid)` helper in `coinbase/root.rs::get_network_to_prune`.
* Dissolution settlement: price per P12; shorts pay q × price into the staker pot, falling back to C, then residual.
* SDK (`sdk/python/bittensor/{reads,intents,cli/commands}/lending.py`, `_generated/*`) and docs (`docs/tx/*loan*`, `docs/query/lending-*`, `docs/errors/chain/*`): regenerate for the short-only calls.

## 4. Storage, extrinsics, events, errors

**Storage (pallet_lending)**
* `Enabled: bool`
* v2.0 parameter storage (root-only, section 2.1; each has a `Pending…` twin applied in the next `on_initialize`):
  * `LendingParamsStore: LendingParams { apr_bps, charge_interval_blocks, equity_floor, min_interest_cover_blocks, pace_bps_per_block, pause_twap_bps, pause_sma_bps, min_pool_tao, min_notional, deposit, withdraw_buffer_blocks, twap_samples, twap_interval_blocks, exit_hysteresis_bps }` (shared by both sides; `exit_hysteresis_bps` v2.0.1)
  * `ShortParamsStore: ShortParams { stress_bps, stress_slope_bps, open_limit_bps }` (replaces `SynthParams`; `zone_bps` moves to Dereg, `buy_ceiling_sma_bps` becomes `pause_sma_bps`, `sma_pause_max_blocks` is deleted)
  * `DeregParamsStore: DeregParams { zone_bps, pool_tao_ema_blocks, rank_window_days, max_long_positions_per_subnet }`
* `Positions: DoubleMap<(AccountId, NetUid) → ShortPosition { custody, minted: u64, collateral: u64, proceeds: u64, deposit: u64, zone_minted: u64, zone_proceeds: u64, opened_at, last_accrued, interest_remainder, state: Open | Deleveraging{since} , terms_version }>` (v2.0: `sma_paused_blocks` removed)
* `SyntheticOutstanding: Map<NetUid → u64>`, `ResidualAlpha: Map<NetUid → u64>`
* **Removed v1.4:** `ResidualReserve`, `RetireCursor` (present in the branch at cfe087e89) and the retire sweep in `on_idle`.
* `TwapRing: Map<NetUid → BoundedVec<u64 price, 72>, cursor, last_sample_block>` (sampled every 300 blocks; read as a median)
* `RankRing: Map<NetUid → BoundedVec<u64, 30> daily means + hourly accumulator>` (P13, seeded at registration; lives in subtensor `coinbase/root.rs`)
* `BlockNetMinted: Map<NetUid → (block, i128)>` (P8, cleared on_finalize)
* `DeleverageQueue: BoundedVec<(AccountId, NetUid)>` plus a cursor; kept: `Due*`, `InterestRetry*`, `Dissolutions*`, `OpenByNetuid`, `PositionCount`, `TotalPositions`, `EscrowOwner`

**Extrinsics**
* `open_short(netuid, mint_alpha, collateral, min_tao_out)`
* `increase_short(netuid, extra_mint, extra_collateral, min_tao_out)`
* `add_collateral(netuid, tao)`
* `withdraw_collateral(netuid, tao)` (only while coverage holds)
* `close_short(netuid, alpha, max_tao_in)`
* `poke(who, netuid)` (permissionless health check and deleverage step)
* root (v2.0, section 2.1): `set_enabled(bool)`, `set_lending_params(LendingParams)`, `set_short_params(ShortParams)` (replaces `set_synth_params`), `set_dereg_params(DeregParams)`; `set_longs_enabled` and `set_long_params` are in 7.5

**Events**
* `ShortOpened{who, netuid, minted, proceeds, collateral}`, `ShortIncreased{..}`
* `CollateralAdded{..}`, `CollateralWithdrawn{..}`
* `ShortClosed{who, netuid, burned, tao_spent, refund}`
* `InterestCharged{who, netuid, side, to_pool}`
* `DeleverageStarted{who, netuid, equity_twap, book_cost}`, `DeleverageEnded{who, netuid, equity_twap, book_cost}` (v2.0.1) (v1.9: `book_cost` = BuyCost(Q_book) used for the share), `DeleverageStep{burned, tao_spent}`
* `ShortDefaulted{who, netuid, residual_alpha}` (`ResidualRetired` removed v1.4)
* `ShortSettledOnDissolution{who, netuid, price, paid}`
* `LendingParamsSet{old, new}`, `ShortParamsSet{old, new}`, `DeregParamsSet{old, new}` (v2.0; replace `SynthParamsSet`), `EnabledSet{..}`

**Errors**
* `Disabled`, `SubnetNotEligible`, `BelowMinNotional`
* `CoverageNotMet` (including a book larger than the pumped pool), `SlippageExceeded`, `OpenImpactTooHigh`
* `NoPosition`, `PositionDeleveraging`, `WithdrawBreaksCoverage`
* `DissolutionInProgress`, `InvalidParams`, `CustodyUnavailable`, `Overflow`
* `AlphaSupplyCap` (P23, v1.7; replaces `DilutionBreakerActive`), `PoolTooShallow` (P18), `WithdrawBufferBreached` (P20)

## 5. Invariants (assert in try-state and in tests)
* **I1** Σ open and deleveraging `minted` per netuid == `SyntheticOutstanding[netuid]`.
* **I2** Alpha is created only by open/increase and destroyed only by close, deleverage and dissolution. `ΔIssuance == Δminted − Δburned`.
* **I3** Custody TAO == Σ (collateral + proceeds + deposits). The protocol never owes TAO; no balance goes negative.
* **I4** Every open, increase and withdraw ends with the coverage rule satisfied for every open position: the whole book pro rata at Stress_eff. An open that grows the book re-checks the opener only. Other positions can fall short through book growth, but they are judged by TWAP health, not coverage.
* **I5** Charged interest ≤ collateral. Once collateral is below 1 day of interest, the position is in Deleveraging within one `poke`/idle pass.
* **I6** `emission_alpha_price == spot`. Shorts affect emission only through the pool.
* **I7** (v1.7) The halving schedule and `root_proportion` use `get_alpha_issuance`, which includes SyntheticOutstanding and ResidualAlpha. An open of q raises it by exactly q, and a close or burn of q lowers it by exactly q.
* **I8** Dissolution settles each position exactly once at q × r. Pot (pool TAO + Σ q·r) ≥ Σ staker payouts, and each position's q·r ≤ C + P. Zone-minted alpha settles at zone_proceeds ≤ P.
* **I9** (v2.0) A short deleverage buy executes only if spot ≤ min(TWAP_median × `pause_twap_bps`, SMA30_rank × `pause_sma_bps`)/10000. No lapse. Per-block price impact from short deleverage is ≤ `pace`, and from all opens in the block combined is ≤ `open_impact_bps`. Deleverage fills are pro rata to each position's remaining q.
* **I11** No open or increase on either side succeeds while P18 (SubnetTAO < `min_pool_tao` before or after the trade) applies. No close, top-up, deleverage, default or dereg step ever fails because of P18. Σ zone_minted ≤ minted per position.
* **I21** (v1.7) `get_alpha_issuance(netuid) ≤ 21,000,000 α` after every block, for every netuid. No open or increase succeeds if it would break this.
* **I22** *(removed in v1.9 with P24)*
* **I23** (v1.9) For every subnet and side:
  * Σ_pos H_pos == the whole-book cost BuyCost(Q_book | live pool) × TWAP/spot (shorts), and Σ_pos V_pos == SellProceeds(A_book) × TWAP/spot (longs), within N rao of rounding (N = positions).
  * Splitting a position into N positions with the same total q and funds leaves every piece's health sign, and the deleverage trigger block, unchanged (up to rounding).
  * A lone position's H_pos equals BuyCost(q_pos) exactly.
* **I25** (v1.9.1) At the end of every block, every live non-root subnet has a full ring (30 daily means) with `RankRingGeneration == RankGeneration` and a published `RankPrice`, whether lending is enabled or not.
  * No ranking or coverage read (`dereg_rank_price`, `prune_rank_price`, `rank_price`) returns the emission EMA, zero, or a partial mean.
  * After the upgrade migration and after each enable, every ring equals 30 copies of that block's `SubnetMovingPrice` until new samples roll in.
* **I26** (v2.0) Parameters:
  * Every value in `LendingParams`, `ShortParams`, `LongParams` and `DeregParams` satisfies the section 2.1 bounds at all times.
  * Live params change only in `on_initialize` from a `Pending…` value written by a root setter, and every change has exactly one `<Group>ParamsSet` event.
  * No setter call re-checks or mutates any position.
  * `TotalSupply` (21M) is not a field of any params struct.
* **I27** (v2.0.1) Deleverage exit, both sides:
  * A position leaves `Deleveraging` only by exit, full close, default or dissolution settlement. An exit happens only at a trigger check point, only when the section 1 item 4 (or 7.1 item 5) exit test holds on that block's values, and never on a dissolving subnet.
  * Right after an exit the position passes the trigger test, and it is not in `DeleverageQueue`. An exit never mutates q, P, C, D or A.
  * With `exit_hysteresis_bps = 0`, after any check point a position is `Deleveraging` iff the trigger test fails or it failed at an earlier check point and has not passed since.
* **I10** When disabled: no opens or increases; close, add_collateral and poke still work.

## 6. Test plan

**Unit tests (pallet mock)**
* Open/close round trip: with no external flow, the cost equals fees and interest. Partial close, `increase_short`, and withdraw blocked when coverage would break.
* Coverage math against the Python reference (`sim2.size_by_collateral`) on a golden set: 50/50 pools, q/x of 1–60%, stress 1.5/2/3, within 1 rao.
* v1.9 health: H_pos golden values against `split_health_v19.py`. Σ H_pos vs the whole-book cost within N rao (I23). A 1/10/100-way split of the same book gives the same trigger k. A lone position gives the same H as v1.8. Q_book ≥ pool alpha makes every position unhealthy. The long mirror is tested the same way.
* Whole-book pro-rata coverage and Stress_eff against `sim3.need_book` (golden set); a book exceeding the pumped pool fails with `CoverageNotMet`. Minimum position. P8 net-minted limit across accounts and with mint+burn in one block.
* Interest: per-block accrual with remainder carry, the 360-block charge, the full charge credited to SubnetTAO with no inflow, volume or flow record (I19), flat APR (with a root change mid-period), issuance including synthetic alpha (v1.7), and C exhausted in the middle of a period.
* TWAP ring (median): one-block ×145 spike leaves the TWAP unchanged, wrap-around, a subnet that registers mid-window, a pruned and re-registered netuid, a one-block spike, and a sustained move.
* Deleverage: pace respected per block, partial fills across blocks, default when funds run out, and residual recorded and left outstanding (no retire path exists).
* Dissolution at r: T = pool TAO only, all_alpha including synthetic and residual, no reserve in the pot, and a solvency check against the I8 inequality on random states. Mixed healthy, deleveraging and defaulted positions. The pot is ≥ the no-short pot. A cursor resumes across blocks.
* P13 ranking: the SMA ring (wrap-around, a newly registered subnet seeded at its first price, a re-registered netuid reset). v1.9.1: upgrade prefill with lending disabled, re-seed on enable and on a `RankGeneration` bump, sampling continuing while disabled, and no EMA, zero or partial-mean fallback on any read (I25). the raw price with no add-back, and ties broken by registration block.
* Coldkey swap re-keys positions. Proxy filter allows only the short calls. Toggling disabled. Root param bounds.
* Issuance schedule moved by exactly q per open and close (I7). P23: an open at 21M − q succeeds, at 21M − q + 1 rao it fails with `AlphaSupplyCap`, and the same check applies to an increase. A mock coinbase at S = 21M − ε emits ≤ 4ε/21M α and never crosses 21M (I21).
* **Longs (v1.3):**
  * Atomic lift-buy (I13), with cap and L8 boundaries.
  * Coverage at m = 2 and the floor 1.5 (the setter rejects 1.49).
  * Health at TWAP, paced down-budget, L9 pause, and default writing off D from `LongLiftedTao` as a pool shortfall.
  * Partial and full close, including `CloseInsolvent`.
  * Locked-alpha guards on every subtensor stake path.
  * Coldkey swap re-keys `LongPositions` and all side-aware maps.
  * The Side::Short migration with an empty and a non-empty fixture.
  * P18: opens and increases at 999.999 / 1,000 TAO, before and after the trade, on both sides. An open that would take SubnetTAO below 1,000 fails. Close, top-up, withdraw, deleverage, default and dereg succeed on a shallow pool.
  * Emission price is raw with long alpha (L15 rejected): golden check against `sim.E_block` after a 10% lift.
  * P17/P22 (both sides, shared band): the min/max reference with no lapse, and a 60-day sustained rally/decline that stays paused until C runs out (v2.0).
  * v2.0 references: coverage ref == max(spot, TWAP_median, SMA30) for shorts and min(spot, TWAP_median) for longs; interest and min notional on q × TWAP_median; no read of `SubnetMovingPrice` or `NotionalEma` in lending except the I25 ring seed.
  * v2.0 params: each setter's bounds at the edges (±1), `stress_bps < stress_floor_bps` rejected, `withdraw_buffer_blocks < min_interest_cover_blocks` rejected, apply-next-block, ring re-seed on TWAP/rank window change. The prune-step sale ignores pace and pauses.
  * Dissolution (v1.9.1): phase A in one call, with one swap, the pro-rata split, lender-first repay / C gap / refund, shortfall write-off, the permutation test (I24) and the `TooManyLongPositions` bound. Then B/D/E/F resume across blocks. r is read after phase A.
  * I12–I18 in try-state.
* **Deleverage exit (v2.0.1, both sides):** a position at 1.04× H_pos (shorts) or 1.04× D (longs) stays `Deleveraging`, at 1.05× it exits; the same at the cover term (1.04 vs 1.05 days of interest); a top-up via `add_collateral` exits in the same call; an exiting position takes no fill that block; no exit on a dissolving subnet; h = 0 makes exit equal to "not triggered"; setter bounds 0 and 5,000 accepted, 5,001 rejected; I27 in try-state. Differential: the reference model with the same h gives the same trigger and exit blocks on the squeeze, rally and cascade scenarios.

**Mainnet-clone e2e (Finney clone, as in Greg's #3268 QA)**
1. **Green path** on SN91, SN101, SN112, SN128 and SN51: open, increase, add collateral, partial close, full close with refund. Check the interest-to-pool and issuance reconciliations, as Greg did.
2. **Squeeze:** short a 25% push at stress 3×, then a team account pumps +100%, +200% and +400% and holds for 24 h, 72 h and 7 days. Expect no deleverage through +200%/7d, deleverage at +400%, and per-block pacing respected. Repeat at stress 2× to confirm the defaults the sim predicts.
3. **Defend-and-hold:** the team buys back to the open price and holds. The shorter closes on day 1 and day 7. Expect a loss of about 25–50% of proceeds, no deleverage, and SubnetTAO up by exactly the interest charged.
4. **Dereg griefing:** pin SN79 (or another mid-ranked subnet) at 0.5 × L for 30 days, with and without a scripted dip-buyer. Expect the 30-day SMA to cross L at about day 23 with no buyer and never with the buyer. Then pin SN29 at 0.3 × L and expect it to be the prune candidate within about 15 days. On prune, check that shorts pay q × EMA into the pot, that the minted alpha is burned, and that the pot is ≥ the no-short pot.
5. **Default and dilution:** a 1.5× position (via a root param), pump to default, residual recorded, issuance up by the residual, residual permanent until dissolution, staker balances diluted pro rata, and opens still allowed afterwards (v1.7: no breaker). The residual counts in issuance, so the halving schedule moves by exactly the residual.
6. **Interest edge cases:** a position at minimum notional, a withdraw down to the P20 buffer, C covering exactly 1 day of interest, a charge landing mid-deleverage, a root change to the flat APR between accrual and charge, a root param change between accrual and charge.
7. **Cascade:** 5 positions at stress 1.5–3× on one subnet, then a pump. Check that fills are pro rata (not FIFO), that pacing is shared across positions, that the P17/P22 pause holds while spot > 1.25 × min(TWAP, SMA30), and that no TAO is owed.
8. **Emission interaction:** push a subnet across the gate bar (e.g. SN61, 2% drop) and check emission share over the next tempos against `sim.E_block`.
9. **Chain-buy sink:** pin SN112 against its chain buys for several days. Confirm that alpha bought by chain buys (`SubnetProtocolAlpha`) can't be bought back and that coverage tightens accordingly.
10. **Team exit:** on the SN101 clone, the team account holds all of AlphaOut and sells the miner share each block. A shorter pins spot at the emission-plus-root implied path (section 10) for 30 days. Then the team dumps. Expect team TAO about 13% below the no-short control, pool TAO plus short escrow ≥ the control, and the shorter closing after the dump at a profit.
11. **Dereg snipe:** SN40, the current candidate. Open the coverage-maximum short one block before a registration. Expect zone_minted = q, and on prune a settlement at zone_proceeds, so the net is −interest. Control (P16 off): about +241 TAO, with stakers 272 worse off.
12. **TWAP spike:** on SN101, a ×145 one-block spike at the sample block. Expect no deleverage with the median and deleverage with the mean (control).
13. **Pump into deleverage:** trigger deleveraging, then pump to 2× and hold for 12 h. Expect no buys while spot > 1.25 × TWAP, and buys resuming as the TWAP catches up.
14. **Multi-account open:** 10 coldkeys each mint 0.2% of SubnetAlphaIn in one block. Expect the 6th onward to fail with `OpenImpactTooHigh`. A third party selling 2% of real alpha in the same block must not block an open, and a mint+burn in the same block must not consume the limit.
15. **Worst-case dilution:** 10 positions push SN29 by 9–90% over 5 days, then a team pumps 10× and holds 7 days. Expect residual ≤ about 7% of issuance with slope 2, no TAO owed, and opens still allowed (v1.7).

**Longs e2e (v1.3).** SN29 clone unless noted. Expected values come from `longs_sim.py` at m = 2 and a 10% cap.
16. **Team long, then dump:** a team account opens the cap-size long, then dumps all real alpha, and the long defaults.
    * Expect the team below the no-long baseline (sim: 3,198 vs 3,387).
    * Expect the written-off shortfall ≤ 151 TAO, and SubnetTAO lower by exactly that against the lender claim.
    * Expect I12–I16 to hold.
17. **Pump, then long, then dump:** the team pumps 2× with its own TAO and holds 24 h, opens the cap long, then dumps. Expect the team below the pump-and-dump control (sim: 3,065 vs 3,392).
18. **Short in place, team long cancels it:** a 1× short is open, then the team opens a long (whose buy removes pool alpha) and dumps. Expect the team ≤ the shorts-only result (sim: 2,474 vs 2,613).
19. **Squeeze with long leverage:**
    * The team opens a long and pumps with its own TAO, holding ≥ 36 h to force a minimum-collateral short. Then it closes the long at the top and dumps.
    * Record own-TAO and collateral needed (sim at 10%: 7.6k vs 8.3k without longs) and the team result (4,228 vs 4,128).
    * Also run the "hold just above threshold → short defaults" path. With P22 the short must close without residual (e2e 27–30).
20. **Third-party long as exit liquidity (accepted behaviour):** an outside long at the cap, then the team dumps.
    * Expect the long to deleverage paced and paused.
    * Expect the shortfall bounded and written off. The team gains vs baseline by design (sim +190).
    * Expect no TAO to reach the team from the lifted TAO except through its own alpha sales.
21. **Prune delay (accepted behaviour):** a long is opened on SN29 under a standing short.
    * Expect the ranking SMA to rise and prune to be delayed (sim: day 26 → 36 at 10%).
    * At dissolution, expect the v1.9.1 one-shot sale inside the prune: lender-first repayment, then refund; nothing settles at r.
    * A team-owned long must end below no-long (sim at 10%: 2,682 vs 2,690; at 25%: 2,626).
22. **No free option at dereg:** check that no path settles a long at its entry price. A long into a dying subnet loses (sim at 25%: −403 of 706 collateral).
23. **Entry and exit price gaming:**
    * Sandwich an `open_long` (front-run buy, back-run sell). Expect `min_alpha_out` to bind.
    * Same-block lift-and-close, and multi-account lifts in one block. Expect L8 to cap the net lift per block, and lift-then-close in one block not to consume the limit.
24. **Cap gaming:** pump SubnetTAO with own TAO to raise the 10% cap, open the larger long, then unwind the pump and dump. Expect cap checks on pre-open SubnetTAO, no breach of I14 at open, and the team below the control.
25. **Pause griefing on long deleverage:**
    * Trigger a long deleverage, then the team re-dumps each time the TWAP catches up.
    * Expect no sells while spot < TWAP/1.25, interest still accruing, and default only when C is exhausted.
    * Expect the bad debt to match the sim bound.
26. **Longs and shorts in the same block:** a lift-buy followed by a short mint, and the reverse, plus a coldkey trying both sides (`OppositeSideOpen`).
    * Expect each book's coverage to use the post-trade pool.
    * Expect up/down budgets metered separately, and I1–I20 to hold.

**P22 and pause e2e (v1.5).** SN29 clone, 1×-budget short at minimum collateral (Q = 18% of pool alpha, C = 7,235). The team pumps with its own TAO to k× post-short spot, holds, and dumps one day later. Expect no residual in any of 27–30, and buys only while spot ≤ 1.25 × min(TWAP, SMA30). Sim values (team / shorter, β = 0):
27. **6× held 3 days:** 2,800 / +761. The short closes at h ≈ 96, after the dump.
28. **6× held 10 days:** 3,194 / +738. Closes at h ≈ 264.
29. **6× held 30 days:** 6,961 / −1,778. Closes at h ≈ 743, when the SMA term catches up. Premium +916 vs the same pump with no short.
30. **10× held 30 days:** 9,547 / −2,756. Premium +1,404.
31. **Liveness, permanent re-rating and rally:**
    * 6× held for 150 days. v2.0 (no lapse): expect buys paused until SMA30 catches up (about 23 days at 6×), then buys under the band, then default with residual ≈ 102k α (v1.5 no-lapse run; ≈ 1.6% of issuance).
    * A 4%/day rally for 45 days. Expect default with residual ≈ 57k α.
    * A 2%/day rally. Expect no deleverage.
32. **Long mirror (bear raid):**
    * An outside 10% long at m = 2. The team dumps 50% of its alpha, waits 3–7 days and buys back.
    * Expect the long's sells paused while spot < max(TWAP, SMA30)/1.25 and the long to close in profit (sim +7 to +11, team 3,848–3,851 vs 3,935 without a raid).
    * Control with the TWAP-only pause: the long loses its 379 collateral and the team gains +158.
    * Also: a steady 2–5%/day decline into dereg. Expect the long paused until prune, then sold in one shot in the prune step (v1.9.1). Any shortfall is written off.
33. **Pool falls below the P18 depth (v1.6):** a clone subnet at about 1,100 TAO with an open short and an open long. Sell pool TAO out until SubnetTAO < 1,000.
    * Expect `PoolTooShallow` on a new short, a short mint increase, a new long and a long lift increase.
    * Expect these to succeed: a partial and a full close on each side, a collateral top-up, and a collateral withdraw within coverage.
    * Force each side into deleverage (pump for the short, dump for the long). Expect paced forced close with the P17/P22/L9 pauses as usual, then dereg settlement (v1.9.1: one-shot long sale in the prune step, then B/D/E/F).
    * Restore SubnetTAO to ≥ 1,000 and expect opens to succeed again.
34. **21M cap and supply counting (v1.7):** clone a subnet and set its SubnetAlphaOut so that S = 21M − 1,000 α.
    * Expect a short of 1,000 α to succeed and a short of 1,001 α (or an increase past the cap) to fail with `AlphaSupplyCap`.
    * Run 10,000 blocks of coinbase. Expect S ≤ 21M on every block, and alpha emission 0 once S = 21M.
    * Close the short and expect S to drop by q and emission to resume at the schedule rate for the new S.
    * On the SN29 clone, open a 30%-pool-α book. Expect `root_proportion` and the alpha-emission halving point to move exactly as the formula gives with S including the book, then revert on close. Residual from a forced default stays counted.
35. *(Removed in v1.9 with P24.)*
36. **Split-invariant health (v1.9):** SN29 clone. Open the same 18%-pool-α short book as 1, 10 and 100 coldkeys, each at minimum whole-book coverage (same total funds). Run a held pump ramp in each case.
    * Expect the first deleverage block to be identical across the three runs. Sim: all at 3.92×; v1.8 gave 3.92× / 5.41× / 5.59×.
    * Expect identical total burned per block afterwards, and Σ H_pos == the book cost within N rao at every block.
    * Repeat for longs (10% lift book split 1/10 ways, dump ramp).
    * Side effect: open a 1% short alone, then a 17% book from another coldkey. Expect the 1% position to stay healthy at the open and to trigger at ≈ 3.11× held pump, not 4.18×.
37. **Dereg with longs (v1.9.1):** SN29 clone with a 10% short book and an outside long at the 10% lift cap (plus a second and a 256th long for ordering and weight). Prune is triggered by a registration at day 10. Run three paths: flat; −30%/day for the 4 days before prune; +50% held 3 days before prune.
    * Expect in all three: one `LongBookSoldOnPrune` in the registration extrinsic before r is frozen; per-position `LongSettledOnDissolution` in ascending PositionId; I24 holds; shuffling position order changes nothing; shorts settle at r computed after the sale; no long alpha reaches stakers at r.
    * Model medians over 110 subnets (long P&L % of C / shortfall % of D): flat −23.9% / 0; crash −101% / 3.6% (max 23.5%); rally +51.4% / 0. SN29 flat: long −86, stakers 2,912, recycled 4,016.
    * The registration's charged weight covers the 256-long case.
38. **Rank rings (v1.9.1):** on a clone, run the runtime upgrade with `Enabled = false`.
    * Expect every live subnet's ring to be full with `SubnetMovingPrice` and `RankRingGeneration == RankGeneration` (I25).
    * Enable. Expect a re-seed. A griefer pinning SN98 at 0.95 × L should need ≥ 19 days to become prune candidate (model: 19.59 days prefill vs 0.50 days with EMA fallback).
    * Disable, wait 10 days, enable. Expect another re-seed from the current `SubnetMovingPrice` and the same ≥ 19-day bound.
    * Register a new subnet. Expect a seed at registration.
    * Delete a ring by hand (fault injection). Expect try-state to fail, and no ranking read to return the EMA or a partial mean.
39. **Root setters (v2.0):** for each of `set_enabled`, `set_longs_enabled`, `set_lending_params`, `set_short_params`, `set_long_params` and `set_dereg_params`:
    * a root call with in-bounds values changes the live value from the next block and emits one `…ParamsSet{old, new}`;
    * each field just outside its bound (and each cross-check: long stress < floor, withdraw buffer < cover, longs cap below a subnet's current count) fails `InvalidParams` with nothing changed;
    * a signed non-root call fails `BadOrigin`;
    * open positions keep their coverage state, while health picks up a raised APR or cover at the next check;
    * changing `twap_samples` or `rank_window_days` re-seeds the rings as in 2.1;
    * nothing can change the 21M cap.
40. **Deleverage exit (v2.0.1):** SN29 clone, a 10% short book at minimum coverage and an outside long at the lift cap.
    * Pump to trigger the shorts, let the paced buy-back run for 10 blocks, then let spot and the TWAP fall back. Expect `DeleverageEnded` only once equity is ≥ 1.05 × H_pos, not at 1.01×, and no re-entry at the next charge.
    * Trigger again, then `add_collateral` to 1.05×. Expect exit inside that call and no fill in that block.
    * Mirror on the long with a dump.
    * Set `exit_hysteresis_bps` to 0 and repeat the first case: exit at the first healthy check. 5,001 fails `InvalidParams`.

## 7. Longs (normative, v1.3)

### 7.1 Mechanism
1. **Open** `open_long(netuid, lift_tao L, collateral C, min_alpha_out)`. In one extrinsic and one storage transaction:
   * C moves from the owner's free balance to the position custody account.
   * `SubnetTAO −= L` (the lift: a pool-internal debit, never credited to any account).
   * L is swapped into the same pool to buy alpha A (normal swap math and fee), enforcing `min_alpha_out`. Net change to SubnetTAO is 0. SubnetAlphaIn falls by A and SubnetAlphaOut rises by A.
   * A is credited as stake of the position custody coldkey on `PalletHotkey`. It is **locked**: it can't be unstaked, moved, transferred or swapped by anyone but the pallet, and it is never released to the borrower.
   * The position records debt `D = L` (the lender claim, owed to the pool) and `A`.
2. **Open checks:**
   * `LongsEnabled`.
   * Lift cap: `LongLiftedTao[netuid] + L ≤ lift_cap × SubnetTAO` (pre-open).
   * Per-block net lift limit (L8).
   * Minimum notional (P10, shared).
   * P18 depth (1,000 TAO, before and after the lift-buy; `PoolTooShallow`).
   * **Coverage** (L2), evaluated after the buy with the whole long book counted pro rata.
3. **Coverage (L2):**
   `C ≥ D_pos − SellProceeds(LongLockedAlpha | pool after a hypothetical dump to ref/m) × A_pos/LongLockedAlpha`,
   with `ref = min(spot, TWAP_median)` (v2.0: EMA removed) and m = `LongParams.stress_bps` (2.0; the setter rejects values below `stress_floor_bps` = 1.5).
4. **Interest:**
   * Accrues per block on D at the shared **flat APR** (P3) and is charged from C through the shared `Due` queue (side-aware key).
   * **v1.4:** all long interest is credited to the subnet's SubnetTAO, the same as short interest (P4). No burn, no reserve. It is not counted as inflow or volume (I19).
5. **Health:**
   * A long is healthy while `Equity_TWAP = C + V_pos − D_pos ≥ 0` and C covers ≥ 1 day of interest.
   * **v1.9 (mirror of the short rule):** `V_pos = SellProceeds(A_book | live pool) × a_pos / A_book × TWAP/spot`.
     * A_book = `LongLockedAlpha[netuid]` (Open + Deleveraging).
     * Each share is rounded down; fee-free, as before.
     * It reuses the L2 coverage share calculation without the dump, so a lone long is valued exactly as in v1.8 and splitting gains nothing.
   * When either fails (same shared `equity_floor` / `min_interest_cover_blocks` as P5), it enters `Deleveraging`: a paced sale of A at the shared pace (P7), never a spot liquidation.
   * **Exit (v2.0.1, mirror of section 1 item 4).** A `Deleveraging` long returns to `Open` when `C + V_pos − D_pos ≥ equity_floor + h × D_pos` (launch: `C + V_pos ≥ 1.05 × D_pos`) **and** `C ≥ (1 + h) × min_interest_cover_blocks` of interest, with the same h, the same check points, no coverage re-check and no exit while dissolving. Emits `LongDeleverageEnded{who, netuid, equity_twap}`.
6. **Deleverage (mirror of P5/P7/P17):**
   * Each block the subnet's long down-budget sells locked alpha pro rata across deleveraging longs. The budget is ≤ `pace_bps_per_block` (shared, P7) price drop per block, metered separately from the short up-budget.
   * The budget is **paused while spot < max(TWAP_median × 10000/`pause_twap_bps`, SMA30_rank × 10000/`pause_sma_bps`)** (shared band, P17/P22). v2.0: no lapse; interest keeps accruing while paused.
   * Sale proceeds repay D first; C pays what remains once A is exhausted.
   * If D reaches 0 with alpha left, the rest is sold under the same pace and refunded with C.
   * If A and C are both exhausted with D > 0, the position **defaults**: the remainder is written off as a pool shortfall (item 7) and the deposit is forfeited to burn.
7. **Unpaid debt (v1.4):**
   * At default, `LongLiftedTao −= D_remaining`, and the amount is added to `LongFlowTotals.shortfall` and `LongDefaulted{shortfall}`.
   * Nothing repays it. The pool simply holds that much less TAO than its claim, and every holder bears it through the price.
   * At dereg it shows up as a lower T in r: the existing haircut. The forced close (items 5–6) is the only loss control.
8. **Close** `close_long(netuid, alpha, min_tao_out)`:
   * Sells alpha from A. Proceeds repay D first; any excess goes to the owner.
   * A partial close must leave coverage satisfied.
   * A full close must satisfy `proceeds + C ≥ D`, or it fails with `CloseInsolvent` (an insolvent long can only exit through deleverage).
   * The refund is C plus any surplus. The TAO paid to the borrower is only its own collateral and realized profit, never the lifted TAO.
9. **Other calls:** `add_long_collateral(netuid, tao)`, `withdraw_long_collateral(netuid, tao)` (leaves coverage + the shared withdraw buffer; P20), `poke_long(who, netuid)`.
10. **One position per (coldkey, netuid, side).** A coldkey may not hold a short and a long on the same netuid at launch (`OppositeSideOpen`). This is a test-surface cut, not a safety rule; other coldkeys can always take the other side.
11. **Dereg: one-shot lender-first sale inside the prune step** (v1.9.1, section 7.4). There is no paced phase A, and longs never settle at r.

### 7.2 Parameters (launch values; v2.0: long-only ones via `set_long_params`, shared ones via `set_lending_params`, section 2.1)
| # | Parameter | Value | Evidence |
|---|---|---|---|
| L1 | `LongsEnabled` | **false** (launch dark; independent of `Enabled`) | Root turns longs on per network. Shorts can run without longs |
| L2 | `LongParams.stress_bps` / `stress_floor_bps` | **20000 / 15000**. Ref = min(spot, TWAP_median) (v2.0: EMA removed). Whole long book pro rata | SN29: a team long followed by a dump of real alpha loses at every m ≥ 1.3 (−90 to −1,160 at m = 2, caps 5–50%). Breakeven at m ≈ 1.3; profitable only at m ≤ 1.2 (+3 to +121). On deep subnets m = 2 gives 1.5–2.0× leverage with liquidation at −50% (SN64, SN51, SN4) |
| L3 | *(merged into P3, v2.0)* | Shared `apr_bps` (2000) | One APR for both sides |
| L4 | Interest destination (v1.4) | **100% to SubnetTAO**, as P4 | Symmetric with shorts. A team-owned long now recycles its own interest into its own pool (section 15) |
| L5 | `lift_cap_bps` | **1000** of SubnetTAO per subnet (aggregate `LongLiftedTao`) | At 10%: SN64 capacity 19k TAO. The book's own entry impact is +11%. Worst bad debt on an SN29-sized pool after a team dump at m = 2: 151 TAO |
| L6 | TWAP | Shared `TwapRing`, median of 72 hourly samples | As P6 |
| L7 | *(merged into P7, v2.0)* | Shared `pace_bps_per_block` (5). The down-budget stays separately metered in `DeleverageBudget` (side-tagged) | One pace, two budgets |
| L8 | `long_open_limit_bps` | Net lifted TAO per block per subnet ≤ **100** bps of SubnetTAO, summed over accounts. Net = lifted − repaid by close in the block | Mirrors P8. Anti-flash only |
| L9 | *(merged into P17/P22, v2.0)* | Shared `pause_twap_bps` / `pause_sma_bps` (12500 / 12500), mirrored: sells run only while spot ≥ max(TWAP_median, SMA30_rank)/1.25. No lapse | v1.5 bear-raid evidence still applies: with the SMA term, the long closes +7 to +11 instead of losing its 379 collateral |
| L10 | *(merged into P10, v2.0)* | Shared `min_notional` (10 TAO of D) | |
| L11 | *(merged into P19, v2.0)* | Shared `deposit` (0.1 TAO) | |
| L12 | *(merged into P20, v2.0)* | Shared `withdraw_buffer_blocks` (7 days) | |
| L13 | *(removed v1.9.1)* | Was `long_unwind_blocks`, the minimum phase-A length. Phase A is now an atomic one-shot sale inside the prune step (7.4), so there is no window to size | Subtensor prunes inside `register_network` with no flag window (review v1.6 #3), so a paced sell-down can't run |
| L14 | Ranking price | **Raw** (long buying counts) | Accepted consequence: longs delay prune |
| L15 | Emission price for long-bought alpha | **Raw** (rejected v1.5: no add-back). Long-bought alpha counts in spot, EMA and emission exactly like any buy | Accepted consequence, quantified in 8.4: a full 10% lift raises price +11.1%. On 77 of 97 emitting subnets the extra emission exceeds the interest (SN64 +28.2 vs 10.5 TAO/day) |

### 7.3 Accounting
* **Lender claim:** `LongLiftedTao[netuid]` = Σ D_pos is a receivable of the pool. SubnetTAO itself never falls because of an open; the lift and buy legs net to zero inside one extrinsic.
* **Locked alpha:** `LongLockedAlpha[netuid]` = Σ A_pos.
  * It is ordinary issued alpha. It counts in alpha issuance like everything else (v1.7: so does SyntheticOutstanding).
  * It is held on `PalletHotkey` (not registered, so it earns no dividends and no validator weight).
  * It counts in all_alpha at dereg.
* **Flow:** the lift, buy, deleverage-sell, repay and interest-credit legs are pool-internal.
  * They do **not** call `record_tao_inflow`/`record_tao_outflow` (`SubnetTaoFlow`) or `record_protocol_inflow`.
  * They are excluded from `SubnetVolume`.
  * They are emitted as `LongFlow{netuid, kind, tao, alpha}` events and accumulated in `LongFlowTotals[netuid]` for reconciliation.
* **Separate long issuance and flow accounting:** none of the short counters (`SyntheticOutstanding`, `ResidualAlpha`, `BlockNetMinted`, `DeleveragingMinted`) are touched by longs.
* **Emission price:** raw for both sides (P14, L15 rejected v1.5).
* **Shortfall (v1.4):** there is no bad-debt balance. A default writes D off the lender claim; the cumulative write-off is a statistic in `LongFlowTotals.shortfall`.

### 7.4 Dissolution (v1.9.1: one-shot lender-first long sale inside the prune)
Subtensor dissolves a subnet inside the extrinsic that prunes it: `do_register_network` → `do_dissolve_network` → `T::Lending::start_dissolution`, then `NetworksAdded` is removed. There is no flag window, so v1.3's paced phase A could never run. Settling longs at r instead is confiscatory: A·r < D on most subnets even with no price move (model Finding 3: long −101% of C on the flat, crash and rally paths). v1.9.1 replaces both.

* **Phase A, one-shot long sale (atomic, inside `start_dissolution`, before T_A and r are read):**
  1. **Freeze.** Long opens and increases are refused from here on (the subnet is dissolving). Accrue every long's interest to this block.
  2. **One swap.** Sell the whole long book, A_book = `LongLockedAlpha[netuid]`, from long custody into the **live** pool in a single swap: the normal sell path with fees, no pacing, no L9 pause and no `min_tao_out`. Call the gross TAO out `P_book`.
  3. **Pro-rata split.** Each position gets `p_pos = ⌊P_book × a_pos / A_book⌋`. The rounding dust (< N rao) stays in SubnetTAO.
  4. **Lender first, per position.** The claim is `claim = D_pos + accrued interest`.
     * `repay = min(claim, p_pos + C_pos)`, taken from p_pos first and then C_pos. It is credited to SubnetTAO.
     * Any gap `claim − repay > 0` is a pool shortfall, written off `LongLiftedTao` as in I16 (nothing is received).
     * The surplus `p_pos + C_pos − repay` is refunded to the owner (free TAO).
  5. **Order and determinism.** Amounts depend only on the pre-sale snapshot (A_book, a_pos, D_pos, interest, C_pos) and the one swap, never on processing order.
     * Positions are processed in ascending `PositionId` (SCALE-encoded `(AccountId, NetUid, Side::Long)`). That fixes the order of storage writes and events only.
     * One aggregate sale with a pro-rata split, rather than sequential per-position sales, means no key gets a better price by sorting first, and it matches v1.9's whole-book pro-rata health.
     * (The reference model sells per position in owner order. The total P_book is the same, but the per-position split differs, so the model should switch to the pro-rata split.)
  6. **No sandwich.** The swap and the subnet's destruction happen in the same extrinsic, before `NetworksAdded` is removed and with no later trading. No one can buy back afterwards, so the sale can't be sandwiched for profit inside the prune.
     * A front-run in the same block before the prune extrinsic is ordinary trading against a known prune (prune timing risk, review #14). It isn't specific to longs.
  7. **Bounded.** At most `DeregParams.max_long_positions_per_subnet` (**256**, root-settable up to the runtime ceiling `MaxLongPositionsCeiling` = 1,024; v2.0) longs exist per subnet; `open_long` beyond that fails `TooManyLongPositions`. Phase A is never split across blocks: it is one swap plus ≤ 256 settlements, charged up front (weights, section 3).
* **Phase B, rate fix:** `r = min(T_flag, T_ema, T_A)/all_alpha`.
  * T_flag and T_ema are as in section 1 item 8. T_A is SubnetTAO right after phase A, with escrow excluded.
  * all_alpha includes synthetic and residual alpha. The sold long alpha is now pool alpha and counts as such; no locked long alpha remains.
  * The min stops collateral paid in phase A from raising r above what stakers would get without longs.
* **Phase C:** *(removed v1.9.1; the lender claim is settled in phase A, and no long alpha is left to settle at r).*
* **Phase D, shorts:** unchanged. q·r (or zone proceeds, P16) from P then C, and the minted alpha is burned.
* **Phase E, stakers:** paid r per staked alpha. Long custody holds no alpha after phase A.
* **Phase F, leftover:** takes the existing dissolution leftover path (the pool-alpha share).
* **Cursor:** `Dissolutions[netuid]` gains `phase: B | D | E | F`, a per-phase cursor and `t_a`. Phase A completes inside `start_dissolution`; B, D, E and F stay resumable under the existing meter. The existing basket-sale refreeze of r (`freeze_dissolution_price` after `SubnetBasketHoldingsToRoot`) still runs; phase A's repayments are already in SubnetTAO by then.
* **Solvency:** Σ staker payouts = staked·r ≤ (all_alpha − pool alpha)·r ≤ T_A ≤ pot. Proven in try-state (I17).
* **Residual risk (accepted, recorded).** In a fast crash into prune, `P_book + C` can fall short of D. The model's −30%/day for 4 days before prune gives a pool shortfall of median **3.6% of D, max 23.5%** across 110 subnets (paced phase A: 2.3%, max 23.0%). It is written off the lender claim as today (I16). Flat and +50% rally paths: 0 shortfall.

### 7.5 Storage, calls, events, errors (longs)
**Storage (new names; none of `Lifts`, `LiftBook`, `LiftParams`, `LongInterestPending`, `LongInterestSince` are reused)**
* `LongsEnabled: bool`
* `LongParamsStore: LongParams { stress_bps, stress_floor_bps, lift_cap_bps, open_limit_bps }` (v2.0: APR, pace, pause, min notional, deposit and withdraw buffer moved to the shared `LendingParams`; `floor_sma_bps` became `pause_sma_bps`; the longs cap moved to `DeregParams.max_long_positions_per_subnet` under the runtime ceiling `MaxLongPositionsCeiling` = 1,024)
* `LongPositions: DoubleMap<AccountId, NetUid → LongPosition { custody, debt: u64 TAO, locked_alpha: u64, collateral: u64, deposit: u64, opened_at, last_accrued, interest_remainder, state: Open | Deleveraging{since}, terms_version }>` (v2.0: `sma_paused_blocks` removed) (v1.9.1: `Unwinding` removed)
* `LongLiftedTao` and `LongLockedAlpha`: each a `Map<NetUid → u64>`. (`LongBadDebt` and `LongDebtReserve` are removed in v1.4.)
* `LongBlockNetLifted: Map<NetUid → (block, i128)>` (L8, cleared on_finalize)
* `LongFlowTotals: Map<NetUid → LongFlowTotals { lifted, bought_alpha, sold_alpha, repaid, interest_to_pool, shortfall }>`
* **Side-aware keys.** Add `enum Side { Short, Long }` and `PositionId = (AccountId, NetUid, Side)`. Re-key these maps:
  * `Due`: `(AccountId, NetUid, Side)`
  * `UncollectedInterest`: `(AccountId, (NetUid, Side))`
  * `DeleverageQueue` and `DeleverageFilled`: `(NetUid, (AccountId, Side))`
  * `OpenByNetuid`: `(NetUid, (AccountId, Side))`
  * `StrandedCustody`: `(NetUid, (AccountId, Side))`
  * `DeleverageCursor`: `(NetUid, AccountId, Side)`
  * `DeleverageBudget` becomes per (NetUid, direction: Up | Down).
  * `Positions` keeps its name and type (`ShortPosition`).
  * `PositionCount` and `TotalPositions` count both sides, plus `LongPositionCount: Map<NetUid → u32>`.
  * **Migration:** a storage-version bump translates existing entries to `Side::Short`. Launch is dark, so the expected count is 0, and the migration asserts that before translating.

**Extrinsics:** `open_long`, `increase_long(netuid, extra_lift, extra_collateral, min_alpha_out)`, `add_long_collateral`, `withdraw_long_collateral`, `close_long`, `poke_long`. Root: `set_longs_enabled(bool)`, `set_long_params(LongParams)`.

**Events:** `LongOpened{who, netuid, lifted, alpha, collateral}`, `LongIncreased`, `LongClosed{who, netuid, alpha_sold, tao_out, repaid, refund}`, `LongInterestCharged{to_pool}`, `LongDeleverageStarted{equity_twap}`, `LongDeleverageEnded{who, netuid, equity_twap}` (v2.0.1), `LongDeleverageStep{alpha_sold, repaid}`, `LongDefaulted{shortfall}`, `LongFlow{kind, tao, alpha}`, `LongBookSoldOnPrune{netuid, alpha, tao_out}` (v1.9.1, once per prune), `LongSettledOnDissolution{who, netuid, proceeds, repaid, collateral_used, refund, shortfall}` (v1.9.1: per position, phase A, ascending PositionId), `LongParamsSet{old, new}`, `LongsEnabledSet`.

**Errors:** `LongsDisabled`, `LiftCapExceeded`, `LongOpenLimit`, `LongCoverageNotMet`, `CloseInsolvent`, `OppositeSideOpen`, `LongStressBelowFloor`, `LongWithdrawBreaksCoverage`, `LockedAlpha` (returned by any subtensor unstake/move/transfer/swap path that touches long custody stake), `TooManyLongPositions` (v1.9.1). `UnwindInProgress` is removed (v1.9.1).

**Code touchpoints (branch now at cfe087e89, 2026-11-25 17:34 UTC; not yet reviewed against v1.3):**
* `staking/lending.rs`: add `lift_and_buy(netuid, tao, min_alpha_out)` and `sell_and_repay(netuid, alpha, min_tao_out)`. These are pool-internal debits and credits with no flow recording.
* `stake_utils.rs::emission_alpha_price`: no change (raw spot; L15 rejected).
* Subtensor stake guards: extend the existing custody guard hooks to `LockedAlpha`.
* Dissolution (v1.9.1): `start_dissolution` runs phase A (one swap of A_book plus pro-rata lender-first settlement) before anything reads SubnetTAO for r. Phases B/D/E/F stay in the metered cleanup.
* SDK and docs: the long calls.
* Estimate: +1.5–2k LOC in the pallet, +1k in tests.

### 7.6 Invariants (longs)
* **I12** Σ D_pos == `LongLiftedTao`, Σ A_pos == `LongLockedAlpha` == the long custody stake on `PalletHotkey`, per netuid.
* **I13** Every open is atomic, with ΔSubnetTAO == 0 (lift then buy). No lifted TAO ever reaches a user account. The only TAO credited to an owner is their collateral and realized surplus from a sale.
* **I14** At every open and increase, `LongLiftedTao ≤ lift_cap × SubnetTAO` (pre-open) and coverage (L2) holds for the opener.
* **I15** (v2.0) The per-block long down-budget is ≤ the shared `pace_bps_per_block`. Long sells run only while spot ≥ max(TWAP × 10000/`pause_twap_bps`, SMA30 × 10000/`pause_sma_bps`), with no lapse. The prune-step sale (7.4 phase A) is exempt from pace and pause. Short deleverage (the only up-budget user) is ≤ `pace`.
* **I16** A default writes D off `LongLiftedTao` exactly once. Σ write-offs == `LongFlowTotals.shortfall`.
* **I17** (v1.9.1) Dissolution runs phase A exactly once, atomically inside `start_dissolution`, before T_A and r are read. Then B → D → E → F, each once per position. Pot ≥ Σ payouts.
* **I24** (v1.9.1) Phase A conservation per subnet:
  * After phase A, `LongLockedAlpha == 0`, `LongLiftedTao == 0` and the long custody stake is 0.
  * P_book == Σ p_pos + dust (dust < N rao).
  * For each position, repay + shortfall == D_pos + interest, repay + refund == p_pos + C_pos, and all three are ≥ 0.
  * ΔSubnetTAO over phase A == −P_book + dust + Σ repay.
  * Results are identical under any permutation of position processing order.
* **I18** Long legs never change `SubnetTaoFlow`, `SubnetProtocolFlow` or `SubnetVolume`, or any short counter. Alpha issuance is unchanged by long opens and closes.
* **I19** (v1.4) For both sides, ΔSubnetTAO from interest == Σ interest charged. Interest credits never call `record_tao_inflow`, `record_tao_outflow` or `record_protocol_inflow`, and never touch `SubnetVolume` or `SubnetTaoFlow`.
* **I20** *(removed v2.0 with the SMA-pause lapse and its counter)*

## 8. Open questions

**One open item (v1.9.1), not adopted: 8.6.** Every other design item is decided; the record is below. The test-plan items (β calibration on the clone, e2e 1–33) remain.

### 8.1 Minimum pool depth (P18): decided v1.6
* **Decision:** 1,000 TAO for both shorts and longs (Jacob, 2026-11-25 19:20 UTC). The 100-TAO dust floor was rejected.
* **Existing positions on a pool that falls below:**
  * No opens or increases.
  * Close, top-up, withdraw within coverage, interest, forced close/deleverage, default and dereg all work (section 1 item 9).
* **Excluded now** (finney block 9,585,867 (2026-11-25 19:21:15 UTC)): 18 of 128 subnets. SN78 (87), SN16 (118), SN113 (127), SN47 (130), SN109 (135), SN84 (179), SN35 (187), SN57 (217), SN86 (219), SN7 (263), SN26 (282), SN70 (285), SN76 (335), SN103 (335), SN108 (398), SN59 (758), SN36 (825), SN99 (882).
* **Effects:**
  * The thinnest high-emission pools are out of reach on both sides, including SN26, 36, 59, 70, 99 and 103 from the 8.4 most-exposed list. Those pools lose short-side price discovery (11.9), which is accepted.
  * Still exposed to long emission farming above the depth: SN116, 58, 31, 80, 92 and 117.

### 8.2 Decided and closed (record; revisit only with new evidence)
* **Minimum pool depth (v1.6).** 1,000 TAO for both sides. See 8.1.
* **P22 adopted (v1.5).** See 8.3.
* **L15 rejected (v1.5).** See 8.4.
* **Interest routing (v1.4):** all interest goes to SubnetTAO. Holder dilution (shorts) and a pool shortfall (longs) are the bad-debt policy.
* **Dereg window (P13):** 30-day SMA, seeded at registration (section 9). v1.9.1: prefilled at the runtime upgrade from `SubnetMovingPrice`, re-seeded on every enable and `RankGeneration` reset, no EMA or partial-mean fallback (model Finding 4).
* **Dereg payout:** r for shorts and stakers, plus P16. r_ex was not adopted.
* **Griefing with no dip-buyers:** accepted (section 9; it needs a month with nobody buying a 50% drawdown).
* **No supply cap (v1.2):** book-aware stress slope 2 at launch. v1.7: bounded only by coverage and the P23 21M alpha-supply backstop.
* **v2.0:** every tunable is root-settable (section 2.1), no SMA-pause lapse, references limited to spot / TWAP_median / SMA30, one shared parameter set for both sides.
* **Split-resistant health (v1.9):** whole-book pro-rata health on both sides (section 1 item 4, I23). Closes review v1.6 #2.
* **P24 halving guard: removed (v1.9).** The halving pull is accepted (8.5).
* **P21 dilution breaker: removed (v1.7).** Replaced by counting minted and residual alpha in supply, plus P23 (8.5).
* **Flat APR:** 20% (P3).
* **Stress:** 3× (P2).
* **Tiny high-emission pools:** excluded below 1,000 TAO by P18 (v1.6).
* **Close-order run:** no per-block batching at launch. The first closer gains about 1–9% of notional. Revisit after the clone e2e.
* **Calibration:** the dip-buyer β is a test-plan item (validate on the clone), not a design choice.
* **Longs (v1.3–v1.4):**
  * no open gate;
  * dereg option (c);
  * third-party longs as team exit liquidity accepted (SN29: +92 / +190 / +519 to the team at caps of 5 / 10 / 25%);
  * longs cut squeeze capital by 4–41%;
  * a team-owned 10% long into dereg is +7 vs no long under v1.4 routing;
  * same coldkey on both sides is forbidden at launch.

### 8.3 P22: SMA30 buy ceiling (adopted v1.5)
**Hole closed.**
* Health uses the 72 h median TWAP, which a held pump catches within 36 h. Without P22 the paced buy-back spends the short's C + P at the pumped price, straight into the team's sell side.
* SN29 (finney block 9,581,283, 2026-11-25 04:04 UTC), with a minimum-collateral 1× short: a 6× pump held 10 days defaults the short (−7,235) and gives the team 11.9k.

**Rule.**
* Shorts buy only while spot ≤ 1.25 × min(TWAP_median, SMA30_rank).
* Longs sell only while spot ≥ max(TWAP_median, SMA30_rank)/1.25.
* v2.0: the SMA term has no lapse. The prune-step long sale (v1.9.1) ignores the band.
* Numbers are in section 16.

**Liveness and bad-debt bounds.**
1. **The SMA term can't stall forever.** For a one-off re-rating to k×, spot ≤ 1.25 × SMA30 again after t = 30·(k/1.25 − 1)/(k − 1) days: 22.8 d at 6×, 23.3 d at 10×, < 24 d for any k. Only a sustained rally faster than about +1.5%/day keeps it paused. **v2.0:** with no lapse, that case is bounded only by interest exhausting C, or by the rally slowing. Rough SN29 1× bound, not simulated: C lasts about 150 days at a constant 4%/day, because interest grows with the TWAP.
2. **The TWAP term clears itself** within 36 h of any held move.
3. **Interest keeps accruing while paused.** Default happens when C is exhausted, so a paused position always terminates.
4. **Dilution** (v1.7: P21 removed) is bounded by 3× coverage with the book term, forced close and P22, and capped by P23. Residual counts in supply, so it permanently pulls the halving forward by residual/issuance-rate (SN29: 76.4k α ≈ 10 days). On a genuine re-rating P22 roughly doubles residual: 6× forever 36.0k → 76.4k α (0.55% → 1.18% of issuance); a 4%/day rally 20.5k → 57.5k α. Without the lapse it would be 102.5k / 73.0k.
5. **Longs:** the floor can keep a long paused while a subnet falls steadily. Shortfall goes 9 → 14 TAO at a 2%/day drip and 19 → 87 TAO at 5%/day. **v2.0 (no lapse):** a steady decline faster than about 1.5%/day keeps a forced long unsold until the decline slows, C runs out (SN29 example: D 651, C 355 → about 995 days at 20%), or prune, where the one-shot sale settles it. Bad debt is therefore bounded by D − C − proceeds at that point, written off as a pool shortfall. This is the accepted cost.

### 8.4 L15 rejected (v1.5): long-bought alpha counts in the emission price
**Accepted consequence.**
* A long can raise its own subnet's emission with pool-financed leverage. At the 10% cap the lift raises price by +11.1%.
* Break-even is the share s* of the subnet's extra emission that the long's owner must receive to cover the 20% interest.
* Under v1.4 the interest goes back into the same pool, so a holder of that pool's alpha recaptures part of it. The figures below are an upper bound on the cost.

| Subnet | Extra emission (TAO/day) | Interest (TAO/day) | Break-even share s* |
|---|---|---|---|
| SN64 | +28.2 | 10.5 | 37% |
| SN51 | +53.1 | 10.6 | 20% |
| SN4 | +24.6 | 7.4 | 30% |
| SN29 | +0.63 | 0.40 | 63% |

**Exposure across the 97 emitting subnets at the 10% cap.**
* 77 have s* ≤ 100%: the subnet as a whole gains more emission than the long pays.
* 70 have s* ≤ 59% (owner plus validators), 65 have s* ≤ 41% (validators), and 37 have s* ≤ 18% (the owner cut alone pays for it).
* Most exposed (s* < 6%): SN26, 99, 70, 36, 116, 59, 58, 31, 80, 92, 103 and 117. These are thin, high-emission pools of 219–5.9k TAO.
* A 5% cap gives the same picture (77 / 67 / 64 / 36).

**Bounds.**
* The lift cap (10%, so +11% price at most).
* Collateral at risk at m = 2.
* Emission is zero-sum: normalized shares, so the gain is taken from other subnets.
* Ranking is raw too, so the prune delay is the same accepted consequence (L14).

### 8.5 v1.7: minted alpha counts in supply; 21M cap (decided; one consequence flagged)
**Decision.** Jacob (2026-11-25 20:24 UTC): keep 3× collateral, count minted and residual short alpha in alpha supply, and never let supply exceed 21M. P21 is removed (review #1: it could lock a subnet out of shorts for about 2+ years after one default).

**Safe choice at the cap (flagged).**
* Minting yields: a mint that would take S over 21M is refused, not partially filled.
* Emission is unchanged and can't cross 21M on its own.
* No buffer is reserved for future emission, since coverage keeps books far below the cap (section 17).

**Halving effect.** An open book of B α moves the next halving earlier by B/(alpha issuance per block × 7,200) days while it is open; residual makes the shift permanent. On SN29 a book of 10 / 18 / 30 / 60% of pool alpha moves it 45 / 81 / 134 / 269 days. A 76.4k α residual is a permanent ≈ 10 days. A `root_proportion` drop at a 30% book of 0.8–1.6 points lowers the alpha-injection cap.

**v1.8 → v1.9: P24 halving guard removed** (Jacob: "the halving is bullish for a subnet I don't see an issue here").
* An open short book pulls the next halving forward by B/(issuance per block × 7,200) days while open; residual makes the shift permanent. This is **accepted**.
* The v1.8 indirect-pull numbers (section 18) are kept as the record of the size: e.g. SN4 30 days out, about 244k α defers about 179 TAO/day of alpha emission for about 6.7 TAO/day of interest.

### 8.6 Open item (not adopted; Jacob hasn't decided): slice-built books (model Finding 2 residual)
* **Issue.** Building a book in sequential slices isn't equivalent to opening it at once. Earlier slices pass coverage against a smaller book; later slices then raise their v1.9 health cost to the book average (the side effect accepted in section 1 item 4).
* **Model** (`split_held_short_5x_10`): an 18% book built as 10 sequential coldkeys under a held 5× pump triggers on 110/110 subnets at day 3.3–3.5, leaving ResidualAlpha median 1.25% of issuance (max 3.15%). The same book opened as one position triggers on 64/110 (median day 17) and leaves residual on 12/110 only.
* It hurts the slicer (earlier trigger), so it isn't a delay attack. But the extra default is permanent dilution for every holder, and since v1.7 it also pulls halvings forward.
* **Proposal (not adopted):** price the opener's coverage at its marginal cost, `BuyCost(Q_after | pumped) − BuyCost(Q_before | pumped)`, floored at the pro-rata share. This is the V_dyn rule section 1 item 4 says "fits a one-off opener check". Marginal costs telescope, so N sequential slices post exactly the one-shot book's coverage.

### 8.7 Flagged in v2.0 (no decision needed unless Jacob wants to change them)
* **T_ema is kept.** It's the 7,200-block EMA of pool **TAO** used in r = min(T_flag, T_ema, T_A)/all_alpha. It isn't a price, and it is what stops a last-day TAO pump into the pool from inflating the dereg payout.
  * It isn't trivially replaceable: a TAO TWAP would need a second median ring (72 more values per subnet) for the same role.
  * Its window is now settable (`pool_tao_ema_blocks`).
* **Rank-ring seed still reads `SubnetMovingPrice`** at the runtime upgrade and on enable (Jacob's v1.9.1 rule, I25). That one-time copy is the only lending read of the emission EMA left.
  * A cold TWAP ring can't be used for the seed instead, since it is also empty at upgrade.
  * Window changes re-seed from the published SMA30, not the EMA.
* **The EMA removal wasn't re-simulated.** It changes coverage only when the 7.7 h EMA sat above spot, the 72 h median and SMA30 at once (shorts), or below spot and the median (longs).

## 9. Dereg ranking window (P13)

**v1.9.1 ring lifecycle (normative; model Finding 4).** The branch today starts rings empty, warms them only after 30 days, resets them on every `RankGeneration` bump (each enable), stops sampling while disabled, and meanwhile falls back to the emission EMA (≈ 7.7 h half-life). That reopens a fast-ranking window after every enable.
1. **Runtime upgrade.** A migration fills every live non-root subnet's ring with 30 daily means equal to its current `SubnetMovingPrice`, publishes `RankPrice`, and sets `RankRingGeneration[netuid] = RankGeneration`, whether or not lending is enabled. Weight is O(live subnets), charged once.
2. **New registration, or a re-registered netuid:** seed the same way at registration (unchanged from v1.0, now stated as the same seed).
3. **Enable (`set_enabled` false→true), and any other `RankGeneration` bump:** in the same call, re-seed every live subnet's ring from its current `SubnetMovingPrice` and tag it with the new generation. Weight is O(subnet_limit), charged in `set_enabled`.
4. **Disable:** no reset. Rings keep sampling hourly while disabled, and ranking keeps reading them. Re-enabling re-seeds per (3).
5. **No fallback anywhere.**
   * `dereg_rank_price` (subtensor `coinbase/root.rs:399`), `prune_rank_price` and `rank_price` (lending) read only the published ring SMA.
   * The `unwrap_or_else(get_moving_alpha_price)` fallback, the `unwrap_or_default` zero, and any partial mean of filled days are deleted.
   * A missing or wrong-generation ring for a live subnet is a try-state failure (I25), not a reason to read the EMA.
6. **Numbers** (model `finding4.py`, 109 deep subnets, griefer with a 100M TAO budget pinning spot at 0.95 × L for up to 40 days):

   | Ring mode | Newly prune-eligible | Days, median (min) | Capital, median (min) |
   |---|---|---|---|
   | Empty rings + EMA fallback (branch today) | 60 / 109 | 1.43 (0.50) | 47.4k (777 TAO) |
   | Partial mean | 57 / 109 | 2.59 (0.50) | 32.2k (777) |
   | **Prefill (v1.9.1)** | **46 / 109** | **27.6 (19.6)** | 13.5k (780) |

   On the 46 subnets reachable both ways, the EMA fallback is about 21× faster at similar capital. SN98: 777 TAO in 0.50 days vs 780 TAO in 19.59 days. SN29: 9.1k in 1.05 days vs 9.6k in 26.59 days.
7. **Residual (noted, not changed):** a re-seed copies the current `SubnetMovingPrice`, so an EMA depressed just before root enables lending is copied into the ring. Root controls enable timing, and lending is disabled at that moment, so no leveraged short can be used to depress it.


**Setup** (`dereg3.py`, results in `results_dereg_window2.json`). Finney state is read only, at block 9,578,833.
* **Target.** L is the lowest non-immune ranking price, 0.001627.
* **Griefer.** Pins the victim's spot at f × L, choosing the cheapest f in {0.9, 0.7, 0.5, 0.3}, until the ranking MA drops below L. It then holds for the expected wait until the next registration and settles at the fast EMA.
* **Shorter rules.** The shorter uses the 3× coverage rule, withdraws collateral above that need, has capital of 2× the victim's pool TAO (0.5× as a sensitivity case), and pays 20% APR.
* **Team.** Sells the miner share. Outside dip-buyers use the v2 β × 30-day volume model.
* **Windows.** SMA over N days, and EMA with the same mean lag (τ = N/2). History starts flat at today's moving price.

**Cadence and immunity.**
* `NetworkImmunityPeriod` = 432,000 blocks (60 days), `NetworkRateLimit` = 10,800 blocks (36 h).
* There were 13 registrations (and prunes) in the last 30 days, so a slot opens about every 2.3 days.
* The window, not the cadence, sets the time to the slot. A 60-day immune subnet can't be targeted, and a subnet leaving immunity carries its whole price history into the SMA. Seed the ring at registration with the first price, so a new subnet can't be pruned on day 61 off a single bad day.

**Result: healthy mid-ranked victims** (SN79, SN63, SN25, SN19, SN41; L/EMA = 0.55, 0.46, 0.39, 0.21, 0.17).
* **With dip-buyers present (β = 0.5 or 1.0):** no window reaches the slot within 200 days, at 0.5× or 2× pool-TAO capital. The griefer runs out of coverage before the MA crosses.
* **SN19 and SN41** can't be reached at any β, including β = 0: their pools are too deep relative to L.
* **With no dip-buyers (β = 0)** the window only buys time. The collateral tied up is the whole budget (2× pool TAO: 13.6k on SN79, 20.6k on SN63, 21.5k on SN25). The griefer *profits* at EMA settlement, because the fast EMA has already crashed when the slot fires.

| β = 0 | SMA3 | SMA7 | SMA14 | **SMA30** | SMA60 | SMA90 | EMA30 (τ=15 d) | EMA90 |
|---|---|---|---|---|---|---|---|---|
| SN79: days to prune / griefer net at EMA settle | 5 / +1,081 | 8 / +1,094 | 12 / +1,087 | **23 / +1,067** | 44 / +1,030 | 64 / +993 | 20 / +1,072 | 54 / +1,011 |
| SN63 | 5 / +1,639 | 9 / +1,645 | 14 / +1,625 | **28 / +1,579** | 52 / +1,492 | 77 / +1,407 | 30 / +1,572 | 79 / +1,399 |
| SN25 | 6 / +1,722 | 10 / +1,718 | 16 / +1,698 | **31 / +1,652** | 59 / +1,566 | 87 / +1,480 | 45 / +1,609 | 111 / +1,402 |
| Interest paid, SN63 | 34 | 57 | 96 | **186** | 351 | 512 | 199 | 527 |

The positive numbers are griefer *profit*, mostly what dip-buyers would have paid; at β = 0 nobody pays it. Settling at the ranking MA instead of the fast EMA cuts it (SN79: +314 at SMA30 and about −64 at SMA90), but the profit doesn't go away at β = 0. The real defence is outside demand, not the window.

**Result: extractive targets (the legitimate use).** Days to prune, shorter cost (+ = loss, − = profit), at 2× pool-TAO capital:

| SN29 | SMA3 | SMA7 | SMA14 | **SMA30** | SMA60 | SMA90 | EMA30 | EMA90 |
|---|---|---|---|---|---|---|---|---|
| β = 0 | 4 / −1,116 | 5 / −1,156 | 8 / −1,163 | **15 / −1,158** | 27 / −1,146 | 39 / −1,134 | 10 / −1,162 | 26 / −1,147 |
| β = 0.5 | 4 / −669 | 6 / −466 | 10 / −61 | **29 / +212** | 61 / +387 | 91 / +545 | 16 / +273 | 67 / +698 |
| β = 1.0 | 4 / −262 | 8 / +177 | 13 / +615 | **32 / +857** | not reached | not reached | not reached | not reached |

* **SN101:** at β = 0 every window gets there (SMA30: 25 days, profit 602). At β = 0.5 only windows of 7 days or less do. At β = 1 none does.
* **0.5× capital:** only SN29 SMA3 is reached.

**Recommendation: a 30-day SMA of raw spot.**
* It is the longest window that still lets shorts prune SN29 when outside buyers defend it (β = 1: 32 days, about 857 TAO). 60 and 90 days block that.
* It forces a no-buyer griefer to hold about 2× the victim's pool TAO for 3–4 weeks.
* With buyers present it adds nothing over shorter windows, because no window is griefable there.
* **SMA over EMA:** at equal mean lag the EMA reacts faster to a deep, short push (SN29: EMA30 10 days vs SMA30 15 days), and it never fully forgets a spike. The SMA makes the griefer sustain the push for most of the window, and it has a hard memory bound.
* **Cost of the SMA:** a 720-entry hourly ring per subnet (about 1.8 MB for 128 subnets of u128). It can be cut to 30 daily means plus a running hourly accumulator (31 entries).
* **If ring storage is unacceptable,** use EMA τ = 15 days. Its times to prune are within about −35% to +45% of SMA30's, but it can't prune SN29 at β = 1.

## 10. Team exit: realizable TAO for SN29, SN101 and SN128

**Setup** (`team_exit.py`, results in `results_team_exit.json` and `team_exit_table.md`). Block 9,578,833.
* **Team.** Holds 100% of AlphaOut (SN29 matches the chain; for SN101 and SN128 this is an upper bound, with a 50% run in the JSON). It receives all non-root alpha emission, sells the miner share (0.41 α/block) hourly, and dumps everything at H.
* **Root.** Sells 0.41 × root_prop α/block. The root sell flag is on: the sum of EMA prices is 1.15 > 1.
* **Implied level p_imp(t).** The spot path with *only* emission plus root sell pressure: the team sells all of its emission, and there are no outside buyers.
* **"Shorts" case.** One shorter keeps spot ≤ p_imp(t) under the 3× coverage rule, 20% APR and the 20% supply cap, and is still open when the team dumps.
* **Exit measures.** Team exit = TAO from the hourly miner sales + the final dump. The dereg alternative = the same sales + the payout. The payout is computed two ways: the current chain rule (`destroy_alpha_in_out_stakes`: pool TAO pro rata to staked alpha) and the r rule, min(T, T_ema)/all alpha. Short settlement adds q × EMA to the pot.

| Subnet | H | Pool TAO, no shorts | Pool TAO with shorts (+ escrow) | Team exit by dump: no → shorts | Dereg, current rule: no → shorts | Dereg, r rule: no → shorts | Shorter cost of hold (+ = loss) | Shorter P&L closing after the dump | Collateral |
|---|---|---|---|---|---|---|---|---|---|
| SN29 β=0 | 30 d | 7,232 → 7,057 | 6,810 (+250) | 3,489 → 3,308 (−5%) | 7,237 → 7,219 | 3,547 → 3,473 | −5 | +178 | 501 |
| SN29 β=0 | 90 d | → 6,723 | 6,089 (+668) | 3,681 → 3,192 (−13%) | 7,235 → 7,113 | 3,843 → 3,587 | −37 | +473 | 1,330 |
| SN29 β=0 | 180 d | → 6,268 | 5,245 (+1,148) | 3,926 → 3,074 (−22%) | 7,216 → 6,869 | 4,216 → 3,665 | −118 | +808 | 2,263 |
| SN101 β=0 | 30 d | 3,355 → 3,146 | 2,821 (+329) | 2,486 → 2,174 (−13%) | 3,399 → 3,322 | 2,546 → 2,418 | −27 | +300 | 653 |
| SN101 β=0 | 90 d | → 2,751 | 2,108 (+730) | 2,531 → 1,849 (−27%) | 3,397 → 3,063 | 2,676 → 2,249 | −149 | +651 | 1,392 |
| SN101 β=0 | 180 d | → 2,285 | 1,522 (+1,051) | 2,569 → 1,615 (−37%) | 3,343 → 2,716 | 2,783 → 2,027 | −347 | +920 | 1,899 |
| SN128 β=0 | 30 d | 3,989 → 3,791 | 3,499 (+297) | 2,817 → 2,544 (−10%) | 4,014 → 3,964 | 2,876 → 2,773 | −17 | +266 | 594 |
| SN128 β=0 | 90 d | → 3,421 | 2,786 (+704) | 2,878 → 2,235 (−22%) | 4,015 → 3,759 | 3,026 → 2,663 | −105 | +617 | 1,374 |
| SN128 β=0 | 180 d | → 2,961 | 2,125 (+1,071) | 2,940 → 1,981 (−33%) | 3,977 → 3,430 | 3,174 → 2,475 | −270 | +920 | 2,025 |
| SN101 β=0.5 | 90 d | → 3,283 | 2,655 (+1,747) | 3,083 → 2,382 (−23%) | 4,029 → 4,645 | 3,270 → 3,575 | +1,391 | +1,524 | 61.7k (peak 77k) |
| SN128 β=0.5 | 90 d | → 3,954 | 3,438 (+2,056) | 3,412 → 2,857 (−16%) | 4,625 → 5,829 | 3,596 → 4,328 | +1,987 | +1,748 | 77k (peak 91k) |

**Reading.**
1. **Holding the implied level costs the shorter almost nothing when there are no outside buyers (β = 0).** Its minted alpha replaces exactly the alpha the team withholds to prop the price: 177 α/hour, or 127k α per 30 days. The hold is mildly profitable (5–350 TAO), and it needs collateral of only 500–2,300 TAO. It cuts the team's dump exit by 5–13% at 30 days, 13–27% at 90 days and 22–37% at 180 days.
2. **The TAO the shorts sell for leaves the pool but sits in escrow,** so the team can't extract it. Pool + escrow ends at or above the no-short pool in every case.
3. **If the shorter is still open when the team dumps, closing afterwards earns it 180–920 TAO** at β = 0. That is the incentive that makes the hold self-funding.
4. **With dip-buyers (β = 0.5), the hold becomes expensive.** The shorter fights the buyers, hits the 20% supply cap, and is 1.4–2.0k TAO underwater at 90 days, with 60–90k TAO of peak collateral. The team still loses 16–23% of its dump exit.
5. **The bigger extraction route is deregistration, not dumping.**
   * Under the current chain rule, a team that owns all of AlphaOut collects about the whole pool: 7.2k on SN29, versus a 3.5–3.9k dump. Shorts reduce that by only 0–5% on SN29 and 2–19% on SN101 and SN128 at β = 0, because their EMA settlement (q × EMA) flows into the same pot.
   * At β = 0.5 the shorts' settlement *raises* the team's dereg payout by 6–26%. This is partly a modelling upper bound: dip-buyers' alpha is not counted in the pot denominator.
   * The r rule halves the dereg route on SN29 (3.5–4.2k) and keeps the short effect at −2 to −27% (β = 0).

**Spec consequences.**
* Keep P12 (settle at EMA). *Superseded in v1.1: settle at r; see section 10.1.*
* Treat the payout rule as a separate open call (section 8). Without the r rule, shorts that push an extractive subnet into deregistration pay the team.
* Add e2e test 10.

### 9.1 v1.1 re-run: settlement at r (`dereg4.py`, `results_dereg_v11.json`)

**Changes from v1.0.**
* Settlement is at r. I is tracked (1 α/block alpha-out emission, injected alpha, minted alpha). T_ema is a 7,200-block EMA of pool TAO.
* Budgets of 2× and 5× pool TAO.
* SMA windows of 7, 14, 30, 45, 60 and 90 days.
* Flat 20% APR. The v1.0 run used a 100% step above 20% OI, so the interest figures here are lower.
* Griefer profit at SMA30 is shown as +, with the v1.0 EMA-settle figure for comparison:

| Victim | β | Budget | Days to prune | Griefer net at r | (at EMA) | (at r_ex) | Collateral tied up | r / spot at flag |
|---|---|---|---|---|---|---|---|---|
| SN79 | 0 | 2× | 23 | **+1,889** | +1,148 | +845 | 13,647 | 0.54 |
| SN63 | 0 | 2× | 28 | **+2,782** | +1,727 | +1,087 | 20,626 | 0.57 |
| SN25 | 0 | 2× | 31 | **+3,077** | +1,823 | +1,588 | 21,489 | 0.51 |
| SN79/63/25/19/41 | 0.5, 1.0 | 2× and 5× | never in 200 d | – | – | – | – | – |
| SN19, SN41 | 0 | 2× | never | – | – | – | – | – |
| SN29 | 0 / 0.5 / 1.0 | 2× | 15 / 21 / 32 | +1,810 / +1,250 / +1,090 | +1,214 / −247 / −652 | +333 / +44 / +19 | 14,463 | 0.66 / 0.48 / 0.45 |
| SN101 | 0 | 2× | 25 | +1,086 | +602 | +767 | 6,710 | 0.39 |
| SN101 | 0.5 / 1.0 | 2× and 5× | SMA30 not reached (≤ 14 d windows only) | | | | | |
| SN128 | 0 | 2× | 21 | +1,233 | +687 | +810 | 7,978 | 0.42 |
| SN128 | 0.5 / 1.0 | 2× and 5× | SMA30 not reached | | | | | |

**Window choice under r: keep the 30-day SMA.**
* A longer window does not reduce griefing profit; it rises slightly with time. SN79: +1,860 at 7 days, +1,889 at 30, +1,955 at 90.
* A longer window only extends the lock-up: 23 → 64 days on SN79. On 13.6–21.5k TAO that is an opportunity cost of about 0.8% per month, small against a 14–20% return.
* Reachability is set by outside demand, not by the window. Even 5× capital never reaches the slot on a healthy subnet at β ≥ 0.5.
* Longer windows do remove legitimate prunes. SN29 at β = 1 with 2× capital is reachable only at windows ≤ 30 days, and SN101/SN128 at β ≥ 0.5 only at ≤ 14 days.
* So 45–90 days buys nothing against griefers and costs the extractive-subnet use.

**Pot solvency.**
* Payouts are out × r. Sources are pool T + Σ q × r. Because r ≤ T/I and out ≤ I, the surplus T − (out − q)·r is ≥ 0 always.
* Each short can pay: the coverage rule implies C + P ≥ BuyCost(q) ≥ q·spot ≥ q·r.
* Simulated surplus was +0.9k to +4.9k on every run (the pool-alpha share x·r), and P alone covered q·r on every run.
* Including escrow in T makes r higher. That is still solvent in the sims, but it is not provable in general. Keep T = pool TAO.

### 10.1 v1.1 team exit under r (`team_exit_v11.py`, `results_team_exit_v11.json`)

On a 50/50 pool, the r payout equals the dump exit exactly: dumping `out` yields y·out/(x+out) = r·out. So the team's dereg route and dump route now coincide, and the 7.2k legacy windfall on SN29 is gone.

| Case | Team dereg payout (= dump), no shorts → shorts at implied level | Shorter net if pruned at H (r settle) | Shorter mark if still open | Pot surplus |
|---|---|---|---|---|
| SN29 β=0, 30 / 90 / 180 d | 3,489→3,309 (−5%) / 3,682→3,193 (−13%) / 3,927→3,075 (−22%) | +121 / +344 / +631 | +5 / +37 / +118 | 3.4–3.8k |
| SN101 β=0 | 2,486→2,174 (−13%) / 2,532→1,849 (−27%) / 2,569→1,616 (−37%) | +243 / +553 / +819 | +27 / +149 / +347 | 0.8–0.96k |
| SN128 β=0 | 2,818→2,545 (−10%) / 2,878→2,236 (−22%) / 2,941→1,981 (−33%) | +210 / +509 / +796 | +17 / +105 / +270 | 1.1–1.2k |
| SN101 β=0.5 | 2,622→2,049 (−22%) / 2,983→2,125 (−29%) / 3,443→2,483 (−28%) | +784 / +1,309 / +1,301 | −271 / −1,391 / −880 | 1.0–1.1k |
| SN128 β=0.5 | 2,938→2,360 (−20%) / 3,302→2,505 (−24%) / 3,809→2,932 (−23%) | +938 / +1,477 / +1,446 | −453 / −1,987 / −1,599 | 1.3–1.5k |

**Reading.**
* Under r, the β = 0.5 hold that was underwater on mark (−0.3k to −2.0k) becomes profitable (+0.8k to +1.5k) if the subnet is pruned. That pays shorters to finish the job on an extractive subnet.
* Pool TAO paths are unchanged from section 10.
* Under r_ex, the team's payout is nearly untouched at β = 0 (−0.1% to −5%), and it *rises* at β = 0.5, because dip-buyers' TAO is added while minted alpha is excluded. That is why r is recommended over r_ex for Jacob's goal.

## 11. Red-team of the v1.1 parameter set (`redteam.py`, `snipe.py`, `gate_grief.py`, `dereg4.py zone`)

**11.1 Dereg snipe (new with r; fixed by P16).** *v1.2: the freeze is replaced by zone-proceeds settlement; see 12.4.*
* The attack: open the maximum position one block before the prune, sell at spot, and settle at r. Its own sale lowers T_flag and its mint raises I.
* SN40 (the current candidate): +50 TAO at a 5% cap, or +177 at 20%. Stakers lose 71 / 215.
* SN29: +306 / +1,098, with stakers losing 438 / 1,334. SN101: +384 / +1,095. SN128: +399 / +1,178.
* That is 15–19% on collateral, instantly.
* P16 (no opens or increases within 1.25× of the prune threshold) blocks it. It left all legitimate and griefing pushes within 0.5%: SN29 β = 1 is still pruned in 32 days for +1,090.
* Alternatives: settling young alpha at max(r, EMA) only helps for windows ≥ 30 days, which then catch legitimate pushes; r_ex keeps a snipe profit of +259 on SN29, paid from the protocol surplus.

**11.2 TWAP flash spike (fixed by P6 median).**
* A mean-of-72 TWAP is moved 2–3× by one sample at 73–145× spot.
* Capital needed for one block: SN101 25k–37k TAO, SN128 30k–44k, SN29 55k–80k, SN79 52k–75k, SN51/SN64 1.4–2.1M.
* The round-trip loss is only 3–10 TAO on the small pools and 170–180 on SN51/SN64.
* A team with flash capital could force every short on SN101 into deleveraging, then sell into the paced buys.
* With a median, the pump must be held for 36 h.

**11.3 Pump into deleverage (fixed by P17).** Pace bounds the impact per block, but not the price paid. P17 pauses deleverage buys while spot > 1.25 × TWAP_median.

**11.4 Multi-account split.**
* Deleverage pace and reserve retire are already per subnet (I9), so splitting doesn't dodge pacing.
* The open-impact limit was per open, so it was bypassable. v1.1 aggregates it per block (P8).
* Minimum size: splitting only adds positions. A deposit prices that (P19).
* No per-subnet position cap: a cap can be squatted for about 512 TAO/yr.
* Cascade ordering: v1.0 FIFO lets the first position in the queue buy cheapest. v1.1 makes fills pro rata (I9).

**11.5 Cap squatting (P9 → 50%).** *Superseded in v1.2: there is no cap, so there is nothing to squat.*
* At 20% of issuance, a team mints the whole cap, sells, buys back (about 2 TAO in fees) and holds.
* That costs 100% APR on 2.5–2.9k notional: 2.5–2.9k TAO/yr on SN29/101/128. SN29's chain emission is 481 TAO/yr and SN101's is 1,603.
* Outside shorts are then blocked entirely.
* At 50% with 250% APR at the cap, squatting costs 15–18k TAO/yr.

**11.6 Interest and reserve edges.**
* At 20% APR, interest is immaterial to griefing: 11–45 TAO over a 3–4 week push, against 1.9–3.1k of profit. Raising the floor doesn't defend against griefing; the APR's job remains cross-subnet emission gaming.
* Withdraw-to-the-edge triggers deleveraging; fixed by P20.
* Reserve at dereg is now specified: it goes to the pot.
* The APR knee uses Issuance_ex, so a short's own mint doesn't move the denominator. *v1.2: there is no knee; the APR is flat.*
* Accrual at the minimum position is about 760 rao/block, so carry the remainder.

**11.7 Emission gate.**
* Halving a subnet's emission takes a median 23% push across 83 emitting subnets.
* On zero-volume subnets the pin holds and pays the shorter, while the victim loses about 58–60% of 30-day emission:
  * SN26: 376 → 156, shorter +33.
  * SN36: 616 → 256, shorter +76.
* At β = 0 on SN49, SN58, SN99 and SN117, the pin also holds and pays: +48 to +139 TAO, with emission −55% to −64%.
* With dip-buyers (β = 0.5) the pin fails. The shorter loses 36–250 TAO and the victim's emission is unchanged or higher.
* This is the intended "shorts push emission down" behaviour, and it only bites where nobody trades. No change is recommended.

**11.8 Halving and issuance.**
* Minted and residual alpha are excluded from Issuance_ex, so opens don't move halving or root_proportion (I7). *(Superseded v1.7: minted and residual alpha count in supply; P23 caps it at 21M.)*
* Real supply can exceed the schedule by SyntheticOutstanding (≤ 50% of Issuance_ex with v1.1 P9) + ResidualAlpha. Minted alpha is burned on close or dereg, so only residual is permanent. This is the accepted dilution policy.
* Flag: the 50% cap doubles that worst case.

**11.9 Tiny high-emission pools.**
* SN26 has a 219 TAO pool and 38 TAO/day of emission. 45 TAO of notional (178 collateral, 0.02 TAO/day interest) halves its emission.
* That is the market pricing emission against depth. Only a 100-TAO dust floor is recommended (P18). *(Superseded v1.6: P18 = 1,000 TAO.)*

**11.10 Close race.**
* First closer gains 1–9% of notional (v2).
* At dereg there is no race under r, since every position gets the same rate.
* Voluntary closes are ordinary AMM ordering. Accept it.
* Deleveraging is pro rata (11.4).

**11.11 Sandwiching opens.**
* An open is an unstake-like sale, so extraction is bounded by its `min_tao_out` and by P8's 2% per block.
* Make `min_tao_out` mandatory and non-zero, with an SDK default of 0.5% slippage.
* Not simulated.

## 12. v1.2: no cap, flat APR (`sim3.py`, `v12.py`, `v12b.py`, `team_exit_v12.py`; results in `results_v12*.json`, `results_team_exit_v12.json`, `results_gaming_pairs.json`)

**Engine (`sim3.run3`).** SPEC v1.1 rules:
* 3× coverage on the whole book pro rata, with ref = max(EMA, spot) or max(EMA, spot, SMA30).
* Flat APR and no cap.
* Health on the median 72 h TWAP. Buy-backs pause while spot > 1.25 × TWAP. Pro-rata paced deleverage at 0.05%/block.
* 50% of interest goes to a reserve (capped at 2% of pool TAO) that retires residual alpha.
* Hourly steps, so the per-block P8 limit is not binding in the sim.

**12.1 Flat rate (a, d).**

| 25% pin, 30 days, net TAO at 15% / 20% / 30% | β = 0 | β = 0.5 |
|---|---|---|
| SN29 | +28 / +25 / +20 | −978 / −984 / −998 |
| SN101 | +28 / +27 / +23 | −530 / −534 / −541 |
| SN128 | +24 / +22 / +18 | −2,454 / −2,460 / −2,472 |
| SN19 | −78 / −92 / −120 | −7,853 / −7,888 / −7,956 |

* **Dereg griefing** (section 9.1): interest is 11–52 TAO per push at 20%, against 1.1–3.1k of profit. 15% or 30% moves the profit by ≤ 26 TAO.
* **Team exit with no cap, β = 0.5:**
  * The shorter is no longer stopped at 20% of issuance; it grows to 22–24%, held at that size by coverage.
  * The team's dereg payout is cut by 29–31% at 90–180 days (24–29% with the cap): SN101 2,983 → 2,073; SN128 3,302 → 2,446.
  * The shorter nets +1.4–1.6k at r if the subnet is pruned.
  * β = 0 is unchanged; the cap never bound there.
* **Rival emission gaming** (25% push on a victim, gain to the attacker subnet's total emission; 1,152 pairs over the top 12 attacker subnets):

| Victim pool | Pairs | Break-even > 15% / 20% / 30% | Median break-even |
|---|---|---|---|
| < 1k TAO | 96 | 60 / 60 / 58 | 58% (max 1,900%: SN51 ← SN26) |
| 1k–10k | 623 | 109 / 79 / 55 | 1.7% |
| ≥ 10k | 433 | 28 / 13 / 7 | 4.1% (max 46%: SN51 ← SN15) |

  These are whole-subnet gains. An attacker holding h of the attacker subnet earns h × gain, so with h ≤ 10% no deep-victim pair breaks even at 20%. The pin cost against dip-buyers (12.1 table) is 35–60× the interest anyway. **Recommend 20%.** 15% doubles the deep-victim pairs, and 30% adds 50% carry to legitimate shorts for little gain.

**12.2 Cascade and worst-case dilution (b).**

Setup:
* 10 shorters with unbounded capital target −9% to −90%, one starting every 12 h.
* A team pump to 3×/5×/10× of today's price at day 7, held 7 days, then unwound over 24 h.

Results:
* **With ref = EMA,** the book reaches 1.4–1.6× pool alpha (SN29 0.74–0.86 of issuance, SN19 0.79–0.91). A 5× or 10× pump leaves residual of 2–10% of issuance at β = 0 and up to 20% at β = 0.5.
* **With ref incl. SMA30,** the book stops near 0.6× pool alpha (SN29 0.32, SN101 0.17, SN128 0.19, SN112 0.13, SN19 0.35 of issuance), and residual is ≤ 15%.
* **Book-aware stress** (Stress × (1 + slope·Q/SubnetAlphaIn)), worst residual over 3×/5×/10× pumps, with ref incl. SMA30:

| Rule | SN29 | SN101 | SN112 | SN128 | SN19 | Lone 25% pin collateral (SN29 / SN101, β = 0) |
|---|---|---|---|---|---|---|
| Flat 3× | 11.5–12.4% | 6.7–7.2% | 5.0% | 4.4–7.6% | 13.7–14.9% | 2,870 / 1,654 |
| Flat 5× | 5.9–6.2% | 3.4–3.7% | 2.6% | 1.9–3.8% | 5.4–7.1% | 5,872 / 3,462 |
| **3×, slope 2** | **6.8–7.3%** | **4.0–4.3%** | **3.0%** | **2.4–4.6%** | **6.6–8.3%** | **3,947 / 2,443** |
| 3×, slope 4 | 4.4–5.6% | 2.7–3.3% | 2.4% | 2.2–3.8% | 5.6–6.7% | 5,081 / 3,297 |

* **The buy-back pause (P17) helps.** Without it, worst residual rises 10–20% (SN29 10× pump: 6.8% → 8.5%), and the team's squeeze profit roughly doubles (SN101 5×: 5.6k → 12.7k; SN19 5×: 42k → 102k).
* **The squeeze pays the team.** It nets the team 5–40k TAO, and up to 150k on SN19 with a 10× pump. That is the shorters' loss, not the protocol's. The protocol's exposure is only the residual dilution, and that falls on all stakers, including the team.
* **Ratchet over 60 days** with a single unbounded shorter pushing as far as coverage allows: the book grows to 1.24–1.31× pool alpha (ref incl. SMA30) or 1.4–1.5× (ref = EMA), and spot falls to 15–20% of today. A 5× pump at day 45 left no residual in these runs. The ref then sits well above the crashed spot, so coverage is ample.
* **Recommendation:** ref = max(EMA, spot, SMA30), Stress 3 with slope 2, and the P21 breaker at 2%. *(v1.7: P21 removed; see P23.)* Worst-case dilution in the sims is 2.4–8.3% of issuance. There is no hard bound without a cap.

**12.3 Squatting and lock-out (c).**
* **Gone with the cap and the curve:** cap squatting, and raising other shorters' APR by opening big.
* **P8 lock-out:** the v1.1 price-based block limit could be consumed by a team selling real alpha at the top of each block. That costs 533 TAO/day on SN29, 247 on SN101, 294 on SN128 and 2.1k on SN19, against emission of 1.3–30 TAO/day. It is expensive but possible, so v1.2 switches to a net-minted quantity, which other trades can't consume.
* **P16 freeze lock-out:** 8 subnets are within 1.25× of the prune threshold today and could not be newly shorted, and an extractive team near the bottom gains immunity from new shorts. Replaced in 12.4.
* **Pause abuse:** a team holding a pump to keep deleveraging paused only bleeds the positions' C at 20% APR, about 0.4% of notional per week. Not material.
* **Other routes:** minimum notional, the deposit and the 100 TAO pool depth create no lock-out. *(v1.6: the depth is 1,000 TAO. It still blocks only opens, not closes or settlement, so there is still no lock-out.)*

**12.4 v1.1 fixes with unbounded books (e).**
* **Snipe:** with no cap, size is bounded only by coverage, at 16–30% of issuance in one block.
  * Coverage-max profit at r: SN40 +241, SN29 +1,536, SN101 +937, SN128 +1,085 TAO. Stakers lose 272 / 1,716 / 1,102 / 1,282.
  * With 2k TAO of capital: +140 to +382.
  * The zone-proceeds rule makes every snipe net −interest while allowing opens. Settlement of pre-zone alpha at r is unchanged, so the dereg sims in 9.1 and the pushes in 11.1 are unchanged: their alpha is minted before the zone, except the tail of a pin. Settling that tail at proceeds lowers the griefer's r windfall slightly. This was not re-simulated.
* **Median TWAP (P6):** unaffected by book size; a one-block spike still needs 25k–80k TAO.
* **Pause (P17):** holds, and matters more without a cap (12.2).
* **P8:** must be quantity-based (12.3).
* **Pro-rata deleverage:** holds; it shares pace across 10 positions in 12.2.
* **Pot solvency at r:** the proof doesn't depend on book size. The v1.2 team-exit runs show a surplus of 1.0–1.5k.
* **Reserve:** the 2% of pool TAO cap is far below the 2–8% residual worst case. Dilution remains the bad-debt policy, so this is accepted.
* **Halving and issuance:** with no cap, real supply above the schedule is bounded only by the book (≤ about 0.6× pool alpha under 30-day pressure). Minted alpha is burned on close or dereg, so only residual is permanent. Issuance_ex excludes both (I7). *(Superseded v1.7: both count in supply.)*

## 13. Can a big short stop SN29's team extracting the pool? (`pull_sn29.py`, `pull_sn29_holders.py`, `sn29_exit.py`, `sn29_exit_alt.py`; results in `sn29_live.json`, `results_sn29_exit*.json`, `sn29_exit_tables.md`)

**Live state** (finney block 9,581,283, 2026-11-25 04:04 UTC, read-only):
* Pool TAO **7,235**. AlphaIn 3.45M, AlphaOut 3.04M. Spot 0.002096, EMA 0.002150.
* The team holds **68.9%** of AlphaOut: coldkey 5HEEEE has 2.08M α, mostly on hotkey 5Hmmmm; the owner coldkey 5HHHHH has 18k. The holdings are unlocked; the only SN29 lock is 3.2k α on the owner hotkey.
* 256 registered hotkeys hold 2.18M α. The other 0.86M sits on unregistered hotkeys.

**Reconciling the 18k.** No on-chain SN29 quantity is near 18k.
* Pool TAO is 7,235 (7,232 at block 9,578,833; 7,905 thirty days ago).
* Pool liquidity counted on both sides is 14.5k. Market cap at spot is 13.6k.
* The most the team can take out is **3,387 TAO** (47% of the pool), by dumping 100% of AlphaOut into a 50/50 pool: y·H/(x+H). With the identified 69% it is 2,731.
* The dereg payout at r gives the same number, because r·H = y·H/(x+out).
* The other ~3.85k always stays in the pool. At prune it is the pool-alpha share, which is burned under this spec.

**Results, team holding 100%, β = 0** (team TAO: no short → with short; the shorter's net profit when it settles at r):

| Case | 0.25× pool TAO collateral (Q = 8% of pool α) | 1× (Q = 18%) | 10× (Q = 38%) |
|---|---|---|---|
| a. Team dumps first, short after | 3,387 → 3,387 (0%) | 0% | 0% |
| a. Short builds (2–7 min at 1%/block), team dumps right after | → 3,016 (−11%), shorter +377 | → 2,613 (−23%), +805 | → 2,047 (−40%), +1,449 |
| b. Team dumps at day 7 | 3,412 → 3,040 (−11%) | → 2,634 (−23%) | → 2,065 (−39%) |
| b. Team dumps at day 30 (pruned first at 1×/10×, day 18–26) | 3,493 → 3,114 (−11%) | → 2,690 (−23%) | → 2,092 (−40%) |
| c. Team holds and takes r at prune | 3,855* → 3,269 (−15%), prune day 82 | → 2,690 (−30%), day 26 | → 2,092 (−46%), day 18 |

\* No-short baseline for c: never pruned within 150 days; valued as miner sales plus holdings at r on day 150.

**Other cases.**
* **69% team:** −11 to −12% at 0.25×, −23 to −34% at 1×, −40 to −49% at 10×.
* **β = 0.5:** almost no change, because SN29 has almost no outside volume. The one exception: in case c at 1×, SN29 isn't pruned within 150 days.

**Where the denied TAO goes.**
* Almost 1:1 to the shorter's profit: team −774 vs shorter +805 at 1×, case a. Interest is 1–7 TAO.
* The burned leftover (3.7–4.0k) and other stakers' payouts (0–0.9k) barely move.
* So a short doesn't keep TAO in the pool. It takes the TAO the team would have taken.

**Best case and limits.**
1. **Order.** A team that sells before the short exists keeps everything. There is no limit on selling real alpha; the short's mint is capped at 1% of pool alpha per block, so it needs 2–7 minutes to build. A symmetric 1%/block sell limit on all swaps only gets the short to −10% to −27% in the race.
2. **AMM math.** The team's dump is y'·H/(x'+H). A short shrinks y' by its proceeds and grows x' by Q. Cutting the team below half its baseline needs Q ≥ 0.5× pool alpha (team −47% at Q = 0.5x, −67% at Q = x, −84% at Q = 2x).
3. **Collateral.** Under v1.2 (3×(1+2Q/x) on the whole book), Q is only 18% / 25% / 38% of pool alpha at 1× / 2× / 10× pool TAO of collateral. Flat 3× gives 23–49%, flat 2× gives 32–61%, flat 1.5× gives 41–72%. The best of these (flat 1.5× at 10×, i.e. 72k TAO) cuts the team by −58%, but 1.5× defaulted under a +100% pump held 72 h in v1.0.
* **Bottom line:** with a short already in place, the design stops **23% (at 1× pool TAO collateral) to 40–47% (at 10×)** of the team's 3.4k. That is **11–22% of pool TAO**. It stops none of it if the team sells first. About 53% of the pool is never extractable by the team in any case.

**Spec options (Jacob's call; none recommended for the shorts pallet).**
* **Lower stress (flat 2× or 1.5×):** −53% / −58% at 10×, but squeezes and dilution get worse (sections 2 and 12.2).
* **Dereg haircut on owner or team alpha:** ineffective. The team dumps instead (dump ≥ r payout), and on chain only the owner coldkey (18k α, 0.6%) is identifiable as team.
* **Holder unstake rate limit** (outside this pallet, e.g. coldkeys holding > X% of AlphaOut may sell ≤ 1%/day): this turns case a into the short-first rows. That gives −23% to −47%, not "most", unless combined with flat ≤ 2× stress and ≥ 10× pool TAO of short collateral.
* **What the design does offer:** the short is profitable in every case (+0.3k to +1.4k), so a standing short on an extractive subnet is self-funding. It must be in place before the team moves.

## 14. Longs evidence (v1.3; `longs_sim.py`, `longs_sweep.py`, `longs_q2.py`, `longs_report.py`; results in `results_longs.json`, `results_longs_q2.json`, `longs_tables.md`; full draft in `DESIGN.md`)
Live SN29: finney block 9,581,283 (2026-11-25 04:04 UTC), read-only. Deep subnets: `state.json` (block 9,578,833, 2026-11-24 19:54 UTC). The model is the section 7 mechanism. "Escrow" (lift with no buy) was simulated and rejected: it depresses the price like a short, fights its own impact, and makes the pool pay winners.

| Question | Result (m = 2, cap 10% unless noted) |
|---|---|
| Team long, then dump (team 100%, β = 0) | 3,198 vs 3,387 baseline. At m = 1.5–3 and caps 5–50%: −32 to −1,516. Only m ≤ 1.2 profits (+3 to +121) |
| With a 1× short in place | 2,474 vs 2,613 shorts-only |
| Pump 2× for 24 h first | 3,065 (worse) |
| Outside long, team dumps (accepted) | +190 to the team. Bad debt 151, repaid from the reserve |
| Squeeze capital (1× minimum-collateral short) | 7.6k vs 8.3k without longs. Team 4,228 vs 4,128. See 8.1 for the larger shorts hole |
| Deep-subnet long (SN64 / SN51 / SN4) | Leverage 1.88× (cap 10%). +32% / −39% on a ±20% move. Liquidation at −50% |
| Dereg (c) vs (a) vs (b), cap 25% outside long | Team 3,029 / 3,027 / 3,374. Long −403 / −706 / −103. Bad debt 0 / 235 / 0. Burn 3,895 / 4,199 / 3,311. (b) rejected as a free option |
| Prune delay | SN29 under a 1× short: day 26 → 36 (10% book), day 123 (25%) |

## 15. v1.4 interest re-check (`interest_v14.py`, `results_v14_recheck.json`)
Live SN29: finney block 9,581,283 (2026-11-25 04:04 UTC). Rivals: `state.json` (block 9,578,833, 2026-11-24 19:54 UTC). Read-only. Each comparison changes only the interest routing: v1.3 is 50% burn / 50% reserve (reserve to the pot at dereg); v1.4 credits 100% to SubnetTAO.

**Accounting choice.** The interest credit is **not** a TAO inflow (I19). Two reasons:
* If it counted, a self-dealt position on the owner's subnet could manufacture recorded inflow, and its interest would come back to the owner's own pool.
* As a price lever it is weaker than a buy: a credit ΔY raises spot by ΔY/y, a buy of ΔY by about 2ΔY/y on 50/50, and the payer gets no alpha. So it opens no emission route that spot buying doesn't already offer.

| Check | v1.3 | v1.4 |
|---|---|---|
| SN29 team exit, short opened first (all budgets, β 0 / 0.5, team 100% / 69%) | 3,016 / 2,613 / 2,047 (100%) | identical |
| SN29 team, dumps at day 7 / day 30 / holds to prune (1× short, β = 0) | 2,634 / 2,690 / 2,690 | 2,635 / 2,696 / 2,696 (+1 to +6) |
| Largest change (holds, β = 0.5, 1× short, no prune by day 150) | 3,704 | 3,733 (+30, +0.8%) |
| Shorter P&L (1× short) | +805 / +584 | +805 / +583 (≤ −6 anywhere) |
| Longs: team long + dump; with a 1× short; outside long (10%) | 3,198 / 2,474 / 3,577 | identical (dump precedes interest) |
| Team-owned long into dereg (c), 10% / 25% | 2,682 / 2,626 (no long 2,690) | 2,703 / 2,762 (no long 2,696) |
| Shorts hole 6× for 10 d / 30 d: v1.2 and P22 | 11,893 / 15,195; P22 3,167 / 6,868 | 11,903 / 15,215; P22 3,194 / 6,961 |
| Rival gaming SN51 shorting SN64 by 25%, break-even APR | 10.98% | 10.95% (interest refills the victim pool, so the shorter must mint more) |
| Pairs breaking even above 20% APR (victim pool < 1k / 1k–10k / ≥ 10k) | 60 / 79 / 13 | 60 / 79 / 13 (at 15%: 109 → 108 mid pairs; at 30%: 55 → 53) |

## 16. v1.5 evidence: P22 and L9 under v1.4 interest routing (`p22_v15.py`, `results_v15.json`)
Live SN29: finney block 9,581,283 (2026-11-25 04:04 UTC). All subnets: `state.json` (block 9,578,833, 2026-11-24 19:54 UTC). Read-only. The setup is a 1×-budget short at minimum collateral, with the team pumping k× with its own TAO, holding d days, then dumping. Premium = team result minus the same pump with no short.

| Team 100%, β = 0 | v1.4 without P22: team / shorter / residual α / premium | v1.5 P22: team / shorter / residual / premium |
|---|---|---|
| 6×, 3 d | 7,906 / −4,278 / 0 / +4,237 | 2,800 / +761 / 0 / −868 |
| 6×, 10 d | 11,903 / −7,235 / 36.0k / +7,649 | 3,194 / +738 / 0 / −1,061 |
| 6×, 30 d | 15,215 / −7,235 / 36.0k / +9,170 | 6,961 / −1,778 / 0 / +916 |
| 8×, 10 d | 12,332 / −7,235 / 162.5k / +7,735 | 3,472 / +725 / 0 / −1,125 |
| 10×, 30 d | 17,584 / −7,235 / 242.8k / +9,441 | 9,547 / −2,756 / 0 / +1,404 |

* **β = 0.5:** within ±30 of these.
* **Team share 69%:** premium +4,285 to +9,574 → −1,004 to +1,514 at β = 0. The worst case, β = 0.5 at 10× for 30 days, is +9,109 → +3,173.
* **Liveness:**

  | Case | v1.4 | P22 | P22 without the 30-day lapse |
  |---|---|---|---|
  | 6× re-rate held 150 days | default, residual 36.0k α | default, 76.4k | 102.5k |
  | 4%/day rally, 45 days | 20.5k | 57.5k | 73.0k |
  | 2%/day rally, 60 days | never forced | never forced | never forced |

* **Long mirror, bear raid** (outside 10% long, m = 2; team dumps 50% and rebuys after 3–7 days; no-raid team 3,935):

  | Pause rule | Team | Long | Shortfall |
  |---|---|---|---|
  | TWAP only | 4,093 | −379 | 9 |
  | max(TWAP, SMA30) | 3,848–3,851 | +7 to +11 | 0 |

  A 30% raid doesn't force the long under either rule.
* **Long liveness, steady decline** (team drips 2% / 5% of its alpha per day): shortfall 9 / 19 (TWAP only) vs 14 / 87 (with the floor). The long stays paused until prune on day 30 / 22 and settles in phase C.
* **L15 rejected, own-subnet emission at the 10% cap:** see 8.4.

## 17. v1.7 evidence: alpha supply and halving (`pull_supply.py`, `supply_v17.py`, `results_v17_supply.json`)
Read-only finney block 9,586,188 (2026-11-25 20:25:29 UTC). S = SubnetAlphaIn + SubnetAlphaOut (the Balancer alpha reservoir is empty on finney). The issuance rate is SubnetAlphaInEmission + SubnetAlphaOutEmission per block. First halving at S = 10.5M.

* **Closest to 21M:** SN4 6.80M (32.4%), SN64 6.78M (32.3%), SN12 6.72M (32.0%), SN28 6.66M (31.7%), SN9 6.66M (31.7%). No subnet has reached 10.5M; the minimum headroom to 21M is 14.2M α.
* **No book of 10 / 18 / 30 / 60% of pool alpha takes any subnet across a halving today.**

| Subnet | Pool TAO | Pool α | S | α/block | Days to 10.5M | Halving pulled forward (days) by a book of 10 / 18 / 30 / 60% pool α | root_proportion (drop at 30%) |
|---|---|---|---|---|---|---|---|
| SN29 | 7,195 | 3.47M | 6.50M | 1.076 | 517 | 45 / 81 / 134 / 269 | 0.1335 (−0.0163) |
| SN4 | 133,518 | 2.68M | 6.80M | 1.128 | 456 | 33 / 59 / 99 / 198 | 0.1283 (−0.0120) |
| SN64 | 190,627 | 3.18M | 6.78M | 1.129 | 458 | 39 / 70 / 117 / 235 | 0.1286 (−0.0140) |
| SN51 | 189,222 | 1.62M | 6.48M | 1.134 | 493 | 20 / 36 / 60 / 119 | 0.1338 (−0.0082) |
| SN8 | 94,067 | 2.52M | 6.63M | 1.131 | 476 | 31 / 56 / 93 / 186 | 0.1312 (−0.0119) |
| SN3 | 64,337 | 3.61M | 6.61M | 1.131 | 477 | 44 / 80 / 133 / 266 | 0.1314 (−0.0164) |
| SN120 | 72,802 | 1.90M | 4.80M | 1.172 | 675 | 22 / 40 / 67 / 135 | 0.1724 (−0.0154) |

* **Permanent shift from residual on SN29** (1.076 α/block): 36.0k α ≈ 4.6 days, 76.4k α ≈ 9.9 days, 243k α (the v1.4 10×/30-day squeeze without P22) ≈ 31 days.
* **18% of pool α** is the SN29 1×-budget book (627,865 α). 60% is roughly the coverage ceiling under 30-day pressure (section 12).

## 18. v1.8 evidence (record; P24 removed in v1.9): halving guard and indirect pull (`halving_guard_v18.py`, `results_v18_guard.json`; data from section 17, finney block 9,586,188, 2026-11-25 20:25:29 UTC)
* **Thresholds** (subtensor formula, float replica checked one rao either side): T1–T5 = 10.5M, 15.75M, 18.375M, 19.6875M, 20.34M α. No subnet is past T1 today.
* **Indirect pull** (open q = rate × 7,200 × d just under T, emission crosses next block, halving d days early while held):

| Subnet, d days out | q (α) | % pool α | Notional TAO | Interest TAO/day | Emission deferred TAO/day | Ratio |
|---|---|---|---|---|---|---|
| SN29, 7 d | 54,243 | 1.6% | 112 | 0.06 | 7.5 | 121× |
| SN29, 30 d | 232,468 | 6.7% | 482 | 0.26 | 7.5 | 28× |
| SN29, 90 d | 697,404 | 20.1% | 1,445 | 0.79 | 7.5 | 9× |
| SN4, 7 d | 56,868 | 2.1% | 2,834 | 1.55 | 179.4 | 116× |
| SN4, 30 d | 243,722 | 9.1% | 12,146 | 6.66 | 179.4 | 27× |
| SN4, 90 d | 731,166 | 27.3% | 36,437 | 19.97 | 179.4 | 9× |
| SN64, 7 d | 56,882 | 1.8% | 3,413 | 1.87 | 216.0 | 116× |
| SN64, 30 d | 243,780 | 7.7% | 14,629 | 8.02 | 216.0 | 27× |
| SN64, 90 d | 731,341 | 23.0% | 43,886 | 24.05 | 216.0 | 9× |
| SN51, 7 d | 57,143 | 3.5% | 6,676 | 3.66 | 420.6 | 115× |
| SN51, 30 d | 244,899 | 15.1% | 28,612 | 15.68 | 420.6 | 27× |
| SN51, 90 d | 734,696 | 45.4% | 85,837 | 47.03 | 420.6 | 9× |
| SN3, 7 d | 57,025 | 1.6% | 1,017 | 0.56 | 64.2 | 115× |
| SN3, 30 d | 244,392 | 6.8% | 4,357 | 2.39 | 64.2 | 27× |
| SN3, 90 d | 733,175 | 20.3% | 13,072 | 7.16 | 64.2 | 9× |
| SN120, 7 d | 59,090 | 3.1% | 2,267 | 1.24 | 138.1 | 111× |
| SN120, 30 d | 253,241 | 13.3% | 9,718 | 5.32 | 138.1 | 26× |
| SN120, 90 d | 759,723 | 40.0% | 29,153 | 15.97 | 138.1 | 9× |

* "Emission deferred" is half of 1 α/block alpha-out emission × 7,200 × today's spot. Injection-cap effects are not included. The pull ends when the book closes or when natural issuance reaches T, whichever comes first.

## 19. v1.9 evidence: split-resistant health (`split_health_v19.py`, `results_v19_split.json`)
Same model as `review/split_health.py`: 50/50 constant product, fees ignored, SN29-like y = 7,235 TAO, x = 3.45M α. The book opens at minimum whole-book coverage (P2, ref = spot), and a held pump makes TWAP = spot. "Trigger" is the held pump multiple k at which Equity_TWAP hits 0.

| Book Q (pool α) | Positions | v1.8 trigger | v1.9 trigger | Literal last-in-line (rejected) | v1.8: funds buy back at trigger | v1.8 residual α |
|---|---|---|---|---|---|---|
| 10% | 1 | 3.55× | 3.55× | 3.55× | 100% | 0 |
| 10% | 10 | 4.20× | 3.55× | 3.08× | 86% | 49,532 |
| 10% | 100 | 4.27× | 3.55× | 3.04× | 84% | 54,098 |
| 18% | 1 | 3.92× | 3.92× | 3.92× | 100% | 0 |
| 18% | 10 | 5.41× | 3.92× | 3.11× | 76% | 150,294 |
| 18% | 100 | 5.59× | 3.92× | 3.03× | 74% | 163,366 |
| 30% | 1 | 4.38× | 4.38× | 4.38× | 100% | 0 |
| 30% | 10 | 7.93× | 4.38× | 3.16× | 63% | 381,994 |
| 30% | 100 | 8.43× | 4.38× | 3.05× | 60% | 412,718 |

* **Last-in-line overcount** (Σ tail costs ÷ whole-book cost, at the v1.9 trigger): Q 10% N 2: 1.09×; Q 10% N 10: 1.18×; Q 10% N 100: 1.20×; Q 18% N 2: 1.18×; Q 18% N 10: 1.37×; Q 18% N 100: 1.43×; Q 30% N 2: 1.32×; Q 30% N 10: 1.77×; Q 30% N 100: 1.92×.
* **Side effect: an earlier small position beside a later big book** (small opened alone at minimum coverage, then the big book at minimum whole-book coverage):

| Small / big (pool α) | Small trigger v1.8 → v1.9 | Big trigger v1.8 → v1.9 | Small equity right after the big open, v1.8 → v1.9 (TAO; funds) | Per-α cost ratio, book vs standalone | Small opened after big (v1.9) |
|---|---|---|---|---|---|
| 1% / 17% | 4.18× → 3.11× | 3.99× → 3.92× | 168 → 159 (221) | 1.17 | 3.92× |
| 1% / 30% | 5.15× → 3.07× | 4.52× → 4.42× | 178 → 166 (221) | 1.30 | 4.42× |
| 5% / 25% | 5.04× → 3.23× | 4.88× → 4.38× | 957 → 902 (1180) | 1.25 | 4.38× |
| 1% / 40% | 5.96× → 3.01× | 4.85× → 4.74× | 184 → 169 (221) | 1.40 | 4.74× |
| 1% / 55% | 7.30× → 2.89× | 5.28× → 5.15× | 191 → 174 (221) | 1.55 | 5.15× |

* **Lone position unchanged vs v1.8** (live pools: SubnetTAO from `depth_live.json` block 9,585,867 (2026-11-25 19:21:15 UTC); SubnetAlphaIn from `supply_live.json` block 9,586,188; q = 10% of pool α): SN29 H = 799.415 TAO both rules, diff 0 rao; SN64 H = 21,190.208 TAO both rules, diff 0 rao; SN4 H = 14,834.782 TAO both rules, diff 0 rao.

## 20. v1.9.1 evidence: reference-model Findings 3 and 4 (`model/FINDINGS.md`; `model/finding3.py`, `model/finding4.py`; results in `model/results_v19/`)
Finney snapshot: block 9,586,128 (2026-11-25 20:13:32 UTC), read-only; 110 subnets with SubnetTAO ≥ 1,000 TAO. Integer-rao, block-by-block model.

**Finding 3: longs at dereg** (`finding3_dereg_longs.json`, `finding3_summary.json`). Each subnet has a 10% short and an outside long at the 10% lift cap, with prune at day 10. I1–I23 and TAO conservation: 0 violations in 1,920 runs. Medians over 110 subnets:

| Path | Variant | Long P&L % C | Pool shortfall % D (max) | Stakers vs no-long (TAO) | Extra recycled vs no-long, % D |
|---|---|---|---|---|---|
| flat | v1.3 phase A (infeasible) | −16.5% | 0 (0) | +46 | 5% |
| flat | settle at r, r fixed first | −101% | 9.4% (32.6%) | +41.5 | 51.7% |
| flat | **v1.9.1 one-shot sale** | **−23.9%** | **0 (0)** | +41.5 | 9.0% |
| crash | v1.3 phase A | −101% | 2.3% (23.0%) | −29 | 39.9% |
| crash | settle at r | −101% | 21.1% (39.1%) | −29 | 40.9% |
| crash | **v1.9.1 one-shot sale** | −101% | **3.6% (23.5%)** | −29 | 40.0% |
| rally | v1.3 phase A | +63.7% | 0 (0) | −106 | −17.2% |
| rally | settle at r | −101% | 1.7% (29.9%) | −3 | 57.1% |
| rally | **v1.9.1 one-shot sale** | **+51.4%** | **0 (0)** | −76 | −14.2% |

* Settling longs at r is confiscatory under any r timing: flat totals are long P&L −93.6k TAO at r vs −12.4k under phase A. With r fixed first, a median 91% of D is repaid after r and ends up recycled.
* The one-shot sale reproduces phase A closely. The gap is one-shot price impact vs paced selling.
* SN29 flat (D 651, C 355, A 334k α): long −86, stakers 2,912, recycled 4,016 (phase A −60 / 2,912 / 3,990).

**Finding 4: rank rings** (`finding4_rank_rings.json`; 109 subnets, excluding SN40, which already sits below L). Pin 0.95 × L: EMA fallback 60/109, median 1.43 days (min 0.50), min capital 777 TAO; partial mean 57/109, 2.59 days; prefill 46/109, median 27.59 days (min 19.59). Pin 0.8 × L is the worse attack (EMA fallback median capital 10.3M). SN64, SN4 and SN51 can't be pushed in any mode with 100M TAO.

## 21. v2.0.1 evidence: deleverage exit rule (`model/v201`, `compare_v201.py`; results in `model/results_v201/`)
Same 110 live subnets and scenarios as the v2.0 rerun, v2.0 vs v2.0.1 with `exit_hysteresis_bps` = 500. The only change between the two runs is the exit rule.
* **No new invariant violations** in any scenario. No residual alpha and no long shortfall appear or disappear.
* **Squeezes (6× for 3, 10 and 30 days; 10× for 30 days):** positions that recover now return to Open instead of staying in a paced buy-back. Median per-subnet change: shorter −9.5 to −11.8 TAO, team premium −2.9 to −5.3 TAO (the team does slightly worse), interest to the pool +5.7 to +6.1 TAO. The largest single-subnet move is 2.36k TAO on a 6× 30-day squeeze.
* **Split books (1 and 10 coldkeys, 5× held):** the median is unchanged. Some subnets end with more q still open (up to 19.2k α), because positions left deleverage once healthy instead of buying back at the top.
* **Rally (20%/day), dereg (long at r, zone dilution) and the long crash paths:** identical to v2.0.
* Model fix found while writing the differential vectors: the model priced health with the swap fee, but spec 1.4 says fee-free. `model/v201` now uses fee-free BuyCost and SellProceeds for health, rounding the short cost up and the long value down. The fuzz rerun uses that version.
