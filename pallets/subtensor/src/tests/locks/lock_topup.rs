#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
//! Incremental locks (top-up).

use super::helpers::*;
use super::prelude::*;

// =========================================================================
// GROUP 3: Incremental locks (top-up)
// =========================================================================

#[test]
fn test_lock_stake_topup() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        let first_lock = 1000u64;
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey,
            first_lock.into()
        ));

        step_block(100);

        let second_lock = 500u64;
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey,
            second_lock.into()
        ));

        let lock = Lock::<Test>::get((coldkey, netuid, hotkey)).unwrap();
        // locked_mass should be decayed(first_lock) + second_lock
        // Since tau is large (216000), decay over 100 blocks is small; locked_mass ~ 1000 + 500
        assert!(lock.locked_mass > 1490.into());
        assert!(lock.locked_mass < 1501.into());
        // conviction should have grown from the time the first lock was active
        assert!(lock.conviction > U64F64::from_num(0));
        assert_eq!(
            lock.last_update,
            SubtensorModule::get_current_block_as_u64()
        );

        // Hotkey lock should also be created
        let hotkey_lock = HotkeyLock::<Test>::get(netuid, hotkey).unwrap();
        assert!(hotkey_lock.locked_mass > 1490.into());
        assert_eq!(hotkey_lock.locked_mass, lock.locked_mass);
        assert!(hotkey_lock.conviction > U64F64::from_num(0));
    });
}

#[test]
fn test_lock_stake_topup_multiple_times() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        let chunk = 500u64.into();

        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey, netuid, &hotkey, chunk
        ));
        step_block(50);
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey, netuid, &hotkey, chunk
        ));
        step_block(50);
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey, netuid, &hotkey, chunk
        ));

        let lock = Lock::<Test>::get((coldkey, netuid, hotkey)).unwrap();
        // After three top-ups with small decay, should be close to 1500
        assert!(lock.locked_mass > 1490.into());
        assert!(lock.locked_mass <= 1500.into());
        assert!(lock.conviction > U64F64::from_num(0));

        // Hotkey lock should also be updated
        let hotkey_lock = HotkeyLock::<Test>::get(netuid, hotkey).unwrap();
        assert!(hotkey_lock.locked_mass > 1490.into());
        assert_eq!(hotkey_lock.locked_mass, lock.locked_mass);
        assert!(hotkey_lock.conviction > U64F64::from_num(0));
    });
}

#[test]
fn test_lock_stake_topup_same_block() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        let first = 1000u64.into();
        let second = 500u64.into();

        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey, netuid, &hotkey, first
        ));
        // No block advancement — same block top-up
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey, netuid, &hotkey, second
        ));

        let lock = Lock::<Test>::get((coldkey, netuid, hotkey)).unwrap();
        // dt=0 means no decay, simple addition
        assert_eq!(lock.locked_mass, first + second);
        assert_eq!(lock.conviction, U64F64::from_num(0));

        // Hotkey lock should also be updated
        let hotkey_lock = HotkeyLock::<Test>::get(netuid, hotkey).unwrap();
        assert_eq!(hotkey_lock.locked_mass, first + second);
        assert_eq!(hotkey_lock.conviction, U64F64::from_num(0));
    });
}

#[test]
fn test_locking_coldkeys_added_once_by_lock_stake() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey,
            100u64.into(),
        ));
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey,
            50u64.into(),
        ));

        assert!(LockingColdkeys::<Test>::contains_key((
            netuid, hotkey, coldkey
        )));
        assert_eq!(
            LockingColdkeys::<Test>::iter_prefix((netuid, hotkey)).count(),
            1
        );
    });
}

#[test]
fn test_locking_coldkeys_removed_when_lock_is_fully_reduced() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);
        let amount = 100u64.into();

        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey, netuid, &hotkey, amount
        ));
        assert!(LockingColdkeys::<Test>::contains_key((
            netuid, hotkey, coldkey
        )));

        SubtensorModule::force_reduce_lock(&coldkey, netuid, amount);

        assert!(Lock::<Test>::get((coldkey, netuid, hotkey)).is_none());
        assert!(!LockingColdkeys::<Test>::contains_key((
            netuid, hotkey, coldkey
        )));
    });
}

#[test]
fn test_lock_state_is_zero_uses_dust_threshold() {
    let below_threshold = LockState {
        locked_mass: AlphaBalance::from(99u64),
        conviction: U64F64::from_num(99),
        last_update: 0,
    };
    let locked_mass_at_threshold = LockState {
        locked_mass: AlphaBalance::from(100u64),
        conviction: U64F64::from_num(99),
        last_update: 0,
    };
    let conviction_at_threshold = LockState {
        locked_mass: AlphaBalance::from(99u64),
        conviction: U64F64::from_num(100),
        last_update: 0,
    };

    assert!(below_threshold.is_zero());
    assert!(!locked_mass_at_threshold.is_zero());
    assert!(!conviction_at_threshold.is_zero());
}
