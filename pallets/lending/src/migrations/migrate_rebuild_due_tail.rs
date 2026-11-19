//! Rebuild [`DueTail`] from the Due schedule head when missing.
//!
//! Moved out of `on_runtime_upgrade` so the walk has an explicit
//! `HasMigrationRun` stamp and O(buckets) weight (audit M2). Bound the walk
//! so a corrupted schedule cannot burn unbounded upgrade weight.
use core::marker::PhantomData;

use frame_support::{BoundedVec, traits::OnRuntimeUpgrade, weights::Weight};

use crate::*;

/// Hard cap on DueNext hops during the one-shot rebuild.
const MAX_DUE_TAIL_WALK: u32 = 65_536;

fn migration_key() -> BoundedVec<u8, MigrationKeyMaxLen> {
    BoundedVec::truncate_from(b"migrate_rebuild_due_tail_v1".to_vec())
}

/// Idempotent: guarded by [`HasMigrationRun`].
pub fn migrate_rebuild_due_tail<T: Config>() -> Weight {
    let migration_name = migration_key();
    let mut weight = T::DbWeight::get().reads(1);
    if HasMigrationRun::<T>::get(&migration_name) {
        return weight;
    }

    weight = weight.saturating_add(T::DbWeight::get().reads(1));
    if DueTail::<T>::get().is_none() {
        if let Some(mut cur) = NextDue::<T>::get() {
            weight = weight.saturating_add(T::DbWeight::get().reads(1));
            let mut hops = 0u32;
            while hops < MAX_DUE_TAIL_WALK {
                weight = weight.saturating_add(T::DbWeight::get().reads(1));
                match DueNext::<T>::get(cur) {
                    Some(n) => {
                        cur = n;
                        hops = hops.saturating_add(1);
                    }
                    None => break,
                }
            }
            DueTail::<T>::put(cur);
            weight = weight.saturating_add(T::DbWeight::get().writes(1));
        }
    }

    HasMigrationRun::<T>::insert(&migration_name, true);
    weight.saturating_add(T::DbWeight::get().writes(1))
}

pub struct Migration<T: Config>(PhantomData<T>);

impl<T: Config> OnRuntimeUpgrade for Migration<T> {
    fn on_runtime_upgrade() -> Weight {
        migrate_rebuild_due_tail::<T>()
    }
}
