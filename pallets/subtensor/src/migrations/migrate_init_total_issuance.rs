use super::*;
use frame_support::pallet_prelude::{Identity, OptionQuery, Weight};
use frame_support::storage_alias;
use sp_std::vec::Vec;

// TODO: Implement comprehensive tests for this migration

/// Module containing deprecated storage format for LoadedEmission
pub mod deprecated_loaded_emission_format {
    use super::*;

    #[storage_alias]
    pub(super) type LoadedEmission<T: Config> =
        StorageMap<Pallet<T>, Identity, u16, Vec<(AccountIdOf<T>, u64)>, OptionQuery>;
}

/// This on-going migration is disabled as part of imbalances work.
pub(crate) fn migrate_init_total_issuance<T: Config>() -> Weight {
    // let subnets_len = crate::NetworksAdded::<T>::iter().count() as u64;

    // // Retrieve the total balance of all accounts
    // let total_account_balances = <<T as crate::Config>::Currency as fungible::Inspect<
    //     <T as frame_system::Config>::AccountId,
    // >>::total_issuance();

    // // Get the total stake from the system
    // let prev_total_stake = crate::TotalStake::<T>::get();

    // // Calculate new total stake using the sum of all subnet TAO
    // let total_subnet_tao =
    //     crate::SubnetTAO::<T>::iter().fold(TaoBalance::ZERO, |acc, (_, v)| acc.saturating_add(v));

    // let total_stake = total_subnet_tao;
    // // Update the total stake in storage
    // crate::TotalStake::<T>::put(total_stake);
    // log::info!(
    //     "Subtensor Pallet Total Stake Updated: previous: {prev_total_stake:?}, new: {total_stake:?}"
    // );
    // // Retrieve the previous total issuance for logging purposes
    // let prev_total_issuance = crate::TotalIssuance::<T>::get();

    // // Calculate the new total issuance
    // let new_total_issuance: TaoBalance = total_account_balances.saturating_add(total_stake).into();

    // // Update the total issuance in storage
    // crate::TotalIssuance::<T>::put(new_total_issuance);

    // // Log the change in total issuance
    // log::info!(
    //     "Subtensor Pallet Total Issuance Updated: previous: {prev_total_issuance:?}, new: {new_total_issuance:?}"
    // );

    // // Return the weight of the operation
    // // We performed subnets_len + 5 reads and 1 write
    // <T as frame_system::Config>::DbWeight::get().reads_writes(subnets_len.saturating_add(5), 2)

    log::info!("Subtensor Pallet Total Issuance ongoing update migration is disabled.");
    T::DbWeight::get().reads(0)
}

/// This on-going migration is disabled as part of imbalances work.
pub(crate) fn migrate_init_total_issuance_once<T: Config>() -> Weight {
    let migration_name = b"migrate_init_total_issuance_once".to_vec();
    let weight = T::DbWeight::get().reads(1);

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

    ////////////////////////////////////////////////////////
    // Actual migration
    let subnets_len = crate::NetworksAdded::<T>::iter().count() as u64;

    // Retrieve the total balance of all accounts
    let total_account_balances = <<T as crate::Config>::Currency as fungible::Inspect<
        <T as frame_system::Config>::AccountId,
    >>::total_issuance();

    // Get the total stake from the system
    let prev_total_stake = crate::TotalStake::<T>::get();

    // Calculate new total stake using the sum of all subnet TAO
    let total_subnet_tao =
        crate::SubnetTAO::<T>::iter().fold(TaoBalance::ZERO, |acc, (_, v)| acc.saturating_add(v));

    let total_stake = total_subnet_tao;
    // Update the total stake in storage
    crate::TotalStake::<T>::put(total_stake);
    log::info!(
        "Subtensor Pallet Total Stake Updated: previous: {prev_total_stake:?}, new: {total_stake:?}"
    );
    // Retrieve the previous total issuance for logging purposes
    let prev_total_issuance = crate::TotalIssuance::<T>::get();

    // Calculate the new total issuance
    let new_total_issuance: TaoBalance = total_account_balances.saturating_add(total_stake).into();

    // Update the total issuance in storage
    crate::TotalIssuance::<T>::put(new_total_issuance);

    // Log the change in total issuance
    log::info!(
        "Subtensor Pallet Total Issuance Updated: previous: {prev_total_issuance:?}, new: {new_total_issuance:?}"
    );

    ////////////////////////////////////////////////////////

    HasMigrationRun::<T>::insert(&migration_name, true);

    log::info!(
        target: "runtime",
        "Migration '{}' completed successfully.",
        String::from_utf8_lossy(&migration_name)
    );

    // Return the weight of the operation
    // We performed subnets_len + 5 reads and 1 write
    <T as frame_system::Config>::DbWeight::get().reads_writes(subnets_len.saturating_add(6), 3)
}

pub mod initialise_total_issuance {
    use frame_support::pallet_prelude::Weight;
    use frame_support::traits::OnRuntimeUpgrade;

    use crate::*;

    pub struct Migration<T: Config>(PhantomData<T>);

    impl<T: Config> OnRuntimeUpgrade for Migration<T> {
        /// Performs the migration to initialize and update the total issuance.
        ///
        /// This function does the following:
        /// 1. Calculates the total locked tokens across all subnets
        /// 2. Retrieves the total account balances and total stake
        /// 3. Computes and updates the new total issuance
        ///
        /// Returns the weight of the migration operation.
        fn on_runtime_upgrade() -> Weight {
            let mut weight = super::migrate_init_total_issuance::<T>();
            weight.saturating_accrue(super::migrate_init_total_issuance_once::<T>());
            weight
        }

        /// Performs post-upgrade checks to ensure the migration was successful.
        ///
        /// This function is only compiled when the "try-runtime" feature is enabled.
        #[cfg(feature = "try-runtime")]
        fn post_upgrade(_state: Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
            Ok(())
        }
    }
}
