use super::*;
use frame_support::{
    pallet_prelude::{Identity, OptionQuery},
    storage_alias,
    traits::{GetStorageVersion, StorageVersion},
    weights::Weight,
};
use log::{error, info};
use sp_core::Get;
use sp_std::vec::Vec;
use subtensor_runtime_common::NetUid;

/// Constant for logging purposes
const LOG_TARGET: &str = "migrate_transfer_ownership";

/// Module containing deprecated storage format
pub mod deprecated_loaded_emission_format {
    use super::*;

    #[storage_alias]
    pub(super) type LoadedEmission<T: Config> =
        StorageMap<Pallet<T>, Identity, u16, Vec<(AccountIdOf<T>, u64)>, OptionQuery>;
}

/// Transfers subnet ownership (`SubnetOwner`) to the foundation coldkey and updates related registration timestamps.
///
/// Does not use [`HasMigrationRun`]; caller supplies the 32-byte foundation coldkey.
pub fn migrate_transfer_ownership_to_foundation<T: Config>(coldkey: [u8; 32]) -> Weight {
    let new_storage_version = 3;

    // Start with one read (on-chain storage version).
    let mut weight = T::DbWeight::get().reads(1);

    // Get current on-chain storage version
    let onchain_version = Pallet::<T>::on_chain_storage_version();

    // Only proceed if current version is less than the new version
    if onchain_version < new_storage_version {
        info!(
            target: LOG_TARGET,
            "Migrating subnet 1 and 11 to foundation control. Current version: {onchain_version:?}"
        );

        // Decode the foundation's coldkey into an AccountId — if it fails, log and abort migration.
        let Ok(coldkey_account) = T::AccountId::decode(&mut &coldkey[..]) else {
            error!(
                target: LOG_TARGET,
                "migration error: failed to decode foundation coldkey from 32 bytes"
            );
            return weight;
        };
        info!(target: LOG_TARGET, "Foundation coldkey: {coldkey_account:?}");

        // Get the current block number
        let current_block = Pallet::<T>::get_current_block_as_u64();
        weight.saturating_accrue(T::DbWeight::get().reads(1));

        // Transfer ownership of subnets 1 and 11 to the foundation
        SubnetOwner::<T>::insert(NetUid::from(1), coldkey_account.clone());
        SubnetOwner::<T>::insert(NetUid::from(11), coldkey_account);

        // Set the registration time for subnet 1 to extend immunity period
        NetworkRegisteredAt::<T>::insert(NetUid::from(1), current_block.saturating_add(13 * 7200));
        // Set the registration time for subnet 11 to the current block
        NetworkRegisteredAt::<T>::insert(NetUid::from(11), current_block);

        weight.saturating_accrue(T::DbWeight::get().writes(4));

        // Update the storage version to prevent re-running this migration
        StorageVersion::new(new_storage_version).put::<Pallet<T>>();
        weight.saturating_accrue(T::DbWeight::get().writes(1));

        weight
    } else {
        info!(target: LOG_TARGET, "Migration to v3 already completed");
        Weight::zero()
    }
}
