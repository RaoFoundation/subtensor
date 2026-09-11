//! `swap_basket` liquidity cap ([`BasketLiquidityCap`]): a fund may not hold more than the
//! cap's share of a destination pool's alpha reserve after a buy.
//!
//! Motivation: the concentration cap marks holdings at realizable value, which is bounded by
//! the pool's TAO reserve. On a thin pool a stolen key can buy in 2%-band slices while a
//! counterparty sells alpha back to the EMA between slices; every slice passes the slippage,
//! turnover, and concentration rules, yet the fund's realizable NAV collapses by roughly the
//! turnover it spent. The liquidity cap stops that accumulation.
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used)]

use crate::tests::claim_root::{escrow_alpha, register_on_root};
use crate::tests::mock::*;
use crate::{
    BasketDailyTurnoverCap, BasketLiquidityCap, BasketTradingEnabled, Error, Owner, SubnetAlphaIn,
    SubnetAlphaOut, SubnetMovingPrice, SubnetTAO, TotalStake,
};
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
