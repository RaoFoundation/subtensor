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

    /// Exact `self - a` for `self >= a`, computed in U256 at the finer of the two
    /// granularities so no digit of either operand is dropped before subtracting.
    ///
    /// The plain `sub` first truncates the smaller operand to the larger operand's
    /// granularity, so it can report a difference larger than the true one. This
    /// helper never does: if the exact result needs more than `SAFE_FLOAT_MAX_EXP`
    /// digits it is rounded toward zero, so the returned value is always `<=` the
    /// true difference. Returns `None` when `self < a` or the exponent gap does not
    /// fit in U256; callers fall back to `sub` in that (practically unreachable) case.
    pub fn sub_exact_floor(&self, a: &SafeFloat) -> Option<Self> {
        if a.is_zero() {
            return Some(self.clone());
        }
        if self.is_zero() {
            return None;
        }

        let exponent = self.exponent.min(a.exponent);
        let scale = |value: &SafeFloat| -> Option<U256> {
            let shift = value.exponent.checked_sub(exponent)?;
            let factor = U256::from(10).checked_pow(U256::from(shift as u64))?;
            U256::from(value.mantissa).checked_mul(factor)
        };
        let minuend = scale(self)?;
        let subtrahend = scale(a)?;
        let difference = minuend.checked_sub(subtrahend)?;

        let mut result = SafeFloat::zero();
        if result.normalize(&difference, exponent) {
            Some(result)
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
        // Zero is stored with exponent 0, which is not comparable by exponent.
        if self.is_zero() {
            return false;
        }
        if a.is_zero() {
            return true;
        }

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
    ///
    /// Writing a zero denominator means the pool has been fully drained and is being
    /// closed. Implementations must retire every share recorded for the pool at that
    /// point: any share written before the zero denominator must read back as zero (or
    /// absent) afterwards, so that a later deposit re-opens the pool with a clean ledger.
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

    /// Value of one member's shares: `floor(V * S / D)`, capped at `V`.
    ///
    /// The cap is a correctness invariant, not a workaround. Every member's shares `S` are a
    /// fraction of the denominator `D`, so no single member can ever own more than the whole
    /// pool value `V`. Rounding in `SafeFloat` can nevertheless drive stored `S` above `D`
    /// over many updates. Without the cap that rounding drift turns into a quote above `V`
    /// that callers would then treat as spendable, letting more alpha leave (or be credited
    /// elsewhere) than the pool holds. Clamping at `V` keeps the quote backed by real value.
    pub fn get_value(&self, key: &K) -> u64 {
        let shared_value: u64 = self.state_ops.get_shared_value();
        let current_share: SafeFloat = self.state_ops.get_share(key);
        let denominator: SafeFloat = self.state_ops.get_denominator();
        Self::try_get_value_from_parts(shared_value, &current_share, &denominator)
            .unwrap_or_default()
    }

    pub fn get_value_from_shares(&self, current_share: SafeFloat) -> u64 {
        let shared_value: u64 = self.state_ops.get_shared_value();
        let denominator: SafeFloat = self.state_ops.get_denominator();
        Self::try_get_value_from_parts(shared_value, &current_share, &denominator)
            .unwrap_or_default()
    }

    /// See `get_value` for why the result is capped at `shared_value`.
    fn try_get_value_from_parts(
        shared_value: u64,
        current_share: &SafeFloat,
        denominator: &SafeFloat,
    ) -> Option<u64> {
        let shared_value_sf = SafeFloat::new(shared_value as u128, 0)?;
        shared_value_sf
            .mul_div(current_share, denominator)
            .map(u64::from)
            .map(|value| value.min(shared_value))
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

        // A pool with no denominator, or with shares but no value, is (re)opened by a
        // deposit and always accepts it; otherwise the deposit must buy at least one share.
        if denominator.mantissa == 0 || (update > 0 && shared_value == 0) {
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

    /// Update the value associated with an item identified by the Key.
    ///
    /// Accounting invariant: the denominator `D` is the sum of all stored member shares.
    /// Every rounding in this function is chosen so that `sum(shares) <= D` can never be
    /// violated, and it holds with equality except for sub-ulp truncation of the member's
    /// own share:
    ///
    /// * The denominator is moved first, by the (rounded) `shares_per_update`.
    /// * The member share is then moved by the *exact* change the denominator actually
    ///   underwent, never by an independently rounded copy of `shares_per_update`. A member
    ///   can therefore never gain more shares than the denominator gained, and never lose
    ///   fewer shares than the denominator lost.
    /// * A withdrawal that leaves the pool with zero value closes the pool: the denominator
    ///   is written to zero, which retires every remaining share (see
    ///   [`SharePoolDataOperations::set_denominator`]). Nothing of value is lost because the
    ///   pool is empty, and no stale share can claim a later deposit.
    /// * A deposit into a pool that has shares but no value cannot be priced (it would divide
    ///   by zero and hand the depositor no shares). Such a pool is closed first and the
    ///   deposit re-opens it.
    pub fn update_value_for_one(&mut self, key: &K, update: i64) {
        let shared_value: u64 = self.state_ops.get_shared_value();
        let mut denominator: SafeFloat = self.state_ops.get_denominator();
        let magnitude: u64 = update.unsigned_abs();

        if update > 0 && shared_value == 0 && !denominator.is_zero() {
            // Shares with nothing behind them are worth exactly zero. Retire them so the
            // deposit below opens a clean pool instead of being donated to those shares.
            self.state_ops.set_denominator(SafeFloat::zero());
            denominator = SafeFloat::zero();
        }

        if denominator.is_zero() {
            // Initialize the pool. The first key gets all. A withdrawal from an empty pool
            // has nothing to remove and must not conjure shares.
            if update > 0 {
                let update_float: SafeFloat =
                    SafeFloat::new(magnitude as u128, 0).unwrap_or_default();
                self.state_ops.set_denominator(update_float.clone());
                self.state_ops.set_share(key, update_float);
            }
        } else {
            let current_share: SafeFloat = self.state_ops.get_share(key);
            let shares_per_update: SafeFloat =
                self.get_shares_per_update(update, shared_value, &denominator);

            // Handle SafeFloat overflows quietly here because this overflow of i64 exponent
            // is extremely hypothetical and should never happen in practice.
            let (mut new_denominator, mut new_current_share) = if update > 0 {
                let new_denominator = match denominator.add(&shares_per_update) {
                    Some(new_denominator) => new_denominator,
                    None => {
                        log::error!(
                            "SafeFloat::add overflow when adding {:?} to {:?}; keeping old denominator",
                            shares_per_update,
                            denominator,
                        );
                        denominator.clone()
                    }
                };
                // The member may gain at most what the denominator really gained.
                let denominator_gain = match new_denominator.sub_exact_floor(&denominator) {
                    Some(gain) => gain,
                    None => {
                        log::error!(
                            "SafeFloat::sub_exact_floor failed for {:?} - {:?}; using truncating sub",
                            new_denominator,
                            denominator,
                        );
                        new_denominator.sub(&denominator).unwrap_or_default()
                    }
                };
                let new_current_share = match current_share.add(&denominator_gain) {
                    Some(new_current_share) => new_current_share,
                    None => {
                        log::error!(
                            "SafeFloat::add overflow when adding {:?} to {:?}; keeping old current_share",
                            denominator_gain,
                            current_share,
                        );
                        current_share
                    }
                };
                (new_denominator, new_current_share)
            } else {
                let new_denominator = match denominator.sub(&shares_per_update) {
                    Some(new_denominator) => new_denominator,
                    None => {
                        log::error!(
                            "SafeFloat::sub overflow when subtracting {:?} from {:?}; keeping old denominator",
                            shares_per_update,
                            denominator,
                        );
                        denominator.clone()
                    }
                };
                // The member must lose at least what the denominator really lost. The exact
                // difference is representable here, so this is the true loss.
                let denominator_loss = match denominator.sub_exact_floor(&new_denominator) {
                    Some(loss) => loss,
                    None => {
                        log::error!(
                            "SafeFloat::sub_exact_floor failed for {:?} - {:?}; using truncating sub",
                            denominator,
                            new_denominator,
                        );
                        denominator.sub(&new_denominator).unwrap_or_default()
                    }
                };
                if denominator_loss.gt(&current_share) {
                    // Callers only withdraw up to the quoted value, so the loss never exceeds
                    // the share. Should it ever, the denominator must not drop below the sum
                    // of the remaining shares: take only this member's whole share out of it.
                    let new_denominator = denominator.sub(&current_share).unwrap_or_default();
                    (new_denominator, SafeFloat::zero())
                } else {
                    // `sub` returns None only for a zero share, which means nothing is left.
                    let new_current_share = current_share
                        .sub(&denominator_loss)
                        .unwrap_or_else(SafeFloat::zero);
                    (new_denominator, new_current_share)
                }
            };

            let updated_shared_value = if update >= 0 {
                shared_value.saturating_add(magnitude)
            } else {
                shared_value.saturating_sub(magnitude)
            };

            if update < 0 && updated_shared_value == 0 {
                // The pool is empty. Close it so no share, dust or stale, survives to claim
                // the next deposit.
                new_denominator = SafeFloat::zero();
                new_current_share = SafeFloat::zero();
            } else if update < 0
                && !new_current_share.is_zero()
                && Self::try_get_value_from_parts(
                    updated_shared_value,
                    &new_current_share,
                    &new_denominator,
                ) == Some(0)
                && let Some(denominator_without_dust) = new_denominator.sub(&new_current_share)
            {
                // Withdrawing the integer value reported for a position can leave a positive
                // fractional share worth less than one rao. If retained, later emissions can
                // make that supposedly drained position visible again. Canonicalize such
                // withdrawal dust to zero and remove it from the denominator so the remaining
                // pool shares continue to sum to the denominator. Never interpret a failed
                // valuation as zero.
                new_denominator = denominator_without_dust;
                new_current_share = SafeFloat::zero();
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
#[allow(clippy::unwrap_used, clippy::arithmetic_side_effects)]
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
            // Contract: a zero denominator closes the pool and retires every share.
            if update.is_zero() {
                self.share.clear();
            }
            self.denominator = update;
        }
    }

    /// Deterministic LCG so grind tests are reproducible without extra dependencies.
    fn lcg(seed: u64) -> impl FnMut() -> u64 {
        let mut state = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        move || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            state
        }
    }

    /// Sum of all stored shares, accumulated in SafeFloat.
    fn sum_of_shares(pool: &SharePool<u16, MockSharePoolDataOperations>) -> SafeFloat {
        pool.state_ops
            .share
            .values()
            .fold(SafeFloat::zero(), |acc, share| acc.add(share).unwrap())
    }

    /// `a / b` as f64, computed in SafeFloat first so tiny exponents do not underflow f64.
    fn ratio(a: &SafeFloat, b: &SafeFloat) -> f64 {
        if b.is_zero() {
            return 0.0;
        }
        a.div(b).map(f64::from).unwrap_or(f64::INFINITY)
    }

    /// Replays the audit grind (pool-wide dividend, small member deposit, partial member
    /// withdrawal, all in a random order) and returns the peak raw `S / D` seen for any
    /// member together with the peak `sum(S) / D`.
    fn grind(seed: u64, ops: usize) -> (f64, f64) {
        const RAO: u64 = 1_000_000_000;
        let mock_ops = MockSharePoolDataOperations::new();
        let mut pool = SharePool::<u16, MockSharePoolDataOperations>::new(mock_ops);
        let members = [1_u16, 2_u16];
        // Tiny genuine seeds, as in the audit.
        pool.update_value_for_one(&members[0], (6 * RAO / 1000) as i64);
        pool.update_value_for_one(&members[1], (6 * RAO / 1000) as i64);

        let mut next = lcg(seed);
        let mut peak_member_ratio = 0.0_f64;
        let mut peak_sum_ratio = 0.0_f64;
        for _ in 0..ops {
            let who = members[(next() % 2) as usize];
            match next() % 3 {
                0 => {
                    let dividend = 1 + (next() % (100 * RAO));
                    pool.update_value_for_all(dividend as i64);
                }
                1 => {
                    let amount = 1 + (next() % (RAO / 1000));
                    pool.update_value_for_one(&who, amount as i64);
                }
                _ => {
                    let quote = pool.get_value(&who);
                    if quote > 0 {
                        let amount = 1 + (next() % quote);
                        pool.update_value_for_one(&who, -(amount as i64));
                    }
                }
            }
            let denominator = pool.state_ops.get_denominator();
            let shared_value = pool.state_ops.get_shared_value();
            let mut sum_of_quotes = 0_u128;
            for member in &members {
                let share = pool.state_ops.get_share(member);
                peak_member_ratio = peak_member_ratio.max(ratio(&share, &denominator));
                sum_of_quotes += pool.get_value(member) as u128;
                // The uncapped quote floor(V * S / D) must already be within the pool: the
                // `min(V)` cap in `get_value` is a backstop that a reachable state never needs.
                let uncapped: u64 = SafeFloat::from(shared_value)
                    .mul_div(&share, &denominator)
                    .map(u64::from)
                    .unwrap_or_default();
                assert!(
                    uncapped <= shared_value,
                    "seed {seed}: uncapped quote {uncapped} exceeds pool value {shared_value}; the cap was needed"
                );
            }
            peak_sum_ratio = peak_sum_ratio.max(ratio(&sum_of_shares(&pool), &denominator));
            assert!(
                sum_of_quotes <= shared_value as u128,
                "seed {seed}: members are quoted {sum_of_quotes} but the pool only holds {shared_value}"
            );
        }
        (peak_member_ratio, peak_sum_ratio)
    }

    // The pre-fix accounting applied an independently rounded `shares_per_update` to the
    // denominator and to the member share, so `S / D` drifted above 1 without bound under
    // dividends interleaved with partial withdrawals (the audit measured 1458x in 299 ops).
    // With the denominator-derived share update the ratio is bounded by 1 on every step.
    // cargo test --package share-pool --lib -- tests::test_grind_keeps_member_share_within_denominator --exact
    #[test]
    fn test_grind_keeps_member_share_within_denominator() {
        let mut worst_member = 0.0_f64;
        let mut worst_sum = 0.0_f64;
        for seed in 0..200_u64 {
            let (member_ratio, sum_ratio) = grind(seed, 3000);
            worst_member = worst_member.max(member_ratio);
            worst_sum = worst_sum.max(sum_ratio);
        }
        println!("peak S/D over 200 seeds x 3000 ops = {worst_member:.18}");
        println!("peak sum(S)/D over 200 seeds x 3000 ops = {worst_sum:.18}");
        assert!(
            worst_sum <= 1.0,
            "sum of shares exceeded the denominator: {worst_sum}"
        );
        assert!(
            worst_member <= 1.0,
            "a member share exceeded the denominator: {worst_member}"
        );
    }

    // Deposits and withdrawals move the denominator and the member share by the same exact
    // amount. sum(S) never exceeds D, and the only slack below D is sub-ulp truncation of a
    // member's own share (when a deposit dwarfs its existing position) or of the denominator
    // (dust cleanup), each at most one unit in the 21st digit.
    // cargo test --package share-pool --lib -- tests::test_denominator_equals_sum_of_shares --exact
    #[test]
    fn test_denominator_equals_sum_of_shares() {
        let mock_ops = MockSharePoolDataOperations::new();
        let mut pool = SharePool::<u16, MockSharePoolDataOperations>::new(mock_ops);
        let members = [1_u16, 2_u16, 3_u16];
        let mut next = lcg(42);
        pool.update_value_for_one(&members[0], 1_000_000_000);
        for step in 0..5000 {
            let who = members[(next() % 3) as usize];
            let shared_value = pool.state_ops.get_shared_value();
            match next() % 4 {
                0 => pool.update_value_for_all(
                    (1 + next() % (shared_value / 10).clamp(1, 1_000_000_000_000_000)) as i64,
                ),
                1 | 2 => pool.update_value_for_one(
                    &who,
                    (1 + next() % shared_value.min(1_000_000_000_000_000)) as i64,
                ),
                _ => {
                    let quote = pool.get_value(&who);
                    if quote > 0 {
                        pool.update_value_for_one(
                            &who,
                            -((1 + next() % (quote / 10).max(1)) as i64),
                        );
                    }
                }
            }
            let denominator = pool.state_ops.get_denominator();
            let sum = sum_of_shares(&pool);
            assert!(
                !sum.gt(&denominator),
                "step {step}: sum(S) {sum:?} > D {denominator:?}"
            );
            let slack = denominator.sub_exact_floor(&sum).unwrap();
            assert!(
                ratio(&slack, &denominator) < 1e-15,
                "step {step}: D - sum(S) = {slack:?} is not sub-ulp of D {denominator:?}"
            );
        }
    }

    // A pool whose last member withdraws everything is closed (D == 0) and re-opens cleanly:
    // the next depositor owns the whole pool and old members quote nothing.
    // cargo test --package share-pool --lib -- tests::test_drained_pool_reopens_clean --exact
    #[test]
    fn test_drained_pool_reopens_clean() {
        let mock_ops = MockSharePoolDataOperations::new();
        let mut pool = SharePool::<u16, MockSharePoolDataOperations>::new(mock_ops);

        pool.update_value_for_one(&1, 1_000);
        pool.update_value_for_one(&2, 3_000);
        pool.update_value_for_all(400);
        assert_eq!(pool.get_value(&1), 1_100);
        assert_eq!(pool.get_value(&2), 3_300);

        pool.update_value_for_one(&1, -1_100);
        pool.update_value_for_one(&2, -3_300);
        assert_eq!(pool.state_ops.get_shared_value(), 0);
        assert!(pool.state_ops.get_denominator().is_zero());
        assert!(pool.state_ops.get_share(&1).is_zero());
        assert!(pool.state_ops.get_share(&2).is_zero());

        pool.update_value_for_one(&3, 700);
        assert_eq!(pool.get_value(&3), 700);
        pool.update_value_for_all(300);
        assert_eq!(pool.get_value(&3), 1_000);
        assert_eq!(pool.get_value(&1), 0);
        assert_eq!(pool.get_value(&2), 0);
        assert_eq!(
            pool.state_ops.get_denominator(),
            pool.state_ops.get_share(&3)
        );
    }

    // Legacy state: stored shares exceed the denominator (S = 2D for two members). The
    // inflated member drains the whole pool through the value cap. That must close the pool,
    // so the other inflated share cannot sweep a later depositor, who recovers in full.
    // cargo test --package share-pool --lib -- tests::test_stale_inflated_share_cannot_claim_later_deposit --exact
    #[test]
    fn test_stale_inflated_share_cannot_claim_later_deposit() {
        let mut mock_ops = MockSharePoolDataOperations::new();
        mock_ops.set_shared_value(1_000_000);
        mock_ops.set_denominator(100u64.into());
        mock_ops.set_share(&1, 200u64.into());
        mock_ops.set_share(&2, 200u64.into());
        let mut pool = SharePool::<u16, MockSharePoolDataOperations>::new(mock_ops);

        // Capped quote: the whole pool.
        assert_eq!(pool.get_value(&1), 1_000_000);
        pool.update_value_for_one(&1, -1_000_000);
        assert_eq!(pool.state_ops.get_shared_value(), 0);
        assert!(pool.state_ops.get_denominator().is_zero());
        assert!(
            pool.state_ops.get_share(&2).is_zero(),
            "stale share retired"
        );

        // Innocent later depositor.
        pool.update_value_for_one(&3, 50_000);
        assert_eq!(pool.get_value(&3), 50_000);
        assert_eq!(pool.get_value(&1), 0);
        assert_eq!(pool.get_value(&2), 0);
        pool.update_value_for_all(50_000);
        assert_eq!(pool.get_value(&3), 100_000);
        assert_eq!(pool.get_value(&2), 0);
        pool.update_value_for_one(&3, -100_000);
        assert_eq!(pool.state_ops.get_shared_value(), 0);
    }

    // A pool that still has shares but no value cannot price a deposit. It is closed and
    // re-opened by the deposit, so the depositor gets the whole pool instead of nothing.
    // cargo test --package share-pool --lib -- tests::test_deposit_into_valueless_pool_reopens_it --exact
    #[test]
    fn test_deposit_into_valueless_pool_reopens_it() {
        let mut mock_ops = MockSharePoolDataOperations::new();
        mock_ops.set_shared_value(0);
        mock_ops.set_denominator(100u64.into());
        mock_ops.set_share(&1, 300u64.into());
        let mut pool = SharePool::<u16, MockSharePoolDataOperations>::new(mock_ops);

        pool.update_value_for_one(&2, 5_000);
        assert_eq!(pool.get_value(&2), 5_000);
        assert_eq!(pool.get_value(&1), 0);
        assert_eq!(
            pool.state_ops.get_denominator(),
            pool.state_ops.get_share(&2)
        );
    }

    // A withdrawal from an empty pool must not create shares out of nothing.
    #[test]
    fn test_withdrawal_from_empty_pool_is_noop() {
        let mock_ops = MockSharePoolDataOperations::new();
        let mut pool = SharePool::<u16, MockSharePoolDataOperations>::new(mock_ops);
        pool.update_value_for_one(&1, -10);
        assert!(pool.state_ops.get_denominator().is_zero());
        assert!(pool.state_ops.get_share(&1).is_zero());
        assert_eq!(pool.state_ops.get_shared_value(), 0);
    }

    // Regression for the original unbacked-value report: under the pre-fix grind a single
    // member quote reached 5x the pool and the pair together far more. Now every quote is
    // backed on every step, and after the grind each member can withdraw exactly its quote
    // with the pool never going negative and the last member closing it.
    // cargo test --package share-pool --lib -- tests::test_no_unbacked_value_after_grind --exact
    #[test]
    fn test_no_unbacked_value_after_grind() {
        for seed in [5_u64, 17, 99, 1234] {
            let (member_ratio, _) = grind(seed, 333);
            assert!(member_ratio <= 1.0, "seed {seed}: S/D = {member_ratio}");
        }

        let mock_ops = MockSharePoolDataOperations::new();
        let mut pool = SharePool::<u16, MockSharePoolDataOperations>::new(mock_ops);
        let mut next = lcg(5);
        pool.update_value_for_one(&1, 6_000_000);
        pool.update_value_for_one(&2, 6_000_000);
        for _ in 0..2000 {
            let who = if next() % 2 == 0 { 1_u16 } else { 2_u16 };
            match next() % 3 {
                0 => pool.update_value_for_all((1 + next() % 100_000_000_000) as i64),
                1 => pool.update_value_for_one(&who, (1 + next() % 1_000_000) as i64),
                _ => {
                    let quote = pool.get_value(&who);
                    if quote > 0 {
                        pool.update_value_for_one(&who, -((1 + next() % quote) as i64));
                    }
                }
            }
        }
        let shared_value = pool.state_ops.get_shared_value();
        let quote_1 = pool.get_value(&1);
        let quote_2 = pool.get_value(&2);
        assert!(quote_1 as u128 + quote_2 as u128 <= shared_value as u128);
        pool.update_value_for_one(&1, -(quote_1 as i64));
        assert_eq!(pool.state_ops.get_shared_value(), shared_value - quote_1);
        let quote_2_after = pool.get_value(&2);
        assert!(
            quote_2_after >= quote_2,
            "member 2 must not lose value to member 1's exit"
        );
        pool.update_value_for_one(&2, -(quote_2_after as i64));
        let leftover = pool.state_ops.get_shared_value();
        assert!(
            leftover <= 2,
            "at most rounding dust may remain: {leftover}"
        );
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

    // cargo test --package share-pool --lib -- tests::test_get_value_is_capped_at_shared_value --exact
    #[test]
    fn test_get_value_is_capped_at_shared_value() {
        // Divergent state: the member's share S exceeds the denominator D (S/D = 1.5),
        // so the raw quote floor(V * S / D) = 150 is above the whole pool value V = 100.
        let mut mock_ops = MockSharePoolDataOperations::new();
        mock_ops.set_shared_value(100u64);
        mock_ops.set_denominator(10u64.into());
        mock_ops.set_share(&1_u16, 15u64.into());
        mock_ops.set_share(&2_u16, 5u64.into());
        let pool = SharePool::<u16, MockSharePoolDataOperations>::new(mock_ops);

        assert_eq!(
            pool.get_value(&1),
            100,
            "quote must never exceed pool value"
        );
        assert_eq!(pool.try_get_value(&1), Ok(100));
        assert_eq!(pool.get_value_from_shares(15u64.into()), 100);
        // A member whose S/D is below 1 is unaffected by the cap.
        assert_eq!(pool.get_value(&2), 50);

        // Extreme divergence (S/D = 10^21) still quotes exactly V.
        let mut mock_ops = MockSharePoolDataOperations::new();
        mock_ops.set_shared_value(1_000_000_000u64);
        mock_ops.set_denominator(SafeFloat::new(1u128, -21).unwrap());
        mock_ops.set_share(&1_u16, 1u64.into());
        let pool = SharePool::<u16, MockSharePoolDataOperations>::new(mock_ops);
        assert_eq!(pool.get_value(&1), 1_000_000_000);
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

    #[test]
    fn test_full_integer_withdrawal_clears_sub_rao_share_residue() {
        let mock_ops = MockSharePoolDataOperations::new();
        let mut pool = SharePool::<u16, MockSharePoolDataOperations>::new(mock_ops);

        pool.update_value_for_one(&1, 1);
        pool.update_value_for_one(&2, 2);
        pool.update_value_for_all(1);

        // Key 1 owns 4/3 rao but can only withdraw the displayed integer rao. Without dust
        // canonicalization this leaves a positive 1/3-rao share which later revives.
        assert_eq!(pool.get_value(&1), 1);
        pool.update_value_for_one(&1, -1);

        assert!(pool.state_ops.get_share(&1).is_zero());
        assert_eq!(pool.get_value(&2), 3);

        pool.update_value_for_all(10);
        assert_eq!(pool.get_value(&1), 0, "a drained position must not revive");
        assert_eq!(pool.get_value(&2), 13);
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
    fn test_safefloat_gt_handles_zero() {
        let zero = SafeFloat::zero();
        let small = SafeFloat::new(1u128, -12).unwrap();
        let large = SafeFloat::new(1u128, 12).unwrap();
        assert!(!zero.gt(&zero));
        assert!(!zero.gt(&small));
        assert!(!zero.gt(&large));
        assert!(small.gt(&zero));
        assert!(large.gt(&zero));
        assert!(large.gt(&small));
        assert!(!small.gt(&large));
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
            if done_outer % progress_step == 0 {
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
            if done % 10_000 == 0 {
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
