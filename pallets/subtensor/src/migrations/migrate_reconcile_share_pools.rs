use crate::staking::stake_utils::HotkeyAlphaSharePoolDataOperations;
use crate::{Alpha, AlphaV2, Config, Event, HasMigrationRun, Pallet};
use alloc::collections::BTreeSet;
use codec::Decode;
#[cfg(feature = "try-runtime")]
use codec::Encode;
use frame_support::{traits::Get, weights::Weight};
use scale_info::prelude::string::String;
use share_pool::{SafeFloat, SharePoolDataOperations};
use sp_core::crypto::Ss58Codec;
use sp_runtime::AccountId32;
use subtensor_runtime_common::NetUid;

pub(crate) const MIGRATION_NAME: &[u8] = b"migrate_reconcile_share_pools_v1";

/// Most `Alpha` + `AlphaV2` keys this one-shot may visit on one hotkey prefix.
///
/// The walk is `(hotkey,)` across every subnet, not just the target netuid. Live mainnet
/// for the pinned target (`5DXdHix…L1S1`) is 2,376 `Alpha` + 10,273 `AlphaV2` = 12,649
/// prefix keys, and only ~58 rows on netuid 73. The old 2,048 visit cap aborted
/// `try-runtime` / skipped the write on that prefix. 65,536 is ~5× today's live
/// prefix (headroom for more nominators before the 459→464 upgrade) and still
/// bounded: RocksDbWeight at the cap is 65,536 × 25e6 ≈ 1.64e12, under the 4e12
/// block. A larger prefix is skipped, not scanned without a cap.
pub(crate) const MAX_RECONCILE_PREFIX_VISITS: u64 = 65_536;
/// Most rows of the target `(hotkey, netuid)` pool this one-shot will sum. Live
/// target has ~58 members; 1,024 is ~17× that. Not the failing cap on mainnet.
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
            // A failed add is not a partial sum we may write. Treat it as
            // oversized so the caller skips the write and does not stamp.
            match sum.add(&share) {
                Some(next) => *sum = next,
                None => return false,
            }
        }
    }
    true
}

/// Result of attempting to reconcile one target pool.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReconcileOutcome {
    /// Denominator was rewritten to the live share sum.
    Written,
    /// Pool was already consistent, or had no live rows.
    Unchanged,
    /// Prefix or row cap hit, or a share add did not fit. Do not stamp.
    Oversized,
    /// Target SS58 did not decode. Do not stamp.
    Undecodable,
}

/// Set the pool's denominator to the sum of its live shares when they differ. Pool value is
/// untouched, so every member ends up quoted exactly its fraction of the same value. Returns
/// the weight spent and whether a write happened.
pub fn reconcile_pool<T: Config>(
    hotkey: &T::AccountId,
    netuid: NetUid,
) -> (Weight, ReconcileOutcome) {
    let scan = live_share_sum::<T>(hotkey, netuid);
    // One read per prefix key visited, plus share + epoch per matching row.
    let mut weight = T::DbWeight::get().reads(
        scan.visits
            .saturating_add(scan.rows.saturating_mul(2))
            .saturating_add(3),
    );
    if scan.oversized {
        log::warn!(
            "Migration skipped an oversized pool (visits={}, rows={})",
            scan.visits,
            scan.rows
        );
        return (weight, ReconcileOutcome::Oversized);
    }
    if scan.sum.is_zero() {
        // No live rows: nothing to reconcile against. Left for a product decision.
        return (weight, ReconcileOutcome::Unchanged);
    }
    let mut ops = HotkeyAlphaSharePoolDataOperations::<T>::new(hotkey.clone(), netuid);
    let denominator = ops.get_denominator();
    if !scan.sum.gt(&denominator) && !denominator.gt(&scan.sum) {
        return (weight, ReconcileOutcome::Unchanged);
    }
    ops.set_denominator(scan.sum);
    weight.saturating_accrue(T::DbWeight::get().writes(2));
    Pallet::<T>::deposit_event(Event::SharePoolDenominatorReconciled {
        hotkey: hotkey.clone(),
        netuid,
    });
    (weight, ReconcileOutcome::Written)
}

/// Stamp only when every target decoded and was in-bound. An oversized or
/// undecodable target must stay retryable on a later spec.
fn should_stamp_reconcile(outcomes: &[ReconcileOutcome]) -> bool {
    !outcomes.is_empty()
        && outcomes.iter().all(|outcome| {
            matches!(
                outcome,
                ReconcileOutcome::Written | ReconcileOutcome::Unchanged
            )
        })
}

/// Retry after spec 464: same targets, same stamp rule. Needed because 464
/// stamped `migrate_reconcile_share_pools_v1` even when a target was skipped.
pub const MIGRATION_NAME_V2: &[u8] = b"migrate_reconcile_share_pools_v2";

/// One-shot for the 464 key. If v2 has not run, skip the prefix walk: v2 is
/// the 465 walker, so a 459→465 jump must not pay twice. Already-stamped v1
/// (464) stays a single read.
pub fn migrate_reconcile_share_pools<T: Config>() -> Weight {
    if HasMigrationRun::<T>::get(MIGRATION_NAME) {
        return T::DbWeight::get().reads(1);
    }
    if !HasMigrationRun::<T>::get(MIGRATION_NAME_V2) {
        return T::DbWeight::get().reads(2);
    }
    migrate_reconcile_share_pools_named::<T>(MIGRATION_NAME)
}

pub fn migrate_reconcile_share_pools_v2<T: Config>() -> Weight {
    migrate_reconcile_share_pools_named::<T>(MIGRATION_NAME_V2)
}

fn migrate_reconcile_share_pools_named<T: Config>(migration_name: &[u8]) -> Weight {
    let mut weight = T::DbWeight::get().reads(1);
    if HasMigrationRun::<T>::get(migration_name) {
        return weight;
    }
    let mut reconciled: u32 = 0;
    let mut outcomes: sp_std::vec::Vec<ReconcileOutcome> = sp_std::vec::Vec::new();
    for (ss58, netuid) in RECONCILE_TARGETS {
        let Some(hotkey) = decode_account_id32::<T>(ss58) else {
            log::warn!(
                "Migration '{}' skipped an undecodable target",
                String::from_utf8_lossy(migration_name)
            );
            outcomes.push(ReconcileOutcome::Undecodable);
            continue;
        };
        let (spent, outcome) = reconcile_pool::<T>(&hotkey, NetUid::from(*netuid));
        weight.saturating_accrue(spent);
        if outcome == ReconcileOutcome::Written {
            reconciled = reconciled.saturating_add(1);
        }
        outcomes.push(outcome);
    }
    if should_stamp_reconcile(&outcomes) {
        HasMigrationRun::<T>::insert(migration_name, true);
        weight.saturating_accrue(T::DbWeight::get().writes(1));
        log::info!(
            "Migration '{}' completed: {} pools reconciled",
            String::from_utf8_lossy(migration_name),
            reconciled
        );
    } else {
        log::warn!(
            "Migration '{}' did not stamp HasMigrationRun ({} pools written); retry later",
            String::from_utf8_lossy(migration_name),
            reconciled
        );
    }
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

/// Shared try-runtime state for v1 and v2. Stamp rule matches production:
/// skip (oversized / undecodable) → no stamp.
#[cfg(feature = "try-runtime")]
#[derive(Encode, Decode)]
struct ReconcileTargetSnap {
    value: u64,
    rows: u64,
    oversized: bool,
    decodable: bool,
}

#[cfg(feature = "try-runtime")]
#[derive(Encode, Decode)]
struct ReconcilePreUpgradeState {
    already_run: bool,
    /// v1 skipped the walk because v2 will run in this upgrade.
    deferred_to_v2: bool,
    targets: sp_std::vec::Vec<ReconcileTargetSnap>,
}

#[cfg(feature = "try-runtime")]
fn reconcile_pre_upgrade<T: Config>(
    migration_name: &[u8],
) -> Result<sp_std::vec::Vec<u8>, sp_runtime::TryRuntimeError> {
    use crate::TotalHotkeyAlpha;
    use codec::Encode;
    use subtensor_runtime_common::Token;

    let already_run = HasMigrationRun::<T>::get(migration_name.to_vec());
    let deferred_to_v2 = migration_name == MIGRATION_NAME
        && !already_run
        && !HasMigrationRun::<T>::get(MIGRATION_NAME_V2.to_vec());

    let mut targets = sp_std::vec::Vec::new();
    for (ss58, netuid) in RECONCILE_TARGETS {
        match decode_account_id32::<T>(ss58) {
            None => targets.push(ReconcileTargetSnap {
                value: 0,
                rows: 0,
                oversized: false,
                decodable: false,
            }),
            Some(hotkey) => {
                let netuid = NetUid::from(*netuid);
                let scan = live_share_sum::<T>(&hotkey, netuid);
                targets.push(ReconcileTargetSnap {
                    value: TotalHotkeyAlpha::<T>::get(&hotkey, netuid).to_u64(),
                    rows: scan.rows,
                    oversized: scan.oversized,
                    decodable: true,
                });
            }
        }
    }
    Ok(ReconcilePreUpgradeState {
        already_run,
        deferred_to_v2,
        targets,
    }
    .encode())
}

#[cfg(feature = "try-runtime")]
fn reconcile_post_upgrade<T: Config>(
    migration_name: &[u8],
    state: sp_std::vec::Vec<u8>,
) -> Result<(), sp_runtime::TryRuntimeError> {
    use crate::TotalHotkeyAlpha;
    use frame_support::ensure;
    use subtensor_runtime_common::Token;

    let before: ReconcilePreUpgradeState =
        Decode::decode(&mut &state[..]).map_err(|_| "pre_upgrade state must decode")?;
    let stamped = HasMigrationRun::<T>::get(migration_name.to_vec());
    let skip = before
        .targets
        .iter()
        .any(|snap| !snap.decodable || snap.oversized);

    if before.deferred_to_v2 {
        ensure!(!stamped, "v1 must not stamp when v2 is the walker");
    } else if before.already_run {
        ensure!(stamped, "already-run marker must stay set");
    } else if skip {
        ensure!(!stamped, "skip must not stamp");
    } else {
        ensure!(stamped, "in-bound reconcile must stamp");
    }

    for ((ss58, netuid), snap) in RECONCILE_TARGETS.iter().zip(before.targets.iter()) {
        if !snap.decodable {
            continue;
        }
        let hotkey = decode_account_id32::<T>(ss58).ok_or("target hotkey must decode")?;
        let netuid = NetUid::from(*netuid);
        let scan = live_share_sum::<T>(&hotkey, netuid);
        ensure!(
            TotalHotkeyAlpha::<T>::get(&hotkey, netuid).to_u64() == snap.value,
            "reconciliation must not change pool value"
        );
        ensure!(
            scan.rows == snap.rows,
            "reconciliation must not add or remove rows"
        );
        if !before.already_run && !before.deferred_to_v2 && !skip {
            ensure!(
                pool_is_consistent::<T>(&hotkey, netuid),
                "target pool shares must sum to its denominator"
            );
        }
    }
    Ok(())
}

/// [`OnRuntimeUpgrade`](frame_support::traits::OnRuntimeUpgrade) wrapper with try-runtime
/// validation: every target pool ends with `Σ shares == D` (within rounding), its value and
/// row set are unchanged, and the marker is set.
pub mod reconcile_share_pools {
    use super::*;
    use frame_support::traits::OnRuntimeUpgrade;
    use sp_std::marker::PhantomData;

    pub struct Migration<T: Config>(PhantomData<T>);

    impl<T: Config> OnRuntimeUpgrade for Migration<T> {
        fn on_runtime_upgrade() -> Weight {
            migrate_reconcile_share_pools::<T>()
        }

        #[cfg(feature = "try-runtime")]
        fn pre_upgrade() -> Result<sp_std::vec::Vec<u8>, sp_runtime::TryRuntimeError> {
            reconcile_pre_upgrade::<T>(MIGRATION_NAME)
        }

        #[cfg(feature = "try-runtime")]
        fn post_upgrade(state: sp_std::vec::Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
            reconcile_post_upgrade::<T>(MIGRATION_NAME, state)
        }
    }
}

/// Retry after spec 464: same targets and stamp rule, new `HasMigrationRun` key.
/// try-runtime hooks are the v1 checks against this marker.
pub mod reconcile_share_pools_v2 {
    use super::*;
    use frame_support::traits::OnRuntimeUpgrade;
    use sp_std::marker::PhantomData;

    pub struct Migration<T: Config>(PhantomData<T>);

    impl<T: Config> OnRuntimeUpgrade for Migration<T> {
        fn on_runtime_upgrade() -> Weight {
            migrate_reconcile_share_pools_v2::<T>()
        }

        #[cfg(feature = "try-runtime")]
        fn pre_upgrade() -> Result<sp_std::vec::Vec<u8>, sp_runtime::TryRuntimeError> {
            reconcile_pre_upgrade::<T>(MIGRATION_NAME_V2)
        }

        #[cfg(feature = "try-runtime")]
        fn post_upgrade(state: sp_std::vec::Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
            reconcile_post_upgrade::<T>(MIGRATION_NAME_V2, state)
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
            assert_eq!(changed, ReconcileOutcome::Written);

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
            assert_eq!(changed_again, ReconcileOutcome::Unchanged);
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
            assert_eq!(changed, ReconcileOutcome::Unchanged);
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
            assert_eq!(changed, ReconcileOutcome::Unchanged);
            assert_eq!(
                TotalHotkeyAlpha::<Test>::get(stranded, netuid),
                5_000_000u64.into()
            );

            assert!(!HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));
            assert!(!HasMigrationRun::<Test>::get(MIGRATION_NAME_V2.to_vec()));
            let deferred = migrate_reconcile_share_pools::<Test>();
            assert_eq!(
                deferred,
                <Test as frame_system::Config>::DbWeight::get().reads(2)
            );
            assert!(!HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));
            migrate_reconcile_share_pools_v2::<Test>();
            assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME_V2.to_vec()));
            let second = migrate_reconcile_share_pools_v2::<Test>();
            assert_eq!(
                second,
                <Test as frame_system::Config>::DbWeight::get().reads(1)
            );
        });
    }

    #[test]
    fn skip_outcomes_do_not_stamp() {
        assert!(should_stamp_reconcile(&[ReconcileOutcome::Written]));
        assert!(should_stamp_reconcile(&[ReconcileOutcome::Unchanged]));
        assert!(should_stamp_reconcile(&[
            ReconcileOutcome::Written,
            ReconcileOutcome::Unchanged
        ]));
        assert!(!should_stamp_reconcile(&[ReconcileOutcome::Oversized]));
        assert!(!should_stamp_reconcile(&[ReconcileOutcome::Undecodable]));
        assert!(!should_stamp_reconcile(&[
            ReconcileOutcome::Written,
            ReconcileOutcome::Undecodable
        ]));
        assert!(!should_stamp_reconcile(&[]));
    }

    fn target_hotkey_and_netuid() -> (U256, NetUid) {
        let (ss58, netuid) = RECONCILE_TARGETS[0];
        (
            decode_account_id32::<Test>(ss58).expect("pinned target decodes in tests"),
            NetUid::from(netuid),
        )
    }

    /// v1 already stamped must not block v2: a still-divergent target is repaired,
    /// v2 stamps, and a second run is a one-read no-op.
    #[test]
    fn v2_repairs_after_v1_stamp_and_is_idempotent() {
        new_test_ext(1).execute_with(|| {
            let (hotkey, netuid) = target_hotkey_and_netuid();
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
            AlphaV2::<Test>::insert(
                (hotkey, alice, netuid),
                SafeFloat::new(1_500_000, 0).unwrap(),
            );
            assert!(!pool_is_consistent::<Test>(&hotkey, netuid));
            let value_before = TotalHotkeyAlpha::<Test>::get(hotkey, netuid);

            HasMigrationRun::<Test>::insert(MIGRATION_NAME.to_vec(), true);
            assert!(!HasMigrationRun::<Test>::get(MIGRATION_NAME_V2.to_vec()));

            migrate_reconcile_share_pools_v2::<Test>();

            assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));
            assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME_V2.to_vec()));
            assert!(pool_is_consistent::<Test>(&hotkey, netuid));
            assert_eq!(TotalHotkeyAlpha::<Test>::get(hotkey, netuid), value_before);

            let second = migrate_reconcile_share_pools_v2::<Test>();
            assert_eq!(
                second,
                <Test as frame_system::Config>::DbWeight::get().reads(1)
            );
            assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME_V2.to_vec()));
        });
    }

    /// 459→465: v1 must not walk; v2 is the only walker.
    #[test]
    fn v1_defers_walk_when_v2_will_run() {
        new_test_ext(1).execute_with(|| {
            let (hotkey, netuid) = target_hotkey_and_netuid();
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
            AlphaV2::<Test>::insert(
                (hotkey, alice, netuid),
                SafeFloat::new(1_500_000, 0).unwrap(),
            );
            assert!(!pool_is_consistent::<Test>(&hotkey, netuid));

            migrate_reconcile_share_pools::<Test>();
            assert!(!HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));
            assert!(!pool_is_consistent::<Test>(&hotkey, netuid));

            migrate_reconcile_share_pools_v2::<Test>();
            assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME_V2.to_vec()));
            assert!(pool_is_consistent::<Test>(&hotkey, netuid));
        });
    }

    /// An oversized target must not stamp v2, so a later spec can retry.
    #[test]
    fn v2_skip_leaves_marker_clear() {
        new_test_ext(1).execute_with(|| {
            let (hotkey, netuid) = target_hotkey_and_netuid();
            add_network(netuid, 1, 0);
            for i in 0..=MAX_RECONCILE_POOL_ROWS {
                AlphaV2::<Test>::insert(
                    (hotkey, U256::from(10_000 + i), netuid),
                    SafeFloat::new(1, 0).unwrap(),
                );
            }
            migrate_reconcile_share_pools_v2::<Test>();
            assert!(!HasMigrationRun::<Test>::get(MIGRATION_NAME_V2.to_vec()));
        });
    }
}
