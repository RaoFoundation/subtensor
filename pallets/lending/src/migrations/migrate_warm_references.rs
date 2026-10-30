//! Seed lending `References` / `G_ema` / `ActiveNets` for every live subnet.
//!
//! Before this migration, opens could never commit a reference on a real chain
//! (transactional open + gate-before-init). Existing subnets therefore need a
//! one-shot warm so EMA aging can begin without a manual `warm_reference`.
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
    BoundedVec::truncate_from(b"migrate_warm_references_v1".to_vec())
}

/// Idempotent: guarded by [`HasMigrationRun`]. Skips subnets that cannot be
/// seeded (no price, dissolving, not alive) without failing the upgrade.
pub fn migrate_warm_references<T: Config>() -> Weight {
    let migration_name = migration_key();
    let mut weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(&migration_name) {
        return weight;
    }

    // Ensure custody once up front; individual seeds are no-ops if it fails.
    weight = weight.saturating_add(T::DbWeight::get().reads_writes(2, 2));
    let _ = Pallet::<T>::initialize_custody();

    let live = T::Pool::live_subnets();
    weight = weight.saturating_add(T::DbWeight::get().reads(1));

    for netuid in live {
        // Per-subnet probes: alive, dissolution, price, reference, lifts, ActiveNets.
        weight = weight.saturating_add(T::WeightInfo::warm_reference());
        let _ = Pallet::<T>::try_seed_reference(netuid);
    }

    HasMigrationRun::<T>::insert(&migration_name, true);
    weight.saturating_add(T::DbWeight::get().writes(1))
}

pub struct Migration<T: Config>(PhantomData<T>);

impl<T: Config> OnRuntimeUpgrade for Migration<T> {
    fn on_runtime_upgrade() -> Weight {
        migrate_warm_references::<T>()
    }

    #[cfg(feature = "try-runtime")]
    fn pre_upgrade() -> Result<Vec<u8>, TryRuntimeError> {
        let already_run = HasMigrationRun::<T>::get(migration_key());
        let active = ActiveNetCount::<T>::get();
        Ok((already_run, active).encode())
    }

    #[cfg(feature = "try-runtime")]
    fn post_upgrade(state: Vec<u8>) -> Result<(), TryRuntimeError> {
        let (already_run, active_before): (bool, u32) =
            Decode::decode(&mut &state[..]).map_err(|_| "pre_upgrade state must decode")?;
        ensure!(
            HasMigrationRun::<T>::get(migration_key()),
            "warm-reference migration must be stamped"
        );
        if already_run {
            ensure!(
                ActiveNetCount::<T>::get() == active_before,
                "already-run warm-reference migration must not change ActiveNetCount"
            );
        }
        // Seeded nets must have a Reference when the pool was alive and priced.
        for (netuid, _) in ActiveNets::<T>::iter() {
            ensure!(
                References::<T>::contains_key(netuid),
                "ActiveNets entry must have a Reference after warm migration"
            );
        }
        Ok(())
    }
}
