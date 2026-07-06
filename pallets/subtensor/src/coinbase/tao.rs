/// This file contains all critical operations with TAO and Alpha:
///
///   - Minting, burning, recycling, and transferring
///   - Reading colkey TAO balances
///   - Access to subnet TAO reserves
///
use frame_support::traits::{
    Imbalance,
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

pub type BalanceOf<T> =
    <<T as Config>::Currency as fungible::Inspect<<T as frame_system::Config>::AccountId>>::Balance;

pub type CreditOf<T> = Credit<<T as frame_system::Config>::AccountId, <T as Config>::Currency>;

pub const MAX_TAO_ISSUANCE: u64 = 21_000_000_000_000_000_u64;

impl<T: Config> Pallet<T> {
    /// Returns Subnet TAO reserve using SubnetTAO map.
    /// Do not use subnet account balance because it may also contain
    /// locked TAO.
    pub fn get_subnet_tao(netuid: NetUid) -> TaoBalance {
        SubnetTAO::<T>::get(netuid)
    }

    /// Internal function that transfers and updates subtensor pallet total issuance
    /// in case of dust collection.
    fn transfer_allow_death_update_ti(
        origin_coldkey: &T::AccountId,
        destination_coldkey: &T::AccountId,
        amount: BalanceOf<T>,
    ) -> DispatchResult {
        // If account balance remainder drops below ED, then account is killed, balance
        // is lost, and we need to reduce total issuance in subtensor pallet. Measure
        // balance TI before and after to detect the dust.
        let balances_ti_before = <T as pallet::Config>::Currency::total_issuance();

        <T as pallet::Config>::Currency::transfer(
            origin_coldkey,
            destination_coldkey,
            amount,
            Preservation::Expendable,
        )?;

        let balances_ti_after = <T as pallet::Config>::Currency::total_issuance();
        if balances_ti_after < balances_ti_before {
            let burned = balances_ti_before.saturating_sub(balances_ti_after);
            TotalIssuance::<T>::mutate(|total| {
                *total = total.saturating_sub(burned);
            });
        }

        Ok(())
    }

    /// Transfer TAO from one coldkey account to another.
    ///
    /// This is a plain transfer and may reap the origin account if `amount` reduces
    /// its balance below the existential deposit (ED).    
    pub fn transfer_tao(
        origin_coldkey: &T::AccountId,
        destination_coldkey: &T::AccountId,
        amount: BalanceOf<T>,
    ) -> DispatchResult {
        // Get full balance including ED
        let max_transferrable = Self::get_coldkey_balance(origin_coldkey);
        ensure!(amount <= max_transferrable, Error::<T>::InsufficientBalance);

        Self::transfer_allow_death_update_ti(origin_coldkey, destination_coldkey, amount)
    }

    /// Transfer all transferable TAO from `origin_coldkey` to `destination_coldkey`,
    /// allowing the origin account to be reaped.
    ///
    /// # Parameters
    /// - `origin_coldkey`: Source account.
    /// - `destination_coldkey`: Destination account.
    ///
    /// # Returns
    /// DispatchResult of the operation.
    ///
    /// # Errors
    /// - Any error returned by the underlying currency transfer.
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
            Self::transfer_allow_death_update_ti(
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
    /// # Parameters
    /// - `netuid`: Subnet identifier.
    /// - `origin_coldkey`: Account to transfer TAO from.
    /// - `destination_coldkey`: Account to transfer TAO to.
    /// - `amount`: Requested amount to transfer.
    ///
    /// # Returns
    /// Returns the actual amount transferred.
    ///
    /// # Errors
    /// Returns [`Error::<T>::InsufficientBalance`] if no positive amount can be
    /// transferred while preserving the origin account.
    ///
    /// Propagates any other transfer error from the underlying currency.
    pub fn transfer_tao_to_subnet(
        netuid: NetUid,
        origin_coldkey: &T::AccountId,
        amount: BalanceOf<T>,
    ) -> Result<BalanceOf<T>, DispatchError> {
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
            Error::<T>::InsufficientBalance
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
        amount: BalanceOf<T>,
    ) -> DispatchResult {
        let subnet_account: T::AccountId =
            Self::get_subnet_account_id(netuid).ok_or(Error::<T>::SubnetNotExists)?;
        Self::transfer_tao(&subnet_account, coldkey, amount)
    }

    /// Permanently remove TAO amount from existence by moving to the burn
    /// address. Does not effect issuance rate
    pub fn burn_tao(coldkey: &T::AccountId, amount: BalanceOf<T>) -> DispatchResult {
        let burn_address: T::AccountId = T::BurnAccountId::get().into_account_truncating();
        Self::transfer_tao(coldkey, &burn_address, amount)?;
        Ok(())
    }

    /// Remove TAO from existence and reduce total issuance.
    /// Effects issuance rate by reducing TI.
    /// Does not allow the account to drop below ED.
    pub fn recycle_tao(coldkey: &T::AccountId, amount: BalanceOf<T>) -> DispatchResult {
        // Ensure that the coldkey doesn't drop below ED
        let max_preserving_amount = <T as Config>::Currency::reducible_balance(
            coldkey,
            Preservation::Preserve,
            Fortitude::Polite,
        );
        ensure!(
            amount <= max_preserving_amount,
            Error::<T>::InsufficientBalance
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

    pub fn can_remove_balance_from_coldkey_account(
        coldkey: &T::AccountId,
        amount: BalanceOf<T>,
    ) -> bool {
        amount <= Self::get_coldkey_balance(coldkey)
    }

    /// Returns the full coldkey balance including existential deposit
    pub fn get_coldkey_balance(coldkey: &T::AccountId) -> BalanceOf<T> {
        <T as Config>::Currency::reducible_balance(
            coldkey,
            Preservation::Expendable,
            Fortitude::Polite,
        )
    }

    /// Returns the balance that can be transfered without killing account
    pub fn get_keep_alive_balance(coldkey: &T::AccountId) -> BalanceOf<T> {
        <T as Config>::Currency::reducible_balance(
            coldkey,
            Preservation::Preserve,
            Fortitude::Polite,
        )
    }

    /// Create TAO and return the imbalance.
    ///
    /// The mint workflow is following:
    ///   1. mint_tao in block_emission
    ///   2. spend_tao in run_coinbase (distribute to subnets)
    ///   3. None should be left, so burn the remainder using burn_credit for records
    pub fn mint_tao(amount: BalanceOf<T>) -> CreditOf<T> {
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
        credit: CreditOf<T>,
        part: BalanceOf<T>,
    ) -> Result<CreditOf<T>, CreditOf<T>> {
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

    /// Finalizes the unused part of the minted TAO.
    pub fn recycle_credit(credit: CreditOf<T>) {
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

    pub fn get_total_issuance() -> TaoBalance {
        TotalIssuance::<T>::get()
    }
}
