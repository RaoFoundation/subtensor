//! Rebuild [`DueTail`] from the Due schedule head when missing.
//!
//! Moved out of `on_runtime_upgrade` so the walk has an explicit
//! `HasMigrationRun` stamp and O(buckets) weight (audit M2). Bound the walk
//! so a corrupted schedule cannot burn unbounded upgrade weight. When the hop
//! cap is hit mid-list: charge weight for hops done, persist
//! [`DueTailRebuildCursor`], and **do not** stamp success — `on_idle`
//! resumes until the walk reaches a real end.
use core::marker::PhantomData;

use frame_support::{BoundedVec, traits::OnRuntimeUpgrade, weights::Weight};

use crate::*;

/// Hard cap on DueNext hops per pass during the rebuild.
pub const MAX_DUE_TAIL_WALK: u32 = 65_536;

fn migration_key() -> BoundedVec<u8, MigrationKeyMaxLen> {
    // v2: do not stamp incomplete walks (audit M2 follow-up).
    BoundedVec::truncate_from(b"migrate_rebuild_due_tail_v2".to_vec())
}

/// Public key for hooks / tests.
pub fn rebuild_due_tail_migration_key() -> BoundedVec<u8, MigrationKeyMaxLen> {
    migration_key()
}

/// Idempotent: guarded by [`HasMigrationRun`]. Resumable via
/// [`DueTailRebuildCursor`] when the hop budget is exhausted mid-list.
pub fn migrate_rebuild_due_tail<T: Config>() -> Weight {
    migrate_rebuild_due_tail_limited::<T>(MAX_DUE_TAIL_WALK)
}

/// Same as [`migrate_rebuild_due_tail`] with an explicit hop budget (tests).
pub fn migrate_rebuild_due_tail_limited<T: Config>(max_hops: u32) -> Weight {
    let migration_name = migration_key();
    let mut weight = T::DbWeight::get().reads(1);
    if HasMigrationRun::<T>::get(&migration_name) {
        return weight;
    }

    weight = weight.saturating_add(T::DbWeight::get().reads(2));
    let cursor = DueTailRebuildCursor::<T>::get();
    // Already has a tail and no in-progress walk → nothing to rebuild.
    if cursor.is_none() && DueTail::<T>::get().is_some() {
        HasMigrationRun::<T>::insert(&migration_name, true);
        return weight.saturating_add(T::DbWeight::get().writes(1));
    }

    let Some(mut cur) = cursor.or_else(NextDue::<T>::get) else {
        // Empty schedule.
        DueTailRebuildCursor::<T>::kill();
        HasMigrationRun::<T>::insert(&migration_name, true);
        return weight.saturating_add(T::DbWeight::get().writes(2));
    };
    weight = weight.saturating_add(T::DbWeight::get().reads(1));

    let mut hops = 0u32;
    let mut finished = false;
    while hops < max_hops {
        weight = weight.saturating_add(T::DbWeight::get().reads(1));
        match DueNext::<T>::get(cur) {
            Some(n) => {
                cur = n;
                hops = hops.saturating_add(1);
            }
            None => {
                finished = true;
                break;
            }
        }
    }

    if finished {
        DueTail::<T>::put(cur);
        DueTailRebuildCursor::<T>::kill();
        HasMigrationRun::<T>::insert(&migration_name, true);
        weight.saturating_add(T::DbWeight::get().writes(3))
    } else {
        // Cap hit with remaining links — leave cursor, do not stamp, do not
        // publish an intermediate DueTail (would orphan the suffix).
        DueTailRebuildCursor::<T>::put(cur);
        weight.saturating_add(T::DbWeight::get().writes(1))
    }
}

/// True while the rebuild is unfinished (cursor present or stamp missing with
/// a live head and no tail). Used by `on_idle` to resume.
pub fn rebuild_due_tail_pending<T: Config>() -> bool {
    if HasMigrationRun::<T>::get(migration_key()) {
        return false;
    }
    DueTailRebuildCursor::<T>::get().is_some()
        || (DueTail::<T>::get().is_none() && NextDue::<T>::get().is_some())
}

pub struct Migration<T: Config>(PhantomData<T>);

impl<T: Config> OnRuntimeUpgrade for Migration<T> {
    fn on_runtime_upgrade() -> Weight {
        migrate_rebuild_due_tail::<T>()
    }
}
