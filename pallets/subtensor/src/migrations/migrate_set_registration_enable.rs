use alloc::string::String;

use frame_support::IterableStorageMap;
use frame_support::{traits::Get, weights::Weight};

use super::*;

/// Enables network registration on every non-root subnet where it is currently disabled.
///
/// Idempotency key (frozen): `migrate_set_registration_enable`.
pub fn migrate_set_registration_enable<T: Config>() -> Weight {
    let migration_name = b"migrate_set_registration_enable".to_vec();

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

        if !Pallet::<T>::get_network_registration_allowed(*netuid) {
            Pallet::<T>::set_network_registration_allowed(*netuid, true);
            weight = weight.saturating_add(T::DbWeight::get().writes(1));
        }
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
