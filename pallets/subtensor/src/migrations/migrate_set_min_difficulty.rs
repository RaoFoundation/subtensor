use alloc::string::String;

use frame_support::IterableStorageMap;
use frame_support::{traits::Get, weights::Weight};
use subtensor_runtime_common::NetUid;

use super::*;

/// Sets min PoW registration difficulty to 10_000_000 for all subnets.
///
/// Idempotency key (frozen): `migrate_set_min_difficulty`.
pub fn migrate_set_min_difficulty<T: Config>() -> Weight {
    let migration_name = b"migrate_set_min_difficulty".to_vec();

    // Initialize the weight with one read operation.
    let mut weight = T::DbWeight::get().reads(1);

    // Check if the migration has already run
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

    let netuids: Vec<NetUid> = <NetworksAdded<T> as IterableStorageMap<NetUid, bool>>::iter()
        .map(|(netuid, _)| netuid)
        .collect();
    weight = weight.saturating_add(T::DbWeight::get().reads(netuids.len() as u64));

    for netuid in netuids.iter() {
        if netuid.is_root() {
            continue;
        }
        // Set min difficulty to 10 million for all subnets
        Pallet::<T>::set_min_difficulty(*netuid, 10_000_000);
        weight = weight.saturating_add(T::DbWeight::get().writes(1));
    }

    // Mark the migration as completed
    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{:?}' completed.",
        String::from_utf8_lossy(&migration_name)
    );

    // Return the migration weight.
    weight
}
