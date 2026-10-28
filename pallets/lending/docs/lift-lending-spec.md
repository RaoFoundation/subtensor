# Lift-based lending on main's Balancer pools: final spec (v5.4: one-year term + directional EMA band on opens)
**Version:** v5.4, 2026-10-28. Encodes Jacob's locked decisions as of 2026-10-20 (term locked that day; EMA band added 2026-10-28). Supersedes v5.3 (2026-10-20, kept as `lift-lending-spec.v5.3-backup-20261027.md`), v5.2 (2026-10-20, kept as `lift-lending-spec.v5.2-pre-gaps-20261020.md`), v5.1 (2026-10-19, kept as `lift-lending-spec.v5.1-backup-20261020.md`), v5 (2026-10-19, kept as `lift-lending-spec.v5-backup-20261019.md`) and v4 (floor 33%, 2026-10-16, kept as `lift-lending-spec.v4-backup-20261019.md`).
**Target:** RaoFoundation/subtensor `main` @ 7ba9601d (2026-10-15 15:53 CT). PR #3211 is a parts bin only (§11).
**Numbers:** live chain state at block 9,298,539 (2026-10-16), with real `Swap.SwapBalancer` weights; P&L uses the same 117-subnet bootstrap paths as before.
**Files:** scripts `lending-pnl/{final,final2,final3,sim_final2,pnl_final2,plot11}.py`; data `data/final*.{json,csv}` (floor sweep: `data/final3.json`, `data/final3_pnl.csv`), `data/balancer_now.json`; figure `fig11_final.png`. v5 additions: `lift-lending-raise/calc.py` + `res.json` (escrowed-long impacts, raise example); companion implementer notes `lift-lending-raise-cli.md`; v5.1: `lift-lending-dereg/calc.py` + `res.json` (dereg settlement examples); why not zero-loss LTV: `zero-loss-ltv-example.md`; v5.2: `sn29-analysis.md` + `sn29/calc.py` → `res.json` (`r`-pump attack and fix numbers); v5.3: `lending-pnl/sim_final4.py` (escrowed-long books, same seeds and paths as `sim_final2.py`) + `lending-pnl/final4.py` → `data/final4.json`, `data/final4_pnl.csv`, `data/final4_long_default.csv` (thin floor on, tier LTV, escrowed-long P&L); `lift-lending-raise/calc.py` → `res.json` key `v53_raise_table` (raise tables, thin-tier mirrors).

## Changelog: v5.4 vs v5.3
* **2026-10-27 (Oct 27, 2026): one-year fixed term (Jacob, locked 2026-10-20).**
* **Every lending position (long and short) expires one year after it is opened** and is then settled automatically at the smoothed lending reference (EMA) price, using the same settlement math as dissolution smoothed settles (§13.3), with **no liquidations** (§15).
* Storage shape: `Position.expires_at`, plus an expiry due-index / sorted bucket queue (`Expiry` / `ExpiryIndex` / `NextExpiry` / …) processed in `on_idle` with bounded per-block weight (resume via `ExpiryBucketCursor`).
* **Top-ups / adds keep the original `expires_at`.** Partial repay leaves `expires_at` unchanged. If subnet dissolution starts before expiry, **dissolution settle wins** and clears the expiry queue entry.
* **2026-10-28: directional EMA band on opens (§5.4).** New opens (and increases) that buy alpha reject if post-trade spot > `EMA · (1 + ema_band_bps)`; opens that sell alpha reject if post-trade spot < `EMA · (1 − ema_band_bps)`. Default `ema_band_bps = 1_500` (15%). Opposite-side opens always allowed; closes / repays / settles / expiries never blocked. Makes trade-splitting pointless (limit is on price state). Error: `PriceTooFarFromEma`.
* Economics unchanged: 25% LTV / 15% thin, 100% APR burn-all interest, escrowed longs and shorts, pro-rata partial repay, custody earns no emissions, dissolution `r` protections, new-subnet gate and ramp.
* Touched: header, §0, §5.4 (new), §8, §9, §15 (new).

## Changelog: v5.3 vs v5.2
* **2026-10-20 (Oct 20, 2026): gap-closing release.**
* **Thin floor on everywhere (§2.2, §6, §9, §10).** `thin_floor = 15%` below `thin_tao = 3,000 τ` is the locked value, and every table now uses it: capacity 826k τ total / 413k per side (was quoted 834k / 417k without it), 32 thin pools at 7.5% per side, thin long LTV 15% in the raise tables.
* **Dissolution phase-0 gap (§13.1, §13.2, §13.3, §13.6).** Main's first cleanup phase sells every basket fund's alpha on the dissolving subnet through the AMM and sends the TAO to root, *after* v5.2 snapshotted `T_flag`. The alpha stays in `D` (it moves into pool alpha), but the TAO leaves the pot, so v5.2's `r` was too high. v5.3 reads the pot at the start of `LendingSettlement` (after phase 0) and shifts the frozen EMA by the same outflow: `T_settle = min(T_flag, t_ema_flag) − Δ0`, `Δ0 = max(0, T_flag − T_pre)`. `t_ema` stops updating at the flag.
* **New rules for older-review gaps:** execution limits (min-out / max-price and a block deadline on every trading extrinsic, §5.2); partial repay for both sides (§4.4); what counts as long collateral, i.e. outside alpha is never counted (§5.3); custody, hotkeys, emissions and proxies (§14); new-subnet minimum age and capacity ramp (§2.3).
* **P&L re-run with escrowed longs (§10.2, §10.3, §10.5).** The stale free-long flags are gone. Fair paths, adopted policy: pool **−19 / −44 / −87 kτ** (mean / p5 / p1; was −60 / −135 / −168 with free longs). Historical-drift paths flip sign: **−163 / −1,110 / −1,869 kτ** (was +835 / +89 / +43), because an escrowed long leaves the pool short the alpha the borrower bought, and the borrower only repays when that pays. Burn unchanged (881 kτ/yr fair).
* Touched: header, §0, §2.2, §2.3 (new), §4.4 (new), §5, §5.2–5.3 (new), §6, §6.1, §7, §8, §9, §10, §12.2, §13.1–13.3, §13.6, §13.7, §14 (new).

## Changelog: v5.2 vs v5.1
* **2026-10-20 (Oct 20, 2026): pump-resistant dissolution rate (§13.2).** v5.1 read `r = SubnetTAO / D` live in the dissolve block, so an owner could buy TAO into the pool in the same block as the `register_network` that prunes the subnet, inflate `r`, and take the shorts' escrow and collateral (SN29: ≈6.4k τ erases the shorters' gain; ≈70.1k τ for one block takes all 5,264 τ of collateral, `sn29-analysis.md`). v5.2 settles on `T_settle = min(T_flag, T_ema)` (the pot TAO at the dissolving flag vs its 24 h EMA), `r = T_settle / D` with the same all-alpha `D`, then `r = min(r, SubnetMovingPrice)`. Short and long settlement formulas are unchanged. New root-settable `LiftParams.settle_ema_blocks` (default 7,200 = 24 h) and per-subnet `t_ema` storage, initialised to the current `SubnetTAO`.
* SN29 numbers with the fix: shorts' debt capped at ≈917 τ, shorts keep ≥ ≈181 τ gross; a 70.1k τ one-block pump moves the 24 h EMA by ≈9.7 τ (0.14%) and the debt by ≈0.8 τ (§13.2.1).
* Touched: §0, §8, §9, §13.2, §13.3, §13.5, §13.6, §13.7.

## Changelog: v5.1 vs v5
* **New §13 "Subnet deregistration / dissolution settlement",** built around Jacob's core requirement: **shorters must make money on a dereg event.** Short escrow is never swept into the pot. Shorts stay open-able right up to the dissolving flag.
* Main's dissolution pot logic is verified against local main (`6a972693a`, a descendant of `7ba9601d`). The settlement rate `r` is defined as **pot TAO per alpha, counting all alpha including pool alpha**. Main's literal legacy *staker* rate was rejected because on legacy subnets it confiscates shorts (13.2, 13.5).
* Settlement rules for shorts (`E_S + C − P·r` to the borrower), longs (`r·(A+C)` repays `P` first; underwater is the one shortfall case), pending interest (burned at `r`), lift-leg re-add, a new metered `LendingSettlement` phase between main's 2.1 and 2.2, and `Settled` events.
* SN32 worked example (13.5): 500 τ short, −90% over 30 weeks → payout 2,112.7 τ on 2,000 τ posted, gross dereg gain 400 τ, net +112.7 τ after 287.7 τ interest.
* §0, §5, §7, §8, §9 updated to match. §9 drops #3211's "max(historical mark, funded value)" short-debt valuation and its "recoveries outside the pot" routing.

## Changelog: v5 vs v4
* **Longs are escrowed, not free.** v4 paid borrowed TAO out freely. v5 forces the borrowed TAO into the pool as an alpha buy in the same extrinsic and escrows the alpha bought, mirroring forced shorts. The borrower never gets unlocked TAO from the loan itself. Rewritten: §0, §4 (long flow), §5, §8, §9, §10.2–10.4.
* **Long defaults are now quantity-whole,** like short defaults. v4's "long default loses `P` TAO" is gone (§4, §8).
* **New atomic raise path** (§4.3, §12): `open_long_and_sell` extrinsic / `btcli lending raise`. One extrinsic opens an escrowed long and sells the team's *own, non-collateral* alpha for free TAO. The free TAO comes from the spot sale, not from unlocking the loan.
* **Pull-on-borrow made explicit** (§0, §2.1, §3). Lending liquidity is lifted from the Balancer pool proportionally, only when someone borrows. There is no pre-sized vault allocation. Borrow demand decides how much liquidity is in lending, up to the capacity bounds.
* **Interest:** 100% APR, **burn all** interest. Explicitly **not** return-until-whole. Rationale added (§6.1).
* **LTV:** 25% / 15% thin is unchanged. Karl's full zero-loss floor LTV is rejected, with reasons (§5.1).
* `G` (un-lifted depth reference) redefined for escrowed longs, because the forced buy returns the lifted TAO to `y` (§2).
* §10.3 long P&L and §10.2 raise tables are flagged: they were modelled on free longs and need a re-run (§10).
* Unchanged: capacity rule (floor 33%, thin floor 15% < 3k τ, impact rule, ceiling 50%, 50/50 sides, no per-account caps), Karl dynamic valuation, no liquidations, paced long-interest sales.

## 0. Overview
* **Pools:** today's Balancer pools, curve unchanged. No superellipse, no floors, **no vault**.
* **Pull-on-borrow (locked).**
  * Liquidity moves into lending **only when someone borrows**. It is lifted out of the Balancer pool **proportionally** (both reserves scaled by the same factor, weights and price unchanged, §3).
  * Nothing is pre-sized or parked in a fixed vault allocation. **How much liquidity sits in lending is decided by borrow demand**, up to the capacity bounds: floor / impact rule / ceiling, and 50% per side (§2).
  * This keeps lending separate from trading: the pool trades on its normal curve with whatever is not currently lent. Repays and defaults re-add (§3).
* **Emission distribution and injection are unchanged from main.**
  * `get_subnet_terms` sets `alpha_in = tao_emission/price`, capped at `root_proportion · alpha_emission`, with `root_proportion = tao_weight/(tao_weight + alpha_issuance)`.
  * TAO above the cap goes to chain buys.
  * `inject_and_maybe_swap` → `adjust_protocol_liquidity` adds the injection to the reserves and re-weights, so price is unchanged and **the pool deepens**.
  * As a subnet ages, injection gives way to chain buys. Chain buys also add TAO to the pool.
* **Capacity** is defined by the pool's own depth and **grows automatically as the pool fills**. Thin pools (`G_ref` < 3,000 τ) use the 15% thin floor. A brand-new subnet gets no lending until it reaches a minimum age, then capacity ramps in (§2.3).
* **Escrow on both sides (locked).**
  * **Forced shorts:** every alpha loan is sold into the pool at once and the TAO proceeds are escrowed (Karl). No one can borrow alpha and sit on it, so a subnet owner can't block shorting by self-borrowing the alpha side.
  * **Forced longs:** every TAO loan buys alpha from the pool at once and the alpha bought is escrowed. **There are no free longs:** the borrower never receives usable TAO from the loan itself.
  * Both defaults are therefore **quantity-whole** for the pool.
* **Raising TAO (teams).** A team that needs free TAO uses the **atomic raise** (§4.3, §12). One extrinsic opens an escrowed long against alpha collateral **and** sells the team's own, non-collateral alpha into the pool. The free TAO is the proceeds of that spot sale. Because the sale reverses the long's forced buy in the same block, net price impact is ≈0, and the raise behaves like a repo: sell alpha now, buy it back for `P` TAO on close.
* **Interest: 100% APR, all burned (locked).**
  * Shorts pay TAO, which is burned.
  * Longs pay alpha, which is sold into the pool on a paced schedule, and the TAO received is burned.
  * **Not** return-until-whole (§6.1).
* **No liquidations, no recall, no recourse.**
* **One-year fixed term (locked).** Every long and short expires one year after open and auto-settles at the smoothed reference price (§15). Adds keep the original expiry; partial repay does not change it; dissolution settle wins if it comes first.
* **Directional EMA band on opens (locked).** Longs (buys) cannot open if post-trade spot > EMA·1.15; shorts (sells) cannot if post-trade spot < EMA·0.85. Opposite side always allowed; exits never blocked (§5.4).
* **User protections:** every trading extrinsic carries a min-out / max-price limit and a block deadline (§5.2). Partial repay is supported on both sides (§4.4). Escrow and collateral sit in lending custody on a non-validating hotkey and earn no emissions (§14).
* **Shorters make money on deregistration (core requirement, §13).** Shorting a subnet down to dereg pays the shorter: at dissolution a short's debt is `P` alpha valued at the low dissolution rate `r` (smoothed and capped at the EMA price so it can't be pumped at the flag, §13.2). They keep `E_S + C − P·r`. Their escrow is **never** confiscated or swept into the pot. New shorts are **not** blocked near the dereg threshold, only once the subnet is actually flagged dissolving. All loans settle inside the dissolution flow, before the staker pot is paid.

## 1. Pool facts (main)
* Spot price: `p = (w_b/w_q)·y/x` (`Balancer::calculate_price`).
* A fee-free buy of `Δy` TAO gives `p'/p = (1 + Δy/y)^{1/w_b}` (the inverse of main's `calculate_quote_delta_in`).
* Selling alpha `Δx` returns `Δy = y·(1 − (x/(x+Δx))^{w_b/w_q})`.
* Live `w_b` ∈ [0.500000, 0.5000003] on all 128 subnets. The formulas use each subnet's live `w_b`.
* Default fee: 33/65535 ≈ 0.05%.

## 2. Capacity (total, then split by side)
### 2.1 What capacity means under pull-on-borrow
* `Cap_side` is an **upper bound**, not an allocation. At any moment the amount in lending is `L_S + L_L`, which equals whatever borrowers have opened. Between zero and the cap, the market decides.
* Nothing is lifted until a loan opens. Each loan's lift is sized to its own principal (§3).

### 2.2 Rule
* **"A 1,000 τ buy moves price 10%"** means spot after a fee-free, atomic buy of exactly 1,000 TAO divided by spot before, minus 1, equals 0.10.
* **Threshold depth:** `y* = 1000/(1.10^{w_b} − 1)`, which is **20,488 τ** at `w_b = 0.5`.
* **Un-lifted depth reference:** `G = y + L_S + E_S + (L_L − B_L)`.
  * `L_S`, `L_L` = the TAO legs of outstanding short and long lifts, recorded at open.
  * `E_S` = TAO held in short escrow, so the forced short sale's TAO is counted back.
  * `B_L` = TAO spent on forced long buys, which goes back into `y`. A long's lifted TAO leg *is* its principal and is fully spent on the forced buy, so `B_L = L_L`. The long term cancels, which gives **`G = y + L_S + E_S`**. It is written out in full so implementers see why.
  * Lifts, forced sales and forced buys leave `G` unchanged (up to price moves). **One side's lending activity therefore cannot shrink the other side's limit.**
  * Ordinary trades, including the raise's spot sale (§4.3), move `y` and therefore `G`, like any other unstake. That is intended: the pool really has less TAO after a sale.
* **Smoothing:**
  * `G_ema`: 24 h half-life geometric EMA, using #3211's `EMA_WEIGHT_BITS`, updated in `on_finalize` for every subnet.
  * `G_ref = min(G_spot, G_ema)`.
* **Rule.** Governance parameters: `floor = 33%`; **`thin_floor = 15%`, on by default (locked)**, when `G_ref < thin_tao = 3,000 τ` (§10.5); `ref = 1,000 τ`; `move = 10%`; `ceiling = 50%`; `split_short = 50%`. Today 32 pools are thin: their total capacity is 15% of `G` (7.5% per side), and their longs use the 15% LTV tier (§5).
```
f         = (G_ref < thin_tao) ? thin_floor : floor
Cap       = ramp(age) · max( f·G_ref ,  min( G_ref − y* ,  ceiling·G_ref ) )      // ramp = 1 for every subnet past the new-subnet ramp (§2.3)
Cap_short = split_short · Cap ,   Cap_long = (1 − split_short) · Cap
open short iff  L_S + ΔL ≤ Cap_short ;   open long iff  L_L + ΔL ≤ Cap_long
always:  y − ΔL ≥ MinimumReserve
```
* In words: capacity = MAX(floor % of pool, the depth above which a 1k τ buy moves price 10%), capped by the 50% ceiling, then split 50/50 between sides.
* `floor` and `thin_floor` are set by root via `set_lift_params`, bounded to ≤ `ceiling`. Lowering them never recalls anything (§7).
* **No cross-borrowing:** unused capacity on one side never carries over to the other.
* **No per-account, per-hotkey or per-position caps** (§5).
* The thin tier switches on `G_ref`, the same smoothed depth as the rest of the rule, so a one-block buy can't lift a pool out of the thin tier. Crossing 3,000 τ moves capacity from 15% to 33% of `G` (450 τ to 990 τ at the boundary). Crossing back blocks new opens on any side that is now over its cap; nothing is recalled (§7).
* **Future parameter (off by default):** `rebalance_target` (for example 60/40 toward the busier side) with `rebalance_rate` (at most a few % of `Cap` per day). It moves `split_short` slowly, and only within the unused headroom of the side being reduced.

### 2.3 New subnets: minimum age, then a capacity ramp
* **Gate.** No `open`, `open_exact`, `open_long_and_sell` or increase on a subnet until **all** of these hold:
  * subtoken trading is enabled on it (`SubtokenEnabled`, the owner's start call);
  * its age `now − NetworkRegisteredAt ≥ new_subnet_min_age` (default **50,400 blocks ≈ 7 days**);
  * the lending reference is warm (`ReferenceWarmup`, 24 h) and `G_ema` has been initialised for at least one full EMA window (24 h).
  * Otherwise opens fail with `SubnetTooNew`.
* **Ramp.** After the gate, `ramp(age) = min(1, (age − new_subnet_min_age) / new_subnet_ramp)`, default `new_subnet_ramp` = **216,000 blocks ≈ 30 days**, linear. It multiplies the whole of `Cap` (both sides, floor and thin floor included), so the 50/50 split, no cross-borrow and the thin tier are unchanged. A subnet registered 37 days or more ago has `ramp = 1`. The §10 numbers assume `ramp = 1` for every live pool; this was not checked per subnet against registration blocks.
* **Why.** A new pool starts at the 1,000 τ lock (thin tier). Its first days have no meaningful EMA price, `G_ema` or `SubnetMovingPrice`. These are exactly the inputs that protect `V_dyn` (§5), capacity smoothing (§2.2) and dissolution (§13.2). The owner also holds most of the float, so day-0 borrowing would mostly mean the owner trading against seed liquidity. The ramp spreads the forced-trade impact over the period when the EMA references become meaningful.
* **Unchanged:** the formula itself (§2.2), the thin floor, LTVs, and shorts staying open-able up to the dissolving flag. Network immunity is not used as the gate, since that would mean no lending for the whole immunity period (runtime default 1,296,000 blocks ≈ 180 days).
* Lowering `ramp` (or the age falling back under the gate after a netuid is reused) blocks opens only; it never recalls anything (§7). A reused netuid starts its age again at its new `NetworkRegisteredAt`.
* Both parameters are root-settable in `LiftParams` (`new_subnet_min_age`, `new_subnet_ramp`). **Jacob's call:** the defaults (7 d + 30 d linear), or tie the gate to the end of network immunity.

## 3. Lift primitive (swap pallet): the pull-on-borrow mechanism
* **`lift(netuid, g)`:** reduce both reserves by the factor `g` with **weights unchanged**, so price is unchanged.
  * Move the amounts to lending custody, adjusting `SubnetTAO`/`SubnetAlphaIn`/`TotalStake` as #3211's `fund_unreachable_reserves` does.
  * It is called only from a loan open or increase. There is no standing allocation.
* **`readd(netuid, τ, α)`:** reuse main's `adjust_protocol_liquidity`/`try_update_balancer`.
  * Proportional amounts re-add with weights unchanged.
  * One-sided amounts re-weight price-neutrally.
  * Anything out of weight bounds waits in `BalancerTaoReservoir`/`BalancerAlphaReservoir`.
* **`g`** is sized so that the lent leg equals the principal: `g = P/x` for a short (alpha principal), `g = P/y` for a long (TAO principal).

## 4. Flows
Notation: `C` = collateral, `P` = principal, `(α_L, τ_L)` = the lift legs, `E_S` = short escrow (TAO), `A` = long escrow (alpha).

### 4.1 Forced short (borrow alpha, TAO collateral `C`, LTV 25%)
* Open:
  1. Checks (§5).
  2. `lift` gives `(P α, τ_L)`.
  3. **The `P` alpha is sold through the pool in the same extrinsic, with no option to receive alpha.** The proceeds `E_S` are escrowed.
  4. `τ_L` is held.
  5. `L_S += τ_L`.
* Repay:
  1. `E_S` plus collateral buys back `P` alpha, or the borrower supplies `P` alpha and takes `E_S`.
  2. `readd(τ_L, P)`.
  3. Refund the remaining collateral.
* Default (collateral exhausted by interest): `readd(τ_L + E_S, 0)`. **The pool is quantity-whole**: the sold alpha is already in the pool, and the burned interest came from the borrower.

### 4.2 Forced long (borrow TAO, alpha collateral `C`, LTV 25% / 15% thin): escrowed, no free TAO
* Open:
  1. Checks, including dynamic valuation (§5).
  2. `lift` gives `(α_L, P τ)`.
  3. **The `P` TAO buys alpha through the pool in the same extrinsic, with no option to receive TAO.** The alpha bought, `A`, is escrowed in lending custody under the position. It is not staked to the borrower and cannot be unstaked, moved or sold while the position is open.
  4. `α_L` is held.
  5. `L_L += P`; `B_L += P`.
* Repay (mirror of the short):
  1. The borrower supplies `P` TAO and takes `A`, **or** (`--no-repay-from-wallet`) `A` plus as much collateral as needed is sold for `P` TAO, and any surplus TAO goes to the borrower.
  2. `readd(P, α_L)`; `B_L −= P`; `L_L −= P`.
  3. Refund the remaining collateral (and `A`, if the borrower repaid from the wallet).
* Default (collateral exhausted by interest; there is no liquidation):
  1. `readd(0, α_L + A)`.
  2. **The pool is quantity-whole.** The `P` TAO is already back in the pool from the forced buy, and the alpha it bought comes back from escrow. Starting from `(y, x)`: open gives `(y, x − α_L − A)`, then default gives `(y, x)`.
  3. All of the collateral has by then been sold into the pool as interest, and the TAO received was burned.
* What the borrower gets: leveraged long exposure only. Rising price makes `A` worth more than `P` on close. **No usable TAO.**

### 4.3 Atomic raise (escrowed long + spot sale of own alpha), the "borrow TAO on alpha" flow
* For teams that need free TAO. One extrinsic, `open_long_and_sell` (§12), does all of this or nothing:
  1. Lock `C` collateral alpha from the caller's stake on `hotkey`.
  2. Open the escrowed long of §4.2: lift `(α_L, P τ)`, forced buy of `A` with `P`, escrow `A`.
  3. Sell `S` alpha of the caller's **own, unlocked, non-collateral** stake into the pool. The TAO out goes to the caller's coldkey as free balance. By default `S = A`.
  4. Emit events.
* **The free TAO is the proceeds of step 3, a spot sale.** It is not borrowed TAO that has been released. The loan's `P` stays escrowed as `A` for the whole life of the position.
* Why atomic, and why `S = A` by default:
  * Step 2's buy and step 3's sale of the same alpha quantity are an exact round trip on the lifted pool. Price ends where it started (two fees ≈ 0.1%), and **free TAO ≈ 0.999·P**.
  * Example, SN32 (median, 6,699 τ), `P = 500 τ`: the forced buy alone moves price **+16.8%**. Selling `S = A = 179,274 α` returns **499.5 τ** to the caller, and price ends **+0.008%** from the start (`lift-lending-raise/res.json`).
  * Done as two transactions, the first leg's +17% spike would be sandwichable, and a failure between legs would strand the team with a long and no TAO.
* **Economics:** a repo.
  * The team sells `S` alpha now for ≈`P` TAO and holds the right to buy `A` alpha back for `P` TAO on close.
  * Its alpha cost is `C + S` (≈ `C + P/p`).
  * **If the team walks away, the pool is still quantity-whole** (§4.2 default). The team keeps the sale proceeds, which were a normal market sale, and loses `A` and `C`; `C` was burned as interest.
* `S ≠ A` is allowed (any `S ≥ 0`, bounded by `min_tao_out`). `S < A` leaves net upward price impact; `S > A` leaves net downward impact. `S = 0` is just an escrowed long through the raise entry point; the CLI rejects it and points to `btcli lending open`.

### 4.4 Partial repay (both sides)
* **Call:** `repay(netuid, principal_part, repay_from_wallet, max_payment, min_refund, valid_until)`. It repays `ΔP` of principal with `k = ΔP / P`, where `0 < k ≤ 1`; `k = 1` is `close`. It goes in the same proxy group as `close` (§14). Limits and deadline follow §5.2.
* **Interest first.** Accrue the position's interest to the current block on the **whole** position and charge it the normal way: shorts burn TAO from collateral; longs move alpha into `LongInterestPending`. Then scale `annual_interest` by `(1 − k)`. The per-block remainder carries over, so nothing is rounded up until the final close. There is no refund of interest already charged and no prepayment.
* **Short, slice `k`:**
  * Either the borrower supplies `k·P` alpha and takes `k·E_S`, or (`repay_from_wallet = false`) `k·E_S`, then up to `k·C`, buys back `k·P` alpha through the pool, limited by `max_payment`;
  * `readd(k·τ_L, k·P)`; `L_S −= k·τ_L`;
  * release the slice's leftover escrow plus `k·C` to the borrower (≥ `min_refund`).
  * If the slice's own `k·(E_S + C)` can't buy `k·P` back (the price rose sharply), the call fails with `InsufficientEscrow`. The borrower can repay that slice with wallet alpha instead. The rest of the position is never tapped to fund a slice.
* **Long, slice `k`:**
  * Either the borrower supplies `k·P` TAO and takes `k·A`, or (`repay_from_wallet = false`) `k·A`, then up to `k·C`, is sold for `k·P` TAO, and any surplus goes to the borrower (≥ `min_refund`);
  * `readd(k·P, k·α_L)`; `L_L −= k·P`; `B_L −= k·P`;
  * release `k·C` (and `k·A` if repaid from the wallet) as stake to `(position.hotkey, owner)`.
* **Pro-rata release, not "release down to LTV".** Every position field (`P`, `τ_L`/`α_L`, `E_S`/`A`, `C`, `annual_interest`) scales by `(1 − k)`. The remaining position therefore keeps exactly the LTV, `V_dyn` headroom and interest backing it was opened with, and no revaluation is needed on a non-open path, consistent with no re-check and no liquidation (§5). **Jacob's call (optional):** also allow releasing extra collateral down to the tier LTV against a fresh `V_dyn`. This is more capital-efficient but adds a valuation step to repay.
* **Dust:** the remainder must be either 0 or have opening value `≥ MinimumLoanValue` (1 τ), else `AmountTooSmall` (close in full instead). A later increase re-runs `V_dyn` on the whole position, as now.
* Capacity frees up immediately (`L_side` falls). Dissolution settles whatever remains (§13.3). Once dissolving, `repay` fails with `SubnetUnavailable`, like `close`.

## 5. Opening checks
* Enabled; subnet past the new-subnet gate (§2.3); subnet live and **not flagged dissolving** (`do_dissolve_network` has not run, §13.1). There is deliberately **no** block for being the prune candidate, being near the dereg threshold, or a low price (§13). Reference warmed up (#3211 `References`, `ReferenceWarmup`).
* `opening_value ≥ MinimumLoanValue = 1 τ`.
* **LTV (locked):**
  * shorts 25%;
  * longs (including the raise) 25%, and **15% where `G_ref < 3,000 τ`** (thin tier, §10.5);
  * for shorts, LTV doesn't change the pool's per-unit tails, so no thin tier is needed there.
* **Karl's dynamic long valuation** (at open; on increase it covers the whole position). Kept unchanged:
```
A_ahead = alpha collateral of all open longs on the subnet + LongInterestPending   // alpha that can hit the pool first
pool'   = reserves after this loan's lift (y − P, x·(1 − P/y)); same weights
pool''  = pool' after selling A_ahead            (main's sell formula)
Q       = TAO out of selling C into pool''       (main's sell formula, fee included)
V_dyn   = min( C·p_ref , C·p_spot , Q )          // p_ref = 24 h EMA (References)
require P ≤ LTV_tier · V_dyn
```
  * `pool'` is the state after the lift and **before** the forced buy. This is conservative: the buy would raise `p_spot`. For a raise with `S = A`, `pool'` is also (to fees) the state after the whole extrinsic.
  * `p_spot` is read **before** the extrinsic's own forced buy, so a long can't value its collateral at the price its own buy creates.
  * Escrow `A` is **not** in `A_ahead`: on default it re-adds price-neutrally and is never sold. It is not collateral.
  * For `open_exact`, `P` is known, so this is one evaluation. `open` (max borrow) uses ≤ 16 bisection steps.
  * It is checked on open and increase only. With no liquidations, a later breach could not be acted on, so there is no re-check.
  * Because `A_ahead` includes every existing long's collateral, splitting a long into many positions or keys gains nothing. Later longs see a thinner effective pool.
  * With escrowed longs, principal is quantity-whole on default, so LTV and `V_dyn` now protect **interest backing** (the burn) and the **repay incentive**, not principal recovery.
* **No per-account, per-hotkey or per-position caps.** Keys are sybil-cheap, and every risk limit is pool-level:
  * side caps;
  * dynamic valuation with `A_ahead`;
  * `MinimumLoanValue` (dust and weight).
* The old per-position cap is removed. Its only remaining purposes were spreading forced-short price impact, which is unchanged by splitting, and splitting longs, which `A_ahead` now handles.
* Keep `MaxPositionsPerSubnet`/`MaxTotalPositions` as storage and weight bounds only.
* **APR 100%:** `annual_interest` = opening value, in collateral units, charged every `InterestPeriod` (7 days). Reuse #3211 `Due`/`NextDue`/`collect_due`.

### 5.1 Why not Karl's zero-loss floor LTV
* Karl's floor sets LTV so the collateral's worst-case sale covers principal. The worst case is: full lift, full short book force-sold, **all** outstanding alpha plus 30 days of emissions dumped ahead of the collateral.
* On live pools that gives **≈8–9% (median SN32) and ≈6% (thinnest SN70)**; see `zero-loss-ltv-example.md`. It is rejected as too strict:
  * The "all outstanding alpha is sold first" stress drives the result. Outstanding alpha is 1.4–1.6× pool alpha and includes long-term, owner and validator stake.
  * At 6–9%, a team needs ≈11–17× the TAO it raises in alpha collateral. That would make the raise path useless on most subnets.
  * Under v5 escrow, a long default is quantity-whole anyway, so the premise (collateral sale must repay principal) no longer applies. Collateral backs interest.
* `A_ahead` in `V_dyn` already captures the realistic part of the dump: other longs' collateral and pending interest.

### 5.2 Execution limits: slippage and deadline on every trading extrinsic
* **Deadline.** Every user extrinsic that trades or moves position funds (`open`, `open_exact`, `repay`, `close`, `open_long_and_sell`) takes `valid_until: BlockNumber` and fails with `DeadlinePassed` if `now > valid_until`. This comes on top of transaction mortality, which wallets often set long. CLI default: `--deadline-blocks 10`.
* **Limits on forced trades** (amounts are the user's floors and ceilings; `0` / `u64::MAX` mean no limit, and the CLI never sends those):

| extrinsic / path | forced trade | user limit | fails with |
|---|---|---|---|
| short `open` / `open_exact` / increase | sell `P` alpha | `min_proceeds`: escrow TAO `E_S` ≥ limit (≡ max price impact) | `SlippageExceeded` |
| long `open` / `open_exact` / increase | buy with `P` TAO | `min_escrow_alpha`: escrow `A` ≥ limit (≡ max average buy price `P/A`) | `SlippageExceeded` |
| `open` (max-borrow mode), both sides | none (sizing) | `min_borrow`: solved `P` ≥ limit | `BelowMinimumBorrow` |
| `open_long_and_sell` | buy with `P`, then sell `S` | `min_tao_out` on the sale (§12.2); new `min_escrow_alpha` on the buy, needed when `S` is given explicitly (with `S = A` the sale floor already bounds both legs) | `SlippageExceeded` |
| short `close` / `repay`, from escrow | buy back `P` (or `k·P`) alpha | `max_payment`: TAO spent ≤ limit; `min_refund` | `SlippageExceeded` / `BelowMinimumRefund` |
| long `close` / `repay`, `repay_from_wallet = false` | sell `A` (then `C`) for `P` | `min_refund`: surplus TAO ≥ limit | `BelowMinimumRefund` |
| wallet-repay paths | none | `max_payment` caps `P` + interest charged now | `SlippageExceeded` |

* **Forced trades without a user limit are price-neutral or protocol-paced:** default re-adds (no swap); paced long-interest sales (1 bps per block cap plus the crash guard, §6); dissolution (no swaps, §13).
* The runtime only enforces limits. The CLI computes every limit from the runtime quote APIs and `--max-slippage` (default 1%) and shows them in the review. Submission goes through the MEV-shield flow.
* The old raise-only errors `RaiseSlippage` and `RaiseDeadlinePassed` become the general `SlippageExceeded` and `DeadlinePassed`.


### 5.3 What counts as long collateral (outside alpha)
* **Only locked collateral `C` counts.** `V_dyn` (§5) values `C` and nothing else. Alpha the borrower holds outside lending custody is **never** counted toward LTV, `V_dyn`, interest backing or settlement. That includes other stake on the same or other hotkeys, other coldkeys, the raise's sold `S`, and alpha received after open. There is no cross-margin, and no lien on outside alpha.
  * Rationale: with no liquidations and no recourse (locked), the protocol can't seize outside alpha. Counting alpha that can be unstaked or moved the next block would be an LTV the protocol can't enforce, and keys are sybil-cheap.
* **Escrow `A` is never collateral** (unchanged): it re-adds price-neutrally on default and backs principal, not interest.
* **`A_ahead`** counts other longs' collateral plus `LongInterestPending`, i.e. lending-controlled alpha that can be sold before this `C`. It does **not** count ordinary holders' alpha, the borrower's outside alpha included. Counting all outstanding alpha is Karl's zero-loss stress, rejected in §5.1.
* **Unlocked stake for the raise** (`C + S` check, §12.3) is stake on `(hotkey, coldkey, netuid)` minus lending collateral, minus miner/hotkey locks (`HotkeyLock`, `DecayingHotkeyLock`, miner collateral), minus anything else main treats as locked.
* **At dereg** the borrower's outside alpha is paid by main's 2.2 like any stake, and it never offsets a long's `P − V` shortfall (§13.3.4). On default it isn't touched.
* To borrow more against more alpha, the borrower increases `C` (an increase re-runs `V_dyn` on the whole position).

### 5.4 Directional EMA band on opens (anti trade-splitting)
> **Locked (Jacob):** a new open (including an increase and the raise's buy leg) cannot leave the spot price more than **15%** away from the lending EMA on the **exploitable** side. The check uses **post-trade** spot so one large trade cannot jump the band, and so splitting into many small trades in the same block cannot get past it either (the EMA only updates in `on_finalize`).

* **Longs / buy legs** (`open` long, `open_exact` long, increase long, `open_long_and_sell`'s forced buy): reject with `PriceTooFarFromEma` if `spot_after > EMA · (1 + ema_band_bps/10_000)`.
* **Shorts / sell legs** (`open` short, `open_exact` short, increase short): reject if `spot_after < EMA · (1 − ema_band_bps/10_000)`.
* **Opposite side always allowed.** If spot is already above the long ceiling, shorts may still open (they push price down). If spot is already below the short floor, longs may still open.
* **Never blocks:** `close`, `repay` (partial or full), dissolution settle, fixed-term expiry settle, interest collection, paced long-interest sales.
* **Parameter:** `LiftParams.ema_band_bps`, default **1_500** (15%), root-settable via `set_lift_params`, validated `≤ 10_000` like the other bps fields.
* **Why not a per-trade impact cap.** A size/impact cap per extrinsic can be bypassed by splitting. A band on **price state vs EMA** cannot: after the first fills that walk spot to the edge, further same-side opens fail until the EMA catches up or the other side / ordinary flow mean-reverts spot.
* **Relation to capacity `move_bps` / `ref_tao`.** Those size `Cap` ("a 1k τ buy moves price 10%") and are unchanged. They are not a per-open trade limit.
* **Relation to §5.2 user slippage limits.** `min_proceeds` / `min_escrow_alpha` / `max_payment` remain: they protect the caller against sandwiches. The EMA band is a protocol policy on opens only.
* **#3211 `MinPriceImpactBps`.** Already dropped from this pallet (only dead weight stubs remain). Do not revive it; the EMA band replaces the anti-splitting purpose that a protocol-side per-trade impact dial would have served.

## 6. Interest: 100% APR, burn all, with paced long sales
* **Short TAO interest:** burned immediately (#3211 `burn_interest_tao`).
* **Long alpha interest:** goes to `LongInterestPending[netuid]` and is sold over the next 7 days, one aggregated sale per subnet per block:
  * slice = `pending / blocks_left`, capped at a 1 bps spot move, `Δx_max = x·((1/(1 − 10⁻⁴))^{w_q} − 1)`;
  * slices below 0.01 τ wait;
  * subnets are served round-robin in `on_idle` (#3211 `ConversionCursor`), bounded by `WeightMeter`;
  * **crash guard:** skip while spot < 95% of the 24 h EMA, but sell at the cap anyway once the backlog exceeds 4 weeks of accrual;
  * the TAO received is burned.
* **Effect over time:** pacing removes spikes, not the cumulative pressure.
  * At full long use with no offsetting flows: ≈ **−36% per year on 33%-floor pools** (16.5% of `G` lent long, sold into a 67% pool; was −10% at a 10% floor), **≈ −15.6% per year on thin pools** (7.5% sold into an 85% pool, thin floor on), and up to −56% per year on the ceiling-bound deepest pools. Median subnet −36%/yr.
  * The 1 bps/block cap is ≈ 2,600× the per-block accrual on a 1k τ pool, so it binds only on backlogs.

### 6.1 Burn, not return-until-whole (locked)
* All interest is burned from day one. None is retained to make the pool whole after losses. There is no "return to pool until whole, then burn" mode.
* Rationale:
  * **A burn is hard to add later** (it is a one-way issuance decision). A retention mode can be added later if losses show up.
  * **TAO scarcity now:** at full use the burn removes ≈881 kτ/yr (fair paths) to ≈1,014–2,261 kτ/yr (historical drift) of TAO (§10.3; thin floor on, escrowed longs).
  * **The pools are already overly liquid,** so topping them back up isn't needed.
  * Under v5 escrow, both sides' defaults are quantity-whole, so return-until-whole would have little to restore.

## 7. Below threshold
If `L_side ≥ Cap_side`:
* new opens on that side (including raises, which count against the long side) are blocked. The same holds when `Cap` falls because a pool drops into the thin tier (§2.2) or a new subnet's ramp is lowered (§2.3);
* there is no recall and no forced action;
* repays and defaults re-add normally;
* capacity reopens once `L_side < Cap_side` on `G_ref`.

Approaching deregistration is **not** a threshold event: shorts and longs stay open-able until the dissolving flag (§13.4).

## 8. Invariants and tests
* `lift` then `readd` restores reserves and weights.
* `lift` and `readd` leave price unchanged.
* A short default leaves the pool quantity-whole.
* **A long default leaves the pool quantity-whole** (`readd(0, α_L + A)`). This replaces v4's "loses exactly `P` TAO".
* The `G` invariance holds under short and long lending activity (lift, forced sale, forced buy, repay, default).
* Every alpha loan is sold within the same extrinsic. **Every TAO loan buys within the same extrinsic.** No call path delivers borrowed alpha or borrowed TAO to a user account.
* Escrow `A` and `E_S` leave custody only on close, default or dissolution settlement (§13).
* `V_dyn` is monotone in `A_ahead`; `p_spot` in `V_dyn` is the pre-extrinsic spot.
* **Dissolution (§13.6):**
  * short escrow is never swept: only `min(P·r, E_S + C)` reaches the pot, and nothing from lending custody is recycled;
  * short payout = `max(0, E_S + C − P·r − due)`; long payout = `max(0, r·(A+C) − P − r·due)`;
  * the pot includes the lift legs (`held_tao` → `SubnetTAO`, `held_alpha` → `SubnetAlphaIn`) before 2.2;
  * no new opens once dissolving; **shorts are allowed right up to the dissolving flag**;
  * `r` is frozen once and settlement is idempotent across blocks;
  * `r = min((min(T_flag, t_ema_flag) − Δ0)/D, SubnetMovingPrice)`: a TAO buy in the flag block (batched with `register_network`) leaves `r` unchanged (§13.2);
  * phase-0 basket conversions: `Δ0` = TAO the basket sales took out of `SubnetTAO`, and `r` uses the post-phase-0 pot (§13.2.2);
  * main's 2.1 total excludes lending custody rows;
  * TAO is conserved.
* Raise (§12.5): atomicity, `S` drawn only from unlocked non-collateral stake, the `S = A` round trip ends within 2 fees of the start price, and every failure point rolls back all state.
* **Thin tier (§2.2):** `Cap = 0.15·G_ref` when `G_ref < 3,000 τ`, else the 33% rule; the long LTV switches at the same `G_ref`; a one-block buy that lifts spot `G` above 3,000 τ does not change the tier (because `G_ref = min(G_spot, G_ema)`).
* **New subnets (§2.3):** opens fail with `SubnetTooNew` before `new_subnet_min_age` or before the reference and `G_ema` are warm; `Cap` scales linearly over `new_subnet_ramp`; a reused netuid restarts the clock.
* **Execution limits (§5.2):** every trading extrinsic fails with `DeadlinePassed` after `valid_until`; each forced trade fails with `SlippageExceeded` / `BelowMinimumRefund` when a sandwich moves the price past the user's limit; all state rolls back.
* **Partial repay (§4.4):** after `repay(k)`, every position field equals `(1 − k)×` the old value (to rounding), `L_S`/`L_L`/`B_L` fall by the slice, the pool's reserves change exactly as a `k`-sized close would, accrued interest to the block is charged once (no double charge on a later close), dust remainders are rejected, and a sequence of partial repays summing to `P` leaves the same end state as one `close`.
* **Outside alpha (§5.3):** `V_dyn` is unchanged by any amount of the borrower's unlocked stake; the raise's `C + S` check excludes collateral, escrow and miner/hotkey locks.
* **EMA band (§5.4):** same-side opens that would leave post-trade spot outside EMA±`ema_band_bps` fail with `PriceTooFarFromEma`; opposite side and all exits succeed.
* **Fixed term (§15):** every position carries `expires_at = open + BlocksPerYear`; adds keep it; expiry auto-settles at the reference EMA with bounded `on_idle` weight; dissolution wins if earlier.
* **Custody (§14):** escrow and collateral alpha sit on the lending custody hotkey and earn no emissions; the custody hotkey can't register, set children or take; coldkey swap re-keys positions to the new coldkey and later payouts go there; hotkey swap updates `position.hotkey`; the raise and all lending calls dispatch only under proxy types that already allow staking management.

## 9. Code changes vs main
**swap pallet:**
* add `lift_protocol_liquidity`;
* add `readd_protocol_liquidity`, wrapping `adjust_protocol_liquidity`/`try_update_balancer` and the reservoirs;
* add `buy_depth_threshold(ref, bps) = ref/((1+bps)^{w_b} − 1)` via `SafeInt::pow_ratio_scaled`;
* add `quote_sell_on(y, x, w, Δx)` and `quote_buy_on(y, x, w, Δy)` for hypothetical states.
* Respect `MinimumReserve` and `MAX_SWAP_INPUT_RESERVE_MULTIPLIER`.
* **Injection, chain buys and `get_subnet_terms` are unchanged.**

**New `pallet-lending`** (from #3211 minus the vault):
* Storage:
  * `Positions` (add `lift_tao`, `lift_alpha`; `proceeds` = short escrow TAO; new `escrow_alpha` = long escrow `A`; `custody` = the position's custody coldkey, stored rather than re-derived, so a coldkey swap doesn't move funds, §14), `OpenByNetuid`, `PositionCount`, `TotalPositions`, `LoanHotkeys`, `EscrowOwner`, `References`, `Due`, `NextDue`, `Expiry`, `NextExpiry`, `PalletHotkey`;
  * new: `Lifts: NetUid → {L_S, L_L, B_L, held_tao, held_alpha, escrow_tao, escrow_alpha, long_collateral_alpha, g_ema, t_ema, last}`, `LongInterestPending`, `LiftParams`, `SettleSnapshot: NetUid → {t_flag, t_ema_flag, moving_price}` (§13.2; `Δ0` and `T_settle` are computed at the start of `LendingSettlement`).
  * `t_ema` (§13.2): EMA of the subnet's pot TAO, updated once per block in `on_finalize` for every **live, non-dissolving** subnet, next to `g_ema`. It is frozen once the dissolving flag is set. The migration initialises it to the current `SubnetTAO` on every subnet; a newly registered subnet starts at its `SubnetTAO` at registration.
* `LiftParams` (defaults in brackets): `floor` [3,300 bps], `thin_floor` [**1,500 bps, on**], `ref_tao`, `move_bps`, `ceiling`, `split_short`, `rebalance_target` (off), `rebalance_rate`, `thin_tao`, `ltv_short`, `ltv_long`, `thin_ltv_long`, `apr`, `slice_bps`, `crash_guard`, `settle_ema_blocks` (root-settable EMA window for `t_ema`, default 7,200 blocks = 24 h; changing it keeps the current `t_ema` value), `thin_tao` [3,000 τ], `thin_ltv_long` [1,500 bps], `new_subnet_min_age` [50,400 blocks], `new_subnet_ramp` [216,000 blocks] (§2.3), `ema_band_bps` [**1,500 bps**, §5.4].
* Calls:
  * `open`, `open_exact`, `close`; the short path always sells, and **the long path always buys and escrows** (no TAO payout);
  * **new `repay`** (partial repay, §4.4);
  * every trading call takes `valid_until` and its §5.2 limits (`min_proceeds` / `min_escrow_alpha` on opens, `max_payment` / `min_refund` on close and repay);
  * **new `open_long_and_sell`** (§12);
  * root `set_lift_params`, `set_enabled`.
* Events:
  * `Opened`/`Closed`/`Increased`/`Forfeited` (with lift legs and escrow), `Lifted`, `Readded`, `InterestBurned{side, tao}`, `LongInterestSold{alpha, tao}`, `LiftParamsSet`;
  * new `LongEscrowed{position, tao_in, alpha_escrowed}` and `Raised{…}` (§12.4);
  * new `Settled{position, side, r, paid_to_pot, paid_to_borrower, burned}` and `LendingSettled{netuid, r, positions, to_pot, to_borrowers, burned, shortfall}` (§13.3).
* Errors:
  * `SideCapacity`, `DepthBelowThreshold`, `DynamicCollateral`, plus #3211's `Disabled`, `SubnetUnavailable`, `ReferenceWarmingUp`, `AmountTooSmall`, `InsufficientCollateral`;
  * new `InsufficientSellStake`, `ConflictingSizing` (§12.3), `SlippageExceeded`, `DeadlinePassed` (§5.2; these replace `RaiseSlippage`/`RaiseDeadlinePassed`), `SubnetTooNew` (§2.3), `InsufficientEscrow` (§4.4).

**subtensor:**
* custody and escrow moves (#3211 `lock_alpha_collateral`, `repay_alpha`, `transfer_tao`), plus alpha-escrow custody for longs;
* reserve and `TotalStake` bookkeeping for lifts, forced buys and forced sales;
* the raise's sale leg calls the existing unstake path (`unstake_from_subnet` with `validate_remove_stake` semantics, `allow_partial = false`), so min-stake, subtoken-enabled and staking rate-limit rules are identical to `remove_stake_limit`;
* **dissolution settlement (§13):**
  * `do_dissolve_network` re-adds `Lifts.held_tao`/`held_alpha` after the reservoir purge and before `TotalStake −= SubnetTAO`, freezes interest and `t_ema`, and writes `SettleSnapshot[netuid] = {T_flag, t_ema_flag, SubnetMovingPrice}` (§13.2);
  * `LendingSettlement` reads `T_pre = SubnetTAO` when it starts (after phase 0's basket conversions) and sets `T_settle = min(T_flag, t_ema_flag) − max(0, T_flag − T_pre)` (§13.2.2);
  * lending's `on_idle` (`collect_due`, paced interest sales) skips dissolving netuids;
  * new `DissolveCleanupPhase::LendingSettlement` (appended discriminant, executed between `AlphaInOutStakesGetTotalAlphaValue` and `AlphaInOutStakesSettleStakes`; `lock_refund_pending` = true), with a `LendingSettleStatus` cursor (#3211 `Dissolutions`/`settle_*` pattern minus the vault);
  * main's 2.1/2.2 loops skip lending custody rows via `LendingInterface::is_custody`;
  * lending custody accounts are never the subnet account, so phase 2.7's recycle can't touch `E_S`/`C`;
  * `SubnetUnavailable` on every open/close once dissolving;
* hotkey/coldkey swap hooks (§14: hotkey swap rewrites `position.hotkey`; coldkey swap re-keys `Positions`/`OpenByNetuid`/`EscrowOwner` to the new coldkey and keeps the stored custody account); proxy filters (§14: `repay` joins `open`/`close`; `open_long_and_sell` gets the stricter of `open`'s and `remove_stake`'s filters); custody hotkey guards (§14); new-subnet gate reads `NetworkRegisteredAt` and `SubtokenEnabled` (§2.3); runtime API (`lending_raise_quote`, §12.6, plus `lending_quote` for open/close/repay limits, §5.2).
* **No coinbase change.**

**Reuse from #3211:**
* position, interest and due machinery;
* `References` EMA and warm-up;
* `opening_quote`, `ensure_historical_collateral`, `executable_quote`;
* `burn_interest_tao`;
* `ConversionCursor`/`convert_pending` (reworked for paced long sales);
* custody accounts, dissolution settlement, hooks, benchmarks, SDK/CLI intents and docs (CLI conventions: `btcli lending open|close|list`, `--max-slippage`, review/sign/MEV-shield flow).

**Drop from #3211:**
* the superellipse (curve, migration, `swap_step`/`impls` changes);
* the vault: `Vault*`, `fund_*`, `extract_unreachable_reserves`, `ExtractedReserves`, `MaxFundedSubnets` (**no pre-sized vault allocation in any form**);
* `ensure_borrow_capacity`/`BorrowingLimit`;
* the price-impact dial: `MinPriceImpactBps`, `set_/tune_min_price_impact`;
* `ReservesFunded`, `MinPriceImpactSet`, the vault redemption-basis terms;
* free alpha delivery on shorts, **and free TAO delivery on longs** (#3211's "receive transferable TAO" long);
* #3211's dereg short-debt mark, "higher of frozen historical EMA mark and funded redemption value". It charges shorts a pre-collapse price at dereg and would erase their profit, contradicting §13. It is replaced by `P·r`.
* #3211's routing of principal recoveries to a protocol recovery destination outside the pot. v5.1 pays short debt into the pot before it is computed (§13.3).

## 10. Numbers (floor 33%, thin floor 15% below 3k τ on; v3's 10% floor in brackets where noted)
> **v5.3 status:** all of §10 is computed with the thin floor on, the tier long LTV (15% thin / 25%) and **escrowed** longs. Sources: `lending-pnl/final4.py` → `data/final4.json` (capacity, probes, new subnets, P&L), `lending-pnl/sim_final4.py` (escrowed-long books on the same 117-subnet bootstrap paths and seeds as v5's `sim_final2.py`; the short books reproduce v5's exactly), and `lift-lending-raise/calc.py` → `res.json` (`v53_raise_table`, thin-tier mirrors). Pools: live state at block 9,298,539.

### 10.1 Capacity
* What binds the total: **thin floor 32, floor 81, impact rule 4, ceiling 11** (v3 at a 10% floor: floor 106 / impact 11 / ceiling 11). The 33% floor wins from 3k τ up to `G = y*/(1 − 0.33)` = 30.6k τ.
* Total **826k τ, per side 413k** (834k / 417k with the thin floor off; [654k / 327k] at 10%). Min 36 τ, median 2,178 τ. Most capacity sits in the 15 deep pools, which neither floor touches.
* These are **ceilings on what borrow demand can pull**, not amounts lifted up front (§2.1).

| pool | TAO | tier | per-side cap | 500 τ buy impact before → after full lift | 1,000 τ buy impact before → after | price drop when the short side is fully used (short only / both sides lifted) |
|---|---|---|---|---|---|---|
| SN70 (thinnest) | 237 | thin 15% | 18 [12] | 866 → 1,111% | 2,620 → 3,451% | 14.4 / 15.6% |
| SN47 (~1k) | 993 | thin 15% | 74 [50] | 126 → 154% | 303 → 377% | 14.4 / 15.6% |
| SN119 (~3k) | 3,087 | floor 33% | 509 | 35 → 54% | 75 → 120% | 30.3 / 35.6% |
| SN32 (median) | 6,699 | floor 33% | 1,105 [335] | 15.5 → 23.5% [17.3] | 32 → 50% [36] | 30.3 / 35.6% |
| SN107 (~20k) | 20,576 | floor 33% | 3,395 [1,029] | 4.9 → 7.4% [5.5] | 10.0 → 15.0% [11.1] | 30.3 / 35.6% |
| SN75 (p90, impact-bound) | 34,629 | impact | 7,070 [same] | 2.9 → 4.9% | 5.9 → 10.0% | 36.7 / 44.7% |
| SN64 (deepest, ceiling) | 200,003 | ceiling | 50,001 [same] | 0.5 → 1.0% | 1.0 → 2.0% | 43.8 / 55.6% |

* The short-side drop is the forced sale of the full short book (alpha worth `Cap_short`) into the lifted pool. It is scale-free within a tier: ≈14–16% on thin pools (7.5% per side), ≈30–36% on 33%-floor pools, ≈10% at 10%.
* **Forced-long mirror** (scale-free, fee-free; `calc.py`):
  * a full long book bought into the lifted pool raises price **+43.4%** (long side only lifted) or **+55.3%** (both lifted) on floor pools, and **+16.9% / +18.4%** on thin pools;
  * with both books fully used, the forced buys and sales largely cancel: net −9.1% to +10.0% on floor pools and −1.4% to +1.4% on thin pools, depending on order.
* **Raises are ≈ price-neutral** (§4.3): the spot sale reverses the forced buy in the same extrinsic. Only plain escrowed longs (`btcli lending open --side long`) leave the upward impact.

### 10.2 TAO a team can raise via the atomic raise (§4.3): 25% LTV, 15% on thin pools
* `P` is the escrowed-long principal; the free TAO is the proceeds of the `S = A` spot sale, simulated with fees on the lifted pool (`calc.py`, `v53_raise_table`). **Free TAO = 0.9991–0.9992·P** in every row. The net price move after the extrinsic is ≤ +0.025%. The peak inside the extrinsic, at the full long cap, is +16.9% on thin pools and +43.4% on floor pools.
* The team additionally needs `S` alpha worth ≈ `P` at spot (SN32, `P = 500 τ`: 462 τ of alpha, on top of 2,954 τ of collateral; unchanged).
* Dynamic valuation (`pool'` before the forced buy) caps one long book at 20% of live pool TAO at 25% LTV (`P ≤ 0.25·(y − P)`) and 13.0% at 15% LTV, with unlimited collateral. The thin long cap (7.5% of `G`) is always reachable. On 33%-floor pools the 16.5% cap is reachable, but dynamic valuation binds on collateral: the full cap needs alpha worth **≈3.15× pool TAO** at spot (static 0.66×). On thin pools it needs ≈1.09× (static 0.5×).

| pool | tier LTV | long cap | collateral for the full cap: static / dynamic | `P` with collateral worth 0.5× / 1× / 2× pool TAO | free TAO at 1× | max `P` with the short side full |
|---|---|---|---|---|---|---|
| SN70 | 15% | 17.8 | 119 / 258 | 11.7 / 17.1 / 17.8 | 17.1 | 17.8 |
| SN47 | 15% | 74.5 | 497 / 1,081 | 48.8 / 71.7 / 74.5 | 71.6 | 74.5 |
| SN119 (~3k) | 25% | 509 | 2,038 / 9,723 | 250 / 362 / 461 | 362 | 431 |
| SN32 median | 25% | 1,105 | 4,422 / 21,097 | 542 / 785 / 1,000 | 785 | 934 |
| SN107 | 25% | 3,395 | 13,580 / 64,797 | 1,666 / 2,412 / 3,070 | 2,410 | 2,869 |
| SN75 p90 | 25% | 7,070 | 28,281 / impossible | 2,803 / 4,059 / 5,167 | 4,055 | 4,386 |
| SN64 deepest | 25% | 50,001 | 200,003 / impossible | 16,191 / 23,444 / 29,844 | 23,423 | 22,500 |

* Network long book reachable with unlimited collateral: **363k τ**; 247k τ with the short side full. With the thin floor off it was 367k / 249k.
* **New subnet** (main's injection path with `root_proportion`; seed 1,000 τ; thin floor and tier LTV on; **before** the §2.3 gate and ramp, which zero days 0–7 and scale days 7–37). Long book in τ reachable with unlimited collateral (= the long cap except where noted); in brackets, the raise `P` with collateral worth 1× pool TAO (free TAO is 0.999× of it):

| share | day 0 / 30 / 90 / 180 / 365 / 730 | v3 (10% floor) |
|---|---|---|
| 0.046% | 75 / 79 / 86 / 97 / 120 / 166 (72 … 160) | 50 / 52 / 57 / 65 / 80 / 111 |
| 1% | 75 / 156 / 700 / 1,234 / 2,333 / 4,501 (72 … 3,198) | 50 / 104 / 212 / 374 / 707 / 3,396 |
| 3% | 75 / 700 / 1,769 / 3,373 / 8,084 / 15,968 (72 … 9,359) | 50 / 212 / 536 / 1,022 / 8,084 / 15,968 |

* 0.046% subnets never reach 3k τ in two years, so they stay in the thin tier (75–166 τ). On the 3% path, day 365 is impact-bound and day 730 ceiling-bound, and dynamic valuation binds on both (long cap 9,966 / 19,960; reachable 8,084 / 15,968).

### 10.3 Pool P&L, 1 year, full use, 25% LTV (15% thin longs), 100% APR, burn (burn not counted in P&L), escrowed longs
* Model (`sim_final4.py`): per unit of opening value, an escrowed long that repays leaves the pool `+P` TAO and `−A` alpha, worth `1 − x_T` (`x_T` = price relative to open). A default re-adds `A` and is quantity-whole (0). The borrower repays iff `(R + 1)·x − 1 ≥ 0` (escrow returned on repay). Interest alpha sold into the pool is marked like the v5 free-long model (`+I` alpha, `−SB` TAO). The short book is unchanged from v5.

| paths | floor policy | pool mean / p5 / p1 (kτ) | P(loss) | shorts mean / p5 / p1 | escrowed longs mean / p5 / p1 | TAO burned (kτ/yr) | v5.2 free-long pool (for reference) |
|---|---|---|---|---|---|---|---|
| fair | 10% | −12 / −31 / −37 | 89% | −37 / −100 / −116 | +25 / −31 / −60 | 696 | −50 / −116 / −154 |
| fair | 33%, thin off | −20 / −49 / −88 | 93% | −46 / −112 / −131 | +26 / −33 / −69 | 888 | −62 / −139 / −169 |
| fair | **33%, thin 15% (adopted)** | **−19 / −44 / −87** | 91% | −44 / −109 / −130 | +25 / −35 / −71 | **881** (446 S + 435 L) | −60 / −135 / −168 |
| hist drift | 10% | −27 / −799 / −1,024 | 95% | +127 / +1 / −48 | −153 / −936 / −1,130 | 1,992 (806 conservative) | +703 / +123 / +66 |
| hist drift | 33%, thin off | −208 / −1,113 / −2,233 | 96% | +93 / −32 / −75 | −302 / −1,295 / −2,347 | 2,289 (1,041) | +894 / +84 / +40 |
| hist drift | **33%, thin 15% (adopted)** | **−163 / −1,110 / −1,869** | 96% | +97 / −29 / −71 | −260 / −1,257 / −2,038 | **2,261** (1,014 conservative) | +835 / +89 / +43 |

* **Loan level, escrowed longs** (weighted by long cap, `final4_long_default.csv`): fair paths, 25% LTV: P(default) 0.42%, pool P&L per loan mean +1.0%, p5 −24%, p1 −68% of principal. Historical drift: P(default) 0.53%, mean −22%, p5 −77%, p1 −310%. The v5 free-long model had P(default) 0.45% with loss given default ≈ −110%. That term is gone, since defaults are quantity-whole.
* **What changed.** Free longs lost money only on default. An escrowed long makes the pool the seller of the alpha the borrower bought, and with no liquidation the borrower holds the option: repay when the price has risen, walk away (quantity-whole) when it has fallen far enough. On fair (martingale) paths the escrowed long book comes out roughly break-even to slightly positive (+25 kτ mean, +1.0% per loan), with a fat left tail from rallies (p1 −68% per loan). On historical-drift paths it is a large loss: the sample's upward drift on several subnets, extended by the bootstrap, makes repaid winners cost the pool `x_T − 1` each. The shorts' upside to the pool is capped, because a short borrower walks away from a rally (quantity-whole), while a long borrower always repays one.
* The burn is essentially unchanged (881 vs 881 kτ/yr fair; 2,261 vs 2,262 hist). Under historical drift it exceeds the pool's mean loss about 14× (about 6× on the conservative burn).
* Network numbers move little between floor policies, because the deep pools dominate and neither floor changes them.

### 10.4 Pump-and-borrow
* v4: static spot would let a 2× pool-TAO flash pump borrow ≈ +52% of the long cap free and clear, which at 33% means 3.3× more TAO per floor pool (1.5× per thin pool).
* **v5:** escrow removes the payoff. An over-valued long receives no TAO, only escrowed alpha bought at the pumped price.
* Through the raise, the free TAO comes from selling the attacker's own alpha into their own pump, which is ordinary trading with no lending subsidy.
* Dynamic valuation (min of reference, pre-extrinsic spot, and sale after `A_ahead`) stays mandatory as a second layer.

### 10.5 Thin pools (32 pools under 3k τ): pool P&L per subnet-year, as % of that subnet's pool TAO, fair paths (hist-drift p1 in brackets), escrowed longs

| policy | mean | p5 | p1 | v5.2 free-long mean / p5 / p1 [hist p1] |
|---|---|---|---|---|
| floor 10% | −1.8% | −9.6% | −21.0% [−285%] | −2.9 / −17.2 / −23.6% [−27.1%] |
| floor 33%, thin off | −5.9% | −31.6% | −69.2% [−940%] | −9.6 / −56.8 / −77.9% [−89.5%] |
| **floor 33%, `thin_floor` 15% (adopted)** | **−2.7%** | **−14.4%** | **−31.4%** [−427%] | −4.4 / −25.8 / −35.4% [−40.7%] |

* Per unit of capacity on thin pools: shorts −266 / −375% (p5 / p1, unchanged); escrowed longs at 15% LTV +10% mean, −218 / −702% (p5 / p1). Under the free-long model it was −120 / −150%. The thin tail is now driven by **both** sides. Neither is moved by LTV: an escrowed long's repaid payoff `1 − x_T` doesn't depend on LTV, and short tails never did. The lever is the floor, which is why `thin_floor` = 15% (locked) roughly halves the per-subnet tail vs 33%.
* Under historical drift the thin long tail is extreme (p1 −427% of pool TAO per subnet-year): a handful of thin subnets rallied several-fold in the sample, and the bootstrap extends those rallies.
* **Decision (locked):** `floor = 33%`, `thin_floor = 15%` for `G_ref < 3k τ`, 15% thin long LTV. Network cost: 8k τ of capacity (834k → 826k).

### 10.6 Long-interest price pressure
* Paced sales of long interest at full long use: **≈ −36%/yr spot drift on floor-bound pools** [−10%], ≈ −15.6%/yr on thin pools. Median subnet −36%/yr, deepest −56%/yr.
* Pacing (1 bps/block cap, 7-day spread, crash guard) removes spikes, not the drift. Buys and repayments offset part of it in practice.

**Real vs modelled.**
* Real: main's code, the live reserves and weights, the emission constants, and 118 days of prices.
* Modelled:
  * Balancer impact maths (exact);
  * new-subnet path (seed, constant share);
  * P&L (bootstrap paths, borrowers who repay when it pays, full use, **no price feedback** from forced sales, forced buys or interest sales; at 33% the ~30–36% forced-sale drop and +43% forced-buy rise make this omission material);
  * the attack table (analytic).

## 11. PR #3211 as parts bin
See §9 "Reuse" and "Drop". Nothing from #3211's vault, superellipse or free-delivery paths is kept.

## 12. Atomic raise: `open_long_and_sell` extrinsic and `btcli lending raise`
Implementer notes, CLI UX detail and a test matrix are in `lift-lending-raise-cli.md`.

### 12.1 Names
* **Extrinsic:** `pallet_lending::open_long_and_sell`.
  * It fits the pallet's `open` / `open_exact` / `close` family and says exactly what happens: an escrowed **long** is opened, then alpha is **sold**.
  * Rejected names:
    * `add_stake_borrow_and_sell` is wrong: nothing is staked. The sale leg is an unstake.
    * `borrow_tao_raise` suggests the borrowed TAO is what gets raised, which is the free-long misconception v5 removes.
* **CLI:** `btcli lending raise` (Jacob: "extra command for borrowing TAO on alpha in the CLI which does the full TAO loan atomically"). It sits alongside `btcli lending open|close|list` and inherits their wallet, review, signing, MEV-shield and `--max-slippage` conventions.
* **SDK intent:** `RaiseLoan` (next to `OpenLoan`/`CloseLoan`).

### 12.2 Call signature
```rust
#[pallet::call_index(N)]
pub fn open_long_and_sell(
    origin: OriginFor<T>,          // signed coldkey (position owner, beneficiary of free TAO)
    netuid: NetUid,
    hotkey: T::AccountId,          // hotkey whose stake supplies both collateral and sell alpha
    collateral_alpha: Option<AlphaBalance>, // C. Exactly one of collateral_alpha / borrow_tao must be Some
    borrow_tao: Option<TaoBalance>,         // P (exact principal). If None, P = max allowed by LTV·V_dyn and caps for C
    max_collateral_alpha: AlphaBalance,     // amount mode: bound on C the runtime may solve for (≥ quoted C)
    min_borrow_tao: TaoBalance,             // collateral mode: floor on solved P
    sell_alpha: Option<AlphaBalance>,       // S. None = S equals A (escrowed alpha bought), the price-neutral default
    min_tao_out: TaoBalance,                // floor on free TAO from the sale leg (slippage)
    min_escrow_alpha: AlphaBalance,         // floor on escrow A from the forced buy (§5.2); needed when sell_alpha is Some
    valid_until: BlockNumberFor<T>,         // explicit deadline on top of tx mortality
) -> DispatchResultWithPostInfo
```
* Sizing modes mirror `btcli lending open`: `--amount` (exact `P`, solve `C ≤ max_collateral_alpha`) or `--collateral` (exact `C`, solve `P ≥ min_borrow_tao`). Both `Some` or both `None` → `ConflictingSizing`.
* An "LTV" input is a CLI convenience only. The CLI turns `--ltv x` into `borrow_tao = x · quoted V_dyn`, capped at the tier LTV. The runtime always enforces the tier LTV.

### 12.3 Atomic steps (one extrinsic, `#[transactional]` storage layer; all-or-nothing)
0. **Pre-checks** (no state written):
   * `now ≤ valid_until`, else `DeadlinePassed` (§5.2);
   * lending enabled, subnet live, reference warm (§5);
   * the caller's unlocked stake on `(hotkey, coldkey, netuid)` covers `C + S`. `S` uses `A` quoted on the current state when `sell_alpha = None`. Stake already locked as collateral, escrow, or miner/other locks doesn't count. Else `InsufficientSellStake`/`InsufficientCollateral`.
1. **Lock collateral:** move `C` from the caller's stake to lending custody (`lock_alpha_collateral`).
2. **Open escrowed long:**
   * check side cap and `MinimumReserve`, and `P ≤ LTV_tier·V_dyn` with pre-extrinsic `p_spot` (§5);
   * `lift(netuid, P/y)`;
   * forced buy of `A` with `P`, require `A ≥ min_escrow_alpha` (else `SlippageExceeded`), escrow `A` (§4.2);
   * update `L_L`, `B_L`, position, `Due`/`NextDue`.
3. **Spot sale of own alpha:** `S` (or `A` if `None`) from the caller's **remaining unlocked** stake, through the normal unstake path, to the caller's coldkey. `allow_partial = false`; standard swap fee; same rate-limit bucket as `remove_stake`. Require `tao_out ≥ min_tao_out`, else `SlippageExceeded`.
   * The sale can never draw on `C` (locked in step 1) or on `A` (custody, not the caller's stake). This is the line between a raise and a free long.
4. **Events:** `Lifted`, `LongEscrowed`, `Opened` or `Increased`, then `Raised { owner, hotkey, netuid, position, collateral_alpha, principal_tao, escrow_alpha, sell_alpha, free_tao_out, price_before, price_after }`. The node's standard `StakeRemoved` event for the sale leg is also emitted.

* **Failure rollback.** Any error in steps 0–3 (deadline, caps, `DynamicCollateral`, `MinimumReserve`, rate limit, min-stake dust, `SlippageExceeded`, arithmetic) returns `Err`. The transactional layer reverts **every** storage write: collateral lock, lift, forced buy, escrow, sale, `L_L`/`B_L`, position and `Due` records. The caller ends exactly as before, minus the transaction fee. There is no partial state where a long exists without the sale or the sale happened without the long.
* **Ordering is fixed (buy before sell)** so the sale reverses the forced buy on the same lifted pool. The reverse order would value the sale at an un-pumped price and leave the full forced-buy impact in place.
* **Increase:** if the caller already has a long on `(netuid, coldkey)`, the call increases it. `V_dyn` and the side cap cover the whole position, as for `open` on increase.
* **Weight:** `open_exact` long weight + `remove_stake_limit` weight + bisection bound (collateral mode ≤ 16 steps), benchmarked as one call.

### 12.4 Close
* No special close. `btcli lending close` repays `P` from the wallet and returns `A` + remaining `C`. `--no-repay-from-wallet` sells `A` (then collateral, if needed) to cover `P` and refunds the rest (§4.2).
* A team that wants to unwind the repo buys back with TAO and receives `A`.

### 12.5 Contrast: `open` (pure borrow) vs `open_long_and_sell` (raise)

| | `btcli lending open --side long` / `open` | `btcli lending raise` / `open_long_and_sell` |
|---|---|---|
| Alpha needed | `C` | `C + S` (S ≈ `P/p` by default) |
| Loan TAO `P` | forced buy → escrow `A` | forced buy → escrow `A` (same) |
| Free TAO to caller | **none** | ≈ `0.999·P` when `S = A`, **from the spot sale** |
| Net price impact | up (forced buy) | ≈ 0 when `S = A` |
| Exposure | leveraged long | repo: sold `S` now, right to buy `A` back for `P` |
| Pool on default | quantity-whole | quantity-whole (the sale was an ordinary trade) |

### 12.6 CLI
```bash
# Raise ~500 TAO against alpha on SN32: exact principal, CLI solves collateral, sells S = A (default).
btcli lending raise --netuid 32 --hotkey <hotkey> --amount 500 -w mywallet

# Collateral mode: lock 1,200,000 alpha, borrow the max it supports, sell S = A.
btcli lending raise --netuid 32 --hotkey <hotkey> --collateral 1200000 -w mywallet

# Sell a different amount of own alpha (net price impact ≠ 0; review shows it).
btcli lending raise --netuid 32 --hotkey <hotkey> --amount 500 --sell 150000 -w mywallet

# Tighter slippage and a 20-block deadline.
btcli lending raise --netuid 32 --hotkey <hotkey> --amount 500 --max-slippage 0.5 --deadline-blocks 20 -w mywallet
```
* Flags:
  * `--amount` / `--collateral` (mutually exclusive, as in `open`); `--ltv` (optional, amount-mode helper);
  * `--sell <alpha>` (default: match escrow `A`); `--max-slippage` (default 1%: sets `max_collateral_alpha`, `min_borrow_tao`, `min_tao_out` and `min_escrow_alpha`);
  * `--deadline-blocks` (default 10 → `valid_until`); `--hotkey`; `-w`.
* Flow:
  1. Quote with runtime API `lending_raise_quote(netuid, coldkey, hotkey, amount|collateral, sell?)`. It returns `P, C, A, S, free_tao_out, price_before/after_buy/after, V_dyn, cap headroom, annual_interest`.
  2. Check the caller's unlocked stake ≥ `C + S` and the signer's fee balance.
  3. Show the review: borrowed (escrowed), collateral locked, alpha sold, **free TAO you receive (from the sale)**, annual interest (burned), net price impact.
  4. Sign and submit through the MEV-shield flow.
* The review states plainly: *"The loan's TAO stays escrowed as alpha. The TAO you receive comes from selling S of your own alpha."*
* `--sell 0` is rejected with a pointer to `btcli lending open --side long`.

## 13. Subnet deregistration / dissolution settlement
> **Core requirement (Jacob, locked): shorters must make money on a dereg event.** Shorting a subnet down to deregistration should pay them a reasonable amount. Dissolution must **never** confiscate their short escrow `E_S` or sweep it into the dissolution pot. New shorts are **not** blocked near the dereg threshold; that is intended. Opens are blocked only once the subnet is actually flagged dissolving.

Worked numbers: `lift-lending-dereg/calc.py` → `res.json`. Main's code was read from the local RaoFoundation/subtensor checkout at `6a972693a` (2026-10-16), which descends from the spec target `7ba9601d`. Between the two, `dissolution.rs` differs by one line and `staking/` by 10 lines, none in the pot logic.

### 13.1 How main dissolves a subnet (verified in local main)
* **Trigger.** On a new network registration, `get_network_to_prune` picks the non-immune subnet with the lowest **moving (EMA) alpha price** (ties go to the earliest registration). `do_register_network` then calls `do_dissolve_network`. Root/admin can also call it directly.
* **`do_dissolve_network`** (synchronous, one block):
  * purges the Balancer reservoirs into reserves and `TotalStake`;
  * removes `NetworksAdded[netuid]` (so `if_subnet_exist` = false and swaps stop);
  * runs `TotalStake −= SubnetTAO`;
  * pushes `netuid` to `DissolveCleanupQueue`;
  * emits `NetworkRemoved`.
  * **This is the "dissolving" flag** used below: `!NetworksAdded[netuid] || DissolveCleanupQueue.contains(netuid)`.
* **Metered cleanup in `on_idle`** (`clean_up_data_for_one_dissolved_network`), multi-block, in phases:
  * **0 `SubnetBasketHoldingsToRoot`** (`staking/claim_root.rs::convert_subnet_basket_holdings_to_root`). For every basket fund (validator hotkey) with a holding on the netuid, it removes the fund's alpha from its escrow stake row (`decrease_stake_for_hotkey_and_coldkey_on_subnet`) and **sells it through the subnet's AMM**: `swap_basket_alpha_for_tao_chunks`, fee-free (`drop_fees = true`), `min_price` limit, in reserve-bounded chunks. It moves the TAO out of the subnet account to the **root** subnet account (`transfer_tao_from_subnet`), credits it to the fund's root slot, and records a protocol outflow; `TotalStake` is restored, since `SubnetTAO` already left it at the flag. Net effect on the dissolving pool: **`SubnetTAO` falls by the TAO paid out, `SubnetAlphaIn` rises by the alpha sold, and that alpha leaves the staker rows.** A holding with no executable sale ("terminal garbage" or a terminal-liquidity failure) is **burned** instead (`burn_subnet_alpha`): no TAO moves, and the alpha leaves supply. Metered, multi-block, resumable by `BasketShares` cursor. No user trade can touch the pool now, because swaps stopped when `NetworksAdded` was removed;
  * 1.x root-claim cleanup;
  * **2.1 `AlphaInOutStakesGetTotalAlphaValue`** sums the alpha denominator;
  * **2.2 `AlphaInOutStakesSettleStakes`** pays the pot;
  * 2.3–2.6 clean rows and locks;
  * **2.7 `AlphaInOutStakes`** (legacy owner refund, then **recycle all TAO left on the subnet account**);
  * 3 protocol liquidity; 4–5 purge.
* **The pot and the rate** (`staking/remove_stake.rs`):
  * `pot = SubnetTAO[netuid]` (pool TAO reserve after the reservoir purge), held on the subnet account.
  * `A_main` = Σ share-pool value of every non-retired `(hotkey, coldkey)` alpha row on the netuid, plus `SubnetProtocolAlpha`, plus `SubnetAlphaIn` **only if** `NetworkRegisteredAt > TaoInRefundDeploymentBlock` ("new" subnets; legacy subnets ignore pool alpha).
  * Each **staker row** receives `pot · value / A_main`, rounded by largest remainder, credited to the coldkey's free balance.
  * The shares belonging to `SubnetProtocolAlpha` and (new subnets) `SubnetAlphaIn` are **not paid to anyone**. They stay on the subnet account and go to the legacy owner refund or are recycled in phase 2.7.
  * So main's staker rate is **`r_main = SubnetTAO / A_main`**.
* **Where the shorts' alpha is.** The `P` alpha a forced short borrowed was lifted from `SubnetAlphaIn` and sold straight back into it, so it sits in pool reserves. Main pays pool alpha nothing to stakers: it is recycled on new subnets and left out of the denominator on legacy subnets.

### 13.2 Settlement rate `r`: smoothed pot TAO per alpha, all alpha counted, capped at the EMA price
```
T_flag     = SubnetTAO at the dissolving flag           // in do_dissolve_network, right after the lift re-add (13.3 step 1)
t_ema_flag = t_ema[netuid] at the dissolving flag       // EMA of the same quantity, window settle_ema_blocks (default 7,200 = 24 h); frozen from here on
T_pre      = SubnetTAO when LendingSettlement starts    // after phase 0 (basket → root sales) and 1.x; no user trade can reach the pool after the flag
Δ0         = max(0, T_flag − T_pre)                     // TAO main's phase 0 took out of the pot (13.2.2)
T_settle   = min(T_flag, t_ema_flag) − Δ0               // = min(T_pre, t_ema_flag − Δ0); saturating at 0
D = A_main(excluding lending custody rows)            // phase 2.1 result (unchanged; already after phase 0)
  + (legacy subnet ? SubnetAlphaIn : 0)               // count pool alpha on every subnet (it holds the shorts' sold P)
  + Σ long collateral C + Σ long escrow A + LongInterestPending   // lending-held alpha, counted once, by lending
r = min(T_settle / D, SubnetMovingPrice_at_flag)      // computed once at the start of LendingSettlement, before any lending flow; frozen
```
* **`t_ema`** tracks `SubnetTAO + Lifts.held_tao` (the pot basis after the step-1 re-add, so opening or closing loans doesn't move it). It is updated once per block in `on_finalize`: `t_ema += (T_now − t_ema) / settle_ema_blocks`. A buy inside the flag block is never in it: the dissolve extrinsic runs before that block's `on_finalize`. **It stops updating once the subnet is flagged dissolving.** After the flag, a flag-block pump can no longer be arbitraged out (swaps are closed), so a running EMA would absorb `X / W` of it every cleanup block. New storage initialises to the current `SubnetTAO` (migration: every existing subnet; afterwards: each new subnet at registration). `settle_ema_blocks` is root-settable (`set_lift_params`), default 7,200 blocks (24 h).
* `SubnetMovingPrice` is main's own per-block EMA price (the one prune uses), updated in `block_step`, so it can't move within the flag block either.
* On **new** subnets, `r` is at most main's own rate at the flag (with lending custody alpha counted as stake-like). On **legacy** subnets it also counts pool alpha, so `r ≤ r_main`.
* **Why not main's staker rate on legacy subnets** (the literal "TAO paid per staked alpha"):
  * The short's debt is pool alpha, which main never pays to stakers.
  * As holders dump, the legacy staker float shrinks while pool TAO stays, so `r_main` rises **above the opening spot even after a −90% price fall**.
  * SN32 example (13.5): the legacy staker rate is 17.8× the opening spot. Every short loses 100% of escrow plus collateral. That is exactly the confiscation the core requirement forbids.
  * With "all alpha" `r`, the dereg debt is close to the cost of buying `P` back on-market just before dissolution (0.9–1.2× in the examples), which is the fair benchmark.
* **Non-circular.** `r` is fixed before the shorts pay in. A self-consistent rate that includes their payments (`r = T/(D − ΣP_S)`) was rejected: on legacy subnets it diverges once `ΣP_S` nears the staker float.
* Main's own 2.2 distribution then runs unchanged on the post-lending `SubnetTAO`, using its own `A_main` and legacy/new rule. Stakers' realised rate differs from `r` by the net lending flows (the intended transfer) and by the difference between the live pot and `T_settle`, which main's 2.2 distributes like any other pot TAO.

#### 13.2.1 Why `T_settle` and the EMA-price cap (the `r` pump)
* **The attack.** v5.1 read `r = SubnetTAO / D` live in the dissolve block. When the subnet is the prune candidate, an owner can batch a TAO buy and `register_network` in the same block. The buy inflates `SubnetTAO`, and therefore `r` and every short's debt `P·r`. Where the owner holds nearly all stake and protocol alpha is 0, they get the buy back through the pot and keep the extra debt as profit. SN29 (`sn29-analysis.md` §5, §8 G2′; `sn29/res.json`): each τ injected earns +8.3% from the shorts. **≈6.4k τ erases the shorters' gain, and ≈70.1k τ for one block takes all 5,264 τ of short collateral** (`r` = 6.9× dereg spot, debt 6,363 τ; the owners end with 13,241 τ).
* **The fix.** `T_settle = min(T_flag, t_ema_flag)` (less `Δ0`, 13.2.2): a pump at the flag raises `T_flag` but not the EMA, so the `min` ignores it. To move `t_ema` the TAO has to stay in the pool across block boundaries, exposed to arbitrage, and each block held only moves it by `X / 7,200`. The cap `r ≤ SubnetMovingPrice` bounds the debt at `P ×` the EMA price whatever happens to `T`. Shorts still pay `min(P·r, E_S + C)` and keep the rest; longs' `r·(A + C)` still repays `P` first (13.3).
* **SN29 numbers with the fix** (full 1,316 τ short book, churn route, `sn29/res.json`):
  * EMA price at the flag ≈ 0.0017818 τ/α, so the **shorts' debt is capped at ≈917 τ** (`P` = 514,916 α) and **shorts keep at least ≈181 τ gross** (`E_S` 1,098.5 τ − 917.5 τ), whatever the size of a short-lived pump. Only holding a price pump for hours (the moving price has an 8.1 h half-life) would raise the cap.
  * Unpumped `T` is 6,878 τ, `r` = 0.0011040 (below the cap), debt 568.5 τ. A **70.1k τ pump held for one block moves the 24 h EMA by ≈9.7 τ (0.14% of `T`)**, which raises the debt by ≈0.8 τ. The 6.4k τ pump moves the EMA by ≈0.9 τ and the debt by ≈0.07 τ. In the flag block itself (buy batched with `register_network`), both change by 0.
* **Cost.** Genuine TAO inflows in the last day before the flag are underweighted, since `T_settle` is at most their EMA-weighted share. This lowers `r` slightly, which favours shorts and costs stakers a little. The EMA-price cap also applies to longs (`V = r·(A + C)`), so a capped `r` can turn a marginal long underwater. The `P − V` shortfall stays with stakers, as in 13.3.4.

#### 13.2.2 Phase-0 basket conversions: read the pot after them, and shift the frozen EMA by the same outflow
* **The gap (v5.2).** v5.2 snapshotted `T_flag` in `do_dissolve_network`. Main's phase 0 (13.1) then sells every basket holding on the netuid through the AMM and sends the TAO to root, and only after that does 2.1 count `D`. The sold alpha stays in `D`: it moves from a staker row into `SubnetAlphaIn`, and lending's `D` counts pool alpha on every subnet. The TAO, though, has left `SubnetTAO`. So v5.2's `r = T_flag / D` priced every alpha against TAO that was no longer in the pot.
  * The consequence: shorts overpaid by `P·Δ0/D` each (a transfer from shorters to stakers, eroding "shorters profit on dereg"). Longs were valued `r·(A + C)` against a pot that no longer held it, paid out of stakers' TAO. Main's own 2.2 rate (`T_pre / A_main`) and lending's rate disagreed by exactly the phase-0 outflow.
  * Burned "garbage" holdings are already handled: their alpha leaves supply before 2.1, so `D` shrinks and no TAO moves.
* **Fix (chosen): take the pot after phase 0, keep the EMA frozen at the flag, and subtract the same outflow from both branches of the `min`.** `T_settle = min(T_flag, t_ema_flag) − Δ0`, where `Δ0 = max(0, T_flag − T_pre)` and `T_pre` is `SubnetTAO` read when `LendingSettlement` starts. That is after phase 0 and the root-claim phases (1.x touch no `SubnetTAO`) and after 2.1, and before any lending flow.
  * This is the preferred "snapshot after phase 0" option. The pot term *is* the post-swap pot: `T_flag − Δ0 = T_pre`. The EMA term is the flag-time EMA re-based by the same TAO outflow, so the two branches stay comparable.
* **Why this variant:**
  * **Shorters' profit is preserved, and slightly improved vs v5.2.** `Δ0 ≥ 0` can only lower `r`, so short debt `P·r` falls and the short keeps more of `E_S + C`. The fix removes a v5.2 transfer from shorters; it adds none.
  * **Pump resistance is unchanged.** `Δ0` is set by protocol state only (the basket funds' holdings and the AMM curve). Nothing a user does after the flag can change `T_pre`, because swaps and stake ops stop when `NetworksAdded` is removed and lending opens/closes are refused. Before the flag, a pump changes `T_flag` but not `t_ema_flag`, and the `min` still discards it. The `max(0, ·)` makes `T_settle` one-sided: it can never exceed the flag-time `min`. The `SubnetMovingPrice` cap is untouched.
  * **Consistent with main and with `D`.** `r` is now computed on the same post-phase-0 pot that main's 2.2 distributes, and against the same post-phase-0 alpha. On new subnets, before lending flows, it equals main's own rate. On legacy subnets it differs only by counting pool alpha, as 13.2 intends. Lending flows then move value only by the intended amounts: shorts' `P·r` in, longs' payouts out, burns.
* **Rejected alternatives:**
  * *Snapshot `t_ema` after phase 0 with the EMA still running through cleanup.* After the flag the pumped TAO can't be arbitraged out, so every cleanup block would fold `X / W` of a flag-block pump into `t_ema`. A running EMA would also take ~`W` blocks to reflect `Δ0`, so the EMA branch of the `min` would still ignore the outflow.
  * *Keep `T_flag` and add the phase-0 TAO back into the settlement pot.* This values alpha against TAO that has gone to root, so shorts overpay by `P·Δ0/D` and longs are paid from TAO the pot doesn't have. Worse for "shorters profit" and inconsistent with main's 2.2.
  * *Move lending settlement ahead of phase 0.* That means reordering main's metered phases: phase 0 must finish before stake teardown, and main counts `D` after it. Lending would then settle against a pot and an alpha count that main's 2.2 never distributes.
* **Sizing.** `Δ0` is the TAO the basket funds realise on the dissolving subnet, which is the funds' holdings sold fee-free into the post-re-add pool. On a subnet with no basket holdings, `Δ0 = 0` and v5.2's numbers stand. No live basket-holding figures are quoted here.

### 13.3 Settlement rules: all loans closed inside the dissolution flow, before the staker pot is computed; no liquidations
1. **Re-add every lift leg** (synchronous, O(1), inside `do_dissolve_network`, right after the reservoir purge and **before** `TotalStake −= SubnetTAO`):
   * `Lifts[netuid].held_tao` (shorts' `τ_L`) → `SubnetTAO` plus the subnet account;
   * `held_alpha` (longs' `α_L`) → `SubnetAlphaIn`.
   * These are per-subnet aggregates, so no position iteration is needed. The pot now includes all lent liquidity.
   * Also freeze interest at the cutoff (the dissolve block): uncollected interest up to the cutoff becomes "due"; nothing accrues after.
   * Then snapshot `T_flag` (the post-re-add `SubnetTAO`), `t_ema_flag` and `SubnetMovingPrice` into `SettleSnapshot[netuid]`, and stop updating `t_ema` for this netuid (13.2). Lending's `on_idle` interest collection and paced interest sales skip the netuid from here on.
2. **Fix `r`** (13.2) at the start of the new `LendingSettlement` phase (13.4): read `T_pre = SubnetTAO` (post phase 0), then `Δ0 = max(0, T_flag − T_pre)` and `T_settle = min(T_flag, t_ema_flag) − Δ0` from the `SettleSnapshot` written at the flag, plus `SubnetMovingPrice` and the phase-2.1 `D`. Emit `SettleRateFixed { netuid, t_flag, t_ema_flag, t_pre, delta0, d, r }`.
3. **Shorts:**
   * Debt = `P` alpha valued at `P·r` TAO. Pay `min(P·r, E_S + C)` into the pot, from escrow `E_S` first, then TAO collateral `C`. This is a lending-custody → subnet-account transfer, and `SubnetTAO` increases by the same amount.
   * Then burn due interest (TAO) from what remains. The **borrower receives `max(0, E_S + C − P·r − due)`**.
   * `E_S` was the forced-sale proceeds at the pre-collapse price and `r` is low at dereg, so **the short profits**.
   * If `P·r > E_S + C` (alpha rose sharply), everything goes into the pot, the borrower gets 0, and the shortfall `P·r − E_S − C` is a pool loss. There is no recourse.
   * **The escrow is never swept wholesale and never recycled.** Only `P·r` moves to the pot.
4. **Longs:**
   * Escrow `A` plus collateral `C` (after interest already collected) are valued like stake: `V = r·(A + C)`.
   * From `V`, first **repay `P` TAO** (it stays in the pot), then burn due interest (`r ×` due alpha), then pay the remainder to the borrower. Net pot change: `−max(0, V − P − due·r)` paid out to the borrower, plus the burn.
   * **Underwater (`V < P`): the one underwater case.** The borrower gets 0 and the pot is short `P − V` against full repayment. Stakers receive `P − V` less than if the long had repaid in full.
     * Because v5 longs are escrowed, the lent TAO is already back in `SubnetTAO` from the forced buy, and `A + C`'s share stays in the pot.
     * So stakers are not below their **no-loan** baseline. The shortfall is foregone repayment, not lost principal. (Under v4 free longs it would have been a real loss.)
5. **Pending long interest** (`LongInterestPending`) is converted at `r`: `SubnetTAO −= r·pending`, and that TAO is **burned**.
6. **Ordering and weight:**
   * Bounded iteration over `OpenByNetuid[netuid]` (≤ `MaxPositionsPerSubnet`) with a cursor in `LendingSettleStatus`, multi-block under the `on_idle` `WeightMeter`. This reuses #3211's `Dissolutions`/`settle_*` pattern minus the vault.
   * Main's **2.2 pot distribution starts only after every lending position on the netuid is settled.** The pot is therefore computed once, on the final `SubnetTAO`.
   * Each settlement emits `Settled { position, side, r, paid_to_pot, paid_to_borrower, burned }`. The phase ends with `LendingSettled { netuid, r, positions, to_pot, to_borrowers, burned, shortfall }`.
   * Positions, custody and `Lifts` records are cleared before cleanup completes, so the netuid can't be reused with stale state.

### 13.4 Integration with main's cleanup
* New `DissolveCleanupPhase::LendingSettlement`, **appended** to the enum (stable discriminants, as main does for later phases). It is **executed between 2.1 and 2.2**: `AlphaInOutStakesGetTotalAlphaValue → LendingSettlement → AlphaInOutStakesSettleStakes`. `lock_refund_pending()` returns true for it.
* Main's 2.1 and 2.2 loops must **skip lending custody rows** (short collateral and escrow are TAO and not involved; long collateral `C` and escrow `A` are alpha). Lending counts and pays them itself. Hook: `T::LendingInterface::is_custody(&hot, &cold)`.
* **Lending custody accounts must not be the subnet account.** Phase 2.7 recycles every TAO left on `get_subnet_account_id(netuid)`. `E_S` and `C` must sit in lending-pallet accounts until 13.3 moves exactly `P·r` (and burns) out of them.
* Lending transfers in settlement are **not best-effort**. Main ignores `transfer_tao_from_subnet` errors for stakers; lending must assert each transfer and keep the cursor on failure.
* `TotalStake`: step 1 adds `held_tao` back before main's subtraction. Later lending flows touch only `SubnetTAO` and balances, not `TotalStake`, which main has already removed for this netuid.
* **Opens:** `open`, `open_exact` and `open_long_and_sell` (and increases) fail with `SubnetUnavailable` once the dissolving flag is set. **Nothing earlier blocks them.** Being the prune candidate, being under a price threshold, or being near the bottom of the EMA ranking is not a block. `close` is also refused once dissolving (swaps are gone); settlement is authoritative.

### 13.5 Worked example: SN32, short on part of the short cap, price falls 90% to dereg
* **Setup.** Live SN32: 6,699 τ / 2.597M α, spot 0.0025797 τ/α, staked float 3.633M α. SN32 was registered at block 2,515,294, so it is a legacy subnet for main's pot.
  * The short has opening value **500 τ** (45% of the 1,105 τ per-side cap): `P = 193,824 α`, **`C = 2,000 τ`** (25% LTV).
  * Lift `τ_L = 500 τ`. The forced sale gives **`E_S = 462.5 τ`** (92.5% of spot; spot −14.4% after the sale).
* **Collapse.** Over **30 weeks**, holder emissions (1 α/block) add 1.51M α to the float. Holders dump 5.09M α until spot is **−90%** (0.00025797 τ/α).
  * At dereg: pool 1,993 τ / 7.73M α; staked float 54k α.
  * Interest: 30 weekly charges at 100% APR on 500 τ = **287.7 τ burned** from `C` → `C = 1,712.3 τ`.
  * (On SN32 a −90% fall within 26 weeks is infeasible: the float plus emissions is too small to dump that far.)
* **Settlement** (13.3). The table below was computed under v5.1's `r = T/D` and is not re-run for v5.2/v5.3. It assumes no basket holdings on SN32 (`Δ0 = 0`, 13.2.2); any basket holding would lower `r` and raise the short's payout. Here `T` falls slowly over 30 weeks. If it is flat or falling over the last day, as on this path, then `T_ema ≥ T_flag` and `T_settle = T_flag`, so the `T/D` value is unchanged. The new EMA-price cap can bind, though, because `r` is 1.24× the dereg spot and the moving price tracks spot closely after a slow decline. If it binds, `r` and the debt fall (with `r` at exactly the dereg spot, the debt would be 50.0 τ instead of 62.1 τ), so the short's payout below is a lower bound. The same applies to the other collapse variants.

| | recommended `r` (all alpha) | literal legacy staker rate (rejected) |
|---|---|---|
| pot after lift re-add `T` | 2,493 τ | 2,493 τ |
| `r` (vs opening spot / vs dereg spot) | 0.000320 (0.124× / 1.24×) | 0.0460 (17.8× / 178×) |
| debt `P·r` | **62.1 τ** | 8,910 τ |
| paid to pot | 62.1 τ | 2,174.8 τ (all of `E_S + C`) |
| **payout to shorter** `E_S + C − P·r` | **2,112.7 τ** | 0 |
| gross dereg gain `E_S − P·r` | **400.4 τ** (0.80× opening value; `E_S/(P·r)` = **7.4×**) | −8,448 τ |
| net after 287.7 τ interest (vs 2,000 τ posted) | **+112.7 τ** (+5.6% of `C`, +22.5% of opening value) | −2,000 τ (−100%) |
| benchmark: buy `P` back on-market just before dissolution | 51.3 τ (debt = 1.21×) | (debt = 174×) |

* **Faster collapse** (13 weeks, −80%): `r` = 0.185× opening spot, debt 92.5 τ, gross gain 370 τ, interest 124.7 τ → **net +245.4 τ (+12.3% of `C`, +49% of opening value)**. The literal staker rate gives −90.9 τ.
* **Slow collapse** (52 weeks, −90%): gross gain 407.8 τ, but interest is 498.6 τ → **net −90.8 τ**.
  * At 100% APR on opening value, a short breaks even only if dereg arrives within **≈48 weeks**, even at `r → 0` (`E_S/V` = 0.925 of a year).
  * This is the interest bound working as designed (13.7). It is also the main limit on "shorts make money".
* **Same scenario, a 500 τ raised long** (`A + C` = 1.32M α, interest deductions ignored): `V = r·(A+C)` = 424 τ < `P` → borrower 0, pot keeps 424 τ, **shortfall 75.7 τ vs full repayment**. With 30 weeks of interest deducted (≈112k α), `V` ≈ 388 τ and the shortfall ≈ 112 τ. Stakers are not below the no-loan baseline (13.3.4).
* **Short insolvency threshold** in this setup: `r > (E_S + C)/P` = 0.0127 τ/α = 4.9× the opening spot (with `C` before interest; 4.3× after 30 weeks of interest). A short only loses its collateral if the subnet rallies about 5× before dissolving.

### 13.6 Invariants and tests (also listed in §8)
* **Short escrow is never swept:** across dissolution, the total TAO leaving `E_S + C` = `paid_to_pot + burned + paid_to_borrower`, `paid_to_pot ≤ P·r`, and nothing from lending custody is recycled by phase 2.7.
* **Short payout = `max(0, E_S + C − P·r − due)`** with `r` from 13.2; `paid_to_pot = min(P·r, E_S + C)`.
* **Long payout = `max(0, r·(A + C) − P − r·due)`**; `repaid ≤ P`; the underwater case pays the borrower 0.
* **Pot includes the lift legs:** `SubnetTAO` at the start of 2.2 = pre-dissolve `SubnetTAO` + `held_tao` + Σ short `paid_to_pot` − Σ long payouts − `r·LongInterestPending` − long due-interest burns − `Δ0` (phase-0 basket TAO sent to root; short interest burns come out of lending custody, not the pot). `SubnetAlphaIn` includes `held_alpha`.
* **No new opens once dissolving** (`open`, `open_exact`, `open_long_and_sell`, increases → `SubnetUnavailable`). **Shorts are allowed right up to the dissolving flag:** a test opens a short in the block before `do_dissolve_network` on the prune candidate and must succeed.
* **Pump resistance (13.2):** `r = min((min(T_flag, t_ema_flag) − Δ0)/D, SubnetMovingPrice)` with `T_flag`, `t_ema_flag`, `SubnetMovingPrice` from `SettleSnapshot`. Tests:
  * a batch `buy X TAO; register_network` that prunes the subnet gives the same `r` as without the buy, for any `X`;
  * from a steady state, a buy of `X` held for `k` blocks before the flag moves `t_ema` by `X·(1 − (1 − 1/W)^k)`, with `W = settle_ema_blocks`;
  * `t_ema` is unchanged in every block after the flag (on_finalize skips dissolving netuids), however long cleanup takes;
  * `r ≤ SubnetMovingPrice` at the flag, so every short's `paid_to_pot ≤ P · SubnetMovingPrice`;
  * after the migration, `t_ema = SubnetTAO` on every subnet. A new subnet starts at its registration `SubnetTAO`. `set_lift_params` changing `settle_ema_blocks` keeps `t_ema`.
* **Phase-0 basket conversions (13.2.2):**
  * a dissolving subnet with basket holdings `b` (sold for `τ_b`): `Δ0 = τ_b` (= Σ `BasketHoldingConverted.tao` for the netuid), `T_pre = T_flag − τ_b`, `r = (min(T_flag, t_ema_flag) − τ_b)/D`; each short's `paid_to_pot` is `P·Δ0/D` lower than with v5.2's `r`;
  * on a new subnet (pool alpha in `A_main`), `r` before lending flows equals `T_pre / A_main`, main's own 2.2 rate (lending custody counted as stake-like); stakers' realised rate differs from it only by the lending flows;
  * a write-off (burned holding): `Δ0` unchanged, `D` lower by the burned alpha;
  * combined attack: `buy X; register_network` batched, then phase 0 runs: `r` is identical to the same dissolution without the buy;
  * `Δ0` is never negative, and `T_settle ≤ min(T_flag, t_ema_flag)` always;
  * a dissolution whose phase 0 spans many blocks (tight weight) gives the same `r` as a single-block phase 0.
* `r` is frozen once per netuid and identical in every `Settled` event; settlement is idempotent across blocks (the cursor resumes, with no double pay).
* Main's 2.1 total excludes lending custody rows; 2.2 starts only when `LendingSettleStatus` is done.
* Conservation: TAO in (pot, lending custody) = TAO out (stakers, borrowers, burn, recycle/refund), up to rounding.

### 13.7 Discussion: shorting weak subnets to deregistration is a feature
* It removes low-value subnets faster (prune picks the lowest EMA price) and pays the people who identified them. Dereg must therefore be the short's payday, never a confiscation.
* **Bounds that keep it reasonable:**
  * **50/50 side cap:** the short book is ≤ `Cap_short` (16.5% of `G` on floor-bound pools, 7.5% on thin pools). The most stakers can transfer to shorts at dereg is Σ `E_S` ≤ ≈13.8% of pool TAO at open (`c·(1−c)` with `c` = 16.5%; ≈6.9% on thin pools), minus the `P·r` paid back.
  * **The forced sale moves price as you short:** a 500 τ short on SN32 sells at 7.5% below spot and moves it −14%; a full book moves it −30%. The first shorts get the best `E_S`, and late shorts get little.
  * **100% APR interest, burned:** the short pays its whole opening value per year. A short held ≈48 weeks or more cannot profit even from a total wipeout, so only collapses that happen within months pay.
  * **The owner can't pump `r` at the flag:** `T_settle = min(T_flag, t_ema_flag) − Δ0` and the EMA-price cap make a same-block buy worthless, and the phase-0 basket outflow is netted out instead of being charged to shorts (13.2.2). On SN29's churn route a short book's debt can't exceed ≈917 τ against 1,098.5 τ of escrow unless the pump is held for hours (13.2.1).
  * **Timing isn't in the shorter's hands:** prune happens on the next network registration and picks the lowest EMA price among non-immune subnets. The EMA lags spot, so a quick push doesn't immediately make a subnet the candidate. Triggering a registration costs the registration burn.

## 14. Custody, hotkeys, staking/emissions and proxies
### 14.1 Who holds what

| funds | held as | coldkey | hotkey |
|---|---|---|---|
| short TAO collateral `C` and escrow `E_S` | free TAO balance | the position's custody account `custody = PalletId.sub(hash("position", owner, netuid))`, stored in `Position.custody` | n/a |
| long alpha collateral `C` and escrow `A` | alpha stake on the netuid | the same per-position custody account | the lending custody hotkey `PalletHotkey` |
| lift legs `held_tao` / `held_alpha` | free TAO / alpha stake | `PalletId.sub(("lift", netuid))` | `PalletHotkey` |
| `LongInterestPending` | alpha stake until sold | pallet account | `PalletHotkey` |

* Opening moves collateral alpha from the borrower's `(hotkey, owner)` stake to `(PalletHotkey, custody)` with `transfer_staked_alpha` (no swap, no unstake). Close, repay and settlement return alpha to `(position.hotkey, owner)` and TAO to `owner`. **Payouts only ever go to the position owner's coldkey**, never to a proxy, a delegate or the source hotkey's owner.
* Custody accounts are never the subnet account (phase 2.7 recycles that, §13.4). Main's 2.1/2.2 skip `(PalletHotkey, custody)` rows via `is_custody` (§13.4), and lending counts them once in `D`.
* Custody alpha remains outstanding alpha (`SubnetAlphaOut`/`TotalStake` bookkeeping unchanged by the move).

### 14.2 Staking and emissions
* **Escrow `A`, collateral `C`, lift legs and pending interest earn no emissions and carry no stake weight.** `PalletHotkey` is a keyless pallet sub-account. It is associated with the pallet coldkey (`create_account_if_non_existent`), never registered as a neuron on any subnet, never a delegate, sets no weights, children or take, and has no root stake. Stake on it receives no dividends or incentive.
  * Rationale for `A`: it was bought with the pool's lifted TAO. Letting it earn dividends or count as validator stake would let a borrower rent consensus weight and emissions with pool liquidity.
  * Rationale for `C`: one custody path for all lending alpha keeps settlement, `is_custody` and swap hooks simple. The cost: a borrower forgoes `C`'s dividends while it is locked (on top of the 100% APR).
  * **Jacob's call (optional alternative):** lock `C` in place on the borrower's own hotkey (main's lock machinery, like miner collateral), so it keeps earning and keeps its stake weight while unmovable. `A` would still sit in custody. Cost: a second custody path in close/repay/settlement and in the 2.1 skip logic. Default in this spec: custody, no emissions.
* Guards: the custody hotkey can't be the target of `register`/`burned_register`, `become_delegate`, `set_children`, `set_childkey_take` or `swap_hotkey` (they all fail on ownership already; add explicit tests). Third-party `add_stake` onto `PalletHotkey` is rejected with `CustodyHotkey`. It would be harmless (rows under other coldkeys are not custody) but confusing in explorers and the 2.1 skip check.
* Interest on long collateral is taken from custody stake and sold on the paced schedule (§6). Escrow `A` is never sold for interest.

### 14.3 Hotkey and coldkey swaps
* **Hotkey swap** of the borrower's source hotkey: `on_hotkey_swap` rewrites `position.hotkey` (and `LoanHotkeys`), so returns go to the new hotkey. Custody rows don't move, because they are on `PalletHotkey`.
* **Coldkey swap** of the owner: re-key `Positions`, `OpenByNetuid`, `EscrowOwner` and the `Due` index from the old to the new coldkey, and keep `Position.custody` as stored, so no funds move. Every later close, repay or settlement payout goes to the new coldkey. A coldkey swap is how a user escapes a compromised key, so leaving positions on the old key would send dereg payouts to the attacker.
* Neither swap is allowed to change `Position.custody`, `PalletHotkey` or the lift accounts.

### 14.4 Proxies and origins
* All position calls (`open`, `open_exact`, `repay`, `close`, `open_long_and_sell`) need a signed origin: the owner coldkey, or a proxy acting for it. Root setters (`set_lift_params`, `set_enabled`) are root-only.
* Proxy filter groups:
  * `LendingCalls` = `open`, `open_exact`, `close`, **`repay`**. Allowed under `Any`, `Staking` and `NonTransfer`: these calls move the owner's stake and return it only to the owner, like `add_stake`/`remove_stake`.
  * `LendingRaiseCalls` = `open_long_and_sell`. Allowed only under types that allow **both** `LendingCalls` and stake removal (`Any`, `Staking`, `NonTransfer`), because its sale leg is a `remove_stake` to the owner's free balance.
  * Every other proxy type is denied, `NonCritical` and `NonFungible` included. `NonCritical` grants stake management but not lending; that matches PR #3268's filters.
* A proxy can never redirect funds: there is no `to`/`beneficiary` parameter on any lending call.

## 15. One-year fixed term and auto-settle at the smoothed price
> **Locked (Jacob, 2026-10-20):** every lending position — long and short — expires **one year after it is opened** and is then settled automatically at the **smoothed (EMA / lending `References`) price**, with the same settlement math the pallet already uses for smoothed settles (§13.3), and **with no liquidations**.

### 15.1 Term length
* Term = `BlocksPerYear` (runtime constant; ≈ 2,628,000 blocks at 12 s ≈ one year).
* On a **new** open (not an increase), set `Position.expires_at = now + BlocksPerYear` and enqueue `(owner, netuid)` in the expiry schedule at that block.
* Interest schedule (`due` / `Due*`) is unchanged and independent of the term.

### 15.2 What happens on expiry
* In `on_idle`, after interest collection, the pallet walks the expiry schedule with a `WeightMeter` (same bounded / resumable pattern as `Due` and dissolution settle).
* For each due entry whose subnet is **not** dissolving:
  1. Accrue and charge interest through `now` (final charge).
  2. Take settlement rate `r = References[netuid].price` (the dedicated lending EMA). If the reference is missing, fall back to the live spot price.
  3. Settle with the §13.3 smoothed formulas (no AMM buyback / no liquidation):
     * **Short:** debt = `⌈P · r⌉` TAO; pay `min(debt, E_S + C)` into the pool/reserve; borrower keeps the remainder; re-add recovered TAO; clear the position.
     * **Long:** `V = ⌊(A + C) · r⌋`; repay `P` first from `V`; pay remainder to the borrower from the pool; destroy custody alpha; clear the position (underwater shortfall stays with the pool, as in §13.3.4).
  4. Emit `Expired { owner, netuid, side, principal_tao_equiv, tao_to_pool, tao_refund, rate }`.
* Weight is bounded per block. A cursor (`ExpiryBucketCursor`) resumes mid-bucket. Many expiries in one block may span several blocks of `on_idle`.

### 15.3 Top-ups, partial repay, and dissolution
* **Add / increase** on an existing position **keeps the original `expires_at`**. Topping up does not restart the one-year clock (chosen rule; Jacob allowed either, implementer locked "keep original").
* **Partial repay** (§4.4) leaves `expires_at` unchanged. Full repay / `close` removes the position and its expiry queue entry.
* **Dissolution before expiry:** if the subnet is flagged dissolving while the position is still open, §13 settlement is authoritative. Dissolution settle clears the position (and therefore the expiry index). Expiry processing **skips** dissolving netuids, so it never double-settles.

### 15.4 Unchanged economics
* LTV 25% / 15% thin, 100% APR burn-all interest, escrowed longs and shorts, pro-rata partial repay, custody earns no emissions (§14), dissolution `r` protections (§13.2), new-subnet gate and capacity ramp (§2.3) are all unchanged by the term.

### 15.5 Tests
* Expiry auto-settles a short at the smoothed reference price (not spot) and emits `Expired`.
* Many expiries in one bucket: a tight `WeightMeter` settles some but not all; a later full-weight pass finishes the rest.
* An add / increase after open does not extend `expires_at`; partial repay likewise.
* Dissolution before `expires_at` settles via §13 and leaves no `Expired` event when the old expiry block arrives.
