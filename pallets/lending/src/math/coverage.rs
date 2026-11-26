//! Opening coverage (SPEC 1.1 and P2 for shorts, 7.1.3 and L2 for longs): each position's
//! pro-rata share of unwinding the whole book against a stressed pool, with swap fees.

use super::{
    PriceRefs,
    curve::{Curve, PoolState},
    fixed::{BPS, mul_div_ceil, mul_div_floor, pro_rata_floor, scale_price_floor},
};

/// Book-aware short stress in bps (SPEC P2):
/// `Stress_eff = Stress * (1 + slope * Q / X) = floor(stress * (1e4 * X + slope * Q) / (1e4 * X))`
/// with `Q` the short book and `X` the pool alpha. `None` (infinite) for an empty pool.
#[must_use]
pub fn stress_eff_bps(stress_bps: u32, slope_bps: u32, book: u64, pool_alpha: u64) -> Option<u128> {
    let base = BPS.saturating_mul(u128::from(pool_alpha));
    let lifted = base.saturating_add(u128::from(slope_bps).saturating_mul(u128::from(book)));
    mul_div_floor(u128::from(stress_bps), lifted, base)
}

/// Short coverage need (SPEC 1.1): `ceil(BuyCost(book | pumped pool) * q_pos / book)`, the pool
/// pumped to `Stress_eff * max(spot, TWAP_median, SMA30)` and the buy-back quoted with fees.
/// The position is covered while `C + P >= need`.
///
/// `book` is `SyntheticOutstanding` with this position counted. `None` means the whole-book
/// buy-back is infinite (the book is at least the pumped pool's alpha), so the open fails.
#[must_use]
pub fn short_need<C: Curve>(
    pool: &PoolState,
    refs: &PriceRefs,
    q_pos: u64,
    book: u64,
    stress_bps: u32,
    slope_bps: u32,
) -> Option<u64> {
    if book == 0 {
        return Some(0);
    }
    let stress = stress_eff_bps(stress_bps, slope_bps, book, pool.alpha)?;
    let reference = refs.spot.max(refs.twap).max(refs.sma);
    let target = scale_price_floor(reference, stress, BPS)?;
    let pumped = C::pumped(pool, target)?;
    let book_cost = C::tao_in_for_alpha(&pumped, book)?;
    let share = mul_div_ceil(book_cost, u128::from(q_pos), u128::from(book))?;
    u64::try_from(share).ok()
}

/// Long coverage need (SPEC 7.1.3):
/// `max(0, D_pos - floor(SellProceeds(book | dumped pool) * a_pos / book))`, the pool dumped to
/// `min(spot, TWAP_median) / m` and the sale quoted with fees. The position is covered while
/// `C >= need`.
///
/// `book` is `LongLockedAlpha` with this position counted. When the dumped pool is not
/// representable the sale is counted as worthless, so the need saturates at `D_pos`.
#[must_use]
pub fn long_need<C: Curve>(
    pool: &PoolState,
    refs: &PriceRefs,
    debt: u64,
    a_pos: u64,
    book: u64,
    stress_bps: u32,
) -> u64 {
    let reference = refs.spot.min(refs.twap);
    let book_proceeds = scale_price_floor(reference, BPS, u128::from(stress_bps))
        .and_then(|target| C::dumped(pool, target))
        .map_or(0, |dumped| C::out_tao_for_alpha(&dumped, book));
    debt.saturating_sub(pro_rata_floor(book_proceeds, a_pos, book))
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

    fn flat_refs() -> PriceRefs {
        let spot = Cp::spot(&POOL);
        PriceRefs {
            spot,
            twap: spot,
            sma: spot,
        }
    }

    #[test]
    fn stress_grows_with_the_book() {
        assert_eq!(stress_eff_bps(30_000, 20_000, 0, 1_000), Some(30_000));
        // Q/X = 10 %: 3 * (1 + 2 * 0.1) = 3.6
        assert_eq!(stress_eff_bps(30_000, 20_000, 100, 1_000), Some(36_000));
        assert_eq!(stress_eff_bps(30_000, 0, 100, 1_000), Some(30_000));
        // floor: 3 * (1 + 2/3) = 5 exactly; 3 * (1 + 2 * 1/7) = 3.857142...
        assert_eq!(stress_eff_bps(30_000, 20_000, 1, 7), Some(38_571));
        assert_eq!(stress_eff_bps(30_000, 20_000, 1, 0), None);
    }

    #[test]
    fn short_need_is_zero_without_a_book_or_a_position() {
        assert_eq!(
            short_need::<Cp>(&POOL, &flat_refs(), 0, 0, 30_000, 20_000),
            Some(0)
        );
        assert_eq!(
            short_need::<Cp>(&POOL, &flat_refs(), 0, RAO, 30_000, 20_000),
            Some(0)
        );
    }

    #[test]
    fn short_need_is_infinite_once_the_book_outgrows_the_pumped_pool() {
        // Pumped 3x+ the pool holds < X / sqrt(3) alpha, so a book of X / 2 cannot be bought.
        let book = POOL.alpha / 2;
        assert_eq!(
            short_need::<Cp>(&POOL, &flat_refs(), book, book, 30_000, 20_000),
            None
        );
    }

    #[test]
    fn short_need_uses_the_highest_reference_and_rounds_shares_up() {
        let refs = flat_refs();
        let book = POOL.alpha / 100;
        let base = short_need::<Cp>(&POOL, &refs, book, book, 30_000, 20_000).unwrap();
        // At least Stress x the spot value of the book.
        assert!(base > 3 * (POOL.tao / 100));
        let high = Price::from_bits(refs.spot.to_bits() * 2);
        for lifted in [
            PriceRefs { twap: high, ..refs },
            PriceRefs { sma: high, ..refs },
        ] {
            assert!(short_need::<Cp>(&POOL, &lifted, book, book, 30_000, 20_000).unwrap() > base);
        }
        let low = Price::from_bits(refs.spot.to_bits() / 2);
        let lowered = PriceRefs {
            twap: low,
            sma: low,
            ..refs
        };
        assert_eq!(
            short_need::<Cp>(&POOL, &lowered, book, book, 30_000, 20_000),
            Some(base)
        );
        // Three equal thirds each round up, so together they cover at least the whole book.
        let third = short_need::<Cp>(&POOL, &refs, book / 3, book, 30_000, 20_000).unwrap();
        assert!(third * 3 >= base - 3 && third >= base / 3);
    }

    #[test]
    fn long_need_is_the_debt_without_a_book_or_a_usable_dump() {
        let refs = flat_refs();
        assert_eq!(
            long_need::<Cp>(&POOL, &refs, 7 * RAO, 0, 0, 20_000),
            7 * RAO
        );
        assert_eq!(long_need::<Cp>(&POOL, &refs, 7 * RAO, RAO, RAO, 0), 7 * RAO);
        let wide = PoolState {
            tao: u64::MAX,
            alpha: u64::MAX,
        };
        let one = Price::from_bits(1 << 64);
        let wide_refs = PriceRefs {
            spot: one,
            twap: one,
            sma: one,
        };
        assert_eq!(long_need::<Cp>(&wide, &wide_refs, 7, 5, 5, 40_000), 7);
    }

    #[test]
    fn long_need_uses_the_lowest_reference_and_never_goes_negative() {
        let refs = flat_refs();
        let book = POOL.alpha / 100;
        let debt = POOL.tao / 100;
        let base = long_need::<Cp>(&POOL, &refs, debt, book, book, 20_000);
        // Dumped 2x, the book sells for a little under half its spot value.
        assert!(base > debt / 2 && base < debt * 6 / 10);
        let low = Price::from_bits(refs.spot.to_bits() / 2);
        assert!(
            long_need::<Cp>(
                &POOL,
                &PriceRefs { twap: low, ..refs },
                debt,
                book,
                book,
                20_000
            ) > base
        );
        let low_sma = PriceRefs { sma: low, ..refs };
        assert_eq!(
            long_need::<Cp>(&POOL, &low_sma, debt, book, book, 20_000),
            base
        );
        assert_eq!(long_need::<Cp>(&POOL, &refs, 1, book, book, 20_000), 0);
    }
}
