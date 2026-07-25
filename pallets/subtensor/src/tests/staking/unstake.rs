#![allow(clippy::unwrap_used)]
#![allow(clippy::arithmetic_side_effects)]
//! Tests for [`crate::staking::remove_stake`] unstake-all / unstake-from-subnet paths.

use approx::assert_abs_diff_eq;
use frame_support::sp_runtime::DispatchError;
use frame_support::{assert_err, assert_ok};
use safe_math::FixedExt;
use sp_core::U256;
use substrate_fixed::traits::FromFixed;
use substrate_fixed::types::{I96F32, I110F18};
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};
use subtensor_swap_interface::{Order, SwapHandler};

use super::super::mock;
use super::super::mock::*;
use crate::*;

/// cargo test --package pallet-subtensor --lib -- tests::staking::unstake::test_unstake_all_hits_liquidity_min --exact --show-output
#[test]
fn test_unstake_all_hits_liquidity_min() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);

        let stake_amount = AlphaBalance::from(190_000_000_000_u64); // 190 Alpha

        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        register_ok_neuron(netuid, hotkey, coldkey, 192213123);
        // Give the neuron some stake to remove
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            stake_amount,
        );

        // Setup the Alpha pool so that removing all the Alpha will bring liqudity below the minimum
        let remaining_tao = TaoBalance::from(u64::from(mock::SwapMinimumReserve::get()) - 1);
        let alpha_reserves = AlphaBalance::from(stake_amount.to_u64() + 10_000_000);
        mock::setup_reserves(netuid, remaining_tao, alpha_reserves);

        // Try to unstake, but we reduce liquidity too far

        assert_ok!(SubtensorModule::unstake_all(
            RuntimeOrigin::signed(coldkey),
            hotkey,
        ));

        // Expect nothing to be unstaked
        let new_alpha =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);
        assert_abs_diff_eq!(new_alpha, stake_amount, epsilon = AlphaBalance::ZERO);
    });
}

#[test]
fn test_unstake_all_alpha_hits_liquidity_min() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);

        let stake_amount = TaoBalance::from(100_000_000_000_u64); // 100 TAO

        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        register_ok_neuron(netuid, hotkey, coldkey, 192213123);
        add_balance_to_coldkey_account(&coldkey, stake_amount + ExistentialDeposit::get());
        // Give the neuron some stake to remove
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            stake_amount
        ));

        // Setup the pool so that removing all the TAO will bring liqudity below the minimum
        let remaining_tao = I96F32::from_num(u64::from(mock::SwapMinimumReserve::get()) - 1)
            .saturating_sub(I96F32::from(1));
        let alpha =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);
        let alpha_reserves = I110F18::from(u64::from(alpha) + 10_000_000);

        let k = I110F18::from_fixed(remaining_tao)
            .saturating_mul(alpha_reserves.saturating_add(I110F18::from(u64::from(alpha))));
        let tao_reserves = k.safe_div(alpha_reserves);

        mock::setup_reserves(
            netuid,
            (tao_reserves.to_num::<u64>() / 100_u64).into(),
            alpha_reserves.to_num::<u64>().into(),
        );

        // Try to unstake, but we reduce liquidity too far

        assert_err!(
            SubtensorModule::unstake_all_alpha(RuntimeOrigin::signed(coldkey), hotkey),
            Error::<Test>::AmountTooLow
        );

        // Expect nothing to be unstaked
        let new_alpha =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);
        assert_eq!(new_alpha, alpha);
    });
}

#[test]
fn test_unstake_all_alpha_works() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);

        let stake_amount = TaoBalance::from(190_000_000_000_u64); // 190 TAO

        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        register_ok_neuron(netuid, hotkey, coldkey, 192213123);
        add_balance_to_coldkey_account(&coldkey, stake_amount + ExistentialDeposit::get());

        // Give the neuron some stake to remove
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            stake_amount
        ));

        // Setup the pool so that removing all the TAO will keep liq above min
        mock::setup_reserves(
            netuid,
            stake_amount * 10.into(),
            u64::from(stake_amount * 100.into()).into(),
        );

        // Unstake all alpha to root
        assert_ok!(SubtensorModule::unstake_all_alpha(
            RuntimeOrigin::signed(coldkey),
            hotkey,
        ));

        let new_alpha =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);
        assert_abs_diff_eq!(new_alpha, AlphaBalance::ZERO, epsilon = 1_000.into());
        let new_root = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            NetUid::ROOT,
        );
        assert!(new_root > 100_000.into());
    });
}

#[test]
fn test_unstake_all_works() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);

        let stake_amount = TaoBalance::from(190_000_000_000_u64); // 190 TAO

        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        register_ok_neuron(netuid, hotkey, coldkey, 192213123);
        add_balance_to_coldkey_account(&coldkey, stake_amount + ExistentialDeposit::get());

        // Give the neuron some stake to remove
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            stake_amount
        ));

        // Setup the pool so that removing all the TAO will keep liq above min
        mock::setup_reserves(
            netuid,
            stake_amount * 10.into(),
            u64::from(stake_amount * 100.into()).into(),
        );
        // Unstake all alpha to free balance
        assert_ok!(SubtensorModule::unstake_all(
            RuntimeOrigin::signed(coldkey),
            hotkey,
        ));

        let new_alpha =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);
        assert_abs_diff_eq!(new_alpha, AlphaBalance::ZERO, epsilon = 1_000.into());
        let new_balance = SubtensorModule::get_coldkey_balance(&coldkey);
        assert!(new_balance > 100_000.into());
    });
}

#[test]
fn test_unstake_from_subnet_low_amount() {
    new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(1);
        let owner_coldkey = U256::from(2);
        let hotkey = U256::from(3);
        let coldkey = U256::from(4);
        let amount = 10;

        // add network
        let netuid = add_dynamic_network(&owner_hotkey, &owner_coldkey);

        // Forse-set alpha in and tao reserve to make price equal 0.01
        let tao_reserve = TaoBalance::from(100_000_000_000_u64);
        let alpha_in = AlphaBalance::from(1_000_000_000_000_u64);
        mock::setup_reserves(netuid, tao_reserve, alpha_in);

        // Initialize swap v3
        let order = GetAlphaForTao::<Test>::with_amount(0);
        assert_ok!(<tests::mock::Test as pallet::Config>::SwapInterface::swap(
            netuid.into(),
            order,
            TaoBalance::MAX,
            false,
            true
        ));

        // Add stake and check if the result is ok
        let large_balance = 20_000_000_000_000_000_u64;
        add_balance_to_coldkey_account(&coldkey, large_balance.into());
        assert_ok!(SubtensorModule::stake_into_subnet(
            &hotkey,
            &coldkey,
            netuid,
            amount.into(),
            large_balance.into(),
            false,
        ));

        // Remove stake
        let alpha =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);
        assert_ok!(SubtensorModule::unstake_from_subnet(
            &hotkey,
            &coldkey,
            &coldkey,
            netuid,
            alpha,
            TaoBalance::ZERO,
            false,
        ));

        // Check if stake is zero
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid),
            AlphaBalance::ZERO,
        );
    });
}

#[test]
fn test_unstake_from_subnet_prohibitive_limit() {
    new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(1);
        let owner_coldkey = U256::from(2);
        let coldkey = U256::from(4);
        let amount = 100_000_000;

        // add network
        let netuid = add_dynamic_network(&owner_hotkey, &owner_coldkey);
        add_balance_to_coldkey_account(&coldkey, amount.into());

        // Forse-set alpha in and tao reserve to make price equal 0.01
        let tao_reserve = TaoBalance::from(100_000_000_000_u64);
        let alpha_in = AlphaBalance::from(1_000_000_000_000_u64);
        mock::setup_reserves(netuid, tao_reserve, alpha_in);

        // Initialize swap v3
        let order = GetAlphaForTao::<Test>::with_amount(0);
        assert_ok!(<tests::mock::Test as pallet::Config>::SwapInterface::swap(
            netuid.into(),
            order,
            TaoBalance::MAX,
            false,
            true
        ));

        // Add stake and check if the result is ok
        assert_ok!(SubtensorModule::stake_into_subnet(
            &owner_hotkey,
            &coldkey,
            netuid,
            amount.into(),
            TaoBalance::MAX,
            false,
        ));

        // Remove stake
        // Use prohibitive limit price
        let balance_before = SubtensorModule::get_coldkey_balance(&coldkey);
        let alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &owner_hotkey,
            &coldkey,
            netuid,
        );
        assert_err!(
            SubtensorModule::remove_stake_limit(
                RuntimeOrigin::signed(coldkey),
                owner_hotkey,
                netuid,
                alpha,
                TaoBalance::MAX,
                true,
            ),
            DispatchError::from(pallet_subtensor_swap::Error::<Test>::PriceLimitExceeded)
        );

        // Check if stake has NOT decreased
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &owner_hotkey,
                &coldkey,
                netuid
            ),
            alpha
        );

        // Check if balance has NOT increased
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&coldkey),
            balance_before,
        );
    });
}

#[test]
fn test_unstake_full_amount() {
    new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(1);
        let owner_coldkey = U256::from(2);
        let coldkey = U256::from(4);
        let amount = 100_000_000;

        // add network
        let netuid = add_dynamic_network(&owner_hotkey, &owner_coldkey);
        add_balance_to_coldkey_account(&coldkey, amount.into());

        // Forse-set alpha in and tao reserve to make price equal 0.01
        let tao_reserve = TaoBalance::from(100_000_000_000_u64);
        let alpha_in = AlphaBalance::from(1_000_000_000_000_u64);
        mock::setup_reserves(netuid, tao_reserve, alpha_in);

        // Initialize swap v3
        let order = GetAlphaForTao::<Test>::with_amount(0);
        assert_ok!(<tests::mock::Test as pallet::Config>::SwapInterface::swap(
            netuid.into(),
            order,
            TaoBalance::MAX,
            false,
            true
        ));

        // Add stake and check if the result is ok
        assert_ok!(SubtensorModule::stake_into_subnet(
            &owner_hotkey,
            &coldkey,
            netuid,
            amount.into(),
            TaoBalance::MAX,
            false,
        ));

        // Remove stake
        // Use prohibitive limit price
        let balance_before = SubtensorModule::get_coldkey_balance(&coldkey);
        let alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &owner_hotkey,
            &coldkey,
            netuid,
        );
        assert_ok!(SubtensorModule::remove_stake(
            RuntimeOrigin::signed(coldkey),
            owner_hotkey,
            netuid,
            alpha,
        ));

        // Check if stake is zero
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &owner_hotkey,
                &coldkey,
                netuid
            ),
            AlphaBalance::ZERO
        );

        // Check if balance has increased accordingly
        let balance_after = SubtensorModule::get_coldkey_balance(&coldkey);
        let actual_balance_increase = u64::from(balance_after - balance_before) as f64;
        let fee_rate = pallet_subtensor_swap::FeeRate::<Test>::get(NetUid::from(netuid)) as f64
            / u16::MAX as f64;
        let expected_balance_increase = amount as f64 * (1. - fee_rate) / (1. + fee_rate);
        assert_abs_diff_eq!(
            actual_balance_increase,
            expected_balance_increase,
            epsilon = expected_balance_increase / 10_000.
        );
    });
}
