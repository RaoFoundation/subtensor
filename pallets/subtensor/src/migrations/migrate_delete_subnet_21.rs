use super::*;
use frame_support::{
    storage_alias,
    traits::{Get, GetStorageVersion, StorageVersion},
    weights::Weight,
};
use log::info;
use sp_std::vec::Vec;
use subtensor_runtime_common::{NetUid, NetUidStorageIndex};

/// Constant for logging purposes
const LOG_TARGET: &str = "migrate_delete_subnet_21";

/// Module containing deprecated storage format
pub mod deprecated_loaded_emission_format {
    use super::*;

    #[storage_alias]
    pub(super) type LoadedEmission<T: Config> =
        StorageMap<Pallet<T>, Identity, u16, Vec<(AccountIdOf<T>, u64)>, OptionQuery>;
}

/// Migrates the storage to delete subnet 21
///
/// This function performs the following steps:
/// 1. Checks if the migration is necessary
/// 2. Removes all storage related to subnet 21
/// 3. Updates the storage version
///
/// # Arguments
///
/// * `T` - The Config trait of the pallet
///
/// # Returns
///
/// * `Weight` - The computational weight of this operation
///
/// # Example
///
/// ```ignore
/// let weight = migrate_delete_subnet_21::<T>();
/// ```
pub fn migrate_delete_subnet_21<T: Config>() -> Weight {
    let new_storage_version = 4;

    // Setup migration weight
    let mut weight = T::DbWeight::get().reads(1);

    // Grab current version
    let onchain_version = Pallet::<T>::on_chain_storage_version();

    // Only runs if we haven't already updated version past above new_storage_version and subnet 21 exists.
    if onchain_version < new_storage_version && Pallet::<T>::if_subnet_exist(NetUid::from(21)) {
        info!(target: LOG_TARGET, ">>> Removing subnet 21 {onchain_version:?}");

        let netuid = NetUid::from(21);

        // We do this all manually as we don't want to call code related to giving subnet owner back their locked token cost.
        // Remove network count
        SubnetworkN::<T>::remove(netuid);

        // Remove netuid from added networks
        NetworksAdded::<T>::remove(netuid);

        // Decrement the network counter
        TotalNetworks::<T>::mutate(|n| *n = n.saturating_sub(1));

        // Remove network registration time
        NetworkRegisteredAt::<T>::remove(netuid);

        weight.saturating_accrue(T::DbWeight::get().writes(5));

        // Remove incentive mechanism memory
        let _ = Uids::<T>::clear_prefix(netuid, u32::MAX, None);
        let _ = Keys::<T>::clear_prefix(netuid, u32::MAX, None);
        let _ = Bonds::<T>::clear_prefix(NetUidStorageIndex::from(netuid), u32::MAX, None);
        let _ = Weights::<T>::clear_prefix(NetUidStorageIndex::from(netuid), u32::MAX, None);

        weight.saturating_accrue(T::DbWeight::get().writes(4));

        // Remove various network-related parameters
        Active::<T>::remove(netuid);
        Emission::<T>::remove(netuid);
        Incentive::<T>::remove(NetUidStorageIndex::from(netuid));
        Consensus::<T>::remove(netuid);
        Dividends::<T>::remove(netuid);
        LastUpdate::<T>::remove(NetUidStorageIndex::from(netuid));
        ValidatorPermit::<T>::remove(netuid);
        ValidatorTrust::<T>::remove(netuid);

        weight.saturating_accrue(T::DbWeight::get().writes(8));

        // Erase network parameters
        Tempo::<T>::remove(netuid);
        Kappa::<T>::remove(netuid);
        Difficulty::<T>::remove(netuid);
        MaxAllowedUids::<T>::remove(netuid);
        ImmunityPeriod::<T>::remove(netuid);
        ActivityCutoff::<T>::remove(netuid);
        MinAllowedWeights::<T>::remove(netuid);
        RegistrationsThisInterval::<T>::remove(netuid);
        POWRegistrationsThisInterval::<T>::remove(netuid);
        BurnRegistrationsThisInterval::<T>::remove(netuid);

        weight.saturating_accrue(T::DbWeight::get().writes(10));

        // Update storage version
        StorageVersion::new(new_storage_version).put::<Pallet<T>>();
        weight.saturating_accrue(T::DbWeight::get().writes(1));

        weight
    } else {
        info!(target: LOG_TARGET, "Migration to v4 already done or subnet 21 doesn't exist!");
        Weight::zero()
    }
}

// TODO: Add unit tests for this migration
// TODO: Consider adding error handling for storage operations
// TODO: Verify that all relevant storage items for subnet 21 are removed
