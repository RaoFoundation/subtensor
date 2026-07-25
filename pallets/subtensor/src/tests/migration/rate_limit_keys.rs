#![allow(
    unused,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]
//! rate-limit key migrations and last-tx block maps.

use super::helpers::*;
use super::prelude::*;

#[test]
fn test_migrate_network_last_registered() {
    new_test_ext(1).execute_with(|| {
        // ------------------------------
        // Step 1: Simulate Old Storage Entry
        // ------------------------------
        const MIGRATION_NAME: &str = "migrate_network_last_registered";

        let pallet_name = "SubtensorModule";
        let storage_name = "NetworkLastRegistered";
        let pallet_name_hash = twox_128(pallet_name.as_bytes());
        let storage_name_hash = twox_128(storage_name.as_bytes());
        let prefix = [pallet_name_hash, storage_name_hash].concat();

        let mut full_key = prefix.clone();

        let original_value: u64 = 123;
        put_raw(&full_key, &original_value.encode());

        let stored_before = get_raw(&full_key).expect("Expected RateLimit to exist");
        assert_eq!(
            u64::decode(&mut &stored_before[..]).expect("Failed to decode RateLimit"),
            original_value
        );

        assert!(
            !HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should not have run yet"
        );

        // ------------------------------
        // Step 2: Run the Migration
        // ------------------------------
        let weight = crate::migrations::migrate_rate_limiting_last_blocks::
        migrate_obsolete_rate_limiting_last_blocks_storage::<Test>();

        assert!(
            HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should be marked as completed"
        );

        // ------------------------------
        // Step 3: Verify Migration Effects
        // ------------------------------

        assert_eq!(
            SubtensorModule::get_network_last_lock_block(),
            original_value
        );
        assert_eq!(
            get_raw(&full_key),
            None,
            "RateLimit storage should have been cleared"
        );

        assert!(!weight.is_zero(), "Migration weight should be non-zero");
    });
}

#[allow(deprecated)]
#[test]
fn test_migrate_last_block_tx() {
    new_test_ext(1).execute_with(|| {
        // ------------------------------
        // Step 1: Simulate Old Storage Entry
        // ------------------------------
        const MIGRATION_NAME: &str = "migrate_last_tx_block";

        let test_account: U256 = U256::from(1);
        let original_value: u64 = 123;

        LastTxBlock::<Test>::insert(test_account, original_value);

        assert!(
            !HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should not have run yet"
        );

        // ------------------------------
        // Step 2: Run the Migration
        // ------------------------------
        let weight = crate::migrations::migrate_rate_limiting_last_blocks::
        migrate_obsolete_rate_limiting_last_blocks_storage::<Test>();

        assert!(
            HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should be marked as completed"
        );

        // ------------------------------
        // Step 3: Verify Migration Effects
        // ------------------------------

        assert_eq!(
            SubtensorModule::get_last_tx_block(&test_account),
            original_value
        );
        assert!(
            !LastTxBlock::<Test>::contains_key(test_account),
            "RateLimit storage should have been cleared"
        );

        assert!(!weight.is_zero(), "Migration weight should be non-zero");
    });
}

#[allow(deprecated)]
#[test]
fn test_migrate_last_tx_block_childkey_take() {
    new_test_ext(1).execute_with(|| {
        // ------------------------------
        // Step 1: Simulate Old Storage Entry
        // ------------------------------
        const MIGRATION_NAME: &str = "migrate_last_tx_block_childkey_take";

        let test_account: U256 = U256::from(1);
        let original_value: u64 = 123;

        LastTxBlockChildKeyTake::<Test>::insert(test_account, original_value);

        assert!(
            !HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should not have run yet"
        );

        // ------------------------------
        // Step 2: Run the Migration
        // ------------------------------
        let weight = crate::migrations::migrate_rate_limiting_last_blocks::
        migrate_obsolete_rate_limiting_last_blocks_storage::<Test>();

        assert!(
            HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should be marked as completed"
        );

        // ------------------------------
        // Step 3: Verify Migration Effects
        // ------------------------------

        assert_eq!(
            SubtensorModule::get_last_tx_block_childkey_take(&test_account),
            original_value
        );
        assert!(
            !LastTxBlockChildKeyTake::<Test>::contains_key(test_account),
            "RateLimit storage should have been cleared"
        );

        assert!(!weight.is_zero(), "Migration weight should be non-zero");
    });
}

// PerU16 must SCALE-encode byte-identically to u16, so the take/epoch storages
// retyped from u16 to PerU16 require no storage migration.
#[test]
fn test_per_u16_encodes_identically_to_u16() {
    assert_eq!(PerU16::from_parts(5).encode(), 5u16.encode());
    assert_eq!(PerU16::from_parts(u16::MAX).encode(), u16::MAX.encode());
    assert_eq!(PerU16::zero().encode(), 0u16.encode());
}

#[allow(deprecated)]
#[test]
fn test_migrate_last_tx_block_delegate_take() {
    new_test_ext(1).execute_with(|| {
        // ------------------------------
        // Step 1: Simulate Old Storage Entry
        // ------------------------------
        const MIGRATION_NAME: &str = "migrate_last_tx_block_delegate_take";

        let test_account: U256 = U256::from(1);
        let original_value: u64 = 123;

        LastTxBlockDelegateTake::<Test>::insert(test_account, original_value);

        assert!(
            !HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should not have run yet"
        );

        // ------------------------------
        // Step 2: Run the Migration
        // ------------------------------
        let weight = crate::migrations::migrate_rate_limiting_last_blocks::
        migrate_last_tx_block_delegate_take::<Test>();

        assert!(
            HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should be marked as completed"
        );

        // ------------------------------
        // Step 3: Verify Migration Effects
        // ------------------------------

        assert_eq!(
            SubtensorModule::get_last_tx_block_delegate_take(&test_account),
            original_value
        );
        assert!(
            !LastTxBlockDelegateTake::<Test>::contains_key(test_account),
            "RateLimit storage should have been cleared"
        );

        assert!(!weight.is_zero(), "Migration weight should be non-zero");
    });
}

#[test]
fn test_migrate_rate_limit_keys() {
    new_test_ext(1).execute_with(|| {
        const MIGRATION_NAME: &[u8] = b"migrate_rate_limit_keys";
        let prefix = {
            let pallet_prefix = twox_128("SubtensorModule".as_bytes());
            let storage_prefix = twox_128("LastRateLimitedBlock".as_bytes());
            [pallet_prefix, storage_prefix].concat()
        };

        // Seed new-format entries that must survive the migration untouched.
        let new_last_account = U256::from(10);
        SubtensorModule::set_last_tx_block(&new_last_account, 555);
        let new_child_account = U256::from(11);
        SubtensorModule::set_last_tx_block_childkey(&new_child_account, 777);
        let new_delegate_account = U256::from(12);
        SubtensorModule::set_last_tx_block_delegate_take(&new_delegate_account, 888);

        // Legacy NetworkLastRegistered entry (index 1)
        let mut legacy_network_key = prefix.clone();
        legacy_network_key.push(1u8);
        sp_io::storage::set(&legacy_network_key, &111u64.encode());

        // Legacy LastTxBlock entry (index 2) for an account that already has a new-format value.
        let mut legacy_last_key = prefix.clone();
        legacy_last_key.push(2u8);
        legacy_last_key.extend_from_slice(&new_last_account.encode());
        sp_io::storage::set(&legacy_last_key, &666u64.encode());

        // Legacy LastTxBlockChildKeyTake entry (index 3)
        let legacy_child_account = U256::from(3);
        ChildKeys::<Test>::insert(
            legacy_child_account,
            NetUid::from(0),
            vec![(0u64, U256::from(99))],
        );
        let mut legacy_child_key = prefix.clone();
        legacy_child_key.push(3u8);
        legacy_child_key.extend_from_slice(&legacy_child_account.encode());
        sp_io::storage::set(&legacy_child_key, &333u64.encode());

        // Legacy LastTxBlockDelegateTake entry (index 4)
        let legacy_delegate_account = U256::from(4);
        Delegates::<Test>::insert(legacy_delegate_account, PerU16::from_parts(500));
        let mut legacy_delegate_key = prefix.clone();
        legacy_delegate_key.push(4u8);
        legacy_delegate_key.extend_from_slice(&legacy_delegate_account.encode());
        sp_io::storage::set(&legacy_delegate_key, &444u64.encode());

        let weight = crate::migrations::migrate_rate_limit_keys::migrate_rate_limit_keys::<Test>();
        assert!(
            HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()),
            "Migration should be marked as executed"
        );
        assert!(!weight.is_zero(), "Migration weight should be non-zero");

        // Legacy entries were migrated and cleared.
        assert_eq!(
            SubtensorModule::get_network_last_lock_block(),
            111u64,
            "Network last lock block should match migrated value"
        );
        assert!(
            sp_io::storage::get(&legacy_network_key).is_none(),
            "Legacy network entry should be cleared"
        );

        assert_eq!(
            SubtensorModule::get_last_tx_block(&new_last_account),
            666u64,
            "LastTxBlock should reflect the merged legacy value"
        );
        assert!(
            sp_io::storage::get(&legacy_last_key).is_none(),
            "Legacy LastTxBlock entry should be cleared"
        );

        assert_eq!(
            SubtensorModule::get_last_tx_block_childkey_take(&legacy_child_account),
            333u64,
            "Child key take block should be migrated"
        );
        assert!(
            sp_io::storage::get(&legacy_child_key).is_none(),
            "Legacy child take entry should be cleared"
        );

        assert_eq!(
            SubtensorModule::get_last_tx_block_delegate_take(&legacy_delegate_account),
            444u64,
            "Delegate take block should be migrated"
        );
        assert!(
            sp_io::storage::get(&legacy_delegate_key).is_none(),
            "Legacy delegate take entry should be cleared"
        );

        // New-format entries remain untouched.
        assert_eq!(
            SubtensorModule::get_last_tx_block_childkey_take(&new_child_account),
            777u64,
            "Existing child take entry should be preserved"
        );
        assert_eq!(
            SubtensorModule::get_last_tx_block_delegate_take(&new_delegate_account),
            888u64,
            "Existing delegate take entry should be preserved"
        );
    });
}

#[test]
fn test_migrate_remove_add_stake_burn_rate_limit() {
    new_test_ext(1).execute_with(|| {
        const MIGRATION_NAME: &[u8] = b"migrate_remove_add_stake_burn_rate_limit";
        let netuid = NetUid::from(1);
        let other_netuid = NetUid::from(2);
        let preserved_netuid = NetUid::from(3);
        let add_stake_burn_key = RateLimitKey::AddStakeBurn(netuid);
        let other_add_stake_burn_key = RateLimitKey::AddStakeBurn(other_netuid);
        let preserved_key = RateLimitKey::SetSNOwnerHotkey(preserved_netuid);

        SubtensorModule::set_rate_limited_last_block(&add_stake_burn_key, 100);
        SubtensorModule::set_rate_limited_last_block(&other_add_stake_burn_key, 200);
        SubtensorModule::set_rate_limited_last_block(&preserved_key, 300);

        let weight =
            crate::migrations::migrate_remove_add_stake_burn_rate_limit::migrate_remove_add_stake_burn_rate_limit::<Test>();

        assert!(
            HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()),
            "Migration should be marked as executed"
        );
        assert!(!weight.is_zero(), "Migration weight should be non-zero");

        assert_eq!(
            SubtensorModule::get_rate_limited_last_block(&add_stake_burn_key),
            0
        );
        assert_eq!(
            SubtensorModule::get_rate_limited_last_block(&other_add_stake_burn_key),
            0
        );
        assert_eq!(
            SubtensorModule::get_rate_limited_last_block(&preserved_key),
            300
        );
    });
}
