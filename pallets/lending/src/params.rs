//! Root-settable parameter groups (SPEC 2.1): launch values, bounds and cross-checks.
//!
//! Each setter replaces a whole struct, so validation is all-or-nothing: `validate` passes
//! only if every bound in the SPEC 2.1 table holds. A validated value is written to its
//! `Pending…` twin and becomes live in the next block's `on_initialize`, with one
//! `<Group>ParamsSet` event (I26).
//!
//! `LendingParams` is the exception when a call tightens it (SPEC 2.1, rate-tightening delay):
//! an increase of `apr_bps` or `rate_cap_bps`, any change of `rate_k`, any change of
//! `twap_samples` or `twap_interval_blocks`, and a decrease of `rate_delay_blocks` waits the
//! delay live at the call in `PendingLendingParams` before the whole new group becomes live, so
//! a holder sees a higher rate coming before it is charged. A decrease of the delay therefore
//! cannot take effect before the old delay has run out.

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use frame_support::{dispatch::DispatchResult, storage::StorageValue, traits::Get};
use frame_system::pallet_prelude::BlockNumberFor;
use scale_info::TypeInfo;
use sp_runtime::traits::{One, Saturating};
use subtensor_macros::freeze_struct;
use subtensor_runtime_common::NetUid;

use crate::{
    Config, DeregParamsStore, Error, Event, LendingParamsStore, LongParamsStore, Pallet,
    PendingDeregParams, PendingLendingParams, PendingLongParams, PendingShortParams,
    ShortParamsStore, math::rate::RateCurve,
};

/// One TAO (or one alpha) in rao.
pub const RAO: u64 = 1_000_000_000;

/// Value of the runtime constant `Config::MaxLongPositionsCeiling`, the ceiling for
/// `DeregParams::max_long_positions_per_subnet` (SPEC 7.4): the prune weight is benchmarked
/// against it, so it is not settable.
pub const MAX_LONG_POSITIONS_CEILING: u32 = 1_024;

/// Launch value of `LendingParams::rate_k`, the exponent of the rate curve (SPEC 2.1).
pub const LAUNCH_RATE_K: u8 = 1;

/// Launch value of `LendingParams::rate_cap_bps`, the cap of the rate curve (SPEC 2.1):
/// 200% a year.
pub const LAUNCH_RATE_CAP_BPS: u32 = 20_000;

/// Highest `LendingParams::rate_cap_bps` the setter accepts (SPEC 2.1): 10,000% a year.
pub const MAX_RATE_CAP_BPS: u32 = 1_000_000;

/// Launch value of `LendingParams::rate_delay_blocks`: 3 days, one TWAP window.
pub const LAUNCH_RATE_DELAY_BLOCKS: u32 = 21_600;

/// Highest `LendingParams::rate_delay_blocks` the setter accepts: 30 days.
pub const MAX_RATE_DELAY_BLOCKS: u32 = 216_000;

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

/// Parameters shared by both sides (`set_lending_params`).
#[freeze_struct("8aa7e9e63672c054")]
#[derive(
    Encode, Decode, DecodeWithMemTracking, MaxEncodedLen, TypeInfo, Clone, PartialEq, Eq, Debug,
)]
pub struct LendingParams {
    /// Base APR, both sides (P3/L3): the yearly rate of a position at `u <= 1`, and the floor
    /// of every position's rate. Must be positive: interest is what ends a stretched position.
    pub apr_bps: u32,
    /// Exponent `k` of the rate curve `min(cap, apr * max(1, u^k))`, both sides (P5).
    pub rate_k: u8,
    /// Cap of the yearly rate in bps, both sides (P5). At least `apr_bps`.
    pub rate_cap_bps: u32,
    /// Blocks a `set_lending_params` call that tightens the group waits before the new group is
    /// live. The value in force at the call sets that call's wait.
    pub rate_delay_blocks: u32,
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
            rate_delay_blocks: LAUNCH_RATE_DELAY_BLOCKS,
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
    /// The rate curve these parameters define (SPEC section 1 item 2).
    #[must_use]
    pub fn rate_curve(&self) -> RateCurve {
        RateCurve {
            apr_bps: self.apr_bps,
            k: self.rate_k,
            cap_bps: self.rate_cap_bps,
        }
    }

    /// Whether moving from `live` to these parameters tightens the group (SPEC 2.1): an
    /// increase of `apr_bps` or `rate_cap_bps`, any change of `rate_k`, any change of
    /// `twap_samples` or `twap_interval_blocks`, or a decrease of `rate_delay_blocks`. A
    /// tightening waits the delay live at the call (the old one, for a shorter delay). Any other
    /// change (a decrease of the rate terms, a longer delay, the charge cadence, the minimums or
    /// the deposit) is live from the next block.
    #[must_use]
    pub fn tightens(&self, live: &Self) -> bool {
        self.apr_bps > live.apr_bps
            || self.rate_cap_bps > live.rate_cap_bps
            || self.rate_k != live.rate_k
            || self.twap_samples != live.twap_samples
            || self.twap_interval_blocks != live.twap_interval_blocks
            || self.rate_delay_blocks < live.rate_delay_blocks
    }

    /// Checks every bound of the SPEC 2.1 table for this group, and those of the field added to
    /// it. The cap's lower bound is the base APR of the same value, so the curve reaches the
    /// base rate at `u = 1`.
    pub fn validate(&self) -> Result<(), ParamsError> {
        ensure_range("apr_bps", self.apr_bps, 1, 10_000)?;
        ensure_range("rate_k", self.rate_k, 1, 8)?;
        ensure_range(
            "rate_cap_bps",
            self.rate_cap_bps,
            self.apr_bps,
            MAX_RATE_CAP_BPS,
        )?;
        ensure_range(
            "rate_delay_blocks",
            self.rate_delay_blocks,
            0,
            MAX_RATE_DELAY_BLOCKS,
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

    /// [`Self::validate`], and the cap cross-check of SPEC 2.1 against the live group: the cap
    /// is at least the base APR under both the live and the new values, so the bound holds
    /// whichever of the two is in force.
    pub fn validate_against_live(&self, live: &Self) -> Result<(), ParamsError> {
        self.validate()?;
        if self.rate_cap_bps.min(live.rate_cap_bps) < self.apr_bps.max(live.apr_bps) {
            return Err(ParamsError::OutOfRange("rate_cap_bps"));
        }
        Ok(())
    }
}

/// Short-only parameters (`set_short_params`).
#[freeze_struct("99c4185eab9d27e5")]
#[derive(
    Encode, Decode, DecodeWithMemTracking, MaxEncodedLen, TypeInfo, Clone, PartialEq, Eq, Debug,
)]
pub struct ShortParams {
    /// Coverage stress (P2), flat for every book size.
    pub stress_bps: u32,
    /// Net minted alpha per block as a share of SubnetAlphaIn (P8).
    pub open_limit_bps: u32,
}

impl Default for ShortParams {
    fn default() -> Self {
        Self {
            stress_bps: 30_000,
            open_limit_bps: 100,
        }
    }
}

impl ShortParams {
    /// Checks every bound of the SPEC 2.1 table for this group.
    pub fn validate(&self) -> Result<(), ParamsError> {
        ensure_range("stress_bps", self.stress_bps, 10_000, 100_000)?;
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
    /// `set_lending_params`: validate, then schedule the whole group (SPEC 2.1).
    ///
    /// A call that tightens the live group (see [`LendingParams::tightens`]) is written to
    /// `PendingLendingParams` with `apply_at` the live `rate_delay_blocks` after this
    /// block, and emits `LendingParamsScheduled`. A later call replaces the pending group
    /// whatever it holds. Any other call is applied in the next block. The delay is at least one
    /// block, so `apply_at` is always in the future.
    pub(crate) fn queue_lending_params(params: LendingParams) -> DispatchResult {
        let live = LendingParamsStore::<T>::get();
        params
            .validate_against_live(&live)
            .map_err(|_| Error::<T>::InvalidParams)?;
        let now = frame_system::Pallet::<T>::block_number();
        if params.tightens(&live) {
            let delay = BlockNumberFor::<T>::from(live.rate_delay_blocks).max(One::one());
            let apply_at = now.saturating_add(delay);
            PendingLendingParams::<T>::put((params.clone(), apply_at));
            Self::deposit_event(Event::LendingParamsScheduled {
                old: live,
                new: params,
                apply_at,
            });
        } else {
            PendingLendingParams::<T>::put((params, now.saturating_add(One::one())));
        }
        Ok(())
    }

    /// Makes the pending `LendingParams` live once its `apply_at` is reached, with one
    /// `LendingParamsSet` event, and re-seeds the TWAP rings if their window changed. Runs first
    /// in `on_initialize`, before any charge of that block, and returns whether it applied a
    /// group. No position is touched: each one reads the new group at its next accrual.
    pub(crate) fn apply_pending_lending_params(now: BlockNumberFor<T>, subnets: &[NetUid]) -> bool {
        let Some((new, apply_at)) = PendingLendingParams::<T>::get() else {
            return false;
        };
        if now < apply_at {
            return false;
        }
        PendingLendingParams::<T>::kill();
        let old = LendingParamsStore::<T>::get();
        LendingParamsStore::<T>::put(&new);
        if (old.twap_samples, old.twap_interval_blocks)
            != (new.twap_samples, new.twap_interval_blocks)
        {
            Self::reseed_twap_rings_from_median(subnets, new.twap_samples);
        }
        Self::deposit_event(Event::LendingParamsSet { old, new });
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

    /// Applies every pending group other than `LendingParams` (SPEC 2.1, I26): one
    /// `<Group>ParamsSet` event each, and the rank ring re-seed that a rank window change
    /// requires. No position is touched. Returns whether any group was pending.
    pub(crate) fn apply_pending_params(subnets: &[NetUid]) -> bool {
        let mut applied = false;
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
                p.rate_delay_blocks,
                p.charge_interval_blocks
            ),
            (2_000, 1, 20_000, 21_600, 360)
        );
        assert_eq!(
            (p.rate_k, p.rate_cap_bps, p.rate_delay_blocks),
            (LAUNCH_RATE_K, LAUNCH_RATE_CAP_BPS, LAUNCH_RATE_DELAY_BLOCKS)
        );
        assert_eq!(
            (p.min_pool_tao, p.min_notional, p.deposit),
            (1_000_000_000_000, 10_000_000_000, 100_000_000)
        );
        assert_eq!((p.twap_samples, p.twap_interval_blocks), (72, 300));
        assert_eq!(p.validate(), Ok(()));

        let s = ShortParams::default();
        assert_eq!((s.stress_bps, s.open_limit_bps), (30_000, 100));
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
        check_bounds(
            &base,
            "rate_delay_blocks",
            1,
            (0, 216_000),
            |p, x| p.rate_delay_blocks = x,
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
    fn the_cap_is_checked_against_the_live_and_the_new_base_apr() {
        let live = LendingParams::default(); // apr 2,000, cap 20,000
        let with = |apr_bps, rate_cap_bps| {
            LendingParams {
                apr_bps,
                rate_cap_bps,
                ..live.clone()
            }
            .validate_against_live(&live)
        };
        assert_eq!(with(2_000, 2_000), Ok(()));
        assert_eq!(with(10_000, 10_000), Ok(()));
        // A new base APR above the live cap, with the cap lowered to it, is rejected: the live
        // group would be briefly invalid if it were applied first.
        let high = LendingParams {
            apr_bps: 10_000,
            rate_cap_bps: 10_000,
            ..live.clone()
        };
        assert_eq!(
            LendingParams {
                apr_bps: 10_000,
                rate_cap_bps: 9_999,
                ..live.clone()
            }
            .validate_against_live(&live),
            Err(ParamsError::OutOfRange("rate_cap_bps"))
        );
        // Lowering both rate terms in one call is checked against the larger of the two base
        // APRs (here the live 10,000), so it needs the cap to stay high for one step: the cap
        // comes down in a second call, once the base APR has.
        let lower_both = LendingParams {
            apr_bps: 2_000,
            rate_cap_bps: 2_000,
            ..live.clone()
        };
        assert_eq!(
            lower_both.validate_against_live(&high),
            Err(ParamsError::OutOfRange("rate_cap_bps"))
        );
        let first_step = LendingParams {
            apr_bps: 2_000,
            rate_cap_bps: 10_000,
            ..live.clone()
        };
        assert_eq!(first_step.validate_against_live(&high), Ok(()));
        assert_eq!(lower_both.validate_against_live(&first_step), Ok(()));
    }

    #[test]
    fn a_change_tightens_only_if_it_raises_a_rate_term_or_moves_k_or_the_twap() {
        let live = LendingParams::default();
        let tightens = |f: fn(&mut LendingParams)| {
            let mut next = live.clone();
            f(&mut next);
            next.tightens(&live)
        };
        assert!(!live.tightens(&live));
        // Decreases of the rate terms are not tightening.
        assert!(!tightens(|p| p.apr_bps -= 1));
        assert!(!tightens(|p| p.rate_cap_bps -= 1));
        assert!(!tightens(|p| {
            p.apr_bps = 1;
            p.rate_cap_bps = 1;
        }));
        // Increases of either term are.
        assert!(tightens(|p| p.apr_bps += 1));
        assert!(tightens(|p| p.rate_cap_bps += 1));
        // One term up tightens whatever comes down with it.
        assert!(tightens(|p| {
            p.apr_bps -= 1;
            p.rate_cap_bps += 1;
        }));
        // Any change of the exponent tightens: a higher k raises the rate above u = 1, a lower
        // one raises it below.
        assert!(tightens(|p| p.rate_k += 1));
        assert!(tightens(|p| p.rate_k -= 1));
        // Any change of the TWAP window tightens.
        assert!(tightens(|p| p.twap_samples += 1));
        assert!(tightens(|p| p.twap_samples -= 1));
        assert!(tightens(|p| p.twap_interval_blocks += 1));
        assert!(tightens(|p| p.twap_interval_blocks -= 1));
        // A longer delay is not a tightening; a shorter one is, since it waits the current delay.
        assert!(!tightens(|p| p.rate_delay_blocks += 1));
        assert!(tightens(|p| p.rate_delay_blocks -= 1));
        // The cadence, the minimums and the deposit are not rate terms.
        assert!(!tightens(|p| p.charge_interval_blocks += 1));
        assert!(!tightens(|p| p.min_pool_tao += RAO));
        assert!(!tightens(|p| p.min_notional += RAO));
        assert!(!tightens(|p| p.deposit += RAO));
    }

    #[test]
    fn the_rate_curve_reads_apr_k_and_cap() {
        let moved = LendingParams {
            apr_bps: 1_500,
            rate_k: 3,
            rate_cap_bps: 50_000,
            ..Default::default()
        };
        let curve = moved.rate_curve();
        assert_eq!((curve.apr_bps, curve.k, curve.cap_bps), (1_500, 3, 50_000));
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
