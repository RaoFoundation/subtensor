//! Seed lending `References` / `G_ema` / `ActiveNets` for every live subnet.
//!
//! Before this migration, opens could never commit a reference on a real chain
//! (transactional open + gate-before-init). Existing subnets therefore need a
//! one-shot warm so EMA aging can begin without a manual `warm_reference`.
use frame_support::{BoundedVec, weights::Weight};

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
