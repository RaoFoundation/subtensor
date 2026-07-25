#![allow(
    unused,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]
//! populate LockingColdkeys aggregate.

use super::helpers::*;
use super::prelude::*;

#[test]
fn test_migrate_populate_locking_coldkeys() {
    new_test_ext(1).execute_with(|| {
        const MIGRATION_NAME: &[u8] = b"migrate_populate_locking_coldkeys";

        let netuid = NetUid::from(1);
        let coldkey_1 = U256::from(1001);
        let coldkey_2 = U256::from(1002);
        let hotkey = U256::from(2001);
        let expired_hotkey = U256::from(2002);

        Lock::<Test>::insert(
            (coldkey_1, netuid, hotkey),
            LockState {
                locked_mass: AlphaBalance::from(1_000_u64),
                conviction: U64F64::from_num(0),
                last_update: 1,
            },
        );
        Lock::<Test>::insert(
            (coldkey_2, netuid, hotkey),
            LockState {
                locked_mass: AlphaBalance::from(2_000_u64),
                conviction: U64F64::from_num(0),
                last_update: 1,
            },
        );
        Lock::<Test>::insert(
            (coldkey_1, netuid, expired_hotkey),
            LockState {
                locked_mass: AlphaBalance::ZERO,
                conviction: U64F64::from_num(1),
                last_update: 1,
            },
        );

        assert_eq!(
            LockingColdkeys::<Test>::iter_prefix((netuid, hotkey)).count(),
            0
        );
        assert_eq!(
            LockingColdkeys::<Test>::iter_prefix((netuid, expired_hotkey)).count(),
            0
        );
        assert!(!HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));

        let weight =
            crate::migrations::migrate_populate_locking_coldkeys::migrate_populate_locking_coldkeys::<Test>();

        assert!(!weight.is_zero(), "migration weight should be non-zero");
        assert!(LockingColdkeys::<Test>::contains_key((
            netuid, hotkey, coldkey_1
        )));
        assert!(LockingColdkeys::<Test>::contains_key((
            netuid, hotkey, coldkey_2
        )));
        assert_eq!(
            LockingColdkeys::<Test>::iter_prefix((netuid, hotkey)).count(),
            2
        );
        assert_eq!(
            LockingColdkeys::<Test>::iter_prefix((netuid, expired_hotkey)).count(),
            0
        );
        assert!(Lock::<Test>::get((coldkey_1, netuid, expired_hotkey)).is_none());
        assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));

        let _ = LockingColdkeys::<Test>::clear_prefix((netuid, hotkey), u32::MAX, None);
        let second_weight =
            crate::migrations::migrate_populate_locking_coldkeys::migrate_populate_locking_coldkeys::<Test>();

        assert_eq!(
            second_weight,
            <Test as frame_system::Config>::DbWeight::get().reads(1),
            "second run should only read the migration flag"
        );
        assert_eq!(
            LockingColdkeys::<Test>::iter_prefix((netuid, hotkey)).count(),
            0
        );
    });
}

#[test]
fn test_migrate_populate_locking_coldkeys_removes_dust_from_aggregate() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let coldkey_1 = U256::from(1101);
        let coldkey_2 = U256::from(1102);
        let hotkey = U256::from(2101);
        let dust_lock = LockState {
            locked_mass: AlphaBalance::from(60_u64),
            conviction: U64F64::from_num(0),
            last_update: 1,
        };

        DecayingLock::<Test>::insert(coldkey_1, netuid, false);
        DecayingLock::<Test>::insert(coldkey_2, netuid, false);
        Lock::<Test>::insert((coldkey_1, netuid, hotkey), dust_lock.clone());
        Lock::<Test>::insert((coldkey_2, netuid, hotkey), dust_lock);
        HotkeyLock::<Test>::insert(
            netuid,
            hotkey,
            LockState {
                locked_mass: AlphaBalance::from(120_u64),
                conviction: U64F64::from_num(0),
                last_update: 1,
            },
        );

        crate::migrations::migrate_populate_locking_coldkeys::migrate_populate_locking_coldkeys::<
            Test,
        >();

        assert!(Lock::<Test>::get((coldkey_1, netuid, hotkey)).is_none());
        assert!(Lock::<Test>::get((coldkey_2, netuid, hotkey)).is_none());
        assert!(HotkeyLock::<Test>::get(netuid, hotkey).is_none());
        assert_eq!(
            LockingColdkeys::<Test>::iter_prefix((netuid, hotkey)).count(),
            0
        );
    });
}
