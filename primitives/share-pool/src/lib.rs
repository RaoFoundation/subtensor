#![cfg_attr(not(feature = "std"), no_std)]
#![allow(clippy::result_unit_err, clippy::indexing_slicing)]

use codec::{Decode, Encode};
#[cfg(not(feature = "std"))]
use num_traits::float::FloatCore as _;
use scale_info::TypeInfo;
use sp_core::U256;
use sp_std::marker;
use sp_std::ops::Neg;
use substrate_fixed::types::U64F64;
use subtensor_macros::freeze_struct;

// Maximum mantissa that can be used with SafeFloat
pub const SAFE_FLOAT_MAX: u128 = 1_000_000_000_000_000_000_000_u128;
pub const SAFE_FLOAT_MAX_EXP: i64 = 21_i64;

/// Controlled precision floating point number with efficient storage
///
/// Precision is controlled in a way that keeps enough mantissa digits so
/// that updating hotkey stake by 1 rao makes difference in the resulting shared
/// pool variables (both coldkey share and share pool denominator), but also
/// precision should be limited so that updating by 0.1 rao does not make the
/// difference (because there's no such thing as 0.1 rao, rao is integer).
#[freeze_struct("9a55fbe2d60efb41")]
#[derive(Encode, Decode, Default, TypeInfo, Clone, PartialEq, Eq, Debug)]
pub struct SafeFloat {
    mantissa: u128,
    exponent: i64,
}

/// Capped power of 10 in U256
/// Cap at 10^SAFE_FLOAT_MAX_EXP+1, we don't need greater powers here
fn cappow10(e: u64) -> U256 {
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
    pub fn zero() -> Self {
        SafeFloat {
            mantissa: 0_u128,
            exponent: 0_i64,
        }
    }

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

    /// Sets the new mantissa and exponent adjustsing mantissa and exponent so that
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

        // Calcuate new mantissa, normalize, and return result
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
                .checked_div(cappow10(exp_diff as u64))
                .unwrap_or_default();
            (m1.saturating_add(m2), self.exponent)
        } else {
            let exp_diff = a.exponent.saturating_sub(self.exponent);
            let m1 = U256::from(self.mantissa)
                .checked_div(cappow10(exp_diff as u64))
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
                .checked_div(cappow10(exp_diff as u64))
                .unwrap_or_default();
            (m1.saturating_sub(m2), self.exponent)
        } else {
            let exp_diff = a.exponent.saturating_sub(self.exponent);
            let m1 = U256::from(self.mantissa)
                .checked_div(cappow10(exp_diff as u64))
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

    pub fn is_zero(&self) -> bool {
        self.mantissa == 0u128
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
        let scale = cappow10(value.exponent.unsigned_abs());

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

pub trait SharePoolDataOperations<Key> {
    /// Gets shared value (always "the real thing" measured in rao, not fractional)
    fn get_shared_value(&self) -> u64;
    /// Gets single share for a given key
    fn get_share(&self, key: &Key) -> SafeFloat;
    // Tries to get a single share for a given key, as a result.
    fn try_get_share(&self, key: &Key) -> Result<SafeFloat, ()>;
    /// Gets share pool denominator
    fn get_denominator(&self) -> SafeFloat;
    /// Updates shared value by provided signed value
    fn set_shared_value(&mut self, value: u64);
    /// Update single share for a given key by provided signed value
    fn set_share(&mut self, key: &Key, share: SafeFloat);
    /// Update share pool denominator by provided signed value
    fn set_denominator(&mut self, update: SafeFloat);
}

/// SharePool struct that depends on the Key type and uses the SharePoolDataOperations
#[derive(Debug)]
pub struct SharePool<K, Ops>
where
    K: Eq,
    Ops: SharePoolDataOperations<K>,
{
    state_ops: Ops,
    phantom_key: marker::PhantomData<K>,
}

impl<K, Ops> SharePool<K, Ops>
where
    K: Eq,
    Ops: SharePoolDataOperations<K>,
{
    pub fn new(ops: Ops) -> Self {
        SharePool {
            state_ops: ops,
            phantom_key: marker::PhantomData,
        }
    }

    pub fn get_value(&self, key: &K) -> u64 {
        let shared_value: SafeFloat =
            SafeFloat::new(self.state_ops.get_shared_value() as u128, 0).unwrap_or_default();
        let current_share: SafeFloat = self.state_ops.get_share(key);
        let denominator: SafeFloat = self.state_ops.get_denominator();
        shared_value
            .mul_div(&current_share, &denominator)
            .unwrap_or_default()
            .into()
    }

    pub fn get_value_from_shares(&self, current_share: SafeFloat) -> u64 {
        let shared_value: SafeFloat =
            SafeFloat::new(self.state_ops.get_shared_value() as u128, 0).unwrap_or_default();
        let denominator: SafeFloat = self.state_ops.get_denominator();
        shared_value
            .mul_div(&current_share, &denominator)
            .unwrap_or_default()
            .into()
    }

    pub fn try_get_value(&self, key: &K) -> Result<u64, ()> {
        match self.state_ops.try_get_share(key) {
            Ok(_) => Ok(self.get_value(key)),
            Err(i) => Err(i),
        }
    }

    /// Update the total shared value.
    /// Every key's associated value effectively updates with this operation
    pub fn update_value_for_all(&mut self, update: i64) {
        let shared_value: u64 = self.state_ops.get_shared_value();
        self.state_ops.set_shared_value(if update >= 0 {
            shared_value.saturating_add(update as u64)
        } else {
            shared_value.saturating_sub(update.neg() as u64)
        });
    }

    pub fn sim_update_value_for_one(&mut self, update: i64) -> bool {
        let shared_value: u64 = self.state_ops.get_shared_value();
        let denominator: SafeFloat = self.state_ops.get_denominator();

        // Then, update this key's share
        if denominator.mantissa == 0 {
            true
        } else {
            // There are already keys in the pool, set or update this key
            let shares_per_update = self.get_shares_per_update(update, shared_value, &denominator);

            !shares_per_update.is_zero()
        }
    }

    fn get_shares_per_update(
        &self,
        update: i64,
        shared_value: u64,
        denominator: &SafeFloat,
    ) -> SafeFloat {
        let shared_value: SafeFloat = SafeFloat::new(shared_value as u128, 0).unwrap_or_default();
        let update_sf: SafeFloat =
            SafeFloat::new(update.unsigned_abs() as u128, 0).unwrap_or_default();
        update_sf
            .mul_div(denominator, &shared_value)
            .unwrap_or_default()
    }

    /// Update the value associated with an item identified by the Key
    /// Returns actual update
    ///
    pub fn update_value_for_one(&mut self, key: &K, update: i64) {
        let shared_value: u64 = self.state_ops.get_shared_value();
        let current_share: SafeFloat = self.state_ops.get_share(key);
        let denominator: SafeFloat = self.state_ops.get_denominator();

        // Then, update this key's share
        if denominator.is_zero() {
            // Initialize the pool. The first key gets all.
            let update_float: SafeFloat =
                SafeFloat::new(update.unsigned_abs() as u128, 0).unwrap_or_default();
            self.state_ops.set_denominator(update_float.clone());
            self.state_ops.set_share(key, update_float);
        } else {
            let new_denominator;
            let new_current_share;

            let shares_per_update: SafeFloat =
                self.get_shares_per_update(update, shared_value, &denominator);

            // Handle SafeFloat overflows quietly here because this overflow of i64 exponent
            // is extremely hypothetical and should never happen in practice.
            if update > 0 {
                new_denominator = match denominator.add(&shares_per_update) {
                    Some(new_denominator) => new_denominator,
                    None => {
                        log::error!(
                            "SafeFloat::add overflow when adding {:?} to {:?}; keeping old denominator",
                            shares_per_update,
                            denominator,
                        );
                        // Return the value as it was before the failed addition
                        denominator
                    }
                };

                new_current_share = match current_share.add(&shares_per_update) {
                    Some(new_current_share) => new_current_share,
                    None => {
                        log::error!(
                            "SafeFloat::add overflow when adding {:?} to {:?}; keeping old current_share",
                            shares_per_update,
                            current_share,
                        );
                        // Return the value as it was before the failed addition
                        current_share
                    }
                };
            } else {
                new_denominator = match denominator.sub(&shares_per_update) {
                    Some(new_denominator) => new_denominator,
                    None => {
                        log::error!(
                            "SafeFloat::add overflow when adding {:?} to {:?}; keeping old denominator",
                            shares_per_update,
                            denominator,
                        );
                        // Return the value as it was before the failed addition
                        denominator
                    }
                };

                new_current_share = match current_share.sub(&shares_per_update) {
                    Some(new_current_share) => new_current_share,
                    None => {
                        log::error!(
                            "SafeFloat::add overflow when adding {:?} to {:?}; keeping old current_share",
                            shares_per_update,
                            current_share,
                        );
                        // Return the value as it was before the failed addition
                        current_share
                    }
                };
            }

            self.state_ops.set_denominator(new_denominator);
            self.state_ops.set_share(key, new_current_share);
        }

        // Update shared value
        self.update_value_for_all(update);
    }
}

// cargo test --package share-pool --lib -- tests --nocapture
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;
    use std::collections::BTreeMap;
    use substrate_fixed::types::U64F64;

    struct MockSharePoolDataOperations {
        shared_value: u64,
        share: BTreeMap<u16, SafeFloat>,
        denominator: SafeFloat,
    }

    impl MockSharePoolDataOperations {
        fn new() -> Self {
            MockSharePoolDataOperations {
                shared_value: 0u64,
                share: BTreeMap::new(),
                denominator: SafeFloat::zero(),
            }
        }
    }

    impl SharePoolDataOperations<u16> for MockSharePoolDataOperations {
        fn get_shared_value(&self) -> u64 {
            self.shared_value
        }

        fn get_share(&self, key: &u16) -> SafeFloat {
            self.share.get(key).cloned().unwrap_or_else(SafeFloat::zero)
        }

        fn try_get_share(&self, key: &u16) -> Result<SafeFloat, ()> {
            match self.share.get(key).cloned() {
                Some(value) => Ok(value),
                None => Err(()),
            }
        }

        fn get_denominator(&self) -> SafeFloat {
            self.denominator.clone()
        }

        fn set_shared_value(&mut self, value: u64) {
            self.shared_value = value;
        }

        fn set_share(&mut self, key: &u16, share: SafeFloat) {
            self.share.insert(*key, share);
        }

        fn set_denominator(&mut self, update: SafeFloat) {
            self.denominator = update;
        }
    }

    #[test]
    fn test_get_value() {
        let mut mock_ops = MockSharePoolDataOperations::new();
        mock_ops.set_denominator(10u64.into());
        mock_ops.set_share(&1_u16, 3u64.into());
        mock_ops.set_share(&2_u16, 7u64.into());
        mock_ops.set_shared_value(100u64.into());
        let share_pool = SharePool::new(mock_ops);
        let result1 = share_pool.get_value(&1);
        let result2 = share_pool.get_value(&2);
        assert_eq!(result1, 30);
        assert_eq!(result2, 70);
    }

    #[test]
    fn test_division_by_zero() {
        let mut mock_ops = MockSharePoolDataOperations::new();
        mock_ops.set_denominator(SafeFloat::zero()); // Zero denominator
        let pool = SharePool::<u16, MockSharePoolDataOperations>::new(mock_ops);

        let value = pool.get_value(&1);
        assert_eq!(value, 0, "Value should be 0 when denominator is zero");
    }

    #[test]
    fn test_max_shared_value() {
        let mut mock_ops = MockSharePoolDataOperations::new();
        mock_ops.set_shared_value(u64::MAX.into());
        mock_ops.set_share(&1, 3u64.into()); // Use a neutral value for share
        mock_ops.set_share(&2, 7u64.into()); // Use a neutral value for share
        mock_ops.set_denominator(10u64.into()); // Neutral value to see max effect
        let pool = SharePool::<u16, MockSharePoolDataOperations>::new(mock_ops);

        let max_value = pool.get_value(&1) + pool.get_value(&2);
        assert!(u64::MAX - max_value <= 5, "Max value should map to u64 MAX");
    }

    #[test]
    fn test_max_share_value() {
        let mut mock_ops = MockSharePoolDataOperations::new();
        mock_ops.set_shared_value(1_000_000_000u64); // Use a neutral value for shared value
        mock_ops.set_share(&1, (u64::MAX / 2).into());
        mock_ops.set_share(&2, (u64::MAX / 2).into());
        mock_ops.set_denominator((u64::MAX).into());
        let pool = SharePool::<u16, MockSharePoolDataOperations>::new(mock_ops);

        let value1 = pool.get_value(&1) as i128;
        let value2 = pool.get_value(&2) as i128;

        assert_abs_diff_eq!(value1 as f64, 500_000_000_f64, epsilon = 1.);
        assert!((value2 - 500_000_000).abs() <= 1);
    }

    #[test]
    fn test_denom_precision() {
        let mock_ops = MockSharePoolDataOperations::new();
        let mut pool = SharePool::<u16, MockSharePoolDataOperations>::new(mock_ops);

        pool.update_value_for_one(&1, 1000);

        let value_tmp = pool.get_value(&1) as i128;
        assert_eq!(value_tmp, 1000);

        pool.update_value_for_one(&1, -990);
        pool.update_value_for_one(&2, 1000);
        pool.update_value_for_one(&2, -990);

        let value1 = pool.get_value(&1) as i128;
        let value2 = pool.get_value(&2) as i128;

        assert_eq!(value1, 10);
        assert_eq!(value2, 10);
    }

    // cargo test --package share-pool --lib -- tests::test_denom_high_precision --exact --show-output
    #[test]
    fn test_denom_high_precision() {
        let mock_ops = MockSharePoolDataOperations::new();
        let mut pool = SharePool::<u16, MockSharePoolDataOperations>::new(mock_ops);

        // 50%/50% stakes consisting of 1 rao each
        pool.update_value_for_one(&1, 1);
        pool.update_value_for_one(&2, 1);

        // Huge emission resulting in 1M Alpha
        // Both stakers should have 500k Alpha each
        pool.update_value_for_all(999_999_999_999_998);

        // Everyone unstakes almost everything, leaving 10 rao in the stake
        pool.update_value_for_one(&1, -499_999_999_999_990);
        pool.update_value_for_one(&2, -499_999_999_999_990);

        // Huge emission resulting in 1M Alpha
        // Both stakers should have 500k Alpha each
        pool.update_value_for_all(999_999_999_999_980);

        // Stakers add 1k Alpha each
        pool.update_value_for_one(&1, 1_000_000_000_000);
        pool.update_value_for_one(&2, 1_000_000_000_000);

        let value1 = pool.get_value(&1) as f64;
        let value2 = pool.get_value(&2) as f64;
        assert_abs_diff_eq!(value1, 501_000_000_000_000_f64, epsilon = 1.);
        assert_abs_diff_eq!(value2, 501_000_000_000_000_f64, epsilon = 1.);
    }

    // cargo test --package share-pool --lib -- tests::test_denom_high_precision_many_small_unstakes --exact --show-output
    #[test]
    fn test_denom_high_precision_many_small_unstakes() {
        let mock_ops = MockSharePoolDataOperations::new();
        let mut pool = SharePool::<u16, MockSharePoolDataOperations>::new(mock_ops);

        // 50%/50% stakes consisting of 1 rao each
        pool.update_value_for_one(&1, 1);
        pool.update_value_for_one(&2, 1);

        // Huge emission resulting in 1M Alpha
        // Both stakers should have 500k Alpha + 1 rao each
        pool.update_value_for_all(1_000_000_000_000_000);

        // Run X number of small unstake transactions
        let tx_count = 1000;
        let unstake_amount = -500_000_000;
        for _ in 0..tx_count {
            pool.update_value_for_one(&1, unstake_amount);
            pool.update_value_for_one(&2, unstake_amount);
        }

        // Emit 1M - each gets 500k Alpha
        pool.update_value_for_all(1_000_000_000_000_000);

        // Each adds 1k Alpha
        pool.update_value_for_one(&1, 1_000_000_000_000);
        pool.update_value_for_one(&2, 1_000_000_000_000);

        // Result, each should get
        //   (500k+1) + tx_count * unstake_amount + 500k + 1k
        let value1 = pool.get_value(&1) as i128;
        let value2 = pool.get_value(&2) as i128;
        let expected = 1_001_000_000_000_000 + tx_count * unstake_amount;

        assert_abs_diff_eq!(value1 as f64, expected as f64, epsilon = 1.);
        assert_abs_diff_eq!(value2 as f64, expected as f64, epsilon = 1.);
    }

    #[test]
    fn test_update_value_for_one() {
        let mock_ops = MockSharePoolDataOperations::new();
        let mut pool = SharePool::<u16, MockSharePoolDataOperations>::new(mock_ops);

        pool.update_value_for_one(&1, 1000);

        let value = pool.get_value(&1) as i128;
        assert_eq!(value, 1000);
    }

    #[test]
    fn test_update_value_for_all() {
        let mock_ops = MockSharePoolDataOperations::new();
        let mut pool = SharePool::<u16, MockSharePoolDataOperations>::new(mock_ops);

        pool.update_value_for_all(1000);
        assert_eq!(
            pool.state_ops.shared_value,
            U64F64::saturating_from_num(1000)
        );
    }

    // cargo test --package share-pool --lib -- tests::test_get_shares_per_update --exact --show-output
    #[test]
    fn test_get_shares_per_update() {
        // Test case (update, shared_value, denominator_mantissa, denominator_exponent)
        [
            (1_i64, 1_u64, 1_u64, 0_i64),
            (1, 1_000_000_000_000_000_000, 1, 0),
            (1, 21_000_000_000_000_000, 1, 5),
            (1, 21_000_000_000_000_000, 1, -1_000_000),
            (1, 21_000_000_000_000_000, 1, -1_000_000_000),
            (1, 21_000_000_000_000_000, 1, -1_000_000_001),
            (1_000, 21_000_000_000_000_000, 1, 5),
            (21_000_000_000_000_000, 21_000_000_000_000_000, 1, 5),
            (21_000_000_000_000_000, 21_000_000_000_000_000, 1, -5),
            (21_000_000_000_000_000, 21_000_000_000_000_000, 1, -100),
            (21_000_000_000_000_000, 21_000_000_000_000_000, 1, 100),
            (210_000_000_000_000_000, 21_000_000_000_000_000, 1, 5),
            (1_000, 1_000, 21_000_000_000_000_000, 0),
            (1_000, 1_000, 21_000_000_000_000_000, -1),
        ]
        .into_iter()
        .for_each(
            |(update, shared_value, denominator_mantissa, denominator_exponent)| {
                let mock_ops = MockSharePoolDataOperations::new();
                let pool = SharePool::<u16, MockSharePoolDataOperations>::new(mock_ops);

                let denominator_float =
                    SafeFloat::new(denominator_mantissa as u128, denominator_exponent)
                        .unwrap_or_default();
                let denominator_f64: f64 = denominator_float.clone().into();
                let spu: f64 = pool
                    .get_shares_per_update(update, shared_value, &denominator_float)
                    .into();
                let expected = update as f64 * denominator_f64 / shared_value as f64;
                let precision = 1000.;
                assert_abs_diff_eq!(expected, spu, epsilon = expected / precision);
            },
        );
    }

    #[test]
    fn test_safefloat_normalize() {
        // Test case: mantissa, exponent, expected mantissa, expected exponent
        [
            (1_u128, 0, 1_000_000_000_000_000_000_000_u128, -21_i64),
            (0, 0, 0, 0),
            (10_u128, 0, 1_000_000_000_000_000_000_000_u128, -20),
            (1_000_u128, 0, 1_000_000_000_000_000_000_000_u128, -18),
            (
                100_000_000_000_000_000_000_u128,
                0,
                1_000_000_000_000_000_000_000_u128,
                -1,
            ),
            (SAFE_FLOAT_MAX, 0, SAFE_FLOAT_MAX, 0),
        ]
        .into_iter()
        .for_each(|(m, e, expected_m, expected_e)| {
            let a = SafeFloat::new(m, e).unwrap();
            assert_eq!(a.mantissa, expected_m);
            assert_eq!(a.exponent, expected_e);
        });
    }

    #[test]
    fn test_safefloat_add() {
        // Test case: man_a, exp_a, man_b, exp_b, expected mantissa of a+b, expected exponent of a+b
        [
            // 1 + 1 = 2
            (
                1_u128,
                0,
                1_u128,
                0,
                200_000_000_000_000_000_000_u128,
                -20_i64,
            ),
            // 0 + 1 = 1
            (0, 0, 1, 0, 1_000_000_000_000_000_000_000_u128, -21_i64),
            // 0 + 0.1 = 0.1
            (0, 0, 1, -1, 1_000_000_000_000_000_000_000_u128, -22_i64),
            // 1e-1000 + 0.1 = 0.1
            (1, -1000, 1, -1, 1_000_000_000_000_000_000_000_u128, -22_i64),
            // SAFE_FLOAT_MAX + SAFE_FLOAT_MAX
            (
                SAFE_FLOAT_MAX,
                0,
                SAFE_FLOAT_MAX,
                0,
                SAFE_FLOAT_MAX * 2 / 10,
                1_i64,
            ),
            // Expected loss of precision: tiny + huge
            (
                1_u128,
                0,
                1_000_000_000_000_000_000_000_u128,
                1,
                1_000_000_000_000_000_000_000_u128,
                1_i64,
            ),
            (
                1_u128,
                0,
                1_u128,
                22,
                1_000_000_000_000_000_000_000_u128,
                1_i64,
            ),
            (
                1_u128,
                0,
                1_u128,
                23,
                1_000_000_000_000_000_000_000_u128,
                2_i64,
            ),
            (
                123_u128,
                0,
                1_u128,
                23,
                1_000_000_000_000_000_000_000_u128,
                2_i64,
            ),
            (
                123_u128,
                1,
                1_u128,
                23,
                100_000_000_000_000_000_001_u128,
                3_i64,
            ),
            // Small-ish + very large (10^22 + 42)
            // 42 * 10^0 + 1 * 10^22 ≈ 1e22 + 42
            // Normalized ≈ (1e21 + 4) * 10^1
            (
                42_u128,
                0,
                1_u128,
                22,
                1_000_000_000_000_000_000_000_u128,
                1_i64,
            ),
            // "Almost 10^21" + 10^22
            // (10^21 - 1) + 10^22 → floor((10^22 + 10^21 - 1) / 100) * 10^2
            (
                999_999_999_999_999_999_999_u128,
                0,
                1_u128,
                22,
                109_999_999_999_999_999_999_u128,
                2_i64,
            ),
            // Small-ish + 10^23 where the small part is completely lost
            // 42 + 10^23 -> floor((10^23 + 42)/100) * 10^2 ≈ 1e21 * 10^2
            (
                42_u128,
                0,
                1_u128,
                23,
                1_000_000_000_000_000_000_000_u128,
                2_i64,
            ),
            // Small-ish + 10^23 where tiny part slightly affects mantissa
            // 4200 + 10^23 -> floor((10^23 + 4200)/100) * 10^2 = (1e21 + 42) * 10^2
            (
                4_200_u128,
                0,
                1_u128,
                23,
                100_000_000_000_000_000_004_u128,
                3_i64,
            ),
            // (10^21 - 1) + 10^23
            // -> floor((10^23 + 10^21 - 1)/100) = 1e21 + 1e19 - 1
            (
                999_999_999_999_999_999_999_u128,
                0,
                1_u128,
                23,
                100_999_999_999_999_999_999_u128,
                3_i64,
            ),
            // Medium + 10^23 with exponent 1 on the smaller term
            // 999_999 * 10^1 + 1 * 10^23 -> (10^22 + 999_999) * 10^1
            // Normalized ≈ (1e21 + 99_999) * 10^2
            (
                999_999_u128,
                1,
                1_u128,
                23,
                100_000_000_000_000_009_999_u128,
                3_i64,
            ),
            // Check behaviour with exponent 24, tiny second term
            // 1 * 10^24 + 1 -> floor((10^24 + 1)/1000) * 10^3 ≈ 1e21 * 10^3
            (
                1_u128,
                24,
                1_u128,
                0,
                1_000_000_000_000_000_000_000_u128,
                3_i64,
            ),
            // 1 * 10^24 + a non-trivial small mantissa
            // 1e24 + 123456789012345678901 -> floor(/1000) = 1e21 + 123456789012345678
            (
                1_u128,
                24,
                123_456_789_012_345_678_901_u128,
                0,
                100_012_345_678_901_234_567_u128,
                4_i64,
            ),
            // 10^22 and 10^23 combined:
            // 1 * 10^22 + 1 * 10^23 = 11 * 10^22 = (1.1 * 10^23)
            // Normalized → (1.1e20) * 10^3
            (
                1_u128,
                22,
                1_u128,
                23,
                110_000_000_000_000_000_000_u128,
                3_i64,
            ),
            // Both operands already aligned at a huge scale:
            // (10^21 - 1) * 10^22 + 1 * 10^22 = 10^21 * 10^22 = 10^43
            // Canonical form: (1e21) * 10^22
            (
                999_999_999_999_999_999_999_u128,
                22,
                1_u128,
                22,
                1_000_000_000_000_000_000_000_u128,
                22_i64,
            ),
        ]
        .into_iter()
        .for_each(|(m_a, e_a, m_b, e_b, expected_m, expected_e)| {
            let a = SafeFloat::new(m_a, e_a).unwrap();
            let b = SafeFloat::new(m_b, e_b).unwrap();

            let a_plus_b = a.add(&b).unwrap();
            let b_plus_a = b.add(&a).unwrap();

            assert_eq!(a_plus_b.mantissa, expected_m);
            assert_eq!(a_plus_b.exponent, expected_e);
            assert_eq!(b_plus_a.mantissa, expected_m);
            assert_eq!(b_plus_a.exponent, expected_e);
        });
    }

    #[test]
    fn test_safefloat_div_by_zero_is_none() {
        let a = SafeFloat::new(1u128, 0).unwrap();
        assert!(a.div(&SafeFloat::zero()).is_none());
    }

    #[test]
    fn test_safefloat_div() {
        // Test case: man_a, exp_a, man_b, exp_b
        [
            (1_u128, 0_i64, 100_000_000_000_000_000_000_u128, -20_i64),
            (1_u128, 0, 1_u128, 0),
            (1_u128, 1, 1_u128, 0),
            (1_u128, 7, 1_u128, 0),
            (1_u128, 50, 1_u128, 0),
            (1_u128, 100, 1_u128, 0),
            (1_u128, 0, 7_u128, 0),
            (1_u128, 1, 7_u128, 0),
            (1_u128, 7, 7_u128, 0),
            (1_u128, 50, 7_u128, 0),
            (1_u128, 100, 7_u128, 0),
            (1_u128, 0, 3_u128, 0),
            (1_u128, 1, 3_u128, 0),
            (1_u128, 7, 3_u128, 0),
            (1_u128, 50, 3_u128, 0),
            (1_u128, 100, 3_u128, 0),
            (2_u128, 0, 3_u128, 0),
            (2_u128, 1, 3_u128, 0),
            (2_u128, 7, 3_u128, 0),
            (2_u128, 50, 3_u128, 0),
            (2_u128, 100, 3_u128, 0),
            (5_u128, 0, 3_u128, 0),
            (5_u128, 1, 3_u128, 0),
            (5_u128, 7, 3_u128, 0),
            (5_u128, 50, 3_u128, 0),
            (5_u128, 100, 3_u128, 0),
            (10_u128, 0, 100_000_000_000_000_000_000_u128, -19),
            (1_000_u128, 0, 100_000_000_000_000_000_000_u128, -17),
            (
                100_000_000_000_000_000_000_u128,
                0,
                1_000_000_000_000_000_000_000_u128,
                -1,
            ),
            (SAFE_FLOAT_MAX, 0, SAFE_FLOAT_MAX, 0),
            (SAFE_FLOAT_MAX, 100, SAFE_FLOAT_MAX, -100),
            (SAFE_FLOAT_MAX, 100, SAFE_FLOAT_MAX - 1, -100),
            (SAFE_FLOAT_MAX - 1, 100, SAFE_FLOAT_MAX, -100),
            (SAFE_FLOAT_MAX - 2, 100, SAFE_FLOAT_MAX, -100),
            (SAFE_FLOAT_MAX, 100, SAFE_FLOAT_MAX / 2 - 1, -100),
            (SAFE_FLOAT_MAX, 100, SAFE_FLOAT_MAX / 2 - 1, 100),
            (1_u128, 0, 100_000_000_000_000_000_000_u128, -20_i64),
            (
                123_456_789_123_456_789_123_u128,
                20_i64,
                87_654_321_987_654_321_987_u128,
                -20_i64,
            ),
            (
                123_456_789_123_456_789_123_u128,
                100_i64,
                87_654_321_987_654_321_987_u128,
                -100_i64,
            ),
            (
                123_456_789_123_456_789_123_u128,
                -100_i64,
                87_654_321_987_654_321_987_u128,
                100_i64,
            ),
            (
                123_456_789_123_456_789_123_u128,
                -99_i64,
                87_654_321_987_654_321_987_u128,
                99_i64,
            ),
            (
                123_456_789_123_456_789_123_u128,
                123_i64,
                87_654_321_987_654_321_987_u128,
                -32_i64,
            ),
            (
                123_456_789_123_456_789_123_u128,
                -123_i64,
                87_654_321_987_654_321_987_u128,
                32_i64,
            ),
        ]
        .into_iter()
        .for_each(|(ma, ea, mb, eb)| {
            let a = SafeFloat::new(ma, ea).unwrap();
            let b = SafeFloat::new(mb, eb).unwrap();

            let actual: f64 = a.div(&b).unwrap().into();
            let expected =
                ma as f64 * (10_f64).powi(ea as i32) / (mb as f64 * (10_f64).powi(eb as i32));

            assert_abs_diff_eq!(actual, expected, epsilon = actual / 100_000_000_000_000_f64);
        });
    }

    #[test]
    fn test_safefloat_mul_div() {
        // result = a * b / c
        // should not lose precision gained in a * b
        // Test case: man_a, exp_a, man_b, exp_b, man_c, exp_c
        [
            (1_u128, -20_i64, 1_u128, -20_i64, 1_u128, -20_i64),
            (123_u128, 20_i64, 123_u128, -20_i64, 321_u128, 0_i64),
            (
                123_123_123_123_123_123_u128,
                20_i64,
                321_321_321_321_321_321_u128,
                -20_i64,
                777_777_777_777_777_777_u128,
                0_i64,
            ),
            (
                11_111_111_111_111_111_111_u128,
                20_i64,
                99_321_321_321_321_321_321_u128,
                -20_i64,
                77_777_777_777_777_777_777_u128,
                0_i64,
            ),
        ]
        .into_iter()
        .for_each(|(ma, ea, mb, eb, mc, ec)| {
            let a = SafeFloat::new(ma, ea).unwrap();
            let b = SafeFloat::new(mb, eb).unwrap();
            let c = SafeFloat::new(mc, ec).unwrap();

            let actual: f64 = a.mul_div(&b, &c).unwrap().into();
            let expected = (ma as f64 * (10_f64).powi(ea as i32))
                * (mb as f64 * (10_f64).powi(eb as i32))
                / (mc as f64 * (10_f64).powi(ec as i32));

            assert_abs_diff_eq!(actual, expected, epsilon = actual / 100_000_000_000_000_f64);
        });
    }

    #[test]
    fn test_safefloat_from_u64f64() {
        [
            // U64F64::from_num(1000.0),
            // U64F64::from_num(10.0),
            // U64F64::from_num(1.0),
            U64F64::from_num(0.1),
            // U64F64::from_num(0.00000001),
            // U64F64::from_num(123_456_789_123_456u128),
            // // Exact zero
            // U64F64::from_num(0.0),
            // // Very small positive value (well above Q64.64 resolution)
            // U64F64::from_num(1e-18),
            // // Value just below 1
            // U64F64::from_num(0.999_999_999_999_999_f64),
            // // Value just above 1
            // U64F64::from_num(1.000_000_000_000_001_f64),
            // // "Random-looking" fractional with many digits
            // U64F64::from_num(1.234_567_890_123_45_f64),
            // // Large integer, but smaller than the max integer part of U64F64
            // U64F64::from_num(999_999_999_999_999_999u128),
            // // Very large integer near the upper bound of integer range
            // U64F64::from_num(u64::MAX as u128),
            // // Large number with fractional part
            // U64F64::from_num(123_456_789_123_456.78_f64),
            // // Medium-large with tiny fractional part to test precision on tail digits
            // U64F64::from_num(1_000_000_000_000.000_001_f64),
            // // Smallish with long fractional part
            // U64F64::from_num(0.123_456_789_012_345_f64),
        ]
        .into_iter()
        .for_each(|f| {
            let safe_float: SafeFloat = f.into();
            let actual: f64 = safe_float.into();
            let expected = f.to_num::<f64>();

            // Relative epsilon ~1e-14 of the magnitude
            let epsilon = if actual == 0.0 {
                0.0
            } else {
                actual.abs() / 100_000_000_000_000_f64
            };

            assert_abs_diff_eq!(actual, expected, epsilon = epsilon);
        });
    }

    /// This is a real-life scenario test when someone lost 7 TAO on Chutes (SN64)
    /// when paying fees in Alpha. The scenario occured because the update of share value
    /// of one coldkey (update_value_for_one) hit the scenario of full unstake.
    ///
    /// Specifically, the following condition was triggered:
    ///
    ///    `(shared_value + 2_628_000_000_000_000_u64).checked_div(new_denominator)`
    ///
    /// returned None because new_denominator was too low and division of
    /// `shared_value + 2_628_000_000_000_000_u64` by new_denominator has overflown U64F64.
    ///
    /// This test fails on the old version of share pool (with much lower tolerances).
    ///
    /// cargo test --package share-pool --lib -- tests::test_loss_due_to_precision --exact --nocapture
    #[test]
    fn test_loss_due_to_precision() {
        let mock_ops = MockSharePoolDataOperations::new();
        let mut pool = SharePool::<u16, MockSharePoolDataOperations>::new(mock_ops);

        // Setup pool so that initial coldkey's alpha is 10% of 1e12 = 1e11 rao.
        let low_denominator = SafeFloat::new(1u128, -14).unwrap();
        let low_share = SafeFloat::new(1u128, -15).unwrap();
        pool.state_ops.set_denominator(low_denominator);
        pool.state_ops.set_shared_value(1_000_000_000_000_u64);
        pool.state_ops.set_share(&1, low_share);

        let value_before = pool.get_value(&1) as i128;
        assert_abs_diff_eq!(value_before as f64, 100_000_000_000., epsilon = 0.1);

        // Remove a little stake
        let unstake_amount = 1000i64;
        pool.update_value_for_one(&1, unstake_amount.neg());

        let value_after = pool.get_value(&1) as i128;
        assert_abs_diff_eq!(
            (value_before - value_after) as f64,
            unstake_amount as f64,
            epsilon = unstake_amount as f64 / 1_000_000_000.
        );
    }

    fn rel_err(a: f64, b: f64) -> f64 {
        let denom = a.abs().max(b.abs()).max(1.0);
        (a - b).abs() / denom
    }

    fn push_unique(v: &mut Vec<u128>, x: u128) {
        if x != 0 && !v.contains(&x) {
            v.push(x);
        }
    }

    // cargo test --package share-pool --lib -- tests::test_safefloat_mul_div_wide_range --exact --include-ignored --show-output
    #[test]
    #[ignore = "long-running sweep test; run explicitly when needed"]
    fn test_safefloat_mul_div_wide_range() {
        use rayon::prelude::*;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        // Build mantissa corpus
        let mut mantissas = Vec::<u128>::new();

        let linear_steps: u128 = 200;
        let linear_step = (SAFE_FLOAT_MAX / linear_steps).max(1);
        let mut m = 1u128;
        while m <= SAFE_FLOAT_MAX {
            push_unique(&mut mantissas, m);
            match m.checked_add(linear_step) {
                Some(next) if next > m => m = next,
                _ => break,
            }
        }
        push_unique(&mut mantissas, SAFE_FLOAT_MAX);

        let mut p = 1u128;
        while p <= SAFE_FLOAT_MAX {
            push_unique(&mut mantissas, p);
            if p > 1 {
                push_unique(&mut mantissas, p - 1);
            }
            if let Some(next) = p.checked_add(1)
                && next <= SAFE_FLOAT_MAX
            {
                push_unique(&mut mantissas, next);
            }

            match p.checked_mul(10) {
                Some(next) if next > p && next <= SAFE_FLOAT_MAX => p = next,
                _ => break,
            }
        }

        for delta in [
            0u128, 1, 2, 3, 7, 9, 10, 11, 99, 100, 101, 999, 1_000, 10_000,
        ] {
            if SAFE_FLOAT_MAX > delta {
                push_unique(&mut mantissas, SAFE_FLOAT_MAX - delta);
            }
        }

        mantissas.sort_unstable();
        mantissas.dedup();

        let exp_min: i64 = -120;
        let exp_max: i64 = 120;
        let exp_step: usize = 5;
        let exponents: Vec<i64> = (exp_min..=exp_max).step_by(exp_step).collect();

        // Precompute all (a, b) pairs as outer work items.
        // Each Rayon task will then iterate all c's sequentially.
        let mut outer_cases: Vec<(u128, i64, u128, i64)> = Vec::new();

        for &ma in &mantissas {
            for &ea in &exponents {
                for &mb in &mantissas {
                    for &eb in &exponents {
                        outer_cases.push((ma, ea, mb, eb));
                    }
                }
            }
        }

        let checked = Arc::new(AtomicUsize::new(0));
        let skipped_non_finite = Arc::new(AtomicUsize::new(0));
        let skipped_invalid_sf = Arc::new(AtomicUsize::new(0));

        let progress_step = 10_000usize;
        let total_outer = outer_cases.len();

        outer_cases.into_par_iter().for_each(|(ma, ea, mb, eb)| {
            let a = match SafeFloat::new(ma, ea) {
                Some(x) => x,
                None => {
                    skipped_invalid_sf.fetch_add(1, Ordering::Relaxed);
                    return;
                }
            };

            let b = match SafeFloat::new(mb, eb) {
                Some(x) => x,
                None => {
                    skipped_invalid_sf.fetch_add(1, Ordering::Relaxed);
                    return;
                }
            };

            for &mc in &mantissas {
                for &ec in &exponents {
                    let c = match SafeFloat::new(mc, ec) {
                        Some(x) => x,
                        None => {
                            skipped_invalid_sf.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                    };

                    let actual_sf = a.mul_div(&b, &c).unwrap();
                    let actual: f64 = actual_sf.into();

                    let expected =
                        (ma as f64 * 10_f64.powi(ea as i32))
                        * (mb as f64 * 10_f64.powi(eb as i32))
                        / (mc as f64 * 10_f64.powi(ec as i32));

                    if !expected.is_finite() || !actual.is_finite() {
                        skipped_non_finite.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }

                    let err = rel_err(actual, expected);

                    assert!(
                        err <= 1e-12,
                        concat!(
                            "mul_div mismatch:\n",
                            "  a = {}e{}\n",
                            "  b = {}e{}\n",
                            "  c = {}e{}\n",
                            "  actual   = {:.20e}\n",
                            "  expected = {:.20e}\n",
                            "  rel_err  = {:.20e}"
                        ),
                        ma, ea, mb, eb, mc, ec, actual, expected, err
                    );

                    checked.fetch_add(1, Ordering::Relaxed);
                }
            }

            let done_outer = checked.load(Ordering::Relaxed);
            if done_outer.is_multiple_of(progress_step) {
                let invalid = skipped_invalid_sf.load(Ordering::Relaxed);
                let non_finite = skipped_non_finite.load(Ordering::Relaxed);
                log::debug!(
                    "progress: checked={}, skipped_invalid_sf={}, skipped_non_finite={}, outer_total={}",
                    done_outer,
                    invalid,
                    non_finite,
                    total_outer,
                );
            }
        });

        let checked = checked.load(Ordering::Relaxed);
        let skipped_non_finite = skipped_non_finite.load(Ordering::Relaxed);
        let skipped_invalid_sf = skipped_invalid_sf.load(Ordering::Relaxed);

        println!(
            "checked={}, skipped_non_finite={}, skipped_invalid_sf={}, mantissas={}, exponents={}, outer_cases={}",
            checked,
            skipped_non_finite,
            skipped_invalid_sf,
            mantissas.len(),
            exponents.len(),
            total_outer,
        );

        assert!(checked > 0, "test did not validate any finite cases");
    }

    #[test]
    #[ignore = "long-running broad-range test; run explicitly when needed"]
    fn test_safefloat_div_wide_range() {
        use rayon::prelude::*;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        fn rel_err(a: f64, b: f64) -> f64 {
            let denom = a.abs().max(b.abs()).max(1.0);
            (a - b).abs() / denom
        }

        fn push_unique(v: &mut Vec<u128>, x: u128) {
            if x != 0 && !v.contains(&x) {
                v.push(x);
            }
        }

        // Build a broad mantissa corpus:
        // - coarse linear sweep
        // - powers of 10 and neighbors
        // - values near SAFE_FLOAT_MAX
        let mut mantissas = Vec::<u128>::new();

        let linear_steps: u128 = 200;
        let linear_step = (SAFE_FLOAT_MAX / linear_steps).max(1);
        let mut m = 1u128;
        while m <= SAFE_FLOAT_MAX {
            push_unique(&mut mantissas, m);
            match m.checked_add(linear_step) {
                Some(next) if next > m => m = next,
                _ => break,
            }
        }
        push_unique(&mut mantissas, SAFE_FLOAT_MAX);

        let mut p = 1u128;
        while p <= SAFE_FLOAT_MAX {
            push_unique(&mut mantissas, p);
            if p > 1 {
                push_unique(&mut mantissas, p - 1);
            }
            if let Some(next) = p.checked_add(1)
                && next <= SAFE_FLOAT_MAX
            {
                push_unique(&mut mantissas, next);
            }

            match p.checked_mul(10) {
                Some(next) if next > p && next <= SAFE_FLOAT_MAX => p = next,
                _ => break,
            }
        }

        for delta in [
            0u128, 1, 2, 3, 7, 9, 10, 11, 99, 100, 101, 999, 1_000, 10_000,
        ] {
            if SAFE_FLOAT_MAX > delta {
                push_unique(&mut mantissas, SAFE_FLOAT_MAX - delta);
            }
        }

        mantissas.sort_unstable();
        mantissas.dedup();

        // Exponent sweep.
        // Keep it large enough to stress normalization / exponent math,
        // but still practical for f64 reference calculations.
        let exp_min: i64 = -120;
        let exp_max: i64 = 120;
        let exp_step: usize = 5;
        let exponents: Vec<i64> = (exp_min..=exp_max).step_by(exp_step).collect();

        let m_len = mantissas.len();
        let e_len = exponents.len();
        let total_cases = m_len * e_len * m_len * e_len;

        let checked = Arc::new(AtomicUsize::new(0));
        let skipped_non_finite = Arc::new(AtomicUsize::new(0));
        let skipped_invalid_sf = Arc::new(AtomicUsize::new(0));
        let done_counter = Arc::new(AtomicUsize::new(0));

        (0..total_cases).into_par_iter().for_each(|idx| {
            let mut rem = idx;

            let eb_idx = rem % e_len;
            rem /= e_len;

            let mb_idx = rem % m_len;
            rem /= m_len;

            let ea_idx = rem % e_len;
            rem /= e_len;

            let ma_idx = rem % m_len;

            let ma = mantissas[ma_idx];
            let ea = exponents[ea_idx];
            let mb = mantissas[mb_idx];
            let eb = exponents[eb_idx];

            let a = match SafeFloat::new(ma, ea) {
                Some(x) => x,
                None => {
                    skipped_invalid_sf.fetch_add(1, Ordering::Relaxed);
                    done_counter.fetch_add(1, Ordering::Relaxed);
                    return;
                }
            };

            let b = match SafeFloat::new(mb, eb) {
                Some(x) => x,
                None => {
                    skipped_invalid_sf.fetch_add(1, Ordering::Relaxed);
                    done_counter.fetch_add(1, Ordering::Relaxed);
                    return;
                }
            };

            let actual_sf = match a.div(&b) {
                Some(x) => x,
                None => {
                    skipped_invalid_sf.fetch_add(1, Ordering::Relaxed);
                    done_counter.fetch_add(1, Ordering::Relaxed);
                    return;
                }
            };

            let actual: f64 = actual_sf.into();
            let expected =
                (ma as f64 * 10_f64.powi(ea as i32)) / (mb as f64 * 10_f64.powi(eb as i32));

            if !actual.is_finite() || !expected.is_finite() {
                skipped_non_finite.fetch_add(1, Ordering::Relaxed);
            } else {
                let err = rel_err(actual, expected);

                assert!(
                    err <= 1e-12,
                    concat!(
                        "div mismatch:\n",
                        "  a = {}e{}\n",
                        "  b = {}e{}\n",
                        "  actual   = {:.20e}\n",
                        "  expected = {:.20e}\n",
                        "  rel_err  = {:.20e}"
                    ),
                    ma,
                    ea,
                    mb,
                    eb,
                    actual,
                    expected,
                    err
                );

                checked.fetch_add(1, Ordering::Relaxed);
            }

            let done = done_counter.fetch_add(1, Ordering::Relaxed) + 1;
            if done.is_multiple_of(10_000) {
                let progress = done as f64 / total_cases as f64 * 100.0;
                log::debug!("div progress = {progress:.4}%");
            }
        });

        let checked = checked.load(Ordering::Relaxed);
        let skipped_non_finite = skipped_non_finite.load(Ordering::Relaxed);
        let skipped_invalid_sf = skipped_invalid_sf.load(Ordering::Relaxed);

        println!(
            "div checked={}, skipped_non_finite={}, skipped_invalid_sf={}, mantissas={}, exponents={}, total_cases={}",
            checked,
            skipped_non_finite,
            skipped_invalid_sf,
            mantissas.len(),
            exponents.len(),
            total_cases,
        );

        assert!(checked > 0, "div test did not validate any finite cases");
    }
}
