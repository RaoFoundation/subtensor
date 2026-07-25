use super::*;
use crate::HasMigrationRun;
use frame_support::pallet_prelude::ValueQuery;
use frame_support::storage_alias;
use frame_support::{traits::Get, weights::Weight};
use scale_info::prelude::string::String;

/// Module containing deprecated storage format for WeightsMinStake
pub mod deprecated_weights_min_stake {
    use super::*;

    #[storage_alias]
    pub(super) type WeightsMinStake<T: Config> = StorageValue<Pallet<T>, u64, ValueQuery>;
}

/// Copies deprecated `WeightsMinStake` into `StakeThreshold`, then kills the old storage value.
///
/// Idempotency key (frozen): `migrate_stake_threshold`.
pub fn migrate_stake_threshold<T: Config>() -> Weight {
    let migration_name = b"migrate_stake_threshold".to_vec();
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

    let min_stake = deprecated_weights_min_stake::WeightsMinStake::<T>::get();
    StakeThreshold::<T>::set(min_stake);
    deprecated_weights_min_stake::WeightsMinStake::<T>::kill();

    weight = weight.saturating_add(T::DbWeight::get().reads_writes(1, 1));

    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{:?}' completed successfully.",
        String::from_utf8_lossy(&migration_name)
    );

    weight
}
