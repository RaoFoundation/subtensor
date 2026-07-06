#![allow(clippy::expect_used, clippy::unwrap_used)]
use super::mock::*;
use crate::subnets::symbols::{DEFAULT_SYMBOL, SYMBOLS};
use crate::*;
use frame_support::{assert_err, assert_noop, assert_ok};
use frame_system::Config;
use sp_core::U256;
use std::collections::BTreeSet;
use subtensor_runtime_common::{AlphaBalance, TaoBalance};

use super::mock;

/***************************
  pub fn do_start_call() tests
*****************************/

#[test]
fn test_do_start_call_ok() {
    new_test_ext(0).execute_with(|| {
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        let coldkey_account_id = U256::from(0);

        add_network_without_emission_block(netuid, tempo, 0);
        assert_eq!(FirstEmissionBlockNumber::<Test>::get(netuid), None);
        mock::setup_reserves(netuid, 1_000_000_000.into(), 1_000_000_000.into());

        // account 0 is the default owner for any subnet
        assert_eq!(SubnetOwner::<Test>::get(netuid), coldkey_account_id);

        let block_number = System::block_number() + StartCallDelay::<Test>::get();
        System::set_block_number(block_number);

        assert_ok!(SubtensorModule::start_call(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey_account_id),
            netuid
        ));

        assert_eq!(
            FirstEmissionBlockNumber::<Test>::get(netuid),
            Some(block_number + 1)
        );
    });
}

#[test]
fn test_do_start_call_fail_with_not_existed_subnet() {
    new_test_ext(0).execute_with(|| {
        let netuid = NetUid::from(1);
        let coldkey_account_id = U256::from(0);
        assert_noop!(
            SubtensorModule::start_call(
                <<Test as Config>::RuntimeOrigin>::signed(coldkey_account_id),
                netuid
            ),
            Error::<Test>::SubnetNotExists
        );
    });
}

#[test]
fn test_do_start_call_fail_not_owner() {
    new_test_ext(0).execute_with(|| {
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        let coldkey_account_id = U256::from(0);
        let wrong_owner_account_id = U256::from(2);
        let burn_cost = TaoBalance::from(1000);
        //add network
        SubtensorModule::set_burn(netuid, burn_cost);
        add_network_without_emission_block(netuid, tempo, 0);
        mock::setup_reserves(netuid, 1_000_000_000.into(), 1_000_000_000.into());
        // Give it some $$$ in his coldkey balance
        add_balance_to_coldkey_account(&coldkey_account_id, 10000.into());

        add_network_without_emission_block(netuid, tempo, 0);

        assert_eq!(SubnetOwner::<Test>::get(netuid), coldkey_account_id);

        System::set_block_number(System::block_number() + StartCallDelay::<Test>::get());

        assert_noop!(
            SubtensorModule::start_call(
                <<Test as Config>::RuntimeOrigin>::signed(wrong_owner_account_id),
                netuid
            ),
            DispatchError::BadOrigin
        );
    });
}

#[test]
fn test_do_start_call_can_start_now() {
    new_test_ext(0).execute_with(|| {
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        let coldkey_account_id = U256::from(0);
        let burn_cost = TaoBalance::from(1000);
        //add network
        SubtensorModule::set_burn(netuid, burn_cost);
        add_network_without_emission_block(netuid, tempo, 0);
        mock::setup_reserves(netuid, 1_000_000_000.into(), 1_000_000_000.into());
        // Give it some $$$ in his coldkey balance
        add_balance_to_coldkey_account(&coldkey_account_id, 10000.into());

        add_network_without_emission_block(netuid, tempo, 0);

        assert_eq!(SubnetOwner::<Test>::get(netuid), coldkey_account_id);

        assert_ok!(SubtensorModule::start_call(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey_account_id),
            netuid
        ));
    });
}

#[test]
fn test_do_start_call_fail_for_set_again() {
    new_test_ext(0).execute_with(|| {
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        let coldkey_account_id = U256::from(0);
        let hotkey_account_id = U256::from(1);
        let burn_cost = TaoBalance::from(1000);

        // Create the network first. Network init helpers may overwrite defaults.
        add_network_without_emission_block(netuid, tempo, 0);
        assert_eq!(FirstEmissionBlockNumber::<Test>::get(netuid), None);
        mock::setup_reserves(netuid, 1_000_000_000.into(), 1_000_000_000.into());

        // Set burn AFTER network creation so it is not overwritten.
        SubtensorModule::set_burn(netuid, burn_cost);

        // Fund coldkey based on the actual burn.
        let burn_u64 = SubtensorModule::get_burn(netuid);
        add_balance_to_coldkey_account(
            &coldkey_account_id,
            burn_u64
                .saturating_add(ExistentialDeposit::get())
                .saturating_add(10_000.into()),
        );

        // Subscribe and check extrinsic output
        assert_ok!(SubtensorModule::burned_register(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey_account_id),
            netuid,
            hotkey_account_id
        ));

        assert_eq!(SubnetOwner::<Test>::get(netuid), coldkey_account_id);

        let block_number = System::block_number() + StartCallDelay::<Test>::get();
        System::set_block_number(block_number);

        assert_ok!(SubtensorModule::start_call(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey_account_id),
            netuid
        ));

        assert_noop!(
            SubtensorModule::start_call(
                <<Test as Config>::RuntimeOrigin>::signed(coldkey_account_id),
                netuid
            ),
            Error::<Test>::FirstEmissionBlockNumberAlreadySet
        );
    });
}

#[test]
fn test_do_start_call_ok_with_same_block_number_after_coinbase() {
    new_test_ext(0).execute_with(|| {
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        let coldkey_account_id = U256::from(0);

        add_network_without_emission_block(netuid, tempo, 0);
        assert_eq!(FirstEmissionBlockNumber::<Test>::get(netuid), None);
        mock::setup_reserves(netuid, 1_000_000_000.into(), 1_000_000_000.into());

        assert_eq!(SubnetOwner::<Test>::get(netuid), coldkey_account_id);

        let block_number = System::block_number() + StartCallDelay::<Test>::get();
        System::set_block_number(block_number);

        assert_ok!(SubtensorModule::start_call(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey_account_id),
            netuid
        ));

        assert_eq!(
            FirstEmissionBlockNumber::<Test>::get(netuid),
            Some(block_number + 1)
        );

        step_block(tempo);
        match FirstEmissionBlockNumber::<Test>::get(netuid) {
            Some(new_emission_block_number) => {
                assert_eq!(new_emission_block_number, block_number + 1)
            }
            None => assert!(FirstEmissionBlockNumber::<Test>::get(netuid).is_some()),
        }
    });
}

#[test]
fn test_register_network_min_burn_at_default() {
    new_test_ext(1).execute_with(|| {
        let sn_owner_coldkey = U256::from(0);
        let sn_owner_hotkey = U256::from(1);
        let cost = SubtensorModule::get_network_lock_cost();

        // Give coldkey enough for lock
        add_balance_to_coldkey_account(&sn_owner_coldkey, ExistentialDeposit::get() + cost.into());

        // Register network
        assert_ok!(SubtensorModule::register_network(
            <<Test as Config>::RuntimeOrigin>::signed(sn_owner_coldkey),
            sn_owner_hotkey
        ));

        // Get netuid of the new network
        let netuid = match last_event() {
            RuntimeEvent::SubtensorModule(Event::<Test>::NetworkAdded(netuid, _)) => netuid,
            _ => panic!("Expected NetworkAdded event"),
        };

        // Check min burn is set to default
        assert_eq!(MinBurn::<Test>::get(netuid), InitialMinBurn::get().into());

        // Check registration allowed
        assert!(NetworkRegistrationAllowed::<Test>::get(netuid));
        assert!(NetworkPowRegistrationAllowed::<Test>::get(netuid));
    });
}

#[test]
fn test_register_network_use_symbol_for_subnet_if_available() {
    new_test_ext(1).execute_with(|| {
        SubtensorModule::set_max_subnets(SYMBOLS.len() as u16);
        for i in 0..(SYMBOLS.len() - 1) {
            let coldkey = U256::from(1_000_000 + i);
            let hotkey = U256::from(2_000_000 + i);
            let cost = SubtensorModule::get_network_lock_cost();
            add_balance_to_coldkey_account(&coldkey, ExistentialDeposit::get() + cost.into());

            assert_ok!(SubtensorModule::register_network(
                <<Test as Config>::RuntimeOrigin>::signed(coldkey),
                hotkey
            ));

            let netuid = match last_event() {
                RuntimeEvent::SubtensorModule(Event::<Test>::NetworkAdded(netuid, _)) => netuid,
                _ => panic!("Expected NetworkAdded event"),
            };

            // Ensure the symbol correspond to the netuid has been set
            let expected_symbol = SYMBOLS.get(usize::from(u16::from(netuid))).unwrap();
            assert_eq!(TokenSymbol::<Test>::get(netuid), *expected_symbol);

            // Check registration allowed
            assert!(NetworkRegistrationAllowed::<Test>::get(netuid));
            assert!(NetworkPowRegistrationAllowed::<Test>::get(netuid));

            // Reduce lock cost to avoid exponential cost growth
            NetworkLastLockCost::<Test>::set(1_000.into());
        }
    });
}

#[test]
fn test_register_network_use_next_available_symbol_if_symbol_for_subnet_is_taken() {
    new_test_ext(1).execute_with(|| {
        // Register 50 networks (additionnaly to the root network)
        for i in 0..50 {
            let coldkey = U256::from(1_000_000 + i);
            let hotkey = U256::from(2_000_000 + i);
            let cost = SubtensorModule::get_network_lock_cost();
            add_balance_to_coldkey_account(&coldkey, ExistentialDeposit::get() + cost.into());

            assert_ok!(SubtensorModule::register_network(
                <<Test as Config>::RuntimeOrigin>::signed(coldkey),
                hotkey
            ));

            let netuid = match last_event() {
                RuntimeEvent::SubtensorModule(Event::<Test>::NetworkAdded(netuid, _)) => netuid,
                _ => panic!("Expected NetworkAdded event"),
            };

            // Ensure the symbol correspond to the netuid has been set
            let expected_symbol = SYMBOLS.get(usize::from(u16::from(netuid))).unwrap();
            assert_eq!(TokenSymbol::<Test>::get(netuid), *expected_symbol);

            // Check registration allowed
            assert!(NetworkRegistrationAllowed::<Test>::get(netuid));
            assert!(NetworkPowRegistrationAllowed::<Test>::get(netuid));

            // Reduce lock cost to avoid exponential cost growth
            NetworkLastLockCost::<Test>::set(1_000.into());
        }

        // Swap some of the network symbol for the network 25 to network 51 symbol (not registered yet)
        TokenSymbol::<Test>::insert(NetUid::from(25), SYMBOLS.get(51).unwrap().to_vec());

        // Register a new network
        let coldkey = U256::from(1_000_000 + 50);
        let hotkey = U256::from(2_000_000 + 50);
        let cost = SubtensorModule::get_network_lock_cost();
        add_balance_to_coldkey_account(&coldkey, ExistentialDeposit::get() + cost.into());

        assert_ok!(SubtensorModule::register_network(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            hotkey
        ));

        // Get netuid of the new network
        let netuid = match last_event() {
            RuntimeEvent::SubtensorModule(Event::<Test>::NetworkAdded(netuid, _)) => netuid,
            _ => panic!("Expected NetworkAdded event"),
        };

        // We expect the symbol to be the one that was previously taken by network 25, before it was swapped
        let expected_symbol = SYMBOLS.get(25).unwrap();
        assert_eq!(TokenSymbol::<Test>::get(netuid), *expected_symbol);
    });
}

#[test]
fn test_register_network_use_default_symbol_if_all_symbols_are_taken() {
    new_test_ext(1).execute_with(|| {
        // Register networks until we have exhausted all symbols
        SubtensorModule::set_max_subnets(SYMBOLS.len() as u16);
        for i in 0..(SYMBOLS.len() - 1) {
            let coldkey = U256::from(1_000_000 + i);
            let hotkey = U256::from(2_000_000 + i);
            let cost = SubtensorModule::get_network_lock_cost();
            add_balance_to_coldkey_account(&coldkey, ExistentialDeposit::get() + cost.into());

            assert_ok!(SubtensorModule::register_network(
                <<Test as Config>::RuntimeOrigin>::signed(coldkey),
                hotkey
            ));

            // Reduce lock cost to avoid exponential cost growth
            NetworkLastLockCost::<Test>::set(1_000.into());
        }

        // Register a new network
        let coldkey = U256::from(1_000_000 + 50);
        let hotkey = U256::from(2_000_000 + 50);
        let cost = SubtensorModule::get_network_lock_cost();
        add_balance_to_coldkey_account(&coldkey, ExistentialDeposit::get() + cost.into());

        assert_ok!(SubtensorModule::register_network(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            hotkey
        ));

        // Get netuid of the new network
        let netuid = match last_event() {
            RuntimeEvent::SubtensorModule(Event::<Test>::NetworkAdded(netuid, _)) => netuid,
            _ => panic!("Expected NetworkAdded event"),
        };
        assert_eq!(netuid, NetUid::from(SYMBOLS.len() as u16));

        // We expect the symbol to be the default symbol
        assert_eq!(TokenSymbol::<Test>::get(netuid), *DEFAULT_SYMBOL);

        // Check registration allowed
        assert!(NetworkRegistrationAllowed::<Test>::get(netuid));
        assert!(NetworkPowRegistrationAllowed::<Test>::get(netuid));
    });
}

// cargo test --package pallet-subtensor --lib -- tests::subnet::test_subtoken_enable --exact --show-output
#[test]
fn test_subtoken_enable() {
    // ensure_subtoken_enabled
    new_test_ext(1).execute_with(|| {
        let account = U256::from(0);
        let netuid = NetUid::from(1);
        // let to_be_set: u64 = 10
        add_network_disable_subtoken(netuid, 10, 0);
        assert!(!SubtokenEnabled::<Test>::get(netuid));

        let block_number = System::block_number() + StartCallDelay::<Test>::get();
        System::set_block_number(block_number);

        assert_ok!(SubtensorModule::start_call(
            <<Test as Config>::RuntimeOrigin>::signed(account),
            netuid
        ));

        assert!(SubtokenEnabled::<Test>::get(netuid));
    });
}

// cargo test --package pallet-subtensor --lib -- tests::subnet::test_subtoken_enable_reject_trading_before_enable --exact --show-output
#[allow(clippy::unwrap_used)]
#[test]
fn test_subtoken_enable_reject_trading_before_enable() {
    // ensure_subtoken_enabled
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let netuid2 = NetUid::from(2);
        let hotkey_account_id: U256 = U256::from(1);
        let coldkey_account_id = U256::from(2);
        let hotkey_account_2_id: U256 = U256::from(3);
        let amount = DefaultMinStake::<Test>::get().to_u64() * 10;

        let stake_bal = AlphaBalance::from(10_000_000_000_u64); // 10 Alpha

        let limit_price = TaoBalance::from(1_000_000_000_u64); // not important

        add_network_disable_subtoken(netuid, 10, 0);
        add_network_disable_subtoken(netuid2, 10, 0);

        assert!(!SubtokenEnabled::<Test>::get(netuid));
        assert!(!SubtokenEnabled::<Test>::get(netuid2));

        // Set liq high enough to not trigger other errors
        SubnetTAO::<Test>::set(netuid, TaoBalance::from(20_000_000_000_u64));
        SubnetAlphaIn::<Test>::set(netuid, AlphaBalance::from(20_000_000_000_u64));

        // Register so staking *could* work
        register_ok_neuron(netuid, hotkey_account_id, coldkey_account_id, 0);
        register_ok_neuron(netuid2, hotkey_account_id, coldkey_account_id, 100);
        register_ok_neuron(netuid, hotkey_account_2_id, coldkey_account_id, 0);
        register_ok_neuron(netuid2, hotkey_account_2_id, coldkey_account_id, 100);

        add_balance_to_coldkey_account(&coldkey_account_id, 10_000.into());

        // Give some stake
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &coldkey_account_id,
            netuid,
            stake_bal,
        );

        // all trading extrinsic should be rejected.
        assert_noop!(
            SubtensorModule::add_stake(
                RuntimeOrigin::signed(coldkey_account_id),
                hotkey_account_id,
                netuid,
                amount.into()
            ),
            Error::<Test>::SubtokenDisabled
        );

        assert_noop!(
            SubtensorModule::add_stake_limit(
                RuntimeOrigin::signed(coldkey_account_id),
                hotkey_account_id,
                netuid,
                amount.into(),
                limit_price,
                false
            ),
            Error::<Test>::SubtokenDisabled
        );

        // For unstake_all the result is Ok, but the
        // operation is not performed.
        assert_ok!(SubtensorModule::unstake_all(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id
        ));
        // Check that the stake is still the same
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey_account_id,
                &coldkey_account_id,
                netuid
            ),
            stake_bal
        );

        // For unstake_all_alpha, the result is AmountTooLow because no re-staking happens.
        assert_noop!(
            SubtensorModule::unstake_all_alpha(
                RuntimeOrigin::signed(coldkey_account_id),
                hotkey_account_id
            ),
            Error::<Test>::AmountTooLow
        );
        // Check that the stake is still the same
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey_account_id,
                &coldkey_account_id,
                netuid
            ),
            stake_bal
        );

        SubtensorModule::remove_stake_limit(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
            netuid,
            amount.into(),
            limit_price,
            false,
        )
        .unwrap();

        assert_noop!(
            SubtensorModule::remove_stake(
                RuntimeOrigin::signed(coldkey_account_id),
                hotkey_account_id,
                netuid,
                amount.into()
            ),
            Error::<Test>::SubtokenDisabled
        );

        assert_noop!(
            SubtensorModule::recycle_alpha(
                RuntimeOrigin::signed(coldkey_account_id),
                hotkey_account_id,
                amount.into(),
                netuid
            ),
            Error::<Test>::SubtokenDisabled
        );

        assert_noop!(
            SubtensorModule::burn_alpha(
                RuntimeOrigin::signed(coldkey_account_id),
                hotkey_account_id,
                amount.into(),
                netuid
            ),
            Error::<Test>::SubtokenDisabled
        );

        assert_noop!(
            SubtensorModule::move_stake(
                RuntimeOrigin::signed(coldkey_account_id),
                hotkey_account_id,
                hotkey_account_2_id,
                netuid,
                netuid2,
                amount.into(),
            ),
            Error::<Test>::SubtokenDisabled
        );

        assert_noop!(
            SubtensorModule::transfer_stake(
                RuntimeOrigin::signed(coldkey_account_id),
                hotkey_account_id,
                hotkey_account_2_id,
                netuid,
                netuid2,
                amount.into(),
            ),
            Error::<Test>::SubtokenDisabled
        );

        assert_noop!(
            SubtensorModule::swap_stake(
                RuntimeOrigin::signed(coldkey_account_id),
                hotkey_account_id,
                netuid,
                netuid2,
                amount.into(),
            ),
            Error::<Test>::SubtokenDisabled
        );
    });
}

// cargo test --package pallet-subtensor --lib -- tests::subnet::test_subtoken_enable_trading_ok_with_enable --exact --show-output
#[test]
fn test_subtoken_enable_trading_ok_with_enable() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let netuid2 = NetUid::from(2);
        let hotkey_account_id: U256 = U256::from(1);
        let coldkey_account_id = U256::from(2);
        let hotkey_account_2_id: U256 = U256::from(3);
        // stake big enough
        let stake_amount = DefaultMinStake::<Test>::get() * 10000.into();
        // unstake, transfer, swap just very little
        let unstake_amount = AlphaBalance::from(DefaultMinStake::<Test>::get().to_u64() * 10);

        add_network(netuid, 10, 0);
        add_network(netuid2, 10, 0);

        let reserve = stake_amount.to_u64() * 1000;
        mock::setup_reserves(netuid, reserve.into(), reserve.into());
        mock::setup_reserves(netuid2, reserve.into(), reserve.into());
        SubnetAlphaOut::<Test>::insert(netuid, AlphaBalance::from(reserve));
        SubnetAlphaOut::<Test>::insert(netuid2, AlphaBalance::from(reserve));

        // Register so staking works
        register_ok_neuron(netuid, hotkey_account_id, coldkey_account_id, 0);
        register_ok_neuron(netuid2, hotkey_account_id, coldkey_account_id, 100);
        register_ok_neuron(netuid, hotkey_account_2_id, coldkey_account_id, 0);
        register_ok_neuron(netuid2, hotkey_account_2_id, coldkey_account_id, 100);

        add_balance_to_coldkey_account(&coldkey_account_id, stake_amount * 10.into());

        // all trading extrinsic should be possible now that subtoken is enabled.
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
            netuid,
            stake_amount
        ));

        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
            netuid2,
            stake_amount
        ));

        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_2_id,
            netuid,
            stake_amount
        ));

        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_2_id,
            netuid2,
            stake_amount
        ));

        assert_ok!(SubtensorModule::remove_stake(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
            netuid,
            unstake_amount
        ));

        assert_ok!(SubtensorModule::recycle_alpha(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
            unstake_amount,
            netuid
        ));

        assert_ok!(SubtensorModule::burn_alpha(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
            unstake_amount,
            netuid
        ));

        assert_ok!(SubtensorModule::move_stake(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
            hotkey_account_2_id,
            netuid,
            netuid2,
            unstake_amount,
        ));

        assert_ok!(SubtensorModule::transfer_stake(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
            hotkey_account_2_id,
            netuid,
            netuid2,
            unstake_amount,
        ));

        assert_ok!(SubtensorModule::swap_stake(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
            netuid,
            netuid2,
            unstake_amount,
        ));

        assert_ok!(SubtensorModule::unstake_all_alpha(
            RuntimeOrigin::signed(coldkey_account_id),
            hotkey_account_id,
        ));
    });
}

// cargo test --package pallet-subtensor --lib -- tests::subnet::test_subtoken_enable_ok_for_burn_register_before_enable --exact --show-output
#[test]
fn test_subtoken_enable_ok_for_burn_register_before_enable() {
    // ensure_subtoken_enabled
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let netuid2 = NetUid::from(2);
        let hotkey_account_id: U256 = U256::from(1);
        let coldkey_account_id = U256::from(2);
        let hotkey_account_2_id: U256 = U256::from(3);

        let burn_cost = TaoBalance::from(1000);

        // Add the networks with subtoken disabled.
        add_network_disable_subtoken(netuid, 10, 0);
        add_network_disable_subtoken(netuid2, 10, 0);

        // Ensure reserves exist for swap/burn path.
        mock::setup_reserves(netuid, 1_000_000_000.into(), 1_000_000_000.into());
        mock::setup_reserves(netuid2, 1_000_000_000.into(), 1_000_000_000.into());

        // Set burn AFTER network creation for BOTH networks.
        SubtensorModule::set_burn(netuid, burn_cost);
        SubtensorModule::set_burn(netuid2, burn_cost);

        // Fund enough to burned-register twice + keep-alive buffer.
        let burn_1 = SubtensorModule::get_burn(netuid);
        let burn_2 = SubtensorModule::get_burn(netuid2);
        add_balance_to_coldkey_account(
            &coldkey_account_id,
            burn_1
                .saturating_add(burn_2)
                .saturating_add(ExistentialDeposit::get())
                .saturating_add(5_000.into()),
        );

        // Should be possible to burned register before enable is activated
        assert_ok!(SubtensorModule::burned_register(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey_account_id),
            netuid,
            hotkey_account_id
        ));

        assert_ok!(SubtensorModule::burned_register(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey_account_id),
            netuid2,
            hotkey_account_2_id
        ));
    });
}

// cargo test --package pallet-subtensor --lib -- tests::subnet::test_no_duplicates_in_symbol_static --exact --show-output
#[test]
fn test_no_duplicates_in_symbol_static() {
    use std::collections::HashSet;

    let mut seen = HashSet::new();
    for netuid in 0..(SYMBOLS.len() as u16) {
        let symbol = SYMBOLS.get(usize::from(netuid)).unwrap();
        assert!(
            seen.insert(symbol.to_vec()),
            "Duplicate symbol found for netuid {netuid}: {symbol:?}"
        );
    }
}

#[test]
fn test_get_symbol_for_subnet_returns_default_symbol_if_netuid_is_out_of_bounds() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(SYMBOLS.len() as u16 + 1);
        let symbol = Pallet::<Test>::get_symbol_for_subnet(netuid);
        assert_eq!(symbol, DEFAULT_SYMBOL);
    });
}

#[test]
fn test_update_symbol_works_as_root_if_symbol_exists_and_available() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 10, 0);

        // only one network so we can set any symbol, except the root symbol
        for i in 1..SYMBOLS.len() {
            let symbol = SYMBOLS.get(i).unwrap().to_vec();
            assert_ok!(SubtensorModule::update_symbol(
                <Test as Config>::RuntimeOrigin::root(),
                netuid,
                symbol.clone()
            ));

            assert_eq!(TokenSymbol::<Test>::get(netuid), symbol);
            assert_last_event::<Test>(Event::SymbolUpdated { netuid, symbol }.into());
        }
    });
}

#[test]
fn test_update_symbol_works_as_subnet_owner_if_symbol_exists_and_available() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let netuid = NetUid::from(1);
        add_network(netuid, 10, 0);
        SubnetOwner::<Test>::insert(netuid, coldkey);

        // only one network so we can set any symbol, except the root symbol
        for i in 1..SYMBOLS.len() {
            let symbol = SYMBOLS.get(i).unwrap().to_vec();

            assert_ok!(SubtensorModule::update_symbol(
                <Test as Config>::RuntimeOrigin::signed(coldkey),
                netuid,
                symbol.clone()
            ));

            assert_eq!(TokenSymbol::<Test>::get(netuid), symbol);
            assert_last_event::<Test>(Event::SymbolUpdated { netuid, symbol }.into());
        }
    });
}

#[test]
fn test_update_symbol_fails_if_symbol_doesnt_exist() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let netuid = NetUid::from(1);
        add_network(netuid, 10, 0);
        SubnetOwner::<Test>::insert(netuid, coldkey);

        assert_err!(
            SubtensorModule::update_symbol(
                <Test as Config>::RuntimeOrigin::signed(coldkey),
                netuid,
                b"TEST".to_vec()
            ),
            Error::<Test>::SymbolDoesNotExist
        );
    });
}

#[test]
fn test_update_symbol_fails_if_symbol_already_in_use() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let netuid = NetUid::from(1);
        add_network(netuid, 10, 0);
        SubnetOwner::<Test>::insert(netuid, coldkey);

        let coldkey2 = U256::from(2);
        let netuid2 = NetUid::from(2);
        add_network(netuid2, 10, 0);
        SubnetOwner::<Test>::insert(netuid2, coldkey2);

        assert_ok!(SubtensorModule::update_symbol(
            <Test as Config>::RuntimeOrigin::signed(coldkey),
            netuid,
            SYMBOLS.get(42).unwrap().to_vec()
        ));

        assert_err!(
            SubtensorModule::update_symbol(
                <Test as Config>::RuntimeOrigin::signed(coldkey2),
                netuid2,
                SYMBOLS.get(42).unwrap().to_vec()
            ),
            Error::<Test>::SymbolAlreadyInUse
        );
    });
}

// cargo test --package pallet-subtensor --lib -- tests::subnet::test_get_subnet_account_id_exists_and_is_distinct_for_257_consecutive_subnets --exact --nocapture
#[test]
fn test_get_subnet_account_id_exists_and_is_distinct_for_257_consecutive_subnets() {
    new_test_ext(1).execute_with(|| {
        let mut account_ids = BTreeSet::new();

        for raw_netuid in 0u16..=256u16 {
            let netuid = NetUid::from(raw_netuid);
            add_network(netuid, 10, 0);

            let account_id = SubtensorModule::get_subnet_account_id(netuid);
            assert!(
                account_ids.insert(account_id),
                "duplicate subnet account id for netuid {:?}",
                netuid
            );
        }

        assert_eq!(account_ids.len(), 257);
    });
}

// cargo test --package pallet-subtensor --lib -- tests::subnet::test_is_subnet_account_id --exact --nocapture
#[test]
fn test_is_subnet_account_id() {
    new_test_ext(1).execute_with(|| {
        for raw_netuid in 0u16..=2048u16 {
            let netuid = NetUid::from(raw_netuid);
            add_network(netuid, 10, 0);

            let account_id = SubtensorModule::get_subnet_account_id(netuid).unwrap();
            let roudtrip_netuid = SubtensorModule::is_subnet_account_id(&account_id);
            assert_eq!(netuid, roudtrip_netuid.unwrap());
        }

        // Not a subnet account
        let not_subnet_account_id = U256::from(1);
        assert!(SubtensorModule::is_subnet_account_id(&not_subnet_account_id).is_none());
    });
}

// cargo test --package pallet-subtensor --lib -- tests::subnet::test_cannot_register_system_hotkey --exact --nocapture
#[test]
fn test_cannot_register_system_hotkey() {
    new_test_ext(1).execute_with(|| {
        for raw_netuid in 0u16..=2048u16 {
            let coldkey = U256::from(1);
            let netuid = NetUid::from(raw_netuid);
            add_network(netuid, 10, 0);

            let account_id = SubtensorModule::get_subnet_account_id(netuid).unwrap();
            assert_err!(
                SubtensorModule::create_account_if_non_existent(&coldkey, &account_id),
                Error::<Test>::CannotUseSystemAccount
            );
            assert!(!SubtensorModule::coldkey_owns_hotkey(&coldkey, &account_id),);
        }
    });
}

#[test]
fn test_burned_register_increases_subnet_tao_flow() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let coldkey = U256::from(77);
        let hotkey = U256::from(88);

        add_network(netuid, 13, 0);
        mock::setup_reserves(netuid, DEFAULT_RESERVE.into(), DEFAULT_RESERVE.into());

        let burn = 1_000u64;
        SubtensorModule::set_burn(netuid, burn.into());
        let flow_before = SubnetTaoFlow::<Test>::get(netuid);

        add_balance_to_coldkey_account(
            &coldkey,
            ExistentialDeposit::get() + burn.into() + 10u64.into(),
        );

        assert_ok!(SubtensorModule::burned_register(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            netuid,
            hotkey
        ));

        assert_eq!(
            SubnetTaoFlow::<Test>::get(netuid),
            flow_before + burn as i64
        );
    });
}

#[test]
fn test_register_network_gives_owner_no_initial_alpha_distribution() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(5001);
        let owner_hotkey = U256::from(5002);
        let lock_cost = SubtensorModule::get_network_lock_cost();
        let netuids_before = SubtensorModule::get_all_subnet_netuids();

        add_balance_to_coldkey_account(
            &owner_coldkey,
            ExistentialDeposit::get() + lock_cost.into(),
        );

        assert_ok!(SubtensorModule::register_network(
            <<Test as Config>::RuntimeOrigin>::signed(owner_coldkey),
            owner_hotkey
        ));

        let netuid = SubtensorModule::get_all_subnet_netuids()
            .into_iter()
            .find(|netuid| !netuids_before.contains(netuid))
            .expect("new subnet should be added");

        assert_eq!(SubnetOwner::<Test>::get(netuid), owner_coldkey);
        assert_eq!(SubnetOwnerHotkey::<Test>::get(netuid), owner_hotkey);
        assert_eq!(SubnetAlphaOut::<Test>::get(netuid), AlphaBalance::ZERO);
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &owner_hotkey,
                &owner_coldkey,
                netuid
            ),
            AlphaBalance::ZERO
        );
        assert!(
            Lock::<Test>::iter_prefix((&owner_coldkey, netuid))
                .next()
                .is_none()
        );
    });
}
