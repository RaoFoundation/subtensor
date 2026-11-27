//! The lending pallet's pricing function on subnet pools (pallet_lending DESIGN-v2 section 4 and
//! section 10 item 3).
//!
//! Subnet pools are Balancer weighted pools. [`BalancerCurve`] implements `pallet_lending`'s
//! `Curve` with the swap pallet's own pure functions (`pallet_subtensor_swap::balancer`): the
//! fee is [`fee_amount`], a buy converts with `Balancer::alpha_out_for_tao_in` and a sell with
//! `Balancer::tao_out_for_alpha_in`. Those are the functions every swap executes, so a quote
//! equals what the swap returns on the same reserves, weight and fee rate.
//!
//! At quote weight 1/2 the pool is constant product and the quotes agree with the lending
//! model's `ConstantProduct` up to rounding; `tests::lending_curve` measures the difference.

use pallet_lending::math::{
    curve::{Curve, PoolState},
    fixed::Price,
};
use pallet_subtensor_swap::balancer::{Balancer, fee_amount};

/// Fee rates are parts of `u16::MAX`.
const FEE_DEN: u128 = 65_535;

/// Balancer weighted-pool pricing, identical to the swap pallet's.
pub struct BalancerCurve;

/// The pool's balancer. `None` if the weight is outside the range the swap pallet accepts; no
/// live pool has such a weight.
fn balancer(pool: &PoolState) -> Option<Balancer> {
    Balancer::new(pool.quote_weight).ok()
}

/// The part of a swap input that reaches the curve: the input less the swap pallet's fee.
fn net_of_fee(pool: &PoolState, amount: u64) -> u64 {
    amount.saturating_sub(fee_amount(pool.fee, amount))
}

/// `ceil(net * 65535 / (65535 - fee))`: an input whose fee'd part is about `net`. Only a
/// starting point for [`smallest_input`].
fn gross_up(pool: &PoolState, net: u64) -> u64 {
    let keep = FEE_DEN.saturating_sub(u128::from(pool.fee));
    u128::from(net)
        .saturating_mul(FEE_DEN)
        .saturating_add(keep.saturating_sub(1))
        .checked_div(keep)
        .and_then(|gross| u64::try_from(gross).ok())
        .unwrap_or(u64::MAX)
}

/// The smallest input for which `reaches` holds, for a predicate that is false below some
/// input and true from it on. Starts at `estimate`, brackets the threshold by doubling steps
/// and bisects; each probe is one swap conversion, and an exact estimate costs two. `None` if
/// no `u64` input reaches.
fn smallest_input(estimate: u64, reaches: impl Fn(u64) -> bool) -> Option<u64> {
    let mut step = 1u64;
    // Invariant after bracketing: `reaches(high)` and not `reaches(low)`.
    let (mut low, mut high) = if reaches(estimate) {
        let mut high = estimate;
        loop {
            if high == 0 {
                return Some(0);
            }
            let probe = high.saturating_sub(step);
            if reaches(probe) {
                high = probe;
                step = step.saturating_mul(2);
            } else {
                break (probe, high);
            }
        }
    } else {
        let mut low = estimate;
        loop {
            let probe = low.checked_add(step)?;
            if reaches(probe) {
                break (low, probe);
            }
            low = probe;
            step = step.saturating_mul(2);
        }
    };
    while high.saturating_sub(low) > 1 {
        let mid = low.saturating_add(high.saturating_sub(low) >> 1);
        if reaches(mid) {
            high = mid;
        } else {
            low = mid;
        }
    }
    Some(high)
}

/// The smallest TAO input whose buy returns at least `alpha_out`, with the fee taken from the
/// input or fee-free. `None` if `alpha_out >= pool.alpha` or no input reaches it.
fn buy_input(pool: &PoolState, alpha_out: u64, with_fee: bool) -> Option<u64> {
    if alpha_out == 0 {
        return Some(0);
    }
    let left = pool.alpha.checked_sub(alpha_out).filter(|left| *left > 0)?;
    let balancer = balancer(pool)?;
    // The invariant's own answer, rounded up: y * ((x / (x - a))^(w1/w2) - 1).
    let net = balancer
        .tao_reserve_on_invariant(pool.tao, pool.alpha, left, true)?
        .saturating_sub(pool.tao);
    let estimate = if with_fee { gross_up(pool, net) } else { net };
    smallest_input(estimate, |tao_in| {
        let reaching = if with_fee {
            net_of_fee(pool, tao_in)
        } else {
            tao_in
        };
        balancer.alpha_out_for_tao_in(pool.tao, pool.alpha, reaching) >= alpha_out
    })
}

impl BalancerCurve {
    /// The hypothetical pool at spot `price` on the same weighted invariant (no fee):
    /// `X' = X * (p0 / p)^w_quote` rounded down, then `T' = T * (X / X')^(w_base / w_quote)`,
    /// which equals `T * (p / p0)^w_base` and keeps the integer state on the invariant. `T'` is
    /// rounded up when `tao_up`, else down. `None` if the state is not representable.
    #[must_use]
    pub fn at_price(pool: &PoolState, price: Price, tao_up: bool) -> Option<PoolState> {
        let balancer = balancer(pool)?;
        let alpha = balancer.alpha_reserve_at_price(pool.tao, pool.alpha, price)?;
        // A pump never adds alpha and a dump never removes it, whatever the rounding.
        let alpha = if tao_up {
            alpha.min(pool.alpha).max(1)
        } else {
            alpha.max(pool.alpha)
        };
        let tao = balancer.tao_reserve_on_invariant(pool.tao, pool.alpha, alpha, tao_up)?;
        Some(pool.with_reserves(tao, alpha))
    }
}

impl Curve for BalancerCurve {
    /// `(w_base / w_quote) * (tao / alpha)`, the swap pallet's `current_price`.
    fn spot(pool: &PoolState) -> Price {
        if pool.alpha == 0 {
            return Price::from_bits(0);
        }
        balancer(pool).map_or(Price::from_bits(0), |balancer| {
            balancer.calculate_price(pool.alpha, pool.tao)
        })
    }

    fn out_alpha_for_tao(pool: &PoolState, tao_in: u64) -> u64 {
        balancer(pool).map_or(0, |balancer| {
            balancer.alpha_out_for_tao_in(pool.tao, pool.alpha, net_of_fee(pool, tao_in))
        })
    }

    fn out_tao_for_alpha(pool: &PoolState, alpha_in: u64) -> u64 {
        balancer(pool).map_or(0, |balancer| {
            balancer.tao_out_for_alpha_in(pool.tao, pool.alpha, net_of_fee(pool, alpha_in))
        })
    }

    fn tao_in_for_alpha(pool: &PoolState, alpha_out: u64) -> Option<u128> {
        buy_input(pool, alpha_out, true).map(u128::from)
    }

    fn buy_cost_nofee(pool: &PoolState, alpha_out: u64) -> Option<u128> {
        buy_input(pool, alpha_out, false).map(u128::from)
    }

    fn sell_proceeds_nofee(pool: &PoolState, alpha_in: u64) -> u64 {
        balancer(pool).map_or(0, |balancer| {
            balancer.tao_out_for_alpha_in(pool.tao, pool.alpha, alpha_in)
        })
    }

    /// Rounds toward the pool as `ConstantProduct` does: alpha down, TAO up.
    fn pumped(pool: &PoolState, target: Price) -> Option<PoolState> {
        if Self::spot(pool) >= target {
            return Some(*pool);
        }
        Self::at_price(pool, target, true)
    }

    /// Rounds as `ConstantProduct` does: alpha down, TAO down.
    fn dumped(pool: &PoolState, target: Price) -> Option<PoolState> {
        if target == Price::from_bits(0) || Self::spot(pool) <= target {
            return Some(*pool);
        }
        Self::at_price(pool, target, false)
    }

    fn spot_after_buy(pool: &PoolState, alpha_out: u64) -> Option<Price> {
        let (after, _) = Self::after_buy_alpha(pool, alpha_out)?;
        (after.alpha > 0).then(|| Self::spot(&after))
    }

    fn after_sell(pool: &PoolState, alpha_in: u64) -> Option<(PoolState, u64)> {
        let tao_out = Self::out_tao_for_alpha(pool, alpha_in);
        let tao = pool.tao.checked_sub(tao_out)?;
        let alpha = pool.alpha.checked_add(net_of_fee(pool, alpha_in))?;
        Some((pool.with_reserves(tao, alpha), tao_out))
    }

    /// The pool gives exactly `alpha_out`; alpha the minimal input would buy beyond it stays in
    /// the reserve.
    fn after_buy_alpha(pool: &PoolState, alpha_out: u64) -> Option<(PoolState, u64)> {
        let cost = buy_input(pool, alpha_out, true)?;
        let tao = pool.tao.checked_add(net_of_fee(pool, cost))?;
        let alpha = pool.alpha.checked_sub(alpha_out)?;
        Some((pool.with_reserves(tao, alpha), cost))
    }

    fn after_buy_tao(pool: &PoolState, tao_in: u64) -> Option<(PoolState, u64)> {
        let alpha_out = Self::out_alpha_for_tao(pool, tao_in);
        let tao = pool.tao.checked_add(net_of_fee(pool, tao_in))?;
        let alpha = pool.alpha.checked_sub(alpha_out)?;
        Some((pool.with_reserves(tao, alpha), alpha_out))
    }
}
