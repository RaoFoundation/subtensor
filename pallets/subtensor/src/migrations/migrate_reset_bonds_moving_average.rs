use super::*;
use frame_support::{traits::Get, weights::Weight};
use log;
use scale_info::prelude::string::String;

/// Caps each subnet's `BondsMovingAverage` at 975_000 when the stored value exceeds that ceiling.
///
/// Idempotency key (frozen): `migrate_reset_bonds_moving_average`.
pub fn migrate_reset_bonds_moving_average<T: Config>() -> Weight {
    let migration_name = b"migrate_reset_bonds_moving_average".to_vec();
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
    // Step 1: Reset all subnet's BondsMovingAverage to 975000 if the value exceeds 975000
    // ------------------------------

    let mut reset_entries_count = 0u64;

    for netuid in BondsMovingAverage::<T>::iter_keys() {
        BondsMovingAverage::<T>::mutate(netuid, |average| {
            *average = (*average).min(975000);
        });
        reset_entries_count = reset_entries_count.saturating_add(1);
    }

    weight = weight
        .saturating_add(T::DbWeight::get().reads_writes(reset_entries_count, reset_entries_count));

    log::info!("Reset {reset_entries_count} subnets from BondsMovingAverage.");

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
