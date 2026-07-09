// Balancer swap
//
// Unlike uniswap v2 or v3, it allows adding liquidity disproportionally to price. This is
// achieved by introducing the weights w1 and w2 so that w1 + w2 = 1. In these formulas x
// means base currency (alpha) and y means quote currency (tao). The w1 weight in the code
// below is referred as weight_base, and w2 as weight_quote. Because of the w1 + w2 = 1
// constraint, only weight_quote is stored, and weight_base is always calculated.
//
// The formulas used for pool operation are following:
//
// Price: p = (w1*y) / (w2*x)
//
// Reserve deltas / (or -1 * payouts) in swaps are computed by:
//
//   if ∆x is given (sell) ∆y = y * ((x / (x+∆x))^(w1/w2) - 1)
//   if ∆y is given (buy)  ∆x = x * ((y / (y+∆y))^(w2/w1) - 1)
//
// When swaps are executing the orders with slippage control, we need to know what amount
// we can swap before the price reaches the limit value of p':
//
//   If p' < p (sell): ∆x = x * ((p / p')^w2 - 1)
//   If p' < p (buy):  ∆y = y * ((p' / p)^w1 - 1)
//
// In order to initialize weights with existing reserve values and price:
//
//   w1 = px / (px + y)
//   w2 = y / (px + y)
//
// Weights are adjusted when some amounts are added to the reserves. This prevents price
// from changing.
//
//   new_w1 = p * (x + ∆x) / (p * (x + ∆x) + y + ∆y)
//   new_w2 = (y + ∆y) / (p * (x + ∆x) + y + ∆y)
//
// Weights are limited to stay within [0.1, 0.9] range to avoid precision issues in exponentiation.
// Practically, these limitations will not be achieved, but if they are, the swap will not allow injection
// that will push the weights out of this interval because we prefer chain and swap stability over success
// of a single injection. Currently, we only allow the protocol to inject disproportionally to price, and
// the amount of disproportion will not cause weigths to get far from 0.5.
//

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
///   - x: Base reserve (alplha reserve)
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
/// Lower imit of weights is 0.01
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

// cargo test --package pallet-subtensor-swap --lib -- pallet::balancer::tests --nocapture
#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
#[cfg(feature = "std")]
mod tests {
    use crate::pallet::Balancer;
    use crate::pallet::balancer::*;
    use approx::assert_abs_diff_eq;
    use sp_arithmetic::Perquintill;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    // Helper: convert Perquintill to f64 for comparison
    fn perquintill_to_f64(p: Perquintill) -> f64 {
        let parts = p.deconstruct() as f64;
        parts / ACCURACY as f64
    }

    // Helper: convert U64F64 to f64 for comparison
    fn f(v: U64F64) -> f64 {
        v.to_num::<f64>()
    }

    fn assert_no_panic<R, F>(label: &str, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| panic!("{label} panicked"))
    }

    #[test]
    fn test_balancer_rejects_invalid_boundary_weights_without_panicking() {
        [
            Perquintill::zero(),
            Perquintill::from_parts(1),
            MIN_WEIGHT.saturating_sub(Perquintill::from_parts(1)),
            ONE.saturating_sub(MIN_WEIGHT)
                .saturating_add(Perquintill::from_parts(1)),
            ONE,
        ]
        .into_iter()
        .for_each(|quote| {
            assert_no_panic("Balancer::new invalid boundary weight", || {
                assert!(Balancer::new(quote).is_err());
            });
        });

        let mut balancer = Balancer::default();
        assert_no_panic("Balancer::set_quote_weight invalid boundary weight", || {
            assert!(balancer.set_quote_weight(Perquintill::zero()).is_err());
        });
        assert_eq!(
            balancer.get_quote_weight(),
            Perquintill::from_rational(1u128, 2u128)
        );
    }

    #[test]
    fn test_balancer_extreme_exp_inputs_do_not_panic() {
        let weights = [
            MIN_WEIGHT,
            Perquintill::from_rational(1u128, 2u128),
            ONE.saturating_sub(MIN_WEIGHT),
        ];
        let inputs = [
            (0u64, 0u64),
            (0u64, 1u64),
            (1u64, 0u64),
            (1u64, 1u64),
            (1u64, u64::MAX),
            (u64::MAX, 0u64),
            (u64::MAX, 1u64),
            (u64::MAX, u64::MAX),
        ];

        for quote in weights {
            let balancer = Balancer::new(quote).unwrap();
            for (reserve, delta) in inputs {
                assert_no_panic("exp_base_quote extreme input", || {
                    let _ = balancer.exp_base_quote(reserve, delta);
                });
                assert_no_panic("exp_quote_base extreme input", || {
                    let _ = balancer.exp_quote_base(reserve, delta);
                });
                assert_no_panic("exp_scaled negative extreme input", || {
                    let _ = balancer.exp_scaled(reserve, -(delta as i128), true);
                    let _ = balancer.exp_scaled(reserve, -(delta as i128), false);
                });
            }
        }
    }

    #[test]
    fn test_balancer_price_and_limit_delta_corner_cases_do_not_panic() {
        let balancer = Balancer::new(MIN_WEIGHT).unwrap();
        let prices = [
            U64F64::from_num(0),
            U64F64::from_num(1),
            U64F64::from_num(u64::MAX),
        ];
        let reserves = [0u64, 1u64, u64::MAX];

        for x in reserves {
            for y in reserves {
                assert_no_panic("calculate_price corner reserves", || {
                    let _ = balancer.calculate_price(x, y);
                });
            }
        }

        for current_price in prices {
            for target_price in prices {
                for reserve in reserves {
                    assert_no_panic("calculate_quote_delta_in corner input", || {
                        let _ =
                            balancer.calculate_quote_delta_in(current_price, target_price, reserve);
                    });
                    assert_no_panic("calculate_base_delta_in corner input", || {
                        let _ =
                            balancer.calculate_base_delta_in(current_price, target_price, reserve);
                    });
                }
            }
        }
    }

    #[test]
    fn test_balancer_liquidity_weight_update_extremes_do_not_panic() {
        let inputs = [
            (0u64, 0u64, 0u64, 0u64),
            (0u64, 0u64, u64::MAX, u64::MAX),
            (0u64, u64::MAX, u64::MAX, 0u64),
            (u64::MAX, 0u64, 0u64, u64::MAX),
            (u64::MAX, u64::MAX, u64::MAX, u64::MAX),
            (1u64, u64::MAX, u64::MAX, 1u64),
            (u64::MAX, 1u64, 1u64, u64::MAX),
        ];

        for (tao_reserve, alpha_reserve, tao_delta, alpha_delta) in inputs {
            let mut balancer = Balancer::default();
            assert_no_panic("update_weights_for_added_liquidity extreme input", || {
                let _ = balancer.update_weights_for_added_liquidity(
                    tao_reserve,
                    alpha_reserve,
                    tao_delta,
                    alpha_delta,
                );
            });
        }
    }

    #[test]
    fn test_balancer_base_needed_for_quote_extremes_do_not_panic() {
        let balancer = Balancer::new(ONE.saturating_sub(MIN_WEIGHT)).unwrap();
        let inputs = [
            (0u64, 0u64, 0u64),
            (0u64, 1u64, 1u64),
            (1u64, 0u64, 1u64),
            (1u64, 1u64, 0u64),
            (1u64, 1u64, 1u64),
            (1u64, 1u64, u64::MAX),
            (u64::MAX, u64::MAX, 0u64),
            (u64::MAX, u64::MAX, u64::MAX),
        ];

        for (tao_reserve, alpha_reserve, delta_tao) in inputs {
            assert_no_panic("get_base_needed_for_quote extreme input", || {
                let _ = balancer.get_base_needed_for_quote(tao_reserve, alpha_reserve, delta_tao);
            });
        }
    }

    #[test]
    fn test_safe_bigmath_pow_ratio_internal_paths_do_not_panic() {
        let base_num = SafeInt::from(999_999_937u64);
        let base_den = SafeInt::from(1_000_000_003u64);
        let scale = SafeInt::from(1_000_000u64);
        let cases = [
            // Exact integer/root path with exponent values at the safe-bigmath threshold.
            (
                SafeInt::from(1024u32),
                SafeInt::one(),
                "exact max numerator",
            ),
            (
                SafeInt::from(999u32),
                SafeInt::from(1024u32),
                "exact root denominator",
            ),
            // One step over the threshold forces the fixed-point ln/exp fallback path.
            (SafeInt::from(1025u32), SafeInt::one(), "fallback numerator"),
            (
                SafeInt::from(999u32),
                SafeInt::from(1025u32),
                "fallback denominator",
            ),
            // GCD reduction should route this back to the exact path.
            (
                SafeInt::from(2048u32),
                SafeInt::from(4096u32),
                "gcd reduced",
            ),
        ];

        for (exp_num, exp_den, label) in cases {
            let result = assert_no_panic(label, || {
                SafeInt::pow_ratio_scaled(&base_num, &base_den, &exp_num, &exp_den, 64, &scale)
            });
            assert!(result.is_some(), "{label} should produce a result");
        }
    }

    #[test]
    fn test_balancer_near_equal_weights_with_tiny_delta_do_not_panic() {
        let weights = [
            Perquintill::from_parts(500_000_000_500_000_000),
            Perquintill::from_parts(499_999_999_500_000_000),
            Perquintill::from_parts(500_000_000_000_500_000),
            Perquintill::from_parts(499_999_999_999_500_000),
        ];
        let reserve = 21_000_000_000_000_000u64;
        let tiny_deltas = [1u64, 100u64, 100_000u64];

        for quote in weights {
            let balancer = Balancer::new(quote).unwrap();
            for delta in tiny_deltas {
                assert_no_panic("near-equal exp_base_quote tiny delta", || {
                    let e = balancer.exp_base_quote(reserve, delta);
                    assert!(e <= U64F64::from_num(1));
                    assert!(e > U64F64::from_num(0));
                });
                assert_no_panic("near-equal exp_quote_base tiny delta", || {
                    let e = balancer.exp_quote_base(reserve, delta);
                    assert!(e <= U64F64::from_num(1));
                    assert!(e > U64F64::from_num(0));
                });
            }
        }
    }

    #[test]
    fn test_balancer_log_normalization_reserve_shapes_do_not_panic() {
        let balancer = Balancer::new(Perquintill::from_parts(500_000_000_500_000_000)).unwrap();
        let reserves = [
            (1u64 << 42) - 1,
            1u64 << 42,
            (1u64 << 42) + 1,
            ((1u64 << 42) + (1u64 << 41)) - 1,
            (1u64 << 42) + (1u64 << 41),
            ((1u64 << 42) + (1u64 << 41)) + 1,
        ];

        for reserve in reserves {
            for delta in [1u64, reserve / 1_000, reserve / 2] {
                assert_no_panic("log-normalization exp_base_quote", || {
                    let e = balancer.exp_base_quote(reserve, delta);
                    assert!(e <= U64F64::from_num(1));
                });
                assert_no_panic("log-normalization exp_quote_base", || {
                    let e = balancer.exp_quote_base(reserve, delta);
                    assert!(e <= U64F64::from_num(1));
                });
            }
        }
    }

    #[test]
    fn test_perquintill_power() {
        const PRECISION: u32 = 4096;
        const PERQUINTILL: u128 = ACCURACY as u128;

        let x = SafeInt::from(21_000_000_000_000_000u64);
        let delta = SafeInt::from(7_000_000_000_000_000u64);
        let w1 = SafeInt::from(600_000_000_000_000_000u128);
        let w2 = SafeInt::from(400_000_000_000_000_000u128);
        let denominator = &x + &delta;
        assert_eq!(w1.clone() + w2.clone(), SafeInt::from(PERQUINTILL));

        let perquintill_result = SafeInt::pow_ratio_scaled(
            &x,
            &denominator,
            &w1,
            &w2,
            PRECISION,
            &SafeInt::from(PERQUINTILL),
        )
        .expect("perquintill integer result");

        assert_eq!(
            perquintill_result,
            SafeInt::from(649_519_052_838_328_985u128)
        );
        let readable = safe_bigmath::SafeDec::<18>::from_raw(perquintill_result);
        assert_eq!(format!("{}", readable), "0.649519052838328985");
    }

    /// Validate realistic values that can be calculated with f64 precision
    #[test]
    fn test_exp_base_quote_happy_path() {
        // Outer test cases: w_quote
        [
            Perquintill::from_rational(500_000_000_000_u128, 1_000_000_000_000_u128),
            Perquintill::from_rational(500_000_000_001_u128, 1_000_000_000_000_u128),
            Perquintill::from_rational(499_999_999_999_u128, 1_000_000_000_000_u128),
            Perquintill::from_rational(500_000_000_100_u128, 1_000_000_000_000_u128),
            Perquintill::from_rational(500_000_001_000_u128, 1_000_000_000_000_u128),
            Perquintill::from_rational(500_000_010_000_u128, 1_000_000_000_000_u128),
            Perquintill::from_rational(500_000_100_000_u128, 1_000_000_000_000_u128),
            Perquintill::from_rational(500_001_000_000_u128, 1_000_000_000_000_u128),
            Perquintill::from_rational(500_010_000_000_u128, 1_000_000_000_000_u128),
            Perquintill::from_rational(500_100_000_000_u128, 1_000_000_000_000_u128),
            Perquintill::from_rational(501_000_000_000_u128, 1_000_000_000_000_u128),
            Perquintill::from_rational(510_000_000_000_u128, 1_000_000_000_000_u128),
            Perquintill::from_rational(100_000_000_000_u128, 1_000_000_000_000_u128),
            Perquintill::from_rational(100_000_000_001_u128, 1_000_000_000_000_u128),
            Perquintill::from_rational(200_000_000_000_u128, 1_000_000_000_000_u128),
            Perquintill::from_rational(300_000_000_000_u128, 1_000_000_000_000_u128),
            Perquintill::from_rational(400_000_000_000_u128, 1_000_000_000_000_u128),
            Perquintill::from_rational(600_000_000_000_u128, 1_000_000_000_000_u128),
            Perquintill::from_rational(700_000_000_000_u128, 1_000_000_000_000_u128),
            Perquintill::from_rational(800_000_000_000_u128, 1_000_000_000_000_u128),
            Perquintill::from_rational(899_999_999_999_u128, 1_000_000_000_000_u128),
            Perquintill::from_rational(900_000_000_000_u128, 1_000_000_000_000_u128),
            Perquintill::from_rational(
                102_337_248_363_782_924_u128,
                1_000_000_000_000_000_000_u128,
            ),
        ]
        .into_iter()
        .for_each(|w_quote| {
            // Inner test cases: y, x, ∆x
            [
                (1_000_u64, 1_000_u64, 0_u64),
                (1_000_u64, 1_000_u64, 1_u64),
                (1_500_u64, 1_000_u64, 1_u64),
                (
                    1_000_000_000_000_u64,
                    100_000_000_000_000_u64,
                    100_000_000_u64,
                ),
                (
                    1_000_000_000_000_u64,
                    100_000_000_000_000_u64,
                    100_000_000_u64,
                ),
                (
                    100_000_000_000_u64,
                    100_000_000_000_000_u64,
                    100_000_000_u64,
                ),
                (100_000_000_000_u64, 100_000_000_000_000_u64, 1_000_000_u64),
                (
                    100_000_000_000_u64,
                    100_000_000_000_000_u64,
                    1_000_000_000_000_u64,
                ),
                (
                    1_000_000_000_u64,
                    100_000_000_000_000_u64,
                    1_000_000_000_000_u64,
                ),
                (
                    1_000_000_u64,
                    100_000_000_000_000_u64,
                    1_000_000_000_000_u64,
                ),
                (1_000_u64, 100_000_000_000_000_u64, 1_000_000_000_000_u64),
                (1_000_u64, 100_000_000_000_000_u64, 1_000_000_000_u64),
                (1_000_u64, 100_000_000_000_000_u64, 1_000_000_u64),
                (1_000_u64, 100_000_000_000_000_u64, 1_000_u64),
                (1_000_u64, 100_000_000_000_000_u64, 100_000_000_000_000_u64),
                (10_u64, 100_000_000_000_000_u64, 100_000_000_000_000_u64),
                // Extreme values of ∆x for small x
                (1_000_000_000_u64, 4_000_000_000_u64, 1_000_000_000_000_u64),
                (1_000_000_000_000_u64, 1_000_u64, 1_000_000_000_000_u64),
                (
                    5_628_038_062_729_553_u64,
                    400_775_553_u64,
                    14_446_633_907_665_582_u64,
                ),
                (
                    5_600_000_000_000_000_u64,
                    400_000_000_u64,
                    14_000_000_000_000_000_u64,
                ),
            ]
            .into_iter()
            .for_each(|(y, x, dx)| {
                let bal = Balancer::new(w_quote).unwrap();
                let e1 = bal.exp_base_quote(x, dx);
                let e2 = bal.exp_quote_base(x, dx);
                let one = U64F64::from_num(1);
                let y_fixed = U64F64::from_num(y);
                let dy1 = y_fixed * (one - e1);
                let dy2 = y_fixed * (one - e2);

                if dx > x.saturating_mul(1_000) {
                    assert!(e1 <= one);
                    assert!(e2 <= one);
                    return;
                }

                let w1 = perquintill_to_f64(bal.get_base_weight());
                let w2 = perquintill_to_f64(bal.get_quote_weight());
                let e1_expected = (x as f64 / (x as f64 + dx as f64)).powf(w1 / w2);
                let dy1_expected = y as f64 * (1. - e1_expected);
                let e2_expected = (x as f64 / (x as f64 + dx as f64)).powf(w2 / w1);
                let dy2_expected = y as f64 * (1. - e2_expected);

                // Start tolerance with 0.001 rao
                let mut eps1 = 0.001;
                let mut eps2 = 0.001;

                // If swapping more than 100k tao/alpha, relax tolerance to 1.0 rao
                if dy1_expected > 100_000_000_000_000_f64 {
                    eps1 = 1.0;
                }
                if dy2_expected > 100_000_000_000_000_f64 {
                    eps2 = 1.0;
                }
                assert_abs_diff_eq!(f(dy1), dy1_expected, epsilon = eps1);
                assert_abs_diff_eq!(f(dy2), dy2_expected, epsilon = eps2);
            })
        });
    }

    /// This test exercises practical application edge cases of exp_base_quote
    /// The practical formula where this function is used:
    ///    ∆y = y * (exp_base_quote(x, ∆x) - 1)
    ///
    /// The test validates that two different sets of parameters produce (sensibly)
    /// different results
    ///
    #[test]
    fn test_exp_base_quote_dy_precision() {
        // Test cases: y, x1, ∆x1, w_quote1, x2, ∆x2, w_quote2
        // Realized dy1 should be greater than dy2
        [
            (
                1_000_000_000_u64,
                21_000_000_000_000_000_u64,
                21_000_000_000_u64,
                Perquintill::from_rational(1_000_000_000_000_u128, 2_000_000_000_000_u128),
                21_000_000_000_000_000_u64,
                21_000_000_000_u64,
                Perquintill::from_rational(1_000_000_000_001_u128, 2_000_000_000_000_u128),
            ),
            (
                1_000_000_000_u64,
                21_000_000_000_000_000_u64,
                21_000_000_000_u64,
                Perquintill::from_rational(1_000_000_000_000_u128, 2_000_000_000_001_u128),
                21_000_000_000_000_000_u64,
                21_000_000_000_u64,
                Perquintill::from_rational(1_000_000_000_000_u128, 2_000_000_000_000_u128),
            ),
            (
                1_000_000_000_u64,
                21_000_000_000_000_000_u64,
                2_u64,
                Perquintill::from_rational(1_000_000_000_000_u128, 2_000_000_000_000_u128),
                21_000_000_000_000_000_u64,
                1_u64,
                Perquintill::from_rational(1_000_000_000_000_u128, 2_000_000_000_000_u128),
            ),
            (
                1_000_000_000_u64,
                21_000_000_000_000_000_u64,
                1_u64,
                Perquintill::from_rational(1_000_000_000_000_u128, 2_000_000_000_000_u128),
                21_000_000_000_000_000_u64,
                1_u64,
                Perquintill::from_rational(1_010_000_000_000_u128, 2_000_000_000_000_u128),
            ),
            (
                1_000_000_000_u64,
                21_000_000_000_000_000_u64,
                1_u64,
                Perquintill::from_rational(1_000_000_000_000_u128, 2_010_000_000_000_u128),
                21_000_000_000_000_000_u64,
                1_u64,
                Perquintill::from_rational(1_000_000_000_000_u128, 2_000_000_000_000_u128),
            ),
        ]
        .into_iter()
        .for_each(|(y, x1, dx1, w_quote1, x2, dx2, w_quote2)| {
            let bal1 = Balancer::new(w_quote1).unwrap();
            let bal2 = Balancer::new(w_quote2).unwrap();

            let exp1 = bal1.exp_base_quote(x1, dx1);
            let exp2 = bal2.exp_base_quote(x2, dx2);

            let one = U64F64::from_num(1);
            let y_fixed = U64F64::from_num(y);
            let dy1 = y_fixed * (one - exp1);
            let dy2 = y_fixed * (one - exp2);

            assert!(dy1 > dy2);

            let zero = U64F64::from_num(0);
            assert!(dy1 != zero);
            assert!(dy2 != zero);
        })
    }

    /// Test the broad range of w_quote values, usually should be ignored
    #[ignore]
    #[test]
    fn test_exp_quote_broad_range() {
        let y = 1_000_000_000_000_u64;
        let x = 100_000_000_000_000_u64;
        let dx = 10_000_000_u64;

        let mut prev = U64F64::from_num(1_000_000_000);
        let mut last_progress = 0.;
        let start = 100_000_000_000_u128;
        let stop = 900_000_000_000_u128;
        for num in (start..=stop).step_by(1000_usize) {
            let w_quote = Perquintill::from_rational(num, 1_000_000_000_000_u128);
            let bal = Balancer::new(w_quote).unwrap();
            let e = bal.exp_base_quote(x, dx);

            let one = U64F64::from_num(1);
            let dy = U64F64::from_num(y) * (one - e);

            let progress = (num as f64 - start as f64) / (stop as f64 - start as f64);
            if progress - last_progress >= 0.0001 {
                // Replace with println for real-time progress
                log::debug!("progress = {:?}%", progress * 100.);
                log::debug!("dy = {:?}", dy);
                last_progress = progress;
            }

            assert!(dy != U64F64::from_num(0));
            assert!(dy <= prev);
            prev = dy;
        }
    }

    // cargo test --package pallet-subtensor-swap --lib -- pallet::balancer::tests::test_exp_quote_fuzzy --include-ignored --exact --nocapture
    #[ignore]
    #[test]
    fn test_exp_quote_fuzzy() {
        use rand::rngs::StdRng;
        use rand::{Rng, SeedableRng};
        use rayon::prelude::*;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        const ITERATIONS: usize = 1_000_000_000;
        let counter = Arc::new(AtomicUsize::new(0));

        (0..ITERATIONS)
        .into_par_iter()
        .for_each(|i| {
            // Each iteration gets its own deterministic RNG.
            // Seed depends on i, so runs are reproducible.
            let mut rng = StdRng::seed_from_u64(42 + i as u64);
            let max_supply: u64 = 21_000_000_000_000_000;
            let full_range = true;

            let x: u64 = rng.gen_range(1_000..=max_supply); // Alpha reserve
            let y: u64 = if full_range {
                // TAO reserve (allow huge prices)
                rng.gen_range(1_000..=max_supply)
            } else {
                // TAO reserve (limit prices with 0-1000)
                rng.gen_range(1_000..x.saturating_mul(1000).min(max_supply))
            };
            let dx: u64 = if full_range {
                // Alhpa sold (allow huge values)
                rng.gen_range(1_000..=21_000_000_000_000_000)
            } else {
                // Alhpa sold (do not sell more than 100% of what's in alpha reserve)
                rng.gen_range(1_000..=x)
            };
            let w_numerator: u64 = rng.gen_range(ACCURACY / 10..=ACCURACY / 10 * 9);
            let w_quote = Perquintill::from_rational(w_numerator, ACCURACY);

            let bal = Balancer::new(w_quote).unwrap();
            let e = bal.exp_base_quote(x, dx);

            let one = U64F64::from_num(1);
            let dy = U64F64::from_num(y) * (one - e);

            // Calculate expected in f64 and approx-assert
            let w1 = perquintill_to_f64(bal.get_base_weight());
            let w2 = perquintill_to_f64(bal.get_quote_weight());
            let e_expected = (x as f64 / (x as f64 + dx as f64)).powf(w1 / w2);
            let dy_expected = y as f64 * (1. - e_expected);

            let actual = dy.to_num::<f64>();
            let eps = (dy_expected / 1_000_000.).clamp(1.0, 1000.0);

            assert!(
                (actual - dy_expected).abs() <= eps,
                "dy mismatch:\n actual:   {}\n expected: {}\n eps: {}\nParameters:\n x:  {}\n y:  {}\n dx: {}\n w_numerator: {}\n",
                actual, dy_expected, eps, x, y, dx, w_numerator,
            );

            // Assert that we aren't giving out more than reserve y
            assert!(dy <= y, "dy = {},\ny =  {}", dy, y,);            

            // Print progress
            let done = counter.fetch_add(1, Ordering::Relaxed) + 1;
            if done % 10_000_000 == 0 {
                let progress = done as f64 / ITERATIONS as f64 * 100.0;
                // Replace with println for real-time progress
                log::debug!("progress = {progress:.4}%");
            }
        });
    }

    #[test]
    fn test_calculate_quote_delta_in() {
        let num = 250_000_000_000_u128; // w1 = 0.75 
        let w_quote = Perquintill::from_rational(num, 1_000_000_000_000_u128);
        let bal = Balancer::new(w_quote).unwrap();

        let current_price: U64F64 = U64F64::from_num(0.1);
        let target_price: U64F64 = U64F64::from_num(0.2);
        let tao_reserve: u64 = 1_000_000_000;

        let dy = bal.calculate_quote_delta_in(current_price, target_price, tao_reserve);

        // ∆y = y•[(p'/p)^w1 - 1]
        let dy_expected = tao_reserve as f64
            * ((target_price.to_num::<f64>() / current_price.to_num::<f64>()).powf(0.75) - 1.0);

        assert_eq!(dy, dy_expected as u64,);
    }

    #[test]
    fn test_calculate_base_delta_in() {
        let num = 250_000_000_000_u128; // w2 = 0.25 
        let w_quote = Perquintill::from_rational(num, 1_000_000_000_000_u128);
        let bal = Balancer::new(w_quote).unwrap();

        let current_price: U64F64 = U64F64::from_num(0.2);
        let target_price: U64F64 = U64F64::from_num(0.1);
        let alpha_reserve: u64 = 1_000_000_000;

        let dx = bal.calculate_base_delta_in(current_price, target_price, alpha_reserve);

        // ∆x = x•[(p/p')^w2 - 1]
        let dx_expected = alpha_reserve as f64
            * ((current_price.to_num::<f64>() / target_price.to_num::<f64>()).powf(0.25) - 1.0);

        assert_eq!(dx, dx_expected as u64,);
    }

    #[test]
    fn test_calculate_quote_delta_in_impossible() {
        let num = 250_000_000_000_u128; // w1 = 0.75 
        let w_quote = Perquintill::from_rational(num, 1_000_000_000_000_u128);
        let bal = Balancer::new(w_quote).unwrap();

        // Impossible price (lower)
        let current_price: U64F64 = U64F64::from_num(0.1);
        let target_price: U64F64 = U64F64::from_num(0.05);
        let tao_reserve: u64 = 1_000_000_000;

        let dy = bal.calculate_quote_delta_in(current_price, target_price, tao_reserve);
        let dy_expected = 0u64;

        assert_eq!(dy, dy_expected);
    }

    #[test]
    fn test_calculate_base_delta_in_impossible() {
        let num = 250_000_000_000_u128; // w2 = 0.25 
        let w_quote = Perquintill::from_rational(num, 1_000_000_000_000_u128);
        let bal = Balancer::new(w_quote).unwrap();

        // Impossible price (higher)
        let current_price: U64F64 = U64F64::from_num(0.1);
        let target_price: U64F64 = U64F64::from_num(0.2);
        let alpha_reserve: u64 = 1_000_000_000;

        let dx = bal.calculate_base_delta_in(current_price, target_price, alpha_reserve);
        let dx_expected = 0u64;

        assert_eq!(dx, dx_expected);
    }

    #[test]
    fn test_calculate_delta_in_reverse_swap() {
        let num = 500_000_000_000_u128;
        let w_quote = Perquintill::from_rational(num, 1_000_000_000_000_u128);
        let bal = Balancer::new(w_quote).unwrap();

        let current_price: U64F64 = U64F64::from_num(0.1);
        let target_price: U64F64 = U64F64::from_num(0.2);
        let tao_reserve: u64 = 1_000_000_000;

        // Here is the simple case of w1 = w2 = 0.5, so alpha = tao / price
        let alpha_reserve: u64 = (tao_reserve as f64 / current_price.to_num::<f64>()) as u64;

        let dy = bal.calculate_quote_delta_in(current_price, target_price, tao_reserve);
        let dx = alpha_reserve as f64
            * (1.0
                - (tao_reserve as f64 / (tao_reserve as f64 + dy as f64))
                    .powf(num as f64 / (1_000_000_000_000 - num) as f64));

        // Verify that buying with dy will in fact bring the price to target_price
        let actual_price = bal.calculate_price(alpha_reserve - dx as u64, tao_reserve + dy);
        assert_abs_diff_eq!(
            actual_price.to_num::<f64>(),
            target_price.to_num::<f64>(),
            epsilon = target_price.to_num::<f64>() / 1_000_000_000.
        );
    }

    #[test]
    fn test_mul_round_zero_and_one() {
        let v = 1_000_000u128;

        // p = 0 -> always 0
        assert_eq!(Balancer::mul_perquintill_round(Perquintill::zero(), v), 0);

        // p = 1 -> identity
        assert_eq!(Balancer::mul_perquintill_round(Perquintill::one(), v), v);
    }

    #[test]
    fn test_mul_round_half_behaviour() {
        // p = 1/2
        let p = Perquintill::from_rational(1u128, 2u128);

        // Check rounding around .5 boundaries
        // value * 1/2, rounded to nearest
        assert_eq!(Balancer::mul_perquintill_round(p, 0), 0); // 0.0  -> 0
        assert_eq!(Balancer::mul_perquintill_round(p, 1), 1); // 0.5  -> 1 (round up)
        assert_eq!(Balancer::mul_perquintill_round(p, 2), 1); // 1.0  -> 1
        assert_eq!(Balancer::mul_perquintill_round(p, 3), 2); // 1.5  -> 2
        assert_eq!(Balancer::mul_perquintill_round(p, 4), 2); // 2.0  -> 2
        assert_eq!(Balancer::mul_perquintill_round(p, 5), 3); // 2.5  -> 3
        assert_eq!(Balancer::mul_perquintill_round(p, 1023), 512); // 511.5  -> 512
        assert_eq!(Balancer::mul_perquintill_round(p, 1025), 513); // 512.5  -> 513
    }

    #[test]
    fn test_mul_round_third_behaviour() {
        // p = 1/3
        let p = Perquintill::from_rational(1u128, 3u128);

        // value * 1/3, rounded to nearest
        assert_eq!(Balancer::mul_perquintill_round(p, 3), 1); // 1.0      -> 1
        assert_eq!(Balancer::mul_perquintill_round(p, 4), 1); // 1.333... -> 1
        assert_eq!(Balancer::mul_perquintill_round(p, 5), 2); // 1.666... -> 2
        assert_eq!(Balancer::mul_perquintill_round(p, 6), 2); // 2.0      -> 2
    }

    #[test]
    fn test_mul_round_large_values_simple_rational() {
        // p = 7/10 (exact in perquintill: 0.7)
        let p = Perquintill::from_rational(7u128, 10u128);
        let v: u128 = 1_000_000_000_000_000_000;

        let res = Balancer::mul_perquintill_round(p, v);

        // Expected = round(0.7 * v) with pure integer math:
        // round(v * 7 / 10) = (v*7 + 10/2) / 10
        let expected = (v.saturating_mul(7) + 10 / 2) / 10;

        assert_eq!(res, expected);
    }

    #[test]
    fn test_mul_round_max_value_with_one() {
        let v = u128::MAX;
        let p = ONE;

        // For p = 1, result must be exactly value, and must not overflow
        let res = Balancer::mul_perquintill_round(p, v);
        assert_eq!(res, v);
    }

    #[test]
    fn test_price_with_equal_weights_is_y_over_x() {
        // quote = 0.5, base = 0.5 -> w1 / w2 = 1, so price = y/x
        let quote = Perquintill::from_rational(1u128, 2u128);
        let bal = Balancer::new(quote).unwrap();

        let x = 2u64;
        let y = 5u64;

        let price = bal.calculate_price(x, y);
        let price_f = f(price);

        let expected_f = (y as f64) / (x as f64);
        assert_abs_diff_eq!(price_f, expected_f, epsilon = 1e-12);
    }

    #[test]
    fn test_price_scales_with_weight_ratio_two_to_one() {
        // Assume base = 1 - quote.
        // quote = 1/3 -> base = 2/3, so w1 / w2 = 2.
        // Then price = 2 * (y/x).
        let quote = Perquintill::from_rational(1u128, 3u128);
        let bal = Balancer::new(quote).unwrap();

        let x = 4u64;
        let y = 10u64;

        let price_f = f(bal.calculate_price(x, y));
        let expected_f = 2.0 * (y as f64 / x as f64);

        assert_abs_diff_eq!(price_f, expected_f, epsilon = 1e-10);
    }

    #[test]
    fn test_price_is_zero_when_y_is_zero() {
        // If y = 0, y/x = 0 so price must be 0 regardless of weights (for x > 0).
        let quote = Perquintill::from_rational(3u128, 10u128); // 0.3
        let bal = Balancer::new(quote).unwrap();

        let x = 10u64;
        let y = 0u64;

        let price_f = f(bal.calculate_price(x, y));
        assert_abs_diff_eq!(price_f, 0.0, epsilon = 0.0);
    }

    #[test]
    fn test_price_invariant_when_scaling_x_and_y_with_equal_weights() {
        // For equal weights, price(x, y) == price(kx, ky).
        let quote = Perquintill::from_rational(1u128, 2u128); // 0.5
        let bal = Balancer::new(quote).unwrap();

        let x1 = 3u64;
        let y1 = 7u64;
        let k = 10u64;
        let x2 = x1 * k;
        let y2 = y1 * k;

        let p1 = f(bal.calculate_price(x1, y1));
        let p2 = f(bal.calculate_price(x2, y2));

        assert_abs_diff_eq!(p1, p2, epsilon = 1e-12);
    }

    #[test]
    fn test_price_matches_formula_for_general_quote() {
        // General check: price = (w1 / w2) * (y/x),
        // where w1 = base_weight, w2 = quote_weight.
        // Here we assume get_base_weight = 1 - quote.
        let quote = Perquintill::from_rational(2u128, 5u128); // 0.4
        let bal = Balancer::new(quote).unwrap();

        let x = 9u64;
        let y = 25u64;

        let price_f = f(bal.calculate_price(x, y));

        let base = Perquintill::one() - quote;
        let w1 = base.deconstruct() as f64;
        let w2 = quote.deconstruct() as f64;

        let expected_f = (w1 / w2) * (y as f64 / x as f64);
        assert_abs_diff_eq!(price_f, expected_f, epsilon = 1e-9);
    }

    #[test]
    fn test_price_high_values_non_equal_weights() {
        // Non-equal weights, high x and y (up to 21e15)
        let quote = Perquintill::from_rational(3u128, 10u128); // 0.3
        let bal = Balancer::new(quote).unwrap();

        let x: u64 = 21_000_000_000_000_000;
        let y: u64 = 15_000_000_000_000_000;

        let price = bal.calculate_price(x, y);
        let price_f = f(price);

        // Expected: (w1 / w2) * (y / x), using Balancer's actual weights
        let w1 = bal.get_base_weight().deconstruct() as f64;
        let w2 = bal.get_quote_weight().deconstruct() as f64;
        let expected_f = (w1 / w2) * (y as f64 / x as f64);

        assert_abs_diff_eq!(price_f, expected_f, epsilon = 1e-9);
    }

    // cargo test --package pallet-subtensor-swap --lib -- pallet::balancer::tests::test_exp_scaled --exact --nocapture
    #[test]
    fn test_exp_scaled() {
        [
            // base_weight_numerator, base_weight_denominator, reserve, d_reserve, base_quote
            (5_u64, 10_u64, 100000_u64, 100_u64, true, 0.999000999000999),
            (1_u64, 4_u64, 500000_u64, 5000_u64, true, 0.970590147927644),
            (3_u64, 4_u64, 200000_u64, 2000_u64, false, 0.970590147927644),
            (
                9_u64,
                10_u64,
                13513642_u64,
                1673_u64,
                false,
                0.998886481979889,
            ),
            (
                773_u64,
                1000_u64,
                7_000_000_000_u64,
                10_000_u64,
                true,
                0.999999580484586,
            ),
        ]
        .into_iter()
        .map(|v| {
            (
                Perquintill::from_rational(v.0, v.1),
                v.2,
                v.3,
                v.4,
                U64F64::from_num(v.5),
            )
        })
        .for_each(|(quote_weight, reserve, d_reserve, base_quote, expected)| {
            let balancer = Balancer::new(quote_weight).unwrap();
            let result = balancer.exp_scaled(reserve, d_reserve as i128, base_quote);
            assert_abs_diff_eq!(
                result.to_num::<f64>(),
                expected.to_num::<f64>(),
                epsilon = 0.000000001
            );
        });
    }

    // cargo test --package pallet-subtensor-swap --lib -- pallet::balancer::tests::test_base_needed_for_quote --exact --nocapture
    #[test]
    fn test_base_needed_for_quote() {
        let num = 250_000_000_000_u128; // w1 = 0.75 
        let w_quote = Perquintill::from_rational(num, 1_000_000_000_000_u128);
        let bal = Balancer::new(w_quote).unwrap();

        let tao_reserve: u64 = 1_000_000_000;
        let alpha_reserve: u64 = 1_000_000_000;
        let tao_delta: u64 = 1_123_432; // typical fee range

        let dx = bal.get_base_needed_for_quote(tao_reserve, alpha_reserve, tao_delta);

        // ∆x = x•[(y/(y+∆y))^(w2/w1) - 1]
        let dx_expected = tao_reserve as f64
            * ((tao_reserve as f64 / ((tao_reserve - tao_delta) as f64)).powf(0.25 / 0.75) - 1.0);

        assert_eq!(dx, dx_expected as u64,);
    }
}
