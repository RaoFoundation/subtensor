//! One-shot migration: set [`OldestStoredRound`](crate::OldestStoredRound) to the minimum
//! key present in [`Pulses`](crate::Pulses) (or `0` if empty).

use crate::*;
use frame_support::weights::Weight;
use log;

/// Scan `Pulses` keys once and write the minimum as `OldestStoredRound`; idempotent via
/// [`HasMigrationRun`](crate::HasMigrationRun). Does not modify `LastStoredRound` or pulse data.
pub fn migrate_set_oldest_round<T: Config>() -> Weight {
    use frame_support::traits::Get;

    let migration_name = BoundedVec::truncate_from(b"migrate_set_oldest_round".to_vec());

    // Start with one read for HasMigrationRun
    let mut weight = T::DbWeight::get().reads(1);

    // Skip if already run.
    if HasMigrationRun::<T>::get(&migration_name) {
        log::info!(
            "Migration '{}' has already run. Skipping.",
            String::from_utf8_lossy(&migration_name)
        );
        return weight;
    }
    log::info!(
        "Running migration '{}'",
        String::from_utf8_lossy(&migration_name)
    );

    // Single-pass over keys: track min and how many keys we read.
    let mut reads: u64 = 0;
    let mut min_round: Option<RoundNumber> = None;

    for r in Pulses::<T>::iter_keys() {
        reads = reads.saturating_add(1);
        if min_round.is_none_or(|m| r < m) {
            min_round = Some(r);
        }
    }

    // Account for all key reads
    weight = weight.saturating_add(T::DbWeight::get().reads(reads));

    let oldest = min_round.unwrap_or(0u64);
    OldestStoredRound::<T>::put(oldest);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    // Mark as completed.
    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{}' completed. OldestStoredRound set to {} (scanned {} rounds).",
        String::from_utf8_lossy(&migration_name),
        oldest,
        reads
    );

    weight
}
