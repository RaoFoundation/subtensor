use super::*;
use frame_support::weights::Weight;
use log;
use scale_info::prelude::string::String;
use sp_std::vec::Vec;
use subtensor_runtime_common::NetUidStorageIndex;

/// Clears every stored root basket weight vector.
///
/// Legacy `set_root_weights` / root-network weight vectors reused `Weights[ROOT]` and
/// are not meaningful basket curation; wiping them avoids carrying old subnet-score
/// vectors into Root Reborn. No vector is seeded in their place: an empty stored vector
/// means the fund is *uncurated* — dividends accumulate in place on the subnet they
/// arrive on, trade-free (see [`crate::Pallet::distribute_root_alpha_to_basket`]) —
/// so every validator starts Root Reborn holding exactly what its dividends earn,
/// with no protocol-imposed sells or buys, until it sets an explicit vector.
///
/// Uses a fresh `HasMigrationRun` key (`clear_root_basket_weights_v2`) so environments
/// that ran the superseded balanced-seed variant (`seed_balanced_root_basket_weights`)
/// get their seeded (and now-stale) 1/n vectors wiped too.
pub fn migrate_clear_root_basket_weights<T: Config>() -> Weight {
    let mig_name: Vec<u8> = b"clear_root_basket_weights_v2".to_vec();
    let mig_name_str = String::from_utf8_lossy(&mig_name);

    let mut total_weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(&mig_name) {
        log::info!("Migration '{mig_name_str}' already executed - skipping");
        return total_weight;
    }

    log::info!("Running migration '{mig_name_str}'");

    // Bounded by construction: `Weights[ROOT]` holds at most one entry per root uid, and
    // the root network is hard-capped at 64 uids (`MaxAllowedUids[ROOT] = 64`, set at
    // genesis and in `migrate_create_root_network`), so a single-block clear is safe.
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

    HasMigrationRun::<T>::insert(&mig_name, true);
    total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!("Migration '{mig_name_str}' completed");

    total_weight
}
