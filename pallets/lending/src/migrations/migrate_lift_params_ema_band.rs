//! Append `ema_band_bps` (default 1500) to stored [`LiftParams`](crate::LiftParams).
//!
//! Pre-band layouts decode as [`LiftParamsBeforeEmaBand`](crate::LiftParamsBeforeEmaBand).
//! Empty storage is left empty (`ValueQuery` still supplies [`Default`]).
//! Production path uses `DecodeAll`: if the current layout already decodes, the
//! migration is a stamped no-op (audit L6).
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
    BoundedVec::truncate_from(b"migrate_lift_params_ema_band_v1".to_vec())
}

fn from_before(old: LiftParamsBeforeEmaBand) -> LiftParams {
    // Jump straight to the current economic layout so a chain that still has
    // the pre-band blob does not depend on the follow-on expand migration.
    LiftParamsBeforeEconomic {
        floor_bps: old.floor_bps,
        thin_floor_bps: old.thin_floor_bps,
        ref_tao: old.ref_tao,
        move_bps: old.move_bps,
        ceiling_bps: old.ceiling_bps,
        split_short_bps: old.split_short_bps,
        rebalance_target_bps: old.rebalance_target_bps,
        rebalance_rate_bps: old.rebalance_rate_bps,
        thin_tao: old.thin_tao,
        ltv_short_bps: old.ltv_short_bps,
        ltv_long_bps: old.ltv_long_bps,
        thin_ltv_long_bps: old.thin_ltv_long_bps,
        apr_bps: old.apr_bps,
        slice_bps: old.slice_bps,
        crash_guard_bps: old.crash_guard_bps,
        total_tao_ema_blocks: old.total_tao_ema_blocks,
        new_subnet_min_age: old.new_subnet_min_age,
        new_subnet_ramp: old.new_subnet_ramp,
        ema_band_bps: 1_500,
    }
    .into_current()
}

/// Idempotent: guarded by [`HasMigrationRun`].
///
/// DecodeAll on the current layout is a stamped no-op. Pre-band blobs are
/// rewritten. Undecodable values leave the guard clear for a later retry.
pub fn migrate_lift_params_ema_band<T: Config>() -> Weight {
    let migration_name = migration_key();
    let mut weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(&migration_name) {
        return weight;
    }

    weight = weight.saturating_add(T::DbWeight::get().reads(1));
    let key = LiftParamsStore::<T>::hashed_key();
    let Some(bytes) = unhashed::get_raw(&key) else {
        // Empty storage: ValueQuery Default already has ema_band_bps.
        HasMigrationRun::<T>::insert(&migration_name, true);
        return weight.saturating_add(T::DbWeight::get().writes(1));
    };

    if LiftParams::decode_all(&mut &bytes[..]).is_ok() {
        // Already current layout — stamped no-op (audit L6).
        HasMigrationRun::<T>::insert(&migration_name, true);
        return weight.saturating_add(T::DbWeight::get().writes(1));
    }

    if let Ok(old) = LiftParamsBeforeEmaBand::decode_all(&mut &bytes[..]) {
        LiftParamsStore::<T>::put(from_before(old));
        HasMigrationRun::<T>::insert(&migration_name, true);
        weight = weight.saturating_add(T::DbWeight::get().writes(2));
    }
    // Undecodable: leave guard clear for a later upgrade retry.
    weight
}

pub struct Migration<T: Config>(PhantomData<T>);

impl<T: Config> OnRuntimeUpgrade for Migration<T> {
    fn on_runtime_upgrade() -> Weight {
        migrate_lift_params_ema_band::<T>()
    }

    #[cfg(feature = "try-runtime")]
    fn pre_upgrade() -> Result<Vec<u8>, TryRuntimeError> {
        let already_run = HasMigrationRun::<T>::get(migration_key());
        let raw = unhashed::get_raw(&LiftParamsStore::<T>::hashed_key());
        let snapshot = match raw.as_deref() {
            None => None,
            Some(bytes) => {
                if let Ok(old) = LiftParamsBeforeEmaBand::decode_all(&mut &bytes[..]) {
                    let mut p = from_before(old);
                    p.ema_band_bps = 0;
                    Some(p)
                } else if let Ok(cur) = LiftParams::decode_all(&mut &bytes[..]) {
                    Some(cur)
                } else {
                    return Err("LiftParamsStore must decode as pre-band or current layout".into());
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
            "ema-band migration must be stamped"
        );
        let after = LiftParamsStore::<T>::get();
        if already_run {
            if let Some(before) = before {
                ensure!(
                    after == before,
                    "already-run ema-band migration must be a no-op"
                );
            }
        } else {
            ensure!(
                after.ema_band_bps == 1_500,
                "ema_band_bps must be the default after first run"
            );
            if let Some(before) = before {
                ensure!(
                    after.floor_bps == before.floor_bps
                        && after.apr_long_bps == before.apr_long_bps
                        && after.apr_short_bps == before.apr_short_bps
                        && after.new_subnet_ramp == before.new_subnet_ramp,
                    "ema-band migration must preserve existing LiftParams fields"
                );
            }
        }
        Ok(())
    }
}
