//! Append [`LiftParams::thin_blend_tao`](crate::LiftParams::thin_blend_tao)
//! to live [`LiftParamsStore`](crate::LiftParamsStore).
//!
//! Pre-append layout decodes as
//! [`LiftParamsBeforeThinBlend`](crate::LiftParamsBeforeThinBlend).
//! Empty storage is left empty (`ValueQuery` still supplies [`Default`]).
//! Production path uses `DecodeAll`: if the current layout already decodes, the
//! migration is a stamped no-op.
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
    BoundedVec::truncate_from(b"migrate_lift_params_thin_blend_v1".to_vec())
}

/// Idempotent: guarded by [`HasMigrationRun`].
///
/// DecodeAll on the current layout is a stamped no-op. Pre-blend blobs are
/// rewritten with default `thin_blend_tao`. Undecodable values leave the guard
/// clear for a later retry.
pub fn migrate_lift_params_thin_blend<T: Config>() -> Weight {
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

    if LiftParams::decode_all(&mut &bytes[..]).is_ok() {
        HasMigrationRun::<T>::insert(&migration_name, true);
        return weight.saturating_add(T::DbWeight::get().writes(1));
    }

    if let Ok(old) = LiftParamsBeforeThinBlend::decode_all(&mut &bytes[..]) {
        LiftParamsStore::<T>::put(old.into_current());
        HasMigrationRun::<T>::insert(&migration_name, true);
        weight = weight.saturating_add(T::DbWeight::get().writes(2));
    }
    weight
}

pub struct Migration<T: Config>(PhantomData<T>);

impl<T: Config> OnRuntimeUpgrade for Migration<T> {
    fn on_runtime_upgrade() -> Weight {
        migrate_lift_params_thin_blend::<T>()
    }

    #[cfg(feature = "try-runtime")]
    fn pre_upgrade() -> Result<Vec<u8>, TryRuntimeError> {
        let already_run = HasMigrationRun::<T>::get(migration_key());
        let raw = unhashed::get_raw(&LiftParamsStore::<T>::hashed_key());
        let snapshot = match raw.as_deref() {
            None => None,
            Some(bytes) => {
                if let Ok(old) = LiftParamsBeforeThinBlend::decode_all(&mut &bytes[..]) {
                    Some(old.into_current())
                } else if let Ok(cur) = LiftParams::decode_all(&mut &bytes[..]) {
                    Some(cur)
                } else {
                    return Err(
                        "LiftParamsStore must decode as pre-thin-blend or current layout".into(),
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
            "thin-blend LiftParams migration must be stamped"
        );
        let after = LiftParamsStore::<T>::get();
        if already_run {
            if let Some(before) = before {
                ensure!(
                    after == before,
                    "already-run thin-blend migration must be a no-op"
                );
            }
        } else {
            ensure!(
                after.thin_blend_tao == LiftParams::default().thin_blend_tao,
                "thin_blend_tao must default after first expand"
            );
            if let Some(before) = before {
                ensure!(
                    after.floor_bps == before.floor_bps
                        && after.expiry_max_slippage_bps == before.expiry_max_slippage_bps
                        && after.max_expiry_settlements_per_block
                            == before.max_expiry_settlements_per_block,
                    "thin-blend migration must preserve existing LiftParams fields"
                );
            }
        }
        Ok(())
    }
}
