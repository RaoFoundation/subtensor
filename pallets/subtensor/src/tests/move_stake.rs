#![allow(clippy::unwrap_used)]

use approx::assert_abs_diff_eq;
use frame_support::{assert_err, assert_noop, assert_ok};
use sp_core::{Get, U256};
use substrate_fixed::types::{U64F64, U96F32};
use subtensor_runtime_common::TaoBalance;
use subtensor_swap_interface::SwapHandler;

use super::mock;
use super::mock::*;
use crate::*;

// 1. test_do_move_success
// Description: Test a successful move of stake between two hotkeys in the same subnet
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test move -- test_do_move_success --exact --nocapture
#[test]
fn test_do_move_success() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let coldkey = U256::from(1);
        let origin_hotkey = U256::from(2);
        let destination_hotkey = U256::from(3);
        let stake_amount = DefaultMinStake::<Test>::get() * 10.into();

        // Set up initial stake
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &origin_hotkey);
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &destination_hotkey);
        add_balance_to_coldkey_account(&coldkey, stake_amount);
        SubtensorModule::stake_into_subnet(
            &origin_hotkey,
            &coldkey,
            netuid.into(),
            stake_amount,
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();
        let alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &origin_hotkey,
            &coldkey,
            netuid,
        );

        // Perform the move
        let expected_alpha = alpha;
        assert_ok!(SubtensorModule::do_move_stake(
            RuntimeOrigin::signed(coldkey),
            origin_hotkey,
            destination_hotkey,
            netuid,
            netuid,
            alpha,
        ));

        // Check that the stake has been moved
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &origin_hotkey,
                &coldkey,
                netuid
            ),
            AlphaBalance::ZERO
        );
        assert_abs_diff_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &destination_hotkey,
                &coldkey,
                netuid
            ),
            expected_alpha,
            epsilon = expected_alpha / 1000.into()
        );
    });
}

// 2. test_do_move_different_subnets
// Description: Test moving stake between two hotkeys in different subnets
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::move_stake::test_do_move_different_subnets --exact --show-output --nocapture
#[test]
fn test_do_move_different_subnets() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let origin_netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let destination_netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let coldkey = U256::from(1);
        let origin_hotkey = U256::from(2);
        let destination_hotkey = U256::from(3);
        let stake_amount = DefaultMinStake::<Test>::get().to_u64() * 10;

        mock::setup_reserves(
            origin_netuid,
            (stake_amount * 100).into(),
            (stake_amount * 100).into(),
        );
        mock::setup_reserves(
            destination_netuid,
            (stake_amount * 100).into(),
            (stake_amount * 100).into(),
        );

        // Set up initial stake and subnets
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &origin_hotkey);
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &destination_hotkey);
        add_balance_to_coldkey_account(&coldkey, stake_amount.into());
        SubtensorModule::stake_into_subnet(
            &origin_hotkey,
            &coldkey,
            origin_netuid,
            stake_amount.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();
        let alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &origin_hotkey,
            &coldkey,
            origin_netuid,
        );

        // Perform the move
        assert_ok!(SubtensorModule::do_move_stake(
            RuntimeOrigin::signed(coldkey),
            origin_hotkey,
            destination_hotkey,
            origin_netuid,
            destination_netuid,
            alpha,
        ));

        // Check that the stake has been moved
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &origin_hotkey,
                &coldkey,
                origin_netuid
            ),
            AlphaBalance::ZERO
        );
        let fee =
            <Test as Config>::SwapInterface::approx_fee_amount(destination_netuid.into(), alpha);
        assert_abs_diff_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &destination_hotkey,
                &coldkey,
                destination_netuid
            ),
            alpha - fee,
            epsilon = alpha / 1000.into()
        );
    });
}

// 4. test_do_move_nonexistent_subnet
// Description: Attempt to move stake to a non-existent subnet, which should fail
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test move -- test_do_move_nonexistent_subnet --exact --nocapture
#[test]
fn test_do_move_nonexistent_subnet() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let origin_netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let coldkey = U256::from(1);
        let origin_hotkey = U256::from(2);
        let destination_hotkey = U256::from(3);
        let nonexistent_netuid = NetUid::from(99); // Assuming this subnet doesn't exist
        let stake_amount = 1_000_000;

        let reserve = stake_amount * 1000;
        mock::setup_reserves(origin_netuid, reserve.into(), reserve.into());

        // Set up initial stake
        add_balance_to_coldkey_account(&coldkey, stake_amount.into());
        SubtensorModule::stake_into_subnet(
            &origin_hotkey,
            &coldkey,
            origin_netuid,
            stake_amount.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();
        let alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &origin_hotkey,
            &coldkey,
            origin_netuid,
        );

        // Attempt to move stake to a non-existent subnet
        assert_noop!(
            SubtensorModule::do_move_stake(
                RuntimeOrigin::signed(coldkey),
                origin_hotkey,
                destination_hotkey,
                origin_netuid,
                nonexistent_netuid,
                alpha,
            ),
            Error::<Test>::SubnetNotExists
        );

        // Check that the stake remains unchanged
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &origin_hotkey,
                &coldkey,
                origin_netuid
            ),
            alpha,
        );
    });
}

// 5. test_do_move_nonexistent_origin_hotkey
// Description: Attempt to move stake from a non-existent origin hotkey, which should fail
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test move -- test_do_move_nonexistent_origin_hotkey --exact --nocapture
#[test]
fn test_do_move_nonexistent_origin_hotkey() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let coldkey = U256::from(1);
        let nonexistent_origin_hotkey = U256::from(99); // Assuming this hotkey doesn't exist
        let destination_hotkey = U256::from(3);

        // Attempt to move stake from a non-existent origin hotkey
        assert_noop!(
            SubtensorModule::do_move_stake(
                RuntimeOrigin::signed(coldkey),
                nonexistent_origin_hotkey,
                destination_hotkey,
                netuid,
                netuid,
                123.into()
            ),
            Error::<Test>::HotKeyAccountNotExists
        );

        // Check that no stake was moved
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &nonexistent_origin_hotkey,
                &coldkey,
                netuid
            ),
            AlphaBalance::ZERO
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &destination_hotkey,
                &coldkey,
                netuid
            ),
            AlphaBalance::ZERO
        );
    });
}

// 6. test_do_move_nonexistent_destination_hotkey
// Description: Attempt to move stake to a non-existent destination hotkey, which should fail
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test move -- test_do_move_nonexistent_destination_hotkey --exact --nocapture
#[test]
fn test_do_move_nonexistent_destination_hotkey() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let origin_hotkey = U256::from(2);
        let nonexistent_destination_hotkey = U256::from(99); // Assuming this hotkey doesn't exist
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let stake_amount = 1_000_000;

        let reserve = stake_amount * 1000;
        mock::setup_reserves(netuid, reserve.into(), reserve.into());

        // Set up initial stake
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &origin_hotkey);
        add_balance_to_coldkey_account(&coldkey, stake_amount.into());
        let alpha = SubtensorModule::stake_into_subnet(
            &origin_hotkey,
            &coldkey,
            netuid,
            stake_amount.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();

        // Attempt to move stake from a non-existent origin hotkey
        add_network(netuid, 1, 0);
        assert_noop!(
            SubtensorModule::do_move_stake(
                RuntimeOrigin::signed(coldkey),
                origin_hotkey,
                nonexistent_destination_hotkey,
                netuid,
                netuid,
                alpha
            ),
            Error::<Test>::HotKeyAccountNotExists
        );

        // Check that the stake was not moved
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &origin_hotkey,
                &coldkey,
                netuid
            ),
            alpha
        );

        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &nonexistent_destination_hotkey,
                &coldkey,
                netuid
            ),
            AlphaBalance::ZERO
        );
    });
}

// 9. test_do_move_partial_stake (replaces "move half" and "move all" tests)
// Description: Test moving a portion of stake from one hotkey to another
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test move -- test_do_move_partial_stake --exact --nocapture
#[test]
fn test_do_move_partial_stake() {
    // Test case: portion of stake to move (in tenths)
    [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
        .into_iter()
        .for_each(|portion_moved| {
            new_test_ext(1).execute_with(|| {
                let subnet_owner_coldkey = U256::from(1001);
                let subnet_owner_hotkey = U256::from(1002);
                let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
                let coldkey = U256::from(1);
                let origin_hotkey = U256::from(2);
                let destination_hotkey = U256::from(3);
                let total_stake = DefaultMinStake::<Test>::get().to_u64() * 20;

                // Set up initial stake
                add_balance_to_coldkey_account(&coldkey, total_stake.into());
                SubtensorModule::stake_into_subnet(
                    &origin_hotkey,
                    &coldkey,
                    netuid,
                    total_stake.into(),
                    <Test as Config>::SwapInterface::max_price(),
                    false,
                )
                .unwrap();
                let alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &origin_hotkey,
                    &coldkey,
                    netuid,
                );

                // Move partial stake
                let alpha_moved = AlphaBalance::from(alpha.to_u64() * portion_moved / 10);
                let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &origin_hotkey);
                let _ =
                    SubtensorModule::create_account_if_non_existent(&coldkey, &destination_hotkey);
                assert_ok!(SubtensorModule::do_move_stake(
                    RuntimeOrigin::signed(coldkey),
                    origin_hotkey,
                    destination_hotkey,
                    netuid,
                    netuid,
                    alpha_moved,
                ));

                // Check that the correct amount of stake was moved
                assert_abs_diff_eq!(
                    SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                        &origin_hotkey,
                        &coldkey,
                        netuid
                    ),
                    alpha - alpha_moved,
                    epsilon = 10.into()
                );
                assert_abs_diff_eq!(
                    SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                        &destination_hotkey,
                        &coldkey,
                        netuid
                    ),
                    alpha_moved,
                    epsilon = 10_000.into()
                );
            });
        });
}

// 10. test_do_move_multiple_times
// Description: Test moving stake multiple times between the same hotkeys
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::move_stake::test_do_move_multiple_times --exact --show-output
#[test]
fn test_do_move_multiple_times() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let coldkey = U256::from(1);
        let hotkey1 = U256::from(2);
        let hotkey2 = U256::from(3);
        let initial_stake = DefaultMinStake::<Test>::get().to_u64() * 10;

        // Set up initial stake
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey1);
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey2);
        add_balance_to_coldkey_account(&coldkey, initial_stake.into());
        SubtensorModule::stake_into_subnet(
            &hotkey1,
            &coldkey,
            netuid,
            initial_stake.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();
        let alpha =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey1, &coldkey, netuid);

        // Move stake multiple times
        let expected_alpha = alpha;
        for _ in 0..3 {
            let alpha1 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey1, &coldkey, netuid,
            );
            assert_ok!(SubtensorModule::do_move_stake(
                RuntimeOrigin::signed(coldkey),
                hotkey1,
                hotkey2,
                netuid,
                netuid,
                alpha1,
            ));
            let alpha2 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey2, &coldkey, netuid,
            );
            assert_ok!(SubtensorModule::do_move_stake(
                RuntimeOrigin::signed(coldkey),
                hotkey2,
                hotkey1,
                netuid,
                netuid,
                alpha2,
            ));
        }

        // Check final stake distribution
        assert_abs_diff_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey1, &coldkey, netuid),
            expected_alpha,
            epsilon = expected_alpha / 1000.into(),
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey2, &coldkey, netuid),
            AlphaBalance::ZERO
        );
    });
}

// 13. test_do_move_wrong_origin
// Description: Attempt to move stake with a different origin than the coldkey, which should fail
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test move -- test_do_move_wrong_origin --exact --nocapture
#[test]
fn test_do_move_wrong_origin() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let wrong_coldkey = U256::from(99);
        let origin_hotkey = U256::from(2);
        let destination_hotkey = U256::from(3);
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let stake_amount = DefaultMinStake::<Test>::get().to_u64() * 10;

        let reserve = stake_amount * 1000;
        mock::setup_reserves(netuid, reserve.into(), reserve.into());

        // Set up initial stake
        add_balance_to_coldkey_account(&coldkey, stake_amount.into());
        SubtensorModule::stake_into_subnet(
            &origin_hotkey,
            &coldkey,
            netuid,
            stake_amount.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();
        let alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &origin_hotkey,
            &coldkey,
            netuid,
        );

        // Attempt to move stake with wrong origin
        add_network(netuid, 1, 0);
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &origin_hotkey);
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &destination_hotkey);
        assert_err!(
            SubtensorModule::do_move_stake(
                RuntimeOrigin::signed(wrong_coldkey),
                origin_hotkey,
                destination_hotkey,
                netuid,
                netuid,
                alpha,
            ),
            Error::<Test>::NotEnoughStakeToWithdraw
        );

        // Check that no stake was moved
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &origin_hotkey,
                &coldkey,
                netuid
            ),
            alpha
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &destination_hotkey,
                &coldkey,
                netuid
            ),
            AlphaBalance::ZERO
        );
    });
}

// 14. test_do_move_same_hotkey_fails
// Description: Attempt to move stake to the same hotkey, which should fail
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test move -- test_do_move_same_hotkey_fails --exact --nocapture
#[test]
fn test_do_move_same_hotkey_fails() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let stake_amount = DefaultMinStake::<Test>::get().to_u64() * 10;

        // Set up initial stake
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey);
        add_balance_to_coldkey_account(&coldkey, stake_amount.into());
        SubtensorModule::stake_into_subnet(
            &hotkey,
            &coldkey,
            netuid,
            stake_amount.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();
        let alpha =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);

        // Attempt to move stake to the same hotkey
        assert_eq!(
            SubtensorModule::do_move_stake(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                hotkey,
                netuid,
                netuid,
                alpha,
            ),
            Err(Error::<Test>::SameNetuid.into())
        );

        // Check that stake remains unchanged
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid),
            alpha,
        );
    });
}

// 15. test_do_move_event_emission
// Description: Verify that the correct event is emitted after a successful move
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test move -- test_do_move_event_emission --exact --nocapture
#[test]
fn test_do_move_event_emission() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let coldkey = U256::from(1);
        let origin_hotkey = U256::from(2);
        let destination_hotkey = U256::from(3);
        let stake_amount = DefaultMinStake::<Test>::get().to_u64() * 10;

        // Set up initial stake
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &origin_hotkey);
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &destination_hotkey);
        add_balance_to_coldkey_account(&coldkey, stake_amount.into());
        SubtensorModule::stake_into_subnet(
            &origin_hotkey,
            &coldkey,
            netuid,
            stake_amount.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();
        let alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &origin_hotkey,
            &coldkey,
            netuid,
        );

        // Move stake and capture events
        System::reset_events();
        let current_price = U96F32::from_num(
            <Test as pallet::Config>::SwapInterface::current_alpha_price(netuid.into()),
        );
        let tao_equivalent = (current_price * U96F32::from_num(alpha)).to_num::<u64>(); // no fee conversion
        assert_ok!(SubtensorModule::do_move_stake(
            RuntimeOrigin::signed(coldkey),
            origin_hotkey,
            destination_hotkey,
            netuid,
            netuid,
            alpha,
        ));

        // Check for the correct event emission
        System::assert_last_event(
            Event::StakeMoved(
                coldkey,
                origin_hotkey,
                netuid,
                destination_hotkey,
                netuid,
                tao_equivalent.into(), // Should be TAO equivalent
            )
            .into(),
        );
    });
}

// 16. test_do_move_storage_updates
// Description: Verify that all relevant storage items are correctly updated after a move
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test move -- test_do_move_storage_updates --exact --nocapture
#[test]
fn test_do_move_storage_updates() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let origin_netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let destination_netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let coldkey = U256::from(1);
        let origin_hotkey = U256::from(2);
        let destination_hotkey = U256::from(3);
        let stake_amount = DefaultMinStake::<Test>::get().to_u64() * 10;

        // Set up initial stake
        add_balance_to_coldkey_account(&coldkey, stake_amount.into());
        SubtensorModule::stake_into_subnet(
            &origin_hotkey,
            &coldkey,
            origin_netuid,
            stake_amount.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();

        // Move stake
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &origin_hotkey);
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &destination_hotkey);
        let alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &origin_hotkey,
            &coldkey,
            origin_netuid,
        );

        let (tao_equivalent, _) = mock::swap_alpha_to_tao_ext(origin_netuid, alpha, true);
        let (alpha2, _) = mock::swap_tao_to_alpha(destination_netuid, tao_equivalent);
        assert_ok!(SubtensorModule::do_move_stake(
            RuntimeOrigin::signed(coldkey),
            origin_hotkey,
            destination_hotkey,
            origin_netuid,
            destination_netuid,
            alpha,
        ));

        // Verify storage updates
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &origin_hotkey,
                &coldkey,
                origin_netuid
            ),
            AlphaBalance::ZERO
        );

        assert_abs_diff_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &destination_hotkey,
                &coldkey,
                destination_netuid
            ),
            alpha2,
            epsilon = 50.into()
        );
    });
}

#[test]
fn test_move_full_amount_same_netuid() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let coldkey = U256::from(1);
        let origin_hotkey = U256::from(2);
        let destination_hotkey = U256::from(3);
        let stake_amount = DefaultMinStake::<Test>::get().to_u64() * 10;
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &origin_hotkey);
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &destination_hotkey);
        add_balance_to_coldkey_account(&coldkey, stake_amount.into());

        // Set up initial stake
        SubtensorModule::stake_into_subnet(
            &origin_hotkey,
            &coldkey,
            netuid,
            stake_amount.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();

        // Move all stake
        let alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &origin_hotkey,
            &coldkey,
            netuid,
        );
        assert_ok!(SubtensorModule::do_move_stake(
            RuntimeOrigin::signed(coldkey),
            origin_hotkey,
            destination_hotkey,
            netuid,
            netuid,
            alpha,
        ));

        // Verify storage updates
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &origin_hotkey,
                &coldkey,
                netuid
            ),
            AlphaBalance::ZERO
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &destination_hotkey,
                &coldkey,
                netuid
            ),
            alpha
        );
    });
}

// 18. test_do_move_max_values
// Description: Test moving the maximum possible stake values to check for overflows
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::move_stake::test_do_move_max_values --exact --show-output
#[test]
fn test_do_move_max_values() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let coldkey = U256::from(1);
        let origin_hotkey = U256::from(2);
        let destination_hotkey = U256::from(3);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let max_stake = 20_000_000_000_000_000_u64;

        // Set up initial stake with maximum value
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &origin_hotkey);
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &destination_hotkey);
        add_balance_to_coldkey_account(&coldkey, max_stake.into());

        // Add lots of liquidity to bypass low liquidity check
        let reserve = max_stake / 1000;
        mock::setup_reserves(netuid, reserve.into(), reserve.into());

        SubtensorModule::stake_into_subnet(
            &origin_hotkey,
            &coldkey,
            netuid,
            max_stake.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();
        let alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &origin_hotkey,
            &coldkey,
            netuid,
        );

        // Move maximum stake
        assert_ok!(SubtensorModule::do_move_stake(
            RuntimeOrigin::signed(coldkey),
            origin_hotkey,
            destination_hotkey,
            netuid,
            netuid,
            alpha,
        ));

        // Verify stake movement without overflow
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &origin_hotkey,
                &coldkey,
                netuid
            ),
            AlphaBalance::ZERO
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &destination_hotkey,
                &coldkey,
                netuid
            ),
            alpha
        );
    });
}

// Verify moving too low amount is impossible
#[test]
fn test_moving_too_little_unstakes() {
    new_test_ext(1).execute_with(|| {
        let hotkey_account_id = U256::from(533453);
        let coldkey_account_id = U256::from(55453);
        let amount = DefaultMinStake::<Test>::get();

        //add network
        let netuid = add_dynamic_network(&hotkey_account_id, &coldkey_account_id);
        let netuid2 = add_dynamic_network(&hotkey_account_id, &coldkey_account_id);

        // Give it some $$$ in his coldkey balance

        let (_, fee) = mock::swap_tao_to_alpha(netuid, amount);

        add_balance_to_coldkey_account(&coldkey_account_id, amount + (fee * 2).into());

        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
            netuid,
            (amount.to_u64() + fee * 2).into()
        ));

        assert_err!(
            SubtensorModule::move_stake(
                RuntimeOrigin::signed(coldkey_account_id),
                hotkey_account_id,
                hotkey_account_id,
                netuid,
                netuid2,
                1.into()
            ),
            Error::<Test>::AmountTooLow
        );
    });
}

#[test]
fn test_do_transfer_success() {
    new_test_ext(1).execute_with(|| {
        // 1. Create a new dynamic network and IDs.
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        // 2. Define the origin coldkey, destination coldkey, and hotkey to be used.
        let origin_coldkey = U256::from(1);
        let destination_coldkey = U256::from(2);
        let hotkey = U256::from(3);
        let stake_amount = DefaultMinStake::<Test>::get().to_u64() * 10;

        // 3. Set up initial stake: (origin_coldkey, hotkey) on netuid.
        let _ = SubtensorModule::create_account_if_non_existent(&origin_coldkey, &hotkey);
        let _ = SubtensorModule::create_account_if_non_existent(&destination_coldkey, &hotkey);
        add_balance_to_coldkey_account(&origin_coldkey, stake_amount.into());
        SubtensorModule::stake_into_subnet(
            &hotkey,
            &origin_coldkey,
            netuid,
            stake_amount.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();
        let alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &origin_coldkey,
            netuid,
        );

        // 4. Transfer the entire stake to the destination coldkey on the same subnet (netuid, netuid).
        let expected_alpha = alpha;
        assert_ok!(SubtensorModule::do_transfer_stake(
            RuntimeOrigin::signed(origin_coldkey),
            destination_coldkey,
            hotkey,
            netuid,
            netuid,
            alpha
        ));

        // 5. Check that the stake has moved.
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &origin_coldkey,
                netuid
            ),
            AlphaBalance::ZERO
        );
        assert_abs_diff_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &destination_coldkey,
                netuid
            ),
            expected_alpha,
            epsilon = expected_alpha / 1000.into()
        );
    });
}

#[test]
fn test_do_transfer_nonexistent_subnet() {
    new_test_ext(1).execute_with(|| {
        let origin_coldkey = U256::from(1);
        let destination_coldkey = U256::from(2);
        let hotkey = U256::from(3);
        let nonexistent_netuid = NetUid::from(9999);
        let stake_amount = DefaultMinStake::<Test>::get().to_u64() * 5;

        assert_noop!(
            SubtensorModule::do_transfer_stake(
                RuntimeOrigin::signed(origin_coldkey),
                destination_coldkey,
                hotkey,
                nonexistent_netuid,
                nonexistent_netuid,
                stake_amount.into()
            ),
            Error::<Test>::SubnetNotExists
        );
    });
}

#[test]
fn test_do_transfer_nonexistent_hotkey() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let origin_coldkey = U256::from(1);
        let destination_coldkey = U256::from(2);
        let nonexistent_hotkey = U256::from(999);

        assert_noop!(
            SubtensorModule::do_transfer_stake(
                RuntimeOrigin::signed(origin_coldkey),
                destination_coldkey,
                nonexistent_hotkey,
                netuid,
                netuid,
                100.into()
            ),
            Error::<Test>::HotKeyAccountNotExists
        );
    });
}

#[test]
fn test_do_transfer_insufficient_stake() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let origin_coldkey = U256::from(1);
        let destination_coldkey = U256::from(2);
        let hotkey = U256::from(3);
        let stake_amount = DefaultMinStake::<Test>::get().to_u64() * 10;

        let _ = SubtensorModule::create_account_if_non_existent(&origin_coldkey, &hotkey);
        add_balance_to_coldkey_account(&origin_coldkey, stake_amount.into());
        SubtensorModule::stake_into_subnet(
            &hotkey,
            &origin_coldkey,
            netuid,
            stake_amount.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();

        let origin_alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &origin_coldkey,
            netuid,
        );
        let destination_alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &destination_coldkey,
            netuid,
        );

        let alpha = stake_amount * 2;
        assert_noop!(
            SubtensorModule::do_transfer_stake(
                RuntimeOrigin::signed(origin_coldkey),
                destination_coldkey,
                hotkey,
                netuid,
                netuid,
                alpha.into()
            ),
            Error::<Test>::NotEnoughStakeToWithdraw
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &origin_coldkey,
                netuid
            ),
            origin_alpha_before
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &destination_coldkey,
                netuid
            ),
            destination_alpha_before
        );
    });
}

#[test]
fn test_do_transfer_wrong_origin() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1010);
        let subnet_owner_hotkey = U256::from(1011);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let origin_coldkey = U256::from(1);
        let wrong_coldkey = U256::from(9999);
        let destination_coldkey = U256::from(2);
        let hotkey = U256::from(3);
        let stake_amount = DefaultMinStake::<Test>::get().to_u64() * 10;
        let fee: u64 = 0; // FIXME: DefaultStakingFee is deprecated

        let _ = SubtensorModule::create_account_if_non_existent(&origin_coldkey, &hotkey);
        add_balance_to_coldkey_account(&origin_coldkey, (stake_amount + fee).into());
        SubtensorModule::stake_into_subnet(
            &hotkey,
            &origin_coldkey,
            netuid,
            stake_amount.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();

        assert_noop!(
            SubtensorModule::do_transfer_stake(
                RuntimeOrigin::signed(wrong_coldkey),
                destination_coldkey,
                hotkey,
                netuid,
                netuid,
                stake_amount.into()
            ),
            Error::<Test>::NotEnoughStakeToWithdraw
        );
    });
}

#[test]
fn test_do_transfer_minimum_stake_check() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let origin_coldkey = U256::from(1);
        let destination_coldkey = U256::from(2);
        let hotkey = U256::from(3);

        let stake_amount = DefaultMinStake::<Test>::get();
        let _ = SubtensorModule::create_account_if_non_existent(&origin_coldkey, &hotkey);
        add_balance_to_coldkey_account(&origin_coldkey, stake_amount.into());
        SubtensorModule::stake_into_subnet(
            &hotkey,
            &origin_coldkey,
            netuid,
            stake_amount,
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();

        assert_err!(
            SubtensorModule::do_transfer_stake(
                RuntimeOrigin::signed(origin_coldkey),
                destination_coldkey,
                hotkey,
                netuid,
                netuid,
                1.into()
            ),
            Error::<Test>::AmountTooLow
        );
    });
}

#[test]
fn test_do_transfer_different_subnets() {
    new_test_ext(1).execute_with(|| {
        // 1. Create two distinct subnets.
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let origin_netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let destination_netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        // 2. Define origin/destination coldkeys and hotkey.
        let origin_coldkey = U256::from(1);
        let destination_coldkey = U256::from(2);
        let hotkey = U256::from(3);
        let stake_amount = DefaultMinStake::<Test>::get().to_u64() * 10;

        // 3. Create accounts if needed.
        let _ = SubtensorModule::create_account_if_non_existent(&origin_coldkey, &hotkey);
        let _ = SubtensorModule::create_account_if_non_existent(&destination_coldkey, &hotkey);

        // 4. Deposit free balance so transaction fees do not reduce staked funds.
        add_balance_to_coldkey_account(&origin_coldkey, 1_000_000_000.into());

        // 5. Stake into the origin subnet.
        SubtensorModule::stake_into_subnet(
            &hotkey,
            &origin_coldkey,
            origin_netuid,
            stake_amount.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();

        // 6. Transfer entire stake from origin_netuid -> destination_netuid.
        let alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &origin_coldkey,
            origin_netuid,
        );

        let (tao_equivalent, _) = mock::swap_alpha_to_tao_ext(origin_netuid, alpha, true);
        let (expected_alpha, _) = mock::swap_tao_to_alpha(destination_netuid, tao_equivalent);

        assert_ok!(SubtensorModule::do_transfer_stake(
            RuntimeOrigin::signed(origin_coldkey),
            destination_coldkey,
            hotkey,
            origin_netuid,
            destination_netuid,
            alpha
        ));

        // 7. Verify origin now has 0 in origin_netuid.
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &origin_coldkey,
                origin_netuid
            ),
            AlphaBalance::ZERO
        );

        // 8. Verify stake ended up in destination subnet for destination coldkey.
        assert_abs_diff_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &destination_coldkey,
                destination_netuid,
            ),
            expected_alpha,
            epsilon = 1000.into()
        );
    });
}

#[test]
fn test_do_swap_success() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let origin_netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let destination_netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let stake_amount = DefaultMinStake::<Test>::get().to_u64() * 10;

        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey);
        add_balance_to_coldkey_account(&coldkey, stake_amount.into());
        SubtensorModule::stake_into_subnet(
            &hotkey,
            &coldkey,
            origin_netuid,
            stake_amount.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();
        let alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            origin_netuid,
        );

        let (tao_equivalent, _) = mock::swap_alpha_to_tao_ext(origin_netuid, alpha_before, true);
        let (expected_alpha, _) = mock::swap_tao_to_alpha(destination_netuid, tao_equivalent);
        assert_ok!(SubtensorModule::do_swap_stake(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            origin_netuid,
            destination_netuid,
            alpha_before,
        ));

        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &coldkey,
                origin_netuid
            ),
            AlphaBalance::ZERO
        );

        let alpha_after = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            destination_netuid,
        );

        assert_abs_diff_eq!(alpha_after, expected_alpha, epsilon = 1000.into());
    });
}

#[test]
fn test_do_swap_nonexistent_subnet() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let nonexistent_netuid1 = NetUid::from(9998);
        let nonexistent_netuid2 = NetUid::from(9999);
        let stake_amount = 1_000_000;

        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey);

        assert_noop!(
            SubtensorModule::do_swap_stake(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                nonexistent_netuid1,
                nonexistent_netuid2,
                stake_amount.into()
            ),
            Error::<Test>::SubnetNotExists
        );
    });
}

#[test]
fn test_do_swap_nonexistent_hotkey() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid1 = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let netuid2 = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let coldkey = U256::from(1);
        let nonexistent_hotkey = U256::from(999);
        let stake_amount = 10_000;

        assert_noop!(
            SubtensorModule::do_swap_stake(
                RuntimeOrigin::signed(coldkey),
                nonexistent_hotkey,
                netuid1,
                netuid2,
                stake_amount.into()
            ),
            Error::<Test>::HotKeyAccountNotExists
        );
    });
}

#[test]
fn test_do_swap_insufficient_stake() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid1 = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let netuid2 = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let stake_amount = DefaultMinStake::<Test>::get().to_u64() * 5;
        let attempted_swap = stake_amount * 2;

        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey);
        add_balance_to_coldkey_account(&coldkey, stake_amount.into());
        SubtensorModule::stake_into_subnet(
            &hotkey,
            &coldkey,
            netuid1,
            stake_amount.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();

        assert_noop!(
            SubtensorModule::do_swap_stake(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid1,
                netuid2,
                attempted_swap.into()
            ),
            Error::<Test>::NotEnoughStakeToWithdraw
        );
    });
}

#[test]
fn test_do_swap_wrong_origin() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1010);
        let subnet_owner_hotkey = U256::from(1011);
        let netuid1 = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let netuid2 = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let real_coldkey = U256::from(1);
        let wrong_coldkey = U256::from(9999);
        let hotkey = U256::from(3);
        let stake_amount = 100_000;

        let _ = SubtensorModule::create_account_if_non_existent(&real_coldkey, &hotkey);
        add_balance_to_coldkey_account(&real_coldkey, stake_amount.into());
        SubtensorModule::stake_into_subnet(
            &hotkey,
            &real_coldkey,
            netuid1,
            stake_amount.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();

        assert_noop!(
            SubtensorModule::do_swap_stake(
                RuntimeOrigin::signed(wrong_coldkey),
                hotkey,
                netuid1,
                netuid2,
                stake_amount.into()
            ),
            Error::<Test>::NotEnoughStakeToWithdraw
        );
    });
}

#[test]
fn test_do_swap_minimum_stake_check() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid1 = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let netuid2 = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let coldkey = U256::from(1);
        let hotkey = U256::from(3);
        let total_stake = DefaultMinStake::<Test>::get();
        let swap_amount = 1;

        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey);
        add_balance_to_coldkey_account(&coldkey, total_stake);
        SubtensorModule::stake_into_subnet(
            &hotkey,
            &coldkey,
            netuid1,
            total_stake,
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();

        assert_err!(
            SubtensorModule::do_swap_stake(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid1,
                netuid2,
                swap_amount.into()
            ),
            Error::<Test>::AmountTooLow
        );
    });
}

#[test]
fn test_do_swap_same_subnet() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1100);
        let subnet_owner_hotkey = U256::from(1101);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let stake_amount = DefaultMinStake::<Test>::get().to_u64() * 10;

        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey);
        add_balance_to_coldkey_account(&coldkey, stake_amount.into());
        SubtensorModule::stake_into_subnet(
            &hotkey,
            &coldkey,
            netuid,
            stake_amount.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();

        let alpha_before =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);

        assert_err!(
            SubtensorModule::do_swap_stake(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid,
                netuid,
                alpha_before
            ),
            DispatchError::from(Error::<Test>::SameNetuid)
        );

        let alpha_after =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);
        assert_eq!(alpha_after, alpha_before);
    });
}

// cargo test --package pallet-subtensor --lib -- tests::move_stake::test_do_swap_partial_stake --exact --show-output
#[test]
fn test_do_swap_partial_stake() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1100);
        let subnet_owner_hotkey = U256::from(1101);
        let origin_netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let destination_netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let total_stake_tao = DefaultMinStake::<Test>::get().to_u64() * 10;

        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey);
        add_balance_to_coldkey_account(&coldkey, total_stake_tao.into());
        SubtensorModule::stake_into_subnet(
            &hotkey,
            &coldkey,
            origin_netuid,
            total_stake_tao.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();
        let total_stake_alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            origin_netuid,
        );

        let swap_amount = total_stake_alpha / 2.into();
        let (tao_equivalent, _) = mock::swap_alpha_to_tao_ext(origin_netuid, swap_amount, true);
        let (expected_alpha, _) = mock::swap_tao_to_alpha(destination_netuid, tao_equivalent);
        assert_ok!(SubtensorModule::do_swap_stake(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            origin_netuid,
            destination_netuid,
            swap_amount,
        ));

        assert_abs_diff_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &coldkey,
                destination_netuid
            ),
            expected_alpha,
            epsilon = 1000.into()
        );
    });
}

#[test]
fn test_do_swap_storage_updates() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1300);
        let subnet_owner_hotkey = U256::from(1301);
        let origin_netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let destination_netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let stake_amount = DefaultMinStake::<Test>::get().to_u64() * 10;

        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey);
        add_balance_to_coldkey_account(&coldkey, stake_amount.into());
        SubtensorModule::stake_into_subnet(
            &hotkey,
            &coldkey,
            origin_netuid,
            stake_amount.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();

        let alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            origin_netuid,
        );
        let (tao_equivalent, _) = mock::swap_alpha_to_tao_ext(origin_netuid, alpha, true);
        let (expected_alpha, _) = mock::swap_tao_to_alpha(destination_netuid, tao_equivalent);
        assert_ok!(SubtensorModule::do_swap_stake(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            origin_netuid,
            destination_netuid,
            alpha
        ));

        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &coldkey,
                origin_netuid
            ),
            AlphaBalance::ZERO
        );

        assert_abs_diff_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &coldkey,
                destination_netuid
            ),
            expected_alpha,
            epsilon = 1000.into()
        );
    });
}

#[test]
fn test_do_swap_multiple_times() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1500);
        let subnet_owner_hotkey = U256::from(1501);
        let netuid1 = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let netuid2 = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let initial_stake = DefaultMinStake::<Test>::get().to_u64() * 10;

        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey);
        add_balance_to_coldkey_account(&coldkey, initial_stake.into());
        SubtensorModule::stake_into_subnet(
            &hotkey,
            &coldkey,
            netuid1,
            initial_stake.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();

        let mut expected_alpha = AlphaBalance::ZERO;
        for _ in 0..3 {
            let alpha1 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &coldkey, netuid1,
            );
            if !alpha1.is_zero() {
                assert_ok!(SubtensorModule::do_swap_stake(
                    RuntimeOrigin::signed(coldkey),
                    hotkey,
                    netuid1,
                    netuid2,
                    alpha1
                ));
            }
            let alpha2 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &coldkey, netuid2,
            );
            if !alpha2.is_zero() {
                let (tao_equivalent, _) = mock::swap_alpha_to_tao_ext(netuid2, alpha2, true);
                // we do this in the loop, because we need the value before the swap
                expected_alpha = mock::swap_tao_to_alpha(netuid1, tao_equivalent).0;
                assert_ok!(SubtensorModule::do_swap_stake(
                    RuntimeOrigin::signed(coldkey),
                    hotkey,
                    netuid2,
                    netuid1,
                    alpha2
                ));
            }
        }

        assert_abs_diff_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid1),
            expected_alpha,
            epsilon = 1000.into()
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid2),
            AlphaBalance::ZERO
        );
    });
}

// cargo test --package pallet-subtensor --lib -- tests::move_stake::test_do_swap_allows_non_owned_hotkey --exact --show-output
#[test]
fn test_do_swap_allows_non_owned_hotkey() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let origin_netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let destination_netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let foreign_coldkey = U256::from(3);
        let stake_amount = DefaultMinStake::<Test>::get().to_u64() * 10;

        let _ = SubtensorModule::create_account_if_non_existent(&foreign_coldkey, &hotkey);
        add_balance_to_coldkey_account(&coldkey, stake_amount.into());
        SubtensorModule::stake_into_subnet(
            &hotkey,
            &coldkey,
            origin_netuid,
            stake_amount.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();
        let alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            origin_netuid,
        );

        assert_ok!(SubtensorModule::do_swap_stake(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            origin_netuid,
            destination_netuid,
            alpha_before,
        ));
    });
}

#[test]
// RUST_LOG=info cargo test --package pallet-subtensor --lib -- tests::move_stake::test_move_stake_specific_stake_into_subnet_fail --exact --show-output
fn test_move_stake_specific_stake_into_subnet_fail() {
    new_test_ext(1).execute_with(|| {
        let sn_owner_coldkey = U256::from(55453);

        let hotkey_account_id = U256::from(533453);
        let coldkey_account_id = U256::from(55454);
        let hotkey_owner_account_id = U256::from(533454);

        let existing_shares: U64F64 =
            U64F64::from_num(161_986_254).saturating_div(U64F64::from_num(u64::MAX));
        let existing_stake = AlphaBalance::from(36_711_495_953_u64);

        let tao_in = TaoBalance::from(2_409_892_148_947_u64);
        let alpha_in = AlphaBalance::from(15_358_708_513_716_u64);

        let tao_staked = 200_000_000;

        //add network
        let netuid = add_dynamic_network(&sn_owner_coldkey, &sn_owner_coldkey);

        let origin_netuid = add_dynamic_network(&sn_owner_coldkey, &sn_owner_coldkey);

        // Register hotkey on netuid
        register_ok_neuron(netuid, hotkey_account_id, hotkey_owner_account_id, 0);
        // Register hotkey on origin netuid
        register_ok_neuron(origin_netuid, hotkey_account_id, hotkey_owner_account_id, 0);

        // Check we have zero staked
        assert_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&hotkey_account_id),
            TaoBalance::ZERO
        );

        // Set a hotkey pool for the hotkey on destination subnet
        let mut hotkey_pool = SubtensorModule::get_alpha_share_pool(hotkey_account_id, netuid);
        hotkey_pool.update_value_for_one(&hotkey_owner_account_id, 1234); // Doesn't matter, will be overridden

        // Adjust the total hotkey stake and shares to match the existing values
        TotalHotkeyShares::<Test>::insert(hotkey_account_id, netuid, existing_shares);
        TotalHotkeyAlpha::<Test>::insert(hotkey_account_id, netuid, existing_stake);

        // Make the hotkey a delegate
        Delegates::<Test>::insert(hotkey_account_id, 0);

        // Setup Subnet pool
        SubnetAlphaIn::<Test>::insert(netuid, alpha_in);
        SubnetTAO::<Test>::insert(netuid, tao_in);

        // Give TAO balance to coldkey
        add_balance_to_coldkey_account(&coldkey_account_id, (tao_staked + 1_000_000_000).into());

        // Setup Subnet pool for origin netuid
        SubnetAlphaIn::<Test>::insert(origin_netuid, alpha_in + 10_000_000.into());
        SubnetTAO::<Test>::insert(origin_netuid, tao_in + 10_000_000.into());

        // Add stake as new hotkey
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
            origin_netuid,
            tao_staked.into(),
        ),);
        let alpha_to_move = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &coldkey_account_id,
            origin_netuid,
        );

        // Move stake to destination subnet
        let (tao_equivalent, _) = mock::swap_alpha_to_tao_ext(origin_netuid, alpha_to_move, true);
        let (expected_value, _) = mock::swap_tao_to_alpha(netuid, tao_equivalent);
        assert_ok!(SubtensorModule::move_stake(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
            hotkey_account_id,
            origin_netuid,
            netuid,
            alpha_to_move,
        ));

        // Check that the stake has been moved
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey_account_id,
                &coldkey_account_id,
                origin_netuid
            ),
            AlphaBalance::ZERO
        );

        assert_abs_diff_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey_account_id,
                &coldkey_account_id,
                netuid
            ),
            expected_value,
            epsilon = 1000.into()
        );
    });
}

#[test]
fn test_transfer_stake_same_netuid_not_rate_limited() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let origin_coldkey = U256::from(1);
        let destination_coldkey = U256::from(2);
        let hotkey = U256::from(3);
        let stake_amount = DefaultMinStake::<Test>::get().to_u64() * 10;

        let _ = SubtensorModule::create_account_if_non_existent(&origin_coldkey, &hotkey);
        let _ = SubtensorModule::create_account_if_non_existent(&destination_coldkey, &hotkey);
        add_balance_to_coldkey_account(&origin_coldkey, stake_amount.into());
        SubtensorModule::stake_into_subnet(
            &hotkey,
            &origin_coldkey,
            netuid,
            stake_amount.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();
        let alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &origin_coldkey,
            netuid,
        );

        // add_stake set the limiter for (hotkey, origin_coldkey, netuid), but a same-netuid
        // transfer performs no AMM swap (no price impact), so it is NOT rate limited
        assert_ok!(SubtensorModule::do_transfer_stake(
            RuntimeOrigin::signed(origin_coldkey),
            destination_coldkey,
            hotkey,
            netuid,
            netuid,
            alpha
        ));

        // The whole position was moved to the destination coldkey on the same subnet.
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &origin_coldkey,
                netuid
            ),
            AlphaBalance::ZERO
        );
        assert_ne!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &destination_coldkey,
                netuid
            ),
            AlphaBalance::ZERO
        );
    });
}

#[test]
fn test_transfer_stake_doesnt_limit_destination_coldkey() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let netuid2 = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let origin_coldkey = U256::from(1);
        let destination_coldkey = U256::from(2);
        let hotkey = U256::from(3);
        let stake_amount = DefaultMinStake::<Test>::get().to_u64() * 10;

        let _ = SubtensorModule::create_account_if_non_existent(&origin_coldkey, &hotkey);
        let _ = SubtensorModule::create_account_if_non_existent(&destination_coldkey, &hotkey);
        add_balance_to_coldkey_account(&origin_coldkey, stake_amount.into());
        SubtensorModule::stake_into_subnet(
            &hotkey,
            &origin_coldkey,
            netuid,
            stake_amount.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();
        let alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &origin_coldkey,
            netuid,
        );

        assert_ok!(SubtensorModule::do_transfer_stake(
            RuntimeOrigin::signed(origin_coldkey),
            destination_coldkey,
            hotkey,
            netuid,
            netuid2,
            alpha
        ),);
    });
}
