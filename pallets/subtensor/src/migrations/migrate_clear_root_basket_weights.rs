use super::*;
use frame_support::weights::Weight;
use log;
use scale_info::prelude::string::String;
use subtensor_runtime_common::NetUidStorageIndex;

/// Clears every stored root basket weight vector (`Weights[ROOT][uid]`).
///
/// Validators with no stored vector already default to 100% root (TAO in the fund's
/// root slot) at distribution time. Any validator that previously called
/// `set_root_weights` may still have a custom vector on chain; this migration
/// removes those entries so everyone uses the runtime default until they opt in
/// again with a fresh `set_root_weights` call.
pub fn migrate_clear_root_basket_weights<T: Config>() -> Weight {
    let mig_name: Vec<u8> = b"clear_root_basket_weights".to_vec();
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
