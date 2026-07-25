#![allow(
    unused,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]
//! orphan / deprecated storage item removals.

use super::helpers::*;
use super::prelude::*;

#[test]
fn test_migrate_remove_total_hotkey_coldkey_stakes_this_interval() {
    new_test_ext(1).execute_with(|| {
        const MIGRATION_NAME: &str = "migrate_remove_total_hotkey_coldkey_stakes_this_interval";

        let pallet_name = twox_128(b"SubtensorModule");
        let storage_name = twox_128(b"TotalHotkeyColdkeyStakesThisInterval");
        let prefix = [pallet_name, storage_name].concat();

        // Set up 200 000 entries to be deleted.
        for i in 0..200_000{
            let hotkey = U256::from(i as u64);
            let coldkey = U256::from(i as u64);
            let key = [prefix.clone(), hotkey.encode(), coldkey.encode()].concat();
            let value = (100 + i, 200 + i);
            put_raw(&key, &value.encode());
        }

        assert!(frame_support::storage::unhashed::contains_prefixed_key(&prefix), "Entries should exist before migration.");
        assert!(
            !HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should not have run yet."
        );

        // Run migration
        let weight = crate::migrations::migrate_remove_total_hotkey_coldkey_stakes_this_interval::migrate_remove_total_hotkey_coldkey_stakes_this_interval::<Test>();

        assert!(!frame_support::storage::unhashed::contains_prefixed_key(&prefix), "All entries should have been removed.");
        assert!(
            HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should be marked as run."
        );
        assert!(!weight.is_zero(),"Migration weight should be non-zero.");
    });
}

fn test_migrate_remove_last_hotkey_coldkey_emission_on_netuid() {
    const MIGRATION_NAME: &str = "migrate_remove_last_hotkey_coldkey_emission_on_netuid";
    let pallet_name = "SubtensorModule";
    let storage_name = "LastHotkeyColdkeyEmissionOnNetuid";
    let migration =  crate::migrations::migrate_orphaned_storage_items::remove_last_hotkey_coldkey_emission_on_netuid::<Test>;

    test_remove_storage_item(
        MIGRATION_NAME,
        pallet_name,
        storage_name,
        migration,
        200_000,
    );
}

#[test]
fn test_migrate_remove_subnet_alpha_emission_sell() {
    const MIGRATION_NAME: &str = "migrate_remove_subnet_alpha_emission_sell";
    let pallet_name = "SubtensorModule";
    let storage_name = "SubnetAlphaEmissionSell";
    let migration =
        crate::migrations::migrate_orphaned_storage_items::remove_subnet_alpha_emission_sell::<Test>;

    test_remove_storage_item(
        MIGRATION_NAME,
        pallet_name,
        storage_name,
        migration,
        200_000,
    );
}

#[test]
fn test_migrate_remove_neurons_to_prune_at_next_epoch() {
    const MIGRATION_NAME: &str = "migrate_remove_neurons_to_prune_at_next_epoch";
    let pallet_name = "SubtensorModule";
    let storage_name = "NeuronsToPruneAtNextEpoch";
    let migration =
        crate::migrations::migrate_orphaned_storage_items::remove_neurons_to_prune_at_next_epoch::<
            Test,
        >;

    test_remove_storage_item(
        MIGRATION_NAME,
        pallet_name,
        storage_name,
        migration,
        200_000,
    );
}

#[test]
fn test_migrate_remove_total_stake_at_dynamic() {
    const MIGRATION_NAME: &str = "migrate_remove_total_stake_at_dynamic";
    let pallet_name = "SubtensorModule";
    let storage_name = "TotalStakeAtDynamic";
    let migration =
        crate::migrations::migrate_orphaned_storage_items::remove_total_stake_at_dynamic::<Test>;

    test_remove_storage_item(
        MIGRATION_NAME,
        pallet_name,
        storage_name,
        migration,
        200_000,
    );
}

#[test]
fn test_migrate_remove_subnet_name() {
    const MIGRATION_NAME: &str = "migrate_remove_subnet_name";
    let pallet_name = "SubtensorModule";
    let storage_name = "SubnetName";
    let migration = crate::migrations::migrate_orphaned_storage_items::remove_subnet_name::<Test>;

    test_remove_storage_item(
        MIGRATION_NAME,
        pallet_name,
        storage_name,
        migration,
        200_000,
    );
}

#[test]
fn test_migrate_remove_network_min_allowed_uids() {
    const MIGRATION_NAME: &str = "migrate_remove_network_min_allowed_uids";
    let pallet_name = "SubtensorModule";
    let storage_name = "NetworkMinAllowedUids";
    let migration =
        crate::migrations::migrate_orphaned_storage_items::remove_network_min_allowed_uids::<Test>;

    test_remove_storage_item(MIGRATION_NAME, pallet_name, storage_name, migration, 1);
}

#[test]
fn test_migrate_remove_dynamic_block() {
    const MIGRATION_NAME: &str = "migrate_remove_dynamic_block";
    let pallet_name = "SubtensorModule";
    let storage_name = "DynamicBlock";
    let migration = crate::migrations::migrate_orphaned_storage_items::remove_dynamic_block::<Test>;

    test_remove_storage_item(MIGRATION_NAME, pallet_name, storage_name, migration, 1);
}

#[test]
fn test_migrate_remove_commitments_rate_limit() {
    new_test_ext(1).execute_with(|| {
        // ------------------------------
        // Step 1: Simulate Old Storage Entry
        // ------------------------------
        const MIGRATION_NAME: &str = "migrate_remove_commitments_rate_limit";

        // Build the raw storage key: twox128("Commitments") ++ twox128("RateLimit")
        let pallet_prefix = twox_128("Commitments".as_bytes());
        let storage_prefix = twox_128("RateLimit".as_bytes());

        let mut key = Vec::new();
        key.extend_from_slice(&pallet_prefix);
        key.extend_from_slice(&storage_prefix);

        let original_value: u64 = 123;
        put_raw(&key, &original_value.encode());

        let stored_before = get_raw(&key).expect("Expected RateLimit to exist");
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
        let weight = crate::migrations::migrate_remove_commitments_rate_limit::
            migrate_remove_commitments_rate_limit::<Test>();

        assert!(
            HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should be marked as completed"
        );

        // ------------------------------
        // Step 3: Verify Migration Effects
        // ------------------------------
        assert!(
            get_raw(&key).is_none(),
            "RateLimit storage should have been cleared"
        );

        assert!(!weight.is_zero(), "Migration weight should be non-zero");
    });
}

#[test]
fn test_migrate_remove_tao_dividends() {
    const MIGRATION_NAME: &str = "migrate_remove_tao_dividends";
    let pallet_name = "SubtensorModule";
    let storage_name = "TaoDividendsPerSubnet";
    let migration =
        crate::migrations::migrate_remove_tao_dividends::migrate_remove_tao_dividends::<Test>;

    test_remove_storage_item(
        MIGRATION_NAME,
        pallet_name,
        storage_name,
        migration,
        200_000,
    );

    let storage_name = "PendingAlphaSwapped";
    test_remove_storage_item(
        MIGRATION_NAME,
        pallet_name,
        storage_name,
        migration,
        200_000,
    );

    let storage_name = "PendingRootDivs";
    test_remove_storage_item(
        MIGRATION_NAME,
        pallet_name,
        storage_name,
        migration,
        200_000,
    );
}

fn test_migrate_remove_old_identity_maps() {
    let migration =
        crate::migrations::migrate_remove_old_identity_maps::migrate_remove_old_identity_maps::<Test>;

    const MIGRATION_NAME: &str = "migrate_remove_old_identity_maps";

    let pallet_name = "SubtensorModule";

    test_remove_storage_item(MIGRATION_NAME, pallet_name, "Identities", migration, 100);

    test_remove_storage_item(
        MIGRATION_NAME,
        pallet_name,
        "SubnetIdentities",
        migration,
        100,
    );

    test_remove_storage_item(
        MIGRATION_NAME,
        pallet_name,
        "SubnetIdentitiesV2",
        migration,
        100,
    );
}
