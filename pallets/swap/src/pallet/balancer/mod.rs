//! Weighted-balancer AMM math for TAO (quote) ↔ alpha (base) swaps.
//!
//! Unlike Uniswap v2/v3, liquidity may be added off-price via weights `w1 + w2 = 1`
//! (`w1` = base/alpha, `w2` = quote/TAO). Only quote weight is stored; base = `1 - quote`.
//!
//! Formulas:
//! - Price: `p = (w1*y) / (w2*x)`
//! - Sell (`∆x` given): `∆y = y * ((x / (x+∆x))^(w1/w2) - 1)`
//! - Buy (`∆y` given): `∆x = x * ((y / (y+∆y))^(w2/w1) - 1)`
//! - Limit sell to `p' < p`: `∆x = x * ((p / p')^w2 - 1)`
//! - Limit buy to `p' > p`: `∆y = y * ((p' / p)^w1 - 1)`
//! - Init from reserves + price: `w1 = px / (px + y)`, `w2 = y / (px + y)`
//! - Reweight after injection (price-preserving):
//!   `new_w2 = (y + ∆y) / (p * (x + ∆x) + y + ∆y)`
//!
//! Weights are clamped to stay away from `{0,1}` so exponentiation stays stable; failed
//! injections go to per-subnet reservoirs instead of moving price.

use codec::{Decode, Encode, MaxEncodedLen};
use frame_support::pallet_prelude::*;
use safe_bigmath::*;
use safe_math::*;
use sp_arithmetic::Perquintill;
use sp_core::U256;
use sp_runtime::Saturating;
use sp_std::ops::Neg;
use substrate_fixed::types::U64F64;
use subtensor_macros::freeze_struct;

/// Balancer implements all high complexity math for swap operations such as:
///   - Swapping x for y, which includes limit orders
///   - Adding and removing liquidity (including unbalanced)
///
/// Notation used in this file:
///   - x: Base reserve (alpha reserve)
///   - y: Quote reserve (tao reserve)
///   - ∆x: Alpha paid in/out
///   - ∆y: Tao paid in/out
///   - w1: Base weight (a.k.a weight_base)
///   - w2: Quote weight (a.k.a weight_quote)
#[freeze_struct("33a4fb0774da77c7")]
#[derive(Clone, Encode, Decode, PartialEq, Eq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
pub struct Balancer {
    quote: Perquintill,
}

/// Accuracy matches to 18 decimal digits used to represent weights
pub const ACCURACY: u64 = 1_000_000_000_000_000_000_u64;
/// Lower limit of weights is 0.01
pub const MIN_WEIGHT: Perquintill = Perquintill::from_parts(ACCURACY / 100);
/// 1.0 in Perquintill
pub const ONE: Perquintill = Perquintill::from_parts(ACCURACY);

#[derive(Debug)]
pub enum BalancerError {
    /// The provided weight value is out of range
    InvalidValue,
}

impl Default for Balancer {
    /// The default value of weights is 0.5 for pool initialization
    fn default() -> Self {
        Self {
            quote: Perquintill::from_rational(1u128, 2u128),
        }
    }
}

impl Balancer {
    /// Creates a new instance of balancer with a given quote weight
    pub fn new(quote: Perquintill) -> Result<Self, BalancerError> {
        if Self::check_constraints(quote) {
            Ok(Balancer { quote })
        } else {
            Err(BalancerError::InvalidValue)
        }
    }

    /// Constraints limit balancer weights within certain range of values:
    ///   - Both weights are above minimum
    ///   - Sum of weights is equal to 1.0
    fn check_constraints(quote: Perquintill) -> bool {
        let base = ONE.saturating_sub(quote);
        (base >= MIN_WEIGHT) && (quote >= MIN_WEIGHT)
    }

    /// We store quote weight as Perquintill
    pub fn get_quote_weight(&self) -> Perquintill {
        self.quote
    }

    /// Base weight is calculated as 1.0 - quote_weight
    pub fn get_base_weight(&self) -> Perquintill {
        ONE.saturating_sub(self.quote)
    }

    /// Sets quote currency weight in the balancer.
    /// Because sum of weights is always 1.0, there is no need to
    /// store base currency weight
    pub fn set_quote_weight(&mut self, new_value: Perquintill) -> Result<(), BalancerError> {
        if Self::check_constraints(new_value) {
            self.quote = new_value;
            Ok(())
        } else {
            Err(BalancerError::InvalidValue)
        }
    }

    /// If base_quote is true, calculate (x / (x + ∆x))^(weight_base / weight_quote),
    /// otherwise, calculate (x / (x + ∆x))^(weight_quote / weight_base)
    ///
    /// Here we use SafeInt from bigmath crate for high-precision exponentiation,
    /// which exposes the function pow_ratio_scaled.
    ///
    /// Note: ∆x may be negative
    fn exp_scaled(&self, x: u64, dx: i128, base_quote: bool) -> U64F64 {
        let x_plus_dx = if dx >= 0 {
            x.saturating_add(dx as u64)
        } else {
            x.saturating_sub(dx.neg() as u64)
        };

        if x_plus_dx == 0 {
            return U64F64::saturating_from_num(0);
        }
        let w1: u128 = self.get_base_weight().deconstruct() as u128;
        let w2: u128 = self.get_quote_weight().deconstruct() as u128;

        let precision = 256;
        let x_safe = SafeInt::from(x);
        let w1_safe = SafeInt::from(w1);
        let w2_safe = SafeInt::from(w2);
        let perquintill_scale = SafeInt::from(ACCURACY as u128);
        let denominator = SafeInt::from(x_plus_dx);
        log::debug!("x = {:?}", x);
        log::debug!("dx = {:?}", dx);
        log::debug!("x_safe = {:?}", x_safe);
        log::debug!("denominator = {:?}", denominator);
        log::debug!("w1_safe = {:?}", w1_safe);
        log::debug!("w2_safe = {:?}", w2_safe);
        log::debug!("precision = {:?}", precision);
        log::debug!("perquintill_scale = {:?}", perquintill_scale);

        let maybe_result_safe_int = if base_quote {
            SafeInt::pow_ratio_scaled(
                &x_safe,
                &denominator,
                &w1_safe,
                &w2_safe,
                precision,
                &perquintill_scale,
            )
        } else {
            SafeInt::pow_ratio_scaled(
                &x_safe,
                &denominator,
                &w2_safe,
                &w1_safe,
                precision,
                &perquintill_scale,
            )
        };

        if let Some(result_safe_int) = maybe_result_safe_int
            && let Some(result_u64) = result_safe_int.to_u64()
        {
            let result = U64F64::saturating_from_num(result_u64)
                .safe_div(U64F64::saturating_from_num(ACCURACY));
            return if dx >= 0 {
                result.min(U64F64::from_num(1))
            } else {
                result
            };
        }
        U64F64::saturating_from_num(0)
    }

    /// Calculates exponent of (x / (x + ∆x)) ^ (w_base/w_quote)
    /// This method is used in sell swaps
    /// (∆x is given by user, ∆y is paid out by the pool)
    pub fn exp_base_quote(&self, x: u64, dx: u64) -> U64F64 {
        self.exp_scaled(x, dx as i128, true)
    }

    /// Calculates exponent of (y / (y + ∆y)) ^ (w_quote/w_base)
    /// This method is used in buy swaps
    /// (∆y is given by user, ∆x is paid out by the pool)
    pub fn exp_quote_base(&self, y: u64, dy: u64) -> U64F64 {
        self.exp_scaled(y, dy as i128, false)
    }

    /// Calculates price as (w1/w2) * (y/x), where
    ///   - w1 is base weight
    ///   - w2 is quote weight
    ///   - x is base reserve
    ///   - y is quote reserve
    pub fn calculate_price(&self, x: u64, y: u64) -> U64F64 {
        let w2_fixed = U64F64::saturating_from_num(self.get_quote_weight().deconstruct());
        let w1_fixed = U64F64::saturating_from_num(self.get_base_weight().deconstruct());
        let x_fixed = U64F64::saturating_from_num(x);
        let y_fixed = U64F64::saturating_from_num(y);
        w1_fixed
            .safe_div(w2_fixed)
            .saturating_mul(y_fixed.safe_div(x_fixed))
    }

    /// Multiply a u128 value by a Perquintill with u128 result rounded to the
    /// nearest integer
    fn mul_perquintill_round(p: Perquintill, value: u128) -> u128 {
        let parts = p.deconstruct() as u128;
        let acc = ACCURACY as u128;

        let num = U256::from(value).saturating_mul(U256::from(parts));
        let den = U256::from(acc);

        // Add 0.5 before integer division to achieve rounding to the nearest
        // integer
        let zero = U256::from(0);
        let res = num
            .saturating_add(den.checked_div(U256::from(2u8)).unwrap_or(zero))
            .checked_div(den)
            .unwrap_or(zero);
        res.min(U256::from(u128::MAX))
            .try_into()
            .unwrap_or_default()
    }

    /// When liquidity is added to balancer swap, it may be added with arbitrary proportion,
    /// not necessarily in the proportion of price, like with uniswap v2 or v3. In order to
    /// stay within balancer pool invariant, the weights need to be updated. Invariant:
    ///
    ///   L = x ^ weight_base * y ^ weight_quote
    ///
    /// Note that weights must remain within the proper range (both be above MIN_WEIGHT),
    /// so only reasonably small disproportions of updates are appropriate.
    pub fn update_weights_for_added_liquidity(
        &mut self,
        tao_reserve: u64,
        alpha_reserve: u64,
        tao_delta: u64,
        alpha_delta: u64,
    ) -> Result<(), BalancerError> {
        // Calculate new to-be reserves (do not update here)
        let tao_reserve_u128 = u64::from(tao_reserve) as u128;
        let alpha_reserve_u128 = u64::from(alpha_reserve) as u128;
        let tao_delta_u128 = u64::from(tao_delta) as u128;
        let alpha_delta_u128 = u64::from(alpha_delta) as u128;
        let new_tao_reserve_u128 = tao_reserve_u128.saturating_add(tao_delta_u128);
        let new_alpha_reserve_u128 = alpha_reserve_u128.saturating_add(alpha_delta_u128);

        // Calculate new weights
        let quantity_1: u128 = Self::mul_perquintill_round(
            self.get_base_weight(),
            tao_reserve_u128.saturating_mul(new_alpha_reserve_u128),
        );
        let quantity_2: u128 = Self::mul_perquintill_round(
            self.get_quote_weight(),
            alpha_reserve_u128.saturating_mul(new_tao_reserve_u128),
        );
        let q_sum = quantity_1.saturating_add(quantity_2);

        // Calculate new reserve weights
        let new_reserve_weight = if q_sum != 0 {
            // Both TAO and Alpha are non-zero, normal case
            Perquintill::from_rational(quantity_2, q_sum)
        } else {
            // Either TAO or Alpha reserve were and/or remain zero => Initialize weights to 0.5
            Perquintill::from_rational(1u128, 2u128)
        };

        self.set_quote_weight(new_reserve_weight)
    }

    /// Calculates quote delta needed to reach the price up when byuing
    /// This method is needed for limit orders.
    ///
    /// Formula is:
    ///   ∆y = y * ((price_new / price)^weight_base - 1)
    /// price_new >= price
    pub fn calculate_quote_delta_in(
        &self,
        current_price: U64F64,
        target_price: U64F64,
        reserve: u64,
    ) -> u64 {
        let base_numerator: u128 = target_price.to_bits();
        let base_denominator: u128 = current_price.to_bits();
        let w1_fixed: u128 = self.get_base_weight().deconstruct() as u128;
        let scale: u128 = 10u128.pow(18);

        let maybe_exp_result = SafeInt::pow_ratio_scaled(
            &SafeInt::from(base_numerator),
            &SafeInt::from(base_denominator),
            &SafeInt::from(w1_fixed),
            &SafeInt::from(ACCURACY),
            1024,
            &SafeInt::from(scale),
        );

        if let Some(exp_result_safe_int) = maybe_exp_result {
            let reserve_fixed = U64F64::saturating_from_num(reserve);
            let one = U64F64::saturating_from_num(1);
            let scale_fixed = U64F64::saturating_from_num(scale);
            let exp_result_fixed = if let Some(exp_result_u64) = exp_result_safe_int.to_u64() {
                U64F64::saturating_from_num(exp_result_u64)
            } else if u64::MAX < exp_result_safe_int {
                U64F64::saturating_from_num(u64::MAX)
            } else {
                U64F64::saturating_from_num(0)
            };
            reserve_fixed
                .saturating_mul(exp_result_fixed.safe_div(scale_fixed).saturating_sub(one))
                .saturating_to_num::<u64>()
        } else {
            0u64
        }
    }

    /// Calculates base delta needed to reach the price down when selling
    /// This method is needed for limit orders.
    ///
    /// Formula is:
    ///   ∆x = x * ((price / price_new)^weight_quote - 1)
    /// price_new <= price
    pub fn calculate_base_delta_in(
        &self,
        current_price: U64F64,
        target_price: U64F64,
        reserve: u64,
    ) -> u64 {
        let base_numerator: u128 = current_price.to_bits();
        let base_denominator: u128 = target_price.to_bits();
        let w2_fixed: u128 = self.get_quote_weight().deconstruct() as u128;
        let scale: u128 = 10u128.pow(18);

        let maybe_exp_result = SafeInt::pow_ratio_scaled(
            &SafeInt::from(base_numerator),
            &SafeInt::from(base_denominator),
            &SafeInt::from(w2_fixed),
            &SafeInt::from(ACCURACY),
            1024,
            &SafeInt::from(scale),
        );

        if let Some(exp_result_safe_int) = maybe_exp_result {
            let one = U64F64::saturating_from_num(1);
            let scale_fixed = U64F64::saturating_from_num(scale);
            let reserve_fixed = U64F64::saturating_from_num(reserve);
            let exp_result_fixed = if let Some(exp_result_u64) = exp_result_safe_int.to_u64() {
                U64F64::saturating_from_num(exp_result_u64)
            } else if u64::MAX < exp_result_safe_int {
                U64F64::saturating_from_num(u64::MAX)
            } else {
                U64F64::saturating_from_num(0)
            };
            reserve_fixed
                .saturating_mul(exp_result_fixed.safe_div(scale_fixed).saturating_sub(one))
                .saturating_to_num::<u64>()
        } else {
            0u64
        }
    }

    /// Calculates amount of Alpha that needs to be sold to get a given amount of TAO
    pub fn get_base_needed_for_quote(
        &self,
        tao_reserve: u64,
        alpha_reserve: u64,
        delta_tao: u64,
    ) -> u64 {
        let e = self.exp_scaled(tao_reserve, (delta_tao as i128).neg(), false);
        let one = U64F64::from_num(1);
        let alpha_reserve_fixed = U64F64::from_num(alpha_reserve);
        // e > 1 in this case
        alpha_reserve_fixed
            .saturating_mul(e.saturating_sub(one))
            .saturating_to_num::<u64>()
    }
}

#[cfg(all(test, feature = "std"))]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests;
