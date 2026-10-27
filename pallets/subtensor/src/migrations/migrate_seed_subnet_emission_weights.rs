use super::*;
use frame_support::{IterableStorageMap, traits::Get, weights::Weight};
use log;
use scale_info::prelude::string::String;
use sp_std::vec::Vec;
use subtensor_runtime_common::{AlphaBalance, NetUid};

/// `HasMigrationRun` key for this migration.
pub const MIGRATION_NAME: &[u8] = b"seed_subnet_emission_weights_v1";

/// Seed every live subnet's emission weight vector from the legacy owner-cut state so the
/// first post-upgrade block pays out exactly what the last pre-upgrade block did:
///
/// * owner slot = legacy `SubnetOwnerCut` scaled to u64 parts when `OwnerCutEnabled`, else 0;
/// * validator slot = half of the remainder (the old 50/50 split);
/// * miner slot = whatever is left, absorbing the rounding ulp;
/// * accrued owner cut = `alpha_issuance × legacy SubnetOwnerCut`, the exact deduction the
///   legacy lock refund applied before this upgrade, so dissolution pays no more than it
///   would have;
/// * `SubnetCreatedViaLease` = true for subnets with a live lease mapping (earlier leases
///   that already ended cannot be recovered and are not flagged).
///
/// Subnets that are already dissolving (out of `NetworksAdded`, still in
/// `DissolveCleanupQueue`) are paid their lock refund later from the cleanup queue, so the
/// ones whose refund is still pending get their accrued owner cut seeded too; otherwise the
/// refund would read an empty entry. Nothing else is seeded for them: the rest of the
/// vector state would only be orphaned by cleanup.
pub fn migrate_seed_subnet_emission_weights<T: Config>() -> Weight {
    let mig_name: Vec<u8> = MIGRATION_NAME.to_vec();
    let mig_name_str = String::from_utf8_lossy(&mig_name);

    let mut total_weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(&mig_name) {
        log::info!(target: "runtime", "Migration '{mig_name_str}' already executed - skipping");
        return total_weight;
    }

    log::info!(target: "runtime", "Running migration '{mig_name_str}'");

    let legacy_owner_cut = Pallet::<T>::get_subnet_owner_cut();
    total_weight = total_weight.saturating_add(T::DbWeight::get().reads(1));

    let netuids: Vec<NetUid> = <NetworksAdded<T> as IterableStorageMap<NetUid, bool>>::iter()
        .map(|(netuid, _)| netuid)
        .collect();
    total_weight = total_weight.saturating_add(T::DbWeight::get().reads(netuids.len() as u64));

    let mut seeded: u32 = 0;
    for netuid in netuids.into_iter().filter(|n| !n.is_root()) {
        let weights = seed_for::<T>(netuid, legacy_owner_cut);
        total_weight = total_weight.saturating_add(T::DbWeight::get().reads(2));

        SubnetEmissionWeights::<T>::insert(netuid, Pallet::<T>::emission_weights_to_vec(weights));
        SubnetOwnerCutAccrued::<T>::insert(
            netuid,
            legacy_owner_cut_accrued::<T>(netuid, legacy_owner_cut),
        );
        total_weight = total_weight.saturating_add(T::DbWeight::get().reads(2));
        if SubnetUidToLeaseId::<T>::contains_key(netuid) {
            SubnetCreatedViaLease::<T>::insert(netuid, true);
            total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));
        }
        total_weight = total_weight.saturating_add(T::DbWeight::get().writes(3));
        seeded = seeded.saturating_add(1);
    }

    let (dissolving, dissolving_weight) = seed_dissolving_subnets::<T>(legacy_owner_cut);
    total_weight = total_weight.saturating_add(dissolving_weight);

    HasMigrationRun::<T>::insert(&mig_name, true);
    total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        target: "runtime",
        "Migration '{mig_name_str}' completed: seeded {seeded} subnets, {dissolving} dissolving subnets with a pending lock refund"
    );

    total_weight
}

/// Netuids in the dissolve cleanup queue whose owner lock refund has not been paid yet.
/// The subnet under cleanup is still in the queue; its phase decides, every other queued
/// subnet has not started cleanup at all.
pub fn dissolving_with_refund_pending<T: Config>() -> Vec<NetUid> {
    let current = CurrentDissolveCleanupStatus::<T>::get();
    DissolveCleanupQueue::<T>::get()
        .into_iter()
        .filter(|netuid| !netuid.is_root())
        .filter(|netuid| match &current {
            Some(status) if status.netuid == *netuid => status.phase.lock_refund_pending(),
            _ => true,
        })
        .collect()
}

/// Seed `SubnetOwnerCutAccrued` for dissolving subnets that still owe a lock refund and
/// have no entry yet. Returns `(count, weight)`.
fn seed_dissolving_subnets<T: Config>(legacy_owner_cut: u16) -> (u32, Weight) {
    let mut weight = T::DbWeight::get().reads(2);
    let mut count: u32 = 0;
    for netuid in dissolving_with_refund_pending::<T>() {
        weight = weight.saturating_add(T::DbWeight::get().reads(1));
        if SubnetOwnerCutAccrued::<T>::contains_key(netuid) {
            continue;
        }
        SubnetOwnerCutAccrued::<T>::insert(
            netuid,
            Pallet::<T>::legacy_owner_cut_accrued(netuid, legacy_owner_cut),
        );
        weight = weight.saturating_add(T::DbWeight::get().reads_writes(2, 1));
        count = count.saturating_add(1);
    }
    (count, weight)
}

/// The vector a subnet receives, given the legacy global owner cut.
pub fn seed_for<T: Config>(netuid: NetUid, legacy_owner_cut: u16) -> EmissionWeights {
    let owner = if OwnerCutEnabled::<T>::get(netuid) {
        Pallet::<T>::scale_u16_fraction_to_emission_weight(legacy_owner_cut)
    } else {
        0
    };
    Pallet::<T>::emission_weights_with_owner(owner)
}

/// The owner α the legacy lock refund would have deducted before this upgrade.
pub fn legacy_owner_cut_accrued<T: Config>(netuid: NetUid, legacy_owner_cut: u16) -> AlphaBalance {
    Pallet::<T>::legacy_owner_cut_accrued(netuid, legacy_owner_cut)
}

/// [`OnRuntimeUpgrade`] wrapper for [`migrate_seed_subnet_emission_weights`], registered in
/// the runtime `Migrations` tuple so try-runtime validates the seed against real network
/// state.
pub mod seed_subnet_emission_weights {
    use super::*;
    use frame_support::traits::OnRuntimeUpgrade;
    use sp_std::marker::PhantomData;

    #[cfg(feature = "try-runtime")]
    use crate::subnets::emission_weights::EMISSION_WEIGHT_ONE;
    #[cfg(feature = "try-runtime")]
    use codec::{Decode, Encode};
    #[cfg(feature = "try-runtime")]
    use frame_support::ensure;
    #[cfg(feature = "try-runtime")]
    use sp_runtime::TryRuntimeError;

    /// State carried from `pre_upgrade` to `post_upgrade`: whether the migration had
    /// already run, the legacy owner cut, `(netuid, owner_cut_enabled, leased,
    /// legacy_accrued_owner_alpha)` per live non-root subnet, and
    /// `(netuid, had_accrued_entry, legacy_accrued_owner_alpha)` per dissolving subnet
    /// whose lock refund is still pending.
    #[cfg(feature = "try-runtime")]
    type PreUpgradeState = (
        bool,
        u16,
        Vec<(NetUid, bool, bool, AlphaBalance)>,
        Vec<(NetUid, bool, AlphaBalance)>,
    );

    pub struct Migration<T: Config>(PhantomData<T>);

    impl<T: Config> OnRuntimeUpgrade for Migration<T> {
        fn on_runtime_upgrade() -> Weight {
            migrate_seed_subnet_emission_weights::<T>()
        }

        #[cfg(feature = "try-runtime")]
        fn pre_upgrade() -> Result<Vec<u8>, TryRuntimeError> {
            let already_run = HasMigrationRun::<T>::get(MIGRATION_NAME.to_vec());
            let legacy_owner_cut = Pallet::<T>::get_subnet_owner_cut();
            let subnets: Vec<(NetUid, bool, bool, AlphaBalance)> =
                <NetworksAdded<T> as IterableStorageMap<NetUid, bool>>::iter()
                    .map(|(netuid, _)| netuid)
                    .filter(|n| !n.is_root())
                    .map(|netuid| {
                        (
                            netuid,
                            OwnerCutEnabled::<T>::get(netuid),
                            SubnetUidToLeaseId::<T>::contains_key(netuid),
                            legacy_owner_cut_accrued::<T>(netuid, legacy_owner_cut),
                        )
                    })
                    .collect();
            let dissolving: Vec<(NetUid, bool, AlphaBalance)> =
                dissolving_with_refund_pending::<T>()
                    .into_iter()
                    .map(|netuid| {
                        (
                            netuid,
                            SubnetOwnerCutAccrued::<T>::contains_key(netuid),
                            legacy_owner_cut_accrued::<T>(netuid, legacy_owner_cut),
                        )
                    })
                    .collect();
            log::info!(
                target: "runtime",
                "seed_subnet_emission_weights pre_upgrade: {} live subnets, {} dissolving subnets with a pending lock refund",
                subnets.len(),
                dissolving.len()
            );
            Ok((already_run, legacy_owner_cut, subnets, dissolving).encode())
        }

        #[cfg(feature = "try-runtime")]
        fn post_upgrade(state: Vec<u8>) -> Result<(), TryRuntimeError> {
            let (already_run, legacy_owner_cut, subnets, dissolving): PreUpgradeState =
                Decode::decode(&mut &state[..]).map_err(|_| "pre_upgrade state must decode")?;
            ensure!(
                HasMigrationRun::<T>::get(MIGRATION_NAME.to_vec()),
                "the migration must be stamped"
            );
            // Holds whether or not the migration ran in this upgrade: no subnet that still
            // owes a lock refund may be without an accrued owner-cut entry.
            for netuid in dissolving_with_refund_pending::<T>() {
                ensure!(
                    SubnetOwnerCutAccrued::<T>::contains_key(netuid),
                    "a dissolving subnet with a pending lock refund must have an accrued owner cut entry"
                );
            }
            if already_run {
                return Ok(());
            }
            for (netuid, had_entry, legacy_accrued) in dissolving {
                if !had_entry {
                    ensure!(
                        SubnetOwnerCutAccrued::<T>::get(netuid) == legacy_accrued,
                        "a dissolving subnet must seed its accrued owner cut to the legacy refund deduction"
                    );
                }
                ensure!(
                    !SubnetEmissionWeights::<T>::contains_key(netuid),
                    "dissolving subnets must not receive an emission weight vector"
                );
            }
            for (netuid, owner_cut_enabled, leased, legacy_accrued) in subnets {
                let weights = Pallet::<T>::get_subnet_emission_weights(netuid);
                let total: u128 = weights.iter().map(|w| *w as u128).sum();
                ensure!(
                    total == EMISSION_WEIGHT_ONE as u128,
                    "seeded vector must sum to one"
                );
                let owner = Pallet::<T>::get_subnet_owner_emission_cut(netuid);
                if owner_cut_enabled {
                    ensure!(
                        owner
                            == Pallet::<T>::scale_u16_fraction_to_emission_weight(legacy_owner_cut),
                        "owner slot must equal the scaled legacy owner cut"
                    );
                } else {
                    ensure!(owner == 0, "disabled owner cut must seed a zero owner slot");
                }
                let validator = Pallet::<T>::get_subnet_validator_emission_cut(netuid);
                ensure!(
                    validator == EMISSION_WEIGHT_ONE.saturating_sub(owner) / 2,
                    "validator slot must be half of the post-owner remainder"
                );
                ensure!(
                    Pallet::<T>::get_subnet_created_via_lease(netuid) == leased,
                    "lease marker must mirror the live lease mapping"
                );
                ensure!(
                    Pallet::<T>::get_subnet_burn_emission(netuid) == 0
                        && Pallet::<T>::get_subnet_recycle_emission(netuid) == 0,
                    "burn and recycle slots must seed to zero"
                );
                ensure!(
                    SubnetOwnerCutAccrued::<T>::get(netuid) == legacy_accrued,
                    "accrued owner cut must seed to the legacy refund deduction"
                );
            }
            Ok(())
        }
    }
}
