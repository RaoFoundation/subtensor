use super::*;
use codec::{Decode, DecodeWithMemTracking, Encode};
use frame_support::{storage_alias, traits::Get, weights::Weight};
use scale_info::TypeInfo;
use scale_info::prelude::string::String;
use sp_std::collections::btree_set::BTreeSet;
use sp_std::vec::Vec;

pub const MIGRATION_NAME: &[u8] = b"migrate_cleanup_staking_hotkeys";

/// Persistent progress for the bounded `StakingHotkeys` cleanup.
///
/// The current row's hotkeys are stored as independent candidates so a pass can remove a
/// bounded subset from the latest on-chain vector. This avoids overwriting hotkeys added by
/// normal staking operations while the migration is running.
#[derive(Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo)]
pub struct StakingHotkeysCleanupProgress {
    /// Hashed key of the last fully processed `StakingHotkeys` row.
    pub after: Option<Vec<u8>>,
    /// SCALE-encoded coldkey for a row that is only partially processed.
    pub coldkey: Option<Vec<u8>>,
    /// SCALE-encoded hotkeys from that row which still need classification.
    pub remaining_hotkeys: Vec<Vec<u8>>,
    pub rows_scanned: u64,
    pub relationships_scanned: u64,
    pub relationships_removed: u64,
    pub vector_writes: u64,
}

#[storage_alias]
pub type StakingHotkeysCleanupMigration<T: Config> =
    StorageValue<Pallet<T>, StakingHotkeysCleanupProgress, OptionQuery>;

fn candidate_weight<T: Config>() -> Weight {
    // One prefix probe in each share map plus one BasketClaimed read. The predicate may
    // short-circuit, but charging all three reads keeps each candidate conservatively bounded.
    T::DbWeight::get().reads(3)
}

fn vector_rewrite_weight<T: Config>() -> Weight {
    T::DbWeight::get().reads_writes(1, 1)
}

fn row_load_weight<T: Config>() -> Weight {
    // Iterating the next StakingHotkeys row performs one backend read.
    T::DbWeight::get().reads(1)
}

/// A conservative keep predicate for one hotkey/coldkey relationship.
///
/// Any stored share row is retained, including a zero-valued legacy row. The storage-bloat
/// migration runs first and clears zero legacy rows, while treating an unexpected remaining row
/// as live makes this cleanup fail safe. A basket watermark is also sufficient to retain the
/// relationship because zero-root-stake claimants still need to be discoverable by claims and
/// coldkey swaps.
fn relationship_must_remain<T: Config>(hotkey: &T::AccountId, coldkey: &T::AccountId) -> bool {
    AlphaV2::<T>::iter_prefix((hotkey, coldkey))
        .next()
        .is_some()
        || Alpha::<T>::iter_prefix((hotkey, coldkey)).next().is_some()
        || BasketClaimed::<T>::get(hotkey, coldkey) != 0
}

/// Starts the cleanup without scanning `StakingHotkeys` in the runtime-upgrade block.
pub fn kickoff_staking_hotkeys_cleanup<T: Config>() -> Weight {
    let mut weight = T::DbWeight::get().reads(2);
    if HasMigrationRun::<T>::get(MIGRATION_NAME) || StakingHotkeysCleanupMigration::<T>::exists() {
        return weight;
    }

    StakingHotkeysCleanupMigration::<T>::put(StakingHotkeysCleanupProgress {
        after: None,
        coldkey: None,
        remaining_hotkeys: Vec::new(),
        rows_scanned: 0,
        relationships_scanned: 0,
        relationships_removed: 0,
        vector_writes: 0,
    });
    weight.saturating_accrue(T::DbWeight::get().writes(1));
    log::info!(
        "Migration '{}' scheduled for bounded on_idle execution",
        String::from_utf8_lossy(MIGRATION_NAME)
    );
    weight
}

/// Continues the cleanup without exceeding `limit`.
///
/// Normal operations remain enabled. Each batch removes candidates from the latest stored
/// vector, so stake or hotkeys added between passes are never overwritten by an old snapshot.
pub fn continue_staking_hotkeys_cleanup<T: Config>(limit: Weight) -> Weight {
    // Cursor read plus either cursor persistence, or completion marker + cursor removal.
    let pass_overhead = T::DbWeight::get().reads_writes(1, 2);
    if !pass_overhead.all_lte(limit) {
        return Weight::zero();
    }

    let Some(mut progress) = StakingHotkeysCleanupMigration::<T>::get() else {
        return T::DbWeight::get().reads(1);
    };
    let work_limit = limit.saturating_sub(pass_overhead);
    let mut work_weight = Weight::zero();
    let mut finished = false;

    loop {
        if progress.coldkey.is_none() {
            // Reserve a possible write as well: an encoded empty vector is removed immediately
            // after it is read, without carrying an ambiguous empty-row cursor across blocks.
            let row_load_reserve =
                row_load_weight::<T>().saturating_add(T::DbWeight::get().writes(1));
            if !work_weight
                .saturating_add(row_load_reserve)
                .all_lte(work_limit)
            {
                break;
            }

            // Drop the iterator before this row can be mutated below. Altering a map while one
            // of its iterators is alive has undefined iteration results.
            let next = {
                let mut iter = match progress.after.as_ref() {
                    Some(raw) => StakingHotkeys::<T>::iter_from(raw.clone()),
                    None => StakingHotkeys::<T>::iter(),
                };
                iter.next()
            };
            work_weight.saturating_accrue(row_load_weight::<T>());

            let Some((coldkey, hotkeys)) = next else {
                finished = true;
                break;
            };

            if hotkeys.is_empty() {
                let empty_row_write = T::DbWeight::get().writes(1);
                StakingHotkeys::<T>::remove(&coldkey);
                work_weight.saturating_accrue(empty_row_write);
                progress.after = Some(StakingHotkeys::<T>::hashed_key_for(&coldkey));
                progress.rows_scanned = progress.rows_scanned.saturating_add(1);
                progress.vector_writes = progress.vector_writes.saturating_add(1);
                continue;
            }

            progress.coldkey = Some(coldkey.encode());
            progress.remaining_hotkeys =
                hotkeys.into_iter().map(|hotkey| hotkey.encode()).collect();
        }

        let Some(encoded_coldkey) = progress.coldkey.as_ref() else {
            continue;
        };
        let Ok(coldkey) = T::AccountId::decode(&mut &encoded_coldkey[..]) else {
            log::error!(
                "Migration '{}' found an undecodable coldkey cursor; stopping this pass",
                String::from_utf8_lossy(MIGRATION_NAME)
            );
            break;
        };

        // Reserve enough room to rewrite the current vector if any processed candidate is stale.
        let rewrite_weight = vector_rewrite_weight::<T>();
        let mut removals: BTreeSet<T::AccountId> = BTreeSet::new();
        let mut processed_any = false;

        while let Some(encoded_hotkey) = progress.remaining_hotkeys.last() {
            if !work_weight
                .saturating_add(candidate_weight::<T>())
                .saturating_add(rewrite_weight)
                .all_lte(work_limit)
            {
                break;
            }

            let encoded_hotkey = encoded_hotkey.clone();
            progress.remaining_hotkeys.pop();
            let Ok(hotkey) = T::AccountId::decode(&mut &encoded_hotkey[..]) else {
                // The candidate came from a successfully decoded StakingHotkeys vector. If the
                // persisted cursor is ever corrupted, preserving this relationship is safer than
                // deleting an account we cannot identify.
                log::error!(
                    "Migration '{}' found an undecodable hotkey candidate; preserving it",
                    String::from_utf8_lossy(MIGRATION_NAME)
                );
                processed_any = true;
                progress.relationships_scanned = progress.relationships_scanned.saturating_add(1);
                work_weight.saturating_accrue(candidate_weight::<T>());
                continue;
            };

            if !relationship_must_remain::<T>(&hotkey, &coldkey) {
                removals.insert(hotkey);
            }
            processed_any = true;
            progress.relationships_scanned = progress.relationships_scanned.saturating_add(1);
            work_weight.saturating_accrue(candidate_weight::<T>());
        }

        if !removals.is_empty() {
            let mut removed = 0_u64;
            StakingHotkeys::<T>::mutate_exists(&coldkey, |maybe_hotkeys| {
                if let Some(hotkeys) = maybe_hotkeys {
                    let before = hotkeys.len();
                    hotkeys.retain(|hotkey| !removals.contains(hotkey));
                    removed =
                        u64::try_from(before.saturating_sub(hotkeys.len())).unwrap_or(u64::MAX);
                    if hotkeys.is_empty() {
                        *maybe_hotkeys = None;
                    }
                }
            });
            work_weight.saturating_accrue(rewrite_weight);
            progress.relationships_removed = progress.relationships_removed.saturating_add(removed);
            progress.vector_writes = progress.vector_writes.saturating_add(1);
        }

        if progress.remaining_hotkeys.is_empty() {
            progress.after = Some(StakingHotkeys::<T>::hashed_key_for(&coldkey));
            progress.coldkey = None;
            progress.rows_scanned = progress.rows_scanned.saturating_add(1);
            continue;
        }

        if !processed_any {
            break;
        }

        // The current row was only partially classified. Persist its remaining candidates and
        // resume from the latest vector on the next idle pass.
        break;
    }

    if finished {
        HasMigrationRun::<T>::insert(MIGRATION_NAME, true);
        StakingHotkeysCleanupMigration::<T>::kill();
        log::info!(
            "Migration '{}' completed: scanned {} rows / {} relationships, removed {} relationships with {} vector writes",
            String::from_utf8_lossy(MIGRATION_NAME),
            progress.rows_scanned,
            progress.relationships_scanned,
            progress.relationships_removed,
            progress.vector_writes,
        );
    } else {
        StakingHotkeysCleanupMigration::<T>::put(progress);
    }

    pass_overhead.saturating_add(work_weight)
}
