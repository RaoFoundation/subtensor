use super::*;
use alloc::string::String;
use frame_support::{traits::Get, weights::Weight};

/// Widens every `SubnetVolume` value from `u64` to `u128` in place via `translate`.
///
/// Idempotency key (frozen): `migrate_subnet_volume`.
pub fn migrate_subnet_volume<T: Config>() -> Weight {
    let migration_name = b"migrate_subnet_volume".to_vec();

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

    let mut migrated = 0u64;

    SubnetVolume::<T>::translate::<u64, _>(|_key, old_value| {
        migrated = migrated.saturating_add(1);
        Some(old_value as u128) // Convert and store as u128
    });

    log::info!("Migrated {migrated} entries in SubnetVolume");
    weight = weight.saturating_add(T::DbWeight::get().reads_writes(migrated, migrated));

    // Mark the migration as completed
    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    weight
}
