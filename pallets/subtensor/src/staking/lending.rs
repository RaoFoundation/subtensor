//! Pool / custody adapter for synthetic shorts.
//!
//! `mint_and_sell` mints alpha onto custody stake and sells it into the AMM;
//! `buy_and_burn` buys it back with custody TAO and destroys it. Both trade
//! through the ordinary stake paths, so TAO flow records and the short moves
//! emission. Sale proceeds sit on custody coldkeys, outside `SubnetTAO`.
use super::*;
use crate::SubnetFastMovingPrice;
use frame_support::{
    traits::{Imbalance, tokens::fungible::Inspect},
    transactional,
};
use pallet_lending::LendingPoolInterface;
use sp_runtime::traits::AccountIdConversion;
#[cfg(feature = "runtime-benchmarks")]
use substrate_fixed::types::I96F32;
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

    fn prune_immune(netuid: NetUid) -> bool {
        Self::is_network_prune_immune(netuid)
    }

    fn quote_buy(netuid: NetUid, tao: TaoBalance) -> Result<AlphaBalance, DispatchError> {
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        let quote = T::SwapInterface::sim_swap(netuid, GetAlphaForTao::<T>::with_amount(tao))?;
        ensure!(
            quote.amount_paid_in.saturating_add(quote.fee_paid) == tao,
            Error::<T>::SlippageTooHigh
        );
        ensure!(!quote.amount_paid_out.is_zero(), Error::<T>::AmountTooLow);
        Ok(quote.amount_paid_out)
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

    /// Pool TAO only: the `T` of the dissolution rate. Short escrow (sale
    /// proceeds and collateral on custody coldkeys) never enters `SubnetTAO`.
    fn pool_tao(netuid: NetUid) -> TaoBalance {
        SubnetTAO::<T>::get(netuid)
    }

    fn pool_alpha(netuid: NetUid) -> AlphaBalance {
        SubnetAlphaIn::<T>::get(netuid)
    }

    fn quote_weight_parts(netuid: NetUid) -> u64 {
        T::SwapInterface::balancer_quote_weight(netuid)
    }

    fn quote_sell_on(
        tao_reserve: TaoBalance,
        alpha_reserve: AlphaBalance,
        quote_weight_parts: u64,
        delta_alpha: AlphaBalance,
    ) -> TaoBalance {
        T::SwapInterface::quote_sell_on(tao_reserve, alpha_reserve, quote_weight_parts, delta_alpha)
    }

    fn quote_buy_on(
        tao_reserve: TaoBalance,
        alpha_reserve: AlphaBalance,
        quote_weight_parts: u64,
        delta_tao: TaoBalance,
    ) -> AlphaBalance {
        T::SwapInterface::quote_buy_on(tao_reserve, alpha_reserve, quote_weight_parts, delta_tao)
    }

    /// Full issuance, the `all_alpha` of the dissolution rate. Sold synthetic
    /// alpha sits in `SubnetAlphaIn` and defaulted (residual) alpha stays in
    /// circulation, so both are counted.
    fn alpha_issuance(netuid: NetUid) -> AlphaBalance {
        Self::get_alpha_issuance(netuid)
    }

    fn moving_alpha_price(netuid: NetUid) -> U64F64 {
        Self::get_moving_alpha_price(netuid)
    }

    fn notional_alpha_price(netuid: NetUid) -> U64F64 {
        let spot = T::SwapInterface::current_alpha_price(netuid.into());
        SubnetFastMovingPrice::<T>::get(netuid)
            .unwrap_or(spot)
            .max(spot)
    }

    fn live_subnets() -> alloc::vec::Vec<NetUid> {
        Self::get_all_subnet_netuids()
            .into_iter()
            .filter(|netuid| !netuid.is_root())
            .collect()
    }

    #[transactional]
    fn mint_and_sell(
        netuid: NetUid,
        custody_cold: &T::AccountId,
        custody_hot: &T::AccountId,
        mint_alpha: AlphaBalance,
        min_tao_out: TaoBalance,
    ) -> Result<TaoBalance, DispatchError> {
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        ensure!(!mint_alpha.is_zero(), Error::<T>::AmountTooLow);
        ensure!(
            mint_alpha.to_u64() <= i64::MAX as u64
                && Self::try_increase_stake_for_hotkey_and_coldkey_on_subnet(
                    custody_hot,
                    netuid,
                    mint_alpha
                ),
            Error::<T>::NotEnoughStakeToWithdraw
        );
        Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
            custody_hot,
            custody_cold,
            netuid,
            mint_alpha,
        );
        SubnetAlphaOut::<T>::mutate(netuid, |out| *out = out.saturating_add(mint_alpha));
        let tao = <Self as subtensor_swap_interface::OrderSwapInterface<T::AccountId>>::sell_alpha(
            custody_cold,
            custody_hot,
            netuid,
            mint_alpha,
            TaoBalance::ZERO,
            false,
        )?;
        ensure!(tao >= min_tao_out, Error::<T>::SlippageTooHigh);
        Ok(tao)
    }

    #[transactional]
    fn buy_and_burn(
        netuid: NetUid,
        custody_cold: &T::AccountId,
        custody_hot: &T::AccountId,
        alpha: AlphaBalance,
        max_tao_in: TaoBalance,
    ) -> Result<(TaoBalance, AlphaBalance), DispatchError> {
        ensure!(!alpha.is_zero(), Error::<T>::AmountTooLow);
        let tao = Self::tao_for_exact_alpha(netuid, alpha, max_tao_in)?;
        let bought =
            <Self as subtensor_swap_interface::OrderSwapInterface<T::AccountId>>::buy_alpha(
                custody_cold,
                custody_hot,
                netuid,
                tao,
                u64::MAX.into(),
                false,
            )?;
        ensure!(bought >= alpha, Error::<T>::SlippageTooHigh);
        Self::destroy_custody_alpha(custody_cold, custody_hot, netuid, bought)?;
        Ok((tao, bought))
    }

    #[transactional]
    fn burn_interest_tao(account: &T::AccountId, amount: TaoBalance) -> DispatchResult {
        if amount.is_zero() {
            return Ok(());
        }
        let burn: T::AccountId = T::BurnAccountId::get().into_account_truncating();
        ensure!(*account != burn, Error::<T>::InsufficientLiquidity);
        let source_before = <T as Config>::Currency::total_balance(account);
        let burn_before = <T as Config>::Currency::total_balance(&burn);
        Self::burn_tao(account, amount)?;
        let source_after = <T as Config>::Currency::total_balance(account);
        let burn_after = <T as Config>::Currency::total_balance(&burn);
        ensure!(
            source_before.saturating_sub(source_after) == amount
                && burn_after.saturating_sub(burn_before) == amount,
            Error::<T>::InsufficientLiquidity
        );
        Ok(())
    }

    #[transactional]
    fn collect_interest_tao(
        from: &T::AccountId,
        to: &T::AccountId,
        amount: TaoBalance,
    ) -> Result<TaoBalance, DispatchError> {
        Self::transfer_lending_tao_or_recycle_dust(from, to, amount)
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
    fn terminal_split_tao(
        from: &T::AccountId,
        vault: &T::AccountId,
        recipient: &T::AccountId,
        recovery: TaoBalance,
        refund: TaoBalance,
    ) -> Result<(TaoBalance, TaoBalance), DispatchError> {
        let total = recovery
            .checked_add(&refund)
            .ok_or(Error::<T>::InsufficientLiquidity)?;
        if total.is_zero() {
            return Ok((TaoBalance::ZERO, TaoBalance::ZERO));
        }
        let credit = Self::withdraw_tao_as_credit(from, total)?;
        let (credit, recovered) =
            Self::resolve_lending_credit_or_recycle_dust(vault, credit, recovery)?;
        let (credit, refunded) =
            Self::resolve_lending_credit_or_recycle_dust(recipient, credit, refund)?;
        Self::recycle_credit(credit);
        Ok((recovered, refunded))
    }

    #[transactional]
    fn terminal_settle_short_tao(
        from: &T::AccountId,
        vault: &T::AccountId,
        recipient: &T::AccountId,
        recovery: TaoBalance,
        interest: TaoBalance,
        refund: TaoBalance,
    ) -> Result<(TaoBalance, TaoBalance, TaoBalance), DispatchError> {
        let total = recovery
            .checked_add(&interest)
            .and_then(|t| t.checked_add(&refund))
            .ok_or(Error::<T>::InsufficientLiquidity)?;
        if total.is_zero() {
            return Ok((TaoBalance::ZERO, TaoBalance::ZERO, TaoBalance::ZERO));
        }
        let credit = Self::withdraw_tao_as_credit(from, total)?;
        let burn: T::AccountId = T::BurnAccountId::get().into_account_truncating();
        let (credit, burned) =
            Self::resolve_lending_credit_or_recycle_dust(&burn, credit, interest)?;
        let (credit, recovered) =
            Self::resolve_lending_credit_or_recycle_dust(vault, credit, recovery)?;
        let (credit, refunded) =
            Self::resolve_lending_credit_or_recycle_dust(recipient, credit, refund)?;
        Self::recycle_credit(credit);
        Ok((recovered, burned, refunded))
    }

    fn subnet_pot_account(netuid: NetUid) -> Option<T::AccountId> {
        Self::get_subnet_account_id(netuid)
    }

    /// `TotalStake` follows only while the subnet is live because dissolution
    /// already dropped the pot from it.
    fn credit_pot_reserve(netuid: NetUid, amount: TaoBalance) {
        Self::increase_provided_tao_reserve(netuid, amount);
        if Self::if_subnet_exist(netuid) {
            TotalStake::<T>::mutate(|total| *total = total.saturating_add(amount));
        }
    }

    /// Credit `amount` from `from` to the subnet account and `SubnetTAO`.
    /// Works after the subnet is flagged.
    #[transactional]
    fn pay_into_pot(
        from: &T::AccountId,
        netuid: NetUid,
        amount: TaoBalance,
    ) -> Result<TaoBalance, DispatchError> {
        if amount.is_zero() {
            return Ok(TaoBalance::ZERO);
        }
        let subnet = Self::get_subnet_account_id(netuid).ok_or(Error::<T>::SubnetNotExists)?;
        let paid = Self::transfer_lending_tao_or_recycle_dust(from, &subnet, amount)?;
        <Self as LendingPoolInterface<T::AccountId>>::credit_pot_reserve(netuid, paid);
        Ok(paid)
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn benchmark_seed_pool(netuid: NetUid, tao: u64, alpha: u64) {
        use frame_support::traits::Currency;
        if !Self::if_subnet_exist(netuid) {
            NetworksAdded::<T>::insert(netuid, true);
            SubnetMechanism::<T>::insert(netuid, 1u16);
        }
        // Benchmark presets may register `netuid` without a start-call or with
        // a recent registration; opens would fail before measuring bodies.
        SubtokenEnabled::<T>::insert(netuid, true);
        NetworkRegisteredAt::<T>::insert(netuid, 0u64);
        SubnetTAO::<T>::set(netuid, tao.into());
        SubnetAlphaIn::<T>::set(netuid, alpha.into());
        // Short notional and interest use the moving price; settle it at spot.
        SubnetMovingPrice::<T>::insert(
            netuid,
            I96F32::saturating_from_num(tao)
                .checked_div(I96F32::saturating_from_num(alpha.max(1)))
                .unwrap_or_default(),
        );
        TotalStake::<T>::mutate(|total| *total = total.saturating_add(tao.into()));
        if let Some(subnet) = Self::get_subnet_account_id(netuid) {
            let _ = <T as Config>::Currency::make_free_balance_be(&subnet, tao.into());
        }
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn benchmark_mint_tao(who: &T::AccountId, amount: u64) {
        use frame_support::traits::Currency;
        let _ = <T as Config>::Currency::make_free_balance_be(who, amount.into());
    }
}

impl<T: Config> Pallet<T> {
    /// Smallest TAO input, at most `max_tao_in`, whose buy quote yields at least
    /// `alpha`. Grows from the constant-product estimate because skewed Balancer
    /// weights can need more, then bisects down.
    fn tao_for_exact_alpha(
        netuid: NetUid,
        alpha: AlphaBalance,
        max_tao_in: TaoBalance,
    ) -> Result<TaoBalance, DispatchError> {
        let want = alpha.to_u64();
        let x = SubnetAlphaIn::<T>::get(netuid).to_u64();
        let y = SubnetTAO::<T>::get(netuid).to_u64();
        ensure!(want < x, Error::<T>::InsufficientLiquidity);
        let cap = max_tao_in
            .min(T::SwapInterface::max_swap_input::<GetAlphaForTao<T>>(
                netuid,
            ))
            .to_u64();
        let fills = |tao: u64| {
            <Self as LendingPoolInterface<T::AccountId>>::quote_buy(netuid, tao.into())
                .is_ok_and(|out| out >= alpha)
        };
        let estimate = u128::from(want)
            .saturating_mul(u128::from(y))
            .checked_div(u128::from(x.saturating_sub(want)))
            .unwrap_or(u128::MAX)
            .saturating_add(1);
        let mut hi = u64::try_from(estimate).unwrap_or(u64::MAX).min(cap);
        while !fills(hi) {
            ensure!(hi < cap, Error::<T>::SlippageTooHigh);
            hi = hi.saturating_mul(2).min(cap);
        }
        let mut lo = 0u64;
        while hi.saturating_sub(lo) > 1 {
            let mid = lo.saturating_add(hi.saturating_sub(lo) / 2);
            if fills(mid) {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        Ok(hi.into())
    }

    fn destroy_custody_alpha(
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        netuid: NetUid,
        amount: AlphaBalance,
    ) -> DispatchResult {
        let removed =
            Self::decrease_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, netuid, amount);
        ensure!(removed == amount, Error::<T>::NotEnoughStakeToWithdraw);
        SubnetAlphaOut::<T>::try_mutate(netuid, |out| -> DispatchResult {
            ensure!(*out >= amount, Error::<T>::NotEnoughStakeToWithdraw);
            *out = out.saturating_sub(amount);
            Ok(())
        })
    }

    /// Spend `amount` of `credit` on `to`, recycling it when too small to
    /// create `to`. Returns the remaining credit and the amount credited.
    fn resolve_lending_credit_or_recycle_dust(
        to: &T::AccountId,
        credit: crate::coinbase::tao::CreditOf<T>,
        amount: TaoBalance,
    ) -> Result<(crate::coinbase::tao::CreditOf<T>, TaoBalance), DispatchError> {
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
            .map_err(|_credit| Error::<T>::InsufficientLiquidity)?;
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
}
