use alloc::string::String;
use alloc::vec::Vec;
use frame_support::{traits::Get, weights::Weight};

use crate::{Config, HasMigrationRun, LastRateLimitedBlock, RateLimitKey};

const MIGRATION_NAME: &[u8] = b"migrate_remove_add_stake_burn_rate_limit";

pub fn migrate_remove_add_stake_burn_rate_limit<T: Config>() -> Weight {
    let mut weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(MIGRATION_NAME) {
        log::info!(
            "Migration '{}' already executed - skipping",
            String::from_utf8_lossy(MIGRATION_NAME)
        );
        return weight;
    }

    log::info!(
        "Running migration '{}'",
        String::from_utf8_lossy(MIGRATION_NAME)
    );

    let mut scanned_count = 0u64;
    let keys_to_remove = LastRateLimitedBlock::<T>::iter_keys()
        .filter_map(|key| {
            scanned_count = scanned_count.saturating_add(1);
            matches!(key, RateLimitKey::AddStakeBurn(_)).then_some(key)
        })
        .collect::<Vec<_>>();
    let removed_count = keys_to_remove.len() as u64;

    weight = weight.saturating_add(T::DbWeight::get().reads(scanned_count));

    for key in &keys_to_remove {
        LastRateLimitedBlock::<T>::remove(key);
    }

    weight = weight.saturating_add(T::DbWeight::get().writes(removed_count));

    HasMigrationRun::<T>::insert(MIGRATION_NAME, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{}' completed. scanned_entries={}, removed_add_stake_burn_entries={}",
        String::from_utf8_lossy(MIGRATION_NAME),
        scanned_count,
        removed_count
    );

    weight
}
