use super::*;
use frame_support::weights::Weight;
use log;
use scale_info::prelude::string::String;
use sp_std::vec::Vec;
use subtensor_runtime_common::{NetUid, NetUidStorageIndex};

/// Clears every stored root basket weight vector and reseeds every registered root
/// validator with a fully balanced `1/n` vector over every existing non-root subnet.
///
/// Legacy `set_root_weights` / root-network weight vectors reused `Weights[ROOT]` and
/// are not meaningful basket curation; wiping them avoids carrying old subnet-score
/// vectors into Root Reborn. The balanced seed is the network-wide starting point:
/// equal weight on each live subnet (root / netuid 0 is excluded — that slot holds
/// TAO cash and is not part of the equal-subnet basket). Validators may overwrite
/// with `set_root_weights` afterwards.
///
/// Uses a fresh `HasMigrationRun` key (`seed_balanced_root_basket_weights`) so chains
/// that already ran the superseded clear-only migration (`clear_root_basket_weights`)
/// still receive the balanced seed.
pub fn migrate_clear_root_basket_weights<T: Config>() -> Weight {
    let mig_name: Vec<u8> = b"seed_balanced_root_basket_weights".to_vec();
    let mig_name_str = String::from_utf8_lossy(&mig_name);

    let mut total_weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(&mig_name) {
        log::info!("Migration '{mig_name_str}' already executed - skipping");
        return total_weight;
    }

    log::info!("Running migration '{mig_name_str}'");

    // Bounded by construction: `Weights[ROOT]` holds at most one entry per root uid, and
    // the root network is hard-capped at 64 uids (`MaxAllowedUids[ROOT] = 64`, set at
    // genesis and in `migrate_create_root_network`), so a single-block clear+reseeds is safe.
    let result = Weights::<T>::clear_prefix(NetUidStorageIndex::ROOT, u32::MAX, None);
    let removed = result.unique as u64;
    total_weight = total_weight.saturating_add(T::DbWeight::get().reads_writes(removed, removed));

    if result.maybe_cursor.is_some() {
        log::error!(
            "Migration '{mig_name_str}' did not finish clearing Weights[ROOT]; \
             {removed} entries removed"
        );
    } else {
        log::info!(
            "Migration '{mig_name_str}' cleared {removed} stored root basket weight vector(s)"
        );
    }

    // Destinations: every live non-root subnet, sorted for a deterministic stored vector.
    let mut dests: Vec<u16> = Pallet::<T>::get_all_subnet_netuids()
        .into_iter()
        .filter(|netuid| !netuid.is_root())
        .map(u16::from)
        .collect();
    dests.sort_unstable();
    total_weight = total_weight.saturating_add(T::DbWeight::get().reads(dests.len() as u64 + 1));

    let balanced: Vec<(u16, u16)> = if dests.is_empty() {
        // No productive subnets yet — leave empty so the runtime default (100% root) applies.
        Vec::new()
    } else {
        // Equal relative weights, max-upscaled like `set_root_weights` (all `u16::MAX`).
        dests
            .into_iter()
            .map(|netuid| (netuid, u16::MAX))
            .collect()
    };

    let mut seeded: u64 = 0;
    if !balanced.is_empty() {
        for (uid, _hotkey) in Keys::<T>::iter_prefix(NetUid::ROOT) {
            Weights::<T>::insert(NetUidStorageIndex::ROOT, uid, balanced.clone());
            seeded = seeded.saturating_add(1);
            total_weight = total_weight.saturating_add(T::DbWeight::get().reads_writes(1, 1));
        }
    }

    log::info!(
        "Migration '{mig_name_str}' seeded balanced 1/n root basket weights on {seeded} root uid(s) \
         across {} subnet destination(s)",
        balanced.len()
    );

    HasMigrationRun::<T>::insert(&mig_name, true);
    total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!("Migration '{mig_name_str}' completed");

    total_weight
}
