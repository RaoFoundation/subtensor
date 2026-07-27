//! One-shot clear of removed MevShield v1 maps and reset of [`CurrentKey`].

use crate::{Config, CurrentKey, HasMigrationRun};
use frame_support::pallet_prelude::{BoundedVec, Get, Weight};
use frame_support::storage::unhashed;
use scale_info::prelude::string::String;
use sp_io::hashing::twox_128;

/// Clears removed v1 storage items (`Submissions`, `KeyHashByBlock`) and resets `CurrentKey`.
///
/// Idempotent via [`HasMigrationRun`] key `"migrate_clear_v1_storage"`. Does not touch
/// [`NextKey`] / [`AuthorKeys`]. Storage prefix is the runtime pallet name `MevShield`.
pub fn migrate_clear_v1_storage<T: Config>() -> Weight {
    let migration_name = b"migrate_clear_v1_storage".to_vec();
    let bounded_name = BoundedVec::truncate_from(migration_name.clone());
    let mut weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(&bounded_name) {
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

    let pallet_prefix = twox_128("MevShield".as_bytes());

    // Clear removed storage maps.
    for name in ["Submissions", "KeyHashByBlock"] {
        let prefix = [pallet_prefix.as_slice(), &twox_128(name.as_bytes())].concat();
        let result = unhashed::clear_prefix(&prefix, Some(u32::MAX), None);
        weight = weight.saturating_add(T::DbWeight::get().writes(result.backend as u64));

        log::info!("Removed {} entries from {name:?}.", result.backend,);
    }

    // Reset current key.
    CurrentKey::<T>::kill();
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    HasMigrationRun::<T>::insert(&bounded_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{}' completed successfully.",
        String::from_utf8_lossy(&migration_name)
    );

    weight
}
