//! On-chain standardized beta pricing: index-spliced display prices, staker yield, and
//! total-return stake prices, one source of truth for every consumer (SDK, explorers,
//! EVM).
//!
//! Raw beta prices (`NAV / shares`) carry arbitrary historical baselines: a fund
//! seeded when pools were cheap shows a high price forever, and a fund launched
//! yesterday starts at 1.0 regardless of skill. Two common index lines fix that:
//!
//! * The **bag index** is the NAV-weighted mean of every fund's display price
//!   (`raw / price_divisor`). It moves with aggregate fund performance (mix skill),
//!   not with flows.
//! * The **stake index** is the same weighted mean of every fund's total-return stake
//!   price: the wealth of τ1 of root stake earning the average fund's dividends. Bag
//!   prices only enter through the value of accrued β, so this moves with dividend
//!   flow, not with mix.
//!
//! Each fund's [`BetaBaselineOf`] is stamped once at its first share mint (see
//! [`Pallet::stamp_beta_baseline_if_new`]) so both spliced prices start at the index
//! levels of that block. What a level means, for every fund of every age: the wealth
//! of τ1 invested in the average fund at the index epoch, switched into this fund at
//! its birth. Above the index line = beating the market.
//!
//! ## Two marks: spot for live display, realizable for permanent stamps
//!
//! Live levels and prices mark at **spot** (zero-size mark), matching the SDK's
//! display layer: realizable NAV — a full-fund dump — punishes large books for pool
//! impact a normal buyer never pays. Spot is cheap to manipulate on a thin pool, but
//! a manipulated *live* number heals the moment the pool does.
//!
//! A **baseline stamp is permanent**, so it must not trust spot: an attacker could
//! pump a thin holding, drag the live index, and poison every baseline stamped in
//! that window forever. Stamps therefore mark everything — the newborn's own raw
//! price and the index levels it splices onto — at **realizable** quotes
//! ([`Pallet::realizable_tao_for_alpha`]), which are bounded by pool TAO reserves
//! and cannot be inflated without depositing real value. Every divisor shares this
//! convention, so funds stay comparable; a newborn starts on the live line up to its
//! book's spot-vs-realizable gap (small for liquid books). Money-moving paths keep
//! using realizable NAV exclusively (`basket_views.rs`); nothing in this module
//! sizes a mint or a redemption.
//!
//! ## Bounded work: stamps read a snapshot, a paged sweep maintains it
//!
//! Valuing every stamped fund is O(funds × holdings) — unbounded, since retired
//! funds keep entries while shares are outstanding. That sweep therefore never runs
//! inline in a state-changing path. Instead, block processing advances a **paged
//! background sweep** ([`Pallet::advance_beta_index_sweep`]): at most
//! [`BETA_INDEX_SWEEP_ROWS_PER_BLOCK`] rows per block, partial sums carried in
//! [`BetaIndexSweep`], and the finished pass published as [`BetaIndexSnapshot`]. A
//! stamp splices onto the latest snapshot — one read plus the fund's own holdings
//! scan, both inside every caller's declared weight envelope. Snapshot staleness is
//! bounded by the refresh interval plus one pass and is benign: the level drifts
//! slowly and stays realizable-marked. The live (RPC-only) getters still sweep at
//! spot; they never run in consensus paths.
//!
//! ## Fund lives
//!
//! Display state describes one fund *life*. It is born at the first share mint
//! (stamp) and retired when the last share is claimed or when a dust revival starts
//! a new life ([`Pallet::retire_beta_display_state`]) — a revived fund re-mints at
//! par, so an old divisor would splice it to an arbitrary level forever. Invariant:
//! `BetaBaseline` / `BasketTwr` entries exist only while the fund has outstanding
//! shares.
//!
//! ## Canonical period yield
//!
//! Both pipes reduce to "sample one cumulative series at two blocks":
//!
//! * **Allocators** (bought β): return over `[t0, t1]` is
//!   `display_price(t1) / display_price(t0) - 1`. The raw price is flow-invariant
//!   (deposits mint at NAV, redemptions exit at NAV), so only bag performance moves it.
//! * **Root stakers** (τ on netuid 0): return is `BasketTwr(t1) / BasketTwr(t0) - 1`
//!   under the claim-and-restake convention — each dividend compounds the accumulator
//!   by the value it added per rao of claimant stake, locked at deposit-time pricing.
//!   The hold-without-claiming alternative is
//!   `(BasketRate(t1) - BasketRate(t0)) * spot_price(t1)` (entitlement accrued over the
//!   window, marked at the end).
//!
//! Historical samples come from archive state (or these runtime APIs evaluated at a
//! historical block hash), so every consumer reproduces identical numbers.

use super::*;
use crate::rpc_info::basket_info::{BetaPosition, BetaPricing};
use sp_std::collections::btree_map::BTreeMap;
use substrate_fixed::types::{I96F32, U64F64};
use subtensor_swap_interface::SwapHandler;

/// Funds below this NAV (rao, at the sweep's mark) are too shallow to price the live
/// index: they still get display prices, but they do not pull the average. Mirrors the
/// SDK's dust floor (τ0.1).
pub const MIN_INDEX_NAV_RAO: u64 = 100_000_000;

/// Hard per-block budget for the background beta-index sweep, in rows (funds visited
/// plus holdings quoted). The same order as one batched dividend deposit's quote work,
/// so a page never dominates block processing regardless of how many fund lives have
/// accumulated.
pub const BETA_INDEX_SWEEP_ROWS_PER_BLOCK: u64 = 256;

/// Blocks between snapshot refreshes (~2 hours): a completed pass this recent is fresh
/// enough for stamps, so no new pass starts. Staleness at a stamp is bounded by this
/// plus one pass duration.
pub const BETA_INDEX_REFRESH_INTERVAL_BLOCKS: u64 = 600;

/// Per-netuid effective spot price cache for one pricing sweep (τ per alpha; 1.0 for
/// root and non-dynamic mechanisms).
type SpotPriceCache = BTreeMap<NetUid, U64F64>;

/// How a sweep marks fund holdings. `Spot` is the live display convention: a zero-size
/// mark, cheap, and self-healing if manipulated. `Realizable` is the
/// manipulation-resistant mark (bounded by pool TAO reserves), required wherever a
/// level gets frozen permanently (baseline stamps).
#[derive(Clone, Copy, PartialEq, Eq)]
enum IndexMark {
    Spot,
    Realizable,
}

/// One fund's resolved display marks, all derived from a single read of its baseline
/// so the divisor convention and the splice formulas cannot fork between the index
/// sweep, fund snapshots, and position views.
struct FundMarks {
    /// Raw-to-display divisor. Never zero, so `raw / divisor` and `amount * divisor`
    /// stay well-defined.
    divisor: U64F64,
    /// `raw / divisor` — the bag mark, comparable across fund ages.
    display: U64F64,
    /// `max(BasketRate - rate0, 0) * raw` — what τ1 of root stake earned here since
    /// the stamp, marked at today's raw price (`BasketRate` is denominated in raw β
    /// units per rao, so the delta pairs with the raw price).
    staker_yield: U64F64,
    /// `(1 + yield) * tr_splice` — the wealth of τ1 staked at the stamp block, in
    /// stake-index units.
    stake: U64F64,
    /// Block of the baseline stamp; 0 while provisional.
    first_block: u64,
    /// True while the fund has no frozen baseline: it prices pinned to the current
    /// index levels until its next share mint stamps one.
    provisional: bool,
}

impl<T: Config> Pallet<T> {
    fn cached_spot_price(netuid: NetUid, cache: &mut SpotPriceCache) -> U64F64 {
        if let Some(price) = cache.get(&netuid) {
            return *price;
        }
        let price = if netuid.is_root() || SubnetMechanism::<T>::get(netuid) != 1 {
            U64F64::saturating_from_num(1)
        } else {
            T::SwapInterface::current_alpha_price(netuid.into())
        };
        cache.insert(netuid, price);
        price
    }

    /// A fund's spot-marked NAV in rao and the number of holding rows scanned (for
    /// work pricing). Spot = `Σ price * alpha`, the same zero-size mark for every
    /// fund regardless of book size.
    fn basket_spot_nav_rao(hotkey: &T::AccountId, cache: &mut SpotPriceCache) -> (u64, u64) {
        let mut nav: u64 = 0;
        let mut rows: u64 = 0;
        for (netuid, alpha) in Self::get_basket_holdings(hotkey) {
            rows = rows.saturating_add(1);
            nav = nav.saturating_add(
                Self::cached_spot_price(netuid, cache)
                    .saturating_mul(U64F64::saturating_from_num(alpha.to_u64()))
                    .saturating_to_num::<u64>(),
            );
        }
        (nav, rows)
    }

    /// A fund's realizable NAV in rao and the number of holding rows scanned. Same
    /// valuation as [`Self::get_validator_basket_nav_tao`] (the money-path mark),
    /// with row accounting for callers that price work.
    fn basket_realizable_nav_rao(hotkey: &T::AccountId) -> (u64, u64) {
        let mut nav: u64 = 0;
        let mut rows: u64 = 0;
        for (netuid, alpha) in Self::get_basket_holdings(hotkey) {
            rows = rows.saturating_add(1);
            nav = nav.saturating_add(Self::realizable_tao_for_alpha(netuid, alpha.to_u64()));
        }
        (nav, rows)
    }

    /// A fund's marks against its frozen baseline. Zero-divisor baselines (defensive:
    /// the stamp path clamps to at least one ULP) resolve to divisor 1.
    fn fund_marks_from(baseline: &BetaBaselineOf, raw: U64F64, rate: I96F32) -> FundMarks {
        let one = U64F64::saturating_from_num(1);
        let divisor = if baseline.price_divisor == 0 {
            one
        } else {
            baseline.price_divisor
        };
        let display = raw.checked_div(divisor).unwrap_or(raw);
        let delta = rate.saturating_sub(baseline.rate0);
        let staker_yield = if delta.is_negative() {
            U64F64::saturating_from_num(0)
        } else {
            U64F64::saturating_from_num(delta).saturating_mul(raw)
        };
        let stake = one
            .saturating_add(staker_yield)
            .saturating_mul(baseline.tr_splice);
        FundMarks {
            divisor,
            display,
            staker_yield,
            stake,
            first_block: baseline.first_block,
            provisional: false,
        }
    }

    /// A fund's marks: from its frozen baseline, or *provisionally* pinned to the
    /// given live index levels (zero yield, implied divisor `raw / bag_level`) while
    /// unstamped. One baseline read either way.
    fn fund_marks(
        hotkey: &T::AccountId,
        raw: U64F64,
        bag_level: U64F64,
        stake_level: U64F64,
    ) -> FundMarks {
        if let Some(baseline) = BetaBaseline::<T>::get(hotkey) {
            return Self::fund_marks_from(&baseline, raw, BasketRate::<T>::get(hotkey));
        }
        let one = U64F64::saturating_from_num(1);
        let implied = raw.checked_div(bag_level).unwrap_or(one);
        let divisor = if implied == 0 { one } else { implied };
        FundMarks {
            divisor,
            display: raw.checked_div(divisor).unwrap_or(raw),
            staker_yield: U64F64::saturating_from_num(0),
            stake: stake_level,
            first_block: 0,
            provisional: true,
        }
    }

    /// One fund's index-sweep sample at the given mark: rows scanned plus, when the
    /// fund qualifies (outstanding shares, NAV over the dust floor, priceable), its
    /// `(nav, display, stake)` marks. Shared by the live spot sweep and the paged
    /// realizable sweep so the qualification rules cannot fork.
    fn index_fund_sample(
        hotkey: &T::AccountId,
        baseline: &BetaBaselineOf,
        mark: IndexMark,
        cache: &mut SpotPriceCache,
    ) -> (u64, Option<(u64, U64F64, U64F64)>) {
        let shares = BasketShares::<T>::get(hotkey);
        if shares == 0 {
            return (0, None);
        }
        let (nav, rows) = match mark {
            IndexMark::Spot => Self::basket_spot_nav_rao(hotkey, cache),
            IndexMark::Realizable => Self::basket_realizable_nav_rao(hotkey),
        };
        if nav < MIN_INDEX_NAV_RAO {
            return (rows, None);
        }
        let raw = U64F64::saturating_from_num(nav)
            .checked_div(U64F64::saturating_from_num(shares))
            .unwrap_or_default();
        if raw == 0 {
            return (rows, None);
        }
        let marks = Self::fund_marks_from(baseline, raw, BasketRate::<T>::get(hotkey));
        if marks.display == 0 {
            return (rows, None);
        }
        (rows, Some((nav, marks.display, marks.stake)))
    }

    /// One full index sweep at the given mark: `(bag_level, stake_level, rows_scanned)`.
    /// NAV-weighted means over every stamped fund with outstanding shares above the
    /// dust floor. Both levels default to 1.0 while no fund qualifies. Unbounded —
    /// RPC-only; consensus paths use [`Self::advance_beta_index_sweep`]'s snapshot.
    fn beta_index_levels_with(
        mark: IndexMark,
        cache: &mut SpotPriceCache,
    ) -> (U64F64, U64F64, u64) {
        let one = U64F64::saturating_from_num(1);
        let mut weight_sum: u128 = 0;
        let mut bag_sum = U64F64::saturating_from_num(0);
        let mut stake_sum = U64F64::saturating_from_num(0);
        let mut work: u64 = 0;
        for (hotkey, baseline) in BetaBaseline::<T>::iter() {
            work = work.saturating_add(1);
            let (rows, sample) = Self::index_fund_sample(&hotkey, &baseline, mark, cache);
            work = work.saturating_add(rows);
            let Some((nav, display, stake)) = sample else {
                continue;
            };
            let nav_weight = U64F64::saturating_from_num(nav);
            weight_sum = weight_sum.saturating_add(u128::from(nav));
            bag_sum = bag_sum.saturating_add(nav_weight.saturating_mul(display));
            stake_sum = stake_sum.saturating_add(nav_weight.saturating_mul(stake));
        }
        if weight_sum == 0 {
            return (one, one, work);
        }
        let denom = U64F64::saturating_from_num(weight_sum);
        (
            bag_sum.checked_div(denom).unwrap_or(one),
            stake_sum.checked_div(denom).unwrap_or(one),
            work,
        )
    }

    /// Advance the background beta-index sweep by one strictly bounded page; runs every
    /// block from `block_step`, right after the deposit-queue drain.
    ///
    /// No pass in progress: start one only when the published [`BetaIndexSnapshot`] is
    /// absent or at least [`BETA_INDEX_REFRESH_INTERVAL_BLOCKS`] old. Mid-pass: resume
    /// from the stored cursor, fold each fund's realizable sample into the partial sums,
    /// and stop after [`BETA_INDEX_SWEEP_ROWS_PER_BLOCK`] rows (overshooting only to
    /// finish the fund in hand, so the bound is `page + one fund's holdings`). A
    /// completed pass publishes the NAV-weighted levels (1.0 defaults while no fund
    /// qualifies) and clears the sweep state.
    ///
    /// Funds stamped or retired mid-pass may be counted once off; the snapshot is a
    /// splice target, not money-path state, so that approximation is benign. Returns the
    /// rows of work performed.
    pub(crate) fn advance_beta_index_sweep() -> u64 {
        let now = Self::get_current_block_as_u64();
        let mut state = match BetaIndexSweep::<T>::get() {
            Some(state) => state,
            None => {
                let fresh = BetaIndexSnapshot::<T>::get().is_some_and(|snapshot| {
                    now.saturating_sub(snapshot.block) < BETA_INDEX_REFRESH_INTERVAL_BLOCKS
                });
                if fresh {
                    return 0;
                }
                BetaIndexSweepOf {
                    cursor: Vec::new(),
                    weight_sum: 0,
                    bag_sum: U64F64::saturating_from_num(0),
                    stake_sum: U64F64::saturating_from_num(0),
                }
            }
        };
        let mut iter = if state.cursor.is_empty() {
            BetaBaseline::<T>::iter()
        } else {
            BetaBaseline::<T>::iter_from(state.cursor.clone())
        };
        let mut cache = SpotPriceCache::new();
        let mut work: u64 = 0;
        loop {
            let Some((hotkey, baseline)) = iter.next() else {
                let one = U64F64::saturating_from_num(1);
                let (bag_level, stake_level) = if state.weight_sum == 0 {
                    (one, one)
                } else {
                    let denom = U64F64::saturating_from_num(state.weight_sum);
                    (
                        state.bag_sum.checked_div(denom).unwrap_or(one),
                        state.stake_sum.checked_div(denom).unwrap_or(one),
                    )
                };
                BetaIndexSnapshot::<T>::put(BetaIndexSnapshotOf {
                    bag_level,
                    stake_level,
                    block: now,
                });
                BetaIndexSweep::<T>::kill();
                return work;
            };
            work = work.saturating_add(1);
            let (rows, sample) =
                Self::index_fund_sample(&hotkey, &baseline, IndexMark::Realizable, &mut cache);
            work = work.saturating_add(rows);
            if let Some((nav, display, stake)) = sample {
                let nav_weight = U64F64::saturating_from_num(nav);
                state.weight_sum = state.weight_sum.saturating_add(u128::from(nav));
                state.bag_sum = state
                    .bag_sum
                    .saturating_add(nav_weight.saturating_mul(display));
                state.stake_sum = state
                    .stake_sum
                    .saturating_add(nav_weight.saturating_mul(stake));
            }
            if work >= BETA_INDEX_SWEEP_ROWS_PER_BLOCK {
                state.cursor = BetaBaseline::<T>::hashed_key_for(&hotkey);
                BetaIndexSweep::<T>::put(state);
                return work;
            }
        }
    }

    /// The live (spot-marked) index sweep: `(bag_level, stake_level, rows_scanned)`.
    fn beta_index_levels_scanned(cache: &mut SpotPriceCache) -> (U64F64, U64F64, u64) {
        Self::beta_index_levels_with(IndexMark::Spot, cache)
    }

    /// The live `(bag index, stake index)` levels. See the module docs for what each
    /// level means.
    pub fn get_beta_index_levels() -> (U64F64, U64F64) {
        let mut cache = SpotPriceCache::new();
        let (bag, stake, _) = Self::beta_index_levels_scanned(&mut cache);
        (bag, stake)
    }

    /// Price one fund against already-computed index levels. An unstamped fund prices
    /// *provisionally*: pinned to the current index levels (zero yield) until its next
    /// share mint stamps a real baseline.
    fn beta_pricing_against(
        hotkey: &T::AccountId,
        shares: u64,
        bag_level: U64F64,
        stake_level: U64F64,
        cache: &mut SpotPriceCache,
    ) -> BetaPricing<T::AccountId> {
        let (spot_nav, _) = Self::basket_spot_nav_rao(hotkey, cache);
        let raw = U64F64::saturating_from_num(spot_nav)
            .checked_div(U64F64::saturating_from_num(shares))
            .unwrap_or_default();
        let marks = Self::fund_marks(hotkey, raw, bag_level, stake_level);
        BetaPricing {
            hotkey: hotkey.clone(),
            spot_price: raw,
            display_price: marks.display,
            stake_price: marks.stake,
            staker_yield: marks.staker_yield,
            staker_twr: BasketTwr::<T>::get(hotkey),
            bag_index: bag_level,
            stake_index: stake_level,
            first_block: marks.first_block,
            provisional: marks.provisional,
            spot_nav_tao: spot_nav.into(),
            shares,
            display_shares: U64F64::saturating_from_num(shares).saturating_mul(marks.divisor),
        }
    }

    /// One fund's standardized pricing snapshot, or `None` when the hotkey has no
    /// outstanding shares.
    pub fn get_beta_pricing(hotkey: &T::AccountId) -> Option<BetaPricing<T::AccountId>> {
        let shares = BasketShares::<T>::get(hotkey);
        if shares == 0 {
            return None;
        }
        let mut cache = SpotPriceCache::new();
        let (bag, stake, _) = Self::beta_index_levels_scanned(&mut cache);
        Some(Self::beta_pricing_against(
            hotkey, shares, bag, stake, &mut cache,
        ))
    }

    /// Pricing snapshots for every fund with outstanding shares, all marked against
    /// the same live index sweep — the whole leaderboard in one consistent call.
    pub fn get_all_beta_pricing() -> Vec<BetaPricing<T::AccountId>> {
        let mut cache = SpotPriceCache::new();
        let (bag, stake, _) = Self::beta_index_levels_scanned(&mut cache);
        BasketShares::<T>::iter()
            .filter(|(_, shares)| *shares != 0)
            .map(|(hotkey, shares)| {
                Self::beta_pricing_against(&hotkey, shares, bag, stake, &mut cache)
            })
            .collect()
    }

    /// One staker's position on one fund, against precomputed index levels. `None`
    /// when the fund has no shares or the staker no owed β.
    fn beta_position_against(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        bag_level: U64F64,
        stake_level: U64F64,
        cache: &mut SpotPriceCache,
    ) -> Option<BetaPosition<T::AccountId>> {
        let shares_total = BasketShares::<T>::get(hotkey);
        if shares_total == 0 {
            return None;
        }
        let beta = Self::get_basket_owed_shares(hotkey, coldkey).min(shares_total);
        if beta == 0 {
            return None;
        }
        let (spot_nav, _) = Self::basket_spot_nav_rao(hotkey, cache);
        // Realizable NAV prices what a claim would actually pay; the spot mark is for
        // display, mirroring the BetaPricing convention.
        let nav = Self::get_validator_basket_nav_tao(hotkey).to_u64();
        let raw = U64F64::saturating_from_num(spot_nav)
            .checked_div(U64F64::saturating_from_num(shares_total))
            .unwrap_or_default();
        let marks = Self::fund_marks(hotkey, raw, bag_level, stake_level);
        Some(BetaPosition {
            hotkey: hotkey.clone(),
            beta,
            display_beta: U64F64::saturating_from_num(beta).saturating_mul(marks.divisor),
            display_price: marks.display,
            value_tao: Self::basket_payout_from(beta, nav, shares_total).into(),
            spot_value_tao: Self::mul_div_u64(beta, spot_nav, shares_total)
                .min(spot_nav)
                .into(),
            provisional: marks.provisional,
        })
    }

    /// One staker's display-denominated position on one validator's fund, in the same
    /// units as [`BetaPricing`] (`display_beta * display_price` = spot value). `None`
    /// when the staker has no owed β there.
    pub fn get_beta_position(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
    ) -> Option<BetaPosition<T::AccountId>> {
        let mut cache = SpotPriceCache::new();
        let (bag, stake, _) = Self::beta_index_levels_scanned(&mut cache);
        Self::beta_position_against(hotkey, coldkey, bag, stake, &mut cache)
    }

    /// A coldkey's full display-denominated β portfolio: one position per validator on
    /// which it has owed β, all against one consistent index sweep.
    pub fn get_beta_portfolio(coldkey: &T::AccountId) -> Vec<BetaPosition<T::AccountId>> {
        let mut cache = SpotPriceCache::new();
        let (bag, stake, _) = Self::beta_index_levels_scanned(&mut cache);
        StakingHotkeys::<T>::get(coldkey)
            .into_iter()
            .filter_map(|hotkey| {
                Self::beta_position_against(&hotkey, coldkey, bag, stake, &mut cache)
            })
            .collect()
    }

    /// Compound the fund's staker total-return accumulator ([`BasketTwr`]) for a
    /// dividend deposit that added `stakers_value` TAO of realizable value across
    /// `total_root` rao of claimant stake: the deposit grew every staked rao by
    /// `stakers_value / total_root`, locked in at the deposit's own pricing. Staker
    /// yield over any window is then a pure ratio of two samples of this series.
    ///
    /// Display state only — never read by money paths. Called inside the deposit
    /// transaction so a rolled-back mint never compounds the series.
    pub(crate) fn accrue_basket_twr(hotkey: &T::AccountId, stakers_value: u64, total_root: u64) {
        BasketTwr::<T>::mutate(hotkey, |twr| {
            let gain = U64F64::saturating_from_num(stakers_value)
                .checked_div(U64F64::saturating_from_num(total_root))
                .unwrap_or_default();
            *twr = twr.saturating_mul(U64F64::saturating_from_num(1).saturating_add(gain));
        });
    }

    /// Retire a fund's display state at the end of a fund life. The frozen baseline
    /// and the TWR accumulator describe *this* life's history: a revived fund
    /// re-mints at par, so a leftover divisor would splice its new life to an
    /// arbitrary level forever. Called when a claim drains the last share and when a
    /// dust-revival mint starts a new life; the next share mint re-stamps fresh.
    /// Maintains the invariant that display state exists only for funds with
    /// outstanding shares (which is what lets a hotkey swap move it without merge
    /// semantics — see [`Self::transfer_beta_display_state`]).
    pub(crate) fn retire_beta_display_state(hotkey: &T::AccountId) {
        BetaBaseline::<T>::remove(hotkey);
        BasketTwr::<T>::remove(hotkey);
    }

    /// Move a fund's display state (frozen baseline + TWR accumulator) to its new
    /// hotkey on a hotkey swap. A pure move, never a merge: the clean-root gate in
    /// `do_swap_hotkey` requires the destination to hold zero `BasketShares`, and
    /// display state exists only while a fund has shares (stamped at a mint, retired
    /// on drain or dust revival), so the destination cannot hold either entry.
    pub(crate) fn transfer_beta_display_state(
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
    ) {
        if let Some(baseline) = BetaBaseline::<T>::take(old_hotkey) {
            BetaBaseline::<T>::insert(new_hotkey, baseline);
        }
        if BasketTwr::<T>::contains_key(old_hotkey) {
            BasketTwr::<T>::insert(new_hotkey, BasketTwr::<T>::take(old_hotkey));
        }
    }

    /// Stamp a fund's frozen [`BetaBaselineOf`] at its first share mint ("birth"), so
    /// its display and stake prices start at the published index levels. Idempotent
    /// and cheap once stamped (one `contains_key`). Returns the approximate scan work
    /// performed (the fund's own holding rows), for callers that price deposit weight.
    ///
    /// The stamp is permanent, so **everything here marks at realizable quotes** —
    /// the newborn's own raw price and the index levels it splices onto. Spot marks
    /// would let an attacker pump a thin pool (cheaply, recoverably) to poison either
    /// side of the divisor forever; realizable quotes are bounded by pool TAO
    /// reserves, so distorting a stamp requires depositing real value and losing it
    /// to slippage. See the module docs ("Two marks").
    ///
    /// The index levels come from [`BetaIndexSnapshot`] — the paged background sweep's
    /// output — never from an inline sweep, so the stamp's work is bounded by the
    /// fund's own holdings and fits every caller's declared weight envelope (signed
    /// deposits and the block-processing flush both call this). While no snapshot has
    /// been published yet (fresh chain, or the first blocks after the upgrade that
    /// introduced it) the fund stays unstamped and prices provisionally; the next mint
    /// retries. The sweep only sees stamped funds, so a newborn cannot skew its own
    /// splice regardless of timing.
    ///
    /// Called after a successful share mint in both deposit flows, so the fund's
    /// holdings, shares, and `BasketRate` are final for the block.
    pub(crate) fn stamp_beta_baseline_if_new(hotkey: &T::AccountId) -> u64 {
        if BetaBaseline::<T>::contains_key(hotkey) {
            return 0;
        }
        let shares = BasketShares::<T>::get(hotkey);
        if shares == 0 {
            return 0;
        }
        let Some(snapshot) = BetaIndexSnapshot::<T>::get() else {
            return 0;
        };
        let (nav, work) = Self::basket_realizable_nav_rao(hotkey);
        if nav == 0 {
            // Nothing to price against (every holding realizes to zero); leave the
            // fund unstamped so a later, priceable mint stamps it.
            return work;
        }
        let raw = U64F64::saturating_from_num(nav)
            .checked_div(U64F64::saturating_from_num(shares))
            .unwrap_or_default();
        if raw == 0 {
            return work;
        }
        let mut price_divisor = raw.checked_div(snapshot.bag_level).unwrap_or(raw);
        if price_divisor == 0 {
            // Extreme raw/level ratio rounded below one ULP; clamp so later divisions
            // stay defined (the fund then displays saturated-high rather than at zero).
            price_divisor = U64F64::from_bits(1);
        }
        BetaBaseline::<T>::insert(
            hotkey,
            BetaBaselineOf {
                price_divisor,
                rate0: BasketRate::<T>::get(hotkey),
                tr_splice: snapshot.stake_level,
                first_block: Self::get_current_block_as_u64(),
            },
        );
        Self::deposit_event(Event::BetaBaselineStamped {
            hotkey: hotkey.clone(),
        });
        work
    }
}
