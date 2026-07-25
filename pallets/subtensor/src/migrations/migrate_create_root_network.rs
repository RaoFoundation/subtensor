use super::*;
use frame_support::{
    pallet_prelude::{Identity, OptionQuery},
    storage_alias,
    traits::Get,
    weights::Weight,
};
use sp_std::vec::Vec;
use subtensor_runtime_common::NetUid;

// TODO (camfairchild): TEST MIGRATION

/// Module containing deprecated storage format for LoadedEmission
pub mod deprecated_loaded_emission_format {
    use super::*;

    #[storage_alias]
    pub(super) type LoadedEmission<T: Config> =
        StorageMap<Pallet<T>, Identity, u16, Vec<(AccountIdOf<T>, u64)>, OptionQuery>;
}

/// Creates the root subnet (`NetUid::ROOT`) with bootstrap hyperparameters if it is not already present.
///
/// Sets max uids/validators to 64, tempo 100, registration open, and target registrations to 1 per interval.
/// No-op when `NetworksAdded(ROOT)` is already true. Does not use [`HasMigrationRun`].
pub fn migrate_create_root_network<T: Config>() -> Weight {
    // Initialize weight counter
    let mut weight = T::DbWeight::get().reads(1);

    // Check if root network already exists
    if NetworksAdded::<T>::get(NetUid::ROOT) {
        // Return early if root network already exists
        return weight;
    }

    // Set the root network as added
    NetworksAdded::<T>::insert(NetUid::ROOT, true);

    // Increment the total number of networks
    TotalNetworks::<T>::mutate(|n| *n = n.saturating_add(1));

    // Set the maximum number of UIDs to the number of senate members
    MaxAllowedUids::<T>::insert(NetUid::ROOT, 64);

    // Set the maximum number of validators to all members
    MaxAllowedValidators::<T>::insert(NetUid::ROOT, 64);

    // Set the minimum allowed weights to zero (no weight restrictions)
    MinAllowedWeights::<T>::insert(NetUid::ROOT, 0);

    // Set default root tempo
    Tempo::<T>::insert(NetUid::ROOT, 100);

    // Set the root network as open for registration
    NetworkRegistrationAllowed::<T>::insert(NetUid::ROOT, true);

    // Set target registrations for validators as 1 per block
    TargetRegistrationsPerInterval::<T>::insert(NetUid::ROOT, 1);

    // TODO: Consider if WeightsSetRateLimit should be set
    // WeightsSetRateLimit::<T>::insert(NetUid::ROOT, 7200);

    // Accrue weight for database writes
    weight.saturating_accrue(T::DbWeight::get().writes(7));

    // Remove all existing triumvirate votes and senate members
    remove_prefix::<T>("Triumvirate", "Votes", &mut weight);
    remove_prefix::<T>("SenateMembers", "Members", &mut weight);

    log::info!("Migrated create root network");
    weight
}
