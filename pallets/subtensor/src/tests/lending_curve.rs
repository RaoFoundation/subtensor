//! `BalancerCurve`, the lending pallet's pricing function on subnet pools: its agreement with
//! the lending model's `ConstantProduct` at weight 1/2 on the golden vectors, and its equality
//! with the swap pallet's actual swaps at other weights.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]

use frame_support::assert_ok;
use pallet_lending::math::{
    curve::{ConstantProduct as Cp, Curve, PoolState},
    fixed::Price,
};
use pallet_subtensor_swap::{FeeRate, PalSwapInitialized, SwapBalancer, balancer::Balancer};
use serde_json::Value;
use sp_core::U256;
use sp_runtime::Perquintill;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance};
use subtensor_swap_interface::{Order, SwapHandler};

use super::mock::*;
use crate::staking::lending::BalancerCurve as Bal;
use crate::*;

/// Row counts of the golden curve section (`gen_golden.py`); a shorter file must not pass.
const CURVE_QUOTE_ROWS: usize = 320;
const CURVE_PRICE_ROWS: usize = 200;

fn golden() -> Value {
    serde_json::from_str(include_str!("../../../lending/src/tests/golden/math.json"))
        .expect("golden JSON parses")
}

fn num(v: &Value) -> u128 {
    v.as_str()
        .expect("decimal string")
        .parse()
        .expect("integer")
}

fn amount(v: &Value) -> u64 {
    u64::try_from(num(v)).expect("fits u64")
}

/// The largest difference seen per quantity, in rao.
#[derive(Debug, Default, PartialEq, Eq)]
struct Gap {
    spot_bits: u128,
    buy_cost_nofee: u128,
    sell_proceeds_nofee: u128,
    pumped: u128,
    dumped: u128,
    out_alpha_for_tao: u128,
    out_tao_for_alpha: u128,
    tao_in_for_alpha: u128,
}

fn widen(max: &mut u128, a: u128, b: u128) -> u128 {
    let gap = a.abs_diff(b);
    *max = (*max).max(gap);
    gap
}

fn pool_gap(a: Option<PoolState>, b: Option<PoolState>) -> u128 {
    match (a, b) {
        (Some(a), Some(b)) => a.tao.abs_diff(b.tao).max(a.alpha.abs_diff(b.alpha)).into(),
        (None, None) => 0,
        other => panic!("one curve is infinite: {other:?}"),
    }
}

/// Largest absolute gap, in rao, between a fee-free quote of `q` on a pool with `t` TAO and the
/// swap pallet's. The swap pallet evaluates `(t / (t + q))^(w)` at 18 decimals, so the ratio
/// carries an absolute error of 1e-18 and the quote one of `q * (q + t) / t * 1e-18`. That is
/// the dominant term for large quotes; `3` covers the one-rao rounding at each end.
fn rounding_bound(q: u128, t: u128) -> u128 {
    3 + 2 * q.saturating_mul(q.saturating_add(t)) / t.saturating_mul(1_000_000_000_000_000_000)
}

/// (a) At weight 1/2 `BalancerCurve` is `ConstantProduct` up to rounding, on every curve row of
/// the lending golden vectors. Each row is checked against its own bound:
///
/// - `spot`: the same `U64F64` quotient, so 0.
/// - Pumped and dumped pools: within 1 rao per reserve.
/// - Fee-free `buy_cost_nofee`, `sell_proceeds_nofee`: the 18-decimal power, see
///   [`rounding_bound`]. Sells are within 1 rao (their quotient is at most the reserve).
/// - Fee'd `out_alpha_for_tao`, `out_tao_for_alpha`: the model takes the fee
///   `ceil(in * 33 / 65535)`, the swap pallet `floor(in * 33 / 65535)` up to a 2^-64 rate error,
///   so the net input differs by at most 2 rao. One rao of input moves the output by at most the
///   spot price, so the bound is `3 * ceil(spot) + 1` rao of output.
/// - Fee'd `tao_in_for_alpha`: the same fee difference on the gross input, plus the power term.
///
/// The one row where the minimal fee-free cost exceeds `u64::MAX` is `None` for `BalancerCurve`:
/// no swap can take an input that large (the swap pallet caps inputs at 1000x the reserve).
#[test]
fn balancer_at_half_weight_agrees_with_constant_product_on_golden_rows() {
    let doc = golden();
    let mut gap = Gap::default();
    let (mut quotes, mut prices, mut beyond_u64) = (0, 0, 0);
    for (i, row) in doc["curve"].as_array().unwrap().iter().enumerate() {
        let p = PoolState::model(amount(&row["tao"]), amount(&row["alpha"]));
        if row.get("price").is_some() {
            let target = Price::from_bits(num(&row["price"]));
            let spot = widen(
                &mut gap.spot_bits,
                Bal::spot(&p).to_bits(),
                Cp::spot(&p).to_bits(),
            );
            assert_eq!(spot, 0, "row {i}: spot");
            let up = pool_gap(Bal::pumped(&p, target), Cp::pumped(&p, target));
            let down = pool_gap(Bal::dumped(&p, target), Cp::dumped(&p, target));
            assert!(up <= 1 && down <= 1, "row {i}: pumped {up}, dumped {down}");
            gap.pumped = gap.pumped.max(up);
            gap.dumped = gap.dumped.max(down);
            prices += 1;
        } else {
            let (a, tin) = (amount(&row["a"]), amount(&row["tin"]));

            match (Bal::buy_cost_nofee(&p, a), Cp::buy_cost_nofee(&p, a)) {
                (Some(bal), Some(cp)) => {
                    let g = widen(&mut gap.buy_cost_nofee, bal, cp);
                    assert!(
                        g <= rounding_bound(cp, u128::from(p.tao)),
                        "row {i}: buy_cost_nofee {bal} vs {cp}"
                    );
                }
                // The model's cost is beyond any `u64` input, which no swap can take: `None`.
                (None, Some(cp)) if cp > u128::from(u64::MAX) => beyond_u64 += 1,
                other => panic!("buy_cost_nofee, row {i}: {other:?}"),
            }

            let (bal, cp) = (
                Bal::sell_proceeds_nofee(&p, a),
                Cp::sell_proceeds_nofee(&p, a),
            );
            let g = widen(&mut gap.sell_proceeds_nofee, bal.into(), cp.into());
            assert!(g <= 1, "row {i}: sell_proceeds_nofee {bal} vs {cp}");

            let (bal, cp) = (
                Bal::out_alpha_for_tao(&p, tin),
                Cp::out_alpha_for_tao(&p, tin),
            );
            let g = widen(&mut gap.out_alpha_for_tao, bal.into(), cp.into());
            let bound = 3 * u128::from(p.alpha).div_ceil(u128::from(p.tao)) + 1;
            assert!(g <= bound, "row {i}: out_alpha_for_tao {bal} vs {cp}");

            let (bal, cp) = (Bal::out_tao_for_alpha(&p, a), Cp::out_tao_for_alpha(&p, a));
            let g = widen(&mut gap.out_tao_for_alpha, bal.into(), cp.into());
            let bound = 3 * u128::from(p.tao).div_ceil(u128::from(p.alpha)) + 1;
            assert!(g <= bound, "row {i}: out_tao_for_alpha {bal} vs {cp}");

            match (Bal::tao_in_for_alpha(&p, a), Cp::tao_in_for_alpha(&p, a)) {
                (Some(bal), Some(cp)) => {
                    let g = widen(&mut gap.tao_in_for_alpha, bal, cp);
                    assert!(
                        g <= rounding_bound(cp, u128::from(p.tao)),
                        "row {i}: tao_in_for_alpha {bal} vs {cp}"
                    );
                }
                (None, Some(cp)) if cp > u128::from(u64::MAX) => beyond_u64 += 1,
                other => panic!("tao_in_for_alpha, row {i}: {other:?}"),
            }
            quotes += 1;
        }
    }
    assert_eq!((quotes, prices), (CURVE_QUOTE_ROWS, CURVE_PRICE_ROWS));
    println!("measured 1/2-weight agreement: {gap:?}, beyond u64: {beyond_u64}");
}

/// SplitMix64: a fixed, seeded stream of sizes.
struct Seeded(u64);

impl Seeded {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Log-uniform between 2^20 rao and about `cap`.
    fn size(&mut self, cap: u64) -> u64 {
        let bits = 20 + self.next() % u64::from(cap.ilog2() - 19);
        ((1 << bits) + self.next() % (1 << bits)).min(cap)
    }
}

const RAO: u64 = 1_000_000_000;
const SIZES: usize = 50;

/// The live pool as the lending pallet's curve sees it.
fn live_pool(netuid: NetUid) -> PoolState {
    PoolState {
        tao: SubnetTAO::<Test>::get(netuid).to_u64(),
        alpha: SubnetAlphaIn::<Test>::get(netuid).to_u64(),
        quote_weight: SwapBalancer::<Test>::get(netuid).get_quote_weight(),
        fee: FeeRate::<Test>::get(netuid),
    }
}

/// A dry run of the swap pallet's buy on the live pool: the alpha it pays out.
fn sim_buy(netuid: NetUid, tao: u64, drop_fees: bool) -> u64 {
    let order = GetAlphaForTao::<Test>::with_amount(TaoBalance::from(tao));
    let limit = <Test as Config>::SwapInterface::max_price();
    <Test as Config>::SwapInterface::swap(netuid, order, limit, drop_fees, true)
        .map_or(0, |result| result.amount_paid_out.to_u64())
}

/// A dry run of the swap pallet's sell on the live pool: the TAO it pays out.
fn sim_sell(netuid: NetUid, alpha: u64, drop_fees: bool) -> u64 {
    let order = GetTaoForAlpha::<Test>::with_amount(AlphaBalance::from(alpha));
    let limit = <Test as Config>::SwapInterface::min_price();
    <Test as Config>::SwapInterface::swap(netuid, order, limit, drop_fees, true)
        .map_or(0, |result| result.amount_paid_out.to_u64())
}

/// (b) At quote weights 0.3 and 0.7 every `BalancerCurve` quote equals what the swap pallet
/// does on a subnet pool, for 50 seeded sizes per pool: the executed buy and sell (amount paid
/// out and the reserves left), the fee-free dry runs, and the minimality of the required
/// inputs. The pool moves with every executed swap, so each size meets a different state.
#[test]
fn balancer_quotes_equal_the_swap_pallet_on_weighted_pools() {
    for (case, (weight, fee)) in [(30u64, 33u16), (70, 33), (70, 500)]
        .into_iter()
        .enumerate()
    {
        new_test_ext(1).execute_with(|| {
            let netuid = add_dynamic_network(&U256::from(1), &U256::from(2));
            setup_reserves(
                netuid,
                TaoBalance::from(50_000 * RAO),
                AlphaBalance::from(2_000_000 * RAO),
            );
            let balancer = Balancer::new(Perquintill::from_percent(weight)).unwrap();
            SwapBalancer::<Test>::insert(netuid, balancer);
            PalSwapInitialized::<Test>::insert(netuid, true);
            FeeRate::<Test>::insert(netuid, fee);
            SubnetAlphaOut::<Test>::insert(netuid, AlphaBalance::from(10_000_000 * RAO));

            let mut rng = Seeded(0x1E4D_0000 + case as u64);
            for i in 0..SIZES {
                let pool = live_pool(netuid);
                assert_eq!(
                    Bal::spot(&pool),
                    <Test as Config>::SwapInterface::current_alpha_price(netuid),
                    "spot, size {i}"
                );
                let tao = rng.size(pool.tao / 20);
                let alpha = rng.size(pool.alpha / 20);

                // Fee-free quotes against fee-free dry runs.
                assert_eq!(
                    Bal::sell_proceeds_nofee(&pool, alpha),
                    sim_sell(netuid, alpha, true),
                    "sell_proceeds_nofee, size {i}"
                );
                let cost = u64::try_from(Bal::buy_cost_nofee(&pool, alpha).unwrap()).unwrap();
                assert!(
                    sim_buy(netuid, cost, true) >= alpha,
                    "buy_cost_nofee, size {i}"
                );
                assert!(
                    sim_buy(netuid, cost - 1, true) < alpha,
                    "buy_cost_nofee, size {i}"
                );

                // The minimal fee'd input for an exact amount of alpha.
                let (after, cost) = Bal::after_buy_alpha(&pool, alpha).unwrap();
                assert_eq!(Bal::tao_in_for_alpha(&pool, alpha), Some(cost.into()));
                assert!(
                    sim_buy(netuid, cost, false) >= alpha,
                    "tao_in_for_alpha, size {i}"
                );
                assert!(
                    sim_buy(netuid, cost - 1, false) < alpha,
                    "tao_in_for_alpha, size {i}"
                );
                assert_eq!(Bal::spot_after_buy(&pool, alpha), Some(Bal::spot(&after)));

                // Fee'd quotes against dry runs, then against the executed swaps.
                assert_eq!(
                    Bal::out_alpha_for_tao(&pool, tao),
                    sim_buy(netuid, tao, false)
                );
                assert_eq!(
                    Bal::out_tao_for_alpha(&pool, alpha),
                    sim_sell(netuid, alpha, false)
                );

                let (after, quoted) = Bal::after_buy_tao(&pool, tao).unwrap();
                let limit = <Test as Config>::SwapInterface::max_price();
                let swapped =
                    SubtensorModule::swap_tao_for_alpha(netuid, tao.into(), limit, false).unwrap();
                assert_eq!(quoted, swapped.amount_paid_out.to_u64(), "buy, size {i}");
                assert!(quoted > 0);
                assert_eq!(after, live_pool(netuid), "pool after buy, size {i}");

                let pool = live_pool(netuid);
                let (after, quoted) = Bal::after_sell(&pool, alpha).unwrap();
                let limit = <Test as Config>::SwapInterface::min_price();
                let swapped =
                    SubtensorModule::swap_alpha_for_tao(netuid, alpha.into(), limit, false)
                        .unwrap();
                assert_eq!(quoted, swapped.amount_paid_out.to_u64(), "sell, size {i}");
                assert!(quoted > 0);
                assert_eq!(after, live_pool(netuid), "pool after sell, size {i}");
            }
            assert_ok!(Balancer::new(live_pool(netuid).quote_weight));
        });
    }
}

/// The hypothetical pump and dump keep the weighted invariant and land on the target price:
/// the pumped pool's spot is at or just above the target, the dumped pool's within a rao of
/// reserve rounding of it, and buying or selling back along the curve returns to the start.
#[test]
fn at_price_moves_along_the_weighted_invariant() {
    for weight in [30u64, 50, 70] {
        let pool = PoolState {
            quote_weight: Perquintill::from_percent(weight),
            ..PoolState::model(50_000 * RAO, 2_000_000 * RAO)
        };
        let spot = Bal::spot(&pool).to_bits();
        assert_eq!(Bal::pumped(&pool, Price::from_bits(spot)), Some(pool));
        assert_eq!(Bal::pumped(&pool, Price::from_bits(spot / 2)), Some(pool));
        assert_eq!(Bal::dumped(&pool, Price::from_bits(spot)), Some(pool));
        assert_eq!(Bal::dumped(&pool, Price::from_bits(spot * 2)), Some(pool));
        assert_eq!(Bal::dumped(&pool, Price::from_bits(0)), Some(pool));

        for (num, den) in [(3u128, 2u128), (4, 1), (11, 10), (10_005, 10_000)] {
            let target = spot * num / den;
            let up = Bal::pumped(&pool, Price::from_bits(target)).unwrap();
            assert!(up.tao > pool.tao && up.alpha < pool.alpha);
            let reached = Bal::spot(&up).to_bits();
            assert!(reached >= target, "{weight}%: pumped spot below target");
            assert!(
                reached - target <= target / 1_000_000_000,
                "{weight}%: {reached}"
            );
            // The fee-free buy of the alpha that left costs the TAO that entered.
            let cost = Bal::buy_cost_nofee(&pool, pool.alpha - up.alpha).unwrap();
            assert!(
                cost.abs_diff(u128::from(up.tao - pool.tao)) <= 1,
                "{weight}%"
            );

            let target = spot * den / num;
            let down = Bal::dumped(&pool, Price::from_bits(target)).unwrap();
            assert!(down.tao < pool.tao && down.alpha > pool.alpha);
            let reached = Bal::spot(&down).to_bits();
            assert!(
                reached.abs_diff(target) <= target / 1_000_000_000,
                "{weight}%: {reached}"
            );
            let proceeds = Bal::sell_proceeds_nofee(&pool, down.alpha - pool.alpha);
            assert!(proceeds.abs_diff(pool.tao - down.tao) <= 1, "{weight}%");
        }
    }
}

#[test]
fn impossible_quotes_are_infinite_or_zero() {
    let pool = PoolState {
        quote_weight: Perquintill::from_percent(30),
        ..PoolState::model(50_000 * RAO, 2_000_000 * RAO)
    };
    assert_eq!(Bal::tao_in_for_alpha(&pool, 0), Some(0));
    assert_eq!(Bal::buy_cost_nofee(&pool, 0), Some(0));
    assert_eq!(Bal::after_buy_alpha(&pool, 0), Some((pool, 0)));
    assert_eq!(Bal::after_buy_tao(&pool, 0), Some((pool, 0)));
    assert_eq!(Bal::after_sell(&pool, 0), Some((pool, 0)));
    for a in [pool.alpha, pool.alpha + 1, u64::MAX] {
        assert_eq!(Bal::tao_in_for_alpha(&pool, a), None);
        assert_eq!(Bal::buy_cost_nofee(&pool, a), None);
        assert_eq!(Bal::after_buy_alpha(&pool, a), None);
        assert_eq!(Bal::spot_after_buy(&pool, a), None);
    }
    let empty = pool.with_reserves(0, 0);
    assert_eq!(Bal::spot(&empty).to_bits(), 0);
    assert_eq!(Bal::pumped(&empty, Price::from_bits(1 << 64)), None);
    // A weight the swap pallet refuses prices nothing.
    let invalid = PoolState {
        quote_weight: Perquintill::from_parts(1),
        ..pool
    };
    assert_eq!(Bal::spot(&invalid).to_bits(), 0);
    assert_eq!(Bal::out_alpha_for_tao(&invalid, RAO), 0);
    assert_eq!(Bal::tao_in_for_alpha(&invalid, RAO), None);
}
