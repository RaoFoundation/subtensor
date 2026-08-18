use alloc::string::String;
use frame_support::{BoundedVec, traits::Get, weights::Weight};

use crate::*;

fn migration_key() -> BoundedVec<u8, MigrationKeyMaxLen> {
    BoundedVec::truncate_from(b"migrate_register_pallet_hotkey".to_vec())
}

/// One-shot migration that disables the limit-orders pallet on first upgrade and
/// registers the pallet intermediary hotkey if it has not been registered yet.
///
/// Guarded by `HasMigrationRun` so it is safe to include in every runtime upgrade:
/// subsequent calls return immediately after a single storage read.
pub fn migrate_register_pallet_hotkey<T: Config>() -> Weight {
    let migration_name = migration_key();
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

    // Register the pallet intermediary hotkey if it has not been registered yet.
    let pallet_acct = Pallet::<T>::pallet_account();
    let pallet_hotkey = T::PalletHotkey::get();
    weight = weight.saturating_add(T::DbWeight::get().reads(1));

    if !T::SwapInterface::pallet_hotkey_registered(&pallet_acct, &pallet_hotkey) {
        let _ = T::SwapInterface::register_pallet_hotkey(&pallet_acct, &pallet_hotkey);
        // register_pallet_hotkey writes Owner, OwnedHotkeys, StakingHotkeys
        weight = weight.saturating_add(T::DbWeight::get().writes(3));
    }

    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{:?}' completed successfully.",
        String::from_utf8_lossy(&migration_name)
    );

    weight
}
