//! Reset N24 shortfall counters recorded under the pre-fix semantics.
//!
//! Clone-503 measured per-reserve dips while Balancer `L` was already restored.
//! N24 (clone-504) records only a real `L` deficit valued in TAO. Clear the
//! stale `pool_value_given_below_baseline_*` fields on every per-netuid row and
//! the global totals so post-upgrade counters start clean (U7 / E2E-504).
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
    BoundedVec::truncate_from(b"migrate_reset_n24_shortfall_counters_v1".to_vec())
}

fn clear_shortfall(stats: &mut LendingStats) {
    stats.pool_value_given_below_baseline_tao = 0;
    stats.pool_value_given_below_baseline_alpha = 0;
}

/// Idempotent: guarded by [`HasMigrationRun`]. Zeroes the shortfall fields
/// only — other health accumulators are left intact.
pub fn migrate_reset_n24_shortfall_counters<T: Config>() -> Weight {
    let migration_name = migration_key();
    let mut weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(&migration_name) {
        return weight;
    }

    let mut reads: u64 = 0;
    let mut writes: u64 = 0;
    for (netuid, mut stats) in StatsByNetuid::<T>::iter() {
        reads = reads.saturating_add(1);
        if stats.pool_value_given_below_baseline_tao != 0
            || stats.pool_value_given_below_baseline_alpha != 0
        {
            clear_shortfall(&mut stats);
            StatsByNetuid::<T>::insert(netuid, stats);
            writes = writes.saturating_add(1);
        }
    }
    weight = weight.saturating_add(T::DbWeight::get().reads_writes(reads, writes));

    StatsGlobal::<T>::mutate(|stats| {
        clear_shortfall(stats);
    });
    weight = weight.saturating_add(T::DbWeight::get().reads_writes(1, 1));

    HasMigrationRun::<T>::insert(&migration_name, true);
    weight.saturating_add(T::DbWeight::get().writes(1))
}

pub struct Migration<T: Config>(PhantomData<T>);

impl<T: Config> OnRuntimeUpgrade for Migration<T> {
    fn on_runtime_upgrade() -> Weight {
        migrate_reset_n24_shortfall_counters::<T>()
    }

    #[cfg(feature = "try-runtime")]
    fn pre_upgrade() -> Result<Vec<u8>, TryRuntimeError> {
        let already_run = HasMigrationRun::<T>::get(migration_key());
        let pending_netuids = StatsByNetuid::<T>::iter()
            .filter(|(_, s)| {
                s.pool_value_given_below_baseline_tao != 0
                    || s.pool_value_given_below_baseline_alpha != 0
            })
            .count() as u32;
        let global = StatsGlobal::<T>::get();
        // Snapshot global shortfall so an already-stamped no-op can prove it
        // left live post-migration counters alone (Auditor fid:b4c3af08).
        Ok((
            already_run,
            pending_netuids,
            global.pool_value_given_below_baseline_tao,
            global.pool_value_given_below_baseline_alpha,
        )
            .encode())
    }

    #[cfg(feature = "try-runtime")]
    fn post_upgrade(state: Vec<u8>) -> Result<(), TryRuntimeError> {
        let (already_run, _pending_before, global_tao_before, global_alpha_before): (
            bool,
            u32,
            u128,
            u128,
        ) = Decode::decode(&mut &state[..]).map_err(|_| "pre_upgrade state must decode")?;
        ensure!(
            HasMigrationRun::<T>::get(migration_key()),
            "reset-n24-shortfall migration must be stamped"
        );
        if already_run {
            // Idempotent no-op: counters may have grown again after the first
            // run (under-recovery settlements). Require the stamp and that
            // this upgrade did not rewrite the global shortfall snapshot.
            let global = StatsGlobal::<T>::get();
            ensure!(
                global.pool_value_given_below_baseline_tao == global_tao_before
                    && global.pool_value_given_below_baseline_alpha == global_alpha_before,
                "already-run reset must leave global shortfall counters unchanged"
            );
            return Ok(());
        }
        ensure!(
            StatsByNetuid::<T>::iter().all(|(_, s)| {
                s.pool_value_given_below_baseline_tao == 0
                    && s.pool_value_given_below_baseline_alpha == 0
            }),
            "per-netuid shortfall counters must be zero after first migration run"
        );
        let global = StatsGlobal::<T>::get();
        ensure!(
            global.pool_value_given_below_baseline_tao == 0
                && global.pool_value_given_below_baseline_alpha == 0,
            "global shortfall counters must be zero after first migration run"
        );
        Ok(())
    }
}
