use super::*;
use crate::AccountIdOf;
use frame_support::{
    IterableStorageMap,
    pallet_prelude::{Blake2_128Concat, OptionQuery},
    storage_alias,
    traits::Get,
    weights::Weight,
};
use scale_info::prelude::string::String;

/// Module containing deprecated storage format for AutoStakeDestination
pub mod deprecated_auto_stake_destination_format {
    use super::*;

    #[storage_alias]
    pub(super) type AutoStakeDestination<T: Config> =
        StorageMap<Pallet<T>, Blake2_128Concat, AccountIdOf<T>, AccountIdOf<T>, OptionQuery>;
}

/// Migrate the AutoStakeDestination map from single map to double map format
pub fn migrate_auto_stake_destination<T: Config>() -> Weight {
    use deprecated_auto_stake_destination_format as old;

    let migration_name = b"migrate_auto_stake_destination".to_vec();
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

    // ------------------------------
    // Step 1: Migrate AutoStakeDestination entries
    // ------------------------------

    let curr_keys: Vec<AccountIdOf<T>> = old::AutoStakeDestination::<T>::iter_keys().collect();
    let root_netuid = NetUid::ROOT;
    let netuids: Vec<NetUid> = <NetworksAdded<T> as IterableStorageMap<NetUid, bool>>::iter()
        .map(|(netuid, _)| netuid)
        .collect();

    for coldkey in &curr_keys {
        weight.saturating_accrue(T::DbWeight::get().reads(1));

        if let Some(hotkey) = old::AutoStakeDestination::<T>::get(coldkey) {
            for netuid in netuids.iter() {
                if *netuid == root_netuid {
                    continue;
                }
                AutoStakeDestination::<T>::insert(coldkey, netuid, hotkey.clone());
                AutoStakeDestinationColdkeys::<T>::mutate(hotkey.clone(), netuid, |v| {
                    if !v.contains(coldkey) {
                        v.push(coldkey.clone());
                    }
                });
            }

            old::AutoStakeDestination::<T>::remove(coldkey);

            weight.saturating_accrue(T::DbWeight::get().writes(netuids.len() as u64));
        }
    }

    // ------------------------------
    // Step 2: Mark Migration as Completed
    // ------------------------------

    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{:?}' completed successfully. {} entries migrated.",
        String::from_utf8_lossy(&migration_name),
        curr_keys.len()
    );

    weight
}
