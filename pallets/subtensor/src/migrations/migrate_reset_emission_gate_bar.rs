use super::*;
use frame_support::{traits::Get, weights::Weight};
use log;
use scale_info::prelude::string::String;

/// Kills the emission gate bar so it is recomputed on the first block after
/// the upgrade. Without this, the stale quantile-derived theta would keep
/// gating emissions for up to EMISSION_BAR_UPDATE_INTERVAL blocks before the
/// new rank-64 bar (DefaultEmissionBarRank) takes effect.
pub fn migrate_reset_emission_gate_bar<T: Config>() -> Weight {
    let mig_name: Vec<u8> = b"reset_emission_gate_bar_rank_64".to_vec();

    // 1 read: HasMigrationRun flag
    let mut total_weight = T::DbWeight::get().reads(1);

    // Run once guard
    if HasMigrationRun::<T>::get(&mig_name) {
        log::info!(
            "Migration '{}' already executed - skipping",
            String::from_utf8_lossy(&mig_name)
        );
        return total_weight;
    }
    log::info!("Running migration '{}'", String::from_utf8_lossy(&mig_name));

    EmissionGateBar::<T>::kill();
    total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));

    // Mark as done
    HasMigrationRun::<T>::insert(&mig_name, true);
    total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{}' completed",
        String::from_utf8_lossy(&mig_name)
    );
    total_weight
}
