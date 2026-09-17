use super::*;
use crate::subnets::dissolution::DissolveCleanupStatus;
use crate::weights::WeightInfo;
use frame_support::weights::{Weight, WeightMeter};
use num_traits::ToPrimitive;
use sp_core::Get;
use sp_runtime::DispatchError;
use sp_std::collections::btree_map::BTreeMap;
use sp_std::collections::btree_set::BTreeSet;
use substrate_fixed::types::U96F32;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};
use subtensor_swap_interface::{Order, SwapHandler};

/// Storage reads spent on a subnet that `unstake_all` visits but does not unstake from: the
/// subtoken flag, the position quote (legacy and current share rows, pool value, both
/// denominator maps) and the subnet-existence check in `validate_remove_stake`.
const UNSTAKE_ALL_SKIPPED_SUBNET_READS: u64 = 8;

/// Work done by one `unstake_all` / `unstake_all_alpha` dispatch, reported so the call can
/// refund its declared worst-case weight down to what it really did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UnstakeAllWork {
    /// Subnets the loop visited.
    pub scanned: u32,
    /// Subnets where a position was unstaked (each is a full `remove_stake` worth of work).
    pub legs: u32,
    /// Subnets where `validate_remove_stake` ran. That path walks `StakingHotkeys`
    /// (lock / collateral), so a locked skip still costs the walk, not just the
    /// cheap quote reads.
    pub validated: u32,
}

impl<T: Config> Pallet<T> {
    /// The implementation for the extrinsic remove_stake: Removes stake from a hotkey account and adds it onto a coldkey.
    ///
    /// # Arguments
    /// * `origin`: The signature of the caller's coldkey.
    ///
    /// * `hotkey`: The associated hotkey account.
    ///
    /// * `netuid`: Subnetwork UID.
    ///
    /// * `alpha_unstaked`: The amount of stake to be removed from the staking account.
    ///
    /// # Events
    /// * `StakeRemoved`: On the successfully removing stake from the hotkey account.
    ///
    /// # Errors
    /// * `NotRegistered`: Thrown if the account we are attempting to unstake from is non existent.
    ///
    /// * `NonAssociatedColdKey`: Thrown if the coldkey does not own the hotkey we are unstaking from.
    ///
    /// * `NotEnoughStakeToWithdraw`: Thrown if there is not enough stake on the hotkey to withdwraw this amount.
    ///
    /// * `TxRateLimitExceeded`: Thrown if key has hit transaction rate limit.
    ///
    pub fn do_remove_stake(
        origin: OriginFor<T>,
        hotkey: T::AccountId,
        netuid: NetUid,
        alpha_unstaked: AlphaBalance,
    ) -> dispatch::DispatchResult {
        // 1. We check the transaction is signed by the caller and retrieve the T::AccountId coldkey information.
        let coldkey = ensure_signed(origin)?;
        log::debug!(
            "do_remove_stake( origin:{coldkey:?} hotkey:{hotkey:?}, netuid: {netuid:?}, alpha_unstaked:{alpha_unstaked:?} )"
        );

        Self::ensure_subtoken_enabled(netuid)?;

        // 1.1. Cap the alpha_unstaked at available Alpha because user might be paying transaxtion fees
        // in Alpha and their total is already reduced by now.
        let alpha_available =
            Self::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);
        let alpha_unstaked = alpha_unstaked.min(alpha_available);
        Self::ensure_remove_stake_input_within_swap_limit(netuid, alpha_unstaked)?;

        // 2. Validate the user input
        Self::validate_remove_stake(
            &coldkey,
            &hotkey,
            netuid,
            alpha_unstaked,
            alpha_unstaked,
            false,
        )?;

        // 3. Swap the alpba to tao and update counters for this subnet.
        Self::unstake_from_subnet(
            &hotkey,
            &coldkey,
            &coldkey,
            netuid,
            alpha_unstaked,
            T::SwapInterface::min_price(),
            false,
            true,
        )?;

        // 5. If the stake is below the minimum, we clear the nomination from storage.
        Self::clear_small_nomination_if_required(&hotkey, &coldkey, netuid);

        // 6. Queue the live child relations for the threshold re-check in on_idle. Pending
        // (not yet matured) schedules are re-checked against the threshold when they mature
        // (`do_set_pending_children`), so no all-subnet valuation runs inside this call.
        Self::queue_childkey_threshold_check(&hotkey);

        // Done and ok.
        Ok(())
    }

    /// The implementation for the extrinsic unstake_all: Removes all stake from a hotkey account across all subnets and adds it onto a coldkey.
    ///
    /// # Arguments
    /// * `origin`: The signature of the caller's coldkey.
    ///
    /// * `hotkey`: The associated hotkey account.
    ///
    /// # Events
    /// * `StakeRemoved`: On the successfully removing stake from the hotkey account.
    ///
    /// # Errors
    /// * `NotRegistered`: Thrown if the account we are attempting to unstake from is non existent.
    ///
    /// * `NonAssociatedColdKey`: Thrown if the coldkey does not own the hotkey we are unstaking from.
    ///
    /// * `NotEnoughStakeToWithdraw`: Thrown if there is not enough stake on the hotkey to withdraw this amount.
    ///
    /// * `TxRateLimitExceeded`: Thrown if key has hit transaction rate limit.
    ///
    /// Returns the work performed so the dispatch can report its actual weight.
    pub fn do_unstake_all(
        origin: OriginFor<T>,
        hotkey: T::AccountId,
    ) -> Result<UnstakeAllWork, DispatchError> {
        // 1. We check the transaction is signed by the caller and retrieve the T::AccountId coldkey information.
        let coldkey = ensure_signed(origin)?;
        log::debug!("do_unstake_all( origin:{coldkey:?} hotkey:{hotkey:?} )");

        // 2. Ensure that the hotkey account exists this is only possible through registration.
        ensure!(
            Self::hotkey_account_exists(&hotkey),
            Error::<T>::HotKeyAccountNotExists
        );

        // 3. Get all netuids.
        let netuids = Self::get_all_subnet_netuids();
        log::debug!("All subnet netuids: {netuids:?}");

        // 4. Iterate through subnets and remove stake, stopping at the admission
        // envelope so the declared weight stays under the normal-class block.
        let mut work = UnstakeAllWork::default();
        for netuid in netuids.into_iter() {
            if work.validated >= crate::MAX_UNSTAKE_ALL_LEGS {
                break;
            }
            if let Some(alpha_unstaked) =
                Self::unstake_all_consider_subnet(&mut work, &coldkey, &hotkey, netuid)
            {
                work.legs = work.legs.saturating_add(1);
                Self::unstake_from_subnet(
                    &hotkey,
                    &coldkey,
                    &coldkey,
                    netuid,
                    alpha_unstaked,
                    T::SwapInterface::min_price(),
                    false,
                    true,
                )?;
                Self::clear_small_nomination_if_required(&hotkey, &coldkey, netuid);
            }
        }

        // 5. Queue the live child relations for the threshold re-check in on_idle.
        Self::queue_childkey_threshold_check(&hotkey);

        // 6. Done and ok.
        Ok(work)
    }

    /// Decide whether one subnet in an `unstake_all*` loop is a cheap skip, a
    /// validated skip (locked / failed), or a position to unstake. Empty and
    /// subtoken-disabled subnets do not consume the admission envelope, so a
    /// later call can keep walking leftover positions.
    fn unstake_all_consider_subnet(
        work: &mut UnstakeAllWork,
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        netuid: NetUid,
    ) -> Option<AlphaBalance> {
        work.scanned = work.scanned.saturating_add(1);
        if !SubtokenEnabled::<T>::get(netuid) {
            return None;
        }
        let alpha = Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, netuid);
        if alpha.is_zero() {
            return None;
        }
        work.validated = work.validated.saturating_add(1);
        if Self::validate_remove_stake(coldkey, hotkey, netuid, alpha, alpha, false).is_err() {
            return None;
        }
        Some(alpha)
    }

    /// Weight of an `unstake_all`-style dispatch over `base` (the benchmarked fixed part of
    /// the call) plus the per-subnet loop: every subnet where a position was unstaked costs
    /// a full `remove_stake` (the same validation, swap, transfer and event work). A
    /// validated-but-skipped subnet (locked, liquidity) still walked `StakingHotkeys`
    /// and is charged that walk. Cheap skips (disabled / empty) cost only the quote
    /// reads that decided to skip them.
    /// `walk` is the `StakingHotkeys` walk every validated subnet performs, which the
    /// `remove_stake` benchmark does not include.
    fn unstake_all_weight(base: Weight, work: UnstakeAllWork, walk: Weight) -> Weight {
        let cheap = u64::from(work.scanned.saturating_sub(work.validated));
        let expensive_skips = u64::from(work.validated.saturating_sub(work.legs));
        base.saturating_add(
            <T as crate::pallet::Config>::WeightInfo::remove_stake()
                .saturating_add(walk)
                .saturating_mul(u64::from(work.legs)),
        )
        .saturating_add(walk.saturating_mul(expensive_skips))
        .saturating_add(
            T::DbWeight::get().reads(UNSTAKE_ALL_SKIPPED_SUBNET_READS.saturating_mul(cheap)),
        )
    }

    /// Worst case one call will admit: a full unstake on every envelope slot.
    fn unstake_all_worst_case_work() -> UnstakeAllWork {
        let cap = u32::from(TotalNetworks::<T>::get()).min(crate::MAX_UNSTAKE_ALL_LEGS);
        UnstakeAllWork {
            scanned: cap,
            legs: cap,
            validated: cap,
        }
    }

    /// Pre-dispatch weight of `unstake_all`: the benchmarked fixed part plus, per
    /// envelope slot, one `remove_stake` and a `StakingHotkeys` walk at the cap.
    /// Refunded to the actual work post-dispatch.
    pub fn unstake_all_declared_weight() -> Weight {
        Self::unstake_all_weight(
            <T as crate::pallet::Config>::WeightInfo::unstake_all(),
            Self::unstake_all_worst_case_work(),
            Self::staking_hotkeys_walk_bound(),
        )
    }

    /// Post-dispatch weight of `unstake_all` for the work it really did.
    pub fn unstake_all_actual_weight(coldkey: &T::AccountId, work: UnstakeAllWork) -> Weight {
        Self::unstake_all_weight(
            <T as crate::pallet::Config>::WeightInfo::unstake_all(),
            work,
            Self::staking_hotkeys_walk_actual(coldkey),
        )
    }

    /// Pre-dispatch weight of `unstake_all_alpha`: the benchmarked fixed part (which already
    /// includes the final root restake) plus one `remove_stake` per envelope slot.
    pub fn unstake_all_alpha_declared_weight() -> Weight {
        Self::unstake_all_weight(
            <T as crate::pallet::Config>::WeightInfo::unstake_all_alpha(),
            Self::unstake_all_worst_case_work(),
            Self::staking_hotkeys_walk_bound(),
        )
    }

    /// Post-dispatch weight of `unstake_all_alpha` for the work it really did.
    pub fn unstake_all_alpha_actual_weight(coldkey: &T::AccountId, work: UnstakeAllWork) -> Weight {
        Self::unstake_all_weight(
            <T as crate::pallet::Config>::WeightInfo::unstake_all_alpha(),
            work,
            Self::staking_hotkeys_walk_actual(coldkey),
        )
    }

    /// The implementation for the extrinsic unstake_all: Removes all stake from a hotkey account across all subnets and adds it onto a coldkey.
    ///
    /// # Arguments
    /// * `origin`: The signature of the caller's coldkey.
    ///
    /// * `hotkey`: The associated hotkey account.
    ///
    /// # Events
    /// * `StakeRemoved`: On the successfully removing stake from the hotkey account.
    ///
    /// # Errors
    /// * `NotRegistered`: Thrown if the account we are attempting to unstake from is non existent.
    ///
    /// * `NonAssociatedColdKey`: Thrown if the coldkey does not own the hotkey we are unstaking from.
    ///
    /// * `NotEnoughStakeToWithdraw`: Thrown if there is not enough stake on the hotkey to withdraw this amount.
    ///
    /// * `TxRateLimitExceeded`: Thrown if key has hit transaction rate limit.
    ///
    pub fn do_unstake_all_alpha(
        origin: OriginFor<T>,
        hotkey: T::AccountId,
    ) -> Result<UnstakeAllWork, DispatchError> {
        // 1. We check the transaction is signed by the caller and retrieve the T::AccountId coldkey information.
        let coldkey = ensure_signed(origin)?;
        Self::ensure_beta_basket_seed_idle()?;
        log::debug!("do_unstake_all( origin:{coldkey:?} hotkey:{hotkey:?} )");

        // 2. Ensure that the hotkey account exists this is only possible through registration.
        ensure!(
            Self::hotkey_account_exists(&hotkey),
            Error::<T>::HotKeyAccountNotExists
        );

        // 3. Get all netuids.
        let netuids = Self::get_all_subnet_netuids();
        log::debug!("All subnet netuids: {netuids:?}");

        // 4. Iterate through non-root subnets and remove stake, stopping at the
        // admission envelope so the declared weight stays under the normal-class block.
        let mut work = UnstakeAllWork::default();
        let mut total_tao_unstaked = TaoBalance::ZERO;
        for netuid in netuids.into_iter() {
            if work.validated >= crate::MAX_UNSTAKE_ALL_LEGS {
                break;
            }
            if netuid.is_root() {
                work.scanned = work.scanned.saturating_add(1);
                continue;
            }
            if let Some(alpha_unstaked) =
                Self::unstake_all_consider_subnet(&mut work, &coldkey, &hotkey, netuid)
            {
                work.legs = work.legs.saturating_add(1);
                let tao_unstaked = Self::unstake_from_subnet(
                    &hotkey,
                    &coldkey,
                    &coldkey,
                    netuid,
                    alpha_unstaked,
                    T::SwapInterface::min_price(),
                    false,
                    true,
                )?;
                total_tao_unstaked = total_tao_unstaked.saturating_add(tao_unstaked);
                Self::clear_small_nomination_if_required(&hotkey, &coldkey, netuid);
            }
        }

        // Stake into root.
        Self::stake_into_subnet(
            &hotkey,
            &coldkey,
            NetUid::ROOT,
            total_tao_unstaked,
            T::SwapInterface::max_price(),
            false,
        )?;

        // 5. Queue the live child relations for the threshold re-check in on_idle.
        Self::queue_childkey_threshold_check(&hotkey);

        // 6. Done and ok.
        Ok(work)
    }

    /// The implementation for the extrinsic remove_stake_limit: Removes stake from
    /// a hotkey on a subnet with a price limit.
    ///
    /// In case if slippage occurs and the price shall move beyond the limit
    /// price, the staking order may execute only partially or not execute
    /// at all.
    ///
    /// # Arguments
    /// * `origin`: The signature of the caller's coldkey.
    ///
    /// * `hotkey`: The associated hotkey account.
    ///
    /// * `netuid`: Subnetwork UID.
    ///
    /// * `amount_unstaked`: The amount of stake to be added to the hotkey staking account.
    ///
    /// * `limit_price`: The limit price expressed in units of RAO per one Alpha.
    ///
    /// * `allow_partial`: Allows partial execution of the amount. If set to false, this becomes
    ///   fill or kill type of order.
    ///
    /// # Events
    /// * `StakeRemoved`: On the successfully removing stake from the hotkey account.
    ///
    /// # Errors
    /// * `NotRegistered`: Thrown if the account we are attempting to unstake from is non existent.
    ///
    /// * `NonAssociatedColdKey`: Thrown if the coldkey does not own the hotkey we are unstaking from.
    ///
    /// * `NotEnoughStakeToWithdraw`: Thrown if there is not enough stake on the hotkey to withdwraw this amount.
    ///
    pub fn do_remove_stake_limit(
        origin: OriginFor<T>,
        hotkey: T::AccountId,
        netuid: NetUid,
        alpha_unstaked: AlphaBalance,
        limit_price: TaoBalance,
        allow_partial: bool,
    ) -> dispatch::DispatchResult {
        // 1. We check the transaction is signed by the caller and retrieve the T::AccountId coldkey information.
        let coldkey = ensure_signed(origin)?;
        log::debug!(
            "do_remove_stake( origin:{coldkey:?} hotkey:{hotkey:?}, netuid: {netuid:?}, alpha_unstaked:{alpha_unstaked:?} )"
        );

        Self::ensure_remove_stake_input_within_swap_limit(netuid, alpha_unstaked)?;

        // 2. Calculate the maximum amount that can be executed with price limit
        let max_amount = Self::get_max_amount_remove(netuid, limit_price)?;
        let mut possible_alpha = alpha_unstaked;
        if possible_alpha > max_amount {
            possible_alpha = max_amount;
        }

        // 3. Validate the user input
        Self::validate_remove_stake(
            &coldkey,
            &hotkey,
            netuid,
            alpha_unstaked,
            max_amount,
            allow_partial,
        )?;

        // 4. Swap the alpha to tao and update counters for this subnet.
        Self::unstake_from_subnet(
            &hotkey,
            &coldkey,
            &coldkey,
            netuid,
            possible_alpha,
            limit_price,
            false,
            true,
        )?;

        // 5. If the stake is below the minimum, we clear the nomination from storage.
        Self::clear_small_nomination_if_required(&hotkey, &coldkey, netuid);

        // 6. Queue the live child relations for the threshold re-check in on_idle. Pending
        // (not yet matured) schedules are re-checked against the threshold when they mature
        // (`do_set_pending_children`), so no all-subnet valuation runs inside this call.
        Self::queue_childkey_threshold_check(&hotkey);

        // Done and ok.
        Ok(())
    }

    // Returns the maximum amount of RAO that can be executed with price limit
    pub fn get_max_amount_remove(
        netuid: NetUid,
        limit_price: TaoBalance,
    ) -> Result<AlphaBalance, DispatchError> {
        // Corner case: root and stao
        // There's no slippage for root or stable subnets, so if limit price is 1e9 rao or
        // lower, then max_amount equals u64::MAX, otherwise it is 0.
        if netuid.is_root() || SubnetMechanism::<T>::get(netuid) == 0 {
            if limit_price <= 1_000_000_000.into() {
                return Ok(AlphaBalance::MAX);
            } else {
                return Ok(AlphaBalance::ZERO);
            }
        }

        // Use the largest supported input instead of probing the swap path with u64::MAX.
        let max_supported_input = SubnetAlphaIn::<T>::get(netuid).saturating_mul(1_000.into());
        let order = GetTaoForAlpha::<T>::with_amount(max_supported_input);
        let result = T::SwapInterface::swap(netuid.into(), order, limit_price.into(), false, true)
            .map(|r| r.amount_paid_in.saturating_add(r.fee_paid))?;

        Ok(result)
    }

    fn ensure_remove_stake_input_within_swap_limit(
        netuid: NetUid,
        amount: AlphaBalance,
    ) -> Result<(), Error<T>> {
        if !netuid.is_root() && SubnetMechanism::<T>::get(netuid) == 1 {
            let max_supported_input = SubnetAlphaIn::<T>::get(netuid).saturating_mul(1_000.into());
            ensure!(
                amount <= max_supported_input,
                Error::<T>::InsufficientLiquidity
            );
        }

        Ok(())
    }

    pub fn do_remove_stake_full_limit(
        origin: OriginFor<T>,
        hotkey: T::AccountId,
        netuid: NetUid,
        limit_price: Option<TaoBalance>,
    ) -> DispatchResult {
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        let coldkey = ensure_signed(origin.clone())?;

        let alpha_unstaked =
            Self::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);

        if let Some(limit_price) = limit_price {
            Self::do_remove_stake_limit(origin, hotkey, netuid, alpha_unstaked, limit_price, false)
        } else {
            Self::do_remove_stake(origin, hotkey, netuid, alpha_unstaked)
        }
    }

    pub fn destroy_alpha_in_out_stakes(
        netuid: NetUid,
        weight_meter: &mut WeightMeter,
        status: &mut DissolveCleanupStatus,
    ) -> bool {
        let Some(total_alpha_value_u128) = status.subnet_total_alpha_value else {
            log::warn!("DissolveCleanupStatus.subnet_total_alpha_value not set");
            return false;
        };

        let Some(mut distributed_tao_value_u128) = status.subnet_distributed_tao else {
            log::warn!("DissolveCleanupStatus.subnet_distributed_tao not set");
            return false;
        };

        // Check if there is enought weight to complete all the operations in this function
        // It is the maximum weight that can be consumed by the function. including all potential reads and writes.
        let max_weight = T::DbWeight::get().reads_writes(20, 12);
        if !weight_meter.can_consume(max_weight) {
            return false;
        }
        weight_meter.consume(max_weight);
        let owner_coldkey: T::AccountId = SubnetOwner::<T>::get(netuid);
        let lock_cost: TaoBalance = Self::get_subnet_locked_balance(netuid);

        // Determine if this subnet is eligible for a lock refund (legacy).
        let reg_at: u64 = NetworkRegisteredAt::<T>::get(netuid);

        let start_block: u64 = NetworkRegistrationStartBlock::<T>::get();
        let should_refund_owner: bool = reg_at < start_block;

        let protocol_alpha_value_u128: u128 =
            SubnetProtocolAlpha::<T>::get(netuid).to_u64() as u128;

        let pot_tao: TaoBalance = SubnetTAO::<T>::get(netuid);
        let pot_u128: u128 = pot_tao.into();

        // Compute owner's received emission in TAO at current price (ONLY if we may refund).
        // We:
        //      - get the current alpha issuance,
        //      - apply owner fraction to get owner α,
        //      - price that α using a *simulated* AMM swap.
        let mut owner_emission_tao = TaoBalance::ZERO;
        if should_refund_owner && !lock_cost.is_zero() {
            let total_emitted_alpha_u128: u128 = Self::get_alpha_issuance(netuid).to_u64() as u128;

            if total_emitted_alpha_u128 > 0 {
                let owner_fraction: U96F32 = Self::get_float_subnet_owner_cut();
                let owner_alpha_u64 = U96F32::from_num(total_emitted_alpha_u128)
                    .saturating_mul(owner_fraction)
                    .floor()
                    .saturating_to_num::<u64>();

                owner_emission_tao = if owner_alpha_u64 > 0 {
                    let cur_price: U96F32 = U96F32::saturating_from_num(
                        T::SwapInterface::current_alpha_price(netuid.into()),
                    );
                    let val_u64 = U96F32::from_num(owner_alpha_u64)
                        .saturating_mul(cur_price)
                        .floor()
                        .saturating_to_num::<u64>();
                    val_u64.into()
                } else {
                    TaoBalance::ZERO
                };
            }
        }

        let mut protocol_tao_share = TaoBalance::ZERO;
        if protocol_alpha_value_u128 > 0 {
            let prod: u128 = pot_u128.saturating_mul(protocol_alpha_value_u128);
            let share_u128: u128 = prod.checked_div(total_alpha_value_u128).unwrap_or_default();
            protocol_tao_share = (share_u128.min(u128::from(u64::MAX)) as u64).into();
        }

        // Remove α‑in/α‑out counters (fully destroyed).
        SubnetAlphaIn::<T>::remove(netuid);
        SubnetAlphaOut::<T>::remove(netuid);
        SubnetProtocolAlpha::<T>::remove(netuid);

        // Clear the locked balance on the subnet.
        Self::set_subnet_locked_balance(netuid, TaoBalance::ZERO);

        // Finalize lock handling:
        //    - Legacy subnets (registered before NetworkRegistrationStartBlock) receive:
        //        refund = max(0, lock_cost(τ) − owner_received_emission_in_τ).
        //    - New subnets: no refund.
        let mut refund: TaoBalance = if should_refund_owner {
            lock_cost.saturating_sub(owner_emission_tao)
        } else {
            TaoBalance::ZERO
        };

        if !refund.is_zero()
            && let Some(subnet_account) = Self::get_subnet_account_id(netuid)
        {
            // Transfer maximum transferrable up to refund to owner
            let transferrable =
                Self::get_coldkey_balance(&subnet_account).saturating_sub(protocol_tao_share);

            distributed_tao_value_u128 = distributed_tao_value_u128.saturating_add(refund.into());

            if distributed_tao_value_u128 < pot_u128 {
                let final_leftover: u128 = pot_u128.saturating_sub(distributed_tao_value_u128);

                refund = refund.saturating_add(final_leftover.into());
            }
            // We do our best effort to refund owner to as full amount of refund as possible, but
            // we cannot fail new subnet registration, so the result is ignored.
            let _ = Self::transfer_tao(&subnet_account, &owner_coldkey, refund.min(transferrable));
        }

        // 9) Recycle TAO remaining on the subnet account, forgive errors.
        if let Some(subnet_account) = Self::get_subnet_account_id(netuid) {
            let remaining_subnet_balance = Self::get_keep_alive_balance(&subnet_account);
            if Self::recycle_tao(&subnet_account, remaining_subnet_balance).is_ok() {
                RAORecycledForRegistration::<T>::insert(netuid, remaining_subnet_balance);
            }
        }

        status.subnet_total_alpha_value = None;
        status.subnet_distributed_tao = None;
        SubnetTAO::<T>::remove(netuid);

        true
    }

    /// This function calculates the total alpha value for a subnet.
    /// It iterates through all hotkeys in the subnet and calculates the total alpha value.
    /// It returns true if all hotkeys are iterated, otherwise false.
    ///
    /// # Arguments
    /// * `netuid`: The subnet to calculate the total alpha value for.
    ///
    /// * `weight_meter`: The weight meter to consume the weight for the operation.
    ///
    /// # Returns
    /// * `bool`: True if all hotkeys are iterated, otherwise false.
    ///
    pub fn destroy_alpha_in_out_stakes_get_total_alpha_value(
        netuid: NetUid,
        weight_meter: &mut WeightMeter,
        last_key: Option<Vec<u8>>,
        status: &mut DissolveCleanupStatus,
    ) -> (bool, Option<Vec<u8>>) {
        let r = T::DbWeight::get().reads(1);
        let mut read_all = true;

        let mut total_alpha_value_u128: u128;

        if let Some(value) = status.subnet_total_alpha_value {
            total_alpha_value_u128 = value;
        } else {
            let reg_at: u64 = NetworkRegisteredAt::<T>::get(netuid);
            let tao_in_refund_deployment_block: u64 = TaoInRefundDeploymentBlock::<T>::get();

            // Legacy subnets keep the old dereg behavior: ignore SubnetAlphaIn.
            // New subnets include SubnetAlphaIn.
            let protocol_alpha_value_u128: u128 = if reg_at > tao_in_refund_deployment_block {
                SubnetAlphaIn::<T>::get(netuid)
                    .saturating_add(SubnetProtocolAlpha::<T>::get(netuid))
                    .to_u64() as u128
            } else {
                SubnetProtocolAlpha::<T>::get(netuid).to_u64() as u128
            };
            total_alpha_value_u128 = protocol_alpha_value_u128;
        }

        // Preserve the inbound cursor so a pass that only skips other-netuid rows (or
        // exhausts weight before finishing another matching hotkey) cannot return None
        // and restart from the beginning — that would double-count into
        // status.subnet_total_alpha_value.
        let mut last_completed_key = last_key.clone();
        // Once a hotkey's Alpha/AlphaV2 prefix scan is started, finish it even if the
        // remaining on_idle budget is exhausted. A single hotkey can have more prefix
        // entries across all subnets than one block's leftover weight; aborting mid-hotkey
        // and retrying forever livelocks dissolution.
        let mut exhausted = false;

        let iter = match last_key {
            Some(key) => TotalHotkeyAlpha::<T>::iter_from(key),
            None => TotalHotkeyAlpha::<T>::iter(),
        };

        for (hot, this_netuid, _) in iter {
            if exhausted || !weight_meter.can_consume(r) {
                read_all = false;
                break;
            }
            weight_meter.consume(r);

            if this_netuid != netuid {
                continue;
            }

            for (cold, this_netuid, share_u64f64) in Self::alpha_iter_single_prefix(&hot) {
                // Row read plus the pool/row epoch reads of the retirement check.
                let inner_reads = r.saturating_mul(3_u64);
                if weight_meter.can_consume(inner_reads) {
                    weight_meter.consume(inner_reads);
                } else {
                    exhausted = true;
                }

                if this_netuid != netuid {
                    continue;
                }

                // Rows left behind by a closed pool are worth nothing; never let the raw
                // share fallback below revive them.
                if Self::alpha_share_is_retired(&hot, &cold, netuid) {
                    continue;
                }

                // Primary: actual α value via share pool.
                let pool = Self::get_alpha_share_pool(hot.clone(), netuid);
                let actual_val_u64 = pool.try_get_value(&cold).unwrap_or(0);

                // Fallback: if pool uninitialized, treat raw Alpha share as value.
                let val_u64 = if actual_val_u64 == 0 {
                    u64::from(share_u64f64)
                } else {
                    actual_val_u64
                };

                if val_u64 > 0 {
                    let val_u128 = val_u64 as u128;
                    total_alpha_value_u128 = total_alpha_value_u128.saturating_add(val_u128);
                }
            }

            last_completed_key = Some(TotalHotkeyAlpha::<T>::hashed_key_for(&hot, netuid));
            if exhausted {
                read_all = false;
                break;
            }
        }

        status.subnet_total_alpha_value = Some(total_alpha_value_u128);

        (read_all, last_completed_key)
    }

    pub fn destroy_alpha_in_out_stakes_settle_stakes(
        netuid: NetUid,
        weight_meter: &mut WeightMeter,
        last_key: Option<Vec<u8>>,
        status: &mut DissolveCleanupStatus,
    ) -> (bool, Option<Vec<u8>>) {
        let r = T::DbWeight::get().reads(1);
        let w = T::DbWeight::get().writes(1);
        let weight_for_tansfer_tao = T::DbWeight::get().reads_writes(11, 3);
        let mut read_all = true;

        let mut stakers: Vec<(T::AccountId, T::AccountId, u128)> = Vec::new();
        let Some(total_alpha_value_u128) = status.subnet_total_alpha_value else {
            log::warn!("DissolveCleanupStatus.subnet_total_alpha_value not set");
            return (false, None);
        };
        let Some(mut distributed_tao_value_u128) = status.subnet_distributed_tao else {
            log::warn!("DissolveCleanupStatus.subnet_distributed_tao not set");
            return (false, None);
        };

        let mut hotkeys_in_subnet: Vec<T::AccountId> = Vec::new();
        let mut coldkeys = BTreeSet::<T::AccountId>::new();
        let mut last_completed_key = last_key.clone();
        // Finish the current hotkey even if weight runs out mid-prefix; otherwise a fat
        // Alpha/AlphaV2 hotkey prefix livelocks dissolution across blocks.
        let mut exhausted = false;

        let iter = match last_key {
            Some(key) => TotalHotkeyAlpha::<T>::iter_from(key),
            None => TotalHotkeyAlpha::<T>::iter(),
        };

        for (hot, this_netuid, _) in iter {
            if exhausted || !weight_meter.can_consume(r) {
                read_all = false;
                break;
            }
            weight_meter.consume(r);

            if this_netuid != netuid {
                continue;
            }
            hotkeys_in_subnet.push(hot.clone());

            let mut coldkey_value_vec: Vec<(T::AccountId, u128)> = Vec::new();

            // Drain the whole hotkey prefix once started. Weight is accounted when it
            // still fits; overshoot is allowed so the cursor can advance past this hotkey.
            for (cold, this_netuid, share_u64f64) in Self::alpha_iter_single_prefix(&hot) {
                // Row read, pool valuation read, and the pool/row epoch reads of the
                // retirement check.
                let inner_reads = r.saturating_mul(4_u64);
                if weight_meter.can_consume(inner_reads) {
                    weight_meter.consume(inner_reads);
                } else {
                    exhausted = true;
                }
                if this_netuid != netuid {
                    continue;
                }

                // Rows left behind by a closed pool are worth nothing; never let the raw
                // share fallback below revive them.
                if Self::alpha_share_is_retired(&hot, &cold, netuid) {
                    continue;
                }

                // Primary: actual α value via share pool.
                let pool = Self::get_alpha_share_pool(hot.clone(), netuid);
                let actual_val_u64 = pool.try_get_value(&cold).unwrap_or(0);

                // Fallback: if pool uninitialized, treat raw Alpha share as value.
                let val_u64 = if actual_val_u64 == 0 {
                    u64::from(share_u64f64)
                } else {
                    actual_val_u64
                };

                if val_u64 > 0 {
                    let mut need_to_consume_weight = w;

                    // if the coldkey is not in the set, we need to consume the weight for the transfer_tao_from_subnet function call
                    if !coldkeys.contains(&cold) {
                        need_to_consume_weight =
                            need_to_consume_weight.saturating_add(weight_for_tansfer_tao);
                        coldkeys.insert(cold.clone());
                    }

                    if weight_meter.can_consume(need_to_consume_weight) {
                        weight_meter.consume(need_to_consume_weight);
                    } else {
                        exhausted = true;
                    }
                    let val_u128 = val_u64 as u128;
                    coldkey_value_vec.push((cold.clone(), val_u128));
                }
            }

            for (cold, value) in coldkey_value_vec {
                stakers.push((hot.clone(), cold, value));
            }
            last_completed_key = Some(TotalHotkeyAlpha::<T>::hashed_key_for(&hot, netuid));
            if exhausted {
                read_all = false;
                break;
            }
        }

        // total TAO in the subnet pool
        let pot_tao: TaoBalance = SubnetTAO::<T>::get(netuid);
        let pot_u64: u64 = pot_tao.into();

        struct Portion<A, C> {
            _hot: A,
            cold: C,
            share: u64, // TAO to credit to coldkey balance
            rem: u128,  // remainder for largest‑remainder method
        }
        let mut portions: Vec<Portion<_, _>> = Vec::with_capacity(stakers.len());

        // Pro‑rata distribution of the pot by α value (largest‑remainder),
        //    **credited directly to each staker's COLDKEY free balance**.
        if pot_u64 > 0 && total_alpha_value_u128 > 0 && !stakers.is_empty() {
            let pot_u128: u128 = pot_u64 as u128;

            let mut distributed: u128 = 0;
            let mut total_rem: u128 = 0;

            for (hot, cold, alpha_val) in &stakers {
                let prod: u128 = pot_u128.saturating_mul(*alpha_val);
                let share_u128: u128 = prod.checked_div(total_alpha_value_u128).unwrap_or_default();
                let share_u64: u64 = share_u128.min(u128::from(u64::MAX)) as u64;
                distributed = distributed.saturating_add(u128::from(share_u64));

                let rem: u128 = prod.checked_rem(total_alpha_value_u128).unwrap_or_default();
                total_rem = total_rem.saturating_add(rem);
                portions.push(Portion {
                    _hot: hot.clone(),
                    cold: cold.clone(),
                    share: share_u64,
                    rem,
                });
            }

            let leftover: u128 = total_rem
                .checked_div(total_alpha_value_u128)
                .unwrap_or_default();
            if leftover > 0 {
                portions.sort_by(|a, b| b.rem.cmp(&a.rem));
                let give: usize = core::cmp::min(leftover, portions.len() as u128) as usize;
                for p in portions.iter_mut().take(give) {
                    p.share = p.share.saturating_add(1);
                }
            }

            portions = portions
                .into_iter()
                .filter(|p| p.share > 0)
                .collect::<Vec<_>>();

            // Aggregate the transfer amount for each coldkey
            let mut transfer_map = BTreeMap::<T::AccountId, TaoBalance>::new();
            for p in portions {
                if transfer_map.contains_key(&p.cold) {
                    transfer_map.insert(
                        p.cold.clone(),
                        transfer_map
                            .get(&p.cold)
                            .unwrap_or(&TaoBalance::ZERO)
                            .saturating_add(p.share.into()),
                    );
                } else {
                    transfer_map.insert(p.cold.clone(), p.share.into());
                }
            }

            // Credit each share directly to coldkey free balance.
            for transfer in transfer_map.iter() {
                // Cannot fail the whole transaction if this transfer fails
                distributed_tao_value_u128 = distributed_tao_value_u128
                    .saturating_add(transfer.1.to_u128().unwrap_or(0_u128));
                let _ = Self::transfer_tao_from_subnet(netuid, transfer.0, *transfer.1);
            }
        }

        // ignore the weight for handling the final operation, we must set the correct status for the next run
        status.subnet_distributed_tao = Some(distributed_tao_value_u128);

        (read_all, last_completed_key)
    }

    pub fn destroy_alpha_in_out_stakes_clean_alpha(
        netuid: NetUid,
        weight_meter: &mut WeightMeter,
        last_key: Option<Vec<u8>>,
    ) -> (bool, Option<Vec<u8>>) {
        let r = T::DbWeight::get().reads(1);
        let w = T::DbWeight::get().writes(1);
        let mut read_all = true;

        // Same cursor preserve as settle/get_total: do not wipe last_key when a pass
        // only skips non-matching rows or runs out of weight before the next hotkey.
        let mut last_completed_key = last_key.clone();
        // Finish the current hotkey's Alpha/AlphaV2 cleanup even if weight is exhausted.
        let mut exhausted = false;

        let iter = match last_key {
            Some(key) => TotalHotkeyAlpha::<T>::iter_from(key),
            None => TotalHotkeyAlpha::<T>::iter(),
        };

        for (hot, this_netuid, _) in iter {
            let mut coldkeys: Vec<T::AccountId> = Vec::new();
            if exhausted || !weight_meter.can_consume(r) {
                read_all = false;
                break;
            }
            weight_meter.consume(r);

            if this_netuid != netuid {
                continue;
            }

            for (cold, this_netuid, _) in Self::alpha_iter_single_prefix(&hot) {
                if weight_meter.can_consume(r) {
                    weight_meter.consume(r);
                } else {
                    exhausted = true;
                }
                if this_netuid != netuid {
                    continue;
                }
                coldkeys.push(cold.clone());
            }

            // Alpha, AlphaV2 and AlphaShareEpoch removals per coldkey.
            let weight_for_all_remove = w
                .saturating_mul(3_u64)
                .saturating_mul(coldkeys.len() as u64);
            if weight_meter.can_consume(weight_for_all_remove) {
                weight_meter.consume(weight_for_all_remove);
            } else {
                exhausted = true;
            }

            last_completed_key = Some(TotalHotkeyAlpha::<T>::hashed_key_for(&hot, netuid));

            for cold in coldkeys {
                Alpha::<T>::remove((&hot, &cold, netuid));
                AlphaV2::<T>::remove((&hot, &cold, netuid));
                AlphaShareEpoch::<T>::remove((&hot, &cold, netuid));
            }

            if exhausted {
                read_all = false;
                break;
            }
        }

        (read_all, last_completed_key)
    }

    pub fn destroy_alpha_in_out_stakes_clear_hotkey_totals(
        netuid: NetUid,
        weight_meter: &mut WeightMeter,
        last_key: Option<Vec<u8>>,
    ) -> (bool, Option<Vec<u8>>) {
        let iter_read = T::DbWeight::get().reads(1);
        // Updating TotalAlphaStaked reads and writes the aggregate; clearing a
        // hotkey also removes TotalHotkeyAlpha, both share-denominator maps and
        // the share-pool epoch.
        let removal_weight = T::DbWeight::get().reads_writes(1, 5);
        let iter = match last_key {
            Some(key) => TotalHotkeyAlpha::<T>::iter_from(key),
            None => TotalHotkeyAlpha::<T>::iter(),
        };

        let mut read_all = true;
        let mut last_item = None;
        let mut to_remove = Vec::new();

        for item @ (_, this_netuid, _) in iter {
            if !weight_meter.can_consume(iter_read) {
                read_all = false;
                break;
            }
            weight_meter.consume(iter_read);

            if this_netuid == netuid {
                if !weight_meter.can_consume(removal_weight) {
                    read_all = false;
                    break;
                }
                weight_meter.consume(removal_weight);
                to_remove.push((item.0.clone(), item.2));
            }
            last_item = Some(item);
        }

        for (hotkey, alpha) in to_remove {
            crate::migrations::migrate_total_alpha_staked::apply_live_delta::<T>(
                &hotkey,
                netuid,
                alpha,
                AlphaBalance::ZERO,
            );
            TotalHotkeyAlpha::<T>::remove(&hotkey, netuid);
            TotalHotkeyShares::<T>::remove(&hotkey, netuid);
            TotalHotkeySharesV2::<T>::remove(&hotkey, netuid);
            AlphaSharePoolEpoch::<T>::remove(&hotkey, netuid);
        }

        (
            read_all,
            last_item.map(|(hotkey, nu, _)| TotalHotkeyAlpha::<T>::hashed_key_for(&hotkey, nu)),
        )
    }

    pub fn destroy_alpha_in_out_stakes_clear_locks(
        netuid: NetUid,
        weight_meter: &mut WeightMeter,
        last_key: Option<Vec<u8>>,
    ) -> (bool, Option<Vec<u8>>) {
        let iter = match last_key {
            Some(key) => Lock::<T>::iter_from(key),
            None => Lock::<T>::iter(),
        };

        let (read_all, last_item) = Self::remove_storage_entries_for_netuid(
            weight_meter,
            iter,
            |((_, this_netuid, _), _)| *this_netuid == netuid,
            |((coldkey, _this_netuid, hotkey), _)| (coldkey, hotkey),
            |(coldkey, hotkey)| Lock::<T>::remove((coldkey.clone(), netuid, hotkey.clone())),
            1,
        );

        (
            read_all,
            last_item.map(|((coldkey, _, hotkey), _)| {
                Lock::<T>::hashed_key_for((&coldkey, netuid, &hotkey))
            }),
        )
    }

    pub fn destroy_alpha_in_out_stakes_clear_decaying_locks(
        netuid: NetUid,
        weight_meter: &mut WeightMeter,
        last_key: Option<Vec<u8>>,
    ) -> (bool, Option<Vec<u8>>) {
        let iter = match last_key {
            Some(key) => DecayingLock::<T>::iter_from(key),
            None => DecayingLock::<T>::iter(),
        };

        let (read_all, last_item) = Self::remove_storage_entries_for_netuid(
            weight_meter,
            iter,
            |(_, this_netuid, _)| *this_netuid == netuid,
            |(coldkey, _, _)| coldkey,
            |coldkey| DecayingLock::<T>::remove(coldkey, netuid),
            1,
        );

        (
            read_all,
            last_item.map(|(coldkey, nu, _)| DecayingLock::<T>::hashed_key_for(&coldkey, nu)),
        )
    }
}
