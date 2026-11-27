//! Flat-APR interest (SPEC 1.2, P3, P15, 7.1.4): per-block accrual with a carried remainder.

use super::fixed::{BPS, Price, floor_div, mul_price_floor, saturate_u64};

/// Blocks per day at 12 s.
pub const BLOCKS_PER_DAY: u32 = 7_200;
/// Blocks per year used for APR accrual.
pub const BLOCKS_PER_YEAR: u128 = 7_200 * 365;
/// Denominator of one block of interest: `notional * apr_bps / (1e4 * blocks per year)`.
const ACCRUAL_DEN: u128 = BPS * BLOCKS_PER_YEAR;

/// Interest accrued over a span of blocks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Accrued {
    /// Whole rao owed, rounded down (saturating at `u64::MAX`).
    pub owed: u64,
    /// Fraction of a rao carried to the next accrual, in units of `1 / (1e4 * blocks per year)`.
    pub remainder: u128,
}

/// Short interest notional (SPEC 1.2): `floor(q * TWAP_median)`. A long's notional is its debt.
#[must_use]
pub fn short_notional(q: u64, twap: Price) -> u64 {
    mul_price_floor(q, twap)
}

/// Accrues `blocks` of interest on `notional` (SPEC 1.2, P15):
/// `num = notional * apr_bps * blocks + remainder`, `owed = floor(num / den)`,
/// `remainder = num mod den`, with `den = 1e4 * blocks per year`.
///
/// `remainder` is the value returned by the previous accrual (below `den`).
#[must_use]
pub fn accrue(notional: u64, apr_bps: u32, blocks: u64, remainder: u128) -> Accrued {
    // notional * apr = whole * den + part, so num = whole * blocks * den + (part * blocks + rem)
    // and no product leaves u128 for any u64 notional and block count.
    let rate = u128::from(notional).saturating_mul(u128::from(apr_bps));
    let whole = floor_div(rate, ACCRUAL_DEN).unwrap_or(0);
    let part = rate.checked_rem(ACCRUAL_DEN).unwrap_or(0);
    let tail = part
        .saturating_mul(u128::from(blocks))
        .saturating_add(remainder);
    let owed = whole
        .saturating_mul(u128::from(blocks))
        .saturating_add(floor_div(tail, ACCRUAL_DEN).unwrap_or(0));
    Accrued {
        owed: saturate_u64(Some(owed)),
        remainder: tail.checked_rem(ACCRUAL_DEN).unwrap_or(0),
    }
}

/// The first charge block after `now` on the charge grid of a position opened at `opened_at`
/// with `interval` blocks: the smallest `opened_at + k * interval`, `k >= 0`, that is strictly
/// after `now`.
///
/// The model charges at every block where `(block - opened_at) % interval == 0`, with the
/// interval live at that block. A charge that ran late therefore schedules its next charge
/// back on that grid, not `interval` after itself, so lateness does not shift the grid. A
/// `now` before `opened_at` gives the first grid block. An `interval` of zero is read as one
/// block; `charge_interval_blocks` is validated to 1..=7200, so that is only a guard. Every
/// operation saturates.
#[must_use]
pub fn next_charge_block(opened_at: u64, now: u64, interval: u32) -> u64 {
    let step = u64::from(interval.max(1));
    // Whole intervals since the open, rounded down; the next grid block is one more.
    let periods = now.saturating_sub(opened_at).checked_div(step).unwrap_or(0);
    opened_at.saturating_add(periods.saturating_add(1).saturating_mul(step))
}

/// Interest on `notional` over `blocks`, rounded down, with no carried remainder. Used for the
/// interest cover (`min_interest_cover_blocks`, P5) and the withdraw buffer (P20); at 7,200
/// blocks it is the model's `floor(notional * apr_bps / (1e4 * 365))`.
#[must_use]
pub fn interest_for_blocks(notional: u64, apr_bps: u32, blocks: u32) -> u64 {
    accrue(notional, apr_bps, u64::from(blocks), 0).owed
}

#[cfg(test)]
mod tests {
    #![allow(clippy::arithmetic_side_effects)]
    use super::*;

    const RAO: u64 = 1_000_000_000;

    #[test]
    fn nothing_accrues_on_zero_notional_or_zero_blocks() {
        assert_eq!(
            accrue(0, 2_000, 360, 5),
            Accrued {
                owed: 0,
                remainder: 5
            }
        );
        assert_eq!(
            accrue(10 * RAO, 2_000, 0, 5),
            Accrued {
                owed: 0,
                remainder: 5
            }
        );
    }

    #[test]
    fn one_year_at_twenty_percent() {
        let year = u64::try_from(BLOCKS_PER_YEAR).unwrap_or(0);
        assert_eq!(
            accrue(10 * RAO, 2_000, year, 0),
            Accrued {
                owed: 2 * RAO,
                remainder: 0
            }
        );
    }

    #[test]
    fn the_remainder_carries_so_split_accrual_equals_one_accrual() {
        // The 10 TAO minimum at 20 % accrues about 761 rao per block (SPEC P15).
        let whole = accrue(10 * RAO, 2_000, 720, 0);
        let mut owed = 0;
        let mut remainder = 0;
        for _ in 0..720 {
            let step = accrue(10 * RAO, 2_000, 1, remainder);
            assert!(step.owed == 761 || step.owed == 762);
            owed += step.owed;
            remainder = step.remainder;
        }
        assert_eq!(Accrued { owed, remainder }, whole);
    }

    #[test]
    fn extreme_inputs_saturate_instead_of_overflowing() {
        let top = accrue(u64::MAX, u32::MAX, u64::MAX, 0);
        assert_eq!(top.owed, u64::MAX);
        assert!(top.remainder < ACCRUAL_DEN);
    }

    #[test]
    fn day_of_interest_rounds_down() {
        // 10 TAO * 20 % / 365 = 5_479_452.05... rao
        assert_eq!(
            interest_for_blocks(10 * RAO, 2_000, BLOCKS_PER_DAY),
            5_479_452
        );
        assert_eq!(interest_for_blocks(10 * RAO, 2_000, 0), 0);
        assert_eq!(interest_for_blocks(0, 2_000, BLOCKS_PER_DAY), 0);
    }

    #[test]
    fn next_charge_is_the_next_grid_block_after_an_on_grid_charge() {
        // Opened at 100, 360 blocks: the grid is 460, 820, 1180, ...
        assert_eq!(next_charge_block(100, 460, 360), 820);
        assert_eq!(next_charge_block(100, 820, 360), 1_180);
    }

    #[test]
    fn next_charge_realigns_after_a_charge_one_block_late() {
        assert_eq!(next_charge_block(100, 461, 360), 820);
        // One block before the grid block is still before it.
        assert_eq!(next_charge_block(100, 819, 360), 820);
    }

    #[test]
    fn next_charge_realigns_after_a_charge_several_intervals_late() {
        // 1,205 is 2 intervals and 25 blocks after the grid block 820 (the grid is 100 + k*360).
        assert_eq!(next_charge_block(100, 1_205, 360), 1_540);
        // Late by exactly three intervals: the next grid block is the one after.
        assert_eq!(next_charge_block(100, 1_540, 360), 1_900);
    }

    #[test]
    fn next_charge_follows_an_interval_changed_between_charges() {
        // Charged at 820 under 360: the next is on the 360 grid.
        assert_eq!(next_charge_block(100, 820, 360), 1_180);
        // Under 7 from a charge at 1_181: the grid is 100 + k*7, the next block after 1_181.
        let next = next_charge_block(100, 1_181, 7);
        assert_eq!(next, 1_185);
        assert_eq!((next - 100) % 7, 0);
        assert!(next > 1_181 && next <= 1_181 + 7);
    }

    #[test]
    fn next_charge_at_the_open_is_one_interval_later() {
        assert_eq!(next_charge_block(100, 100, 360), 460);
        assert_eq!(next_charge_block(100, 100, 1), 101);
        // A `now` before the open gives the first grid block.
        assert_eq!(next_charge_block(100, 50, 360), 460);
    }

    #[test]
    fn next_charge_is_always_on_the_grid_and_within_one_interval() {
        for interval in [1u32, 7, 360, 7_200] {
            for now in 100..100 + 3 * u64::from(interval) + 5 {
                let next = next_charge_block(100, now, interval);
                assert!(next > now, "interval {interval} now {now}");
                assert!(
                    next <= now + u64::from(interval),
                    "interval {interval} now {now}"
                );
                assert_eq!((next - 100) % u64::from(interval), 0);
            }
        }
    }

    #[test]
    fn a_zero_interval_is_read_as_one_block_and_nothing_overflows() {
        assert_eq!(next_charge_block(100, 150, 0), 151);
        assert_eq!(next_charge_block(100, 150, 1), 151);
        assert_eq!(next_charge_block(u64::MAX - 5, u64::MAX - 1, 360), u64::MAX);
        assert_eq!(next_charge_block(0, u64::MAX, u32::MAX), u64::MAX);
    }

    #[test]
    fn short_notional_is_floor_of_q_times_twap() {
        let half = Price::from_bits(1 << 63);
        assert_eq!(short_notional(7, half), 3);
        assert_eq!(short_notional(0, half), 0);
    }
}
