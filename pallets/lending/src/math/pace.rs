//! Deleverage pacing (SPEC P7, P17/P22, 7.1.6): the pause band and the per-block price-impact
//! budgets. The short up-budget and the long down-budget are metered separately.

use super::{
    PriceRefs,
    curve::{Curve, PoolState},
    fixed::{BPS, Price, price, scale_price_ceil, scale_price_floor},
};

/// `spot * 1e4 <= band_bps * reference`, i.e. `spot <= floor(reference * band_bps / 1e4)`.
/// A bound above every representable price always holds.
fn at_most_band(spot: Price, reference: Price, band_bps: u32) -> bool {
    scale_price_floor(reference, u128::from(band_bps), BPS).is_none_or(|limit| spot <= limit)
}

/// Short deleverage buys run only while `spot <= pause_twap * TWAP_median` and
/// `spot <= pause_sma * SMA30` (SPEC P17/P22).
#[must_use]
pub fn short_buys_allowed(refs: &PriceRefs, pause_twap_bps: u32, pause_sma_bps: u32) -> bool {
    at_most_band(refs.spot, refs.twap, pause_twap_bps)
        && at_most_band(refs.spot, refs.sma, pause_sma_bps)
}

/// Long deleverage sells run only while `spot >= TWAP_median / pause_twap` and
/// `spot >= SMA30 / pause_sma` (SPEC 7.1.6), the mirror of [`short_buys_allowed`].
#[must_use]
pub fn long_sells_allowed(refs: &PriceRefs, pause_twap_bps: u32, pause_sma_bps: u32) -> bool {
    at_most_band(refs.twap, refs.spot, pause_twap_bps)
        && at_most_band(refs.sma, refs.spot, pause_sma_bps)
}

/// Highest spot the up-budget may reach this block: `floor(spot * (1e4 + pace) / 1e4)` (P7).
/// Saturates at the largest price.
#[must_use]
pub fn up_price_cap(spot: Price, pace_bps: u32) -> Price {
    let num = BPS.saturating_add(u128::from(pace_bps));
    scale_price_floor(spot, num, BPS).unwrap_or(Price::max_value())
}

/// Lowest spot the down-budget may reach this block: `ceil(spot * 1e4 / (1e4 + pace))` (P7).
#[must_use]
pub fn down_price_floor(spot: Price, pace_bps: u32) -> Price {
    let den = BPS.saturating_add(u128::from(pace_bps));
    scale_price_ceil(spot, BPS, den).unwrap_or(spot)
}

/// Largest `a` in `[0, max]` with `fits(a)`, by the model's bisection (`fits(0)` is assumed).
fn largest_fitting(max: u64, fits: impl Fn(u64) -> bool) -> u64 {
    let (mut lo, mut hi) = (0u64, max);
    while lo < hi {
        let mid = lo.saturating_add(hi.saturating_sub(lo).div_ceil(2));
        if fits(mid) {
            lo = mid;
        } else {
            hi = mid.saturating_sub(1);
        }
    }
    lo
}

/// Largest alpha bought by one swap such that the post-swap spot is at most `cap`
/// (`model/v201/pool.py::alpha_for_up_impact`).
#[must_use]
pub fn alpha_for_up_impact<C: Curve>(pool: &PoolState, cap: Price) -> u64 {
    largest_fitting(pool.alpha.saturating_sub(1), |a| {
        C::spot_after_buy(pool, a).is_some_and(|spot| spot <= cap)
    })
}

/// Largest alpha `a <= a_max` whose sale leaves spot at least `floor`, measured after up to
/// `debt` of the proceeds is repaid into the pool (SPEC 7.1.6; the model's
/// `_alpha_for_down_impact`, net of the repay leg).
#[must_use]
pub fn alpha_for_down_impact<C: Curve>(
    pool: &PoolState,
    floor: Price,
    a_max: u64,
    debt: u64,
) -> u64 {
    largest_fitting(a_max, |a| {
        C::after_sell(pool, a).is_some_and(|(after, tao_out)| {
            let tao = after.tao.saturating_add(tao_out.min(debt));
            price(tao, u128::from(after.alpha)) >= floor
        })
    })
}

/// Alpha the short deleverage may buy this block: at most `pace_bps` of upward price impact.
#[must_use]
pub fn up_budget<C: Curve>(pool: &PoolState, pace_bps: u32) -> u64 {
    alpha_for_up_impact::<C>(pool, up_price_cap(C::spot(pool), pace_bps))
}

/// Alpha the long deleverage may sell this block out of `a_max`: at most `pace_bps` of
/// downward price impact after repaying up to `debt`.
#[must_use]
pub fn down_budget<C: Curve>(pool: &PoolState, pace_bps: u32, a_max: u64, debt: u64) -> u64 {
    alpha_for_down_impact::<C>(pool, down_price_floor(C::spot(pool), pace_bps), a_max, debt)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::arithmetic_side_effects, clippy::unwrap_used)]
    use super::*;
    use crate::math::{curve::ConstantProduct as Cp, fixed::Q64};

    const RAO: u64 = 1_000_000_000;
    const POOL: PoolState = PoolState {
        tao: 1_000 * RAO,
        alpha: 480_000 * RAO,
    };

    fn refs(spot: u128, twap: u128, sma: u128) -> PriceRefs {
        PriceRefs {
            spot: Price::from_bits(spot),
            twap: Price::from_bits(twap),
            sma: Price::from_bits(sma),
        }
    }

    #[test]
    fn short_buys_pause_above_either_band() {
        let one = 10_000 * Q64;
        assert!(short_buys_allowed(
            &refs(one * 125 / 100, one, one),
            12_500,
            12_500
        ));
        assert!(!short_buys_allowed(
            &refs(one * 125 / 100 + 1, one, one),
            12_500,
            12_500
        ));
        assert!(!short_buys_allowed(
            &refs(one * 125 / 100, one, one - 1),
            12_500,
            12_500
        ));
        assert!(!short_buys_allowed(
            &refs(one * 125 / 100, one - 1, one),
            12_500,
            12_500
        ));
        assert!(short_buys_allowed(&refs(0, 0, 0), 12_500, 12_500));
    }

    #[test]
    fn long_sells_pause_below_either_band() {
        let one = 10_000 * Q64;
        assert!(long_sells_allowed(
            &refs(one, one * 125 / 100, one),
            12_500,
            12_500
        ));
        assert!(!long_sells_allowed(
            &refs(one, one * 125 / 100 + 1, one),
            12_500,
            12_500
        ));
        assert!(!long_sells_allowed(
            &refs(one, one, one * 125 / 100 + 1),
            12_500,
            12_500
        ));
        assert!(long_sells_allowed(
            &refs(one, one, one * 125 / 100),
            12_500,
            12_500
        ));
    }

    #[test]
    fn bands_hold_at_the_top_of_the_price_range() {
        let max = u128::MAX;
        assert!(short_buys_allowed(&refs(max, max, max), 12_500, 12_500));
        assert!(long_sells_allowed(&refs(max, max, max), 12_500, 12_500));
    }

    #[test]
    fn price_limits_round_towards_less_impact() {
        let spot = Price::from_bits(20_001);
        assert_eq!(up_price_cap(spot, 5).to_bits(), 20_011);
        assert_eq!(down_price_floor(spot, 5).to_bits(), 19_992);
        assert_eq!(up_price_cap(Price::max_value(), 5), Price::max_value());
        assert!(down_price_floor(Price::max_value(), 5) < Price::max_value());
        assert_eq!(down_price_floor(Price::from_bits(0), 5).to_bits(), 0);
    }

    #[test]
    fn up_budget_is_the_largest_buy_within_the_cap() {
        let cap = up_price_cap(Cp::spot(&POOL), 5);
        let a = up_budget::<Cp>(&POOL, 5);
        assert!(a > 0);
        assert!(Cp::spot_after_buy(&POOL, a).unwrap() <= cap);
        assert!(Cp::spot_after_buy(&POOL, a + 1).unwrap() > cap);
        // 0.05 % of price is about 0.025 % of pool alpha.
        assert!(a > POOL.alpha / 4_100 && a < POOL.alpha / 3_900);
    }

    #[test]
    fn up_budget_is_zero_for_degenerate_pools() {
        assert_eq!(up_budget::<Cp>(&PoolState { tao: 0, alpha: 0 }, 5), 0);
        assert_eq!(up_budget::<Cp>(&PoolState { tao: RAO, alpha: 1 }, 5), 0);
        assert_eq!(alpha_for_up_impact::<Cp>(&POOL, Price::from_bits(0)), 0);
    }

    #[test]
    fn down_budget_is_the_largest_sale_within_the_floor() {
        let floor = down_price_floor(Cp::spot(&POOL), 5);
        let spot_after = |a: u64, debt: u64| {
            let (after, out) = Cp::after_sell(&POOL, a).unwrap();
            price(after.tao + out.min(debt), u128::from(after.alpha))
        };
        let a = down_budget::<Cp>(&POOL, 5, POOL.alpha, 0);
        assert!(a > 0 && spot_after(a, 0) >= floor && spot_after(a + 1, 0) < floor);
        // Repaying the proceeds keeps TAO in the pool, so the same floor admits a larger sale.
        let repaid = down_budget::<Cp>(&POOL, 5, POOL.alpha, u64::MAX);
        assert!(repaid > a && spot_after(repaid, u64::MAX) >= floor);
        assert!(spot_after(repaid + 1, u64::MAX) < floor);
    }

    #[test]
    fn down_budget_is_capped_by_the_alpha_on_offer() {
        assert_eq!(down_budget::<Cp>(&POOL, 5, 0, 0), 0);
        assert_eq!(down_budget::<Cp>(&POOL, 5, 7, 0), 7);
    }
}
