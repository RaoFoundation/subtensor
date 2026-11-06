//! Entitlements to dividend alpha whose conversion into basket shares is deferred.
//!
//! For each origin/generation, `owed_alpha = root_stake * rate - debt`.
//! A root stake change of `delta` adds `delta * rate` to the account's signed debt,
//! so neither arrivals nor departures change the alpha already earned. Rates are
//! activated lazily before the first stake/fund change that cannot settle a batch;
//! subsequent epoch credits record their earning stake immediately. Unsnapshotted
//! sub-threshold dust retains the existing queue policy.
//!
//! Once conversion is safe, `F = minted_shares / remaining_alpha` advances
//! BasketRate by `rate * F`. Signed debts become BasketClaimed corrections by
//! `debt * F`, lazily on the account's next operation. A generation's conversion
//! remains until every debt using it is consumed. This avoids walking stakers.
//! A claimant may instead sell their own alpha directly. Dissolution uses the
//! same accounting with a separate TAO accumulator, outside the basket NAV.

use super::*;
use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use scale_info::TypeInfo;
use substrate_fixed::types::{I64F64, I96F32};
use subtensor_swap_interface::SwapHandler;

/// Unpriced alpha earned per unit of real root stake. Stake changes adjust a
/// separate signed debt, preserving the entitlement earned before the change.
#[derive(Clone, Encode, Decode, DecodeWithMemTracking, MaxEncodedLen, TypeInfo)]
pub struct DeferredBasketState {
    pub generation: u64,
    pub rate: I64F64,
    pub debt_count: u32,
}

#[derive(Clone, Encode, Decode, DecodeWithMemTracking, MaxEncodedLen, TypeInfo)]
pub struct DeferredBasketDebt {
    pub generation: u64,
    pub amount: I64F64,
}

/// Conversion is retained until every signed debt has been converted to a
/// BasketClaimed watermark. Accounts with unchanged stake need no debt row.
#[derive(Clone, Encode, Decode, DecodeWithMemTracking, MaxEncodedLen, TypeInfo)]
pub struct DeferredBasketConversion {
    pub shares_per_alpha: I64F64,
    pub cash_per_alpha: I64F64,
    pub debt_count: u32,
}

impl<T: Config> Pallet<T> {
    /// Make a pending credit independent of future root stake and escrow changes.
    /// The fund's own dividend is credited in alpha immediately; it belongs to
    /// the shareholders who held the fund when the dividend was earned.
    pub(crate) fn snapshot_deferred_dividends(hotkey: &T::AccountId) -> DispatchResult {
        let pending: Vec<_> = PendingBasketDeposits::<T>::iter_prefix(hotkey)
            .take((crate::MAX_ROOT_CLAIM_WORK as usize).saturating_add(1))
            .collect();
        ensure!(
            pending.len() <= crate::MAX_ROOT_CLAIM_WORK as usize,
            Error::<T>::BasketDepositPending
        );
        for (netuid, alpha) in pending {
            if !DeferredBasketRates::<T>::contains_key(hotkey, netuid) {
                Self::record_deferred_dividend(hotkey, netuid, alpha)?;
            }
        }
        Ok(())
    }

    fn record_deferred_dividend(
        hotkey: &T::AccountId,
        netuid: NetUid,
        alpha: AlphaBalance,
    ) -> DispatchResult {
        if alpha.is_zero() {
            return Ok(());
        }
        ensure!(
            DeferredBasketRates::<T>::contains_key(hotkey, netuid)
                || DeferredBasketRates::<T>::iter_prefix(hotkey)
                    .take(crate::MAX_ROOT_CLAIM_WORK as usize)
                    .count()
                    < crate::MAX_ROOT_CLAIM_WORK as usize,
            Error::<T>::BasketDepositPending
        );
        let escrow = Self::get_beta_escrow_account_id();
        let escrow_stake =
            Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, &escrow, NetUid::ROOT)
                .to_u64();
        let total = Self::get_stake_for_hotkey_on_subnet(hotkey, NetUid::ROOT).to_u64();
        let root = total.saturating_sub(escrow_stake);
        let real_alpha = Self::mul_div_u64(alpha.to_u64(), root, total);
        let fund_alpha = alpha.to_u64().saturating_sub(real_alpha);
        if fund_alpha > 0 && escrow_stake > 0 && BasketShares::<T>::get(hotkey) > 0 {
            Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
                hotkey,
                &escrow,
                netuid,
                fund_alpha.into(),
            );
        } else if fund_alpha > 0 {
            Self::recycle_subnet_alpha(netuid, fund_alpha.into());
        }
        if real_alpha == 0 {
            PendingBasketDeposits::<T>::remove(hotkey, netuid);
            return Ok(());
        }
        let increment = I64F64::checked_from_num(real_alpha)
            .ok_or(sp_runtime::ArithmeticError::Overflow)?
            .checked_div(
                I64F64::checked_from_num(root).ok_or(sp_runtime::ArithmeticError::Overflow)?,
            )
            .ok_or(sp_runtime::ArithmeticError::Overflow)?;
        let state = match DeferredBasketRates::<T>::get(hotkey, netuid) {
            Some(mut state) => {
                state.rate = state
                    .rate
                    .checked_add(increment)
                    .ok_or(sp_runtime::ArithmeticError::Overflow)?;
                state
            }
            None => {
                let generation = DeferredBasketGeneration::<T>::get(hotkey)
                    .checked_add(1)
                    .ok_or(sp_runtime::ArithmeticError::Overflow)?;
                DeferredBasketGeneration::<T>::insert(hotkey, generation);
                DeferredBasketState {
                    generation,
                    rate: increment,
                    debt_count: 0,
                }
            }
        };
        DeferredBasketRates::<T>::insert(hotkey, netuid, state);
        // Dissolution scans this fund index even if all shareholders/root
        // stakers have exited and only deferred dividend rights remain.
        BasketShares::<T>::mutate(hotkey, |_| {});
        PendingBasketDeposits::<T>::insert(hotkey, netuid, AlphaBalance::from(real_alpha));
        Ok(())
    }

    /// Coinbase uses this entry point; a retry must only requeue alpha, never
    /// record the same earned dividend a second time.
    pub(crate) fn enqueue_root_dividend(
        hotkey: &T::AccountId,
        netuid: NetUid,
        alpha: AlphaBalance,
    ) {
        if DeferredBasketRates::<T>::iter_prefix(hotkey)
            .next()
            .is_none()
        {
            Self::enqueue_basket_deposit(hotkey, netuid, alpha);
            return;
        }
        let old = PendingBasketDeposits::<T>::get(hotkey, netuid);
        // All writes, including the escrow yield, roll back on accounting failure.
        let outcome = frame_support::storage::with_transaction(|| {
            match Self::record_deferred_dividend(hotkey, netuid, alpha) {
                Ok(()) => {
                    PendingBasketDeposits::<T>::mutate(hotkey, netuid, |pending| {
                        *pending = pending.saturating_add(old)
                    });
                    frame_support::storage::TransactionOutcome::Commit(Ok(()))
                }
                Err(error) => frame_support::storage::TransactionOutcome::Rollback(Err(error)),
            }
        });
        // Overflow is not a reason to silently assign the credit to a later
        // claimant base. Leave the original accounting intact and recycle it.
        if outcome.is_err() {
            Self::recycle_subnet_alpha(netuid, alpha);
        }
    }

    fn converted_debt(debt: &DeferredBasketDebt, factor: I64F64) -> Result<i128, DispatchError> {
        // Round toward +infinity: negative corrections cannot grant more shares
        // than earned, positive corrections cannot let a new staker capture dust.
        let shares = debt
            .amount
            .checked_mul(factor)
            .ok_or(sp_runtime::ArithmeticError::Overflow)?;
        Ok(shares
            .checked_ceil()
            .ok_or(sp_runtime::ArithmeticError::Overflow)?
            .saturating_to_num::<i128>())
    }

    pub(super) fn deferred_claimed_correction(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
    ) -> i128 {
        DeferredBasketDebts::<T>::get(hotkey, coldkey)
            .iter()
            .fold(
                0i128,
                |sum, (netuid, debt)| match DeferredBasketConversions::<T>::get((
                    hotkey,
                    netuid,
                    debt.generation,
                )) {
                    Some(conversion) => sum.saturating_add(
                        Self::converted_debt(debt, conversion.shares_per_alpha)
                            .unwrap_or(i128::MAX),
                    ),
                    None => sum,
                },
            )
    }

    pub(super) fn settle_deferred_debts(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
    ) -> DispatchResult {
        frame_support::storage::with_storage_layer(|| {
            DeferredBasketDebts::<T>::try_mutate_exists(
                hotkey,
                coldkey,
                |entry| -> DispatchResult {
                    let Some(debts) = entry else {
                        return Ok(());
                    };
                    let mut completed = Vec::new();
                    for (netuid, debt) in debts.iter() {
                        if let Some(mut conversion) =
                            DeferredBasketConversions::<T>::get((hotkey, netuid, debt.generation))
                        {
                            let correction =
                                Self::converted_debt(debt, conversion.shares_per_alpha)?;
                            BasketClaimed::<T>::try_mutate(
                                hotkey,
                                coldkey,
                                |claimed| -> DispatchResult {
                                    *claimed = claimed
                                        .checked_add(correction)
                                        .ok_or(sp_runtime::ArithmeticError::Overflow)?;
                                    Ok(())
                                },
                            )?;
                            let cash = Self::converted_debt(debt, conversion.cash_per_alpha)?;
                            if cash != 0 {
                                DeferredBasketCashClaimed::<T>::try_mutate(
                                    hotkey,
                                    coldkey,
                                    |claimed| -> DispatchResult {
                                        *claimed = claimed
                                            .checked_add(cash)
                                            .ok_or(sp_runtime::ArithmeticError::Overflow)?;
                                        Ok(())
                                    },
                                )?;
                            }
                            conversion.debt_count = conversion.debt_count.saturating_sub(1);
                            if conversion.debt_count == 0 {
                                DeferredBasketConversions::<T>::remove((
                                    hotkey,
                                    netuid,
                                    debt.generation,
                                ));
                            } else {
                                DeferredBasketConversions::<T>::insert(
                                    (hotkey, netuid, debt.generation),
                                    conversion,
                                );
                            }
                            completed.push(*netuid);
                        }
                    }
                    for netuid in completed {
                        debts.remove(&netuid);
                    }
                    if debts.is_empty() {
                        *entry = None;
                    }
                    Ok(())
                },
            )
        })
    }

    pub(crate) fn rebase_deferred_dividends(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        delta: i128,
    ) -> DispatchResult {
        Self::settle_deferred_debts(hotkey, coldkey)?;
        let cash = DeferredBasketCashRate::<T>::get(hotkey)
            .checked_mul(
                I64F64::checked_from_num(delta).ok_or(sp_runtime::ArithmeticError::Overflow)?,
            )
            .ok_or(sp_runtime::ArithmeticError::Overflow)?;
        if cash != I64F64::from_num(0) {
            DeferredBasketCashClaimed::<T>::try_mutate(
                hotkey,
                coldkey,
                |claimed| -> DispatchResult {
                    *claimed = claimed
                        .checked_add(
                            cash.checked_ceil()
                                .ok_or(sp_runtime::ArithmeticError::Overflow)?
                                .saturating_to_num::<i128>(),
                        )
                        .ok_or(sp_runtime::ArithmeticError::Overflow)?;
                    Ok(())
                },
            )?;
        }
        if DeferredBasketRates::<T>::iter_prefix(hotkey)
            .next()
            .is_none()
        {
            return Ok(());
        }
        DeferredBasketDebts::<T>::try_mutate(hotkey, coldkey, |debts| -> DispatchResult {
            for (netuid, mut state) in DeferredBasketRates::<T>::iter_prefix(hotkey) {
                let correction = state
                    .rate
                    .checked_mul(
                        I64F64::checked_from_num(delta)
                            .ok_or(sp_runtime::ArithmeticError::Overflow)?,
                    )
                    .ok_or(sp_runtime::ArithmeticError::Overflow)?;
                if let Some(debt) = debts.get_mut(&netuid) {
                    ensure!(
                        debt.generation == state.generation,
                        DispatchError::Other("unsettled dividend generation")
                    );
                    debt.amount = debt
                        .amount
                        .checked_add(correction)
                        .ok_or(sp_runtime::ArithmeticError::Overflow)?;
                } else {
                    debts
                        .try_insert(
                            netuid,
                            DeferredBasketDebt {
                                generation: state.generation,
                                amount: correction,
                            },
                        )
                        .map_err(|_| Error::<T>::BasketDepositPending)?;
                    state.debt_count = state
                        .debt_count
                        .checked_add(1)
                        .ok_or(sp_runtime::ArithmeticError::Overflow)?;
                    DeferredBasketRates::<T>::insert(hotkey, netuid, state);
                }
            }
            Ok(())
        })
    }

    /// Bind a settled credit's share mint to the root stake that earned its alpha,
    /// without iterating stakers. Closed generations are applied lazily per coldkey.
    pub(super) fn convert_deferred_dividend(
        hotkey: &T::AccountId,
        netuid: NetUid,
        alpha: u64,
        shares: u64,
    ) -> DispatchResult {
        let state = DeferredBasketRates::<T>::take(hotkey, netuid)
            .ok_or(DispatchError::Other("missing dividend entitlement"))?;
        let factor = I64F64::checked_from_num(shares)
            .ok_or(sp_runtime::ArithmeticError::Overflow)?
            .checked_div(
                I64F64::checked_from_num(alpha).ok_or(sp_runtime::ArithmeticError::Overflow)?,
            )
            .ok_or(sp_runtime::ArithmeticError::Overflow)?;
        let increment = state
            .rate
            .checked_mul(factor)
            .ok_or(sp_runtime::ArithmeticError::Overflow)?;
        BasketRate::<T>::try_mutate(hotkey, |rate| -> DispatchResult {
            *rate = rate
                .checked_add(I96F32::from_num(increment))
                .ok_or(sp_runtime::ArithmeticError::Overflow)?;
            Ok(())
        })?;
        if state.debt_count > 0 {
            DeferredBasketConversions::<T>::insert(
                (hotkey, netuid, state.generation),
                DeferredBasketConversion {
                    shares_per_alpha: factor,
                    cash_per_alpha: I64F64::from_num(0),
                    debt_count: state.debt_count,
                },
            );
        }
        Ok(())
    }

    /// A claimant can redeem their own deferred alpha without repricing the
    /// basket. Only that claimant bears the sale price/slippage, so this needs
    /// no common EMA window and cannot transfer value from basket shareholders.
    pub(super) fn claim_deferred_dividends(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        ignore_minimum: bool,
    ) -> Result<(u64, u32), DispatchError> {
        let states: Vec<_> = DeferredBasketRates::<T>::iter_prefix(hotkey).collect();
        Self::settle_deferred_debts(hotkey, coldkey)?;
        let mut total = 0u64;
        let mut rows = 0u32;
        for (netuid, state) in states {
            let stake =
                Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, NetUid::ROOT)
                    .to_u64();
            let debt = DeferredBasketDebts::<T>::get(hotkey, coldkey)
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

    /// Close a dissolving origin into a separate cash entitlement, never into
    /// basket shares priced against unrelated (possibly manipulated) pools.
    pub(crate) fn dissolve_deferred_dividend(
        hotkey: &T::AccountId,
        netuid: NetUid,
        alpha: AlphaBalance,
    ) -> DispatchResult {
        frame_support::storage::with_storage_layer(|| -> DispatchResult {
            let state = DeferredBasketRates::<T>::take(hotkey, netuid)
                .ok_or(DispatchError::Other("missing dividend entitlement"))?;
            let total_stake_before_sale =
                (!NetworksAdded::<T>::get(netuid)).then(TotalStake::<T>::get);
            let tao = if alpha.is_zero() {
                0
            } else {
                match Self::sell_basket_alpha_for_root_tao(netuid, alpha) {
                    Ok(tao) => tao.to_u64(),
                    Err(error)
                        if T::SwapInterface::classify_failure(&error)
                            == subtensor_swap_interface::SwapFailureKind::TerminalLiquidity =>
                    {
                        Self::burn_subnet_alpha(netuid, alpha);
                        Self::deposit_event(Event::BasketAlphaWrittenOff {
                            hotkey: hotkey.clone(),
                            netuid,
                            alpha,
                        });
                        0
                    }
                    Err(error) => return Err(error),
                }
            };
            // Dissolve start already removed this subnet's reserves from TotalStake.
            if let Some(total) = total_stake_before_sale {
                TotalStake::<T>::put(total);
            }
            SubnetRootSellTao::<T>::mutate(netuid, |total| {
                *total = total.saturating_add(tao.into())
            });
            let factor = I64F64::checked_from_num(tao)
                .ok_or(sp_runtime::ArithmeticError::Overflow)?
                .checked_div(
                    I64F64::checked_from_num(alpha.to_u64().max(1))
                        .ok_or(sp_runtime::ArithmeticError::Overflow)?,
                )
                .ok_or(sp_runtime::ArithmeticError::Overflow)?;
            DeferredBasketCashRate::<T>::try_mutate(hotkey, |rate| -> DispatchResult {
                *rate = rate
                    .checked_add(
                        state
                            .rate
                            .checked_mul(factor)
                            .ok_or(sp_runtime::ArithmeticError::Overflow)?,
                    )
                    .ok_or(sp_runtime::ArithmeticError::Overflow)?;
                Ok(())
            })?;
            DeferredBasketCashReserve::<T>::try_mutate(hotkey, |reserve| -> DispatchResult {
                *reserve = reserve
                    .checked_add(tao)
                    .ok_or(sp_runtime::ArithmeticError::Overflow)?;
                Ok(())
            })?;
            if state.debt_count > 0 {
                DeferredBasketConversions::<T>::insert(
                    (hotkey, netuid, state.generation),
                    DeferredBasketConversion {
                        shares_per_alpha: I64F64::from_num(0),
                        cash_per_alpha: factor,
                        debt_count: state.debt_count,
                    },
                );
            }
            Self::credit_root_reserves(tao.into());
            PendingBasketDeposits::<T>::remove(hotkey, netuid);
            Ok(())
        })
    }

    pub(super) fn move_deferred_dividends(old: &T::AccountId, new: &T::AccountId) -> u32 {
        let generation = DeferredBasketGeneration::<T>::take(old);
        DeferredBasketGeneration::<T>::insert(new, generation);
        let rates: Vec<_> = DeferredBasketRates::<T>::drain_prefix(old).collect();
        let debts: Vec<_> = DeferredBasketDebts::<T>::drain_prefix(old).collect();
        let conversions: Vec<_> = DeferredBasketConversions::<T>::drain_prefix((old,)).collect();
        let cash_claimed: Vec<_> = DeferredBasketCashClaimed::<T>::drain_prefix(old).collect();
        let rows = rates
            .len()
            .saturating_add(debts.len())
            .saturating_add(conversions.len())
            .saturating_add(cash_claimed.len()) as u32;
        let cash_rate = DeferredBasketCashRate::<T>::take(old);
        if cash_rate != I64F64::from_num(0) {
            DeferredBasketCashRate::<T>::insert(new, cash_rate);
        }
        let cash_reserve = DeferredBasketCashReserve::<T>::take(old);
        if cash_reserve != 0 {
            DeferredBasketCashReserve::<T>::insert(new, cash_reserve);
        }
        if !rates.is_empty() {
            BasketShares::<T>::mutate(new, |_| {});
        }
        for (coldkey, claimed) in cash_claimed {
            DeferredBasketCashClaimed::<T>::insert(new, &coldkey, claimed);
            if claimed < 0 && !StakingHotkeys::<T>::get(&coldkey).contains(new) {
                HotkeySwapBasketCredits::<T>::insert(&coldkey, new, ());
            }
        }
        for (netuid, state) in rates {
            DeferredBasketRates::<T>::insert(new, netuid, state);
        }
        for (coldkey, debt) in debts {
            let retained = debt
                .values()
                .any(|entry| entry.amount < I64F64::from_num(0));
            DeferredBasketDebts::<T>::insert(new, &coldkey, debt);
            if (HotkeySwapBasketCredits::<T>::take(&coldkey, old).is_some() || retained)
                && !StakingHotkeys::<T>::get(&coldkey).contains(new)
            {
                HotkeySwapBasketCredits::<T>::insert(&coldkey, new, ());
            }
        }
        for ((netuid, generation), conversion) in conversions {
            DeferredBasketConversions::<T>::insert((new, netuid, generation), conversion);
        }
        rows
    }
}
