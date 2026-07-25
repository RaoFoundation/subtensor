#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
//! Deferred `NetworkRegistrationQueue` processing after dissolve cleanup.

use super::prelude::*;

#[test]
fn register_network_queues_when_waiting_for_dissolve_cleanup() {
    new_test_ext(0).execute_with(|| {
        SubnetLimit::<Test>::put(2u16);

        let n1 = add_dynamic_network(&U256::from(9102), &U256::from(9101));
        let _n2 = add_dynamic_network(&U256::from(9202), &U256::from(9201));

        assert_ok!(SubtensorModule::do_dissolve_network(n1));
        assert!(DissolveCleanupQueue::<Test>::get().contains(&n1));

        let cold = U256::from(9301);
        let hot = U256::from(9302);
        let lock_amount = SubtensorModule::get_network_lock_cost();
        add_balance_to_coldkey_account(&cold, lock_amount.saturating_mul(2.into()).into());
        TotalIssuance::<Test>::mutate(|total| *total = total.saturating_add(lock_amount));

        assert_ok!(SubtensorModule::do_register_network(
            RuntimeOrigin::signed(cold),
            &hot,
            1,
            None,
        ));

        assert_eq!(NetworkRegistrationQueue::<Test>::get().len(), 1);
        assert_eq!(NetworkRegistrationQueue::<Test>::get()[0].coldkey, cold);
        assert_eq!(TotalNetworks::<Test>::get(), 1);
        assert!(!SubtensorModule::hotkey_account_exists(&hot));
    });
}

#[test]
fn process_network_registration_queue_registers_after_cleanup_slot_available() {
    new_test_ext(0).execute_with(|| {
        SubnetLimit::<Test>::put(2u16);

        let n1 = add_dynamic_network(&U256::from(9402), &U256::from(9401));
        let n2 = add_dynamic_network(&U256::from(9502), &U256::from(9501));

        assert_ok!(SubtensorModule::do_dissolve_network(n1));
        assert!(DissolveCleanupQueue::<Test>::get().contains(&n1));

        let cold = U256::from(9601);
        let hot = U256::from(9602);
        let lock_amount = SubtensorModule::get_network_lock_cost();
        add_balance_to_coldkey_account(&cold, lock_amount.saturating_mul(3.into()).into());
        TotalIssuance::<Test>::mutate(|total| *total = total.saturating_add(lock_amount));

        assert_ok!(SubtensorModule::do_register_network(
            RuntimeOrigin::signed(cold),
            &hot,
            1,
            None,
        ));
        assert_eq!(NetworkRegistrationQueue::<Test>::get().len(), 1);

        // Simulate dissolve cleanup completing and freeing a subnet slot.
        DissolveCleanupQueue::<Test>::kill();

        run_network_registration_queue();

        assert!(NetworkRegistrationQueue::<Test>::get().is_empty());
        assert!(SubtensorModule::hotkey_account_exists(&hot));
        assert_eq!(TotalNetworks::<Test>::get(), 2);

        let registered_netuid = NetworksAdded::<Test>::iter()
            .find(|(netuid, added)| *added && *netuid != n2)
            .map(|(netuid, _)| netuid)
            .expect("queued registration should create a new subnet");
        assert_eq!(SubnetOwner::<Test>::get(registered_netuid), cold);
    });
}

#[test]
fn register_network_prune_registers_registration_queued() {
    new_test_ext(0).execute_with(|| {
        SubnetLimit::<Test>::put(2u16);

        let n1 = add_dynamic_network(&U256::from(9702), &U256::from(9701));
        let n2 = add_dynamic_network(&U256::from(9802), &U256::from(9801));

        let imm = SubtensorModule::get_network_immunity_period();
        System::set_block_number(imm + 100);
        Emission::<Test>::insert(n1, vec![AlphaBalance::from(1)]);
        Emission::<Test>::insert(n2, vec![AlphaBalance::from(1_000)]);

        let cold = U256::from(9901);
        let hot = U256::from(9902);
        let lock_amount = SubtensorModule::get_network_lock_cost();
        add_balance_to_coldkey_account(&cold, lock_amount.saturating_mul(10.into()).into());
        TotalIssuance::<Test>::mutate(|total| *total = total.saturating_add(lock_amount));

        assert_ok!(SubtensorModule::do_register_network(
            RuntimeOrigin::signed(cold),
            &hot,
            1,
            None,
        ));

        assert!(NetworkRegistrationQueue::<Test>::get().len() == 1);
        assert!(DissolveCleanupQueue::<Test>::get().contains(&n1));
        assert!(!NetworksAdded::<Test>::get(n1));
    });
}

#[test]
fn process_network_registration_queue_noop_when_empty() {
    new_test_ext(1).execute_with(|| {
        let networks_before = TotalNetworks::<Test>::get();

        SubtensorModule::process_network_registration_queue();

        assert!(NetworkRegistrationQueue::<Test>::get().is_empty());
        assert_eq!(TotalNetworks::<Test>::get(), networks_before);
    });
}

#[test]
fn process_network_registration_queue_waits_for_cleanup_completion() {
    new_test_ext(0).execute_with(|| {
        SubnetLimit::<Test>::put(2u16);

        let n1 = add_dynamic_network(&U256::from(10_502), &U256::from(10_501));
        let _n2 = add_dynamic_network(&U256::from(10_602), &U256::from(10_601));

        assert_ok!(SubtensorModule::do_dissolve_network(n1));

        let cold = U256::from(10_701);
        let hot = U256::from(10_702);
        let lock_amount = SubtensorModule::get_network_lock_cost();
        add_balance_to_coldkey_account(&cold, lock_amount.saturating_mul(2.into()).into());

        assert_ok!(SubtensorModule::do_register_network(
            RuntimeOrigin::signed(cold),
            &hot,
            1,
            None,
        ));
        assert_eq!(NetworkRegistrationQueue::<Test>::get().len(), 1);

        // Cleanup is still pending: the queued registration must not be released.
        SubtensorModule::process_network_registration_queue();

        assert_eq!(NetworkRegistrationQueue::<Test>::get().len(), 1);
        assert!(!SubtensorModule::hotkey_account_exists(&hot));
        assert_eq!(TotalNetworks::<Test>::get(), 1);

        // Once cleanup completes, the same call releases the registration.
        DissolveCleanupQueue::<Test>::kill();
        SubtensorModule::process_network_registration_queue();

        assert!(NetworkRegistrationQueue::<Test>::get().is_empty());
        assert!(SubtensorModule::hotkey_account_exists(&hot));
        assert_eq!(TotalNetworks::<Test>::get(), 2);
    });
}

#[test]
fn process_network_registration_queue_processes_one_entry_per_call() {
    new_test_ext(0).execute_with(|| {
        SubnetLimit::<Test>::put(3u16);

        let n1 = add_dynamic_network(&U256::from(10_802), &U256::from(10_801));
        let n2 = add_dynamic_network(&U256::from(10_902), &U256::from(10_901));
        let _n3 = add_dynamic_network(&U256::from(11_002), &U256::from(11_001));

        assert_ok!(SubtensorModule::do_dissolve_network(n1));
        assert_ok!(SubtensorModule::do_dissolve_network(n2));
        assert_eq!(DissolveCleanupQueue::<Test>::get().len(), 2);

        let cold_a = U256::from(11_101);
        let hot_a = U256::from(11_102);
        let cold_b = U256::from(11_201);
        let hot_b = U256::from(11_202);
        for cold in [&cold_a, &cold_b] {
            let lock_amount = SubtensorModule::get_network_lock_cost();
            add_balance_to_coldkey_account(cold, lock_amount.saturating_mul(2.into()).into());
        }

        assert_ok!(SubtensorModule::do_register_network(
            RuntimeOrigin::signed(cold_a),
            &hot_a,
            1,
            None,
        ));
        assert_ok!(SubtensorModule::do_register_network(
            RuntimeOrigin::signed(cold_b),
            &hot_b,
            1,
            None,
        ));
        assert_eq!(NetworkRegistrationQueue::<Test>::get().len(), 2);

        DissolveCleanupQueue::<Test>::kill();

        // First call processes only the first (FIFO) entry.
        SubtensorModule::process_network_registration_queue();
        assert_eq!(NetworkRegistrationQueue::<Test>::get().len(), 1);
        assert!(SubtensorModule::hotkey_account_exists(&hot_a));
        assert!(!SubtensorModule::hotkey_account_exists(&hot_b));
        assert_eq!(NetworkRegistrationQueue::<Test>::get()[0].coldkey, cold_b);

        // Second call processes the remaining entry.
        SubtensorModule::process_network_registration_queue();
        assert!(NetworkRegistrationQueue::<Test>::get().is_empty());
        assert!(SubtensorModule::hotkey_account_exists(&hot_b));
        assert_eq!(TotalNetworks::<Test>::get(), 3);
    });
}

#[test]
fn process_network_registration_queue_unlocks_funds_and_charges_coldkey() {
    new_test_ext(0).execute_with(|| {
        SubnetLimit::<Test>::put(2u16);

        let n1 = add_dynamic_network(&U256::from(11_302), &U256::from(11_301));
        let n2 = add_dynamic_network(&U256::from(11_402), &U256::from(11_401));

        assert_ok!(SubtensorModule::do_dissolve_network(n1));

        let cold = U256::from(11_501);
        let hot = U256::from(11_502);
        let lock_amount = SubtensorModule::get_network_lock_cost();
        let lock_id = NetworkRegistrationLockId::<Test>::get();
        let mut identifier = [0u8; 8];
        identifier[..4].copy_from_slice(b"rglk");
        identifier[4..8].copy_from_slice(&lock_id.to_le_bytes());
        add_balance_to_coldkey_account(&cold, lock_amount.saturating_mul(3.into()).into());

        assert_ok!(SubtensorModule::do_register_network(
            RuntimeOrigin::signed(cold),
            &hot,
            1,
            None,
        ));

        // Funds are locked while queued.
        assert!(
            pallet_balances::Locks::<Test>::get(cold)
                .iter()
                .any(|l| l.id == identifier)
        );
        let queued_lock = NetworkRegistrationQueue::<Test>::get()[0].lock_amount;
        // Use free balance: the reducible balance is already reduced by the lock.
        let balance_before = pallet_balances::Pallet::<Test>::free_balance(cold);

        DissolveCleanupQueue::<Test>::kill();
        SubtensorModule::process_network_registration_queue();

        // Lock released and the lock cost transferred to the new subnet.
        assert!(
            pallet_balances::Locks::<Test>::get(cold)
                .iter()
                .all(|l| l.id != identifier)
        );
        let balance_after = pallet_balances::Pallet::<Test>::free_balance(cold);
        assert_eq!(balance_before.saturating_sub(balance_after), queued_lock);

        let new_netuid = NetworksAdded::<Test>::iter()
            .find(|(netuid, added)| *added && *netuid != n2)
            .map(|(netuid, _)| netuid)
            .expect("queued registration should create a new subnet");
        assert_eq!(SubnetOwner::<Test>::get(new_netuid), cold);
        assert_eq!(SubnetLocked::<Test>::get(new_netuid), queued_lock);
    });
}
