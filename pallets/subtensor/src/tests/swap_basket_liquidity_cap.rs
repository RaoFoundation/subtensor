//! `swap_basket` liquidity cap ([`BasketLiquidityCap`]): a fund may not hold more than the
//! cap's share of a destination pool's alpha reserve after a buy.
//!
//! Motivation: the concentration cap marks holdings at realizable value, which is bounded by
//! the pool's TAO reserve. On a thin pool a stolen key can buy in 2%-band slices while a
//! counterparty sells alpha back to the EMA between slices; every slice passes the slippage,
//! turnover, and concentration rules, yet the fund's realizable NAV collapses by roughly the
//! turnover it spent. The liquidity cap stops that accumulation.
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used)]

use crate::tests::claim_root::{
    escrow_alpha, flush_baskets, register_on_root, set_root_weights_direct, zero_claim_threshold,
};
use crate::tests::mock::*;
use crate::{
    BASKET_TRADE_REFILL_BLOCKS, BasketDailyTurnoverCap, BasketLiquidityCap, BasketTradeBucket,
    BasketTradingEnabled, Error, Owner, PendingBasketDeposits, RootClaimableThreshold,
    RootWeightsCap, SubnetAlphaIn, SubnetAlphaOut, SubnetMovingPrice, SubnetTAO, TotalStake,
};
use frame_support::dispatch::GetDispatchInfo;
use frame_support::pallet_prelude::Weight;
use frame_support::{assert_noop, assert_ok};
use sp_core::U256;
use sp_runtime::Saturating;
use substrate_fixed::types::I96F32;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};
use subtensor_swap_interface::SwapHandler;

const TAO: u64 = 1_000_000_000;

fn escrow() -> U256 {
    SubtensorModule::get_beta_escrow_account_id()
}

/// A dynamic subnet with a pool of `tao` TAO against `alpha` alpha and its moving price
/// pinned to spot, so the slippage band never intervenes.
fn make_pool(hotkey: &U256, coldkey: &U256, tao: u64, alpha: u64) -> NetUid {
    let netuid = add_dynamic_network(hotkey, coldkey);
    remove_owner_registration_stake(netuid);
    SubnetTAO::<Test>::insert(netuid, TaoBalance::from(tao));
    SubnetAlphaIn::<Test>::insert(netuid, AlphaBalance::from(alpha));
    let subnet_account = SubtensorModule::get_subnet_account_id(netuid).unwrap();
    add_balance_to_coldkey_account(&subnet_account, TaoBalance::from(tao));
    SubnetMovingPrice::<Test>::insert(netuid, I96F32::from_num(tao as f64 / alpha as f64));
    netuid
}

/// A root-registered validator whose fund holds `tao` TAO in its root (cash) slot, with
/// the root reserves credited the way `credit_root_reserves` does.
fn make_fund_with_cash(coldkey: U256, hotkey: U256, tao: u64) {
    register_on_root(&hotkey, 0);
    Owner::<Test>::insert(hotkey, coldkey);
    SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
        &hotkey,
        &escrow(),
        NetUid::ROOT,
        tao.into(),
    );
    SubnetTAO::<Test>::mutate(NetUid::ROOT, |t| *t = t.saturating_add(tao.into()));
    SubnetAlphaOut::<Test>::mutate(NetUid::ROOT, |t| *t = t.saturating_add(tao.into()));
    TotalStake::<Test>::mutate(|t| *t = t.saturating_add(tao.into()));
    let root_account = SubtensorModule::get_subnet_account_id(NetUid::ROOT).unwrap();
    add_balance_to_coldkey_account(&root_account, TaoBalance::from(tao));
}

fn nav(hotkey: &U256) -> u64 {
    SubtensorModule::get_validator_basket_nav_tao(hotkey).to_u64()
}

/// The counterparty sells alpha until spot is back at the moving price (constant product:
/// `A' = sqrt(k / p)`), so the fund's next slice sees an un-moved reference.
fn sell_back_to_ema(netuid: NetUid) {
    let r = SubnetTAO::<Test>::get(netuid).to_u64() as f64;
    let a = SubnetAlphaIn::<Test>::get(netuid).to_u64() as f64;
    let target = SubnetMovingPrice::<Test>::get(netuid).to_num::<f64>();
    let to_sell = (((r * a / target).sqrt() - a).max(0.0) * 1.003) as u64;
    if to_sell > 0 {
        assert_ok!(SubtensorModule::swap_alpha_for_tao(
            netuid,
            to_sell.into(),
            <Test as crate::Config>::SwapInterface::min_price::<TaoBalance>(),
            false,
        ));
    }
}

/// The thin-pool drain: 9 τ slices (< 1% of the reserve, so < 2% marginal move) into a
/// 1,000 τ pool with a counterparty selling back to the EMA between slices. Without the
/// liquidity cap this runs until the turnover budget is spent and loses ~90% of what it
/// spends; with the cap it stops once the fund holds 10% of the pool's alpha reserve, and
/// the fund's realizable NAV loss is bounded to a sliver of the pool's TAO reserve.
#[test]
fn test_liquidity_cap_stops_thin_pool_drain() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        BasketTradingEnabled::<Test>::put(true);
        SubtensorModule::set_tao_weight(u64::MAX);
        let thin = make_pool(&U256::from(4), &U256::from(3), 1_000 * TAO, 100_000 * TAO);
        make_fund_with_cash(coldkey, hotkey, 100_000 * TAO);

        let nav_before = nav(&hotkey);
        let budget = SubtensorModule::basket_trade_budget_tao(nav_before);
        let slice = 9 * TAO;
        let mut spent = 0u64;
        let mut refused = None;
        for _ in 0..2_000 {
            match SubtensorModule::do_swap_basket(coldkey, hotkey, NetUid::ROOT, thin, slice) {
                Ok(_) => spent += slice,
                Err(err) => {
                    refused = Some(err);
                    break;
                }
            }
            sell_back_to_ema(thin);
        }

        assert_eq!(
            refused,
            Some(Error::<Test>::BasketLiquidityCapExceeded.into()),
            "drain must be stopped by the liquidity cap"
        );
        // Far short of the turnover budget the drain used to exhaust.
        assert!(spent < budget / 5, "spent {spent} of budget {budget}");

        // The fund holds at most the cap's share of the pool's alpha reserve.
        let held = escrow_alpha(&hotkey, thin);
        let reserve = SubnetAlphaIn::<Test>::get(thin).to_u64();
        assert!(
            u128::from(held) * u128::from(u16::MAX)
                <= u128::from(BasketLiquidityCap::<Test>::get()) * u128::from(reserve),
            "held {held} exceeds cap share of reserve {reserve}"
        );

        // Loss is bounded by roughly `R × L² / (1 + L)` ≈ 1% of the pool's TAO reserve
        // (plus fees), not by the turnover budget.
        let loss = nav_before.saturating_sub(nav(&hotkey));
        assert!(
            loss < 20 * TAO,
            "loss {loss} rao should be a sliver of the 1,000 τ pool"
        );
    });
}

/// The fund's holding on `netuid` as a share of the pool's alpha reserve, in basis points.
fn held_share_bps(hotkey: &U256, netuid: NetUid) -> u64 {
    let held = escrow_alpha(hotkey, netuid);
    let reserve = SubnetAlphaIn::<Test>::get(netuid).to_u64();
    (u128::from(held) * 10_000 / u128::from(reserve.max(1))) as u64
}

/// Build a position the way a manager must on a real pool: slices of ~0.9% of the TAO
/// reserve (< 2% marginal move), letting the moving price catch up to spot between slices.
/// Returns the slice that was refused and the error.
fn buy_slices_until_refused(
    coldkey: U256,
    hotkey: U256,
    netuid: NetUid,
) -> (u64, sp_runtime::DispatchError) {
    for _ in 0..200 {
        let spot = <Test as crate::Config>::SwapInterface::current_alpha_price(netuid);
        SubnetMovingPrice::<Test>::insert(netuid, I96F32::from_num(spot.to_num::<f64>()));
        let slice = SubnetTAO::<Test>::get(netuid).to_u64() * 9 / 1000;
        if let Err(err) =
            SubtensorModule::do_swap_basket(coldkey, hotkey, NetUid::ROOT, netuid, slice)
        {
            return (slice, err);
        }
    }
    panic!("position never hit the cap");
}

/// Happy path up to the boundary: band-sized slices succeed until the holding sits just
/// under the cap; the slice that would cross it fails and rolls back, leaving the holding
/// and the cash slot unchanged. Selling out is never capped.
#[test]
fn test_liquidity_cap_boundary() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        BasketTradingEnabled::<Test>::put(true);
        BasketDailyTurnoverCap::<Test>::put(u16::MAX); // isolate the liquidity rule
        SubtensorModule::set_tao_weight(u64::MAX);
        // Deep pool at price 1.0: 1,000,000 τ against 1,000,000 α.
        let sn = make_pool(
            &U256::from(4),
            &U256::from(3),
            1_000_000 * TAO,
            1_000_000 * TAO,
        );
        make_fund_with_cash(coldkey, hotkey, 1_000_000 * TAO);

        let (slice, err) = buy_slices_until_refused(coldkey, hotkey, sn);
        assert_eq!(err, Error::<Test>::BasketLiquidityCapExceeded.into());

        // Just under the cap: within one slice (~0.9% of the reserve) of 10%.
        let held = escrow_alpha(&hotkey, sn);
        let share = held_share_bps(&hotkey, sn);
        assert!(held > 0);
        assert!((900..=1_000).contains(&share), "share {share} bps");

        // The refused slice rolled back: nothing moved.
        let cash_before = escrow_alpha(&hotkey, NetUid::ROOT);
        assert_noop!(
            SubtensorModule::do_swap_basket(coldkey, hotkey, NetUid::ROOT, sn, slice),
            Error::<Test>::BasketLiquidityCapExceeded
        );
        assert_eq!(escrow_alpha(&hotkey, sn), held);
        assert_eq!(escrow_alpha(&hotkey, NetUid::ROOT), cash_before);

        // Selling out of the position is not subject to the cap, and root (the cash slot)
        // is never a capped destination. One band-sized slice of alpha.
        let sell = SubnetAlphaIn::<Test>::get(sn).to_u64() * 9 / 1000;
        assert_ok!(SubtensorModule::do_swap_basket(
            coldkey,
            hotkey,
            sn,
            NetUid::ROOT,
            sell
        ));
        assert_eq!(escrow_alpha(&hotkey, sn), held - sell);
    });
}

/// The cap is governance-tunable: raising `BasketLiquidityCap` admits the trade the default
/// refused.
#[test]
fn test_liquidity_cap_follows_storage() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        BasketTradingEnabled::<Test>::put(true);
        BasketDailyTurnoverCap::<Test>::put(u16::MAX);
        SubtensorModule::set_tao_weight(u64::MAX);
        let sn = make_pool(
            &U256::from(4),
            &U256::from(3),
            1_000_000 * TAO,
            1_000_000 * TAO,
        );
        make_fund_with_cash(coldkey, hotkey, 1_000_000 * TAO);

        assert_eq!(
            BasketLiquidityCap::<Test>::get(),
            crate::DEFAULT_BASKET_LIQUIDITY_CAP
        );
        let (slice, err) = buy_slices_until_refused(coldkey, hotkey, sn);
        assert_eq!(err, Error::<Test>::BasketLiquidityCapExceeded.into());
        assert!(held_share_bps(&hotkey, sn) <= 1_000);

        // Governance loosens the cap to 25%: the same slice is now admitted, and the
        // position can keep growing past 10%.
        BasketLiquidityCap::<Test>::put(u16::MAX / 4);
        assert_ok!(SubtensorModule::do_swap_basket(
            coldkey,
            hotkey,
            NetUid::ROOT,
            sn,
            slice
        ));
        let (_, err) = buy_slices_until_refused(coldkey, hotkey, sn);
        assert_eq!(err, Error::<Test>::BasketLiquidityCapExceeded.into());
        let share = held_share_bps(&hotkey, sn);
        assert!((2_350..=2_500).contains(&share), "share {share} bps");
    });
}

// ---------------------------------------------------------------------------------------
// Winners must be allowed to run: a holding that grows past either cap through price
// appreciation is never sold, swept, penalized, or used to block trades elsewhere. Only
// further buys into that subnet are refused.
// ---------------------------------------------------------------------------------------

/// Give the fund `alpha` of `netuid` directly (as if bought earlier at a lower price).
fn hold(hotkey: &U256, netuid: NetUid, alpha: u64) {
    SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
        hotkey,
        &escrow(),
        netuid,
        alpha.into(),
    );
}

/// Appreciate `netuid` by moving the pool along its constant-product curve: TAO reserve
/// × `num`, alpha reserve ÷ `num` (price × `num²`). The moving price is pinned to the new
/// spot so the band is not what decides the outcome. The subnet account is topped up so
/// sells can physically pay out.
fn appreciate(netuid: NetUid, num: u64) {
    let tao = SubnetTAO::<Test>::get(netuid).to_u64() * num;
    let alpha = SubnetAlphaIn::<Test>::get(netuid).to_u64() / num;
    SubnetTAO::<Test>::insert(netuid, TaoBalance::from(tao));
    SubnetAlphaIn::<Test>::insert(netuid, AlphaBalance::from(alpha));
    let subnet_account = SubtensorModule::get_subnet_account_id(netuid).unwrap();
    add_balance_to_coldkey_account(&subnet_account, TaoBalance::from(tao));
    pin_ema_to_spot(netuid);
}

fn pin_ema_to_spot(netuid: NetUid) {
    let spot = <Test as crate::Config>::SwapInterface::current_alpha_price(netuid);
    SubnetMovingPrice::<Test>::insert(netuid, I96F32::from_num(spot.to_num::<f64>()));
}

/// Realizable share of the fund a holding represents, in basis points.
fn nav_share_bps(hotkey: &U256, netuid: NetUid) -> u64 {
    let value = SubtensorModule::realizable_tao_for_alpha(netuid, escrow_alpha(hotkey, netuid));
    (u128::from(value) * 10_000 / u128::from(nav(hotkey).max(1))) as u64
}

/// Fund with cash plus equal positions in two deep subnets; the concentration cap is set
/// to 1/2 so it binds with root + two subnets (mainnet's 1/16 needs sixteen).
fn winner_env(coldkey: U256, hotkey: U256) -> (NetUid, NetUid) {
    BasketTradingEnabled::<Test>::put(true);
    BasketDailyTurnoverCap::<Test>::put(u16::MAX);
    RootWeightsCap::<Test>::insert(NetUid::ROOT, u16::MAX / 2 + 1);
    SubtensorModule::set_tao_weight(u64::MAX);
    let a = make_pool(
        &U256::from(11),
        &U256::from(10),
        1_000_000 * TAO,
        1_000_000 * TAO,
    );
    // B is the validator's own subnet (registered there), so root dividends paid out of B
    // reach this hotkey in the dividend tests.
    let b = make_pool(&hotkey, &coldkey, 1_000_000 * TAO, 1_000_000 * TAO);
    mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
        &hotkey,
        &coldkey,
        b,
        (10_000 * TAO).into(),
    );
    make_fund_with_cash(coldkey, hotkey, 100 * TAO);
    hold(&hotkey, a, 1_000 * TAO);
    hold(&hotkey, b, 1_000 * TAO);
    (a, b)
}

/// A holding that appreciated past the concentration cap: only buys into it are refused.
/// Partial sells (to cash or to another subnet) and every trade between other holdings
/// proceed, and the winner is left exactly where the manager put it.
#[test]
fn test_over_cap_winner_only_blocks_further_buys() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let (a, b) = winner_env(coldkey, hotkey);
        assert!(nav_share_bps(&hotkey, a) < 5_000);

        // A quadruples in price: ~78% of NAV, far over the 50% cap.
        appreciate(a, 2);
        let share = nav_share_bps(&hotkey, a);
        assert!(share > 7_000, "share {share} bps");
        let winner = escrow_alpha(&hotkey, a);

        // Refused: topping up the winner.
        assert_noop!(
            SubtensorModule::do_swap_basket(coldkey, hotkey, NetUid::ROOT, a, 10 * TAO),
            Error::<Test>::RootWeightCapExceeded
        );
        assert_noop!(
            SubtensorModule::do_swap_basket(coldkey, hotkey, b, a, 10 * TAO),
            Error::<Test>::RootWeightCapExceeded
        );
        assert_eq!(escrow_alpha(&hotkey, a), winner);

        // Allowed: take profit into another subnet, and into cash.
        assert_ok!(SubtensorModule::do_swap_basket(
            coldkey,
            hotkey,
            a,
            b,
            100 * TAO
        ));
        assert_ok!(SubtensorModule::do_swap_basket(
            coldkey,
            hotkey,
            a,
            NetUid::ROOT,
            100 * TAO
        ));
        assert_eq!(escrow_alpha(&hotkey, a), winner - 200 * TAO);
        assert!(nav_share_bps(&hotkey, a) > 5_000, "still over cap");

        // Allowed: trades that do not touch the winner, in both directions.
        pin_ema_to_spot(b);
        assert_ok!(SubtensorModule::do_swap_basket(
            coldkey,
            hotkey,
            NetUid::ROOT,
            b,
            50 * TAO
        ));
        pin_ema_to_spot(b);
        assert_ok!(SubtensorModule::do_swap_basket(
            coldkey,
            hotkey,
            b,
            NetUid::ROOT,
            10 * TAO
        ));
        assert_eq!(escrow_alpha(&hotkey, a), winner - 200 * TAO);
    });
}

/// The non-trade paths never consult a cap: curated dividend deployment keeps buying the
/// over-cap winner per the weight vector, dust consolidation leaves it alone (curated or
/// not), a claim redeems it strictly pro-rata with every other holding, and a hotkey swap
/// moves it by value.
#[test]
fn test_over_cap_winner_untouched_by_dividends_dust_claims_and_hotkey_swap() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let alice = U256::from(3);
        let (a, b) = winner_env(coldkey, hotkey);
        set_root_weights_direct(&hotkey, 0, &[(a, u16::MAX / 2), (b, u16::MAX / 2)]);
        zero_claim_threshold();
        // A real staker so dividends have a claimant base.
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &alice,
            NetUid::ROOT,
            (2 * TAO).into(),
        );
        appreciate(a, 2);
        assert!(nav_share_bps(&hotkey, a) > 7_000);
        let winner = escrow_alpha(&hotkey, a);

        // Dividends (origin B) are sold and redeployed per weights: half lands in A even
        // though A is over the cap.
        SubtensorModule::distribute_emission(
            b,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            (10 * TAO).into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();
        let after_dividend = escrow_alpha(&hotkey, a);
        assert!(
            after_dividend > winner,
            "dividends must keep flowing into the winner"
        );

        // Dust consolidation: with A orphaned from the weight vector and a live threshold,
        // an above-threshold holding is not swept.
        set_root_weights_direct(&hotkey, 0, &[(b, u16::MAX)]);
        RootClaimableThreshold::<Test>::insert(NetUid::ROOT, I96F32::from_num(TAO));
        assert_eq!(
            SubtensorModule::consolidate_dust_basket_holdings(&hotkey),
            0
        );
        assert_eq!(escrow_alpha(&hotkey, a), after_dividend);
        zero_claim_threshold();
        set_root_weights_direct(&hotkey, 0, &[(a, u16::MAX / 2), (b, u16::MAX / 2)]);

        // A claim redeems pro-rata: A and B shrink by the same fraction (±1%).
        let a_before = escrow_alpha(&hotkey, a);
        let b_before = escrow_alpha(&hotkey, b);
        assert!(SubtensorModule::get_basket_owed_shares(&hotkey, &alice) > 0);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(alice),
            hotkey
        ));
        let a_frac = (a_before - escrow_alpha(&hotkey, a)) * 10_000 / a_before;
        let b_frac = (b_before - escrow_alpha(&hotkey, b)) * 10_000 / b_before;
        assert!(a_frac > 0, "claim must take from the winner too");
        assert!(
            a_frac.abs_diff(b_frac) <= 100,
            "pro-rata: A took {a_frac} bps, B took {b_frac} bps"
        );

        // Hotkey swap moves the holding by value, no cap consulted.
        let new_hotkey = U256::from(4);
        let a_now = escrow_alpha(&hotkey, a);
        SubtensorModule::transfer_basket_for_new_hotkey(&hotkey, &new_hotkey);
        assert_eq!(escrow_alpha(&new_hotkey, a), a_now);
        assert_eq!(escrow_alpha(&hotkey, a), 0);
    });
}

/// Same for the liquidity cap: after a run-up shrinks the pool's alpha reserve, a fund that
/// now holds more than the cap's share of it can still sell, trade elsewhere, and receive
/// dividends; only buys into that subnet are refused.
#[test]
fn test_over_liquidity_cap_winner_only_blocks_further_buys() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let alice = U256::from(3);
        BasketTradingEnabled::<Test>::put(true);
        BasketDailyTurnoverCap::<Test>::put(u16::MAX);
        SubtensorModule::set_tao_weight(u64::MAX);
        zero_claim_threshold();
        // Thin pool: 10,000 τ against 1,000,000 α. The fund holds 9% of the reserve.
        let thin = make_pool(
            &U256::from(11),
            &U256::from(10),
            10_000 * TAO,
            1_000_000 * TAO,
        );
        let deep = make_pool(&hotkey, &coldkey, 1_000_000 * TAO, 1_000_000 * TAO);
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            deep,
            (10_000 * TAO).into(),
        );
        make_fund_with_cash(coldkey, hotkey, 10_000 * TAO);
        hold(&hotkey, thin, 90_000 * TAO);
        hold(&hotkey, deep, 1_000 * TAO);
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &alice,
            NetUid::ROOT,
            (2 * TAO).into(),
        );
        assert!(held_share_bps(&hotkey, thin) < 1_000);

        // Run-up: alpha leaves the pool, the fund's 90k α is now 22.5% of the reserve.
        appreciate(thin, 2);
        // (`appreciate` halves the reserve: 500k α; buy-side pressure in the mock only.)
        SubnetAlphaIn::<Test>::insert(thin, AlphaBalance::from(400_000 * TAO));
        pin_ema_to_spot(thin);
        let share = held_share_bps(&hotkey, thin);
        assert!(share > 2_000, "share {share} bps");
        let winner = escrow_alpha(&hotkey, thin);

        // Refused: any further buy, even a tiny one.
        assert_noop!(
            SubtensorModule::do_swap_basket(coldkey, hotkey, NetUid::ROOT, thin, TAO),
            Error::<Test>::BasketLiquidityCapExceeded
        );
        assert_noop!(
            SubtensorModule::do_swap_basket(coldkey, hotkey, deep, thin, TAO),
            Error::<Test>::BasketLiquidityCapExceeded
        );

        // Allowed: take profit (one band-sized slice) and trade the other holdings.
        let slice = SubnetAlphaIn::<Test>::get(thin).to_u64() * 9 / 1000;
        assert_ok!(SubtensorModule::do_swap_basket(
            coldkey,
            hotkey,
            thin,
            NetUid::ROOT,
            slice
        ));
        assert_eq!(escrow_alpha(&hotkey, thin), winner - slice);
        assert!(held_share_bps(&hotkey, thin) > 1_000, "still over the cap");
        assert_ok!(SubtensorModule::do_swap_basket(
            coldkey,
            hotkey,
            NetUid::ROOT,
            deep,
            100 * TAO
        ));

        // Allowed: dividends deployed into it by the weight vector.
        set_root_weights_direct(&hotkey, 0, &[(thin, u16::MAX)]);
        let before = escrow_alpha(&hotkey, thin);
        SubtensorModule::distribute_emission(
            deep,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            (10 * TAO).into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();
        assert!(escrow_alpha(&hotkey, thin) > before);
    });
}

/// Profit-taking after a sharp run-up is not blocked by the band. The sell floor is
/// `0.98 × max(EMA, spot)`, so with spot far above a stale EMA the first slice fills at
/// 0.98 × spot, and chained slices may walk the price down to 0.98 × EMA. Only a sale that
/// would push spot below that (a drawdown relative to the EMA) waits for the EMA.
#[test]
fn test_profit_taking_after_run_up_is_not_blocked_by_band() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        BasketTradingEnabled::<Test>::put(true);
        BasketDailyTurnoverCap::<Test>::put(u16::MAX);
        SubtensorModule::set_tao_weight(u64::MAX);
        let a = make_pool(
            &U256::from(11),
            &U256::from(10),
            1_000_000 * TAO,
            1_000_000 * TAO,
        );
        make_fund_with_cash(coldkey, hotkey, 100 * TAO);
        hold(&hotkey, a, 1_000_000 * TAO);
        // Run-up ×4 with the EMA left where it was (price 1.0): spot is 4× EMA.
        appreciate(a, 2);
        SubnetMovingPrice::<Test>::insert(a, I96F32::from_num(1.0));
        let p0 = <Test as crate::Config>::SwapInterface::current_alpha_price(a).to_num::<f64>();
        assert!(p0 > 3.9);

        // Buying the runaway subnet is refused by the band (spot > 1.02 × EMA).
        assert_noop!(
            SubtensorModule::do_swap_basket(coldkey, hotkey, NetUid::ROOT, a, TAO),
            Error::<Test>::SlippageTooHigh
        );

        // Selling is not: band-sized slices fill until spot reaches 0.98 × EMA.
        let mut legs = 0;
        let mut sold = 0u64;
        loop {
            let slice = SubnetAlphaIn::<Test>::get(a).to_u64() * 9 / 1000;
            match SubtensorModule::do_swap_basket(coldkey, hotkey, a, NetUid::ROOT, slice) {
                Ok(_) => {
                    legs += 1;
                    sold += slice;
                }
                Err(err) => {
                    assert_eq!(err, Error::<Test>::SlippageTooHigh.into());
                    break;
                }
            }
            assert!(legs < 200);
        }
        let p1 = <Test as crate::Config>::SwapInterface::current_alpha_price(a).to_num::<f64>();
        assert!(legs >= 30, "only {legs} legs");
        assert!(sold > 200_000 * TAO, "sold {sold}");
        // Stopped at the EMA floor, not at the run-up price.
        assert!((0.97..1.02).contains(&p1), "price {p1}");
        assert!(escrow_alpha(&hotkey, NetUid::ROOT) > 100 * TAO);
    });
}

// ---------------------------------------------------------------------------------------
// Weight: the pending-deposit flush a trade performs is charged, not free.
// ---------------------------------------------------------------------------------------

fn declared_swap_weight(
    coldkey: U256,
    hotkey: U256,
    from: NetUid,
    to: NetUid,
    amount: u64,
) -> Weight {
    let _ = coldkey;
    RuntimeCall::SubtensorModule(crate::Call::swap_basket {
        hotkey,
        origin_netuid: from,
        destination_netuid: to,
        amount: amount.into(),
    })
    .get_dispatch_info()
    .call_weight
}

/// The declared weight grows with the number of queued dividend credits, the actual weight
/// includes the flush work actually done, and it refunds below the declared cap. With an
/// empty queue the trade costs exactly its holding-count weight.
#[test]
fn test_swap_basket_weight_charges_pending_deposit_flush() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let alice = U256::from(3);
        let (a, b) = winner_env(coldkey, hotkey);
        set_root_weights_direct(&hotkey, 0, &[(a, u16::MAX / 2), (b, u16::MAX / 2)]);
        zero_claim_threshold();
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &alice,
            NetUid::ROOT,
            (2 * TAO).into(),
        );
        pin_ema_to_spot(a);
        pin_ema_to_spot(b);

        // Empty queue: declared is the bare 256-row cap (plus whatever fixed extension
        // weight the runtime adds to every call), actual is the bare row weight.
        let bare_declared = declared_swap_weight(coldkey, hotkey, a, b, TAO);
        assert!(bare_declared.all_gte(SubtensorModule::swap_basket_weight(256)));
        let bare_actual = SubtensorModule::do_swap_basket(coldkey, hotkey, a, b, TAO).unwrap();
        assert_eq!(bare_actual, SubtensorModule::swap_basket_weight(3));

        // One queued origin raises the declared weight by the flush estimate; a second
        // origin raises it by one more unit.
        SubtensorModule::enqueue_basket_deposit(&hotkey, b, (5 * TAO).into());
        let declared_one = declared_swap_weight(coldkey, hotkey, a, b, TAO);
        assert_eq!(
            declared_one.saturating_sub(bare_declared),
            SubtensorModule::basket_flush_weight(1 + 4 * 256)
        );
        SubtensorModule::enqueue_basket_deposit(&hotkey, a, (5 * TAO).into());
        let declared_two = declared_swap_weight(coldkey, hotkey, a, b, TAO);
        assert_eq!(
            declared_two.saturating_sub(declared_one),
            SubtensorModule::basket_flush_weight(1)
        );

        // Learn the flush work this exact queue implies, then restore the queue: the trade
        // must charge precisely that on top of its row weight, and refund below declared.
        let (flush_work, _, _) = SubtensorModule::flush_basket_deposits_for_hotkey(&hotkey);
        assert!(flush_work > 0);
        SubtensorModule::enqueue_basket_deposit(&hotkey, b, (5 * TAO).into());
        SubtensorModule::enqueue_basket_deposit(&hotkey, a, (5 * TAO).into());
        pin_ema_to_spot(a);
        pin_ema_to_spot(b);
        let actual = SubtensorModule::do_swap_basket(coldkey, hotkey, a, b, TAO).unwrap();
        assert!(
            PendingBasketDeposits::<Test>::iter_prefix(hotkey)
                .next()
                .is_none()
        );
        assert_eq!(
            actual,
            SubtensorModule::swap_basket_weight(3)
                .saturating_add(SubtensorModule::basket_flush_weight(flush_work))
        );
        assert!(actual.all_gt(bare_actual), "flush work must be charged");
        assert!(
            actual.all_lt(declared_two),
            "actual must refund below declared"
        );
    });
}

// ---------------------------------------------------------------------------------------
// Turnover bucket: clamping and the hotkey-swap carry.
// ---------------------------------------------------------------------------------------

/// The bucket level is clamped to one *current* budget, so a NAV drop cannot leave a fund
/// with more spendable turnover than its shrunken budget; a hotkey swap onto a hotkey that
/// already has a bucket keeps the lower level and the later refill block.
#[test]
fn test_turnover_bucket_clamps_to_current_budget_and_carries_conservatively() {
    new_test_ext(1).execute_with(|| {
        let hotkey = U256::from(2);
        let other = U256::from(4);
        let now = 1_000u64;

        // Never traded: full, whatever the budget.
        assert_eq!(
            SubtensorModule::basket_trade_bucket_at(&hotkey, now, 500),
            500
        );

        // Stored level above a (shrunken) budget clamps; refill accrues pro rata and clamps.
        BasketTradeBucket::<Test>::insert(hotkey, (800u64, now));
        assert_eq!(
            SubtensorModule::basket_trade_bucket_at(&hotkey, now, 500),
            500
        );
        BasketTradeBucket::<Test>::insert(hotkey, (100u64, now));
        let quarter = BASKET_TRADE_REFILL_BLOCKS / 4;
        assert_eq!(
            SubtensorModule::basket_trade_bucket_at(&hotkey, now + quarter, 1_000),
            100 + 250
        );
        assert_eq!(
            SubtensorModule::basket_trade_bucket_at(
                &hotkey,
                now + BASKET_TRADE_REFILL_BLOCKS,
                1_000
            ),
            1_000
        );
        assert_eq!(
            SubtensorModule::basket_trade_bucket_at(
                &hotkey,
                now + 10 * BASKET_TRADE_REFILL_BLOCKS,
                1_000
            ),
            1_000
        );

        // Carry on hotkey swap: min level, max refill block; the old row is removed.
        BasketTradeBucket::<Test>::insert(hotkey, (100u64, now + 50));
        BasketTradeBucket::<Test>::insert(other, (40u64, now + 10));
        SubtensorModule::transfer_basket_for_new_hotkey(&hotkey, &other);
        assert_eq!(BasketTradeBucket::<Test>::get(other), Some((40, now + 50)));
        assert_eq!(BasketTradeBucket::<Test>::get(hotkey), None);

        // A full (unstored) source leaves the destination's bucket as it is.
        SubtensorModule::transfer_basket_for_new_hotkey(&hotkey, &other);
        assert_eq!(BasketTradeBucket::<Test>::get(other), Some((40, now + 50)));
    });
}
