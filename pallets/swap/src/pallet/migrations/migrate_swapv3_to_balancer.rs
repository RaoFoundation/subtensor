use super::*;
use crate::HasMigrationRun;
use frame_support::{storage_alias, traits::Get, weights::Weight};
use scale_info::prelude::string::String;
use substrate_fixed::types::U64F64;

pub mod deprecated_swap_maps {
    use super::*;

    #[storage_alias]
    pub type AlphaSqrtPrice<T: Config> =
        StorageMap<Pallet<T>, Twox64Concat, NetUid, U64F64, ValueQuery>;

    /// TAO reservoir for scraps of protocol claimed fees.
    #[storage_alias]
    pub type ScrapReservoirTao<T: Config> =
        StorageMap<Pallet<T>, Twox64Concat, NetUid, TaoBalance, ValueQuery>;

    /// Alpha reservoir for scraps of protocol claimed fees.
    #[storage_alias]
    pub type ScrapReservoirAlpha<T: Config> =
        StorageMap<Pallet<T>, Twox64Concat, NetUid, AlphaBalance, ValueQuery>;
}

pub fn migrate_swapv3_to_balancer<T: Config>() -> Weight {
    let migration_name = BoundedVec::truncate_from(b"migrate_swapv3_to_balancer".to_vec());
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
    // Step 1: Initialize swaps with price before price removal
    // ------------------------------
    for (netuid, price_sqrt) in deprecated_swap_maps::AlphaSqrtPrice::<T>::iter() {
        let price = price_sqrt.saturating_mul(price_sqrt);
        if let Err(error) = crate::Pallet::<T>::maybe_initialize_palswap(netuid, Some(price)) {
            log::warn!(
                "Migration '{}' failed to initialize balancer with V3 price for netuid {}: {:?}. Falling back to default balancer.",
                String::from_utf8_lossy(&migration_name),
                netuid,
                error,
            );
            SwapBalancer::<T>::insert(netuid, Balancer::default());
            PalSwapInitialized::<T>::insert(netuid, true);
            weight = weight.saturating_add(T::DbWeight::get().writes(2));
        }
    }

    // ------------------------------
    // Step 2: Clear Map entries
    // ------------------------------
    remove_prefix::<T>("Swap", "AlphaSqrtPrice", &mut weight);
    remove_prefix::<T>("Swap", "CurrentTick", &mut weight);
    remove_prefix::<T>("Swap", "EnabledUserLiquidity", &mut weight);
    remove_prefix::<T>("Swap", "FeeGlobalTao", &mut weight);
    remove_prefix::<T>("Swap", "FeeGlobalAlpha", &mut weight);
    remove_prefix::<T>("Swap", "LastPositionId", &mut weight);
    // Scrap reservoirs can be just cleaned because they are already included in reserves
    remove_prefix::<T>("Swap", "ScrapReservoirTao", &mut weight);
    remove_prefix::<T>("Swap", "ScrapReservoirAlpha", &mut weight);
    remove_prefix::<T>("Swap", "Ticks", &mut weight);
    remove_prefix::<T>("Swap", "TickIndexBitmapWords", &mut weight);
    remove_prefix::<T>("Swap", "SwapV3Initialized", &mut weight);
    remove_prefix::<T>("Swap", "CurrentLiquidity", &mut weight);
    remove_prefix::<T>("Swap", "Positions", &mut weight);

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
