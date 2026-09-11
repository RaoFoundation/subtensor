//! Beta basket: validator-directed rebalancing (`swap_basket_alpha`).
//!
//! Trades change only the fund's composition. Shares, the claimable rate, and every
//! staker's watermark are untouched, TotalStake is conserved, reserves stay in lockstep,
//! and both legs are booked as protocol flow.

use crate::tests::claim_root::{
    escrow_alpha, flush_baskets, fund_pool, fund_shares, register_on_root, root_stake_of,
    swap_all_basket_alpha, zero_claim_threshold,
};
use crate::tests::mock::*;
use crate::{
    BasketClaimed, BasketRate, Error, Event, SubnetAlphaIn, SubnetAlphaOut, SubnetProtocolFlow,
    SubnetTAO, SubnetTaoFlow, TotalStake,
};
use approx::assert_abs_diff_eq;
use frame_support::{assert_noop, assert_ok};
use sp_core::U256;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance};

/// Tight economic bound (1%): a fee-free trade through deep pools moves NAV only by
/// slippage and rounding.
const SLIPPAGE_EPS_DENOM: u64 = 100;

/// A root validator whose fund holds a dividend on subnet A, plus a second deep pool B to
/// trade into. Returns `(hotkey, staker coldkey, netuid_a, netuid_b)`.
fn setup_fund_with_holding() -> (U256, U256, NetUid, NetUid) {
    let owner_a = U256::from(1001);
    let hotkey = U256::from(1002);
    let coldkey = U256::from(1003);
    let owner_b = U256::from(2001);
    let hotkey_b = U256::from(2002);

    let netuid_a = add_dynamic_network(&hotkey, &owner_a);
    let netuid_b = add_dynamic_network(&hotkey_b, &owner_b);
    remove_owner_registration_stake(netuid_a);
    fund_pool(netuid_a);
    fund_pool(netuid_b);

    SubtensorModule::set_tao_weight(u64::MAX);
    zero_claim_threshold();

    mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
        &hotkey,
        &coldkey,
        NetUid::ROOT,
        2_000_000u64.into(),
    );
    mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
        &hotkey,
        &owner_a,
        netuid_a,
        10_000_000u64.into(),
    );
    register_on_root(&hotkey, 0);

    SubtensorModule::distribute_emission(
        netuid_a,
        AlphaBalance::ZERO,
        AlphaBalance::ZERO,
        1_000_000u64.into(),
        AlphaBalance::ZERO,
    );
    flush_baskets();
    assert!(escrow_alpha(&hotkey, netuid_a) > 0);
    assert!(fund_shares(&hotkey) > 0);

    (hotkey, coldkey, netuid_a, netuid_b)
}

fn nav(hotkey: &U256) -> u64 {
    SubtensorModule::get_validator_basket_nav_tao(hotkey).to_u64()
}

#[test]
fn test_swap_basket_alpha_rejections() {
    new_test_ext(1).execute_with(|| {
        let (hotkey, _coldkey, netuid_a, netuid_b) = setup_fund_with_holding();
        let held = escrow_alpha(&hotkey, netuid_a);

        // Not a root validator.
        let stranger = U256::from(777);
        assert_noop!(
            SubtensorModule::swap_basket_alpha(
                RuntimeOrigin::signed(stranger),
                netuid_a,
                netuid_b,
                1_000u64.into(),
            ),
            Error::<Test>::HotKeyNotRegisteredInSubNet
        );

        // Origin and destination must differ.
        assert_noop!(
            SubtensorModule::swap_basket_alpha(
                RuntimeOrigin::signed(hotkey),
                netuid_a,
                netuid_a,
                1_000u64.into(),
            ),
            Error::<Test>::SameNetuid
        );

        // Both sides must be root or an existing subnet.
        assert_noop!(
            SubtensorModule::swap_basket_alpha(
                RuntimeOrigin::signed(hotkey),
                netuid_a,
                NetUid::from(99u16),
                1_000u64.into(),
            ),
            Error::<Test>::SubnetNotExists
        );
        assert_noop!(
            SubtensorModule::swap_basket_alpha(
                RuntimeOrigin::signed(hotkey),
                NetUid::from(99u16),
                netuid_a,
                1_000u64.into(),
            ),
            Error::<Test>::SubnetNotExists
        );

        // Zero amount.
        assert_noop!(
            SubtensorModule::swap_basket_alpha(
                RuntimeOrigin::signed(hotkey),
                netuid_a,
                netuid_b,
                AlphaBalance::ZERO,
            ),
            Error::<Test>::AmountTooLow
        );

        // More than the fund holds on the origin (including an empty origin).
        assert_noop!(
            SubtensorModule::swap_basket_alpha(
                RuntimeOrigin::signed(hotkey),
                netuid_a,
                netuid_b,
                (held + 1).into(),
            ),
            Error::<Test>::NotEnoughStakeToWithdraw
        );
        assert_noop!(
            SubtensorModule::swap_basket_alpha(
                RuntimeOrigin::signed(hotkey),
                NetUid::ROOT,
                netuid_b,
                1u64.into(),
            ),
            Error::<Test>::NotEnoughStakeToWithdraw
        );

        // Nothing moved.
        assert_eq!(escrow_alpha(&hotkey, netuid_a), held);
        assert_eq!(escrow_alpha(&hotkey, netuid_b), 0);
    });
}

/// Subnet -> subnet: the holding moves, entitlements are untouched, TotalStake is conserved,
/// NAV moves only by slippage, and the event reports both legs.
#[test]
fn test_swap_basket_alpha_subnet_to_subnet_is_composition_only() {
    new_test_ext(1).execute_with(|| {
        let (hotkey, coldkey, netuid_a, netuid_b) = setup_fund_with_holding();

        let held = escrow_alpha(&hotkey, netuid_a);
        let shares_before = fund_shares(&hotkey);
        let rate_before = BasketRate::<Test>::get(hotkey);
        let claimed_before = BasketClaimed::<Test>::get(hotkey, coldkey);
        let owed_before = SubtensorModule::get_basket_owed_shares(&hotkey, &coldkey);
        let nav_before = nav(&hotkey);
        let ts_before = TotalStake::<Test>::get();

        // Trade half, then the rest.
        let half = held / 2;
        assert_ok!(SubtensorModule::swap_basket_alpha(
            RuntimeOrigin::signed(hotkey),
            netuid_a,
            netuid_b,
            half.into(),
        ));
        assert_eq!(escrow_alpha(&hotkey, netuid_a), held - half);
        let bought_first = escrow_alpha(&hotkey, netuid_b);
        assert!(bought_first > 0);

        swap_all_basket_alpha(&hotkey, netuid_a, netuid_b);
        assert_eq!(escrow_alpha(&hotkey, netuid_a), 0);
        assert!(escrow_alpha(&hotkey, netuid_b) > bought_first);

        // Composition-only: nothing about entitlements changed.
        assert_eq!(fund_shares(&hotkey), shares_before);
        assert_eq!(BasketRate::<Test>::get(hotkey), rate_before);
        assert_eq!(BasketClaimed::<Test>::get(hotkey, coldkey), claimed_before);
        assert_eq!(
            SubtensorModule::get_basket_owed_shares(&hotkey, &coldkey),
            owed_before
        );

        // TAO-neutral and NAV-continuous minus slippage.
        assert_eq!(TotalStake::<Test>::get(), ts_before);
        let nav_after = nav(&hotkey);
        assert!(nav_after <= nav_before, "a trade cannot create value");
        assert_abs_diff_eq!(
            nav_after,
            nav_before,
            epsilon = nav_before / SLIPPAGE_EPS_DENOM
        );

        // The event names both legs.
        assert!(System::events().iter().any(|e| {
            matches!(
                &e.event,
                RuntimeEvent::SubtensorModule(Event::BasketAlphaSwapped {
                    hotkey: h,
                    origin_netuid,
                    destination_netuid,
                    alpha_in,
                    tao,
                    alpha_out,
                }) if *h == hotkey
                    && *origin_netuid == netuid_a
                    && *destination_netuid == netuid_b
                    && alpha_in.to_u64() == half
                    && tao.to_u64() > 0
                    && alpha_out.to_u64() == bought_first
            )
        }));

        // The staker still redeems ~the same value after the rebalance.
        let root_before = root_stake_of(&hotkey, &coldkey);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(coldkey),
            hotkey
        ));
        let gain = root_stake_of(&hotkey, &coldkey).saturating_sub(root_before);
        assert_abs_diff_eq!(gain, nav_before, epsilon = nav_before / SLIPPAGE_EPS_DENOM);
    });
}

/// Subnet -> root -> subnet: the root (cash) slot is credited and debited in lockstep with
/// the root reserves, and the round trip is TotalStake-neutral.
#[test]
fn test_swap_basket_alpha_root_slot_round_trip_keeps_reserves_in_lockstep() {
    new_test_ext(1).execute_with(|| {
        let (hotkey, _coldkey, netuid_a, netuid_b) = setup_fund_with_holding();

        let ts_start = TotalStake::<Test>::get();
        let root_tao_start = SubnetTAO::<Test>::get(NetUid::ROOT);
        let root_alpha_out_start = SubnetAlphaOut::<Test>::get(NetUid::ROOT);
        let nav_start = nav(&hotkey);

        // Sell the whole holding into cash.
        swap_all_basket_alpha(&hotkey, netuid_a, NetUid::ROOT);
        let cash = escrow_alpha(&hotkey, NetUid::ROOT);
        assert!(cash > 0);
        assert_eq!(escrow_alpha(&hotkey, netuid_a), 0);
        // Cash is TAO 1:1, so NAV is exactly the root slot.
        assert_eq!(nav(&hotkey), cash);
        assert_abs_diff_eq!(cash, nav_start, epsilon = nav_start / SLIPPAGE_EPS_DENOM);
        // Root reserves credited by exactly the cash parked.
        assert_eq!(
            SubnetTAO::<Test>::get(NetUid::ROOT),
            root_tao_start.saturating_add(cash.into())
        );
        assert_eq!(
            SubnetAlphaOut::<Test>::get(NetUid::ROOT),
            root_alpha_out_start.saturating_add(cash.into())
        );
        assert_eq!(TotalStake::<Test>::get(), ts_start);

        // Spend the cash on subnet B.
        swap_all_basket_alpha(&hotkey, NetUid::ROOT, netuid_b);
        assert_eq!(escrow_alpha(&hotkey, NetUid::ROOT), 0);
        assert!(escrow_alpha(&hotkey, netuid_b) > 0);
        // Root reserves debited back to where they started.
        assert_eq!(SubnetTAO::<Test>::get(NetUid::ROOT), root_tao_start);
        assert_eq!(
            SubnetAlphaOut::<Test>::get(NetUid::ROOT),
            root_alpha_out_start
        );
        assert_eq!(TotalStake::<Test>::get(), ts_start);
        assert_abs_diff_eq!(
            nav(&hotkey),
            nav_start,
            epsilon = nav_start / SLIPPAGE_EPS_DENOM
        );

        // Root -> subnet event reports TAO == alpha_in (cash is 1:1).
        assert!(System::events().iter().any(|e| {
            matches!(
                &e.event,
                RuntimeEvent::SubtensorModule(Event::BasketAlphaSwapped {
                    origin_netuid,
                    destination_netuid,
                    alpha_in,
                    tao,
                    ..
                }) if origin_netuid.is_root()
                    && *destination_netuid == netuid_b
                    && alpha_in.to_u64() == cash
                    && *tao == TaoBalance::from(cash)
            )
        }));
    });
}

/// Both legs are booked as protocol flow (outflow on the origin pool, inflow on the
/// destination pool) and never as user flow, so validator trading is neutral to the
/// TAO-flow emission metric.
#[test]
fn test_swap_basket_alpha_books_protocol_flow_not_user_flow() {
    new_test_ext(1).execute_with(|| {
        let (hotkey, _coldkey, netuid_a, netuid_b) = setup_fund_with_holding();

        assert_eq!(SubnetProtocolFlow::<Test>::get(netuid_a), 0);
        assert_eq!(SubnetProtocolFlow::<Test>::get(netuid_b), 0);
        let user_flow_a = SubnetTaoFlow::<Test>::get(netuid_a);
        let user_flow_b = SubnetTaoFlow::<Test>::get(netuid_b);

        swap_all_basket_alpha(&hotkey, netuid_a, netuid_b);

        let flow_a = SubnetProtocolFlow::<Test>::get(netuid_a);
        let flow_b = SubnetProtocolFlow::<Test>::get(netuid_b);
        assert!(
            flow_a < 0,
            "sell leg must be a protocol outflow, got {flow_a}"
        );
        assert!(
            flow_b > 0,
            "buy leg must be a protocol inflow, got {flow_b}"
        );
        assert_eq!(flow_b, -flow_a, "every TAO sold on A is spent on B");

        assert_eq!(SubnetTaoFlow::<Test>::get(netuid_a), user_flow_a);
        assert_eq!(SubnetTaoFlow::<Test>::get(netuid_b), user_flow_b);
    });
}

/// A trade whose buy leg would round to zero alpha is rejected and rolled back in full,
/// including the sell leg.
#[test]
fn test_swap_basket_alpha_rolls_back_when_buy_rounds_to_zero() {
    new_test_ext(1).execute_with(|| {
        let (hotkey, _coldkey, netuid_a, netuid_b) = setup_fund_with_holding();

        // Make B astronomically expensive so a tiny TAO buy yields zero alpha.
        SubnetTAO::<Test>::insert(netuid_b, TaoBalance::from(1_000_000_000_000_000u64));
        SubnetAlphaIn::<Test>::insert(netuid_b, AlphaBalance::from(1_000u64));

        let held = escrow_alpha(&hotkey, netuid_a);
        let ts_before = TotalStake::<Test>::get();
        let nav_before = nav(&hotkey);

        // The engine may surface its own error for a degenerate buy; whichever it is, the
        // whole trade (sell leg included) must roll back.
        assert!(
            SubtensorModule::swap_basket_alpha(
                RuntimeOrigin::signed(hotkey),
                netuid_a,
                netuid_b,
                1_000u64.into(),
            )
            .is_err()
        );

        assert_eq!(escrow_alpha(&hotkey, netuid_a), held);
        assert_eq!(escrow_alpha(&hotkey, netuid_b), 0);
        assert_eq!(TotalStake::<Test>::get(), ts_before);
        assert_eq!(nav(&hotkey), nav_before);
    });
}
