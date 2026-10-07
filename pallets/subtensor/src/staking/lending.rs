//! Custody and accounting adapter for lending; no assets are minted by reserve extraction.
use super::*;
use crate::weights::WeightInfo;
use frame_support::{
    traits::tokens::{Fortitude, Preservation, fungible::Inspect},
    transactional,
};
use pallet_lending::{LendingInterface, LendingPoolInterface};
use sp_runtime::traits::AccountIdConversion;
use substrate_fixed::types::U64F64;
use subtensor_swap_interface::{Order, SwapHandler};

impl<T: Config> LendingPoolInterface<T::AccountId> for Pallet<T> {
    fn subnet_exists(netuid: NetUid) -> bool {
        !netuid.is_root()
            && Self::if_subnet_exist(netuid)
            && SubtokenEnabled::<T>::get(netuid)
            && SubnetMechanism::<T>::get(netuid) == 1
    }

    fn owner_allowed(owner: &T::AccountId) -> bool {
        !ColdkeySwapAnnouncements::<T>::contains_key(owner)
            && !ColdkeySwapDisputes::<T>::contains_key(owner)
    }

    fn fast_alpha_price(netuid: NetUid) -> Option<U64F64> {
        SubnetFastMovingPrice::<T>::get(netuid).filter(|price| *price > U64F64::from_num(0))
    }

    #[transactional]
    fn tune_min_price_impact(
        netuid: NetUid,
        bps: u16,
    ) -> Result<(TaoBalance, AlphaBalance), DispatchError> {
        ensure!(
            <Self as LendingPoolInterface<T::AccountId>>::subnet_exists(netuid)
                && !DissolveCleanupQueue::<T>::get().contains(&netuid),
            Error::<T>::SubnetNotExists
        );
        if bps == 0 {
            return Ok((TaoBalance::ZERO, AlphaBalance::ZERO));
        }
        let changed = T::SwapInterface::tune_min_price_impact(
            netuid,
            TaoBalance::from(500_000_000_000_u64),
            bps,
        )?;
        if !changed {
            return Ok((TaoBalance::ZERO, AlphaBalance::ZERO));
        }
        Self::fund_unreachable_reserves_inner(netuid, true, true)
    }

    fn redemption_alpha_supply(netuid: NetUid) -> Result<u128, DispatchError> {
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        ensure!(
            !crate::migrations::migrate_total_alpha_staked::in_progress::<T>(),
            Error::<T>::LendingUnavailable
        );
        // AlphaOut includes burns and pending emissions that have no redemption
        // claim. The canonical stake aggregate counts actual funded claims.
        // Include pool alpha for legacy subnets too: it can become an eligible
        // holder's claim before deregistration, so excluding it overvalues loans.
        let supply = u128::from(SubnetAlphaIn::<T>::get(netuid).to_u64())
            .checked_add(u128::from(SubnetProtocolAlpha::<T>::get(netuid).to_u64()))
            .and_then(|value| {
                value.checked_add(u128::from(TotalAlphaStaked::<T>::get(netuid).to_u64()))
            })
            .and_then(|value| {
                // Already materialized protocol alpha is restored to AlphaIn
                // before the ordinary dissolution denominator is frozen.
                value.checked_add(u128::from(
                    T::SwapInterface::protocol_alpha_reservoir(netuid).to_u64(),
                ))
            })
            .ok_or(DispatchError::Arithmetic(
                sp_runtime::ArithmeticError::Overflow,
            ))?;
        ensure!(supply > 0, Error::<T>::AmountTooLow);
        Ok(supply)
    }

    fn alpha_loan_redemption_basis(
        netuid: NetUid,
        remaining_unloaned_alpha: AlphaBalance,
    ) -> Result<(TaoBalance, u128), DispatchError> {
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        // Protocol buffers are restored before ordinary deregistration payouts.
        // The lending vault's TAO is added by the caller, after this snapshot.
        let pot = SubnetTAO::<T>::get(netuid)
            .saturating_add(T::SwapInterface::protocol_tao_reservoir(netuid));
        let protocol_alpha = SubnetProtocolAlpha::<T>::get(netuid);
        let eligible =
            if NetworkRegisteredAt::<T>::get(netuid) > TaoInRefundDeploymentBlock::<T>::get() {
                // Match the ordinary payout's saturating pool/protocol balance. The
                // post-grant vault inventory returns to AlphaIn before that payout.
                SubnetAlphaIn::<T>::get(netuid)
                    .saturating_add(T::SwapInterface::protocol_alpha_reservoir(netuid))
                    .saturating_add(remaining_unloaned_alpha)
                    .saturating_add(protocol_alpha)
            } else {
                // Legacy payouts exclude AlphaIn. Holder stake aggregates cannot
                // lower-bound eligible claims because individual shares round down.
                protocol_alpha
            };
        Ok((pot, u128::from(eligible.to_u64())))
    }

    fn quote_sell(netuid: NetUid, alpha: AlphaBalance) -> Result<TaoBalance, DispatchError> {
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        let quote = T::SwapInterface::sim_swap(netuid, GetTaoForAlpha::<T>::with_amount(alpha))?;
        ensure!(
            quote.amount_paid_in.saturating_add(quote.fee_paid) == alpha,
            Error::<T>::SlippageTooHigh
        );
        ensure!(!quote.amount_paid_out.is_zero(), Error::<T>::AmountTooLow);
        Ok(quote.amount_paid_out)
    }

    fn max_buy_input(netuid: NetUid) -> TaoBalance {
        T::SwapInterface::max_buy_input(netuid)
    }

    fn quote_buy(netuid: NetUid, tao: TaoBalance) -> Result<AlphaBalance, DispatchError> {
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        let quote = T::SwapInterface::swap(
            netuid,
            GetAlphaForTao::<T>::with_amount(tao),
            u64::MAX.into(),
            false,
            true,
        )?;
        ensure!(
            quote.amount_paid_in.saturating_add(quote.fee_paid) == tao,
            Error::<T>::SlippageTooHigh
        );
        ensure!(!quote.amount_paid_out.is_zero(), Error::<T>::AmountTooLow);
        Ok(quote.amount_paid_out)
    }

    fn buy_spendable_tao(account: &T::AccountId) -> TaoBalance {
        <T as Config>::Currency::reducible_balance(
            account,
            Preservation::Preserve,
            Fortitude::Polite,
        )
    }

    #[transactional]
    fn repay_alpha(
        from: &T::AccountId,
        source_hotkey: &T::AccountId,
        vault: &T::AccountId,
        custody: &T::AccountId,
        netuid: NetUid,
        amount: AlphaBalance,
    ) -> DispatchResult {
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        Self::ensure_subtoken_enabled(netuid)?;
        ensure!(!amount.is_zero(), Error::<T>::AmountTooLow);
        ensure!(
            Self::hotkey_account_exists(source_hotkey),
            Error::<T>::HotKeyAccountNotExists
        );
        Self::ensure_available_to_unstake(from, netuid, amount)?;
        Self::ensure_hotkey_covers_collateral(from, source_hotkey, netuid, amount)?;
        // Fixed-token repayment is valid even after its spot value falls below the
        // minimum trade size. The normal transfer still requires an exact stake debit.
        <Self as subtensor_swap_interface::OrderSwapInterface<T::AccountId>>::transfer_staked_alpha(
            from,
            source_hotkey,
            vault,
            custody,
            netuid,
            amount,
            false,
            false,
        )
    }

    #[transactional]
    fn terminal_split_tao(
        from: &T::AccountId,
        vault: &T::AccountId,
        recipient: &T::AccountId,
        recovery: TaoBalance,
        refund: TaoBalance,
    ) -> Result<(TaoBalance, TaoBalance), DispatchError> {
        let total = recovery
            .checked_add(&refund)
            .ok_or(Error::<T>::InsufficientTaoBalance)?;
        if total.is_zero() {
            return Ok((TaoBalance::ZERO, TaoBalance::ZERO));
        }
        // Withdraw once: paying one leg must not reap the sender's remaining dust.
        let credit = Self::withdraw_tao_as_credit(from, total)?;
        let (credit, recovered) =
            Self::resolve_lending_credit_or_recycle_dust(vault, credit, recovery)?;
        let (credit, refunded) =
            Self::resolve_lending_credit_or_recycle_dust(recipient, credit, refund)?;
        Self::recycle_credit(credit);
        Ok((recovered, refunded))
    }

    #[transactional]
    fn collect_interest_tao(
        from: &T::AccountId,
        vault: &T::AccountId,
        amount: TaoBalance,
    ) -> Result<TaoBalance, DispatchError> {
        Self::transfer_lending_tao_or_recycle_dust(from, vault, amount)
    }

    #[transactional]
    fn burn_interest_tao(account: &T::AccountId, amount: TaoBalance) -> DispatchResult {
        if amount.is_zero() {
            return Ok(());
        }
        let burn: T::AccountId = T::BurnAccountId::get().into_account_truncating();
        ensure!(*account != burn, Error::<T>::InsufficientTaoBalance);
        let source_before = <T as Config>::Currency::total_balance(account);
        let burn_before = <T as Config>::Currency::total_balance(&burn);
        Self::burn_tao(account, amount)?;
        let source_after = <T as Config>::Currency::total_balance(account);
        let burn_after = <T as Config>::Currency::total_balance(&burn);
        // A fee cannot consume unrelated principal through sender reaping, nor
        // be recorded as burned unless the canonical address receives it in full.
        ensure!(
            source_before.saturating_sub(source_after) == amount
                && burn_after.saturating_sub(burn_before) == amount,
            Error::<T>::InsufficientTaoBalance
        );
        Ok(())
    }

    #[transactional]
    fn refund_dissolution_tao(
        from: &T::AccountId,
        to: &T::AccountId,
        amount: TaoBalance,
    ) -> Result<TaoBalance, DispatchError> {
        Self::transfer_lending_tao_or_recycle_dust(from, to, amount)
    }

    #[transactional]
    fn transfer_dissolution_alpha(
        from: &T::AccountId,
        to: &T::AccountId,
        hotkey: &T::AccountId,
        netuid: NetUid,
        amount: AlphaBalance,
    ) -> DispatchResult {
        ensure!(
            DissolveCleanupQueue::<T>::get().contains(&netuid),
            Error::<T>::SubnetNotExists
        );
        let (vault, custody) =
            T::LendingInterface::custody_accounts(netuid).ok_or(Error::<T>::LendingUnavailable)?;
        ensure!(
            *to == vault && *hotkey == custody,
            Error::<T>::LendingUnavailable
        );
        ensure!(
            amount.to_u64() <= i64::MAX as u64
                && Self::try_increase_stake_for_hotkey_and_coldkey_on_subnet(
                    hotkey, netuid, amount
                ),
            Error::<T>::NotEnoughStakeToWithdraw
        );
        ensure!(
            Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, from, netuid) >= amount,
            Error::<T>::NotEnoughStakeToWithdraw
        );
        let removed =
            Self::decrease_stake_for_hotkey_and_coldkey_on_subnet(hotkey, from, netuid, amount);
        ensure!(removed == amount, Error::<T>::NotEnoughStakeToWithdraw);
        Self::increase_stake_for_hotkey_and_coldkey_on_subnet(hotkey, to, netuid, amount);
        Ok(())
    }

    #[transactional]
    fn return_dissolution_reserves(
        netuid: NetUid,
        account: &T::AccountId,
        hotkey: &T::AccountId,
        tao: TaoBalance,
        alpha: AlphaBalance,
    ) -> DispatchResult {
        let subnet = Self::get_subnet_account_id(netuid).ok_or(Error::<T>::SubnetNotExists)?;
        if !tao.is_zero() {
            Self::transfer_tao(account, &subnet, tao)?;
            Self::increase_provided_tao_reserve(netuid, tao);
            // Deregistration already removed this subnet's balance from TotalStake.
            if Self::if_subnet_exist(netuid) {
                TotalStake::<T>::mutate(|total| *total = total.saturating_add(tao));
            }
        }
        if !alpha.is_zero() {
            let removed = Self::decrease_stake_for_hotkey_and_coldkey_on_subnet(
                hotkey, account, netuid, alpha,
            );
            ensure!(removed == alpha, Error::<T>::NotEnoughStakeToWithdraw);
            SubnetAlphaOut::<T>::try_mutate(netuid, |out| -> DispatchResult {
                ensure!(*out >= alpha, Error::<T>::NotEnoughStakeToWithdraw);
                *out = out.saturating_sub(alpha);
                Ok(())
            })?;
            Self::increase_provided_alpha_reserve(netuid, alpha);
        }
        Ok(())
    }
}

impl<T: Config> Pallet<T> {
    fn resolve_lending_credit_or_recycle_dust(
        to: &T::AccountId,
        credit: crate::coinbase::tao::CreditOf<T>,
        amount: TaoBalance,
    ) -> Result<(crate::coinbase::tao::CreditOf<T>, TaoBalance), DispatchError> {
        use frame_support::traits::Imbalance;
        if amount.is_zero() {
            return Ok((credit, TaoBalance::ZERO));
        }
        if amount < <T as Config>::Currency::minimum_balance()
            && <T as Config>::Currency::total_balance(to).is_zero()
        {
            let (dust, remainder) = credit.split(amount);
            Self::recycle_credit(dust);
            return Ok((remainder, TaoBalance::ZERO));
        }
        let remainder = Self::spend_tao(to, credit, amount)
            .map_err(|_credit| Error::<T>::InsufficientTaoBalance)?;
        Ok((remainder, amount))
    }

    /// Recycle only a payment too small to create its destination. Large failures retry.
    pub(crate) fn transfer_lending_tao_or_recycle_dust(
        from: &T::AccountId,
        to: &T::AccountId,
        amount: TaoBalance,
    ) -> Result<TaoBalance, DispatchError> {
        if !amount.is_zero()
            && amount < <T as Config>::Currency::minimum_balance()
            && <T as Config>::Currency::total_balance(to).is_zero()
        {
            let credit = Self::withdraw_tao_as_credit(from, amount)?;
            Self::recycle_credit(credit);
            return Ok(TaoBalance::ZERO);
        }
        Self::transfer_tao(from, to, amount)?;
        Ok(amount)
    }

    /// Extract fixed-curve floors into real custody, preserving TAO and alpha issuance.
    /// This whole operation rolls back if the account transfer, share pool, or vault fails.
    #[transactional]
    pub fn fund_unreachable_reserves(netuid: NetUid, historical: bool) -> DispatchResult {
        Self::fund_unreachable_reserves_inner(netuid, historical, false).map(|_| ())
    }

    /// Automatic admission funds each vault once. Explicit governance tightening
    /// can append newly unreachable physical assets to a mature vault without
    /// touching outstanding principal, fixed coupons or its price reference.
    #[transactional]
    fn fund_unreachable_reserves_inner(
        netuid: NetUid,
        historical: bool,
        fund_existing: bool,
    ) -> Result<(TaoBalance, AlphaBalance), DispatchError> {
        if netuid.is_root()
            || SubnetMechanism::<T>::get(netuid) != 1
            || (!fund_existing && T::LendingInterface::has_vault(netuid))
        {
            return Ok((TaoBalance::ZERO, AlphaBalance::ZERO));
        }
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        // Admission is bounded independently of the number of live subnet pools.
        // A full vault set leaves the pool and its unreachable floors untouched;
        // the ordinary bounded hook retries admission after a slot becomes free.
        if !T::LendingInterface::has_vault(netuid) && !T::LendingInterface::has_funding_capacity() {
            ensure!(!fund_existing, Error::<T>::LendingUnavailable);
            return Ok((TaoBalance::ZERO, AlphaBalance::ZERO));
        }
        if SubnetTAO::<T>::get(netuid).is_zero() || SubnetAlphaIn::<T>::get(netuid).is_zero() {
            return Ok((TaoBalance::ZERO, AlphaBalance::ZERO));
        }
        let (alpha, tao) = T::SwapInterface::extract_unreachable_reserves(netuid)?;
        if alpha.is_zero() && tao.is_zero() {
            return Ok((TaoBalance::ZERO, AlphaBalance::ZERO));
        }
        let (vault, hotkey) =
            T::LendingInterface::custody_accounts(netuid).ok_or(Error::<T>::LendingUnavailable)?;
        let subnet = Self::get_subnet_account_id(netuid).ok_or(Error::<T>::SubnetNotExists)?;
        if !tao.is_zero() {
            Self::transfer_tao(&subnet, &vault, tao)?;
            TotalStake::<T>::mutate(|total| *total = total.saturating_sub(tao));
        }
        if !alpha.is_zero() {
            ensure!(
                alpha.to_u64() <= i64::MAX as u64
                    && Self::try_increase_stake_for_hotkey_and_coldkey_on_subnet(
                        &hotkey, netuid, alpha
                    ),
                Error::<T>::NotEnoughStakeToWithdraw
            );
            Self::increase_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &vault, netuid, alpha);
            SubnetAlphaOut::<T>::mutate(netuid, |out| *out = out.saturating_add(alpha));
        }
        let stored = U64F64::checked_from_num(SubnetMovingPrice::<T>::get(netuid));
        let (reference, has_history) = match stored.filter(|p| *p > U64F64::from_num(0)) {
            Some(price)
                if historical
                    && Self::get_current_block_as_u64()
                        .saturating_sub(NetworkRegisteredAt::<T>::get(netuid))
                        >= 7_200 =>
            {
                (price, true)
            }
            _ => (T::SwapInterface::current_alpha_price(netuid), false),
        };
        T::LendingInterface::fund_reserves(netuid, tao, alpha, reference, has_history)?;
        Ok((tao, alpha))
    }

    /// At most one new subnet per block; mature vaults are never re-extracted implicitly.
    pub(crate) fn fund_one_new_lending_vault() -> Weight {
        let mut weight = T::DbWeight::get().reads(1);
        if !HasMigrationRun::<T>::get(
            crate::migrations::migrate_pool_lending::MIGRATION_NAME.to_vec(),
        ) {
            return weight;
        }
        let netuids = Self::get_all_subnet_netuids();
        weight.saturating_accrue(T::DbWeight::get().reads(netuids.len() as u64));
        if let Some(index) = Self::get_current_block_as_u64().checked_rem(netuids.len() as u64)
            && let Some(netuid) = netuids.get(index as usize)
        {
            weight.saturating_accrue(<T as Config>::WeightInfo::fund_lending_reserves());
            if let Err(error) = Self::fund_unreachable_reserves(*netuid, false) {
                log::warn!("New lending vault {netuid:?} not funded: {error:?}");
            }
        }
        weight
    }
}
