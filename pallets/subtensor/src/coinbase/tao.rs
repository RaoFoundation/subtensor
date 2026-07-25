//! TAO currency operations for Subtensor: mint, burn, recycle, transfer, and registration locks.
//!
//! Deliberately does **not** treat the subnet account's free balance as the pool reserve —
//! use [`Pallet::get_subnet_tao`] ([`SubnetTAO`]) because the account may also hold locked TAO.
//!
//! Mint workflow for the coinbase:
//! 1. [`Pallet::mint_tao`] in block emission
//! 2. [`Pallet::spend_tao`] while distributing to subnets
//! 3. [`Pallet::recycle_credit`] for any leftover credit
//!
use frame_support::traits::{
    Imbalance, LockableCurrency, WithdrawReasons,
    fungible::Mutate,
    tokens::{
        Fortitude, Precision, Preservation,
        fungible::{Balanced, Credit, Inspect},
    },
};
use sp_runtime::traits::AccountIdConversion;
use sp_runtime::{DispatchError, DispatchResult};
use subtensor_runtime_common::{NetUid, TaoBalance};

use super::*;

/// Currency balance type for Subtensor's TAO (`Config::Currency`).
pub type TaoCurrencyBalanceOf<T> =
    <<T as Config>::Currency as fungible::Inspect<<T as frame_system::Config>::AccountId>>::Balance;

/// Fungible credit (imbalance) produced by [`Pallet::mint_tao`] / withdraw paths.
pub type TaoCreditOf<T> = Credit<<T as frame_system::Config>::AccountId, <T as Config>::Currency>;

pub const MAX_TAO_ISSUANCE: u64 = 21_000_000_000_000_000_u64;

/// Balances lock id for TAO locked during network registration.
const TAO_REGISTRATION_LOCK_PREFIX: [u8; 4] = *b"rglk";

impl<T: Config> Pallet<T> {
    /// Returns Subnet TAO reserve using SubnetTAO map.
    /// Do not use subnet account balance because it may also contain
    /// locked TAO.
    pub fn get_subnet_tao(netuid: NetUid) -> TaoBalance {
        SubnetTAO::<T>::get(netuid)
    }

    /// Transfer TAO allowing the origin account to be reaped (existential-deposit dust
    /// handled by the runtime Balances `DustRemoval` impl). Does not touch pallet
    /// [`TotalIssuance`] — name historically suggested otherwise.
    fn transfer_tao_allow_death(
        origin_coldkey: &T::AccountId,
        destination_coldkey: &T::AccountId,
        amount: TaoCurrencyBalanceOf<T>,
    ) -> DispatchResult {
        <T as pallet::Config>::Currency::transfer(
            origin_coldkey,
            destination_coldkey,
            amount,
            Preservation::Expendable,
        )?;

        Ok(())
    }

    /// Transfer TAO from one coldkey account to another.
    ///
    /// This is a plain transfer and may reap the origin account if `amount` reduces
    /// its balance below the existential deposit (ED).    
    pub fn transfer_tao(
        origin_coldkey: &T::AccountId,
        destination_coldkey: &T::AccountId,
        amount: TaoCurrencyBalanceOf<T>,
    ) -> DispatchResult {
        // Get full balance including ED
        let max_transferrable = Self::get_coldkey_balance(origin_coldkey);
        ensure!(
            amount <= max_transferrable,
            Error::<T>::InsufficientTaoBalance
        );

        Self::transfer_tao_allow_death(origin_coldkey, destination_coldkey, amount)
    }

    /// Transfer all transferable TAO from `origin_coldkey` to `destination_coldkey`,
    /// allowing the origin account to be reaped.
    ///
    /// # Arguments
    /// * `origin_coldkey`: Source account.
    /// * `destination_coldkey`: Destination account.
    ///
    /// # Returns
    /// DispatchResult of the operation.
    ///
    /// # Errors
    /// * Any error returned by the underlying currency transfer.
    pub fn transfer_all_tao_and_kill(
        origin_coldkey: &T::AccountId,
        destination_coldkey: &T::AccountId,
    ) -> DispatchResult {
        let amount_to_transfer = <T as pallet::Config>::Currency::reducible_balance(
            origin_coldkey,
            Preservation::Expendable,
            Fortitude::Polite,
        );

        if !amount_to_transfer.is_zero() {
            Self::transfer_tao_allow_death(
                origin_coldkey,
                destination_coldkey,
                amount_to_transfer,
            )?;
        }

        Ok(())
    }

    /// Transfer TAO from a coldkey account for staking.
    ///
    /// If transferring the full `amount` would reap the origin account, this
    /// function leaves the existential deposit (ED) in place and transfers less.
    ///
    /// # Arguments
    /// * `netuid`: Subnet identifier.
    /// * `origin_coldkey`: Account to transfer TAO from.
    /// * `destination_coldkey`: Account to transfer TAO to.
    /// * `amount`: Requested amount to transfer.
    ///
    /// # Returns
    /// Returns the actual amount transferred.
    ///
    /// # Errors
    /// Returns [`Error::<T>::InsufficientTaoBalance`] if no positive amount can be
    /// transferred while preserving the origin account.
    ///
    /// Propagates any other transfer error from the underlying currency.
    pub fn transfer_tao_to_subnet(
        netuid: NetUid,
        origin_coldkey: &T::AccountId,
        amount: TaoCurrencyBalanceOf<T>,
    ) -> Result<TaoCurrencyBalanceOf<T>, DispatchError> {
        if amount.is_zero() {
            return Ok(0.into());
        }

        let subnet_account: T::AccountId =
            Self::get_subnet_account_id(netuid).ok_or(Error::<T>::SubnetNotExists)?;

        let max_preserving_amount = <T as Config>::Currency::reducible_balance(
            origin_coldkey,
            Preservation::Preserve,
            Fortitude::Polite,
        );

        let amount_to_transfer = amount.min(max_preserving_amount);

        ensure!(
            !amount_to_transfer.is_zero(),
            Error::<T>::InsufficientTaoBalance
        );

        <T as Config>::Currency::transfer(
            origin_coldkey,
            &subnet_account,
            amount_to_transfer,
            Preservation::Preserve,
        )?;

        Ok(amount_to_transfer)
    }

    /// Move unstaked TAO from subnet account to coldkey.
    pub fn transfer_tao_from_subnet(
        netuid: NetUid,
        coldkey: &T::AccountId,
        amount: TaoCurrencyBalanceOf<T>,
    ) -> DispatchResult {
        let subnet_account: T::AccountId =
            Self::get_subnet_account_id(netuid).ok_or(Error::<T>::SubnetNotExists)?;
        Self::transfer_tao(&subnet_account, coldkey, amount)
    }

    /// Move TAO to the burn address. Does **not** reduce pallet [`TotalIssuance`].
    pub fn burn_tao(coldkey: &T::AccountId, amount: TaoCurrencyBalanceOf<T>) -> DispatchResult {
        let burn_address: T::AccountId = T::BurnAccountId::get().into_account_truncating();
        Self::transfer_tao(coldkey, &burn_address, amount)?;
        Ok(())
    }

    /// Destroy TAO and reduce pallet [`TotalIssuance`] (affects the emission schedule).
    /// Preserves the account existential deposit.
    pub fn recycle_tao(coldkey: &T::AccountId, amount: TaoCurrencyBalanceOf<T>) -> DispatchResult {
        // Ensure that the coldkey doesn't drop below ED
        let max_preserving_amount = <T as Config>::Currency::reducible_balance(
            coldkey,
            Preservation::Preserve,
            Fortitude::Polite,
        );
        ensure!(
            amount <= max_preserving_amount,
            Error::<T>::InsufficientTaoBalance
        );

        // Decrease subtensor pallet total issuance
        TotalIssuance::<T>::mutate(|total| {
            *total = total.saturating_sub(amount);
        });

        let _ = <T as Config>::Currency::withdraw(
            coldkey,
            amount,
            Precision::Exact,
            Preservation::Expendable,
            Fortitude::Force,
        )
        .map_err(|_| Error::<T>::BalanceWithdrawalError)?
        .peek();

        Ok(())
    }

    /// Whether `coldkey` has at least `amount` transferable (expendable) balance.
    pub fn can_remove_balance_from_coldkey_account(
        coldkey: &T::AccountId,
        amount: TaoCurrencyBalanceOf<T>,
    ) -> bool {
        amount <= Self::get_coldkey_balance(coldkey)
    }

    /// Returns the full coldkey balance including existential deposit
    pub fn get_coldkey_balance(coldkey: &T::AccountId) -> TaoCurrencyBalanceOf<T> {
        <T as Config>::Currency::reducible_balance(
            coldkey,
            Preservation::Expendable,
            Fortitude::Polite,
        )
    }

    /// Reducible balance that preserves the account (keep-alive / above ED).
    pub fn get_keep_alive_balance(coldkey: &T::AccountId) -> TaoCurrencyBalanceOf<T> {
        <T as Config>::Currency::reducible_balance(
            coldkey,
            Preservation::Preserve,
            Fortitude::Polite,
        )
    }

    /// Issue up to `amount` TAO (hard-capped at [`MAX_TAO_ISSUANCE`]) and bump [`TotalIssuance`].
    ///
    /// Coinbase path: mint here → [`Pallet::spend_tao`] in run_coinbase → [`Pallet::recycle_credit`]
    /// for any leftover.
    pub fn mint_tao(amount: TaoCurrencyBalanceOf<T>) -> TaoCreditOf<T> {
        // Hard-limit maximum issuance to 21M TAO. Never issue more.
        let current_issuance = <T as Config>::Currency::total_issuance();

        let remaining_issuance =
            TaoBalance::from(MAX_TAO_ISSUANCE).saturating_sub(current_issuance);
        let amount_to_issue = amount.min(remaining_issuance);

        // Increase subtensor pallet total issuance
        TotalIssuance::<T>::mutate(|total| {
            *total = total.saturating_add(amount_to_issue);
        });

        <T as Config>::Currency::issue(amount_to_issue)
    }

    /// Spend part of the imbalance
    /// The part parameter is the balance itself that will be credited to the coldkey
    /// Return the remaining credit or error
    pub fn spend_tao(
        coldkey: &T::AccountId,
        credit: TaoCreditOf<T>,
        part: TaoCurrencyBalanceOf<T>,
    ) -> Result<TaoCreditOf<T>, TaoCreditOf<T>> {
        // Reject overspending.
        if credit.peek() < part {
            return Err(credit);
        }

        let (to_spend, remainder) = credit.split(part);

        match <T as Config>::Currency::resolve(coldkey, to_spend) {
            Ok(()) => Ok(remainder),
            Err(unresolved_to_spend) => Err(unresolved_to_spend.merge(remainder)),
        }
    }

    /// Withdraw TAO from an account into a fresh credit.
    ///
    /// This is useful when a previous `spend_tao` resolve must be undone without
    /// changing total issuance.
    pub fn withdraw_tao_as_credit(
        coldkey: &T::AccountId,
        amount: TaoCurrencyBalanceOf<T>,
    ) -> Result<TaoCreditOf<T>, DispatchError> {
        let credit = <T as Config>::Currency::withdraw(
            coldkey,
            amount,
            Precision::Exact,
            Preservation::Expendable,
            Fortitude::Polite,
        )?;

        Ok(credit)
    }

    /// Drop leftover minted credit and subtract it from pallet [`TotalIssuance`].
    pub fn recycle_credit(credit: TaoCreditOf<T>) {
        let amount = credit.peek();
        if !amount.is_zero() {
            // Some credit is remaining: Decrease subtensor pallet total issuance
            log::debug!(
                "recycle_credit received non-zero credit ({}); will reduce TotalIssuance",
                amount,
            );

            TotalIssuance::<T>::mutate(|total| {
                *total = total.saturating_sub(amount);
            });
        }
    }

    /// Pallet-tracked total TAO issuance ([`TotalIssuance`]), used by the emission curve.
    pub fn get_total_issuance() -> TaoBalance {
        TotalIssuance::<T>::get()
    }

    /// 8-byte Balances lock id: `rglk` prefix + little-endian `lock_id`.
    fn get_network_registration_lock_identifier(lock_id: u32) -> [u8; 8] {
        let mut id: frame_support::traits::LockIdentifier = [0; 8];
        id[..4].copy_from_slice(&TAO_REGISTRATION_LOCK_PREFIX);
        id[4..8].copy_from_slice(&lock_id.to_le_bytes());
        id
    }

    /// Lock `amount` TAO on `coldkey` under the network-registration lock id.
    pub fn lock_network_registration_cost(
        coldkey: &T::AccountId,
        amount: TaoCurrencyBalanceOf<T>,
        lock_id: u32,
    ) -> DispatchResult {
        ensure!(
            Self::can_remove_balance_from_coldkey_account(coldkey, amount),
            Error::<T>::InsufficientTaoBalance
        );

        let identifier = Self::get_network_registration_lock_identifier(lock_id);

        <<T as Config>::Currency as LockableCurrency<<T as frame_system::Config>::AccountId>>::set_lock(
            identifier,
            coldkey,
            amount,
            WithdrawReasons::all(),
        );

        Ok(())
    }

    /// Remove the network-registration Balances lock for `lock_id` on `coldkey`.
    pub fn unlock_network_registration_cost(
        coldkey: &T::AccountId,
        lock_id: u32,
    ) -> DispatchResult {
        let identifier = Self::get_network_registration_lock_identifier(lock_id);
        <<T as Config>::Currency as LockableCurrency<<T as frame_system::Config>::AccountId>>::remove_lock(
            identifier,
            coldkey,
        );

        Ok(())
    }
}
