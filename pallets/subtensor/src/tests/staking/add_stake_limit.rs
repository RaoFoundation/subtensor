#![allow(clippy::unwrap_used)]
#![allow(clippy::arithmetic_side_effects)]
//! Tests for [`crate::staking::add_stake`] limit / max-amount add paths.

use approx::assert_abs_diff_eq;
use frame_support::sp_runtime::DispatchError;
use frame_support::{assert_err, assert_noop, assert_ok};
use sp_core::U256;
use substrate_fixed::types::U96F32;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};
use subtensor_swap_interface::SwapHandler;

use super::super::mock;
use super::super::mock::*;
use crate::*;

#[test]
fn test_max_amount_add_root() {
    new_test_ext(0).execute_with(|| {
        // 0 price on root => max is 0
        assert_eq!(
            SubtensorModule::get_max_amount_add(NetUid::ROOT, TaoBalance::ZERO),
            Ok(0u64.into())
        );

        // 0.999999... price on root => max is 0
        assert_eq!(
            SubtensorModule::get_max_amount_add(NetUid::ROOT, TaoBalance::from(999_999_999)),
            Ok(0u64.into())
        );

        // 1.0 price on root => max is u64::MAX
        assert_eq!(
            SubtensorModule::get_max_amount_add(NetUid::ROOT, TaoBalance::from(1_000_000_000)),
            Ok(u64::MAX)
        );

        // 1.000...001 price on root => max is u64::MAX
        assert_eq!(
            SubtensorModule::get_max_amount_add(NetUid::ROOT, TaoBalance::from(1_000_000_001)),
            Ok(u64::MAX)
        );

        // 2.0 price on root => max is u64::MAX
        assert_eq!(
            SubtensorModule::get_max_amount_add(NetUid::ROOT, TaoBalance::from(2_000_000_000)),
            Ok(u64::MAX)
        );
    });
}

#[test]
fn test_max_amount_add_stable() {
    new_test_ext(0).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);

        // 0 price => max is 0
        assert_eq!(
            SubtensorModule::get_max_amount_add(netuid, TaoBalance::ZERO),
            Ok(0u64.into())
        );

        // 0.999999... price => max is 0
        assert_eq!(
            SubtensorModule::get_max_amount_add(netuid, TaoBalance::from(999_999_999)),
            Ok(0u64.into())
        );

        // 1.0 price => max is u64::MAX
        assert_eq!(
            SubtensorModule::get_max_amount_add(netuid, TaoBalance::from(1_000_000_000)),
            Ok(u64::MAX)
        );

        // 1.000...001 price => max is u64::MAX
        assert_eq!(
            SubtensorModule::get_max_amount_add(netuid, TaoBalance::from(1_000_000_001)),
            Ok(u64::MAX)
        );

        // 2.0 price => max is u64::MAX
        assert_eq!(
            SubtensorModule::get_max_amount_add(netuid, TaoBalance::from(2_000_000_000)),
            Ok(u64::MAX)
        );
    });
}

// cargo test --package pallet-subtensor --lib -- tests::staking::add_stake_limit::test_max_amount_add_dynamic --exact --show-output
#[test]
fn test_max_amount_add_dynamic() {
    // tao_in, alpha_in, limit_price, expected_max_swappable (with 0.05% fees)
    [
        // Zero handling (no panics)
        (
            1_000_000_000,
            1_000_000_000,
            0,
            Err(DispatchError::from(
                pallet_subtensor_swap::Error::<Test>::PriceLimitExceeded,
            )),
        ),
        // Low bounds
        (100, 100, 1_100_000_000, Ok(4)),
        (1_000, 1_000, 1_100_000_000, Ok(48)),
        (10_000, 10_000, 1_100_000_000, Ok(488)),
        // Basic math
        (1_000_000, 1_000_000, 4_000_000_000, Ok(1_000_500)),
        (1_000_000, 1_000_000, 9_000_000_000, Ok(2_001_000)),
        (1_000_000, 1_000_000, 16_000_000_000, Ok(3_001_500)),
        (
            1_000_000_000_000,
            1_000_000_000_000,
            16_000_000_000,
            Ok(3_001_500_000_000),
        ),
        // Normal range values with edge cases
        (
            150_000_000_000,
            100_000_000_000,
            0,
            Err(DispatchError::from(
                pallet_subtensor_swap::Error::<Test>::PriceLimitExceeded,
            )),
        ),
        (
            150_000_000_000,
            100_000_000_000,
            100_000_000,
            Err(DispatchError::from(
                pallet_subtensor_swap::Error::<Test>::PriceLimitExceeded,
            )),
        ),
        (
            150_000_000_000,
            100_000_000_000,
            500_000_000,
            Err(DispatchError::from(
                pallet_subtensor_swap::Error::<Test>::PriceLimitExceeded,
            )),
        ),
        (
            150_000_000_000,
            100_000_000_000,
            1_499_999_999,
            Err(DispatchError::from(
                pallet_subtensor_swap::Error::<Test>::PriceLimitExceeded,
            )),
        ),
        (
            150_000_000_000,
            100_000_000_000,
            1_500_000_000,
            Err(DispatchError::from(
                pallet_subtensor_swap::Error::<Test>::PriceLimitExceeded,
            )),
        ),
        (150_000_000_000, 100_000_000_000, 1_500_000_001, Ok(49)),
        (
            150_000_000_000,
            100_000_000_000,
            6_000_000_000,
            Ok(150_075_000_000),
        ),
        // Miscellaneous overflows and underflows
        (u64::MAX / 2, u64::MAX, u64::MAX, Ok(u64::MAX)),
    ]
    .into_iter()
    .for_each(|(tao_in, alpha_in, limit_price, expected_max_swappable)| {
        new_test_ext(0).execute_with(|| {
            let alpha_in = AlphaBalance::from(alpha_in);
            let subnet_owner_coldkey = U256::from(1001);
            let subnet_owner_hotkey = U256::from(1002);
            let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

            // Forse-set alpha in and tao reserve to achieve relative price of subnets
            SubnetTAO::<Test>::insert(netuid, TaoBalance::from(tao_in));
            SubnetAlphaIn::<Test>::insert(netuid, alpha_in);

            // Force the swap to initialize
            <Test as pallet::Config>::SwapInterface::init_swap(netuid, None);

            if !alpha_in.is_zero() {
                let expected_price = U96F32::from_num(tao_in) / U96F32::from_num(alpha_in);
                assert_abs_diff_eq!(
                    <Test as pallet::Config>::SwapInterface::current_alpha_price(netuid.into())
                        .to_num::<f64>(),
                    expected_price.to_num::<f64>(),
                    epsilon = expected_price.to_num::<f64>() / 1_000_f64
                );
            }

            match expected_max_swappable {
                Err(e) => assert_err!(
                    SubtensorModule::get_max_amount_add(netuid, limit_price.into()),
                    e
                ),
                Ok(v) => assert_abs_diff_eq!(
                    SubtensorModule::get_max_amount_add(netuid, limit_price.into()).unwrap(),
                    v,
                    epsilon = v / 10000
                ),
            }
        });
    });
}

#[test]
fn test_add_stake_limit_ok() {
    new_test_ext(1).execute_with(|| {
        let hotkey_account_id = U256::from(533453);
        let coldkey_account_id = U256::from(55453);
        let amount = 900_000_000_000; // over the maximum

        // add network
        let netuid = add_dynamic_network(&hotkey_account_id, &coldkey_account_id);
        remove_owner_registration_stake(netuid);

        // Forse-set alpha in and tao reserve to make price equal 1.5
        let tao_reserve = TaoBalance::from(150_000_000_000_u64);
        let alpha_in = AlphaBalance::from(100_000_000_000_u64);
        mock::setup_reserves(netuid, tao_reserve, alpha_in);
        let current_price =
            <Test as pallet::Config>::SwapInterface::current_alpha_price(netuid.into());
        assert_eq!(current_price, U96F32::from_num(1.5));

        // Give it some $$$ in his coldkey balance
        add_balance_to_coldkey_account(&coldkey_account_id, amount.into());

        // Setup limit price so that it doesn't peak above 4x of current price
        // The amount that can be executed at this price is 450 TAO only
        // Alpha produced will be equal to 75 = 450*100/(450+150)
        let limit_price = TaoBalance::from(24_000_000_000_u64);
        let expected_executed_stake = AlphaBalance::from(75_000_000_000_u64);

        // Add stake with slippage safety and check if the result is ok
        assert_ok!(SubtensorModule::add_stake_limit(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
            netuid,
            amount.into(),
            limit_price,
            true
        ));

        // Check if stake has increased only by 75 Alpha
        assert_abs_diff_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey_account_id,
                &coldkey_account_id,
                netuid
            ),
            expected_executed_stake,
            epsilon = expected_executed_stake / 1000.into(),
        );

        // Check that 450 TAO less fees balance still remains free on coldkey
        let fee = <tests::mock::Test as pallet::Config>::SwapInterface::approx_fee_amount(
            netuid.into(),
            TaoBalance::from(amount / 2),
        )
        .to_u64() as f64;
        assert_abs_diff_eq!(
            SubtensorModule::get_coldkey_balance(&coldkey_account_id),
            (amount / 2 - fee as u64).into(),
            epsilon = (amount / 2 / 1000).into()
        );

        // Check that price has updated to ~24 = (150+450) / (100 - 75)
        let exp_price = U96F32::from_num(24.0);
        let current_price =
            <Test as pallet::Config>::SwapInterface::current_alpha_price(netuid.into());
        assert_abs_diff_eq!(
            exp_price.to_num::<f64>(),
            current_price.to_num::<f64>(),
            epsilon = 0.001,
        );
    });
}

#[test]
fn test_add_stake_limit_fill_or_kill() {
    new_test_ext(1).execute_with(|| {
        let hotkey_account_id = U256::from(533453);
        let coldkey_account_id = U256::from(55453);
        let amount = 900_000_000_000_u64; // over the maximum

        // add network
        let netuid = add_dynamic_network(&hotkey_account_id, &coldkey_account_id);

        // Force-set alpha in and tao reserve to make price equal 1.5
        let tao_reserve = TaoBalance::from(150_000_000_000_u64);
        let alpha_in = AlphaBalance::from(100_000_000_000_u64);
        SubnetTAO::<Test>::insert(netuid, tao_reserve);
        SubnetAlphaIn::<Test>::insert(netuid, alpha_in);
        let current_price =
            <Test as pallet::Config>::SwapInterface::current_alpha_price(netuid.into());
        // FIXME it's failing because in the swap pallet, the alpha price is set only after an
        // initial swap
        assert_eq!(current_price, U96F32::from_num(1.5));

        // Give it some $$$ in his coldkey balance
        add_balance_to_coldkey_account(&coldkey_account_id, amount.into());

        // Setup limit price so that it doesn't peak above 4x of current price
        // The amount that can be executed at this price is 450 TAO only
        // Alpha produced will be equal to 25 = 100 - 450*100/(150+450)
        let limit_price = TaoBalance::from(24_000_000_000_u64);

        // Add stake with slippage safety and check if it fails
        assert_noop!(
            SubtensorModule::add_stake_limit(
                RuntimeOrigin::signed(coldkey_account_id),
                hotkey_account_id,
                netuid,
                amount.into(),
                limit_price,
                false
            ),
            Error::<Test>::SlippageTooHigh
        );

        // Lower the amount and it should succeed now
        let amount_ok = TaoBalance::from(150_000_000_000_u64); // fits the maximum
        assert_ok!(SubtensorModule::add_stake_limit(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
            netuid,
            amount_ok,
            limit_price,
            false
        ));
    });
}

#[test]
fn test_add_stake_limit_rejects_input_over_swap_reserve_cap() {
    new_test_ext(1).execute_with(|| {
        let hotkey_account_id = U256::from(533454);
        let coldkey_account_id = U256::from(55454);

        let netuid = add_dynamic_network(&hotkey_account_id, &coldkey_account_id);
        let tao_reserve = TaoBalance::from(1_000_u64);
        mock::setup_reserves(netuid, tao_reserve, AlphaBalance::from(1_000_000_000_u64));

        let amount = tao_reserve.saturating_mul(1_000.into()) + TaoBalance::from(1_u64);
        add_balance_to_coldkey_account(&coldkey_account_id, amount);

        assert_noop!(
            SubtensorModule::add_stake_limit(
                RuntimeOrigin::signed(coldkey_account_id),
                hotkey_account_id,
                netuid,
                amount,
                <Test as pallet::Config>::SwapInterface::max_price(),
                true
            ),
            Error::<Test>::InsufficientLiquidity
        );
    });
}

#[test]
fn test_add_stake_limit_partial_zero_max_stake_amount_error() {
    new_test_ext(1).execute_with(|| {
        let hotkey_account_id = U256::from(533453);
        let coldkey_account_id = U256::from(55453);

        // Exact values from the error:
        // https://taostats.io/extrinsic/5338471-0009?network=finney
        let amount = 19980000000_u64;
        let limit_price = TaoBalance::from(26953618);
        let tao_reserve = TaoBalance::from(5_032_494_439_940_u64);
        let alpha_in = AlphaBalance::from(186_268_425_402_874_u64);

        let netuid = add_dynamic_network(&hotkey_account_id, &coldkey_account_id);
        SubnetTAO::<Test>::insert(netuid, tao_reserve);
        SubnetAlphaIn::<Test>::insert(netuid, alpha_in);

        add_balance_to_coldkey_account(&coldkey_account_id, amount.into());

        assert_noop!(
            SubtensorModule::add_stake_limit(
                RuntimeOrigin::signed(coldkey_account_id),
                hotkey_account_id,
                netuid,
                amount.into(),
                limit_price,
                true
            ),
            DispatchError::from(pallet_subtensor_swap::Error::<Test>::PriceLimitExceeded)
        );
    });
}
