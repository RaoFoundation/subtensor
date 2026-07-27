//! One-shot migration: trim [`Pulses`](crate::Pulses) to [`MAX_KEPT_PULSES`](crate::MAX_KEPT_PULSES)
//! and set [`OldestStoredRound`](crate::OldestStoredRound) / [`LastStoredRound`](crate::LastStoredRound).

use crate::*;
use frame_support::{traits::Get, weights::Weight};
use log;

/// Prune excess historical pulses and refresh round watermarks; idempotent via [`HasMigrationRun`].
pub fn migrate_prune_old_pulses<T: Config>() -> Weight {
    let migration_name = BoundedVec::truncate_from(b"migrate_prune_old_pulses".to_vec());

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

    // Collect all round numbers
    let mut rounds: Vec<RoundNumber> = Pulses::<T>::iter_keys().collect();
    weight = weight.saturating_add(T::DbWeight::get().reads(rounds.len() as u64));

    if rounds.is_empty() {
        OldestStoredRound::<T>::put(0u64);
        LastStoredRound::<T>::put(0u64);
        weight = weight.saturating_add(T::DbWeight::get().writes(2));
    } else {
        rounds.sort();
        let num_pulses = rounds.len() as u64;

        let mut new_oldest = rounds[0];
        if num_pulses > MAX_KEPT_PULSES {
            let num_to_delete = num_pulses.saturating_sub(MAX_KEPT_PULSES);
            new_oldest = rounds[num_to_delete as usize];

            for &round in &rounds[0..num_to_delete as usize] {
                Pulses::<T>::remove(round);
                weight = weight.saturating_add(T::DbWeight::get().writes(1));
            }
        }

        OldestStoredRound::<T>::put(new_oldest);
        LastStoredRound::<T>::put(*rounds.last().unwrap());
        weight = weight.saturating_add(T::DbWeight::get().writes(2));
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
