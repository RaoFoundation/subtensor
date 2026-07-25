use crate::{Config, Event, HasMigrationRun, NetworkImmunityPeriod, Pallet, Weight};
use scale_info::prelude::string::String;

/// Sets the global `NetworkImmunityPeriod` hyperparameter to the cutover value used at migration time.
///
/// Idempotency key (frozen): `migrate_network_immunity_period`.
pub fn migrate_network_immunity_period<T: Config>() -> Weight {
    use frame_support::traits::Get;

    const NEW_VALUE: u64 = 864_000;

    let migration_name = b"migrate_network_immunity_period".to_vec();
    let mut weight = T::DbWeight::get().reads(1);

    // Skip if already executed
    if HasMigrationRun::<T>::get(&migration_name) {
        log::info!(
            target: "runtime",
            "Migration '{}' already run - skipping.",
            String::from_utf8_lossy(&migration_name)
        );
        return weight;
    }

    // ── 1) Set new value ─────────────────────────────────────────────────────
    NetworkImmunityPeriod::<T>::put(NEW_VALUE);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    Pallet::<T>::deposit_event(Event::NetworkImmunityPeriodSet(NEW_VALUE));

    // ── 2) Mark migration done ───────────────────────────────────────────────
    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        target: "runtime",
        "Migration '{}' completed - NetworkImmunityPeriod => {}.",
        String::from_utf8_lossy(&migration_name),
        NEW_VALUE
    );

    weight
}
