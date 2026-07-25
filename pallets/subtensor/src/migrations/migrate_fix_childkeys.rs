use super::*;
use alloc::string::String;

/// Repairs `ChildKeys` / `ParentKeys` linkage inconsistencies from an earlier childkey bug.
///
/// Idempotency key (frozen): `migrate_fix_childkeys`.
pub fn migrate_fix_childkeys<T: Config>() -> Weight {
    let migration_name = b"migrate_fix_childkeys".to_vec();
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

    ////////////////////////////////////////////////////////
    // Actual migration

    Pallet::<T>::clean_zero_childkey_vectors(&mut weight);
    Pallet::<T>::clean_zero_parentkey_vectors(&mut weight);
    Pallet::<T>::clean_self_loops(&mut weight);
    Pallet::<T>::repair_child_parent_consistency(&mut weight);

    ////////////////////////////////////////////////////////

    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        target: "runtime",
        "Migration '{}' completed successfully.",
        String::from_utf8_lossy(&migration_name)
    );
    weight
}
