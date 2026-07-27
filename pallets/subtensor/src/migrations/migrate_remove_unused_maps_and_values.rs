use super::*;
use crate::HasMigrationRun;
use frame_support::{traits::Get, weights::Weight};
use scale_info::prelude::string::String;
use sp_io::storage::clear;

/// Removes unused maps/values `EmissionValues`, `NetworkMaxStake`, and the old `SubnetLimit` value key.
///
/// Idempotency key (frozen): `migrate_remove_unused_maps_and_values`.
pub fn migrate_remove_unused_maps_and_values<T: Config>() -> Weight {
    let migration_name = b"migrate_remove_unused_maps_and_values".to_vec();
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

    // Remove EmissionValues entries
    remove_prefix::<T>("SubtensorModule", "EmissionValues", &mut weight);

    // Remove NetworkMaxStake
    remove_prefix::<T>("SubtensorModule", "NetworkMaxStake", &mut weight);

    // Remove SubnetLimit
    clear(b"SubtensorModule::SubnetLimit");

    // Mark Migration as Completed
    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{:?}' completed successfully.",
        String::from_utf8_lossy(&migration_name)
    );

    weight
}
