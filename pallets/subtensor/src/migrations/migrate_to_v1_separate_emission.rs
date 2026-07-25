use super::*;
use frame_support::{
    storage_alias,
    traits::{Get, GetStorageVersion, StorageVersion},
    weights::Weight,
};
use log::{info, warn};
use sp_std::vec::Vec;
use subtensor_runtime_common::NetUid;

/// Constant for logging purposes
const LOG_TARGET: &str = "loadedemissionmigration";
const LOG_TARGET_1: &str = "fixtotalstakestorage";

/// Module containing deprecated storage format
pub mod deprecated_loaded_emission_format {
    use super::*;

    type AccountIdOf<T> = <T as frame_system::Config>::AccountId;

    #[storage_alias]
    pub(super) type LoadedEmission<T: Config> =
        StorageMap<Pallet<T>, Identity, u16, Vec<(AccountIdOf<T>, u64)>, OptionQuery>;
}

/// Migrates legacy `LoadedEmission` into the separated emission storage format (storage version bump era).
///
/// Does not use [`HasMigrationRun`]; gated by pallet storage version in the body.
pub fn migrate_to_v1_separate_emission<T: Config>() -> Weight {
    use deprecated_loaded_emission_format as old;

    // Initialize weight counter
    let mut weight = T::DbWeight::get().reads_writes(1, 0);

    // Get current on-chain storage version
    let onchain_version = Pallet::<T>::on_chain_storage_version();

    // Only proceed if current version is less than 1
    if onchain_version < 1 {
        info!(
            target: LOG_TARGET,
            ">>> Updating the LoadedEmission to a new format {onchain_version:?}"
        );

        // Collect all network IDs (netuids) from old LoadedEmission storage
        let curr_loaded_emission: Vec<u16> = old::LoadedEmission::<T>::iter_keys().collect();

        // Remove any undecodable entries
        for netuid in curr_loaded_emission {
            weight.saturating_accrue(T::DbWeight::get().reads(1));
            if old::LoadedEmission::<T>::try_get(netuid).is_err() {
                weight.saturating_accrue(T::DbWeight::get().writes(1));
                old::LoadedEmission::<T>::remove(netuid);
                warn!("Was unable to decode old loaded_emission for netuid {netuid}");
            }
        }

        // Translate old storage values to new format
        LoadedEmission::<T>::translate::<Vec<(AccountIdOf<T>, u64)>, _>(
            |netuid: NetUid,
             netuid_emissions: Vec<(AccountIdOf<T>, u64)>|
             -> Option<Vec<(AccountIdOf<T>, u64, u64)>> {
                info!(target: LOG_TARGET, "     Do migration of netuid: {netuid:?}...");

                // Convert old format (server, validator_emission) to new format (server, server_emission, validator_emission)
                // Assume all loaded emission is validator emissions
                let new_netuid_emissions = netuid_emissions
                    .into_iter()
                    .map(|(server, validator_emission)| (server, 0_u64, validator_emission))
                    .collect();

                // Update weight for read and write operations
                weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 1));

                Some(new_netuid_emissions)
            },
        );

        // Update storage version to 1
        StorageVersion::new(1).put::<Pallet<T>>();
        weight.saturating_accrue(T::DbWeight::get().writes(1));

        weight
    } else {
        info!(target: LOG_TARGET_1, "Migration to v1 already completed!");
        Weight::zero()
    }
}

// TODO: Add unit tests for this migration
// TODO: Consider adding error handling for edge cases
// TODO: Verify that all possible states of the old format are handled correctly
