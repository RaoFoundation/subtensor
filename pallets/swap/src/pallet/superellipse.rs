//! A translated, fixed exponent-two superellipse on its positive-price branch.
//!
//! With X = center_alpha - alpha and Y = center_tao - tao, the invariant is
//! X² b² + Y² a² = K, and the marginal TAO/alpha price is b² X / (a² Y).
//! Only X > 0 and Y > 0 are tradable. Both centers and scales are represented
//! in Q32 atomic currency units; all products are checked U512 arithmetic.
//!
//! K is derived from the actual reserves on each operation. Integer payouts
//! are rounded down, moving K inward in the pool's favor rather than granting
//! a trader the rounding residue on a subsequent reverse trade. Liquidity
//! injections translate the centers without altering X, Y, price, or scales.

use codec::{Decode, Encode, MaxEncodedLen};
use frame_support::pallet_prelude::{RuntimeDebug, TypeInfo};
use sp_arithmetic::Perquintill;
use sp_core::U512;
use substrate_fixed::types::U64F64;
use subtensor_macros::freeze_struct;

const SCALE: u128 = 1 << 32;
const WEIGHT_SCALE: u128 = 1_000_000_000_000_000_000;

#[freeze_struct("a374c3b634a94ebc")]
#[derive(Clone, Encode, Decode, PartialEq, Eq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
pub struct Superellipse {
    alpha_scale: u128,
    tao_scale: u128,
    center_alpha: i128,
    center_tao: i128,
}

#[derive(Clone, Copy, PartialEq, Eq, RuntimeDebug)]
pub enum EllipseError {
    InvalidParameters,
    Overflow,
    OutsideDomain,
    InsufficientReserves,
}

type MathResult<T> = Result<T, EllipseError>;

fn mul(a: U512, b: U512) -> MathResult<U512> {
    a.checked_mul(b).ok_or(EllipseError::Overflow)
}

fn add(a: U512, b: U512) -> MathResult<U512> {
    a.checked_add(b).ok_or(EllipseError::Overflow)
}

fn sub(a: U512, b: U512) -> MathResult<U512> {
    a.checked_sub(b).ok_or(EllipseError::Overflow)
}

fn div(a: U512, b: U512) -> MathResult<U512> {
    a.checked_div(b).ok_or(EllipseError::InvalidParameters)
}

fn narrow(value: U512) -> MathResult<u128> {
    if value > U512::from(u128::MAX) {
        return Err(EllipseError::Overflow);
    }
    Ok(value.low_u128())
}

fn narrow_amount(value: u128) -> MathResult<u64> {
    u64::try_from(value).map_err(|_| EllipseError::Overflow)
}

/// Floor square root using bounded-precision Newton refinement.
/// The initial estimate is strictly above the root, so all iterates decrease.
/// The initial shift is at most 256 bits. For value > 1, every root is positive
/// and at least sqrt(value); root + value/root is at most 2^257, fitting U512.
#[allow(clippy::arithmetic_side_effects)]
fn sqrt_floor(value: U512) -> U512 {
    if value <= U512::one() {
        return value;
    }
    let mut root = U512::one() << value.bits().div_ceil(2);
    loop {
        let next = (root + value / root) >> 1;
        if next >= root {
            return root;
        }
        root = next;
    }
}

impl Superellipse {
    /// Match Balancer's current price and logarithmic price sensitivity.
    /// The initial normalized coordinates are X/a = Y/b = 1, so K = 2a²b².
    /// a = 2*w_quote*alpha, b = 2*w_base*tao; b = 2/(d ln(p)/d tao).
    pub fn from_weights(alpha: u64, tao: u64, quote: Perquintill) -> MathResult<Self> {
        let weight = u128::from(quote.deconstruct());
        if alpha == 0 || tao == 0 || weight == 0 || weight >= WEIGHT_SCALE {
            return Err(EllipseError::InvalidParameters);
        }
        // weight < 10^18 fits in 60 bits, so shifting by 33 cannot overflow.
        // Reserve-to-Q32 shifts likewise fit in 96 bits.
        let a = narrow(div(
            mul(U512::from(alpha), U512::from(weight << 33))?,
            U512::from(WEIGHT_SCALE),
        )?)?;
        let b = narrow(div(
            mul(
                U512::from(tao),
                U512::from(
                    WEIGHT_SCALE
                        .checked_sub(weight)
                        .ok_or(EllipseError::InvalidParameters)?
                        << 33,
                ),
            )?,
            U512::from(WEIGHT_SCALE),
        )?)?;
        if a == 0 || b == 0 {
            return Err(EllipseError::InvalidParameters);
        }
        let center_alpha = i128::try_from(
            (u128::from(alpha) << 32)
                .checked_add(a)
                .ok_or(EllipseError::Overflow)?,
        )
        .map_err(|_| EllipseError::Overflow)?;
        let center_tao = i128::try_from(
            (u128::from(tao) << 32)
                .checked_add(b)
                .ok_or(EllipseError::Overflow)?,
        )
        .map_err(|_| EllipseError::Overflow)?;
        Ok(Self {
            alpha_scale: a,
            tao_scale: b,
            center_alpha,
            center_tao,
        })
    }

    pub fn from_balancer(alpha: u64, tao: u64, quote: Perquintill) -> MathResult<Self> {
        Self::from_weights(alpha, tao, quote)
    }

    /// Anchor directly at the requested actual spot price. Derive the implied
    /// Balancer scales without first quantizing a weight: near an endpoint,
    /// rounding that weight could otherwise materially change the starting price.
    pub fn from_price(alpha: u64, tao: u64, price: U64F64) -> MathResult<Self> {
        if alpha == 0 || tao == 0 || price.to_bits() == 0 {
            return Err(EllipseError::InvalidParameters);
        }
        let tao_scaled = mul(U512::from(tao), U512::from(1_u128 << 64))?;
        let px = mul(U512::from(alpha), U512::from(price.to_bits()))?;
        let total = add(tao_scaled, px)?;
        let a = narrow(div(
            mul(mul(U512::from(alpha), tao_scaled)?, U512::from(SCALE << 1))?,
            total,
        )?)?;
        let b = narrow(div(
            mul(mul(U512::from(tao), px)?, U512::from(SCALE << 1))?,
            total,
        )?)?;
        Self::at_scales(alpha, tao, a, b)
    }

    fn at_scales(alpha: u64, tao: u64, a: u128, b: u128) -> MathResult<Self> {
        if a == 0 || b == 0 {
            return Err(EllipseError::InvalidParameters);
        }
        let center_alpha = i128::try_from(
            (u128::from(alpha) << 32)
                .checked_add(a)
                .ok_or(EllipseError::Overflow)?,
        )
        .map_err(|_| EllipseError::Overflow)?;
        let center_tao = i128::try_from(
            (u128::from(tao) << 32)
                .checked_add(b)
                .ok_or(EllipseError::Overflow)?,
        )
        .map_err(|_| EllipseError::Overflow)?;
        Ok(Self {
            alpha_scale: a,
            tao_scale: b,
            center_alpha,
            center_tao,
        })
    }

    /// The balances which no point on this invariant's positive-price branch
    /// can withdraw. The extra Q32 root unit is conservative even when K/scale²
    /// is not an integer. Reserve floors are retained in addition to that bound.
    pub fn extractable_reserves(
        &self,
        alpha: u64,
        tao: u64,
        reserve_floor: u64,
    ) -> MathResult<(u64, u64)> {
        let (x, y) = self.coordinates(alpha, tao)?;
        let k = self.invariant(x, y)?;
        let (a2, b2) = self.squares()?;
        let alpha_bound = add(sqrt_floor(div(k, b2)?), U512::one())?;
        let tao_bound = add(sqrt_floor(div(k, a2)?), U512::one())?;
        let floor = |center: i128, bound: U512, reserve: u64| -> MathResult<u64> {
            if center <= 0 || bound >= U512::from(center as u128) {
                return Ok(0);
            }
            let amount = narrow(div(
                sub(U512::from(center as u128), bound)?,
                U512::from(SCALE),
            )?)?;
            Ok(narrow_amount(amount)?
                .saturating_sub(reserve_floor)
                .min(reserve))
        };
        Ok((
            floor(self.center_alpha, alpha_bound, alpha)?,
            floor(self.center_tao, tao_bound, tao)?,
        ))
    }

    /// Apply a matching reserve withdrawal. The caller must debit exactly the
    /// same amounts from actual reserves in the same storage transaction.
    pub fn withdraw_liquidity(&mut self, alpha_delta: u64, tao_delta: u64) -> MathResult<()> {
        let alpha = self
            .center_alpha
            .checked_sub(i128::from(alpha_delta) << 32)
            .ok_or(EllipseError::Overflow)?;
        let tao = self
            .center_tao
            .checked_sub(i128::from(tao_delta) << 32)
            .ok_or(EllipseError::Overflow)?;
        self.center_alpha = alpha;
        self.center_tao = tao;
        Ok(())
    }

    fn coordinates(&self, alpha: u64, tao: u64) -> MathResult<(U512, U512)> {
        if self.alpha_scale == 0 || self.tao_scale == 0 {
            return Err(EllipseError::InvalidParameters);
        }
        let x = self
            .center_alpha
            .checked_sub(i128::from(alpha) << 32)
            .ok_or(EllipseError::Overflow)?;
        let y = self
            .center_tao
            .checked_sub(i128::from(tao) << 32)
            .ok_or(EllipseError::Overflow)?;
        if x <= 0 || y <= 0 {
            return Err(EllipseError::OutsideDomain);
        }
        Ok((U512::from(x as u128), U512::from(y as u128)))
    }

    fn squares(&self) -> MathResult<(U512, U512)> {
        Ok((
            mul(U512::from(self.alpha_scale), U512::from(self.alpha_scale))?,
            mul(U512::from(self.tao_scale), U512::from(self.tao_scale))?,
        ))
    }

    fn invariant(&self, x: U512, y: U512) -> MathResult<U512> {
        let (a2, b2) = self.squares()?;
        add(mul(mul(x, x)?, b2)?, mul(mul(y, y)?, a2)?)
    }

    pub fn calculate_price(&self, alpha: u64, tao: u64) -> MathResult<U64F64> {
        let (x, y) = self.coordinates(alpha, tao)?;
        let (a2, b2) = self.squares()?;
        let numerator = mul(mul(b2, x)?, U512::from(1_u128 << 64))?;
        let denominator = mul(a2, y)?;
        Ok(U64F64::from_bits(narrow(div(numerator, denominator)?)?))
    }

    /// Swap an exact net TAO input, returning an alpha payout rounded down.
    pub fn buy_output(&self, alpha: u64, tao: u64, input: u64) -> MathResult<u64> {
        tao.checked_add(input).ok_or(EllipseError::Overflow)?;
        let (x, y) = self.coordinates(alpha, tao)?;
        if input == 0 {
            return Ok(0);
        }
        let new_y = y
            .checked_sub(mul(U512::from(input), U512::from(SCALE))?)
            .filter(|v| !v.is_zero())
            .ok_or(EllipseError::OutsideDomain)?;
        let (a2, b2) = self.squares()?;
        // Difference form avoids cancellation/rounding of normalized coordinates.
        let gain = mul(sub(y, new_y)?, add(y, new_y)?)?;
        let new_x = sqrt_floor(add(mul(x, x)?, div(mul(gain, a2)?, b2)?)?);
        let output = narrow(div(sub(new_x, x)?, U512::from(SCALE))?)?;
        if output > u128::from(alpha) {
            return Err(EllipseError::InsufficientReserves);
        }
        narrow_amount(output)
    }

    /// Swap an exact net alpha input, returning a TAO payout rounded down.
    pub fn sell_output(&self, alpha: u64, tao: u64, input: u64) -> MathResult<u64> {
        alpha.checked_add(input).ok_or(EllipseError::Overflow)?;
        let (x, y) = self.coordinates(alpha, tao)?;
        if input == 0 {
            return Ok(0);
        }
        let new_x = x
            .checked_sub(mul(U512::from(input), U512::from(SCALE))?)
            .filter(|v| !v.is_zero())
            .ok_or(EllipseError::OutsideDomain)?;
        let (a2, b2) = self.squares()?;
        let gain = mul(sub(x, new_x)?, add(x, new_x)?)?;
        let new_y = sqrt_floor(add(mul(y, y)?, div(mul(gain, b2)?, a2)?)?);
        let output = narrow(div(sub(new_y, y)?, U512::from(SCALE))?)?;
        if output > u128::from(tao) {
            return Err(EllipseError::InsufficientReserves);
        }
        narrow_amount(output)
    }

    /// Maximum exact input on the open positive-price branch, with no reserve
    /// giveaway and no overflow of the post-trade physical input reserve.
    pub fn max_buy_input(&self, alpha: u64, tao: u64) -> MathResult<u64> {
        self.max_buy_input_with_reserve_floor(alpha, tao, 0)
    }

    pub fn max_sell_input(&self, alpha: u64, tao: u64) -> MathResult<u64> {
        self.max_sell_input_with_reserve_floor(alpha, tao, 0)
    }

    /// Maximum buy input retaining at least `floor` atomic alpha in the pool.
    pub fn max_buy_input_with_reserve_floor(
        &self,
        alpha: u64,
        tao: u64,
        floor: u64,
    ) -> MathResult<u64> {
        self.max_input(alpha, tao, true, floor)
    }

    /// Maximum sell input retaining at least `floor` atomic TAO in the pool.
    pub fn max_sell_input_with_reserve_floor(
        &self,
        alpha: u64,
        tao: u64,
        floor: u64,
    ) -> MathResult<u64> {
        self.max_input(alpha, tao, false, floor)
    }

    fn max_input(&self, alpha: u64, tao: u64, buy: bool, floor: u64) -> MathResult<u64> {
        let (x, y) = self.coordinates(alpha, tao)?;
        let (a2, b2) = self.squares()?;
        let (coordinate, output_coordinate, input_scale2, output_scale2, output_reserve, capacity) =
            if buy {
                (
                    y,
                    x,
                    a2,
                    b2,
                    alpha,
                    u64::MAX.checked_sub(tao).ok_or(EllipseError::Overflow)?,
                )
            } else {
                (
                    x,
                    y,
                    b2,
                    a2,
                    tao,
                    u64::MAX.checked_sub(alpha).ok_or(EllipseError::Overflow)?,
                )
            };
        let output_reserve = output_reserve
            .checked_sub(floor)
            .ok_or(EllipseError::InsufficientReserves)?;
        // floor((floor(sqrt(T)) - output_coordinate)/S) <= output_reserve
        // iff T < (output_coordinate + (output_reserve+1)*S)^2.
        // The strict inequality includes every valid rounded payout, even when
        // a real-valued calculation would exceed reserves by a fractional unit.
        let allowance = mul(
            U512::from(
                u128::from(output_reserve)
                    .checked_add(1)
                    .ok_or(EllipseError::Overflow)?,
            ),
            U512::from(SCALE),
        )?;
        let headroom = mul(
            allowance,
            add(mul(output_coordinate, U512::from(2))?, allowance)?,
        )?;
        let initial = mul(mul(coordinate, coordinate)?, input_scale2)?;
        let bound = mul(headroom, output_scale2)?;
        let minimum = if initial >= bound {
            add(
                sqrt_floor(div(sub(initial, bound)?, input_scale2)?),
                U512::one(),
            )?
        } else {
            U512::one()
        };
        let room = coordinate
            .checked_sub(minimum)
            .ok_or(EllipseError::OutsideDomain)?;
        let amount = narrow(div(room, U512::from(SCALE))?)?;
        Ok(amount.min(u128::from(capacity)) as u64)
    }

    fn last_true<F>(&self, upper: u64, mut predicate: F) -> MathResult<u64>
    where
        F: FnMut(u64) -> MathResult<bool>,
    {
        let mut low = 0;
        let mut high = upper;
        while low < high {
            let mid = low
                .checked_add(
                    high.checked_sub(low)
                        .ok_or(EllipseError::Overflow)?
                        .div_ceil(2),
                )
                .ok_or(EllipseError::Overflow)?;
            if predicate(mid)? {
                low = mid;
            } else {
                high = mid.checked_sub(1).ok_or(EllipseError::Overflow)?;
            }
        }
        Ok(low)
    }

    /// Largest buy input whose actual post-trade spot price stays at or below
    /// the limit. A bounded integer search also accounts for payout rounding.
    pub fn quote_delta_to_price(&self, alpha: u64, tao: u64, target: U64F64) -> MathResult<u64> {
        if target <= self.calculate_price(alpha, tao)? {
            return Ok(0);
        }
        let max = self.max_buy_input(alpha, tao)?;
        self.last_true(max, |input| {
            let output = self.buy_output(alpha, tao, input)?;
            match self.calculate_price(
                alpha.checked_sub(output).ok_or(EllipseError::Overflow)?,
                tao.checked_add(input).ok_or(EllipseError::Overflow)?,
            ) {
                Ok(price) => Ok(price <= target),
                Err(EllipseError::Overflow) => Ok(false),
                Err(e) => Err(e),
            }
        })
    }

    /// Largest sell input whose actual post-trade spot price stays at or above
    /// the limit. The valid domain includes no zero-price endpoint.
    pub fn base_delta_to_price(&self, alpha: u64, tao: u64, target: U64F64) -> MathResult<u64> {
        if target >= self.calculate_price(alpha, tao)? {
            return Ok(0);
        }
        let max = self.max_sell_input(alpha, tao)?;
        self.last_true(max, |input| {
            let output = self.sell_output(alpha, tao, input)?;
            Ok(self.calculate_price(
                alpha.checked_add(input).ok_or(EllipseError::Overflow)?,
                tao.checked_sub(output).ok_or(EllipseError::Overflow)?,
            )? >= target)
        })
    }

    /// Minimum alpha input which produces at least the requested TAO payout.
    /// Unlike ordinary exact-input quotes, inverse quotes round input upward.
    pub fn base_needed_for_quote(&self, alpha: u64, tao: u64, output: u64) -> MathResult<u64> {
        self.coordinates(alpha, tao)?;
        if output == 0 {
            return Ok(0);
        }
        if output > tao {
            return Err(EllipseError::InsufficientReserves);
        }
        let mut high = self.max_sell_input(alpha, tao)?;
        if self.sell_output(alpha, tao, high)? < output {
            return Err(EllipseError::OutsideDomain);
        }
        let mut low = 0;
        while low < high {
            let mid = low
                .checked_add(high.checked_sub(low).ok_or(EllipseError::Overflow)? >> 1)
                .ok_or(EllipseError::Overflow)?;
            if self.sell_output(alpha, tao, mid)? >= output {
                high = mid;
            } else {
                low = mid.checked_add(1).ok_or(EllipseError::Overflow)?;
            }
        }
        Ok(low)
    }

    /// Minimum net TAO input which buys at least the requested alpha payout.
    /// Input rounds upward, while ordinary exact-input payouts round downward.
    pub fn quote_needed_for_base(&self, alpha: u64, tao: u64, output: u64) -> MathResult<u64> {
        self.coordinates(alpha, tao)?;
        if output == 0 {
            return Ok(0);
        }
        if output > alpha {
            return Err(EllipseError::InsufficientReserves);
        }
        let mut high = self.max_buy_input(alpha, tao)?;
        if self.buy_output(alpha, tao, high)? < output {
            return Err(EllipseError::OutsideDomain);
        }
        let mut low = 0;
        while low < high {
            let mid = low
                .checked_add(high.checked_sub(low).ok_or(EllipseError::Overflow)? >> 1)
                .ok_or(EllipseError::Overflow)?;
            if self.buy_output(alpha, tao, mid)? >= output {
                high = mid;
            } else {
                low = mid.checked_add(1).ok_or(EllipseError::Overflow)?;
            }
        }
        Ok(low)
    }

    /// Translate both centers by the injected reserves. The existing price,
    /// local response and reachable trade range remain unchanged. This does
    /// not enact any adaptive sensitivity policy.
    pub fn translate_liquidity(&mut self, alpha_delta: u64, tao_delta: u64) -> MathResult<()> {
        let new_alpha = self
            .center_alpha
            .checked_add(i128::from(alpha_delta) << 32)
            .ok_or(EllipseError::Overflow)?;
        let new_tao = self
            .center_tao
            .checked_add(i128::from(tao_delta) << 32)
            .ok_or(EllipseError::Overflow)?;
        self.center_alpha = new_alpha;
        self.center_tao = new_tao;
        Ok(())
    }
}

#[cfg(test)]
#[allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::unwrap_used
)]
mod tests {
    use super::*;

    fn equal(alpha: u64, tao: u64) -> Superellipse {
        Superellipse::from_weights(alpha, tao, Perquintill::from_percent(50)).unwrap()
    }

    #[test]
    fn sqrt_is_exact_at_extremes_and_neighbors() {
        for root in [1_u128, 2, 100, u64::MAX as u128, u128::MAX] {
            let root = U512::from(root);
            let square = root * root;
            assert_eq!(sqrt_floor(square), root);
            assert_eq!(sqrt_floor(square - U512::one()), root - U512::one());
            assert_eq!(sqrt_floor(square + U512::one()), root);
        }
        let root = sqrt_floor(U512::MAX);
        assert_eq!(root, (U512::one() << 256) - U512::one());
    }

    #[test]
    fn baseline_preserves_price_and_local_sensitivity() {
        let (alpha, tao) = (10_000_000_000_000_u64, 500_000_000_000_u64);
        for weight in [1_u64, 10, 30, 50, 70, 90, 99] {
            let quote = Perquintill::from_percent(weight);
            let pool = Superellipse::from_weights(alpha, tao, quote).unwrap();
            let wq = weight as f64 / 100.;
            let expected = (1. - wq) * tao as f64 / (wq * alpha as f64);
            let price = pool.calculate_price(alpha, tao).unwrap().to_num::<f64>();
            assert!((price / expected - 1.).abs() < 1e-12);
            let input = 100_000;
            let output = pool.buy_output(alpha, tao, input).unwrap();
            let after = pool
                .calculate_price(alpha - output, tao + input)
                .unwrap()
                .to_num::<f64>();
            let slope = (after / price).ln() / input as f64;
            let expected_slope = 1. / ((1. - wq) * tao as f64);
            assert!((slope / expected_slope - 1.).abs() < 0.0001);
            let input = 1_000_000;
            let output = pool.sell_output(alpha, tao, input).unwrap();
            let after = pool
                .calculate_price(alpha + input, tao - output)
                .unwrap()
                .to_num::<f64>();
            let slope = -(after / price).ln() / input as f64;
            let expected_slope = 1. / (wq * alpha as f64);
            assert!((slope / expected_slope - 1.).abs() < 0.0001);
        }
    }

    #[test]
    fn buys_and_sells_follow_ellipse_and_round_in_pools_favor() {
        let (alpha, tao) = (10_000_000, 500_000);
        let pool = equal(alpha, tao);
        for input in [0, 1, 100, 1_000, 100_000] {
            let output = pool.buy_output(alpha, tao, input).unwrap();
            let expected =
                alpha as f64 * ((2. - (1. - input as f64 / tao as f64).powi(2)).sqrt() - 1.);
            assert_eq!(output, expected.floor() as u64);
            let (x, y) = pool.coordinates(alpha, tao).unwrap();
            let (nx, ny) = pool.coordinates(alpha - output, tao + input).unwrap();
            assert!(pool.invariant(nx, ny).unwrap() <= pool.invariant(x, y).unwrap());
            assert!(
                pool.sell_output(alpha - output, tao + input, output)
                    .unwrap()
                    <= input
            );
        }
        for input in [1, 100, 1_000, 100_000] {
            let output = pool.sell_output(alpha, tao, input).unwrap();
            let returned = pool
                .buy_output(alpha + input, tao - output, output)
                .unwrap();
            assert!(returned <= input);
        }
    }

    #[test]
    fn price_limits_are_conservative_and_maximal() {
        let (alpha, tao) = (10_000_000, 500_000);
        let pool = equal(alpha, tao);
        let target = U64F64::from_num(0.06);
        let input = pool.quote_delta_to_price(alpha, tao, target).unwrap();
        let output = pool.buy_output(alpha, tao, input).unwrap();
        assert!(pool.calculate_price(alpha - output, tao + input).unwrap() <= target);
        let next = pool.buy_output(alpha, tao, input + 1).unwrap();
        assert!(pool.calculate_price(alpha - next, tao + input + 1).unwrap() > target);
        let target = U64F64::from_num(0.04);
        let input = pool.base_delta_to_price(alpha, tao, target).unwrap();
        let output = pool.sell_output(alpha, tao, input).unwrap();
        assert!(pool.calculate_price(alpha + input, tao - output).unwrap() >= target);
        let next = pool.sell_output(alpha, tao, input + 1).unwrap();
        assert!(pool.calculate_price(alpha + input + 1, tao - next).unwrap() < target);
    }

    #[test]
    fn inverse_quote_rounds_input_up() {
        let (alpha, tao) = (10_000_000, 500_000);
        let pool = equal(alpha, tao);
        for output in [1, 10, 1_000, 100_000] {
            let input = pool.base_needed_for_quote(alpha, tao, output).unwrap();
            assert!(pool.sell_output(alpha, tao, input).unwrap() >= output);
            assert!(pool.sell_output(alpha, tao, input - 1).unwrap() < output);
        }
    }

    #[test]
    fn domain_and_physical_reserves_are_enforced() {
        let pool = equal(10_000, 1_000);
        assert_eq!(pool.max_buy_input(10_000, 1_000).unwrap(), 999);
        assert_eq!(
            pool.buy_output(10_000, 1_000, 1_000),
            Err(EllipseError::OutsideDomain)
        );
        assert_eq!(pool.max_sell_input(10_000, 1_000).unwrap(), 9_999);
        let unbalanced =
            Superellipse::from_weights(10_000, 1_000, Perquintill::from_percent(1)).unwrap();
        let max = unbalanced.max_sell_input(10_000, 1_000).unwrap();
        assert!(unbalanced.sell_output(10_000, 1_000, max).unwrap() <= 1_000);
        assert!(unbalanced.sell_output(10_000, 1_000, max + 1).is_err());
    }

    #[test]
    fn reserve_floors_are_enforced_at_the_exact_integer_boundary() {
        let (alpha, tao) = (10_000, 1_000);
        for weight in [1, 10, 50, 90, 99] {
            let pool =
                Superellipse::from_weights(alpha, tao, Perquintill::from_percent(weight)).unwrap();
            for floor in [0, 1, 100, 1_000] {
                let buy = pool
                    .max_buy_input_with_reserve_floor(alpha, tao, floor)
                    .unwrap();
                let bought = pool.buy_output(alpha, tao, buy).unwrap();
                assert!(bought <= alpha - floor);
                assert!(
                    pool.buy_output(alpha, tao, buy + 1)
                        .map_or(true, |v| v > alpha - floor)
                );
                let sell = pool
                    .max_sell_input_with_reserve_floor(alpha, tao, floor)
                    .unwrap();
                let sold = pool.sell_output(alpha, tao, sell).unwrap();
                assert!(sold <= tao - floor);
                assert!(
                    pool.sell_output(alpha, tao, sell + 1)
                        .map_or(true, |v| v > tao - floor)
                );
            }
            assert_eq!(
                pool.max_buy_input_with_reserve_floor(alpha, tao, alpha + 1),
                Err(EllipseError::InsufficientReserves)
            );
            assert_eq!(
                pool.max_sell_input_with_reserve_floor(alpha, tao, tao + 1),
                Err(EllipseError::InsufficientReserves)
            );
        }
    }

    #[test]
    fn injections_preserve_price_and_trade_response() {
        let (alpha, tao) = (10_000_000, 500_000);
        let mut pool = equal(alpha, tao);
        let price = pool.calculate_price(alpha, tao).unwrap();
        let output = pool.buy_output(alpha, tao, 1_000).unwrap();
        pool.translate_liquidity(7_000_000, 900_000).unwrap();
        assert_eq!(
            pool.calculate_price(alpha + 7_000_000, tao + 900_000)
                .unwrap(),
            price
        );
        assert_eq!(
            pool.buy_output(alpha + 7_000_000, tao + 900_000, 1_000)
                .unwrap(),
            output
        );
    }

    #[test]
    fn full_width_domain_bounds_and_round_trips() {
        // Deterministic full-width samples exercise overflow boundaries and
        // weights far from 50/50 without floating-point reference arithmetic.
        let mut state = 22_u64;
        let mut next = || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            state
        };
        for _ in 0..256 {
            let alpha = next().max(1);
            let tao = next().max(1);
            let quote = Perquintill::from_percent(next() % 99 + 1);
            let pool = Superellipse::from_weights(alpha, tao, quote).unwrap();
            let max_buy = pool.max_buy_input(alpha, tao).unwrap();
            let bought = pool.buy_output(alpha, tao, max_buy).unwrap();
            if max_buy < u64::MAX - tao {
                assert!(pool.buy_output(alpha, tao, max_buy + 1).is_err());
            }
            assert!(
                pool.sell_output(alpha - bought, tao + max_buy, bought)
                    .unwrap()
                    <= max_buy
            );
            let max_sell = pool.max_sell_input(alpha, tao).unwrap();
            let sold = pool.sell_output(alpha, tao, max_sell).unwrap();
            if max_sell < u64::MAX - alpha {
                assert!(pool.sell_output(alpha, tao, max_sell + 1).is_err());
            }
            assert!(pool.buy_output(alpha + max_sell, tao - sold, sold).unwrap() <= max_sell);
        }
    }

    #[test]
    fn baseline_extraction_is_safe_for_full_width_reserves() {
        let mut state = 59_u64;
        let mut next = || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            state
        };
        for _ in 0..256 {
            let alpha = next().max(1);
            let tao = next().max(1);
            let weight = Perquintill::from_percent(next() % 99 + 1);
            let mut curve = Superellipse::from_weights(alpha, tao, weight).unwrap();
            let before = curve.clone();
            let take = curve.extractable_reserves(alpha, tao, 1).unwrap();
            curve.withdraw_liquidity(take.0, take.1).unwrap();
            let active_alpha = alpha - take.0;
            let active_tao = tao - take.1;
            assert_eq!(
                before.calculate_price(alpha, tao),
                curve.calculate_price(active_alpha, active_tao)
            );
            let buy = curve
                .max_buy_input_with_reserve_floor(active_alpha, active_tao, 1)
                .unwrap();
            let sell = curve
                .max_sell_input_with_reserve_floor(active_alpha, active_tao, 1)
                .unwrap();
            assert!(curve.buy_output(active_alpha, active_tao, buy).unwrap() < active_alpha);
            assert!(curve.sell_output(active_alpha, active_tao, sell).unwrap() < active_tao);
            assert!(
                curve
                    .sell_output(active_alpha, active_tao, sell + 1)
                    .is_err()
                    || curve
                        .sell_output(active_alpha, active_tao, sell + 1)
                        .unwrap()
                        >= active_tao
            );
            assert!(
                curve.buy_output(active_alpha, active_tao, buy + 1).is_err()
                    || curve.buy_output(active_alpha, active_tao, buy + 1).unwrap() >= active_alpha
            );
        }
    }

    #[test]
    fn globally_unreachable_extraction_preserves_quotes_and_boundaries() {
        let (alpha, tao) = (2_880_603_110_475_064, 203_305_249_479_705);
        for weight in [1, 10, 50, 90, 99] {
            let mut curve =
                Superellipse::from_weights(alpha, tao, Perquintill::from_percent(weight)).unwrap();
            let before = curve.clone();
            let take = curve.extractable_reserves(alpha, tao, 1_000_000).unwrap();
            let next_alpha = alpha - take.0;
            let next_tao = tao - take.1;
            curve.withdraw_liquidity(take.0, take.1).unwrap();
            assert_eq!(
                before.calculate_price(alpha, tao),
                curve.calculate_price(next_alpha, next_tao)
            );
            assert_eq!(
                curve
                    .extractable_reserves(next_alpha, next_tao, 1_000_000)
                    .unwrap(),
                (0, 0)
            );
            let max_buy = before
                .max_buy_input_with_reserve_floor(alpha, tao, 1_000_000)
                .unwrap();
            let max_sell = before
                .max_sell_input_with_reserve_floor(alpha, tao, 1_000_000)
                .unwrap();
            assert_eq!(
                curve
                    .max_buy_input_with_reserve_floor(next_alpha, next_tao, 1_000_000)
                    .unwrap(),
                max_buy
            );
            assert_eq!(
                curve
                    .max_sell_input_with_reserve_floor(next_alpha, next_tao, 1_000_000)
                    .unwrap(),
                max_sell
            );
            for buy in [0, 1, max_buy / 2, max_buy] {
                assert_eq!(
                    before.buy_output(alpha, tao, buy),
                    curve.buy_output(next_alpha, next_tao, buy)
                );
                assert!(
                    curve.buy_output(next_alpha, next_tao, buy).unwrap() <= next_alpha - 1_000_000
                );
            }
            for sell in [0, 1, max_sell / 2, max_sell] {
                assert_eq!(
                    before.sell_output(alpha, tao, sell),
                    curve.sell_output(next_alpha, next_tao, sell)
                );
                assert!(
                    curve.sell_output(next_alpha, next_tao, sell).unwrap() <= next_tao - 1_000_000
                );
            }
        }
    }

    #[test]
    fn extraction_uses_actual_post_trade_invariant_and_remains_safe() {
        let (mut alpha, mut tao) = (10_000_000_u64, 500_000_u64);
        let mut curve = equal(alpha, tao);
        let take = curve.extractable_reserves(alpha, tao, 100).unwrap();
        curve.withdraw_liquidity(take.0, take.1).unwrap();
        alpha -= take.0;
        tao -= take.1;
        let initial = curve.coordinates(alpha, tao).unwrap();
        let mut k = curve.invariant(initial.0, initial.1).unwrap();
        for amount in [1, 7, 13, 53, 97, 101, 313, 1_003] {
            let bought = curve.buy_output(alpha, tao, amount).unwrap();
            alpha -= bought;
            tao += amount;
            let sold = curve.sell_output(alpha, tao, bought).unwrap();
            alpha += bought;
            tao -= sold;
            let coordinates = curve.coordinates(alpha, tao).unwrap();
            let next_k = curve.invariant(coordinates.0, coordinates.1).unwrap();
            assert!(next_k <= k);
            k = next_k;
            let max_buy = curve
                .max_buy_input_with_reserve_floor(alpha, tao, 100)
                .unwrap();
            let max_sell = curve
                .max_sell_input_with_reserve_floor(alpha, tao, 100)
                .unwrap();
            assert!(curve.buy_output(alpha, tao, max_buy).unwrap() <= alpha - 100);
            assert!(curve.sell_output(alpha, tao, max_sell).unwrap() <= tao - 100);
        }
    }

    #[test]
    fn inverse_buy_quote_is_minimal_and_preserves_rounding() {
        let (alpha, tao) = (10_000_000, 500_000);
        let curve = equal(alpha, tao);
        for output in [1, 10, 1_000, 100_000] {
            let input = curve.quote_needed_for_base(alpha, tao, output).unwrap();
            assert!(curve.buy_output(alpha, tao, input).unwrap() >= output);
            assert!(curve.buy_output(alpha, tao, input - 1).unwrap() < output);
        }
        assert_eq!(curve.quote_needed_for_base(alpha, tao, 0).unwrap(), 0);
        assert_eq!(
            curve.quote_needed_for_base(alpha, tao, alpha + 1),
            Err(EllipseError::InsufficientReserves)
        );
        assert_eq!(
            curve.quote_needed_for_base(alpha, tao, alpha),
            Err(EllipseError::OutsideDomain)
        );
    }

    #[test]
    fn extreme_balances_and_invalid_initialization() {
        for (alpha, tao) in [(1, 1), (u64::MAX - 1_000, u64::MAX - 1_000), (u64::MAX, 1)] {
            let pool = equal(alpha, tao);
            assert!(pool.calculate_price(alpha, tao).is_ok());
            if alpha > 1 && alpha != u64::MAX {
                assert!(pool.sell_output(alpha, tao, 1).is_ok());
            }
        }
        assert_eq!(
            Superellipse::from_weights(0, 1, Perquintill::from_percent(50)),
            Err(EllipseError::InvalidParameters)
        );
        assert_eq!(
            Superellipse::from_weights(1, 1, Perquintill::from_percent(0)),
            Err(EllipseError::InvalidParameters)
        );
        let pool = equal(u64::MAX, u64::MAX);
        assert_eq!(
            pool.buy_output(u64::MAX, u64::MAX, 1),
            Err(EllipseError::Overflow)
        );
        let price = U64F64::from_num(0.05);
        let pool = Superellipse::from_price(10_000_000, 500_000, price).unwrap();
        assert!(
            (pool
                .calculate_price(10_000_000, 500_000)
                .unwrap()
                .to_num::<f64>()
                - 0.05)
                .abs()
                < 1e-15
        );
    }
}
