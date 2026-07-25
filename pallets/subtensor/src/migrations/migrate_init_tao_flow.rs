use alloc::string::String;

use frame_support::{traits::Get, weights::Weight};

use super::*;

/// Clears all `SubnetEmaTaoFlow` entries so EMA tao-flow state restarts empty after the feature cutover.
///
/// Idempotency key (frozen): `migrate_init_tao_flow`.
pub fn migrate_init_tao_flow<T: Config>() -> Weight {
    let migration_name = b"migrate_init_tao_flow".to_vec();

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

    let _ = SubnetEmaTaoFlow::<T>::clear(u32::MAX, None);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

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
