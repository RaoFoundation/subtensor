//! [`SafeFloat`]: controlled-precision decimal used by [`crate::SharePool`] stake shares.
//!
//! Backed by a u128 mantissa and i64 base-10 exponent, normalized so that
//! `SAFE_FLOAT_MAX / 10 < mantissa <= SAFE_FLOAT_MAX` (except zero).

use codec::{Decode, Encode};
#[cfg(not(feature = "std"))]
use num_traits::float::FloatCore as _;
use scale_info::TypeInfo;
use sp_core::U256;
use substrate_fixed::types::U64F64;
use subtensor_macros::freeze_struct;

/// Maximum mantissa digits retained after [`SafeFloat::normalize`] (10^21).
pub const SAFE_FLOAT_MAX: u128 = 1_000_000_000_000_000_000_000_u128;
/// `log10(SAFE_FLOAT_MAX)`; also the scale used when dividing mantissas in U256.
pub const SAFE_FLOAT_MAX_EXP: i64 = 21_i64;

/// Controlled-precision float for share-pool stake accounting (rao-scale).
///
/// Mantissa precision is tuned so a +1 rao hotkey stake update moves both the
/// coldkey share and the share-pool denominator, while a fractional 0.1 rao
/// (which cannot exist on-chain) does not.
#[freeze_struct("9358e1962fcbda0d")]
#[derive(Encode, Decode, Default, TypeInfo, Clone, PartialEq, Eq, Debug)]
pub struct SafeFloat {
    mantissa: u128,
    exponent: i64,
}

/// Return `10^e` as [`U256`], capped at `10^(SAFE_FLOAT_MAX_EXP+1)`.
///
/// Used when aligning mantissas across exponents during add/sub and u64 conversion.
fn capped_pow10(e: u64) -> U256 {
    if e > (SAFE_FLOAT_MAX_EXP as u64).saturating_add(1) {
        return U256::from(SAFE_FLOAT_MAX.saturating_mul(10));
    }
    if e == 0 {
        return U256::from(1);
    }
    U256::from(10)
        .checked_pow(U256::from(e))
        .unwrap_or_default()
}

impl SafeFloat {
    /// Zero value (`mantissa = 0`, `exponent = 0`).
    pub fn zero() -> Self {
        SafeFloat {
            mantissa: 0_u128,
            exponent: 0_i64,
        }
    }

    /// Construct and normalize; returns `None` if `mantissa > SAFE_FLOAT_MAX`.
    pub fn new(mantissa: u128, exponent: i64) -> Option<Self> {
        // Cap mantissa at SAFE_FLOAT_MAX
        if mantissa > SAFE_FLOAT_MAX {
            return None;
        }

        let mut safe_float = SafeFloat::zero();

        if safe_float.normalize(&U256::from(mantissa), exponent) {
            Some(safe_float)
        } else {
            None
        }
    }

    /// Sets the new mantissa and exponent adjusting mantissa and exponent so that
    /// SAFE_FLOAT_MAX / 10 < mantissa <= SAFE_FLOAT_MAX
    ///
    /// Returns true in case of success or false if exponent over- or underflows
    pub(crate) fn normalize(&mut self, new_mantissa: &U256, new_exponent: i64) -> bool {
        if new_mantissa.is_zero() {
            self.mantissa = 0;
            self.exponent = 0;
            return true;
        }

        let ten = U256::from(10);
        let max_mantissa = U256::from(SAFE_FLOAT_MAX);
        let min_mantissa = U256::from(SAFE_FLOAT_MAX)
            .checked_div(ten)
            .unwrap_or_default();

        // Loops are safe because they are bounded by U256 size and result
        // in no more than 78 iterations together
        let mut normalized_mantissa = *new_mantissa;
        let mut normalized_exponent = new_exponent;

        while normalized_mantissa > max_mantissa {
            let Some(next_mantissa) = normalized_mantissa.checked_div(ten) else {
                return false;
            };
            let Some(next_exponent) = normalized_exponent.checked_add(1) else {
                return false;
            };

            normalized_mantissa = next_mantissa;
            normalized_exponent = next_exponent;
        }

        while normalized_mantissa <= min_mantissa {
            let Some(next_mantissa) = normalized_mantissa.checked_mul(ten) else {
                return false;
            };
            let Some(next_exponent) = normalized_exponent.checked_sub(1) else {
                return false;
            };

            normalized_mantissa = next_mantissa;
            normalized_exponent = next_exponent;
        }

        self.mantissa = normalized_mantissa.low_u128();
        self.exponent = normalized_exponent;

        true
    }

    /// Divide current value by a preserving precision (SAFE_FLOAT_MAX digits in mantissa)
    ///   result = m1 * 10^e1 / m2 * 10^e2
    pub fn div(&self, a: &SafeFloat) -> Option<Self> {
        // - In m1 / m2 division we need enough digits for a u128.
        //   This can be calculated in a lossless way in U256 as m1 * MAX_MANTISSA / m2
        // - The new exponent is e1 - e2 - SAFE_FLOAT_MAX_EXP
        let maybe_m1_scaled_u256 =
            U256::from(self.mantissa).checked_mul(U256::from(SAFE_FLOAT_MAX));
        let m2_u256 = U256::from(a.mantissa);

        // Calculate new exponent
        let new_exponent_i128 = (self.exponent as i128)
            .saturating_sub(a.exponent as i128)
            .saturating_sub(SAFE_FLOAT_MAX_EXP as i128);
        if (new_exponent_i128 > i64::MAX as i128) || (new_exponent_i128 < i64::MIN as i128) {
            return None;
        }
        let new_exponent = new_exponent_i128 as i64;

        // Calculate new mantissa, normalize, and return result
        if let Some(m1_scaled_u256) = maybe_m1_scaled_u256 {
            let maybe_new_mantissa_u256 = m1_scaled_u256.checked_div(m2_u256);
            if let Some(new_mantissa_u256) = maybe_new_mantissa_u256 {
                let mut safe_float = SafeFloat::zero();
                if safe_float.normalize(&new_mantissa_u256, new_exponent) {
                    Some(safe_float)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Add two normalized values, aligning exponents via [`capped_pow10`].
    pub fn add(&self, a: &SafeFloat) -> Option<Self> {
        if self.is_zero() {
            return Some(a.clone());
        }
        if a.is_zero() {
            return Some(self.clone());
        }

        let (new_mantissa, new_exponent) = if self.exponent >= a.exponent {
            let exp_diff = self.exponent.saturating_sub(a.exponent);
            let m1 = U256::from(self.mantissa);
            let m2 = U256::from(a.mantissa)
                .checked_div(capped_pow10(exp_diff as u64))
                .unwrap_or_default();
            (m1.saturating_add(m2), self.exponent)
        } else {
            let exp_diff = a.exponent.saturating_sub(self.exponent);
            let m1 = U256::from(self.mantissa)
                .checked_div(capped_pow10(exp_diff as u64))
                .unwrap_or_default();
            let m2 = U256::from(a.mantissa);
            (m1.saturating_add(m2), a.exponent)
        };

        let mut safe_float = SafeFloat::zero();
        if safe_float.normalize(&new_mantissa, new_exponent) {
            Some(safe_float)
        } else {
            None
        }
    }

    /// Subtract `a` from `self`; returns `None` if the result would be negative.
    pub fn sub(&self, a: &SafeFloat) -> Option<Self> {
        if self.is_zero() && a.is_zero() {
            return Some(Self::zero());
        } else if self.is_zero() {
            return None;
        }
        if a.is_zero() {
            return Some(self.clone());
        }

        let (new_mantissa, new_exponent) = if self.exponent >= a.exponent {
            let exp_diff = self.exponent.saturating_sub(a.exponent);
            let m1 = U256::from(self.mantissa);
            let m2 = U256::from(a.mantissa)
                .checked_div(capped_pow10(exp_diff as u64))
                .unwrap_or_default();
            (m1.saturating_sub(m2), self.exponent)
        } else {
            let exp_diff = a.exponent.saturating_sub(self.exponent);
            let m1 = U256::from(self.mantissa)
                .checked_div(capped_pow10(exp_diff as u64))
                .unwrap_or_default();
            let m2 = U256::from(a.mantissa);
            (m1.saturating_sub(m2), a.exponent)
        };

        let mut safe_float = SafeFloat::zero();
        if safe_float.normalize(&new_mantissa, new_exponent) {
            Some(safe_float)
        } else {
            None
        }
    }

    /// Calculate self * a / b without loss of precision
    pub fn mul_div(&self, a: &SafeFloat, b: &SafeFloat) -> Option<Self> {
        if b.mantissa == 0_u128 {
            return None;
        }

        // No overflows here, just unwrap or default
        let self_a_mantissa_u256 = U256::from(self.mantissa)
            .checked_mul(U256::from(a.mantissa))
            .unwrap_or_default();
        let maybe_self_a_exponent = self.exponent.checked_add(a.exponent);

        if let Some(self_a_exponent) = maybe_self_a_exponent {
            // Divide by b in U256
            let maybe_new_exponent = self_a_exponent.checked_sub(b.exponent);
            if let Some(new_exponent) = maybe_new_exponent {
                let new_mantissa = self_a_mantissa_u256
                    .checked_div(U256::from(b.mantissa))
                    .unwrap_or_default();
                let mut result = SafeFloat::zero();
                if result.normalize(&new_mantissa, new_exponent) {
                    Some(result)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
    }

    /// True when the mantissa is zero (canonical zero representation).
    pub fn is_zero(&self) -> bool {
        self.mantissa == 0u128
    }

    /// Normalized mantissa digits (test-only; production code uses [`Self::is_zero`] etc.).
    #[cfg(test)]
    pub(crate) fn mantissa(&self) -> u128 {
        self.mantissa
    }

    /// Base-10 exponent (test-only).
    #[cfg(test)]
    pub(crate) fn exponent(&self) -> i64 {
        self.exponent
    }

    /// Returns true if self > a
    /// Both values should be normalized
    pub fn gt(&self, a: &SafeFloat) -> bool {
        let ten = U256::from(10);

        if self.exponent == a.exponent {
            self.mantissa > a.mantissa
        } else if self.exponent > a.exponent {
            let exp_diff = self.exponent.saturating_sub(a.exponent);
            if exp_diff > 1_i64 {
                true
            } else {
                ten.saturating_mul(U256::from(self.mantissa)) > U256::from(a.mantissa)
            }
        } else {
            let exp_diff = a.exponent.saturating_sub(self.exponent);
            if exp_diff > 1_i64 {
                false
            } else {
                U256::from(self.mantissa) > ten.saturating_mul(U256::from(a.mantissa))
            }
        }
    }
}

// Saturating conversion: negatives -> 0, overflow -> u64::MAX
impl From<&SafeFloat> for u64 {
    fn from(value: &SafeFloat) -> Self {
        // If exponent is zero, it's just an integer mantissa
        if value.exponent == 0 {
            return u64::try_from(value.mantissa).unwrap_or(u64::MAX);
        }

        // scale = 10^exponent
        let scale = capped_pow10(value.exponent.unsigned_abs());

        // mantissa * 10^exponent
        let q: U256 = if value.exponent > 0 {
            U256::from(value.mantissa).saturating_mul(scale)
        } else {
            U256::from(value.mantissa)
                .checked_div(scale)
                .unwrap_or_default()
        };

        // Convert quotient to u64, saturating on overflow
        if q.is_zero() {
            0
        } else {
            q.try_into().unwrap_or(u64::MAX)
        }
    }
}

// Convenience impl for owning values
impl From<SafeFloat> for u64 {
    fn from(value: SafeFloat) -> Self {
        u64::from(&value)
    }
}

impl From<u64> for SafeFloat {
    fn from(value: u64) -> Self {
        SafeFloat::new(value as u128, 0).unwrap_or_default()
    }
}

impl From<U64F64> for SafeFloat {
    fn from(value: U64F64) -> Self {
        let bits = value.to_bits();
        // High 64 bits = integer part
        let int = (bits >> 64) as u64;
        // Low 64 bits = fractional part
        let frac = (bits & 0xFFFF_FFFF_FFFF_FFFF) as u64;

        // If strictly zero, shortcut
        if bits == 0 {
            return SafeFloat::zero();
        }

        // SafeFloat for integer part: int * 10^0
        let safe_int = SafeFloat::new(int as u128, 0).unwrap_or_default();

        // Numerator of fractional part: frac * 10^0
        let safe_frac_num = SafeFloat::new(frac as u128, 0).unwrap_or_default();

        // Denominator = 2^64 as an integer SafeFloat: (2^64) * 10^0
        let two64: u128 = 1u128 << 64;
        let safe_two64 = SafeFloat::new(two64, 0).unwrap_or_default();

        // frac_part = frac / 2^64
        let safe_frac = safe_frac_num.div(&safe_two64).unwrap_or_default();

        // int + frac/2^64, with all mantissa/exponent normalization
        safe_int.add(&safe_frac).unwrap_or_default()
    }
}

impl From<&SafeFloat> for f64 {
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "This code is only used in tests"
    )]
    fn from(value: &SafeFloat) -> Self {
        let mant = value.mantissa as f64;

        // powi takes i32, so clamp i64 exponent into i32 range (test-only).
        let e = value.exponent.clamp(i32::MIN as i64, i32::MAX as i64) as i32;

        mant * 10_f64.powi(e)
    }
}

impl From<SafeFloat> for f64 {
    fn from(value: SafeFloat) -> Self {
        f64::from(&value)
    }
}
