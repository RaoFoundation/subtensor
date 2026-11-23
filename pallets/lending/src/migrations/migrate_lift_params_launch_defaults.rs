//! Rewrite live [`LiftParamsStore`](crate::LiftParamsStore) launch defaults:
//! short APR 20%, deep-pool LTV 33%, Cap_short ≈ 2× Cap_long (`split_short_bps`
//! 6667). Only fields still equal to the prior defaults are rewritten so a
//! root that already sudo-set a value is left alone.
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

/// Prior defaults replaced by this migration.
const OLD_APR_SHORT_BPS: u16 = 10_000;
const OLD_LTV_BPS: u16 = 2_500;
const OLD_SPLIT_SHORT_BPS: u16 = 5_000;

fn migration_key() -> BoundedVec<u8, MigrationKeyMaxLen> {
    BoundedVec::truncate_from(b"migrate_lift_params_launch_defaults_v1".to_vec())
}

fn apply_launch_defaults(params: &mut LiftParams) -> bool {
    let mut changed = false;
    let fresh = LiftParams::default();
    if params.apr_short_bps == OLD_APR_SHORT_BPS {
        params.apr_short_bps = fresh.apr_short_bps;
        changed = true;
    }
    if params.ltv_short_bps == OLD_LTV_BPS {
        params.ltv_short_bps = fresh.ltv_short_bps;
        changed = true;
    }
    if params.ltv_long_bps == OLD_LTV_BPS {
        params.ltv_long_bps = fresh.ltv_long_bps;
        changed = true;
    }
    if params.split_short_bps == OLD_SPLIT_SHORT_BPS {
        params.split_short_bps = fresh.split_short_bps;
        changed = true;
    }
    changed
}

/// Idempotent: guarded by [`HasMigrationRun`].
pub fn migrate_lift_params_launch_defaults<T: Config>() -> Weight {
    let migration_name = migration_key();
    let mut weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(&migration_name) {
        return weight;
    }

    weight = weight.saturating_add(T::DbWeight::get().reads(1));
    let mut params = LiftParamsStore::<T>::get();
    if apply_launch_defaults(&mut params) {
        LiftParamsStore::<T>::put(params);
        weight = weight.saturating_add(T::DbWeight::get().writes(1));
    }
    HasMigrationRun::<T>::insert(&migration_name, true);
    weight.saturating_add(T::DbWeight::get().writes(1))
}

pub struct Migration<T: Config>(PhantomData<T>);

impl<T: Config> OnRuntimeUpgrade for Migration<T> {
    fn on_runtime_upgrade() -> Weight {
        migrate_lift_params_launch_defaults::<T>()
    }

    #[cfg(feature = "try-runtime")]
    fn pre_upgrade() -> Result<Vec<u8>, TryRuntimeError> {
        let already_run = HasMigrationRun::<T>::get(migration_key());
        let before = LiftParamsStore::<T>::get();
        Ok((already_run, before).encode())
    }

    #[cfg(feature = "try-runtime")]
    fn post_upgrade(state: Vec<u8>) -> Result<(), TryRuntimeError> {
        let (already_run, before): (bool, LiftParams) =
            Decode::decode(&mut &state[..]).map_err(|_| "pre_upgrade state must decode")?;
        ensure!(
            HasMigrationRun::<T>::get(migration_key()),
            "launch-defaults LiftParams migration must be stamped"
        );
        let after = LiftParamsStore::<T>::get();
        let fresh = LiftParams::default();
        if already_run {
            ensure!(
                after == before,
                "already-run launch-defaults must be a no-op"
            );
        } else {
            if before.apr_short_bps == OLD_APR_SHORT_BPS {
                ensure!(
                    after.apr_short_bps == fresh.apr_short_bps,
                    "apr_short_bps must move to the 20% launch default"
                );
            }
            if before.ltv_short_bps == OLD_LTV_BPS {
                ensure!(
                    after.ltv_short_bps == fresh.ltv_short_bps,
                    "ltv_short_bps must move to the 33% launch default"
                );
            }
            if before.ltv_long_bps == OLD_LTV_BPS {
                ensure!(
                    after.ltv_long_bps == fresh.ltv_long_bps,
                    "ltv_long_bps must move to the 33% launch default"
                );
            }
            if before.split_short_bps == OLD_SPLIT_SHORT_BPS {
                ensure!(
                    after.split_short_bps == fresh.split_short_bps,
                    "split_short_bps must move to the 2× short-cap launch default"
                );
            }
        }
        Ok(())
    }
}
