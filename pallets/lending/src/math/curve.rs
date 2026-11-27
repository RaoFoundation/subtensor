//! Pool quotes and the hypothetical pump/dump (SPEC 1.1, 1.4, 7.1.3, 7.1.5).
//!
//! [`ConstantProduct`] is the reference model's pool (`model/v201/pool.py`) bit for bit: fee
//! 33/65535 taken from the input (rounded up), outputs rounded down, required inputs rounded up.

use super::fixed::{Price, Q64, ceil_div, isqrt, mul_div_ceil, mul_div_floor, price};

/// Swap fee numerator (subtensor swap pallet): 33/65535 of the input.
pub const FEE_NUM: u128 = 33;
/// Swap fee denominator.
pub const FEE_DEN: u128 = 65_535;
/// `FEE_DEN - FEE_NUM`: the part of an input that reaches the curve.
const FEE_KEEP: u128 = 65_502;

/// Pool reserves in rao: `SubnetTAO` and `SubnetAlphaIn`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PoolState {
    pub tao: u64,
    pub alpha: u64,
}

/// The pool's pricing function. All methods are pure; quotes never move reserves.
///
/// Costs that can exceed any real balance are `u128`; `None` always means "infinite"
/// (the pool cannot supply the amount, or the result is not representable).
pub trait Curve {
    /// Spot price `floor(tao * 2^64 / alpha)`; zero for an empty pool.
    #[must_use]
    fn spot(pool: &PoolState) -> Price;

    /// Alpha bought by swapping `tao_in` (fee on input), rounded down.
    #[must_use]
    fn out_alpha_for_tao(pool: &PoolState, tao_in: u64) -> u64;

    /// TAO received for swapping `alpha_in` (fee on input), rounded down.
    #[must_use]
    fn out_tao_for_alpha(pool: &PoolState, alpha_in: u64) -> u64;

    /// Smallest TAO input whose swap returns at least `alpha_out`. `None` if
    /// `alpha_out >= pool.alpha`. Used by coverage on the pumped pool (SPEC 1.1).
    #[must_use]
    fn tao_in_for_alpha(pool: &PoolState, alpha_out: u64) -> Option<u128>;

    /// Fee-free TAO to buy `alpha_out`, rounded up (`buy_cost_on`, SPEC 1.4). `None` if
    /// `alpha_out >= pool.alpha`.
    #[must_use]
    fn buy_cost_nofee(pool: &PoolState, alpha_out: u64) -> Option<u128>;

    /// Fee-free TAO for selling `alpha_in`, rounded down (SPEC 7.1.5).
    #[must_use]
    fn sell_proceeds_nofee(pool: &PoolState, alpha_in: u64) -> u64;

    /// The pool after a hypothetical fee-free buy up to spot `target` on the same invariant
    /// (SPEC 1.1). Unchanged if spot is already at or above `target`.
    #[must_use]
    fn pumped(pool: &PoolState, target: Price) -> Option<PoolState>;

    /// The pool after a hypothetical fee-free sale down to spot `target` on the same invariant
    /// (SPEC 7.1.3). Unchanged if `target` is zero or spot is already at or below it.
    #[must_use]
    fn dumped(pool: &PoolState, target: Price) -> Option<PoolState>;

    /// Spot right after one swap that buys exactly `alpha_out` at its minimal TAO input, the
    /// fee leaving the pool. `None` if the buy is impossible or the price is not representable.
    #[must_use]
    fn spot_after_buy(pool: &PoolState, alpha_out: u64) -> Option<Price>;

    /// The pool right after one swap that sells `alpha_in`, the fee leaving the pool, with the
    /// TAO paid out. `None` if the reserves are not representable.
    #[must_use]
    fn after_sell(pool: &PoolState, alpha_in: u64) -> Option<(PoolState, u64)>;

    /// The pool after one swap that buys exactly `alpha_out` at its minimal TAO input `cost`
    /// (SPEC 1.1). The fee leaves the pool: `net_of_fee(cost)` reaches it. Returns the pool and
    /// `cost`. `None` if the buy is impossible or the reserves are not representable.
    #[must_use]
    fn after_buy_alpha(pool: &PoolState, alpha_out: u64) -> Option<(PoolState, u64)>;

    /// The pool after one swap that spends exactly `tao_in` (SPEC 7.1.1): `net_of_fee(tao_in)`
    /// reaches the pool and the alpha it buys leaves it. Returns the pool and the alpha bought.
    /// `None` if the reserves are not representable.
    #[must_use]
    fn after_buy_tao(pool: &PoolState, tao_in: u64) -> Option<(PoolState, u64)>;
}

/// 50/50 constant-product pool: exact integer math identical to `model/v201/pool.py`.
pub struct ConstantProduct;

/// `amount - ceil(amount * 33 / 65535)`, written as the equal `floor(amount * 65502 / 65535)`.
fn net_of_fee(amount: u128) -> Option<u128> {
    mul_div_floor(amount, FEE_KEEP, FEE_DEN)
}

/// `floor(reserve_out * eff / (reserve_in + eff))` with `eff` the fee'd input.
fn swap_out(reserve_in: u64, reserve_out: u64, amount_in: u64) -> Option<u64> {
    let eff = net_of_fee(u128::from(amount_in))?;
    let out = mul_div_floor(
        u128::from(reserve_out),
        eff,
        u128::from(reserve_in).checked_add(eff)?,
    )?;
    u64::try_from(out).ok()
}

/// `tao * alpha`, the constant-product invariant (always fits `u128`).
fn invariant(pool: &PoolState) -> u128 {
    u128::from(pool.tao).saturating_mul(u128::from(pool.alpha))
}

/// `floor(sqrt(floor(k * 2^64 / target)))`: the alpha reserve at which spot equals `target`.
fn alpha_at_price(k: u128, target: Price) -> Option<u128> {
    mul_div_floor(k, Q64, target.to_bits()).map(isqrt)
}

fn pool_from(tao: u128, alpha: u128) -> Option<PoolState> {
    Some(PoolState {
        tao: u64::try_from(tao).ok()?,
        alpha: u64::try_from(alpha).ok()?,
    })
}

impl Curve for ConstantProduct {
    fn spot(pool: &PoolState) -> Price {
        price(pool.tao, u128::from(pool.alpha))
    }

    fn out_alpha_for_tao(pool: &PoolState, tao_in: u64) -> u64 {
        swap_out(pool.tao, pool.alpha, tao_in).unwrap_or(0)
    }

    fn out_tao_for_alpha(pool: &PoolState, alpha_in: u64) -> u64 {
        swap_out(pool.alpha, pool.tao, alpha_in).unwrap_or(0)
    }

    /// The model searches around `ceil(eff * 65535 / 65502)`; that value is already the
    /// minimum because `tin - fee(tin) = floor(tin * 65502 / 65535) >= eff` is equivalent to
    /// `tin >= ceil(eff * 65535 / 65502)`, so no search is needed.
    fn tao_in_for_alpha(pool: &PoolState, alpha_out: u64) -> Option<u128> {
        let eff = Self::buy_cost_nofee(pool, alpha_out)?;
        mul_div_ceil(eff, FEE_DEN, FEE_KEEP)
    }

    fn buy_cost_nofee(pool: &PoolState, alpha_out: u64) -> Option<u128> {
        if alpha_out == 0 {
            return Some(0);
        }
        let left = pool.alpha.checked_sub(alpha_out).filter(|left| *left > 0)?;
        ceil_div(
            u128::from(alpha_out).saturating_mul(u128::from(pool.tao)),
            u128::from(left),
        )
    }

    fn sell_proceeds_nofee(pool: &PoolState, alpha_in: u64) -> u64 {
        let reserve = u128::from(pool.alpha).saturating_add(u128::from(alpha_in));
        let out = mul_div_floor(u128::from(pool.tao), u128::from(alpha_in), reserve);
        out.and_then(|v| u64::try_from(v).ok()).unwrap_or(0)
    }

    fn pumped(pool: &PoolState, target: Price) -> Option<PoolState> {
        if Self::spot(pool) >= target {
            return Some(*pool);
        }
        let k = invariant(pool);
        let alpha = alpha_at_price(k, target)?.max(1);
        pool_from(ceil_div(k, alpha)?, alpha)
    }

    fn dumped(pool: &PoolState, target: Price) -> Option<PoolState> {
        if target == Price::from_bits(0) || Self::spot(pool) <= target {
            return Some(*pool);
        }
        let k = invariant(pool);
        let alpha = alpha_at_price(k, target)?;
        pool_from(k.checked_div(alpha)?, alpha)
    }

    fn spot_after_buy(pool: &PoolState, alpha_out: u64) -> Option<Price> {
        let cost = Self::tao_in_for_alpha(pool, alpha_out)?;
        let tao = u128::from(pool.tao).checked_add(net_of_fee(cost)?)?;
        let alpha = pool.alpha.checked_sub(alpha_out)?;
        mul_div_floor(tao, Q64, u128::from(alpha)).map(Price::from_bits)
    }

    fn after_sell(pool: &PoolState, alpha_in: u64) -> Option<(PoolState, u64)> {
        let tao_out = Self::out_tao_for_alpha(pool, alpha_in);
        let alpha = u128::from(pool.alpha).checked_add(net_of_fee(u128::from(alpha_in))?)?;
        let after = PoolState {
            tao: pool.tao.checked_sub(tao_out)?,
            alpha: u64::try_from(alpha).ok()?,
        };
        Some((after, tao_out))
    }

    fn after_buy_alpha(pool: &PoolState, alpha_out: u64) -> Option<(PoolState, u64)> {
        let cost = Self::tao_in_for_alpha(pool, alpha_out)?;
        let cost_tao = u64::try_from(cost).ok()?;
        let tao = pool
            .tao
            .checked_add(u64::try_from(net_of_fee(cost)?).ok()?)?;
        let alpha = pool.alpha.checked_sub(alpha_out)?;
        Some((PoolState { tao, alpha }, cost_tao))
    }

    fn after_buy_tao(pool: &PoolState, tao_in: u64) -> Option<(PoolState, u64)> {
        let alpha_out = Self::out_alpha_for_tao(pool, tao_in);
        let tao = pool
            .tao
            .checked_add(u64::try_from(net_of_fee(u128::from(tao_in))?).ok()?)?;
        let alpha = pool.alpha.checked_sub(alpha_out)?;
        Some((PoolState { tao, alpha }, alpha_out))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::arithmetic_side_effects, clippy::unwrap_used)]
    use super::*;

    type Cp = ConstantProduct;
    const RAO: u64 = 1_000_000_000;
    const POOL: PoolState = PoolState {
        tao: 1_000 * RAO,
        alpha: 480_000 * RAO,
    };

    #[test]
    fn zero_amounts_quote_zero() {
        assert_eq!(Cp::out_alpha_for_tao(&POOL, 0), 0);
        assert_eq!(Cp::out_tao_for_alpha(&POOL, 0), 0);
        assert_eq!(Cp::tao_in_for_alpha(&POOL, 0), Some(0));
        assert_eq!(Cp::buy_cost_nofee(&POOL, 0), Some(0));
        assert_eq!(Cp::sell_proceeds_nofee(&POOL, 0), 0);
    }

    #[test]
    fn buying_the_whole_pool_or_more_is_infinite() {
        for a in [POOL.alpha, POOL.alpha + 1, u64::MAX] {
            assert_eq!(Cp::tao_in_for_alpha(&POOL, a), None);
            assert_eq!(Cp::buy_cost_nofee(&POOL, a), None);
            assert_eq!(Cp::spot_after_buy(&POOL, a), None);
        }
        assert!(Cp::buy_cost_nofee(&POOL, POOL.alpha - 1).is_some());
    }

    #[test]
    fn empty_pool_is_total() {
        let empty = PoolState { tao: 0, alpha: 0 };
        assert_eq!(Cp::spot(&empty).to_bits(), 0);
        assert_eq!(Cp::out_alpha_for_tao(&empty, 0), 0);
        assert_eq!(Cp::out_tao_for_alpha(&empty, 0), 0);
        assert_eq!(Cp::sell_proceeds_nofee(&empty, 0), 0);
        assert_eq!(Cp::buy_cost_nofee(&empty, 1), None);
        assert_eq!(
            Cp::pumped(&empty, Price::from_bits(Q64)),
            Some(PoolState { tao: 0, alpha: 1 })
        );
        assert_eq!(Cp::dumped(&empty, Price::from_bits(Q64)), Some(empty));
    }

    #[test]
    fn fee_is_taken_from_the_input_rounded_up() {
        // 65535 in: fee 33, 65502 reach the curve. One rao in: the whole rao is fee.
        assert_eq!(net_of_fee(65_535), Some(65_502));
        assert_eq!(net_of_fee(1), Some(0));
        assert_eq!(net_of_fee(65_536), Some(65_502));
        assert_eq!(Cp::out_alpha_for_tao(&POOL, 1), 0);
    }

    #[test]
    fn tao_in_for_alpha_is_the_minimal_input() {
        for pool in [
            POOL,
            PoolState {
                tao: 1_000 * RAO + 1,
                alpha: 1_000 * RAO + 7,
            },
        ] {
            for a in [
                1,
                2,
                37,
                65_535,
                RAO,
                pool.alpha / 1_000,
                pool.alpha / 3,
                pool.alpha / 2,
            ] {
                let tin = u64::try_from(Cp::tao_in_for_alpha(&pool, a).unwrap()).unwrap();
                assert!(Cp::out_alpha_for_tao(&pool, tin) >= a, "{a}");
                assert!(Cp::out_alpha_for_tao(&pool, tin - 1) < a, "{a}");
            }
        }
    }

    #[test]
    fn fee_free_quotes_round_for_the_pool() {
        let pool = PoolState { tao: 10, alpha: 7 };
        // buy 2: 2*10/5 = 4 exactly; buy 3: 30/4 = 7.5 -> 8.
        assert_eq!(Cp::buy_cost_nofee(&pool, 2), Some(4));
        assert_eq!(Cp::buy_cost_nofee(&pool, 3), Some(8));
        // sell 3: 10*3/10 = 3; sell 2: 20/9 = 2.2 -> 2.
        assert_eq!(Cp::sell_proceeds_nofee(&pool, 3), 3);
        assert_eq!(Cp::sell_proceeds_nofee(&pool, 2), 2);
    }

    #[test]
    fn pump_and_dump_leave_the_pool_when_already_past_the_target() {
        let spot = Cp::spot(&POOL);
        let below = Price::from_bits(spot.to_bits() / 2);
        let above = Price::from_bits(spot.to_bits() * 2);
        assert_eq!(Cp::pumped(&POOL, spot), Some(POOL));
        assert_eq!(Cp::pumped(&POOL, below), Some(POOL));
        assert_eq!(Cp::dumped(&POOL, spot), Some(POOL));
        assert_eq!(Cp::dumped(&POOL, above), Some(POOL));
        assert_eq!(Cp::dumped(&POOL, Price::from_bits(0)), Some(POOL));
    }

    #[test]
    fn pump_and_dump_move_along_the_invariant() {
        // Spot 2^-9 exactly, so a 4x move halves or doubles each reserve with no rounding.
        let pool = PoolState {
            tao: 1_000 * RAO,
            alpha: 512_000 * RAO,
        };
        let spot = Cp::spot(&pool).to_bits();
        let up = Cp::pumped(&pool, Price::from_bits(spot * 4)).unwrap();
        assert_eq!(
            up,
            PoolState {
                tao: 2_000 * RAO,
                alpha: 256_000 * RAO
            }
        );
        let down = Cp::dumped(&pool, Price::from_bits(spot / 4)).unwrap();
        assert_eq!(
            down,
            PoolState {
                tao: 500 * RAO,
                alpha: 1_024_000 * RAO
            }
        );
    }

    #[test]
    fn unrepresentable_pump_is_infinite() {
        let pool = PoolState {
            tao: u64::MAX,
            alpha: u64::MAX,
        };
        assert_eq!(Cp::pumped(&pool, Price::from_bits(4 * Q64)), None);
        assert_eq!(Cp::dumped(&pool, Price::from_bits(Q64 / 4)), None);
    }

    #[test]
    fn after_sell_keeps_the_fee_out_of_the_pool() {
        let a = 65_535 * 1_000;
        let (after, out) = Cp::after_sell(&POOL, a).unwrap();
        assert_eq!(out, Cp::out_tao_for_alpha(&POOL, a));
        assert_eq!(after.tao, POOL.tao - out);
        assert_eq!(after.alpha, POOL.alpha + 65_502 * 1_000);
        assert_eq!(
            Cp::after_sell(
                &PoolState {
                    tao: 1,
                    alpha: u64::MAX
                },
                u64::MAX
            ),
            None
        );
    }

    #[test]
    fn after_buy_alpha_pays_the_minimal_cost_and_keeps_the_fee_out() {
        let a = POOL.alpha / 1_000;
        let (after, cost) = Cp::after_buy_alpha(&POOL, a).unwrap();
        assert_eq!(u128::from(cost), Cp::tao_in_for_alpha(&POOL, a).unwrap());
        assert_eq!(after.alpha, POOL.alpha - a);
        assert_eq!(
            after.tao,
            POOL.tao + u64::try_from(net_of_fee(u128::from(cost)).unwrap()).unwrap()
        );
        assert!(Cp::out_alpha_for_tao(&POOL, cost) >= a);
        assert_eq!(Cp::after_buy_alpha(&POOL, POOL.alpha), None);
        assert_eq!(Cp::after_buy_alpha(&POOL, 0), Some((POOL, 0)));
    }

    #[test]
    fn after_buy_tao_matches_the_swap_quote() {
        let tin = 3 * RAO;
        let (after, out) = Cp::after_buy_tao(&POOL, tin).unwrap();
        assert_eq!(out, Cp::out_alpha_for_tao(&POOL, tin));
        assert_eq!(after.alpha, POOL.alpha - out);
        assert_eq!(
            after.tao,
            POOL.tao + u64::try_from(net_of_fee(u128::from(tin)).unwrap()).unwrap()
        );
        assert_eq!(Cp::after_buy_tao(&POOL, 0), Some((POOL, 0)));
    }

    #[test]
    fn spot_after_buy_rises() {
        let after = Cp::spot_after_buy(&POOL, POOL.alpha / 100).unwrap();
        assert!(after > Cp::spot(&POOL));
        assert_eq!(Cp::spot_after_buy(&POOL, 0), Some(Cp::spot(&POOL)));
    }
}
