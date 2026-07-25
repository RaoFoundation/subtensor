use super::*;
use crate::HasMigrationRun;
use frame_support::{traits::Get, weights::Weight};
use scale_info::prelude::string::String;

/// Force-disables `CommitRevealWeightsEnabled` for every subnet (emergency / cutover helper).
///
/// Idempotency key (frozen): `disable_commit_reveal_v1`.
pub fn migrate_disable_commit_reveal<T: Config>() -> Weight {
    const MIG_NAME: &[u8] = b"disable_commit_reveal_v1";

    // 1 ─ check if we already ran
    if HasMigrationRun::<T>::get(MIG_NAME) {
        log::info!(
            "Migration '{}' already executed - skipping",
            String::from_utf8_lossy(MIG_NAME)
        );
        return T::DbWeight::get().reads(1);
    }

    log::info!("Running migration '{}'", String::from_utf8_lossy(MIG_NAME));

    let mut total_weight = T::DbWeight::get().reads(1);

    // 2 ─ iterate over every stored key and set value -> false
    for (netuid, _) in CommitRevealWeightsEnabled::<T>::drain() {
        CommitRevealWeightsEnabled::<T>::insert(netuid, false);
        total_weight = total_weight.saturating_add(T::DbWeight::get().reads_writes(1, 1));
    }

    // 3 ─ mark migration as done
    HasMigrationRun::<T>::insert(MIG_NAME, true);
    total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{}' completed: commit-reveal disabled on all subnets",
        String::from_utf8_lossy(MIG_NAME)
    );
    total_weight
}
