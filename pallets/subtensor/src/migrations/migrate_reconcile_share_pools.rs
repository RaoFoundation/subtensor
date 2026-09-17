use crate::staking::stake_utils::HotkeyAlphaSharePoolDataOperations;
use crate::{Alpha, AlphaV2, Config, Event, HasMigrationRun, Pallet};
use alloc::collections::BTreeSet;
use codec::Decode;
use frame_support::{traits::Get, weights::Weight};
use scale_info::prelude::string::String;
use share_pool::{SafeFloat, SharePoolDataOperations};
use sp_core::crypto::Ss58Codec;
use sp_runtime::AccountId32;
use subtensor_runtime_common::NetUid;

pub(crate) const MIGRATION_NAME: &[u8] = b"migrate_reconcile_share_pools_v1";

/// Most `Alpha` + `AlphaV2` keys this one-shot may visit on one hotkey prefix. The production
/// target has ~60 members on one subnet. A larger prefix is left alone so the upgrade block
/// cannot grow with nominators added after the scan. Not a paged rewrite: oversized pools
/// are skipped.
pub(crate) const MAX_RECONCILE_PREFIX_VISITS: u64 = 2_048;
/// Most rows of the target `(hotkey, netuid)` pool this one-shot will sum.
pub(crate) const MAX_RECONCILE_POOL_ROWS: u64 = 1_024;

/// Pools whose live shares no longer sum to their denominator, so a member is quoted more
/// than its fraction of the pool value. Identified by a full scan of production state at a
/// pinned block; every other pool was within rounding of its denominator.
pub(crate) const RECONCILE_TARGETS: &[(&str, u16)] =
    &[("5DXdHixxtCvoa6GHKs2Jgrdzc61882Ftx1zN2sYFQuwgL1S1", 73)];

fn decode_account_id32<T: Config>(ss58_string: &str) -> Option<T::AccountId> {
    let account_id32: AccountId32 = AccountId32::from_ss58check(ss58_string).ok()?;
    let mut account_id32_slice: &[u8] = account_id32.as_ref();
    T::AccountId::decode(&mut account_id32_slice).ok()
}

/// Result of a bounded walk of one pool's live shares.
pub struct LiveShareSum {
    pub sum: SafeFloat,
    pub rows: u64,
    pub visits: u64,
    pub oversized: bool,
}

/// Sum of the live (current-epoch, non-zero) shares of every row of the pool.
///
/// Walks `Alpha` and `AlphaV2` prefixes for `hotkey` without collecting every subnet into a
/// map. Stops if the prefix visit cap or the per-pool row cap is exceeded; the caller then
/// skips the write.
pub fn live_share_sum<T: Config>(hotkey: &T::AccountId, netuid: NetUid) -> LiveShareSum {
    let ops = HotkeyAlphaSharePoolDataOperations::<T>::new(hotkey.clone(), netuid);
    let mut sum = SafeFloat::zero();
    let mut rows: u64 = 0;
    let mut visits: u64 = 0;
    let mut seen: BTreeSet<T::AccountId> = BTreeSet::new();

    // Legacy first so a (coldkey, netuid) present in both maps is counted once; `try_get_share`
    // still prefers the V1 row.
    if !accumulate_prefix::<T, _>(
        Alpha::<T>::iter_prefix((hotkey.clone(),))
            .map(|((coldkey, row_netuid), _)| (coldkey, row_netuid)),
        netuid,
        &ops,
        &mut seen,
        &mut sum,
        &mut rows,
        &mut visits,
    ) {
        return LiveShareSum {
            sum,
            rows,
            visits,
            oversized: true,
        };
    }
    if !accumulate_prefix::<T, _>(
        AlphaV2::<T>::iter_prefix((hotkey,))
            .map(|((coldkey, row_netuid), _)| (coldkey, row_netuid)),
        netuid,
        &ops,
        &mut seen,
        &mut sum,
        &mut rows,
        &mut visits,
    ) {
        return LiveShareSum {
            sum,
            rows,
            visits,
            oversized: true,
        };
    }

    LiveShareSum {
        sum,
        rows,
        visits,
        oversized: false,
    }
}

/// Visit one share-map prefix. Returns `false` when a cap is hit.
fn accumulate_prefix<T, I>(
    keys: I,
    netuid: NetUid,
    ops: &HotkeyAlphaSharePoolDataOperations<T>,
    seen: &mut BTreeSet<T::AccountId>,
    sum: &mut SafeFloat,
    rows: &mut u64,
    visits: &mut u64,
) -> bool
where
    T: Config,
    I: Iterator<Item = (T::AccountId, NetUid)>,
{
    for (coldkey, row_netuid) in keys {
        *visits = visits.saturating_add(1);
        if *visits > MAX_RECONCILE_PREFIX_VISITS {
            return false;
        }
        if row_netuid != netuid || !seen.insert(coldkey.clone()) {
            continue;
        }
        *rows = rows.saturating_add(1);
        if *rows > MAX_RECONCILE_POOL_ROWS {
            return false;
        }
        if let Ok(share) = ops.try_get_share(&coldkey)
            && !share.is_zero()
        {
            *sum = sum.add(&share).unwrap_or_else(|| sum.clone());
        }
    }
    true
}

/// Set the pool's denominator to the sum of its live shares when they differ. Pool value is
/// untouched, so every member ends up quoted exactly its fraction of the same value. Returns
/// the weight spent and whether a write happened.
pub fn reconcile_pool<T: Config>(hotkey: &T::AccountId, netuid: NetUid) -> (Weight, bool) {
    let scan = live_share_sum::<T>(hotkey, netuid);
    // One read per prefix key visited, plus share + epoch per matching row.
    let mut weight = T::DbWeight::get().reads(
        scan.visits
            .saturating_add(scan.rows.saturating_mul(2))
            .saturating_add(3),
    );
    if scan.oversized {
        log::warn!(
            "Migration '{}' skipped an oversized pool (visits={}, rows={})",
            String::from_utf8_lossy(MIGRATION_NAME),
            scan.visits,
            scan.rows
        );
        return (weight, false);
    }
    if scan.sum.is_zero() {
        // No live rows: nothing to reconcile against. Left for a product decision.
        return (weight, false);
    }
    let mut ops = HotkeyAlphaSharePoolDataOperations::<T>::new(hotkey.clone(), netuid);
    let denominator = ops.get_denominator();
    if !scan.sum.gt(&denominator) && !denominator.gt(&scan.sum) {
        return (weight, false);
    }
    ops.set_denominator(scan.sum);
    weight.saturating_accrue(T::DbWeight::get().writes(2));
    Pallet::<T>::deposit_event(Event::SharePoolDenominatorReconciled {
        hotkey: hotkey.clone(),
        netuid,
    });
    (weight, true)
}

/// One-shot: reconcile every target pool. Guarded by `HasMigrationRun`.
pub fn migrate_reconcile_share_pools<T: Config>() -> Weight {
    let mut weight = T::DbWeight::get().reads(1);
    if HasMigrationRun::<T>::get(MIGRATION_NAME) {
        return weight;
    }
    let mut reconciled: u32 = 0;
    for (ss58, netuid) in RECONCILE_TARGETS {
        let Some(hotkey) = decode_account_id32::<T>(ss58) else {
            log::warn!(
                "Migration '{}' skipped an undecodable target",
                String::from_utf8_lossy(MIGRATION_NAME)
            );
            continue;
        };
        let (spent, changed) = reconcile_pool::<T>(&hotkey, NetUid::from(*netuid));
        weight.saturating_accrue(spent);
        if changed {
            reconciled = reconciled.saturating_add(1);
        }
    }
    HasMigrationRun::<T>::insert(MIGRATION_NAME, true);
    weight.saturating_accrue(T::DbWeight::get().writes(1));
    log::info!(
        "Migration '{}' completed: {} pools reconciled",
        String::from_utf8_lossy(MIGRATION_NAME),
        reconciled
    );
    weight
}

/// `Σ shares` is within one part in 10^12 of the denominator (the rounding the share
/// arithmetic itself allows) for a pool with live rows.
#[cfg(any(feature = "try-runtime", test))]
pub fn pool_is_consistent<T: Config>(hotkey: &T::AccountId, netuid: NetUid) -> bool {
    let scan = live_share_sum::<T>(hotkey, netuid);
    if scan.oversized {
        return false;
    }
    let sum = scan.sum;
    if sum.is_zero() {
        return true;
    }
    let ops = HotkeyAlphaSharePoolDataOperations::<T>::new(hotkey.clone(), netuid);
    let denominator = ops.get_denominator();
    let one = SafeFloat::new(1, 0).unwrap_or_default();
    let trillion = SafeFloat::new(1, 12).unwrap_or_default();
    let tolerance = denominator
        .mul_div(&one, &trillion)
        .unwrap_or_else(SafeFloat::zero);
    let upper = denominator
        .add(&tolerance)
        .unwrap_or_else(|| denominator.clone());
    let lower = denominator.sub(&tolerance).unwrap_or_else(SafeFloat::zero);
    !sum.gt(&upper) && !lower.gt(&sum)
}

/// [`OnRuntimeUpgrade`](frame_support::traits::OnRuntimeUpgrade) wrapper with try-runtime
/// validation: every target pool ends with `Σ shares == D` (within rounding), its value and
/// row set are unchanged, and the marker is set.
pub mod reconcile_share_pools {
    use super::*;
    use frame_support::traits::OnRuntimeUpgrade;
    use sp_std::marker::PhantomData;

    #[cfg(feature = "try-runtime")]
    use crate::TotalHotkeyAlpha;
    #[cfg(feature = "try-runtime")]
    use codec::Encode;
    #[cfg(feature = "try-runtime")]
    use frame_support::ensure;
    #[cfg(feature = "try-runtime")]
    use sp_runtime::TryRuntimeError;
    #[cfg(feature = "try-runtime")]
    use sp_std::vec::Vec;
    #[cfg(feature = "try-runtime")]
    use subtensor_runtime_common::Token;

    #[cfg(feature = "try-runtime")]
    #[derive(Encode, Decode)]
    struct PreUpgradeState {
        already_run: bool,
        /// `(value, rows)` per target, in `RECONCILE_TARGETS` order.
        targets: Vec<(u64, u64)>,
    }

    pub struct Migration<T: Config>(PhantomData<T>);

    impl<T: Config> OnRuntimeUpgrade for Migration<T> {
        fn on_runtime_upgrade() -> Weight {
            migrate_reconcile_share_pools::<T>()
        }

        #[cfg(feature = "try-runtime")]
        fn pre_upgrade() -> Result<Vec<u8>, TryRuntimeError> {
            let mut targets = Vec::new();
            for (ss58, netuid) in RECONCILE_TARGETS {
                let hotkey = decode_account_id32::<T>(ss58).ok_or("target hotkey must decode")?;
                let netuid = NetUid::from(*netuid);
                let scan = live_share_sum::<T>(&hotkey, netuid);
                ensure!(
                    !scan.oversized,
                    "target pool exceeds the reconcile visit/row bound"
                );
                targets.push((
                    TotalHotkeyAlpha::<T>::get(&hotkey, netuid).to_u64(),
                    scan.rows,
                ));
            }
            Ok(PreUpgradeState {
                already_run: HasMigrationRun::<T>::get(MIGRATION_NAME.to_vec()),
                targets,
            }
            .encode())
        }

        #[cfg(feature = "try-runtime")]
        fn post_upgrade(state: Vec<u8>) -> Result<(), TryRuntimeError> {
            let before: PreUpgradeState =
                Decode::decode(&mut &state[..]).map_err(|_| "pre_upgrade state must decode")?;
            ensure!(
                HasMigrationRun::<T>::get(MIGRATION_NAME.to_vec()),
                "reconciliation marker must be set"
            );
            for ((ss58, netuid), (value_before, rows_before)) in
                RECONCILE_TARGETS.iter().zip(before.targets)
            {
                let hotkey = decode_account_id32::<T>(ss58).ok_or("target hotkey must decode")?;
                let netuid = NetUid::from(*netuid);
                let scan = live_share_sum::<T>(&hotkey, netuid);
                ensure!(
                    !scan.oversized,
                    "target pool exceeds the reconcile visit/row bound"
                );
                ensure!(
                    TotalHotkeyAlpha::<T>::get(&hotkey, netuid).to_u64() == value_before,
                    "reconciliation must not change pool value"
                );
                ensure!(
                    scan.rows == rows_before,
                    "reconciliation must not add or remove rows"
                );
                ensure!(
                    pool_is_consistent::<T>(&hotkey, netuid),
                    "target pool shares must sum to its denominator"
                );
            }
            let _ = before.already_run;
            Ok(())
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use crate::{tests::mock::*, *};
    use sp_core::U256;

    // A pool whose shares add up to more than its denominator quotes a member more than its
    // fraction; the reconciliation sets the denominator to the sum so quotes are exact again
    // and the pool value is untouched.
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
            assert!(!pool_is_consistent::<Test>(&hotkey, netuid));
            let value_before = TotalHotkeyAlpha::<Test>::get(hotkey, netuid);
            let alice_quote_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &alice, netuid,
            );

            let (_, changed) = reconcile_pool::<Test>(&hotkey, netuid);
            assert!(changed);

            let ops = HotkeyAlphaSharePoolDataOperations::<Test>::new(hotkey, netuid);
            let expected = inflated
                .add(&SafeFloat::new(1_000_000, 0).unwrap())
                .unwrap();
            assert!(!ops.get_denominator().gt(&expected) && !expected.gt(&ops.get_denominator()));
            assert_eq!(TotalHotkeyAlpha::<Test>::get(hotkey, netuid), value_before);
            assert!(pool_is_consistent::<Test>(&hotkey, netuid));
            // Quotes now split the value by the real share fractions: 1.5M / 2.5M of 2M.
            let alice_quote = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &alice, netuid,
            );
            let bob_quote =
                SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &bob, netuid);
            assert!(alice_quote < alice_quote_before);
            assert_eq!(alice_quote, 1_200_000u64.into());
            assert_eq!(bob_quote, 800_000u64.into());

            // Idempotent: a second pass finds nothing to change.
            let (_, changed_again) = reconcile_pool::<Test>(&hotkey, netuid);
            assert!(!changed_again);
        });
    }

    // Healthy pools and pools without live rows are left alone; the one-shot runs once.
    #[test]
    fn healthy_and_rowless_pools_are_left_alone() {
        new_test_ext(1).execute_with(|| {
            let netuid = NetUid::from(2);
            let hotkey = U256::from(1);
            add_network(netuid, 1, 0);
            SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &U256::from(11),
                netuid,
                1_000_000u64.into(),
            );
            let denominator = TotalHotkeySharesV2::<Test>::get(hotkey, netuid);
            let (_, changed) = reconcile_pool::<Test>(&hotkey, netuid);
            assert!(!changed);
            let now = TotalHotkeySharesV2::<Test>::get(hotkey, netuid);
            assert!(!now.gt(&denominator) && !denominator.gt(&now));

            // Value with a denominator but no rows: untouched.
            let stranded = U256::from(2);
            TotalHotkeyAlpha::<Test>::insert(stranded, netuid, AlphaBalance::from(5_000_000u64));
            TotalHotkeySharesV2::<Test>::insert(
                stranded,
                netuid,
                SafeFloat::new(5_000_000, 0).unwrap(),
            );
            let (_, changed) = reconcile_pool::<Test>(&stranded, netuid);
            assert!(!changed);
            assert_eq!(
                TotalHotkeyAlpha::<Test>::get(stranded, netuid),
                5_000_000u64.into()
            );

            assert!(!HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));
            migrate_reconcile_share_pools::<Test>();
            assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));
            let second = migrate_reconcile_share_pools::<Test>();
            assert_eq!(
                second,
                <Test as frame_system::Config>::DbWeight::get().reads(1)
            );
        });
    }
}
