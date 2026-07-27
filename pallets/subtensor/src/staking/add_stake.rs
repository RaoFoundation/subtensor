use subtensor_runtime_common::{NetUid, TaoBalance};
use subtensor_swap_interface::{Order, SwapHandler};

use super::*;

impl<T: Config> Pallet<T> {
    /// ---- The implementation for the extrinsic add_stake: Adds stake to a hotkey account.
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
    /// * 'stake_to_be_added' (u64):
    ///     -  The amount of stake to be added to the hotkey staking account.
    ///
    /// # Event:
    /// * StakeAdded;
    ///     -  On the successfully adding stake to a global account.
    ///
    /// # Raises:
    /// * 'NotEnoughBalanceToStake':
    ///     -  Not enough balance on the coldkey to add onto the global account.
    ///
    /// * 'NonAssociatedColdKey':
    ///     -  The calling coldkey is not associated with this hotkey.
    ///
    /// * 'BalanceWithdrawalError':
    ///     -  Errors stemming from transaction pallet.
    ///
    /// * 'TxRateLimitExceeded':
    ///     -  Thrown if key has hit transaction rate limit
    ///
    pub fn do_add_stake(
        origin: OriginFor<T>,
        hotkey: T::AccountId,
        netuid: NetUid,
        stake_to_be_added: TaoBalance,
    ) -> Result<AlphaBalance, DispatchError> {
        // 1. We check that the transaction is signed by the caller and retrieve the T::AccountId coldkey information.
        let coldkey = ensure_signed(origin)?;
        log::debug!(
            "do_add_stake( origin:{coldkey:?} hotkey:{hotkey:?}, netuid:{netuid:?}, stake_to_be_added:{stake_to_be_added:?} )"
        );

        Self::ensure_add_stake_input_within_swap_limit(netuid, stake_to_be_added)?;

        // 2. Validate user input
        Self::validate_add_stake(
            &coldkey,
            &hotkey,
            netuid,
            stake_to_be_added,
            stake_to_be_added,
            false,
        )?;

        // 3. Swap the stake into alpha on the subnet and increase counters.
        // Emit the staking event.
        Self::stake_into_subnet(
            &hotkey,
            &coldkey,
            netuid,
            stake_to_be_added,
            T::SwapInterface::max_price(),
            false,
        )
    }

    /// ---- The implementation for the extrinsic add_stake_limit: Adds stake to a hotkey
    /// account on a subnet with price limit.
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
    /// * 'stake_to_be_added' (u64):
    ///     -  The amount of stake to be added to the hotkey staking account.
    ///
    ///  * 'limit_price' (u64):
    ///     - The limit price expressed in units of RAO per one Alpha.
    ///
    ///  * 'allow_partial' (bool):
    ///     - Allows partial execution of the amount. If set to false, this becomes
    ///       fill or kill type or order.
    ///
    /// # Event:
    /// * StakeAdded;
    ///     -  On the successfully adding stake to a global account.
    ///
    /// # Raises:
    /// * 'NotEnoughBalanceToStake':
    ///     -  Not enough balance on the coldkey to add onto the global account.
    ///
    /// * 'NonAssociatedColdKey':
    ///     -  The calling coldkey is not associated with this hotkey.
    ///
    /// * 'BalanceWithdrawalError':
    ///     -  Errors stemming from transaction pallet.
    ///
    /// * 'TxRateLimitExceeded':
    ///     -  Thrown if key has hit transaction rate limit
    ///
    pub fn do_add_stake_limit(
        origin: OriginFor<T>,
        hotkey: T::AccountId,
        netuid: NetUid,
        stake_to_be_added: TaoBalance,
        limit_price: TaoBalance,
        allow_partial: bool,
    ) -> Result<AlphaBalance, DispatchError> {
        // 1. We check that the transaction is signed by the caller and retrieve the T::AccountId coldkey information.
        let coldkey = ensure_signed(origin)?;
        log::debug!(
            "do_add_stake( origin:{coldkey:?} hotkey:{hotkey:?}, netuid:{netuid:?}, stake_to_be_added:{stake_to_be_added:?} )"
        );

        Self::ensure_add_stake_input_within_swap_limit(netuid, stake_to_be_added)?;

        // 2. Calculate the maximum amount that can be executed with price limit
        let max_amount: TaoBalance = Self::get_max_amount_add(netuid, limit_price)?.into();
        let mut possible_stake = stake_to_be_added;
        if possible_stake > max_amount {
            possible_stake = max_amount;
        }

        // 3. Validate user input
        Self::validate_add_stake(
            &coldkey,
            &hotkey,
            netuid,
            stake_to_be_added,
            max_amount.into(),
            allow_partial,
        )?;

        // 4. If the coldkey is not the owner, make the hotkey a delegate.
        if Self::get_owning_coldkey_for_hotkey(&hotkey) != coldkey {
            Self::maybe_become_delegate(&hotkey);
        }

        // 5. Swap the stake into alpha on the subnet and increase counters.
        // Emit the staking event.
        Self::stake_into_subnet(
            &hotkey,
            &coldkey,
            netuid,
            possible_stake,
            limit_price,
            false,
        )
    }

    // Returns the maximum amount of RAO that can be executed with price limit
    pub fn get_max_amount_add(
        netuid: NetUid,
        limit_price: TaoBalance,
    ) -> Result<u64, DispatchError> {
        // Corner case: root and stao
        // There's no slippage for root or stable subnets, so if limit price is 1e9 rao or
        // higher, then max_amount equals u64::MAX, otherwise it is 0.
        if netuid.is_root() || SubnetMechanism::<T>::get(netuid) == 0 {
            if limit_price >= 1_000_000_000.into() {
                return Ok(u64::MAX);
            } else {
                // Price will never move down, so maximum amount that can be staked is zero
                return Ok(0_u64);
            }
        }

        // Use the largest supported input instead of probing the swap path with u64::MAX.
        let max_supported_input = SubnetTAO::<T>::get(netuid).saturating_mul(1_000.into());
        let order = GetAlphaForTao::<T>::with_amount(max_supported_input);
        let result = T::SwapInterface::swap(netuid.into(), order, limit_price, false, true)
            .map(|r| r.amount_paid_in.saturating_add(r.fee_paid))?;

        Ok(result.into())
    }

    fn ensure_add_stake_input_within_swap_limit(
        netuid: NetUid,
        amount: TaoBalance,
    ) -> Result<(), Error<T>> {
        if !netuid.is_root() && SubnetMechanism::<T>::get(netuid) == 1 {
            let max_supported_input = SubnetTAO::<T>::get(netuid).saturating_mul(1_000.into());
            ensure!(
                amount <= max_supported_input,
                Error::<T>::InsufficientLiquidity
            );
        }

        Ok(())
    }
}
