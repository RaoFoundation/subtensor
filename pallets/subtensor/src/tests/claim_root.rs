#![allow(clippy::expect_used, clippy::unwrap_used)]

use crate::staking::BasketFlushWork;
use crate::tests::mock::*;
use crate::weights::WeightInfo;
use crate::{
    AlphaV2, BasketClaimed, BasketRate, BasketRedeemedTao, BasketShares, BurnIncreaseMult,
    DefaultMinRootClaimAmount, Error, Keys, LastEpochBlock, MAX_ROOT_CLAIM_HOTKEY_WORK,
    MAX_ROOT_CLAIM_HOTKEY_WORK_TESTNET, MAX_ROOT_CLAIM_THRESHOLD, MAX_ROOT_CLAIM_WORK,
    NetworksAdded, NumStakingColdkeys, PendingBasketDeposits, RegistrationsThisInterval,
    RootAlphaDividendsPerSubnet, RootClaimableThreshold, StakingColdkeys, StakingColdkeysByIndex,
    StakingHotkeys, SubnetAlphaIn, SubnetAlphaOut, SubnetMovingPrice, SubnetOwnerHotkey,
    SubnetProtocolFlow, SubnetTAO, SubnetworkN, Tempo, TotalStake, Uids,
};
use approx::assert_abs_diff_eq;
use frame_support::dispatch::{DispatchClass, GetDispatchInfo, RawOrigin};
use frame_support::pallet_prelude::Weight;
use frame_support::traits::Get;
use frame_support::{assert_err, assert_err_ignore_postinfo, assert_ok, assert_storage_noop};
use sp_core::{H256, U256};
use sp_runtime::DispatchError;
use sp_std::collections::btree_set::BTreeSet;
use substrate_fixed::types::{I96F32, U64F64, U96F32};
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};

// =============================================================================
// Helpers
// =============================================================================

/// Ensure a subnet has deep, balanced AMM reserves so basket swaps execute with negligible
/// slippage and never fail for lack of liquidity. Also funds the subnet free-balance pot so
/// protocol redeploys can physically move sold TAO onto destination accounts.
pub(super) fn fund_pool(netuid: NetUid) {
    let tao = TaoBalance::from(1_000_000_000_000u64);
    SubnetTAO::<Test>::insert(netuid, tao);
    SubnetAlphaIn::<Test>::insert(netuid, AlphaBalance::from(1_000_000_000_000u64));
    if let Some(subnet_account) = SubtensorModule::get_subnet_account_id(netuid) {
        add_balance_to_coldkey_account(&subnet_account, tao);
    }
}

/// Claims are fund-level and consult only the ROOT threshold entry; zero it for tests that
/// exercise small claims.
pub(super) fn zero_claim_threshold() {
    RootClaimableThreshold::<Test>::insert(NetUid::ROOT, I96F32::from_num(0));
}

/// Epochs enqueue basket deposits into `PendingBasketDeposits`; the per-block drain (or any
/// touch of the hotkey) performs the actual deposit. Tests that drive dividends through
/// `distribute_emission` directly call this where the pre-queue code deposited inline.
/// The chain drain flushes one hotkey per block; here every queued hotkey is flushed at
/// once so assertions see all deposits landed.
pub(super) fn flush_baskets() {
    let hotkeys: BTreeSet<U256> = crate::PendingBasketDeposits::<Test>::iter_keys()
        .map(|(hotkey, _)| hotkey)
        .collect();
    for hotkey in hotkeys {
        let _ = SubtensorModule::flush_basket_deposits_for_hotkey(&hotkey);
    }
}

/// Grant a hotkey a root-network UID: it qualifies for root dividends (the epoch split pays
/// root dividends only to root-registered hotkeys), which accumulate in place on their
/// origin subnet.
pub(super) fn register_on_root(hotkey: &U256, uid: u16) {
    Uids::<Test>::insert(NetUid::ROOT, hotkey, uid);
}

pub(super) fn escrow_alpha(hotkey: &U256, netuid: NetUid) -> u64 {
    let escrow = SubtensorModule::get_beta_escrow_account_id();
    SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, &escrow, netuid).to_u64()
}

pub(super) fn fund_shares(hotkey: &U256) -> u64 {
    BasketShares::<Test>::get(hotkey)
}

pub(super) fn has_fund(hotkey: &U256) -> bool {
    BasketRate::<Test>::get(hotkey) > I96F32::from_num(0)
}

pub(super) fn root_stake_of(hotkey: &U256, coldkey: &U256) -> u64 {
    SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, NetUid::ROOT)
        .to_u64()
}

// =============================================================================
// Still-valid utility tests (independent of the beta-basket accrual mechanics)
// =============================================================================

#[test]
fn test_populate_staking_maps() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1000);
        let coldkey1 = U256::from(1001);
        let coldkey2 = U256::from(1002);
        let coldkey3 = U256::from(1003);
        let hotkey = U256::from(1004);
        let _netuid = add_dynamic_network(&hotkey, &owner_coldkey);
        let netuid2 = NetUid::from(2);

        let root_stake = 200_000_000u64;
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey1,
            NetUid::ROOT,
            root_stake.into(),
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey2,
            NetUid::ROOT,
            root_stake.into(),
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey3,
            netuid2,
            root_stake.into(),
        );

        assert_eq!(NumStakingColdkeys::<Test>::get(), 0);

        // Populate maps through block step
        run_to_block(2);

        assert_eq!(NumStakingColdkeys::<Test>::get(), 2);

        assert!(StakingColdkeysByIndex::<Test>::contains_key(0));
        assert!(StakingColdkeysByIndex::<Test>::contains_key(1));

        assert!(StakingColdkeys::<Test>::contains_key(coldkey1));
        assert!(StakingColdkeys::<Test>::contains_key(coldkey2));
        assert!(!StakingColdkeys::<Test>::contains_key(coldkey3));
    });
}

#[test]
fn test_claim_root_threshold() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&hotkey, &owner_coldkey);

        assert_eq!(
            RootClaimableThreshold::<Test>::get(NetUid::ROOT),
            DefaultMinRootClaimAmount::<Test>::get()
        );

        let threshold = 1000u64;
        assert_ok!(SubtensorModule::sudo_set_root_claim_threshold(
            RawOrigin::Root.into(),
            NetUid::ROOT,
            threshold
        ));
        assert_eq!(
            RootClaimableThreshold::<Test>::get(NetUid::ROOT),
            I96F32::from(threshold)
        );

        // Errors: bad origin, non-ROOT netuid (only the ROOT entry is consulted by claims, so
        // anything else would be silently inert and is rejected), out-of-range value.
        assert_err!(
            SubtensorModule::sudo_set_root_claim_threshold(
                RawOrigin::Signed(hotkey).into(),
                NetUid::ROOT,
                threshold
            ),
            DispatchError::BadOrigin,
        );

        assert_err!(
            SubtensorModule::sudo_set_root_claim_threshold(RawOrigin::Root.into(), netuid, 500),
            Error::<Test>::InvalidRootClaimThreshold,
        );
        assert_eq!(
            RootClaimableThreshold::<Test>::get(netuid),
            DefaultMinRootClaimAmount::<Test>::get(),
            "non-ROOT entry must not be written"
        );

        assert_err!(
            SubtensorModule::sudo_set_root_claim_threshold(
                RawOrigin::Root.into(),
                NetUid::ROOT,
                MAX_ROOT_CLAIM_THRESHOLD + 1
            ),
            Error::<Test>::InvalidRootClaimThreshold,
        );
    });
}

#[test]
fn test_claim_root_declared_weight_covers_bounded_work() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let escrow = SubtensorModule::get_beta_escrow_account_id();

        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            NetUid::ROOT,
            1_u64.into(),
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &escrow,
            NetUid::ROOT,
            1_000_000_u64.into(),
        );
        BasketShares::<Test>::insert(hotkey, 1);
        BasketRate::<Test>::insert(hotkey, I96F32::from_num(1));
        zero_claim_threshold();

        let subnets = BTreeSet::from([NetUid::ROOT]);
        let call = RuntimeCall::SubtensorModule(crate::Call::claim_root {
            subnets: subnets.clone(),
        });
        let declared_weight = call.get_dispatch_info().call_weight;
        assert_eq!(
            SubtensorModule::root_claim_declared_work(),
            MAX_ROOT_CLAIM_WORK
        );
        // Claim envelope plus the flat pending-deposit flush allowance every flushing
        // extrinsic declares.
        let envelope = <Test as crate::Config>::WeightInfo::claim_root(MAX_ROOT_CLAIM_WORK)
            .saturating_add(<Test as crate::Config>::WeightInfo::claim_root_scan(
                MAX_ROOT_CLAIM_WORK,
            ))
            .saturating_add(SubtensorModule::basket_flush_weight_bound());
        assert!(
            declared_weight.all_gte(envelope),
            "declared {declared_weight:?} must cover the {envelope:?} admission envelope"
        );
        // Network count is not part of admission; only relevant hotkeys and stored basket rows
        // consume the fixed envelope.
        for raw_netuid in 1..=MAX_ROOT_CLAIM_WORK as u16 {
            NetworksAdded::<Test>::insert(NetUid::from(raw_netuid), true);
        }
        let actual_weight = SubtensorModule::claim_root(RuntimeOrigin::signed(coldkey), subnets)
            .expect("claim succeeds")
            .actual_weight
            .expect("claim reports benchmark-derived actual weight");

        assert!(actual_weight.all_lte(declared_weight));

        let max_extrinsic = BlockWeights::get()
            .get(DispatchClass::Normal)
            .max_extrinsic
            .expect("normal extrinsics have a configured maximum");
        assert!(
            declared_weight.all_lte(max_extrinsic),
            "declared weight {declared_weight:?} exceeds max extrinsic {max_extrinsic:?}"
        );

        // The single-hotkey declaration reserves 128 subnet slots plus root, not
        // the coldkey-wide 256-unit envelope.
        let single_work = MAX_ROOT_CLAIM_HOTKEY_WORK;
        assert_eq!(single_work, 129);
        let single_call =
            RuntimeCall::SubtensorModule(crate::Call::claim_root_with_hotkey { hotkey });
        let single_declared = single_call.get_dispatch_info().call_weight;
        let single_envelope = SubtensorModule::root_claim_hotkey_declared_weight();
        assert!(single_declared.all_gte(single_envelope));
        assert!(single_declared.all_lt(declared_weight));
    });
}

#[test]
fn test_claim_root_hotkey_work_limit_is_raised_only_on_finney_testnet() {
    new_test_ext(1).execute_with(|| {
        const FINNEY_TESTNET_GENESIS_HASH: [u8; 32] =
            hex_literal::hex!("8f9cf856bf558a14440e75569c9e58594757048d7b3a84b5d25f6bd978263105");

        assert_eq!(
            SubtensorModule::root_claim_hotkey_declared_work(),
            MAX_ROOT_CLAIM_HOTKEY_WORK
        );

        frame_system::BlockHash::<Test>::insert(
            0_u64,
            H256::from_slice(&FINNEY_TESTNET_GENESIS_HASH),
        );
        assert_eq!(
            SubtensorModule::root_claim_hotkey_declared_work(),
            MAX_ROOT_CLAIM_HOTKEY_WORK_TESTNET
        );

        frame_system::BlockHash::<Test>::insert(0_u64, H256::from_low_u64_be(0xdeadbeef));
        assert_eq!(
            SubtensorModule::root_claim_hotkey_declared_work(),
            MAX_ROOT_CLAIM_HOTKEY_WORK
        );
    });
}

/// A claim first flushes the validator's queued dividend credits; that work is charged into
/// the post-dispatch weight through the same `basket_flush_weight` model as `swap_basket`
/// and `stake_into_basket`, and the total refunds below the flat declared cap.
#[test]
fn test_claim_root_charges_flush_work_and_refunds_below_declared() {
    new_test_ext(1).execute_with(|| {
        SubtensorModule::set_tao_weight(u64::MAX);
        zero_claim_threshold();
        let coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let owner = U256::from(1003);
        let owner_hot = U256::from(1004);
        let netuid = add_dynamic_network(&owner_hot, &owner);
        remove_owner_registration_stake(netuid);
        fund_pool(netuid);
        register_on_root(&hotkey, 0);
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            NetUid::ROOT,
            10_000_000_000u64.into(),
        );
        // Queue one uncurated credit; the claim's flush deposits it (scan 1 + attempt on
        // an empty fund: 0 holdings + 2 quotes, and one in-place row).
        let credit = 1_000_000u64;
        SubnetAlphaOut::<Test>::mutate(netuid, |t| *t = t.saturating_add(credit.into()));
        SubtensorModule::enqueue_basket_deposit(&hotkey, netuid, credit.into());
        let expected_flush_work = BasketFlushWork::new(1 + 2, 1);

        let declared = RuntimeCall::SubtensorModule(crate::Call::claim_root_with_hotkey { hotkey })
            .get_dispatch_info()
            .call_weight;

        let outcome =
            SubtensorModule::root_claim_for_hotkey(&hotkey, &coldkey, false).expect("claim runs");
        assert_eq!(outcome.flush, expected_flush_work);
        assert!(
            !PendingBasketDeposits::<Test>::contains_key(hotkey, netuid),
            "the claim flushed the queued credit"
        );
        assert!(outcome.tao > 0, "the flushed dividend was redeemed");

        let actual = SubtensorModule::root_claim_actual_weight(1, 0, &outcome);
        let without_flush = SubtensorModule::root_claim_actual_weight(
            1,
            0,
            &crate::staking::RootClaimOutcome {
                flush: BasketFlushWork::default(),
                ..outcome
            },
        );
        assert_eq!(
            actual,
            without_flush.saturating_add(SubtensorModule::basket_flush_weight(expected_flush_work))
        );
        assert!(
            actual.all_lt(declared),
            "actual {actual:?} must refund below declared {declared:?}"
        );
    });
}

#[test]
fn test_claim_root_rejects_work_above_declared_budget() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1001);
        let hotkeys: Vec<U256> = (0..=MAX_ROOT_CLAIM_WORK)
            .map(|i| U256::from(2_000u32.saturating_add(i)))
            .collect();
        assert!(!SubtensorModule::root_claim_fits_declared_budget(&hotkeys));

        // Candidate classification is independently bounded even if every relationship would
        // subsequently be filtered as non-root.
        StakingHotkeys::<Test>::insert(coldkey, hotkeys);
        assert_storage_noop!(assert_err_ignore_postinfo!(
            SubtensorModule::claim_root(RuntimeOrigin::signed(coldkey), BTreeSet::new()),
            Error::<Test>::RootClaimTooHeavy
        ));
    });
}

#[test]
fn test_claim_root_ignores_network_count_and_bounds_actual_basket_rows() {
    new_test_ext(1).execute_with(|| {
        for raw_netuid in 1..=(MAX_ROOT_CLAIM_WORK as u16 + 10) {
            NetworksAdded::<Test>::insert(NetUid::from(raw_netuid), true);
        }

        let hotkey = U256::from(1002);
        let escrow = SubtensorModule::get_beta_escrow_account_id();
        assert!(SubtensorModule::root_claim_fits_declared_budget(&[hotkey]));

        // One hotkey plus 255 raw basket rows exactly fills the 256-unit envelope.
        for raw_netuid in 1..MAX_ROOT_CLAIM_WORK as u16 {
            AlphaV2::<Test>::insert(
                (hotkey, escrow, NetUid::from(raw_netuid)),
                share_pool::SafeFloat::from(1_u64),
            );
        }
        assert!(SubtensorModule::root_claim_fits_declared_budget(&[hotkey]));

        AlphaV2::<Test>::insert(
            (hotkey, escrow, NetUid::from(MAX_ROOT_CLAIM_WORK as u16)),
            share_pool::SafeFloat::from(1_u64),
        );
        assert!(!SubtensorModule::root_claim_fits_declared_budget(&[hotkey]));

        // The single-hotkey gate must use the 129-unit cap, not the 256-unit
        // coldkey-wide envelope. A 130-row basket (1 hotkey + 129 rows) would
        // otherwise be admitted under a 129-unit declaration.
        let single = U256::from(1003);
        for raw_netuid in 1..MAX_ROOT_CLAIM_HOTKEY_WORK as u16 {
            AlphaV2::<Test>::insert(
                (single, escrow, NetUid::from(raw_netuid)),
                share_pool::SafeFloat::from(1_u64),
            );
        }
        assert!(SubtensorModule::root_claim_hotkey_fits_declared_budget(
            &single
        ));
        AlphaV2::<Test>::insert(
            (
                single,
                escrow,
                NetUid::from(MAX_ROOT_CLAIM_HOTKEY_WORK as u16),
            ),
            share_pool::SafeFloat::from(1_u64),
        );
        assert!(!SubtensorModule::root_claim_hotkey_fits_declared_budget(
            &single
        ));
        assert_storage_noop!(assert_err_ignore_postinfo!(
            SubtensorModule::claim_root_with_hotkey(
                RuntimeOrigin::signed(U256::from(1001)),
                single
            ),
            Error::<Test>::RootClaimTooHeavy
        ));
    });
}

#[test]
fn test_coldkey_wide_claim_selects_only_root_relevant_hotkeys() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1001);
        let root_hotkey = U256::from(1002);
        let subnet_hotkey = U256::from(1003);
        let outstanding_hotkey = U256::from(1004);
        let stale_hotkey = U256::from(1005);
        let subnet = add_dynamic_network(&subnet_hotkey, &coldkey);

        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &root_hotkey,
            &coldkey,
            NetUid::ROOT,
            1_u64.into(),
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &subnet_hotkey,
            &coldkey,
            subnet,
            1_u64.into(),
        );
        BasketClaimed::<Test>::insert(outstanding_hotkey, coldkey, -1);
        StakingHotkeys::<Test>::insert(
            coldkey,
            vec![root_hotkey, subnet_hotkey, outstanding_hotkey, stale_hotkey],
        );

        assert_eq!(
            SubtensorModule::root_claim_hotkeys(&coldkey, StakingHotkeys::<Test>::get(coldkey)),
            vec![root_hotkey, outstanding_hotkey]
        );
        assert_ok!(SubtensorModule::claim_root(
            RuntimeOrigin::signed(coldkey),
            BTreeSet::new()
        ));
    });
}

// =============================================================================
// Beta basket: accrual
// =============================================================================

#[test]
fn test_root_basket_accrues_dividend_as_fund_shares() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let coldkey = U256::from(1003);
        let netuid = add_dynamic_network(&hotkey, &owner_coldkey);
        remove_owner_registration_stake(netuid);
        fund_pool(netuid);

        SubtensorModule::set_tao_weight(u64::MAX); // tao_weight = 1.0

        let root_stake = 2_000_000u64;
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            NetUid::ROOT,
            root_stake.into(),
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &owner_coldkey,
            netuid,
            10_000_000u64.into(),
        );

        // Root-registered so the epoch pays this hotkey root dividends.
        register_on_root(&hotkey, 0);

        assert_eq!(escrow_alpha(&hotkey, netuid), 0);
        assert_eq!(fund_shares(&hotkey), 0);

        let pending_root_alpha = 1_000_000u64;
        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            pending_root_alpha.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();

        // Fund shares minted, escrow holds the basket alpha, and a claimable rate exists.
        assert!(fund_shares(&hotkey) > 0);
        assert!(escrow_alpha(&hotkey, netuid) > 0);
        assert!(has_fund(&hotkey));

        // At a ~1:1 pool price the fund NAV and outstanding shares should match (N/P starts
        // at 1). NAV is a realizable (slippage-aware, fee-included) quote, so it sits at or
        // slightly below the TAO-denominated shares minted at deposit.
        let nav = SubtensorModule::get_validator_basket_nav_tao(&hotkey).to_u64();
        let shares = fund_shares(&hotkey);
        assert!(
            nav <= shares,
            "realizable NAV must not exceed shares at N/P=1"
        );
        assert_abs_diff_eq!(nav, shares, epsilon = shares / 100);
    });
}

#[test]
fn test_root_basket_accumulates_in_place_without_weights() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let coldkey = U256::from(1003);
        let netuid = add_dynamic_network(&hotkey, &owner_coldkey);
        remove_owner_registration_stake(netuid);
        fund_pool(netuid);

        SubtensorModule::set_tao_weight(u64::MAX);

        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            NetUid::ROOT,
            2_000_000u64.into(),
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &owner_coldkey,
            netuid,
            10_000_000u64.into(),
        );

        // Root-registered (required to earn root dividends) but with no weights set: the
        // fund is uncurated — the dividend accumulates in place on its origin subnet,
        // trade-free (no sell, no redeploy, pool untouched).
        register_on_root(&hotkey, 0);
        let pool_tao_before = SubnetTAO::<Test>::get(netuid);
        let pool_alpha_in_before = SubnetAlphaIn::<Test>::get(netuid);
        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();

        let shares = fund_shares(&hotkey);
        assert!(shares > 0, "fund shares must be minted");
        assert!(has_fund(&hotkey));
        assert!(
            escrow_alpha(&hotkey, netuid) > 0,
            "accumulate must credit the origin-subnet holding"
        );
        assert_eq!(
            SubnetTAO::<Test>::get(netuid),
            pool_tao_before,
            "accumulate must not move TAO through the pool"
        );
        assert_eq!(
            SubnetAlphaIn::<Test>::get(netuid),
            pool_alpha_in_before,
            "accumulate must not sell alpha into the pool"
        );
        assert!(
            SubtensorModule::get_basket_owed_shares(&hotkey, &coldkey) > 0,
            "staker must accrue fund shares"
        );
    });
}

#[test]
fn test_subnet_owner_root_validator_dividend_is_basketed_and_claimable() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1101);
        let owner_hotkey = U256::from(1102);
        let netuid = add_dynamic_network(&owner_hotkey, &owner_coldkey);
        remove_owner_registration_stake(netuid);
        fund_pool(netuid);
        zero_claim_threshold();

        // Model the reported identity collision explicitly: this single hotkey owns the
        // subnet, is a neuron on that subnet, and is also a root validator. Owner immunity
        // applies to its miner incentive, but must not apply to its Root Reborn dividend.
        register_on_root(&owner_hotkey, 0);
        assert_eq!(SubnetOwnerHotkey::<Test>::get(netuid), owner_hotkey);
        assert!(Uids::<Test>::contains_key(netuid, owner_hotkey));
        assert!(Uids::<Test>::contains_key(NetUid::ROOT, owner_hotkey));

        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &owner_hotkey,
            &owner_coldkey,
            NetUid::ROOT,
            2_000_000u64.into(),
        );

        // Use zero validator take so the complete Root Reborn dividend belongs to the
        // root staker and the basket-side accounting can be asserted exactly.
        crate::Delegates::<Test>::insert(owner_hotkey, sp_runtime::PerU16::from_parts(0));

        let miner_incentive = 100_000u64;
        let root_reborn_dividend = 1_000_000u64;
        let issued =
            SubtensorModule::mint_alpha(netuid, (miner_incentive + root_reborn_dividend).into());
        SubtensorModule::resolve_to_alpha_out(issued);

        let burned_before = pallet_alpha_assets::AlphaBurned::<Test>::get(netuid);
        let recycled_before = pallet_alpha_assets::AlphaRecycled::<Test>::get(netuid);

        let mut incentives = alloc::collections::BTreeMap::new();
        incentives.insert(owner_hotkey, miner_incentive.into());
        let mut root_dividends = alloc::collections::BTreeMap::new();
        root_dividends.insert(owner_hotkey, U96F32::from_num(root_reborn_dividend));

        SubtensorModule::distribute_dividends_and_incentives(
            netuid,
            AlphaBalance::ZERO,
            incentives,
            alloc::collections::BTreeMap::new(),
            root_dividends,
        );

        // This is the distinction the bug report misses: the owner-directed miner incentive
        // is burned by the owner-immunity rule, while the Root Reborn dividend is preserved
        // verbatim in the pending basket queue.
        assert_eq!(
            pallet_alpha_assets::AlphaBurned::<Test>::get(netuid),
            burned_before.saturating_add(miner_incentive.into())
        );
        assert_eq!(
            pallet_alpha_assets::AlphaRecycled::<Test>::get(netuid),
            recycled_before
        );
        assert_eq!(
            RootAlphaDividendsPerSubnet::<Test>::get(netuid, owner_hotkey),
            root_reborn_dividend.into()
        );
        assert_eq!(
            PendingBasketDeposits::<Test>::get(owner_hotkey, netuid),
            root_reborn_dividend.into()
        );

        flush_baskets();

        assert_eq!(
            PendingBasketDeposits::<Test>::get(owner_hotkey, netuid),
            AlphaBalance::ZERO
        );
        assert_eq!(
            escrow_alpha(&owner_hotkey, netuid),
            root_reborn_dividend,
            "the owner's Root Reborn dividend must enter basket custody, not burn"
        );
        assert!(fund_shares(&owner_hotkey) > 0);
        assert!(
            SubtensorModule::get_basket_owed_shares(&owner_hotkey, &owner_coldkey) > 0,
            "the owner coldkey must receive a claimable basket entitlement"
        );

        let root_stake_before = root_stake_of(&owner_hotkey, &owner_coldkey);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(owner_coldkey),
            owner_hotkey
        ));
        assert!(root_stake_of(&owner_hotkey, &owner_coldkey) > root_stake_before);
        assert!(BasketRedeemedTao::<Test>::get(owner_hotkey) > 0.into());
        assert_eq!(
            pallet_alpha_assets::AlphaBurned::<Test>::get(netuid),
            burned_before.saturating_add(miner_incentive.into()),
            "claiming the Root Reborn dividend must not add to burned alpha"
        );
    });
}

// =============================================================================
// Beta basket: protocol-flow accounting (symmetric)
// =============================================================================

/// Protocol flow is booked symmetrically and only when TAO actually crosses a pool: an
/// in-place dividend records nothing, a direct deposit's buy is an inflow, and the claim
/// sell is an outflow that nets the deposit-then-claim round-trip back toward zero.
#[test]
fn test_root_basket_records_symmetric_protocol_flow() {
    new_test_ext(1).execute_with(|| {
        let owner_a = U256::from(1001);
        let hotkey = U256::from(1002);
        let coldkey = U256::from(1003);
        let bob = U256::from(1004);

        let netuid_a = add_dynamic_network(&hotkey, &owner_a);
        remove_owner_registration_stake(netuid_a);
        fund_pool(netuid_a);

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
        assert_eq!(SubnetProtocolFlow::<Test>::get(netuid_a), 0);

        // An in-place dividend moves no TAO through the pool: nothing is booked.
        SubtensorModule::distribute_emission(
            netuid_a,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();
        assert!(escrow_alpha(&hotkey, netuid_a) > 0);
        assert_eq!(
            SubnetProtocolFlow::<Test>::get(netuid_a),
            0,
            "accumulating in place must not book protocol flow"
        );

        // A direct deposit mirrors the holding: its buy on A is an inflow.
        let deposit = 5_000_000u64;
        add_balance_to_coldkey_account(&bob, TaoBalance::from(2 * deposit));
        assert_ok!(SubtensorModule::do_stake_into_basket(
            bob,
            hotkey,
            deposit.into(),
        ));
        let flow_in = SubnetProtocolFlow::<Test>::get(netuid_a);
        assert!(flow_in > 0, "buy on A must be an inflow, got {flow_in}");

        // Bob redeems: the claim sells his slice back to TAO, booking an outflow that nets
        // the round trip back toward zero (only fees/slippage remain).
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(bob),
            hotkey
        ));
        let flow_after = SubnetProtocolFlow::<Test>::get(netuid_a);
        assert!(
            flow_after < flow_in,
            "claim must book an outflow on A: {flow_after} !< {flow_in}"
        );
        assert!(
            flow_after.abs() < flow_in,
            "round-trip residual should be smaller than the inflow: {flow_after} vs {flow_in}"
        );
    });
}

// =============================================================================
// Beta basket: claiming (pro-rata fund redemption, swapped to root TAO)
// =============================================================================

/// A claim consolidates dust holdings (realizable value below the claim threshold) into
/// the fund's root slot, deleting the holding row, before redeeming. Dust rows otherwise
/// persist forever: their pro-rata takes floor to zero, yet every claim pays weight per row.
#[test]
fn test_root_claim_consolidates_dust_holdings() {
    new_test_ext(1).execute_with(|| {
        let owner_a = U256::from(1001);
        let owner_b = U256::from(1004);
        let owner_c = U256::from(1006);
        let hotkey = U256::from(1002);
        let hotkey_b = U256::from(1005);
        let hotkey_c = U256::from(1007);
        let coldkey = U256::from(1003);
        let netuid_a = add_dynamic_network(&hotkey, &owner_a);
        let netuid_b = add_dynamic_network(&hotkey_b, &owner_b);
        let netuid_c = add_dynamic_network(&hotkey_c, &owner_c);
        remove_owner_registration_stake(netuid_a);
        fund_pool(netuid_a);
        fund_pool(netuid_b);
        fund_pool(netuid_c);

        SubtensorModule::set_tao_weight(u64::MAX);
        // Dust bar: holdings realizing below 10_000 rao consolidate. The claim payout
        // (~1e6 rao) clears the same threshold, so the claim itself still pays out.
        RootClaimableThreshold::<Test>::insert(NetUid::ROOT, I96F32::from_num(10_000));

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

        // The dividend lands on A: a healthy holding well above the dust bar.
        register_on_root(&hotkey, 0);
        SubtensorModule::distribute_emission(
            netuid_a,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();
        assert!(
            escrow_alpha(&hotkey, netuid_a) > 0,
            "fund must hold A alpha"
        );

        // Plant a dust holding on C (e.g. a position whose value collapsed).
        let escrow = SubtensorModule::get_beta_escrow_account_id();
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &escrow,
            netuid_c,
            1_000u64.into(),
        );

        let stake_before = root_stake_of(&hotkey, &coldkey);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(coldkey),
            hotkey
        ));

        assert_eq!(
            escrow_alpha(&hotkey, netuid_c),
            0,
            "dust holding must be consolidated away"
        );
        assert!(
            root_stake_of(&hotkey, &coldkey) > stake_before,
            "claim must still pay out"
        );
    });
}

/// A below-threshold claim is a no-op for redemption but still consolidates every dust
/// holding (there is no exempt set: a row that cannot pay any claimant is cashed) and is
/// charged for the swept rows plus a scan, not as a full per-row claim.
#[test]
fn test_root_claim_noop_below_threshold_costs_scan_and_sweeps_dust() {
    new_test_ext(1).execute_with(|| {
        let owner_a = U256::from(1001);
        let owner_c = U256::from(1006);
        let hotkey = U256::from(1002);
        let hotkey_c = U256::from(1007);
        let coldkey = U256::from(1003);
        let netuid_a = add_dynamic_network(&hotkey, &owner_a);
        let netuid_c = add_dynamic_network(&hotkey_c, &owner_c);
        remove_owner_registration_stake(netuid_a);
        fund_pool(netuid_a);
        fund_pool(netuid_c);

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
        assert!(
            escrow_alpha(&hotkey, netuid_a) > 0,
            "fund must hold A alpha"
        );
        assert_eq!(escrow_alpha(&hotkey, NetUid::ROOT), 0);

        // Plant a dust holding on C.
        let escrow = SubtensorModule::get_beta_escrow_account_id();
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &escrow,
            netuid_c,
            1_000u64.into(),
        );

        // Threshold above the whole fund NAV: the claim no-ops, and both subnet rows —
        // the C dust and the A holding, each realizably below the bar — are cashed.
        RootClaimableThreshold::<Test>::insert(
            NetUid::ROOT,
            I96F32::from_num(MAX_ROOT_CLAIM_THRESHOLD),
        );

        let shares_before = fund_shares(&hotkey);
        let stake_before = root_stake_of(&hotkey, &coldkey);
        let nav_before = SubtensorModule::get_validator_basket_nav_tao(&hotkey).to_u64();

        let post = SubtensorModule::claim_root_with_hotkey(RuntimeOrigin::signed(coldkey), hotkey)
            .expect("no-op claim succeeds");

        // No redemption happened...
        assert_eq!(fund_shares(&hotkey), shares_before, "shares untouched");
        assert_eq!(
            root_stake_of(&hotkey, &coldkey),
            stake_before,
            "no payout below threshold"
        );
        // ...every dust row was consolidated into the root slot, NAV-continuous (minus
        // slippage on sub-threshold amounts).
        assert_eq!(escrow_alpha(&hotkey, netuid_c), 0, "C dust swept");
        assert_eq!(escrow_alpha(&hotkey, netuid_a), 0, "A dust swept");
        let root_slot = escrow_alpha(&hotkey, NetUid::ROOT);
        assert!(
            root_slot > 0,
            "swept value held as the fund's root (TAO) slot"
        );
        assert!(root_slot <= nav_before);
        assert!(root_slot >= nav_before * 99 / 100);

        // Charged as two active units (the swept rows) plus a one-row scan (the root
        // slot) — not the full per-row claim weight.
        let actual = post
            .actual_weight
            .expect("claim reports benchmark-derived actual weight");
        let expected = <Test as crate::Config>::WeightInfo::claim_root(2)
            .saturating_add(<Test as crate::Config>::WeightInfo::claim_root_scan(1));
        assert_eq!(actual, expected);
    });
}

#[test]
fn test_root_basket_claim_swaps_to_root() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let coldkey = U256::from(1003);
        let netuid = add_dynamic_network(&hotkey, &owner_coldkey);
        remove_owner_registration_stake(netuid);
        fund_pool(netuid);

        SubtensorModule::set_tao_weight(u64::MAX);
        zero_claim_threshold();

        let root_stake = 2_000_000u64;
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            NetUid::ROOT,
            root_stake.into(),
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &owner_coldkey,
            netuid,
            10_000_000u64.into(),
        );

        register_on_root(&hotkey, 0);

        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();

        let shares_before = fund_shares(&hotkey);
        assert!(shares_before > 0);
        let root_before = root_stake_of(&hotkey, &coldkey);
        assert_eq!(root_before, root_stake);

        // Claim: the staker's owed fraction of the fund is sold to TAO and staked on root.
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(coldkey),
            hotkey
        ));

        // Staker's root stake increased, fund shares consumed, watermark advanced.
        assert!(root_stake_of(&hotkey, &coldkey) > root_before);
        assert!(fund_shares(&hotkey) < shares_before);
        assert!(BasketClaimed::<Test>::get(hotkey, coldkey) > 0);
    });
}

#[test]
fn test_root_basket_proportional_two_stakers() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let alice = U256::from(1003);
        let bob = U256::from(1004);
        let netuid = add_dynamic_network(&hotkey, &owner_coldkey);
        remove_owner_registration_stake(netuid);
        fund_pool(netuid);

        SubtensorModule::set_tao_weight(u64::MAX);
        zero_claim_threshold();

        // Equal root stake for both stakers.
        let root_stake = 1_000_000u64;
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &alice,
            NetUid::ROOT,
            root_stake.into(),
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &bob,
            NetUid::ROOT,
            root_stake.into(),
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &owner_coldkey,
            netuid,
            10_000_000u64.into(),
        );

        register_on_root(&hotkey, 0);

        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            10_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();

        let alice_before = root_stake_of(&hotkey, &alice);
        let bob_before = root_stake_of(&hotkey, &bob);

        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(alice),
            hotkey
        ));
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(bob),
            hotkey
        ));

        let alice_gain = root_stake_of(&hotkey, &alice).saturating_sub(alice_before);
        let bob_gain = root_stake_of(&hotkey, &bob).saturating_sub(bob_before);

        assert!(alice_gain > 0);
        // Equal root stake => equal basket payout (small AMM slippage between the two
        // sequential claims on the same pool).
        assert_abs_diff_eq!(alice_gain, bob_gain, epsilon = 1_000u64);
    });
}

#[test]
fn test_claim_root_targets_one_hotkey_only() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hot_a = U256::from(1002);
        let hot_b = U256::from(1003);
        let coldkey = U256::from(1004);
        let netuid_a = add_dynamic_network(&hot_a, &owner_coldkey);
        let netuid_b = add_dynamic_network(&hot_b, &owner_coldkey);

        fund_pool(netuid_a);
        fund_pool(netuid_b);
        zero_claim_threshold();
        SubtensorModule::set_tao_weight(u64::MAX);

        let root_stake = 2_000_000u64;
        for hotkey in [&hot_a, &hot_b] {
            mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
                hotkey,
                &coldkey,
                NetUid::ROOT,
                root_stake.into(),
            );
        }
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hot_a,
            &owner_coldkey,
            netuid_a,
            10_000_000u64.into(),
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hot_b,
            &owner_coldkey,
            netuid_b,
            10_000_000u64.into(),
        );

        register_on_root(&hot_a, 0);
        register_on_root(&hot_b, 1);

        SubtensorModule::distribute_emission(
            netuid_a,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            5_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        SubtensorModule::distribute_emission(
            netuid_b,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            5_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();

        let owed_a = SubtensorModule::get_basket_owed_shares(&hot_a, &coldkey);
        let owed_b = SubtensorModule::get_basket_owed_shares(&hot_b, &coldkey);
        assert!(owed_a > 0, "hot_a should have accrued yield");
        assert!(owed_b > 0, "hot_b should have accrued yield");

        let root_a_before = root_stake_of(&hot_a, &coldkey);
        let root_b_before = root_stake_of(&hot_b, &coldkey);

        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(coldkey),
            hot_a
        ));

        let owed_a_after = SubtensorModule::get_basket_owed_shares(&hot_a, &coldkey);
        assert!(
            owed_a_after < owed_a && owed_a_after <= 1,
            "claimed hotkey must be drained (owed {owed_a} -> {owed_a_after})"
        );
        assert_eq!(
            SubtensorModule::get_basket_owed_shares(&hot_b, &coldkey),
            owed_b,
            "other hotkey must remain claimable"
        );
        assert!(root_stake_of(&hot_a, &coldkey) > root_a_before);
        assert_eq!(root_stake_of(&hot_b, &coldkey), root_b_before);
    });
}

// =============================================================================
// Beta basket: hotkey swap migration
// =============================================================================

#[test]
fn test_root_basket_hotkey_swap_migrates() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let coldkey = U256::from(1003);
        let new_hotkey = U256::from(10030);
        let netuid = add_dynamic_network(&hotkey, &owner_coldkey);
        remove_owner_registration_stake(netuid);
        fund_pool(netuid);

        SubtensorModule::set_tao_weight(u64::MAX);

        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            NetUid::ROOT,
            2_000_000u64.into(),
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &owner_coldkey,
            netuid,
            10_000_000u64.into(),
        );

        register_on_root(&hotkey, 0);

        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();

        let basket_before = escrow_alpha(&hotkey, netuid);
        let shares_before = fund_shares(&hotkey);
        assert!(basket_before > 0);
        assert!(shares_before > 0);

        // Swap the validator's root hotkey: the whole fund must follow it.
        let mut weight = Weight::zero();
        assert_ok!(SubtensorModule::perform_hotkey_swap_on_one_subnet(
            &hotkey,
            &new_hotkey,
            &mut weight,
            NetUid::ROOT,
            false,
        ));

        // Fund moved to the new hotkey, old fund emptied.
        assert_eq!(escrow_alpha(&hotkey, netuid), 0);
        assert_eq!(fund_shares(&hotkey), 0);
        assert!(!has_fund(&hotkey));
        assert_abs_diff_eq!(
            escrow_alpha(&new_hotkey, netuid),
            basket_before,
            epsilon = 10u64
        );
        assert_eq!(fund_shares(&new_hotkey), shares_before);
        assert!(has_fund(&new_hotkey));
    });
}

// =============================================================================
// Beta basket: subnet dissolution converts the holding into the fund's root slot
// =============================================================================

#[test]
fn test_root_basket_dissolve_converts_to_root_slot() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let coldkey = U256::from(1003);
        let netuid = add_dynamic_network(&hotkey, &owner_coldkey);
        remove_owner_registration_stake(netuid);
        fund_pool(netuid);

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
            &owner_coldkey,
            netuid,
            10_000_000u64.into(),
        );

        register_on_root(&hotkey, 0);

        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();

        let subnet_holding = escrow_alpha(&hotkey, netuid);
        assert!(subnet_holding > 0);
        assert_eq!(escrow_alpha(&hotkey, NetUid::ROOT), 0);
        let shares_before = fund_shares(&hotkey);

        // Dissolving queues the subnet; metered cleanup converts the holding into the fund's
        // root (TAO) slot. Shares, rates, and watermarks are untouched — NAV continuous minus slippage.
        // Sync the mock to the live-reserve invariant. This is deliberately the only funded
        // dynamic subnet: removing it takes `TotalStake` to zero, exercising the saturating
        // subtraction in the subsequent basket sale.
        let sync_total_stake = || {
            let live: u64 = NetworksAdded::<Test>::iter()
                .filter(|(_, added)| *added)
                .map(|(n, _)| SubnetTAO::<Test>::get(n).to_u64())
                .sum();
            live
        };
        TotalStake::<Test>::put(TaoBalance::from(sync_total_stake()));

        assert_ok!(SubtensorModule::do_dissolve_network(netuid));
        assert_eq!(TotalStake::<Test>::get(), TaoBalance::ZERO);
        run_block_idle();

        // The dissolved subnet's TAO already left `TotalStake` when the network was removed;
        // selling the fund's holding on it and crediting the fund's root slot must not take
        // it out a second time (finney block 9111229, subnet 108: -26.5 TAO of drift).
        assert_eq!(
            TotalStake::<Test>::get().to_u64(),
            sync_total_stake(),
            "TotalStake must equal the sum of live SubnetTAO after a dissolution converts basket holdings"
        );

        assert_eq!(escrow_alpha(&hotkey, netuid), 0);
        assert!(
            escrow_alpha(&hotkey, NetUid::ROOT) > 0,
            "holding must be converted to the fund's root slot"
        );
        assert_eq!(fund_shares(&hotkey), shares_before);
        assert!(has_fund(&hotkey));

        // The staker's claim survives dissolution and is redeemable from the root slot.
        let root_before = root_stake_of(&hotkey, &coldkey);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(coldkey),
            hotkey
        ));
        assert!(root_stake_of(&hotkey, &coldkey) > root_before);
    });
}

/// Dissolution must not create a windfall for a "fresh" staker who joined after the basket
/// accrued (zero owed): after conversion, only the staker who accrued the fund can redeem it.
#[test]
fn test_root_basket_dissolve_preserves_owed_not_stake() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let alice = U256::from(1003);
        let bob = U256::from(1004);
        let netuid = add_dynamic_network(&hotkey, &owner_coldkey);
        remove_owner_registration_stake(netuid);
        fund_pool(netuid);

        SubtensorModule::set_tao_weight(u64::MAX);
        zero_claim_threshold();

        // Alice is the sole root staker while the basket accrues — she funds all of it.
        let stake = 2_000_000u64;
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &alice,
            NetUid::ROOT,
            stake.into(),
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &owner_coldkey,
            netuid,
            10_000_000u64.into(),
        );
        register_on_root(&hotkey, 0);

        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();
        assert!(escrow_alpha(&hotkey, netuid) > 0);

        // Bob joins AFTER accrual with the SAME root stake; his watermark is rebased exactly as
        // real `add_stake` would, so his owed entitlement is zero.
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &bob,
            NetUid::ROOT,
            stake.into(),
        );
        SubtensorModule::add_stake_adjust_root_claimed_for_hotkey_and_coldkey(&hotkey, &bob, stake);

        // Equal current root stake, but only Alice is owed the fund.
        assert_eq!(root_stake_of(&hotkey, &alice), root_stake_of(&hotkey, &bob));
        assert!(SubtensorModule::get_basket_owed_shares(&hotkey, &alice) > 0);
        assert_eq!(SubtensorModule::get_basket_owed_shares(&hotkey, &bob), 0);

        assert_ok!(SubtensorModule::do_dissolve_network(netuid));
        run_block_idle();

        // Owed entitlements are untouched by the conversion.
        assert!(SubtensorModule::get_basket_owed_shares(&hotkey, &alice) > 0);
        assert_eq!(SubtensorModule::get_basket_owed_shares(&hotkey, &bob), 0);

        let alice_before = root_stake_of(&hotkey, &alice);
        let bob_before = root_stake_of(&hotkey, &bob);

        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(alice),
            hotkey
        ));
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(bob),
            hotkey
        ));

        let alice_gain = root_stake_of(&hotkey, &alice).saturating_sub(alice_before);
        let bob_gain = root_stake_of(&hotkey, &bob).saturating_sub(bob_before);

        // The fund goes to Alice (who accrued it); Bob (zero owed) gets nothing — even though
        // a stake-proportional split would have handed him ~half.
        assert!(alice_gain > 0, "accruing staker must receive the basket");
        assert_eq!(
            bob_gain, 0,
            "fresh staker with zero owed must receive nothing"
        );
    });
}

// =============================================================================
// Beta basket: conservation invariants ("prove it works")
// =============================================================================

/// TotalStake (the global TAO ledger) must be neutral across both basket distribution
/// (sell origin alpha -> rebuy across w) and redemption (swap basket -> TAO on root):
/// no TAO is minted or destroyed by the round trips.
#[test]
fn test_root_basket_total_stake_conserved() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let coldkey = U256::from(1003);
        let netuid = add_dynamic_network(&hotkey, &owner_coldkey);
        remove_owner_registration_stake(netuid);
        fund_pool(netuid);

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
            &owner_coldkey,
            netuid,
            10_000_000u64.into(),
        );
        register_on_root(&hotkey, 0);

        // --- Distribution must not move TotalStake (sell + rebuy is TAO-neutral).
        let ts_before_distribute = TotalStake::<Test>::get().to_u64();
        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();
        let ts_after_distribute = TotalStake::<Test>::get().to_u64();
        assert_eq!(
            ts_before_distribute, ts_after_distribute,
            "distribution must be TotalStake-neutral"
        );

        // --- Redemption must also be TotalStake-neutral (swap out then stake on root).
        let ts_before_claim = TotalStake::<Test>::get().to_u64();
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(coldkey),
            hotkey
        ));
        let ts_after_claim = TotalStake::<Test>::get().to_u64();
        assert_eq!(
            ts_before_claim, ts_after_claim,
            "redemption must be TotalStake-neutral"
        );
    });
}

/// The basket compounds: if the escrow position grows (validator earns more on the subnet)
/// after accrual, a sole staker redeems MORE than the fund's original NAV — the `N/P`
/// multiplier carries the growth through to the staker.
#[test]
fn test_root_basket_compounds_when_escrow_grows() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let coldkey = U256::from(1003);
        let netuid = add_dynamic_network(&hotkey, &owner_coldkey);
        remove_owner_registration_stake(netuid);
        fund_pool(netuid);

        SubtensorModule::set_tao_weight(u64::MAX);
        zero_claim_threshold();

        // Single root staker => owns 100% of the basket.
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            NetUid::ROOT,
            2_000_000u64.into(),
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &owner_coldkey,
            netuid,
            10_000_000u64.into(),
        );
        register_on_root(&hotkey, 0);

        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();

        let shares = fund_shares(&hotkey);
        let escrow_before = escrow_alpha(&hotkey, netuid);
        assert!(shares > 0);

        // Validator earns more nominator dividends on the subnet => escrow value grows,
        // shares stay fixed (N/P rises above 1).
        SubtensorModule::increase_stake_for_hotkey_on_subnet(
            &hotkey,
            netuid,
            100_000_000u64.into(),
        );
        let escrow_after = escrow_alpha(&hotkey, netuid);
        assert!(
            escrow_after > escrow_before,
            "escrow must grow with dividends"
        );

        let root_before = root_stake_of(&hotkey, &coldkey);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(coldkey),
            hotkey
        ));
        let gain = root_stake_of(&hotkey, &coldkey).saturating_sub(root_before);

        // The sole staker realizes the *grown* basket, strictly more than the original shares'
        // par value.
        assert!(
            gain > shares,
            "compounding: realized {gain} must exceed original share value {shares}"
        );
    });
}

/// Claiming drains the basket exactly: after all stakers redeem, the escrow position and the
/// outstanding fund shares both go to ~zero (Σ payouts == fund value; no residual,
/// no over-draw).
#[test]
fn test_root_basket_fully_drains_on_claims() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let alice = U256::from(1003);
        let bob = U256::from(1004);
        let netuid = add_dynamic_network(&hotkey, &owner_coldkey);
        remove_owner_registration_stake(netuid);
        fund_pool(netuid);

        SubtensorModule::set_tao_weight(u64::MAX);
        zero_claim_threshold();

        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &alice,
            NetUid::ROOT,
            1_000_000u64.into(),
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &bob,
            NetUid::ROOT,
            3_000_000u64.into(),
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &owner_coldkey,
            netuid,
            10_000_000u64.into(),
        );
        register_on_root(&hotkey, 0);

        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            10_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();

        let escrow_filled = escrow_alpha(&hotkey, netuid);
        assert!(escrow_filled > 0);

        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(alice),
            hotkey
        ));
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(bob),
            hotkey
        ));

        // Escrow and shares fully drained (allow tiny rounding dust).
        assert!(
            escrow_alpha(&hotkey, netuid) <= 10,
            "escrow must be drained, got {}",
            escrow_alpha(&hotkey, netuid)
        );
        assert!(
            fund_shares(&hotkey) <= 10,
            "shares must be drained, got {}",
            fund_shares(&hotkey)
        );
    });
}

/// Disproportionate root stake yields proportionate payout: a staker with 2x the root stake
/// redeems ~2x the TAO.
#[test]
fn test_root_basket_disproportional_two_stakers() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let alice = U256::from(1003);
        let bob = U256::from(1004);
        let netuid = add_dynamic_network(&hotkey, &owner_coldkey);
        remove_owner_registration_stake(netuid);
        fund_pool(netuid);

        SubtensorModule::set_tao_weight(u64::MAX);
        zero_claim_threshold();

        // Bob has 2x Alice's root stake.
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &alice,
            NetUid::ROOT,
            1_000_000u64.into(),
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &bob,
            NetUid::ROOT,
            2_000_000u64.into(),
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &owner_coldkey,
            netuid,
            10_000_000u64.into(),
        );
        register_on_root(&hotkey, 0);

        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            10_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();

        let alice_before = root_stake_of(&hotkey, &alice);
        let bob_before = root_stake_of(&hotkey, &bob);

        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(alice),
            hotkey
        ));
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(bob),
            hotkey
        ));

        let alice_gain = root_stake_of(&hotkey, &alice).saturating_sub(alice_before);
        let bob_gain = root_stake_of(&hotkey, &bob).saturating_sub(bob_before);

        assert!(alice_gain > 0);
        // Bob staked 2x => ~2x payout (small AMM slippage between sequential claims).
        assert_abs_diff_eq!(bob_gain, 2 * alice_gain, epsilon = 2_000u64);
    });
}

/// Dividends from several subnets each stay on the subnet they were earned on: a fund's
/// composition is the emission-weighted portfolio its dividends describe, with no protocol
/// trade and no other subnet bought.
#[test]
fn test_root_basket_dividends_accumulate_on_their_origins() {
    new_test_ext(1).execute_with(|| {
        let owner_a = U256::from(1001);
        let hotkey = U256::from(1002);
        let coldkey = U256::from(1003);
        let owner_b = U256::from(2001);
        let hotkey_b = U256::from(2002);
        let owner_c = U256::from(3001);
        let hotkey_c = U256::from(3002);

        let netuid_a = add_dynamic_network(&hotkey, &owner_a);
        let netuid_b = add_dynamic_network(&hotkey_b, &owner_b);
        let netuid_c = add_dynamic_network(&hotkey_c, &owner_c);
        remove_owner_registration_stake(netuid_a);
        fund_pool(netuid_a);
        fund_pool(netuid_b);
        fund_pool(netuid_c);

        SubtensorModule::set_tao_weight(u64::MAX);

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

        let pool_c_before = (
            SubnetTAO::<Test>::get(netuid_c),
            SubnetAlphaIn::<Test>::get(netuid_c),
        );
        // Dividends earned on A and on B (the fund's hotkey is the origin's dividend
        // earner through `distribute_emission` in the mock).
        SubtensorModule::distribute_emission(
            netuid_a,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            10_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();

        assert!(
            escrow_alpha(&hotkey, netuid_a) > 0,
            "origin holds the dividend"
        );
        assert_eq!(
            escrow_alpha(&hotkey, netuid_b),
            0,
            "no other subnet is bought"
        );
        assert_eq!(
            escrow_alpha(&hotkey, netuid_c),
            0,
            "no other subnet is bought"
        );
        assert_eq!(
            escrow_alpha(&hotkey, NetUid::ROOT),
            0,
            "nothing is sold to cash"
        );
        assert_eq!(
            (
                SubnetTAO::<Test>::get(netuid_c),
                SubnetAlphaIn::<Test>::get(netuid_c),
            ),
            pool_c_before,
            "an unrelated pool is untouched"
        );
        assert!(fund_shares(&hotkey) > 0);
    });
}

/// Inflows never change composition. A fund holding B:C = 3:1 receives a dividend on A and
/// a direct deposit; afterwards B:C is still 3:1 (the dividend sits on A as a new holding
/// and the deposit mirrored every holding). Only `swap_basket` moves the ratio.
#[test]
fn test_root_basket_composition_unchanged_by_inflows() {
    new_test_ext(1).execute_with(|| {
        let owner_a = U256::from(1001);
        let hotkey = U256::from(1002);
        let coldkey = U256::from(1003);
        let owner_b = U256::from(2001);
        let hotkey_b = U256::from(2002);
        let owner_c = U256::from(3001);
        let hotkey_c = U256::from(3002);

        let netuid_a = add_dynamic_network(&hotkey, &owner_a);
        let netuid_b = add_dynamic_network(&hotkey_b, &owner_b);
        let netuid_c = add_dynamic_network(&hotkey_c, &owner_c);
        remove_owner_registration_stake(netuid_a);
        for netuid in [netuid_a, netuid_b, netuid_c] {
            fund_pool(netuid);
        }

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

        // The fund holds B:C = 3:1 (equal-depth pools, price ~1).
        let escrow = SubtensorModule::get_beta_escrow_account_id();
        let held_b = 3_000_000u64;
        let held_c = 1_000_000u64;
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &escrow,
            netuid_b,
            held_b.into(),
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &escrow,
            netuid_c,
            held_c.into(),
        );
        BasketShares::<Test>::insert(hotkey, held_b + held_c);
        let ratio_bps =
            |hk: &U256| escrow_alpha(hk, netuid_b) * 10_000 / escrow_alpha(hk, netuid_c);
        let target_bps = held_b * 10_000 / held_c;

        // A dividend earned on A: lands on A, B and C untouched.
        SubtensorModule::distribute_emission(
            netuid_a,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            4_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();
        assert!(escrow_alpha(&hotkey, netuid_a) > 0);
        assert_eq!(escrow_alpha(&hotkey, netuid_b), held_b);
        assert_eq!(escrow_alpha(&hotkey, netuid_c), held_c);

        // A direct deposit: mirrors A, B, and C by value, so B:C stays 3:1 (±1%).
        let bob = U256::from(4001);
        add_balance_to_coldkey_account(&bob, TaoBalance::from(20_000_000u64));
        assert_ok!(SubtensorModule::do_stake_into_basket(
            bob,
            hotkey,
            8_000_000u64.into(),
        ));
        assert!(escrow_alpha(&hotkey, netuid_b) > held_b);
        assert!(escrow_alpha(&hotkey, netuid_c) > held_c);
        let after = ratio_bps(&hotkey);
        assert!(
            after.abs_diff(target_bps) <= target_bps / 100,
            "inflows moved composition: {after} bps vs {target_bps}"
        );
    });
}

// =============================================================================
// Claims 1-4: the staker-facing guarantees, proven directly.
// =============================================================================

/// CLAIM 1 — staking principal can never be lost: the basket only ever deploys the validator's
/// dividends, never the staker's root principal. A distribution leaves the staker's root stake
/// untouched, and a claim only ever *adds* to it.
#[test]
fn test_claim1_principal_never_lost() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let coldkey = U256::from(1003);
        let netuid = add_dynamic_network(&hotkey, &owner_coldkey);
        remove_owner_registration_stake(netuid);
        fund_pool(netuid);

        SubtensorModule::set_tao_weight(u64::MAX);
        zero_claim_threshold();

        let principal = 2_000_000u64;
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            NetUid::ROOT,
            principal.into(),
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &owner_coldkey,
            netuid,
            10_000_000u64.into(),
        );
        register_on_root(&hotkey, 0);

        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();

        // Dividend distribution did not touch the staker's root principal.
        assert_eq!(root_stake_of(&hotkey, &coldkey), principal);

        // Claiming only adds TAO to the root principal (never subtracts).
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(coldkey),
            hotkey
        ));
        assert!(root_stake_of(&hotkey, &coldkey) >= principal);
    });
}

/// A pre-execution `get_basket_payout_tao` quote is not the post-claim balance:
/// the claim first flushes pending basket deposits. `move_stake(MAX)` must cap
/// to the live origin after that flush, not to the stale quote.
#[test]
fn test_claim_then_move_max_includes_pending_basket() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let origin_hotkey = U256::from(1002);
        let dest_hotkey = U256::from(1005);
        let coldkey = U256::from(1003);
        let netuid = add_dynamic_network(&origin_hotkey, &owner_coldkey);
        remove_owner_registration_stake(netuid);
        fund_pool(netuid);

        SubtensorModule::set_tao_weight(u64::MAX);
        zero_claim_threshold();

        let principal = 2_000_000u64;
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &origin_hotkey,
            &coldkey,
            NetUid::ROOT,
            principal.into(),
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &origin_hotkey,
            &owner_coldkey,
            netuid,
            10_000_000u64.into(),
        );
        register_on_root(&origin_hotkey, 0);
        NetworksAdded::<Test>::insert(NetUid::ROOT, true);
        crate::SubtokenEnabled::<Test>::insert(NetUid::ROOT, true);
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &dest_hotkey);

        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );

        let quoted_payout = SubtensorModule::get_basket_payout_tao(&origin_hotkey, &coldkey);
        assert_eq!(
            quoted_payout, 0,
            "pending credits must not count in the pre-flush payout quote"
        );
        assert!(
            crate::PendingBasketDeposits::<Test>::iter_prefix(origin_hotkey)
                .next()
                .is_some(),
            "epoch must have queued a pending basket credit"
        );

        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(coldkey),
            origin_hotkey
        ));
        let post_claim = root_stake_of(&origin_hotkey, &coldkey);
        assert!(
            post_claim > principal,
            "claim must realize the flushed pending credit"
        );

        assert_ok!(SubtensorModule::do_move_stake(
            RuntimeOrigin::signed(coldkey),
            origin_hotkey,
            dest_hotkey,
            NetUid::ROOT,
            NetUid::ROOT,
            AlphaBalance::MAX,
        ));

        assert_eq!(root_stake_of(&origin_hotkey, &coldkey), 0);
        assert_eq!(root_stake_of(&dest_hotkey, &coldkey), post_claim);
    });
}

/// CLAIM 2 — accrued beta is unaffected by *others* staking the same validator: another staker
/// joining does not change your already-accrued basket value, and they accrue nothing of yours.
#[test]
fn test_claim2_accrued_basket_unchanged_when_others_stake() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let alice = U256::from(1003);
        let bob = U256::from(1004);
        let netuid = add_dynamic_network(&hotkey, &owner_coldkey);
        remove_owner_registration_stake(netuid);
        fund_pool(netuid);

        SubtensorModule::set_tao_weight(u64::MAX);
        zero_claim_threshold();

        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &alice,
            NetUid::ROOT,
            2_000_000u64.into(),
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &owner_coldkey,
            netuid,
            10_000_000u64.into(),
        );
        register_on_root(&hotkey, 0);

        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();

        let alice_before = SubtensorModule::get_basket_payout_tao(&hotkey, &alice);
        assert!(alice_before > 0);

        // Bob stakes the same validator (no new distribution). The mock stake helper bypasses the
        // root-claimed watermark that the real add_stake applies, so set it explicitly.
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &bob,
            NetUid::ROOT,
            5_000_000u64.into(),
        );
        SubtensorModule::add_stake_adjust_root_claimed_for_hotkey_and_coldkey(
            &hotkey,
            &bob,
            5_000_000u64,
        );

        // Alice's accrued basket is unchanged; Bob has accrued nothing of it.
        assert_eq!(
            SubtensorModule::get_basket_payout_tao(&hotkey, &alice),
            alice_before
        );
        assert_eq!(SubtensorModule::get_basket_payout_tao(&hotkey, &bob), 0);
    });
}

/// CLAIM 3 — earned beta compounds: while it sits staked under the validator it earns the
/// validator's subnet dividends, so the staker's claimable value grows beyond what they earned.
#[test]
fn test_claim3_basket_compounds() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let coldkey = U256::from(1003);
        let netuid = add_dynamic_network(&hotkey, &owner_coldkey);
        remove_owner_registration_stake(netuid);
        fund_pool(netuid);

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
            &owner_coldkey,
            netuid,
            10_000_000u64.into(),
        );
        register_on_root(&hotkey, 0);

        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();

        let before = SubtensorModule::get_basket_payout_tao(&hotkey, &coldkey);
        assert!(before > 0);

        // The validator earns subnet dividends on the basket position (escrow value grows).
        let escrow = SubtensorModule::get_beta_escrow_account_id();
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &escrow,
            netuid,
            before.into(),
        );

        // The sole staker's claimable value compounded upward.
        assert!(SubtensorModule::get_basket_payout_tao(&hotkey, &coldkey) > before);
    });
}

/// CLAIM 4 — a late staker can neither claim the existing basket nor skim its past compounding.
/// Proven two ways: (a) a fresh staker's owed is zero, and (b) a deposit into an already
/// compounded fund leaves the `N/P` multiplier unchanged (deposit-at-NAV), so the late
/// staker only ever earns their fair share of *new* distributions — never the old compounding.
#[test]
fn test_claim4_no_dilution_or_skim_on_late_stake() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let alice = U256::from(1003);
        let bob = U256::from(1004);
        let netuid = add_dynamic_network(&hotkey, &owner_coldkey);
        remove_owner_registration_stake(netuid);
        fund_pool(netuid);

        SubtensorModule::set_tao_weight(u64::MAX);
        zero_claim_threshold();

        // Equal root stake for Alice and Bob.
        let stake = 2_000_000u64;
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &alice,
            NetUid::ROOT,
            stake.into(),
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &owner_coldkey,
            netuid,
            10_000_000u64.into(),
        );
        register_on_root(&hotkey, 0);

        // Alice accrues a basket.
        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();

        // The basket compounds heavily (escrow value grows ~4x; shares unchanged).
        let escrow = SubtensorModule::get_beta_escrow_account_id();
        let e0 = escrow_alpha(&hotkey, netuid);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &escrow,
            netuid,
            (3 * e0).into(),
        );

        let mult = |hk: &U256| -> f64 {
            let n = SubtensorModule::get_validator_basket_nav_tao(hk).to_u64() as f64;
            let p = fund_shares(hk) as f64;
            n / p
        };
        let mult_before = mult(&hotkey);
        assert!(
            mult_before > 3.0,
            "basket should have compounded, got {mult_before}"
        );
        let alice_before = SubtensorModule::get_basket_payout_tao(&hotkey, &alice);

        // Bob stakes the heavily-compounded validator. The mock stake helper bypasses the
        // root-claimed watermark that the real add_stake applies, so set it explicitly.
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &bob,
            NetUid::ROOT,
            stake.into(),
        );
        SubtensorModule::add_stake_adjust_root_claimed_for_hotkey_and_coldkey(&hotkey, &bob, stake);

        // (4a) Bob cannot claim any of the existing basket; Alice's accrual is untouched.
        assert_eq!(SubtensorModule::get_basket_payout_tao(&hotkey, &bob), 0);
        assert_eq!(
            SubtensorModule::get_basket_payout_tao(&hotkey, &alice),
            alice_before
        );

        // A new distribution deposits into the already-compounded basket.
        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();

        // (4b) Deposit-at-NAV: the N/P multiplier is unchanged, so no dilution occurred.
        let mult_after = mult(&hotkey);
        assert_abs_diff_eq!(mult_after, mult_before, epsilon = 0.02);

        let alice_after = SubtensorModule::get_basket_payout_tao(&hotkey, &alice);
        let bob_after = SubtensorModule::get_basket_payout_tao(&hotkey, &bob);

        // Alice was not diluted: her value only grew.
        assert!(alice_after >= alice_before);

        // The new distribution split fairly (equal root stake) — and crucially, Bob's *entire*
        // basket equals only Alice's *increment* from the new distribution. Bob captured none of
        // Alice's pre-existing compounding (alice_before).
        let alice_increment = alice_after.saturating_sub(alice_before);
        assert!(bob_after > 0);
        assert_abs_diff_eq!(alice_increment, bob_after, epsilon = 1_000u64);
        assert!(
            bob_after < alice_before,
            "late staker skimmed past compounding: bob={bob_after} alice_before={alice_before}"
        );
    });
}

/// The read-only views (RPC surface) report the basket correctly: a sole staker's "owed TAO"
/// equals the validator NAV equals the network total, and the breakdown lists the holding.
#[test]
fn test_root_basket_rpc_views() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let coldkey = U256::from(1003);
        let netuid = add_dynamic_network(&hotkey, &owner_coldkey);
        remove_owner_registration_stake(netuid);
        fund_pool(netuid); // price ~= 1.0 (TAO reserve == alpha reserve)

        SubtensorModule::set_tao_weight(u64::MAX);
        zero_claim_threshold();

        // Empty baskets read as zero everywhere.
        assert_eq!(SubtensorModule::get_root_basket_total_nav_tao().to_u64(), 0);
        assert_eq!(
            SubtensorModule::get_validator_basket_nav_tao(&hotkey).to_u64(),
            0
        );

        // Single staker => owns 100% of the basket.
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            NetUid::ROOT,
            2_000_000u64.into(),
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &owner_coldkey,
            netuid,
            10_000_000u64.into(),
        );
        register_on_root(&hotkey, 0);

        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();

        let nav = SubtensorModule::get_validator_basket_nav_tao(&hotkey).to_u64();
        let total = SubtensorModule::get_root_basket_total_nav_tao().to_u64();
        let owed = SubtensorModule::get_root_basket_owed_tao(&coldkey).to_u64();
        let basket = SubtensorModule::get_validator_basket(&hotkey);

        assert!(nav > 0, "validator NAV must be positive");
        // Single validator => network total == this validator's NAV.
        assert_eq!(total, nav);
        // Sole staker => owed (marked) == NAV (marked), both value the same fund.
        assert_abs_diff_eq!(owed, nav, epsilon = 10u64);

        // Breakdown lists exactly the one funded subnet, and its TAO value sums to the NAV.
        assert_eq!(basket.len(), 1);
        let (slot_netuid, slot_alpha, slot_tao) = basket.first().copied().unwrap();
        assert_eq!(slot_netuid, netuid);
        assert!(slot_alpha.to_u64() > 0); // alpha held
        assert_eq!(slot_tao.to_u64(), nav); // tao value == NAV
    });
}

/// End-to-end through the real coinbase path (block_step -> run_coinbase -> emit_to_subnets
/// -> drain_pending -> distribute_emission), proving the basket forms from actual block
/// emission rather than a direct `distribute_emission` call.
#[test]
fn test_root_basket_end_to_end_via_coinbase() {
    new_test_ext(0).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let coldkey = U256::from(1003);
        let netuid = add_dynamic_network(&hotkey, &owner_coldkey);
        remove_owner_registration_stake(netuid);

        Tempo::<Test>::insert(netuid, 1);
        SubtensorModule::set_tao_weight(u64::MAX);

        let root_stake = 200_000_000u64;
        SubnetTAO::<Test>::insert(NetUid::ROOT, TaoBalance::from(root_stake));
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            NetUid::ROOT,
            root_stake.into(),
        );

        // Turn root-sell ON: moving price + spot price > 1.
        SubnetMovingPrice::<Test>::insert(netuid, I96F32::from_num(2));
        SubnetTAO::<Test>::insert(netuid, TaoBalance::from(10_000_000_000_000u64));
        SubnetAlphaIn::<Test>::insert(netuid, AlphaBalance::from(1_000_000_000_000u64));
        zero_claim_threshold();
        assert!(
            SubtensorModule::get_network_root_sell_flag(&[netuid]),
            "root sell flag must be ON"
        );

        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &owner_coldkey,
            netuid,
            10_000_000u64.into(),
        );

        // Validator routes its basket back into the subnet.
        register_on_root(&hotkey, 0);

        assert_eq!(escrow_alpha(&hotkey, netuid), 0);

        // Run real blocks: emission accrues and drains through the coinbase.
        run_to_block(3);

        // The basket formed end-to-end from actual block emission.
        assert!(
            escrow_alpha(&hotkey, netuid) > 0,
            "basket must form from coinbase emission"
        );
        assert!(fund_shares(&hotkey) > 0);
        assert!(has_fund(&hotkey));

        // And it is redeemable to root TAO.
        let root_before = root_stake_of(&hotkey, &coldkey);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(coldkey),
            hotkey
        ));
        assert!(root_stake_of(&hotkey, &coldkey) > root_before);
    });
}

// =============================================================================
// Beta basket: root (UID 0) slot — the fund's TAO cash position
// =============================================================================

/// Redeeming a root slot reassigns the escrow's root stake to the staker: the staker's root
/// stake grows, the escrow drains, shares are consumed, and it is TotalStake-neutral (no swap,
/// no minted TAO — total root stake is conserved, just moved between coldkeys). The cash slot
/// is opened the way a fund gets cash without trading: a direct deposit into an empty fund.
#[test]
fn test_root_basket_uid0_claim_reassigns_no_swap() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let bob = U256::from(1003);
        let netuid = add_dynamic_network(&hotkey, &owner_coldkey);
        remove_owner_registration_stake(netuid);
        fund_pool(netuid);

        SubtensorModule::set_tao_weight(u64::MAX);
        zero_claim_threshold();
        register_on_root(&hotkey, 0);

        let deposit = 2_000_000u64;
        add_balance_to_coldkey_account(&bob, TaoBalance::from(2 * deposit));
        assert_ok!(SubtensorModule::do_stake_into_basket(
            bob,
            hotkey,
            deposit.into(),
        ));

        let shares_before = fund_shares(&hotkey);
        let escrow_before = escrow_alpha(&hotkey, NetUid::ROOT);
        let root_before = root_stake_of(&hotkey, &bob);
        assert_eq!(shares_before, deposit);
        assert_eq!(escrow_before, deposit);
        assert_eq!(root_before, 0);

        let ts_before = TotalStake::<Test>::get().to_u64();
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(bob),
            hotkey
        ));
        let ts_after = TotalStake::<Test>::get().to_u64();

        let gain = root_stake_of(&hotkey, &bob);
        let escrow_after = escrow_alpha(&hotkey, NetUid::ROOT);

        // Staker gained root stake; the escrow gave up exactly the same amount (a pure
        // reassignment, no swap).
        assert_eq!(gain, deposit, "staker must receive the whole cash slot");
        assert_eq!(escrow_after, 0);

        // Shares consumed, watermark settled, TotalStake untouched (no swap, no mint).
        assert_eq!(fund_shares(&hotkey), 0);
        assert_eq!(BasketClaimed::<Test>::get(hotkey, bob), 0);
        assert_eq!(ts_before, ts_after, "root claim must be TotalStake-neutral");
    });
}

/// The root slot compounds like the alpha slots: if the escrow's root stake grows (root
/// dividends) after accrual, the sole staker redeems strictly MORE than the original share
/// value.
#[test]
fn test_root_basket_uid0_compounds() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let coldkey = U256::from(1003);
        let netuid = add_dynamic_network(&hotkey, &owner_coldkey);
        remove_owner_registration_stake(netuid);
        fund_pool(netuid);

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
            &owner_coldkey,
            netuid,
            10_000_000u64.into(),
        );
        register_on_root(&hotkey, 0);

        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();

        let shares = fund_shares(&hotkey);
        assert!(shares > 0);

        // Simulate root dividends compounding the escrow's root stake (N grows, P fixed).
        let escrow_before = escrow_alpha(&hotkey, NetUid::ROOT);
        let escrow_ck = SubtensorModule::get_beta_escrow_account_id();
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &escrow_ck,
            NetUid::ROOT,
            5_000_000u64.into(),
        );
        assert!(escrow_alpha(&hotkey, NetUid::ROOT) > escrow_before);

        let root_before = root_stake_of(&hotkey, &coldkey);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(coldkey),
            hotkey
        ));
        let gain = root_stake_of(&hotkey, &coldkey).saturating_sub(root_before);

        assert!(
            gain > shares,
            "compounding: realized {gain} must exceed original share value {shares}"
        );
    });
}

// =============================================================================
// Edge cases: adversarial invariants
// =============================================================================

/// Conservation under interleaved activity: three stakers with unequal stakes, a fund spread
/// across a subnet holding AND the root (cash) slot, three deposits interleaved with claims.
/// After everyone claims, every holding and the share supply must drain to ~zero (no stranded
/// value, no over-draw), and TotalStake must be conserved through the whole sequence.
#[test]
fn test_root_basket_conservation_interleaved() {
    new_test_ext(1).execute_with(|| {
        let owner_a = U256::from(1001);
        let hotkey = U256::from(1002);
        let alice = U256::from(1003);
        let bob = U256::from(1004);
        let carol = U256::from(1005);
        let owner_b = U256::from(2001);
        let hotkey_b = U256::from(2002);

        let netuid_a = add_dynamic_network(&hotkey, &owner_a);
        let netuid_b = add_dynamic_network(&hotkey_b, &owner_b);
        remove_owner_registration_stake(netuid_a);
        fund_pool(netuid_a);
        fund_pool(netuid_b);

        SubtensorModule::set_tao_weight(u64::MAX);
        zero_claim_threshold();

        // Unequal root stakes 1:2:3.
        for (ck, stake) in [
            (alice, 1_000_000u64),
            (bob, 2_000_000u64),
            (carol, 3_000_000u64),
        ] {
            mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &ck,
                NetUid::ROOT,
                stake.into(),
            );
        }
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &owner_a,
            netuid_a,
            10_000_000u64.into(),
        );

        // Fund composition: 50% subnet B, 50% root (cash) slot.
        register_on_root(&hotkey, 0);

        let ts_start = TotalStake::<Test>::get().to_u64();
        let deposit = |amount: u64| {
            SubtensorModule::distribute_emission(
                netuid_a,
                AlphaBalance::ZERO,
                AlphaBalance::ZERO,
                amount.into(),
                AlphaBalance::ZERO,
            );
            flush_baskets();
        };

        // Interleave deposits and claims.
        deposit(1_000_000);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(alice),
            hotkey
        ));
        deposit(2_000_000);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(bob),
            hotkey
        ));
        deposit(1_500_000);

        // Final round: everyone claims everything.
        for ck in [alice, bob, carol] {
            assert_ok!(SubtensorModule::claim_root_with_hotkey(
                RuntimeOrigin::signed(ck),
                hotkey
            ));
        }

        // The fund is fully drained: no stranded value in any holding, no outstanding shares.
        let residual_b = escrow_alpha(&hotkey, netuid_b);
        let residual_root = escrow_alpha(&hotkey, NetUid::ROOT);
        let residual_shares = fund_shares(&hotkey);
        assert!(residual_b <= 100, "subnet holding stranded: {residual_b}");
        assert!(residual_root <= 100, "root slot stranded: {residual_root}");
        assert!(residual_shares <= 100, "shares stranded: {residual_shares}");

        // TotalStake conserved across the whole interleaved sequence.
        assert_eq!(
            ts_start,
            TotalStake::<Test>::get().to_u64(),
            "TAO minted or destroyed by deposit/claim round trips"
        );
    });
}

/// Claim idempotency: an immediate second claim must be a complete no-op — the payout staked
/// onto root by the first claim must not re-inflate the staker's owed (the watermark rebase
/// covers it).
#[test]
fn test_root_basket_claim_idempotent() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let coldkey = U256::from(1003);
        let netuid = add_dynamic_network(&hotkey, &owner_coldkey);
        remove_owner_registration_stake(netuid);
        fund_pool(netuid);

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
            &owner_coldkey,
            netuid,
            10_000_000u64.into(),
        );
        register_on_root(&hotkey, 0);

        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();

        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(coldkey),
            hotkey
        ));

        let root_after_first = root_stake_of(&hotkey, &coldkey);
        let shares_after_first = fund_shares(&hotkey);
        let escrow_after_first = escrow_alpha(&hotkey, netuid);
        // The payout staked on root must not re-inflate owed; fixed-point truncation in the
        // watermark rebase may leave at most ~1 share of dust, never a compounding remainder.
        assert!(
            SubtensorModule::get_basket_owed_shares(&hotkey, &coldkey) <= 1,
            "payout staked on root re-inflated owed: {}",
            SubtensorModule::get_basket_owed_shares(&hotkey, &coldkey)
        );

        // Repeated claims: at most the 1-share dust moves once; nothing compounds.
        for _ in 0..3 {
            assert_ok!(SubtensorModule::claim_root_with_hotkey(
                RuntimeOrigin::signed(coldkey),
                hotkey
            ));
        }
        assert!(root_stake_of(&hotkey, &coldkey) <= root_after_first + 2);
        assert!(shares_after_first.saturating_sub(fund_shares(&hotkey)) <= 2);
        assert!(escrow_after_first.saturating_sub(escrow_alpha(&hotkey, netuid)) <= 2);
    });
}

/// Self-referential origin: the fund already holds alpha on the subnet the dividend originates
/// from, so the deposit's own origin sell moves the fund's mark mid-flight. The NAV snapshot is
/// taken after the sell, so: (a) the existing staker is not diluted, and (b) a late equal
/// staker's entire entitlement equals only the new deposit's increment — the mid-flight price
/// move cannot be used to skim the existing holder's value.
#[test]
fn test_root_basket_self_referential_origin() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let alice = U256::from(1003);
        let bob = U256::from(1004);
        let netuid = add_dynamic_network(&hotkey, &owner_coldkey);
        remove_owner_registration_stake(netuid);
        fund_pool(netuid);

        SubtensorModule::set_tao_weight(u64::MAX);
        zero_claim_threshold();

        let stake = 2_000_000u64;
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &alice,
            NetUid::ROOT,
            stake.into(),
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &owner_coldkey,
            netuid,
            10_000_000u64.into(),
        );

        // 100% of the basket routed back into the origin subnet: every future dividend both
        // sells and buys the very asset the fund holds.
        register_on_root(&hotkey, 0);

        // Alice accrues the first deposit alone.
        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();
        let alice_before = SubtensorModule::get_basket_payout_tao(&hotkey, &alice);
        assert!(alice_before > 0);

        // Bob joins with equal stake (watermark rebased as real add_stake would).
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &bob,
            NetUid::ROOT,
            stake.into(),
        );
        SubtensorModule::add_stake_adjust_root_claimed_for_hotkey_and_coldkey(&hotkey, &bob, stake);

        // Second deposit with the fund holding origin-subnet alpha.
        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();

        let alice_after = SubtensorModule::get_basket_payout_tao(&hotkey, &alice);
        let bob_after = SubtensorModule::get_basket_payout_tao(&hotkey, &bob);

        // (a) Alice is not diluted by the mid-flight sell of the fund's own holding (small AMM
        // slippage tolerance).
        assert!(
            alice_after + 1_000 >= alice_before,
            "existing holder diluted: {alice_before} -> {alice_after}"
        );

        // (b) Bob's whole entitlement equals only Alice's increment from the new deposit: the
        // self-referential price move gave him no claim on her pre-existing value.
        let alice_increment = alice_after.saturating_sub(alice_before);
        assert!(bob_after > 0);
        assert_abs_diff_eq!(bob_after, alice_increment, epsilon = 2_000u64);
        assert!(
            bob_after < alice_before,
            "late staker skimmed via self-referential deposit: bob={bob_after} alice_before={alice_before}"
        );
    });
}

/// Fixed-point saturation regression: at chain-scale magnitudes (fund shares and NAV around
/// 2e16 rao — the full TAO supply), the mint and payout math must be exact. The previous
/// `U96F32` formulation saturated at ~7.9e28 in the intermediate product, silently underpaying
/// by orders of magnitude.
#[test]
fn test_root_basket_large_magnitudes_no_saturation() {
    // Unit check: owed * nav overflows 96 fixed-point integer bits (4e32 > 2^96) but must
    // compute exactly in u128. A saturating implementation returns ~3.9e12 here.
    let supply = 21_000_000u64 * 1_000_000_000; // 2.1e16 rao
    assert_eq!(
        SubtensorModule::basket_payout_from(supply, supply, supply),
        supply
    );
    // Half the shares of a supply-sized fund pay exactly half the NAV.
    assert_eq!(
        SubtensorModule::basket_payout_from(supply / 2, supply, supply),
        supply / 2
    );

    // End-to-end: a large dividend deposited into a supply-scale fund mints ~value * P / N
    // shares, not a saturated fraction of it.
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let coldkey = U256::from(1003);
        let netuid = add_dynamic_network(&hotkey, &owner_coldkey);
        remove_owner_registration_stake(netuid);

        // Very deep pool at price 1 so a 2e13 trade has negligible slippage.
        SubnetTAO::<Test>::insert(netuid, TaoBalance::from(1_000_000_000_000_000_000u64));
        SubnetAlphaIn::<Test>::insert(netuid, AlphaBalance::from(1_000_000_000_000_000_000u64));

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
            &owner_coldkey,
            netuid,
            10_000_000u64.into(),
        );
        register_on_root(&hotkey, 0);

        // Fund at supply scale: escrow holds 2e16 alpha (price 1 => NAV 2e16), 2e16 shares out.
        // (Direct stake write: the mock helper's subnet-balance top-up overflows the test-chain
        // issuance at this scale.)
        let fund_scale = 20_000_000_000_000_000u64; // 2e16
        let escrow_ck = SubtensorModule::get_beta_escrow_account_id();
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &escrow_ck,
            netuid,
            fund_scale.into(),
        );
        BasketShares::<Test>::insert(hotkey, fund_scale);

        // Deposit a 2e13-rao dividend directly into the basket (bypassing the emission split,
        // which is stake-proportional and not what is under test): N/P ~= 1, so ~2e13 shares
        // must be minted. NAV is a realizable quote, so redeeming the whole 2e16 fund against
        // the 1e18 pool marks ~2% below par (N/P slightly < 1) and the mint lands slightly
        // above the dividend — far from the ~5x collapse a saturated mint would show.
        let dividend = 20_000_000_000_000u64; // 2e13
        SubtensorModule::distribute_root_alpha_to_basket(&hotkey, netuid, dividend.into());

        let minted = fund_shares(&hotkey).saturating_sub(fund_scale);
        assert!(
            minted > dividend / 2 && minted < dividend * 2,
            "mint saturated or mispriced: minted {minted} for a {dividend} deposit at N/P~=1"
        );
        assert_abs_diff_eq!(minted, dividend, epsilon = dividend / 20);
    });
}

/// Removing root stake never destroys already-accrued entitlement: the watermark rebase makes
/// `owed = rate*(stake-Δ) - (claimed - rate*Δ)` algebraically identical to the pre-unstake owed.
#[test]
fn test_root_basket_unstake_preserves_accrued() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let coldkey = U256::from(1003);
        let netuid = add_dynamic_network(&hotkey, &owner_coldkey);
        remove_owner_registration_stake(netuid);
        fund_pool(netuid);

        SubtensorModule::set_tao_weight(u64::MAX);
        zero_claim_threshold();

        let stake = 2_000_000u64;
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            NetUid::ROOT,
            stake.into(),
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &owner_coldkey,
            netuid,
            10_000_000u64.into(),
        );
        register_on_root(&hotkey, 0);

        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();

        let owed_before = SubtensorModule::get_basket_owed_shares(&hotkey, &coldkey);
        let payout_before = SubtensorModule::get_basket_payout_tao(&hotkey, &coldkey);
        assert!(owed_before > 0);

        // Unstake half the root stake, mirroring the real remove_stake path (stake decrease +
        // watermark rebase).
        let removed = stake / 2;
        SubtensorModule::decrease_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            NetUid::ROOT,
            removed.into(),
        );
        SubtensorModule::remove_stake_adjust_root_claimed_for_hotkey_and_coldkey(
            &hotkey,
            &coldkey,
            removed.into(),
        );

        // Accrued entitlement is unchanged (±1 for fixed-point floor).
        let owed_after = SubtensorModule::get_basket_owed_shares(&hotkey, &coldkey);
        assert_abs_diff_eq!(owed_after, owed_before, epsilon = 1u64);
        assert_abs_diff_eq!(
            SubtensorModule::get_basket_payout_tao(&hotkey, &coldkey),
            payout_before,
            epsilon = 1u64
        );

        // And it remains fully claimable. The claim executes fee-free while the quoted payout
        // is a fee-included realizable value, so the realized gain can exceed the quote by up
        // to the swap fee.
        let root_before = root_stake_of(&hotkey, &coldkey);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(coldkey),
            hotkey
        ));
        let gain = root_stake_of(&hotkey, &coldkey).saturating_sub(root_before);
        assert_abs_diff_eq!(gain, payout_before, epsilon = payout_before / 500);
    });
}

/// Pro-rata redemption preserves fund composition: after one of two equal stakers claims from a
/// fund holding three subnets (a 2:1 split plus the dividend's origin), the ratio between the
/// remaining holdings is unchanged, and the second claimant's payout matches the first (no
/// ordering advantage beyond AMM slippage).
#[test]
fn test_root_basket_claim_preserves_composition() {
    new_test_ext(1).execute_with(|| {
        let owner_a = U256::from(1001);
        let hotkey = U256::from(1002);
        let alice = U256::from(1003);
        let bob = U256::from(1004);
        let owner_b = U256::from(2001);
        let hotkey_b = U256::from(2002);
        let owner_c = U256::from(3001);
        let hotkey_c = U256::from(3002);

        let netuid_a = add_dynamic_network(&hotkey, &owner_a);
        let netuid_b = add_dynamic_network(&hotkey_b, &owner_b);
        let netuid_c = add_dynamic_network(&hotkey_c, &owner_c);
        remove_owner_registration_stake(netuid_a);
        fund_pool(netuid_a);
        fund_pool(netuid_b);
        fund_pool(netuid_c);

        SubtensorModule::set_tao_weight(u64::MAX);
        zero_claim_threshold();

        let stake = 2_000_000u64;
        for ck in [alice, bob] {
            mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &ck,
                NetUid::ROOT,
                stake.into(),
            );
        }
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &owner_a,
            netuid_a,
            10_000_000u64.into(),
        );

        // 2:1 composition across B and C (held from an earlier fund life, at par), then a
        // dividend on A mints alice's and bob's shares against that NAV.
        register_on_root(&hotkey, 0);
        let escrow = SubtensorModule::get_beta_escrow_account_id();
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &escrow,
            netuid_b,
            4_000_000u64.into(),
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &escrow,
            netuid_c,
            2_000_000u64.into(),
        );
        BasketShares::<Test>::insert(hotkey, 6_000_000u64);

        SubtensorModule::distribute_emission(
            netuid_a,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            6_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();
        let a_before = escrow_alpha(&hotkey, netuid_a) as f64;
        assert!(a_before > 0.0);

        let b_before = escrow_alpha(&hotkey, netuid_b) as f64;
        let c_before = escrow_alpha(&hotkey, netuid_c) as f64;
        assert!(b_before > 0.0 && c_before > 0.0);
        let ratio_before = b_before / c_before;

        // Alice (half the shares) claims.
        let alice_root_before = root_stake_of(&hotkey, &alice);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(alice),
            hotkey
        ));
        let alice_gain = root_stake_of(&hotkey, &alice).saturating_sub(alice_root_before);

        // Composition is preserved: every holding shrank by the same fraction.
        let a_after = escrow_alpha(&hotkey, netuid_a) as f64;
        let b_after = escrow_alpha(&hotkey, netuid_b) as f64;
        let c_after = escrow_alpha(&hotkey, netuid_c) as f64;
        let ratio_after = b_after / c_after;
        assert!(
            (ratio_after - ratio_before).abs() / ratio_before < 0.001,
            "claim skewed composition: {ratio_before} -> {ratio_after}"
        );
        assert!(
            ((a_after / a_before) - (b_after / b_before)).abs() < 0.001,
            "claim must take the same fraction of A as of B"
        );

        // Bob's payout matches Alice's (equal stakes), modulo slippage from her claim.
        let bob_root_before = root_stake_of(&hotkey, &bob);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(bob),
            hotkey
        ));
        let bob_gain = root_stake_of(&hotkey, &bob).saturating_sub(bob_root_before);
        assert!(alice_gain > 0 && bob_gain > 0);
        assert_abs_diff_eq!(alice_gain, bob_gain, epsilon = 3_000u64);
    });
}

/// A dividend whose rate increment rounds to zero (huge claimant base, tiny deposit) must be
/// rolled back and recycled — never deposited without crediting stakers, which would strand
/// value and break `Σ owed == P`.
#[test]
fn test_root_basket_dust_deposit_recycled() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let coldkey = U256::from(1003);
        let netuid = add_dynamic_network(&hotkey, &owner_coldkey);
        remove_owner_registration_stake(netuid);
        fund_pool(netuid);

        SubtensorModule::set_tao_weight(u64::MAX);
        zero_claim_threshold();

        // Enormous claimant base: increment = shares / total_root rounds below I96F32's 2^-32
        // resolution for a ~1e6 deposit. (Direct stake write: the mock helper's subnet-balance
        // top-up overflows the test-chain issuance at this scale.)
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            NetUid::ROOT,
            10_000_000_000_000_000u64.into(), // 1e16
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &owner_coldkey,
            netuid,
            10_000_000u64.into(),
        );
        register_on_root(&hotkey, 0);

        let ts_before = TotalStake::<Test>::get().to_u64();
        // Deposit directly into the basket: the rate increment (~1e3 / 1e16 < 2^-32) rounds to
        // zero, so the whole deposit must roll back.
        SubtensorModule::distribute_root_alpha_to_basket(&hotkey, netuid, 1_000u64.into());

        // The deposit was rolled back and recycled: no shares, no rate, no escrow position, and
        // no TAO moved.
        assert_eq!(fund_shares(&hotkey), 0);
        assert!(!has_fund(&hotkey));
        assert_eq!(escrow_alpha(&hotkey, netuid), 0);
        assert_eq!(TotalStake::<Test>::get().to_u64(), ts_before);
    });
}

/// A claim below the dust threshold consumes nothing: no shares, no watermark, no payout, and
/// the full amount remains claimable once the threshold permits. The fund's sole holding is
/// itself below the bar, so the dust sweep cashes it into the root slot — NAV-continuous, so
/// the later claim pays out of that cash.
#[test]
fn test_root_basket_threshold_skip_consumes_nothing() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let coldkey = U256::from(1003);
        let netuid = add_dynamic_network(&hotkey, &owner_coldkey);
        remove_owner_registration_stake(netuid);
        fund_pool(netuid);

        SubtensorModule::set_tao_weight(u64::MAX);

        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            NetUid::ROOT,
            2_000_000u64.into(),
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &owner_coldkey,
            netuid,
            10_000_000u64.into(),
        );
        register_on_root(&hotkey, 0);

        // Accrue less than the (soon-raised) claim threshold. The deposit itself must land,
        // so flush with the threshold zeroed — the queue gate uses the same threshold and
        // would otherwise defer the credit instead of depositing it.
        zero_claim_threshold();
        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            100_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();
        RootClaimableThreshold::<Test>::insert(NetUid::ROOT, I96F32::from_num(1_000_000u64));

        let owed_before = SubtensorModule::get_basket_owed_shares(&hotkey, &coldkey);
        let shares_before = fund_shares(&hotkey);
        let escrow_before = escrow_alpha(&hotkey, netuid);
        let root_before = root_stake_of(&hotkey, &coldkey);
        assert!(owed_before > 0);
        assert!(escrow_before > 0);

        // Below threshold: skipped, nothing consumed. The sub-threshold holding is swept
        // into the fund's root cash slot, which keeps its value for the claim below.
        let nav_before = SubtensorModule::get_validator_basket_nav_tao(&hotkey).to_u64();
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(coldkey),
            hotkey
        ));
        assert_eq!(
            SubtensorModule::get_basket_owed_shares(&hotkey, &coldkey),
            owed_before
        );
        assert_eq!(fund_shares(&hotkey), shares_before);
        assert_eq!(root_stake_of(&hotkey, &coldkey), root_before);
        assert_eq!(
            escrow_alpha(&hotkey, netuid),
            0,
            "dust holding swept to cash"
        );
        let cash = escrow_alpha(&hotkey, NetUid::ROOT);
        assert!(cash > 0 && cash <= nav_before && cash >= nav_before * 99 / 100);

        // Lower the threshold: the full amount pays out.
        zero_claim_threshold();
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(coldkey),
            hotkey
        ));
        assert!(root_stake_of(&hotkey, &coldkey) > root_before);
        assert!(fund_shares(&hotkey) <= 10);
    });
}

/// Coldkey swap must carry a staker's basket entitlement even when their current root stake is
/// zero: the signed watermark deliberately represents "accrued owed with no stake" (negative
/// watermark after unstake-all), and gating the transfer on live stake would orphan it on the
/// dead coldkey.
#[test]
fn test_root_basket_coldkey_swap_carries_owed_with_zero_stake() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let old_coldkey = U256::from(1003);
        let new_coldkey = U256::from(1004);
        let netuid = add_dynamic_network(&hotkey, &owner_coldkey);
        remove_owner_registration_stake(netuid);
        fund_pool(netuid);

        SubtensorModule::set_tao_weight(u64::MAX);
        zero_claim_threshold();

        let stake = 2_000_000u64;
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &old_coldkey,
            NetUid::ROOT,
            stake.into(),
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &owner_coldkey,
            netuid,
            10_000_000u64.into(),
        );
        register_on_root(&hotkey, 0);

        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();

        // Unstake ALL root stake, mirroring the real remove_stake path. The watermark goes
        // negative; owed is preserved with zero live stake.
        SubtensorModule::decrease_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &old_coldkey,
            NetUid::ROOT,
            stake.into(),
        );
        SubtensorModule::remove_stake_adjust_root_claimed_for_hotkey_and_coldkey(
            &hotkey,
            &old_coldkey,
            stake.into(),
        );

        assert_eq!(root_stake_of(&hotkey, &old_coldkey), 0);
        let owed_before = SubtensorModule::get_basket_owed_shares(&hotkey, &old_coldkey);
        assert!(owed_before > 0, "owed must survive unstake-all");
        assert!(
            BasketClaimed::<Test>::get(hotkey, old_coldkey) < 0,
            "watermark must be negative after unstake-all"
        );

        // Swap the coldkey.
        assert_ok!(SubtensorModule::do_swap_coldkey(&old_coldkey, &new_coldkey));

        // The entitlement followed the coldkey — nothing orphaned on the dead key.
        assert_abs_diff_eq!(
            SubtensorModule::get_basket_owed_shares(&hotkey, &new_coldkey),
            owed_before,
            epsilon = 1u64
        );
        assert_eq!(
            BasketClaimed::<Test>::get(hotkey, old_coldkey),
            0,
            "old coldkey must hold no watermark after swap"
        );

        // And it is claimable by the new coldkey.
        let root_before = root_stake_of(&hotkey, &new_coldkey);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(new_coldkey),
            hotkey
        ));
        assert!(
            root_stake_of(&hotkey, &new_coldkey) > root_before,
            "new coldkey must be able to realize the carried entitlement"
        );
    });
}

/// A positive marked entitlement must remain claimable when its proportional alpha slice
/// floors to zero. One atomic alpha unit is sold and payment is capped at the entitlement.
#[test]
fn test_root_basket_rounding_zero_take_sells_minimum_unit() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let coldkey = U256::from(1003);
        let netuid = add_dynamic_network(&hotkey, &owner_coldkey);
        remove_owner_registration_stake(netuid);

        SubtensorModule::set_tao_weight(u64::MAX);
        zero_claim_threshold();

        // High-price pool: spot ~= 100 TAO per alpha.
        SubnetTAO::<Test>::insert(netuid, TaoBalance::from(100_000_000_000_000u64));
        SubnetAlphaIn::<Test>::insert(netuid, AlphaBalance::from(1_000_000_000_000u64));

        let stake = 2_000_000u64;
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            NetUid::ROOT,
            stake.into(),
        );

        // Synthetic fund state: 1e6 shares outstanding against a tiny 5_000-alpha holding
        // (marked NAV = 5_000 * 100 = 500_000), and the staker owed 100 shares.
        // estimated_payout = 100 * 500_000 / 1_000_000 = 50 > 0, but the alpha take is
        // 5_000 * 100 / 1_000_000 = 0.5 -> floors to 0.
        let escrow = SubtensorModule::get_beta_escrow_account_id();
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &escrow,
            netuid,
            5_000u64.into(),
        );
        BasketShares::<Test>::insert(hotkey, 1_000_000u64);
        BasketRate::<Test>::insert(hotkey, I96F32::from_num(0.00005)); // owed = 100

        let owed_before = SubtensorModule::get_basket_owed_shares(&hotkey, &coldkey);
        // ~100 (I96F32 floors the 0.00005 rate slightly); anything in this range keeps the
        // estimate positive while every alpha take floors to zero.
        assert!((90..=100).contains(&owed_before), "owed = {owed_before}");
        let shares_before = fund_shares(&hotkey);
        let escrow_before = escrow_alpha(&hotkey, netuid);
        let payout_before = SubtensorModule::get_basket_payout_tao(&hotkey, &coldkey);
        let root_before = root_stake_of(&hotkey, &coldkey);
        assert!(payout_before > 0);

        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(coldkey),
            hotkey
        ));

        // Rebasing the claimed payout's new root stake can leave one share from fixed-point
        // truncation, matching the invariant used by the other claim-drain tests.
        assert!(
            SubtensorModule::get_basket_owed_shares(&hotkey, &coldkey) <= 1,
            "minimum-unit claim left more than rounding dust"
        );
        assert_eq!(fund_shares(&hotkey), shares_before - owed_before);
        assert_eq!(escrow_alpha(&hotkey, netuid), escrow_before - 1);
        assert_eq!(
            root_stake_of(&hotkey, &coldkey) - root_before,
            payout_before,
            "the minimum-unit sale must not overpay the marked entitlement"
        );
    });
}

/// A tiny root-cash row whose claimant slice rounds to zero must not block another payable row.
/// Root cash is already denominated in rao, so the indivisible remainder simply stays in escrow.
#[test]
fn test_root_basket_rounding_zero_root_row_does_not_block_payable_rows() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let alice = U256::from(1003);
        let bob = U256::from(1004);
        let netuid = add_dynamic_network(&hotkey, &owner_coldkey);
        remove_owner_registration_stake(netuid);

        SubtensorModule::set_tao_weight(u64::MAX);
        zero_claim_threshold();

        // High-price pool: 1 alpha realizes meaningful TAO.
        SubnetTAO::<Test>::insert(netuid, TaoBalance::from(100_000_000_000_000u64));
        SubnetAlphaIn::<Test>::insert(netuid, AlphaBalance::from(1_000_000_000_000u64));

        // Alice 99 / Bob 1 of root stake; rate = 1 ⇒ owed shares match stake units.
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &alice,
            NetUid::ROOT,
            99u64.into(),
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &bob,
            NetUid::ROOT,
            1u64.into(),
        );

        // Curated destination so dust consolidation will not flatten the 1-alpha row.
        register_on_root(&hotkey, 0);

        let escrow = SubtensorModule::get_beta_escrow_account_id();
        // One rao of root cash rounds to zero for Alice, while the curated alpha row pays.
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &escrow,
            NetUid::ROOT,
            1u64.into(),
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &escrow,
            netuid,
            1_000u64.into(),
        );
        BasketShares::<Test>::insert(hotkey, 100u64);
        BasketRate::<Test>::insert(hotkey, I96F32::from_num(1));

        let alice_owed = SubtensorModule::get_basket_owed_shares(&hotkey, &alice);
        assert_eq!(alice_owed, 99, "alice owed = {alice_owed}");
        assert_eq!(SubtensorModule::get_basket_owed_shares(&hotkey, &bob), 1);

        // Alice's root take floors: floor(1 * 99 / 100) = 0; subnet take is positive.
        assert_eq!(SubtensorModule::mul_div_u64(1, 99, 100), 0);
        assert!(SubtensorModule::mul_div_u64(1_000, 99, 100) > 0);

        let shares_before = fund_shares(&hotkey);
        let alpha_before = escrow_alpha(&hotkey, netuid);
        let alice_payout = SubtensorModule::get_basket_payout_tao(&hotkey, &alice);
        let alice_subnet_payout =
            SubtensorModule::get_basket_subnet_payout_tao(&hotkey, &alice, netuid);
        let alice_root_before = root_stake_of(&hotkey, &alice);

        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(alice),
            hotkey
        ));

        assert_eq!(SubtensorModule::get_basket_owed_shares(&hotkey, &alice), 0);
        assert_eq!(fund_shares(&hotkey), shares_before - alice_owed);
        assert_eq!(escrow_alpha(&hotkey, netuid), alpha_before - 990);
        assert!(escrow_alpha(&hotkey, NetUid::ROOT) >= 1);
        assert_eq!(
            root_stake_of(&hotkey, &alice) - alice_root_before,
            alice_subnet_payout
        );
        assert!(alice_subnet_payout > 0 && alice_subnet_payout <= alice_payout);
        // Bob can later collect the root remainder together with his remaining alpha share.
        let bob_payout = SubtensorModule::get_basket_payout_tao(&hotkey, &bob);
        let bob_root_before = root_stake_of(&hotkey, &bob);
        assert!(bob_payout > 0);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(bob),
            hotkey
        ));
        assert_eq!(root_stake_of(&hotkey, &bob) - bob_root_before, bob_payout);
        assert_eq!(fund_shares(&hotkey), 0);
    });
}

/// One terminally shallow subnet must not block redemption of the rest of a validator's
/// basket. The claimant's exact pro-rata garbage slice is written off while the executable
/// holding pays normally, leaving the same garbage-per-share ratio for the next holder.
#[test]
fn test_root_basket_claim_writes_off_only_claimants_terminal_garbage_slice() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let alice = U256::from(1003);
        let bob = U256::from(1004);
        let healthy = add_dynamic_network(&hotkey, &owner_coldkey);
        let garbage = add_dynamic_network(&hotkey, &owner_coldkey);
        remove_owner_registration_stake(healthy);
        remove_owner_registration_stake(garbage);
        fund_pool(healthy);

        SubtensorModule::set_tao_weight(u64::MAX);
        zero_claim_threshold();

        // Alpha -> TAO requires the output (TAO) reserve to meet SwapMinimumReserve.
        // This pool is therefore terminal for sales, independent of the sale amount.
        SubnetTAO::<Test>::insert(
            garbage,
            TaoBalance::from(u64::from(SwapMinimumReserve::get()) - 1),
        );
        SubnetAlphaIn::<Test>::insert(garbage, AlphaBalance::from(1_000_000u64));

        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &alice,
            NetUid::ROOT,
            100u64.into(),
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &bob,
            NetUid::ROOT,
            100u64.into(),
        );
        register_on_root(&hotkey, 0);

        let escrow = SubtensorModule::get_beta_escrow_account_id();
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &escrow,
            healthy,
            1_000_000u64.into(),
        );
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &escrow,
            garbage,
            1_000u64.into(),
        );
        BasketShares::<Test>::insert(hotkey, 200u64);
        BasketRate::<Test>::insert(hotkey, I96F32::from_num(1));

        let alice_root_before = root_stake_of(&hotkey, &alice);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(alice),
            hotkey
        ));

        assert!(root_stake_of(&hotkey, &alice) > alice_root_before);
        assert_eq!(fund_shares(&hotkey), 100);
        assert_eq!(escrow_alpha(&hotkey, garbage), 500);
        assert_eq!(SubtensorModule::get_basket_owed_shares(&hotkey, &bob), 100);
    });
}

/// A full holding larger than the swap engine's one-call 1000x input-reserve guard is still
/// executable in reserve-bounded chunks. Its valuation and the money-moving claim must use
/// the same chunk sequence, rather than treating `SwapInputTooLarge` as a zero-valued slot.
#[test]
fn test_root_basket_claim_chunks_oversized_executable_holding() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let coldkey = U256::from(1003);
        let netuid = add_dynamic_network(&hotkey, &owner_coldkey);
        remove_owner_registration_stake(netuid);
        // A liquid high-price pool keeps ample TAO output reserve even after selling more
        // than 1000x its alpha input reserve. This isolates the engine's per-call input guard
        // from a genuinely terminal reserve condition.
        SubnetTAO::<Test>::insert(netuid, TaoBalance::from(1_000_000_000_000_000u64));
        SubnetAlphaIn::<Test>::insert(netuid, AlphaBalance::from(1_000_000_000u64));
        let subnet_account = SubtensorModule::get_subnet_account_id(netuid).unwrap();
        add_balance_to_coldkey_account(&subnet_account, TaoBalance::from(1_000_000_000_000_000u64));

        SubtensorModule::set_tao_weight(u64::MAX);
        zero_claim_threshold();
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            NetUid::ROOT,
            1u64.into(),
        );
        register_on_root(&hotkey, 0);

        let alpha_reserve = SubnetAlphaIn::<Test>::get(netuid).to_u64();
        let oversized = alpha_reserve.saturating_mul(1_100);
        let escrow = SubtensorModule::get_beta_escrow_account_id();
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &escrow,
            netuid,
            oversized.into(),
        );
        BasketShares::<Test>::insert(hotkey, 1u64);
        BasketRate::<Test>::insert(hotkey, I96F32::from_num(1));

        assert!(
            SubtensorModule::try_realizable_tao_for_alpha(netuid, oversized)
                .expect("oversized quote must not fail")
                .expect("deep pool is not terminal")
                > 0
        );
        let root_before = root_stake_of(&hotkey, &coldkey);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(coldkey),
            hotkey
        ));

        assert!(root_stake_of(&hotkey, &coldkey) > root_before);
        assert_eq!(escrow_alpha(&hotkey, netuid), 0);
        assert_eq!(fund_shares(&hotkey), 0);
    });
}

/// A fully-drained fund accepts new deposits cleanly: the revived fund's value belongs to the
/// (current) stakers and is fully redeemable; the drained epoch cannot leak into the new one.
#[test]
fn test_root_basket_revives_after_full_drain() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let coldkey = U256::from(1003);
        let netuid = add_dynamic_network(&hotkey, &owner_coldkey);
        remove_owner_registration_stake(netuid);
        fund_pool(netuid);

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
            &owner_coldkey,
            netuid,
            10_000_000u64.into(),
        );
        register_on_root(&hotkey, 0);

        // Epoch 1: accrue and fully drain.
        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(coldkey),
            hotkey
        ));
        assert!(fund_shares(&hotkey) <= 10, "epoch-1 fund should be drained");

        // Epoch 2: a new deposit into the drained fund.
        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();
        let epoch2_value = SubtensorModule::get_validator_basket_nav_tao(&hotkey).to_u64();
        assert!(epoch2_value > 0);

        // The sole staker redeems ~the entire epoch-2 value; nothing was lost to the drained
        // epoch's residual dust.
        let root_before = root_stake_of(&hotkey, &coldkey);
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(coldkey),
            hotkey
        ));
        let gain = root_stake_of(&hotkey, &coldkey).saturating_sub(root_before);
        assert_abs_diff_eq!(gain, epoch2_value, epsilon = epoch2_value / 100);
        assert!(fund_shares(&hotkey) <= 20);
    });
}

/// The escrow's own root stake is excluded from the claimant base, so a sole staker's claim
/// stays correct across repeated dividends while the fund holds root cash (no value is
/// stranded by denominator dilution): after accrual the staker drains the whole fund, root
/// slot included. Were the escrow's cash counted, each dividend would under-credit the rate
/// and leave `P > Σ owed`, i.e. a root-slot residual after the claim.
#[test]
fn test_root_basket_uid0_excludes_escrow_from_denominator() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let coldkey = U256::from(1003);
        let netuid = add_dynamic_network(&hotkey, &owner_coldkey);
        remove_owner_registration_stake(netuid);
        fund_pool(netuid);

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
            &owner_coldkey,
            netuid,
            10_000_000u64.into(),
        );
        register_on_root(&hotkey, 0);

        // The sole staker opens the fund with cash: the escrow now holds root stake equal to
        // the staker's own root stake, the worst case for a diluted denominator.
        let cash = 2_000_000u64;
        add_balance_to_coldkey_account(&coldkey, TaoBalance::from(2 * cash));
        assert_ok!(SubtensorModule::do_stake_into_basket(
            coldkey,
            hotkey,
            cash.into(),
        ));
        assert_eq!(escrow_alpha(&hotkey, NetUid::ROOT), cash);

        // Two dividends land while the escrow holds root stake.
        for _ in 0..2 {
            SubtensorModule::distribute_emission(
                netuid,
                AlphaBalance::ZERO,
                AlphaBalance::ZERO,
                1_000_000u64.into(),
                AlphaBalance::ZERO,
            );
            flush_baskets();
        }
        let escrow_before = escrow_alpha(&hotkey, NetUid::ROOT);
        assert!(escrow_before > 0);

        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(coldkey),
            hotkey
        ));

        // The sole real staker drains the whole root slot: no value stranded by the escrow's
        // own root holdings.
        let escrow_after = escrow_alpha(&hotkey, NetUid::ROOT);
        assert!(
            escrow_after <= escrow_before / 1_000 + 10,
            "root slot must drain to ~0; residual {escrow_after} of {escrow_before}"
        );
        assert!(fund_shares(&hotkey) <= 20);
    });
}

// =============================================================================
// The full "become a root validator basket" journey, through real extrinsics.
// =============================================================================

/// End-to-end operator flow: burn-based root registration with **zero prior
/// stake**, self-subscription, basket weight curation, an epoch's dividend
/// distribution into the basket, and a delegating staker's claim.
///
/// Pins the burn accounting introduced by burn-based admission: the coldkey
/// pays exactly `Burn(0)`, and the price bumps for the next registrant.
#[test]
fn test_become_root_validator_basket_journey() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        // The validator hotkey is also this subnet's owner hotkey.
        let validator_coldkey = subnet_owner_coldkey;
        let validator_hotkey = U256::from(1004);
        let staker_coldkey = U256::from(1005);

        // A live subnet with a deep pool for the basket to buy into. Root
        // dividends flow through the subnet epoch, so the validator hotkey
        // validates this subnet (it is its registered neuron and the subnet
        // owner delegates alpha to it) — but it holds no ROOT stake yet.
        let netuid = add_dynamic_network(&validator_hotkey, &subnet_owner_coldkey);
        remove_owner_registration_stake(netuid);
        fund_pool(netuid);
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &validator_hotkey,
            &subnet_owner_coldkey,
            netuid,
            10_000_000u64.into(),
        );
        SubtensorModule::set_tao_weight(u64::MAX);
        zero_claim_threshold();

        // The root network, charging a real, demand-priced burn. The mock's
        // `add_network` pins the bump multiplier to 1.0 for subnet tests;
        // restore the production 1.26 to exercise the pricing path.
        add_network(NetUid::ROOT, 10, 0);
        let burn_cost = 1_000_000u64; // 0.001 tao
        SubtensorModule::set_burn(NetUid::ROOT, TaoBalance::from(burn_cost));
        BurnIncreaseMult::<Test>::insert(NetUid::ROOT, U64F64::from_num(1.26));
        // Root staking consumes root alpha 1:1.
        SubnetAlphaIn::<Test>::insert(NetUid::ROOT, AlphaBalance::from(10_000_000_000u64));

        // --- Step 1: register. The hotkey has no root stake; admission is
        // paid for from the coldkey's free balance, not gated on stake.
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_on_subnet(&validator_hotkey, NetUid::ROOT),
            AlphaBalance::ZERO
        );
        let funding = 10_000_000_000u64; // 10 tao
        add_balance_to_coldkey_account(&validator_coldkey, TaoBalance::from(funding));
        let balance_before = SubtensorModule::get_coldkey_balance(&validator_coldkey);
        assert_ok!(SubtensorModule::root_register(
            RuntimeOrigin::signed(validator_coldkey),
            validator_hotkey,
        ));
        assert!(Uids::<Test>::contains_key(NetUid::ROOT, validator_hotkey));
        // Exactly the burn was charged, and the price bumped for the next registrant.
        let balance_after = SubtensorModule::get_coldkey_balance(&validator_coldkey);
        assert_eq!(balance_before - balance_after, TaoBalance::from(burn_cost));
        assert!(SubtensorModule::get_burn(NetUid::ROOT) > TaoBalance::from(burn_cost));

        // --- Step 2: subscribe TAO to the seat. This is what holds it (pruning
        // is lowest-stake) and clears the weight-setting stake threshold.
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(validator_coldkey),
            validator_hotkey,
            NetUid::ROOT,
            TaoBalance::from(2_000_000_000u64),
        ));

        // --- Step 3: a delegator subscribes to the fund. There is nothing to
        // curate: dividends accumulate on the subnet they are earned on.
        add_balance_to_coldkey_account(&staker_coldkey, TaoBalance::from(2_000_000_000u64));
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(staker_coldkey),
            validator_hotkey,
            NetUid::ROOT,
            TaoBalance::from(1_000_000_000u64),
        ));
        let staker_principal = root_stake_of(&validator_hotkey, &staker_coldkey);
        assert!(staker_principal > 0);

        // --- Step 4: an epoch pays the validator root dividends; they land in
        // the fund as alpha on the origin subnet.
        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        flush_baskets();
        assert!(has_fund(&validator_hotkey));
        assert!(escrow_alpha(&validator_hotkey, netuid) > 0);
        assert!(SubtensorModule::get_basket_owed_shares(&validator_hotkey, &staker_coldkey) > 0);

        // --- Step 5: the staker claims — accrued entitlement comes back as
        // TAO staked on root, on top of untouched principal.
        assert_ok!(SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(staker_coldkey),
            validator_hotkey
        ));
        assert!(root_stake_of(&validator_hotkey, &staker_coldkey) > staker_principal);
    });
}

/// Burn-based admission cannot sybil seats: pruning removes the lowest-staked
/// member, so an attacker's own zero-stake keys shield every staked member —
/// each new unstaked registration evicts the previous unstaked key, never a
/// staked validator.
#[test]
fn test_root_register_zero_stake_keys_shield_staked_members() {
    new_test_ext(1).execute_with(|| {
        let staked_coldkey = U256::from(2001);
        let staked_hotkey = U256::from(2002);
        let sybil_coldkey = U256::from(2003);
        let sybil_hotkeys = [U256::from(2004), U256::from(2005), U256::from(2006)];

        add_network(NetUid::ROOT, 10, 0);
        SubtensorModule::set_max_allowed_uids(NetUid::ROOT, 2);
        SubtensorModule::set_max_registrations_per_block(NetUid::ROOT, 10);
        SubtensorModule::set_target_registrations_per_interval(NetUid::ROOT, 10);
        // This test is about stake-order prune, not the immunity window.
        SubtensorModule::set_immunity_period(NetUid::ROOT, 0);

        // A staked validator and one zero-stake key fill the two slots.
        root_register_ok(staked_hotkey, staked_coldkey);
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &staked_hotkey,
            &staked_coldkey,
            NetUid::ROOT,
            5_000_000u64.into(),
        );
        root_register_ok(sybil_hotkeys[0], sybil_coldkey);
        assert_eq!(SubnetworkN::<Test>::get(NetUid::ROOT), 2);

        // Every further zero-stake registration evicts the previous zero-stake
        // key; the staked validator keeps its seat throughout.
        for pair in sybil_hotkeys.windows(2) {
            let Some(prev) = pair.first() else {
                continue;
            };
            let Some(next) = pair.get(1) else {
                continue;
            };
            root_register_ok(*next, sybil_coldkey);
            assert!(
                !Uids::<Test>::contains_key(NetUid::ROOT, prev),
                "previous zero-stake key must be the one pruned"
            );
            assert!(Uids::<Test>::contains_key(NetUid::ROOT, staked_hotkey));
        }
    });
}

/// Root admission with a full senate: a registrant may only evict a seat that holds no more
/// root stake than it does. Zero-stake keys therefore cannot walk the eviction ladder
/// through staked validators, the burn is not charged on refusal, and a registrant that does
/// out-stake the lowest seat evicts exactly that seat.
/// SKIP_WASM_BUILD=1 cargo test --package pallet-subtensor --lib -- tests::claim_root::test_root_register_requires_registrant_to_out_stake_pruned_seat --exact
#[test]
fn test_root_register_requires_registrant_to_out_stake_pruned_seat() {
    new_test_ext(1).execute_with(|| {
        const TAO: u64 = 1_000_000_000;
        add_network(NetUid::ROOT, 100, 0);
        // Deliberately unsorted stakes so eviction order proves stake ordering.
        let stakes_tao: [u64; 8] = [500, 100, 300, 200, 800, 50, 400, 600];
        SubtensorModule::set_max_allowed_uids(NetUid::ROOT, stakes_tao.len() as u16);
        SubtensorModule::set_max_registrations_per_block(NetUid::ROOT, u16::MAX);
        SubtensorModule::set_target_registrations_per_interval(NetUid::ROOT, u16::MAX / 3);
        SubtensorModule::set_min_burn(NetUid::ROOT, TaoBalance::ZERO);
        SubtensorModule::set_burn(NetUid::ROOT, TaoBalance::ZERO);
        let mut incumbents = Vec::new();
        for (i, stake) in stakes_tao.iter().enumerate() {
            let coldkey = U256::from(10_000 + i as u64);
            let hotkey = U256::from(20_000 + i as u64);
            root_register_ok(hotkey, coldkey);
            mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &coldkey,
                NetUid::ROOT,
                AlphaBalance::from(stake * TAO),
            );
            incumbents.push(hotkey);
        }
        assert_eq!(
            SubnetworkN::<Test>::get(NetUid::ROOT) as usize,
            stakes_tao.len()
        );

        // Age every seat past the (mainnet) immunity period, then mainnet admission params.
        run_to_block(7_300);
        SubtensorModule::set_max_registrations_per_block(NetUid::ROOT, 1);
        SubtensorModule::set_target_registrations_per_interval(NetUid::ROOT, 2);
        SubtensorModule::set_immunity_period(NetUid::ROOT, 7200);
        SubtensorModule::set_min_burn(NetUid::ROOT, TaoBalance::from(TAO));
        SubtensorModule::set_burn(NetUid::ROOT, TaoBalance::from(TAO));

        // A zero-stake registrant is refused and not charged; every incumbent keeps its seat.
        let attacker = U256::from(777);
        add_balance_to_coldkey_account(&attacker, TaoBalance::from(1_000 * TAO));
        let balance_before = SubtensorModule::get_coldkey_balance(&attacker);
        for i in 0..stakes_tao.len() as u64 {
            let sybil = U256::from(30_000 + i);
            assert_err!(
                SubtensorModule::root_register(RuntimeOrigin::signed(attacker), sybil),
                Error::<Test>::StakeTooLowForRoot
            );
            step_block(1);
        }
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&attacker),
            balance_before
        );
        for hotkey in &incumbents {
            assert!(Uids::<Test>::contains_key(NetUid::ROOT, hotkey));
        }

        // A registrant with 60 TAO out-stakes only the 50 TAO seat, which it evicts.
        let mid_coldkey = U256::from(40_000);
        let mid_hotkey = U256::from(40_001);
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &mid_coldkey,
            &mid_hotkey
        ));
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &mid_hotkey,
            &mid_coldkey,
            NetUid::ROOT,
            AlphaBalance::from(60 * TAO),
        );
        let victim_uid = SubtensorModule::get_root_neuron_to_prune().expect("candidate");
        let victim_hotkey = Keys::<Test>::get(NetUid::ROOT, victim_uid);
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_on_subnet(&victim_hotkey, NetUid::ROOT),
            AlphaBalance::from(50 * TAO)
        );
        root_register_ok(mid_hotkey, mid_coldkey);
        assert_eq!(Keys::<Test>::get(NetUid::ROOT, victim_uid), mid_hotkey);
        assert!(!Uids::<Test>::contains_key(NetUid::ROOT, victim_hotkey));
        step_block(1);

        // The next lowest non-immune seat holds 100 TAO: another 60 TAO registrant is refused.
        let low_coldkey = U256::from(40_002);
        let low_hotkey = U256::from(40_003);
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &low_coldkey,
            &low_hotkey
        ));
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &low_hotkey,
            &low_coldkey,
            NetUid::ROOT,
            AlphaBalance::from(60 * TAO),
        );
        add_balance_to_coldkey_account(&low_coldkey, TaoBalance::from(100 * TAO));
        assert_err!(
            SubtensorModule::root_register(RuntimeOrigin::signed(low_coldkey), low_hotkey),
            Error::<Test>::StakeTooLowForRoot
        );

        // A whale evicts the lowest non-immune seat as before.
        let whale_coldkey = U256::from(40_004);
        let whale_hotkey = U256::from(40_005);
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &whale_coldkey,
            &whale_hotkey
        ));
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &whale_hotkey,
            &whale_coldkey,
            NetUid::ROOT,
            AlphaBalance::from(10_000 * TAO),
        );
        let victim_uid = SubtensorModule::get_root_neuron_to_prune().expect("candidate");
        let victim_hotkey = Keys::<Test>::get(NetUid::ROOT, victim_uid);
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_on_subnet(&victim_hotkey, NetUid::ROOT),
            AlphaBalance::from(100 * TAO)
        );
        root_register_ok(whale_hotkey, whale_coldkey);
        assert_eq!(Keys::<Test>::get(NetUid::ROOT, victim_uid), whale_hotkey);
        assert_eq!(
            incumbents
                .iter()
                .filter(|hotkey| Uids::<Test>::contains_key(NetUid::ROOT, *hotkey))
                .count(),
            stakes_tao.len() - 2,
            "only the 50 and 100 TAO seats were displaced"
        );
    });
}

/// Root's per-interval registration cap binds once per root tempo. The counter used to be
/// reset every block because it keyed off an epoch anchor root never advances.
/// SKIP_WASM_BUILD=1 cargo test --package pallet-subtensor --lib -- tests::claim_root::test_root_register_interval_cap_binds_per_tempo --exact
#[test]
fn test_root_register_interval_cap_binds_per_tempo() {
    new_test_ext(1).execute_with(|| {
        const TAO: u64 = 1_000_000_000;
        add_network(NetUid::ROOT, 100, 0);
        SubtensorModule::set_max_allowed_uids(NetUid::ROOT, 64);
        SubtensorModule::set_min_burn(NetUid::ROOT, TaoBalance::ZERO);
        SubtensorModule::set_burn(NetUid::ROOT, TaoBalance::ZERO);
        SubtensorModule::set_max_registrations_per_block(NetUid::ROOT, 1);
        SubtensorModule::set_target_registrations_per_interval(NetUid::ROOT, 2);
        let cap = 3 * SubtensorModule::get_target_registrations_per_interval(NetUid::ROOT);
        let coldkey = U256::from(3001);
        add_balance_to_coldkey_account(&coldkey, TaoBalance::from(1_000 * TAO));

        // Start just after a tempo boundary. Root never runs an epoch, so its epoch anchor
        // stays where it was set at creation for the whole test.
        run_to_block(201);
        let stale_anchor = LastEpochBlock::<Test>::get(NetUid::ROOT);

        let mut accepted = 0u16;
        let mut refused = 0u16;
        for i in 0..20u64 {
            let result = SubtensorModule::root_register(
                RuntimeOrigin::signed(coldkey),
                U256::from(4000 + i),
            );
            match result {
                Ok(()) => accepted += 1,
                Err(error) => {
                    assert_eq!(
                        error,
                        Error::<Test>::TooManyRegistrationsThisInterval.into()
                    );
                    refused += 1;
                }
            }
            assert!(
                RegistrationsThisInterval::<Test>::get(NetUid::ROOT) <= cap,
                "interval counter must never exceed the cap"
            );
            step_block(1);
        }
        assert_eq!(accepted, cap, "exactly 3x target registrations per tempo");
        assert_eq!(refused, 20 - cap);
        assert_eq!(RegistrationsThisInterval::<Test>::get(NetUid::ROOT), cap);
        assert_eq!(LastEpochBlock::<Test>::get(NetUid::ROOT), stale_anchor);

        // The counter resets on the next tempo boundary and admission resumes.
        run_to_block(300);
        assert_eq!(RegistrationsThisInterval::<Test>::get(NetUid::ROOT), 0);
        assert_ok!(SubtensorModule::root_register(
            RuntimeOrigin::signed(coldkey),
            U256::from(5000)
        ));
        assert_eq!(RegistrationsThisInterval::<Test>::get(NetUid::ROOT), 1);

        // A zero tempo resets the counter every block instead of locking root.
        Tempo::<Test>::insert(NetUid::ROOT, 0);
        step_block(1);
        assert_eq!(RegistrationsThisInterval::<Test>::get(NetUid::ROOT), 0);
    });
}

/// A just-registered zero-stake key is immune: the next registration evicts an
/// older zero-stake member instead.
#[test]
fn test_root_register_skips_immune_when_pruning() {
    new_test_ext(1).execute_with(|| {
        let staked_coldkey = U256::from(2101);
        let staked_hotkey = U256::from(2102);
        let old_zero_coldkey = U256::from(2103);
        let old_zero_hotkey = U256::from(2104);
        let new_zero_coldkey = U256::from(2105);
        let new_zero_hotkey = U256::from(2106);
        let incoming_coldkey = U256::from(2107);
        let incoming_hotkey = U256::from(2108);

        add_network(NetUid::ROOT, 10, 0);
        SubtensorModule::set_max_allowed_uids(NetUid::ROOT, 3);
        SubtensorModule::set_max_registrations_per_block(NetUid::ROOT, 10);
        SubtensorModule::set_target_registrations_per_interval(NetUid::ROOT, 10);
        SubtensorModule::set_immunity_period(NetUid::ROOT, 100);

        root_register_ok(old_zero_hotkey, old_zero_coldkey);
        step_block(101);
        assert!(!SubtensorModule::get_neuron_is_immune(
            NetUid::ROOT,
            Uids::<Test>::get(NetUid::ROOT, old_zero_hotkey).expect("old zero uid")
        ));

        root_register_ok(staked_hotkey, staked_coldkey);
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &staked_hotkey,
            &staked_coldkey,
            NetUid::ROOT,
            5_000_000u64.into(),
        );
        root_register_ok(new_zero_hotkey, new_zero_coldkey);
        assert_eq!(SubnetworkN::<Test>::get(NetUid::ROOT), 3);
        assert!(SubtensorModule::get_neuron_is_immune(
            NetUid::ROOT,
            Uids::<Test>::get(NetUid::ROOT, new_zero_hotkey).expect("new zero uid")
        ));

        root_register_ok(incoming_hotkey, incoming_coldkey);
        assert!(
            !Uids::<Test>::contains_key(NetUid::ROOT, old_zero_hotkey),
            "older zero-stake key must be pruned"
        );
        assert!(
            Uids::<Test>::contains_key(NetUid::ROOT, new_zero_hotkey),
            "just-registered zero-stake key is immune"
        );
        assert!(Uids::<Test>::contains_key(NetUid::ROOT, staked_hotkey));
        assert!(Uids::<Test>::contains_key(NetUid::ROOT, incoming_hotkey));
    });
}
