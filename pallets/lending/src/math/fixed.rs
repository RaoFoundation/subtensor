//! Named rounding helpers on `u128` and the `U64F64` price convention.
//!
//! A price is rao TAO per rao alpha as `U64F64`, i.e. `floor(tao * 2^64 / alpha)` in its bits,
//! the same convention as the reference model (`model/v201/pool.py::price`).

use sp_arithmetic::{
    Rounding,
    helpers_128bit::{multiply_by_rational_with_rounding, sqrt},
};
use substrate_fixed::types::U64F64;

/// A price: rao TAO per rao alpha.
pub type Price = U64F64;

/// Basis-point denominator.
pub const BPS: u128 = 10_000;

/// One in `U64F64` bits.
pub const Q64: u128 = 1 << 64;

/// `floor(a / b)`; `None` if `b == 0`.
#[must_use]
pub fn floor_div(a: u128, b: u128) -> Option<u128> {
    a.checked_div(b)
}

/// `ceil(a / b)`; `None` if `b == 0`.
#[must_use]
pub fn ceil_div(a: u128, b: u128) -> Option<u128> {
    (b != 0).then(|| a.div_ceil(b))
}

/// `floor(a * b / c)` with a 256-bit product; `None` if `c == 0` or the result exceeds `u128`.
#[must_use]
pub fn mul_div_floor(a: u128, b: u128, c: u128) -> Option<u128> {
    multiply_by_rational_with_rounding(a, b, c, Rounding::Down)
}

/// `ceil(a * b / c)` with a 256-bit product; `None` if `c == 0` or the result exceeds `u128`.
#[must_use]
pub fn mul_div_ceil(a: u128, b: u128, c: u128) -> Option<u128> {
    multiply_by_rational_with_rounding(a, b, c, Rounding::Up)
}

/// `floor(sqrt(n))`, exact.
#[must_use]
pub fn isqrt(n: u128) -> u128 {
    sqrt(n)
}

/// `floor(tao * 2^64 / alpha)`; zero when `alpha == 0` (an empty pool has no price).
#[must_use]
pub fn price(tao: u64, alpha: u128) -> Price {
    let bits = u128::from(tao).saturating_mul(Q64).checked_div(alpha);
    Price::from_bits(bits.unwrap_or(0))
}

/// `floor(price * num / den)`; `None` if `den == 0` or the result exceeds `U64F64`.
#[must_use]
pub fn scale_price_floor(price: Price, num: u128, den: u128) -> Option<Price> {
    mul_div_floor(price.to_bits(), num, den).map(Price::from_bits)
}

/// `ceil(price * num / den)`; `None` if `den == 0` or the result exceeds `U64F64`.
#[must_use]
pub fn scale_price_ceil(price: Price, num: u128, den: u128) -> Option<Price> {
    mul_div_ceil(price.to_bits(), num, den).map(Price::from_bits)
}

/// `floor(amount * price)` in rao, saturating at `u64::MAX`.
#[must_use]
pub fn mul_price_floor(amount: u64, price: Price) -> u64 {
    saturate_u64(mul_div_floor(u128::from(amount), price.to_bits(), Q64))
}

/// `floor(total * weight / weight_sum)`: one participant's pro-rata share. Zero when
/// `weight_sum == 0`. The caller keeps the rounding dust (`total - sum of shares`).
#[must_use]
pub fn pro_rata_floor(total: u64, weight: u64, weight_sum: u64) -> u64 {
    if weight_sum == 0 {
        return 0;
    }
    saturate_u64(mul_div_floor(
        u128::from(total),
        u128::from(weight),
        u128::from(weight_sum),
    ))
}

/// Narrows an amount to rao in `u64`; an overflowed (`None`) or oversized amount saturates.
#[must_use]
pub fn saturate_u64(amount: Option<u128>) -> u64 {
    amount.map_or(u64::MAX, |v| u64::try_from(v).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divisions_round_in_their_named_direction() {
        assert_eq!(floor_div(7, 2), Some(3));
        assert_eq!(ceil_div(7, 2), Some(4));
        assert_eq!(ceil_div(8, 2), Some(4));
        assert_eq!(ceil_div(0, 2), Some(0));
        assert_eq!(floor_div(1, 0), None);
        assert_eq!(ceil_div(1, 0), None);
    }

    #[test]
    fn mul_div_uses_a_wide_product_and_reports_overflow() {
        assert_eq!(mul_div_floor(u128::MAX, 3, 3), Some(u128::MAX));
        assert_eq!(mul_div_floor(10, 10, 3), Some(33));
        assert_eq!(mul_div_ceil(10, 10, 3), Some(34));
        assert_eq!(mul_div_ceil(9, 10, 3), Some(30));
        assert_eq!(mul_div_floor(u128::MAX, 2, 1), None);
        assert_eq!(mul_div_ceil(1, 1, 0), None);
    }

    #[test]
    fn isqrt_is_the_exact_floor() {
        for n in [0u128, 1, 2, 3, 4, 15, 16, 17, 10u128.pow(36), u128::MAX] {
            let r = isqrt(n);
            assert!(r.checked_mul(r).is_some_and(|sq| sq <= n));
            let next = r + 1;
            assert!(next.checked_mul(next).is_none_or(|sq| sq > n));
        }
    }

    #[test]
    fn price_is_floor_of_ratio_in_q64() {
        assert_eq!(price(1, 1).to_bits(), Q64);
        assert_eq!(price(1, 3).to_bits(), Q64 / 3);
        assert_eq!(price(5, 0).to_bits(), 0);
        assert_eq!(price(u64::MAX, 1).to_bits(), u128::from(u64::MAX) * Q64);
    }

    #[test]
    fn price_scaling_and_products() {
        let p = Price::from_bits(7);
        assert_eq!(scale_price_floor(p, 1, 2), Some(Price::from_bits(3)));
        assert_eq!(scale_price_ceil(p, 1, 2), Some(Price::from_bits(4)));
        assert_eq!(scale_price_floor(Price::max_value(), 2, 1), None);
        assert_eq!(scale_price_floor(p, 1, 0), None);
        assert_eq!(mul_price_floor(10, Price::from_bits(Q64 / 4)), 2);
        assert_eq!(
            mul_price_floor(u64::MAX, Price::from_bits(2 * Q64)),
            u64::MAX
        );
    }

    #[test]
    fn pro_rata_floors_and_handles_an_empty_book() {
        assert_eq!(pro_rata_floor(10, 1, 3), 3);
        assert_eq!(pro_rata_floor(10, 3, 3), 10);
        assert_eq!(pro_rata_floor(10, 0, 3), 0);
        assert_eq!(pro_rata_floor(10, 1, 0), 0);
    }
}
