use super::*;
use frame_support::{
    pallet_prelude::{Blake2_128Concat, ValueQuery},
    storage_alias,
    weights::Weight,
};
use log;
use scale_info::prelude::string::String;
use sp_std::vec::Vec;
use subtensor_runtime_common::NetUidStorageIndex;

/// Storage items retired together with the `set_root_weights` extrinsic. Aliased here only
/// so the migration can delete their on-chain values.
pub mod deprecated {
    use super::*;

    #[storage_alias]
    pub type RootWeightSettingEnabled<T: Config> = StorageValue<Pallet<T>, bool, ValueQuery>;

    #[storage_alias]
    pub type RootWeightsCap<T: Config> =
        StorageMap<Pallet<T>, Blake2_128Concat, NetUid, u16, ValueQuery>;
}

/// Clears every stored root basket weight vector and the retired `set_root_weights`
/// gate/cap storage.
///
/// Root validators no longer declare a target weight vector for their beta basket: the
/// `set_root_weights` extrinsic is gone, dividends always accumulate in place on the subnet
/// they arrive on, and validators rebalance the fund directly with `swap_basket_alpha`.
/// Vectors stored under `Weights[ROOT]` (legacy root-network scores, or curation vectors set
/// while `set_root_weights` was live) are read by nothing and are wiped here so they cannot
/// be mistaken for live strategy; the `RootWeightSettingEnabled` switch and the
/// `RootWeightsCap` map that gated the removed extrinsic are deleted for the same reason.
///
/// Uses a fresh `HasMigrationRun` key (`clear_root_basket_weights_v3`) so chains that ran
/// the v2 clear before curation opened get the vectors set since then wiped too.
pub fn migrate_clear_root_basket_weights<T: Config>() -> Weight {
    let mig_name: Vec<u8> = b"clear_root_basket_weights_v3".to_vec();
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

    // Retired gate and cap: a single value and (in practice) a single `ROOT` entry.
    deprecated::RootWeightSettingEnabled::<T>::kill();
    let caps = deprecated::RootWeightsCap::<T>::clear(u32::MAX, None);
    total_weight = total_weight.saturating_add(
        T::DbWeight::get().reads_writes(caps.loops as u64, (caps.unique as u64).saturating_add(1)),
    );

    HasMigrationRun::<T>::insert(&mig_name, true);
    total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!("Migration '{mig_name_str}' completed");

    total_weight
}
