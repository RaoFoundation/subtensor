use super::*;
use frame_support::pallet_prelude::OptionQuery;
use frame_support::{
    pallet_prelude::Identity,
    storage_alias,
    traits::{Get, GetStorageVersion, StorageVersion, fungible::Inspect},
    weights::Weight,
};
use sp_std::vec::Vec;

// TODO: Implement comprehensive tests for this migration

/// Module containing deprecated storage format for LoadedEmission
pub mod deprecated_loaded_emission_format {
    use super::*;

    #[storage_alias]
    pub(super) type LoadedEmission<T: Config> =
        StorageMap<Pallet<T>, Identity, u16, Vec<(AccountIdOf<T>, u64)>, OptionQuery>;
}

/// Note: This migration is now disabled. We needed it to sync up two different total issuance counters:
///   1. Balances pallet
///   2. Subtensor pallet
/// Now that two total issuances are naturally synched, it is not needed anymore, and it will lead to an 
/// incorrect state if it runs.
/// 
/// Performs migration to update the total issuance based on the sum of stakes and total balances.
///
/// This migration is applicable only if the current storage version is 5, after which it updates the storage version to 6.
///
/// # Arguments
///
/// * `test` - A boolean flag to force migration execution for testing purposes.
///
/// # Returns
///
/// * `Weight` - The computational weight of this operation.
///
/// # Example
///
/// ```ignore
///  let weight = migrate_total_issuance::<Runtime>(false);
/// ```
pub fn migrate_total_issuance<T: Config>(test: bool) -> Weight {
    // // Initialize migration weight with the cost of reading the storage version
    // let mut weight = T::DbWeight::get().reads(1);

    // // Execute migration if the current storage version is 5 or if in test mode
    // if Pallet::<T>::on_chain_storage_version() == StorageVersion::new(5) || test {
    //     // Calculate the sum of all stake values
    //     let stake_sum = Owner::<T>::iter()
    //         .map(|(hotkey, _coldkey)| Pallet::<T>::get_total_stake_for_hotkey(&hotkey))
    //         .fold(TaoBalance::ZERO, |acc, stake| acc.saturating_add(stake));
    //     // Add weight for reading all Owner and TotalHotkeyStake entries
    //     weight = weight.saturating_add(
    //         T::DbWeight::get().reads((Owner::<T>::iter().count() as u64).saturating_mul(2)),
    //     );

    //     // Retrieve the total balance sum
    //     let total_balance = <T as Config>::Currency::total_issuance();
    //     // Add weight for reading total issuance
    //     weight = weight.saturating_add(T::DbWeight::get().reads(1));

    //     // Attempt to convert total balance to u64
    //     match TryInto::<u64>::try_into(total_balance) {
    //         Ok(total_balance_sum) => {
    //             // Compute the total issuance value
    //             let total_issuance_value = stake_sum
    //                 .saturating_add(total_balance_sum.into());

    //             // Update the total issuance in storage
    //             TotalIssuance::<T>::put(total_issuance_value);

    //             // Update the storage version to 6
    //             StorageVersion::new(6).put::<Pallet<T>>();

    //             // Add weight for writing total issuance and storage version
    //             weight = weight.saturating_add(T::DbWeight::get().writes(2));
    //         }
    //         Err(_) => {
    //             // TODO: Implement proper error handling for conversion failure
    //             log::error!("Failed to convert total balance to u64, migration aborted");
    //         }
    //     }
    // }

    // // Return the computed weight of the migration process
    // weight

    T::DbWeight::get().reads(0)
}
