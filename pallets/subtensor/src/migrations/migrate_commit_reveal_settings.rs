use alloc::string::String;

use crate::MIN_COMMIT_REVEAL_PEROIDS;
use frame_support::IterableStorageMap;
use frame_support::{traits::Get, weights::Weight};
use subtensor_runtime_common::NetUid;

use super::*;

/// Enables commit-reveal weights on all non-root subnets and ensures `RevealPeriodEpochs` is at least
/// `MIN_COMMIT_REVEAL_PEROIDS` when previously zero.
///
/// Idempotency key (frozen): `migrate_commit_reveal_settings`.
pub fn migrate_commit_reveal_settings<T: Config>() -> Weight {
    let migration_name = b"migrate_commit_reveal_settings".to_vec();

    // Initialize the weight with one read operation.
    let mut weight = T::DbWeight::get().reads(1);

    // Check if the migration has already run
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

    let netuids: Vec<NetUid> = <NetworksAdded<T> as IterableStorageMap<NetUid, bool>>::iter()
        .map(|(netuid, _)| netuid)
        .collect();
    weight = weight.saturating_add(
        T::DbWeight::get()
            .reads(netuids.len() as u64)
            .saturating_mul(2),
    );

    for netuid in netuids.iter() {
        if netuid.is_root() {
            continue;
        }
        if !CommitRevealWeightsEnabled::<T>::get(*netuid) {
            CommitRevealWeightsEnabled::<T>::insert(*netuid, true);
            weight = weight.saturating_add(T::DbWeight::get().writes(1));
        }

        if RevealPeriodEpochs::<T>::get(*netuid) == 0 {
            RevealPeriodEpochs::<T>::insert(*netuid, MIN_COMMIT_REVEAL_PEROIDS);
            weight = weight.saturating_add(T::DbWeight::get().writes(1));
        }
    }

    // Mark the migration as completed
    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{:?}' completed.",
        String::from_utf8_lossy(&migration_name)
    );

    // Return the migration weight.
    weight
}
