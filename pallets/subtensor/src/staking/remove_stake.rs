use super::*;
use substrate_fixed::types::U96F32;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};
use subtensor_swap_interface::{Order, SwapHandler};

impl<T: Config> Pallet<T> {
    /// ---- The implementation for the extrinsic remove_stake: Removes stake from a hotkey account and adds it onto a coldkey.
    ///
    /// # Args:
    /// * 'origin': (<T as frame_system::Config>RuntimeOrigin):
    ///     -  The signature of the caller's coldkey.
    ///
    /// * 'hotkey' (T::AccountId):
    ///     -  The associated hotkey account.
    ///
    /// * 'netuid' (u16):
    ///     - Subnetwork UID
    ///
    /// * 'alpha_unstaked' (Alpha):
    ///     -  The amount of stake to be removed from the staking account.
    ///
    /// # Event:
    /// * StakeRemoved;
    ///     -  On the successfully removing stake from the hotkey account.
    ///
    /// # Raises:
    /// * 'NotRegistered':
    ///     -  Thrown if the account we are attempting to unstake from is non existent.
    ///
    /// * 'NonAssociatedColdKey':
    ///     -  Thrown if the coldkey does not own the hotkey we are unstaking from.
    ///
    /// * 'NotEnoughStakeToWithdraw':
    ///     -  Thrown if there is not enough stake on the hotkey to withdwraw this amount.
    ///
    /// * 'TxRateLimitExceeded':
    ///     -  Thrown if key has hit transaction rate limit
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
        )?;

        // 5. If the stake is below the minimum, we clear the nomination from storage.
        Self::clear_small_nomination_if_required(&hotkey, &coldkey, netuid);

        // 6. Check if stake lowered below MinStake and remove Pending children if it did
        if Self::get_total_stake_for_hotkey(&hotkey) < StakeThreshold::<T>::get().into() {
            Self::get_all_subnet_netuids().iter().for_each(|netuid| {
                PendingChildKeys::<T>::remove(netuid, &hotkey);
            })
        }

        // Done and ok.
        Ok(())
    }

    /// ---- The implementation for the extrinsic unstake_all: Removes all stake from a hotkey account across all subnets and adds it onto a coldkey.
    ///
    /// # Args:
    /// * 'origin': (<T as frame_system::Config>RuntimeOrigin):
    ///     -  The signature of the caller's coldkey.
    ///
    /// * 'hotkey' (T::AccountId):
    ///     -  The associated hotkey account.
    ///
    /// # Event:
    /// * StakeRemoved;
    ///     -  On the successfully removing stake from the hotkey account.
    ///
    /// # Raises:
    /// * 'NotRegistered':
    ///     -  Thrown if the account we are attempting to unstake from is non existent.
    ///
    /// * 'NonAssociatedColdKey':
    ///     -  Thrown if the coldkey does not own the hotkey we are unstaking from.
    ///
    /// * 'NotEnoughStakeToWithdraw':
    ///     -  Thrown if there is not enough stake on the hotkey to withdraw this amount.
    ///
    /// * 'TxRateLimitExceeded':
    ///     -  Thrown if key has hit transaction rate limit
    ///
    pub fn do_unstake_all(origin: OriginFor<T>, hotkey: T::AccountId) -> dispatch::DispatchResult {
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

        // 4. Iterate through all subnets and remove stake.
        for netuid in netuids.into_iter() {
            if !SubtokenEnabled::<T>::get(netuid) {
                continue;
            }
            // Ensure that the hotkey has enough stake to withdraw.
            let alpha_unstaked =
                Self::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);

            if Self::validate_remove_stake(
                &coldkey,
                &hotkey,
                netuid,
                alpha_unstaked,
                alpha_unstaked,
                false,
            )
            .is_err()
            {
                // Don't unstake from this netuid
                continue;
            }

            if !alpha_unstaked.is_zero() {
                // Swap the alpha to tao and update counters for this subnet.
                Self::unstake_from_subnet(
                    &hotkey,
                    &coldkey,
                    &coldkey,
                    netuid,
                    alpha_unstaked,
                    T::SwapInterface::min_price(),
                    false,
                )?;

                // If the stake is below the minimum, we clear the nomination from storage.
                Self::clear_small_nomination_if_required(&hotkey, &coldkey, netuid);
            }
        }

        // 5. Done and ok.
        Ok(())
    }

    /// ---- The implementation for the extrinsic unstake_all: Removes all stake from a hotkey account across all subnets and adds it onto a coldkey.
    ///
    /// # Args:
    /// * 'origin': (<T as frame_system::Config>RuntimeOrigin):
    ///     -  The signature of the caller's coldkey.
    ///
    /// * 'hotkey' (T::AccountId):
    ///     -  The associated hotkey account.
    ///
    /// # Event:
    /// * StakeRemoved;
    ///     -  On the successfully removing stake from the hotkey account.
    ///
    /// # Raises:
    /// * 'NotRegistered':
    ///     -  Thrown if the account we are attempting to unstake from is non existent.
    ///
    /// * 'NonAssociatedColdKey':
    ///     -  Thrown if the coldkey does not own the hotkey we are unstaking from.
    ///
    /// * 'NotEnoughStakeToWithdraw':
    ///     -  Thrown if there is not enough stake on the hotkey to withdraw this amount.
    ///
    /// * 'TxRateLimitExceeded':
    ///     -  Thrown if key has hit transaction rate limit
    ///
    pub fn do_unstake_all_alpha(
        origin: OriginFor<T>,
        hotkey: T::AccountId,
    ) -> dispatch::DispatchResult {
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

        // 4. Iterate through all subnets and remove stake.
        let mut total_tao_unstaked = TaoBalance::ZERO;
        for netuid in netuids.into_iter() {
            if !SubtokenEnabled::<T>::get(netuid) {
                continue;
            }
            // If not Root network.
            if !netuid.is_root() {
                // Ensure that the hotkey has enough stake to withdraw.
                let alpha_unstaked =
                    Self::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);

                if Self::validate_remove_stake(
                    &coldkey,
                    &hotkey,
                    netuid,
                    alpha_unstaked,
                    alpha_unstaked,
                    false,
                )
                .is_err()
                {
                    // Don't unstake from this netuid
                    continue;
                }

                if !alpha_unstaked.is_zero() {
                    // Swap the alpha to tao and update counters for this subnet.
                    let tao_unstaked = Self::unstake_from_subnet(
                        &hotkey,
                        &coldkey,
                        &coldkey,
                        netuid,
                        alpha_unstaked,
                        T::SwapInterface::min_price(),
                        false,
                    )?;

                    // Increment total
                    total_tao_unstaked = total_tao_unstaked.saturating_add(tao_unstaked);

                    // If the stake is below the minimum, we clear the nomination from storage.
                    Self::clear_small_nomination_if_required(&hotkey, &coldkey, netuid);
                }
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

        // 5. Done and ok.
        Ok(())
    }

    /// ---- The implementation for the extrinsic remove_stake_limit: Removes stake from
    /// a hotkey on a subnet with a price limit.
    ///
    /// In case if slippage occurs and the price shall move beyond the limit
    /// price, the staking order may execute only partially or not execute
    /// at all.
    ///
    /// # Args:
    /// * 'origin': (<T as frame_system::Config>Origin):
    ///     - The signature of the caller's coldkey.
    ///
    /// * 'hotkey' (T::AccountId):
    ///     - The associated hotkey account.
    ///
    /// * 'netuid' (u16):
    ///     - Subnetwork UID
    ///
    /// * 'amount_unstaked' (u64):
    ///     - The amount of stake to be added to the hotkey staking account.
    ///
    ///  * 'limit_price' (u64):
    ///     - The limit price expressed in units of RAO per one Alpha.
    ///
    ///  * 'allow_partial' (bool):
    ///     - Allows partial execution of the amount. If set to false, this becomes
    ///       fill or kill type or order.
    ///
    /// # Event:
    /// * StakeRemoved;
    ///     - On the successfully removing stake from the hotkey account.
    ///
    /// # Raises:
    /// * 'NotRegistered':
    ///     - Thrown if the account we are attempting to unstake from is non existent.
    ///
    /// * 'NonAssociatedColdKey':
    ///     - Thrown if the coldkey does not own the hotkey we are unstaking from.
    ///
    /// * 'NotEnoughStakeToWithdraw':
    ///     - Thrown if there is not enough stake on the hotkey to withdwraw this amount.
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
        )?;

        // 5. If the stake is below the minimum, we clear the nomination from storage.
        Self::clear_small_nomination_if_required(&hotkey, &coldkey, netuid);

        // 6. Check if stake lowered below MinStake and remove Pending children if it did
        if Self::get_total_stake_for_hotkey(&hotkey) < StakeThreshold::<T>::get().into() {
            Self::get_all_subnet_netuids().iter().for_each(|netuid| {
                PendingChildKeys::<T>::remove(netuid, &hotkey);
            })
        }

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
        let coldkey = ensure_signed(origin.clone())?;

        let alpha_unstaked =
            Self::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);

        if let Some(limit_price) = limit_price {
            Self::do_remove_stake_limit(origin, hotkey, netuid, alpha_unstaked, limit_price, false)
        } else {
            Self::do_remove_stake(origin, hotkey, netuid, alpha_unstaked)
        }
    }

    pub fn destroy_alpha_in_out_stakes(netuid: NetUid) -> DispatchResult {
        // 1) Ensure the subnet exists.
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);

        // 2) Owner / lock cost.
        let owner_coldkey: T::AccountId = SubnetOwner::<T>::get(netuid);
        let lock_cost: TaoBalance = Self::get_subnet_locked_balance(netuid);

        // Determine if this subnet is eligible for a lock refund (legacy).
        let reg_at: u64 = NetworkRegisteredAt::<T>::get(netuid);
        let start_block: u64 = NetworkRegistrationStartBlock::<T>::get();
        let should_refund_owner: bool = reg_at < start_block;

        // 3) Compute owner's received emission in TAO at current price (ONLY if we may refund).
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

        // 4) Enumerate all α entries on this subnet to build distribution weights and cleanup lists.
        //    - collect keys to remove,
        //    - per (hot,cold) α VALUE (not shares) with fallback to raw share if pool uninitialized,
        //    - track hotkeys to clear pool totals.
        let mut keys_to_remove: Vec<(T::AccountId, T::AccountId)> = Vec::new();
        let mut stakers: Vec<(T::AccountId, T::AccountId, u128)> = Vec::new();

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

        let mut total_alpha_value_u128: u128 = protocol_alpha_value_u128;
        let mut protocol_tao_share = TaoBalance::ZERO;

        let hotkeys_in_subnet: Vec<T::AccountId> = TotalHotkeyAlpha::<T>::iter_keys()
            .filter(|(_, this_netuid)| *this_netuid == netuid)
            .map(|(hot, _)| hot.clone())
            .collect::<Vec<_>>();

        for hot in hotkeys_in_subnet.iter() {
            for (cold, this_netuid, share_u64f64) in Self::alpha_iter_single_prefix(hot) {
                if this_netuid != netuid {
                    continue;
                }
                keys_to_remove.push((hot.clone(), cold.clone()));

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
                    stakers.push((hot.clone(), cold, val_u128));
                }
            }
        }

        // 5) Determine the TAO pot and pre-adjust accounting to avoid double counting.
        let pot_tao: TaoBalance = SubnetTAO::<T>::get(netuid);
        let pot_u64: u64 = pot_tao.into();

        if pot_u64 > 0 {
            SubnetTAO::<T>::remove(netuid);
            TotalStake::<T>::mutate(|total| *total = total.saturating_sub(pot_tao));
        }

        // 6) Pro‑rata distribution of the pot by α value (largest‑remainder),
        //    **credited directly to each staker's COLDKEY free balance**.
        if pot_u64 > 0 && total_alpha_value_u128 > 0 {
            struct Portion<A, C> {
                _hot: A,
                cold: C,
                is_protocol: bool,
                share: u64, // TAO to credit to coldkey balance
                rem: u128,  // remainder for largest‑remainder method
            }

            let pot_u128: u128 = pot_u64 as u128;
            let mut portions: Vec<Portion<_, _>> =
                Vec::with_capacity(stakers.len().saturating_add(1));
            let mut distributed: u128 = 0;

            for (hot, cold, alpha_val) in &stakers {
                let prod: u128 = pot_u128.saturating_mul(*alpha_val);
                let share_u128: u128 = prod.checked_div(total_alpha_value_u128).unwrap_or_default();
                let share_u64: u64 = share_u128.min(u128::from(u64::MAX)) as u64;
                distributed = distributed.saturating_add(u128::from(share_u64));

                let rem: u128 = prod.checked_rem(total_alpha_value_u128).unwrap_or_default();
                portions.push(Portion {
                    _hot: hot.clone(),
                    cold: cold.clone(),
                    is_protocol: false,
                    share: share_u64,
                    rem,
                });
            }

            if protocol_alpha_value_u128 > 0 {
                let prod: u128 = pot_u128.saturating_mul(protocol_alpha_value_u128);
                let share_u128: u128 = prod.checked_div(total_alpha_value_u128).unwrap_or_default();
                let share_u64: u64 = share_u128.min(u128::from(u64::MAX)) as u64;
                distributed = distributed.saturating_add(u128::from(share_u64));
                let rem: u128 = prod.checked_rem(total_alpha_value_u128).unwrap_or_default();
                portions.push(Portion {
                    _hot: owner_coldkey.clone(),
                    cold: owner_coldkey.clone(),
                    is_protocol: true,
                    share: share_u64,
                    rem,
                });
            }

            let leftover: u128 = pot_u128.saturating_sub(distributed);
            if leftover > 0 {
                portions.sort_by(|a, b| b.rem.cmp(&a.rem));
                let give: usize = core::cmp::min(leftover, portions.len() as u128) as usize;
                for p in portions.iter_mut().take(give) {
                    p.share = p.share.saturating_add(1);
                }
            }

            // Credit each share directly to coldkey free balance.
            for p in portions {
                if p.is_protocol {
                    protocol_tao_share = protocol_tao_share.saturating_add(p.share.into());
                } else if p.share > 0 {
                    // Cannot fail the whole transaction if this transfer fails
                    let _ = Self::transfer_tao_from_subnet(netuid, &p.cold, p.share.into());
                }
            }
        }

        // 7) Destroy all α-in/α-out state for this subnet.
        // 7.a) Remove every (hot, cold, netuid) α entry.
        for (hot, cold) in keys_to_remove {
            Alpha::<T>::remove((hot.clone(), cold.clone(), netuid));
            AlphaV2::<T>::remove((hot, cold, netuid));
        }
        // 7.b) Clear share‑pool totals for each hotkey on this subnet.
        for hot in hotkeys_in_subnet {
            TotalHotkeyAlpha::<T>::remove(&hot, netuid);
            TotalHotkeyShares::<T>::remove(&hot, netuid);
            TotalHotkeySharesV2::<T>::remove(&hot, netuid);
        }
        // 7.c) Remove α‑in/α‑out counters (fully destroyed).
        SubnetAlphaIn::<T>::remove(netuid);
        SubnetAlphaOut::<T>::remove(netuid);
        SubnetProtocolAlpha::<T>::remove(netuid);

        // Clear the locked balance on the subnet.
        Self::set_subnet_locked_balance(netuid, TaoBalance::ZERO);

        // 8) Finalize lock handling:
        //    - Legacy subnets (registered before NetworkRegistrationStartBlock) receive:
        //        refund = max(0, lock_cost(τ) − owner_received_emission_in_τ).
        //    - New subnets: no refund.
        let refund: TaoBalance = if should_refund_owner {
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

        // 10) Cleanup all subnet stake locks and lock aggregates if any.
        Self::destroy_lock_maps(netuid);

        Ok(())
    }
}
