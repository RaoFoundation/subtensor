use super::*;
use frame_support::{traits::Get, weights::Weight};
use log;
use scale_info::prelude::string::String;

/// v440: doubles the subnet slot count to 256. Safe alongside the emission
/// gate: the gate bar is a property of the demand distribution, so the new
/// empty slots neither dilute the head nor lower the bar.
pub fn migrate_subnet_limit_256<T: Config>() -> Weight {
    let mig_name: Vec<u8> = b"subnet_limit_256".to_vec();
    const TARGET: u16 = 256;

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

    let current: u16 = SubnetLimit::<T>::get();
    total_weight = total_weight.saturating_add(T::DbWeight::get().reads(1));

    if current < TARGET {
        SubnetLimit::<T>::put(TARGET);
        total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));
        log::info!("SubnetLimit updated: {current} -> {TARGET}");
    } else {
        log::info!("SubnetLimit already at or above {TARGET} ({current}), no update performed.");
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
