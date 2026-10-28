//! Append `ema_band_bps` (default 1500) to stored [`LiftParams`](crate::LiftParams).
//!
//! Pre-band layouts decode as [`LiftParamsBeforeEmaBand`](crate::LiftParamsBeforeEmaBand).
//! Empty storage is left empty (`ValueQuery` still supplies [`Default`]).
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
    BoundedVec::truncate_from(b"migrate_lift_params_ema_band_v1".to_vec())
}

/// Idempotent: guarded by [`HasMigrationRun`].
///
/// Translation failure leaves the old value and does **not** stamp the guard,
/// so a later upgrade can retry instead of locking the pre-band layout in.
pub fn migrate_lift_params_ema_band<T: Config>() -> Weight {
    let migration_name = migration_key();
    let mut weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(&migration_name) {
        return weight;
    }

    weight = weight.saturating_add(T::DbWeight::get().reads_writes(1, 1));
    match LiftParamsStore::<T>::translate::<LiftParamsBeforeEmaBand, _>(|old| {
        old.map(|o| LiftParams {
            floor_bps: o.floor_bps,
            thin_floor_bps: o.thin_floor_bps,
            ref_tao: o.ref_tao,
            move_bps: o.move_bps,
            ceiling_bps: o.ceiling_bps,
            split_short_bps: o.split_short_bps,
            rebalance_target_bps: o.rebalance_target_bps,
            rebalance_rate_bps: o.rebalance_rate_bps,
            thin_tao: o.thin_tao,
            ltv_short_bps: o.ltv_short_bps,
            ltv_long_bps: o.ltv_long_bps,
            thin_ltv_long_bps: o.thin_ltv_long_bps,
            apr_bps: o.apr_bps,
            slice_bps: o.slice_bps,
            crash_guard_bps: o.crash_guard_bps,
            settle_ema_blocks: o.settle_ema_blocks,
            new_subnet_min_age: o.new_subnet_min_age,
            new_subnet_ramp: o.new_subnet_ramp,
            ema_band_bps: 1_500,
        })
    }) {
        Ok(_) => {
            HasMigrationRun::<T>::insert(&migration_name, true);
            weight.saturating_add(T::DbWeight::get().writes(1))
        }
        Err(_) => {
            // Leave the guard clear so a subsequent upgrade can retry.
            weight
        }
    }
}

pub struct Migration<T: Config>(PhantomData<T>);

impl<T: Config> OnRuntimeUpgrade for Migration<T> {
    fn on_runtime_upgrade() -> Weight {
        migrate_lift_params_ema_band::<T>()
    }

    #[cfg(feature = "try-runtime")]
    fn pre_upgrade() -> Result<Vec<u8>, TryRuntimeError> {
        let already_run = HasMigrationRun::<T>::get(migration_key());
        // Decode the on-disk blob as either the pre-band layout or the current
        // layout. `LiftParamsStore::get()` cannot see the old encoding and would
        // substitute Default, invalidating the preservation check.
        let raw = frame_support::storage::unhashed::get_raw(&LiftParamsStore::<T>::hashed_key());
        let snapshot = match raw.as_deref() {
            None => None,
            Some(bytes) => {
                if let Ok(old) = LiftParamsBeforeEmaBand::decode(&mut &bytes[..]) {
                    Some(LiftParams {
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
                        settle_ema_blocks: old.settle_ema_blocks,
                        new_subnet_min_age: old.new_subnet_min_age,
                        new_subnet_ramp: old.new_subnet_ramp,
                        ema_band_bps: 0,
                    })
                } else if let Ok(cur) = LiftParams::decode(&mut &bytes[..]) {
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
                        && after.apr_bps == before.apr_bps
                        && after.new_subnet_ramp == before.new_subnet_ramp,
                    "ema-band migration must preserve existing LiftParams fields"
                );
            }
        }
        Ok(())
    }
}
