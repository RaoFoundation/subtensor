use super::*;
use crate::HasMigrationRun;
use frame_support::{storage_alias, traits::Get, weights::Weight};
use scale_info::prelude::string::String;

pub mod deprecated_swap_maps {
    use super::*;

    /// --- MAP ( netuid ) --> tao_in_user_subnet | Returns the amount of TAO in the subnet reserve provided by users as liquidity.
    #[storage_alias]
    pub type SubnetTaoProvided<T: Config> =
        StorageMap<Pallet<T>, Identity, NetUid, TaoBalance, ValueQuery>;

    /// --- MAP ( netuid ) --> alpha_supply_user_in_pool | Returns the amount of alpha in the pool provided by users as liquidity.
    #[storage_alias]
    pub type SubnetAlphaInProvided<T: Config> =
        StorageMap<Pallet<T>, Identity, NetUid, AlphaBalance, ValueQuery>;
}

pub fn migrate_cleanup_swap_v3<T: Config>() -> Weight {
    let migration_name = b"migrate_cleanup_swap_v3".to_vec();
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
        String::from_utf8_lossy(&migration_name),
    );

    // ------------------------------
    // Step 1: Move provided to reserves
    // ------------------------------
    for (netuid, tao_provided) in deprecated_swap_maps::SubnetTaoProvided::<T>::iter() {
        SubnetTAO::<T>::mutate(netuid, |total| {
            *total = total.saturating_add(tao_provided);
        });
    }
    for (netuid, alpha_provided) in deprecated_swap_maps::SubnetAlphaInProvided::<T>::iter() {
        SubnetAlphaIn::<T>::mutate(netuid, |total| {
            *total = total.saturating_add(alpha_provided);
        });
    }

    // ------------------------------
    // Step 2: Remove Map entries
    // ------------------------------
    remove_prefix::<T>("SubtensorModule", "SubnetTaoProvided", &mut weight);
    remove_prefix::<T>("SubtensorModule", "SubnetAlphaInProvided", &mut weight);

    // ------------------------------
    // Step 3: Mark Migration as Completed
    // ------------------------------

    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{:?}' completed successfully.",
        String::from_utf8_lossy(&migration_name)
    );

    weight
}
