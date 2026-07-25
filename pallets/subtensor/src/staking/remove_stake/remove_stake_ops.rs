//! Extrinsic bodies for `remove_stake`, `unstake_all`, and limit variants.
use super::*;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};
use subtensor_swap_interface::{Order, SwapHandler};

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
}
