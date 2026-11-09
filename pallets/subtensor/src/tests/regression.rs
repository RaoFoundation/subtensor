//! Critical regressions involving significant losses or unavailable functionality.
//! Existing tests are protected by the explicit-human-approval rule in AGENTS.md.

use super::claim_root::{fund_pool, register_on_root};
use super::mock::*;
use crate::{
    BasketRate, BasketShares, BurnIncreaseMult, FirstEmissionBlockNumber, PendingBasketDeposits,
    RootClaimableThreshold, SubnetAlphaOut, SubnetLimit, SubnetMechanism, SubtokenEnabled,
};
use frame_support::assert_ok;
use sp_core::U256;
use sp_std::collections::btree_set::BTreeSet;
use substrate_fixed::types::{I96F32, U64F64};
use subtensor_runtime_common::{AlphaBalance, NetUid, Token};

// PROTECTED_REGRESSION: a tiny volatile holding must not strand earned root dividends; changes require explicit human approval.
#[test]
fn small_out_of_band_holding_allows_dividend_settlement_and_actual_claim() {
    new_test_ext(1).execute_with(|| {
        let hotkey = U256::from(4_301);
        let coldkey = U256::from(4_302);
        let owner = U256::from(4_303);
        let held = add_dynamic_network(&U256::from(4_304), &owner);
        let origin = add_dynamic_network(&U256::from(4_305), &owner);
        for netuid in [held, origin] {
            remove_owner_registration_stake(netuid);
            fund_pool(netuid);
        }
        SubtensorModule::set_tao_weight(u64::MAX);
        super::claim_root::zero_claim_threshold();
        fund_with_claimable_dividends(hotkey, coldkey, 0, &[held], &[]);
        // This position is 0.1% of the cash-backed fund. A 10% spot move
        // breaches the old per-pool gate but shifts the fund by only ~0.01%.
        crate::SubnetTAO::<Test>::mutate(held, |tao| {
            *tao = tao.saturating_add(tao.saturating_div(10u64.into()));
        });
        let credit = 1_000_000_000u64;
        SubnetAlphaOut::<Test>::mutate(origin, |alpha| {
            *alpha = alpha.saturating_add(credit.into());
        });
        SubtensorModule::enqueue_basket_deposit(&hotkey, origin, credit.into());
        let (_, _, completed) = SubtensorModule::flush_basket_deposits_for_hotkey(&hotkey);
        assert!(completed);
        assert!(!PendingBasketDeposits::<Test>::contains_key(hotkey, origin));
        assert!(BasketRate::<Test>::get(hotkey) > I96F32::from_num(1));
        assert_ok!(SubtensorModule::claim_root(
            RuntimeOrigin::signed(coldkey),
            BTreeSet::new(),
        ));
        let paid_root = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            NetUid::ROOT,
        )
        .to_u64();
        // Existing cash alone is insufficient: this asserts the earned dividend
        // actually became claimable root stake, not just an empty successful call.
        assert!(paid_root > ROOT_STAKE + FUND_CASH + credit / 2);
    });
}

const ROOT_STAKE: u64 = 1_000_000_000;
const FUND_CASH: u64 = 1_000_000_000;
const PENDING_DUST: u64 = 1_000;

fn priced_subnets() -> Vec<NetUid> {
    // Match the reported production threshold instead of disabling claim checks.
    RootClaimableThreshold::<Test>::insert(NetUid::ROOT, I96F32::from_num(500_000));
    SubtensorModule::set_tao_weight(u64::MAX);
    (1..=SubnetLimit::<Test>::get())
        .map(|index| {
            // Install live, priced subnets directly: repeated registration's
            // escalating lock cost is unrelated to claim admission.
            let netuid = NetUid::from(index);
            SubtensorModule::init_new_network(netuid, 360);
            SubtokenEnabled::<Test>::insert(netuid, true);
            FirstEmissionBlockNumber::<Test>::insert(netuid, 0);
            SubnetMechanism::<Test>::insert(netuid, 1);
            BurnIncreaseMult::<Test>::insert(netuid, U64F64::from_num(1));
            fund_pool(netuid);
            netuid
        })
        .collect()
}

fn fund_with_claimable_dividends(
    hotkey: U256,
    coldkey: U256,
    uid: u16,
    holdings: &[NetUid],
    pending: &[NetUid],
) {
    register_on_root(&hotkey, uid);
    mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
        &hotkey,
        &coldkey,
        NetUid::ROOT,
        ROOT_STAKE.into(),
    );
    let escrow = SubtensorModule::get_beta_escrow_account_id();
    for netuid in holdings {
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &escrow,
            *netuid,
            1_000_000u64.into(),
        );
    }
    mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
        &hotkey,
        &escrow,
        NetUid::ROOT,
        FUND_CASH.into(),
    );
    // The sole root staker owns all existing fund shares. Claims must redeem
    // actual funded holdings, not merely pass admission with no entitlement.
    BasketShares::<Test>::insert(hotkey, ROOT_STAKE);
    BasketRate::<Test>::insert(hotkey, I96F32::from_num(1));
    assert_eq!(
        SubtensorModule::get_basket_owed_shares(&hotkey, &coldkey),
        ROOT_STAKE
    );
    for netuid in pending {
        SubnetAlphaOut::<Test>::mutate(netuid, |alpha| {
            *alpha = alpha.saturating_add(PENDING_DUST.into());
        });
        SubtensorModule::enqueue_basket_deposit(&hotkey, *netuid, PENDING_DUST.into());
    }
}

fn assert_dust_queue_unchanged(hotkey: U256, subnets: &[NetUid]) {
    assert_eq!(
        PendingBasketDeposits::<Test>::iter_prefix(hotkey).count(),
        subnets.len()
    );
    for netuid in subnets {
        assert_eq!(
            PendingBasketDeposits::<Test>::get(hotkey, netuid),
            AlphaBalance::from(PENDING_DUST)
        );
    }
}

/// F-0033: a full 129-position fund must remain redeemable while all 128 subnet
/// credits are below threshold, including after a drain that leaves them queued.
// PROTECTED_REGRESSION: F-0033 full-basket claims must pay while dividend dust remains queued; changes require explicit human approval.
#[test]
fn f0033_full_root_basket_pays_dividends_with_a_full_dust_queue() {
    new_test_ext(1).execute_with(|| {
        let subnets = priced_subnets();
        assert_eq!(subnets.len(), 128);
        let (hotkey, coldkey) = (U256::from(4_201), U256::from(4_202));
        fund_with_claimable_dividends(hotkey, coldkey, 0, &subnets, &subnets);
        assert_eq!(SubtensorModule::get_basket_holdings(&hotkey).len(), 129);

        let (_, _, completed) = SubtensorModule::flush_basket_deposits_for_hotkey(&hotkey);
        assert!(completed);
        assert_dust_queue_unchanged(hotkey, &subnets);

        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(coldkey),
            hotkey
        ));
        let stake = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            NetUid::ROOT,
        )
        .to_u64();
        assert!(stake >= ROOT_STAKE.saturating_add(FUND_CASH));
        assert_eq!(BasketShares::<Test>::get(hotkey), 0);
        assert_dust_queue_unchanged(hotkey, &subnets);
    });
}

/// F-0033: two 127-position funds exactly fill the coldkey's 256-unit holdings
/// budget. Their persistent dividend queues must use the separate pending axis.
// PROTECTED_REGRESSION: F-0033 coldkey-wide claims must pay both funds with pending dust; changes require explicit human approval.
#[test]
fn f0033_coldkey_wide_claim_pays_both_large_funds_with_pending_dust() {
    new_test_ext(1).execute_with(|| {
        let subnets = priced_subnets();
        let coldkey = U256::from(4_212);
        let hotkeys = [U256::from(4_211), U256::from(4_213)];
        let holdings: Vec<_> = subnets.iter().copied().take(126).collect();
        for (uid, hotkey) in [0_u16, 1_u16].into_iter().zip(hotkeys) {
            fund_with_claimable_dividends(hotkey, coldkey, uid, &holdings, &subnets);
            assert_eq!(SubtensorModule::get_basket_holdings(&hotkey).len(), 127);
        }

        assert_ok!(SubtensorModule::claim_root(
            RuntimeOrigin::signed(coldkey),
            BTreeSet::new()
        ));
        for hotkey in hotkeys {
            let stake = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &coldkey,
                NetUid::ROOT,
            )
            .to_u64();
            assert!(stake >= ROOT_STAKE.saturating_add(FUND_CASH));
            assert_eq!(BasketShares::<Test>::get(hotkey), 0);
            assert_dust_queue_unchanged(hotkey, &subnets);
        }
    });
}
