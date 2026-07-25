#![allow(clippy::unwrap_used)]
#![allow(clippy::arithmetic_side_effects)]
//! Tests for [`crate::staking::add_stake`] and stake-into-subnet paths.

use approx::assert_abs_diff_eq;
use frame_support::dispatch::{DispatchClass, GetDispatchInfo, Pays};
use frame_support::sp_runtime::DispatchError;
use frame_support::{assert_err, assert_noop, assert_ok, traits::Currency};
use frame_system::RawOrigin;
use sp_core::{Get, U256};
use sp_runtime::PerU16;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};
use subtensor_swap_interface::{Order, SwapHandler};

use super::super::mock;
use super::super::mock::*;
use crate::*;

#[test]
fn test_add_stake_dispatch_info_ok() {
    new_test_ext(1).execute_with(|| {
        let hotkey = U256::from(0);
        let amount_staked = TaoBalance::from(5000);
        let netuid = NetUid::from(1);
        let call = RuntimeCall::SubtensorModule(SubtensorCall::add_stake {
            hotkey,
            netuid,
            amount_staked,
        });
        let di = call.get_dispatch_info();
        assert_eq!(di.extension_weight, frame_support::weights::Weight::zero(),);
        assert_eq!(di.class, DispatchClass::Normal,);
        assert_eq!(di.pays_fee, Pays::Yes,);
    });
}

#[test]
fn test_add_stake_ok_no_emission() {
    new_test_ext(1).execute_with(|| {
        let hotkey_account_id = U256::from(533453);
        let coldkey_account_id = U256::from(55453);
        let amount = DefaultMinStake::<Test>::get().to_u64() * 10;

        //add network
        let netuid = add_dynamic_network(&hotkey_account_id, &coldkey_account_id);
        remove_owner_registration_stake(netuid);

        mock::setup_reserves(
            netuid,
            (amount * 1_000_000).into(),
            (amount * 10_000_000).into(),
        );

        // Give it some $$$ in his coldkey balance
        add_balance_to_coldkey_account(&coldkey_account_id, amount.into());

        // Check we have zero staked before transfer
        assert_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&hotkey_account_id),
            TaoBalance::ZERO
        );

        // Also total stake should be equal to the network initial lock
        assert_eq!(
            SubtensorModule::get_total_stake(),
            SubtensorModule::get_network_min_lock()
        );

        // Transfer to hotkey account, and check if the result is ok
        let (alpha_staked, fee) = mock::swap_tao_to_alpha(netuid, amount.into());
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
            netuid,
            amount.into()
        ));

        let (tao_expected, _) = mock::swap_alpha_to_tao(netuid, alpha_staked);
        let approx_fee = <Test as pallet::Config>::SwapInterface::approx_fee_amount(
            netuid.into(),
            TaoBalance::from(amount),
        );

        assert_abs_diff_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&hotkey_account_id),
            tao_expected + approx_fee, // swap returns value after fee, so we need to compensate it
            epsilon = 10000.into(),
        );

        // Check if stake has increased
        assert_abs_diff_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&hotkey_account_id),
            (amount - fee).into(),
            epsilon = 10000.into()
        );

        // Check if balance has decreased
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&coldkey_account_id),
            1.into()
        );

        // Check if total stake has increased accordingly.
        assert_eq!(
            SubtensorModule::get_total_stake(),
            SubtensorModule::get_network_min_lock() + amount.into()
        );
    });
}

#[test]
fn test_add_stake_err_signature() {
    new_test_ext(1).execute_with(|| {
        let hotkey_account_id = U256::from(654); // bogus
        let amount = 20000; // Not used
        let netuid = NetUid::from(1);

        assert_err!(
            SubtensorModule::add_stake(
                RawOrigin::None.into(),
                hotkey_account_id,
                netuid,
                amount.into()
            ),
            DispatchError::BadOrigin
        );
    });
}

#[test]
fn test_add_stake_not_registered_key_pair() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1);
        let subnet_owner_hotkey = U256::from(2);
        let coldkey_account_id = U256::from(435445);
        let hotkey_account_id = U256::from(54544);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let amount = DefaultMinStake::<Test>::get().to_u64() * 10;
        add_balance_to_coldkey_account(&coldkey_account_id, amount.into());
        assert_err!(
            SubtensorModule::add_stake(
                RuntimeOrigin::signed(coldkey_account_id),
                hotkey_account_id,
                netuid,
                amount.into()
            ),
            Error::<Test>::HotKeyAccountNotExists
        );
    });
}

#[test]
fn test_add_stake_ok_neuron_does_not_belong_to_coldkey() {
    new_test_ext(1).execute_with(|| {
        let coldkey_id = U256::from(544);
        let hotkey_id = U256::from(54544);
        let other_cold_key = U256::from(99498);
        let netuid = add_dynamic_network(&hotkey_id, &coldkey_id);
        let stake = DefaultMinStake::<Test>::get() * 10.into();

        // Give it some $$$ in his coldkey balance
        add_balance_to_coldkey_account(&other_cold_key, stake.into());

        // Perform the request which is signed by a different cold key
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(other_cold_key),
            hotkey_id,
            netuid,
            stake,
        ));
    });
}

#[test]
fn test_add_stake_err_not_enough_belance() {
    new_test_ext(1).execute_with(|| {
        let coldkey_id = U256::from(544);
        let hotkey_id = U256::from(54544);
        let stake = DefaultMinStake::<Test>::get() * 10.into();
        let netuid = add_dynamic_network(&hotkey_id, &coldkey_id);

        // Lets try to stake with 0 balance in cold key account
        assert!(SubtensorModule::get_coldkey_balance(&coldkey_id) < stake);
        assert_err!(
            SubtensorModule::add_stake(
                RuntimeOrigin::signed(coldkey_id),
                hotkey_id,
                netuid,
                stake,
            ),
            Error::<Test>::NotEnoughBalanceToStake
        );
    });
}

#[test]
#[ignore]
fn test_add_stake_total_issuance_no_change() {
    // When we add stake, the total issuance of the balances pallet should not change
    //    this is because the stake should be part of the coldkey account balance (reserved/locked)
    new_test_ext(1).execute_with(|| {
        let hotkey_account_id = U256::from(561337);
        let coldkey_account_id = U256::from(61337);
        let netuid = add_dynamic_network(&hotkey_account_id, &coldkey_account_id);

        // Give it some $$$ in his coldkey balance
        let initial_balance = 10000;
        add_balance_to_coldkey_account(&coldkey_account_id, initial_balance.into());

        // Check we have zero staked before transfer
        let initial_stake = SubtensorModule::get_total_stake_for_hotkey(&hotkey_account_id);
        assert_eq!(initial_stake, TaoBalance::ZERO);

        // Check total balance is equal to initial balance
        let initial_total_balance = Balances::total_balance(&coldkey_account_id);
        assert_eq!(initial_total_balance, initial_balance.into());

        // Check total issuance is equal to initial balance
        let initial_total_issuance = Balances::total_issuance();
        assert_eq!(initial_total_issuance, initial_balance.into());

        // Also total stake should be zero
        assert_eq!(SubtensorModule::get_total_stake(), TaoBalance::ZERO);

        // Stake to hotkey account, and check if the result is ok
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
            netuid,
            10000.into()
        ));

        // Check if stake has increased
        let new_stake = SubtensorModule::get_total_stake_for_hotkey(&hotkey_account_id);
        assert_eq!(new_stake, 10000.into());

        // Check if free balance has decreased
        let new_free_balance = SubtensorModule::get_coldkey_balance(&coldkey_account_id);
        assert_eq!(new_free_balance, 0.into());

        // Check if total stake has increased accordingly.
        assert_eq!(SubtensorModule::get_total_stake(), 10000.into());

        // Check if total issuance has remained the same. (no fee, includes reserved/locked balance)
        let total_issuance = Balances::total_issuance();
        assert_eq!(total_issuance, initial_total_issuance);
    });
}

#[test]
fn test_add_stake_partial_below_min_stake_fails() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1);
        let subnet_owner_hotkey = U256::from(2);
        let coldkey_account_id = U256::from(4343);
        let hotkey_account_id = U256::from(4968585);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        register_ok_neuron(netuid, hotkey_account_id, coldkey_account_id, 192213123);

        // Stake TAO amount is above min stake
        let min_stake = DefaultMinStake::<Test>::get();
        let amount = min_stake.to_u64() * 2;
        add_balance_to_coldkey_account(
            &coldkey_account_id,
            TaoBalance::from(amount) + ExistentialDeposit::get(),
        );

        // Setup reserves
        mock::setup_reserves(netuid, (amount * 10).into(), (amount * 10).into());

        // Force the swap to initialize
        <Test as pallet::Config>::SwapInterface::init_swap(netuid, None);

        // Get the current price
        let current_price =
            <Test as pallet::Config>::SwapInterface::current_alpha_price(netuid.into());
        assert!(current_price.to_num::<f64>() > 0.0);

        // Set "max spend" to ~1 TAO around current price
        let current_price_scaled = (current_price.to_num::<f64>() * 1_000_000_000_f64) as u64;
        let max_spend = current_price_scaled.saturating_add(1);

        // Add stake with partial flag on
        assert_err!(
            SubtensorModule::add_stake_limit(
                RuntimeOrigin::signed(coldkey_account_id),
                hotkey_account_id,
                netuid,
                amount.into(),
                max_spend.into(),
                true
            ),
            Error::<Test>::AmountTooLow
        );

        // Price should be unchanged on failure
        let new_current_price =
            <Test as pallet::Config>::SwapInterface::current_alpha_price(netuid.into());
        assert_eq!(new_current_price, current_price);
    });
}

#[test]
fn test_add_stake_insufficient_liquidity() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let amount_staked = DefaultMinStake::<Test>::get().to_u64() * 10;

        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey);
        add_balance_to_coldkey_account(&coldkey, amount_staked.into());

        // Set the liquidity at lowest possible value so that all staking requests fail
        let reserve = u64::from(mock::SwapMinimumReserve::get()) - 1;
        mock::setup_reserves(netuid, reserve.into(), reserve.into());

        // Check the error
        assert_noop!(
            SubtensorModule::add_stake(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid,
                amount_staked.into()
            ),
            Error::<Test>::InsufficientLiquidity
        );
    });
}

/// cargo test --package pallet-subtensor --lib -- tests::staking::add_stake::test_add_stake_input_reserve_too_low_fails --exact --show-output
#[test]
fn test_add_stake_input_reserve_too_low_fails() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let amount_staked = DefaultMinStake::<Test>::get().to_u64() * 10;

        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey);
        add_balance_to_coldkey_account(&coldkey, amount_staked.into());

        // Set the liquidity at lowest possible value so that all staking requests fail
        let reserve_alpha = 1_000_000_000_u64;
        let reserve_tao = u64::from(mock::SwapMinimumReserve::get()) - 1;
        mock::setup_reserves(netuid, reserve_tao.into(), reserve_alpha.into());

        // The output-side reserve is sufficient, but the input-side reserve is too small for the
        // requested swap under the 1000x input-reserve cap.
        assert_noop!(
            SubtensorModule::add_stake(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid,
                amount_staked.into()
            ),
            Error::<Test>::InsufficientLiquidity
        );
    });
}

/// cargo test --package pallet-subtensor --lib -- tests::staking::add_stake::test_add_stake_insufficient_liquidity_one_side_fail --exact --show-output
#[test]
fn test_add_stake_insufficient_liquidity_one_side_fail() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let amount_staked = DefaultMinStake::<Test>::get().to_u64() * 10;

        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey);
        add_balance_to_coldkey_account(&coldkey, amount_staked.into());

        // Set the liquidity at lowest possible value so that all staking requests fail
        let reserve_alpha = u64::from(mock::SwapMinimumReserve::get()) - 1;
        let reserve_tao = u64::from(mock::SwapMinimumReserve::get());
        mock::setup_reserves(netuid, reserve_tao.into(), reserve_alpha.into());

        // Check the error
        assert_noop!(
            SubtensorModule::add_stake(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid,
                amount_staked.into()
            ),
            Error::<Test>::InsufficientLiquidity
        );
    });
}

// /***********************************************************
// 	staking::increase_stake_for_hotkey_and_coldkey_on_subnet() tests
// ************************************************************/
#[test]
fn test_add_stake_to_hotkey_account_ok() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1);
        let subnet_owner_hotkey = U256::from(2);
        let hotkey_id = U256::from(5445);
        let coldkey_id = U256::from(5443433);
        let amount: u64 = 10_000;

        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        register_ok_neuron(netuid, hotkey_id, coldkey_id, 192213123);

        let base_total_stake = SubtensorModule::get_total_stake();

        // Check stake in ALPHA units for this hotkey/coldkey/netuid triple.
        let alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_id,
            &coldkey_id,
            netuid,
        );

        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_id,
            &coldkey_id,
            netuid,
            AlphaBalance::from(amount),
        );

        let alpha_after = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_id,
            &coldkey_id,
            netuid,
        );

        assert_eq!(
            alpha_after,
            alpha_before + AlphaBalance::from(amount),
            "Alpha stake did not increase by the expected amount"
        );

        // Total stake should never decrease when we increase stake.
        let total_stake_after = SubtensorModule::get_total_stake();
        assert!(
            total_stake_after >= base_total_stake,
            "Total stake unexpectedly decreased after increasing stake"
        );
    });
}

// Verify staking too low amount is impossible
#[test]
fn test_staking_too_little_fails() {
    new_test_ext(1).execute_with(|| {
        let hotkey_account_id = U256::from(533453);
        let coldkey_account_id = U256::from(55453);
        let amount = 10_000;

        //add network
        let netuid = add_dynamic_network(&hotkey_account_id, &coldkey_account_id);

        // Give it some $$$ in his coldkey balance
        add_balance_to_coldkey_account(&coldkey_account_id, amount.into());

        // Coldkey / hotkey 0 decreases take to 5%. This should fail as the minimum take is 9%
        assert_err!(
            SubtensorModule::add_stake(
                RuntimeOrigin::signed(coldkey_account_id),
                hotkey_account_id,
                netuid,
                1.into()
            ),
            Error::<Test>::AmountTooLow
        );
    });
}

// cargo test --package pallet-subtensor --lib -- tests::staking::add_stake::test_add_stake_fee_goes_to_subnet_tao --exact --show-output --nocapture
#[ignore = "fee now goes to liquidity provider"]
#[test]
fn test_add_stake_fee_goes_to_subnet_tao() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let existential_deposit = ExistentialDeposit::get();
        let tao_to_stake = DefaultMinStake::<Test>::get() * 10.into();

        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey);
        let subnet_tao_before = SubnetTAO::<Test>::get(netuid);

        // Add stake
        add_balance_to_coldkey_account(&coldkey, tao_to_stake);
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            tao_to_stake
        ));

        // Calculate expected stake
        let expected_alpha = AlphaBalance::from((tao_to_stake - existential_deposit).to_u64());
        let actual_alpha =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);
        let subnet_tao_after = SubnetTAO::<Test>::get(netuid);

        // Total subnet stake should match the sum of delegators' stakes minus existential deposits.
        assert_abs_diff_eq!(
            actual_alpha,
            expected_alpha,
            epsilon = expected_alpha / 1000.into()
        );

        // Subnet TAO should have increased by the full tao_to_stake amount
        assert_abs_diff_eq!(
            subnet_tao_before + tao_to_stake,
            subnet_tao_after,
            epsilon = 10.into()
        );
    });
}

#[test]
fn test_stake_overflow() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let coldkey_account_id = U256::from(435445);
        let hotkey_account_id = U256::from(54544);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        register_ok_neuron(netuid, hotkey_account_id, coldkey_account_id, 192213123);

        // Maximum possible: Max TAO supply less already-issued balance.
        let amount = 21_000_000_000_000_000_u64 - u64::from(Balances::total_issuance());

        // Give it some $$$ in his coldkey balance
        add_balance_to_coldkey_account(&coldkey_account_id, amount.into());

        // Setup liquidity with 21M TAO values
        mock::setup_reserves(netuid, amount.into(), amount.into());

        let total_stake_before = SubtensorModule::get_total_stake();

        // Stake and check if the result is ok
        let (expected_alpha, _) = mock::swap_tao_to_alpha(netuid, amount.into());
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
            netuid,
            amount.into()
        ));

        // Check if stake has increased properly
        assert_abs_diff_eq!(
            SubtensorModule::get_stake_for_hotkey_on_subnet(&hotkey_account_id, netuid),
            expected_alpha,
            epsilon = 1.into()
        );

        // Check if total stake has increased accordingly.
        assert_abs_diff_eq!(
            SubtensorModule::get_total_stake(),
            total_stake_before + amount.into(),
            epsilon = 1.into()
        );
    });
}

#[test]
// RUST_LOG=info cargo test --package pallet-subtensor --lib -- tests::staking::add_stake::test_add_stake_specific_stake_into_subnet_fail --exact --show-output
fn test_add_stake_specific_stake_into_subnet_fail() {
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

        let tao_staked = TaoBalance::from(200_000_000);

        //add network
        let netuid = add_dynamic_network(&sn_owner_coldkey, &sn_owner_coldkey);

        // Register hotkey on netuid
        register_ok_neuron(netuid, hotkey_account_id, hotkey_owner_account_id, 0);
        // Check we have zero staked
        assert_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&hotkey_account_id),
            TaoBalance::ZERO
        );

        // Set a hotkey pool for the hotkey
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
        add_balance_to_coldkey_account(&coldkey_account_id, tao_staked + 1_000_000_000.into());

        // Add stake as new hotkey
        let order = GetAlphaForTao::<Test>::with_amount(tao_staked);
        let expected_alpha = <Test as Config>::SwapInterface::swap(
            netuid.into(),
            order,
            <Test as Config>::SwapInterface::max_price(),
            false,
            true,
        )
        .map(|v| v.amount_paid_out)
        .unwrap_or_default();
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
            netuid,
            tao_staked,
        ));

        // Check we have non-zero staked
        assert!(expected_alpha > AlphaBalance::ZERO);
        assert_abs_diff_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey_account_id,
                &coldkey_account_id,
                netuid
            ),
            expected_alpha,
            epsilon = expected_alpha / 1000.into()
        );
    });
}

#[test]
fn test_stake_into_subnet_ok() {
    new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(1);
        let owner_coldkey = U256::from(2);
        let hotkey = U256::from(3);
        let coldkey = U256::from(4);
        let amount = 100_000_000;

        // add network
        let netuid = add_dynamic_network(&owner_hotkey, &owner_coldkey);

        // Forse-set alpha in and tao reserve to make price equal 0.01
        let tao_reserve = TaoBalance::from(100_000_000_000_u64);
        let alpha_in = AlphaBalance::from(1_000_000_000_000_u64);
        mock::setup_reserves(netuid, tao_reserve, alpha_in);
        let current_price =
            <Test as pallet::Config>::SwapInterface::current_alpha_price(netuid.into())
                .to_num::<f64>();

        // Initialize swap v3
        let order = GetAlphaForTao::<Test>::with_amount(0);
        assert_ok!(<tests::mock::Test as pallet::Config>::SwapInterface::swap(
            netuid.into(),
            order,
            TaoBalance::MAX,
            false,
            true
        ));

        // Add stake with slippage safety and check if the result is ok
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
        let fee_rate = pallet_subtensor_swap::FeeRate::<Test>::get(NetUid::from(netuid)) as f64
            / u16::MAX as f64;
        let expected_stake = (amount as f64) * (1. - fee_rate) / current_price;

        // Check if stake has increased
        assert_abs_diff_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid)
                .to_u64() as f64,
            expected_stake,
            epsilon = expected_stake / 1000.,
        );
    });
}

#[test]
fn test_stake_into_subnet_low_amount() {
    new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(1);
        let owner_coldkey = U256::from(2);
        let hotkey = U256::from(3);
        let coldkey = U256::from(4);
        let amount = 10;

        // add network
        let netuid = add_dynamic_network(&owner_hotkey, &owner_coldkey);

        // Forse-set alpha in and tao reserve to make price equal 0.1
        let tao_reserve = TaoBalance::from(100_000_000_000_u64);
        let alpha_in = AlphaBalance::from(1_000_000_000_000_u64);
        mock::setup_reserves(netuid, tao_reserve, alpha_in);
        let current_price =
            <Test as pallet::Config>::SwapInterface::current_alpha_price(netuid.into())
                .to_num::<f64>();

        // Initialize swap
        let order = GetAlphaForTao::<Test>::with_amount(0);
        assert_ok!(<tests::mock::Test as pallet::Config>::SwapInterface::swap(
            netuid.into(),
            order,
            TaoBalance::MAX,
            false,
            true
        ));

        // Add stake with slippage safety and check if the result is ok
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
        let expected_stake = (amount as f64) * 0.997 / current_price;

        // Check if stake has increased
        assert_abs_diff_eq!(
            u64::from(SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &coldkey, netuid
            )) as f64,
            expected_stake,
            epsilon = expected_stake / 100.
        );
    });
}

#[test]
fn test_stake_into_subnet_prohibitive_limit() {
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
        // Use prohibitive limit price
        assert_err!(
            SubtensorModule::add_stake_limit(
                RuntimeOrigin::signed(coldkey),
                owner_hotkey,
                netuid,
                amount.into(),
                TaoBalance::ZERO,
                true,
            ),
            DispatchError::from(pallet_subtensor_swap::Error::<Test>::PriceLimitExceeded)
        );

        // Check if stake has NOT increased
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &owner_hotkey,
                &coldkey,
                netuid
            ),
            AlphaBalance::ZERO
        );

        // Check if balance has NOT decreased
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&coldkey),
            amount.into()
        );
    });
}

#[test]
fn test_increase_stake_for_hotkey_and_coldkey_on_subnet_adds_to_staking_hotkeys_map() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let coldkey1 = U256::from(2);
        let hotkey = U256::from(3);

        let netuid = NetUid::from(1);
        let stake_amount = 100_000_000_000_u64;

        // Check no entry in the staking hotkeys map
        assert!(!StakingHotkeys::<Test>::contains_key(coldkey));
        // insert manually
        StakingHotkeys::<Test>::insert(coldkey, Vec::<U256>::new());
        // check entry has no hotkey
        assert!(!StakingHotkeys::<Test>::get(coldkey).contains(&hotkey));

        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            stake_amount.into(),
        );

        // Check entry exists in the staking hotkeys map
        assert!(StakingHotkeys::<Test>::contains_key(coldkey));
        // check entry has hotkey
        assert!(StakingHotkeys::<Test>::get(coldkey).contains(&hotkey));

        // Check no entry in the staking hotkeys map for coldkey1
        assert!(!StakingHotkeys::<Test>::contains_key(coldkey1));

        // Run increase stake for hotkey and coldkey1 on subnet
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey1,
            netuid,
            stake_amount.into(),
        );

        // Check entry exists in the staking hotkeys map for coldkey1
        assert!(StakingHotkeys::<Test>::contains_key(coldkey1));
        // check entry has hotkey
        assert!(StakingHotkeys::<Test>::get(coldkey1).contains(&hotkey));
    });
}

// cargo test --package pallet-subtensor --lib -- tests::staking::add_stake::test_add_root_updates_counters --exact --show-output
#[test]
fn test_add_root_updates_counters() {
    new_test_ext(0).execute_with(|| {
        let hotkey_account_id = U256::from(561337);
        let coldkey_account_id = U256::from(61337);
        add_network(NetUid::ROOT, 10, 0);
        assert_ok!(SubtensorModule::root_register(
            RuntimeOrigin::signed(coldkey_account_id).clone(),
            hotkey_account_id,
        ));
        let stake_amount = TaoBalance::from(1_000_000_000_u64);

        // Give it some $$$ in his coldkey balance
        let initial_balance = stake_amount + ExistentialDeposit::get();
        add_balance_to_coldkey_account(&coldkey_account_id, initial_balance);

        // Setup SubnetAlphaIn (because we are going to stake)
        SubnetAlphaIn::<Test>::insert(NetUid::ROOT, AlphaBalance::from(stake_amount.to_u64()));

        // Stake to hotkey account, and check if the result is ok
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
            NetUid::ROOT,
            stake_amount
        ));

        // Check if stake has increased
        let new_stake = SubtensorModule::get_total_stake_for_hotkey(&hotkey_account_id);
        assert_eq!(new_stake, stake_amount);

        // Check if total stake has increased accordingly.
        assert_eq!(SubtensorModule::get_total_stake(), stake_amount);

        // SubnetTAO updated
        assert_eq!(SubnetTAO::<Test>::get(NetUid::ROOT), stake_amount);

        // SubnetAlphaIn updated
        assert_eq!(SubnetAlphaIn::<Test>::get(NetUid::ROOT), 0.into());

        // SubnetAlphaOut updated
        assert_eq!(
            SubnetAlphaOut::<Test>::get(NetUid::ROOT),
            AlphaBalance::from(stake_amount.to_u64())
        );

        // SubnetVolume updated
        assert_eq!(
            SubnetVolume::<Test>::get(NetUid::ROOT),
            stake_amount.to_u64() as u128
        );
    });
}
