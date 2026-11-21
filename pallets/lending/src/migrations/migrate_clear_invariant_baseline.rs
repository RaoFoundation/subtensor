//! Clear the retired pool-wide [`crate::InvariantBaseline`] map.
//!
//! Per-loan readd burn (Jacob 2026-11-03) replaces the idle-book snapshot. Any
//! leftover entries would be stale and must not linger.
use core::marker::PhantomData;

#[cfg(feature = "try-runtime")]
use alloc::vec::Vec;
#[cfg(feature = "try-runtime")]
use codec::{Decode, Encode};
#[cfg(feature = "try-runtime")]
use frame_support::ensure;
use frame_support::{BoundedVec, traits::OnRuntimeUpgrade, weights::Weight};
#[cfg(feature = "try-runtime")]
use sp_runtime::TryRuntimeError;

use crate::*;

fn migration_key() -> BoundedVec<u8, MigrationKeyMaxLen> {
    BoundedVec::truncate_from(b"migrate_clear_invariant_baseline_v1".to_vec())
}

/// Idempotent: guarded by [`HasMigrationRun`]. Drains every
/// [`InvariantBaseline`] entry.
pub fn migrate_clear_invariant_baseline<T: Config>() -> Weight {
    let migration_name = migration_key();
    let mut weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(&migration_name) {
        return weight;
    }

    let removed = InvariantBaseline::<T>::clear(u32::MAX, None);
    // One read+write per cleared key, plus the stamp write.
    weight = weight.saturating_add(T::DbWeight::get().reads_writes(
        u64::from(removed.unique.saturating_add(1)),
        u64::from(removed.unique.saturating_add(1)),
    ));

    HasMigrationRun::<T>::insert(&migration_name, true);
    weight
}

pub struct Migration<T: Config>(PhantomData<T>);

impl<T: Config> OnRuntimeUpgrade for Migration<T> {
    fn on_runtime_upgrade() -> Weight {
        migrate_clear_invariant_baseline::<T>()
    }

    #[cfg(feature = "try-runtime")]
    fn pre_upgrade() -> Result<Vec<u8>, TryRuntimeError> {
        let already_run = HasMigrationRun::<T>::get(migration_key());
        let pending = InvariantBaseline::<T>::iter().count() as u32;
        Ok((already_run, pending).encode())
    }

    #[cfg(feature = "try-runtime")]
    fn post_upgrade(state: Vec<u8>) -> Result<(), TryRuntimeError> {
        let (already_run, pending_before): (bool, u32) =
            Decode::decode(&mut &state[..]).map_err(|_| "pre_upgrade state must decode")?;
        ensure!(
            HasMigrationRun::<T>::get(migration_key()),
            "clear-invariant-baseline migration must be stamped"
        );
        ensure!(
            InvariantBaseline::<T>::iter().next().is_none(),
            "InvariantBaseline must be empty after migration"
        );
        if already_run {
            ensure!(
                pending_before == 0 || InvariantBaseline::<T>::iter().next().is_none(),
                "already-run clear must leave InvariantBaseline empty"
            );
        }
        Ok(())
    }
}
