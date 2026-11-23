//! Append [`LiftParams::short_book_max_pool_alpha_bps`] and revert a live
//! `split_short_bps == 6667` (prior 2× Cap_short experiment) back to 5000.
//!
//! Pre-append layout decodes as
//! [`LiftParamsBeforeShortBookAlpha`](crate::LiftParamsBeforeShortBookAlpha).
use core::marker::PhantomData;

#[cfg(feature = "try-runtime")]
use alloc::vec::Vec;
use codec::DecodeAll;
#[cfg(feature = "try-runtime")]
use codec::{Decode, Encode};
#[cfg(feature = "try-runtime")]
use frame_support::ensure;
use frame_support::{BoundedVec, storage::unhashed, traits::OnRuntimeUpgrade, weights::Weight};
#[cfg(feature = "try-runtime")]
use sp_runtime::TryRuntimeError;

use crate::*;

const PRIOR_2X_SPLIT_SHORT_BPS: u16 = 6_667;

fn migration_key() -> BoundedVec<u8, MigrationKeyMaxLen> {
    BoundedVec::truncate_from(b"migrate_lift_params_short_book_alpha_v1".to_vec())
}

/// Idempotent: guarded by [`HasMigrationRun`].
pub fn migrate_lift_params_short_book_alpha<T: Config>() -> Weight {
    let migration_name = migration_key();
    let mut weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(&migration_name) {
        return weight;
    }

    weight = weight.saturating_add(T::DbWeight::get().reads(1));
    let key = LiftParamsStore::<T>::hashed_key();
    let Some(bytes) = unhashed::get_raw(&key) else {
        HasMigrationRun::<T>::insert(&migration_name, true);
        return weight.saturating_add(T::DbWeight::get().writes(1));
    };

    if let Ok(mut cur) = LiftParams::decode_all(&mut &bytes[..]) {
        // Already current layout: only revert 2× split if still present.
        let mut changed = false;
        if cur.split_short_bps == PRIOR_2X_SPLIT_SHORT_BPS {
            cur.split_short_bps = LiftParams::default().split_short_bps;
            changed = true;
        }
        if changed {
            LiftParamsStore::<T>::put(cur);
            weight = weight.saturating_add(T::DbWeight::get().writes(1));
        }
        HasMigrationRun::<T>::insert(&migration_name, true);
        return weight.saturating_add(T::DbWeight::get().writes(1));
    }

    // Post-short-book / pre-util-apr intermediate: expand (+ revert 6667) so a
    // chain that already appended short_book_max_pool_alpha_bps still leaves
    // Cap_short at 50/50 even when util-apr is what finishes the layout widen.
    if let Ok(mid) = LiftParamsBeforeUtilAprLongBook::decode_all(&mut &bytes[..]) {
        LiftParamsStore::<T>::put(mid.into_current());
        HasMigrationRun::<T>::insert(&migration_name, true);
        return weight.saturating_add(T::DbWeight::get().writes(2));
    }

    if let Ok(old) = LiftParamsBeforeShortBookAlpha::decode_all(&mut &bytes[..]) {
        LiftParamsStore::<T>::put(old.into_current());
        HasMigrationRun::<T>::insert(&migration_name, true);
        weight = weight.saturating_add(T::DbWeight::get().writes(2));
    }
    weight
}

pub struct Migration<T: Config>(PhantomData<T>);

impl<T: Config> OnRuntimeUpgrade for Migration<T> {
    fn on_runtime_upgrade() -> Weight {
        migrate_lift_params_short_book_alpha::<T>()
    }

    #[cfg(feature = "try-runtime")]
    fn pre_upgrade() -> Result<Vec<u8>, TryRuntimeError> {
        let already_run = HasMigrationRun::<T>::get(migration_key());
        let raw = unhashed::get_raw(&LiftParamsStore::<T>::hashed_key());
        let snapshot = match raw.as_deref() {
            None => None,
            Some(bytes) => {
                if let Ok(old) = LiftParamsBeforeShortBookAlpha::decode_all(&mut &bytes[..]) {
                    Some(old.into_current())
                } else if let Ok(cur) = LiftParams::decode_all(&mut &bytes[..]) {
                    Some(cur)
                } else {
                    return Err(
                        "LiftParamsStore must decode as pre-short-book or current layout".into(),
                    );
                }
            }
        };
        Ok((already_run, snapshot).encode())
    }

    #[cfg(feature = "try-runtime")]
    fn post_upgrade(state: Vec<u8>) -> Result<(), TryRuntimeError> {
        let (already_run, before): (bool, Option<LiftParams>) =
            Decode::decode(&mut &state[..]).map_err(|_| "pre_upgrade state must decode")?;
        ensure!(
            HasMigrationRun::<T>::get(migration_key()),
            "short-book-alpha LiftParams migration must be stamped"
        );
        let after = LiftParamsStore::<T>::get();
        if already_run {
            if let Some(before) = before {
                ensure!(
                    after == before,
                    "already-run short-book-alpha migration must be a no-op"
                );
            }
        } else {
            ensure!(
                after.short_book_max_pool_alpha_bps
                    == LiftParams::default().short_book_max_pool_alpha_bps
                    || before.is_some_and(|b| {
                        b.short_book_max_pool_alpha_bps == after.short_book_max_pool_alpha_bps
                    }),
                "short_book_max_pool_alpha_bps must default after first expand"
            );
            ensure!(
                after.split_short_bps != PRIOR_2X_SPLIT_SHORT_BPS,
                "2× Cap_short split must be reverted"
            );
        }
        Ok(())
    }
}
