use super::*;
use crate::HasMigrationRun;
use frame_support::{traits::Get, weights::Weight};
use scale_info::prelude::string::String;
use sp_io::{KillStorageResult, hashing::twox_128, storage::clear_prefix};

/// Clears obsolete `RevealedCommitments` storage under the `Commitments` pallet prefix.
///
/// Idempotency key (frozen): `migrate_revealed_commitments_v2`.
pub fn migrate_upgrade_revealed_commitments<T: Config>() -> Weight {
    let migration_name = b"migrate_revealed_commitments_v2".to_vec();
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
    // 1) Clear the old `RevealedCommitments` storage from the `Commitments` pallet
    // -------------------------------------------------------------
    let mut revealed_commitments_prefix = Vec::new();
    revealed_commitments_prefix.extend_from_slice(&twox_128("Commitments".as_bytes()));
    revealed_commitments_prefix.extend_from_slice(&twox_128("RevealedCommitments".as_bytes()));

    let removal_result = clear_prefix(&revealed_commitments_prefix, Some(u32::MAX));
    let removed_entries_count = match removal_result {
        KillStorageResult::AllRemoved(removed) => removed as u64,
        KillStorageResult::SomeRemaining(removed) => {
            log::warn!("Failed to remove some items during `migrate_revealed_commitments`.");
            removed as u64
        }
    };
    weight = weight.saturating_add(T::DbWeight::get().writes(removed_entries_count));

    log::info!("Removed {removed_entries_count} entries from `RevealedCommitments`.");

    // -------------------------------------------------------------
    // 2) Mark this migration as completed
    // -------------------------------------------------------------
    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{}' completed successfully.",
        String::from_utf8_lossy(&migration_name)
    );

    weight
}
