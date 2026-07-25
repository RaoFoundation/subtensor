use super::*;
use crate::HasMigrationRun;
use frame_support::{traits::Get, weights::Weight};
use scale_info::prelude::string::String;

/// Clears superseded identity maps `Identities`, `SubnetIdentities`, and `SubnetIdentitiesV2`
/// after the V3 identity layout took over.
///
/// Idempotency key (frozen): `migrate_remove_old_identity_maps`.
pub fn migrate_remove_old_identity_maps<T: Config>() -> Weight {
    let migration_name = b"migrate_remove_old_identity_maps".to_vec();
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
        String::from_utf8_lossy(&migration_name),
    );

    // ------------------------------
    // Step 1: Remove Map entries
    // ------------------------------
    remove_prefix::<T>("SubtensorModule", "Identities", &mut weight);
    remove_prefix::<T>("SubtensorModule", "SubnetIdentities", &mut weight);
    remove_prefix::<T>("SubtensorModule", "SubnetIdentitiesV2", &mut weight);

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
