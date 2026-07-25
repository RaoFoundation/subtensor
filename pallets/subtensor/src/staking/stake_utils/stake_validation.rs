//! Pre-flight validation for add / remove / unstake-all / stake-transition extrinsics.
use super::*;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};
use subtensor_swap_interface::{Order, SwapHandler};

impl<T: Config> Pallet<T> {
    /// Validate add_stake user input
    pub fn validate_add_stake(
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        netuid: NetUid,
        mut stake_to_be_added: TaoBalance,
        max_amount: TaoBalance,
        allow_partial: bool,
    ) -> Result<(), Error<T>> {
        // Ensure that the subnet exists.
        ensure!(Self::subnet_exists(netuid), Error::<T>::SubnetNotExists);

        // Ensure that the subnet is enabled.
        Self::ensure_subtoken_enabled(netuid)?;

        // Get the minimum balance (and amount) that satisfies the transaction
        let min_stake = DefaultMinStake::<T>::get();
        let min_amount = {
            let order = GetAlphaForTao::<T>::with_amount(min_stake);
            let fee = T::SwapInterface::sim_swap(netuid.into(), order)
                .map(|res| res.fee_paid)
                .unwrap_or(T::SwapInterface::approx_fee_amount(
                    netuid.into(),
                    min_stake.into(),
                ));
            min_stake.saturating_add(fee.into())
        };

        // Ensure that the stake_to_be_added is at least the min_amount
        ensure!(stake_to_be_added >= min_amount, Error::<T>::AmountTooLow);

        // Ensure that if partial execution is not allowed, the amount will not cause
        // slippage over desired
        if !allow_partial {
            ensure!(stake_to_be_added <= max_amount, Error::<T>::SlippageTooHigh);
        } else {
            stake_to_be_added = max_amount.min(stake_to_be_added);
        }

        // Ensure the callers coldkey has enough stake to perform the transaction.
        ensure!(
            Self::can_remove_balance_from_coldkey_account(coldkey, stake_to_be_added.into()),
            Error::<T>::NotEnoughBalanceToStake
        );

        // Ensure that the hotkey account exists this is only possible through registration.
        ensure!(
            Self::hotkey_account_exists(hotkey),
            Error::<T>::HotKeyAccountNotExists
        );

        let order = GetAlphaForTao::<T>::with_amount(stake_to_be_added);
        let swap_result = T::SwapInterface::sim_swap(netuid.into(), order)
            .map_err(|_| Error::<T>::InsufficientLiquidity)?;

        // Check that actual withdrawn TAO amount is not lower than the minimum stake
        ensure!(
            swap_result.amount_paid_in >= min_stake,
            Error::<T>::AmountTooLow
        );

        ensure!(
            !swap_result.amount_paid_out.is_zero(),
            Error::<T>::InsufficientLiquidity
        );

        // Ensure hotkey pool is precise enough
        let try_stake_result = Self::try_increase_stake_for_hotkey_and_coldkey_on_subnet(
            hotkey,
            netuid,
            swap_result.amount_paid_out.into(),
        );
        ensure!(try_stake_result, Error::<T>::InsufficientLiquidity);

        Ok(())
    }

    /// Validate remove_stake user input
    ///
    pub fn validate_remove_stake(
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        netuid: NetUid,
        alpha_unstaked: AlphaBalance,
        max_amount: AlphaBalance,
        allow_partial: bool,
    ) -> Result<(), Error<T>> {
        // Ensure that the subnet exists.
        ensure!(Self::subnet_exists(netuid), Error::<T>::SubnetNotExists);

        // Ensure that the subnet is enabled.
        // Self::ensure_subtoken_enabled(netuid)?;

        // Do not allow zero unstake amount
        ensure!(!alpha_unstaked.is_zero(), Error::<T>::AmountTooLow);

        // Ensure that the stake amount to be removed is above the minimum in tao equivalent.
        // Bypass this check if the user unstakes full amount
        let remaining_alpha_stake =
            Self::calculate_reduced_stake_on_subnet(hotkey, coldkey, netuid, alpha_unstaked)?;
        let order = GetTaoForAlpha::<T>::with_amount(alpha_unstaked);
        match T::SwapInterface::sim_swap(netuid.into(), order) {
            Ok(res) => {
                if !remaining_alpha_stake.is_zero() {
                    ensure!(
                        res.amount_paid_out >= DefaultMinStake::<T>::get(),
                        Error::<T>::AmountTooLow
                    );
                }
            }
            Err(_) => return Err(Error::<T>::InsufficientLiquidity),
        }

        // Ensure that if partial execution is not allowed, the amount will not cause
        // slippage over desired
        if !allow_partial {
            ensure!(alpha_unstaked <= max_amount, Error::<T>::SlippageTooHigh);
        }

        // Ensure that the hotkey account exists this is only possible through registration.
        ensure!(
            Self::hotkey_account_exists(hotkey),
            Error::<T>::HotKeyAccountNotExists
        );

        // Ensure that unstaked amount is not greater than available to unstake (due to locks)
        Self::ensure_available_to_unstake(coldkey, netuid, alpha_unstaked)?;
        // Collateral is per-hotkey: free stake on a sibling hotkey must not cover
        // stripping the bonded position.
        Self::ensure_hotkey_covers_collateral(coldkey, hotkey, netuid, alpha_unstaked)?;

        Ok(())
    }

    /// Validate if unstake_all can be executed
    ///
    pub fn validate_unstake_all(
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        only_alpha: bool,
    ) -> Result<(), Error<T>> {
        // Get all netuids (filter out root)
        let subnets = Self::get_all_subnet_netuids();

        // Ensure that the hotkey account exists this is only possible through registration.
        ensure!(
            Self::hotkey_account_exists(hotkey),
            Error::<T>::HotKeyAccountNotExists
        );

        let mut unstaking_any = false;
        for netuid in subnets.iter() {
            if only_alpha && netuid.is_root() {
                continue;
            }

            // Get user's stake in this subnet
            let alpha = Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, *netuid);

            // Ensure that unstaked amount is not greater than available to unstake (due to locks)
            // for this subnet.
            Self::ensure_available_to_unstake(coldkey, *netuid, alpha)?;

            if Self::validate_remove_stake(coldkey, hotkey, *netuid, alpha, alpha, false).is_ok() {
                unstaking_any = true;
            }
        }

        // If no unstaking happens, return error
        ensure!(unstaking_any, Error::<T>::AmountTooLow);

        Ok(())
    }

    /// Validate stake transition user input
    /// That works for move_stake, transfer_stake, and swap_stake
    ///
    pub fn validate_stake_transition(
        origin_coldkey: &T::AccountId,
        destination_coldkey: &T::AccountId,
        origin_hotkey: &T::AccountId,
        destination_hotkey: &T::AccountId,
        origin_netuid: NetUid,
        destination_netuid: NetUid,
        alpha_amount: AlphaBalance,
        max_amount: AlphaBalance,
        maybe_allow_partial: Option<bool>,
        check_transfer_toggle: bool,
    ) -> Result<(), Error<T>> {
        // Ensure stake transition is actually happening
        if origin_coldkey == destination_coldkey && origin_hotkey == destination_hotkey {
            ensure!(origin_netuid != destination_netuid, Error::<T>::SameNetuid);
        }

        // Ensure that both subnets exist.
        ensure!(
            Self::subnet_exists(origin_netuid),
            Error::<T>::SubnetNotExists
        );
        if origin_netuid != destination_netuid {
            ensure!(
                Self::subnet_exists(destination_netuid),
                Error::<T>::SubnetNotExists
            );
        }

        ensure!(
            SubtokenEnabled::<T>::get(origin_netuid),
            Error::<T>::SubtokenDisabled
        );

        ensure!(
            SubtokenEnabled::<T>::get(destination_netuid),
            Error::<T>::SubtokenDisabled
        );

        // Ensure that the origin hotkey account exists
        ensure!(
            Self::hotkey_account_exists(origin_hotkey),
            Error::<T>::HotKeyAccountNotExists
        );

        // Ensure that the destination hotkey account exists
        ensure!(
            Self::hotkey_account_exists(destination_hotkey),
            Error::<T>::HotKeyAccountNotExists
        );

        // Ensure there is enough stake in the origin subnet.
        let origin_alpha = Self::get_stake_for_hotkey_and_coldkey_on_subnet(
            origin_hotkey,
            origin_coldkey,
            origin_netuid,
        );
        ensure!(
            alpha_amount <= origin_alpha,
            Error::<T>::NotEnoughStakeToWithdraw
        );

        // If origin and destination netuid are different, do the swap-related checks
        if origin_netuid != destination_netuid {
            // Ensure that the stake amount to be removed is above the minimum in tao equivalent.
            // Transfers (check_transfer_toggle == true) have their own minimum, detached from
            // the staking minimum used by moves and swaps.
            let min_amount = if check_transfer_toggle {
                DefaultMinTransfer::<T>::get()
            } else {
                DefaultMinStake::<T>::get()
            };
            let order = GetTaoForAlpha::<T>::with_amount(alpha_amount);
            let tao_equivalent = T::SwapInterface::sim_swap(origin_netuid.into(), order)
                .map(|res| res.amount_paid_out)
                .map_err(|_| Error::<T>::InsufficientLiquidity)?;
            ensure!(tao_equivalent > min_amount, Error::<T>::AmountTooLow);

            // Ensure that if partial execution is not allowed, the amount will not cause
            // slippage over desired
            if let Some(allow_partial) = maybe_allow_partial
                && !allow_partial
            {
                ensure!(alpha_amount <= max_amount, Error::<T>::SlippageTooHigh);
            }
        }

        if check_transfer_toggle {
            // Ensure transfer is toggled.
            ensure!(
                TransferToggle::<T>::get(origin_netuid),
                Error::<T>::TransferDisallowed
            );
            if origin_netuid != destination_netuid {
                ensure!(
                    TransferToggle::<T>::get(destination_netuid),
                    Error::<T>::TransferDisallowed
                );
            }
        }

        // Enforce lock invariant: if the is cross-subnet move, the remaining amount must
        // cover the lock.
        if origin_netuid != destination_netuid {
            Self::ensure_available_to_unstake(origin_coldkey, origin_netuid, alpha_amount)?;
        } else if origin_coldkey != destination_coldkey {
            // Same-subnet, ownership-changing transfer. Conviction locks follow the
            // stake to the destination coldkey via `transfer_lock`, but miner
            // registration collateral has no transfer exit and does not follow — its
            // `MinerCollateral(netuid, hotkey, coldkey)` stays on the origin. Without
            // this check, a coldkey could liberate locked collateral by transferring the
            // staked alpha to a second coldkey. Require the origin coldkey to retain
            // enough alpha on the subnet to still cover its collateral.
            Self::ensure_transfer_respects_collateral(origin_coldkey, origin_netuid, alpha_amount)?;
        }
        // Always keep bonded alpha on the origin hotkey itself (same-subnet moves
        // to a sibling hotkey would otherwise leave a ghost metagraph bond).
        Self::ensure_hotkey_covers_collateral(
            origin_coldkey,
            origin_hotkey,
            origin_netuid,
            alpha_amount,
        )?;

        Ok(())
    }
}
