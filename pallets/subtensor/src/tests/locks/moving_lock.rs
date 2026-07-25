#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
//! Moving lock.

use super::helpers::*;
use super::prelude::*;

// =========================================================================
// GROUP 19: Moving lock
// =========================================================================

#[test]
fn test_moving_lock() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey_origin = U256::from(2);
        let hotkey_destination = U256::from(3);
        let netuid = setup_subnet_with_stake(coldkey, hotkey_origin, 100_000_000_000);
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &coldkey,
            &hotkey_destination
        ));

        let lock_amount = 5000u64.into();
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey_origin,
            lock_amount
        ));

        // Mock a non-zero conviction
        let mut lock = Lock::<Test>::get((coldkey, netuid, hotkey_origin)).unwrap();
        lock.conviction = U64F64::from_num(1234);
        Lock::<Test>::insert((coldkey, netuid, hotkey_origin), lock);
        let mut hotkey_lock = HotkeyLock::<Test>::get(netuid, hotkey_origin).unwrap();
        hotkey_lock.conviction = U64F64::from_num(1234);
        HotkeyLock::<Test>::insert(netuid, hotkey_origin, hotkey_lock);

        assert_ok!(SubtensorModule::move_lock(
            RuntimeOrigin::signed(coldkey),
            hotkey_destination,
            netuid,
        ));
        let lock = Lock::<Test>::get((coldkey, netuid, hotkey_destination)).unwrap();
        assert_eq!(lock.locked_mass, lock_amount);
        assert_eq!(lock.conviction, U64F64::from_num(1234));

        // Hotkey lock is removed on origin and added on destination
        assert!(HotkeyLock::<Test>::get(netuid, hotkey_origin).is_none());
        let hotkey_lock_destination_after =
            HotkeyLock::<Test>::get(netuid, hotkey_destination).unwrap();
        assert_eq!(hotkey_lock_destination_after.locked_mass, lock_amount);

        // Conviction is not reset because owner is the same for origin and destination
        // hotkeys
        assert_eq!(
            hotkey_lock_destination_after.conviction,
            U64F64::from_num(1234)
        );
    });
}

#[test]
fn test_moving_lock_to_subnet_owner_hotkey_gets_owner_conviction_for_non_owner_coldkey() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey_origin = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey_origin, 100_000_000_000);
        let owner_hotkey = SubnetOwnerHotkey::<Test>::get(netuid);

        let lock_amount = 5000u64.into();
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey_origin,
            lock_amount
        ));

        assert_ok!(SubtensorModule::move_lock(
            RuntimeOrigin::signed(coldkey),
            owner_hotkey,
            netuid,
        ));

        let lock = Lock::<Test>::get((coldkey, netuid, owner_hotkey)).unwrap();
        assert_eq!(lock.locked_mass, lock_amount);
        assert_eq!(lock.conviction, U64F64::from_num(5000));

        assert!(
            HotkeyLock::<Test>::get(netuid, owner_hotkey).is_none(),
            "lock moved to owner hotkey should use OwnerLock"
        );
        let owner_lock = OwnerLock::<Test>::get(netuid).unwrap();
        assert_eq!(owner_lock.locked_mass, lock_amount);
        assert_eq!(owner_lock.conviction, U64F64::from_num(5000));
    });
}

#[test]
fn test_moving_partial_lock() {
    new_test_ext(1).execute_with(|| {
        let coldkey1 = U256::from(1);
        let coldkey2 = U256::from(2);
        let hotkey_origin = U256::from(3);
        let hotkey_destination = U256::from(4);
        let netuid = setup_subnet_with_stake(coldkey1, hotkey_origin, 100_000_000_000);

        // Make hotkey_origin and hotkey_destination owned by different coldkeys
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &coldkey1,
            &hotkey_origin
        ));
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &coldkey2,
            &hotkey_destination
        ));

        // Add coldkey2 stake
        add_balance_to_coldkey_account(&coldkey2, 100_000_000_000u64.into());
        SubtensorModule::stake_into_subnet(
            &hotkey_origin,
            &coldkey2,
            netuid,
            50_000_000_000u64.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();
        DecayingLock::<Test>::insert(coldkey2, netuid, false);

        let lock_amount = 5000u64.into();
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey1,
            netuid,
            &hotkey_origin,
            lock_amount
        ));
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey2,
            netuid,
            &hotkey_origin,
            lock_amount
        ));

        // Mock a non-zero conviction
        let mut lock1 = Lock::<Test>::get((coldkey1, netuid, hotkey_origin)).unwrap();
        lock1.conviction = U64F64::from_num(1000);
        Lock::<Test>::insert((coldkey1, netuid, hotkey_origin), lock1);
        let mut lock2 = Lock::<Test>::get((coldkey2, netuid, hotkey_origin)).unwrap();
        lock2.conviction = U64F64::from_num(1000);
        Lock::<Test>::insert((coldkey2, netuid, hotkey_origin), lock2);
        let mut hotkey_lock = HotkeyLock::<Test>::get(netuid, hotkey_origin).unwrap();
        hotkey_lock.conviction = U64F64::from_num(2000);
        HotkeyLock::<Test>::insert(netuid, hotkey_origin, hotkey_lock);

        // Move lock for coldkey1 to hotkey_destination, coldkey2's lock should be unaffected
        assert_ok!(SubtensorModule::move_lock(
            RuntimeOrigin::signed(coldkey1),
            hotkey_destination,
            netuid,
        ));
        let lock1_after = Lock::<Test>::get((coldkey1, netuid, hotkey_destination)).unwrap();
        let lock2_after = Lock::<Test>::get((coldkey2, netuid, hotkey_origin)).unwrap();
        assert_eq!(lock1_after.locked_mass, lock_amount);
        assert_eq!(lock1_after.conviction, U64F64::from_num(0));
        assert_eq!(lock2_after.locked_mass, lock_amount);
        assert_eq!(lock2_after.conviction, U64F64::from_num(1000));

        // Hotkey lock is removed on origin and added on destination
        let hotkey_lock_origin_after = HotkeyLock::<Test>::get(netuid, hotkey_origin).unwrap();
        let hotkey_lock_destination_after =
            HotkeyLock::<Test>::get(netuid, hotkey_destination).unwrap();
        assert_eq!(hotkey_lock_origin_after.locked_mass, lock_amount);
        assert_eq!(hotkey_lock_origin_after.conviction, U64F64::from_num(1000));
        assert_eq!(hotkey_lock_destination_after.locked_mass, lock_amount);
        assert_eq!(
            hotkey_lock_destination_after.conviction,
            U64F64::from_num(0)
        );
    });
}

#[test]
fn test_moving_partial_lock_same_owners() {
    new_test_ext(1).execute_with(|| {
        let coldkey1 = U256::from(1);
        let coldkey2 = U256::from(2);
        let hotkey_origin = U256::from(3);
        let hotkey_destination = U256::from(4);
        let netuid = setup_subnet_with_stake(coldkey1, hotkey_origin, 100_000_000_000);

        // Add coldkey2 stake
        add_balance_to_coldkey_account(&coldkey2, 100_000_000_000u64.into());

        // Make hotkey_origin and hotkey_destination both owned by coldkey1
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &coldkey1,
            &hotkey_origin
        ));
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &coldkey1,
            &hotkey_destination
        ));
        SubtensorModule::stake_into_subnet(
            &hotkey_origin,
            &coldkey2,
            netuid,
            50_000_000_000u64.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();
        DecayingLock::<Test>::insert(coldkey2, netuid, false);

        let lock_amount = 5000u64.into();
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey1,
            netuid,
            &hotkey_origin,
            lock_amount
        ));
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey2,
            netuid,
            &hotkey_origin,
            lock_amount
        ));

        // Mock a non-zero conviction
        let mut lock1 = Lock::<Test>::get((coldkey1, netuid, hotkey_origin)).unwrap();
        lock1.conviction = U64F64::from_num(1000);
        Lock::<Test>::insert((coldkey1, netuid, hotkey_origin), lock1);
        let mut lock2 = Lock::<Test>::get((coldkey2, netuid, hotkey_origin)).unwrap();
        lock2.conviction = U64F64::from_num(1000);
        Lock::<Test>::insert((coldkey2, netuid, hotkey_origin), lock2);
        let mut hotkey_lock = HotkeyLock::<Test>::get(netuid, hotkey_origin).unwrap();
        hotkey_lock.conviction = U64F64::from_num(2000);
        HotkeyLock::<Test>::insert(netuid, hotkey_origin, hotkey_lock);

        // Move lock for coldkey1 to hotkey_destination, coldkey2's lock should be unaffected
        assert_ok!(SubtensorModule::move_lock(
            RuntimeOrigin::signed(coldkey1),
            hotkey_destination,
            netuid,
        ));
        let lock1_after = Lock::<Test>::get((coldkey1, netuid, hotkey_destination)).unwrap();
        let lock2_after = Lock::<Test>::get((coldkey2, netuid, hotkey_origin)).unwrap();
        assert_eq!(lock1_after.locked_mass, lock_amount);
        assert_eq!(lock1_after.conviction, U64F64::from_num(1000));
        assert_eq!(lock2_after.locked_mass, lock_amount);
        assert_eq!(lock2_after.conviction, U64F64::from_num(1000));

        // Hotkey lock is moved to destination with conviction
        let hotkey_lock_origin_after = HotkeyLock::<Test>::get(netuid, hotkey_origin).unwrap();
        let hotkey_lock_destination_after =
            HotkeyLock::<Test>::get(netuid, hotkey_destination).unwrap();
        assert_eq!(hotkey_lock_origin_after.locked_mass, lock_amount);
        assert_eq!(hotkey_lock_origin_after.conviction, U64F64::from_num(1000));
        assert_eq!(hotkey_lock_destination_after.locked_mass, lock_amount);
        assert_eq!(
            hotkey_lock_destination_after.conviction,
            U64F64::from_num(1000)
        );
    });
}

#[test]
fn test_hotkey_swap_moves_lock_and_conviction_to_new_hotkey() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let old_hotkey = U256::from(2);
        let new_hotkey = U256::from(3);
        let netuid = setup_subnet_with_stake(coldkey, old_hotkey, 100_000_000_000);
        let lock_amount: AlphaBalance = 5000u64.into();
        let conviction = U64F64::from_num(1000);

        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &old_hotkey,
            lock_amount,
        ));

        let mut lock = Lock::<Test>::get((coldkey, netuid, old_hotkey)).unwrap();
        lock.conviction = conviction;
        Lock::<Test>::insert((coldkey, netuid, old_hotkey), lock);

        let mut hotkey_lock = HotkeyLock::<Test>::get(netuid, old_hotkey).unwrap();
        hotkey_lock.conviction = conviction;
        HotkeyLock::<Test>::insert(netuid, old_hotkey, hotkey_lock);

        add_balance_to_coldkey_account(
            &coldkey,
            (SubtensorModule::get_key_swap_cost() + 1000.into()).into(),
        );
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            None,
            false,
        ));

        assert!(Lock::<Test>::get((coldkey, netuid, old_hotkey)).is_none());
        assert!(HotkeyLock::<Test>::get(netuid, old_hotkey).is_none());

        let moved_lock = Lock::<Test>::get((coldkey, netuid, new_hotkey)).unwrap();
        assert_eq!(moved_lock.locked_mass, lock_amount);
        assert_eq!(moved_lock.conviction, conviction);

        let moved_hotkey_lock = HotkeyLock::<Test>::get(netuid, new_hotkey).unwrap();
        assert_eq!(moved_hotkey_lock.locked_mass, lock_amount);
        assert_eq!(moved_hotkey_lock.conviction, conviction);
        assert_eq!(
            SubtensorModule::hotkey_conviction(&new_hotkey, netuid),
            conviction
        );
    });
}

#[test]
fn test_swap_hotkey_v2_on_subnet_moves_lock_and_conviction_to_new_hotkey() {
    new_test_ext(100).execute_with(|| {
        let coldkey = U256::from(1);
        let old_hotkey = U256::from(2);
        let new_hotkey = U256::from(3);
        let netuid = setup_subnet_with_stake(coldkey, old_hotkey, 100_000_000_000);
        let lock_amount: AlphaBalance = 5000u64.into();
        let conviction = U64F64::from_num(1000);

        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &old_hotkey,
            lock_amount,
        ));

        let mut lock = Lock::<Test>::get((coldkey, netuid, old_hotkey)).unwrap();
        lock.conviction = conviction;
        Lock::<Test>::insert((coldkey, netuid, old_hotkey), lock);

        let mut hotkey_lock = HotkeyLock::<Test>::get(netuid, old_hotkey).unwrap();
        hotkey_lock.conviction = conviction;
        HotkeyLock::<Test>::insert(netuid, old_hotkey, hotkey_lock);

        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000u64.into());
        assert_ok!(SubtensorModule::swap_hotkey_v2(
            RuntimeOrigin::signed(coldkey),
            old_hotkey,
            new_hotkey,
            Some(netuid),
            false,
        ));

        assert!(Lock::<Test>::get((coldkey, netuid, old_hotkey)).is_none());
        assert!(HotkeyLock::<Test>::get(netuid, old_hotkey).is_none());

        let moved_lock = Lock::<Test>::get((coldkey, netuid, new_hotkey)).unwrap();
        assert_eq!(moved_lock.locked_mass, lock_amount);
        assert_eq!(moved_lock.conviction, conviction);

        let moved_hotkey_lock = HotkeyLock::<Test>::get(netuid, new_hotkey).unwrap();
        assert_eq!(moved_hotkey_lock.locked_mass, lock_amount);
        assert_eq!(moved_hotkey_lock.conviction, conviction);
        assert_eq!(
            SubtensorModule::hotkey_conviction(&new_hotkey, netuid),
            conviction
        );
    });
}

#[test]
fn test_swap_hotkey_v2_on_subnet_does_not_move_locks_on_other_subnets() {
    new_test_ext(100).execute_with(|| {
        let coldkey = U256::from(1);
        let old_hotkey = U256::from(2);
        let new_hotkey = U256::from(3);
        let swapped_netuid = setup_subnet_with_stake(coldkey, old_hotkey, 100_000_000_000);
        let untouched_netuid = setup_subnet_with_stake(coldkey, old_hotkey, 100_000_000_000);
        let lock_amount: AlphaBalance = 5000u64.into();
        let conviction = U64F64::from_num(1000);

        for netuid in [swapped_netuid, untouched_netuid] {
            assert_ok!(SubtensorModule::do_lock_stake(
                &coldkey,
                netuid,
                &old_hotkey,
                lock_amount,
            ));

            let mut lock = Lock::<Test>::get((coldkey, netuid, old_hotkey)).unwrap();
            lock.conviction = conviction;
            Lock::<Test>::insert((coldkey, netuid, old_hotkey), lock);

            let mut hotkey_lock = HotkeyLock::<Test>::get(netuid, old_hotkey).unwrap();
            hotkey_lock.conviction = conviction;
            HotkeyLock::<Test>::insert(netuid, old_hotkey, hotkey_lock);
        }

        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000u64.into());
        assert_ok!(SubtensorModule::swap_hotkey_v2(
            RuntimeOrigin::signed(coldkey),
            old_hotkey,
            new_hotkey,
            Some(swapped_netuid),
            false,
        ));

        assert!(Lock::<Test>::get((coldkey, swapped_netuid, old_hotkey)).is_none());
        assert!(HotkeyLock::<Test>::get(swapped_netuid, old_hotkey).is_none());
        assert_eq!(
            Lock::<Test>::get((coldkey, swapped_netuid, new_hotkey))
                .unwrap()
                .conviction,
            conviction
        );
        assert_eq!(
            HotkeyLock::<Test>::get(swapped_netuid, new_hotkey)
                .unwrap()
                .conviction,
            conviction
        );

        let untouched_lock = Lock::<Test>::get((coldkey, untouched_netuid, old_hotkey)).unwrap();
        assert_eq!(untouched_lock.locked_mass, lock_amount);
        assert_eq!(untouched_lock.conviction, conviction);
        assert!(Lock::<Test>::get((coldkey, untouched_netuid, new_hotkey)).is_none());

        let untouched_hotkey_lock = HotkeyLock::<Test>::get(untouched_netuid, old_hotkey).unwrap();
        assert_eq!(untouched_hotkey_lock.locked_mass, lock_amount);
        assert_eq!(untouched_hotkey_lock.conviction, conviction);
        assert!(HotkeyLock::<Test>::get(untouched_netuid, new_hotkey).is_none());
    });
}
