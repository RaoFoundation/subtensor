use super::*;
use crate::HasMigrationRun;
use frame_support::{traits::Get, weights::Weight};
use scale_info::prelude::string::String;
use sp_io::{KillStorageResult, hashing::twox_128, storage::clear_prefix};

/// Clears the obsolete commitments `RateLimit` storage prefix under `SubtensorModule`.
///
/// Idempotency key (frozen): `migrate_remove_commitments_rate_limit`.
pub fn migrate_remove_commitments_rate_limit<T: Config>() -> Weight {
    let migration_name = b"migrate_remove_commitments_rate_limit".to_vec();
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

    // -------------------------------------------------------------
    // Step 1: Remove all entries under the `RateLimit` storage key
    // -------------------------------------------------------------
    let mut rate_limit_prefix = Vec::new();
    rate_limit_prefix.extend_from_slice(&twox_128("Commitments".as_bytes()));
    rate_limit_prefix.extend_from_slice(&twox_128("RateLimit".as_bytes()));

    let removal_result = clear_prefix(&rate_limit_prefix, Some(u32::MAX));
    let removed_entries = match removal_result {
        KillStorageResult::AllRemoved(removed) => removed as u64,
        KillStorageResult::SomeRemaining(removed) => {
            log::warn!("Failed to remove some `RateLimit` entries.");
            removed as u64
        }
    };

    weight = weight.saturating_add(T::DbWeight::get().writes(removed_entries));
    log::info!("Removed {removed_entries} entries from `RateLimit`.");

    // -------------------------------------------------------------
    // Step 2: Mark this migration as completed
    // -------------------------------------------------------------
    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{:?}' completed successfully.",
        String::from_utf8_lossy(&migration_name)
    );

    weight
}
