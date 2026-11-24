//! The deferred-dividend claim of runtime 494, for tests only.
//!
//! [`Pallet::claim_deferred_dividends_reference`] is the body that
//! `claim_deferred_dividends` had on `main` at `a68c6e8c6`. It reads the claimant's debts
//! from storage for each rate state, and each sale rebases the debts with a storage walk
//! over all rate states. The equivalence tests run a claim with this code and with the
//! current code on equal states, and compare the complete storage. The only additions are
//! the lines that count storage steps for the tests of the amount of work.

use super::*;
use substrate_fixed::types::{I64F64, I96F32};

impl<T: Config> Pallet<T> {
    /// A claimant can redeem their own deferred alpha without repricing the
    /// basket. Only that claimant bears the sale price/slippage, so this needs
    /// no common EMA window and cannot transfer value from basket shareholders.
    pub(super) fn claim_deferred_dividends_reference(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        ignore_minimum: bool,
    ) -> Result<(u64, u32), DispatchError> {
        let states: Vec<_> = DeferredBasketRates::<T>::iter_prefix(hotkey).collect();
        crate::tests::mock::inc_deferred_dividend_ops(states.len());
        Self::settle_deferred_debts(hotkey, coldkey)?;
        let mut total = 0u64;
        let mut rows = 0u32;
        for (netuid, state) in states {
            let stake =
                Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, NetUid::ROOT)
                    .to_u64();
            let debts = DeferredBasketDebts::<T>::get(hotkey, coldkey);
            crate::tests::mock::inc_deferred_dividend_ops(debts.len());
            let debt = debts
                .get(&netuid)
                .map_or(I64F64::from_num(0), |debt| debt.amount);
            let owed = state
                .rate
                .checked_mul(
                    I64F64::checked_from_num(stake).ok_or(sp_runtime::ArithmeticError::Overflow)?,
                )
                .and_then(|earned| earned.checked_sub(debt))
                .ok_or(sp_runtime::ArithmeticError::Overflow)?;
            if owed <= I64F64::from_num(0) {
                continue;
            }
            let alpha = owed
                .saturating_to_num::<u64>()
                .min(PendingBasketDeposits::<T>::get(hotkey, netuid).to_u64());
            if alpha == 0 {
                continue;
            }
            let value = Self::try_realizable_tao_for_alpha(netuid, alpha)?;
            let Some(value) = value else {
                continue;
            };
            if value == 0
                || (!ignore_minimum
                    && I96F32::from_num(value) < RootClaimableThreshold::<T>::get(NetUid::ROOT))
            {
                continue;
            }
            Self::ensure_staking_hotkeys_can_grow(coldkey, hotkey)?;
            let tao =
                frame_support::storage::with_storage_layer(|| -> Result<u64, DispatchError> {
                    // Reserve the paid entitlement before the sale and before its TAO
                    // becomes new root stake. The stake rebase below is separate.
                    DeferredBasketDebts::<T>::try_mutate(
                        hotkey,
                        coldkey,
                        |debts| -> DispatchResult {
                            crate::tests::mock::inc_deferred_dividend_ops(debts.len());
                            if let Some(debt) = debts.get_mut(&netuid) {
                                debt.amount = debt
                                    .amount
                                    .checked_add(
                                        I64F64::checked_from_num(alpha)
                                            .ok_or(sp_runtime::ArithmeticError::Overflow)?,
                                    )
                                    .ok_or(sp_runtime::ArithmeticError::Overflow)?;
                            } else {
                                debts
                                    .try_insert(
                                        netuid,
                                        DeferredBasketDebt {
                                            generation: state.generation,
                                            amount: I64F64::checked_from_num(alpha)
                                                .ok_or(sp_runtime::ArithmeticError::Overflow)?,
                                        },
                                    )
                                    .map_err(|_| Error::<T>::BasketDepositPending)?;
                                DeferredBasketRates::<T>::mutate(hotkey, netuid, |state| {
                                    if let Some(state) = state {
                                        state.debt_count = state.debt_count.saturating_add(1);
                                    }
                                });
                            }
                            Ok(())
                        },
                    )?;
                    let tao = Self::sell_basket_alpha_for_root_tao(netuid, alpha.into())?.to_u64();
                    ensure!(tao > 0, Error::<T>::AmountTooLow);
                    PendingBasketDeposits::<T>::mutate(hotkey, netuid, |pending| {
                        *pending = pending.saturating_sub(alpha.into())
                    });
                    SubnetRootSellTao::<T>::mutate(netuid, |total| {
                        *total = total.saturating_add(tao.into())
                    });
                    Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
                        hotkey,
                        coldkey,
                        NetUid::ROOT,
                        tao.into(),
                    );
                    Self::credit_root_reserves(tao.into());
                    Self::add_stake_adjust_root_claimed_for_hotkey_and_coldkey(
                        hotkey, coldkey, tao,
                    )?;
                    Self::touch_root_stake_age(coldkey, hotkey);
                    if PendingBasketDeposits::<T>::get(hotkey, netuid).is_zero() {
                        PendingBasketDeposits::<T>::remove(hotkey, netuid);
                        Self::convert_deferred_dividend(hotkey, netuid, 1, 0)?;
                    }
                    Ok(tao)
                })?;
            total = total
                .checked_add(tao)
                .ok_or(sp_runtime::ArithmeticError::Overflow)?;
            rows = rows.saturating_add(1);
        }
        let stake = Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, NetUid::ROOT)
            .to_u64();
        let owed_cash = DeferredBasketCashRate::<T>::get(hotkey)
            .checked_mul(
                I64F64::checked_from_num(stake).ok_or(sp_runtime::ArithmeticError::Overflow)?,
            )
            .ok_or(sp_runtime::ArithmeticError::Overflow)?
            .saturating_to_num::<i128>()
            .saturating_sub(DeferredBasketCashClaimed::<T>::get(hotkey, coldkey));
        let cash = u64::try_from(owed_cash)
            .unwrap_or(0)
            .min(DeferredBasketCashReserve::<T>::get(hotkey));
        if cash > 0
            && (ignore_minimum
                || I96F32::from_num(cash) >= RootClaimableThreshold::<T>::get(NetUid::ROOT))
        {
            Self::ensure_staking_hotkeys_can_grow(coldkey, hotkey)?;
            frame_support::storage::with_storage_layer(|| -> DispatchResult {
                DeferredBasketCashClaimed::<T>::try_mutate(
                    hotkey,
                    coldkey,
                    |claimed| -> DispatchResult {
                        *claimed = claimed
                            .checked_add(i128::from(cash))
                            .ok_or(sp_runtime::ArithmeticError::Overflow)?;
                        Ok(())
                    },
                )?;
                DeferredBasketCashReserve::<T>::mutate(hotkey, |reserve| {
                    *reserve = reserve.saturating_sub(cash)
                });
                Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
                    hotkey,
                    coldkey,
                    NetUid::ROOT,
                    cash.into(),
                );
                Self::add_stake_adjust_root_claimed_for_hotkey_and_coldkey(hotkey, coldkey, cash)?;
                Self::touch_root_stake_age(coldkey, hotkey);
                Ok(())
            })?;
            total = total
                .checked_add(cash)
                .ok_or(sp_runtime::ArithmeticError::Overflow)?;
        }
        if total > 0 {
            BasketRedeemedTao::<T>::mutate(hotkey, |redeemed| {
                *redeemed = redeemed.saturating_add(total.into())
            });
            Self::deposit_event(Event::BasketClaimed {
                hotkey: hotkey.clone(),
                coldkey: coldkey.clone(),
                tao: total.into(),
            });
        }
        Ok((total, rows))
    }
}
