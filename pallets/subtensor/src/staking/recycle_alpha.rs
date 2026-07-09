use super::*;
use crate::{Error, system::ensure_signed};
use frame_support::storage::{TransactionOutcome, with_transaction};
use subtensor_runtime_common::{AlphaBalance, NetUid};

impl<T: Config> Pallet<T> {
    /// Recycles alpha from a cold/hot key pair, reducing AlphaOut on a subnet
    ///
    /// # Arguments
    ///
    /// * `origin` - The origin of the call (must be signed by the coldkey)
    /// * `hotkey` - The hotkey account
    /// * `amount` - The amount of alpha to recycle
    /// * `netuid` - The subnet ID from which to reduce AlphaOut
    ///
    /// # Returns
    ///
    /// * `Result<AlphaBalance, DispatchError>` - The actual amount recycled, or error
    pub fn do_recycle_alpha(
        origin: OriginFor<T>,
        hotkey: T::AccountId,
        amount: AlphaBalance,
        netuid: NetUid,
    ) -> Result<AlphaBalance, DispatchError> {
        let coldkey: T::AccountId = ensure_signed(origin)?;

        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);

        ensure!(
            !netuid.is_root(),
            Error::<T>::CannotBurnOrRecycleOnRootSubnet
        );

        Self::ensure_subtoken_enabled(netuid)?;

        // Ensure that the hotkey account exists this is only possible through registration.
        ensure!(
            Self::hotkey_account_exists(&hotkey),
            Error::<T>::HotKeyAccountNotExists
        );

        // Ensure that the hotkey has enough stake to withdraw.
        // Cap the amount at available Alpha because user might be paying transaxtion fees
        // in Alpha and their total is already reduced by now.
        let alpha_available =
            Self::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);
        let amount = amount.min(alpha_available);

        ensure!(
            SubnetAlphaOut::<T>::get(netuid) >= amount,
            Error::<T>::InsufficientLiquidity
        );

        // Ensure that recycled amount is not greater than available to unstake (due to locks)
        Self::ensure_available_to_unstake(&coldkey, netuid, amount)?;

        // Deduct from the coldkey's stake.
        Self::decrease_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid, amount);

        // Recycle means we should decrease the alpha issuance tracker.
        Self::recycle_subnet_alpha(netuid, amount);

        Self::deposit_event(Event::AlphaRecycled(coldkey, hotkey, amount, netuid));

        Ok(amount)
    }

    /// Burns alpha from a cold/hot key pair without reducing AlphaOut
    ///
    /// # Arguments
    ///
    /// * `origin` - The origin of the call (must be signed by the coldkey)
    /// * `hotkey` - The hotkey account
    /// * `amount` - The "up to" amount of alpha to burn
    /// * `netuid` - The subnet ID
    ///
    /// # Returns
    ///
    /// * `Result<AlphaBalance, DispatchError>` - The actual amount burned, or error
    pub fn do_burn_alpha(
        origin: OriginFor<T>,
        hotkey: T::AccountId,
        amount: AlphaBalance,
        netuid: NetUid,
    ) -> Result<AlphaBalance, DispatchError> {
        let coldkey = ensure_signed(origin)?;

        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);

        ensure!(
            !netuid.is_root(),
            Error::<T>::CannotBurnOrRecycleOnRootSubnet
        );

        Self::ensure_subtoken_enabled(netuid)?;

        // Ensure that the hotkey account exists this is only possible through registration.
        ensure!(
            Self::hotkey_account_exists(&hotkey),
            Error::<T>::HotKeyAccountNotExists
        );

        // Ensure that the hotkey has enough stake to withdraw.
        // Cap the amount at available Alpha because user might be paying transaxtion fees
        // in Alpha and their total is already reduced by now.
        let alpha_available =
            Self::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);
        let amount = amount.min(alpha_available);

        ensure!(
            SubnetAlphaOut::<T>::get(netuid) >= amount,
            Error::<T>::InsufficientLiquidity
        );

        // Ensure that burned amount is not greater than available to unstake (due to locks)
        Self::ensure_available_to_unstake(&coldkey, netuid, amount)?;

        // Deduct from the coldkey's stake.
        Self::decrease_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid, amount);

        Self::burn_subnet_alpha(netuid, amount);

        // Deposit event
        Self::deposit_event(Event::AlphaBurned(coldkey, hotkey, amount, netuid));

        Ok(amount)
    }

    pub(crate) fn do_add_stake_burn(
        origin: OriginFor<T>,
        hotkey: T::AccountId,
        netuid: NetUid,
        amount: TaoBalance,
        limit: Option<TaoBalance>,
    ) -> DispatchResult {
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        with_transaction(|| {
            let result = (|| {
                let alpha = if let Some(limit) = limit {
                    Self::do_add_stake_limit(
                        origin.clone(),
                        hotkey.clone(),
                        netuid,
                        amount,
                        limit,
                        false,
                    )?
                } else {
                    Self::do_add_stake(origin.clone(), hotkey.clone(), netuid, amount)?
                };

                Self::do_burn_alpha(origin, hotkey.clone(), alpha, netuid)?;

                Self::deposit_event(Event::AddStakeBurn {
                    netuid,
                    hotkey,
                    amount,
                    alpha,
                });

                Ok(())
            })();

            match result {
                Ok(()) => TransactionOutcome::Commit(Ok(())),
                Err(err) => TransactionOutcome::Rollback(Err(err)),
            }
        })
    }

    /// Atomically stakes TAO and recycles the resulting alpha.
    /// Permissionless counterpart used by the chain extension so that contracts
    /// can compose the two operations without leaving residual stake if the
    /// second leg fails.
    pub fn do_add_stake_recycle(
        origin: OriginFor<T>,
        hotkey: T::AccountId,
        netuid: NetUid,
        amount: TaoBalance,
    ) -> Result<AlphaBalance, DispatchError> {
        with_transaction(|| {
            let result = (|| {
                let alpha = Self::do_add_stake(origin.clone(), hotkey.clone(), netuid, amount)?;
                Self::do_recycle_alpha(origin, hotkey, alpha, netuid)
            })();

            match result {
                Ok(alpha) => TransactionOutcome::Commit(Ok(alpha)),
                Err(err) => TransactionOutcome::Rollback(Err(err)),
            }
        })
    }

    /// Atomically stakes TAO and burns the resulting alpha. Permissionless
    /// counterpart to `do_add_stake_burn`: return the amount of alpha burned.
    /// limit. Used by the chain extension.
    pub fn do_add_stake_burn_permissionless(
        origin: OriginFor<T>,
        hotkey: T::AccountId,
        netuid: NetUid,
        amount: TaoBalance,
    ) -> Result<AlphaBalance, DispatchError> {
        with_transaction(|| {
            let result = (|| {
                let alpha = Self::do_add_stake(origin.clone(), hotkey.clone(), netuid, amount)?;
                Self::do_burn_alpha(origin, hotkey, alpha, netuid)
            })();

            match result {
                Ok(alpha) => TransactionOutcome::Commit(Ok(alpha)),
                Err(err) => TransactionOutcome::Rollback(Err(err)),
            }
        })
    }
}
