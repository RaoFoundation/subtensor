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

// PROTECTED_REGRESSION: oversized pending baskets must make bounded progress and cannot permanently freeze root principal; changes require explicit human approval.
#[test]
fn oversized_pending_basket_recovers_root_stake_without_a_larger_transaction_weight() {
    new_test_ext(1).execute_with(|| {
        let row_count = crate::MAX_ROOT_CLAIM_WORK.saturating_add(44);
        SubnetLimit::<Test>::put(row_count as u16);
        let subnets = priced_subnets();
        assert_eq!(subnets.len(), row_count as usize);

        let hotkey = U256::from(4_301);
        let incumbent = U256::from(4_302);
        let newcomer = U256::from(4_303);
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &incumbent, &hotkey
        ));
        register_on_root(&hotkey, 0);
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &incumbent,
            NetUid::ROOT,
            ROOT_STAKE.into(),
        );
        for netuid in &subnets {
            SubnetAlphaOut::<Test>::mutate(netuid, |alpha| {
                *alpha = alpha.saturating_add(1_000_000u64.into())
            });
            SubtensorModule::enqueue_basket_deposit(
                &hotkey,
                *netuid,
                AlphaBalance::from(1_000_000u64),
            );
        }
        RootClaimableThreshold::<Test>::insert(NetUid::ROOT, I96F32::from_num(0));
        SubtokenEnabled::<Test>::insert(NetUid::ROOT, true);
        crate::SubnetAlphaIn::<Test>::insert(NetUid::ROOT, AlphaBalance::from(10_000_000_000u64));
        add_balance_to_coldkey_account(&newcomer, 2_000_000_000u64.into());

        let shares_before = BasketShares::<Test>::get(hotkey);
        let first = SubtensorModule::add_stake(
            RuntimeOrigin::signed(newcomer),
            hotkey,
            NetUid::ROOT,
            1_000_000_000u64.into(),
        );
        assert_eq!(
            first,
            Err(crate::Error::<Test>::BasketDepositPending.into())
        );
        assert_eq!(
            PendingBasketDeposits::<Test>::iter_prefix(hotkey).count(),
            row_count as usize,
            "a failed dispatch is transactional and must not partially settle dividends"
        );

        SubtensorModule::flush_pending_basket_deposits_block();
        assert_eq!(
            PendingBasketDeposits::<Test>::iter_prefix(hotkey).count(),
            44,
            "the free block drain must commit one bounded recovery chunk"
        );
        SubtensorModule::flush_pending_basket_deposits_block();
        assert_eq!(
            PendingBasketDeposits::<Test>::iter_prefix(hotkey).count(),
            0
        );
        assert!(BasketShares::<Test>::get(hotkey) > shares_before);

        let stake_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &newcomer,
            NetUid::ROOT,
        );
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(newcomer),
            hotkey,
            NetUid::ROOT,
            1_000_000_000u64.into(),
        ));
        let stake_after = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &newcomer,
            NetUid::ROOT,
        );
        let added = stake_after.saturating_sub(stake_before);
        assert!(
            added.to_u64() > 0,
            "the recovered add_stake must move principal"
        );

        assert_ok!(SubtensorModule::remove_stake(
            RuntimeOrigin::signed(newcomer),
            hotkey,
            NetUid::ROOT,
            added,
        ));
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &newcomer,
                NetUid::ROOT,
            ),
            stake_before
        );
    });
}
