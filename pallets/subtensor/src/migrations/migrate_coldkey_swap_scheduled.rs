use super::*;
use crate::AccountIdOf;
use frame_support::{
    pallet_prelude::{Blake2_128Concat, ValueQuery},
    storage_alias,
    traits::Get,
    weights::Weight,
};
pub use frame_system::pallet_prelude::BlockNumberFor;
use scale_info::prelude::string::String;
/// Module containing deprecated storage format for LoadedEmission
pub mod deprecated_coldkey_swap_scheduled_format {
    use super::*;

    #[storage_alias]
    pub(super) type ColdkeySwapScheduled<T: Config> =
        StorageMap<Pallet<T>, Blake2_128Concat, AccountIdOf<T>, (), ValueQuery>;
}

/// Migrates `ColdkeySwapScheduled` into the post-schedule storage layout used by coldkey-swap scheduling.
///
/// Idempotency key (frozen): `migrate_coldkey_swap_scheduled`.
pub fn migrate_coldkey_swap_scheduled<T: Config>() -> Weight {
    use deprecated_coldkey_swap_scheduled_format as old;

    let migration_name = b"migrate_coldkey_swap_scheduled".to_vec();
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

    // ------------------------------
    // Step 1: Migrate ColdkeySwapScheduled map
    // ------------------------------

    let curr_keys: Vec<AccountIdOf<T>> = old::ColdkeySwapScheduled::<T>::iter_keys().collect();

    // Remove any undecodable entries
    for coldkey in &curr_keys {
        weight.saturating_accrue(T::DbWeight::get().reads(1));
        if old::ColdkeySwapScheduled::<T>::try_get(coldkey).is_err() {
            old::ColdkeySwapScheduled::<T>::remove(coldkey);
            log::warn!(
                "Was unable to decode old coldkey_swap_scheduled for coldkey {:?}",
                &coldkey
            );
        }
    }

    // ------------------------------
    // Step 2: Mark Migration as Completed
    // ------------------------------

    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{:?}' completed successfully.",
        String::from_utf8_lossy(&migration_name)
    );

    weight
}
