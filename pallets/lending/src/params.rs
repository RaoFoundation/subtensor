//! Root-settable parameter groups (SPEC 2.1): launch values, bounds and cross-checks.
//!
//! Each setter replaces a whole struct, so validation is all-or-nothing: `validate` passes
//! only if every bound in the SPEC 2.1 table holds. A validated value is written to its
//! `Pending…` twin and becomes live in the next block's `on_initialize`, with one
//! `<Group>ParamsSet` event (I26).
//!
//! The rate terms of `LendingParams` (`apr_bps`, `rate_k`, `rate_cap_bps`, `rate_floor_bps`)
//! are the exception when a call raises them: the new terms wait `rate_increase_delay_blocks`
//! in `PendingRateIncrease` before they become live (an approved addition beyond SPEC-v2.1),
//! so a holder sees a higher rate coming before it is charged.

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use frame_support::{dispatch::DispatchResult, storage::StorageValue, traits::Get};
use frame_system::pallet_prelude::BlockNumberFor;
use scale_info::TypeInfo;
use sp_runtime::traits::{One, Saturating};
use subtensor_macros::freeze_struct;
use subtensor_runtime_common::NetUid;

use crate::{
    Config, DeregParamsStore, Error, Event, LendingParamsStore, LongParamsStore, Pallet,
    PendingDeregParams, PendingLendingParams, PendingLongParams, PendingRateIncrease,
    PendingShortParams, ShortParamsStore, math::rate::RateCurve,
};

/// One TAO (or one alpha) in rao.
pub const RAO: u64 = 1_000_000_000;

/// Value of the runtime constant `Config::MaxLongPositionsCeiling`, the ceiling for
/// `DeregParams::max_long_positions_per_subnet` (SPEC 7.4): the prune weight is benchmarked
/// against it, so it is not settable.
pub const MAX_LONG_POSITIONS_CEILING: u32 = 1_024;

/// Launch value of `LendingParams::rate_k`, the exponent of the rate curve (SPEC 2.1).
// PROVISIONAL (spec 10.3): the launch exponent is an open decision; change it here only.
pub const LAUNCH_RATE_K: u8 = 2;

/// Launch value of `LendingParams::rate_cap_bps`, the cap of the rate curve (SPEC 2.1).
// PROVISIONAL (spec 10.3): the launch cap is an open decision; change it here only.
pub const LAUNCH_RATE_CAP_BPS: u32 = 10_000;

/// Highest `LendingParams::rate_cap_bps` the setter accepts (SPEC 2.1): 10,000% a year.
pub const MAX_RATE_CAP_BPS: u32 = 1_000_000;

/// Launch value of `LendingParams::rate_floor_bps`, the least yearly rate of a position with
/// debt: the launch base APR.
// PROVISIONAL (pending model results): change the launch floor here only.
pub const LAUNCH_RATE_FLOOR_BPS: u32 = 2_000;

/// Launch value of `LendingParams::rate_increase_delay_blocks`: 3 days.
pub const LAUNCH_RATE_INCREASE_DELAY_BLOCKS: u32 = 21_600;

/// Highest `LendingParams::rate_increase_delay_blocks` the setter accepts: 30 days.
pub const MAX_RATE_INCREASE_DELAY_BLOCKS: u32 = 216_000;

/// Why a parameter group was rejected (surfaced as `InvalidParams`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParamsError {
    /// The named field is outside its fixed bounds.
    OutOfRange(&'static str),
    /// `LongParams::stress_bps` is below `stress_floor_bps` (L2).
    StressBelowFloor,
    /// `max_long_positions_per_subnet` is below a subnet's current long count (7.4).
    BelowCurrentLongCount,
}

fn ensure_range<T: PartialOrd>(
    field: &'static str,
    value: T,
    min: T,
    max: T,
) -> Result<(), ParamsError> {
    if value < min || value > max {
        return Err(ParamsError::OutOfRange(field));
    }
    Ok(())
}

/// The rate terms of [`LendingParams`]: the four values that set what a position pays. A
/// `set_lending_params` call that raises them is delayed (see [`RateTerms::is_decrease_from`]).
#[freeze_struct("d4d3d18134405a8b")]
#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    MaxEncodedLen,
    TypeInfo,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
)]
pub struct RateTerms {
    /// `LendingParams::apr_bps`.
    pub apr_bps: u32,
    /// `LendingParams::rate_k`.
    pub rate_k: u8,
    /// `LendingParams::rate_cap_bps`.
    pub rate_cap_bps: u32,
    /// `LendingParams::rate_floor_bps`.
    pub rate_floor_bps: u32,
}

impl RateTerms {
    /// Whether moving from `current` to these terms can raise no position's rate: the base APR,
    /// the cap and the floor are each at most the current one and the exponent is unchanged. A
    /// change of `k` raises the rate on one side of `u = 1`, so it never counts as a decrease.
    /// Equal terms are a decrease.
    #[must_use]
    pub fn is_decrease_from(&self, current: &Self) -> bool {
        self.apr_bps <= current.apr_bps
            && self.rate_cap_bps <= current.rate_cap_bps
            && self.rate_floor_bps <= current.rate_floor_bps
            && self.rate_k == current.rate_k
    }

    /// Checks the bounds of the four terms (see [`LendingParams::validate`]).
    pub fn validate(&self) -> Result<(), ParamsError> {
        ensure_range("apr_bps", self.apr_bps, 1, 10_000)?;
        ensure_range("rate_k", self.rate_k, 1, 8)?;
        ensure_range(
            "rate_cap_bps",
            self.rate_cap_bps,
            self.apr_bps,
            MAX_RATE_CAP_BPS,
        )?;
        ensure_range("rate_floor_bps", self.rate_floor_bps, 1, self.rate_cap_bps)
    }
}

/// Parameters shared by both sides (`set_lending_params`).
#[freeze_struct("148aeaef958b5228")]
#[derive(
    Encode, Decode, DecodeWithMemTracking, MaxEncodedLen, TypeInfo, Clone, PartialEq, Eq, Debug,
)]
pub struct LendingParams {
    /// Base APR, both sides (P3/L3): the yearly rate of a position at `u = 1`. Must be
    /// positive: interest is what ends a stretched position.
    pub apr_bps: u32,
    /// Exponent `k` of the rate curve `min(cap, apr * u^k)`, both sides (P5).
    pub rate_k: u8,
    /// Cap of the yearly rate in bps, both sides (P5). At least `apr_bps`.
    pub rate_cap_bps: u32,
    /// Least yearly rate in bps of a position with debt, both sides. At least 1, so no
    /// position with debt accrues at a rate of zero, and at most `rate_cap_bps`.
    pub rate_floor_bps: u32,
    /// Blocks a `set_lending_params` call that raises the rate terms waits before the new
    /// terms are live. The value in force at the call sets that call's wait.
    pub rate_increase_delay_blocks: u32,
    /// Blocks between interest charges (P15).
    pub charge_interval_blocks: u32,
    /// Minimum SubnetTAO for opens and increases, rao (P18).
    pub min_pool_tao: u64,
    /// Minimum position notional, rao TAO, both sides (P10/L10).
    pub min_notional: u64,
    /// Position deposit, rao TAO, both sides (P19/L11).
    pub deposit: u64,
    /// TWAP ring length (P6/L6).
    pub twap_samples: u32,
    /// Blocks between TWAP samples (P6).
    pub twap_interval_blocks: u32,
}

impl Default for LendingParams {
    fn default() -> Self {
        Self {
            apr_bps: 2_000,
            rate_k: LAUNCH_RATE_K,
            rate_cap_bps: LAUNCH_RATE_CAP_BPS,
            rate_floor_bps: LAUNCH_RATE_FLOOR_BPS,
            rate_increase_delay_blocks: LAUNCH_RATE_INCREASE_DELAY_BLOCKS,
            charge_interval_blocks: 360,
            min_pool_tao: 1_000 * RAO,
            min_notional: 10 * RAO,
            deposit: RAO / 10,
            twap_samples: 72,
            twap_interval_blocks: 300,
        }
    }
}

impl LendingParams {
    /// The rate curve these parameters define (SPEC section 1 item 2, with the floor).
    #[must_use]
    pub fn rate_curve(&self) -> RateCurve {
        RateCurve {
            apr_bps: self.apr_bps,
            k: self.rate_k,
            cap_bps: self.rate_cap_bps,
            floor_bps: self.rate_floor_bps,
        }
    }

    /// The four rate terms of these parameters.
    #[must_use]
    pub fn rate_terms(&self) -> RateTerms {
        RateTerms {
            apr_bps: self.apr_bps,
            rate_k: self.rate_k,
            rate_cap_bps: self.rate_cap_bps,
            rate_floor_bps: self.rate_floor_bps,
        }
    }

    /// These parameters with their four rate terms replaced by `terms`.
    #[must_use]
    pub fn with_rate_terms(self, terms: RateTerms) -> Self {
        Self {
            apr_bps: terms.apr_bps,
            rate_k: terms.rate_k,
            rate_cap_bps: terms.rate_cap_bps,
            rate_floor_bps: terms.rate_floor_bps,
            ..self
        }
    }

    /// Checks every bound of the SPEC 2.1 table for this group, and those of the two fields
    /// added to it. The cap's lower bound is the base APR of the same value, so the curve
    /// reaches the base rate at `u = 1`; the floor's upper bound is the cap of the same value.
    pub fn validate(&self) -> Result<(), ParamsError> {
        self.rate_terms().validate()?;
        ensure_range(
            "rate_increase_delay_blocks",
            self.rate_increase_delay_blocks,
            0,
            MAX_RATE_INCREASE_DELAY_BLOCKS,
        )?;
        ensure_range(
            "charge_interval_blocks",
            self.charge_interval_blocks,
            1,
            7_200,
        )?;
        ensure_range("min_pool_tao", self.min_pool_tao, 100 * RAO, 100_000 * RAO)?;
        ensure_range("min_notional", self.min_notional, RAO / 10, 10_000 * RAO)?;
        ensure_range("deposit", self.deposit, 0, 10 * RAO)?;
        ensure_range("twap_samples", self.twap_samples, 3, 72)?;
        ensure_range("twap_interval_blocks", self.twap_interval_blocks, 50, 1_200)
    }
}

/// Short-only parameters (`set_short_params`).
#[freeze_struct("7a49a229cde0d334")]
#[derive(
    Encode, Decode, DecodeWithMemTracking, MaxEncodedLen, TypeInfo, Clone, PartialEq, Eq, Debug,
)]
pub struct ShortParams {
    /// Coverage stress (P2).
    pub stress_bps: u32,
    /// Book-aware stress slope (P2).
    pub stress_slope_bps: u32,
    /// Net minted alpha per block as a share of SubnetAlphaIn (P8).
    pub open_limit_bps: u32,
}

impl Default for ShortParams {
    fn default() -> Self {
        Self {
            stress_bps: 30_000,
            stress_slope_bps: 20_000,
            open_limit_bps: 100,
        }
    }
}

impl ShortParams {
    /// Checks every bound of the SPEC 2.1 table for this group.
    pub fn validate(&self) -> Result<(), ParamsError> {
        ensure_range("stress_bps", self.stress_bps, 10_000, 100_000)?;
        ensure_range("stress_slope_bps", self.stress_slope_bps, 0, 100_000)?;
        ensure_range("open_limit_bps", self.open_limit_bps, 1, 1_000)
    }
}

/// Long-only parameters (`set_long_params`).
#[freeze_struct("aedc82c2125baa6d")]
#[derive(
    Encode, Decode, DecodeWithMemTracking, MaxEncodedLen, TypeInfo, Clone, PartialEq, Eq, Debug,
)]
pub struct LongParams {
    /// Coverage stress m (L2).
    pub stress_bps: u32,
    /// Lowest stress the setter accepts (L2).
    pub stress_floor_bps: u32,
    /// Aggregate lift cap as a share of SubnetTAO (L5).
    pub lift_cap_bps: u32,
    /// Net lifted TAO per block as a share of SubnetTAO (L8).
    pub open_limit_bps: u32,
}

impl Default for LongParams {
    fn default() -> Self {
        Self {
            stress_bps: 20_000,
            stress_floor_bps: 15_000,
            lift_cap_bps: 1_000,
            open_limit_bps: 100,
        }
    }
}

impl LongParams {
    /// Checks every bound of the SPEC 2.1 table for this group.
    pub fn validate(&self) -> Result<(), ParamsError> {
        ensure_range("stress_bps", self.stress_bps, 10_000, 100_000)?;
        ensure_range("stress_floor_bps", self.stress_floor_bps, 10_000, 100_000)?;
        ensure_range("lift_cap_bps", self.lift_cap_bps, 1, 5_000)?;
        ensure_range("open_limit_bps", self.open_limit_bps, 1, 1_000)?;
        if self.stress_bps < self.stress_floor_bps {
            return Err(ParamsError::StressBelowFloor);
        }
        Ok(())
    }
}

/// Dereg parameters (`set_dereg_params`).
#[freeze_struct("e2f1b59454de132f")]
#[derive(
    Encode, Decode, DecodeWithMemTracking, MaxEncodedLen, TypeInfo, Clone, PartialEq, Eq, Debug,
)]
pub struct DeregParams {
    /// Dereg zone: SMA30 within this multiple of the lowest non-immune SMA30 (P16).
    pub zone_bps: u32,
    /// Window of the pool-TAO EMA used in r (P12).
    pub pool_tao_ema_blocks: u32,
    /// Rank SMA window in days (P13).
    pub rank_window_days: u32,
    /// Longs allowed per subnet (7.4).
    pub max_long_positions_per_subnet: u32,
}

impl Default for DeregParams {
    fn default() -> Self {
        Self {
            zone_bps: 12_500,
            pool_tao_ema_blocks: 7_200,
            rank_window_days: 30,
            max_long_positions_per_subnet: 256,
        }
    }
}

impl DeregParams {
    /// Checks every bound of the SPEC 2.1 table for this group. `max_long_position_count` is
    /// the largest current `LongPositionCount` on any subnet and `ceiling` is the runtime's
    /// `MaxLongPositionsCeiling`.
    pub fn validate(&self, max_long_position_count: u32, ceiling: u32) -> Result<(), ParamsError> {
        ensure_range("zone_bps", self.zone_bps, 10_000, 30_000)?;
        ensure_range(
            "pool_tao_ema_blocks",
            self.pool_tao_ema_blocks,
            300,
            216_000,
        )?;
        ensure_range("rank_window_days", self.rank_window_days, 7, 30)?;
        ensure_range(
            "max_long_positions_per_subnet",
            self.max_long_positions_per_subnet,
            0,
            ceiling,
        )?;
        if self.max_long_positions_per_subnet < max_long_position_count {
            return Err(ParamsError::BelowCurrentLongCount);
        }
        Ok(())
    }
}

/// Moves a pending group into its live store and returns `(old, new)`; `None` when nothing
/// is pending.
fn take_pending<G, Live, Pending>() -> Option<(G, G)>
where
    G: codec::FullCodec,
    Live: StorageValue<G, Query = G>,
    Pending: StorageValue<G, Query = Option<G>>,
{
    let new = Pending::take()?;
    let old = Live::get();
    Live::put(&new);
    Some((old, new))
}

impl<T: Config> Pallet<T> {
    /// `set_lending_params`: validate, then queue for the next block (SPEC 2.1).
    ///
    /// The rate terms are compared with the live ones. A decrease is queued with the rest and
    /// clears a pending increase. An increase is held in `PendingRateIncrease` for the live
    /// `rate_increase_delay_blocks` (at least until the next block), replacing an earlier
    /// pending one; the rest of the group is queued with the live rate terms, unless that
    /// leaves the live group as it is.
    pub(crate) fn queue_lending_params(params: LendingParams) -> DispatchResult {
        params.validate().map_err(|_| Error::<T>::InvalidParams)?;
        let live = LendingParamsStore::<T>::get();
        let (current, new) = (live.rate_terms(), params.rate_terms());
        if new.is_decrease_from(&current) {
            PendingRateIncrease::<T>::kill();
            PendingLendingParams::<T>::put(params);
        } else {
            let delay = BlockNumberFor::<T>::from(live.rate_increase_delay_blocks);
            let effective_at =
                frame_system::Pallet::<T>::block_number().saturating_add(delay.max(One::one()));
            PendingRateIncrease::<T>::put((new, effective_at));
            let rest = params.with_rate_terms(current);
            if rest == live {
                PendingLendingParams::<T>::kill();
            } else {
                PendingLendingParams::<T>::put(rest);
            }
            Self::deposit_event(Event::RateIncreaseScheduled { new, effective_at });
        }
        Ok(())
    }

    /// Makes a pending rate increase live once its block is reached, with one
    /// `RateIncreaseApplied` event. Runs after [`Self::apply_pending_params`], so the terms land
    /// on the group that block made live. No position is touched: each one reads the new terms
    /// at its next accrual. Returns whether an increase was applied.
    pub(crate) fn apply_pending_rate_increase(now: BlockNumberFor<T>) -> bool {
        let Some((new, effective_at)) = PendingRateIncrease::<T>::get() else {
            return false;
        };
        if now < effective_at {
            return false;
        }
        PendingRateIncrease::<T>::kill();
        let live = LendingParamsStore::<T>::get();
        let old = live.rate_terms();
        LendingParamsStore::<T>::put(live.with_rate_terms(new));
        Self::deposit_event(Event::RateIncreaseApplied { old, new });
        true
    }

    /// `set_short_params`: validate, then queue for the next block (SPEC 2.1).
    pub(crate) fn queue_short_params(params: ShortParams) -> DispatchResult {
        params.validate().map_err(|_| Error::<T>::InvalidParams)?;
        PendingShortParams::<T>::put(params);
        Ok(())
    }

    /// `set_long_params`: validate, then queue for the next block (SPEC 2.1).
    pub(crate) fn queue_long_params(params: LongParams) -> DispatchResult {
        params.validate().map_err(|_| Error::<T>::InvalidParams)?;
        PendingLongParams::<T>::put(params);
        Ok(())
    }

    /// `set_dereg_params`: validate, then queue for the next block (SPEC 2.1). No long
    /// positions exist yet, so the longs cap is checked against a count of zero.
    pub(crate) fn queue_dereg_params(params: DeregParams) -> DispatchResult {
        params
            .validate(0, T::MaxLongPositionsCeiling::get())
            .map_err(|_| Error::<T>::InvalidParams)?;
        PendingDeregParams::<T>::put(params);
        Ok(())
    }

    /// Applies every pending group (SPEC 2.1, I26): one `<Group>ParamsSet` event each, and the
    /// ring re-seeds that a TWAP or rank window change requires. No position is touched.
    /// Returns whether any group was pending.
    pub(crate) fn apply_pending_params(subnets: &[NetUid]) -> bool {
        let mut applied = false;
        if let Some((old, new)) =
            take_pending::<_, LendingParamsStore<T>, PendingLendingParams<T>>()
        {
            applied = true;
            if (old.twap_samples, old.twap_interval_blocks)
                != (new.twap_samples, new.twap_interval_blocks)
            {
                Self::reseed_twap_rings_from_median(subnets, new.twap_samples);
            }
            Self::deposit_event(Event::LendingParamsSet { old, new });
        }
        if let Some((old, new)) = take_pending::<_, ShortParamsStore<T>, PendingShortParams<T>>() {
            applied = true;
            Self::deposit_event(Event::ShortParamsSet { old, new });
        }
        if let Some((old, new)) = take_pending::<_, LongParamsStore<T>, PendingLongParams<T>>() {
            applied = true;
            Self::deposit_event(Event::LongParamsSet { old, new });
        }
        if let Some((old, new)) = take_pending::<_, DeregParamsStore<T>, PendingDeregParams<T>>() {
            applied = true;
            if old.rank_window_days != new.rank_window_days {
                Self::reseed_rank_rings_from_published(subnets, new.rank_window_days);
            }
            Self::deposit_event(Event::DeregParamsSet { old, new });
        }
        applied
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::arithmetic_side_effects)]
    use super::*;

    /// Asserts `field` accepts exactly `[min, max]` around an otherwise valid `base`.
    fn check_bounds<
        P: Clone,
        T: Copy + PartialOrd + core::ops::Add<Output = T> + core::ops::Sub<Output = T>,
    >(
        base: &P,
        name: &'static str,
        one: T,
        (min, max): (T, T),
        set: impl Fn(&mut P, T),
        validate: impl Fn(&P) -> Result<(), ParamsError>,
    ) {
        let with = |v: T| {
            let mut p = base.clone();
            set(&mut p, v);
            validate(&p)
        };
        assert_eq!(with(min), Ok(()), "{name} min");
        assert_eq!(with(max), Ok(()), "{name} max");
        assert_eq!(
            with(max + one),
            Err(ParamsError::OutOfRange(name)),
            "{name} above"
        );
        if min >= one {
            assert_eq!(
                with(min - one),
                Err(ParamsError::OutOfRange(name)),
                "{name} below"
            );
        }
    }

    #[test]
    fn launch_values_match_the_spec_table_and_validate() {
        let p = LendingParams::default();
        assert_eq!(
            (
                p.apr_bps,
                p.rate_k,
                p.rate_cap_bps,
                p.charge_interval_blocks
            ),
            (2_000, 2, 10_000, 360)
        );
        assert_eq!(
            (p.rate_floor_bps, p.rate_increase_delay_blocks),
            (2_000, 21_600)
        );
        assert_eq!(
            (p.rate_floor_bps, p.rate_increase_delay_blocks),
            (LAUNCH_RATE_FLOOR_BPS, LAUNCH_RATE_INCREASE_DELAY_BLOCKS)
        );
        assert_eq!(
            (p.rate_k, p.rate_cap_bps),
            (LAUNCH_RATE_K, LAUNCH_RATE_CAP_BPS)
        );
        assert_eq!(
            (p.min_pool_tao, p.min_notional, p.deposit),
            (1_000_000_000_000, 10_000_000_000, 100_000_000)
        );
        assert_eq!((p.twap_samples, p.twap_interval_blocks), (72, 300));
        assert_eq!(p.validate(), Ok(()));

        let s = ShortParams::default();
        assert_eq!(
            (s.stress_bps, s.stress_slope_bps, s.open_limit_bps),
            (30_000, 20_000, 100)
        );
        assert_eq!(s.validate(), Ok(()));

        let l = LongParams::default();
        assert_eq!(
            (
                l.stress_bps,
                l.stress_floor_bps,
                l.lift_cap_bps,
                l.open_limit_bps
            ),
            (20_000, 15_000, 1_000, 100)
        );
        assert_eq!(l.validate(), Ok(()));

        let d = DeregParams::default();
        assert_eq!(
            (
                d.zone_bps,
                d.pool_tao_ema_blocks,
                d.rank_window_days,
                d.max_long_positions_per_subnet
            ),
            (12_500, 7_200, 30, 256)
        );
        assert_eq!(d.validate(0, MAX_LONG_POSITIONS_CEILING), Ok(()));
        assert_eq!(d.validate(256, MAX_LONG_POSITIONS_CEILING), Ok(()));
    }

    #[test]
    fn lending_bounds() {
        let base = LendingParams::default();
        let v = LendingParams::validate;
        // The base APR's upper bound is tested under a cap that admits it.
        let wide = LendingParams {
            rate_cap_bps: MAX_RATE_CAP_BPS,
            ..Default::default()
        };
        check_bounds(&wide, "apr_bps", 1, (1, 10_000), |p, x| p.apr_bps = x, v);
        check_bounds(&base, "rate_k", 1, (1, 8), |p, x| p.rate_k = x, v);
        check_bounds(
            &base,
            "rate_cap_bps",
            1,
            (2_000, 1_000_000),
            |p, x| p.rate_cap_bps = x,
            v,
        );
        // The launch cap bounds the floor from above.
        check_bounds(
            &base,
            "rate_floor_bps",
            1,
            (1, 10_000),
            |p, x| p.rate_floor_bps = x,
            v,
        );
        check_bounds(
            &base,
            "rate_increase_delay_blocks",
            1,
            (0, 216_000),
            |p, x| p.rate_increase_delay_blocks = x,
            v,
        );
        check_bounds(
            &base,
            "charge_interval_blocks",
            1,
            (1, 7_200),
            |p, x| p.charge_interval_blocks = x,
            v,
        );
        check_bounds(
            &base,
            "min_pool_tao",
            1,
            (100 * RAO, 100_000 * RAO),
            |p, x| p.min_pool_tao = x,
            v,
        );
        check_bounds(
            &base,
            "min_notional",
            1,
            (RAO / 10, 10_000 * RAO),
            |p, x| p.min_notional = x,
            v,
        );
        check_bounds(&base, "deposit", 1, (0, 10 * RAO), |p, x| p.deposit = x, v);
        check_bounds(
            &base,
            "twap_samples",
            1,
            (3, 72),
            |p, x| p.twap_samples = x,
            v,
        );
        check_bounds(
            &base,
            "twap_interval_blocks",
            1,
            (50, 1_200),
            |p, x| p.twap_interval_blocks = x,
            v,
        );
    }

    #[test]
    fn the_rate_cap_may_not_be_below_the_base_apr() {
        let with = |apr_bps, rate_cap_bps| {
            LendingParams {
                apr_bps,
                rate_cap_bps,
                rate_floor_bps: 1,
                ..Default::default()
            }
            .validate()
        };
        // A cap equal to the base APR is a flat rate above u = 1.
        assert_eq!(with(2_000, 2_000), Ok(()));
        assert_eq!(with(1, 1), Ok(()));
        assert_eq!(with(10_000, 10_000), Ok(()));
        assert_eq!(
            with(2_000, 1_999),
            Err(ParamsError::OutOfRange("rate_cap_bps"))
        );
        // Raising the base APR above a cap that stays is rejected the same way.
        assert_eq!(
            with(10_000, 9_999),
            Err(ParamsError::OutOfRange("rate_cap_bps"))
        );
        assert_eq!(with(1, 1_000_000), Ok(()));
    }

    #[test]
    fn the_rate_floor_is_at_least_one_and_at_most_the_cap() {
        let with = |rate_cap_bps, rate_floor_bps| {
            LendingParams {
                rate_cap_bps,
                rate_floor_bps,
                ..Default::default()
            }
            .validate()
        };
        assert_eq!(with(10_000, 1), Ok(()));
        assert_eq!(with(10_000, 10_000), Ok(()));
        assert_eq!(with(2_000, 2_000), Ok(()));
        assert_eq!(with(1_000_000, 1_000_000), Ok(()));
        assert_eq!(
            with(10_000, 0),
            Err(ParamsError::OutOfRange("rate_floor_bps"))
        );
        assert_eq!(
            with(10_000, 10_001),
            Err(ParamsError::OutOfRange("rate_floor_bps"))
        );
        // Lowering the cap under a floor that stays is rejected the same way.
        assert_eq!(
            with(2_000, 2_001),
            Err(ParamsError::OutOfRange("rate_floor_bps"))
        );
    }

    #[test]
    fn a_rate_change_is_a_decrease_only_if_nothing_rises_and_k_stays() {
        let current = LendingParams::default().rate_terms();
        let with = |f: fn(&mut RateTerms)| {
            let mut terms = current;
            f(&mut terms);
            terms.is_decrease_from(&current)
        };
        assert!(current.is_decrease_from(&current));
        assert!(with(|t| t.apr_bps -= 1));
        assert!(with(|t| t.rate_cap_bps -= 1));
        assert!(with(|t| t.rate_floor_bps -= 1));
        assert!(with(|t| {
            t.apr_bps = 1;
            t.rate_cap_bps = 1;
            t.rate_floor_bps = 1;
        }));
        assert!(!with(|t| t.apr_bps += 1));
        assert!(!with(|t| t.rate_cap_bps += 1));
        assert!(!with(|t| t.rate_floor_bps += 1));
        // One term up makes the change an increase, whatever comes down with it.
        assert!(!with(|t| {
            t.apr_bps -= 1;
            t.rate_cap_bps += 1;
        }));
        assert!(!with(|t| {
            t.rate_floor_bps -= 1;
            t.apr_bps += 1;
        }));
        // Any change of the exponent is an increase: a higher k raises the rate above u = 1,
        // a lower k raises it below.
        assert!(!with(|t| t.rate_k += 1));
        assert!(!with(|t| t.rate_k -= 1));
        assert!(!with(|t| {
            t.rate_k -= 1;
            t.apr_bps = 1;
            t.rate_cap_bps = 1;
            t.rate_floor_bps = 1;
        }));
    }

    #[test]
    fn the_rate_terms_round_trip_through_the_group() {
        let base = LendingParams::default();
        assert_eq!(base.clone().with_rate_terms(base.rate_terms()), base);
        let terms = RateTerms {
            apr_bps: 1_500,
            rate_k: 3,
            rate_cap_bps: 50_000,
            rate_floor_bps: 7,
        };
        let moved = base.clone().with_rate_terms(terms);
        assert_eq!(moved.rate_terms(), terms);
        assert_eq!(
            moved,
            LendingParams {
                apr_bps: 1_500,
                rate_k: 3,
                rate_cap_bps: 50_000,
                rate_floor_bps: 7,
                ..base
            }
        );
        let curve = moved.rate_curve();
        assert_eq!(
            (curve.apr_bps, curve.k, curve.cap_bps, curve.floor_bps),
            (1_500, 3, 50_000, 7)
        );
    }

    #[test]
    fn short_bounds() {
        let base = ShortParams::default();
        let v = ShortParams::validate;
        check_bounds(
            &base,
            "stress_bps",
            1,
            (10_000, 100_000),
            |p, x| p.stress_bps = x,
            v,
        );
        check_bounds(
            &base,
            "stress_slope_bps",
            1,
            (0, 100_000),
            |p, x| p.stress_slope_bps = x,
            v,
        );
        check_bounds(
            &base,
            "open_limit_bps",
            1,
            (1, 1_000),
            |p, x| p.open_limit_bps = x,
            v,
        );
    }

    #[test]
    fn long_bounds() {
        let base = LongParams::default();
        let v = LongParams::validate;
        check_bounds(
            &base,
            "lift_cap_bps",
            1,
            (1, 5_000),
            |p, x| p.lift_cap_bps = x,
            v,
        );
        check_bounds(
            &base,
            "open_limit_bps",
            1,
            (1, 1_000),
            |p, x| p.open_limit_bps = x,
            v,
        );
        let with = |stress_bps, stress_floor_bps| {
            LongParams {
                stress_bps,
                stress_floor_bps,
                ..Default::default()
            }
            .validate()
        };
        assert_eq!(with(10_000, 10_000), Ok(()));
        assert_eq!(with(100_000, 100_000), Ok(()));
        assert_eq!(with(100_000, 10_000), Ok(()));
        assert_eq!(
            with(100_001, 15_000),
            Err(ParamsError::OutOfRange("stress_bps"))
        );
        assert_eq!(
            with(20_000, 9_999),
            Err(ParamsError::OutOfRange("stress_floor_bps"))
        );
        assert_eq!(with(14_999, 15_000), Err(ParamsError::StressBelowFloor));
    }

    #[test]
    fn dereg_bounds() {
        let base = DeregParams::default();
        let v = |p: &DeregParams| p.validate(0, MAX_LONG_POSITIONS_CEILING);
        check_bounds(
            &base,
            "zone_bps",
            1,
            (10_000, 30_000),
            |p, x| p.zone_bps = x,
            v,
        );
        check_bounds(
            &base,
            "pool_tao_ema_blocks",
            1,
            (300, 216_000),
            |p, x| p.pool_tao_ema_blocks = x,
            v,
        );
        check_bounds(
            &base,
            "rank_window_days",
            1,
            (7, 30),
            |p, x| p.rank_window_days = x,
            v,
        );
        check_bounds(
            &base,
            "max_long_positions_per_subnet",
            1,
            (0, 1_024),
            |p, x| p.max_long_positions_per_subnet = x,
            v,
        );
    }

    #[test]
    fn longs_cap_may_not_drop_below_a_live_count() {
        let p = DeregParams {
            max_long_positions_per_subnet: 10,
            ..Default::default()
        };
        assert_eq!(p.validate(10, MAX_LONG_POSITIONS_CEILING), Ok(()));
        assert_eq!(
            p.validate(11, MAX_LONG_POSITIONS_CEILING),
            Err(ParamsError::BelowCurrentLongCount)
        );
    }
}
