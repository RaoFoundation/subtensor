use super::migrate_init_total_issuance::migrate_init_total_issuance;
use super::*;
use alloc::string::String;

/// Applies hardcoded root-pool corrections for an over-unstake incident: adjusts root
/// `SubnetTAO`, `SubnetAlphaIn`, `SubnetAlphaOut`, `SubnetVolume`, and `TotalStake` by fixed rao deltas.
///
/// Idempotency key (frozen): `migrate_fix_root_tao_and_alpha_in`.
pub fn migrate_fix_root_tao_and_alpha_in<T: Config>() -> Weight {
    let migration_name = b"migrate_fix_root_tao_and_alpha_in".to_vec();
    let mut weight = T::DbWeight::get().reads(1);

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

    // Update counters (unstaked more than stake)
    let total_staked = 2_109_761_275_100_688_u64;
    let total_unstaked = 2_179_659_173_851_658_u64;
    let reserve_diff = total_unstaked.saturating_sub(total_staked);
    let volume_diff = (total_unstaked as u128).saturating_add(total_staked as u128);
    SubnetTAO::<T>::mutate(NetUid::ROOT, |amount| {
        *amount = amount.saturating_sub(reserve_diff.into());
    });
    SubnetAlphaIn::<T>::mutate(NetUid::ROOT, |amount| {
        *amount = amount.saturating_add(AlphaBalance::from(reserve_diff));
    });
    SubnetAlphaOut::<T>::mutate(NetUid::ROOT, |amount| {
        *amount = amount.saturating_sub(AlphaBalance::from(reserve_diff));
    });
    SubnetVolume::<T>::mutate(NetUid::ROOT, |amount| {
        *amount = amount.saturating_add(volume_diff);
    });
    TotalStake::<T>::mutate(|amount| {
        *amount = amount.saturating_sub(reserve_diff.into());
    });

    weight = weight.saturating_add(T::DbWeight::get().writes(5));

    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        target: "runtime",
        "Migration '{}' completed successfully.",
        String::from_utf8_lossy(&migration_name)
    );

    // We need to run the total issuance migration to update the total issuance
    // when the root subnet TAO has been updated.
    migrate_init_total_issuance::<T>().saturating_add(weight)
}
