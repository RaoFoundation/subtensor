//! Finish lazy share conversion and make one dust sweep in metered idle-block batches.

use crate::weights::WeightInfo;
use crate::*;
use frame_support::traits::fungible::Inspect;
use frame_support::{
    storage::with_storage_layer, storage_alias, traits::OnRuntimeUpgrade, weights::Weight,
};
use share_pool::{SafeFloat, SharePoolDataOperations};
use sp_runtime::traits::AccountIdConversion;
use sp_std::marker::PhantomData;
use substrate_fixed::types::U64F64;
use subtensor_swap_interface::SwapHandler;

pub const MIGRATION_NAME: &[u8] = b"migrate_alpha_v2_and_unstake_dust_v1";
/// Inclusive threshold in TAO rao, valued when the row is processed.
pub const MAX_DUST_TAO: u64 = 3_000_000;
/// Positions below this value are deleted without a coldkey payout.
pub const MIN_PAYOUT_TAO: u64 = 500;

/// Retired storage, excluded from metadata. Transitional getters and lazy writes still
/// understand these keys until the bounded conversion finishes.
pub mod retired {
    use super::*;

    #[storage_alias]
    pub type Alpha<T: Config> = StorageNMap<
        Pallet<T>,
        (
            NMapKey<Blake2_128Concat, <T as frame_system::Config>::AccountId>,
            NMapKey<Blake2_128Concat, <T as frame_system::Config>::AccountId>,
            NMapKey<Identity, NetUid>,
        ),
        U64F64,
        ValueQuery,
    >;

    #[storage_alias]
    pub type TotalHotkeyShares<T: Config> = StorageDoubleMap<
        Pallet<T>,
        Blake2_128Concat,
        <T as frame_system::Config>::AccountId,
        Identity,
        NetUid,
        U64F64,
        ValueQuery,
    >;

    #[storage_alias]
    pub type AlphaMapLastKey<T: Config> = StorageValue<Pallet<T>, Option<Vec<u8>>, ValueQuery>;
}

/// Copy the legacy formats before invoking any V2-only runtime logic. Legacy takes
/// precedence on an overlapping key, exactly as the pre-upgrade share-pool getter did.
/// No epoch is stamped: doing so would revive a retired share.
#[cfg(test)]
fn convert<T: Config>() -> (Vec<(T::AccountId, T::AccountId, NetUid)>, Weight) {
    let mut weight = Weight::zero();
    for (hotkey, netuid, shares) in retired::TotalHotkeyShares::<T>::drain() {
        TotalHotkeySharesV2::<T>::insert(&hotkey, netuid, SafeFloat::from(shares));
        weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 2));
    }
    let mut legacy = Vec::new();
    for ((hotkey, coldkey, netuid), shares) in retired::Alpha::<T>::drain() {
        AlphaV2::<T>::insert((&hotkey, &coldkey, netuid), SafeFloat::from(shares));
        legacy.push((hotkey, coldkey, netuid));
        weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 2));
    }
    retired::AlphaMapLastKey::<T>::kill();
    weight.saturating_accrue(T::DbWeight::get().reads_writes(2, 1));
    (legacy, weight)
}

/// Historical regression fixtures cross the format-conversion boundary before exercising
/// normal staking. Dust policy itself is tested through the complete migration below.
#[cfg(test)]
pub(crate) fn convert_for_test<T: Config>() {
    let _ = convert::<T>();
}

/// Remove a row and its live denominator contribution without reviving retired
/// shares. The caller must first settle any positive value being removed.
fn remove_share<T: Config>(hotkey: &T::AccountId, coldkey: &T::AccountId, netuid: NetUid) {
    use crate::staking::stake_utils::HotkeyAlphaSharePoolDataOperations;
    let mut ops = HotkeyAlphaSharePoolDataOperations::<T>::new(hotkey.clone(), netuid);
    if !Pallet::<T>::alpha_share_is_retired(hotkey, coldkey, netuid) {
        let share = ops.get_share(coldkey);
        if !share.is_zero() {
            let denominator = ops.get_denominator();
            ops.set_denominator(denominator.sub(&share).unwrap_or_default());
        }
    }
    AlphaV2::<T>::remove((hotkey, coldkey, netuid));
    AlphaShareEpoch::<T>::remove((hotkey, coldkey, netuid));
    Pallet::<T>::maybe_remove_staking_hotkey(hotkey, coldkey);
}

/// Settle a deleted dust position at its current spot valuation. There is no
/// AMM swap: sub-rao output cannot be swapped, and the policy burns the captured
/// whole-rao spot value rather than slippage-dependent execution proceeds.
/// Return the position's alpha to the protocol reserve. The caller settles all
/// corresponding TAO together, in the same storage transaction.
fn delete_dust<T: Config>(hotkey: &T::AccountId, coldkey: &T::AccountId, netuid: NetUid) {
    use crate::staking::stake_utils::HotkeyAlphaSharePoolDataOperations;
    let alpha = Pallet::<T>::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, netuid);
    // Debit the whole quoted position and then remove its complete share. Going
    // through the pool data operations also maintains TotalAlphaStaked.
    let mut ops = HotkeyAlphaSharePoolDataOperations::<T>::new(hotkey.clone(), netuid);
    if !alpha.is_zero() {
        ops.set_shared_value(ops.get_shared_value().saturating_sub(alpha.to_u64()));
        SubnetAlphaOut::<T>::mutate(netuid, |total| *total = total.saturating_sub(alpha));
        Pallet::<T>::increase_provided_alpha_reserve(netuid, alpha);
    }
    if netuid.is_root() && !alpha.is_zero() {
        Pallet::<T>::remove_stake_adjust_root_claimed_for_hotkey_and_coldkey(
            hotkey, coldkey, alpha,
        );
    }
    remove_share::<T>(hotkey, coldkey, netuid);
    if netuid.is_root() && !Pallet::<T>::coldkey_has_root_stake(coldkey) {
        Pallet::<T>::maybe_remove_coldkey_index(coldkey);
    }
    Pallet::<T>::cleanup_lock_if_zero(coldkey, netuid);
    Pallet::<T>::queue_childkey_threshold_check(hotkey);
}

/// A cursor is saved only after a row's accounting and deletion commit together.
/// phase: 0 = legacy positions, 1 = leftover denominators, 2 = V2 dust sweep.
#[derive(Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, Default)]
pub struct Progress {
    pub phase: u8,
    pub after: Option<Vec<u8>>,
    pub legacy: u64,
    pub denominators: u64,
    pub scanned: u64,
    pub deleted: u64,
    pub refunded: u64,
    pub burned: u64,
    pub pending_burn: u64,
    /// Number of V2 records deleted in the single sweep.
    pub pass_deleted: u64,
    /// Zero until the V2 cursor is exhausted, then one even while a burn is pending.
    pub passes: u64,
    pub deferred: u64,
}

#[storage_alias]
pub type AlphaV2Migration<T: Config> = StorageValue<Pallet<T>, Progress, OptionQuery>;

pub fn in_progress<T: Config>() -> bool {
    AlphaV2Migration::<T>::get().is_some_and(|p| p.phase != 3)
}

/// Scheduling is constant work, including when the upgrade is replayed.
pub fn migrate<T: Config>() -> Weight {
    let weight = T::DbWeight::get().reads(2);
    if HasMigrationRun::<T>::get(MIGRATION_NAME) || in_progress::<T>() {
        return weight;
    }
    AlphaV2Migration::<T>::put(Progress::default());
    log::info!(target: "runtime", "AlphaV2 migration scheduled");
    weight.saturating_add(T::DbWeight::get().writes(1))
}

/// Root's existing protocol account holds forfeited TAO until a burn transfer can
/// meet ED. Its reserve ledger excludes the pending amount, so it is not stake
/// backing and cannot be withdrawn by stakers. Root cannot be dissolved.
fn collect_burn<T: Config>(netuid: NetUid, amount: u64) -> DispatchResult {
    if amount == 0 {
        return Ok(());
    }
    let tao = TaoBalance::from(amount);
    ensure!(
        SubnetTAO::<T>::get(netuid) >= tao,
        Error::<T>::InsufficientTaoBalance
    );
    let collector =
        Pallet::<T>::get_subnet_account_id(NetUid::ROOT).ok_or(Error::<T>::SubnetNotExists)?;
    let source = Pallet::<T>::get_subnet_account_id(netuid).ok_or(Error::<T>::SubnetNotExists)?;
    if source != collector {
        Pallet::<T>::transfer_tao(&source, &collector, tao)?;
    }
    Pallet::<T>::decrease_provided_tao_reserve(netuid, tao);
    Pallet::<T>::record_tao_outflow(netuid, tao);
    TotalStake::<T>::mutate(|stake| *stake = stake.saturating_sub(tao));
    Ok(())
}

fn flush_burn<T: Config>(progress: &mut Progress) -> DispatchResult {
    if progress.pending_burn == 0 {
        return Ok(());
    }
    let burn: T::AccountId = T::BurnAccountId::get().into_account_truncating();
    let amount = TaoBalance::from(progress.pending_burn);
    if Pallet::<T>::get_coldkey_balance(&burn).is_zero()
        && amount < <T as Config>::Currency::minimum_balance().max(MIN_PAYOUT_TAO.into())
    {
        return Ok(());
    }
    let collector =
        Pallet::<T>::get_subnet_account_id(NetUid::ROOT).ok_or(Error::<T>::SubnetNotExists)?;
    with_storage_layer(|| Pallet::<T>::burn_tao(&collector, amount))?;
    progress.burned = progress.burned.saturating_add(progress.pending_burn);
    progress.pending_burn = 0;
    Ok(())
}

fn convert_row<T: Config>(hotkey: &T::AccountId, coldkey: &T::AccountId, netuid: NetUid) {
    if retired::TotalHotkeyShares::<T>::contains_key(hotkey, netuid) {
        let shares = retired::TotalHotkeyShares::<T>::take(hotkey, netuid);
        TotalHotkeySharesV2::<T>::insert(hotkey, netuid, SafeFloat::from(shares));
    }
    let shares = retired::Alpha::<T>::take((hotkey, coldkey, netuid));
    AlphaV2::<T>::insert((hotkey, coldkey, netuid), SafeFloat::from(shares));
    // Do not stamp AlphaShareEpoch: doing so would revive retired positions.
    if netuid.is_root()
        && !Pallet::<T>::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, netuid)
            .is_zero()
    {
        Pallet::<T>::maybe_add_coldkey_index(coldkey);
    }
}

/// Process only work which fits the remaining block weight. A failed transfer or
/// protected withdrawal leaves the row and cursor unchanged; it cannot produce a
/// false completion marker. Normal staking can still update a queued position.
pub fn continue_migration<T: Config>(limit: Weight) -> Weight {
    // Progress read/write, completion marker/cursor cleanup and a burn transfer.
    let overhead = T::DbWeight::get().reads_writes(12, 10);
    if !overhead.all_lte(limit) {
        return Weight::zero();
    }
    let Some(mut progress) = AlphaV2Migration::<T>::get() else {
        return T::DbWeight::get().reads(1);
    };
    if progress.phase == 3 {
        return T::DbWeight::get().reads(1);
    }
    let mut used = overhead;
    let mut finished = false;
    // Hard cap also bounds iterations independently of the configured DB weights.
    for _ in 0..10_000 {
        if progress.phase == 2 && progress.passes != 0 {
            // A pending burn may need another block, but must not restart the sweep.
            finished = true;
            break;
        }
        // Reserve classification reads before loading the row or its current value.
        let inspect = T::DbWeight::get().reads(20);
        if !used.saturating_add(inspect).all_lte(limit) {
            break;
        }
        used.saturating_accrue(inspect);
        if progress.phase == 1 {
            let cost = T::DbWeight::get().writes(2);
            if !used.saturating_add(cost).all_lte(limit) {
                break;
            }
            let next = retired::TotalHotkeyShares::<T>::iter().next();
            if let Some((hotkey, netuid, shares)) = next {
                retired::TotalHotkeyShares::<T>::remove(&hotkey, netuid);
                TotalHotkeySharesV2::<T>::insert(&hotkey, netuid, SafeFloat::from(shares));
                progress.denominators = progress.denominators.saturating_add(1);
                used.saturating_accrue(cost);
            } else {
                progress.phase = 2;
            }
            continue;
        }
        let legacy = progress.phase == 0;
        let next = if legacy {
            let mut iter = match progress.after.as_ref() {
                Some(key) => retired::Alpha::<T>::iter_from(key.clone()),
                None => retired::Alpha::<T>::iter(),
            };
            iter.next().map(|(key, _)| key)
        } else {
            let mut iter = match progress.after.as_ref() {
                Some(key) => AlphaV2::<T>::iter_from(key.clone()),
                None => AlphaV2::<T>::iter(),
            };
            iter.next().map(|(key, _)| key)
        };
        let Some((hotkey, coldkey, netuid)) = next else {
            if legacy {
                progress.after = None;
                if retired::Alpha::<T>::iter_keys().next().is_some() {
                    break; // Retry failed rows next block, without starving other rows.
                }
                progress.phase = 1;
            } else {
                progress.passes = progress.passes.saturating_add(1);
                // Emissions and valuation changes behind the cursor may leave dust.
                // Completion requires one full sweep, not a globally dust-free V2 map.
                finished = true;
                break;
            }
            continue;
        };
        let alpha =
            Pallet::<T>::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);
        let value = U64F64::from_num(alpha.to_u64())
            .saturating_mul(T::SwapInterface::current_alpha_price(netuid));
        let dust = value < U64F64::from_num(MIN_PAYOUT_TAO);
        let payout = legacy && !dust && value <= U64F64::from_num(MAX_DUST_TAO);
        let mut cost = if legacy {
            T::DbWeight::get().reads_writes(8, 8)
        } else {
            Weight::zero()
        };
        let mut flush_allowance = Weight::zero();
        if dust || payout {
            cost.saturating_accrue(<T as Config>::WeightInfo::remove_stake());
            cost.saturating_accrue(Pallet::<T>::staking_hotkeys_walk_actual(&coldkey));
            if netuid.is_root()
                && payout
                && PendingBasketDeposits::<T>::iter_key_prefix(&hotkey)
                    .next()
                    .is_some()
            {
                flush_allowance = Pallet::<T>::basket_flush_weight_bound();
                cost.saturating_accrue(flush_allowance);
            }
        }
        if !used.saturating_add(cost).all_lte(limit) {
            break;
        }
        used.saturating_accrue(cost);
        let result: Result<(u64, u64, Weight), DispatchError> = with_storage_layer(|| {
            if legacy {
                convert_row::<T>(&hotkey, &coldkey, netuid);
            }
            if dust {
                let burn = value.to_num::<u64>();
                collect_burn::<T>(netuid, burn)?;
                delete_dust::<T>(&hotkey, &coldkey, netuid);
                Ok((burn, 0, Weight::zero()))
            } else if payout {
                ensure!(
                    coldkey != Pallet::<T>::get_beta_escrow_account_id(),
                    Error::<T>::NotEnoughStakeToWithdraw
                );
                let (tao, flush_work) = Pallet::<T>::unstake_from_subnet_with_flush_work(
                    &hotkey,
                    &coldkey,
                    &coldkey,
                    netuid,
                    alpha,
                    T::SwapInterface::min_price(),
                    true,
                    true,
                )?;
                ensure!(
                    Pallet::<T>::get_stake_for_hotkey_and_coldkey_on_subnet(
                        &hotkey, &coldkey, netuid
                    )
                    .is_zero(),
                    Error::<T>::NotEnoughStakeToWithdraw
                );
                Pallet::<T>::queue_childkey_threshold_check(&hotkey);
                Ok((
                    0,
                    tao.to_u64(),
                    Pallet::<T>::basket_flush_weight(flush_work),
                ))
            } else {
                Ok((0, 0, Weight::zero()))
            }
        });
        match result {
            Ok((burn, refund, flush_weight)) => {
                used = used.saturating_sub(flush_allowance.saturating_sub(flush_weight));
                progress.pending_burn = progress.pending_burn.saturating_add(burn);
                progress.refunded = progress.refunded.saturating_add(refund);
                if legacy {
                    progress.legacy = progress.legacy.saturating_add(1);
                    progress.after = Some(retired::Alpha::<T>::hashed_key_for((
                        &hotkey, &coldkey, netuid,
                    )));
                } else {
                    progress.scanned = progress.scanned.saturating_add(1);
                    progress.after =
                        Some(AlphaV2::<T>::hashed_key_for((&hotkey, &coldkey, netuid)));
                }
                if dust {
                    progress.deleted = progress.deleted.saturating_add(1);
                    if !legacy {
                        progress.pass_deleted = progress.pass_deleted.saturating_add(1);
                    }
                }
            }
            Err(error) => {
                log::error!(target: "runtime", "AlphaV2 migration row deferred: {hotkey:?}/{coldkey:?}/{netuid:?}: {error:?}");
                progress.deferred = progress.deferred.saturating_add(1);
                if legacy {
                    progress.after = Some(retired::Alpha::<T>::hashed_key_for((
                        &hotkey, &coldkey, netuid,
                    )));
                } else {
                    break;
                }
            }
        }
    }
    if let Err(error) = flush_burn::<T>(&mut progress) {
        log::error!(target: "runtime", "AlphaV2 migration burn deferred: {error:?}");
    }
    if finished && progress.pending_burn == 0 {
        retired::AlphaMapLastKey::<T>::kill();
        HasMigrationRun::<T>::insert(MIGRATION_NAME, true);
        // Keep final counters for independent chain audits. The completion marker
        // gates subsequent idle calls, and phase 3 makes completion observable.
        progress.phase = 3;
        log::info!(target: "runtime", "AlphaV2 migration complete: {progress:?}");
    }
    AlphaV2Migration::<T>::put(progress);
    used
}

pub struct Migration<T>(PhantomData<T>);

impl<T: Config> OnRuntimeUpgrade for Migration<T> {
    fn on_runtime_upgrade() -> Weight {
        migrate::<T>()
    }

    #[cfg(feature = "try-runtime")]
    fn pre_upgrade() -> Result<Vec<u8>, sp_runtime::TryRuntimeError> {
        // Issuance must not change: refunds and burn_tao both transfer existing TAO
        // out of subnet accounts. Burning here is not issuance-reducing recycling.
        Ok(<T as Config>::Currency::total_issuance().encode())
    }

    #[cfg(feature = "try-runtime")]
    fn post_upgrade(state: Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
        let issuance = TaoBalance::decode(&mut &state[..])
            .map_err(|_| "invalid AlphaV2 migration pre-upgrade state")?;
        ensure!(
            HasMigrationRun::<T>::get(MIGRATION_NAME) || in_progress::<T>(),
            "AlphaV2 migration was not scheduled"
        );
        ensure!(
            <T as Config>::Currency::total_issuance() == issuance,
            "AlphaV2 migration changed issuance"
        );
        Ok(())
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "fixtures require a successfully created subnet"
)]
mod tests {
    use super::*;
    use crate::tests::mock::*;
    use frame_support::assert_ok;
    use sp_core::U256;

    fn run_batches() {
        migrate::<Test>();
        for _ in 0..20 {
            if !in_progress::<Test>() {
                break;
            }
            continue_migration::<Test>(Weight::from_parts(4_000_000_000_000, u64::MAX));
        }
    }

    fn network() -> NetUid {
        let root = SubtensorModule::get_subnet_account_id(NetUid::ROOT).expect("root account");
        add_balance_to_coldkey_account(&root, 1_000_000_000_000u64.into());
        let netuid = add_dynamic_network(&U256::from(1001), &U256::from(1002));
        setup_reserves(
            netuid,
            1_000_000_000_000u64.into(),
            1_000_000_000_000u64.into(),
        );
        let account = SubtensorModule::get_subnet_account_id(netuid).expect("test subnet account");
        add_balance_to_coldkey_account(&account, 1_000_000_000_000u64.into());
        netuid
    }

    fn legacy_position(hot: U256, cold: U256, netuid: NetUid, alpha: u64) {
        assert_ok!(SubtensorModule::create_account_if_non_existent(&cold, &hot));
        add_balance_to_coldkey_account(&cold, 1_000_000_000u64.into());
        StakingHotkeys::<Test>::insert(cold, vec![hot]);
        retired::Alpha::<Test>::insert((hot, cold, netuid), U64F64::from_num(alpha));
        retired::TotalHotkeyShares::<Test>::insert(hot, netuid, U64F64::from_num(alpha));
        TotalHotkeyAlpha::<Test>::insert(hot, netuid, AlphaBalance::from(alpha));
        SubnetAlphaOut::<Test>::mutate(netuid, |v| *v = v.saturating_add(alpha.into()));
    }

    #[test]
    fn inclusive_threshold_refunds_and_converts_in_batches() {
        new_test_ext(1).execute_with(|| {
            let netuid = network();
            let cold = U256::from(2);
            let small = U256::from(3);
            let large = U256::from(4);
            legacy_position(small, cold, netuid, MAX_DUST_TAO);
            legacy_position(large, U256::from(5), netuid, MAX_DUST_TAO * 2);
            let before = SubtensorModule::get_coldkey_balance(&cold);
            let issuance = <Test as Config>::Currency::total_issuance();
            run_batches();
            assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME));
            assert!(retired::Alpha::<Test>::iter().next().is_none());
            assert!(retired::TotalHotkeyShares::<Test>::iter().next().is_none());
            assert!(!AlphaV2::<Test>::contains_key((small, cold, netuid)));
            assert!(SubtensorModule::get_coldkey_balance(&cold) > before);
            assert_eq!(
                SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &large,
                    &U256::from(5),
                    netuid
                ),
                (MAX_DUST_TAO * 2).into()
            );
            assert_eq!(<Test as Config>::Currency::total_issuance(), issuance);
            let root = sp_io::storage::root(sp_runtime::StateVersion::V1);
            run_batches();
            assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), root);
        });
    }

    #[test]
    fn candidate_selection_uses_current_spot_price_without_rounding_tao() {
        for alpha in [2_000_000u64, 2_000_001] {
            new_test_ext(1).execute_with(|| {
                let netuid = network();
                setup_reserves(
                    netuid,
                    1_500_000_000_000u64.into(),
                    1_000_000_000_000u64.into(),
                );
                let hot = U256::from(2);
                let cold = U256::from(3);
                legacy_position(hot, cold, netuid, alpha);
                run_batches();
                assert_eq!(
                    AlphaV2::<Test>::contains_key((hot, cold, netuid)),
                    alpha > 2_000_000
                );
                assert_eq!(
                    SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                        &hot, &cold, netuid
                    ),
                    if alpha > 2_000_000 {
                        alpha.into()
                    } else {
                        AlphaBalance::ZERO
                    }
                );
                assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME));
            });
        }
    }

    #[test]
    fn overlapping_legacy_rows_preserve_old_getter_precedence() {
        new_test_ext(1).execute_with(|| {
            let netuid = network();
            let hot = U256::from(2);
            let cold = U256::from(3);
            let alpha = MAX_DUST_TAO + 1;
            legacy_position(hot, cold, netuid, alpha);
            AlphaV2::<Test>::insert((hot, cold, netuid), SafeFloat::from(1u64));
            TotalHotkeySharesV2::<Test>::insert(hot, netuid, SafeFloat::from(2u64));
            run_batches();
            assert_eq!(
                AlphaV2::<Test>::get((hot, cold, netuid)),
                SafeFloat::from(alpha)
            );
            assert_eq!(
                TotalHotkeySharesV2::<Test>::get(hot, netuid),
                SafeFloat::from(alpha)
            );
            assert_eq!(
                SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hot, &cold, netuid),
                alpha.into()
            );
        });
    }

    #[test]
    fn fractional_empty_share_is_removed_from_live_denominator() {
        new_test_ext(1).execute_with(|| {
            let netuid = network();
            let hot = U256::from(2);
            let cold = U256::from(3);
            legacy_position(hot, cold, netuid, 1);
            retired::Alpha::<Test>::insert((hot, cold, netuid), U64F64::from_num(0.000001));
            TotalHotkeyAlpha::<Test>::insert(hot, netuid, AlphaBalance::from(1000u64));
            AlphaV2::<Test>::insert(
                (hot, U256::from(4), netuid),
                SafeFloat::from(U64F64::from_num(0.999999)),
            );
            run_batches();
            assert!(!AlphaV2::<Test>::contains_key((hot, cold, netuid)));
            assert_eq!(
                TotalHotkeySharesV2::<Test>::get(hot, netuid),
                SafeFloat::from(1u64)
                    .sub(&SafeFloat::from(U64F64::from_num(0.000001)))
                    .expect("positive denominator")
            );
            assert_eq!(TotalHotkeyAlpha::<Test>::get(hot, netuid), 1000u64.into());
        });
    }

    #[test]
    fn existing_v2_above_deletion_threshold_is_preserved() {
        new_test_ext(1).execute_with(|| {
            let netuid = network();
            let hot = U256::from(2);
            let cold = U256::from(3);
            legacy_position(hot, cold, netuid, 1000);
            convert_for_test::<Test>();
            let share = AlphaV2::<Test>::get((hot, cold, netuid));
            run_batches();
            assert_eq!(AlphaV2::<Test>::get((hot, cold, netuid)), share);
        });
    }

    #[test]
    fn sub_500_rao_positions_are_deleted_and_whole_rao_are_burned() {
        use sp_runtime::traits::AccountIdConversion;
        new_test_ext(1).execute_with(|| {
            let netuid = network();
            // Stable pricing isolates the payout boundary from earlier reserve changes.
            SubnetMechanism::<Test>::insert(netuid, 0);
            let burn: U256 = <Test as Config>::BurnAccountId::get().into_account_truncating();
            assert_eq!(
                SubtensorModule::get_coldkey_balance(&burn),
                TaoBalance::ZERO
            );
            for (i, alpha) in [0, 1, 499, 499, 500].into_iter().enumerate() {
                legacy_position(U256::from(i + 10), U256::from(i + 20), netuid, alpha);
            }
            // A missing destination account must not prevent deletion and burning.
            use frame_support::traits::fungible::Mutate;
            let _ = <Test as Config>::Currency::set_balance(&U256::from(21), TaoBalance::ZERO);
            let burn_before = SubtensorModule::get_coldkey_balance(&burn);
            let issuance = <Test as Config>::Currency::total_issuance();
            let tracked_issuance = TotalIssuance::<Test>::get();
            run_batches();
            for i in 0..5 {
                assert!(!AlphaV2::<Test>::contains_key((
                    U256::from(i + 10),
                    U256::from(i + 20),
                    netuid
                )));
                assert!(TotalHotkeySharesV2::<Test>::get(U256::from(i + 10), netuid).is_zero());
                assert_eq!(
                    TotalHotkeyAlpha::<Test>::get(U256::from(i + 10), netuid),
                    AlphaBalance::ZERO
                );
            }
            for i in 0..4 {
                let expected = if i == 1 { 0 } else { 1_000_000_000 };
                assert_eq!(
                    SubtensorModule::get_coldkey_balance(&U256::from(i + 20)),
                    expected.into()
                );
            }
            // 0 + 1 + 499 + 499, no coldkey payout.
            assert_eq!(
                SubtensorModule::get_coldkey_balance(&burn),
                burn_before + TaoBalance::from(999u64)
            );
            // Exactly 500 is paid normally, not deleted by the burn branch.
            assert!(
                SubtensorModule::get_coldkey_balance(&U256::from(24)) > 1_000_000_000u64.into()
            );
            assert_eq!(<Test as Config>::Currency::total_issuance(), issuance);
            assert_eq!(TotalIssuance::<Test>::get(), tracked_issuance);
            let root = sp_io::storage::root(sp_runtime::StateVersion::V1);
            run_batches();
            assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), root);
        });
    }

    #[test]
    fn dust_deletion_keeps_other_pool_members_and_reserve_accounting_intact() {
        use sp_runtime::traits::AccountIdConversion;
        new_test_ext(1).execute_with(|| {
            let netuid = network();
            let hot = U256::from(2);
            let cold = U256::from(3);
            let other = U256::from(4);
            legacy_position(hot, cold, netuid, 499);
            AlphaV2::<Test>::insert((hot, other, netuid), SafeFloat::from(10_000u64));
            retired::TotalHotkeyShares::<Test>::insert(hot, netuid, U64F64::from_num(10_499));
            TotalHotkeyAlpha::<Test>::insert(hot, netuid, AlphaBalance::from(10_499u64));
            TotalAlphaStaked::<Test>::insert(netuid, AlphaBalance::from(10_499u64));
            let burn: U256 = <Test as Config>::BurnAccountId::get().into_account_truncating();
            add_balance_to_coldkey_account(&burn, 500u64.into());
            TotalStake::<Test>::put(TaoBalance::from(1_000_000_000_000u64));
            let account = SubtensorModule::get_subnet_account_id(netuid).expect("subnet account");
            let balance = SubtensorModule::get_coldkey_balance(&account);
            let alpha_in = SubnetAlphaIn::<Test>::get(netuid);
            run_batches();
            assert!(!AlphaV2::<Test>::contains_key((hot, cold, netuid)));
            assert_eq!(
                SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hot, &other, netuid),
                10_000u64.into()
            );
            assert_eq!(
                TotalHotkeySharesV2::<Test>::get(hot, netuid),
                SafeFloat::from(10_000u64)
            );
            assert_eq!(TotalAlphaStaked::<Test>::get(netuid), 10_000u64.into());
            assert_eq!(SubnetAlphaOut::<Test>::get(netuid), AlphaBalance::ZERO);
            assert_eq!(
                SubnetAlphaIn::<Test>::get(netuid),
                alpha_in + AlphaBalance::from(499u64)
            );
            assert_eq!(
                SubnetTAO::<Test>::get(netuid),
                TaoBalance::from(1_000_000_000_000u64 - 499)
            );
            assert_eq!(TotalStake::<Test>::get(), SubnetTAO::<Test>::get(netuid));
            assert_eq!(
                SubtensorModule::get_coldkey_balance(&account),
                balance - TaoBalance::from(499u64)
            );
        });
    }

    #[test]
    fn dust_burns_are_aggregated_across_subnets_before_creating_burn_account() {
        use sp_runtime::traits::AccountIdConversion;
        new_test_ext(1).execute_with(|| {
            let a = network();
            let b = add_dynamic_network(&U256::from(2001), &U256::from(2002));
            setup_reserves(b, 1_000_000_000_000u64.into(), 1_000_000_000_000u64.into());
            let account_a = SubtensorModule::get_subnet_account_id(a).expect("subnet a");
            let account_b = SubtensorModule::get_subnet_account_id(b).expect("subnet b");
            add_balance_to_coldkey_account(&account_b, 1_000_000_000_000u64.into());
            legacy_position(U256::from(2), U256::from(3), a, 249);
            legacy_position(U256::from(4), U256::from(5), b, 251);
            let burn: U256 = <Test as Config>::BurnAccountId::get().into_account_truncating();
            assert_eq!(
                SubtensorModule::get_coldkey_balance(&burn),
                TaoBalance::ZERO
            );
            let before_a = SubtensorModule::get_coldkey_balance(&account_a);
            let before_b = SubtensorModule::get_coldkey_balance(&account_b);
            let issuance = <Test as Config>::Currency::total_issuance();
            run_batches();
            assert!(!AlphaV2::<Test>::contains_key((
                U256::from(2),
                U256::from(3),
                a
            )));
            assert!(!AlphaV2::<Test>::contains_key((
                U256::from(4),
                U256::from(5),
                b
            )));
            assert_eq!(
                SubtensorModule::get_coldkey_balance(&account_a),
                before_a - TaoBalance::from(249u64)
            );
            assert_eq!(
                SubtensorModule::get_coldkey_balance(&account_b),
                before_b - TaoBalance::from(251u64)
            );
            assert_eq!(SubtensorModule::get_coldkey_balance(&burn), 500u64.into());
            let transfers: Vec<_> = System::events()
                .into_iter()
                .filter_map(|record| {
                    if let RuntimeEvent::Balances(pallet_balances::Event::Transfer {
                        to,
                        amount,
                        ..
                    }) = record.event
                        && to == burn
                    {
                        Some(amount)
                    } else {
                        None
                    }
                })
                .collect();
            // No individual sub-ED transfer is sent to the initially empty account.
            assert_eq!(transfers, vec![TaoBalance::from(500u64)]);
            assert_eq!(<Test as Config>::Currency::total_issuance(), issuance);
        });
    }

    #[test]
    fn failed_payout_rolls_back_and_prevents_false_completion() {
        new_test_ext(1).execute_with(|| {
            let netuid = network();
            let hot = U256::from(2);
            let cold = U256::from(3);
            legacy_position(hot, cold, netuid, MAX_DUST_TAO);
            let subnet_account =
                SubtensorModule::get_subnet_account_id(netuid).expect("test subnet account");
            use frame_support::traits::fungible::Mutate;
            let _ = <Test as Config>::Currency::set_balance(&subnet_account, TaoBalance::ZERO);
            let reserves = (
                SubnetTAO::<Test>::get(netuid),
                SubnetAlphaIn::<Test>::get(netuid),
            );
            let balance = SubtensorModule::get_coldkey_balance(&cold);
            run_batches();
            assert_eq!(
                SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hot, &cold, netuid),
                MAX_DUST_TAO.into()
            );
            assert_eq!(
                (
                    SubnetTAO::<Test>::get(netuid),
                    SubnetAlphaIn::<Test>::get(netuid)
                ),
                reserves
            );
            assert_eq!(SubtensorModule::get_coldkey_balance(&cold), balance);
            assert!(retired::Alpha::<Test>::contains_key((hot, cold, netuid)));
            assert!(!HasMigrationRun::<Test>::get(MIGRATION_NAME));
        });
    }

    #[test]
    fn locked_position_is_preserved() {
        new_test_ext(1).execute_with(|| {
            let netuid = network();
            let hot = U256::from(2);
            let cold = U256::from(3);
            legacy_position(hot, cold, netuid, MAX_DUST_TAO);
            convert_for_test::<Test>();
            assert_ok!(SubtensorModule::do_lock_stake(
                &cold,
                netuid,
                &hot,
                MAX_DUST_TAO.into()
            ));
            AlphaV2::<Test>::remove((hot, cold, netuid));
            retired::Alpha::<Test>::insert((hot, cold, netuid), U64F64::from_num(MAX_DUST_TAO));
            run_batches();
            assert_eq!(
                SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hot, &cold, netuid),
                MAX_DUST_TAO.into()
            );
        });
    }

    #[test]
    fn escrow_and_retired_epochs_are_not_revived_or_cashed_out() {
        new_test_ext(1).execute_with(|| {
            let netuid = network();
            let hot = U256::from(2);
            let escrow = SubtensorModule::get_beta_escrow_account_id();
            legacy_position(hot, escrow, netuid, 1000);
            let retired_hot = U256::from(4);
            let cold = U256::from(5);
            legacy_position(retired_hot, cold, netuid, 1000);
            AlphaSharePoolEpoch::<Test>::insert(retired_hot, netuid, 1);
            let denominator = SafeFloat::from(1000u64);
            run_batches();
            assert_eq!(
                SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hot, &escrow, netuid),
                1000.into()
            );
            assert!(!AlphaV2::<Test>::contains_key((retired_hot, cold, netuid)));
            assert_eq!(
                TotalHotkeySharesV2::<Test>::get(retired_hot, netuid),
                denominator
            );
        });
    }
    #[test]
    fn scheduling_and_insufficient_budget_do_not_scan_or_mutate_positions() {
        new_test_ext(1).execute_with(|| {
            let netuid = network();
            let hot = U256::from(2);
            let cold = U256::from(3);
            legacy_position(hot, cold, netuid, 499);
            let before = SubtensorModule::get_coldkey_balance(&cold);
            migrate::<Test>();
            assert!(retired::Alpha::<Test>::contains_key((hot, cold, netuid)));
            assert!(!HasMigrationRun::<Test>::get(MIGRATION_NAME));
            let root = sp_io::storage::root(sp_runtime::StateVersion::V1);
            migrate::<Test>();
            assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), root);
            assert_eq!(continue_migration::<Test>(Weight::zero()), Weight::zero());
            assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), root);
            assert_eq!(SubtensorModule::get_coldkey_balance(&cold), before);
        });
    }

    #[test]
    fn batches_resume_and_respect_a_top_up_between_blocks() {
        new_test_ext(1).execute_with(|| {
            let netuid = network();
            for i in 10..30 {
                legacy_position(U256::from(i), U256::from(i + 100), netuid, 499);
            }
            migrate::<Test>();
            let budget = Weight::from_parts(10_000_000_000, u64::MAX);
            assert!(continue_migration::<Test>(budget).all_lte(budget));
            let progress = AlphaV2Migration::<Test>::get().expect("scheduled");
            assert!(progress.legacy > 0 && progress.legacy < 20);
            assert!(!HasMigrationRun::<Test>::get(MIGRATION_NAME));
            // Simulate a live share-pool write: it must read the legacy pool and
            // atomically promote both formats, retaining the new stake.
            let ((hot, cold, _), _) = retired::Alpha::<Test>::iter()
                .next()
                .expect("remaining row");
            SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &hot,
                &cold,
                netuid,
                10_000_000u64.into(),
            );
            let stake =
                SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hot, &cold, netuid);
            assert!(stake > MAX_DUST_TAO.into());
            for _ in 0..100 {
                assert!(continue_migration::<Test>(budget).all_lte(budget));
                if !in_progress::<Test>() {
                    break;
                }
            }
            assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME));
            assert_eq!(
                SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hot, &cold, netuid),
                stake
            );
            assert!(retired::Alpha::<Test>::iter().next().is_none());
            assert_eq!(AlphaV2::<Test>::iter().count(), 1);
        });
    }

    #[test]
    fn v2_cutoff_is_strict_and_fractional_tao_is_not_rounded_up() {
        for alpha in [0u64, 1, 2, 998, 999, 1000, 2000] {
            new_test_ext(1).execute_with(|| {
                let netuid = network();
                setup_reserves(
                    netuid,
                    500_000_000_000u64.into(),
                    1_000_000_000_000u64.into(),
                );
                let hot = U256::from(10);
                let cold = U256::from(20);
                legacy_position(hot, cold, netuid, alpha);
                convert_for_test::<Test>();
                let burn: U256 = <Test as Config>::BurnAccountId::get().into_account_truncating();
                add_balance_to_coldkey_account(&burn, 500u64.into());
                let balance = SubtensorModule::get_coldkey_balance(&cold);
                run_batches();
                assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME));
                assert_eq!(
                    AlphaV2::<Test>::contains_key((hot, cold, netuid)),
                    alpha >= 1000
                );
                assert_eq!(SubtensorModule::get_coldkey_balance(&cold), balance);
                let progress = AlphaV2Migration::<Test>::get().expect("final counters");
                assert_eq!(progress.deleted, u64::from(alpha < 1000));
                assert_eq!(progress.burned, if alpha < 1000 { alpha / 2 } else { 0 });
                assert_eq!(progress.pending_burn, 0);
                assert_eq!(progress.refunded, 0);
            });
        }
    }

    #[test]
    fn v2_sweep_finishes_without_revisiting_new_dust_behind_cursor() {
        new_test_ext(1).execute_with(|| {
            let netuid = network();
            for i in 10..30 {
                legacy_position(U256::from(i), U256::from(i + 100), netuid, 499);
            }
            convert_for_test::<Test>();
            let original: Vec<_> = AlphaV2::<Test>::iter_keys().collect();
            let burn: U256 = <Test as Config>::BurnAccountId::get().into_account_truncating();
            add_balance_to_coldkey_account(&burn, 500u64.into());
            migrate::<Test>();
            let budget = Weight::from_parts(10_000_000_000, u64::MAX);
            assert!(continue_migration::<Test>(budget).all_lte(budget));
            let progress = AlphaV2Migration::<Test>::get().expect("partial sweep");
            assert_eq!(progress.phase, 2);
            assert!(progress.scanned > 0 && progress.scanned < 20);
            let &(hot, cold, _) = original.first().expect("initial V2 positions");
            assert!(!AlphaV2::<Test>::contains_key((hot, cold, netuid)));
            // Normal emissions can recreate a deleted position behind the cursor.
            SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &hot,
                &cold,
                netuid,
                7u64.into(),
            );
            // New positions ahead of the cursor may also be cleaned; no snapshot is needed.
            let new_hot = (1000..2000)
                .map(U256::from)
                .find(|candidate| {
                    AlphaV2::<Test>::hashed_key_for((candidate, &cold, netuid))
                        > progress.after.clone().expect("cursor")
                })
                .expect("new key ahead of cursor");
            SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &new_hot,
                &cold,
                netuid,
                7u64.into(),
            );
            run_batches();
            assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME));
            let progress = AlphaV2Migration::<Test>::get().expect("completed sweep");
            assert_eq!(progress.passes, 1);
            assert_eq!(progress.scanned, 21);
            assert_eq!(progress.deleted, 21);
            assert_eq!(AlphaV2::<Test>::iter().count(), 1);
            assert!(AlphaV2::<Test>::contains_key((hot, cold, netuid)));
            assert!(!AlphaV2::<Test>::contains_key((new_hot, cold, netuid)));
            assert!(retired::Alpha::<Test>::iter().next().is_none());
            assert!(retired::TotalHotkeyShares::<Test>::iter().next().is_none());
        });
    }

    #[test]
    fn insufficient_burn_account_ed_is_persisted_and_not_reported_complete() {
        new_test_ext(1).execute_with(|| {
            let netuid = network();
            legacy_position(U256::from(2), U256::from(3), netuid, 249);
            convert_for_test::<Test>();
            run_batches();
            let progress = AlphaV2Migration::<Test>::get().expect("pending burn");
            assert_eq!(progress.pending_burn, 249);
            assert_eq!(progress.passes, 1);
            assert_eq!(progress.scanned, 1);
            assert!(!HasMigrationRun::<Test>::get(MIGRATION_NAME));
            // Waiting for burn settlement must not restart a completed V2 sweep.
            SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &U256::from(2),
                &U256::from(3),
                netuid,
                7u64.into(),
            );
            let burn: U256 = <Test as Config>::BurnAccountId::get().into_account_truncating();
            add_balance_to_coldkey_account(&burn, 500u64.into());
            run_batches();
            assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME));
            assert_eq!(SubtensorModule::get_coldkey_balance(&burn), 749u64.into());
            let progress = AlphaV2Migration::<Test>::get().expect("settled burn");
            assert_eq!(progress.scanned, 1);
            assert_eq!(progress.passes, 1);
            assert!(AlphaV2::<Test>::contains_key((
                U256::from(2),
                U256::from(3),
                netuid
            )));
        });
    }
}
