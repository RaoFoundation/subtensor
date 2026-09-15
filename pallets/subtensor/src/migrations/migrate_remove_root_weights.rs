use super::*;
use frame_support::{storage_alias, traits::Get, weights::Weight};
use log;
use scale_info::prelude::string::String;
use sp_std::vec::Vec;
use subtensor_runtime_common::{NetUid, NetUidStorageIndex};

/// Storage items retired with `set_root_weights`, aliased so this migration can read and
/// clear them after the pallet stopped declaring them.
pub mod retired {
    use super::*;

    /// The network-wide `set_root_weights` gate.
    #[storage_alias]
    pub type RootWeightSettingEnabled<T: Config> = StorageValue<Pallet<T>, bool, ValueQuery>;

    /// The concentration cap, formerly keyed by netuid (only `NetUid::ROOT` was ever read).
    #[storage_alias]
    pub type RootWeightsCap<T: Config> =
        StorageMap<Pallet<T>, Blake2_128Concat, NetUid, u16, OptionQuery>;
}

/// Removes the root basket weight vector design.
///
/// * Clears every stored root basket weight vector (`Weights[ROOT]`). No vector is seeded in
///   its place: funds have no target composition. Dividends accumulate in place on the
///   subnet they arrive on and direct deposits mirror the current holdings; the validator
///   changes composition only through `swap_basket`. Bounded by construction: `Weights[ROOT]`
///   holds at most one entry per root uid, and the root network is hard-capped at 64 uids
///   (`MaxAllowedUids[ROOT] = 64`), so a single-block clear is safe.
/// * Kills the `RootWeightSettingEnabled` gate (nothing reads it any more).
/// * Moves the concentration cap from `RootWeightsCap[ROOT]` to
///   [`crate::BasketConcentrationCap`], which now guards `swap_basket` buys alone, and drops
///   the retired map (any non-root entries were inert). A governance-set cap survives the
///   move; a chain still on the default keeps the default.
pub fn migrate_remove_root_weights<T: Config>() -> Weight {
    let mig_name: Vec<u8> = b"remove_root_weights_v1".to_vec();
    let mig_name_str = String::from_utf8_lossy(&mig_name);

    let mut total_weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(&mig_name) {
        log::info!("Migration '{mig_name_str}' already executed - skipping");
        return total_weight;
    }

    log::info!("Running migration '{mig_name_str}'");

    let result = Weights::<T>::clear_prefix(NetUidStorageIndex::ROOT, u32::MAX, None);
    let removed = result.unique as u64;
    total_weight = total_weight.saturating_add(T::DbWeight::get().reads_writes(removed, removed));
    if result.maybe_cursor.is_some() {
        log::error!(
            "Migration '{mig_name_str}' did not finish clearing Weights[ROOT]; \
             {removed} entries removed"
        );
    } else {
        log::info!("Migration '{mig_name_str}' cleared {removed} root basket weight vector(s)");
    }

    retired::RootWeightSettingEnabled::<T>::kill();
    total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));

    if let Some(cap) = retired::RootWeightsCap::<T>::get(NetUid::ROOT) {
        BasketConcentrationCap::<T>::put(cap);
        log::info!("Migration '{mig_name_str}' carried the concentration cap {cap} over");
        total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));
    }
    total_weight = total_weight.saturating_add(T::DbWeight::get().reads(1));
    let caps = retired::RootWeightsCap::<T>::clear(u32::MAX, None);
    let caps_removed = caps.unique as u64;
    total_weight =
        total_weight.saturating_add(T::DbWeight::get().reads_writes(caps_removed, caps_removed));

    HasMigrationRun::<T>::insert(&mig_name, true);
    total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!("Migration '{mig_name_str}' completed");

    total_weight
}
