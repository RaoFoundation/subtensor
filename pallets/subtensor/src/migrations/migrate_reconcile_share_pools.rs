use crate::staking::stake_utils::HotkeyAlphaSharePoolDataOperations;
use crate::{Config, Event, HasMigrationRun, Owner, Pallet, StakingHotkeys, TotalHotkeyAlpha};
use codec::{Decode, DecodeWithMemTracking, Encode};
use frame_support::{pallet_prelude::OptionQuery, storage_alias, traits::Get, weights::Weight};
use scale_info::TypeInfo;
use scale_info::prelude::string::String;
use share_pool::{SafeFloat, SharePoolDataOperations};
use sp_runtime::traits::Zero;
use sp_std::collections::btree_map::BTreeMap;
use sp_std::vec::Vec;
use subtensor_runtime_common::{AlphaBalance, NetUid, Token};

pub(crate) const MIGRATION_NAME: &[u8] = b"migrate_reconcile_share_pools";

/// Persistent cursor for the bounded share-pool reconciliation.
#[derive(Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo)]
pub struct ReconcileSharePoolsProgress {
    /// Raw `TotalHotkeyAlpha` key of the last entry of the last fully processed hotkey.
    /// Empty starts at the first entry.
    pub cursor: Vec<u8>,
    /// Pools whose denominator was rewritten to the sum of live shares.
    pub reconciled: u32,
    /// Pools whose unowned value was assigned to the hotkey owner.
    pub adopted: u32,
}

#[storage_alias]
pub type ReconcileSharePoolsMigration<T: Config> =
    StorageValue<Pallet<T>, ReconcileSharePoolsProgress, OptionQuery>;

/// True while the reconciliation cursor exists.
pub fn in_progress<T: Config>() -> bool {
    ReconcileSharePoolsMigration::<T>::exists()
}

/// What one pool looks like once its live rows are summed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PoolState {
    /// `Σ shares == D`.
    Consistent,
    /// Live rows exist and `Σ shares != D`; `D` must become the sum.
    Divergent { sum: SafeFloat },
    /// Value with no live rows: nobody owns it, the next depositor would.
    Unowned,
}

/// Sum the live (current-epoch, non-zero) shares of `coldkeys` in the pool.
fn classify_pool<T: Config>(
    ops: &HotkeyAlphaSharePoolDataOperations<T>,
    coldkeys: &[T::AccountId],
) -> PoolState {
    let mut sum = SafeFloat::zero();
    let mut live_rows: u32 = 0;
    for coldkey in coldkeys {
        if let Ok(share) = ops.try_get_share(coldkey)
            && !share.is_zero()
        {
            live_rows = live_rows.saturating_add(1);
            sum = sum.add(&share).unwrap_or_else(|| sum.clone());
        }
    }
    if live_rows == 0 {
        return PoolState::Unowned;
    }
    let denominator = ops.get_denominator();
    if sum.gt(&denominator) || denominator.gt(&sum) {
        PoolState::Divergent { sum }
    } else {
        PoolState::Consistent
    }
}

/// Reconcile every pool of one hotkey. `pools` are its `(netuid, value)` rows from
/// `TotalHotkeyAlpha`. Returns `(reconciled, adopted, weight)`.
fn process_hotkey<T: Config>(
    hotkey: &T::AccountId,
    pools: &[(NetUid, AlphaBalance)],
) -> (u32, u32, Weight) {
    let mut rows_by_netuid: BTreeMap<NetUid, Vec<T::AccountId>> = BTreeMap::new();
    let mut row_count: u64 = 0;
    for (coldkey, netuid, _) in Pallet::<T>::alpha_iter_single_prefix(hotkey) {
        row_count = row_count.saturating_add(1);
        rows_by_netuid.entry(netuid).or_default().push(coldkey);
    }
    // Two maps walked for the prefix, then per row the legacy/current share reads and the
    // epoch check; per pool the denominator and epoch reads.
    let mut weight = T::DbWeight::get().reads(
        row_count
            .saturating_mul(4)
            .saturating_add(pools.len() as u64 * 3)
            .saturating_add(2),
    );
    let mut reconciled: u32 = 0;
    let mut adopted: u32 = 0;
    let owner = Owner::<T>::contains_key(hotkey).then(|| Owner::<T>::get(hotkey));

    for (netuid, value) in pools {
        if value.is_zero() {
            continue;
        }
        let mut ops = HotkeyAlphaSharePoolDataOperations::<T>::new(hotkey.clone(), *netuid);
        let coldkeys = rows_by_netuid.get(netuid).map(Vec::as_slice).unwrap_or(&[]);
        match classify_pool::<T>(&ops, coldkeys) {
            PoolState::Consistent => {}
            PoolState::Divergent { sum } => {
                ops.set_denominator(sum);
                weight.saturating_accrue(T::DbWeight::get().writes(2));
                reconciled = reconciled.saturating_add(1);
                Pallet::<T>::deposit_event(Event::SharePoolDenominatorReconciled {
                    hotkey: hotkey.clone(),
                    netuid: *netuid,
                });
            }
            PoolState::Unowned => {
                let Some(owner) = owner.as_ref() else {
                    log::warn!(
                        "Share pool {hotkey:?}/{netuid:?} holds {value:?} with no live rows and no owner; left as is"
                    );
                    continue;
                };
                // Close any stale denominator first so every retired row reads as absent,
                // then open the pool for the owner with all of its value: the same state a
                // first deposit of `value` would create.
                if !ops.get_denominator().is_zero() {
                    ops.set_denominator(SafeFloat::zero());
                }
                let Some(all) = SafeFloat::new(u128::from(value.to_u64()), 0) else {
                    continue;
                };
                ops.set_denominator(all.clone());
                ops.set_share(owner, all);
                let mut staking_hotkeys = StakingHotkeys::<T>::get(owner);
                if !staking_hotkeys.contains(hotkey) {
                    staking_hotkeys.push(hotkey.clone());
                    StakingHotkeys::<T>::insert(owner, staking_hotkeys);
                }
                weight.saturating_accrue(T::DbWeight::get().reads_writes(2, 5));
                adopted = adopted.saturating_add(1);
                Pallet::<T>::deposit_event(Event::SharePoolAdopted {
                    hotkey: hotkey.clone(),
                    netuid: *netuid,
                    coldkey: owner.clone(),
                    alpha: *value,
                });
            }
        }
    }
    (reconciled, adopted, weight)
}

/// Schedule the reconciliation. Does no map walk in the upgrade block.
pub fn migrate_reconcile_share_pools<T: Config>() -> Weight {
    let mut weight = T::DbWeight::get().reads(2);
    if HasMigrationRun::<T>::get(MIGRATION_NAME) || ReconcileSharePoolsMigration::<T>::exists() {
        return weight;
    }
    ReconcileSharePoolsMigration::<T>::put(ReconcileSharePoolsProgress {
        cursor: Vec::new(),
        reconciled: 0,
        adopted: 0,
    });
    weight.saturating_accrue(T::DbWeight::get().writes(1));
    log::info!(
        "Migration '{}' scheduled for bounded on_idle execution",
        String::from_utf8_lossy(MIGRATION_NAME)
    );
    weight
}

/// Continue the reconciliation using no more than `limit`. One hotkey (all of its pools) is
/// the unit of work: its rows are read once and every pool it has is settled in the same
/// pass, so the cursor only ever rests between hotkeys.
pub fn continue_reconcile_share_pools<T: Config>(limit: Weight) -> Weight {
    let pass_overhead = T::DbWeight::get().reads_writes(1, 2);
    if !pass_overhead.all_lte(limit) {
        return Weight::zero();
    }
    let Some(mut progress) = ReconcileSharePoolsMigration::<T>::get() else {
        return T::DbWeight::get().reads(1);
    };

    let work_limit = limit.saturating_sub(pass_overhead);
    let mut work_weight = Weight::zero();

    let iter = if progress.cursor.is_empty() {
        TotalHotkeyAlpha::<T>::iter()
    } else {
        TotalHotkeyAlpha::<T>::iter_from(progress.cursor.clone())
    };

    // Entries of one hotkey are contiguous (the hotkey is the first, concat-hashed key).
    let mut current: Option<(T::AccountId, Vec<(NetUid, AlphaBalance)>, Vec<u8>)> = None;
    for (hotkey, netuid, value) in iter {
        work_weight.saturating_accrue(T::DbWeight::get().reads(1));
        let key = TotalHotkeyAlpha::<T>::hashed_key_for(&hotkey, netuid);
        match current.as_mut() {
            Some((current_hotkey, pools, last_key)) if *current_hotkey == hotkey => {
                pools.push((netuid, value));
                *last_key = key;
            }
            _ => {
                if let Some((done_hotkey, pools, last_key)) = current.take() {
                    let (reconciled, adopted, weight) = process_hotkey::<T>(&done_hotkey, &pools);
                    work_weight.saturating_accrue(weight);
                    progress.reconciled = progress.reconciled.saturating_add(reconciled);
                    progress.adopted = progress.adopted.saturating_add(adopted);
                    progress.cursor = last_key;
                    if !work_weight.all_lte(work_limit) {
                        ReconcileSharePoolsMigration::<T>::put(progress);
                        return pass_overhead.saturating_add(work_weight);
                    }
                }
                current = Some((hotkey, sp_std::vec![(netuid, value)], key));
            }
        }
    }
    if let Some((done_hotkey, pools, _)) = current.take() {
        let (reconciled, adopted, weight) = process_hotkey::<T>(&done_hotkey, &pools);
        work_weight.saturating_accrue(weight);
        progress.reconciled = progress.reconciled.saturating_add(reconciled);
        progress.adopted = progress.adopted.saturating_add(adopted);
    }

    HasMigrationRun::<T>::insert(MIGRATION_NAME, true);
    ReconcileSharePoolsMigration::<T>::kill();
    log::info!(
        "Migration '{}' completed: {} pools reconciled, {} pools adopted",
        String::from_utf8_lossy(MIGRATION_NAME),
        progress.reconciled,
        progress.adopted
    );
    pass_overhead.saturating_add(work_weight)
}

/// Sum of `TotalHotkeyAlpha` per subnet; the migration must leave it unchanged.
#[cfg(any(feature = "try-runtime", test))]
pub fn value_per_subnet<T: Config>() -> BTreeMap<NetUid, u64> {
    let mut totals: BTreeMap<NetUid, u64> = BTreeMap::new();
    for (_, netuid, value) in TotalHotkeyAlpha::<T>::iter() {
        let total = totals.entry(netuid).or_default();
        *total = total.saturating_add(value.to_u64());
    }
    totals
}

/// Every pool with value must have live rows whose shares sum to its denominator (within
/// one part in 10^12, the rounding the share arithmetic itself allows). Returns the first
/// violation as a message.
#[cfg(any(feature = "try-runtime", test))]
pub fn check_all_pools<T: Config>() -> Result<(), &'static str> {
    let mut rows_by_hotkey: BTreeMap<T::AccountId, Vec<(NetUid, AlphaBalance)>> = BTreeMap::new();
    for (hotkey, netuid, value) in TotalHotkeyAlpha::<T>::iter() {
        rows_by_hotkey
            .entry(hotkey)
            .or_default()
            .push((netuid, value));
    }
    for (hotkey, pools) in rows_by_hotkey {
        let mut rows_by_netuid: BTreeMap<NetUid, Vec<T::AccountId>> = BTreeMap::new();
        for (coldkey, netuid, _) in Pallet::<T>::alpha_iter_single_prefix(&hotkey) {
            rows_by_netuid.entry(netuid).or_default().push(coldkey);
        }
        for (netuid, value) in pools {
            if value.is_zero() {
                continue;
            }
            let ops = HotkeyAlphaSharePoolDataOperations::<T>::new(hotkey.clone(), netuid);
            let coldkeys = rows_by_netuid
                .get(&netuid)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            match classify_pool::<T>(&ops, coldkeys) {
                PoolState::Consistent => {}
                PoolState::Divergent { sum } => {
                    let denominator = ops.get_denominator();
                    let tolerance = denominator
                        .mul_div(
                            &SafeFloat::new(1, 0).unwrap_or_default(),
                            &SafeFloat::new(1, 12).unwrap_or_default(),
                        )
                        .unwrap_or_else(SafeFloat::zero);
                    let upper = denominator
                        .add(&tolerance)
                        .unwrap_or_else(|| denominator.clone());
                    let lower = denominator.sub(&tolerance).unwrap_or_else(SafeFloat::zero);
                    if sum.gt(&upper) {
                        return Err("share sum exceeds the pool denominator");
                    }
                    if lower.gt(&sum) {
                        return Err("share sum falls short of the pool denominator");
                    }
                }
                PoolState::Unowned => {
                    if Owner::<T>::contains_key(&hotkey) {
                        return Err("owned pool still holds value without live rows");
                    }
                }
            }
        }
    }
    Ok(())
}

/// [`OnRuntimeUpgrade`](frame_support::traits::OnRuntimeUpgrade) wrapper with try-runtime
/// validation. The upgrade block only schedules the paged work; `post_upgrade` drives the
/// pages to completion offline and then checks every pool: `Σ shares == D` (within rounding)
/// wherever live rows exist, no owned pool holds value without live rows, and the value per
/// subnet is exactly what it was before.
pub mod reconcile_share_pools {
    use super::*;
    use frame_support::traits::OnRuntimeUpgrade;
    use sp_std::marker::PhantomData;

    #[cfg(feature = "try-runtime")]
    use frame_support::ensure;
    #[cfg(feature = "try-runtime")]
    use sp_runtime::TryRuntimeError;

    #[cfg(feature = "try-runtime")]
    #[derive(Encode, Decode)]
    struct PreUpgradeState {
        already_run: bool,
        value_per_subnet: Vec<(NetUid, u64)>,
    }

    pub struct Migration<T: Config>(PhantomData<T>);

    impl<T: Config> OnRuntimeUpgrade for Migration<T> {
        fn on_runtime_upgrade() -> Weight {
            migrate_reconcile_share_pools::<T>()
        }

        #[cfg(feature = "try-runtime")]
        fn pre_upgrade() -> Result<Vec<u8>, TryRuntimeError> {
            Ok(PreUpgradeState {
                already_run: HasMigrationRun::<T>::get(MIGRATION_NAME.to_vec()),
                value_per_subnet: value_per_subnet::<T>().into_iter().collect(),
            }
            .encode())
        }

        #[cfg(feature = "try-runtime")]
        fn post_upgrade(state: Vec<u8>) -> Result<(), TryRuntimeError> {
            let before: PreUpgradeState =
                Decode::decode(&mut &state[..]).map_err(|_| "pre_upgrade state must decode")?;
            if !before.already_run {
                ensure!(
                    in_progress::<T>(),
                    "reconciliation must be scheduled by the upgrade"
                );
            }
            // Drive the paged work to completion offline.
            let mut passes: u32 = 0;
            while in_progress::<T>() {
                continue_reconcile_share_pools::<T>(Weight::from_parts(u64::MAX, u64::MAX));
                passes = passes.saturating_add(1);
                ensure!(passes < 1_000, "reconciliation must terminate");
            }
            ensure!(
                HasMigrationRun::<T>::get(MIGRATION_NAME.to_vec()),
                "reconciliation marker must be set"
            );
            let after: Vec<(NetUid, u64)> = value_per_subnet::<T>().into_iter().collect();
            ensure!(
                after == before.value_per_subnet,
                "reconciliation must not move value between or within subnets"
            );
            check_all_pools::<T>().map_err(TryRuntimeError::Other)?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{tests::mock::*, *};
    use sp_core::U256;

    fn huge_limit() -> Weight {
        Weight::from_parts(u64::MAX, u64::MAX)
    }

    fn run_migration() {
        migrate_reconcile_share_pools::<Test>();
        while in_progress::<Test>() {
            continue_reconcile_share_pools::<Test>(huge_limit());
        }
    }

    // A pool whose shares add up to more than its denominator quotes a member more than its
    // fraction; the migration sets the denominator to the sum so quotes are exact again.
    #[test]
    fn divergent_pool_denominator_becomes_the_share_sum() {
        new_test_ext(1).execute_with(|| {
            let netuid = NetUid::from(2);
            let hotkey = U256::from(1);
            let (alice, bob) = (U256::from(11), U256::from(12));
            add_network(netuid, 1, 0);
            SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &alice,
                netuid,
                1_000_000u64.into(),
            );
            SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &bob,
                netuid,
                1_000_000u64.into(),
            );
            // Pre-fix state: one member's share row inflated beyond what the denominator covers.
            let inflated = SafeFloat::new(1_500_000, 0).unwrap();
            AlphaV2::<Test>::insert((hotkey, alice, netuid), inflated.clone());
            let value_before = value_per_subnet::<Test>();
            assert!(check_all_pools::<Test>().is_err());
            let alice_quote_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &alice, netuid,
            );

            run_migration();

            let ops = HotkeyAlphaSharePoolDataOperations::<Test>::new(hotkey, netuid);
            let expected = inflated
                .add(&SafeFloat::new(1_000_000, 0).unwrap())
                .unwrap();
            assert!(!ops.get_denominator().gt(&expected) && !expected.gt(&ops.get_denominator()));
            assert_eq!(value_per_subnet::<Test>(), value_before);
            assert!(check_all_pools::<Test>().is_ok());
            // Quotes now split the value by the real share fractions: 1.5M / 2.5M of 2M.
            let alice_quote = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &alice, netuid,
            );
            let bob_quote =
                SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &bob, netuid);
            assert!(alice_quote < alice_quote_before);
            assert_eq!(alice_quote, 1_200_000u64.into());
            assert_eq!(bob_quote, 800_000u64.into());
            assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));
            assert!(!in_progress::<Test>());
        });
    }

    // Value with no live rows would go to the next depositor. The migration assigns it to the
    // hotkey owner, closing any stale denominator first.
    #[test]
    fn unowned_pool_value_is_assigned_to_the_owner() {
        new_test_ext(1).execute_with(|| {
            let netuid = NetUid::from(2);
            let hotkey = U256::from(1);
            let owner = U256::from(10);
            let stranger = U256::from(20);
            add_network(netuid, 1, 0);
            Owner::<Test>::insert(hotkey, owner);

            // Orphan: value, no denominator, no rows.
            TotalHotkeyAlpha::<Test>::insert(hotkey, netuid, AlphaBalance::from(5_000_000u64));
            // Stranded: value and a denominator, but the only row is retired.
            let netuid2 = NetUid::from(3);
            add_network(netuid2, 1, 0);
            TotalHotkeyAlpha::<Test>::insert(hotkey, netuid2, AlphaBalance::from(7_000_000u64));
            TotalHotkeySharesV2::<Test>::insert(
                hotkey,
                netuid2,
                SafeFloat::new(7_000_000, 0).unwrap(),
            );
            AlphaSharePoolEpoch::<Test>::insert(hotkey, netuid2, 3);
            AlphaV2::<Test>::insert(
                (hotkey, stranger, netuid2),
                SafeFloat::new(7_000_000, 0).unwrap(),
            );
            AlphaShareEpoch::<Test>::insert((hotkey, stranger, netuid2), 1);
            let value_before = value_per_subnet::<Test>();
            assert!(check_all_pools::<Test>().is_err());

            run_migration();

            assert_eq!(
                SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey, &owner, netuid
                ),
                5_000_000u64.into()
            );
            assert_eq!(
                SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey, &owner, netuid2
                ),
                7_000_000u64.into()
            );
            assert!(
                SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey, &stranger, netuid2
                )
                .is_zero()
            );
            assert!(StakingHotkeys::<Test>::get(owner).contains(&hotkey));
            assert_eq!(value_per_subnet::<Test>(), value_before);
            assert!(check_all_pools::<Test>().is_ok());

            // A later deposit is priced against the owner's shares, not handed the pool.
            SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &stranger,
                netuid,
                1_000_000u64.into(),
            );
            assert_eq!(
                SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey, &stranger, netuid
                ),
                1_000_000u64.into()
            );
            assert_eq!(
                SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey, &owner, netuid
                ),
                5_000_000u64.into()
            );
        });
    }

    // Healthy pools are untouched, the cursor rests between hotkeys, and the pass is paged.
    #[test]
    fn healthy_pools_are_left_alone_and_work_is_paged() {
        new_test_ext(1).execute_with(|| {
            let netuid = NetUid::from(2);
            add_network(netuid, 1, 0);
            for i in 1..=4u64 {
                SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                    &U256::from(i),
                    &U256::from(100 + i),
                    netuid,
                    (i * 1_000_000).into(),
                );
            }
            let denominators: Vec<SafeFloat> = (1..=4u64)
                .map(|i| TotalHotkeySharesV2::<Test>::get(U256::from(i), netuid))
                .collect();

            migrate_reconcile_share_pools::<Test>();
            // Enough for the overhead plus one hotkey's work.
            let one_hotkey = <Test as frame_system::Config>::DbWeight::get().reads_writes(12, 2);
            continue_reconcile_share_pools::<Test>(one_hotkey);
            let progress = ReconcileSharePoolsMigration::<Test>::get().expect("paged");
            assert!(!progress.cursor.is_empty());
            assert_eq!(progress.reconciled, 0);
            assert_eq!(progress.adopted, 0);

            run_migration();
            for (i, denominator) in (1..=4u64).zip(denominators) {
                let now = TotalHotkeySharesV2::<Test>::get(U256::from(i), netuid);
                assert!(!now.gt(&denominator) && !denominator.gt(&now));
            }
            assert!(check_all_pools::<Test>().is_ok());
        });
    }
}
