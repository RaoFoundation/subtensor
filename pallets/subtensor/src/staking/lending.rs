//! Lift / readd / custody adapter for escrowed lending (Jacob v5.1).
//!
//! No assets are minted by reserve extraction. Lift pulls proportional Balancer
//! legs and debits `SubnetTAO` / `SubnetAlphaIn` / `TotalStake`; readd reverses
//! that. Forced buy/sell move custody legs through the AMM into escrow.
use super::*;
use frame_support::{
    traits::tokens::{Fortitude, Preservation, fungible::Inspect},
    transactional,
};
use pallet_lending::LendingPoolInterface;
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

    fn max_buy_input(netuid: NetUid) -> TaoBalance {
        T::SwapInterface::max_swap_input::<GetAlphaForTao<T>>(netuid)
    }

    fn max_sell_input(netuid: NetUid) -> AlphaBalance {
        T::SwapInterface::max_swap_input::<GetTaoForAlpha<T>>(netuid)
    }

    fn quote_may_round_to_zero(error: &DispatchError) -> bool {
        *error == Error::<T>::AmountTooLow.into()
            || T::SwapInterface::classify_failure(error)
                == subtensor_swap_interface::SwapFailureKind::TerminalLiquidity
    }

    fn buy_spendable_tao(account: &T::AccountId) -> TaoBalance {
        <T as Config>::Currency::reducible_balance(
            account,
            Preservation::Preserve,
            Fortitude::Polite,
        )
    }

    #[transactional]
    fn lock_alpha_collateral(
        from: &T::AccountId,
        source_hotkey: &T::AccountId,
        escrow: &T::AccountId,
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
        <Self as subtensor_swap_interface::OrderSwapInterface<T::AccountId>>::transfer_staked_alpha(
            from,
            source_hotkey,
            escrow,
            custody,
            netuid,
            amount,
            false,
            false,
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
        Self::lock_alpha_collateral(from, source_hotkey, vault, custody, netuid, amount)
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
        // Kept for trait compatibility; §13 destroy path uses `destroy_custody_alpha`.
        <Self as subtensor_swap_interface::OrderSwapInterface<T::AccountId>>::transfer_staked_alpha(
            from, hotkey, to, hotkey, netuid, amount, false, false,
        )
    }

    fn pool_tao(netuid: NetUid) -> TaoBalance {
        SubnetTAO::<T>::get(netuid)
    }

    fn pool_alpha(netuid: NetUid) -> AlphaBalance {
        SubnetAlphaIn::<T>::get(netuid)
    }

    fn quote_weight_parts(netuid: NetUid) -> u64 {
        T::SwapInterface::balancer_quote_weight(netuid)
    }

    fn buy_depth_threshold(netuid: NetUid, ref_tao: TaoBalance, move_bps: u16) -> TaoBalance {
        T::SwapInterface::buy_depth_threshold(netuid, ref_tao, move_bps)
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

    #[transactional]
    fn lift_and_debit(
        netuid: NetUid,
        g_numer: u64,
        g_denom: u64,
        custody_cold: &T::AccountId,
        custody_hot: &T::AccountId,
    ) -> Result<(AlphaBalance, TaoBalance), DispatchError> {
        ensure!(
            Self::if_subnet_exist(netuid) || DissolveCleanupQueue::<T>::get().contains(&netuid),
            Error::<T>::SubnetNotExists
        );
        let (lift_alpha, lift_tao) =
            T::SwapInterface::lift_protocol_liquidity(netuid, g_numer, g_denom)?;
        Self::decrease_provided_tao_reserve(netuid, lift_tao);
        Self::decrease_provided_alpha_reserve(netuid, lift_alpha);
        if Self::if_subnet_exist(netuid) {
            TotalStake::<T>::mutate(|total| *total = total.saturating_sub(lift_tao));
        }
        let subnet = Self::get_subnet_account_id(netuid).ok_or(Error::<T>::SubnetNotExists)?;
        if !lift_tao.is_zero() {
            Self::transfer_tao(&subnet, custody_cold, lift_tao)?;
        }
        if !lift_alpha.is_zero() {
            ensure!(
                lift_alpha.to_u64() <= i64::MAX as u64
                    && Self::try_increase_stake_for_hotkey_and_coldkey_on_subnet(
                        custody_hot,
                        netuid,
                        lift_alpha
                    ),
                Error::<T>::NotEnoughStakeToWithdraw
            );
            Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
                custody_hot,
                custody_cold,
                netuid,
                lift_alpha,
            );
            SubnetAlphaOut::<T>::mutate(netuid, |out| *out = out.saturating_add(lift_alpha));
        }
        Ok((lift_alpha, lift_tao))
    }

    #[transactional]
    fn readd_and_credit(
        netuid: NetUid,
        custody_cold: &T::AccountId,
        custody_hot: &T::AccountId,
        tao: TaoBalance,
        alpha: AlphaBalance,
    ) -> DispatchResult {
        ensure!(
            Self::if_subnet_exist(netuid) || DissolveCleanupQueue::<T>::get().contains(&netuid),
            Error::<T>::SubnetNotExists
        );
        let subnet = Self::get_subnet_account_id(netuid).ok_or(Error::<T>::SubnetNotExists)?;
        if !tao.is_zero() {
            Self::transfer_tao(custody_cold, &subnet, tao)?;
        }
        if !alpha.is_zero() {
            let removed = Self::decrease_stake_for_hotkey_and_coldkey_on_subnet(
                custody_hot,
                custody_cold,
                netuid,
                alpha,
            );
            ensure!(removed == alpha, Error::<T>::NotEnoughStakeToWithdraw);
            SubnetAlphaOut::<T>::try_mutate(netuid, |out| -> DispatchResult {
                ensure!(*out >= alpha, Error::<T>::NotEnoughStakeToWithdraw);
                *out = out.saturating_sub(alpha);
                Ok(())
            })?;
        }
        let (d_tao, d_alpha) = T::SwapInterface::readd_protocol_liquidity(netuid, tao, alpha);
        Self::increase_provided_tao_reserve(netuid, d_tao);
        Self::increase_provided_alpha_reserve(netuid, d_alpha);
        // Live subnets keep TotalStake in sync; dissolved pots only track SubnetTAO.
        if Self::if_subnet_exist(netuid) && !d_tao.is_zero() {
            TotalStake::<T>::mutate(|total| *total = total.saturating_add(d_tao));
        }
        Ok(())
    }

    #[transactional]
    fn forced_buy_into_custody(
        custody_cold: &T::AccountId,
        custody_hot: &T::AccountId,
        netuid: NetUid,
        tao: TaoBalance,
    ) -> Result<AlphaBalance, DispatchError> {
        <Self as subtensor_swap_interface::OrderSwapInterface<T::AccountId>>::buy_alpha(
            custody_cold,
            custody_hot,
            netuid,
            tao,
            u64::MAX.into(),
            false,
        )
    }

    #[transactional]
    fn forced_sell_into_escrow(
        custody_cold: &T::AccountId,
        custody_hot: &T::AccountId,
        escrow_cold: &T::AccountId,
        netuid: NetUid,
        alpha: AlphaBalance,
        min_tao_out: TaoBalance,
    ) -> Result<TaoBalance, DispatchError> {
        // Sell from custody into the pool, then move free TAO onto the escrow coldkey.
        let tao = <Self as subtensor_swap_interface::OrderSwapInterface<T::AccountId>>::sell_alpha(
            custody_cold,
            custody_hot,
            netuid,
            alpha,
            TaoBalance::ZERO,
            false,
        )?;
        ensure!(tao >= min_tao_out, Error::<T>::SlippageTooHigh);
        if *custody_cold != *escrow_cold && !tao.is_zero() {
            Self::transfer_tao(custody_cold, escrow_cold, tao)?;
        }
        Ok(tao)
    }

    fn unlocked_stake(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        netuid: NetUid,
    ) -> AlphaBalance {
        let stake = Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, netuid);
        let available = Self::available_to_unstake(coldkey, netuid);
        stake.min(available)
    }

    fn ensure_hotkey_account(hotkey: &T::AccountId) -> DispatchResult {
        ensure!(
            Self::hotkey_account_exists(hotkey),
            Error::<T>::HotKeyAccountNotExists
        );
        Ok(())
    }

    fn ensure_collateral_stake(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        netuid: NetUid,
        amount: AlphaBalance,
    ) -> DispatchResult {
        Self::ensure_hotkey_account(hotkey)?;
        Self::ensure_available_to_unstake(coldkey, netuid, amount)?;
        Self::ensure_hotkey_covers_collateral(coldkey, hotkey, netuid, amount)?;
        Ok(())
    }

    #[transactional]
    fn unstake_for_raise(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        netuid: NetUid,
        alpha: AlphaBalance,
        min_tao_out: TaoBalance,
    ) -> Result<TaoBalance, DispatchError> {
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        Self::ensure_subtoken_enabled(netuid)?;
        Self::ensure_available_to_unstake(coldkey, netuid, alpha)?;
        let tao = Self::unstake_from_subnet(
            hotkey,
            coldkey,
            coldkey,
            netuid,
            alpha,
            TaoBalance::ZERO,
            false,
            false,
        )?;
        ensure!(tao >= min_tao_out, Error::<T>::SlippageTooHigh);
        Ok(tao)
    }

    fn eligible_alpha_for_dissolution(netuid: NetUid) -> Result<u128, DispatchError> {
        // Ordinary staker-side claims only (matches main's legacy/new split).
        // Lending settlement uses `all_alpha_for_lending_settlement` instead.
        let reg_at: u64 = NetworkRegisteredAt::<T>::get(netuid);
        let cutoff: u64 = TaoInRefundDeploymentBlock::<T>::get();
        let protocol = SubnetProtocolAlpha::<T>::get(netuid).to_u64() as u128;
        let base = if reg_at > cutoff {
            (SubnetAlphaIn::<T>::get(netuid).to_u64() as u128).saturating_add(protocol)
        } else {
            protocol
        };
        Ok(base)
    }

    fn all_alpha_for_lending_settlement(netuid: NetUid) -> u128 {
        // §13: always include pool alpha on every subnet (legacy and new).
        u128::from(SubnetAlphaIn::<T>::get(netuid).to_u64())
            .saturating_add(u128::from(SubnetAlphaOut::<T>::get(netuid).to_u64()))
    }

    fn moving_alpha_price(netuid: NetUid) -> U64F64 {
        Self::get_moving_alpha_price(netuid)
    }

    fn live_subnets() -> alloc::vec::Vec<NetUid> {
        Self::get_all_subnet_netuids()
            .into_iter()
            .filter(|netuid| !netuid.is_root())
            .collect()
    }

    #[transactional]
    fn pay_from_pot(
        netuid: NetUid,
        to: &T::AccountId,
        amount: TaoBalance,
    ) -> Result<TaoBalance, DispatchError> {
        if amount.is_zero() {
            return Ok(TaoBalance::ZERO);
        }
        let subnet = Self::get_subnet_account_id(netuid).ok_or(Error::<T>::SubnetNotExists)?;
        ensure!(
            SubnetTAO::<T>::get(netuid) >= amount,
            Error::<T>::InsufficientLiquidity
        );
        Self::decrease_provided_tao_reserve(netuid, amount);
        // Live expiry/settle payouts leave the pool; keep TotalStake aligned with
        // SubnetTAO. Dissolved pots already dropped out of TotalStake earlier.
        if Self::if_subnet_exist(netuid) {
            TotalStake::<T>::mutate(|total| *total = total.saturating_sub(amount));
        }
        Self::transfer_lending_tao_or_recycle_dust(&subnet, to, amount)
    }

    #[transactional]
    fn burn_from_pot(netuid: NetUid, amount: TaoBalance) -> DispatchResult {
        if amount.is_zero() {
            return Ok(());
        }
        let subnet = Self::get_subnet_account_id(netuid).ok_or(Error::<T>::SubnetNotExists)?;
        ensure!(
            SubnetTAO::<T>::get(netuid) >= amount,
            Error::<T>::InsufficientLiquidity
        );
        Self::decrease_provided_tao_reserve(netuid, amount);
        if Self::if_subnet_exist(netuid) {
            TotalStake::<T>::mutate(|total| *total = total.saturating_sub(amount));
        }
        Self::burn_interest_tao(&subnet, amount)
    }

    #[transactional]
    fn destroy_custody_alpha(
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        netuid: NetUid,
        amount: AlphaBalance,
    ) -> DispatchResult {
        if amount.is_zero() {
            return Ok(());
        }
        let removed =
            Self::decrease_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, netuid, amount);
        ensure!(removed == amount, Error::<T>::NotEnoughStakeToWithdraw);
        SubnetAlphaOut::<T>::try_mutate(netuid, |out| -> DispatchResult {
            ensure!(*out >= amount, Error::<T>::NotEnoughStakeToWithdraw);
            *out = out.saturating_sub(amount);
            Ok(())
        })?;
        Ok(())
    }

    fn network_registered_at(netuid: NetUid) -> u64 {
        NetworkRegisteredAt::<T>::get(netuid)
    }

    fn subtoken_enabled(netuid: NetUid) -> bool {
        SubtokenEnabled::<T>::get(netuid)
    }

    fn network_alive(netuid: NetUid) -> bool {
        // Registration-time seed: do not require start-call / subtoken yet.
        !netuid.is_root() && Self::if_subnet_exist(netuid) && SubnetMechanism::<T>::get(netuid) == 1
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn benchmark_seed_pool(netuid: NetUid, tao: u64, alpha: u64) {
        use frame_support::traits::Currency;
        if !Self::if_subnet_exist(netuid) {
            NetworksAdded::<T>::insert(netuid, true);
            SubtokenEnabled::<T>::insert(netuid, true);
            SubnetMechanism::<T>::insert(netuid, 1u16);
            NetworkRegisteredAt::<T>::insert(netuid, 0u64);
        }
        SubnetTAO::<T>::set(netuid, tao.into());
        SubnetAlphaIn::<T>::set(netuid, alpha.into());
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

    #[cfg(feature = "runtime-benchmarks")]
    fn benchmark_mint_alpha(
        who: &T::AccountId,
        hotkey: &T::AccountId,
        netuid: NetUid,
        amount: u64,
    ) {
        let alpha: AlphaBalance = amount.into();
        let _ = Self::try_increase_stake_for_hotkey_and_coldkey_on_subnet(hotkey, netuid, alpha);
        Self::increase_stake_for_hotkey_and_coldkey_on_subnet(hotkey, who, netuid, alpha);
        SubnetAlphaOut::<T>::mutate(netuid, |out| *out = out.saturating_add(alpha));
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
