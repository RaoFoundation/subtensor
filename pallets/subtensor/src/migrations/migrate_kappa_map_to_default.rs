use super::*;
use frame_support::{traits::Get, weights::Weight};
use log;
use scale_info::prelude::string::String;

/// Resets per-subnet `Kappa` to the pallet default for every existing netuid.
///
/// Idempotency key (frozen): `kappa_map_to_default`.
pub fn migrate_kappa_map_to_default<T: Config>() -> Weight {
    let mig_name: Vec<u8> = b"kappa_map_to_default".to_vec();
    let mig_name_str = String::from_utf8_lossy(&mig_name);

    // 1 read for the HasMigrationRun flag
    let mut total_weight = T::DbWeight::get().reads(1);

    // Run once guard
    if HasMigrationRun::<T>::get(&mig_name) {
        log::info!("Migration '{mig_name_str}' already executed - skipping");
        return total_weight;
    }

    log::info!("Running migration '{mig_name_str}'");

    let target: u16 = DefaultKappa::<T>::get();

    let mut reads: u64 = 0;
    let mut writes: u64 = 0;
    let mut visited: u64 = 0;
    let mut updated: u64 = 0;
    let mut unchanged: u64 = 0;

    for (netuid, current) in Kappa::<T>::iter() {
        visited = visited.saturating_add(1);
        reads = reads.saturating_add(1);

        if current != target {
            Kappa::<T>::insert(netuid, target);
            writes = writes.saturating_add(1);
            updated = updated.saturating_add(1);
        } else {
            unchanged = unchanged.saturating_add(1);
        }
    }

    total_weight = total_weight.saturating_add(T::DbWeight::get().reads_writes(reads, writes));

    log::info!(
        "Kappa migration summary: visited={visited}, updated={updated}, unchanged={unchanged}, target_default={target}"
    );

    HasMigrationRun::<T>::insert(&mig_name, true);
    total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!("Migration '{mig_name_str}' completed");

    total_weight
}
