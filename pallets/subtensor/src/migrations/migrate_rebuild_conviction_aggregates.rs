use alloc::{collections::BTreeMap, string::String, vec::Vec};
use frame_support::{traits::Get, weights::Weight};

use crate::{
    Config, DecayingHotkeyLock, DecayingLock, DecayingOwnerLock, HasMigrationRun, HotkeyLock, Lock,
    LockingColdkeys, MaturityRate, OwnerLock, Pallet as Subtensor, SubnetOwnerHotkey, UnlockRate,
    staking::lock::{LockState, roll_lock_state},
};
use subtensor_runtime_common::NetUid;

const MIGRATION_NAME: &[u8] = b"migrate_rebuild_conviction_aggregates";

// Mainnet archive scan at block 8_793_919 on 2026-08-07:
// - 352 Lock rows and 352 matching LockingColdkeys rows
// - 193 aggregate rows across the four aggregate maps
// - 125 subnets with locks, with at most 44 Lock rows on any one subnet
//
// This is intentionally a one-shot runtime-upgrade scan of small existing
// state, not an operation placed on a recurring block path. Keep these
// measurements next to the migration so its practical bound and review
// rationale are not lost.
const OBSERVED_MAINNET_LOCK_ROWS: u64 = 352;
const OBSERVED_MAINNET_AGGREGATE_ROWS: u64 = 193;
const OBSERVED_MAINNET_MAX_LOCKS_PER_SUBNET: u64 = 44;
const OBSERVED_MAINNET_BLOCK: u64 = 8_793_919;

fn merge_into<K: Ord>(aggregates: &mut BTreeMap<K, LockState>, key: K, lock: &LockState) {
    if let Some(aggregate) = aggregates.get_mut(&key) {
        *aggregate = aggregate.add(lock);
    } else {
        aggregates.insert(key, lock.clone());
    }
}

/// Rebuilds conviction aggregates from canonical individual lock rows.
///
/// Runtime v443 could advance an aggregate timestamp after applying only one
/// member's roll delta. Once that happened, the aggregate was no longer the
/// sum of its members at its advertised timestamp. There is no safe way to
/// repair such a bucket incrementally, so this migration ignores every stored
/// aggregate and reconstructs all four maps from `Lock`.
///
/// Each individual is first rolled to the runtime-upgrade block using its
/// current lock mode and owner role. The rolled row is persisted (or removed
/// if it has become dust), then merged into its appropriate new aggregate.
/// This preserves earned conviction while establishing one common timestamp
/// for every individual and aggregate contribution.
pub fn migrate_rebuild_conviction_aggregates<T: Config>() -> Weight {
    let mut weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(MIGRATION_NAME) {
        log::info!(
            "Migration '{}' already executed - skipping",
            String::from_utf8_lossy(MIGRATION_NAME)
        );
        return weight;
    }

    log::info!(
        "Running migration '{}'. Mainnet scan at block {} observed {} individual locks, \
         {} aggregate rows, and at most {} locks on one subnet",
        String::from_utf8_lossy(MIGRATION_NAME),
        OBSERVED_MAINNET_BLOCK,
        OBSERVED_MAINNET_LOCK_ROWS,
        OBSERVED_MAINNET_AGGREGATE_ROWS,
        OBSERVED_MAINNET_MAX_LOCKS_PER_SUBNET,
    );

    let now = Subtensor::<T>::get_current_block_as_u64();
    let unlock_rate = UnlockRate::<T>::get();
    let maturity_rate = MaturityRate::<T>::get();
    weight = weight.saturating_add(T::DbWeight::get().reads(3));

    // Collect before rewriting Lock so mutation cannot disturb the iterator.
    let locks: Vec<_> = Lock::<T>::iter().collect();
    let scanned_count = locks.len() as u64;
    weight = weight.saturating_add(T::DbWeight::get().reads(scanned_count));

    let locking_coldkeys_removal = LockingColdkeys::<T>::clear(u32::MAX, None);
    weight = weight.saturating_add(T::DbWeight::get().reads_writes(
        locking_coldkeys_removal.loops as u64,
        locking_coldkeys_removal.backend as u64,
    ));

    let hotkey_removal = HotkeyLock::<T>::clear(u32::MAX, None);
    weight = weight.saturating_add(
        T::DbWeight::get().reads_writes(hotkey_removal.loops as u64, hotkey_removal.backend as u64),
    );

    let decaying_hotkey_removal = DecayingHotkeyLock::<T>::clear(u32::MAX, None);
    weight = weight.saturating_add(T::DbWeight::get().reads_writes(
        decaying_hotkey_removal.loops as u64,
        decaying_hotkey_removal.backend as u64,
    ));

    let owner_removal = OwnerLock::<T>::clear(u32::MAX, None);
    weight = weight.saturating_add(
        T::DbWeight::get().reads_writes(owner_removal.loops as u64, owner_removal.backend as u64),
    );

    let decaying_owner_removal = DecayingOwnerLock::<T>::clear(u32::MAX, None);
    weight = weight.saturating_add(T::DbWeight::get().reads_writes(
        decaying_owner_removal.loops as u64,
        decaying_owner_removal.backend as u64,
    ));

    let mut perpetual_general = BTreeMap::<(NetUid, T::AccountId), LockState>::new();
    let mut decaying_general = BTreeMap::<(NetUid, T::AccountId), LockState>::new();
    let mut perpetual_owner = BTreeMap::<NetUid, LockState>::new();
    let mut decaying_owner = BTreeMap::<NetUid, LockState>::new();
    let mut retained_count = 0u64;
    let mut removed_dust_count = 0u64;

    for ((coldkey, netuid, hotkey), lock) in locks {
        let owner_lock = SubnetOwnerHotkey::<T>::get(netuid) == hotkey;
        let perpetual_lock = DecayingLock::<T>::get(&coldkey, netuid) == Some(false);
        weight = weight.saturating_add(T::DbWeight::get().reads(2));

        let rolled = roll_lock_state(
            lock,
            now,
            unlock_rate,
            maturity_rate,
            owner_lock,
            perpetual_lock,
        );

        if rolled.is_dust() {
            Lock::<T>::remove((&coldkey, netuid, &hotkey));
            removed_dust_count = removed_dust_count.saturating_add(1);
            weight = weight.saturating_add(T::DbWeight::get().writes(1));
            continue;
        }

        Lock::<T>::insert((&coldkey, netuid, &hotkey), rolled.clone());
        LockingColdkeys::<T>::insert((netuid, &hotkey, &coldkey), ());
        retained_count = retained_count.saturating_add(1);
        weight = weight.saturating_add(T::DbWeight::get().writes(2));

        match (owner_lock, perpetual_lock) {
            (true, true) => merge_into(&mut perpetual_owner, netuid, &rolled),
            (true, false) => merge_into(&mut decaying_owner, netuid, &rolled),
            (false, true) => {
                merge_into(&mut perpetual_general, (netuid, hotkey), &rolled);
            }
            (false, false) => {
                merge_into(&mut decaying_general, (netuid, hotkey), &rolled);
            }
        }
    }

    let aggregate_count = perpetual_general
        .len()
        .saturating_add(decaying_general.len())
        .saturating_add(perpetual_owner.len())
        .saturating_add(decaying_owner.len()) as u64;

    for ((netuid, hotkey), lock) in perpetual_general {
        HotkeyLock::<T>::insert(netuid, hotkey, lock);
    }
    for ((netuid, hotkey), lock) in decaying_general {
        DecayingHotkeyLock::<T>::insert(netuid, hotkey, lock);
    }
    for (netuid, lock) in perpetual_owner {
        OwnerLock::<T>::insert(netuid, lock);
    }
    for (netuid, lock) in decaying_owner {
        DecayingOwnerLock::<T>::insert(netuid, lock);
    }
    weight = weight.saturating_add(T::DbWeight::get().writes(aggregate_count));

    HasMigrationRun::<T>::insert(MIGRATION_NAME, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{}' completed. scanned_entries={}, retained_entries={}, \
         removed_dust_entries={}, rebuilt_aggregate_entries={}",
        String::from_utf8_lossy(MIGRATION_NAME),
        scanned_count,
        retained_count,
        removed_dust_count,
        aggregate_count,
    );

    weight
}
