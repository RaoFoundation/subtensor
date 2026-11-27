//! The pool's arithmetic, free of storage.
//!
//! Swap amounts and prices come from the swap pallet's [`Balancer`] at its default weights of
//! one half each: a constant-product pool. TAO is the balancer's base reserve and tUSD its
//! quote reserve, so the price is tUSD per TAO. Nothing here re-derives that math; this
//! module only applies the fee, bounds the result and does the share bookkeeping.

use pallet_subtensor_swap::balancer::Balancer;
use sp_arithmetic::{
    Permill, Rounding, helpers_128bit::multiply_by_rational_with_rounding,
    traits::IntegerSquareRoot,
};
use substrate_fixed::types::U64F64;

/// Fixed-point scale of prices: a price of `PRICE_SCALE` is one tUSD per TAO.
pub const PRICE_SCALE: u64 = 1_000_000_000;

/// LP shares locked for ever by the first deposit, so the share supply never returns to zero.
pub const MINIMUM_LIQUIDITY: u128 = 1_000;

/// Which asset a swap pays in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// tUSD in, TAO out.
    TusdIn,
    /// TAO in, tUSD out.
    TaoIn,
}

/// The pool's reserves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reserves {
    pub tao: u64,
    pub tusd: u64,
}

impl Reserves {
    /// The reserve a swap on `side` pays into, and the one it pays out of.
    pub fn in_out(&self, side: Side) -> (u64, u64) {
        match side {
            Side::TusdIn => (self.tusd, self.tao),
            Side::TaoIn => (self.tao, self.tusd),
        }
    }
}

/// What a swap takes and gives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwapQuote {
    /// The part of the input kept as the fee. It stays in the pool, for the LPs.
    pub fee: u64,
    /// What the swapper receives.
    pub amount_out: u64,
}

/// The tUSD price of one TAO, scaled by [`PRICE_SCALE`]. Zero while a reserve is empty.
pub fn price(reserves: Reserves) -> u64 {
    if reserves.tao == 0 {
        return 0;
    }
    Balancer::default()
        .calculate_price(reserves.tao, reserves.tusd)
        .saturating_mul(U64F64::saturating_from_num(PRICE_SCALE))
        .saturating_to_num::<u64>()
}

/// Quotes a swap of `amount_in` on `side`. `None` if the pool is empty or pays nothing.
pub fn quote(
    reserves: Reserves,
    side: Side,
    amount_in: u64,
    fee_rate: Permill,
) -> Option<SwapQuote> {
    let (reserve_in, reserve_out) = reserves.in_out(side);
    if reserve_in == 0 || reserve_out == 0 || amount_in == 0 {
        return None;
    }
    let fee = fee_rate.mul_ceil(amount_in);
    let net_in = amount_in.checked_sub(fee)?;
    let balancer = Balancer::default();
    // The fraction of the paying reserve that stays after the swap.
    let kept = match side {
        Side::TusdIn => balancer.exp_quote_base(reserve_in, net_in),
        Side::TaoIn => balancer.exp_base_quote(reserve_in, net_in),
    };
    let paid = U64F64::saturating_from_num(reserve_out)
        .saturating_mul(U64F64::saturating_from_num(1).saturating_sub(kept))
        .saturating_to_num::<u64>();
    // The balancer works in steps of 1e-18 and may round a payout up by a unit. The pool
    // never pays more than leaves the product of its reserves whole.
    let amount_out = paid.min(max_out(reserve_in, reserve_out, net_in)?);
    (amount_out != 0).then_some(SwapQuote { fee, amount_out })
}

/// The most a swap of `net_in` may pay without lowering `reserve_in * reserve_out`.
fn max_out(reserve_in: u64, reserve_out: u64, net_in: u64) -> Option<u64> {
    let product = u128::from(reserve_in).checked_mul(u128::from(reserve_out))?;
    let reserve_in_after = u128::from(reserve_in).checked_add(u128::from(net_in))?;
    // Ceiling division: the smallest paying reserve that keeps the product.
    let reserve_out_after = product
        .checked_add(reserve_in_after.checked_sub(1)?)?
        .checked_div(reserve_in_after)?;
    u64::try_from(u128::from(reserve_out).checked_sub(reserve_out_after)?).ok()
}

/// The shares a first deposit mints: the geometric mean of the two amounts. The depositor
/// receives these less [`MINIMUM_LIQUIDITY`].
pub fn initial_shares(tao: u64, tusd: u64) -> u128 {
    u128::from(tao)
        .saturating_mul(u128::from(tusd))
        .integer_sqrt()
}

/// The shares that at most `max_tao` and `max_tusd` buy in a pool of `reserves` and
/// `total_shares`, and the amounts they cost. Rounds for the pool: shares down, costs up.
pub fn shares_for_deposit(
    reserves: Reserves,
    total_shares: u128,
    max_tao: u64,
    max_tusd: u64,
) -> Option<(u128, Reserves)> {
    let by_tao = mul_div(
        u128::from(max_tao),
        total_shares,
        reserves.tao,
        Rounding::Down,
    )?;
    let by_tusd = mul_div(
        u128::from(max_tusd),
        total_shares,
        reserves.tusd,
        Rounding::Down,
    )?;
    let shares = by_tao.min(by_tusd);
    let cost = Reserves {
        tao: share_of(reserves.tao, shares, total_shares, Rounding::Up)?,
        tusd: share_of(reserves.tusd, shares, total_shares, Rounding::Up)?,
    };
    Some((shares, cost))
}

/// What `shares` of `total_shares` redeem from `reserves`. Rounds down, for the pool.
pub fn redemption(reserves: Reserves, total_shares: u128, shares: u128) -> Option<Reserves> {
    Some(Reserves {
        tao: share_of(reserves.tao, shares, total_shares, Rounding::Down)?,
        tusd: share_of(reserves.tusd, shares, total_shares, Rounding::Down)?,
    })
}

fn share_of(reserve: u64, shares: u128, total_shares: u128, rounding: Rounding) -> Option<u64> {
    u64::try_from(mul_div(
        u128::from(reserve),
        shares,
        total_shares,
        rounding,
    )?)
    .ok()
}

fn mul_div(a: u128, b: u128, c: impl Into<u128>, rounding: Rounding) -> Option<u128> {
    multiply_by_rational_with_rounding(a, b, c.into(), rounding)
}
