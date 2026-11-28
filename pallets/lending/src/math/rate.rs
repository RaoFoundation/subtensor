//! The stretch-priced interest rate (SPEC section 1 item 2, P3, P5):
//! `rate = min(cap, apr * max(1, u^k))`, with `u = num / den` a position's debt over its
//! holdings at the slow price. A position whose holdings cover its debt (`u <= 1`) pays the
//! flat base APR; an underwater one pays more, up to the cap.
//!
//! A short has `num = floor(q * min(TWAP_median, SMA30_rank))` and `den = C + P`; a long has
//! `num = D` and `den = floor(A * max(TWAP_median, SMA30_rank)) + C`. Both are linear in the
//! position's size, so the rate is the same for a position and for any pro-rata piece of it
//! (SPEC I23).

use super::fixed::mul_div_floor;

/// The yearly rate in bps of a position with debt `num` and holdings `den`, computed exactly
/// as the SPEC writes it:
///
/// ```text
/// if num <= den { r = apr_bps }
/// else { r = apr_bps; repeat k times { r = floor(r * num / den); if r >= cap_bps { r = cap_bps; stop } } }
/// r = min(max(r, apr_bps), cap_bps)
/// ```
///
/// So no debt (`num == 0`, also with `den == 0`) pays the base APR, and debt against no
/// holdings (`den == 0`) pays the cap. `floor` in each step is integer division, and the early
/// stop at the cap is part of the definition; the reference model (`model/v21`, with the floor
/// at apr on) computes the same integers. The base APR is also the floor of the result: the
/// final `max(r, apr)`. Each step is a 256-bit product, so no operand overflows; a step whose
/// result exceeds `u128` is above every cap. With `apr_bps <= cap_bps` (the bound
/// `LendingParams::validate` enforces) the result is in `apr_bps..=cap_bps`.
#[must_use]
pub fn rate_bps(apr_bps: u32, k: u8, cap_bps: u32, num: u128, den: u128) -> u32 {
    let apr = u128::from(apr_bps);
    let cap = u128::from(cap_bps);
    let mut rate = apr;
    if num > den {
        for _ in 0..k {
            match mul_div_floor(rate, num, den) {
                Some(next) if next < cap => rate = next,
                // The cap is reached, or the step is not representable: `den == 0` or a
                // result above `u128`, both above every cap.
                _ => {
                    rate = cap;
                    break;
                }
            }
        }
    }
    u32::try_from(rate.max(apr).min(cap)).unwrap_or(cap_bps)
}

/// The three shared, root-settable parameters of the rate (`LendingParams::rate_curve`), as
/// the arithmetic reads them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RateCurve {
    /// The yearly rate of every position with `u <= 1`, and the base of the curve above, in
    /// bps.
    pub apr_bps: u32,
    /// The exponent `k`.
    pub k: u8,
    /// The cap of the yearly rate, in bps.
    pub cap_bps: u32,
}

impl RateCurve {
    /// The yearly rate in bps at debt `num` over holdings `den` (see [`rate_bps`]).
    #[must_use]
    pub fn rate(&self, num: u128, den: u128) -> u32 {
        rate_bps(self.apr_bps, self.k, self.cap_bps, num, den)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const APR: u32 = 2_000;
    const CAP: u32 = 20_000;

    #[test]
    fn no_debt_pays_the_base_apr_and_no_holdings_pay_the_cap() {
        for k in 1..=8 {
            assert_eq!(rate_bps(APR, k, CAP, 0, 1), APR);
            // No debt wins over no holdings: `0 <= 0`.
            assert_eq!(rate_bps(APR, k, CAP, 0, 0), APR);
            assert_eq!(rate_bps(APR, k, CAP, 1, 0), CAP);
            assert_eq!(rate_bps(APR, k, CAP, u128::MAX, 0), CAP);
        }
    }

    #[test]
    fn a_covered_position_pays_the_flat_base_apr() {
        for k in 1..=8 {
            for (num, den) in [
                (1u128, 1u128),
                (1, 2),
                (1, 100),
                (1, u128::MAX),
                (u128::MAX - 1, u128::MAX),
                (u128::MAX, u128::MAX),
            ] {
                assert_eq!(rate_bps(APR, k, CAP, num, den), APR);
            }
        }
    }

    #[test]
    fn above_u_of_one_the_rate_is_apr_times_u_to_the_k() {
        // u = 3/2: 2,000 * 1.5^k.
        assert_eq!(rate_bps(APR, 1, CAP, 3, 2), 3_000);
        assert_eq!(rate_bps(APR, 2, CAP, 3, 2), 4_500);
        assert_eq!(rate_bps(APR, 3, CAP, 3, 2), 6_750);
        // u = 2, k = 2: 8,000, still under the cap.
        assert_eq!(rate_bps(APR, 2, CAP, 2, 1), 8_000);
        // The launch curve is linear in u: k = 1.
        assert_eq!(rate_bps(APR, 1, CAP, 5, 1), 10_000);
        assert_eq!(rate_bps(APR, 1, CAP, 1_000_000_001, 1_000_000_000), APR);
        assert_eq!(rate_bps(APR, 1, CAP, 1_001, 1_000), 2_002);
    }

    #[test]
    fn the_rate_stops_at_the_cap() {
        // u = 4, k = 2: 32,000 uncapped.
        assert_eq!(rate_bps(APR, 2, CAP, 4, 1), CAP);
        // Reached exactly: 2,000 * 10 = 20,000.
        assert_eq!(rate_bps(APR, 1, CAP, 10, 1), CAP);
        // One under the cap is not the cap.
        assert_eq!(rate_bps(APR, 1, CAP, 19_999, 2_000), 19_999);
        // The early stop is final: at u = 3 the first step reaches a cap of 6,000 and the
        // rate stays there for every exponent.
        for k in 1..=8 {
            assert_eq!(rate_bps(APR, k, 6_000, 3, 1), 6_000);
        }
        // A cap equal to the base rate is a flat rate for every position.
        assert_eq!(rate_bps(APR, 2, APR, 1, 1), APR);
        assert_eq!(rate_bps(APR, 2, APR, 7, 3), APR);
        assert_eq!(rate_bps(APR, 2, APR, 1, 2), APR);
    }

    #[test]
    fn every_step_floors() {
        // 7 * 4/3 = 9.33 -> 9, then 9 * 4/3 = 12; one division at the end would give
        // floor(7 * 16 / 9) = 12 too, so take three steps: 12 * 4/3 = 16 against
        // floor(7 * 64 / 27) = 16, and four: 16 * 4/3 = 21.33 -> 21 against
        // floor(7 * 256 / 81) = 22.
        assert_eq!(rate_bps(7, 2, CAP, 4, 3), 12);
        assert_eq!(rate_bps(7, 3, CAP, 4, 3), 16);
        assert_eq!(rate_bps(7, 4, CAP, 4, 3), 21);
        // A step that floors back to the base rate stays at it: 1 * 10/9 = 1.11 -> 1.
        assert_eq!(rate_bps(1, 8, CAP, 10, 9), 1);
    }

    #[test]
    fn no_operand_overflows() {
        // floor(r * (2^128 - 1) / 2^127) = 2r - 1 for a small r.
        let mut expected = 10_000u32;
        for k in 1..=8 {
            expected = expected.saturating_mul(2).saturating_sub(1).min(1_000_000);
            assert_eq!(
                rate_bps(10_000, k, 1_000_000, u128::MAX, 1 << 127),
                expected
            );
            assert_eq!(rate_bps(10_000, k, 1_000_000, u128::MAX, 1), 1_000_000);
            assert_eq!(rate_bps(u32::MAX, k, u32::MAX, u128::MAX, 1), u32::MAX);
        }
    }

    #[test]
    fn scaling_debt_and_holdings_together_keeps_the_rate() {
        for (num, den) in [(3u128, 2u128), (17, 10), (1, 3), (5, 4)] {
            for k in 1..=8 {
                let one = rate_bps(APR, k, 1_000_000, num, den);
                for scale in [10u128, 1_000, 1_000_000_007] {
                    assert_eq!(rate_bps(APR, k, 1_000_000, num * scale, den * scale), one);
                }
            }
        }
    }

    #[test]
    fn the_rate_is_between_the_base_apr_and_the_cap() {
        for (num, den) in [
            (0u128, 0u128),
            (0, 7),
            (1, 100),
            (1, 3),
            (9, 10),
            (1, 1),
            (3, 2),
            (50, 1),
            (1, 0),
        ] {
            for k in 1..=8 {
                for (apr, cap) in [(1u32, 1u32), (1, 1_000_000), (APR, APR), (APR, CAP)] {
                    let curve = RateCurve {
                        apr_bps: apr,
                        k,
                        cap_bps: cap,
                    };
                    assert!((apr..=cap).contains(&curve.rate(num, den)));
                }
            }
        }
    }
}
