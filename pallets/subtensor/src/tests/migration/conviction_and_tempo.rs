#![allow(
    unused,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]
//! tnet conviction locks + dynamic tempo.

use super::helpers::*;
use super::prelude::*;

#[test]
fn test_migrate_reset_tnet_conviction_locks() {
    new_test_ext(1).execute_with(|| {
        const MIGRATION_NAME: &[u8] = b"migrate_reset_tnet_conviction_locks";

        let netuid = NetUid::from(1);
        let other_netuid = NetUid::from(2);
        let coldkey_1 = U256::from(1001);
        let coldkey_2 = U256::from(1002);
        let hotkey_1 = U256::from(2001);
        let hotkey_2 = U256::from(2002);

        let lock_1 = LockState {
            locked_mass: AlphaBalance::from(10_u64),
            conviction: U64F64::from_num(1.5),
            last_update: 11,
        };
        let lock_2 = LockState {
            locked_mass: AlphaBalance::from(20_u64),
            conviction: U64F64::from_num(2.5),
            last_update: 22,
        };

        Lock::<Test>::insert((coldkey_1, netuid, hotkey_1), lock_1.clone());
        Lock::<Test>::insert((coldkey_2, other_netuid, hotkey_2), lock_2.clone());
        HotkeyLock::<Test>::insert(netuid, hotkey_1, lock_1.clone());
        DecayingHotkeyLock::<Test>::insert(other_netuid, hotkey_2, lock_2.clone());
        OwnerLock::<Test>::insert(netuid, lock_1.clone());
        DecayingOwnerLock::<Test>::insert(other_netuid, lock_2.clone());
        DecayingLock::<Test>::insert(coldkey_1, netuid, false);
        DecayingLock::<Test>::insert(coldkey_2, other_netuid, false);

        assert!(!HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));
        assert_eq!(Lock::<Test>::iter().count(), 2);
        assert_eq!(HotkeyLock::<Test>::iter().count(), 1);
        assert_eq!(DecayingHotkeyLock::<Test>::iter().count(), 1);
        assert_eq!(OwnerLock::<Test>::iter().count(), 1);
        assert_eq!(DecayingOwnerLock::<Test>::iter().count(), 1);
        assert_eq!(DecayingLock::<Test>::iter().count(), 2);

        let raw_owner_lock_key = {
            let mut key = Vec::new();
            key.extend_from_slice(&twox_128("SubtensorModule".as_bytes()));
            key.extend_from_slice(&twox_128("OwnerLock".as_bytes()));
            key.extend_from_slice(&NetUid::from(99).encode());
            key
        };
        let raw_decaying_hotkey_lock_key = {
            let mut key = Vec::new();
            key.extend_from_slice(&twox_128("SubtensorModule".as_bytes()));
            key.extend_from_slice(&twox_128("DecayingHotkeyLock".as_bytes()));
            key.extend_from_slice(&NetUid::from(100).encode());
            key.extend_from_slice(&Blake2_128Concat::hash(&U256::from(3003).encode()));
            key
        };

        // Simulate deprecated aggregate entries with bytes that the current
        // `LockState` type should never need to decode during this reset.
        put_raw(&raw_owner_lock_key, &123_u32.encode());
        put_raw(&raw_decaying_hotkey_lock_key, &(456_u32, 789_u32).encode());
        assert!(get_raw(&raw_owner_lock_key).is_some());
        assert!(get_raw(&raw_decaying_hotkey_lock_key).is_some());

        let weight =
            crate::migrations::migrate_reset_tnet_conviction_locks::migrate_reset_tnet_conviction_locks::<Test>();

        assert!(!weight.is_zero(), "migration weight should be non-zero");
        assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));
        assert!(get_raw(&raw_owner_lock_key).is_none());
        assert!(get_raw(&raw_decaying_hotkey_lock_key).is_none());
        assert_eq!(Lock::<Test>::iter().count(), 0);
        assert_eq!(HotkeyLock::<Test>::iter().count(), 0);
        assert_eq!(DecayingHotkeyLock::<Test>::iter().count(), 0);
        assert_eq!(OwnerLock::<Test>::iter().count(), 0);
        assert_eq!(DecayingOwnerLock::<Test>::iter().count(), 0);
        assert_eq!(DecayingLock::<Test>::iter().count(), 0);

        Lock::<Test>::insert((coldkey_1, netuid, hotkey_1), lock_1);
        let second_weight =
            crate::migrations::migrate_reset_tnet_conviction_locks::migrate_reset_tnet_conviction_locks::<Test>();

        assert_eq!(
            second_weight,
            <Test as frame_system::Config>::DbWeight::get().reads(1),
            "second run should only read the migration flag"
        );
        assert_eq!(
            Lock::<Test>::iter().count(),
            1,
            "migration must not run more than once"
        );
    });
}

#[test]
fn test_migrate_dynamic_tempo_aligns_first_post_upgrade_fire() {
    new_test_ext(1).execute_with(|| {
        const MIGRATION_NAME: &str = "dynamic_tempo_v1";
        let netuid = NetUid::from(7u16);
        let tempo: u16 = 360;

        add_network(netuid, tempo, 0);
        let current_block = 1234u64;
        run_to_block(current_block);

        // Compute next-fire block
        let netuid_plus_one = (u16::from(netuid) as u64) + 1;
        let tempo_plus_one = (tempo as u64) + 1;
        let adjusted = current_block + netuid_plus_one;
        let remainder = adjusted % tempo_plus_one;
        let legacy_blocks_until_next = (tempo as u64) - remainder;
        let expected_next_fire = current_block + legacy_blocks_until_next;

        crate::migrations::migrate_dynamic_tempo::migrate_dynamic_tempo::<Test>();

        // New formula: next fire = LastEpochBlock + tempo.
        let last_epoch = LastEpochBlock::<Test>::get(netuid);
        assert_eq!(
            last_epoch + tempo as u64,
            expected_next_fire,
            "back-fill should make new scheduler fire at the same block as legacy modulo"
        );
        assert!(HasMigrationRun::<Test>::get(
            MIGRATION_NAME.as_bytes().to_vec()
        ));
    });
}

#[test]
fn test_migrate_dynamic_tempo_preserves_non_standard_tempo() {
    new_test_ext(1).execute_with(|| {
        // Three subnets — one standard, two with non-standard tempo
        // (simulates the 2 mainnet subnets root configured outside MIN/MAX bounds).
        let standard = NetUid::from(1u16);
        let small = NetUid::from(2u16);
        let large = NetUid::from(3u16);

        add_network(standard, 360, 0);
        add_network(small, 10, 0); // < MIN_TEMPO (360)
        add_network(large, 60_000, 0); // > MAX_TEMPO (50_400)

        crate::migrations::migrate_dynamic_tempo::migrate_dynamic_tempo::<Test>();

        // Tempo values preserved as-is — no clamp.
        assert_eq!(Tempo::<Test>::get(standard), 360);
        assert_eq!(Tempo::<Test>::get(small), 10);
        assert_eq!(Tempo::<Test>::get(large), 60_000);

        // All non-zero tempos got LastEpochBlock seeded.
        assert!(LastEpochBlock::<Test>::contains_key(standard));
        assert!(LastEpochBlock::<Test>::contains_key(small));
        assert!(LastEpochBlock::<Test>::contains_key(large));
    });
}

#[test]
fn test_migrate_dynamic_tempo_activity_cutoff_round_trips_production_values() {
    new_test_ext(1).execute_with(|| {
        // (cutoff_blocks, tempo) combinations from production data.
        let cases: [(u16, u16); 6] = [
            (5000, 360),
            (6000, 360),
            (7200, 360),
            (12000, 360),
            (1000, 360),
            (360, 360),
        ];

        for (i, &(cutoff, tempo)) in cases.iter().enumerate() {
            let netuid = NetUid::from((i + 1) as u16);
            add_network(netuid, tempo, 0);
            ActivityCutoff::<Test>::insert(netuid, cutoff);
        }

        crate::migrations::migrate_dynamic_tempo::migrate_dynamic_tempo::<Test>();

        for (i, &(cutoff, _)) in cases.iter().enumerate() {
            let netuid = NetUid::from((i + 1) as u16);
            // get_activity_cutoff_blocks = factor * tempo / 1000 must equal original cutoff exactly.
            assert_eq!(
                crate::Pallet::<Test>::get_activity_cutoff_blocks(netuid),
                cutoff as u64,
                "ceiling division must round-trip cutoff exactly for netuid {}",
                u16::from(netuid)
            );
        }
    });
}

#[test]
fn test_migrate_dynamic_tempo_idempotent() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1u16);
        add_network(netuid, 360, 0);

        crate::migrations::migrate_dynamic_tempo::migrate_dynamic_tempo::<Test>();
        let last_epoch_first = LastEpochBlock::<Test>::get(netuid);

        // Mutate state to verify second run is a no-op.
        run_to_block(crate::Pallet::<Test>::get_current_block_as_u64() + 100);
        crate::migrations::migrate_dynamic_tempo::migrate_dynamic_tempo::<Test>();

        assert_eq!(
            LastEpochBlock::<Test>::get(netuid),
            last_epoch_first,
            "second migration call must be a no-op"
        );
    });
}
