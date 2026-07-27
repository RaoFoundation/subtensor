#![allow(clippy::unwrap_used)]
#![allow(clippy::arithmetic_side_effects)]
//! Tests for [`crate::staking::helpers`] balances, ownership, nominations, and stake totals.

use approx::assert_abs_diff_eq;
use frame_support::{assert_err, assert_ok};
use sp_core::{Get, H256, U256};
use sp_runtime::PerU16;
use substrate_fixed::types::U96F32;
use subtensor_runtime_common::{AlphaBalance, NetUid, NetUidStorageIndex, TaoBalance, Token};
use subtensor_swap_interface::SwapHandler;

use super::super::mock;
use super::super::mock::*;
use crate::*;

#[test]
fn test_dividends_with_run_to_block() {
    new_test_ext(1).execute_with(|| {
        let neuron_src_hotkey_id = U256::from(1);
        let neuron_dest_hotkey_id = U256::from(2);
        let coldkey_account_id = U256::from(667);
        let hotkey_account_id = U256::from(668);
        let initial_stake: u64 = 5000;

        // add network
        let netuid = add_dynamic_network(&hotkey_account_id, &coldkey_account_id);
        Tempo::<Test>::insert(netuid, 13);

        // Register neuron(s)
        SubtensorModule::set_max_registrations_per_block(netuid, 3);
        SubtensorModule::set_max_allowed_uids(1.into(), 5);

        register_ok_neuron(netuid, neuron_src_hotkey_id, coldkey_account_id, 192213123);
        register_ok_neuron(netuid, neuron_dest_hotkey_id, coldkey_account_id, 12323);

        // Add some stake to src in ALPHA units.
        let src_alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &neuron_src_hotkey_id,
            &coldkey_account_id,
            netuid,
        );

        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &neuron_src_hotkey_id,
            &coldkey_account_id,
            netuid,
            AlphaBalance::from(initial_stake),
        );

        let src_alpha_after_add = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &neuron_src_hotkey_id,
            &coldkey_account_id,
            netuid,
        );

        assert_eq!(
            src_alpha_after_add,
            src_alpha_before + AlphaBalance::from(initial_stake),
            "Src alpha stake did not increase correctly"
        );

        // Check if all three neurons are registered (dynamic subnet owner + 2 registrations).
        assert_eq!(SubtensorModule::get_subnetwork_n(netuid), 3);

        // Run a couple of blocks (may change prices / emission, but shouldn't move stake away).
        run_to_block(2);

        // Re-check ALPHA stake (not TAO value).
        let src_alpha_after_blocks = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &neuron_src_hotkey_id,
            &coldkey_account_id,
            netuid,
        );
        let dest_alpha_after_blocks = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &neuron_dest_hotkey_id,
            &coldkey_account_id,
            netuid,
        );

        // Src stake should not decrease; dest stake should still be zero (no stake transfer/dividends).
        assert!(
            src_alpha_after_blocks >= src_alpha_after_add,
            "Src alpha stake unexpectedly decreased"
        );
        assert!(
            dest_alpha_after_blocks.is_zero(),
            "Dest alpha stake unexpectedly increased"
        );
    });
}

// cargo test --package pallet-subtensor --lib -- tests::staking::helpers::test_staking_sets_div_variables --exact --show-output --nocapture
#[test]
fn test_staking_sets_div_variables() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1);
        let subnet_owner_hotkey = U256::from(2);
        let hotkey_account_id = U256::from(581337);
        let coldkey_account_id = U256::from(81337);
        let amount = 100_000_000_000_u64;
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        remove_owner_registration_stake(netuid);
        let tempo = 10;
        Tempo::<Test>::insert(netuid, tempo);
        register_ok_neuron(netuid, hotkey_account_id, coldkey_account_id, 192213123);

        // Give it some $$$ in his coldkey balance
        add_balance_to_coldkey_account(&coldkey_account_id, amount.into());

        // Verify that divident variables are clear in the beginning
        assert_eq!(
            AlphaDividendsPerSubnet::<Test>::get(netuid, hotkey_account_id),
            AlphaBalance::ZERO
        );
        assert_eq!(
            TotalHotkeyAlphaLastEpoch::<Test>::get(hotkey_account_id, netuid),
            AlphaBalance::ZERO
        );

        // Stake to hotkey account, and check if the result is ok
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
            netuid,
            amount.into()
        ));

        // Verify that divident variables are still clear in the beginning
        assert_eq!(
            AlphaDividendsPerSubnet::<Test>::get(netuid, hotkey_account_id),
            AlphaBalance::ZERO
        );
        assert_eq!(
            TotalHotkeyAlphaLastEpoch::<Test>::get(hotkey_account_id, netuid),
            AlphaBalance::ZERO
        );

        // Wait for 1 epoch
        step_epochs(1, netuid);

        // Verify that divident variables have been set
        let stake = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &coldkey_account_id,
            netuid,
        );

        assert!(
            AlphaDividendsPerSubnet::<Test>::get(netuid, hotkey_account_id) > AlphaBalance::ZERO
        );
        assert_abs_diff_eq!(
            TotalHotkeyAlphaLastEpoch::<Test>::get(hotkey_account_id, netuid),
            stake,
            epsilon = stake / 100_000.into()
        );
    });
}

/***********************************************************
    staking::get_coldkey_balance() tests
************************************************************/
#[test]
fn test_get_coldkey_balance_no_balance() {
    new_test_ext(1).execute_with(|| {
        let coldkey_account_id = U256::from(5454); // arbitrary
        let result = SubtensorModule::get_coldkey_balance(&coldkey_account_id);

        // Arbitrary account should have 0 balance
        assert_eq!(result, 0.into());
    });
}

#[test]
fn test_get_coldkey_balance_with_balance() {
    new_test_ext(1).execute_with(|| {
        let coldkey_account_id = U256::from(5454); // arbitrary
        let amount = 1337;

        // Put the balance on the account
        add_balance_to_coldkey_account(&coldkey_account_id, amount.into());

        let result = SubtensorModule::get_coldkey_balance(&coldkey_account_id);

        // Arbitrary account should have 0 balance
        assert_eq!(result, amount.into());
    });
}

// /************************************************************
// 	staking::increase_total_stake() tests
// ************************************************************/
#[test]
fn test_increase_total_stake_ok() {
    new_test_ext(1).execute_with(|| {
        let increment = TaoBalance::from(10000);
        assert_eq!(SubtensorModule::get_total_stake(), TaoBalance::ZERO);
        SubtensorModule::increase_total_stake(increment);
        assert_eq!(SubtensorModule::get_total_stake(), increment);
    });
}

// /************************************************************
// 	staking::decrease_total_stake() tests
// ************************************************************/
#[test]
fn test_decrease_total_stake_ok() {
    new_test_ext(1).execute_with(|| {
        let initial_total_stake = TaoBalance::from(10000);
        let decrement = TaoBalance::from(5000);

        SubtensorModule::increase_total_stake(initial_total_stake);
        SubtensorModule::decrease_total_stake(decrement);

        // The total stake remaining should be the difference between the initial stake and the decrement
        assert_eq!(
            SubtensorModule::get_total_stake(),
            initial_total_stake - decrement
        );
    });
}

// /************************************************************
// 	staking::add_balance_to_coldkey_account() tests
// ************************************************************/
#[test]
fn test_add_balance_to_coldkey_account_ok() {
    new_test_ext(1).execute_with(|| {
        let coldkey_id = U256::from(4444322);
        let amount = 50000;
        add_balance_to_coldkey_account(&coldkey_id, amount.into());
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&coldkey_id),
            amount.into()
        );
    });
}

// /***********************************************************
// 	staking::remove_balance_from_coldkey_account() tests
// ************************************************************/
#[test]
fn test_remove_balance_from_coldkey_account_ok() {
    new_test_ext(1).execute_with(|| {
        let coldkey_account_id = U256::from(434324); // Random
        let amount = 10000; // Arbitrary
        let netuid = NetUid::from(1);
        // Put some $$ on the bank
        add_balance_to_coldkey_account(&coldkey_account_id, amount.into());
        NetworksAdded::<Test>::insert(netuid, true);
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&coldkey_account_id),
            amount.into()
        );
        // Should be able to withdraw without hassle
        let result =
            SubtensorModule::transfer_tao_to_subnet(netuid, &coldkey_account_id, amount.into());
        assert!(result.is_ok());
    });
}

#[test]
fn test_remove_balance_from_coldkey_account_failed() {
    new_test_ext(1).execute_with(|| {
        let coldkey_account_id = U256::from(434324); // Random
        let amount = 10000; // Arbitrary

        let netuid = NetUid::from(1);
        NetworksAdded::<Test>::insert(netuid, true);

        // Try to remove stake from the coldkey account. This should fail,
        // as there is no balance, nor does the account exist
        let result =
            SubtensorModule::transfer_tao_to_subnet(netuid, &coldkey_account_id, amount.into());
        assert_eq!(result, Err(Error::<Test>::InsufficientTaoBalance.into()));
    });
}

//************************************************************
// 	staking::hotkey_belongs_to_coldkey() tests
// ************************************************************/
#[test]
fn test_hotkey_belongs_to_coldkey_ok() {
    new_test_ext(1).execute_with(|| {
        let hotkey_id = U256::from(4434334);
        let coldkey_id = U256::from(34333);
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        let start_nonce: u64 = 0;
        add_network(netuid, tempo, 0);
        register_ok_neuron(netuid, hotkey_id, coldkey_id, start_nonce);
        assert_eq!(
            SubtensorModule::get_owning_coldkey_for_hotkey(&hotkey_id),
            coldkey_id
        );
    });
}

// /************************************************************
// 	staking::can_remove_balance_from_coldkey_account() tests
// ************************************************************/
#[test]
fn test_can_remove_balane_from_coldkey_account_ok() {
    new_test_ext(1).execute_with(|| {
        let coldkey_id = U256::from(87987984);
        let initial_amount = 10000;
        let remove_amount = 5000;
        add_balance_to_coldkey_account(&coldkey_id, initial_amount.into());
        assert!(SubtensorModule::can_remove_balance_from_coldkey_account(
            &coldkey_id,
            remove_amount.into()
        ));
    });
}

#[test]
fn test_can_remove_balance_from_coldkey_account_err_insufficient_balance() {
    new_test_ext(1).execute_with(|| {
        let coldkey_id = U256::from(87987984);
        let initial_amount = 10000;
        let remove_amount = 20000;
        add_balance_to_coldkey_account(&coldkey_id, initial_amount.into());
        assert!(!SubtensorModule::can_remove_balance_from_coldkey_account(
            &coldkey_id,
            remove_amount.into()
        ));
    });
}

/************************************************************
    staking::has_enough_stake() tests
************************************************************/
#[test]
fn test_has_enough_stake_yes() {
    new_test_ext(1).execute_with(|| {
        let hotkey_id = U256::from(4334);
        let coldkey_id = U256::from(87989);
        let intial_amount = 10_000;
        let netuid = NetUid::from(add_dynamic_network(&hotkey_id, &coldkey_id));
        remove_owner_registration_stake(netuid);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_id,
            &coldkey_id,
            netuid,
            intial_amount.into(),
        );

        assert_abs_diff_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&hotkey_id),
            intial_amount.into(),
            epsilon = 2.into()
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey_id,
                &coldkey_id,
                netuid
            ),
            intial_amount.into()
        );
        assert_ok!(SubtensorModule::calculate_reduced_stake_on_subnet(
            &hotkey_id,
            &coldkey_id,
            netuid,
            (intial_amount / 2).into()
        ));
    });
}

#[test]
fn test_has_enough_stake_no() {
    new_test_ext(1).execute_with(|| {
        let hotkey_id = U256::from(4334);
        let coldkey_id = U256::from(87989);
        let intial_amount = 10_000;
        let netuid = add_dynamic_network(&hotkey_id, &coldkey_id);
        remove_owner_registration_stake(netuid);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_id,
            &coldkey_id,
            netuid,
            intial_amount.into(),
        );

        assert_abs_diff_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&hotkey_id),
            intial_amount.into(),
            epsilon = 2.into()
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey_id,
                &coldkey_id,
                netuid
            ),
            intial_amount.into()
        );
        assert_err!(
            SubtensorModule::calculate_reduced_stake_on_subnet(
                &hotkey_id,
                &coldkey_id,
                netuid,
                (intial_amount * 2).into()
            ),
            Error::<Test>::NotEnoughStakeToWithdraw
        );
    });
}

#[test]
fn test_has_enough_stake_no_for_zero() {
    new_test_ext(1).execute_with(|| {
        let hotkey_id = U256::from(4334);
        let coldkey_id = U256::from(87989);
        let intial_amount = 0;
        let netuid = add_dynamic_network(&hotkey_id, &coldkey_id);
        remove_owner_registration_stake(netuid);

        assert_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&hotkey_id),
            intial_amount.into()
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey_id,
                &coldkey_id,
                netuid
            ),
            intial_amount.into()
        );
        assert_err!(
            SubtensorModule::calculate_reduced_stake_on_subnet(
                &hotkey_id,
                &coldkey_id,
                netuid,
                1_000.into()
            ),
            Error::<Test>::NotEnoughStakeToWithdraw
        );
    });
}

#[test]
fn test_non_existent_account() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &U256::from(0),
            &(U256::from(0)),
            netuid,
            10.into(),
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &U256::from(0),
                &U256::from(0),
                netuid
            ),
            10.into()
        );
        // No subnets => no iteration => zero total stake
        assert_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&(U256::from(0))),
            TaoBalance::ZERO
        );
    });
}

/************************************************************
    staking::delegating
************************************************************/

#[test]
fn test_faucet_ok() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(123560);

        log::info!("Creating work for submission to faucet...");

        let block_number = SubtensorModule::get_current_block_as_u64();
        let difficulty: U256 = U256::from(10_000_000);
        let mut nonce: u64 = 0;
        let mut work: H256 = SubtensorModule::create_seal_hash(block_number, nonce, &coldkey);
        while !SubtensorModule::hash_meets_difficulty(&work, difficulty) {
            nonce += 1;
            work = SubtensorModule::create_seal_hash(block_number, nonce, &coldkey);
        }
        let vec_work: Vec<u8> = SubtensorModule::hash_to_vec(work);

        log::info!("Faucet state: {}", cfg!(feature = "pow-faucet"));

        #[cfg(feature = "pow-faucet")]
        assert_ok!(SubtensorModule::do_faucet(
            RuntimeOrigin::signed(coldkey),
            block_number,
            nonce,
            vec_work
        ));

        #[cfg(not(feature = "pow-faucet"))]
        assert_ok!(SubtensorModule::do_faucet(
            RuntimeOrigin::signed(coldkey),
            block_number,
            nonce,
            vec_work
        ));
    });
}

/// This test ensures that the clear_small_nominations function works as expected.
/// It creates a network with two hotkeys and two coldkeys, and then registers a nominator account for each hotkey.
/// When we call set_nominator_min_required_stake, it should clear all small nominations that are below the minimum required stake.
///
/// cargo test --package pallet-subtensor --lib -- tests::staking::helpers::test_clear_small_nominations --exact --show-output
#[test]
fn test_clear_small_nominations() {
    new_test_ext(0).execute_with(|| {
        // Create subnet and accounts.
        let subnet_owner_coldkey = U256::from(10);
        let subnet_owner_hotkey = U256::from(20);
        let hot1 = U256::from(1);
        let hot2 = U256::from(2);
        let cold1 = U256::from(3);
        let cold2 = U256::from(4);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let amount = DefaultMinStake::<Test>::get() * 10.into();
        let fee = DefaultMinStake::<Test>::get();
        let init_balance = amount + fee + ExistentialDeposit::get();

        // Set fee rate to 0 so that alpha fee is not moved to block producer
        pallet_subtensor_swap::FeeRate::<Test>::insert(netuid, 0);

        // Register hot1.
        register_ok_neuron(netuid, hot1, cold1, 0);
        Delegates::<Test>::insert(
            hot1,
            PerU16::from_parts(SubtensorModule::get_min_delegate_take()),
        );
        assert_eq!(SubtensorModule::get_owning_coldkey_for_hotkey(&hot1), cold1);

        // Register hot2.
        register_ok_neuron(netuid, hot2, cold2, 0);
        Delegates::<Test>::insert(
            hot2,
            PerU16::from_parts(SubtensorModule::get_min_delegate_take()),
        );
        assert_eq!(SubtensorModule::get_owning_coldkey_for_hotkey(&hot2), cold2);

        // Add stake cold1 --> hot1 (non delegation.)
        add_balance_to_coldkey_account(&cold1, init_balance);
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(cold1),
            hot1,
            netuid,
            amount.into()
        ));
        let alpha_stake1 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hot1, &cold1, netuid);
        let unstake_amount1 = AlphaBalance::from(alpha_stake1.to_u64() * 997 / 1000);
        let small1 = alpha_stake1 - unstake_amount1;
        assert_ok!(SubtensorModule::remove_stake(
            RuntimeOrigin::signed(cold1),
            hot1,
            netuid,
            unstake_amount1
        ));
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hot1, &cold1, netuid),
            small1
        );

        // Add stake cold2 --> hot1 (is delegation.)
        add_balance_to_coldkey_account(&cold2, init_balance);
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(cold2),
            hot1,
            netuid,
            amount.into()
        ));
        let alpha_stake2 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hot1, &cold2, netuid);
        let unstake_amount2 = AlphaBalance::from(alpha_stake2.to_u64() * 997 / 1000);
        let small2 = alpha_stake2 - unstake_amount2;
        assert_ok!(SubtensorModule::remove_stake(
            RuntimeOrigin::signed(cold2),
            hot1,
            netuid,
            unstake_amount2
        ));
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hot1, &cold2, netuid),
            small2
        );

        let balance1_before_cleaning = Balances::free_balance(cold1);
        let balance2_before_cleaning = Balances::free_balance(cold2);

        // Run clear all small nominations when min stake is zero (noop)
        SubtensorModule::set_nominator_min_required_stake(0);
        assert_eq!(SubtensorModule::get_nominator_min_required_stake(), 0);
        SubtensorModule::clear_small_nominations();
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hot1, &cold1, netuid),
            small1
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hot1, &cold2, netuid),
            small2
        );

        // Set min nomination to above small1 and small2
        let total_hot1_stake_before = TotalHotkeyAlpha::<Test>::get(hot1, netuid);
        let total_stake_before = TotalStake::<Test>::get();
        SubtensorModule::set_nominator_min_required_stake(
            (small1.to_u64().min(small2.to_u64()) * 2).into(),
        );

        // Run clear all small nominations (removes delegations under 10)
        SubtensorModule::clear_small_nominations();
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hot1, &cold1, netuid),
            small1
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hot1, &cold2, netuid),
            AlphaBalance::ZERO
        );

        // Balances have been added back into accounts.
        let balance1_after_cleaning = Balances::free_balance(cold1);
        let balance2_after_cleaning = Balances::free_balance(cold2);
        assert_eq!(balance1_before_cleaning, balance1_after_cleaning);
        assert!(balance2_before_cleaning < balance2_after_cleaning);

        assert_abs_diff_eq!(
            TotalHotkeyAlpha::<Test>::get(hot1, netuid),
            total_hot1_stake_before - small2,
            epsilon = 1.into()
        );
        assert!(TotalStake::<Test>::get() < total_stake_before);
    });
}

// cargo test --package pallet-subtensor --lib -- tests::staking::helpers::test_get_total_delegated_stake_after_unstaking --exact --show-output
#[test]
fn test_get_total_delegated_stake_after_unstaking() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let delegate_coldkey = U256::from(1);
        let delegate_hotkey = U256::from(2);
        let delegator = U256::from(3);
        let initial_stake = DefaultMinStake::<Test>::get().to_u64() * 10;
        let existential_deposit = ExistentialDeposit::get();
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        register_ok_neuron(netuid, delegate_hotkey, delegate_coldkey, 0);

        // Add balance to delegator
        add_balance_to_coldkey_account(&delegator, initial_stake.into());

        // Delegate stake
        let (_, fee) = mock::swap_tao_to_alpha(netuid, initial_stake.into());
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(delegator),
            delegate_hotkey,
            netuid,
            initial_stake.into()
        ));

        // Check initial delegated stake
        assert_abs_diff_eq!(
            SubtensorModule::get_total_stake_for_coldkey(&delegator),
            (initial_stake - u64::from(existential_deposit) - fee).into(),
            epsilon = TaoBalance::from(initial_stake / 100),
        );
        assert_abs_diff_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&delegate_hotkey),
            (initial_stake - u64::from(existential_deposit) - fee).into(),
            epsilon = TaoBalance::from(initial_stake / 100),
        );
        let delegated_alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &delegate_hotkey,
            &delegator,
            netuid,
        );
        // Unstake part of the delegation
        let unstake_amount_alpha = delegated_alpha / 2.into();
        assert_ok!(SubtensorModule::remove_stake(
            RuntimeOrigin::signed(delegator),
            delegate_hotkey,
            netuid,
            unstake_amount_alpha.into()
        ));
        let current_price = U96F32::from_num(
            <Test as pallet::Config>::SwapInterface::current_alpha_price(netuid.into()),
        );

        // Calculate the expected delegated stake
        let unstake_amount =
            (current_price * U96F32::from_num(unstake_amount_alpha)).to_num::<u64>();
        let expected_delegated_stake: u64 =
            initial_stake - unstake_amount - u64::from(existential_deposit) - fee;

        // Debug prints
        log::debug!("Initial stake: {initial_stake}");
        log::debug!("Unstake amount: {unstake_amount}");
        log::debug!("Existential deposit: {existential_deposit}");
        log::debug!("Expected delegated stake: {expected_delegated_stake}");
        log::debug!(
            "Actual delegated stake: {}",
            SubtensorModule::get_total_stake_for_coldkey(&delegate_coldkey)
        );

        // Check the total delegated stake after unstaking
        assert_abs_diff_eq!(
            SubtensorModule::get_total_stake_for_coldkey(&delegator),
            expected_delegated_stake.into(),
            epsilon = TaoBalance::from(expected_delegated_stake / 1000),
        );
        assert_abs_diff_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&delegate_hotkey),
            expected_delegated_stake.into(),
            epsilon = TaoBalance::from(expected_delegated_stake / 1000),
        );
    });
}

#[test]
fn test_get_total_delegated_stake_no_delegations() {
    new_test_ext(1).execute_with(|| {
        let delegate = U256::from(1);
        let coldkey = U256::from(2);
        let netuid = NetUid::from(1u16);

        add_network(netuid, 1, 0);
        register_ok_neuron(netuid, delegate, coldkey, 0);

        // Check that there's no delegated stake
        assert_eq!(
            SubtensorModule::get_total_stake_for_coldkey(&delegate),
            TaoBalance::ZERO
        );
    });
}

#[test]
fn test_get_total_delegated_stake_single_delegator() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let delegate_coldkey = U256::from(1);
        let delegate_hotkey = U256::from(2);
        let delegator = U256::from(3);
        let stake_amount = DefaultMinStake::<Test>::get().to_u64() * 10 - 1;
        let existential_deposit = ExistentialDeposit::get();
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        register_ok_neuron(netuid, delegate_hotkey, delegate_coldkey, 0);

        // Add stake from delegator
        add_balance_to_coldkey_account(&delegator, stake_amount.into());

        let (_, fee) = mock::swap_tao_to_alpha(netuid, stake_amount.into());

        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(delegator),
            delegate_hotkey,
            netuid,
            stake_amount.into()
        ));

        // Debug prints
        log::debug!("Delegate coldkey: {delegate_coldkey:?}");
        log::debug!("Delegate hotkey: {delegate_hotkey:?}");
        log::debug!("Delegator: {delegator:?}");
        log::debug!("Stake amount: {stake_amount}");
        log::debug!("Existential deposit: {existential_deposit}");
        log::debug!(
            "Total stake for hotkey: {}",
            SubtensorModule::get_total_stake_for_hotkey(&delegate_hotkey)
        );
        log::debug!(
            "Delegated stake for coldkey: {}",
            SubtensorModule::get_total_stake_for_coldkey(&delegate_coldkey)
        );

        // Calculate expected delegated stake
        let expected_delegated_stake = stake_amount - u64::from(existential_deposit) - fee;
        let actual_delegated_stake = SubtensorModule::get_total_stake_for_hotkey(&delegate_hotkey);
        let actual_delegator_stake = SubtensorModule::get_total_stake_for_coldkey(&delegator);

        assert_abs_diff_eq!(
            actual_delegated_stake,
            expected_delegated_stake.into(),
            epsilon = TaoBalance::from(expected_delegated_stake / 100),
        );
        assert_abs_diff_eq!(
            actual_delegator_stake,
            expected_delegated_stake.into(),
            epsilon = TaoBalance::from(expected_delegated_stake / 100),
        );
    });
}

#[test]
fn test_get_alpha_share_stake_multiple_delegators() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let hotkey1 = U256::from(2);
        let hotkey2 = U256::from(20);
        let coldkey1 = U256::from(3);
        let coldkey2 = U256::from(4);
        let existential_deposit = TaoBalance::from(2);
        let stake1 = DefaultMinStake::<Test>::get() * 10.into();
        let stake2 = DefaultMinStake::<Test>::get() * 10.into() - 1.into();

        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        register_ok_neuron(netuid, hotkey1, coldkey1, 0);
        register_ok_neuron(netuid, hotkey2, coldkey2, 0);

        // Add stake from delegator1
        add_balance_to_coldkey_account(&coldkey1, stake1 + existential_deposit);
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey1),
            hotkey1,
            netuid,
            stake1
        ));

        // Add stake from delegator2
        add_balance_to_coldkey_account(&coldkey2, stake2 + existential_deposit);
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey2),
            hotkey2,
            netuid,
            stake2
        ));

        // Calculate expected total delegated stake
        let alpha1 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey1, &coldkey1, netuid,
        );
        let alpha2 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey2, &coldkey2, netuid,
        );
        let expected_total_stake = alpha1 + alpha2;
        let actual_total_stake = SubtensorModule::get_alpha_share_pool(hotkey1, netuid)
            .get_value(&coldkey1)
            + SubtensorModule::get_alpha_share_pool(hotkey2, netuid).get_value(&coldkey2);

        // Total subnet stake should match the sum of delegators' stakes minus existential deposits.
        assert_abs_diff_eq!(
            AlphaBalance::from(actual_total_stake),
            expected_total_stake,
            epsilon = expected_total_stake / 1000.into()
        );
    });
}

#[test]
fn test_get_total_delegated_stake_exclude_owner_stake() {
    new_test_ext(1).execute_with(|| {
        let delegate_coldkey = U256::from(1);
        let delegate_hotkey = U256::from(2);
        let delegator = U256::from(3);
        let owner_stake = DefaultMinStake::<Test>::get().to_u64() * 10;
        let delegator_stake = DefaultMinStake::<Test>::get().to_u64() * 10 - 1;

        let netuid = add_dynamic_network(&delegate_hotkey, &delegate_coldkey);
        remove_owner_registration_stake(netuid);

        // Add owner stake
        add_balance_to_coldkey_account(&delegate_coldkey, owner_stake.into());
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(delegate_coldkey),
            delegate_hotkey,
            netuid,
            owner_stake.into()
        ));

        // Add delegator stake
        add_balance_to_coldkey_account(&delegator, delegator_stake.into());
        let (_, fee) = mock::swap_tao_to_alpha(netuid, delegator_stake.into());
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(delegator),
            delegate_hotkey,
            netuid,
            delegator_stake.into()
        ));

        // Check the total delegated stake (should exclude owner's stake)
        let expected_delegated_stake = delegator_stake - fee;
        let actual_delegated_stake =
            SubtensorModule::get_total_stake_for_coldkey(&delegate_coldkey);

        assert_abs_diff_eq!(
            actual_delegated_stake,
            expected_delegated_stake.into(),
            epsilon = TaoBalance::from(expected_delegated_stake / 100)
        );
    });
}

/// Test that emission is distributed correctly between one validator, one
/// vali-miner, and one miner
#[test]
fn test_mining_emission_distribution_validator_valiminer_miner() {
    new_test_ext(1).execute_with(|| {
        let validator_coldkey = U256::from(1);
        let validator_hotkey = U256::from(2);
        let validator_miner_coldkey = U256::from(3);
        let validator_miner_hotkey = U256::from(4);
        let miner_coldkey = U256::from(5);
        let miner_hotkey = U256::from(6);
        let netuid = NetUid::from(1);
        let subnet_tempo = 10;
        let stake = TaoBalance::from(100_000_000_000_u64);

        // Add network, register hotkeys, and setup network parameters
        add_network(netuid, subnet_tempo, 0);
        register_ok_neuron(netuid, validator_hotkey, validator_coldkey, 0);
        register_ok_neuron(netuid, validator_miner_hotkey, validator_miner_coldkey, 1);
        register_ok_neuron(netuid, miner_hotkey, miner_coldkey, 2);
        add_balance_to_coldkey_account(&validator_coldkey, stake + ExistentialDeposit::get());
        add_balance_to_coldkey_account(&validator_miner_coldkey, stake + ExistentialDeposit::get());
        add_balance_to_coldkey_account(&miner_coldkey, stake + ExistentialDeposit::get());
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);
        step_block(subnet_tempo);
        SubnetOwnerCut::<Test>::set(0);
        // There are two validators and three neurons
        MaxAllowedUids::<Test>::set(netuid, 3);
        SubtensorModule::set_max_allowed_validators(netuid, 2);

        // Setup stakes:
        //   Stake from validator
        //   Stake from valiminer
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(validator_coldkey),
            validator_hotkey,
            netuid,
            stake.into()
        ));
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(validator_miner_coldkey),
            validator_miner_hotkey,
            netuid,
            stake.into()
        ));

        // Setup YUMA so that it creates emissions
        Weights::<Test>::insert(NetUidStorageIndex::from(netuid), 0, vec![(1, 0xFFFF)]);
        Weights::<Test>::insert(NetUidStorageIndex::from(netuid), 1, vec![(2, 0xFFFF)]);
        BlockAtRegistration::<Test>::set(netuid, 0, 1);
        BlockAtRegistration::<Test>::set(netuid, 1, 1);
        BlockAtRegistration::<Test>::set(netuid, 2, 1);
        LastUpdate::<Test>::set(NetUidStorageIndex::from(netuid), vec![2, 2, 2]);
        Kappa::<Test>::set(netuid, u16::MAX / 5);
        ActivityCutoff::<Test>::set(netuid, u16::MAX); // makes all stake active
        ValidatorPermit::<Test>::insert(netuid, vec![true, true, false]);

        // Run run_coinbase until emissions are drained
        let validator_stake_before =
            SubtensorModule::get_total_stake_for_coldkey(&validator_coldkey);
        let valiminer_stake_before =
            SubtensorModule::get_total_stake_for_coldkey(&validator_miner_coldkey);
        let miner_stake_before = SubtensorModule::get_total_stake_for_coldkey(&miner_coldkey);

        step_block(subnet_tempo);

        // Verify how emission is split between keys
        //   - Owner cut is zero => 50% goes to miners and 50% goes to validators
        //   - Validator gets 25% because there are two validators
        //   - Valiminer gets 25% as a validator and 25% as miner
        //   - Miner gets 25% as miner
        let validator_emission = SubtensorModule::get_total_stake_for_coldkey(&validator_coldkey)
            - validator_stake_before;
        let valiminer_emission =
            SubtensorModule::get_total_stake_for_coldkey(&validator_miner_coldkey)
                - valiminer_stake_before;
        let miner_emission =
            SubtensorModule::get_total_stake_for_coldkey(&miner_coldkey) - miner_stake_before;
        let total_emission = validator_emission + valiminer_emission + miner_emission;

        assert_abs_diff_eq!(
            validator_emission,
            total_emission / 4.into(),
            epsilon = 10.into()
        );
        assert_abs_diff_eq!(
            valiminer_emission,
            total_emission / 2.into(),
            epsilon = 10.into()
        );
        assert_abs_diff_eq!(
            miner_emission,
            total_emission / 4.into(),
            epsilon = 10.into()
        );
    });
}

/// This test verifies that minimum stake amount is sufficient to move price and apply
/// non-zero staking fees
#[test]
fn test_default_min_stake_sufficiency() {
    new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(1);
        let owner_coldkey = U256::from(2);
        let coldkey = U256::from(4);
        let min_tao_stake = DefaultMinStake::<Test>::get() * 2.into();
        let amount = min_tao_stake;
        let owner_balance_before = amount * 10.into();
        let user_balance_before = amount * 100.into();

        // add network
        let netuid = add_dynamic_network(&owner_hotkey, &owner_coldkey);
        add_balance_to_coldkey_account(&owner_coldkey, owner_balance_before);
        add_balance_to_coldkey_account(&coldkey, user_balance_before);
        let fee_rate = pallet_subtensor_swap::FeeRate::<Test>::get(NetUid::from(netuid)) as f64
            / u16::MAX as f64;

        // Set some extreme, but realistic TAO and Alpha reserves to minimize slippage
        // 1% of TAO max supply
        // 0.01 Alpha price
        let tao_reserve = TaoBalance::from(210_000_000_000_000_u64);
        let alpha_in = AlphaBalance::from(21_000_000_000_000_000_u64);
        mock::setup_reserves(netuid, tao_reserve, alpha_in);
        let current_price_before =
            <Test as pallet::Config>::SwapInterface::current_alpha_price(netuid.into());

        // Stake and unstake
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey),
            owner_hotkey,
            netuid,
            amount.into(),
        ));
        let fee_stake = (fee_rate * u64::from(amount) as f64) as u64;
        let current_price_after_stake =
            <Test as pallet::Config>::SwapInterface::current_alpha_price(netuid.into());
        let user_alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &owner_hotkey,
            &coldkey,
            netuid,
        );
        assert_ok!(SubtensorModule::remove_stake(
            RuntimeOrigin::signed(coldkey),
            owner_hotkey,
            netuid,
            user_alpha,
        ));
        let fee_unstake = (fee_rate * user_alpha.to_u64() as f64) as u64;
        let current_price_after_unstake =
            <Test as pallet::Config>::SwapInterface::current_alpha_price(netuid.into());

        assert!(fee_stake > 0);
        assert!(fee_unstake > 0);
        assert!(current_price_after_stake > current_price_before);
        assert!(current_price_after_stake > current_price_after_unstake);
    });
}

// cargo test --package pallet-subtensor --lib -- tests::staking::helpers::test_staking_records_flow --exact --show-output
#[test]
fn test_staking_records_flow() {
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

        // Initialize swap v3
        SubtensorModule::swap_tao_for_alpha(
            netuid,
            TaoBalance::ZERO,
            1_000_000_000_000_u64.into(),
            false,
        )
        .unwrap();

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
        let expected_flow = (amount as f64) * (1. - fee_rate);

        // Check that flow has been recorded (less unstaking fees)
        assert_abs_diff_eq!(
            SubnetTaoFlow::<Test>::get(netuid),
            expected_flow as i64,
            epsilon = 1_i64
        );

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

        // Check that outflow has been recorded (less unstaking fees)
        // The block builder will receive a fraction of the fees in alpha and will be forced
        // to unstake it. So, the additional out-flow is recorded for this.
        let unstaked_block_builder_fraction = 1.;
        let expected_unstake_fee =
            expected_flow * fee_rate * (1. - unstaked_block_builder_fraction);
        assert_abs_diff_eq!(
            SubnetTaoFlow::<Test>::get(netuid),
            expected_unstake_fee as i64,
            epsilon = ((expected_unstake_fee / 100.0) as i64).max(1)
        );
    });
}
