//! Append `ema_band_bps` (default 1500) to stored [`LiftParams`](crate::LiftParams).
//!
//! Pre-band layouts decode as [`LiftParamsBeforeEmaBand`](crate::LiftParamsBeforeEmaBand).
//! Empty storage is left empty (`ValueQuery` still supplies [`Default`]).
use frame_support::{BoundedVec, weights::Weight};

use crate::*;

fn migration_key() -> BoundedVec<u8, MigrationKeyMaxLen> {
    BoundedVec::truncate_from(b"migrate_lift_params_ema_band_v1".to_vec())
}

/// Idempotent: guarded by [`HasMigrationRun`].
pub fn migrate_lift_params_ema_band<T: Config>() -> Weight {
    let migration_name = migration_key();
    let mut weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(&migration_name) {
        return weight;
    }

    weight = weight.saturating_add(T::DbWeight::get().reads_writes(1, 1));
    let _ = LiftParamsStore::<T>::translate::<LiftParamsBeforeEmaBand, _>(|old| {
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
    });

    HasMigrationRun::<T>::insert(&migration_name, true);
    weight.saturating_add(T::DbWeight::get().writes(1))
}
