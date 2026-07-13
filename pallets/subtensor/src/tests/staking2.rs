#![allow(clippy::unwrap_used)]

use frame_support::{
    assert_ok,
    dispatch::{GetDispatchInfo, Pays},
    weights::Weight,
};
use sp_core::U256;
use subtensor_runtime_common::{AlphaBalance, TaoBalance, Token};
use subtensor_swap_interface::SwapHandler;

use super::mock;
use super::mock::*;
use crate::*;

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --workspace --test staking2 -- test_swap_tao_for_alpha_dynamic_mechanism --exact --nocapture
#[test]
fn test_stake_base_case() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let tao_to_swap = TaoBalance::from(1_000_000_000); // 1 TAO

        // Set up the subnet with dynamic mechanism
        SubnetMechanism::<Test>::insert(netuid, 1);

        // Initialize subnet with some existing TAO and Alpha
        let initial_subnet_tao = TaoBalance::from(10_000_000_000_u64); // 10 TAO
        let initial_subnet_alpha = AlphaBalance::from(5_000_000_000_u64); // 5 Alpha
        mock::setup_reserves(netuid, initial_subnet_tao, initial_subnet_alpha);
        SubnetAlphaOut::<Test>::insert(netuid, initial_subnet_alpha);

        // Record initial total stake
        let initial_total_stake = TotalStake::<Test>::get();

        // Perform swap
        let (alpha_expected, fee) = mock::swap_tao_to_alpha(netuid, tao_to_swap);
        let alpha_received = AlphaBalance::from(
            SubtensorModule::swap_tao_for_alpha(
                netuid,
                tao_to_swap,
                <Test as Config>::SwapInterface::max_price(),
                false,
            )
            .unwrap()
            .amount_paid_out,
        );

        // Verify correct alpha calculation using constant product formula
        assert_eq!(
            alpha_received, alpha_expected,
            "Alpha received calculation is incorrect"
        );

        // Check subnet updates
        assert_eq!(
            SubnetTAO::<Test>::get(netuid),
            initial_subnet_tao + tao_to_swap - fee.into(),
            "Subnet TAO not updated correctly"
        );
        assert_eq!(
            SubnetAlphaIn::<Test>::get(netuid),
            initial_subnet_alpha - alpha_received,
            "Subnet Alpha In not updated correctly"
        );
        assert_eq!(
            SubnetAlphaOut::<Test>::get(netuid),
            initial_subnet_alpha + alpha_received,
            "Subnet Alpha Out not updated correctly"
        );

        // Check total stake update
        assert_eq!(
            TotalStake::<Test>::get(),
            initial_total_stake + tao_to_swap,
            "Total stake not updated correctly"
        );
    });
}

// Regression for #2735: when the AMM price limit is hit before the full stake is
// swapped, the unswapped TAO must be refunded to the staker and TotalStake must
// reflect only the consumed portion. Without the fix the remainder is stranded on
// the subnet PalletId account and TotalStake over-counts by the refunded amount.
#[test]
fn test_stake_into_subnet_refunds_unswapped_tao_at_price_limit() {
    new_test_ext(1).execute_with(|| {
        let hotkey = U256::from(561337);
        let coldkey = U256::from(61337);
        let netuid = add_dynamic_network(&hotkey, &coldkey);

        // Symmetric reserves => starting price ~1.0; a 1.1 limit allows only a
        // partial swap of a 1 TAO stake, so a refund must occur.
        SubnetMechanism::<Test>::insert(netuid, 1);
        mock::setup_reserves(
            netuid,
            TaoBalance::from(10_000_000_000_u64),
            AlphaBalance::from(10_000_000_000_u64),
        );
        SubnetAlphaOut::<Test>::insert(netuid, AlphaBalance::from(10_000_000_000_u64));

        let stake = TaoBalance::from(1_000_000_000_u64); // 1 TAO
        add_balance_to_coldkey_account(&coldkey, (2_000_000_000_u64).into());

        let balance_before = SubtensorModule::get_coldkey_balance(&coldkey);
        let total_stake_before = TotalStake::<Test>::get();

        // Tight limit (1.1 RAO/Alpha) so the AMM cannot consume the full stake.
        assert_ok!(SubtensorModule::stake_into_subnet(
            &hotkey,
            &coldkey,
            netuid,
            stake,
            TaoBalance::from(1_100_000_000_u64),
            false,
        ));

        let balance_after = SubtensorModule::get_coldkey_balance(&coldkey);
        let total_stake_after = TotalStake::<Test>::get();

        // Only the consumed TAO (amount_paid_in + fee) may leave the coldkey;
        // the unswapped remainder must be refunded, not stranded.
        let consumed = balance_before.saturating_sub(balance_after);
        assert!(
            consumed < stake,
            "unswapped TAO must be refunded: consumed {consumed:?} >= stake {stake:?}"
        );

        // TotalStake must track the consumed TAO exactly, not the full stake.
        assert_eq!(
            total_stake_after.saturating_sub(total_stake_before),
            consumed,
            "TotalStake must equal consumed TAO (no stranded over-count)"
        );
    });
}

// Test: Share-based Staking System
// This test verifies the functionality of the share-based staking system where:
// 1. Stakes are represented as shares in a pool
// 2. Multiple coldkeys can stake to a single hotkey
// 3. Direct hotkey stakes are distributed proportionally among existing coldkey stakes
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::staking2::test_share_based_staking --exact --show-output
#[test]
fn test_share_based_staking() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let primary_hotkey = U256::from(1);
        let primary_coldkey = U256::from(2);
        let stake_amount = AlphaBalance::from(1_000_000_000); // 1 Alpha stake increase

        // Test Case 1: Initial Stake
        // The first stake should create shares 1:1 with the staked amount
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &primary_hotkey,
            &primary_coldkey,
            netuid,
            stake_amount,
        );
        let initial_stake = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &primary_hotkey,
            &primary_coldkey,
            netuid,
        );
        log::info!(
            "Initial stake: {} = {} + {} = {}",
            initial_stake,
            0,
            stake_amount,
            stake_amount
        );
        assert_eq!(
            initial_stake, stake_amount,
            "Initial stake should match the staked amount exactly"
        );

        // Test Case 2: Additional Stake to Same Account
        // Adding more stake to the same account should increase shares proportionally
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &primary_hotkey,
            &primary_coldkey,
            netuid,
            stake_amount,
        );
        let stake_after_second = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &primary_hotkey,
            &primary_coldkey,
            netuid,
        );
        log::info!(
            "Stake after second deposit: {} = {} + {} = {}",
            stake_after_second,
            initial_stake,
            stake_amount,
            initial_stake + stake_amount
        );
        assert!(
            (stake_after_second.to_u64() as i64 - (initial_stake + stake_amount).to_u64() as i64)
                .abs()
                <= 1,
            "Total stake should double after second deposit (within rounding error)"
        );

        // Test Case 3: Direct Hotkey Stake
        // When staking directly to hotkey, the stake should be distributed proportionally
        SubtensorModule::increase_stake_for_hotkey_on_subnet(&primary_hotkey, netuid, stake_amount);
        let stake_after_direct = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &primary_hotkey,
            &primary_coldkey,
            netuid,
        );
        log::info!(
            "Stake after direct hotkey deposit: {} = {} + {} = {}",
            stake_after_direct,
            stake_after_second,
            stake_amount,
            stake_after_second + stake_amount
        );
        assert!(
            (stake_after_direct.to_u64() as i64
                - (stake_after_second + stake_amount).to_u64() as i64)
                .abs()
                <= 1,
            "Direct hotkey stake should be added to existing stake (within rounding error)"
        );

        // Test Case 4: Multiple Coldkey Support
        // System should support multiple coldkeys staking to the same hotkey
        let secondary_coldkey = U256::from(3);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &primary_hotkey,
            &secondary_coldkey,
            netuid,
            stake_amount,
        );
        let secondary_stake = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &primary_hotkey,
            &secondary_coldkey,
            netuid,
        );
        log::info!(
            "Secondary coldkey stake: {} = {} + {} = {}",
            secondary_stake,
            0,
            stake_amount,
            stake_amount
        );
        assert!(
            (secondary_stake.to_u64() as i64 - stake_amount.to_u64() as i64).abs() <= 1,
            "Secondary coldkey should receive full stake amount (within rounding error)"
        );

        // Test Case 5: Total Stake Verification
        // Verify the total stake across all coldkeys matches expected amount
        let total_hotkey_stake =
            SubtensorModule::get_stake_for_hotkey_on_subnet(&primary_hotkey, netuid);
        log::info!(
            "Total hotkey stake: {} = {}",
            total_hotkey_stake,
            stake_after_direct + stake_amount
        );
        assert!(
            (total_hotkey_stake.to_u64() as i64
                - (stake_after_direct + stake_amount).to_u64() as i64)
                .abs()
                <= 1,
            "Total hotkey stake should match sum of all coldkey stakes"
        );

        // Test Case 6: Proportional Distribution
        // When adding stake directly to hotkey, it should be distributed proportionally
        SubtensorModule::increase_stake_for_hotkey_on_subnet(&primary_hotkey, netuid, stake_amount);
        let primary_final_stake = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &primary_hotkey,
            &primary_coldkey,
            netuid,
        );
        let secondary_final_stake = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &primary_hotkey,
            &secondary_coldkey,
            netuid,
        );

        // Calculate expected proportional distribution
        let primary_expected = stake_after_direct.to_u64() as f64
            + stake_amount.to_u64() as f64
                * (stake_after_direct.to_u64() as f64 / total_hotkey_stake.to_u64() as f64);
        let secondary_expected = secondary_stake.to_u64() as f64
            + stake_amount.to_u64() as f64
                * (secondary_stake.to_u64() as f64 / total_hotkey_stake.to_u64() as f64);

        log::info!("Primary final stake: {primary_final_stake} (expected: {primary_expected})");
        log::info!(
            "Secondary final stake: {secondary_final_stake} (expected: {secondary_expected})"
        );

        assert!(
            (primary_final_stake.to_u64() as f64 - primary_expected).abs() <= 1.0,
            "Primary stake should increase proportionally"
        );
        assert!(
            (secondary_final_stake.to_u64() as f64 - secondary_expected).abs() <= 1.0,
            "Secondary stake should increase proportionally"
        );

        // Test Case 7: Stake Removal
        // Verify correct stake removal from both accounts
        SubtensorModule::decrease_stake_for_hotkey_and_coldkey_on_subnet(
            &primary_hotkey,
            &primary_coldkey,
            netuid,
            stake_amount,
        );
        let primary_after_removal = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &primary_hotkey,
            &primary_coldkey,
            netuid,
        );
        log::info!(
            "Primary stake after removal: {} = {} - {} = {}",
            primary_after_removal,
            primary_final_stake,
            stake_amount,
            primary_final_stake - stake_amount
        );
        assert!(
            (primary_after_removal.to_u64() as i64
                - (primary_final_stake - stake_amount).to_u64() as i64)
                .abs()
                <= 1,
            "Stake removal should decrease balance by exact amount"
        );

        SubtensorModule::decrease_stake_for_hotkey_and_coldkey_on_subnet(
            &primary_hotkey,
            &secondary_coldkey,
            netuid,
            stake_amount,
        );
        let secondary_after_removal = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &primary_hotkey,
            &secondary_coldkey,
            netuid,
        );
        log::info!(
            "Secondary stake after removal: {} = {} - {} = {}",
            secondary_after_removal,
            secondary_final_stake,
            stake_amount,
            secondary_final_stake - stake_amount
        );
        assert!(
            (secondary_after_removal.to_u64() as i64
                - (secondary_final_stake - stake_amount).to_u64() as i64)
                .abs()
                <= 1,
            "Stake removal should decrease balance by exact amount"
        );

        // Test Case 8: Final Total Verification
        // Verify final total matches sum of remaining stakes
        let final_total = SubtensorModule::get_stake_for_hotkey_on_subnet(&primary_hotkey, netuid);
        log::info!(
            "Final total stake: {} = {} + {} = {}",
            final_total,
            primary_after_removal,
            secondary_after_removal,
            primary_after_removal + secondary_after_removal
        );
        assert!(
            (final_total.to_u64() as i64
                - (primary_after_removal + secondary_after_removal).to_u64() as i64)
                .abs()
                <= 1,
            "Final total should match sum of remaining stakes"
        );

        // Additional Edge Cases to Test:

        // Test staking with zero amount
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &primary_hotkey,
            &primary_coldkey,
            netuid,
            AlphaBalance::ZERO,
        );
        let zero_stake = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &primary_hotkey,
            &primary_coldkey,
            netuid,
        );
        assert!(
            zero_stake == primary_after_removal,
            "Staking with zero amount should not change the stake"
        );

        // Test removing more stake than available
        let available_stake = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &primary_hotkey,
            &primary_coldkey,
            netuid,
        );
        let excessive_amount = available_stake + 1000.into();
        log::info!(
            "Attempting to remove excessive stake: {available_stake} + 1000 = {excessive_amount}"
        );
        SubtensorModule::decrease_stake_for_hotkey_and_coldkey_on_subnet(
            &primary_hotkey,
            &primary_coldkey,
            netuid,
            excessive_amount,
        );
        let after_excessive_removal = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &primary_hotkey,
            &primary_coldkey,
            netuid,
        );
        log::info!("Stake after attempting excessive removal: {after_excessive_removal}");
        assert!(
            after_excessive_removal == available_stake,
            "Removing more stake performs no action"
        );

        // Test staking to non-existent hotkey
        let non_existent_hotkey = U256::from(4);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &non_existent_hotkey,
            &primary_coldkey,
            netuid,
            stake_amount,
        );
        let non_existent_hotkey_stake = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &non_existent_hotkey,
            &primary_coldkey,
            netuid,
        );
        assert!(
            non_existent_hotkey_stake == stake_amount,
            "Staking to non-existent hotkey should initialize the stake"
        );

        // Test removing stake from non-existent coldkey
        let non_existent_coldkey = U256::from(5);
        SubtensorModule::decrease_stake_for_hotkey_and_coldkey_on_subnet(
            &primary_hotkey,
            &non_existent_coldkey,
            netuid,
            stake_amount,
        );
        let non_existent_coldkey_stake =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &primary_hotkey,
                &non_existent_coldkey,
                netuid,
            );
        assert!(
            non_existent_coldkey_stake.is_zero(),
            "Removing stake from non-existent coldkey should not change the stake"
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::staking2::test_share_based_staking_denominator_precision --exact --show-output
#[test]
fn test_share_based_staking_denominator_precision() {
    // Test case amounts: stake, unstake, inject, tolerance
    [
        (1_000_u64, 990_u64),
        (1_000, 999),
        (1_000_000, 990_000),
        (1_000_000, 999_990),
        (1_000_000_000, 999_999_990),
        (1_000_000_000_000, 999_999_999_990),
    ]
    .iter()
    .for_each(|test_case| {
        new_test_ext(1).execute_with(|| {
            let netuid = NetUid::from(1);
            let hotkey1 = U256::from(1);
            let coldkey1 = U256::from(2);
            let stake_amount = AlphaBalance::from(test_case.0);
            let unstake_amount = AlphaBalance::from(test_case.1);

            SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey1,
                &coldkey1,
                netuid,
                stake_amount,
            );

            let actual_stake: f64 = AlphaV2::<Test>::get((hotkey1, coldkey1, netuid)).into();
            assert_eq!(stake_amount, (actual_stake as u64).into(),);
            SubtensorModule::decrease_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey1,
                &coldkey1,
                netuid,
                unstake_amount,
            );

            let stake1 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey1, &coldkey1, netuid,
            );
            let expected_remaining_stake = stake_amount - unstake_amount;
            assert_eq!(stake1, expected_remaining_stake);
        });
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::staking2::test_share_based_staking_stake_unstake_inject --exact --show-output --nocapture
#[test]
fn test_share_based_staking_stake_unstake_inject() {
    // Test case amounts: stake, unstake, inject, tolerance
    [
        (1_000_u64, 999_u64, 1_000_000_u64, 0),
        (1_000_000, 999_000, 100_000_000, 0),
        (1_000_000, 900_000, 100_000_000, 0),
        (100_000_000_000, 1_000_000_000, 1_000_000_000_000, 1),
        (100_000_000_000, 99_000_000_000, 1_000_000_000_000, 1),
        (100_000_000_000, 99_990_000_000, 1_000_000_000_000, 1),
        (100_000_000_000, 99_990_000_000, 1_234_567_890, 1),
    ]
    .iter()
    .for_each(|test_case| {
        new_test_ext(1).execute_with(|| {
            let netuid = NetUid::from(1);
            let hotkey1 = U256::from(1);
            let coldkey1 = U256::from(2);
            let coldkey2 = U256::from(3);
            let stake_amount = AlphaBalance::from(test_case.0);
            let unstake_amount = AlphaBalance::from(test_case.1);
            let inject_amount = AlphaBalance::from(test_case.2);
            let tolerance = test_case.3;

            SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey1,
                &coldkey1,
                netuid,
                stake_amount,
            );
            SubtensorModule::decrease_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey1,
                &coldkey1,
                netuid,
                unstake_amount,
            );
            SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey1,
                &coldkey2,
                netuid,
                stake_amount,
            );
            SubtensorModule::decrease_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey1,
                &coldkey2,
                netuid,
                unstake_amount,
            );
            SubtensorModule::increase_stake_for_hotkey_on_subnet(&hotkey1, netuid, inject_amount);

            let stake1 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey1, &coldkey1, netuid,
            );
            let stake2 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey1, &coldkey2, netuid,
            );

            assert!(
                (stake1.to_u64() as i64
                    - (stake_amount.to_u64() as i64 - unstake_amount.to_u64() as i64
                        + (inject_amount.to_u64() / 2) as i64))
                    .abs()
                    <= tolerance
            );
            assert!(
                (stake2.to_u64() as i64
                    - (stake_amount.to_u64() as i64 - unstake_amount.to_u64() as i64
                        + (inject_amount.to_u64() / 2) as i64))
                    .abs()
                    <= tolerance
            );
        });
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::staking2::test_share_based_staking_stake_inject_stake_new --exact --show-output --nocapture
#[test]
fn test_share_based_staking_stake_inject_stake_new() {
    // Test case amounts: stake, inject, stake, tolerance
    [
        (1_u64, 2_000_000_000_u64, 500_000_000_u64, 1),
        (1, 5_000_000_000, 50_000_000, 1),
        (500_000_000, 1_000_000_000, 1_000_000_000, 1),
    ]
    .iter()
    .for_each(|test_case| {
        new_test_ext(1).execute_with(|| {
            let netuid = NetUid::from(1);
            let hotkey1 = U256::from(1);
            let coldkey1 = U256::from(2);
            let coldkey2 = U256::from(3);
            let stake_amount = AlphaBalance::from(test_case.0);
            let inject_amount = AlphaBalance::from(test_case.1);
            let stake_amount_2 = AlphaBalance::from(test_case.2);
            let tolerance = test_case.3;

            SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey1,
                &coldkey1,
                netuid,
                stake_amount,
            );
            SubtensorModule::increase_stake_for_hotkey_on_subnet(&hotkey1, netuid, inject_amount);
            SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey1,
                &coldkey2,
                netuid,
                stake_amount_2,
            );

            let stake1 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey1, &coldkey1, netuid,
            );
            let stake2 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey1, &coldkey2, netuid,
            );

            assert!(
                (stake1.to_u64() as i64 - (stake_amount.to_u64() + inject_amount.to_u64()) as i64)
                    .abs()
                    <= tolerance
            );
            assert!((stake2.to_u64() as i64 - stake_amount_2.to_u64() as i64).abs() <= tolerance);
        });
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::staking2::test_try_associate_hotkey --exact --show-output --nocapture
#[test]
fn test_try_associate_hotkey() {
    new_test_ext(1).execute_with(|| {
        let hotkey1 = U256::from(1);
        let coldkey1 = U256::from(2);
        let coldkey2 = U256::from(3);

        // Check initial association
        assert!(!SubtensorModule::hotkey_account_exists(&hotkey1));

        // Associate hotkey1 with coldkey1
        assert_ok!(SubtensorModule::try_associate_hotkey(
            RuntimeOrigin::signed(coldkey1),
            hotkey1
        ));

        // Check that hotkey1 is associated with coldkey1
        assert!(SubtensorModule::hotkey_account_exists(&hotkey1));
        assert_eq!(
            SubtensorModule::get_owning_coldkey_for_hotkey(&hotkey1),
            coldkey1
        );
        assert_ne!(SubtensorModule::get_owned_hotkeys(&coldkey1).len(), 0);
        assert!(SubtensorModule::get_owned_hotkeys(&coldkey1).contains(&hotkey1));

        // Verify this tx requires a fee
        let call =
            RuntimeCall::SubtensorModule(crate::Call::try_associate_hotkey { hotkey: hotkey1 });
        let dispatch_info = call.get_dispatch_info();
        // Verify tx weight > 0
        assert!(dispatch_info.call_weight.all_gte(Weight::from_all(0)));
        // Verify pays Yes is set
        assert_eq!(dispatch_info.pays_fee, Pays::Yes);

        // Check that coldkey2 is not associated with any hotkey
        assert!(!SubtensorModule::get_owned_hotkeys(&coldkey2).contains(&hotkey1));
        assert_eq!(SubtensorModule::get_owned_hotkeys(&coldkey2).len(), 0);

        // Try to associate hotkey1 with coldkey2
        // Should have no effect because coldkey1 is already associated with hotkey1
        assert_ok!(SubtensorModule::try_associate_hotkey(
            RuntimeOrigin::signed(coldkey2),
            hotkey1
        ));

        // Check that hotkey1 is still associated with coldkey1
        assert!(SubtensorModule::hotkey_account_exists(&hotkey1));
        assert_eq!(
            SubtensorModule::get_owning_coldkey_for_hotkey(&hotkey1),
            coldkey1
        );
        assert_ne!(SubtensorModule::get_owned_hotkeys(&coldkey1).len(), 0);
        assert!(SubtensorModule::get_owned_hotkeys(&coldkey1).contains(&hotkey1));

        // Check that coldkey2 is still not associated with any hotkey
        assert!(!SubtensorModule::get_owned_hotkeys(&coldkey2).contains(&hotkey1));
        assert_eq!(SubtensorModule::get_owned_hotkeys(&coldkey2).len(), 0);
    });
}

#[test]
fn test_stake_fee_api() {
    // The API should match the calculation
    new_test_ext(1).execute_with(|| {
        let hotkey1 = U256::from(1);
        let coldkey1 = U256::from(2);
        let hotkey2 = U256::from(3);
        let coldkey2 = U256::from(4);

        let netuid0 = NetUid::from(1);
        let netuid1 = NetUid::from(2);
        let root_netuid = NetUid::ROOT;

        let alpha_divs = AlphaBalance::from(100_000_000_000_u64);
        let total_hotkey_alpha = AlphaBalance::from(100_000_000_000_u64);
        let tao_in = TaoBalance::from(100_000_000_000_u64); // 100 TAO
        let reciprocal_price = 2; // 1 / price
        let stake_amount = 100_000_000_000_u64;

        // Setup alpha out
        SubnetAlphaOut::<Test>::insert(netuid0, AlphaBalance::from(100_000_000_000_u64));
        SubnetAlphaOut::<Test>::insert(netuid1, AlphaBalance::from(100_000_000_000_u64));
        // Set pools using price
        SubnetAlphaIn::<Test>::insert(
            netuid0,
            AlphaBalance::from(tao_in.to_u64() * reciprocal_price),
        );
        SubnetTAO::<Test>::insert(netuid0, tao_in);
        SubnetAlphaIn::<Test>::insert(
            netuid1,
            AlphaBalance::from(tao_in.to_u64() * reciprocal_price),
        );
        SubnetTAO::<Test>::insert(netuid1, tao_in);

        // Setup alpha divs for hotkey1
        AlphaDividendsPerSubnet::<Test>::insert(netuid0, hotkey1, alpha_divs);
        AlphaDividendsPerSubnet::<Test>::insert(netuid1, hotkey1, alpha_divs);

        // Setup total hotkey alpha for hotkey1
        TotalHotkeyAlpha::<Test>::insert(hotkey1, netuid0, total_hotkey_alpha);
        TotalHotkeyAlpha::<Test>::insert(hotkey1, netuid1, total_hotkey_alpha);

        // Test stake fee for add_stake
        let stake_fee_0 = SubtensorModule::get_stake_fee(
            None,
            coldkey1,
            Some((hotkey1, netuid0)),
            coldkey1,
            stake_amount,
        );
        let dynamic_fee_0 = <Test as Config>::SwapInterface::approx_fee_amount(
            netuid0.into(),
            TaoBalance::from(stake_amount),
        )
        .to_u64();
        assert_eq!(stake_fee_0, dynamic_fee_0);

        // Test stake fee for remove on root
        let stake_fee_1 = SubtensorModule::get_stake_fee(
            Some((hotkey1, root_netuid)),
            coldkey1,
            None,
            coldkey1,
            stake_amount,
        );
        let dynamic_fee_1 = <Test as Config>::SwapInterface::approx_fee_amount(
            root_netuid.into(),
            TaoBalance::from(stake_amount),
        )
        .to_u64();
        assert_eq!(stake_fee_1, dynamic_fee_1);

        // Test stake fee for move from root to non-root
        let stake_fee_2 = SubtensorModule::get_stake_fee(
            Some((hotkey1, root_netuid)),
            coldkey1,
            Some((hotkey1, netuid0)),
            coldkey1,
            stake_amount,
        );
        let dynamic_fee_2 = <Test as Config>::SwapInterface::approx_fee_amount(
            netuid0.into(),
            TaoBalance::from(stake_amount),
        )
        .to_u64();
        assert_eq!(stake_fee_2, dynamic_fee_2);

        // Test stake fee for move between hotkeys on root
        let stake_fee_3 = SubtensorModule::get_stake_fee(
            Some((hotkey1, root_netuid)),
            coldkey1,
            Some((hotkey2, root_netuid)),
            coldkey1,
            stake_amount,
        );
        let dynamic_fee_3 = <Test as Config>::SwapInterface::approx_fee_amount(
            root_netuid.into(),
            TaoBalance::from(stake_amount),
        )
        .to_u64();
        assert_eq!(stake_fee_3, dynamic_fee_3);

        // Test stake fee for move between coldkeys on root
        let stake_fee_4 = SubtensorModule::get_stake_fee(
            Some((hotkey1, root_netuid)),
            coldkey1,
            Some((hotkey1, root_netuid)),
            coldkey2,
            stake_amount,
        );
        let dynamic_fee_4 = <Test as Config>::SwapInterface::approx_fee_amount(
            root_netuid.into(),
            TaoBalance::from(stake_amount),
        )
        .to_u64();
        assert_eq!(stake_fee_4, dynamic_fee_4);

        // Test stake fee for *swap* from non-root to root
        let stake_fee_5 = SubtensorModule::get_stake_fee(
            Some((hotkey1, netuid0)),
            coldkey1,
            Some((hotkey1, root_netuid)),
            coldkey1,
            stake_amount,
        );
        let dynamic_fee_5 = <Test as Config>::SwapInterface::approx_fee_amount(
            root_netuid.into(),
            TaoBalance::from(stake_amount),
        )
        .to_u64();
        assert_eq!(stake_fee_5, dynamic_fee_5);

        // Test stake fee for move between hotkeys on non-root
        let stake_fee_6 = SubtensorModule::get_stake_fee(
            Some((hotkey1, netuid0)),
            coldkey1,
            Some((hotkey2, netuid0)),
            coldkey1,
            stake_amount,
        );
        let dynamic_fee_6 = <Test as Config>::SwapInterface::approx_fee_amount(
            netuid0.into(),
            TaoBalance::from(stake_amount),
        )
        .to_u64();
        assert_eq!(stake_fee_6, dynamic_fee_6);

        // Test stake fee for move between coldkeys on non-root
        let stake_fee_7 = SubtensorModule::get_stake_fee(
            Some((hotkey1, netuid0)),
            coldkey1,
            Some((hotkey1, netuid0)),
            coldkey2,
            stake_amount,
        );
        let dynamic_fee_7 = <Test as Config>::SwapInterface::approx_fee_amount(
            netuid0.into(),
            TaoBalance::from(stake_amount),
        )
        .to_u64();
        assert_eq!(stake_fee_7, dynamic_fee_7);

        // Test stake fee for *swap* from non-root to non-root
        let stake_fee_8 = SubtensorModule::get_stake_fee(
            Some((hotkey1, netuid0)),
            coldkey1,
            Some((hotkey1, netuid1)),
            coldkey1,
            stake_amount,
        );
        let dynamic_fee_8 = <Test as Config>::SwapInterface::approx_fee_amount(
            netuid1.into(),
            TaoBalance::from(stake_amount),
        )
        .to_u64();
        assert_eq!(stake_fee_8, dynamic_fee_8);
    });
}

#[ignore = "fees are now calculated by SwapInterface"]
#[test]
fn test_stake_fee_calculation() {
    new_test_ext(1).execute_with(|| {
        let hotkey1 = U256::from(1);

        let netuid0 = NetUid::from(1);
        let netuid1 = NetUid::from(2);
        let root_netuid = NetUid::ROOT;
        // Set SubnetMechanism to 1 (Dynamic)
        SubnetMechanism::<Test>::insert(netuid0, 1);
        SubnetMechanism::<Test>::insert(netuid1, 1);

        let alpha_divs = AlphaBalance::from(100_000_000_000_u64);
        let total_hotkey_alpha = AlphaBalance::from(100_000_000_000_u64);
        let tao_in = TaoBalance::from(100_000_000_000_u64); // 100 TAO
        let reciprocal_price = 2; // 1 / price
        let stake_amount = TaoBalance::from(100_000_000_000_u64);

        let default_fee = TaoBalance::ZERO; // FIXME: DefaultStakingFee is deprecated

        // Setup alpha out
        SubnetAlphaOut::<Test>::insert(netuid0, AlphaBalance::from(100_000_000_000_u64));
        SubnetAlphaOut::<Test>::insert(netuid1, AlphaBalance::from(100_000_000_000_u64));
        // Set pools using price
        mock::setup_reserves(
            netuid0,
            tao_in,
            AlphaBalance::from(tao_in.to_u64() * reciprocal_price),
        );
        mock::setup_reserves(
            netuid1,
            tao_in,
            AlphaBalance::from(tao_in.to_u64() * reciprocal_price),
        );

        // Setup alpha divs for hotkey1
        AlphaDividendsPerSubnet::<Test>::insert(netuid0, hotkey1, alpha_divs);
        AlphaDividendsPerSubnet::<Test>::insert(netuid1, hotkey1, alpha_divs);

        // Setup total hotkey alpha for hotkey1
        TotalHotkeyAlpha::<Test>::insert(hotkey1, netuid0, total_hotkey_alpha);
        TotalHotkeyAlpha::<Test>::insert(hotkey1, netuid1, total_hotkey_alpha);

        // Test stake fee for add_stake

        // Default for adding stake
        let stake_fee =
            <Test as Config>::SwapInterface::approx_fee_amount(netuid0.into(), stake_amount);
        assert_eq!(stake_fee, default_fee);

        // Test stake fee for remove on root
        let stake_fee =
            <Test as Config>::SwapInterface::approx_fee_amount(root_netuid.into(), stake_amount); // Default for removing stake from root
        assert_eq!(stake_fee, default_fee);

        // Test stake fee for move from root to non-root

        // Default for moving stake from root to non-root
        let stake_fee =
            <Test as Config>::SwapInterface::approx_fee_amount(netuid0.into(), stake_amount);
        assert_eq!(stake_fee, default_fee);

        // Test stake fee for move between hotkeys on root
        let stake_fee =
            <Test as Config>::SwapInterface::approx_fee_amount(root_netuid.into(), stake_amount); // Default for moving stake between hotkeys on root
        assert_eq!(stake_fee, default_fee);

        // Test stake fee for *swap* from non-root to non-root

        // Charged a dynamic fee
        let stake_fee =
            <Test as Config>::SwapInterface::approx_fee_amount(netuid1.into(), stake_amount);
        assert_ne!(stake_fee, default_fee);
    });
}
