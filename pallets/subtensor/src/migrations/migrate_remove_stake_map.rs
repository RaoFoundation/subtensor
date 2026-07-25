use super::*;
use crate::HasMigrationRun;
use frame_support::{traits::Get, weights::Weight};
use scale_info::prelude::string::String;
use sp_io::{KillStorageResult, hashing::twox_128, storage::clear_prefix};

/// Clears the legacy `Stake` double map after stake moved to alpha/share-based storage.
///
/// Idempotency key (frozen): `migrate_remove_stake_map`.
pub fn migrate_remove_stake_map<T: Config>() -> Weight {
    let migration_name = b"migrate_remove_stake_map".to_vec();
    let mut weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(&migration_name) {
        log::info!(
            "Migration '{:?}' has already run. Skipping.",
            String::from_utf8_lossy(&migration_name)
        );
        return weight;
    }

    log::info!(
        "Running migration '{}'",
        String::from_utf8_lossy(&migration_name)
    );

    // ------------------------------
    // Step 1: Remove Stake entries
    // ------------------------------

    let mut stake_prefix = Vec::new();
    stake_prefix.extend_from_slice(&twox_128("SubtensorModule".as_bytes()));
    stake_prefix.extend_from_slice(&twox_128("Stake".as_bytes()));

    let removal_results = clear_prefix(&stake_prefix, Some(u32::MAX));

    let removed_entries_count = match removal_results {
        KillStorageResult::AllRemoved(removed) => removed as u64,
        KillStorageResult::SomeRemaining(removed) => {
            log::info!("Failed To Remove Some Items During {migration_name:?}");
            removed as u64
        }
    };

    weight = weight.saturating_add(T::DbWeight::get().writes(removed_entries_count));

    log::info!("Removed {removed_entries_count:?} entries from Stake map.");

    // ------------------------------
    // Step 2: Mark Migration as Completed
    // ------------------------------

    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{:?}' completed successfully.",
        String::from_utf8_lossy(&migration_name)
    );

    weight
}
