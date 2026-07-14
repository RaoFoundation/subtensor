use crate::{Config, Event, HasMigrationRun, NetworksAdded, Pallet, SubnetIdentitiesV3, Weight};
use frame_support::traits::Get;
use scale_info::prelude::string::String;
use sp_std::vec::Vec;
use subtensor_runtime_common::NetUid;

/// Remove `SubnetIdentitiesV3` entries that belong to netuids which are no
/// longer registered networks (`!NetworksAdded`).
///
/// Such orphan identities accumulate when a subnet slot is recycled: a new
/// owner registers a subnet in a previously-used netuid without supplying an
/// identity, and the stale identity from the prior owner is left in place,
/// misleading participants about what the subnet is. The registration path
/// (`set_new_network_state`) only writes on `Some(identity)` and never clears a
/// pre-existing entry, and the historical `migrate_subnet_identities_to_v3`
/// migration copied V2 entries unconditionally.
///
/// This mirrors the identity-clear in `do_dissolve_network` as a one-shot
/// upgrade migration: orphan entries are removed and a `SubnetIdentityRemoved`
/// event is emitted for each; identities of live subnets are untouched.
pub fn migrate_clear_orphan_subnet_identities_v3<T: Config>() -> Weight {
    let migration_name = b"migrate_clear_orphan_subnet_identities_v3".to_vec();
    let mut weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(&migration_name) {
        log::info!(
            target: "runtime",
            "Migration '{}' already run - skipping.",
            String::from_utf8_lossy(&migration_name)
        );
        return weight;
    }

    log::info!(
        target: "runtime",
        "Running migration '{}'",
        String::from_utf8_lossy(&migration_name),
    );

    // Collect identity netuids that no longer correspond to a registered subnet.
    // Every scanned key costs an identity-map read plus a NetworksAdded lookup,
    // even when the identity is kept for a live subnet.
    let mut scanned = 0u64;
    let orphan_netuids: Vec<NetUid> = SubnetIdentitiesV3::<T>::iter_keys()
        .filter(|netuid| {
            scanned = scanned.saturating_add(1);
            !NetworksAdded::<T>::get(netuid)
        })
        .collect();
    let removed = orphan_netuids.len() as u64;
    weight = weight.saturating_add(T::DbWeight::get().reads(scanned.saturating_mul(2)));

    for netuid in orphan_netuids {
        SubnetIdentitiesV3::<T>::remove(netuid);
        Pallet::<T>::deposit_event(Event::SubnetIdentityRemoved(netuid));
    }
    weight = weight.saturating_add(T::DbWeight::get().writes(removed));

    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        target: "runtime",
        "Migration '{}' completed. scanned_identities={}, removed_orphans={}",
        String::from_utf8_lossy(&migration_name),
        scanned,
        removed,
    );

    weight
}
