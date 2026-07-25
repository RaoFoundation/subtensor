use super::*;
use alloc::string::String;
use frame_support::{traits::Get, weights::Weight};

/// Sets nominator min required stake to `10_000_000` rao via `set_nominator_min_required_stake`.
///
/// Idempotency key (frozen): `migrate_set_nominator_min_stake`.
pub fn migrate_set_nominator_min_stake<T: Config>() -> Weight {
    let migration_name = b"migrate_set_nominator_min_stake".to_vec();

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

    // Set nominator min stake to 10 in per-mill format
    Pallet::<T>::set_nominator_min_required_stake(10_000_000);
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
