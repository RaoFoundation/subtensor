use crate::{Config, HasMigrationRun};
use alloc::string::String;
use frame_support::pallet_prelude::Weight;
use frame_support::traits::Get;
use sp_io::KillStorageResult;
use sp_io::hashing::twox_128;
use sp_io::storage::clear_prefix;
use sp_std::vec::Vec;
fn remove_prefix<T: Config>(old_map: &str) -> Weight {
    let mut prefix = Vec::new();
    prefix.extend_from_slice(&twox_128("SubtensorModule".as_bytes()));
    prefix.extend_from_slice(&twox_128(old_map.as_bytes()));

    let removal_results = clear_prefix(&prefix, Some(u32::MAX));

    let removed_entries_count = match removal_results {
        KillStorageResult::AllRemoved(removed) => removed as u64,
        KillStorageResult::SomeRemaining(removed) => {
            log::info!("Failed To Remove Some Items During migration");
            removed as u64
        }
    };

    log::info!("Removed {removed_entries_count:?} entries from {old_map:?} map.");

    T::DbWeight::get().writes(removed_entries_count)
}

/// Clears obsolete dividend maps `TaoDividendsPerSubnet`, `PendingAlphaSwapped`, and `PendingRootDivs`.
///
/// Idempotency key (frozen): `migrate_remove_tao_dividends`.
pub fn migrate_remove_tao_dividends<T: Config>() -> Weight {
    let migration_name = b"migrate_remove_tao_dividends".to_vec();
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

    // Remove obsolete map entries
    let weight1 = remove_prefix::<T>("TaoDividendsPerSubnet");
    let weight2 = remove_prefix::<T>("PendingAlphaSwapped");
    let weight3 = remove_prefix::<T>("PendingRootDivs");

    // Mark Migration as Completed
    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{:?}' completed successfully.",
        String::from_utf8_lossy(&migration_name)
    );

    weight
        .saturating_add(weight1)
        .saturating_add(weight2)
        .saturating_add(weight3)
}
