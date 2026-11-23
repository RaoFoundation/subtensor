//! Append utilization short-APR curve knobs and
//! [`LiftParams::long_book_max_pool_tao_bps`](crate::LiftParams::long_book_max_pool_tao_bps).
//!
//! Pre-append layout decodes as
//! [`LiftParamsBeforeUtilAprLongBook`](crate::LiftParamsBeforeUtilAprLongBook).
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

fn migration_key() -> BoundedVec<u8, MigrationKeyMaxLen> {
    BoundedVec::truncate_from(b"migrate_lift_params_util_apr_long_book_v1".to_vec())
}

/// Idempotent: guarded by [`HasMigrationRun`].
pub fn migrate_lift_params_util_apr_long_book<T: Config>() -> Weight {
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
        // Already current: still revert a live 2× Cap_short split if present.
        if cur.split_short_bps == 6_667 {
            cur.split_short_bps = LiftParams::default().split_short_bps;
            LiftParamsStore::<T>::put(cur);
            weight = weight.saturating_add(T::DbWeight::get().writes(1));
        }
        HasMigrationRun::<T>::insert(&migration_name, true);
        return weight.saturating_add(T::DbWeight::get().writes(1));
    }

    if let Ok(old) = LiftParamsBeforeUtilAprLongBook::decode_all(&mut &bytes[..]) {
        LiftParamsStore::<T>::put(old.into_current());
        HasMigrationRun::<T>::insert(&migration_name, true);
        weight = weight.saturating_add(T::DbWeight::get().writes(2));
    }
    weight
}

pub struct Migration<T: Config>(PhantomData<T>);

impl<T: Config> OnRuntimeUpgrade for Migration<T> {
    fn on_runtime_upgrade() -> Weight {
        migrate_lift_params_util_apr_long_book::<T>()
    }

    #[cfg(feature = "try-runtime")]
    fn pre_upgrade() -> Result<Vec<u8>, TryRuntimeError> {
        let already_run = HasMigrationRun::<T>::get(migration_key());
        let raw = unhashed::get_raw(&LiftParamsStore::<T>::hashed_key());
        let snapshot = match raw.as_deref() {
            None => None,
            Some(bytes) => {
                if let Ok(old) = LiftParamsBeforeUtilAprLongBook::decode_all(&mut &bytes[..]) {
                    Some(old.into_current())
                } else if let Ok(cur) = LiftParams::decode_all(&mut &bytes[..]) {
                    Some(cur)
                } else {
                    return Err(
                        "LiftParamsStore must decode as pre-util-apr or current layout".into(),
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
            "util-apr/long-book LiftParams migration must be stamped"
        );
        let after = LiftParamsStore::<T>::get();
        if already_run {
            if let Some(before) = before {
                ensure!(
                    after == before,
                    "already-run util-apr/long-book migration must be a no-op"
                );
            }
        } else {
            let fresh = LiftParams::default();
            ensure!(
                after.apr_short_kink_bps == fresh.apr_short_kink_bps
                    || before.is_some_and(|b| b.apr_short_kink_bps == after.apr_short_kink_bps),
                "apr_short_kink_bps must default after first expand"
            );
            ensure!(
                after.long_book_max_pool_tao_bps == fresh.long_book_max_pool_tao_bps
                    || before
                        .is_some_and(|b| b.long_book_max_pool_tao_bps == after.long_book_max_pool_tao_bps),
                "long_book_max_pool_tao_bps must default after first expand"
            );
        }
        Ok(())
    }
}
