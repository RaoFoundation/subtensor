use super::*;
use frame_support::{traits::Get, weights::Weight};
use log;
use scale_info::prelude::string::String;
use subtensor_runtime_common::{NetUid, TaoBalance};

/// Align root admission with a normal subnet's rate limits, plus a 1 TAO burn
/// floor: 1 registration per block, 2 per interval (hard cap 6 per tempo),
/// 7200-block immunity (~1 day), and `MinBurn` / current `Burn` at 1 TAO so
/// a new seat can attract stake before the next registration can evict it.
pub fn migrate_tune_root_registration<T: Config>() -> Weight {
    let mig_name: Vec<u8> = b"tune_root_registration_v1".to_vec();
    let mut total_weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(&mig_name) {
        log::info!(
            "Migration '{}' already executed - skipping",
            String::from_utf8_lossy(&mig_name)
        );
        return total_weight;
    }
    log::info!("Running migration '{}'", String::from_utf8_lossy(&mig_name));

    const ONE_TAO: u64 = 1_000_000_000;
    let floor = TaoBalance::from(ONE_TAO);

    ImmunityPeriod::<T>::insert(NetUid::ROOT, 7200u16);
    MaxRegistrationsPerBlock::<T>::insert(NetUid::ROOT, 1u16);
    TargetRegistrationsPerInterval::<T>::insert(NetUid::ROOT, 2u16);
    MinBurn::<T>::insert(NetUid::ROOT, floor);
    total_weight = total_weight.saturating_add(T::DbWeight::get().writes(4));
    total_weight = total_weight.saturating_add(T::DbWeight::get().reads(1));
    if Burn::<T>::get(NetUid::ROOT) < floor {
        Burn::<T>::insert(NetUid::ROOT, floor);
        total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));
    }

    HasMigrationRun::<T>::insert(&mig_name, true);
    total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{}' completed",
        String::from_utf8_lossy(&mig_name)
    );
    total_weight
}
