use super::*;
use frame_support::transactional;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance};
use subtensor_swap_interface::{Order, OrderSwapInterface, SwapHandler, SwapResult};

impl<T: Config> OrderSwapInterface<T::AccountId> for Pallet<T> {
    #[transactional]
    fn buy_alpha(
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        netuid: NetUid,
        tao_amount: TaoBalance,
        limit_price: TaoBalance,
        validate: bool,
    ) -> Result<AlphaBalance, DispatchError> {
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        Self::ensure_subtoken_enabled(netuid)?;
        if validate {
            ensure!(
                Self::hotkey_account_exists(hotkey),
                Error::<T>::HotKeyAccountNotExists
            );
            ensure!(
                tao_amount >= DefaultMinStake::<T>::get(),
                Error::<T>::AmountTooLow
            );
            ensure!(
                Self::can_remove_balance_from_coldkey_account(coldkey, tao_amount),
                Error::<T>::NotEnoughBalanceToStake
            );
        }
        // `limit_price` is already in ×10⁹ scale (same as the `current_alpha_price` RPC
        // endpoint), which is also the scale the AMM uses for its price_limit argument.
        // Pass it directly without any scaling.  u64::MAX means "no ceiling".
        let amm_limit = limit_price;
        // Stable subnets (mechanism_id != 1) are always 1:1 and never stop early.
        if SubnetMechanism::<T>::get(netuid) == 1 {
            let sim_order = GetAlphaForTao::<T>::with_amount(tao_amount);
            let sim: SwapResult<TaoBalance, AlphaBalance> =
                T::SwapInterface::swap(netuid.into(), sim_order, amm_limit, false, true)?;
            ensure!(
                sim.amount_paid_in.saturating_add(sim.fee_paid) >= tao_amount,
                Error::<T>::SlippageTooHigh
            );
        }
        let alpha_out =
            Self::stake_into_subnet(hotkey, coldkey, netuid, tao_amount, amm_limit, false)?;
        Ok(alpha_out)
    }

    #[transactional]
    fn sell_alpha(
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        netuid: NetUid,
        alpha_amount: AlphaBalance,
        limit_price: TaoBalance,
        validate: bool,
    ) -> Result<TaoBalance, DispatchError> {
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        Self::ensure_subtoken_enabled(netuid)?;
        if validate {
            Self::validate_remove_stake(
                coldkey,
                hotkey,
                netuid,
                alpha_amount,
                alpha_amount,
                false,
            )?;
        }
        // `limit_price` is already in ×10⁹ scale (same as the `current_alpha_price` RPC
        // endpoint), which is also the scale the AMM uses for its price_limit argument.
        // Pass it directly without any scaling.  0 means "no floor".
        let amm_limit = limit_price;
        // Stable subnets (mechanism_id != 1) are always 1:1 and never stop early.
        if SubnetMechanism::<T>::get(netuid) == 1 {
            let sim_order = GetTaoForAlpha::<T>::with_amount(alpha_amount);
            let sim: SwapResult<AlphaBalance, TaoBalance> =
                T::SwapInterface::swap(netuid.into(), sim_order, amm_limit, false, true)?;
            ensure!(
                sim.amount_paid_in.saturating_add(sim.fee_paid) >= alpha_amount,
                Error::<T>::SlippageTooHigh
            );
        }
        let tao_out = Self::unstake_from_subnet(
            hotkey,
            coldkey,
            coldkey,
            netuid,
            alpha_amount,
            amm_limit,
            false,
        )?;
        Ok(tao_out)
    }

    fn current_alpha_price(netuid: NetUid) -> U64F64 {
        T::SwapInterface::current_alpha_price(netuid)
    }

    fn transfer_tao(from: &T::AccountId, to: &T::AccountId, amount: TaoBalance) -> DispatchResult {
        Pallet::<T>::transfer_tao(from, to, amount)
    }

    #[transactional]
    fn transfer_staked_alpha(
        from_coldkey: &T::AccountId,
        from_hotkey: &T::AccountId,
        to_coldkey: &T::AccountId,
        to_hotkey: &T::AccountId,
        netuid: NetUid,
        amount: AlphaBalance,
        validate_sender: bool,
        validate_receiver: bool,
    ) -> DispatchResult {
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        Self::ensure_subtoken_enabled(netuid)?;
        if validate_sender {
            ensure!(
                Self::hotkey_account_exists(from_hotkey),
                Error::<T>::HotKeyAccountNotExists
            );
            ensure!(!amount.is_zero(), Error::<T>::AmountTooLow);
            let tao_equiv = T::SwapInterface::current_alpha_price(netuid)
                .saturating_mul(U64F64::saturating_from_num(amount.to_u64()))
                .saturating_to_num::<u64>();
            ensure!(
                TaoBalance::from(tao_equiv) >= DefaultMinStake::<T>::get(),
                Error::<T>::AmountTooLow
            );
            Self::ensure_available_to_unstake(from_coldkey, netuid, amount)?;
        }

        if validate_receiver {
            ensure!(
                Self::hotkey_account_exists(to_hotkey),
                Error::<T>::HotKeyAccountNotExists
            );
        }

        let available =
            Self::get_stake_for_hotkey_and_coldkey_on_subnet(from_hotkey, from_coldkey, netuid);
        ensure!(available >= amount, Error::<T>::NotEnoughStakeToWithdraw);
        Self::decrease_stake_for_hotkey_and_coldkey_on_subnet(
            from_hotkey,
            from_coldkey,
            netuid,
            amount,
        );
        Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
            to_hotkey, to_coldkey, netuid, amount,
        );
        LastColdkeyHotkeyStakeBlock::<T>::insert(
            to_coldkey,
            to_hotkey,
            Self::get_current_block_as_u64(),
        );
        Ok(())
    }

    fn register_pallet_hotkey(coldkey: &T::AccountId, hotkey: &T::AccountId) -> DispatchResult {
        Self::create_account_if_non_existent(coldkey, hotkey)
    }

    fn pallet_hotkey_registered(coldkey: &T::AccountId, hotkey: &T::AccountId) -> bool {
        Self::coldkey_owns_hotkey(coldkey, hotkey)
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn set_up_netuid_for_benchmark(netuid: NetUid) {
        if !Self::if_subnet_exist(netuid) {
            Self::init_new_network(netuid, 100);
        }
        SubtokenEnabled::<T>::insert(netuid, true);
        // Seed pool reserves so the AMM price is well-defined and swaps return non-zero.
        SubnetTAO::<T>::insert(netuid, TaoBalance::from(1_000_000_000_000_u64));
        SubnetAlphaIn::<T>::insert(netuid, AlphaBalance::from(1_000_000_000_000_u64));
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn set_up_acc_for_benchmark(hotkey: &T::AccountId, coldkey: &T::AccountId) {
        let _ = Self::create_account_if_non_existent(coldkey, hotkey);
        let credit = Self::mint_tao(TaoBalance::from(1_000_000_000_000_u64));
        let _ = Self::spend_tao(coldkey, credit, TaoBalance::from(1_000_000_000_000_u64));
    }
}
