use super::*;
use frame_support::{traits::Get, weights::Weight};
use log;
use scale_info::prelude::string::String;
use subtensor_runtime_common::NetUid;

/// Root weight-set rate limit installed at launch: one set per root tempo (100 blocks,
/// ~20 minutes), matching `DefaultWeightsSetRateLimit`. `set_root_weights` is `Pays::No`,
/// so this rate limit is the anti-spam gate. The explicit write is required: mainnet's
/// netuid-0 entry holds `u64::MAX` (set when legacy root weights were frozen), under
/// which `check_rate_limit` can never pass once a uid's `LastUpdateForUid` is stamped —
/// and registration stamps it, so `set_root_weights` would fail for everyone, forever.
pub const ROOT_WEIGHTS_SET_RATE_LIMIT: u64 = 100;

/// Opens root basket curation: flips [`crate::RootWeightSettingEnabled`] on so root
/// validators can call `set_root_weights`, pins the concentration cap
/// [`crate::RootWeightsCap`] to 1/16 (`DEFAULT_ROOT_WEIGHTS_CAP`), so no single
/// destination may take more than 1/16 of a basket vector — a fund must spread across
/// at least 16 destinations — and replaces the frozen `u64::MAX` root weights rate
/// limit with [`ROOT_WEIGHTS_SET_RATE_LIMIT`] so the gate it opens is actually usable.
/// The explicit cap write makes the launch value visible in storage rather than
/// relying on the default.
pub fn migrate_enable_root_weight_setting<T: Config>() -> Weight {
    let mig_name: Vec<u8> = b"enable_root_weight_setting_v1".to_vec();
    let mut total_weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(&mig_name) {
        log::info!(
            "Migration '{}' already executed - skipping",
            String::from_utf8_lossy(&mig_name)
        );
        return total_weight;
    }
    log::info!("Running migration '{}'", String::from_utf8_lossy(&mig_name));

    RootWeightSettingEnabled::<T>::put(true);
    RootWeightsCap::<T>::insert(NetUid::ROOT, crate::DEFAULT_ROOT_WEIGHTS_CAP);
    WeightsSetRateLimit::<T>::insert(NetUid::ROOT, ROOT_WEIGHTS_SET_RATE_LIMIT);
    total_weight = total_weight.saturating_add(T::DbWeight::get().writes(3));

    HasMigrationRun::<T>::insert(&mig_name, true);
    total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{}' completed",
        String::from_utf8_lossy(&mig_name)
    );
    total_weight
}
