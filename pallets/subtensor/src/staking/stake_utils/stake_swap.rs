//! AMM stake swaps (TAO↔alpha), `stake_into_subnet` / `unstake_from_subnet`, and within-subnet transfers.
use super::*;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{AlphaBalance, AuthorshipInfo, NetUid, TaoBalance, Token};
use subtensor_swap_interface::{Order, SwapHandler, SwapResult};

impl<T: Config> Pallet<T> {
    /// Swaps TAO for the alpha token on the subnet.
    ///
    /// Updates TaoIn, AlphaIn, and AlphaOut
    pub fn swap_tao_for_alpha(
        netuid: NetUid,
        tao: TaoBalance,
        price_limit: TaoBalance,
        drop_fees: bool,
    ) -> Result<SwapResult<TaoBalance, AlphaBalance>, DispatchError> {
        // Step 1: Get the mechanism type for the subnet (0 for Stable, 1 for Dynamic)
        let mechanism_id: u16 = SubnetMechanism::<T>::get(netuid);
        let swap_result = if mechanism_id == 1 {
            let order = GetAlphaForTao::<T>::with_amount(tao);
            T::SwapInterface::swap(netuid.into(), order, price_limit.into(), drop_fees, false)?
        } else {
            // Step 3.b.1: Stable mechanism, just return the value 1:1
            SwapResult {
                amount_paid_in: tao,
                amount_paid_out: tao.to_u64().into(),
                fee_paid: TaoBalance::ZERO,
                fee_to_block_author: TaoBalance::ZERO,
            }
        };

        let alpha_decrease = swap_result.paid_out_reserve_delta_i64().unsigned_abs();

        // Decrease Alpha reserves.
        Self::decrease_provided_alpha_reserve(netuid.into(), alpha_decrease.into());

        // Increase Alpha outstanding.
        SubnetAlphaOut::<T>::mutate(netuid, |total| {
            *total = total.saturating_add(swap_result.amount_paid_out.into());
        });

        // Increase the protocol TAO reserve
        SubnetTAO::<T>::mutate(netuid, |total| {
            let delta = swap_result.paid_in_reserve_delta_i64().unsigned_abs();
            *total = total.saturating_add(delta.into());
        });

        // Increase Total Tao reserves.
        TotalStake::<T>::mutate(|total| *total = total.saturating_add(tao));

        // Increase total subnet TAO volume.
        SubnetVolume::<T>::mutate(netuid, |total| {
            *total = total.saturating_add(tao.to_u64() as u128);
        });

        Ok(swap_result)
    }

    /// Swaps a subnet's Alpha token for TAO.
    ///
    /// Updates TaoIn, AlphaIn, and AlphaOut
    pub fn swap_alpha_for_tao(
        netuid: NetUid,
        alpha: AlphaBalance,
        price_limit: TaoBalance,
        drop_fees: bool,
    ) -> Result<SwapResult<AlphaBalance, TaoBalance>, DispatchError> {
        // Step 1: Get the mechanism type for the subnet (0 for Stable, 1 for Dynamic)
        let mechanism_id: u16 = SubnetMechanism::<T>::get(netuid);
        // Step 2: Swap alpha and attain tao
        let swap_result = if mechanism_id == 1 {
            let order = GetTaoForAlpha::<T>::with_amount(alpha);
            T::SwapInterface::swap(netuid.into(), order, price_limit.into(), drop_fees, false)?
        } else {
            // Step 3.b.1: Stable mechanism, just return the value 1:1
            SwapResult {
                amount_paid_in: alpha,
                amount_paid_out: alpha.to_u64().into(),
                fee_paid: AlphaBalance::ZERO,
                fee_to_block_author: AlphaBalance::ZERO,
            }
        };

        // Increase only the protocol Alpha reserve
        let alpha_delta = swap_result.paid_in_reserve_delta_i64().unsigned_abs();
        SubnetAlphaIn::<T>::mutate(netuid, |total| {
            *total = total.saturating_add(alpha_delta.into());
        });

        // Decrease Alpha outstanding.
        SubnetAlphaOut::<T>::mutate(netuid, |total| {
            *total = total.saturating_sub(alpha_delta.into());
        });

        // Decrease tao reserves.
        let tao_delta = swap_result.paid_out_reserve_delta_i64().unsigned_abs();
        Self::decrease_provided_tao_reserve(netuid.into(), tao_delta.into());

        // Reduce total TAO reserves.
        TotalStake::<T>::mutate(|total| *total = total.saturating_sub(swap_result.amount_paid_out));

        // Increase total subnet TAO volume.
        SubnetVolume::<T>::mutate(netuid, |total| {
            *total = total.saturating_add(swap_result.amount_paid_out.to_u64() as u128)
        });

        // Return the tao received.
        Ok(swap_result)
    }

    /// Unstakes alpha from a subnet for a given hotkey and coldkey pair.
    ///
    /// We update the pools associated with a subnet as well as update hotkey alpha shares.
    /// Credits the unstaked TAO to the beneficiary account
    pub fn unstake_from_subnet(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        beneficiary: &T::AccountId,
        netuid: NetUid,
        alpha: AlphaBalance,
        price_limit: TaoBalance,
        drop_fees: bool,
    ) -> Result<TaoBalance, DispatchError> {
        // Refuse to strip conviction-locked or collateral-bonded alpha even when
        // callers (e.g. alpha fee withdrawal) skip the remove-stake validators.
        Self::ensure_available_to_unstake(coldkey, netuid, alpha)?;
        Self::ensure_hotkey_covers_collateral(coldkey, hotkey, netuid, alpha)?;

        //  Decrease alpha on subnet
        Self::decrease_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, netuid, alpha);

        // Swap the alpha for TAO.
        let swap_result = Self::swap_alpha_for_tao(netuid, alpha, price_limit, drop_fees)?;

        // Refund the unused alpha (in case if limit price is hit)
        let refund = alpha.saturating_sub(
            swap_result
                .amount_paid_in
                .saturating_add(swap_result.fee_paid)
                .into(),
        );
        if !refund.is_zero() {
            Self::increase_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, netuid, refund);
        }

        // Transfer unstaked TAO from subnet account to the coldkey.
        Self::transfer_tao_from_subnet(netuid, beneficiary, swap_result.amount_paid_out.into())?;

        // Swap (in a fee-less way) the block builder alpha fee
        let mut fee_outflow = 0_u64;
        let maybe_block_author_coldkey = T::AuthorshipProvider::author();
        if let Some(block_author_coldkey) = maybe_block_author_coldkey {
            let bb_swap_result = Self::swap_alpha_for_tao(
                netuid,
                swap_result.fee_to_block_author,
                T::SwapInterface::min_price::<TaoBalance>(),
                true,
            )?;
            Self::transfer_tao_from_subnet(
                netuid,
                &block_author_coldkey,
                bb_swap_result.amount_paid_out.into(),
            )?;
            fee_outflow = bb_swap_result.amount_paid_out.into();
        } else {
            // block author is not found, burn this alpha
            Self::burn_subnet_alpha(netuid, swap_result.fee_to_block_author);
        }

        // If this is a root-stake
        if netuid == NetUid::ROOT {
            // Adjust root claimed value for this hotkey and coldkey.
            Self::remove_stake_adjust_root_claimed_for_hotkey_and_coldkey(hotkey, coldkey, alpha);

            // If the coldkey no longer holds any root stake, remove it from the
            // auto-claim staking-coldkey index so dead entries do not accumulate.
            if !Self::coldkey_has_root_stake(coldkey) {
                Self::maybe_remove_coldkey_index(coldkey);
            }
        }

        // Step 3: Update StakingHotkeys if the hotkey's total alpha, across all subnets, is zero
        // TODO const: fix.
        // if Self::get_stake(hotkey, coldkey) == 0 {
        //     StakingHotkeys::<T>::mutate(coldkey, |hotkeys| {
        //         hotkeys.retain(|k| k != hotkey);
        //     });
        // }

        // Record TAO outflow
        Self::record_tao_outflow(
            netuid,
            swap_result
                .amount_paid_out
                .saturating_add(fee_outflow.into()),
        );

        // Cleanup locks if needed
        Self::cleanup_lock_if_zero(coldkey, netuid);

        LastColdkeyHotkeyStakeBlock::<T>::insert(coldkey, hotkey, Self::get_current_block_as_u64());

        // Deposit and log the unstaking event.
        Self::deposit_event(Event::StakeRemoved(
            coldkey.clone(),
            hotkey.clone(),
            swap_result.amount_paid_out.into(),
            swap_result.amount_paid_in.into(),
            netuid,
            swap_result.fee_paid.to_u64(),
        ));

        log::debug!(
            "StakeRemoved( coldkey: {:?}, hotkey:{:?}, tao: {:?}, alpha:{:?}, netuid: {:?}, fee {} )",
            coldkey.clone(),
            hotkey.clone(),
            swap_result.amount_paid_out,
            swap_result.amount_paid_in,
            netuid,
            swap_result.fee_paid
        );

        Ok(swap_result.amount_paid_out.into())
    }

    /// Stakes TAO into a subnet for a given hotkey and coldkey pair.
    ///
    /// We update the pools associated with a subnet as well as update hotkey alpha shares.
    pub(crate) fn stake_into_subnet(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        netuid: NetUid,
        tao: TaoBalance,
        price_limit: TaoBalance,
        drop_fees: bool,
    ) -> Result<AlphaBalance, DispatchError> {
        // Transfer TAO from coldkey to the subnet account.
        // Actual transfered may be different within ED amount.
        let tao_staked = Self::transfer_tao_to_subnet(netuid, coldkey, tao)?;

        // Swap the tao to alpha.
        let swap_result = Self::swap_tao_for_alpha(netuid, tao_staked, price_limit, drop_fees)?;

        ensure!(
            !swap_result.amount_paid_out.is_zero(),
            Error::<T>::AmountTooLow
        );

        ensure!(
            Self::try_increase_stake_for_hotkey_and_coldkey_on_subnet(
                hotkey,
                netuid,
                swap_result.amount_paid_out.into(),
            ),
            Error::<T>::InsufficientLiquidity
        );

        // Increase the alpha on the hotkey account.
        Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
            hotkey,
            coldkey,
            netuid,
            swap_result.amount_paid_out.into(),
        );

        // Step 4: Update the list of hotkeys staking for this coldkey
        let mut staking_hotkeys = StakingHotkeys::<T>::get(coldkey);
        if !staking_hotkeys.contains(hotkey) {
            staking_hotkeys.push(hotkey.clone());
            StakingHotkeys::<T>::insert(coldkey, staking_hotkeys.clone());
        }

        // Increase the balance of the block author
        let maybe_block_author_coldkey = T::AuthorshipProvider::author();
        if let Some(block_author_coldkey) = maybe_block_author_coldkey {
            // TAO was transferred to subnet account in the beginning of this fn
            // swap_tao_for_alpha guarantees that input amount of TAO was split into
            // reserve delta + fee_to_block_author.
            // Now transfer the fee from subnet account to block builder.
            Self::transfer_tao_from_subnet(
                netuid,
                &block_author_coldkey,
                swap_result.fee_to_block_author.into(),
            )?;
        } else {
            // Block author is not found - burn this TAO
            if let Some(subnet_account_id) = Self::get_subnet_account_id(netuid) {
                let _ = Self::burn_tao(&subnet_account_id, swap_result.fee_to_block_author.into());
            }
        }

        // Refund the TAO the AMM could not consume (e.g. when the user-supplied
        // price limit is hit before the full `tao_staked` is swapped). Without
        // this, the unswapped remainder is stranded on the subnet PalletId
        // account. Mirrors the alpha refund in `unstake_from_subnet`.
        let consumed_tao = swap_result
            .amount_paid_in
            .saturating_add(swap_result.fee_paid);
        let refund_tao = tao_staked.saturating_sub(consumed_tao);
        if !refund_tao.is_zero() {
            Self::transfer_tao_from_subnet(netuid, coldkey, refund_tao)?;
            // `swap_tao_for_alpha` bumped `TotalStake` by the full `tao_staked`;
            // only `consumed_tao` actually became stake, so back out the refund.
            TotalStake::<T>::mutate(|total| *total = total.saturating_sub(refund_tao));
        }

        // Record TAO inflow
        Self::record_tao_inflow(netuid, swap_result.amount_paid_in.into());

        // Cleanup locks if needed
        Self::cleanup_lock_if_zero(coldkey, netuid);

        LastColdkeyHotkeyStakeBlock::<T>::insert(coldkey, hotkey, Self::get_current_block_as_u64());

        // If this is a root-stake
        if netuid == NetUid::ROOT {
            // Adjust root claimed for this hotkey and coldkey.
            let alpha = swap_result.amount_paid_out.into();
            Self::add_stake_adjust_root_claimed_for_hotkey_and_coldkey(hotkey, coldkey, alpha);
            Self::maybe_add_coldkey_index(coldkey);
        }

        // Deposit and log the staking event.
        Self::deposit_event(Event::StakeAdded(
            coldkey.clone(),
            hotkey.clone(),
            tao_staked,
            swap_result.amount_paid_out.into(),
            netuid,
            swap_result.fee_paid.to_u64(),
        ));

        log::debug!(
            "StakeAdded( coldkey: {:?}, hotkey:{:?}, tao: {:?}, alpha:{:?}, netuid: {:?}, fee {} )",
            coldkey.clone(),
            hotkey.clone(),
            tao_staked,
            swap_result.amount_paid_out,
            netuid,
            swap_result.fee_paid,
        );

        Ok(swap_result.amount_paid_out.into())
    }

    /// Transfers stake between coldkeys and/or hotkey within one subnet without running it
    /// through swap.
    ///
    /// Does not incur any swapping nor fees
    pub fn transfer_stake_within_subnet(
        origin_coldkey: &T::AccountId,
        origin_hotkey: &T::AccountId,
        destination_coldkey: &T::AccountId,
        destination_hotkey: &T::AccountId,
        netuid: NetUid,
        alpha: AlphaBalance,
    ) -> Result<TaoBalance, DispatchError> {
        // Transfer lock (may fail if destination coldkey has a conflicting lock).
        // The lock must follow the stake to the destination hotkey, otherwise a
        // hotkey-changing transfer would leave the recipient's lock and conviction
        // stranded on the origin hotkey.
        Self::transfer_lock(
            origin_coldkey,
            destination_coldkey,
            destination_hotkey,
            netuid,
            alpha,
        )?;

        // Decrease alpha on origin keys
        Self::decrease_stake_for_hotkey_and_coldkey_on_subnet(
            origin_hotkey,
            origin_coldkey,
            netuid,
            alpha,
        );
        if netuid == NetUid::ROOT {
            Self::remove_stake_adjust_root_claimed_for_hotkey_and_coldkey(
                origin_hotkey,
                origin_coldkey,
                alpha,
            );
        }

        // If the destination coldkey does not own the destination hotkey, make the
        // hotkey a delegate, matching the cross-subnet transfer path.
        if Self::get_owning_coldkey_for_hotkey(destination_hotkey) != *destination_coldkey {
            Self::maybe_become_delegate(destination_hotkey);
        }

        // Increase alpha on destination keys
        Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
            destination_hotkey,
            destination_coldkey,
            netuid,
            alpha,
        );
        if netuid == NetUid::ROOT {
            Self::add_stake_adjust_root_claimed_for_hotkey_and_coldkey(
                destination_hotkey,
                destination_coldkey,
                u64::from(alpha).into(),
            );
        }

        // Calculate TAO equivalent based on current price (it is accurate because
        // there's no slippage in this move)
        let current_price =
            <T as pallet::Config>::SwapInterface::current_alpha_price(netuid.into());
        let tao_equivalent: TaoBalance = current_price
            .saturating_mul(U64F64::saturating_from_num(alpha))
            .saturating_to_num::<u64>()
            .into();

        // Ensure tao_equivalent is above the minimum transfer amount
        ensure!(
            tao_equivalent >= DefaultMinTransfer::<T>::get(),
            Error::<T>::AmountTooLow
        );

        // Step 3: Update StakingHotkeys if the hotkey's total alpha, across all subnets, is zero
        // TODO: fix.
        // if Self::get_stake(hotkey, coldkey) == 0 {
        //     StakingHotkeys::<T>::mutate(coldkey, |hotkeys| {
        //         hotkeys.retain(|k| k != hotkey);
        //     });
        // }

        LastColdkeyHotkeyStakeBlock::<T>::insert(
            destination_coldkey,
            destination_hotkey,
            Self::get_current_block_as_u64(),
        );

        // Deposit and log the unstaking event.
        Self::deposit_event(Event::StakeRemoved(
            origin_coldkey.clone(),
            origin_hotkey.clone(),
            tao_equivalent,
            alpha,
            netuid,
            0_u64, // 0 fee
        ));
        Self::deposit_event(Event::StakeAdded(
            destination_coldkey.clone(),
            destination_hotkey.clone(),
            tao_equivalent,
            alpha,
            netuid,
            0_u64, // 0 fee
        ));

        Ok(tao_equivalent)
    }
}
