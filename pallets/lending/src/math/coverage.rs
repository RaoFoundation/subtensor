//! Opening coverage (SPEC 1.1 and P2 for shorts, 7.1.3 and L2 for longs): each position's
//! pro-rata share of unwinding the whole book against a stressed pool, with swap fees.

use super::{
    PriceRefs,
    curve::{Curve, PoolState},
    fixed::{BPS, mul_div_ceil, pro_rata_floor, scale_price_floor},
};

/// Short coverage need (SPEC 1.1): `ceil(BuyCost(book | pumped pool) * q_pos / book)`, the pool
/// pumped to `Stress * max(spot, SMA30_rank)` and the buy-back quoted with fees. `Stress` is the
/// same flat multiple for every book size: a larger book costs more per position only through
/// the slippage of the whole-book buy-back. The TWAP median is not part of the short reference
/// (SPEC P2).
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
) -> Option<u64> {
    if book == 0 {
        return Some(0);
    }
    let reference = refs.spot.max(refs.sma);
    let target = scale_price_floor(reference, u128::from(stress_bps), BPS)?;
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
    const POOL: PoolState = PoolState::model(1_000 * RAO, 480_000 * RAO);

    fn flat_refs() -> PriceRefs {
        let spot = Cp::spot(&POOL);
        PriceRefs {
            spot,
            twap: spot,
            sma: spot,
        }
    }

    #[test]
    fn short_need_is_zero_without_a_book_or_a_position() {
        assert_eq!(short_need::<Cp>(&POOL, &flat_refs(), 0, 0, 30_000), Some(0));
        assert_eq!(
            short_need::<Cp>(&POOL, &flat_refs(), 0, RAO, 30_000),
            Some(0)
        );
    }

    #[test]
    fn short_need_is_infinite_once_the_book_outgrows_the_pumped_pool() {
        // Pumped 3x the pool holds X / sqrt(3) alpha (57.7 % of X), so a book of 60 % of X
        // cannot be bought.
        let book = POOL.alpha / 5 * 3;
        assert_eq!(
            short_need::<Cp>(&POOL, &flat_refs(), book, book, 30_000),
            None
        );
    }

    #[test]
    fn short_need_uses_the_higher_of_spot_and_sma_and_rounds_shares_up() {
        let refs = flat_refs();
        let book = POOL.alpha / 100;
        let base = short_need::<Cp>(&POOL, &refs, book, book, 30_000).unwrap();
        // At least Stress x the spot value of the book.
        assert!(base > 3 * (POOL.tao / 100));
        let high = Price::from_bits(refs.spot.to_bits() * 2);
        let lifted = PriceRefs { sma: high, ..refs };
        assert!(short_need::<Cp>(&POOL, &lifted, book, book, 30_000).unwrap() > base);
        let low = Price::from_bits(refs.spot.to_bits() / 2);
        let lowered = PriceRefs { sma: low, ..refs };
        assert_eq!(
            short_need::<Cp>(&POOL, &lowered, book, book, 30_000),
            Some(base)
        );
        // The TWAP median is not a short reference: it moves the need in neither direction.
        for twap in [high, low] {
            let moved = PriceRefs { twap, ..refs };
            assert_eq!(
                short_need::<Cp>(&POOL, &moved, book, book, 30_000),
                Some(base)
            );
        }
        // An empty pool has no pumped state.
        let empty = PoolState::model(0, 0);
        assert_eq!(short_need::<Cp>(&empty, &refs, 1, 1, 30_000), None);
        // Three equal thirds each round up, so together they cover at least the whole book.
        let third = short_need::<Cp>(&POOL, &refs, book / 3, book, 30_000).unwrap();
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
        let wide = PoolState::model(u64::MAX, u64::MAX);
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
