//! Critical regressions involving significant losses or unavailable functionality.
//! Existing tests are protected by the explicit-human-approval rule in AGENTS.md.

use super::claim_root::{fund_pool, register_on_root};
use super::mock::*;
use crate::{
    BasketRate, BasketShares, BurnIncreaseMult, CheckColdkeySwap, ColdkeySwapAnnouncements,
    ColdkeySwapDisputes, Error, FirstEmissionBlockNumber, PendingBasketDeposits,
    RootClaimableThreshold, SubnetAlphaOut, SubnetLimit, SubnetMechanism, SubtokenEnabled,
};
use frame_support::{assert_ok, traits::ExtendedDispatchable};
use pallet_subtensor_proxy::{Announcements, Call as ProxyCall, Proxies};
use sp_core::U256;
use sp_runtime::traits::Hash;
use sp_std::collections::btree_set::BTreeSet;
use substrate_fixed::types::{I96F32, U64F64};
use subtensor_runtime_common::{AlphaBalance, NetUid, ProxyType, TaoBalance, Token};

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

// PROTECTED_REGRESSION: PR #3319 must not extinguish an exited claimant's alpha rights at a depressed origin price; changes require explicit human approval.
#[test]
fn deferred_origin_cannot_use_another_claimant_groups_price_budget() {
    new_test_ext(1).execute_with(|| {
        let hot = U256::from(4_401);
        let alice = U256::from(4_402);
        let bob = U256::from(4_403);
        let owner = U256::from(4_404);
        let depressed = add_dynamic_network(&U256::from(4_405), &owner);
        let healthy = add_dynamic_network(&U256::from(4_406), &owner);
        for netuid in [depressed, healthy] {
            remove_owner_registration_stake(netuid);
            fund_pool(netuid);
        }
        register_on_root(&hot, 0);
        SubtensorModule::set_tao_weight(u64::MAX);
        super::claim_root::zero_claim_threshold();
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hot,
            &alice,
            NetUid::ROOT,
            ROOT_STAKE.into(),
        );

        let alice_alpha = 100_000_000u64;
        SubnetAlphaOut::<Test>::mutate(depressed, |alpha| {
            *alpha = alpha.saturating_add(alice_alpha.into());
        });
        SubtensorModule::enqueue_root_dividend(&hot, depressed, alice_alpha.into());
        let original_tao = crate::SubnetTAO::<Test>::get(depressed);
        crate::SubnetTAO::<Test>::insert(depressed, original_tao.saturating_div(100u64.into()));
        let (_, _, ready) = SubtensorModule::flush_basket_deposits_for_hotkey(&hot);
        assert!(ready);
        assert!(crate::DeferredBasketRates::<Test>::contains_key(
            hot, depressed
        ));

        // Alice fully exits before Bob joins. Their dividend rights must not mix.
        let removed = SubtensorModule::decrease_stake_for_hotkey_and_coldkey_on_subnet(
            &hot,
            &alice,
            NetUid::ROOT,
            ROOT_STAKE.into(),
        );
        assert_ok!(
            SubtensorModule::remove_stake_adjust_root_claimed_for_hotkey_and_coldkey(
                &hot, &alice, removed,
            )
        );
        SubtensorModule::maybe_remove_staking_hotkey(&hot, &alice);
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hot,
            &bob,
            NetUid::ROOT,
            ROOT_STAKE.into(),
        );
        assert_ok!(
            SubtensorModule::add_stake_adjust_root_claimed_for_hotkey_and_coldkey(
                &hot, &bob, ROOT_STAKE,
            )
        );
        let bob_alpha = 10_000_000_000u64;
        SubnetAlphaOut::<Test>::mutate(healthy, |alpha| {
            *alpha = alpha.saturating_add(bob_alpha.into());
        });
        SubtensorModule::enqueue_root_dividend(&hot, healthy, bob_alpha.into());
        let escrow = SubtensorModule::get_beta_escrow_account_id();
        assert_eq!(SubtensorModule::get_basket_holdings(&hot).len(), 0);

        // The bad origin is down 99%, but contributes under 1% batch error.
        // Bob's healthy dividend can settle without closing Alice's alpha rights.
        SubtensorModule::settle_basket_deposits_for_test(&hot);
        assert_eq!(
            PendingBasketDeposits::<Test>::get(hot, depressed),
            alice_alpha.into()
        );
        assert!(crate::DeferredBasketRates::<Test>::contains_key(
            hot, depressed
        ));
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hot, &escrow, depressed),
            AlphaBalance::ZERO,
        );
        assert!(!PendingBasketDeposits::<Test>::contains_key(hot, healthy));
        assert!(SubtensorModule::get_basket_owed_shares(&hot, &bob) > bob_alpha * 98 / 100);
        assert_eq!(SubtensorModule::get_basket_owed_shares(&hot, &alice), 0);

        // The price recovers. Alice must receive her original dividend's value,
        // not the ~1% share allocation permanently fixed at the depressed price.
        crate::SubnetTAO::<Test>::insert(depressed, original_tao);
        assert_ok!(SubtensorModule::claim_root(
            RuntimeOrigin::signed(alice),
            BTreeSet::new()
        ));
        let paid =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hot, &alice, NetUid::ROOT)
                .to_u64();
        assert!(
            paid >= alice_alpha * 99 / 100,
            "Alice recovered only {paid}"
        );
        assert!(!PendingBasketDeposits::<Test>::contains_key(hot, depressed));
        assert!(!crate::DeferredBasketRates::<Test>::contains_key(
            hot, depressed
        ));
        assert_ok!(SubtensorModule::claim_root(
            RuntimeOrigin::signed(alice),
            BTreeSet::new()
        ));
        assert_eq!(SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hot, &alice, NetUid::ROOT,
        ).to_u64(), paid);
    });
}

// PROTECTED_REGRESSION: PR #3319 must retain deferred alpha rights when positive holding quotes round to the same value; changes require explicit human approval.
#[test]
fn deferred_zero_marginal_credit_survives_until_actual_claim() {
    // Integer rounding and a price-limit-clamped quote both have this failure
    // mode. The latter can strand a large credit rather than just atomic dust.
    for (held, alice_alpha, depressed_tao) in [
        (1_500_000_000_000u64, 10_000_000_000u64, 4_000_000u64),
        (1_500_000_000u64, 10_000u64, 10_000_000u64),
    ] {
        new_test_ext(1).execute_with(|| {
            let hot = U256::from(4_501);
            let alice = U256::from(4_502);
            let bob = U256::from(4_503);
            let owner = U256::from(4_504);
            let depressed = add_dynamic_network(&U256::from(4_505), &owner);
            let healthy = add_dynamic_network(&U256::from(4_506), &owner);
            for netuid in [depressed, healthy] {
                remove_owner_registration_stake(netuid);
                fund_pool(netuid);
            }
            register_on_root(&hot, 0);
            SubtensorModule::set_tao_weight(u64::MAX);
            super::claim_root::zero_claim_threshold();
            mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
                &hot,
                &alice,
                NetUid::ROOT,
                ROOT_STAKE.into(),
            );
            let escrow = SubtensorModule::get_beta_escrow_account_id();
            mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
                &hot,
                &escrow,
                depressed,
                held.into(),
            );
            let original_tao = crate::SubnetTAO::<Test>::get(depressed);
            crate::SubnetTAO::<Test>::insert(depressed, original_tao.saturating_div(2u64.into()));
            SubnetAlphaOut::<Test>::mutate(depressed, |alpha| {
                *alpha = alpha.saturating_add(alice_alpha.into());
            });
            SubtensorModule::enqueue_root_dividend(&hot, depressed, alice_alpha.into());
            let (_, _, ready) = SubtensorModule::flush_basket_deposits_for_hotkey(&hot);
            assert!(ready);
            assert!(crate::DeferredBasketRates::<Test>::contains_key(
                hot, depressed
            ));

            let removed = SubtensorModule::decrease_stake_for_hotkey_and_coldkey_on_subnet(
                &hot,
                &alice,
                NetUid::ROOT,
                ROOT_STAKE.into(),
            );
            assert_ok!(
                SubtensorModule::remove_stake_adjust_root_claimed_for_hotkey_and_coldkey(
                    &hot, &alice, removed,
                )
            );
            SubtensorModule::maybe_remove_staking_hotkey(&hot, &alice);
            mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
                &hot,
                &bob,
                NetUid::ROOT,
                ROOT_STAKE.into(),
            );
            assert_ok!(
                SubtensorModule::add_stake_adjust_root_claimed_for_hotkey_and_coldkey(
                    &hot, &bob, ROOT_STAKE,
                )
            );

            // Bob owns the existing basket. Cash keeps the held-price impact below
            // 0.1% even after a further price drop; Alice owns only her deferred alpha.
            let cash = 1_000_000_000_000_000u64;
            SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &hot,
                &escrow,
                NetUid::ROOT,
                cash.into(),
            );
            let Some(root_account) = SubtensorModule::get_subnet_account_id(NetUid::ROOT) else {
                panic!("root subnet must have a funding account");
            };
            add_balance_to_coldkey_account(&root_account, cash.into());
            BasketShares::<Test>::insert(hot, cash);
            BasketRate::<Test>::insert(hot, I96F32::from_num(1_000_000));
            crate::SubnetTAO::<Test>::insert(
                depressed,
                subtensor_runtime_common::TaoBalance::from(depressed_tao),
            );
            let before = SubtensorModule::try_realizable_tao_for_alpha(depressed, held);
            let after =
                SubtensorModule::try_realizable_tao_for_alpha(depressed, held + alice_alpha);
            assert!(matches!(before, Ok(Some(value)) if value > 0));
            assert_eq!(
                before, after,
                "nonterminal marginal quote must round to zero"
            );

            let bob_payout_before = SubtensorModule::get_basket_payout_tao(&hot, &bob);
            let bob_alpha = 1_000_000_000u64;
            SubnetAlphaOut::<Test>::mutate(healthy, |alpha| {
                *alpha = alpha.saturating_add(bob_alpha.into());
            });
            SubtensorModule::enqueue_root_dividend(&hot, healthy, bob_alpha.into());
            SubtensorModule::settle_basket_deposits_for_test(&hot);
            assert_eq!(
                PendingBasketDeposits::<Test>::get(hot, depressed),
                alice_alpha.into()
            );
            assert!(crate::DeferredBasketRates::<Test>::contains_key(
                hot, depressed
            ));
            assert_eq!(
                SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &hot, &escrow, depressed,
                )
                .to_u64(),
                held
            );
            assert!(!PendingBasketDeposits::<Test>::contains_key(hot, healthy));
            assert!(
                SubtensorModule::get_basket_payout_tao(&hot, &bob)
                    > bob_payout_before + bob_alpha * 98 / 100
            );
            assert_eq!(SubtensorModule::get_basket_owed_shares(&hot, &alice), 0);

            // Recovery must turn the preserved alpha into a real payout to Alice.
            crate::SubnetTAO::<Test>::insert(depressed, original_tao);
            let restored_value =
                SubtensorModule::realizable_tao_for_alpha(depressed, held + alice_alpha)
                    - SubtensorModule::realizable_tao_for_alpha(depressed, held);
            assert!(restored_value > 0);
            assert_ok!(SubtensorModule::claim_root(
                RuntimeOrigin::signed(alice),
                BTreeSet::new()
            ));
            let paid = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hot,
                &alice,
                NetUid::ROOT,
            )
            .to_u64();
            assert!(
                paid >= restored_value * 99 / 100,
                "Alice recovered only {paid} of {restored_value}"
            );
            assert!(!PendingBasketDeposits::<Test>::contains_key(hot, depressed));
            assert!(!crate::DeferredBasketRates::<Test>::contains_key(
                hot, depressed
            ));
            assert_ok!(SubtensorModule::claim_root(
                RuntimeOrigin::signed(alice),
                BTreeSet::new()
            ));
            assert_eq!(
                SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &hot,
                    &alice,
                    NetUid::ROOT,
                )
                .to_u64(),
                paid
            );
        });
    }
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

// PROTECTED_REGRESSION: lease dust must not block termination or strand the synthetic account's stake; changes require explicit human approval.
#[test]
fn lease_termination_hands_over_dust_below_the_transfer_minimum() {
    use super::leasing::{setup_crowdloan, setup_leased_network};
    use crate::{
        Error, Lock, OwnerCutAutoLockEnabled, SubnetLeases, SubnetOwner, SubnetOwnerHotkey,
        SubnetUidToLeaseId,
    };
    use frame_support::{assert_noop, storage::with_storage_layer, traits::Get};
    use sp_runtime::Percent;

    for locked in [false, true] {
        for pool_tao in [1u64, 1_000_000_000] {
            new_test_ext(1).execute_with(|| {
                let beneficiary = U256::from(1);
                setup_crowdloan(
                    0,
                    10_000_000_000,
                    1_000_000_000_000,
                    beneficiary,
                    &[(U256::from(2), 990_000_000_000)],
                );
                let end_block = 500;
                let (lease_id, lease) = setup_leased_network(
                    beneficiary,
                    Percent::from_percent(30),
                    Some(end_block),
                    None,
                );
                run_to_block(end_block + 1);
                remove_owner_registration_stake(lease.netuid);
                setup_reserves(lease.netuid, pool_tao.into(), 1_000_000_000u64.into());
                let dust = AlphaBalance::from(1_000u64);
                assert!(dust.to_u64() < crate::DefaultMinTransfer::<Test>::get().to_u64());
                SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                    &lease.hotkey,
                    &lease.coldkey,
                    lease.netuid,
                    dust,
                );
                let destination = U256::from(100_003);
                assert_ok!(SubtensorModule::create_account_if_non_existent(
                    &beneficiary,
                    &destination
                ));
                // Ordinary transfers retain their minimum, even when the protocol handover is exempt.
                assert_noop!(
                    with_storage_layer(|| SubtensorModule::transfer_stake_within_subnet(
                        &lease.coldkey,
                        &lease.hotkey,
                        &beneficiary,
                        &destination,
                        lease.netuid,
                        dust,
                    )),
                    Error::<Test>::AmountTooLow
                );
                if locked {
                    OwnerCutAutoLockEnabled::<Test>::insert(lease.netuid, true);
                    SubtensorModule::auto_lock_owner_cut(lease.netuid, dust);
                    assert!(Lock::<Test>::contains_key((
                        lease.coldkey,
                        lease.netuid,
                        lease.hotkey
                    )));
                }
                assert_ok!(SubtensorModule::terminate_lease(
                    RuntimeOrigin::signed(beneficiary),
                    lease_id,
                    destination,
                ));
                assert_eq!(SubnetOwner::<Test>::get(lease.netuid), beneficiary);
                assert_eq!(SubnetOwnerHotkey::<Test>::get(lease.netuid), destination);
                assert_eq!(
                    SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                        &lease.hotkey,
                        &lease.coldkey,
                        lease.netuid,
                    ),
                    AlphaBalance::ZERO,
                );
                assert_eq!(
                    SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                        &destination,
                        &beneficiary,
                        lease.netuid,
                    ),
                    dust,
                );
                assert!(!Lock::<Test>::contains_key((
                    lease.coldkey,
                    lease.netuid,
                    lease.hotkey
                )));
                if locked {
                    let Some(lock) = Lock::<Test>::get((beneficiary, lease.netuid, destination))
                    else {
                        panic!("the dust lock must follow the handed-over stake");
                    };
                    assert_eq!(lock.locked_mass, dust);
                }
                assert!(!SubnetLeases::<Test>::contains_key(lease_id));
                assert!(!SubnetUidToLeaseId::<Test>::contains_key(lease.netuid));
            });
        }
    }
}

fn lease_regression_funded(
    stake: u64,
    end_block: u64,
) -> (
    u32,
    crate::subnets::leasing::SubnetLeaseOf<Test>,
    U256,
    U256,
) {
    use super::leasing::{setup_crowdloan, setup_leased_network};
    use sp_runtime::Percent;
    let beneficiary = U256::from(1);
    let contributor = U256::from(2);
    let destination = U256::from(100_003);
    setup_crowdloan(
        0,
        10_000_000_000,
        1_000_000_000_000,
        beneficiary,
        &[(contributor, 990_000_000_000)],
    );
    let (id, lease) = setup_leased_network(
        beneficiary,
        Percent::from_percent(30),
        Some(end_block),
        None,
    );
    remove_owner_registration_stake(lease.netuid);
    setup_reserves(
        lease.netuid,
        1_000_000_000_000u64.into(),
        1_000_000_000_000u64.into(),
    );
    SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
        &lease.hotkey,
        &lease.coldkey,
        lease.netuid,
        stake.into(),
    );
    assert_ok!(SubtensorModule::create_account_if_non_existent(
        &beneficiary,
        &destination
    ));
    (id, lease, contributor, destination)
}

fn lease_regression_conviction_winner(
    lease: &crate::subnets::leasing::SubnetLeaseOf<Test>,
    now: u64,
) -> (U256, U256) {
    use crate::staking::lock::LockState;
    use crate::{DecayingHotkeyLock, Lock, NetworkRegisteredAt, SubnetOwner, SubnetOwnerHotkey};
    let coldkey = U256::from(20);
    let hotkey = U256::from(21);
    let mass = AlphaBalance::from(1_000_000_000_000u64);
    System::set_block_number(now);
    NetworkRegisteredAt::<Test>::insert(lease.netuid, 0);
    assert_ok!(SubtensorModule::create_account_if_non_existent(
        &coldkey, &hotkey
    ));
    SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
        &hotkey,
        &coldkey,
        lease.netuid,
        mass,
    );
    let source = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
        &lease.hotkey,
        &lease.coldkey,
        lease.netuid,
    );
    SubnetAlphaOut::<Test>::insert(lease.netuid, mass.saturating_add(source));
    let state = LockState {
        locked_mass: mass,
        conviction: U64F64::from_num(mass.to_u64()),
        last_update: now,
    };
    Lock::<Test>::insert((coldkey, lease.netuid, hotkey), state.clone());
    DecayingHotkeyLock::<Test>::insert(lease.netuid, hotkey, state);
    SubtensorModule::change_subnet_owner_if_needed(lease.netuid);
    assert_eq!(SubnetOwner::<Test>::get(lease.netuid), coldkey);
    assert_eq!(SubnetOwnerHotkey::<Test>::get(lease.netuid), hotkey);
    (coldkey, hotkey)
}

// PROTECTED_REGRESSION: final accumulated lease dividends must reach their contributors instead of the beneficiary; changes require explicit human approval.
#[test]
fn lease_final_accrual_reaches_contributors_before_residual_handover() {
    use crate::{AccumulatedLeaseDividends, SubnetLeaseShares, SubnetLeases};
    for ended_hook_first in [false, true] {
        new_test_ext(1).execute_with(|| {
            let (id, lease, contributor, destination) =
                lease_regression_funded(60_000_000_000, 500);
            let cut = AlphaBalance::from(10_000_000_000u64);
            SubtensorModule::distribute_leased_network_dividends(id, cut);
            let accrued = AccumulatedLeaseDividends::<Test>::get(id);
            assert_eq!(accrued, AlphaBalance::from(3_000_000_000u64));
            let owed = AlphaBalance::from(
                SubnetLeaseShares::<Test>::get(id, contributor)
                    .saturating_mul(U64F64::from_num(accrued.to_u64()))
                    .to_num::<u64>(),
            );
            assert!(owed > AlphaBalance::from(2_900_000_000u64));
            System::set_block_number(501); // Not a scheduled distribution block.
            if ended_hook_first {
                SubtensorModule::distribute_leased_network_dividends(id, AlphaBalance::ZERO);
                assert_eq!(
                    SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                        &lease.hotkey,
                        &contributor,
                        lease.netuid,
                    ),
                    owed
                );
            }
            assert_ok!(SubtensorModule::terminate_lease(
                RuntimeOrigin::signed(lease.beneficiary),
                id,
                destination,
            ));
            assert_eq!(
                SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &lease.hotkey,
                    &contributor,
                    lease.netuid,
                ),
                owed
            );
            assert_eq!(
                SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &destination,
                    &lease.beneficiary,
                    lease.netuid,
                ),
                AlphaBalance::from(60_000_000_000u64) - owed
            );
            SubtensorModule::distribute_leased_network_dividends(id, AlphaBalance::ZERO);
            assert_eq!(
                SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &lease.hotkey,
                    &contributor,
                    lease.netuid,
                ),
                owed
            );
            assert!(!SubnetLeases::<Test>::contains_key(id));
        });
    }
}

// PROTECTED_REGRESSION: a sub-minimum contributor debt must not freeze the beneficiary's locked lease stake; changes require explicit human approval.
#[test]
fn lease_dust_debt_does_not_hold_back_locked_owner_stake() {
    use crate::{Lock, OwnerCutAutoLockEnabled, SubnetLeaseUnpaidDividends, SubnetLeases};
    new_test_ext(1).execute_with(|| {
        let (id, lease, contributor, destination) = lease_regression_funded(55_000_000_000, 500);
        let owner_lock = AlphaBalance::from(50_000_000_000u64);
        OwnerCutAutoLockEnabled::<Test>::insert(lease.netuid, true);
        SubtensorModule::auto_lock_owner_cut(lease.netuid, owner_lock);
        let debt = AlphaBalance::from(1_000_000u64);
        SubnetLeaseUnpaidDividends::<Test>::insert(id, contributor, debt);
        System::set_block_number(501);
        assert_ok!(SubtensorModule::terminate_lease(
            RuntimeOrigin::signed(lease.beneficiary),
            id,
            destination,
        ));
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &lease.hotkey,
                &contributor,
                lease.netuid,
            ),
            debt
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &destination,
                &lease.beneficiary,
                lease.netuid,
            ),
            AlphaBalance::from(55_000_000_000u64) - debt
        );
        assert!(
            Lock::<Test>::get((lease.beneficiary, lease.netuid, destination))
                .is_some_and(|lock| lock.locked_mass > AlphaBalance::from(49_000_000_000u64))
        );
        assert!(
            SubnetLeaseUnpaidDividends::<Test>::iter_prefix(id)
                .next()
                .is_none()
        );
        assert!(!SubnetLeases::<Test>::contains_key(id));
    });
}

// PROTECTED_REGRESSION: terminated lease retries must preserve a conviction winner and recover outstanding funds without removing the proxy twice; changes require explicit human approval.
#[test]
fn lease_retry_after_conviction_takeover_recovers_funds_without_reclaiming_ownership() {
    use crate::{SubnetLeaseUnpaidDividends, SubnetLeases, SubnetOwner, SubnetOwnerHotkey};
    for (use_hook, legacy_record) in [(false, false), (true, false), (false, true), (true, true)] {
        new_test_ext(1).execute_with(|| {
            let (id, lease, contributor, destination) =
                lease_regression_funded(55_000_000_000, 500);
            let debt = AlphaBalance::from(100_000_000_000u64);
            SubnetLeaseUnpaidDividends::<Test>::insert(id, contributor, debt);
            System::set_block_number(501);
            assert_ok!(SubtensorModule::terminate_lease(
                RuntimeOrigin::signed(lease.beneficiary),
                id,
                destination,
            ));
            assert!(SubnetLeases::<Test>::contains_key(id));
            if legacy_record {
                // Pre-upgrade partial terminations have no new marker, but their proxy was removed.
                crate::SubnetLeaseTerminationHotkey::<Test>::remove(id);
            }
            let (winner, winner_hotkey) =
                lease_regression_conviction_winner(&lease, crate::staking::lock::ONE_YEAR + 501);
            assert_ok!(SubtensorModule::terminate_lease(
                RuntimeOrigin::signed(lease.beneficiary),
                id,
                destination,
            ));
            assert_eq!(SubnetOwner::<Test>::get(lease.netuid), winner);
            assert_eq!(SubnetOwnerHotkey::<Test>::get(lease.netuid), winner_hotkey);
            SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &lease.hotkey,
                &lease.coldkey,
                lease.netuid,
                debt,
            );
            if use_hook {
                SubtensorModule::distribute_leased_network_dividends(id, AlphaBalance::ZERO);
            } else {
                assert_ok!(SubtensorModule::terminate_lease(
                    RuntimeOrigin::signed(lease.beneficiary),
                    id,
                    destination,
                ));
            }
            assert_eq!(SubnetOwner::<Test>::get(lease.netuid), winner);
            assert_eq!(SubnetOwnerHotkey::<Test>::get(lease.netuid), winner_hotkey);
            assert_eq!(
                SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &lease.hotkey,
                    &contributor,
                    lease.netuid,
                ),
                debt
            );
            assert_eq!(
                SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &destination,
                    &lease.beneficiary,
                    lease.netuid,
                ),
                AlphaBalance::from(55_000_000_000u64)
            );
            assert!(!SubnetLeases::<Test>::contains_key(id));
        });
    }
}

// PROTECTED_REGRESSION: lease termination after a conviction takeover must not leave ghost locks or debit the winner's owner bucket; changes require explicit human approval.
#[test]
fn lease_termination_after_conviction_takeover_preserves_lock_buckets() {
    use crate::{DecayingHotkeyLock, DecayingOwnerLock, Lock, OwnerCutAutoLockEnabled};
    new_test_ext(1).execute_with(|| {
        let end = crate::staking::lock::ONE_YEAR + 1_000;
        let (id, lease, _contributor, destination) = lease_regression_funded(55_000_000_000, end);
        System::set_block_number(crate::staking::lock::ONE_YEAR + 1);
        OwnerCutAutoLockEnabled::<Test>::insert(lease.netuid, true);
        SubtensorModule::auto_lock_owner_cut(lease.netuid, 5_000_000_000u64.into());
        let (_winner, winner_hotkey) =
            lease_regression_conviction_winner(&lease, crate::staking::lock::ONE_YEAR + 1);
        assert!(
            DecayingHotkeyLock::<Test>::get(lease.netuid, lease.hotkey)
                .is_some_and(|lock| lock.locked_mass != AlphaBalance::ZERO)
        );
        System::set_block_number(end + 1);
        assert_ok!(SubtensorModule::terminate_lease(
            RuntimeOrigin::signed(lease.beneficiary),
            id,
            destination,
        ));
        assert!(!Lock::<Test>::contains_key((
            lease.coldkey,
            lease.netuid,
            lease.hotkey
        )));
        assert!(!DecayingHotkeyLock::<Test>::contains_key(
            lease.netuid,
            lease.hotkey
        ));
        assert!(
            DecayingHotkeyLock::<Test>::get(lease.netuid, winner_hotkey)
                .is_some_and(|lock| lock.locked_mass > AlphaBalance::from(900_000_000_000u64))
        );
        let Some(beneficiary_lock) =
            Lock::<Test>::get((lease.beneficiary, lease.netuid, destination))
        else {
            panic!("the beneficiary must receive the actual retained lock");
        };
        assert_eq!(
            DecayingOwnerLock::<Test>::get(lease.netuid).map(|lock| lock.locked_mass),
            Some(beneficiary_lock.locked_mass)
        );
    });
}

// PROTECTED_REGRESSION: a coldkey swap must not prevent a delayed proxy controller from cancelling a takeover announcement; changes require explicit human approval.
#[test]
#[allow(clippy::unwrap_used)]
fn proxy_controller_can_cancel_only_its_own_announcements_during_coldkey_swap() {
    new_test_ext(1).execute_with(|| {
        let pure = U256::from(4_901);
        let controller = U256::from(4_902);
        let other_controller = U256::from(4_903);
        let attacker = U256::from(4_904);
        for account in [pure, controller, other_controller] {
            add_balance_to_coldkey_account(&account, TaoBalance::new(1_000_000_000));
        }
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(pure),
            controller,
            ProxyType::Any,
            3,
        ));
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(pure),
            other_controller,
            ProxyType::Any,
            3,
        ));

        let takeover_call = RuntimeCall::Proxy(ProxyCall::add_proxy {
            delegate: attacker,
            proxy_type: ProxyType::Any,
            delay: 0,
        });
        let first_hash = <Test as frame_system::Config>::Hashing::hash_of(&takeover_call);
        let second_hash = <Test as frame_system::Config>::Hashing::hash_of(&RuntimeCall::System(
            frame_system::Call::remark { remark: vec![2] },
        ));
        for hash in [first_hash, second_hash] {
            assert_ok!(Proxy::announce(
                RuntimeOrigin::signed(controller),
                pure,
                hash,
            ));
        }
        assert_ok!(Proxy::announce(
            RuntimeOrigin::signed(other_controller),
            pure,
            first_hash,
        ));

        ColdkeySwapAnnouncements::<Test>::insert(
            controller,
            (
                System::block_number(),
                <Test as frame_system::Config>::Hashing::hash_of(&U256::from(4_905)),
            ),
        );
        let remove = |call_hash| {
            RuntimeCall::Proxy(ProxyCall::remove_announcement {
                real: pure,
                call_hash,
            })
        };
        assert_ok!(<CheckColdkeySwap<Test> as ExtendedDispatchable<
            RuntimeCall,
        >>::dispatch_with_extension(
            RuntimeOrigin::signed(controller),
            remove(first_hash),
        ));
        assert_eq!(Announcements::<Test>::get(controller).0.len(), 1);
        assert_eq!(Announcements::<Test>::get(other_controller).0.len(), 1);

        System::set_block_number(System::block_number() + 3);
        let execute_takeover = RuntimeCall::Proxy(ProxyCall::proxy_announced {
            delegate: controller,
            real: pure,
            force_proxy_type: Some(ProxyType::Any),
            call: Box::new(takeover_call),
        });
        assert!(
            <CheckColdkeySwap<Test> as ExtendedDispatchable<RuntimeCall>>::dispatch_with_extension(
                RuntimeOrigin::signed(attacker),
                execute_takeover,
            )
            .is_err()
        );
        assert!(
            !Proxies::<Test>::get(pure)
                .0
                .iter()
                .any(|definition| definition.delegate == attacker)
        );

        // A dispute keeps every other controller action frozen, but must not freeze cancellation.
        ColdkeySwapDisputes::<Test>::insert(controller, System::block_number());
        let announce = RuntimeCall::Proxy(ProxyCall::announce {
            real: pure,
            call_hash: first_hash,
        });
        assert_eq!(
            <CheckColdkeySwap<Test> as ExtendedDispatchable<RuntimeCall>>::dispatch_with_extension(
                RuntimeOrigin::signed(controller),
                announce,
            )
            .unwrap_err()
            .error,
            Error::<Test>::ColdkeySwapDisputed.into(),
        );
        assert_ok!(<CheckColdkeySwap<Test> as ExtendedDispatchable<
            RuntimeCall,
        >>::dispatch_with_extension(
            RuntimeOrigin::signed(controller),
            remove(second_hash),
        ));
        assert!(Announcements::<Test>::get(controller).0.is_empty());

        // remove_announcement has no delegate argument: entering the pallet as `controller`
        // cannot touch an announcement stored under a different delegate.
        assert!(
            <CheckColdkeySwap<Test> as ExtendedDispatchable<RuntimeCall>>::dispatch_with_extension(
                RuntimeOrigin::signed(controller),
                remove(first_hash),
            )
            .is_err()
        );
        assert_eq!(Announcements::<Test>::get(other_controller).0.len(), 1);
    });
}
