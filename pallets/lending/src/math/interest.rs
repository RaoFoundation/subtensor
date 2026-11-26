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
    fn short_notional_is_floor_of_q_times_twap() {
        let half = Price::from_bits(1 << 63);
        assert_eq!(short_notional(7, half), 3);
        assert_eq!(short_notional(0, half), 0);
    }
}
