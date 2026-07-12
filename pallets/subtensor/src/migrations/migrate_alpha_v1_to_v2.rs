use super::*;
use frame_support::{traits::Get, weights::Weight};
use scale_info::prelude::string::String;
use share_pool::SafeFloat;
use substrate_fixed::types::U64F64;

/// Finalize the lazy `Alpha` -> `AlphaV2` and `TotalHotkeyShares` ->
/// `TotalHotkeySharesV2` migration.
///
/// # Background (issue #2636)
///
/// The alpha share-pool was moved from the legacy `Alpha` / `TotalHotkeyShares`
/// maps (`U64F64`) to `AlphaV2` / `TotalHotkeySharesV2` (`SafeFloat`). To avoid a
/// one-shot rewrite of every staker at the v2 cutover, the move is performed
/// lazily by `HotkeyAlphaSharePoolDataOperations::set_share` /
/// `set_denominator`, which delete the legacy entry and write the v2 entry on
/// every write. Reads still consult the legacy map first and fall back to v2.
///
/// Lazy migration only ever touches keys that are *written*. Keys that are only
/// ever read remain in the legacy maps indefinitely, so the legacy maps can never
/// be retired without an explicit finalization pass. This migration *is* that
/// pass: it copies every remaining legacy entry into the corresponding v2 map and
/// removes the legacy entry. Once it has run, the legacy maps are empty and the
/// v1-first read fallback in `HotkeyAlphaSharePoolDataOperations` becomes a dead
/// branch that a follow-up can delete, closing the "Deprecate v1 alpha share pool
/// maps after they have lazy-migrated" sub-task of #2636.
///
/// # Safety
///
/// The legacy and v2 maps are mutually exclusive per key: the lazy writer always
/// deletes the legacy entry before writing v2, and the legacy maps have no
/// remaining write paths (`insert` / `mutate` / `put`) outside this migration, so
/// a legacy entry is guaranteed to have no v2 counterpart and copying legacy ->
/// v2 can never overwrite a newer v2 value. As defense in depth we still skip the
/// copy when a v2 value is already present, preserving the "v2 wins on duplicate"
/// semantics of `Pallet::alpha_iter`.
///
/// The persisted v2 value is produced with the exact same `SafeFloat::from(U64F64)`
/// conversion the read path (`get_share` / `get_denominator`) already applies on
/// every read, so this migration changes no observable value - it only relocates
/// it from the legacy key to the v2 key.
pub fn migrate_alpha_v1_to_v2<T: Config>() -> Weight {
    let migration_name = b"migrate_alpha_v1_to_v2".to_vec();
    let mut weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(&migration_name) {
        log::info!(
            target: "runtime",
            "Migration '{}' already run - skipping.",
            String::from_utf8_lossy(&migration_name)
        );
        return weight;
    }

    log::info!(target: "runtime", "Running migration 'migrate_alpha_v1_to_v2'");

    let mut storage_reads: u64 = 0;
    let mut storage_writes: u64 = 0;

    // Collect entries up front: we mutate the very map we iterate over, so a live
    // iterator cursor would be invalidated by the removals below.
    let alpha_entries: Vec<((T::AccountId, T::AccountId, NetUid), U64F64)> =
        Alpha::<T>::iter().collect();
    for ((hotkey, coldkey, netuid), legacy_value) in alpha_entries {
        storage_reads = storage_reads.saturating_add(1);

        // Same conversion the read path uses; SafeFloat exposes is_zero (U64F64 does not).
        let migrated = SafeFloat::from(legacy_value);
        if migrated.is_zero() {
            // Defensive: the lazy writer removes zero entries, but clear any that
            // somehow remain instead of carrying them into v2.
            Alpha::<T>::remove((&hotkey, &coldkey, netuid));
            storage_writes = storage_writes.saturating_add(1);
            continue;
        }

        // Defense in depth: never overwrite an existing v2 entry. Legacy and v2
        // are mutually exclusive per key, so this branch is not expected to fire,
        // but if it ever did the v2 value (the newer one) must win.
        if AlphaV2::<T>::try_get((&hotkey, &coldkey, netuid)).is_err() {
            storage_reads = storage_reads.saturating_add(1);
            AlphaV2::<T>::insert((&hotkey, &coldkey, netuid), migrated);
            storage_writes = storage_writes.saturating_add(1);
        }

        Alpha::<T>::remove((&hotkey, &coldkey, netuid));
        storage_writes = storage_writes.saturating_add(1);
    }

    let shares_entries: Vec<(T::AccountId, NetUid, U64F64)> =
        TotalHotkeyShares::<T>::iter().collect();
    for (hotkey, netuid, legacy_value) in shares_entries {
        storage_reads = storage_reads.saturating_add(1);

        let migrated = SafeFloat::from(legacy_value);
        if migrated.is_zero() {
            TotalHotkeyShares::<T>::remove(&hotkey, netuid);
            storage_writes = storage_writes.saturating_add(1);
            continue;
        }

        if TotalHotkeySharesV2::<T>::try_get(&hotkey, netuid).is_err() {
            storage_reads = storage_reads.saturating_add(1);
            TotalHotkeySharesV2::<T>::insert(&hotkey, netuid, migrated);
            storage_writes = storage_writes.saturating_add(1);
        }

        TotalHotkeyShares::<T>::remove(&hotkey, netuid);
        storage_writes = storage_writes.saturating_add(1);
    }

    weight = weight.saturating_add(T::DbWeight::get().reads_writes(storage_reads, storage_writes));

    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(target: "runtime", "Migration 'migrate_alpha_v1_to_v2' completed.");

    weight
}
