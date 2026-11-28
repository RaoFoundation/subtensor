# Lending v2 design: synthetic shorts and pool-lifted longs

Implements `SPEC.md` v2.0.1 (this directory). The spec is normative; this document fixes the code shape, the types and the
order of work. Where the two disagree, the spec wins and this file is a bug. Deviations from the spec's storage sketch
are listed in section 10 for review.

Base: `origin/main` at `24e1fc7b1` (tag `v497`, the runtime finney runs today, spec_version 497). Branch:
`lending-v2-synthetic`. `main` has no lending pallet, so this is a new pallet, not a migration of
`cursor/lift-lending-escrowed-raise-352f` (the old PR #3268 branch). Code is taken from the old branch only where noted in
section 9.

## 1. Principles

1. **One position type, one state machine, two sides.** Shorts and longs share `Position`, the lifecycle
   (open → increase/close/top-up/withdraw → deleverage ↔ open → closed | defaulted | settled), the interest queue,
   the deleverage walker, the setters and the try-state checks. Side-specific behaviour lives in one `match` per
   concept, inside the pure math, never spread across the extrinsic layer.
2. **Pure core, thin shell.** Everything numeric (curve quotes, coverage, health, exit, interest, pacing, the dereg
   rate, the zone split, the long-book prune split) is a pure function of plain values in `src/math/`. No storage,
   no `Config`, no `frame_support`. These functions are what the differential tests drive against the Python reference
   model (`model/v201`), scenario for scenario, rao for rao.
3. **Read once, decide, write once.** Each extrinsic and hook builds a `Market` snapshot (pool, price refs, book
   totals, params), asks the pure core for a decision (`OpenPlan`, `HealthVerdict`, `FillPlan`, `Settlement`), then
   applies it with the adapter in one storage transaction. No function both reads storage and does math.
4. **Rounding always favours the pool.** Costs and needs round up, values and payouts round down, remainders carry.
   Every rounding site names its direction in the code (`ceil_div`, `floor_div`), the same as the model.
5. **No dead code.** Nothing from removed spec versions (P21 breaker, P24 halving guard, EMA price refs, SMA-pause lapse,
   term/expiry/roll, lifts/readd, paced phase A) is carried, even behind a flag.

## 2. Crate layout

```
pallets/lending/
  Cargo.toml
  docs/SPEC.md, docs/DESIGN-v2.md    # this pair
  model/                             # reference model v201 + golden-vector generator (Python, stdlib only)
  runtime-api/                       # LendingApi: quotes, health, params (read-only)
  src/
    lib.rs                           # #[frame_support::pallet]: Config, storage, events, errors, calls, hooks
    types.rs                         # Side, PositionId, Position, Leg, PositionState, Market, PriceRefs
    params.rs                        # LendingParams / ShortParams / LongParams / DeregParams, bounds, Pending apply
    math/
      mod.rs
      fixed.rs                       # ceil_div / floor_div / mul_div on u128, U64F64 helpers, Bps newtype
      curve.rs                       # PoolState + Curve trait; ConstantProduct (tests) ; quotes, at_price
      coverage.rs                    # stress_eff, short_need, long_need (opener, whole book pro rata)
      health.rs                      # book_share, health cost/value, equity, trigger and exit tests
      interest.rs                    # accrual with remainder, day_interest, cover
      pace.rs                        # per-block up/down budgets, pause bands, pro-rata fill split
      dereg.rs                       # r, zone split, long-book prune split + lender-first settlement
      ring.rs                        # median of a ring, SMA of daily means, ring resize/seed
    prices.rs                        # TwapRing / RankRing storage, sampling, prefill, PriceRefs reader
    positions.rs                     # open / increase / close / add / withdraw for both sides
    interest.rs                      # Due queue: schedule, charge, retry
    deleverage.rs                    # trigger/exit transitions, DeleverageQueue, per-block walker
    dissolution.rs                   # phases A (atomic), B, D, E, F; cursor and meter
    hooks.rs                         # LendingInterface impl for pallet_subtensor (custody guards, coldkey swap, prune)
    try_state.rs                     # I1-I27 (each check is one named fn)
    migrations.rs                    # v0 -> v1: custody hotkey, params defaults, rank-ring prefill (I25)
    benchmarking.rs, weights.rs
    tests/ mock.rs, math_*.rs, positions.rs, interest.rs, deleverage.rs, dissolution.rs, params.rs,
           prices.rs, try_state.rs, differential.rs
pallets/subtensor/src/staking/lending.rs   # LendingPoolInterface impl (the only code that moves pool balances)
```

Dependency direction: `math` depends on nothing in the pallet; `types`/`params` depend on `math`; the storage modules
depend on all three; `lib.rs` only wires calls to `positions`/`deleverage`/`params`. `pallet_lending` does not depend
on `pallet_subtensor`; subtensor implements `LendingPoolInterface` and consumes `LendingInterface` (both defined in
`pallet_lending`), as on the old branch.

## 3. Core types (`types.rs`)

```rust
#[derive(Copy, Clone, Encode, Decode, MaxEncodedLen, TypeInfo, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Side { Short, Long }

/// Spec 7.4: processing order and storage key. SCALE order = (owner, netuid, side).
pub type PositionId<AccountId> = (AccountId, NetUid, Side);

/// The side-specific ledger. Shorts owe alpha and hold TAO; longs owe TAO and hold alpha.
pub enum Leg {
    Short { minted: u64, proceeds: u64, zone_minted: u64, zone_proceeds: u64 },  // q, P, P16 ledger
    Long  { locked_alpha: u64, debt: u64 },                                       // A, D
}

pub struct Position<AccountId, BlockNumber> {
    pub custody: AccountId,            // per-position custody coldkey (old-branch derivation, section 9)
    pub leg: Leg,
    pub collateral: u64,               // C, TAO rao
    pub deposit: u64,                  // P19
    pub opened_at: BlockNumber,
    pub interest: Accrual<BlockNumber>,// last_accrued, remainder (u128 fractional rao), uncharged
    pub state: PositionState<BlockNumber>,
}
impl Position { pub fn side(&self) -> Side; /* derived from leg, never stored twice */ }

pub enum PositionState<BlockNumber> { Open, Deleveraging { since: BlockNumber } }

/// Built once per call. Everything the pure core needs; nothing it doesn't.
pub struct Market {
    pub pool: PoolState,               // live SubnetTAO, SubnetAlphaIn, quote weight
    pub refs: PriceRefs,               // spot, twap_median, sma_rank (U64F64)
    pub short_book: u64,               // SyntheticOutstanding (Open + Deleveraging)
    pub long_book: u64,                // LongLockedAlpha
    pub long_lifted: u64,              // LongLiftedTao
    pub dissolving: bool,
}
```

Notes:
* Closed, defaulted and settled positions are removed from storage, so they are not states.
* `Position` carries no `side` field and no `terms_version`: the side is the `Leg` variant, and params are global and
  read live (spec 2.1: coverage is not re-checked when params change, so there is nothing per-position to version).
* `Bps(u32)` and `Rao(u64)` newtypes are used in `params` and `math` signatures so that a bps value can never be passed
  where an amount is expected. Storage uses the plain integers.

## 4. Pure core (`math/`)

All functions are `#[must_use]`, total (no panics; overflow returns `None` or saturates where the spec says "infinite"),
and documented with the spec item they implement.

```rust
// curve.rs
pub struct PoolState { pub tao: u64, pub alpha: u64, pub quote_weight: Perquintill }
pub trait Curve {
    fn spot(p: &PoolState) -> U64F64;
    fn buy_cost(p: &PoolState, alpha_out: u64) -> Option<u64>;      // fee-free, ceil; None if alpha_out >= p.alpha
    fn sell_proceeds(p: &PoolState, alpha_in: u64) -> u64;           // fee-free, floor
    fn at_price(p: &PoolState, price: U64F64) -> PoolState;          // the hypothetical pump/dump (same invariant)
}
pub struct ConstantProduct;   // 50/50, exact integer math identical to model/v201/pool.py (tests, differential)
// The runtime uses `BalancerCurve`, defined in pallet_subtensor's adapter over pallet_swap::Balancer (pure methods).
// A test pins BalancerCurve at weight 1/2 to ConstantProduct within 1 rao on the golden pools.

// coverage.rs (spec 1.1, P2, 7.1.3)
pub fn stress_eff(stress: Bps, slope: Bps, book: u64, pool_alpha: u64) -> U64F64;
pub fn short_need<C: Curve>(m: &Market, q_pos: u64, book_after: u64, sp: &ShortParams) -> Option<u64>;
pub fn long_need<C: Curve>(m: &Market, d_pos: u64, a_pos: u64, book_after: u64, lp: &LongParams) -> u64;

// health.rs (spec 1.4, 7.1.5, v2.0.1 exit)
pub fn book_share_cost<C: Curve>(p: &PoolState, book: u64, q_pos: u64) -> Option<u64>;  // ceil
pub fn book_share_value<C: Curve>(p: &PoolState, book: u64, a_pos: u64) -> u64;         // floor
pub struct HealthVerdict { pub equity_twap: i128, pub at_risk: u64, pub cover_need: u64,
                           pub triggered: bool, pub may_exit: bool }
pub fn assess<C: Curve>(m: &Market, pos: &Leg, collateral: u64, lp: &LendingParams) -> HealthVerdict;
//   at_risk = H_pos (short, TWAP-scaled) or D (long); triggered = equity < floor || C < cover;
//   may_exit = equity >= floor + h*at_risk && C >= (1+h)*cover. Invariant: may_exit => !triggered.

// interest.rs (spec 1.2, P15)
pub fn notional(leg: &Leg, refs: &PriceRefs) -> u64;               // q*TWAP_median (short) | D (long)
pub fn accrue(notional: u64, apr: Bps, blocks: u64, remainder: u128) -> (u64 /*owed*/, u128 /*rem*/);
pub fn interest_for_blocks(notional: u64, apr: Bps, blocks: u64) -> u64;   // ceil; used for cover and buffer

// pace.rs (spec P7, P17/P22, 7.1.6)
pub fn short_buys_allowed(refs: &PriceRefs, lp: &LendingParams) -> bool;   // spot <= min(TWAP*b1, SMA*b2)
pub fn long_sells_allowed(refs: &PriceRefs, lp: &LendingParams) -> bool;   // spot >= max(TWAP/b1, SMA/b2)
pub fn up_budget<C: Curve>(p: &PoolState, pace: Bps) -> u64;               // alpha buyable for <= pace impact
pub fn down_budget<C: Curve>(p: &PoolState, pace: Bps) -> u64;             // alpha sellable for <= pace impact
pub fn pro_rata(total: u64, weights: &[u64]) -> Vec<u64>;                  // floor, dust reported separately

// dereg.rs (spec 1.8, P12, P16, 7.4)
pub fn rate(t_flag: u64, t_ema: u64, t_a: u64, all_alpha: u128) -> U64F64;
pub fn short_owed(leg: &Leg, r: U64F64) -> u64;                            // (q - zone)*r + zone_proceeds
pub struct LongSettlement { pub proceeds: u64, pub repay: u64, pub collateral_used: u64,
                            pub refund: u64, pub shortfall: u64 }
pub fn split_long_book(p_book: u64, longs: &[(u64 /*a*/, u64 /*claim*/, u64 /*C*/)]) -> (Vec<LongSettlement>, u64 /*dust*/);

// ring.rs (spec P6, P13, I25)
pub fn median_lower(samples: &[u64]) -> u64;
pub fn sma(daily_means: &[U64F64]) -> U64F64;
```

## 5. Storage-facing modules

**`params.rs`.** Four `#[freeze_struct]` structs with `Default` = launch values and `fn validate(&self, ctx) -> Result<(), ParamsError>`
(`ctx` carries cross-checks such as the current max `LongPositionCount`). Storage: `LendingParamsStore`, `ShortParamsStore`,
`LongParamsStore`, `DeregParamsStore` plus `Pending…` twins. Setters (`ensure_root`) validate and write the pending value;
`on_initialize` applies all pending values first, emits `<Group>ParamsSet{old, new}`, and triggers ring re-seeds for TWAP or
rank-window changes (spec 2.1). One generic `apply_pending::<G: ParamGroup>()` covers all four groups.

**`prices.rs`.** `TwapRing[netuid]` (bounded 72, cursor, last sample block), `RankRing[netuid]` (30 daily means + hourly
accumulator), `RankPrice`, `RankGeneration`, `RankRingGeneration`, `PoolTaoEma` (T_ema for r only). Sampling runs in
`on_initialize` at `twap_interval_blocks` and hourly for the rank ring, for every live subnet whether lending is enabled or
not. `PriceRefs::read(netuid)` is the only reader; a missing ring is an error (I25), never a fallback. Seeding:
upgrade migration and `set_enabled(false→true)` copy `SubnetMovingPrice` (the single permitted EMA read, flagged in 8.7);
window changes re-seed from the published value; registration seeds at first price.

**`positions.rs`.** `do_open(side, who, netuid, OpenArgs)`, `do_increase`, `do_close`, `do_add_collateral`,
`do_withdraw`. Each: gate checks (enabled, depth P18, dissolving, P8/L8 per-block net limit, min notional, 21M cap for
shorts, lift cap and longs cap for longs) → `Market` → pure plan → adapter calls → ledger writes → event. Gates are one
`ensure_can_grow(side, &market, delta)` so both sides share the order and the errors.

**`interest.rs`.** Reuses the old branch's `Due` queue design: `Due[(block, PositionId)]`, `NextDue`, bounded charge per
block with `InterestRetry` on failure. Charge = accrue to now, take from C, credit SubnetTAO via the adapter (pool-internal,
no flow or volume, I19), then run the health check (trigger or exit).

**`deleverage.rs`.** `fn check(id, &mut pos, &market) -> Transition` is the single place where `Open ↔ Deleveraging`
changes (trigger and v2.0.1 exit). Called from `poke`, the interest charge, `add_collateral`, and the walker before each
fill. The walker runs in `on_initialize` per subnet with a queued position: compute the side's budget (`up_budget` /
`down_budget`, skipped while paused), split it pro rata over that subnet's deleveraging positions of that side, buy-and-burn
(shorts) or sell-and-repay (longs), default exhausted positions (residual alpha / pool shortfall). Bounded by
`MaxDeleveragingPerBlock` with a cursor; benchmarked per fill.

**`dissolution.rs`.** `start_dissolution(netuid)`: phase A atomically (accrue all longs, one sell of the whole long book,
`split_long_book`, lender-first transfers, events in ascending `PositionId`), then snapshot `t_a` and create the
`Dissolutions[netuid]` cursor. `settle(netuid, &mut WeightMeter)` resumes B → D → E → F. Weight of phase A is charged up
front by subtensor's prune paths via `start_dissolution_weight(max_long_positions_per_subnet)`.

**`hooks.rs`.** `LendingInterface` for subtensor: custody guards (`is_lending_custody`, `LockedAlpha` on every stake path
that touches long custody), `on_coldkey_swap` re-key (all side-aware maps), dissolution entry points, `lending_alpha(netuid)`
for `legacy_owner_cut_accrued`, rank-price reads for `get_network_to_prune`.

**Adapter (`pallets/subtensor/src/staking/lending.rs`).** The only code that moves pool balances:
`mint_and_sell`, `buy_and_burn` (shorts); `lift_and_buy`, `sell_and_repay`, `sell_long_book` (longs);
`credit_pool_tao` (interest); custody transfers. Pool-internal legs never call `record_tao_inflow/outflow`,
`record_protocol_inflow` or touch `SubnetVolume` (I18, I19). Also `BalancerCurve`.

## 6. Extrinsics

Thin wrappers; call indices fixed from 0. User calls keep the spec's names because their arguments differ by side
(`min_tao_out` vs `min_alpha_out`, mint vs lift):
`open_short, increase_short, close_short, add_collateral` and
`open_long, increase_long, close_long, add_long_collateral`.
Root: `set_enabled, set_longs_enabled, set_lending_params, set_short_params, set_long_params, set_dereg_params`.
`ProxyType::Lending` (next free index on main, appended) allows exactly the eight user calls and Utility batches of them.

## 7. Invariants, tests and differential checks

* `try_state.rs`: one function per invariant (I1-I27 as numbered in the spec, removed IDs skipped), run in
  `do_try_state` and after every mock test step via a test helper.
* **Unit tests** per module, following spec section 6, including the v2.0.1 exit edges (1.04× stays, 1.05× exits).
* **Differential tests.** `model/gen_golden.py` runs the model's pure functions and short scenario scripts (squeeze,
  rally, cascade, split book, prune with longs) and writes `src/tests/golden/*.json`. `tests/differential.rs` replays the
  same inputs through `math::*` (with `ConstantProduct`) and, for scenarios, through the mock runtime, and asserts equality
  in rao for amounts and equality of trigger/exit/default blocks. The JSON is committed so `cargo test` needs no Python;
  CI can optionally regenerate and diff.
* Property tests as seeded loops over `rand_chacha` (already a workspace dependency; no new crates):
  split invariance (I23), permutation invariance of phase A (I24), solvency of r (I8, I17).
* Benchmarks for every call and for the per-block walkers; `weights.rs` generated, with a CI check that benchmarks compile.
* Mainnet-clone e2e (spec section 6, items 1-40) runs after the pallet is complete, on a fresh clone.

## 8. Upgrade, runtime and client scope

* Runtime: add the pallet (lending disabled, longs disabled), `type Lending` on subtensor, proxy filter, runtime API,
  `spec_version` bump. One migration: create the custody hotkey, write default params, prefill every live subnet's rank
  ring and TWAP ring from current values (I25), weight-charged.
* Subtensor touchpoints (spec section 3 "Rewrite"): `get_alpha_issuance` counts synthetic and residual alpha (delete any
  schedule-specific variant), P23 check on short mints, `legacy_owner_cut_accrued` subtracts `lending_alpha`,
  `get_network_to_prune` uses the rank price, prune paths call `start_dissolution` and charge its weight, stake paths
  respect custody and `LockedAlpha`, coldkey swap calls the hook.
* SDK and btcli (last, after the pallet is stable): regenerate `_generated/*`, add `intents/lending.py`,
  `reads/lending.py`, `cli/commands/lending.py` for the twelve calls and the runtime API quotes; error descriptions;
  website catalogs regenerate.

## 9. Reuse from the old branch

Taken (rewritten to the types above, with their tests): custody account derivation and the pallet hotkey
(`position_account`, `initialize_custody`), the `LendingPoolInterface` / `LendingInterface` trait split, the mint/burn
adapter core in `staking/lending.rs`, the `Due` queue and retry, the dissolution cursor and meter, the coldkey-swap
re-key, `ProxyType::Lending` and its filter, the rank-ring module (`dereg.rs`) minus its EMA fallback.
Not taken: everything listed under the spec's section 3 "Delete", the P21 breaker, `NotionalEma`, `PruneFloor` caching
unless the benchmark shows it is needed, `StrandedCustody` sweeping (replaced by refusing to leave custody non-empty),
and all release-note or website changes.

## 10. Decisions for review (deviations from the spec's storage sketch)

1. **One `Positions` map** keyed by `PositionId = (AccountId, NetUid, Side)` holding `Position { leg: Leg, .. }`,
   instead of `Positions` (short) plus `LongPositions`. Same keys the spec already prescribes for every side-aware map
   (7.5); one map means one coldkey-swap path, one try-state walk and one runtime-API reader.
2. **No legacy migration.** The spec's `Side::Short` translation migration (7.5) applies to the old branch's state;
   `main` has no lending storage, so the only migration is the v1 initialisation in section 8.
3. **`Curve` abstraction.** The spec and model use 50/50 constant-product math. Finney pools are Balancer pools with a
   quote weight; at weight 1/2 they are constant product. The pure core is generic over `Curve`, the runtime uses
   Balancer math, and tests pin the two together at 1/2.
4. **Exit cover term carries the margin too** (`C ≥ 1.05 days` of interest, not `≥ 1 day`), so a top-up to exactly the
   trigger cover doesn't flap at the next charge. Spec v2.0.1 is written this way; flagged in case a plain `≥ 1 day` is
   preferred.
5. **Deleverage runs in `on_initialize`**, not `on_idle`, with a bounded, benchmarked step count and a cursor. `on_idle`
   can starve under full blocks, which would stall pacing exactly when it matters.
