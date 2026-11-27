//! The convex interest rate (SPEC section 1 item 2, P3, P5) with a minimum-rate floor:
//! `rate = max(floor, min(cap, apr * u^k))`, with `u = num / den` a position's debt over its
//! holdings. The floor is an approved addition beyond SPEC-v2.1.
//!
//! A short has `num = floor(q * TWAP_median)` and `den = C + P`; a long has `num = D` and
//! `den = floor(A * TWAP_median) + C`. Both are linear in the position's size, so the rate is
//! the same for a position and for any pro-rata piece of it (SPEC I23).

use super::fixed::mul_div_floor;

/// The curve without the floor, `min(cap, apr * u^k)` in bps a year:
///
/// ```text
/// num == 0 -> 0
/// den == 0 -> cap_bps
/// r = apr_bps
/// repeat k times { r = floor(r * num / den); if r >= cap_bps { return cap_bps } }
/// min(r, cap_bps)
/// ```
///
/// The floor at each step and the early stop are part of the definition: the reference model
/// (`model/v21`) computes the same integers. Each step is a 256-bit product, so no operand
/// overflows; a step whose result exceeds `u128` is above every cap. The result is at most
/// `cap_bps`.
#[must_use]
pub fn curve_bps(apr_bps: u32, k: u8, cap_bps: u32, num: u128, den: u128) -> u32 {
    if num == 0 {
        return 0;
    }
    if den == 0 {
        return cap_bps;
    }
    let cap = u128::from(cap_bps);
    let mut rate = u128::from(apr_bps);
    for _ in 0..k {
        match mul_div_floor(rate, num, den) {
            Some(next) if next < cap => rate = next,
            _ => return cap_bps,
        }
    }
    u32::try_from(rate.min(cap)).unwrap_or(cap_bps)
}

/// The yearly rate in bps of a position with debt `num` and holdings `den`:
/// `max(floor_bps, curve_bps(..))`, except that a position with no debt (`num == 0`) has no
/// notional to charge and pays 0, and one with no holdings (`den == 0`) pays `cap_bps`.
///
/// With `1 <= floor_bps <= cap_bps` (the bounds `LendingParams::validate` enforces) the result
/// is in `floor_bps..=cap_bps` for every position with debt: the curve alone floors to 0 at a
/// small `u`, the rate does not.
#[must_use]
pub fn rate_bps(apr_bps: u32, k: u8, cap_bps: u32, floor_bps: u32, num: u128, den: u128) -> u32 {
    if num == 0 {
        return 0;
    }
    if den == 0 {
        return cap_bps;
    }
    curve_bps(apr_bps, k, cap_bps, num, den).max(floor_bps)
}

/// The four shared, root-settable parameters of the rate (`LendingParams::rate_terms`), as the
/// arithmetic reads them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RateCurve {
    /// The yearly rate at `u = 1`, in bps.
    pub apr_bps: u32,
    /// The exponent `k`.
    pub k: u8,
    /// The cap of the yearly rate, in bps.
    pub cap_bps: u32,
    /// The least yearly rate of a position with debt, in bps.
    pub floor_bps: u32,
}

impl RateCurve {
    /// The yearly rate in bps at debt `num` over holdings `den` (see [`rate_bps`]).
    #[must_use]
    pub fn rate(&self, num: u128, den: u128) -> u32 {
        rate_bps(self.apr_bps, self.k, self.cap_bps, self.floor_bps, num, den)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const APR: u32 = 2_000;
    const CAP: u32 = 10_000;

    #[test]
    fn no_debt_pays_nothing_and_no_holdings_pay_the_cap() {
        for k in 1..=8 {
            assert_eq!(curve_bps(APR, k, CAP, 0, 1), 0);
            // No debt wins over no holdings.
            assert_eq!(curve_bps(APR, k, CAP, 0, 0), 0);
            assert_eq!(curve_bps(APR, k, CAP, 1, 0), CAP);
            assert_eq!(curve_bps(APR, k, CAP, u128::MAX, 0), CAP);
        }
    }

    #[test]
    fn at_u_of_one_the_rate_is_the_base_apr() {
        for k in 1..=8 {
            assert_eq!(curve_bps(APR, k, CAP, 1, 1), APR);
            assert_eq!(curve_bps(APR, k, CAP, u128::MAX, u128::MAX), APR);
        }
    }

    #[test]
    fn the_rate_is_apr_times_u_to_the_k() {
        // u = 1/2: 2,000 / 2^k.
        assert_eq!(curve_bps(APR, 1, CAP, 1, 2), 1_000);
        assert_eq!(curve_bps(APR, 2, CAP, 1, 2), 500);
        assert_eq!(curve_bps(APR, 3, CAP, 1, 2), 250);
        // u = 3/2: 2,000 * 1.5^k.
        assert_eq!(curve_bps(APR, 1, CAP, 3, 2), 3_000);
        assert_eq!(curve_bps(APR, 2, CAP, 3, 2), 4_500);
        assert_eq!(curve_bps(APR, 3, CAP, 3, 2), 6_750);
        // u = 2, k = 2: 8,000, still under the cap.
        assert_eq!(curve_bps(APR, 2, CAP, 2, 1), 8_000);
    }

    #[test]
    fn the_rate_stops_at_the_cap() {
        // u = 3, k = 2: 18,000 uncapped.
        assert_eq!(curve_bps(APR, 2, CAP, 3, 1), CAP);
        // Reached exactly: 2,000 * 5 = 10,000.
        assert_eq!(curve_bps(APR, 1, CAP, 5, 1), CAP);
        // One under the cap is not the cap.
        assert_eq!(curve_bps(APR, 1, CAP, 9_999, 2_000), 9_999);
        // The early stop is final: at u = 3 the first step reaches a cap of 6,000 and the
        // rate stays there for every exponent.
        for k in 1..=8 {
            assert_eq!(curve_bps(APR, k, 6_000, 3, 1), 6_000);
        }
        // A cap equal to the base rate caps every u >= 1.
        assert_eq!(curve_bps(APR, 2, APR, 1, 1), APR);
        assert_eq!(curve_bps(APR, 2, APR, 7, 3), APR);
        assert_eq!(curve_bps(APR, 2, APR, 1, 2), 500);
    }

    #[test]
    fn every_step_floors() {
        // 7 * 2/3 = 4.67 -> 4, then 4 * 2/3 = 2.67 -> 2; one division at the end would give 3.
        assert_eq!(curve_bps(7, 2, CAP, 2, 3), 2);
        // A rate that floors to zero stays zero.
        assert_eq!(curve_bps(1, 8, CAP, 9, 10), 0);
    }

    #[test]
    fn no_operand_overflows() {
        // floor(r * (2^128 - 1) / 2^127) = 2r - 1 for a small r.
        let mut expected = 10_000u32;
        for k in 1..=8 {
            expected = expected.saturating_mul(2).saturating_sub(1).min(1_000_000);
            assert_eq!(
                curve_bps(10_000, k, 1_000_000, u128::MAX, 1 << 127),
                expected
            );
            assert_eq!(curve_bps(10_000, k, 1_000_000, u128::MAX, 1), 1_000_000);
            assert_eq!(curve_bps(u32::MAX, k, u32::MAX, u128::MAX, 1), u32::MAX);
        }
    }

    #[test]
    fn scaling_debt_and_holdings_together_keeps_the_rate() {
        for (num, den) in [(3u128, 2u128), (17, 10), (1, 3), (5, 4)] {
            for k in 1..=8 {
                let one = curve_bps(APR, k, 1_000_000, num, den);
                for scale in [10u128, 1_000, 1_000_000_007] {
                    assert_eq!(curve_bps(APR, k, 1_000_000, num * scale, den * scale), one);
                }
            }
        }
    }

    #[test]
    fn the_floor_lifts_every_position_with_debt() {
        const FLOOR: u32 = 150;
        // u = 1/100, k = 2: the curve alone floors to 0.
        assert_eq!(curve_bps(APR, 2, CAP, 1, 100), 0);
        assert_eq!(rate_bps(APR, 2, CAP, FLOOR, 1, 100), FLOOR);
        // The smallest debt against the largest holdings still pays the floor.
        assert_eq!(rate_bps(APR, 8, CAP, 1, 1, u128::MAX), 1);
        // Above the floor the curve is the rate.
        assert_eq!(rate_bps(APR, 2, CAP, FLOOR, 1, 2), 500);
        assert_eq!(rate_bps(APR, 2, CAP, FLOOR, 3, 1), CAP);
        // A floor equal to the base rate is a flat rate up to u = 1.
        assert_eq!(rate_bps(APR, 2, CAP, APR, 1, 2), APR);
        assert_eq!(rate_bps(APR, 2, CAP, APR, 1, 1), APR);
        assert_eq!(rate_bps(APR, 2, CAP, APR, 3, 2), 4_500);
        // A floor equal to the cap is a flat rate for every position with debt.
        assert_eq!(rate_bps(APR, 2, CAP, CAP, 1, 1_000), CAP);
    }

    #[test]
    fn the_floor_does_not_charge_a_position_without_debt() {
        for k in 1..=8 {
            assert_eq!(rate_bps(APR, k, CAP, APR, 0, 1), 0);
            assert_eq!(rate_bps(APR, k, CAP, APR, 0, 0), 0);
            // No holdings: the cap, as without a floor.
            assert_eq!(rate_bps(APR, k, CAP, APR, 1, 0), CAP);
        }
    }

    #[test]
    fn the_rate_is_the_curve_lifted_to_the_floor() {
        for (num, den) in [(1u128, 100u128), (1, 3), (9, 10), (1, 1), (3, 2), (50, 1)] {
            for k in 1..=8 {
                for floor in [1u32, 150, APR, CAP] {
                    let curve = RateCurve {
                        apr_bps: APR,
                        k,
                        cap_bps: CAP,
                        floor_bps: floor,
                    };
                    let rate = curve.rate(num, den);
                    assert_eq!(rate, curve_bps(APR, k, CAP, num, den).max(floor));
                    assert!((floor..=CAP).contains(&rate));
                }
            }
        }
    }
}
