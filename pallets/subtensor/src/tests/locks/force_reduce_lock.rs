#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
//! Lock force-reduction.

use super::helpers::*;
use super::prelude::*;

// =========================================================================
// GROUP 10: Lock force-reduction
// =========================================================================

#[test]
fn test_reduce_lock_removes_dust() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);
        let lock_amount = AlphaBalance::from(50u64);

        // Lock a small amount
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey,
            lock_amount,
        ));

        // Advance many taus so everything decays well below dust (100)
        let tau = UnlockRate::<Test>::get();
        let target = System::block_number() + tau * 50;
        System::set_block_number(target);

        // Remove full lock amount
        SubtensorModule::force_reduce_lock(&coldkey, netuid, lock_amount);

        assert!(Lock::<Test>::get((coldkey, netuid, hotkey)).is_none());
        assert!(HotkeyLock::<Test>::get(netuid, hotkey).is_none());
    });
}

#[test]
fn test_reduce_lock_partial_reduction() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);
        let lock_amount = AlphaBalance::from(1_000u64);
        let reduce_amount = AlphaBalance::from(400u64);
        let now = SubtensorModule::get_current_block_as_u64();

        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey,
            lock_amount,
        ));

        let conviction = U64F64::from_num(1_000);
        Lock::<Test>::insert(
            (coldkey, netuid, hotkey),
            LockState {
                locked_mass: lock_amount,
                conviction,
                last_update: now,
            },
        );
        HotkeyLock::<Test>::insert(
            netuid,
            hotkey,
            LockState {
                locked_mass: lock_amount,
                conviction,
                last_update: now,
            },
        );

        SubtensorModule::force_reduce_lock(&coldkey, netuid, reduce_amount);

        let lock = Lock::<Test>::get((coldkey, netuid, hotkey)).expect("lock should remain");
        assert_eq!(lock.locked_mass, 600u64.into());
        assert_abs_diff_eq!(
            lock.conviction.to_num::<f64>(),
            600.,
            epsilon = 0.0000000001
        );

        let hotkey_lock =
            HotkeyLock::<Test>::get(netuid, hotkey).expect("hotkey lock should remain");
        assert_eq!(hotkey_lock.locked_mass, 600u64.into());
        assert_abs_diff_eq!(
            hotkey_lock.conviction.to_num::<f64>(),
            600.,
            epsilon = 0.0000000001
        );
    });
}

#[test]
fn test_reduce_lock_no_lock() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let netuid = subtensor_runtime_common::NetUid::from(1);
        // Should be a no-op, no panic
        SubtensorModule::force_reduce_lock(&coldkey, netuid, 100u64.into());
        assert!(
            Lock::<Test>::iter_prefix((coldkey, netuid))
                .next()
                .is_none()
        );
    });
}

#[test]
fn test_reduce_lock_two_coldkeys() {
    new_test_ext(1).execute_with(|| {
        let coldkey1 = U256::from(1);
        let coldkey2 = U256::from(3);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey1, hotkey, 100_000_000_000);

        // Add stake on coldkey 2
        add_balance_to_coldkey_account(&coldkey2, 100_000_000_000u64.into());
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &coldkey2, &hotkey
        ));
        SubtensorModule::stake_into_subnet(
            &hotkey,
            &coldkey2,
            netuid,
            100_000_000_000u64.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();
        DecayingLock::<Test>::insert(coldkey2, netuid, false);

        // Mock a non-zero conviction for both coldkeys
        let lock1 = Lock::<Test>::get((coldkey1, netuid, hotkey)).unwrap_or(LockState {
            locked_mass: 0.into(),
            conviction: U64F64::from_num(1234),
            last_update: System::block_number(),
        });
        let lock2 = Lock::<Test>::get((coldkey2, netuid, hotkey)).unwrap_or(LockState {
            locked_mass: 0.into(),
            conviction: U64F64::from_num(1234),
            last_update: System::block_number(),
        });
        Lock::<Test>::insert((coldkey1, netuid, hotkey), lock1);
        Lock::<Test>::insert((coldkey2, netuid, hotkey), lock2);
        HotkeyLock::<Test>::insert(
            netuid,
            hotkey,
            LockState {
                locked_mass: 0.into(),
                conviction: U64F64::from_num(1234 * 2),
                last_update: System::block_number(),
            },
        );

        // Lock a small amount from both coldkeys
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey1,
            netuid,
            &hotkey,
            50u64.into(),
        ));
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey2,
            netuid,
            &hotkey,
            50u64.into(),
        ));

        SubtensorModule::force_reduce_lock(&coldkey1, netuid, 50u64.into());

        // Should only clean up coldkey1's lock, not coldkey2's
        assert!(
            Lock::<Test>::iter_prefix((coldkey1, netuid))
                .next()
                .is_none()
        );
        assert!(Lock::<Test>::get((coldkey2, netuid, hotkey)).is_some());

        // Hotkey lock should reduce according to coldkey1 lock
        let hotkey_lock = HotkeyLock::<Test>::get(netuid, hotkey).unwrap();
        assert_eq!(hotkey_lock.locked_mass, 50u64.into());

        // Conviction should be reduced by coldkey1's lock conviction,
        // but not fully reset because coldkey2 still has a lock
        assert!(hotkey_lock.conviction == U64F64::from_num(1234));
    });
}

#[test]
fn test_force_reduce_lock_does_not_over_reduce_hotkey_lock() {
    new_test_ext(1).execute_with(|| {
        let coldkey1 = U256::from(1);
        let coldkey2 = U256::from(3);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey1, hotkey, 100_000_000_000);
        let now = SubtensorModule::get_current_block_as_u64();

        Lock::<Test>::insert(
            (coldkey1, netuid, hotkey),
            LockState {
                locked_mass: 1_000u64.into(),
                conviction: U64F64::from_num(1_000),
                last_update: now,
            },
        );
        Lock::<Test>::insert(
            (coldkey2, netuid, hotkey),
            LockState {
                locked_mass: 5_000u64.into(),
                conviction: U64F64::from_num(2_000),
                last_update: now,
            },
        );
        HotkeyLock::<Test>::insert(
            netuid,
            hotkey,
            LockState {
                locked_mass: 6_000u64.into(),
                conviction: U64F64::from_num(3_000),
                last_update: now,
            },
        );

        SubtensorModule::force_reduce_lock(&coldkey1, netuid, 2_000u64.into());

        assert!(Lock::<Test>::get((coldkey1, netuid, hotkey)).is_none());
        assert!(Lock::<Test>::get((coldkey2, netuid, hotkey)).is_some());

        let hotkey_lock =
            HotkeyLock::<Test>::get(netuid, hotkey).expect("hotkey lock should remain");
        assert_eq!(hotkey_lock.locked_mass, 5_000u64.into());
        assert_eq!(hotkey_lock.conviction, U64F64::from_num(2_000));
    });
}
