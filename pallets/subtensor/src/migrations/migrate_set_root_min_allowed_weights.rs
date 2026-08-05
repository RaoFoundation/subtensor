use super::*;
use frame_support::{traits::Get, weights::Weight};
use log;
use scale_info::prelude::string::String;
use subtensor_runtime_common::NetUid;

/// Sets root `MinAllowedWeights` to [`crate::MIN_ROOT_BASKET_WEIGHTS`] so `set_root_weights`
/// requires a diversified basket vector (softened when fewer destinations exist).
pub fn migrate_set_root_min_allowed_weights<T: Config>() -> Weight {
    let mig_name: Vec<u8> = b"set_root_min_allowed_weights_8".to_vec();
    let mut total_weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(&mig_name) {
        log::info!(
            "Migration '{}' already executed - skipping",
            String::from_utf8_lossy(&mig_name)
        );
        return total_weight;
    }
    log::info!("Running migration '{}'", String::from_utf8_lossy(&mig_name));

    MinAllowedWeights::<T>::insert(NetUid::ROOT, crate::MIN_ROOT_BASKET_WEIGHTS);
    total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));

    HasMigrationRun::<T>::insert(&mig_name, true);
    total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{}' completed",
        String::from_utf8_lossy(&mig_name)
    );
    total_weight
}
