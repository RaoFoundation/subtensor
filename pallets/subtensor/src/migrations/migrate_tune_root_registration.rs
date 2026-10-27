use super::*;
use frame_support::{traits::Get, weights::Weight};
use log;
use scale_info::prelude::string::String;
use subtensor_runtime_common::{NetUid, TaoBalance};

/// `HasMigrationRun` key. Genesis `InitialImmunityPeriod` stays 4096;
/// this migration writes the root-only values below after upgrade.
pub const MIGRATION_NAME: &[u8] = b"tune_root_registration_v1";
/// ~1 day at 12s blocks.
pub const ROOT_IMMUNITY_PERIOD: u16 = 7200;
pub const ROOT_MAX_REGISTRATIONS_PER_BLOCK: u16 = 1;
/// Hard cap is still `target * 3` = 6 per tempo.
pub const ROOT_TARGET_REGISTRATIONS_PER_INTERVAL: u16 = 2;
pub const ROOT_MIN_BURN_RAO: u64 = 1_000_000_000;

/// Align root admission with a normal subnet's rate limits, plus a 1 TAO burn
/// floor: 1 registration per block, 2 per interval (hard cap 6 per tempo),
/// 7200-block immunity (~1 day), and `MinBurn` / current `Burn` at 1 TAO so
/// a new seat can attract stake before the next registration can evict it.
pub fn migrate_tune_root_registration<T: Config>() -> Weight {
    let mig_name: Vec<u8> = MIGRATION_NAME.to_vec();
    let mut total_weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(&mig_name) {
        log::info!(
            "Migration '{}' already executed - skipping",
            String::from_utf8_lossy(&mig_name)
        );
        return total_weight;
    }
    log::info!("Running migration '{}'", String::from_utf8_lossy(&mig_name));

    let floor = TaoBalance::from(ROOT_MIN_BURN_RAO);

    ImmunityPeriod::<T>::insert(NetUid::ROOT, ROOT_IMMUNITY_PERIOD);
    MaxRegistrationsPerBlock::<T>::insert(NetUid::ROOT, ROOT_MAX_REGISTRATIONS_PER_BLOCK);
    TargetRegistrationsPerInterval::<T>::insert(
        NetUid::ROOT,
        ROOT_TARGET_REGISTRATIONS_PER_INTERVAL,
    );
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
