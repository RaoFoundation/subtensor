use super::*;
use frame_support::{storage_alias, traits::Get, weights::Weight};
use log;
use scale_info::prelude::string::String;
use sp_std::vec::Vec;
use subtensor_runtime_common::{NetUid, NetUidStorageIndex};

/// `HasMigrationRun` key for this migration.
pub const MIGRATION_NAME: &[u8] = b"remove_root_weights_v1";

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
    let mig_name: Vec<u8> = MIGRATION_NAME.to_vec();
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
             {removed} entries removed; not stamping HasMigrationRun"
        );
        return total_weight;
    }
    log::info!("Migration '{mig_name_str}' cleared {removed} root basket weight vector(s)");

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
    if caps.maybe_cursor.is_some() {
        log::error!(
            "Migration '{mig_name_str}' did not finish clearing RootWeightsCap; \
             {caps_removed} entries removed; not stamping HasMigrationRun"
        );
        return total_weight;
    }

    HasMigrationRun::<T>::insert(&mig_name, true);
    total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!("Migration '{mig_name_str}' completed");

    total_weight
}

/// [`OnRuntimeUpgrade`] wrapper for [`migrate_remove_root_weights`], registered in the
/// runtime `Migrations` tuple (not the pallet hook) so try-runtime validates the cleanup
/// and the cap carry-over against real network state.
pub mod remove_root_weights {
    use super::*;
    use frame_support::traits::OnRuntimeUpgrade;
    use sp_std::marker::PhantomData;

    #[cfg(feature = "try-runtime")]
    use codec::{Decode, Encode};
    #[cfg(feature = "try-runtime")]
    use frame_support::ensure;
    #[cfg(feature = "try-runtime")]
    use sp_runtime::TryRuntimeError;

    /// State carried from `pre_upgrade` to `post_upgrade`: whether the migration had
    /// already run, the governance-set cap under the retired map (if any), the cap in
    /// its new home, and the number of non-root `Weights` rows (which must survive).
    #[cfg(feature = "try-runtime")]
    type PreUpgradeState = (bool, Option<u16>, u16, u64);

    pub struct Migration<T: Config>(PhantomData<T>);

    impl<T: Config> OnRuntimeUpgrade for Migration<T> {
        fn on_runtime_upgrade() -> Weight {
            migrate_remove_root_weights::<T>()
        }

        #[cfg(feature = "try-runtime")]
        fn pre_upgrade() -> Result<Vec<u8>, TryRuntimeError> {
            let already_run = HasMigrationRun::<T>::get(MIGRATION_NAME.to_vec());
            let old_cap = retired::RootWeightsCap::<T>::get(NetUid::ROOT);
            let new_cap = BasketConcentrationCap::<T>::get();
            let non_root_rows = Weights::<T>::iter_keys()
                .filter(|(index, _)| *index != NetUidStorageIndex::ROOT)
                .count() as u64;
            Ok((already_run, old_cap, new_cap, non_root_rows).encode())
        }

        #[cfg(feature = "try-runtime")]
        fn post_upgrade(state: Vec<u8>) -> Result<(), TryRuntimeError> {
            let (already_run, old_cap, new_cap_before, non_root_rows): PreUpgradeState =
                Decode::decode(&mut &state[..]).map_err(|_| "pre_upgrade state must decode")?;

            ensure!(
                HasMigrationRun::<T>::get(MIGRATION_NAME.to_vec()),
                "migrate_remove_root_weights must mark itself as run"
            );
            ensure!(
                Weights::<T>::iter_prefix(NetUidStorageIndex::ROOT)
                    .next()
                    .is_none(),
                "every root basket weight vector must be cleared"
            );
            ensure!(
                !retired::RootWeightSettingEnabled::<T>::exists(),
                "the RootWeightSettingEnabled gate must be killed"
            );
            ensure!(
                retired::RootWeightsCap::<T>::iter().next().is_none(),
                "the retired RootWeightsCap map must be emptied"
            );
            // A governance-set cap moves to its new home; otherwise the new item keeps
            // whatever it held (the default on a first run). A re-run (already marked)
            // must not touch the cap at all.
            let expected_cap = match (already_run, old_cap) {
                (false, Some(cap)) => cap,
                _ => new_cap_before,
            };
            ensure!(
                BasketConcentrationCap::<T>::get() == expected_cap,
                "the concentration cap must be carried over exactly"
            );
            ensure!(
                Weights::<T>::iter_keys()
                    .filter(|(index, _)| *index != NetUidStorageIndex::ROOT)
                    .count() as u64
                    == non_root_rows,
                "non-root subnet weights must be untouched"
            );
            Ok(())
        }
    }
}
