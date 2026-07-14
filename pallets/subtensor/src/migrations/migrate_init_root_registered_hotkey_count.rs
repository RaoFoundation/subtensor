use alloc::string::String;

use frame_support::{traits::Get, weights::Weight};
use subtensor_runtime_common::NetUid;

use super::*;

pub fn migrate_init_root_registered_hotkey_count<T: Config>() -> Weight {
    let migration_name = b"migrate_init_root_registered_hotkey_count".to_vec();

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

    let mut entries: u64 = 0;
    for (_uid, hotkey) in Keys::<T>::iter_prefix(NetUid::ROOT) {
        let coldkey = Owner::<T>::get(&hotkey);
        Pallet::<T>::increment_root_registered_hotkey_count(&coldkey);
        weight.saturating_accrue(T::DbWeight::get().reads_writes(5, 2));
        entries = entries.saturating_add(1);
    }

    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{:?}' completed. {entries} root hotkeys indexed.",
        String::from_utf8_lossy(&migration_name)
    );

    weight
}
