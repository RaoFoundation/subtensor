use super::*;
use frame_support::{traits::Get, weights::Weight};
use log;
use scale_info::prelude::string::String;

/// Sets network last-lock so `get_network_lock_cost()` evaluates to 2_500 TAO at the current block
/// (writes last-lock = 1_250 TAO rao and last-lock block = now).
///
/// Idempotency key (frozen): `migrate_network_lock_cost_2500`.
pub fn migrate_network_lock_cost_2500<T: Config>() -> Weight {
    const RAO_PER_TAO: u64 = 1_000_000_000;
    const TARGET_COST_TAO: u64 = 2_500;
    const NEW_LAST_LOCK_RAO: u64 = (TARGET_COST_TAO / 2) * RAO_PER_TAO; // 1,250 TAO

    let migration_name = b"migrate_network_lock_cost_2500".to_vec();
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

    // Use the current block; ensure it's non-zero so mult == 2 in get_network_lock_cost()
    let current_block = Pallet::<T>::get_current_block_as_u64();
    let block_to_set = if current_block == 0 { 1 } else { current_block };

    // Set last_lock so that price = 2 * last_lock = 2,500 TAO at this block
    Pallet::<T>::set_network_last_lock(NEW_LAST_LOCK_RAO.into());
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    // Start decay from "now" (no backdated decay)
    Pallet::<T>::set_network_last_lock_block(block_to_set);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    // Mark migration done
    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        target: "runtime",
        "Migration '{}' completed. lock_cost set to 2,500 TAO at block {}.",
        String::from_utf8_lossy(&migration_name),
        block_to_set
    );

    weight
}
