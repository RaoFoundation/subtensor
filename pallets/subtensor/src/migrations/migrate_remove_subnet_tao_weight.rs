use super::*;
use frame_support::{traits::Get, weights::Weight};
use log;
use scale_info::prelude::string::String;
use sp_io::hashing::twox_128;
use sp_std::vec::Vec;

/// `HasMigrationRun` key for this migration.
pub const MIGRATION_NAME: &[u8] = b"remove_subnet_tao_weight_v1";

/// Storage prefix of the removed `SubtensorModule::SubnetTaoWeight` map.
pub fn subnet_tao_weight_prefix() -> Vec<u8> {
    let mut prefix = Vec::with_capacity(32);
    prefix.extend_from_slice(&twox_128(b"SubtensorModule"));
    prefix.extend_from_slice(&twox_128(b"SubnetTaoWeight"));
    prefix
}

/// Number of raw storage keys still under the removed map's prefix.
pub fn subnet_tao_weight_entries() -> u64 {
    let prefix = subnet_tao_weight_prefix();
    let mut count: u64 = 0;
    let mut key = prefix.clone();
    while let Some(next) = sp_io::storage::next_key(&key) {
        if !next.starts_with(&prefix) {
            break;
        }
        count = count.saturating_add(1);
        key = next;
    }
    count
}

/// Remove every `SubnetTaoWeight` entry (spec 480's per-subnet frozen TAO weight).
///
/// Spec 481 pays root its live baseline share off the top of the validator slot every
/// block, so the per-subnet weight has no reader any more and the storage item is gone
/// from the pallet. Entries written while spec 480 ran (testnet) are cleared here so they
/// do not linger as orphaned keys. Nothing else changes: the emission weight vectors, the
/// lease markers and the accrued owner cuts stay as they are.
pub fn migrate_remove_subnet_tao_weight<T: Config>() -> Weight {
    let mig_name: Vec<u8> = MIGRATION_NAME.to_vec();
    let mig_name_str = String::from_utf8_lossy(&mig_name);

    let mut total_weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(&mig_name) {
        log::info!(target: "runtime", "Migration '{mig_name_str}' already executed - skipping");
        return total_weight;
    }

    log::info!(target: "runtime", "Running migration '{mig_name_str}'");

    remove_prefix::<T>("SubtensorModule", "SubnetTaoWeight", &mut total_weight);

    HasMigrationRun::<T>::insert(&mig_name, true);
    total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(target: "runtime", "Migration '{mig_name_str}' completed");

    total_weight
}

/// [`OnRuntimeUpgrade`] wrapper for [`migrate_remove_subnet_tao_weight`], registered in
/// the runtime `Migrations` tuple so try-runtime validates the removal against real
/// network state and reports how the new root payout differs from spec 480's.
pub mod remove_subnet_tao_weight {
    use super::*;
    use frame_support::traits::OnRuntimeUpgrade;
    use sp_std::marker::PhantomData;

    #[cfg(feature = "try-runtime")]
    use crate::subnets::emission_weights::root_emission_share;
    #[cfg(feature = "try-runtime")]
    use codec::{Decode, Encode};
    #[cfg(feature = "try-runtime")]
    use frame_support::{IterableStorageMap, ensure};
    #[cfg(feature = "try-runtime")]
    use sp_runtime::TryRuntimeError;
    #[cfg(feature = "try-runtime")]
    use substrate_fixed::types::U96F32;
    #[cfg(feature = "try-runtime")]
    use subtensor_runtime_common::NetUid;

    /// State carried from `pre_upgrade` to `post_upgrade`: the number of
    /// `SubnetTaoWeight` entries found before the upgrade.
    #[cfg(feature = "try-runtime")]
    type PreUpgradeState = u64;

    pub struct Migration<T: Config>(PhantomData<T>);

    impl<T: Config> OnRuntimeUpgrade for Migration<T> {
        fn on_runtime_upgrade() -> Weight {
            migrate_remove_subnet_tao_weight::<T>()
        }

        #[cfg(feature = "try-runtime")]
        fn pre_upgrade() -> Result<Vec<u8>, TryRuntimeError> {
            let entries = subnet_tao_weight_entries();
            log::info!(
                target: "runtime",
                "remove_subnet_tao_weight pre_upgrade: {entries} SubnetTaoWeight entries"
            );
            Ok(entries.encode())
        }

        #[cfg(feature = "try-runtime")]
        fn post_upgrade(state: Vec<u8>) -> Result<(), TryRuntimeError> {
            let entries_before: PreUpgradeState =
                Decode::decode(&mut &state[..]).map_err(|_| "pre_upgrade state must decode")?;
            ensure!(
                HasMigrationRun::<T>::get(MIGRATION_NAME.to_vec()),
                "the migration must be stamped"
            );
            ensure!(
                subnet_tao_weight_entries() == 0,
                "no SubnetTaoWeight entry may remain"
            );

            // Every live subnet now pays root `min(V, r)` of its emission with `r` the live
            // baseline. Report the subnets whose validator slot is off the default `V0`:
            // those are the ones whose root payout changes against spec 480 (frozen
            // weight) and, for owner-cut-disabled subnets seeded at `V = 0.5`, against
            // pre-etao main (which paid root `0.5 · root_proportion` there).
            let v0 = Pallet::<T>::default_validator_emission();
            let mut off_default: u32 = 0;
            let mut capped: u32 = 0;
            for (netuid, _) in <NetworksAdded<T> as IterableStorageMap<NetUid, bool>>::iter()
                .filter(|(netuid, _)| !netuid.is_root())
            {
                let validator = Pallet::<T>::get_subnet_validator_emission_cut(netuid);
                let baseline = Pallet::<T>::baseline_root_emission_share(netuid);
                let root_share = Pallet::<T>::get_root_emission_share(netuid);
                ensure!(
                    root_share <= Pallet::<T>::get_float_subnet_validator_emission_cut(netuid)
                        && root_share <= baseline,
                    "root's share must be the baseline capped at the validator slot"
                );
                if validator != v0 {
                    off_default = off_default.saturating_add(1);
                    // What pre-etao main (and spec 480 without a frozen weight) paid root
                    // here: `V · root_proportion`, i.e. the baseline scaled by `V / V0`.
                    let legacy_share = root_emission_share(
                        Pallet::<T>::get_float_subnet_validator_emission_cut(netuid),
                        Pallet::<T>::get_tao_weight(),
                        U96F32::saturating_from_num(Pallet::<T>::get_subnet_tao(NetUid::ROOT)),
                        U96F32::saturating_from_num(Pallet::<T>::get_alpha_issuance(netuid)),
                    );
                    log::info!(
                        target: "runtime",
                        "remove_subnet_tao_weight: netuid {netuid:?} validator slot {validator} != V0 {v0}; root share {legacy_share:?} (V · root_proportion, as on main) -> {root_share:?} (baseline {baseline:?})"
                    );
                }
                if Pallet::<T>::is_root_emission_share_capped(netuid) {
                    capped = capped.saturating_add(1);
                    log::warn!(
                        target: "runtime",
                        "remove_subnet_tao_weight: netuid {netuid:?} root baseline {baseline:?} exceeds its validator slot; alpha validators receive nothing until V is raised"
                    );
                }
            }
            log::info!(
                target: "runtime",
                "remove_subnet_tao_weight post_upgrade: removed {entries_before} entries; {off_default} subnets off the default validator share, {capped} capped"
            );
            Ok(())
        }
    }
}
