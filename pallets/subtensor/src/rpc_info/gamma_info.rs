//! Gamma reads for `GammaRuntimeApi`.

use frame_support::pallet_prelude::{Decode, Encode};
use frame_support::storage::{TransactionOutcome, with_transaction};
use subtensor_runtime_common::{AlphaBalance, GammaBalance, NetUid, TaoBalance};
use subtensor_swap_interface::SwapHandler;

use super::*;

/// A gamma deposit and what its spender can spend from it now.
#[freeze_struct("1fbec154e8c48c46")]
#[derive(Decode, Encode, PartialEq, Eq, Clone, Debug, TypeInfo)]
pub struct GammaDepositInfo<AccountId: TypeInfo + Encode + Decode> {
    pub netuid: NetUid,
    pub hotkey: AccountId,
    pub deposit: GammaDeposit<AccountId>,
    /// The deposit's balance within what its daily spend cap allows now.
    pub spendable: GammaBalance,
}

impl<T: Config> Pallet<T> {
    fn gamma_deposit_info(
        netuid: NetUid,
        hotkey: T::AccountId,
        deposit: GammaDeposit<T::AccountId>,
    ) -> GammaDepositInfo<T::AccountId> {
        let spendable = Self::gamma_allowance(&deposit, Self::get_current_block_as_u64())
            .map_or(deposit.balance, |allowance| allowance.min(deposit.balance));
        GammaDepositInfo {
            netuid,
            hotkey,
            deposit,
            spendable,
        }
    }

    /// The deposit at `hotkey` on `netuid`.
    pub fn get_gamma_deposit(
        netuid: NetUid,
        hotkey: T::AccountId,
    ) -> Option<GammaDepositInfo<T::AccountId>> {
        let deposit = GammaDeposits::<T>::get(netuid, &hotkey)?;
        Some(Self::gamma_deposit_info(netuid, hotkey, deposit))
    }

    /// Every deposit `coldkey` owns, on every subnet.
    pub fn get_gamma_deposits_for_coldkey(
        coldkey: &T::AccountId,
    ) -> Vec<GammaDepositInfo<T::AccountId>> {
        Self::get_all_subnet_netuids()
            .into_iter()
            .flat_map(|netuid| {
                ColdkeyGammaHotkeys::<T>::get(netuid, coldkey)
                    .into_iter()
                    .filter_map(move |hotkey| Self::get_gamma_deposit(netuid, hotkey))
            })
            .collect()
    }

    /// The alpha burned, TAO value and gamma `mint_gamma` would mint for `amount` now.
    pub fn sim_mint_gamma(
        netuid: NetUid,
        amount: TaoBalance,
    ) -> Option<(AlphaBalance, TaoBalance, GammaBalance)> {
        with_transaction(|| {
            TransactionOutcome::Rollback((|| {
                Self::ensure_gamma_mintable(netuid)?;
                Self::ensure_add_stake_input_within_swap_limit(netuid, amount)?;
                let mark = Self::gamma_mark(netuid);
                let swap =
                    Self::swap_tao_for_alpha(netuid, amount, T::SwapInterface::max_price(), false)?;
                let alpha = swap.amount_paid_out;
                let (value, gamma) = Self::gamma_for_alpha(alpha, mark)?;
                Ok::<_, DispatchError>((alpha, value, gamma))
            })())
        })
        .ok()
    }

    /// The alpha burned, TAO value and gamma `mint_gamma_with_stake` would mint for
    /// `alpha_amount` of stake now.
    pub fn sim_mint_gamma_with_stake(
        netuid: NetUid,
        alpha_amount: AlphaBalance,
    ) -> Option<(AlphaBalance, TaoBalance, GammaBalance)> {
        Self::ensure_gamma_mintable(netuid).ok()?;
        let mark = Self::gamma_mark(netuid);
        let (value, gamma) = Self::gamma_for_alpha(alpha_amount, mark).ok()?;
        Some((alpha_amount, value, gamma))
    }
}
