use super::*;
use crate::HasMigrationRun;
use frame_support::{traits::Get, weights::Weight};
use scale_info::prelude::string::String;

/// Sets `SubtokenEnabled`: root always `true`; other subnets `true` iff `FirstEmissionBlockNumber` is set.
///
/// Idempotency key (frozen): `migrate_set_subtoken_enabled`.
pub fn migrate_set_subtoken_enabled<T: Config>() -> Weight {
    let migration_name = b"migrate_set_subtoken_enabled".to_vec();

    let mut weight = T::DbWeight::get().reads(1);
    if HasMigrationRun::<T>::get(&migration_name) {
        log::info!(
            "Migration '{:?}' has already run. Skipping.",
            String::from_utf8_lossy(&migration_name)
        );
        return weight;
    }

    log::info!(
        "Running migration '{:?}'",
        String::from_utf8_lossy(&migration_name)
    );

    // ------------------------------
    // Step 1: Set the subnet token enabled for all subnets except new subnet
    // ------------------------------
    let netuids = Pallet::<T>::get_all_subnet_netuids();
    for netuid in netuids.iter() {
        if !netuid.is_root() {
            // set it as true if start call executed and value exists for first emission block number
            SubtokenEnabled::<T>::insert(
                netuid,
                FirstEmissionBlockNumber::<T>::get(netuid).is_some(),
            );
        } else {
            SubtokenEnabled::<T>::insert(netuid, true);
        }
    }

    // ------------------------------
    // Step 2: Mark Migration as Completed
    // ------------------------------

    HasMigrationRun::<T>::insert(&migration_name, true);

    weight =
        weight.saturating_add(T::DbWeight::get().writes((netuids.len() as u64).saturating_add(1)));

    log::info!(
        "Migration '{:?}' completed successfully.",
        String::from_utf8_lossy(&migration_name)
    );

    weight
}
