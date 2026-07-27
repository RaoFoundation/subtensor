use super::*;
use frame_support::{
    storage_alias,
    traits::{Get, GetStorageVersion},
    weights::Weight,
};
use log::info;
use sp_std::vec::Vec;

/// Constant for logging purposes
const LOG_TARGET: &str = "fix_total_stake_storage";

/// Module containing deprecated storage format
pub mod deprecated_loaded_emission_format {
    use super::*;

    #[storage_alias]
    pub(super) type LoadedEmission<T: Config> =
        StorageMap<Pallet<T>, Identity, u16, Vec<(AccountIdOf<T>, u64)>, OptionQuery>;
}

/// Resets and recalculates `TotalStake` and `TotalColdkeyStake` from the (then-current) `Stake` map.
///
/// Does not use [`HasMigrationRun`]; intended as a storage-version migration.
pub fn migrate_to_v2_fixed_total_stake<T: Config>() -> Weight {
    let new_storage_version = 2;

    // Initialize weight counter
    let weight = T::DbWeight::get().reads(1);

    // Get current on-chain storage version
    let onchain_version = Pallet::<T>::on_chain_storage_version();

    // Only proceed if current version is less than the new version
    if onchain_version < new_storage_version {
        info!(
            target: LOG_TARGET,
            "Fixing the TotalStake and TotalColdkeyStake storage. Current version: {onchain_version:?}"
        );

        // TODO: Fix or remove migration
        // // Reset TotalStake to 0
        // TotalStake::<T>::put(0);
        // weight.saturating_accrue(T::DbWeight::get().writes(1));

        // // Reset all TotalColdkeyStake entries to 0
        // let total_coldkey_stake_keys = TotalColdkeyStake::<T>::iter_keys().collect::<Vec<_>>();
        // for coldkey in total_coldkey_stake_keys {
        //     weight.saturating_accrue(T::DbWeight::get().reads(1));
        //     TotalColdkeyStake::<T>::insert(coldkey, 0);
        //     weight.saturating_accrue(T::DbWeight::get().writes(1));
        // }

        // // Recalculate TotalStake and TotalColdkeyStake based on the Stake map
        // for (_, coldkey, stake) in Stake::<T>::iter() {
        //     weight.saturating_accrue(T::DbWeight::get().reads(1));

        //     // Update TotalColdkeyStake
        //     let mut total_coldkey_stake = TotalColdkeyStake::<T>::get(coldkey.clone());
        //     weight.saturating_accrue(T::DbWeight::get().reads(1));
        //     total_coldkey_stake = total_coldkey_stake.saturating_add(stake);
        //     TotalColdkeyStake::<T>::insert(coldkey, total_coldkey_stake);
        //     weight.saturating_accrue(T::DbWeight::get().writes(1));

        //     // Update TotalStake
        //     let mut total_stake = TotalStake::<T>::get();
        //     weight.saturating_accrue(T::DbWeight::get().reads(1));
        //     total_stake = total_stake.saturating_add(stake);
        //     TotalStake::<T>::put(total_stake);
        //     weight.saturating_accrue(T::DbWeight::get().writes(1));
        // }

        // // Update storage version to prevent re-running this migration
        // StorageVersion::new(new_storage_version).put::<Pallet<T>>();
        // weight.saturating_accrue(T::DbWeight::get().writes(1));

        weight
    } else {
        info!(target: LOG_TARGET, "Migration to v2 already completed");
        Weight::zero()
    }
}

// TODO: Add unit tests for this migration function
// TODO: Consider adding error handling for potential arithmetic overflow
// TODO: Optimize the iteration over Stake map if possible to reduce database reads
