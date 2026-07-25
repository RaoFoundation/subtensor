#![allow(clippy::unwrap_used)]
#![allow(clippy::arithmetic_side_effects)]
//! Tests for [`crate::staking::remove_stake`] limit / max-amount remove paths.

use approx::assert_abs_diff_eq;
use frame_support::sp_runtime::DispatchError;
use frame_support::{assert_err, assert_noop, assert_ok};
use sp_core::U256;
use substrate_fixed::types::{U64F64, U96F32};
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};
use subtensor_swap_interface::SwapHandler;

use super::super::mock::*;
use crate::*;

#[test]
fn test_max_amount_remove_root() {
    new_test_ext(0).execute_with(|| {
        // 0 price on root => max is u64::MAX
        assert_eq!(
            SubtensorModule::get_max_amount_remove(NetUid::ROOT, TaoBalance::ZERO),
            Ok(AlphaBalance::MAX)
        );

        // 0.5 price on root => max is u64::MAX
        assert_eq!(
            SubtensorModule::get_max_amount_remove(NetUid::ROOT, TaoBalance::from(500_000_000)),
            Ok(AlphaBalance::MAX)
        );

        // 0.999999... price on root => max is u64::MAX
        assert_eq!(
            SubtensorModule::get_max_amount_remove(NetUid::ROOT, TaoBalance::from(999_999_999)),
            Ok(AlphaBalance::MAX)
        );

        // 1.0 price on root => max is u64::MAX
        assert_eq!(
            SubtensorModule::get_max_amount_remove(NetUid::ROOT, TaoBalance::from(1_000_000_000)),
            Ok(AlphaBalance::MAX)
        );

        // 1.000...001 price on root => max is 0
        assert_eq!(
            SubtensorModule::get_max_amount_remove(NetUid::ROOT, TaoBalance::from(1_000_000_001)),
            Ok(0u64.into())
        );

        // 2.0 price on root => max is 0
        assert_eq!(
            SubtensorModule::get_max_amount_remove(NetUid::ROOT, TaoBalance::from(2_000_000_000)),
            Ok(0u64.into())
        );
    });
}

#[test]
fn test_max_amount_remove_stable() {
    new_test_ext(0).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);

        // 0 price => max is u64::MAX
        assert_eq!(
            SubtensorModule::get_max_amount_remove(netuid, TaoBalance::ZERO),
            Ok(AlphaBalance::MAX)
        );

        // 0.999999... price => max is u64::MAX
        assert_eq!(
            SubtensorModule::get_max_amount_remove(netuid, TaoBalance::from(999_999_999)),
            Ok(AlphaBalance::MAX)
        );

        // 1.0 price => max is u64::MAX
        assert_eq!(
            SubtensorModule::get_max_amount_remove(netuid, TaoBalance::from(1_000_000_000)),
            Ok(AlphaBalance::MAX)
        );

        // 1.000...001 price => max is 0
        assert_eq!(
            SubtensorModule::get_max_amount_remove(netuid, TaoBalance::from(1_000_000_001)),
            Ok(0u64.into())
        );

        // 2.0 price => max is 0
        assert_eq!(
            SubtensorModule::get_max_amount_remove(netuid, TaoBalance::from(2_000_000_000)),
            Ok(0u64.into())
        );
    });
}

// cargo test --package pallet-subtensor --lib -- tests::staking::remove_stake_limit::test_max_amount_remove_dynamic --exact --show-output
#[test]
fn test_max_amount_remove_dynamic() {
    new_test_ext(0).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        // tao_in, alpha_in, limit_price, expected_max_swappable (+ 0.05% fee)
        [
            // Zero handling (no panics)
            (
                0_u64,
                1_000_000_000_u64,
                100,
                Err(DispatchError::from(
                    pallet_subtensor_swap::Error::<Test>::ReservesTooLow,
                )),
            ),
            (
                1_000_000_000,
                0,
                100,
                Err(DispatchError::from(
                    pallet_subtensor_swap::Error::<Test>::PriceLimitExceeded,
                )),
            ),
            (10_000_000_000, 10_000_000_000, 0, Ok(10_000_000_000_000)),
            // Low bounds (numbers are empirical, it is only important that result
            // is sharply decreasing when limit price increases)
            (1_000, 1_000, 0, Ok(1_000_000)),
            (1_001, 1_001, 0, Ok(1_001_000)),
            (1_001, 1_001, 1, Ok(1_001_000)),
            (1_001, 1_001, 2, Ok(1_001_000)),
            (1_001, 1_001, 1_001, Ok(1_001_000)),
            (1_001, 1_001, 10_000, Ok(17_472)),
            (1_001, 1_001, 100_000, Ok(17_472)),
            (1_001, 1_001, 1_000_000, Ok(17_472)),
            (1_001, 1_001, 10_000_000, Ok(9_013)),
            (1_001, 1_001, 100_000_000, Ok(2_165)),
            // Basic math
            (1_000_000, 1_000_000, 250_000_000, Ok(1_010_000)),
            (1_000_000, 1_000_000, 62_500_000, Ok(3_030_000)),
            (
                1_000_000_000_000,
                1_000_000_000_000,
                62_500_000,
                Ok(3_030_000_000_000),
            ),
            // Normal range values with edge cases and sanity checks
            (200_000_000_000, 100_000_000_000, 0, Ok(100_000_000_000_000)),
            (
                200_000_000_000,
                100_000_000_000,
                500_000_000,
                Ok(101_000_000_000),
            ),
            (
                200_000_000_000,
                100_000_000_000,
                125_000_000,
                Ok(303_000_000_000),
            ),
            (
                200_000_000_000,
                100_000_000_000,
                2_000_000_000,
                Err(DispatchError::from(
                    pallet_subtensor_swap::Error::<Test>::PriceLimitExceeded,
                )),
            ),
            (
                200_000_000_000,
                100_000_000_000,
                2_000_000_001,
                Err(DispatchError::from(
                    pallet_subtensor_swap::Error::<Test>::PriceLimitExceeded,
                )),
            ),
            (200_000_000_000, 100_000_000_000, 1_999_999_999, Ok(24)),
            (200_000_000_000, 100_000_000_000, 1_999_999_990, Ok(250)),
            // Miscellaneous overflows and underflows
            (
                21_000_000_000_000_000,
                1_000_000,
                21_000_000_000_000_000,
                Ok(17_455_533),
            ),
            (21_000_000_000_000_000, 1_000_000, u64::MAX, Ok(67_000)),
            (
                21_000_000_000_000_000,
                1_000_000_000_000_000_000,
                u64::MAX,
                Err(DispatchError::from(
                    pallet_subtensor_swap::Error::<Test>::PriceLimitExceeded,
                )),
            ),
            (
                21_000_000_000_000_000,
                1_000_000_000_000_000_000,
                20_000_000,
                Ok(24_700_000_000_000_000),
            ),
            (
                21_000_000_000_000_000,
                21_000_000_000_000_000,
                999_999_999,
                Ok(10_605_000),
            ),
            (
                21_000_000_000_000_000,
                21_000_000_000_000_000,
                0,
                Ok(u64::MAX),
            ),
        ]
        .into_iter()
        .for_each(|(tao_in, alpha_in, limit_price, expected_max_swappable)| {
            let alpha_in = AlphaBalance::from(alpha_in);
            // Forse-set alpha in and tao reserve to achieve relative price of subnets
            SubnetTAO::<Test>::insert(netuid, TaoBalance::from(tao_in));
            SubnetAlphaIn::<Test>::insert(netuid, alpha_in);

            if !alpha_in.is_zero() {
                let expected_price = U64F64::from_num(tao_in) / U64F64::from_num(alpha_in);
                assert_eq!(
                    <Test as pallet::Config>::SwapInterface::current_alpha_price(netuid.into()),
                    expected_price
                );
            }

            match expected_max_swappable {
                Err(e) => assert_err!(
                    SubtensorModule::get_max_amount_remove(netuid, limit_price.into()),
                    DispatchError::from(e)
                ),
                Ok(v) => {
                    let v = AlphaBalance::from(v);
                    let actual =
                        SubtensorModule::get_max_amount_remove(netuid, limit_price.into()).unwrap();
                    let epsilon = v / 100.into();
                    let diff = actual.max(v).saturating_sub(actual.min(v));
                    assert!(
                        diff <= epsilon,
                        "max remove mismatch: tao_in={tao_in}, alpha_in={alpha_in:?}, limit_price={limit_price}, actual={actual:?}, expected={v:?}, epsilon={epsilon:?}",
                    );
                }
            }
        });
    });
}

#[test]
fn test_remove_stake_limit_ok() {
    new_test_ext(1).execute_with(|| {
        let hotkey_account_id = U256::from(533453);
        let coldkey_account_id = U256::from(55453);
        let stake_amount = TaoBalance::from(300_000_000_000_u64);

        // add network
        let netuid = add_dynamic_network(&hotkey_account_id, &coldkey_account_id);
        add_balance_to_coldkey_account(
            &coldkey_account_id,
            stake_amount + ExistentialDeposit::get(),
        );

        // Forse-set sufficient reserves
        let tao_reserve = TaoBalance::from(100_000_000_000_u64);
        let alpha_in = AlphaBalance::from(100_000_000_000_u64);
        SubnetTAO::<Test>::insert(netuid, tao_reserve);
        SubnetAlphaIn::<Test>::insert(netuid, alpha_in);

        // Stake to hotkey account, and check if the result is ok
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
            netuid,
            stake_amount
        ));
        let alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &coldkey_account_id,
            netuid,
        );

        // Setup limit price to 99% of current price
        let current_price =
            <Test as pallet::Config>::SwapInterface::current_alpha_price(netuid.into());
        let limit_price = (current_price.to_num::<f64>() * 990_000_000_f64) as u64;

        // Alpha unstaked - calculated using formula from delta_in()
        let expected_alpha_reduction = (0.00138 * (alpha_in.to_u64() as f64)) as u64;
        let fee: u64 = (expected_alpha_reduction as f64 * 0.003) as u64;

        // Remove stake with slippage safety
        assert_ok!(SubtensorModule::remove_stake_limit(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
            netuid,
            alpha_before / 2.into(),
            limit_price.into(),
            true
        ));
        let alpha_after = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &coldkey_account_id,
            netuid,
        );

        // Check if stake has decreased properly
        assert_abs_diff_eq!(
            alpha_before - alpha_after,
            AlphaBalance::from(expected_alpha_reduction + fee),
            epsilon = AlphaBalance::from(expected_alpha_reduction / 10),
        );
    });
}

#[test]
fn test_remove_stake_limit_fill_or_kill() {
    new_test_ext(1).execute_with(|| {
        let hotkey_account_id = U256::from(533453);
        let coldkey_account_id = U256::from(55453);
        let stake_amount = AlphaBalance::from(300_000_000_000_u64);
        let unstake_amount = AlphaBalance::from(150_000_000_000_u64);

        // add network
        let netuid = add_dynamic_network(&hotkey_account_id, &coldkey_account_id);

        // Give the neuron some stake to remove
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &coldkey_account_id,
            netuid,
            stake_amount,
        );

        // Forse-set alpha in and tao reserve to make price equal 1.5
        let tao_reserve = TaoBalance::from(150_000_000_000_u64);
        let alpha_in = AlphaBalance::from(100_000_000_000_u64);
        SubnetTAO::<Test>::insert(netuid, tao_reserve);
        SubnetAlphaIn::<Test>::insert(netuid, alpha_in);
        let current_price =
            <Test as pallet::Config>::SwapInterface::current_alpha_price(netuid.into());
        assert_eq!(current_price, U96F32::from_num(1.5));

        // Setup limit price so that it doesn't drop by more than 10% from current price
        let limit_price = TaoBalance::from(1_350_000_000);

        // Remove stake with slippage safety - fails
        assert_noop!(
            SubtensorModule::remove_stake_limit(
                RuntimeOrigin::signed(coldkey_account_id),
                hotkey_account_id,
                netuid,
                unstake_amount,
                limit_price,
                false
            ),
            Error::<Test>::SlippageTooHigh
        );

        // Lower the amount: Should succeed
        assert_ok!(SubtensorModule::remove_stake_limit(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
            netuid,
            unstake_amount / 100.into(),
            limit_price.into(),
            false
        ),);
    });
}

#[test]
fn test_remove_stake_full_limit_ok() {
    new_test_ext(1).execute_with(|| {
        let hotkey_account_id = U256::from(1);
        let coldkey_account_id = U256::from(2);
        let stake_amount = AlphaBalance::from(10_000_000_000_u64);

        // add network
        let netuid = add_dynamic_network(&hotkey_account_id, &coldkey_account_id);
        remove_owner_registration_stake(netuid);

        // Give the neuron some stake to remove
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &coldkey_account_id,
            netuid,
            stake_amount,
        );

        let tao_reserve = TaoBalance::from(100_000_000_000_u64);
        let alpha_in = AlphaBalance::from(100_000_000_000_u64);
        SubnetTAO::<Test>::insert(netuid, tao_reserve);
        SubnetAlphaIn::<Test>::insert(netuid, alpha_in);

        let limit_price = TaoBalance::from(90_000_000);

        // Remove stake with slippage safety
        assert_ok!(SubtensorModule::remove_stake_full_limit(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
            netuid,
            Some(limit_price),
        ));

        // Check if stake has decreased to zero
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey_account_id,
                &coldkey_account_id,
                netuid
            ),
            AlphaBalance::ZERO
        );

        let new_balance = SubtensorModule::get_coldkey_balance(&coldkey_account_id);
        assert_abs_diff_eq!(
            new_balance,
            9_086_000_000_u64.into(),
            epsilon = 1_000_000.into()
        );
    });
}

#[test]
fn test_remove_stake_full_limit_fails_slippage_too_high() {
    new_test_ext(1).execute_with(|| {
        let hotkey_account_id = U256::from(1);
        let coldkey_account_id = U256::from(2);
        let stake_amount = AlphaBalance::from(10_000_000_000_u64);

        // add network
        let netuid = add_dynamic_network(&hotkey_account_id, &coldkey_account_id);

        // Give the neuron some stake to remove
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &coldkey_account_id,
            netuid,
            stake_amount,
        );

        let tao_reserve = TaoBalance::from(100_000_000_000_u64);
        let alpha_in = AlphaBalance::from(100_000_000_000_u64);
        SubnetTAO::<Test>::insert(netuid, tao_reserve);
        SubnetAlphaIn::<Test>::insert(netuid, alpha_in);

        let invalid_limit_price = TaoBalance::from(910_000_000_u64);

        // Remove stake with slippage safety
        assert_err!(
            SubtensorModule::remove_stake_full_limit(
                RuntimeOrigin::signed(coldkey_account_id),
                hotkey_account_id,
                netuid,
                Some(invalid_limit_price),
            ),
            Error::<Test>::SlippageTooHigh
        );
    });
}

#[test]
fn test_remove_stake_full_limit_ok_with_no_limit_price() {
    new_test_ext(1).execute_with(|| {
        let hotkey_account_id = U256::from(1);
        let coldkey_account_id = U256::from(2);
        let stake_amount = AlphaBalance::from(10_000_000_000_u64);

        // add network
        let netuid = add_dynamic_network(&hotkey_account_id, &coldkey_account_id);
        remove_owner_registration_stake(netuid);

        // Give the neuron some stake to remove
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &coldkey_account_id,
            netuid,
            stake_amount,
        );

        let tao_reserve = TaoBalance::from(100_000_000_000_u64);
        let alpha_in = AlphaBalance::from(100_000_000_000_u64);
        SubnetTAO::<Test>::insert(netuid, tao_reserve);
        SubnetAlphaIn::<Test>::insert(netuid, alpha_in);

        // Remove stake with slippage safety
        assert_ok!(SubtensorModule::remove_stake_full_limit(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
            netuid,
            None,
        ));

        // Check if stake has decreased to zero
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey_account_id,
                &coldkey_account_id,
                netuid
            ),
            AlphaBalance::ZERO
        );

        let new_balance = SubtensorModule::get_coldkey_balance(&coldkey_account_id);
        assert_abs_diff_eq!(
            new_balance,
            9_086_000_000_u64.into(),
            epsilon = 1_000_000.into()
        );
    });
}
