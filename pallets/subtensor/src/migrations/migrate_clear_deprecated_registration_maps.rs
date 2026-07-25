use super::*;
use frame_support::{traits::Get, weights::Weight};
use scale_info::prelude::string::String;

/// Clears deprecated registration tracking maps (`NetworkPowRegistrationAllowed`,
/// `POWRegistrationsThisInterval`, `BurnRegistrationsThisInterval`) without touching the new-model storage.
///
/// Idempotency key (frozen): `migrate_clear_deprecated_registration_maps_v1`.
pub fn migrate_clear_deprecated_registration_maps<T: Config>() -> Weight {
    let migration_name = b"migrate_clear_deprecated_registration_maps_v1".to_vec();
    let mut weight: Weight = T::DbWeight::get().reads(1);

    // --- 0) Skip if already executed
    if HasMigrationRun::<T>::get(&migration_name) {
        log::info!(
            target: "runtime",
            "Migration '{}' already run - skipping.",
            String::from_utf8_lossy(&migration_name)
        );
        return weight;
    }

    // --- 1) Clear deprecated.
    macro_rules! clear_map_and_log {
        ($map:ident, $label:expr) => {{
            let res = $map::<T>::clear(u32::MAX, None);
            weight = weight.saturating_add(T::DbWeight::get().writes(1));
            if res.maybe_cursor.is_some() {
                log::warn!(
                    target: "runtime",
                    "Migration '{}' - '{}' not fully cleared (cursor present).",
                    String::from_utf8_lossy(&migration_name),
                    $label
                );
            } else {
                log::info!(
                    target: "runtime",
                    "Migration '{}' - cleared '{}'.",
                    String::from_utf8_lossy(&migration_name),
                    $label
                );
            }
        }};
    }
    clear_map_and_log!(
        NetworkPowRegistrationAllowed,
        "NetworkPowRegistrationAllowed"
    );

    // Old per-interval tracking (deprecated)
    clear_map_and_log!(POWRegistrationsThisInterval, "POWRegistrationsThisInterval");
    clear_map_and_log!(
        BurnRegistrationsThisInterval,
        "BurnRegistrationsThisInterval"
    );

    // --- 2) Mark migration done
    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        target: "runtime",
        "Migration '{}' completed. Cleared deprecated registration maps only; new-model storage left untouched.",
        String::from_utf8_lossy(&migration_name)
    );

    weight
}
