//! Root-settable parameter groups (SPEC 2.1): launch values, bounds and cross-checks.
//!
//! Each setter replaces a whole struct, so validation is all-or-nothing: `validate` passes
//! only if every bound in the SPEC 2.1 table holds.

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use scale_info::TypeInfo;
use subtensor_macros::freeze_struct;

/// One TAO (or one alpha) in rao.
pub const RAO: u64 = 1_000_000_000;

/// Runtime ceiling for `DeregParams::max_long_positions_per_subnet` (SPEC 7.4): the prune
/// weight is benchmarked against it, so it is not settable.
pub const MAX_LONG_POSITIONS_CEILING: u32 = 1_024;

/// Why a parameter group was rejected (surfaced as `InvalidParams`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParamsError {
    /// The named field is outside its fixed bounds.
    OutOfRange(&'static str),
    /// `min_interest_cover_blocks` exceeds `withdraw_buffer_blocks` (P5/P20).
    CoverExceedsWithdrawBuffer,
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
#[freeze_struct("909b3a24cacff0a5")]
#[derive(
    Encode, Decode, DecodeWithMemTracking, MaxEncodedLen, TypeInfo, Clone, PartialEq, Eq, Debug,
)]
pub struct LendingParams {
    /// Flat APR, both sides (P3/L3). Must be positive: interest terminates paused positions.
    pub apr_bps: u32,
    /// Blocks between interest charges (P15).
    pub charge_interval_blocks: u32,
    /// Equity cutoff in rao TAO (P5): triggered while `Equity_TWAP < equity_floor`.
    pub equity_floor: u64,
    /// Blocks of interest that collateral must cover (P5).
    pub min_interest_cover_blocks: u32,
    /// Deleverage price impact per block, both sides (P7/L7).
    pub pace_bps_per_block: u32,
    /// TWAP term of the pause band, both sides (P17/L9).
    pub pause_twap_bps: u32,
    /// SMA30 term of the pause band, both sides (P22/L9).
    pub pause_sma_bps: u32,
    /// Minimum SubnetTAO for opens and increases, rao (P18).
    pub min_pool_tao: u64,
    /// Minimum position notional, rao TAO, both sides (P10/L10).
    pub min_notional: u64,
    /// Position deposit, rao TAO, both sides (P19/L11).
    pub deposit: u64,
    /// Blocks of interest a withdraw must leave above the coverage need (P20/L12).
    pub withdraw_buffer_blocks: u32,
    /// Deleverage exit margin, both sides (P5, v2.0.1).
    pub exit_hysteresis_bps: u32,
    /// TWAP ring length (P6/L6).
    pub twap_samples: u32,
    /// Blocks between TWAP samples (P6).
    pub twap_interval_blocks: u32,
}

impl Default for LendingParams {
    fn default() -> Self {
        Self {
            apr_bps: 2_000,
            charge_interval_blocks: 360,
            equity_floor: 0,
            min_interest_cover_blocks: 7_200,
            pace_bps_per_block: 5,
            pause_twap_bps: 12_500,
            pause_sma_bps: 12_500,
            min_pool_tao: 1_000 * RAO,
            min_notional: 10 * RAO,
            deposit: RAO / 10,
            withdraw_buffer_blocks: 50_400,
            exit_hysteresis_bps: 500,
            twap_samples: 72,
            twap_interval_blocks: 300,
        }
    }
}

impl LendingParams {
    /// Checks every bound of the SPEC 2.1 table for this group.
    pub fn validate(&self) -> Result<(), ParamsError> {
        ensure_range("apr_bps", self.apr_bps, 1, 10_000)?;
        ensure_range(
            "charge_interval_blocks",
            self.charge_interval_blocks,
            1,
            7_200,
        )?;
        ensure_range("equity_floor", self.equity_floor, 0, 1_000 * RAO)?;
        ensure_range(
            "min_interest_cover_blocks",
            self.min_interest_cover_blocks,
            1,
            216_000,
        )?;
        ensure_range("pace_bps_per_block", self.pace_bps_per_block, 1, 100)?;
        ensure_range("pause_twap_bps", self.pause_twap_bps, 10_001, 50_000)?;
        ensure_range("pause_sma_bps", self.pause_sma_bps, 10_001, 50_000)?;
        ensure_range("min_pool_tao", self.min_pool_tao, 100 * RAO, 100_000 * RAO)?;
        ensure_range("min_notional", self.min_notional, RAO / 10, 10_000 * RAO)?;
        ensure_range("deposit", self.deposit, 0, 10 * RAO)?;
        ensure_range(
            "withdraw_buffer_blocks",
            self.withdraw_buffer_blocks,
            1,
            216_000,
        )?;
        ensure_range("exit_hysteresis_bps", self.exit_hysteresis_bps, 0, 5_000)?;
        ensure_range("twap_samples", self.twap_samples, 3, 72)?;
        ensure_range("twap_interval_blocks", self.twap_interval_blocks, 50, 1_200)?;
        if self.min_interest_cover_blocks > self.withdraw_buffer_blocks {
            return Err(ParamsError::CoverExceedsWithdrawBuffer);
        }
        Ok(())
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
    /// the largest current `LongPositionCount` on any subnet.
    pub fn validate(&self, max_long_position_count: u32) -> Result<(), ParamsError> {
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
            MAX_LONG_POSITIONS_CEILING,
        )?;
        if self.max_long_positions_per_subnet < max_long_position_count {
            return Err(ParamsError::BelowCurrentLongCount);
        }
        Ok(())
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
                p.charge_interval_blocks,
                p.equity_floor,
                p.min_interest_cover_blocks
            ),
            (2_000, 360, 0, 7_200)
        );
        assert_eq!(
            (p.pace_bps_per_block, p.pause_twap_bps, p.pause_sma_bps),
            (5, 12_500, 12_500)
        );
        assert_eq!(
            (p.min_pool_tao, p.min_notional, p.deposit),
            (1_000_000_000_000, 10_000_000_000, 100_000_000)
        );
        assert_eq!(
            (
                p.withdraw_buffer_blocks,
                p.exit_hysteresis_bps,
                p.twap_samples,
                p.twap_interval_blocks
            ),
            (50_400, 500, 72, 300)
        );
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
        assert_eq!(d.validate(0), Ok(()));
        assert_eq!(d.validate(256), Ok(()));
    }

    #[test]
    fn lending_bounds() {
        // Cover and buffer are widened so each field is tested against its own fixed bound.
        let base = LendingParams {
            min_interest_cover_blocks: 1,
            withdraw_buffer_blocks: 216_000,
            ..Default::default()
        };
        let v = LendingParams::validate;
        check_bounds(&base, "apr_bps", 1, (1, 10_000), |p, x| p.apr_bps = x, v);
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
            "equity_floor",
            1,
            (0, 1_000 * RAO),
            |p, x| p.equity_floor = x,
            v,
        );
        check_bounds(
            &base,
            "min_interest_cover_blocks",
            1,
            (1, 216_000),
            |p, x| p.min_interest_cover_blocks = x,
            v,
        );
        check_bounds(
            &base,
            "pace_bps_per_block",
            1,
            (1, 100),
            |p, x| p.pace_bps_per_block = x,
            v,
        );
        check_bounds(
            &base,
            "pause_twap_bps",
            1,
            (10_001, 50_000),
            |p, x| p.pause_twap_bps = x,
            v,
        );
        check_bounds(
            &base,
            "pause_sma_bps",
            1,
            (10_001, 50_000),
            |p, x| p.pause_sma_bps = x,
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
            "exit_hysteresis_bps",
            1,
            (0, 5_000),
            |p, x| p.exit_hysteresis_bps = x,
            v,
        );
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
        check_bounds(
            &base,
            "withdraw_buffer_blocks",
            1,
            (1, 216_000),
            |p, x| p.withdraw_buffer_blocks = x,
            v,
        );
    }

    #[test]
    fn cover_may_not_exceed_the_withdraw_buffer() {
        let equal = LendingParams {
            min_interest_cover_blocks: 50_400,
            ..Default::default()
        };
        assert_eq!(equal.validate(), Ok(()));
        let above = LendingParams {
            min_interest_cover_blocks: 50_401,
            ..Default::default()
        };
        assert_eq!(
            above.validate(),
            Err(ParamsError::CoverExceedsWithdrawBuffer)
        );
        let below = LendingParams {
            withdraw_buffer_blocks: 7_199,
            ..Default::default()
        };
        assert_eq!(
            below.validate(),
            Err(ParamsError::CoverExceedsWithdrawBuffer)
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
        let v = |p: &DeregParams| p.validate(0);
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
        assert_eq!(p.validate(10), Ok(()));
        assert_eq!(p.validate(11), Err(ParamsError::BelowCurrentLongCount));
    }
}
