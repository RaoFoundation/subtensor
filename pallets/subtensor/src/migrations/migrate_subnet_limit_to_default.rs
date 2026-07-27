use super::*;
use frame_support::{traits::Get, weights::Weight};
use log;
use scale_info::prelude::string::String;

/// Resets global `SubnetLimit` to the pallet default value.
///
/// Idempotency key (frozen): `subnet_limit_to_default`.
pub fn migrate_subnet_limit_to_default<T: Config>() -> Weight {
    let mig_name: Vec<u8> = b"subnet_limit_to_default".to_vec();

    // 1 read: HasMigrationRun flag
    let mut total_weight = T::DbWeight::get().reads(1);

    // Run once guard
    if HasMigrationRun::<T>::get(&mig_name) {
        log::info!(
            "Migration '{}' already executed - skipping",
            String::from_utf8_lossy(&mig_name)
        );
        return total_weight;
    }
    log::info!("Running migration '{}'", String::from_utf8_lossy(&mig_name));

    // Read current and compute target default
    let current: u16 = SubnetLimit::<T>::get();
    let target: u16 = DefaultSubnetLimit::<T>::get();

    if current != target {
        total_weight = total_weight.saturating_add(T::DbWeight::get().reads_writes(1, 1));
        SubnetLimit::<T>::put(target);
        log::info!("SubnetLimit updated: {current} -> {target}");
    } else {
        total_weight = total_weight.saturating_add(T::DbWeight::get().reads(1));
        log::info!("SubnetLimit already equals default ({target}), no update performed.");
    }

    // Mark as done
    HasMigrationRun::<T>::insert(&mig_name, true);
    total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{}' completed",
        String::from_utf8_lossy(&mig_name)
    );
    total_weight
}
