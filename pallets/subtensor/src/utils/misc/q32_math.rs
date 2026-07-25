//! Saturating Q32 fixed-point multiply, power, and half-life decay helpers.
use super::*;

impl<T: Config> Pallet<T> {
    /// Multiply an integer `value` by a Q32 fixed-point factor.
    ///
    /// Q32 means:
    ///   1.0 == 1 << 32
    ///   0.5 == 1 << 31
    ///
    /// Safe / non-panicking:
    /// * uses saturating u128 multiplication
    /// * clamps back into u64 range
    pub fn mul_by_q32(value: u64, factor_q32: u64) -> u64 {
        let product: u128 = (value as u128).saturating_mul(factor_q32 as u128);
        let shifted: u128 = product >> 32;
        core::cmp::min(shifted, u64::MAX as u128) as u64
    }

    /// Exponentiation-by-squaring for Q32 values.
    ///
    /// Returns `base_q32 ^ exp` in Q32.
    ///
    /// Safe / non-panicking:
    /// * uses `mul_by_q32`, which is saturating/clamped
    pub fn pow_q32(base_q32: u64, exp: u16) -> u64 {
        let mut result: u64 = 1u64 << 32; // 1.0 in Q32
        let mut factor: u64 = base_q32;
        let mut power: u32 = u32::from(exp);

        while power > 0 {
            if (power & 1) == 1 {
                result = Self::mul_by_q32(result, factor);
            }

            power >>= 1;

            if power > 0 {
                factor = Self::mul_by_q32(factor, factor);
            }
        }

        result
    }

    /// Returns the per-block decay factor `f` in Q32
    pub fn decay_factor_q32(half_life: u16) -> u64 {
        if half_life == 0 {
            return 1u64 << 32; // 1.0
        }

        let one_q32: u64 = 1u64 << 32;
        let half_q32: u64 = 1u64 << 31; // 0.5

        let mut lo: u64 = 0;
        let mut hi: u64 = one_q32;

        while lo.saturating_add(1) < hi {
            let span: u64 = hi.saturating_sub(lo);
            let mid: u64 = lo.saturating_add(span >> 1);
            let mid_pow: u64 = Self::pow_q32(mid, half_life);

            if mid_pow > half_q32 {
                hi = mid;
            } else {
                lo = mid;
            }
        }

        lo
    }
}
