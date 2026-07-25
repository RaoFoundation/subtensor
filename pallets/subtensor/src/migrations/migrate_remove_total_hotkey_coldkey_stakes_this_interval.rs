use super::*;
use crate::HasMigrationRun;
use frame_support::{traits::Get, weights::Weight};
use sp_io::{KillStorageResult, hashing::twox_128, storage::clear_prefix};

/// Clears obsolete `TotalHotkeyColdkeyStakesThisInterval` storage under `SubtensorModule`.
///
/// Idempotency key (frozen): `migrate_remove_total_hotkey_coldkey_stakes_this_interval`.
/// Marks complete only when `clear_prefix` reports `AllRemoved`.
pub fn migrate_remove_total_hotkey_coldkey_stakes_this_interval<T: Config>() -> Weight {
    let migration_name = "migrate_remove_total_hotkey_coldkey_stakes_this_interval";
    let migration_name_bytes = migration_name.as_bytes().to_vec();

    let mut weight = T::DbWeight::get().reads(1);
    if HasMigrationRun::<T>::get(&migration_name_bytes) {
        log::info!("Migration '{migration_name:?}' has already run. Skipping.");
        return weight;
    }

    log::info!("Running migration '{migration_name}'");

    let pallet_name = twox_128(b"SubtensorModule");
    let storage_name = twox_128(b"TotalHotkeyColdkeyStakesThisInterval");
    let prefix = [pallet_name, storage_name].concat();

    // Remove all entries.
    let removed_entries_count = match clear_prefix(&prefix, Some(u32::MAX)) {
        KillStorageResult::AllRemoved(removed) => {
            log::info!("Removed all entries from {storage_name:?}.");

            // Mark migration as completed
            HasMigrationRun::<T>::insert(&migration_name_bytes, true);
            weight = weight.saturating_add(T::DbWeight::get().writes(1));

            removed as u64
        }
        KillStorageResult::SomeRemaining(removed) => {
            log::info!("Failed to remove all entries from {storage_name:?}");
            removed as u64
        }
    };

    weight = weight.saturating_add(T::DbWeight::get().writes(removed_entries_count as u64));

    log::info!(
        "Migration '{migration_name:?}' completed successfully. {removed_entries_count:?} entries removed."
    );

    weight
}
