use super::migrate_init_total_issuance::migrate_init_total_issuance;
use super::*;
use alloc::string::String;

/// Sets root `SubnetTAO` to the sum of all hotkeys' `TotalHotkeyAlpha` on `NetUid::ROOT`, then
/// re-runs total-issuance init so counters stay consistent.
///
/// Idempotency key (frozen): `migrate_fix_root_subnet_tao`.
pub fn migrate_fix_root_subnet_tao<T: Config>() -> Weight {
    let migration_name = b"migrate_fix_root_subnet_tao".to_vec();
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

    let mut total_stake = TaoBalance::ZERO;
    let mut hotkey_count: u64 = 0;
    // We accumulate the total stake for all hotkeys on the root subnet.
    for hotkey in Owner::<T>::iter_keys() {
        let hotkey_stake = TotalHotkeyAlpha::<T>::get(&hotkey, NetUid::ROOT);
        total_stake = total_stake.saturating_add(hotkey_stake.to_u64().into());
        hotkey_count = hotkey_count.saturating_add(1);
    }

    log::info!("Total stake: {total_stake}, hotkey count: {hotkey_count}");

    weight = weight.saturating_add(T::DbWeight::get().reads(hotkey_count).saturating_mul(2));

    // We set the root subnet TAO to the total stake.
    SubnetTAO::<T>::insert(NetUid::ROOT, total_stake);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

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
