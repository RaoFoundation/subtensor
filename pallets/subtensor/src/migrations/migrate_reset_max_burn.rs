use super::*;
use frame_support::{traits::Get, weights::Weight};
use log;
use scale_info::prelude::string::String;

/// Resets every subnet's `MaxBurn` to 100 TAO (rao units).
///
/// Idempotency key (frozen): `migrate_reset_max_burn`.
pub fn migrate_reset_max_burn<T: Config>() -> Weight {
    let migration_name = b"migrate_reset_max_burn".to_vec();
    let mut weight = T::DbWeight::get().reads(1);

    // ------------------------------
    // Step 0: Check if already run
    // ------------------------------
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
    // Step 1: Reset all subnet's MaxBurn to 100 TAO
    // ------------------------------

    let mut reset_entries_count = 0u64;

    for netuid in MaxBurn::<T>::iter_keys() {
        MaxBurn::<T>::mutate(netuid, |max| {
            *max = 100_000_000_000_u64.into();
        });
        reset_entries_count = reset_entries_count.saturating_add(1);
    }

    weight = weight
        .saturating_add(T::DbWeight::get().reads_writes(reset_entries_count, reset_entries_count));

    log::info!("Reset {reset_entries_count} subnets from MaxBurn.");

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
