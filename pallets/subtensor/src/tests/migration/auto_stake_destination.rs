#![allow(
    unused,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]
//! auto-stake destination migration.

use super::helpers::*;
use super::prelude::*;

#[test]
fn test_migrate_auto_stake_destination() {
    new_test_ext(1).execute_with(|| {
        // ------------------------------
        // Step 1: Simulate Old Storage Entries
        // ------------------------------
        const MIGRATION_NAME: &[u8] = b"migrate_auto_stake_destination";
		let netuids = [NetUid::ROOT, NetUid::from(1), NetUid::from(2), NetUid::from(42)];
		for netuid in &netuids {
			NetworksAdded::<Test>::insert(*netuid, true);
		}

        let pallet_prefix = twox_128("SubtensorModule".as_bytes());
        let storage_prefix = twox_128("AutoStakeDestination".as_bytes());

        // Create test accounts
        let coldkey1: U256 = U256::from(1);
        let coldkey2: U256 = U256::from(2);
        let hotkey1: U256 = U256::from(100);
        let hotkey2: U256 = U256::from(200);

        // Construct storage keys for old format (StorageMap)
        let mut key1 = Vec::new();
        key1.extend_from_slice(&pallet_prefix);
        key1.extend_from_slice(&storage_prefix);
        key1.extend_from_slice(&Blake2_128Concat::hash(&coldkey1.encode()));

        let mut key2 = Vec::new();
        key2.extend_from_slice(&pallet_prefix);
        key2.extend_from_slice(&storage_prefix);
        key2.extend_from_slice(&Blake2_128Concat::hash(&coldkey2.encode()));

        // Store old format entries
        put_raw(&key1, &hotkey1.encode());
        put_raw(&key2, &hotkey2.encode());

        // Verify old entries are stored
        assert_eq!(get_raw(&key1), Some(hotkey1.encode()));
        assert_eq!(get_raw(&key2), Some(hotkey2.encode()));

        assert!(
            !HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()),
            "Migration should not have run yet"
        );

        // ------------------------------
        // Step 2: Run the Migration
        // ------------------------------
        let weight = crate::migrations::migrate_auto_stake_destination::migrate_auto_stake_destination::<Test>();

        assert!(
            HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()),
            "Migration should be marked as run"
        );

        // ------------------------------
        // Step 3: Verify Migration Effects
        // ------------------------------

        // Verify new format entries exist
		for netuid in &netuids {
			if *netuid == NetUid::ROOT {
				assert_eq!(
					AutoStakeDestination::<Test>::get(coldkey1, NetUid::ROOT),
					None
				);
				assert_eq!(
					AutoStakeDestination::<Test>::get(coldkey2, NetUid::ROOT),
					None
				);
			} else {
				assert_eq!(
					AutoStakeDestination::<Test>::get(coldkey1, *netuid),
					Some(hotkey1)
				);
				assert_eq!(
					AutoStakeDestination::<Test>::get(coldkey2, *netuid),
					Some(hotkey2)
				);

				// Verify entry for AutoStakeDestinationColdkeys
				assert_eq!(
					AutoStakeDestinationColdkeys::<Test>::get(hotkey1, *netuid),
					vec![coldkey1]
				);
				assert_eq!(
					AutoStakeDestinationColdkeys::<Test>::get(hotkey2, *netuid),
					vec![coldkey2]
				);
			}
		}

        // Verify old format entries are cleared
        assert_eq!(get_raw(&key1), None, "Old storage entry 1 should be cleared");
        assert_eq!(get_raw(&key2), None, "Old storage entry 2 should be cleared");

        // Verify weight calculation
        assert!(!weight.is_zero(), "Migration weight should be non-zero");

        // ------------------------------
        // Step 4: Test Migration Idempotency
        // ------------------------------
        let weight_second_run = crate::migrations::migrate_auto_stake_destination::migrate_auto_stake_destination::<Test>();

        // Second run should only read the migration flag
        assert_eq!(
            weight_second_run,
            <Test as Config>::DbWeight::get().reads(1),
            "Second run should only read the migration flag"
        );
    });
}
