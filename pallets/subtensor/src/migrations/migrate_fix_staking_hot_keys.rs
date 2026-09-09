use super::*;
use frame_support::{traits::Get, weights::Weight};
use log;
use scale_info::prelude::string::String;
use sp_std::collections::btree_map::BTreeMap;

pub fn migrate_fix_staking_hot_keys<T: Config>() -> Weight {
    let migration_name = b"migrate_fix_staking_hot_keys".to_vec();
    let mut weight = T::DbWeight::get().reads(1);

    // Skip if already executed
    if HasMigrationRun::<T>::get(&migration_name) {
        log::info!(
            target: "runtime",
            "Migration '{}' already run - skipping.",
            String::from_utf8_lossy(&migration_name)
        );
        return weight;
    }

    let mut cache: BTreeMap<T::AccountId, Vec<T::AccountId>> = BTreeMap::new();
    let mut storage_reads: u64 = 0;
    let mut storage_writes: u64 = 0;

    for ((hotkey, coldkey, _netuid), alpha) in Alpha::<T>::iter() {
        storage_reads = storage_reads.saturating_add(1);

        if alpha == 0 {
            continue;
        }

        let staking_hotkeys = cache.entry(coldkey.clone()).or_insert_with(|| {
            storage_reads = storage_reads.saturating_add(1);
            StakingHotkeys::<T>::get(&coldkey)
        });

        if !staking_hotkeys.contains(&hotkey) {
            staking_hotkeys.push(hotkey.clone());
            storage_writes = storage_writes.saturating_add(1);
            StakingHotkeys::<T>::insert(&coldkey, staking_hotkeys.clone());
        }
    }
    weight = weight.saturating_add(T::DbWeight::get().reads_writes(storage_reads, storage_writes));

    // Mark migration done
    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        target: "runtime",
        "Migration '{}' completed.",
        String::from_utf8_lossy(&migration_name)
    );

    weight
}
