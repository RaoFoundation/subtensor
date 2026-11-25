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
        // The number of rate states of the hotkey, counted at most once for the whole
        // snapshot. A count for each new state is work that grows with the square of the
        // number of credits.
        let mut states = None;
        for (netuid, alpha) in pending {
            if !DeferredBasketRates::<T>::contains_key(hotkey, netuid) {
                #[cfg(test)]
                if crate::tests::mock::reference_deferred_claims() {
                    states = None;
                }
                Self::record_deferred_dividend(hotkey, netuid, alpha, &mut states)?;
            }
        }
        Ok(())
    }

    /// The number of rate states of `hotkey`, at most [`crate::MAX_ROOT_CLAIM_WORK`].
    /// `states` keeps the number between calls; the caller adds each state that it makes.
    fn deferred_rate_states(hotkey: &T::AccountId, states: &mut Option<usize>) -> usize {
        *states.get_or_insert_with(|| {
            let count = DeferredBasketRates::<T>::iter_prefix(hotkey)
                .take(crate::MAX_ROOT_CLAIM_WORK as usize)
                .count();
            #[cfg(test)]
            crate::tests::mock::inc_deferred_dividend_ops(count);
            count
        })
    }

    /// `states` is the number of rate states of `hotkey` when the caller knows it (see
    /// [`Self::deferred_rate_states`]). `None` means that this call counts them, if it
    /// needs the number.
    fn record_deferred_dividend(
        hotkey: &T::AccountId,
        netuid: NetUid,
        alpha: AlphaBalance,
        states: &mut Option<usize>,
    ) -> DispatchResult {
        if alpha.is_zero() {
            return Ok(());
        }
        let existing = DeferredBasketRates::<T>::contains_key(hotkey, netuid);
        ensure!(
            existing
                || Self::deferred_rate_states(hotkey, states) < crate::MAX_ROOT_CLAIM_WORK as usize,
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
        if !existing && let Some(count) = states.as_mut() {
            *count = count.saturating_add(1);
        }
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
            match Self::record_deferred_dividend(hotkey, netuid, alpha, &mut None) {
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
                    #[cfg(test)]
                    crate::tests::mock::inc_deferred_dividend_ops(debts.len());
                    for (netuid, debt) in debts.iter() {
                        if let Some(conversion) =
                            DeferredBasketConversions::<T>::get((hotkey, netuid, debt.generation))
                        {
                            Self::settle_deferred_debt(hotkey, coldkey, *netuid, debt, conversion)?;
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

    /// Turn one debt into its corrections of the share watermark and of the cash
    /// watermark, with the `conversion` record of its credit, and count the debt off that
    /// record. The caller removes the debt.
    fn settle_deferred_debt(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        netuid: NetUid,
        debt: &DeferredBasketDebt,
        mut conversion: DeferredBasketConversion,
    ) -> DispatchResult {
        let correction = Self::converted_debt(debt, conversion.shares_per_alpha)?;
        BasketClaimed::<T>::try_mutate(hotkey, coldkey, |claimed| -> DispatchResult {
            *claimed = claimed
                .checked_add(correction)
                .ok_or(sp_runtime::ArithmeticError::Overflow)?;
            Ok(())
        })?;
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
            DeferredBasketConversions::<T>::remove((hotkey, netuid, debt.generation));
        } else {
            DeferredBasketConversions::<T>::insert((hotkey, netuid, debt.generation), conversion);
        }
        Ok(())
    }

    pub(crate) fn rebase_deferred_dividends(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        delta: i128,
    ) -> DispatchResult {
        Self::settle_deferred_debts(hotkey, coldkey)?;
        Self::rebase_deferred_cash(hotkey, coldkey, delta)?;
        if DeferredBasketRates::<T>::iter_prefix(hotkey)
            .next()
            .is_none()
        {
            return Ok(());
        }
        DeferredBasketDebts::<T>::try_mutate(hotkey, coldkey, |debts| -> DispatchResult {
            for (netuid, mut state) in DeferredBasketRates::<T>::iter_prefix(hotkey) {
                #[cfg(test)]
                crate::tests::mock::inc_deferred_dividend_ops(1);
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

    /// The cash part of a stake change of `delta`: move the cash watermark of the account
    /// by `cash rate * delta`, so that the change earns no cash of dissolved credits.
    fn rebase_deferred_cash(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        delta: i128,
    ) -> DispatchResult {
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
        Ok(())
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
    ///
    /// The work is linear in the number of deferred credits: [`DeferredClaim`] reads the
    /// rate states and the claimant's debts one time and writes the debts one time.
    pub(super) fn claim_deferred_dividends(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        ignore_minimum: bool,
    ) -> Result<(u64, u32), DispatchError> {
        #[cfg(test)]
        if crate::tests::mock::reference_deferred_claims() {
            return Self::claim_deferred_dividends_reference(hotkey, coldkey, ignore_minimum);
        }
        let mut claim = DeferredClaim::<T>::open(hotkey, coldkey)?;
        let mut total = 0u64;
        let mut rows = 0u32;
        for index in 0..claim.len() {
            let Some((netuid, rate)) = claim.state(index) else {
                continue;
            };
            let stake =
                Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, NetUid::ROOT)
                    .to_u64();
            let debt = claim.debt(index)?;
            let owed = rate
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
                    claim.reserve(index, alpha)?;
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
                    claim.stake_added(tao)?;
                    Self::touch_root_stake_age(coldkey, hotkey);
                    if PendingBasketDeposits::<T>::get(hotkey, netuid).is_zero() {
                        PendingBasketDeposits::<T>::remove(hotkey, netuid);
                        claim.convert(index)?;
                    }
                    Ok(tao)
                })?;
            total = total
                .checked_add(tao)
                .ok_or(sp_runtime::ArithmeticError::Overflow)?;
            rows = rows.saturating_add(1);
        }
        // The cash part below reads the claimant's debts from storage.
        claim.close()?;
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

/// The claimant's debts on one validator.
type DeferredDebts = frame_support::BoundedBTreeMap<
    NetUid,
    DeferredBasketDebt,
    frame_support::traits::ConstU32<{ crate::MAX_ROOT_CLAIM_WORK }>,
>;

/// One rate state of the validator, as the direct sales of one claim see it.
struct DeferredRateSlot {
    netuid: NetUid,
    /// A copy of the stored state. The claim writes each change of `debt_count` to
    /// storage at once, so the copy stays equal to storage.
    state: DeferredBasketState,
    /// False after the claim converted the state: its credit is sold in full.
    live: bool,
    /// How many of the kept stake changes the claimant's debt on this state includes.
    applied: usize,
}

/// The deferred-dividend state of one claimant on one validator, while a claim sells the
/// claimant's part of the deferred credits.
///
/// Each sale adds root stake to the claimant. A stake change of `delta` adds
/// `rate * delta` to the claimant's debt on every rate state of the validator
/// ([`Pallet::rebase_deferred_dividends`]). With storage calls only, each sale walks all
/// rate states and looks up a conversion record for every debt. For `n` deferred credits
/// that is work that grows with `n * n`. This type does the same arithmetic with work
/// that grows with `n`:
/// * It reads the rate states and the debts one time, and it writes the debts one time.
/// * After a sale it looks up only the conversion record of a state that this claim
///   converted. Every other debt was settled when the claim started, and only a sale of
///   this claim makes a new conversion record for the validator.
/// * It keeps the stake changes in a list, and it adds them to a debt when the claim
///   needs that debt. `rate * delta` is exact for a whole `delta`, so the sum of the
///   products is the product of the sum.
///
/// Every stored value and every error is the same as with a storage walk for each sale.
/// The list is used only while a bound shows that no addition can overflow
/// ([`Self::rebase`]). Without that bound, each change goes to each debt at once, as in
/// the storage code.
pub(super) struct DeferredClaim<T: Config> {
    hotkey: T::AccountId,
    coldkey: T::AccountId,
    /// The rate states of the validator when the claim started, in storage order.
    slots: Vec<DeferredRateSlot>,
    /// Number of slots that are live.
    live: usize,
    debts: DeferredDebts,
    /// Whether storage has, or must get, a debt map for the claimant.
    stored: bool,
    /// Whether `debts` or `stored` differ from storage.
    dirty: bool,
    /// The kept stake changes, in order.
    changes: Vec<i128>,
    /// `totals[i]` is the sum of the first `i` kept changes.
    totals: Vec<i128>,
    /// States that this claim converted after the last settlement.
    converted: Vec<NetUid>,
    /// `Some` while stake changes are kept: the largest rate of a live state, and a
    /// value that no debt of a live state exceeds when it includes every kept change.
    kept: Option<(I64F64, I64F64)>,
    /// True after a stake change that the bound did not cover. Each later change goes
    /// to each debt at once.
    each: bool,
}

impl<T: Config> DeferredClaim<T> {
    /// Read the rate states of `hotkey`, settle the debts of `coldkey` on it, and read
    /// the debts that stay.
    pub(super) fn open(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
    ) -> Result<Self, DispatchError> {
        let slots: Vec<_> = DeferredBasketRates::<T>::iter_prefix(hotkey)
            .map(|(netuid, state)| DeferredRateSlot {
                netuid,
                state,
                live: true,
                applied: 0,
            })
            .collect();
        #[cfg(test)]
        crate::tests::mock::inc_deferred_dividend_ops(slots.len());
        Pallet::<T>::settle_deferred_debts(hotkey, coldkey)?;
        // Without a rate state the claim sells nothing, and it does not use the debts.
        let (debts, stored) = if slots.is_empty() {
            (DeferredDebts::new(), false)
        } else {
            match DeferredBasketDebts::<T>::try_get(hotkey, coldkey) {
                Ok(debts) => (debts, true),
                Err(()) => (DeferredDebts::new(), false),
            }
        };
        #[cfg(test)]
        crate::tests::mock::inc_deferred_dividend_ops(debts.len());
        Ok(Self {
            hotkey: hotkey.clone(),
            coldkey: coldkey.clone(),
            live: slots.len(),
            slots,
            debts,
            stored,
            dirty: false,
            changes: Vec::new(),
            totals: Vec::from([0]),
            converted: Vec::new(),
            kept: None,
            each: false,
        })
    }

    /// Number of rate states that the validator had when the claim started.
    pub(super) fn len(&self) -> usize {
        self.slots.len()
    }

    /// Netuid and rate of state `index`.
    pub(super) fn state(&self, index: usize) -> Option<(NetUid, I64F64)> {
        self.slots
            .get(index)
            .map(|slot| (slot.netuid, slot.state.rate))
    }

    /// The claimant's debt on state `index`, with every stake change of this claim.
    pub(super) fn debt(&mut self, index: usize) -> Result<I64F64, DispatchError> {
        self.apply(index)?;
        Ok(self
            .slots
            .get(index)
            .and_then(|slot| self.debts.get(&slot.netuid))
            .map_or(I64F64::from_num(0), |debt| debt.amount))
    }

    /// Add the `alpha` that the claimant sells from state `index` to the claimant's debt
    /// on that state, before the sale.
    pub(super) fn reserve(&mut self, index: usize, alpha: u64) -> DispatchResult {
        self.apply(index)?;
        let Some(slot) = self.slots.get_mut(index) else {
            return Err(DispatchError::Other("missing dividend entitlement"));
        };
        let alpha = I64F64::checked_from_num(alpha).ok_or(sp_runtime::ArithmeticError::Overflow)?;
        let amount = if let Some(debt) = self.debts.get_mut(&slot.netuid) {
            debt.amount = debt
                .amount
                .checked_add(alpha)
                .ok_or(sp_runtime::ArithmeticError::Overflow)?;
            debt.amount
        } else {
            self.debts
                .try_insert(
                    slot.netuid,
                    DeferredBasketDebt {
                        generation: slot.state.generation,
                        amount: alpha,
                    },
                )
                .map_err(|_| Error::<T>::BasketDepositPending)?;
            let stored = DeferredBasketRates::<T>::mutate(&self.hotkey, slot.netuid, |state| {
                if let Some(state) = state {
                    state.debt_count = state.debt_count.saturating_add(1);
                }
                state.clone()
            });
            if let Some(state) = stored {
                slot.state = state;
            }
            // A new debt has none of the earlier stake changes.
            slot.applied = self.changes.len();
            alpha
        };
        if let Some((_, bound)) = self.kept.as_mut() {
            *bound = (*bound).max(amount);
        }
        self.stored = true;
        self.dirty = true;
        #[cfg(test)]
        crate::tests::mock::note_deferred_claim_path(crate::tests::mock::DeferredClaimPath::Sale);
        Ok(())
    }

    /// The claimant's root stake grew by `amount`. Move the claimant's debts and
    /// watermarks, as [`Pallet::add_stake_adjust_root_claimed_for_hotkey_and_coldkey`]
    /// does.
    pub(super) fn stake_added(&mut self, amount: u64) -> DispatchResult {
        let delta = i128::from(amount);
        self.rebase(delta)?;
        Pallet::<T>::rebase_basket_claimed_watermark(&self.hotkey, &self.coldkey, delta);
        Ok(())
    }

    /// The claimant sold the whole credit of state `index`. Close the state, as
    /// [`Pallet::convert_deferred_dividend`] does for a credit that mints no shares.
    pub(super) fn convert(&mut self, index: usize) -> DispatchResult {
        self.apply(index)?;
        let Some(slot) = self.slots.get_mut(index) else {
            return Err(DispatchError::Other("missing dividend entitlement"));
        };
        Pallet::<T>::convert_deferred_dividend(&self.hotkey, slot.netuid, 1, 0)?;
        if slot.live {
            slot.live = false;
            self.live = self.live.saturating_sub(1);
        }
        self.converted.push(slot.netuid);
        #[cfg(test)]
        crate::tests::mock::note_deferred_claim_path(
            crate::tests::mock::DeferredClaimPath::Converted,
        );
        Ok(())
    }

    /// Write the claimant's debts to storage. The claim must call this before other code
    /// reads them.
    pub(super) fn close(mut self) -> DispatchResult {
        for index in 0..self.slots.len() {
            self.apply(index)?;
        }
        if self.dirty {
            #[cfg(test)]
            crate::tests::mock::inc_deferred_dividend_ops(self.debts.len());
            if self.stored {
                DeferredBasketDebts::<T>::insert(&self.hotkey, &self.coldkey, self.debts);
            } else {
                DeferredBasketDebts::<T>::remove(&self.hotkey, &self.coldkey);
            }
        }
        Ok(())
    }

    /// A stake change of `delta`, as [`Pallet::rebase_deferred_dividends`] does it:
    /// settle the debts whose credit converted, move the cash watermark, and add
    /// `rate * delta` to the debt on each live state.
    fn rebase(&mut self, delta: i128) -> DispatchResult {
        self.settle()?;
        Pallet::<T>::rebase_deferred_cash(&self.hotkey, &self.coldkey, delta)?;
        if self.live == 0 {
            return Ok(());
        }
        self.stored = true;
        self.dirty = true;
        if let Some((max_rate, bound)) = self.kept {
            // Each debt of a live state grows by `rate * delta`, and that is at most
            // `max_rate * delta`. If the bound plus that step fits, then no product and
            // no sum for any live state can overflow, now or when the change is added.
            let raised = I64F64::checked_from_num(delta)
                .and_then(|delta| max_rate.checked_mul(delta))
                .and_then(|step| bound.checked_add(step));
            if let Some(raised) = raised
                && delta > 0
            {
                self.kept = Some((max_rate, raised));
                let total = self.totals.last().copied().unwrap_or(0);
                self.totals.push(total.saturating_add(delta));
                self.changes.push(delta);
                #[cfg(test)]
                crate::tests::mock::note_deferred_claim_path(
                    crate::tests::mock::DeferredClaimPath::Kept,
                );
                return Ok(());
            }
            // The bound does not cover this change. Bring every debt up to date, and
            // add this change and each later change to each debt at once.
            #[cfg(test)]
            crate::tests::mock::note_deferred_claim_path(
                crate::tests::mock::DeferredClaimPath::NotCovered,
            );
            for index in 0..self.slots.len() {
                self.apply(index)?;
            }
            self.kept = None;
            self.each = true;
        }
        self.rebase_each(delta)?;
        if !self.each {
            self.keep_later_changes();
        }
        Ok(())
    }

    /// Add `rate * delta` to the claimant's debt on each live state now. A live state
    /// without a debt of the claimant gets one.
    fn rebase_each(&mut self, delta: i128) -> DispatchResult {
        for slot in self.slots.iter_mut().filter(|slot| slot.live) {
            let correction = slot
                .state
                .rate
                .checked_mul(
                    I64F64::checked_from_num(delta).ok_or(sp_runtime::ArithmeticError::Overflow)?,
                )
                .ok_or(sp_runtime::ArithmeticError::Overflow)?;
            if let Some(debt) = self.debts.get_mut(&slot.netuid) {
                ensure!(
                    debt.generation == slot.state.generation,
                    DispatchError::Other("unsettled dividend generation")
                );
                debt.amount = debt
                    .amount
                    .checked_add(correction)
                    .ok_or(sp_runtime::ArithmeticError::Overflow)?;
            } else {
                self.debts
                    .try_insert(
                        slot.netuid,
                        DeferredBasketDebt {
                            generation: slot.state.generation,
                            amount: correction,
                        },
                    )
                    .map_err(|_| Error::<T>::BasketDepositPending)?;
                slot.state.debt_count = slot
                    .state
                    .debt_count
                    .checked_add(1)
                    .ok_or(sp_runtime::ArithmeticError::Overflow)?;
                DeferredBasketRates::<T>::insert(&self.hotkey, slot.netuid, slot.state.clone());
            }
        }
        Ok(())
    }

    /// Start to keep stake changes in the list. This is possible when each live state has
    /// a debt of the claimant and a rate that is not negative: then each debt can only
    /// grow, and one bound covers all of them.
    fn keep_later_changes(&mut self) {
        let zero = I64F64::from_num(0);
        let mut max_rate = zero;
        let mut bound: Option<I64F64> = None;
        for slot in self.slots.iter().filter(|slot| slot.live) {
            let Some(debt) = self.debts.get(&slot.netuid) else {
                return;
            };
            if slot.state.rate < zero {
                return;
            }
            max_rate = max_rate.max(slot.state.rate);
            bound = Some(bound.map_or(debt.amount, |bound| bound.max(debt.amount)));
        }
        let Some(bound) = bound else {
            return;
        };
        let applied = self.changes.len();
        for slot in self.slots.iter_mut().filter(|slot| slot.live) {
            slot.applied = applied;
        }
        self.kept = Some((max_rate, bound));
    }

    /// Bring the claimant's debt on state `index` up to date with the kept stake changes.
    fn apply(&mut self, index: usize) -> DispatchResult {
        if self.kept.is_none() {
            return Ok(());
        }
        let Some(slot) = self.slots.get_mut(index) else {
            return Ok(());
        };
        let Some(pending) = self.changes.get(slot.applied..) else {
            return Ok(());
        };
        if !slot.live || pending.is_empty() {
            return Ok(());
        }
        // Changes are kept only when each live state has a debt of the claimant.
        let Some(debt) = self.debts.get_mut(&slot.netuid) else {
            return Err(DispatchError::Other("missing dividend debt"));
        };
        let total = |count: usize| self.totals.get(count).copied().unwrap_or(0);
        let sum = total(self.changes.len()).saturating_sub(total(slot.applied));
        let lumped = I64F64::checked_from_num(sum)
            .and_then(|sum| slot.state.rate.checked_mul(sum))
            .and_then(|change| debt.amount.checked_add(change));
        debt.amount = match lumped {
            Some(amount) => amount,
            // The product of the sum does not fit. The single products can still fit
            // when the debt is negative: add them one at a time, as the storage code does.
            None => {
                #[cfg(test)]
                crate::tests::mock::note_deferred_claim_path(
                    crate::tests::mock::DeferredClaimPath::SingleProducts,
                );
                let mut amount = debt.amount;
                for delta in pending {
                    let change = slot
                        .state
                        .rate
                        .checked_mul(
                            I64F64::checked_from_num(*delta)
                                .ok_or(sp_runtime::ArithmeticError::Overflow)?,
                        )
                        .ok_or(sp_runtime::ArithmeticError::Overflow)?;
                    amount = amount
                        .checked_add(change)
                        .ok_or(sp_runtime::ArithmeticError::Overflow)?;
                }
                amount
            }
        };
        slot.applied = self.changes.len();
        Ok(())
    }

    /// Settle the claimant's debts on the states that this claim converted, as
    /// [`Pallet::settle_deferred_debts`] does for every debt of the claimant.
    fn settle(&mut self) -> DispatchResult {
        if self.converted.is_empty() {
            return Ok(());
        }
        let mut converted = core::mem::take(&mut self.converted);
        if !self.stored {
            return Ok(());
        }
        // The storage code settles in the order of the debt map: by netuid.
        converted.sort();
        #[cfg(test)]
        crate::tests::mock::inc_deferred_dividend_ops(converted.len());
        for netuid in converted {
            let Some(debt) = self.debts.get(&netuid) else {
                continue;
            };
            if let Some(conversion) =
                DeferredBasketConversions::<T>::get((&self.hotkey, netuid, debt.generation))
            {
                Pallet::<T>::settle_deferred_debt(
                    &self.hotkey,
                    &self.coldkey,
                    netuid,
                    debt,
                    conversion,
                )?;
                self.debts.remove(&netuid);
                self.dirty = true;
            }
        }
        if self.debts.is_empty() {
            self.stored = false;
            self.dirty = true;
        }
        Ok(())
    }
}
