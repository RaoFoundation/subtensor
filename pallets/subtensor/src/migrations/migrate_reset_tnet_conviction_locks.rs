use super::*;
use frame_support::weights::Weight;
use scale_info::prelude::string::String;

/// Clears conviction v2 lock state that only exists on testnet before this
/// conviction design is deployed more broadly.
///
/// `devnet-ready` had `Lock`, `HotkeyLock`, `DecayingHotkeyLock`, `OwnerLock`,
/// and `DecayingLock`, but did not have `DecayingOwnerLock`. `OwnerLock` also
/// used the old owner-coldkey aggregate semantics. Clear these prefixes without
/// decoding values so old or incompatible aggregate bytes are removed safely.
pub fn migrate_reset_tnet_conviction_locks<T: Config>() -> Weight {
    let migration_name = b"migrate_reset_tnet_conviction_locks".to_vec();
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

    // This only affects testnet: mainnet has not had this conviction lock state
    // deployed with live values yet.
    let lock_removal = Lock::<T>::clear(u32::MAX, None);
    weight = weight.saturating_add(
        T::DbWeight::get().reads_writes(lock_removal.loops as u64, lock_removal.backend as u64),
    );

    let hotkey_lock_removal = HotkeyLock::<T>::clear(u32::MAX, None);
    weight = weight.saturating_add(T::DbWeight::get().reads_writes(
        hotkey_lock_removal.loops as u64,
        hotkey_lock_removal.backend as u64,
    ));

    let decaying_hotkey_lock_removal = DecayingHotkeyLock::<T>::clear(u32::MAX, None);
    weight = weight.saturating_add(T::DbWeight::get().reads_writes(
        decaying_hotkey_lock_removal.loops as u64,
        decaying_hotkey_lock_removal.backend as u64,
    ));

    let owner_lock_removal = OwnerLock::<T>::clear(u32::MAX, None);
    weight = weight.saturating_add(T::DbWeight::get().reads_writes(
        owner_lock_removal.loops as u64,
        owner_lock_removal.backend as u64,
    ));

    let decaying_owner_lock_removal = DecayingOwnerLock::<T>::clear(u32::MAX, None);
    weight = weight.saturating_add(T::DbWeight::get().reads_writes(
        decaying_owner_lock_removal.loops as u64,
        decaying_owner_lock_removal.backend as u64,
    ));

    let decaying_lock_removal = DecayingLock::<T>::clear(u32::MAX, None);
    weight = weight.saturating_add(T::DbWeight::get().reads_writes(
        decaying_lock_removal.loops as u64,
        decaying_lock_removal.backend as u64,
    ));

    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{:?}' completed successfully. Removed Lock: {:?}, HotkeyLock: {:?}, DecayingHotkeyLock: {:?}, OwnerLock: {:?}, DecayingOwnerLock: {:?}, DecayingLock: {:?}.",
        String::from_utf8_lossy(&migration_name),
        lock_removal.backend,
        hotkey_lock_removal.backend,
        decaying_hotkey_lock_removal.backend,
        owner_lock_removal.backend,
        decaying_owner_lock_removal.backend,
        decaying_lock_removal.backend,
    );

    weight
}
