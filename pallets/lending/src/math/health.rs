//! Whole-book pro-rata health and the deleverage trigger/exit tests (SPEC 1.4, 7.1.5, P5;
//! exit rule v2.0.1). Health is fee-free and read on the live pool, scaled by TWAP/spot.

use super::{
    PriceRefs,
    curve::{Curve, PoolState},
    fixed::{BPS, mul_div_ceil, mul_div_floor, pro_rata_floor, saturate_u64},
    interest::{interest_for_blocks, short_notional},
};

/// The shared health settings read from `LendingParams` (SPEC 2.1).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HealthTerms {
    /// `apr_bps` (P3).
    pub apr_bps: u32,
    /// `min_interest_cover_blocks` (P5).
    pub min_interest_cover_blocks: u32,
    /// `equity_floor` in rao (P5).
    pub equity_floor: u64,
    /// `exit_hysteresis_bps` (P5, v2.0.1).
    pub exit_hysteresis_bps: u32,
}

/// Outcome of one health evaluation. Invariant: `may_exit` implies `!triggered`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HealthVerdict {
    /// `C + P - H_pos` (short) or `C + V_pos - D` (long). `None` when the short book's
    /// buy-back is infinite.
    pub equity_twap: Option<i128>,
    /// What the exit margin is measured against: `H_pos` (short) or `D` (long).
    pub at_risk: u64,
    /// Interest that `C` must cover: `min_interest_cover_blocks` of interest.
    pub cover: u64,
    /// The position must be (or stay) `Deleveraging`.
    pub triggered: bool,
    /// A `Deleveraging` position may return to `Open`.
    pub may_exit: bool,
}

/// `max(spot, 1)` in bits: the TWAP/spot scaling never divides by zero.
fn spot_divisor(refs: &PriceRefs) -> u128 {
    refs.spot.to_bits().max(1)
}

/// Short health cost (SPEC 1.4):
/// `H_pos = ceil(ceil(BuyCost(book | live pool) * q_pos / book) * TWAP / spot)`, fee-free.
///
/// `book` is `SyntheticOutstanding`; an empty book values the position standalone. `None`
/// means the cost is infinite (`book >= pool.alpha`): every short on the subnet is unhealthy.
#[must_use]
pub fn short_health_cost<C: Curve>(
    pool: &PoolState,
    refs: &PriceRefs,
    book: u64,
    q_pos: u64,
) -> Option<u64> {
    let share = if book == 0 {
        C::buy_cost_nofee(pool, q_pos)?
    } else {
        mul_div_ceil(
            C::buy_cost_nofee(pool, book)?,
            u128::from(q_pos),
            u128::from(book),
        )?
    };
    let scaled = mul_div_ceil(share, refs.twap.to_bits(), spot_divisor(refs))?;
    u64::try_from(scaled).ok()
}

/// Long health value (SPEC 7.1.5):
/// `V_pos = floor(floor(SellProceeds(book | live pool) * a_pos / book) * TWAP / spot)`, fee-free.
///
/// `book` is `LongLockedAlpha`; an empty book values the position standalone. Saturates at
/// `u64::MAX`.
#[must_use]
pub fn long_health_value<C: Curve>(
    pool: &PoolState,
    refs: &PriceRefs,
    book: u64,
    a_pos: u64,
) -> u64 {
    let share = if book == 0 {
        C::sell_proceeds_nofee(pool, a_pos)
    } else {
        pro_rata_floor(C::sell_proceeds_nofee(pool, book), a_pos, book)
    };
    saturate_u64(mul_div_floor(
        u128::from(share),
        refs.twap.to_bits(),
        spot_divisor(refs),
    ))
}

/// The v2.0.1 exit test (SPEC 1.4 "Exit"), given that the trigger test passed:
/// `(equity - floor) * 1e4 >= h * at_risk` and `C * 1e4 >= (1e4 + h) * cover`.
fn clears_exit_margin(
    equity: i128,
    at_risk: u64,
    collateral: u64,
    cover: u64,
    terms: &HealthTerms,
) -> bool {
    let h = u128::from(terms.exit_hysteresis_bps);
    let margin = equity
        .saturating_sub(i128::from(terms.equity_floor))
        .saturating_mul(10_000);
    let margin_need = i128::from(terms.exit_hysteresis_bps).saturating_mul(i128::from(at_risk));
    let cover_need = BPS.saturating_add(h).saturating_mul(u128::from(cover));
    margin >= margin_need && u128::from(collateral).saturating_mul(BPS) >= cover_need
}

/// Trigger and exit tests on already-computed terms (SPEC 1.4, 7.1.5, P5):
/// `triggered = equity < equity_floor || C < cover`, and `may_exit` as in the v2.0.1 exit rule.
/// With `h = 0` the exit test is exactly "not triggered". An infinite cost (`equity = None`)
/// is always triggered.
#[must_use]
pub fn verdict(
    equity_twap: Option<i128>,
    at_risk: u64,
    collateral: u64,
    cover: u64,
    terms: &HealthTerms,
) -> HealthVerdict {
    let healthy =
        equity_twap.filter(|eq| *eq >= i128::from(terms.equity_floor) && collateral >= cover);
    HealthVerdict {
        equity_twap,
        at_risk,
        cover,
        triggered: healthy.is_none(),
        may_exit: healthy
            .is_some_and(|eq| clears_exit_margin(eq, at_risk, collateral, cover, terms)),
    }
}

/// Health of a short owing `q_pos` alpha with locked proceeds `P` and collateral `C`
/// (SPEC 1.4): `Equity_TWAP = C + P - H_pos`, cover on notional `q * TWAP_median`.
#[must_use]
pub fn assess_short<C: Curve>(
    pool: &PoolState,
    refs: &PriceRefs,
    book: u64,
    q_pos: u64,
    proceeds: u64,
    collateral: u64,
    terms: &HealthTerms,
) -> HealthVerdict {
    let cost = short_health_cost::<C>(pool, refs, book, q_pos);
    let funds = i128::from(collateral).saturating_add(i128::from(proceeds));
    let equity = cost.map(|h| funds.saturating_sub(i128::from(h)));
    let notional = short_notional(q_pos, refs.twap);
    let cover = interest_for_blocks(notional, terms.apr_bps, terms.min_interest_cover_blocks);
    verdict(equity, cost.unwrap_or(u64::MAX), collateral, cover, terms)
}

/// Health of a long holding `a_pos` locked alpha against debt `D` with collateral `C`
/// (SPEC 7.1.5): `Equity_TWAP = C + V_pos - D`, cover on notional `D`.
#[must_use]
pub fn assess_long<C: Curve>(
    pool: &PoolState,
    refs: &PriceRefs,
    book: u64,
    a_pos: u64,
    debt: u64,
    collateral: u64,
    terms: &HealthTerms,
) -> HealthVerdict {
    let value = long_health_value::<C>(pool, refs, book, a_pos);
    let equity = i128::from(collateral)
        .saturating_add(i128::from(value))
        .saturating_sub(i128::from(debt));
    let cover = interest_for_blocks(debt, terms.apr_bps, terms.min_interest_cover_blocks);
    verdict(Some(equity), debt, collateral, cover, terms)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::arithmetic_side_effects, clippy::unwrap_used)]
    use super::*;
    use crate::math::{curve::ConstantProduct as Cp, fixed::Price};

    const RAO: u64 = 1_000_000_000;
    const POOL: PoolState = PoolState {
        tao: 1_000 * RAO,
        alpha: 480_000 * RAO,
    };
    const TERMS: HealthTerms = HealthTerms {
        apr_bps: 2_000,
        min_interest_cover_blocks: 7_200,
        equity_floor: 0,
        exit_hysteresis_bps: 500,
    };

    fn flat_refs() -> PriceRefs {
        let spot = Cp::spot(&POOL);
        PriceRefs {
            spot,
            twap: spot,
            sma: spot,
        }
    }

    #[test]
    fn zero_positions_cost_and_are_worth_nothing() {
        let refs = flat_refs();
        assert_eq!(short_health_cost::<Cp>(&POOL, &refs, RAO, 0), Some(0));
        assert_eq!(short_health_cost::<Cp>(&POOL, &refs, 0, 0), Some(0));
        assert_eq!(long_health_value::<Cp>(&POOL, &refs, RAO, 0), 0);
        assert_eq!(long_health_value::<Cp>(&POOL, &refs, 0, 0), 0);
    }

    #[test]
    fn a_lone_position_is_valued_standalone() {
        let refs = flat_refs();
        let q = 1_000 * RAO;
        let alone = u64::try_from(Cp::buy_cost_nofee(&POOL, q).unwrap()).unwrap();
        assert_eq!(short_health_cost::<Cp>(&POOL, &refs, q, q), Some(alone));
        assert_eq!(short_health_cost::<Cp>(&POOL, &refs, 0, q), Some(alone));
        let value = Cp::sell_proceeds_nofee(&POOL, q);
        assert_eq!(long_health_value::<Cp>(&POOL, &refs, q, q), value);
        assert_eq!(long_health_value::<Cp>(&POOL, &refs, 0, q), value);
    }

    #[test]
    fn splitting_a_book_does_not_lower_its_cost_or_raise_its_value() {
        let refs = flat_refs();
        let book = 48_000 * RAO;
        let whole = short_health_cost::<Cp>(&POOL, &refs, book, book).unwrap();
        let tenth = short_health_cost::<Cp>(&POOL, &refs, book, book / 10).unwrap();
        assert!(tenth * 10 >= whole);
        let value = long_health_value::<Cp>(&POOL, &refs, book, book);
        assert!(long_health_value::<Cp>(&POOL, &refs, book, book / 10) * 10 <= value);
    }

    #[test]
    fn health_scales_by_twap_over_spot_with_directed_rounding() {
        let spot = Cp::spot(&POOL);
        let refs = PriceRefs {
            spot,
            twap: Price::from_bits(spot.to_bits() * 3 / 2),
            sma: spot,
        };
        let q = 1_000 * RAO;
        let cost = u64::try_from(Cp::buy_cost_nofee(&POOL, q).unwrap()).unwrap();
        assert_eq!(
            short_health_cost::<Cp>(&POOL, &refs, q, q),
            Some((cost * 3).div_ceil(2))
        );
        let value = Cp::sell_proceeds_nofee(&POOL, q);
        let scaled = u128::from(value) * refs.twap.to_bits() / spot.to_bits();
        assert_eq!(
            u128::from(long_health_value::<Cp>(&POOL, &refs, q, q)),
            scaled
        );
    }

    #[test]
    fn a_book_as_large_as_the_pool_is_infinite_and_always_triggered() {
        let refs = flat_refs();
        assert_eq!(short_health_cost::<Cp>(&POOL, &refs, POOL.alpha, RAO), None);
        let v = assess_short::<Cp>(&POOL, &refs, POOL.alpha, RAO, u64::MAX, u64::MAX, &TERMS);
        assert_eq!(v.equity_twap, None);
        assert!(v.triggered && !v.may_exit);
    }

    #[test]
    fn trigger_is_equity_below_floor_or_collateral_below_cover() {
        let ok = verdict(Some(0), 100, 10, 10, &TERMS);
        assert!(!ok.triggered);
        assert!(verdict(Some(-1), 100, 10, 10, &TERMS).triggered);
        assert!(verdict(Some(0), 100, 9, 10, &TERMS).triggered);
        let floored = HealthTerms {
            equity_floor: RAO,
            ..TERMS
        };
        assert!(verdict(Some(i128::from(RAO) - 1), 100, 10, 10, &floored).triggered);
        assert!(!verdict(Some(i128::from(RAO)), 100, 10, 10, &floored).triggered);
    }

    #[test]
    fn short_at_1_04x_stays_deleveraging_and_at_1_05x_exits() {
        // H_pos = 1_000_000: C + P = 1.04 H is healthy but inside the margin; 1.05 H exits.
        let h_pos = 1_000_000u64;
        let stays = verdict(Some(40_000), h_pos, 10_000, 100, &TERMS);
        assert!(!stays.triggered && !stays.may_exit);
        let just_below = verdict(Some(49_999), h_pos, 10_000, 100, &TERMS);
        assert!(!just_below.triggered && !just_below.may_exit);
        let exits = verdict(Some(50_000), h_pos, 10_000, 100, &TERMS);
        assert!(!exits.triggered && exits.may_exit);
    }

    #[test]
    fn exit_needs_the_cover_margin_too() {
        // cover 100: C = 104 stays, C = 105 exits.
        assert!(!verdict(Some(50_000), 1_000_000, 104, 100, &TERMS).may_exit);
        assert!(verdict(Some(50_000), 1_000_000, 105, 100, &TERMS).may_exit);
    }

    #[test]
    fn exit_margin_sits_above_a_nonzero_floor() {
        let floored = HealthTerms {
            equity_floor: RAO,
            ..TERMS
        };
        let floor = i128::from(RAO);
        assert!(!verdict(Some(floor + 49_999), 1_000_000, 105, 100, &floored).may_exit);
        assert!(verdict(Some(floor + 50_000), 1_000_000, 105, 100, &floored).may_exit);
    }

    #[test]
    fn zero_hysteresis_exits_exactly_when_not_triggered() {
        let terms = HealthTerms {
            exit_hysteresis_bps: 0,
            ..TERMS
        };
        for (equity, collateral) in [(0, 10), (-1, 10), (0, 9), (5, 11)] {
            let v = verdict(Some(equity), 1_000, collateral, 10, &terms);
            assert_eq!(v.may_exit, !v.triggered);
        }
    }

    #[test]
    fn extreme_equity_never_overflows() {
        assert!(verdict(Some(i128::MAX), u64::MAX, u64::MAX, u64::MAX / 2, &TERMS).may_exit);
        let low = verdict(Some(i128::MIN), u64::MAX, u64::MAX, 0, &TERMS);
        assert!(low.triggered && !low.may_exit);
    }

    #[test]
    fn assess_short_at_the_exit_boundary() {
        let refs = flat_refs();
        let q = 1_000 * RAO;
        let h_pos = short_health_cost::<Cp>(&POOL, &refs, q, q).unwrap();
        let collateral = RAO;
        let at = |funds: u64| {
            assess_short::<Cp>(&POOL, &refs, q, q, funds - collateral, collateral, &TERMS)
        };
        let stays = at(h_pos * 104 / 100);
        assert_eq!(stays.at_risk, h_pos);
        assert_eq!(
            stays.cover,
            interest_for_blocks(short_notional(q, refs.twap), 2_000, 7_200)
        );
        assert!(!stays.triggered && !stays.may_exit);
        assert!(at((h_pos * 105).div_ceil(100)).may_exit);
        assert!(at(h_pos - 1).triggered);
        assert!(!at(h_pos).triggered);
    }

    #[test]
    fn assess_long_at_the_exit_boundary() {
        let refs = flat_refs();
        let a = 1_000 * RAO;
        let value = long_health_value::<Cp>(&POOL, &refs, a, a);
        let collateral = RAO;
        // C + V = k * D  <=>  D = (C + V) / k
        let at = |debt: u64| assess_long::<Cp>(&POOL, &refs, a, a, debt, collateral, &TERMS);
        let funds = collateral + value;
        let stays = at(funds * 100 / 104);
        assert_eq!(stays.at_risk, funds * 100 / 104);
        assert!(!stays.triggered && !stays.may_exit);
        assert!(at(funds * 100 / 105).may_exit);
        assert!(at(funds + 1).triggered);
        assert!(!at(funds).triggered);
        // Collateral below one day of interest triggers whatever the equity.
        let thin = assess_long::<Cp>(&POOL, &refs, a, a, 1_000 * RAO, 1, &TERMS);
        assert!(thin.triggered && thin.cover > 1);
    }
}
