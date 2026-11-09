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
