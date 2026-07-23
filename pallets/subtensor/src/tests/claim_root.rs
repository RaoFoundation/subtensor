#![allow(clippy::expect_used, clippy::unwrap_used)]

use crate::tests::mock::*;
use crate::{
    BasketClaimed, BasketRate, BasketShares, DefaultMinRootClaimAmount, Error, Keys,
    MAX_ROOT_CLAIM_THRESHOLD, NetworksAdded, NumStakingColdkeys, RootClaimableThreshold,
    StakingColdkeys, StakingColdkeysByIndex, SubnetAlphaIn, SubnetMovingPrice, SubnetProtocolFlow,
    SubnetTAO, SubnetworkN, Tempo, TotalStake, Uids, Weights,
};
use approx::assert_abs_diff_eq;
use frame_support::dispatch::RawOrigin;
use frame_support::pallet_prelude::Weight;
use frame_support::traits::Get;
use frame_support::{assert_err, assert_noop, assert_ok};
use sp_core::U256;
use sp_runtime::DispatchError;
use substrate_fixed::types::I96F32;
use subtensor_runtime_common::{AlphaBalance, NetUid, NetUidStorageIndex, TaoBalance, Token};

// =============================================================================
// Helpers
// =============================================================================

/// Directly assign a root UID and a beta-basket weight vector `w` to a validator hotkey,
/// bypassing the `set_root_weights` extrinsic's validation (which is exercised separately).
/// `dests` are `(subnet, weight)` pairs.
fn set_root_weights_direct(hotkey: &U256, uid: u16, dests: &[(NetUid, u16)]) {
    Uids::<Test>::insert(NetUid::ROOT, hotkey, uid);
    let zipped: Vec<(u16, u16)> = dests.iter().map(|(n, w)| (u16::from(*n), *w)).collect();
    Weights::<Test>::insert(NetUidStorageIndex::ROOT, uid, zipped);
}

/// Ensure a subnet has deep, balanced AMM reserves so basket swaps execute with negligible
/// slippage and never fail for lack of liquidity.
fn fund_pool(netuid: NetUid) {
    SubnetTAO::<Test>::insert(netuid, TaoBalance::from(1_000_000_000_000u64));
    SubnetAlphaIn::<Test>::insert(netuid, AlphaBalance::from(1_000_000_000_000u64));
}

/// Claims are fund-level and consult only the ROOT threshold entry; zero it for tests that
/// exercise small claims.
fn zero_claim_threshold() {
    RootClaimableThreshold::<Test>::insert(NetUid::ROOT, I96F32::from_num(0));
}

fn escrow_alpha(hotkey: &U256, netuid: NetUid) -> u64 {
    let escrow = SubtensorModule::get_beta_escrow_account_id();
    SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, &escrow, netuid).to_u64()
}

fn fund_shares(hotkey: &U256) -> u64 {
    BasketShares::<Test>::get(hotkey)
}

fn has_fund(hotkey: &U256) -> bool {
    BasketRate::<Test>::get(hotkey) > I96F32::from_num(0)
}

fn root_stake_of(hotkey: &U256, coldkey: &U256) -> u64 {
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

// =============================================================================
// Beta basket: setting weights (extrinsic validation)
// =============================================================================

#[test]
fn test_set_root_weights_rejects_unregistered_hotkey() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&hotkey, &owner_coldkey);

        // `hotkey` is not registered on the root subnet, so it cannot set root weights.
        assert_noop!(
            SubtensorModule::set_root_weights(
                RuntimeOrigin::signed(hotkey),
                vec![u16::from(netuid)],
                vec![u16::MAX],
                0,
            ),
            Error::<Test>::HotKeyNotRegisteredInSubNet
        );
    });
}

// =============================================================================
// Beta basket: accrual
// =============================================================================

#[test]
fn test_root_basket_accrues_per_weights() {
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

        // Route the basket 100% back into this subnet.
        set_root_weights_direct(&hotkey, 0, &[(netuid, u16::MAX)]);

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

        // Fund shares minted, escrow holds the basket alpha, and a claimable rate exists.
        assert!(fund_shares(&hotkey) > 0);
        assert!(escrow_alpha(&hotkey, netuid) > 0);
        assert!(has_fund(&hotkey));

        // At a ~1:1 pool price the fund NAV and outstanding shares should match (N/P starts
        // at 1): the escrow alpha marked at ~1 equals the TAO-denominated shares.
        assert_abs_diff_eq!(
            SubtensorModule::get_validator_basket_nav_tao(&hotkey).to_u64(),
            fund_shares(&hotkey),
            epsilon = 10u64,
        );
    });
}

#[test]
fn test_root_basket_recycles_without_weights() {
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

        // No root weights set for the validator.
        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );

        // Without weights the root dividend is recycled: no basket, no claimable.
        assert_eq!(escrow_alpha(&hotkey, netuid), 0);
        assert_eq!(fund_shares(&hotkey), 0);
        assert!(!has_fund(&hotkey));
    });
}

#[test]
fn test_root_basket_routes_to_target_subnet() {
    new_test_ext(1).execute_with(|| {
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

        // Route the basket entirely into subnet B (different from the dividend origin A).
        set_root_weights_direct(&hotkey, 0, &[(netuid_b, u16::MAX)]);

        SubtensorModule::distribute_emission(
            netuid_a,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );

        // The holding should be on B, not A; the fund is denominated at the validator level.
        assert!(escrow_alpha(&hotkey, netuid_b) > 0);
        assert_eq!(escrow_alpha(&hotkey, netuid_a), 0);
        assert!(fund_shares(&hotkey) > 0);
        assert!(has_fund(&hotkey));
    });
}

// =============================================================================
// Beta basket: protocol-flow accounting (symmetric)
// =============================================================================

/// The basket must book protocol flow symmetrically: the origin sell on A is an outflow, each
/// redistribution buy on B/C is an inflow, and the claim sell on B/C is an outflow that nets the
/// deposit-then-claim round-trip back toward zero on the dest pools.
#[test]
fn test_root_basket_records_symmetric_protocol_flow() {
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

        // Split the basket 50/50 across B and C (neither is the dividend origin A).
        set_root_weights_direct(&hotkey, 0, &[(netuid_b, u16::MAX), (netuid_c, u16::MAX)]);

        // No protocol flow has been recorded on any subnet yet.
        assert_eq!(SubnetProtocolFlow::<Test>::get(netuid_a), 0);
        assert_eq!(SubnetProtocolFlow::<Test>::get(netuid_b), 0);
        assert_eq!(SubnetProtocolFlow::<Test>::get(netuid_c), 0);

        SubtensorModule::distribute_emission(
            netuid_a,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );

        let flow_a = SubnetProtocolFlow::<Test>::get(netuid_a);
        let flow_b = SubnetProtocolFlow::<Test>::get(netuid_b);
        let flow_c = SubnetProtocolFlow::<Test>::get(netuid_c);

        // Origin sell on A is booked as an outflow (negative); the buys on B and C as inflows.
        assert!(flow_a < 0, "origin sell must be an outflow, got {flow_a}");
        assert!(flow_b > 0, "buy on B must be an inflow, got {flow_b}");
        assert!(flow_c > 0, "buy on C must be an inflow, got {flow_c}");

        // Symmetry: every TAO sold on A is spent buying on B and C, so the inflows exactly offset
        // the outflow across subnets.
        assert_abs_diff_eq!(flow_b + flow_c, -flow_a, epsilon = 10i64);

        // Now redeem the basket. The fund-level claim sells the staker's pro-rata slice of BOTH
        // holdings back to TAO, booking an outflow on each dest that nets the round-trip back
        // toward zero.
        assert_ok!(SubtensorModule::claim_root(RuntimeOrigin::signed(coldkey)));

        let flow_b_after = SubnetProtocolFlow::<Test>::get(netuid_b);
        let flow_c_after = SubnetProtocolFlow::<Test>::get(netuid_c);

        // Claim recorded an outflow: the dest flow decreased, and the deposit+claim round-trip
        // leaves a residual far smaller than the original inflow (only swap fees/slippage remain).
        assert!(
            flow_b_after < flow_b,
            "claim must book an outflow on B: {flow_b_after} !< {flow_b}"
        );
        assert!(
            flow_c_after < flow_c,
            "claim must book an outflow on C: {flow_c_after} !< {flow_c}"
        );
        assert!(
            flow_b_after.abs() < flow_b,
            "round-trip residual on B should be smaller than the inflow: {flow_b_after} vs {flow_b}"
        );
        assert!(
            flow_c_after.abs() < flow_c,
            "round-trip residual on C should be smaller than the inflow: {flow_c_after} vs {flow_c}"
        );
    });
}

// =============================================================================
// Beta basket: claiming (pro-rata fund redemption, swapped to root TAO)
// =============================================================================

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

        set_root_weights_direct(&hotkey, 0, &[(netuid, u16::MAX)]);

        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );

        let shares_before = fund_shares(&hotkey);
        assert!(shares_before > 0);
        let root_before = root_stake_of(&hotkey, &coldkey);
        assert_eq!(root_before, root_stake);

        // Claim: the staker's owed fraction of the fund is sold to TAO and staked on root.
        assert_ok!(SubtensorModule::claim_root(RuntimeOrigin::signed(coldkey)));

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

        set_root_weights_direct(&hotkey, 0, &[(netuid, u16::MAX)]);

        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            10_000_000u64.into(),
            AlphaBalance::ZERO,
        );

        let alice_before = root_stake_of(&hotkey, &alice);
        let bob_before = root_stake_of(&hotkey, &bob);

        assert_ok!(SubtensorModule::claim_root(RuntimeOrigin::signed(alice)));
        assert_ok!(SubtensorModule::claim_root(RuntimeOrigin::signed(bob)));

        let alice_gain = root_stake_of(&hotkey, &alice).saturating_sub(alice_before);
        let bob_gain = root_stake_of(&hotkey, &bob).saturating_sub(bob_before);

        assert!(alice_gain > 0);
        // Equal root stake => equal basket payout (small AMM slippage between the two
        // sequential claims on the same pool).
        assert_abs_diff_eq!(alice_gain, bob_gain, epsilon = 1_000u64);
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

        set_root_weights_direct(&hotkey, 0, &[(netuid, u16::MAX)]);

        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );

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

        set_root_weights_direct(&hotkey, 0, &[(netuid, u16::MAX)]);

        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );

        let subnet_holding = escrow_alpha(&hotkey, netuid);
        assert!(subnet_holding > 0);
        assert_eq!(escrow_alpha(&hotkey, NetUid::ROOT), 0);
        let shares_before = fund_shares(&hotkey);

        // Dissolving the subnet converts the holding into the fund's root (TAO) slot: shares,
        // rates, and watermarks are untouched — NAV is continuous minus slippage.
        assert_ok!(SubtensorModule::do_dissolve_network(netuid));

        assert_eq!(escrow_alpha(&hotkey, netuid), 0);
        assert!(
            escrow_alpha(&hotkey, NetUid::ROOT) > 0,
            "holding must be converted to the fund's root slot"
        );
        assert_eq!(fund_shares(&hotkey), shares_before);
        assert!(has_fund(&hotkey));

        // The staker's claim survives dissolution and is redeemable from the root slot.
        let root_before = root_stake_of(&hotkey, &coldkey);
        assert_ok!(SubtensorModule::claim_root(RuntimeOrigin::signed(coldkey)));
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
        set_root_weights_direct(&hotkey, 0, &[(netuid, u16::MAX)]);

        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );
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
        assert_eq!(
            SubtensorModule::get_basket_owed_shares(&hotkey, &bob),
            0
        );

        assert_ok!(SubtensorModule::do_dissolve_network(netuid));

        // Owed entitlements are untouched by the conversion.
        assert!(SubtensorModule::get_basket_owed_shares(&hotkey, &alice) > 0);
        assert_eq!(
            SubtensorModule::get_basket_owed_shares(&hotkey, &bob),
            0
        );

        let alice_before = root_stake_of(&hotkey, &alice);
        let bob_before = root_stake_of(&hotkey, &bob);

        assert_ok!(SubtensorModule::claim_root(RuntimeOrigin::signed(alice)));
        assert_ok!(SubtensorModule::claim_root(RuntimeOrigin::signed(bob)));

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
        set_root_weights_direct(&hotkey, 0, &[(netuid, u16::MAX)]);

        // --- Distribution must not move TotalStake (sell + rebuy is TAO-neutral).
        let ts_before_distribute = TotalStake::<Test>::get().to_u64();
        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        let ts_after_distribute = TotalStake::<Test>::get().to_u64();
        assert_eq!(
            ts_before_distribute, ts_after_distribute,
            "distribution must be TotalStake-neutral"
        );

        // --- Redemption must also be TotalStake-neutral (swap out then stake on root).
        let ts_before_claim = TotalStake::<Test>::get().to_u64();
        assert_ok!(SubtensorModule::claim_root(RuntimeOrigin::signed(coldkey)));
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
        set_root_weights_direct(&hotkey, 0, &[(netuid, u16::MAX)]);

        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );

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
        assert_ok!(SubtensorModule::claim_root(RuntimeOrigin::signed(coldkey)));
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
        set_root_weights_direct(&hotkey, 0, &[(netuid, u16::MAX)]);

        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            10_000_000u64.into(),
            AlphaBalance::ZERO,
        );

        let escrow_filled = escrow_alpha(&hotkey, netuid);
        assert!(escrow_filled > 0);

        assert_ok!(SubtensorModule::claim_root(RuntimeOrigin::signed(alice)));
        assert_ok!(SubtensorModule::claim_root(RuntimeOrigin::signed(bob)));

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
        set_root_weights_direct(&hotkey, 0, &[(netuid, u16::MAX)]);

        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            10_000_000u64.into(),
            AlphaBalance::ZERO,
        );

        let alice_before = root_stake_of(&hotkey, &alice);
        let bob_before = root_stake_of(&hotkey, &bob);

        assert_ok!(SubtensorModule::claim_root(RuntimeOrigin::signed(alice)));
        assert_ok!(SubtensorModule::claim_root(RuntimeOrigin::signed(bob)));

        let alice_gain = root_stake_of(&hotkey, &alice).saturating_sub(alice_before);
        let bob_gain = root_stake_of(&hotkey, &bob).saturating_sub(bob_before);

        assert!(alice_gain > 0);
        // Bob staked 2x => ~2x payout (small AMM slippage between sequential claims).
        assert_abs_diff_eq!(bob_gain, 2 * alice_gain, epsilon = 2_000u64);
    });
}

/// A weight vector that spans multiple subnets splits the basket across them in proportion
/// to the weights.
#[test]
fn test_root_basket_splits_across_multiple_subnets() {
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

        // 50/50 split between B and C (neither is the origin A).
        set_root_weights_direct(&hotkey, 0, &[(netuid_b, u16::MAX), (netuid_c, u16::MAX)]);

        SubtensorModule::distribute_emission(
            netuid_a,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            10_000_000u64.into(),
            AlphaBalance::ZERO,
        );

        let basket_b = escrow_alpha(&hotkey, netuid_b);
        let basket_c = escrow_alpha(&hotkey, netuid_c);

        assert!(basket_b > 0 && basket_c > 0, "both targets must be funded");
        assert_eq!(
            escrow_alpha(&hotkey, netuid_a),
            0,
            "origin must hold nothing"
        );
        // Equal weights + equal-depth pools => ~equal split.
        assert_abs_diff_eq!(basket_b, basket_c, epsilon = 1_000u64);
    });
}

/// The `set_root_weights` extrinsic stores the validator's vector under the root weights index.
#[test]
fn test_set_root_weights_stores_vector() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let coldkey = U256::from(1003);
        let netuid = add_dynamic_network(&hotkey, &owner_coldkey);

        // Register the validator on root (uid 0) and give it stake.
        NetworksAdded::<Test>::insert(NetUid::ROOT, true);
        SubnetworkN::<Test>::insert(NetUid::ROOT, 1);
        Uids::<Test>::insert(NetUid::ROOT, hotkey, 0u16);
        Keys::<Test>::insert(NetUid::ROOT, 0u16, hotkey);
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            NetUid::ROOT,
            2_000_000u64.into(),
        );

        assert_ok!(SubtensorModule::set_root_weights(
            RuntimeOrigin::signed(hotkey),
            vec![u16::from(netuid)],
            vec![u16::MAX],
            0,
        ));

        let stored = Weights::<Test>::get(NetUidStorageIndex::ROOT, 0u16);
        assert_eq!(stored, vec![(u16::from(netuid), u16::MAX)]);
    });
}

/// The `set_root_weights` extrinsic accepts root (uid 0) as a basket destination, so the
/// held-as-root-TAO slot is reachable through the real on-chain path (not just direct storage
/// writes). Producer validation must agree with the `distribute_root_alpha_to_basket` consumer.
#[test]
fn test_set_root_weights_accepts_root_destination() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hotkey = U256::from(1002);
        let coldkey = U256::from(1003);
        let netuid = add_dynamic_network(&hotkey, &owner_coldkey);

        NetworksAdded::<Test>::insert(NetUid::ROOT, true);
        SubnetworkN::<Test>::insert(NetUid::ROOT, 1);
        Uids::<Test>::insert(NetUid::ROOT, hotkey, 0u16);
        Keys::<Test>::insert(NetUid::ROOT, 0u16, hotkey);
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            NetUid::ROOT,
            2_000_000u64.into(),
        );

        // A vector mixing root (uid 0) and a subnet is accepted and stored verbatim.
        assert_ok!(SubtensorModule::set_root_weights(
            RuntimeOrigin::signed(hotkey),
            vec![u16::from(NetUid::ROOT), u16::from(netuid)],
            vec![u16::MAX, u16::MAX],
            0,
        ));

        let stored = Weights::<Test>::get(NetUidStorageIndex::ROOT, 0u16);
        assert_eq!(
            stored,
            vec![
                (u16::from(NetUid::ROOT), u16::MAX),
                (u16::from(netuid), u16::MAX)
            ]
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
        set_root_weights_direct(&hotkey, 0, &[(netuid, u16::MAX)]);

        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );

        // Dividend distribution did not touch the staker's root principal.
        assert_eq!(root_stake_of(&hotkey, &coldkey), principal);

        // Claiming only adds TAO to the root principal (never subtracts).
        assert_ok!(SubtensorModule::claim_root(RuntimeOrigin::signed(coldkey)));
        assert!(root_stake_of(&hotkey, &coldkey) >= principal);
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
        set_root_weights_direct(&hotkey, 0, &[(netuid, u16::MAX)]);

        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );

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
        set_root_weights_direct(&hotkey, 0, &[(netuid, u16::MAX)]);

        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );

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
        set_root_weights_direct(&hotkey, 0, &[(netuid, u16::MAX)]);

        // Alice accrues a basket.
        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );

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
        set_root_weights_direct(&hotkey, 0, &[(netuid, u16::MAX)]);

        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );

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
        set_root_weights_direct(&hotkey, 0, &[(netuid, u16::MAX)]);

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
        assert_ok!(SubtensorModule::claim_root(RuntimeOrigin::signed(coldkey)));
        assert!(root_stake_of(&hotkey, &coldkey) > root_before);
    });
}

// =============================================================================
// Beta basket: root (UID 0) slot — "opt out of subnets, hold yield as root TAO"
// =============================================================================

/// A root-weighted (UID 0) slice is held as root stake under the escrow at 1:1, minting fund
/// shares, and is TotalStake-neutral (the origin sell is balanced by the root-stake credit —
/// no swap, since root has no AMM pool).
#[test]
fn test_root_basket_uid0_holds_as_root_stake() {
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

        // Validator opts out of subnets: 100% of the basket weight on root (UID 0).
        set_root_weights_direct(&hotkey, 0, &[(NetUid::ROOT, u16::MAX)]);

        assert_eq!(escrow_alpha(&hotkey, NetUid::ROOT), 0);
        assert_eq!(fund_shares(&hotkey), 0);

        let ts_before = TotalStake::<Test>::get().to_u64();
        let pending_root_alpha = 1_000_000u64;
        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            pending_root_alpha.into(),
            AlphaBalance::ZERO,
        );
        let ts_after = TotalStake::<Test>::get().to_u64();

        // A root slot now exists: shares minted, escrow holds root stake, claimable rate set.
        let escrow_root = escrow_alpha(&hotkey, NetUid::ROOT);
        let shares = fund_shares(&hotkey);
        assert!(escrow_root > 0, "escrow must hold root stake");
        assert!(shares > 0, "fund shares must be minted");
        assert!(has_fund(&hotkey));

        // Held at 1:1 (N/P starts at 1): escrow root stake ~= minted shares.
        assert_abs_diff_eq!(escrow_root, shares, epsilon = 10u64);

        // No subnet alpha was bought for the root slice (no subnet escrow position created).
        assert_eq!(escrow_alpha(&hotkey, netuid), 0);

        // Sell-origin then credit-to-root nets to zero: distribution is TotalStake-neutral.
        assert_eq!(ts_before, ts_after, "root deposit must be TotalStake-neutral");
    });
}

/// Redeeming a root slot reassigns the escrow's root stake to the staker: the staker's root
/// stake grows, the escrow drains, shares are consumed, and it is TotalStake-neutral (no swap,
/// no minted TAO — total root stake is conserved, just moved between coldkeys).
#[test]
fn test_root_basket_uid0_claim_reassigns_no_swap() {
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
        set_root_weights_direct(&hotkey, 0, &[(NetUid::ROOT, u16::MAX)]);

        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );

        let shares_before = fund_shares(&hotkey);
        let escrow_before = escrow_alpha(&hotkey, NetUid::ROOT);
        let root_before = root_stake_of(&hotkey, &coldkey);
        assert!(shares_before > 0);
        assert!(escrow_before > 0);

        let ts_before = TotalStake::<Test>::get().to_u64();
        assert_ok!(SubtensorModule::claim_root(RuntimeOrigin::signed(coldkey)));
        let ts_after = TotalStake::<Test>::get().to_u64();

        let gain = root_stake_of(&hotkey, &coldkey).saturating_sub(root_before);
        let escrow_after = escrow_alpha(&hotkey, NetUid::ROOT);

        // Staker gained root stake; the escrow gave up ~the same amount (a pure reassignment).
        assert!(gain > 0, "staker must accumulate root TAO");
        assert_abs_diff_eq!(
            gain,
            escrow_before.saturating_sub(escrow_after),
            epsilon = 10u64
        );

        // Shares consumed, watermark advanced, TotalStake untouched (no swap, no mint).
        assert!(fund_shares(&hotkey) < shares_before);
        assert!(BasketClaimed::<Test>::get(hotkey, coldkey) > 0);
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
        set_root_weights_direct(&hotkey, 0, &[(NetUid::ROOT, u16::MAX)]);

        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );

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
        assert_ok!(SubtensorModule::claim_root(RuntimeOrigin::signed(coldkey)));
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
        set_root_weights_direct(&hotkey, 0, &[(netuid_b, 32768), (NetUid::ROOT, 32768)]);

        let ts_start = TotalStake::<Test>::get().to_u64();
        let deposit = |amount: u64| {
            SubtensorModule::distribute_emission(
                netuid_a,
                AlphaBalance::ZERO,
                AlphaBalance::ZERO,
                amount.into(),
                AlphaBalance::ZERO,
            );
        };

        // Interleave deposits and claims.
        deposit(1_000_000);
        assert_ok!(SubtensorModule::claim_root(RuntimeOrigin::signed(alice)));
        deposit(2_000_000);
        assert_ok!(SubtensorModule::claim_root(RuntimeOrigin::signed(bob)));
        deposit(1_500_000);

        // Final round: everyone claims everything.
        for ck in [alice, bob, carol] {
            assert_ok!(SubtensorModule::claim_root(RuntimeOrigin::signed(ck)));
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
        set_root_weights_direct(&hotkey, 0, &[(netuid, u16::MAX)]);

        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );

        assert_ok!(SubtensorModule::claim_root(RuntimeOrigin::signed(coldkey)));

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
            assert_ok!(SubtensorModule::claim_root(RuntimeOrigin::signed(coldkey)));
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
        set_root_weights_direct(&hotkey, 0, &[(netuid, u16::MAX)]);

        // Alice accrues the first deposit alone.
        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );
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
        set_root_weights_direct(&hotkey, 0, &[(netuid, u16::MAX)]);

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
        // which is stake-proportional and not what is under test): N/P == 1, so ~2e13 shares
        // must be minted.
        let dividend = 20_000_000_000_000u64; // 2e13
        SubtensorModule::distribute_root_alpha_to_basket(&hotkey, netuid, dividend.into());

        let minted = fund_shares(&hotkey).saturating_sub(fund_scale);
        assert!(
            minted > dividend / 2 && minted < dividend * 2,
            "mint saturated or mispriced: minted {minted} for a {dividend} deposit at N/P=1"
        );
        assert_abs_diff_eq!(minted, dividend, epsilon = dividend / 100);
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
        set_root_weights_direct(&hotkey, 0, &[(netuid, u16::MAX)]);

        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );

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

        // And it remains fully claimable.
        let root_before = root_stake_of(&hotkey, &coldkey);
        assert_ok!(SubtensorModule::claim_root(RuntimeOrigin::signed(coldkey)));
        let gain = root_stake_of(&hotkey, &coldkey).saturating_sub(root_before);
        assert_abs_diff_eq!(gain, payout_before, epsilon = 100u64);
    });
}

/// Pro-rata redemption preserves fund composition: after one of two equal stakers claims from a
/// fund with a 2:1 split across two subnets, the ratio between the remaining holdings is
/// unchanged, and the second claimant's payout matches the first (no ordering advantage beyond
/// AMM slippage).
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

        // 2:1 composition across B and C.
        set_root_weights_direct(&hotkey, 0, &[(netuid_b, 43690), (netuid_c, 21845)]);

        SubtensorModule::distribute_emission(
            netuid_a,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            6_000_000u64.into(),
            AlphaBalance::ZERO,
        );

        let b_before = escrow_alpha(&hotkey, netuid_b) as f64;
        let c_before = escrow_alpha(&hotkey, netuid_c) as f64;
        assert!(b_before > 0.0 && c_before > 0.0);
        let ratio_before = b_before / c_before;

        // Alice (half the shares) claims.
        let alice_root_before = root_stake_of(&hotkey, &alice);
        assert_ok!(SubtensorModule::claim_root(RuntimeOrigin::signed(alice)));
        let alice_gain = root_stake_of(&hotkey, &alice).saturating_sub(alice_root_before);

        // Composition is preserved: both holdings shrank by the same fraction.
        let b_after = escrow_alpha(&hotkey, netuid_b) as f64;
        let c_after = escrow_alpha(&hotkey, netuid_c) as f64;
        let ratio_after = b_after / c_after;
        assert!(
            (ratio_after - ratio_before).abs() / ratio_before < 0.001,
            "claim skewed composition: {ratio_before} -> {ratio_after}"
        );

        // Bob's payout matches Alice's (equal stakes), modulo slippage from her claim.
        let bob_root_before = root_stake_of(&hotkey, &bob);
        assert_ok!(SubtensorModule::claim_root(RuntimeOrigin::signed(bob)));
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
        set_root_weights_direct(&hotkey, 0, &[(netuid, u16::MAX)]);

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

/// A claim below the dust threshold is a complete no-op: nothing is consumed, and the full
/// amount remains claimable once the threshold permits.
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
        set_root_weights_direct(&hotkey, 0, &[(netuid, u16::MAX)]);

        // Accrue less than the threshold.
        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            100_000u64.into(),
            AlphaBalance::ZERO,
        );
        RootClaimableThreshold::<Test>::insert(NetUid::ROOT, I96F32::from_num(1_000_000u64));

        let owed_before = SubtensorModule::get_basket_owed_shares(&hotkey, &coldkey);
        let shares_before = fund_shares(&hotkey);
        let escrow_before = escrow_alpha(&hotkey, netuid);
        let root_before = root_stake_of(&hotkey, &coldkey);
        assert!(owed_before > 0);

        // Below threshold: skipped, nothing consumed.
        assert_ok!(SubtensorModule::claim_root(RuntimeOrigin::signed(coldkey)));
        assert_eq!(
            SubtensorModule::get_basket_owed_shares(&hotkey, &coldkey),
            owed_before
        );
        assert_eq!(fund_shares(&hotkey), shares_before);
        assert_eq!(escrow_alpha(&hotkey, netuid), escrow_before);
        assert_eq!(root_stake_of(&hotkey, &coldkey), root_before);

        // Lower the threshold: the full amount pays out.
        zero_claim_threshold();
        assert_ok!(SubtensorModule::claim_root(RuntimeOrigin::signed(coldkey)));
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
        set_root_weights_direct(&hotkey, 0, &[(netuid, u16::MAX)]);

        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );

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
        assert_ok!(SubtensorModule::claim_root(RuntimeOrigin::signed(
            new_coldkey
        )));
        assert!(
            root_stake_of(&hotkey, &new_coldkey) > root_before,
            "new coldkey must be able to realize the carried entitlement"
        );
    });
}

/// A claim whose marked estimate is positive but whose per-holding alpha takes all floor to
/// zero (high-price, tiny-alpha holding) must be a complete no-op: settling would burn the
/// staker's owed shares for a zero payout.
#[test]
fn test_root_basket_zero_realized_claim_burns_nothing() {
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
        let root_before = root_stake_of(&hotkey, &coldkey);

        assert_ok!(SubtensorModule::claim_root(RuntimeOrigin::signed(coldkey)));

        // Complete no-op: no shares burned, no watermark advanced, nothing moved.
        assert_eq!(
            SubtensorModule::get_basket_owed_shares(&hotkey, &coldkey),
            owed_before,
            "owed shares must not be burned for a zero payout"
        );
        assert_eq!(fund_shares(&hotkey), shares_before);
        assert_eq!(escrow_alpha(&hotkey, netuid), escrow_before);
        assert_eq!(root_stake_of(&hotkey, &coldkey), root_before);
        assert_eq!(BasketClaimed::<Test>::get(hotkey, coldkey), 0);
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
        set_root_weights_direct(&hotkey, 0, &[(netuid, u16::MAX)]);

        // Epoch 1: accrue and fully drain.
        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        assert_ok!(SubtensorModule::claim_root(RuntimeOrigin::signed(coldkey)));
        assert!(fund_shares(&hotkey) <= 10, "epoch-1 fund should be drained");

        // Epoch 2: a new deposit into the drained fund.
        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            1_000_000u64.into(),
            AlphaBalance::ZERO,
        );
        let epoch2_value = SubtensorModule::get_validator_basket_nav_tao(&hotkey).to_u64();
        assert!(epoch2_value > 0);

        // The sole staker redeems ~the entire epoch-2 value; nothing was lost to the drained
        // epoch's residual dust.
        let root_before = root_stake_of(&hotkey, &coldkey);
        assert_ok!(SubtensorModule::claim_root(RuntimeOrigin::signed(coldkey)));
        let gain = root_stake_of(&hotkey, &coldkey).saturating_sub(root_before);
        assert_abs_diff_eq!(gain, epoch2_value, epsilon = epoch2_value / 100);
        assert!(fund_shares(&hotkey) <= 20);
    });
}

/// The escrow's own root stake is excluded from the claimant base, so a sole staker's claim
/// stays correct across repeated root deposits (no value is stranded by denominator
/// dilution): after accrual the staker can drain the escrow's root slot to ~zero.
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
        set_root_weights_direct(&hotkey, 0, &[(NetUid::ROOT, u16::MAX)]);

        // Two deposits: the second runs while the escrow already holds root stake from the first.
        // If the escrow's root stake were counted in the claimant base, the second deposit would
        // under-credit the rate and strand value in the escrow.
        for _ in 0..2 {
            SubtensorModule::distribute_emission(
                netuid,
                AlphaBalance::ZERO,
                AlphaBalance::ZERO,
                1_000_000u64.into(),
                AlphaBalance::ZERO,
            );
        }

        let escrow_before = escrow_alpha(&hotkey, NetUid::ROOT);
        assert!(escrow_before > 0);

        assert_ok!(SubtensorModule::claim_root(RuntimeOrigin::signed(coldkey)));

        // The sole real staker drains the whole root slot: no value stranded by the escrow's
        // own root holdings.
        let escrow_after = escrow_alpha(&hotkey, NetUid::ROOT);
        assert!(
            escrow_after <= escrow_before / 1_000 + 10,
            "root slot must drain to ~0; residual {escrow_after} of {escrow_before}"
        );
    });
}
