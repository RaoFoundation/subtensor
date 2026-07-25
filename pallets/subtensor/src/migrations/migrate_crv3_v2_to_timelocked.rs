use super::*;
use frame_support::{traits::Get, weights::Weight};
use log;
use scale_info::prelude::string::String;
use sp_std::vec::Vec;

// --------------- Migration ------------------------------------------
/// Moves every (netuid, epoch) commit queue from `CRV3WeightCommitsV2` into `TimelockedWeightCommits`,
/// then clears the old map. Bridge from CRV3 v2 storage to the timelocked commit-reveal layout.
///
/// Idempotency key (frozen): `crv3_v2_to_timelocked_v1`.
pub fn migrate_crv3_v2_to_timelocked<T: Config>() -> Weight {
    let mig_name: Vec<u8> = b"crv3_v2_to_timelocked_v1".to_vec();
    let mut total_weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(&mig_name) {
        log::info!(
            "Migration '{}' already executed - skipping",
            String::from_utf8_lossy(&mig_name)
        );
        return total_weight;
    }
    log::info!("Running migration '{}'", String::from_utf8_lossy(&mig_name));

    for (netuid, epoch, old_q) in CRV3WeightCommitsV2::<T>::drain() {
        total_weight = total_weight.saturating_add(T::DbWeight::get().reads_writes(1, 1));
        TimelockedWeightCommits::<T>::insert(netuid, epoch, old_q);
    }

    HasMigrationRun::<T>::insert(&mig_name, true);
    total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{}' completed",
        String::from_utf8_lossy(&mig_name)
    );
    total_weight
}
