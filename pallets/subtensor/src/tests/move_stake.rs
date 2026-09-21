#![allow(clippy::unwrap_used)]

use approx::assert_abs_diff_eq;
use frame_support::dispatch::{GetDispatchInfo, Pays};
use frame_support::{assert_err, assert_noop, assert_ok};
use frame_system::{CheckNonce, RawOrigin};
use sp_core::{Get, U256};
use sp_runtime::PerU16;
use sp_runtime::traits::{DispatchTransaction, Dispatchable};
use sp_runtime::transaction_validity::InvalidTransaction;
use std::collections::BTreeSet;
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
        assert_total_alpha_staked_invariant(netuid);
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
        assert_total_alpha_staked_invariant(origin_netuid);
        assert_total_alpha_staked_invariant(destination_netuid);
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

#[test]
fn test_do_move_max_caps_to_live_origin() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let coldkey = U256::from(1);
        let origin_hotkey = U256::from(2);
        let destination_hotkey = U256::from(3);
        let stake_amount = DefaultMinStake::<Test>::get() * 10.into();

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

        assert_ok!(SubtensorModule::do_move_stake(
            RuntimeOrigin::signed(coldkey),
            origin_hotkey,
            destination_hotkey,
            netuid,
            netuid,
            AlphaBalance::MAX,
        ));

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
            alpha,
            epsilon = alpha / 1000.into()
        );
    });
}

#[test]
fn test_do_move_oversize_non_max_still_fails() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let coldkey = U256::from(1);
        let origin_hotkey = U256::from(2);
        let destination_hotkey = U256::from(3);
        let stake_amount = DefaultMinStake::<Test>::get() * 10.into();

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

        assert_noop!(
            SubtensorModule::do_move_stake(
                RuntimeOrigin::signed(coldkey),
                origin_hotkey,
                destination_hotkey,
                netuid,
                netuid,
                alpha.saturating_add(1.into()),
            ),
            Error::<Test>::NotEnoughStakeToWithdraw
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &origin_hotkey,
                &coldkey,
                netuid
            ),
            alpha
        );
    });
}

/// v468 defect 1 (spec 469). A position is stored as pool shares; reading it back truncates,
/// so the alpha `move_stake` credits (and reports in `StakeAdded`) can read one rao short.
/// Builds a hotkey pool whose value-per-share is not a whole number, moves an odd amount
/// into an empty position, and returns `(netuid, coldkey, hotkey, moved, readable)` with
/// `readable < moved`. Panics if no amount in a small range reproduces the gap, so the
/// fixture cannot silently stop testing the defect.
fn inexact_pool_position() -> (NetUid, U256, U256, AlphaBalance, AlphaBalance) {
    let subnet_owner_coldkey = U256::from(1001);
    let subnet_owner_hotkey = U256::from(1002);
    let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
    let coldkey = U256::from(1);
    let other_member = U256::from(4);
    let origin_hotkey = U256::from(2);
    let hotkey = U256::from(3);
    let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &origin_hotkey);
    let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey);
    let _ = SubtensorModule::create_account_if_non_existent(&other_member, &hotkey);

    // Another member plus a shareless emission make value / share a non-terminating ratio.
    mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
        &hotkey,
        &other_member,
        netuid,
        AlphaBalance::from(86_737_855_666_u64),
    );
    SubtensorModule::increase_stake_for_hotkey_on_subnet(
        &hotkey,
        netuid,
        AlphaBalance::from(699_304_510_829_u64),
    );

    for candidate in 0_u64..64 {
        let moved = AlphaBalance::from(6_658_030_659_780_u64.saturating_add(candidate));
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &origin_hotkey,
            &coldkey,
            netuid,
            moved,
        );
        assert_ok!(SubtensorModule::do_move_stake(
            RuntimeOrigin::signed(coldkey),
            origin_hotkey,
            hotkey,
            netuid,
            netuid,
            AlphaBalance::MAX,
        ));
        let readable =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);
        assert!(
            readable <= moved,
            "a read never exceeds the credited amount"
        );
        if readable < moved {
            return (netuid, coldkey, hotkey, moved, readable);
        }
        // Exact this time: give the position back and try the next amount.
        assert_ok!(SubtensorModule::do_move_stake(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            origin_hotkey,
            netuid,
            netuid,
            AlphaBalance::MAX,
        ));
    }
    panic!("no amount in range reproduces the one-rao read truncation");
}

/// Replaying the credited amount into `transfer_stake` fails by one rao (the v468
/// symptom); `AlphaBalance::MAX` resolves the whole position at execution and succeeds.
#[test]
fn test_transfer_max_caps_to_live_origin_in_inexact_pool() {
    new_test_ext(1).execute_with(|| {
        let (netuid, coldkey, hotkey, moved, readable) = inexact_pool_position();
        let destination_coldkey = U256::from(5);
        assert_eq!(
            readable,
            moved - 1.into(),
            "the live case is short by one rao"
        );

        crate::assert_noop_ignore_postinfo!(
            SubtensorModule::transfer_stake(
                RuntimeOrigin::signed(coldkey),
                destination_coldkey,
                hotkey,
                netuid,
                netuid,
                moved,
            ),
            Error::<Test>::NotEnoughStakeToWithdraw
        );

        assert_ok!(SubtensorModule::transfer_stake(
            RuntimeOrigin::signed(coldkey),
            destination_coldkey,
            hotkey,
            netuid,
            netuid,
            AlphaBalance::MAX,
        ));
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid),
            AlphaBalance::ZERO
        );
        assert_abs_diff_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &destination_coldkey,
                netuid
            ),
            readable,
            epsilon = 2.into()
        );
    });
}

#[test]
fn test_transfer_stake_and_hotkey_max_caps_to_live_origin() {
    new_test_ext(1).execute_with(|| {
        let (netuid, coldkey, hotkey, _moved, readable) = inexact_pool_position();
        let destination_coldkey = U256::from(5);
        let destination_hotkey = U256::from(6);
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &destination_hotkey);

        assert_ok!(SubtensorModule::transfer_stake_and_hotkey(
            RuntimeOrigin::signed(coldkey),
            destination_coldkey,
            hotkey,
            destination_hotkey,
            netuid,
            netuid,
            AlphaBalance::MAX,
        ));
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid),
            AlphaBalance::ZERO
        );
        assert_abs_diff_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &destination_hotkey,
                &destination_coldkey,
                netuid
            ),
            readable,
            epsilon = 2.into()
        );
    });
}

#[test]
fn test_swap_stake_max_caps_to_live_origin() {
    new_test_ext(1).execute_with(|| {
        let (netuid, coldkey, hotkey, _moved, _readable) = inexact_pool_position();
        let other_owner_coldkey = U256::from(2001);
        let other_owner_hotkey = U256::from(2002);
        let destination_netuid = add_dynamic_network(&other_owner_hotkey, &other_owner_coldkey);
        SubtensorModule::set_tao_weight(u64::MAX);

        assert_ok!(SubtensorModule::swap_stake(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            destination_netuid,
            AlphaBalance::MAX,
        ));
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid),
            AlphaBalance::ZERO
        );
        assert!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &coldkey,
                destination_netuid
            ) > AlphaBalance::ZERO
        );
    });
}

#[test]
fn test_swap_stake_limit_max_caps_to_live_origin() {
    new_test_ext(1).execute_with(|| {
        let (netuid, coldkey, hotkey, _moved, _readable) = inexact_pool_position();
        let other_owner_coldkey = U256::from(2001);
        let other_owner_hotkey = U256::from(2002);
        let destination_netuid = add_dynamic_network(&other_owner_hotkey, &other_owner_coldkey);
        SubtensorModule::set_tao_weight(u64::MAX);

        // A zero relative limit price accepts any fill, so the whole position moves.
        assert_ok!(SubtensorModule::swap_stake_limit(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            destination_netuid,
            AlphaBalance::MAX,
            TaoBalance::ZERO,
            false,
        ));
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid),
            AlphaBalance::ZERO,
            "the whole live position was swapped"
        );
    });
}

/// An explicit oversize amount that is not the sentinel still fails, on every debit call.
#[test]
fn test_transfer_and_swap_oversize_non_max_still_fail() {
    new_test_ext(1).execute_with(|| {
        let (netuid, coldkey, hotkey, moved, _readable) = inexact_pool_position();
        let destination_coldkey = U256::from(5);
        let other_owner_coldkey = U256::from(2001);
        let other_owner_hotkey = U256::from(2002);
        let destination_netuid = add_dynamic_network(&other_owner_hotkey, &other_owner_coldkey);
        let oversize = moved + 1.into();

        crate::assert_noop_ignore_postinfo!(
            SubtensorModule::transfer_stake(
                RuntimeOrigin::signed(coldkey),
                destination_coldkey,
                hotkey,
                netuid,
                netuid,
                oversize,
            ),
            Error::<Test>::NotEnoughStakeToWithdraw
        );
        crate::assert_noop_ignore_postinfo!(
            SubtensorModule::swap_stake(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid,
                destination_netuid,
                oversize,
            ),
            Error::<Test>::NotEnoughStakeToWithdraw
        );
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

        frame_support::assert_err_ignore_postinfo!(
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
fn test_do_transfer_stake_and_hotkey_success() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let origin_coldkey = U256::from(1);
        let destination_coldkey = U256::from(2);
        let origin_hotkey = U256::from(3);
        let destination_hotkey = U256::from(4);
        let stake_amount = DefaultMinStake::<Test>::get().to_u64() * 10;

        let _ = SubtensorModule::create_account_if_non_existent(&origin_coldkey, &origin_hotkey);
        let _ = SubtensorModule::create_account_if_non_existent(
            &destination_coldkey,
            &destination_hotkey,
        );
        add_balance_to_coldkey_account(&origin_coldkey, stake_amount.into());
        SubtensorModule::stake_into_subnet(
            &origin_hotkey,
            &origin_coldkey,
            netuid,
            stake_amount.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();
        let alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &origin_hotkey,
            &origin_coldkey,
            netuid,
        );

        // Transfer the entire stake to (destination_coldkey, destination_hotkey)
        // on the same subnet.
        assert_ok!(SubtensorModule::do_transfer_stake_and_hotkey(
            RuntimeOrigin::signed(origin_coldkey),
            destination_coldkey,
            origin_hotkey,
            destination_hotkey,
            netuid,
            netuid,
            alpha
        ));

        // The origin position is empty and the destination position holds the stake.
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &origin_hotkey,
                &origin_coldkey,
                netuid
            ),
            AlphaBalance::ZERO
        );
        assert_abs_diff_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &destination_hotkey,
                &destination_coldkey,
                netuid
            ),
            alpha,
            epsilon = alpha / 1000.into()
        );

        // Nothing leaked onto the mixed (origin hotkey / destination coldkey) positions.
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &origin_hotkey,
                &destination_coldkey,
                netuid
            ),
            AlphaBalance::ZERO
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &destination_hotkey,
                &origin_coldkey,
                netuid
            ),
            AlphaBalance::ZERO
        );

        // The dedicated event was emitted.
        assert!(System::events().iter().any(|e| matches!(
            &e.event,
            RuntimeEvent::SubtensorModule(Event::StakeAndHotkeyTransferred {
                origin_coldkey: oc,
                destination_coldkey: dc,
                origin_hotkey: oh,
                destination_hotkey: dh,
                ..
            }) if *oc == origin_coldkey
                && *dc == destination_coldkey
                && *oh == origin_hotkey
                && *dh == destination_hotkey
        )));
    });
}

// Regression: locked miner collateral must not be liberated by a same-subnet,
// ownership-changing transfer to a second coldkey. The collateral lock does not
// follow the stake on this path (unlike conviction), so the origin coldkey must
// be blocked from transferring away alpha it needs to cover its collateral.
#[test]
fn test_do_transfer_stake_and_hotkey_same_subnet_respects_collateral() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let origin_coldkey = U256::from(1);
        let destination_coldkey = U256::from(2);
        let origin_hotkey = U256::from(3);
        let destination_hotkey = U256::from(4);
        let stake_amount = DefaultMinStake::<Test>::get().to_u64() * 10;

        let _ = SubtensorModule::create_account_if_non_existent(&origin_coldkey, &origin_hotkey);
        let _ = SubtensorModule::create_account_if_non_existent(
            &destination_coldkey,
            &destination_hotkey,
        );
        add_balance_to_coldkey_account(&origin_coldkey, stake_amount.into());
        SubtensorModule::stake_into_subnet(
            &origin_hotkey,
            &origin_coldkey,
            netuid,
            stake_amount.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();
        let alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &origin_hotkey,
            &origin_coldkey,
            netuid,
        );

        // Flag the origin (hotkey, coldkey) position as registration collateral.
        MinerCollateral::<Test>::insert(
            (netuid, origin_hotkey, origin_coldkey),
            MinerCollateralState {
                locked: alpha,
                drain_ratio: U64F64::from_num(1),
                min_locked: AlphaBalance::ZERO,
                earned: AlphaBalance::ZERO,
            },
        );
        ColdkeyMinerCollateral::<Test>::insert(netuid, origin_coldkey, alpha);

        // Same-subnet transfer to a second coldkey must be rejected: it would
        // liberate the locked collateral.
        assert_err!(
            SubtensorModule::do_transfer_stake_and_hotkey(
                RuntimeOrigin::signed(origin_coldkey),
                destination_coldkey,
                origin_hotkey,
                destination_hotkey,
                netuid,
                netuid,
                alpha
            ),
            Error::<Test>::StakeUnavailable
        );

        // Collateral remains locked on the origin position.
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &origin_hotkey,
                &origin_coldkey,
                netuid
            ),
            alpha
        );
    });
}

#[test]
fn test_do_transfer_stake_and_hotkey_nonexistent_destination_hotkey() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let origin_coldkey = U256::from(1);
        let destination_coldkey = U256::from(2);
        let origin_hotkey = U256::from(3);
        let nonexistent_hotkey = U256::from(999);
        let stake_amount = DefaultMinStake::<Test>::get().to_u64() * 10;

        let _ = SubtensorModule::create_account_if_non_existent(&origin_coldkey, &origin_hotkey);
        add_balance_to_coldkey_account(&origin_coldkey, stake_amount.into());
        SubtensorModule::stake_into_subnet(
            &origin_hotkey,
            &origin_coldkey,
            netuid,
            stake_amount.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();
        let alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &origin_hotkey,
            &origin_coldkey,
            netuid,
        );

        assert_noop!(
            SubtensorModule::do_transfer_stake_and_hotkey(
                RuntimeOrigin::signed(origin_coldkey),
                destination_coldkey,
                origin_hotkey,
                nonexistent_hotkey,
                netuid,
                netuid,
                alpha
            ),
            Error::<Test>::HotKeyAccountNotExists
        );
    });
}

#[test]
fn test_do_transfer_stake_and_hotkey_different_subnets() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let origin_netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let destination_netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let origin_coldkey = U256::from(1);
        let destination_coldkey = U256::from(2);
        let origin_hotkey = U256::from(3);
        let destination_hotkey = U256::from(4);
        let stake_amount = DefaultMinStake::<Test>::get().to_u64() * 10;

        let _ = SubtensorModule::create_account_if_non_existent(&origin_coldkey, &origin_hotkey);
        let _ = SubtensorModule::create_account_if_non_existent(
            &destination_coldkey,
            &destination_hotkey,
        );
        add_balance_to_coldkey_account(&origin_coldkey, 1_000_000_000.into());
        SubtensorModule::stake_into_subnet(
            &origin_hotkey,
            &origin_coldkey,
            origin_netuid,
            stake_amount.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();

        let alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &origin_hotkey,
            &origin_coldkey,
            origin_netuid,
        );
        let (tao_equivalent, _) = mock::swap_alpha_to_tao_ext(origin_netuid, alpha, true);
        let (expected_alpha, _) = mock::swap_tao_to_alpha(destination_netuid, tao_equivalent);

        assert_ok!(SubtensorModule::do_transfer_stake_and_hotkey(
            RuntimeOrigin::signed(origin_coldkey),
            destination_coldkey,
            origin_hotkey,
            destination_hotkey,
            origin_netuid,
            destination_netuid,
            alpha
        ));

        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &origin_hotkey,
                &origin_coldkey,
                origin_netuid
            ),
            AlphaBalance::ZERO
        );
        assert_abs_diff_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &destination_hotkey,
                &destination_coldkey,
                destination_netuid,
            ),
            expected_alpha,
            epsilon = 1000.into()
        );
    });
}

#[test]
fn test_do_transfer_stake_and_hotkey_respects_transfer_toggle() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let origin_coldkey = U256::from(1);
        let destination_coldkey = U256::from(2);
        let origin_hotkey = U256::from(3);
        let destination_hotkey = U256::from(4);
        let stake_amount = DefaultMinStake::<Test>::get().to_u64() * 10;

        let _ = SubtensorModule::create_account_if_non_existent(&origin_coldkey, &origin_hotkey);
        let _ = SubtensorModule::create_account_if_non_existent(
            &destination_coldkey,
            &destination_hotkey,
        );
        add_balance_to_coldkey_account(&origin_coldkey, stake_amount.into());
        SubtensorModule::stake_into_subnet(
            &origin_hotkey,
            &origin_coldkey,
            netuid,
            stake_amount.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();
        let alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &origin_hotkey,
            &origin_coldkey,
            netuid,
        );

        // Disable transfers on the subnet: the call is a transfer, so it must fail.
        assert_ok!(SubtensorModule::toggle_transfer(netuid, false));
        assert_noop!(
            SubtensorModule::do_transfer_stake_and_hotkey(
                RuntimeOrigin::signed(origin_coldkey),
                destination_coldkey,
                origin_hotkey,
                destination_hotkey,
                netuid,
                netuid,
                alpha
            ),
            Error::<Test>::TransferDisallowed
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
        Delegates::<Test>::insert(hotkey_account_id, PerU16::zero());

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

// Regression: a divergent share pool (S/D > 1) must never let a same-subnet move credit the
// destination with more alpha than the origin pool actually lost. Total alpha across both
// positions is conserved, and an oversize request is refused instead of minting.
// SKIP_WASM_BUILD=1 cargo test --package pallet-subtensor --lib -- tests::move_stake::test_move_stake_conserves_alpha_when_origin_quote_is_inflated --exact
#[test]
fn test_move_stake_conserves_alpha_when_origin_quote_is_inflated() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let coldkey = U256::from(1);
        let origin_hotkey = U256::from(2);
        let destination_hotkey = U256::from(3);
        let stake_amount = DefaultMinStake::<Test>::get() * 10.into();

        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &origin_hotkey);
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &destination_hotkey);
        add_balance_to_coldkey_account(&coldkey, stake_amount);
        SubtensorModule::stake_into_subnet(
            &origin_hotkey,
            &coldkey,
            netuid,
            stake_amount,
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();

        // The origin hotkey pool holds exactly this much alpha in total (V).
        let real_alpha = TotalHotkeyAlpha::<Test>::get(origin_hotkey, netuid);
        assert!(!real_alpha.is_zero());

        // Drive the coldkey's share to S = 3D, so the raw quote V * S / D = 3V.
        inflate_alpha_share(&origin_hotkey, &coldkey, netuid, 3);

        // FIX 1: the quote is capped at the pool value.
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &origin_hotkey,
                &coldkey,
                netuid
            ),
            real_alpha
        );

        let total_before = TotalHotkeyAlpha::<Test>::get(origin_hotkey, netuid)
            .saturating_add(TotalHotkeyAlpha::<Test>::get(destination_hotkey, netuid));

        // Asking for more than the pool holds is refused.
        assert_noop!(
            SubtensorModule::do_move_stake(
                RuntimeOrigin::signed(coldkey),
                origin_hotkey,
                destination_hotkey,
                netuid,
                netuid,
                real_alpha.saturating_add(1.into()),
            ),
            Error::<Test>::NotEnoughStakeToWithdraw
        );

        // Moving the whole real position works and conserves alpha.
        assert_ok!(SubtensorModule::do_move_stake(
            RuntimeOrigin::signed(coldkey),
            origin_hotkey,
            destination_hotkey,
            netuid,
            netuid,
            real_alpha,
        ));

        let origin_after = TotalHotkeyAlpha::<Test>::get(origin_hotkey, netuid);
        let destination_after = TotalHotkeyAlpha::<Test>::get(destination_hotkey, netuid);
        assert_eq!(origin_after, AlphaBalance::ZERO);
        assert_eq!(
            origin_after.saturating_add(destination_after),
            total_before,
            "alpha must be conserved across the two hotkey pools"
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &origin_hotkey,
                &coldkey,
                netuid
            ),
            AlphaBalance::ZERO
        );
        assert!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &destination_hotkey,
                &coldkey,
                netuid
            ) <= real_alpha,
            "destination may receive at most what the origin lost"
        );
        assert_total_alpha_staked_invariant(netuid);
    });
}

// Regression: the internal same-subnet transfer used to credit the destination with the full
// requested amount even when the debit silently did nothing. It must now refuse instead.
// SKIP_WASM_BUILD=1 cargo test --package pallet-subtensor --lib -- tests::move_stake::test_transfer_stake_within_subnet_refuses_to_credit_undebited_alpha --exact
#[test]
fn test_transfer_stake_within_subnet_refuses_to_credit_undebited_alpha() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let coldkey = U256::from(1);
        let origin_hotkey = U256::from(2);
        let destination_hotkey = U256::from(3);
        let amount = DefaultMinStake::<Test>::get() * 10.into();

        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &origin_hotkey);
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &destination_hotkey);

        // The coldkey holds nothing on the origin hotkey, so nothing can be debited.
        assert_err!(
            SubtensorModule::transfer_stake_within_subnet(
                &coldkey,
                &origin_hotkey,
                &coldkey,
                &destination_hotkey,
                netuid,
                amount.to_u64().into(),
            ),
            Error::<Test>::NotEnoughStakeToWithdraw
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &destination_hotkey,
                &coldkey,
                netuid
            ),
            AlphaBalance::ZERO
        );
        assert_eq!(
            TotalHotkeyAlpha::<Test>::get(destination_hotkey, netuid),
            AlphaBalance::ZERO
        );
    });
}

#[test]
fn test_transfer_stake_rejects_beta_escrow_destination() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let origin_coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let stake_amount = DefaultMinStake::<Test>::get();

        let _ = SubtensorModule::create_account_if_non_existent(&origin_coldkey, &hotkey);
        add_balance_to_coldkey_account(&origin_coldkey, stake_amount);
        SubtensorModule::stake_into_subnet(
            &hotkey,
            &origin_coldkey,
            netuid,
            stake_amount,
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();

        let escrow = SubtensorModule::get_beta_escrow_account_id();
        assert_noop!(
            SubtensorModule::do_transfer_stake(
                RuntimeOrigin::signed(origin_coldkey),
                escrow,
                hotkey,
                netuid,
                netuid,
                1.into(),
            ),
            Error::<Test>::CannotUseSystemAccount
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

/// Moves alpha to `receiver` from a funded coldkey so `receiver` holds alpha and has never
/// held TAO (no `System::Account` row).
fn endow_alpha_only(funder: U256, receiver: U256, hotkey: U256, netuid: NetUid, tao: u64) {
    let _ = SubtensorModule::create_account_if_non_existent(&funder, &hotkey);
    add_balance_to_coldkey_account(&funder, tao.saturating_add(1_000_000_000).into());
    SubtensorModule::stake_into_subnet(
        &hotkey,
        &funder,
        netuid,
        tao.into(),
        <Test as Config>::SwapInterface::max_price(),
        false,
    )
    .unwrap();
    let alpha =
        SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &funder, netuid);
    assert_ok!(SubtensorModule::do_transfer_stake(
        RuntimeOrigin::signed(funder),
        receiver,
        hotkey,
        netuid,
        netuid,
        alpha
    ));
}

fn system_account_state(who: &U256) -> (bool, u64, u32, u32) {
    let exists = frame_system::Account::<Test>::contains_key(who);
    let account = frame_system::Account::<Test>::get(who);
    (
        exists,
        account.nonce,
        account.providers,
        account.sufficients,
    )
}

// A TAO-less coldkey that transfers stake across subnets to another coldkey must not have
// its own system account created and reaped by the dispatch: the nonce written by the
// nonce extension before dispatch has to survive, so the same signed call replays as Stale.
#[test]
fn test_cross_subnet_transfer_stake_keeps_signer_account_and_nonce() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid_a = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let netuid_b = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let funder = U256::from(1);
        let signer = U256::from(2);
        let destination = U256::from(3);
        let hotkey = U256::from(4);

        endow_alpha_only(
            funder,
            signer,
            hotkey,
            netuid_a,
            40 * DefaultMinStake::<Test>::get().to_u64(),
        );
        let _ = SubtensorModule::create_account_if_non_existent(&destination, &hotkey);

        let signer_alpha_before =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &signer, netuid_a);
        assert!(!signer_alpha_before.is_zero());
        assert_eq!(Balances::free_balance(signer), TaoBalance::ZERO);
        assert_eq!(system_account_state(&signer), (false, 0, 0, 0));

        let chunk: AlphaBalance = (signer_alpha_before.to_u64() / 4).into();
        let call = RuntimeCall::SubtensorModule(SubtensorCall::transfer_stake {
            destination_coldkey: destination,
            hotkey,
            origin_netuid: netuid_a,
            destination_netuid: netuid_b,
            alpha_amount: chunk,
        });

        // The nonce extension admits a reference-less signer for a call whose fee it does
        // not have to cover in TAO, and writes the bumped nonce before dispatch.
        let mut info = call.get_dispatch_info();
        info.pays_fee = Pays::No;
        assert_ok!(
            CheckNonce::<Test>::from(0)
                .validate_and_prepare(RawOrigin::Signed(signer).into(), &call, &info, 0, 0)
                .map(|_| ())
        );
        assert_eq!(system_account_state(&signer), (true, 1, 0, 0));

        assert_ok!(call.clone().dispatch(RawOrigin::Signed(signer).into()));

        // The signer never received TAO, so nothing could remove its account.
        assert_eq!(system_account_state(&signer), (true, 1, 0, 0));
        assert_eq!(Balances::free_balance(signer), TaoBalance::ZERO);
        assert!(
            !SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &destination,
                netuid_b
            )
            .is_zero()
        );

        // Replaying the identical signed call is rejected as stale.
        let replay = CheckNonce::<Test>::from(0)
            .validate_and_prepare(RawOrigin::Signed(signer).into(), &call, &info, 0, 0)
            .map(|_| ());
        assert_eq!(replay, Err(InvalidTransaction::Stale.into()));
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &signer, netuid_a),
            signer_alpha_before.saturating_sub(chunk)
        );
    });
}

/// A third party plants one distinct hotkey on `victim`'s `StakingHotkeys` with a dust
/// same-subnet transfer. Returns the hotkey used.
fn third_party_transfer_new_hotkey(
    attacker: &U256,
    victim: &U256,
    netuid: NetUid,
    idx: u64,
) -> Result<U256, sp_runtime::DispatchError> {
    let hotkey = U256::from(1_000_000_u64.saturating_add(idx));
    let min_transfer = DefaultMinTransfer::<Test>::get().to_u64();
    let _ = SubtensorModule::create_account_if_non_existent(attacker, &hotkey);
    add_balance_to_coldkey_account(attacker, TaoBalance::from(min_transfer.saturating_mul(4)));
    SubtensorModule::stake_into_subnet(
        &hotkey,
        attacker,
        netuid,
        TaoBalance::from(min_transfer.saturating_mul(2)),
        <Test as Config>::SwapInterface::max_price(),
        false,
    )
    .unwrap();
    let alpha =
        SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, attacker, netuid);
    let amount = AlphaBalance::from(min_transfer.saturating_add(1).min(alpha.to_u64()));
    SubtensorModule::do_transfer_stake(
        RuntimeOrigin::signed(*attacker),
        *victim,
        hotkey,
        netuid,
        netuid,
        amount,
    )
    .map(|_| hotkey)
}

// Third parties can grow a coldkey's `StakingHotkeys` only up to a fixed bound; the coldkey
// itself is not limited, and its coldkey-wide root claim stays admissible.
#[test]
fn test_third_party_transfers_cannot_grow_staking_hotkeys_without_bound() {
    new_test_ext(1).execute_with(|| {
        let netuid = add_dynamic_network(&U256::from(9002), &U256::from(9001));
        setup_reserves(
            netuid,
            TaoBalance::from(1_000_000_000_000_000_u64),
            AlphaBalance::from(1_000_000_000_000_000_u64),
        );
        let attacker = U256::from(1);
        let victim = U256::from(2);
        let cap = MAX_THIRD_PARTY_STAKING_HOTKEYS as u64;
        assert!(StakingHotkeys::<Test>::get(victim).is_empty());

        let mut planted = Vec::new();
        for i in 0..cap {
            planted.push(third_party_transfer_new_hotkey(&attacker, &victim, netuid, i).unwrap());
        }
        assert_eq!(StakingHotkeys::<Test>::get(victim).len() as u64, cap);

        // One more distinct hotkey is refused...
        assert_eq!(
            third_party_transfer_new_hotkey(&attacker, &victim, netuid, cap).unwrap_err(),
            Error::<Test>::TooManyStakingHotkeys.into()
        );
        assert_eq!(StakingHotkeys::<Test>::get(victim).len() as u64, cap);

        // ...but a transfer to a hotkey the victim already stakes through is fine.
        let existing = *planted.first().unwrap();
        let min_transfer = DefaultMinTransfer::<Test>::get().to_u64();
        add_balance_to_coldkey_account(&attacker, TaoBalance::from(min_transfer.saturating_mul(4)));
        SubtensorModule::stake_into_subnet(
            &existing,
            &attacker,
            netuid,
            TaoBalance::from(min_transfer.saturating_mul(4)),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();
        assert_ok!(SubtensorModule::do_transfer_stake(
            RuntimeOrigin::signed(attacker),
            victim,
            existing,
            netuid,
            netuid,
            AlphaBalance::from(min_transfer.saturating_mul(2)),
        ));
        assert_eq!(StakingHotkeys::<Test>::get(victim).len() as u64, cap);

        // The victim's own staking is not bounded by the third-party cap.
        let own_hotkey = U256::from(5_000_000);
        let _ = SubtensorModule::create_account_if_non_existent(&victim, &own_hotkey);
        add_balance_to_coldkey_account(&victim, TaoBalance::from(10_000_000_000_u64));
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(victim),
            own_hotkey,
            netuid,
            TaoBalance::from(1_000_000_000_u64),
        ));
        assert_eq!(
            StakingHotkeys::<Test>::get(victim).len() as u64,
            cap.saturating_add(1)
        );

        // The coldkey-wide root claim is still within its admission budget.
        assert!(cap < MAX_ROOT_CLAIM_WORK as u64);
        assert_ok!(SubtensorModule::claim_root(
            RuntimeOrigin::signed(victim),
            BTreeSet::new()
        ));
    });
}
