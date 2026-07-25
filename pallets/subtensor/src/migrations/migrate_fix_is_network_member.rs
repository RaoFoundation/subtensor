use super::*;
use alloc::string::String;
use frame_support::{traits::Get, weights::Weight};
use log;

/// Rebuilds the `IsNetworkMember` index so membership matches current subnet key maps.
///
/// Idempotency key (frozen): `migrate_fix_is_network_member`.
pub fn migrate_fix_is_network_member<T: Config>() -> Weight {
    let migration_name = b"migrate_fix_is_network_member".to_vec();

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

    weight = do_fix_is_network_member::<T>(weight);

    // Mark the migration as completed
    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{:?}' completed. Storage version set to 7.",
        String::from_utf8_lossy(&migration_name)
    );

    // Return the migration weight.
    weight
}

fn do_fix_is_network_member<T: Config>(weight: Weight) -> Weight {
    let mut weight = weight;
    // Clear the IsNetworkMember storage
    let mut curr = IsNetworkMember::<T>::clear(u32::MAX, None);
    weight = weight
        .saturating_add(T::DbWeight::get().reads_writes(curr.loops as u64, curr.unique as u64));
    while curr.maybe_cursor.is_some() {
        // Clear until empty
        curr = IsNetworkMember::<T>::clear(u32::MAX, curr.maybe_cursor.as_deref());
        weight = weight
            .saturating_add(T::DbWeight::get().reads_writes(curr.loops as u64, curr.unique as u64));
    }
    // Repopulate the IsNetworkMember storage using the Keys map
    for (netuid, _uid, key) in Keys::<T>::iter() {
        IsNetworkMember::<T>::insert(key, netuid, true);
        weight = weight.saturating_add(T::DbWeight::get().reads_writes(1, 1));
    }

    weight
}
