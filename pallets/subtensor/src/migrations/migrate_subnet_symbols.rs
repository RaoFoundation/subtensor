use super::*;
use alloc::string::String;
use frame_support::IterableStorageMap;
use frame_support::{traits::Get, weights::Weight};

/// Rewrites `TokenSymbol` for each subnet to the canonical symbol table after a historical shift bug.
///
/// Idempotency key (frozen): `migrate_subnet_symbols`.
pub fn migrate_subnet_symbols<T: Config>() -> Weight {
    let migration_name = b"migrate_subnet_symbols".to_vec();

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

    // Get all netuids
    let netuids: Vec<NetUid> = <NetworksAdded<T> as IterableStorageMap<NetUid, bool>>::iter()
        .map(|(netuid, _)| netuid)
        .collect();

    for netuid in netuids.iter() {
        // Get the symbol for the subnet
        let symbol = Pallet::<T>::get_symbol_for_subnet(*netuid);

        // Set the symbol for the subnet
        TokenSymbol::<T>::insert(*netuid, symbol);
        weight = weight.saturating_add(T::DbWeight::get().writes(1));
    }

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
