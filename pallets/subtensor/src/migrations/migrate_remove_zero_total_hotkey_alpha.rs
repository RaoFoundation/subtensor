use super::*;
use frame_support::{traits::Get, weights::Weight};
use log;
use scale_info::prelude::string::String;

/// Prunes zero-valued `TotalHotkeyAlpha` entries that waste storage without affecting balances.
///
/// Idempotency key (frozen): `migrate_remove_zero_total_hotkey_alpha`.
pub fn migrate_remove_zero_total_hotkey_alpha<T: Config>() -> Weight {
    let migration_name = b"migrate_remove_zero_total_hotkey_alpha".to_vec();
    let mut weight = T::DbWeight::get().reads(1);

    // ------------------------------
    // Step 0: Check if already run
    // ------------------------------
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

    // ------------------------------
    // Step 1: Remove any zero entries in TotalHotkeyAlpha
    // ------------------------------

    let mut removed_entries_count = 0u64;

    // For each (hotkey, netuid, alpha) entry, remove if alpha == 0
    for (hotkey, netuid, alpha) in TotalHotkeyAlpha::<T>::iter() {
        if alpha.is_zero() {
            TotalHotkeyAlpha::<T>::remove(&hotkey, netuid);
            removed_entries_count = removed_entries_count.saturating_add(1);
        }
    }

    weight = weight.saturating_add(T::DbWeight::get().reads(removed_entries_count));
    weight = weight.saturating_add(T::DbWeight::get().writes(removed_entries_count));

    log::info!("Removed {removed_entries_count} zero entries from TotalHotkeyAlpha.");

    // ------------------------------
    // Step 2: Mark Migration as Completed
    // ------------------------------
    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{:?}' completed successfully.",
        String::from_utf8_lossy(&migration_name)
    );

    weight
}
