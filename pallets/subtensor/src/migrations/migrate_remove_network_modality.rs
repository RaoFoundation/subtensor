use super::*;
use crate::HasMigrationRun;
use frame_support::{storage_alias, traits::Get, weights::Weight};
use scale_info::prelude::string::String;
use sp_std::vec::Vec;

/// Module containing deprecated storage format for NetworkModality
pub mod deprecated_network_modality_format {
    use super::*;

    #[storage_alias]
    pub(super) type NetworkModality<T: Config> =
        StorageMap<Pallet<T>, Identity, NetUid, u16, ValueQuery>;
}

/// Deletes deprecated `NetworkModality` per-netuid entries (feature removed).
///
/// Idempotency key (frozen): `migrate_remove_network_modality`.
pub fn migrate_remove_network_modality<T: Config>() -> Weight {
    const MIG_NAME: &[u8] = b"migrate_remove_network_modality";

    // 1 ─ check if we already ran
    if HasMigrationRun::<T>::get(MIG_NAME) {
        log::info!(
            "Migration '{}' already executed - skipping",
            String::from_utf8_lossy(MIG_NAME)
        );
        return T::DbWeight::get().reads(1);
    }

    log::info!("Running migration '{}'", String::from_utf8_lossy(MIG_NAME));

    let mut total_weight = T::DbWeight::get().reads(1);

    // 2 ─ remove NetworkModality entries for all existing networks
    let total_networks = TotalNetworks::<T>::get();
    total_weight = total_weight.saturating_add(T::DbWeight::get().reads(1));

    // Use raw storage operations to remove NetworkModality entries
    // NetworkModality was a StorageMap<_, Identity, NetUid, u16>
    let pallet_prefix = sp_io::hashing::twox_128("SubtensorModule".as_bytes());
    let storage_prefix = sp_io::hashing::twox_128("NetworkModality".as_bytes());

    for netuid in 0..total_networks {
        let netuid = NetUid::from(netuid);
        let mut key = Vec::new();
        key.extend_from_slice(&pallet_prefix);
        key.extend_from_slice(&storage_prefix);
        // Identity encoding for netuid
        key.extend_from_slice(&netuid.encode());

        // Clear the storage entry if it exists
        sp_io::storage::clear_prefix(&key, None);
        total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));
    }

    // 3 ─ mark migration as done
    HasMigrationRun::<T>::insert(MIG_NAME, true);
    total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{}' completed: removed NetworkModality storage for {} networks",
        String::from_utf8_lossy(MIG_NAME),
        total_networks
    );
    total_weight
}
