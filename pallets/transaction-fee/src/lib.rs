#![cfg_attr(not(feature = "std"), no_std)]

// FRAME
use frame_support::{
    pallet_prelude::*,
    storage::{TransactionOutcome, with_transaction},
    traits::{
        Imbalance, IsSubType, OnUnbalanced,
        fungible::{
            Balanced, Credit, Debt, DecreaseIssuance, Imbalance as FungibleImbalance,
            IncreaseIssuance, Inspect,
        },
        tokens::{Precision, WithdrawConsequence},
    },
    weights::{WeightToFeeCoefficient, WeightToFeeCoefficients, WeightToFeePolynomial},
};
use pallet_evm::{
    AddressMapping, BalanceConverter, Config as EvmConfig, EvmBalance, OnChargeEVMTransaction,
};

// Runtime
use sp_runtime::{
    DispatchError, Perbill, Saturating,
    traits::{DispatchInfoOf, PostDispatchInfoOf},
    transaction_validity::{InvalidTransaction, TransactionValidityError},
};

// Pallets
use pallet_subtensor::Call as SubtensorCall;
use pallet_transaction_payment::Config as PTPConfig;
use pallet_transaction_payment::OnChargeTransaction;
use subtensor_swap_interface::SwapHandler;

// Misc
use core::marker::PhantomData;
use smallvec::smallvec;
use sp_core::H160;
use sp_runtime::traits::SaturatedConversion;
use sp_std::vec::Vec;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};

// Tests
#[cfg(test)]
mod tests;

type AccountIdOf<T> = <T as frame_system::Config>::AccountId;
type CallOf<T> = <T as frame_system::Config>::RuntimeCall;

/// Rao charged per unit of declared `ref_time`, as a fraction. 0.00025 rao per
/// picosecond; spec 467 halved this from 0.0005.
pub const WEIGHT_FEE_PER_REF_TIME: Perbill = Perbill::from_parts(250_000);

/// Rao charged per encoded byte of the extrinsic, as a fraction. Half a rao per
/// byte; spec 467 halved this from `IdentityFee` (one rao per byte).
pub const LENGTH_FEE_PER_BYTE: Perbill = Perbill::from_parts(500_000_000);

fn linear_polynomial(coeff_frac: Perbill) -> WeightToFeeCoefficients<TaoBalance> {
    let coefficient: WeightToFeeCoefficient<TaoBalance> = WeightToFeeCoefficient {
        coeff_integer: TaoBalance::new(0),
        coeff_frac,
        negative: false,
        degree: 1,
    };

    smallvec![coefficient] as WeightToFeeCoefficients<TaoBalance>
}

pub struct LinearWeightToFee;
impl WeightToFeePolynomial for LinearWeightToFee {
    type Balance = TaoBalance;

    fn polynomial() -> WeightToFeeCoefficients<Self::Balance> {
        linear_polynomial(WEIGHT_FEE_PER_REF_TIME)
    }
}

pub struct LinearLengthToFee;
impl WeightToFeePolynomial for LinearLengthToFee {
    type Balance = TaoBalance;

    fn polynomial() -> WeightToFeeCoefficients<Self::Balance> {
        linear_polynomial(LENGTH_FEE_PER_BYTE)
    }
}

/// Trait that allows working with Alpha
pub trait AlphaFeeHandler<T: frame_system::Config> {
    fn can_withdraw_in_alpha(
        coldkey: &AccountIdOf<T>,
        alpha_vec: &[(AccountIdOf<T>, NetUid)],
        tao_amount: TaoBalance,
    ) -> bool;
    fn withdraw_in_alpha(
        coldkey: &AccountIdOf<T>,
        alpha_vec: &[(AccountIdOf<T>, NetUid)],
        tao_amount: TaoBalance,
    ) -> Result<(AlphaBalance, TaoBalance, NetUid), TransactionValidityError>;
    /// Give `tao_amount` of an alpha-paid fee back to `coldkey` as `(hotkey, netuid)` alpha:
    /// the TAO recycled at withdraw is re-issued into the subnet account and buys alpha for
    /// the payer, the mirror of [`Self::withdraw_in_alpha`]. Returns the alpha credited, or
    /// `None` (with nothing changed) when the buy-back cannot run, in which case the charge
    /// stays final as before spec 469.
    fn refund_in_alpha(
        coldkey: &AccountIdOf<T>,
        hotkey: &AccountIdOf<T>,
        netuid: NetUid,
        tao_amount: TaoBalance,
    ) -> Option<AlphaBalance>;
    fn get_all_netuids_for_coldkey_and_hotkey(
        coldkey: &AccountIdOf<T>,
        hotkey: &AccountIdOf<T>,
    ) -> Vec<NetUid>;
}

/// Deduct the transaction fee from the Subtensor Pallet TotalIssuance when charging the transaction
/// fee.
pub struct TransactionFeeHandler<T>(core::marker::PhantomData<T>);
impl<T> Default for TransactionFeeHandler<T> {
    fn default() -> Self {
        Self(core::marker::PhantomData)
    }
}

type BalancesImbalanceOf<T> = FungibleImbalance<
    <T as pallet_balances::Config>::Balance,
    DecreaseIssuance<AccountIdOf<T>, pallet_balances::Pallet<T>>,
    IncreaseIssuance<AccountIdOf<T>, pallet_balances::Pallet<T>>,
>;

impl<T> OnUnbalanced<BalancesImbalanceOf<T>> for TransactionFeeHandler<T>
where
    T: frame_system::Config + pallet_balances::Config + pallet_subtensor::Config,
    <T as pallet_balances::Config>::Balance: Into<TaoBalance> + Copy,
{
    fn on_nonzero_unbalanced(imbalance: BalancesImbalanceOf<T>) {
        let amount = imbalance.peek().into();
        pallet_subtensor::TotalIssuance::<T>::mutate(|total| {
            *total = total.saturating_sub(amount);
        });
        drop(imbalance);
    }
}

/// Handle Alpha fees
impl<T> AlphaFeeHandler<T> for TransactionFeeHandler<T>
where
    T: frame_system::Config,
    T: pallet_subtensor::Config,
    T: pallet_subtensor_swap::Config,
{
    /// This function checks if tao_amount fee can be withdraw in Alpha currency
    /// by converting Alpha to TAO using the current pool conditions.
    ///
    /// If this function returns true, the transaction will be added to the mempool
    /// and Alpha will be withdraw from the account, no matter whether transaction
    /// is successful or not.
    ///
    /// If this function returns true, but at the time of execution the Alpha price
    /// changes and it becomes impossible to pay tx fee with the Alpha balance,
    /// the transaction still executes and all Alpha is withdrawn from the account.
    fn can_withdraw_in_alpha(
        coldkey: &AccountIdOf<T>,
        alpha_vec: &[(AccountIdOf<T>, NetUid)],
        tao_amount: TaoBalance,
    ) -> bool {
        if alpha_vec.len() != 1 {
            // Multi-subnet alpha fee deduction is prohibited.
            return false;
        }

        if let Some((hotkey, netuid)) = alpha_vec.first() {
            // Only free (non-collateral, non-conviction-locked) alpha may pay fees.
            // Using total stake here lets a fully bonded miner erode MinerCollateral
            // via failed remove-stake spam while locked accounting stays unchanged.
            let available = pallet_subtensor::Pallet::<T>::available_to_unstake_from_hotkey(
                coldkey, hotkey, *netuid,
            );
            let alpha_fee = pallet_subtensor_swap::Pallet::<T>::get_alpha_amount_for_tao(
                *netuid,
                tao_amount.into(),
            );
            !alpha_fee.is_zero() && available >= alpha_fee
        } else {
            false
        }
    }

    fn withdraw_in_alpha(
        coldkey: &AccountIdOf<T>,
        alpha_vec: &[(AccountIdOf<T>, NetUid)],
        tao_amount: TaoBalance,
    ) -> Result<(AlphaBalance, TaoBalance, NetUid), TransactionValidityError> {
        if alpha_vec.len() != 1 {
            return Ok((0.into(), 0.into(), NetUid::ROOT));
        }

        if let Some((hotkey, netuid)) = alpha_vec.first() {
            let available = pallet_subtensor::Pallet::<T>::available_to_unstake_from_hotkey(
                coldkey, hotkey, *netuid,
            );
            let mut alpha_equivalent = pallet_subtensor_swap::Pallet::<T>::get_alpha_amount_for_tao(
                *netuid,
                tao_amount.into(),
            );
            if alpha_equivalent.is_zero() {
                alpha_equivalent = available;
            }
            let alpha_fee = alpha_equivalent.min(available);
            if alpha_fee.is_zero() {
                return Err(InvalidTransaction::Payment.into());
            }

            // Sell the Alpha fee and recycle the resulting TAO directly from the subnet
            // account. This avoids relying on the payer having enough TAO to keep an account
            // alive. Keeping both operations in one storage transaction ensures that a failure
            // to recycle the TAO also rolls back the Alpha withdrawal and AMM updates.
            with_transaction(
                || -> TransactionOutcome<Result<TaoBalance, DispatchError>> {
                    let Some(subnet_account) =
                        pallet_subtensor::Pallet::<T>::get_subnet_account_id(*netuid)
                    else {
                        return TransactionOutcome::Rollback(Err(
                            pallet_subtensor::Error::<T>::SubnetNotExists.into(),
                        ));
                    };
                    match pallet_subtensor::Pallet::<T>::unstake_from_subnet(
                        hotkey,
                        coldkey,
                        &subnet_account,
                        *netuid,
                        alpha_fee,
                        0.into(),
                        true,
                        false,
                    ) {
                        Ok(tao_amount) => {
                            match pallet_subtensor::Pallet::<T>::recycle_tao(
                                &subnet_account,
                                tao_amount,
                            ) {
                                Ok(()) => TransactionOutcome::Commit(Ok(tao_amount)),
                                Err(err) => TransactionOutcome::Rollback(Err(err)),
                            }
                        }
                        Err(err) => TransactionOutcome::Rollback(Err(err)),
                    }
                },
            )
            .map(|tao_amount| (alpha_fee, tao_amount, *netuid))
            .map_err(|err| {
                log::warn!("Error withdrawing transaction fee in alpha: {err:?}");
                InvalidTransaction::Payment.into()
            })
        } else {
            Ok((0.into(), 0.into(), NetUid::ROOT))
        }
    }

    fn refund_in_alpha(
        coldkey: &AccountIdOf<T>,
        hotkey: &AccountIdOf<T>,
        netuid: NetUid,
        tao_amount: TaoBalance,
    ) -> Option<AlphaBalance> {
        if tao_amount.is_zero() {
            return None;
        }
        // Re-issue the over-recycled TAO into the subnet account and buy the payer's alpha
        // back from there, in one storage transaction: a buy that cannot fill (dust, a
        // closed pool) rolls the re-issue back too.
        with_transaction(
            || -> TransactionOutcome<Result<AlphaBalance, DispatchError>> {
                let Some(subnet_account) =
                    pallet_subtensor::Pallet::<T>::get_subnet_account_id(netuid)
                else {
                    return TransactionOutcome::Rollback(Err(
                        pallet_subtensor::Error::<T>::SubnetNotExists.into(),
                    ));
                };
                let credit = pallet_subtensor::Pallet::<T>::mint_tao(tao_amount);
                if credit.peek() != tao_amount {
                    return TransactionOutcome::Rollback(Err(
                        pallet_subtensor::Error::<T>::InsufficientTaoBalance.into(),
                    ));
                }
                if pallet_subtensor::Pallet::<T>::spend_tao(&subnet_account, credit, tao_amount)
                    .is_err()
                {
                    return TransactionOutcome::Rollback(Err(
                        pallet_subtensor::Error::<T>::InsufficientTaoBalance.into(),
                    ));
                }
                match pallet_subtensor::Pallet::<T>::stake_into_subnet_from(
                    &subnet_account,
                    hotkey,
                    coldkey,
                    netuid,
                    tao_amount,
                    <T as pallet_subtensor::Config>::SwapInterface::max_price(),
                    true,
                ) {
                    Ok(alpha) => TransactionOutcome::Commit(Ok(alpha)),
                    Err(err) => TransactionOutcome::Rollback(Err(err)),
                }
            },
        )
        .map_err(|err| log::debug!("Alpha fee refund not applied, charge stays final: {err:?}"))
        .ok()
    }

    fn get_all_netuids_for_coldkey_and_hotkey(
        coldkey: &AccountIdOf<T>,
        hotkey: &AccountIdOf<T>,
    ) -> Vec<NetUid> {
        pallet_subtensor::Pallet::<T>::alpha_iter_prefix((hotkey, coldkey))
            .map(|(netuid, _)| netuid)
            .filter(|netuid| pallet_subtensor::SubtokenEnabled::<T>::get(netuid))
            .filter(|netuid| {
                pallet_subtensor::Pallet::<T>::get_stake_for_hotkey_and_coldkey_on_subnet(
                    hotkey, coldkey, *netuid,
                ) != 0.into()
            })
            .collect()
    }
}

/// Enum that describes either a withdrawn amount of transaction fee in TAO or
/// the exact charged Alpha amount.
pub enum WithdrawnFee<T: frame_system::Config, F: Balanced<AccountIdOf<T>>> {
    // Contains withdrawn TAO amount
    Tao(Credit<AccountIdOf<T>, F>),
    // Contains withdrawn Alpha amount, the resulting swapped TAO, the subnet, and the
    // hotkey the alpha came from (so an over-charge can be bought back for the payer).
    Alpha((AlphaBalance, TaoBalance, NetUid, AccountIdOf<T>)),
}

/// Custom OnChargeTransaction implementation based on standard FungibleAdapter from transaction_payment
/// FRAME pallet
///
pub struct SubtensorTxFeeHandler<F, OU>(PhantomData<(F, OU)>);

pub struct SubtensorEvmFeeHandler<F, OU>(PhantomData<(F, OU)>);

/// This implementation contains the list of calls that require paying transaction
/// fees in Alpha
impl<F, OU> SubtensorTxFeeHandler<F, OU> {
    /// Returns Vec<(hotkey, netuid)> if the given call should pay fees in Alpha instead of TAO.
    /// The vector represents all subnets where this hotkey has any alpha stake. Fees will be
    /// distributed evenly between subnets in case of multiple subnets.
    pub fn fees_in_alpha<T>(who: &AccountIdOf<T>, call: &CallOf<T>) -> Vec<(AccountIdOf<T>, NetUid)>
    where
        T: frame_system::Config + pallet_subtensor::Config,
        CallOf<T>: IsSubType<pallet_subtensor::Call<T>>,
        OU: AlphaFeeHandler<T>,
    {
        let mut alpha_vec: Vec<(AccountIdOf<T>, NetUid)> = Vec::new();

        // Otherwise, switch to Alpha for the extrinsics that assume converting Alpha
        // to TAO
        // TODO: Populate the list
        match call.is_sub_type() {
            Some(SubtensorCall::remove_stake { hotkey, netuid, .. }) => {
                alpha_vec.push((hotkey.clone(), *netuid))
            }
            Some(SubtensorCall::remove_stake_limit { hotkey, netuid, .. }) => {
                alpha_vec.push((hotkey.clone(), *netuid))
            }
            Some(SubtensorCall::remove_stake_full_limit { hotkey, netuid, .. }) => {
                alpha_vec.push((hotkey.clone(), *netuid))
            }
            Some(SubtensorCall::unstake_all { hotkey, .. }) => {
                let netuids = OU::get_all_netuids_for_coldkey_and_hotkey(who, hotkey);
                netuids
                    .into_iter()
                    .for_each(|netuid| alpha_vec.push((hotkey.clone(), netuid)));
            }
            Some(SubtensorCall::unstake_all_alpha { hotkey, .. }) => {
                let netuids = OU::get_all_netuids_for_coldkey_and_hotkey(who, hotkey);
                netuids
                    .into_iter()
                    .for_each(|netuid| alpha_vec.push((hotkey.clone(), netuid)));
            }
            Some(SubtensorCall::move_stake {
                origin_hotkey,
                destination_hotkey: _,
                origin_netuid,
                ..
            }) => alpha_vec.push((origin_hotkey.clone(), *origin_netuid)),
            Some(SubtensorCall::move_stake_limit {
                origin_hotkey,
                destination_hotkey: _,
                origin_netuid,
                ..
            }) => alpha_vec.push((origin_hotkey.clone(), *origin_netuid)),
            Some(SubtensorCall::transfer_stake {
                destination_coldkey: _,
                hotkey,
                origin_netuid,
                ..
            }) => alpha_vec.push((hotkey.clone(), *origin_netuid)),
            Some(SubtensorCall::transfer_stake_and_hotkey {
                destination_coldkey: _,
                origin_hotkey,
                origin_netuid,
                ..
            }) => alpha_vec.push((origin_hotkey.clone(), *origin_netuid)),
            Some(SubtensorCall::swap_stake {
                hotkey,
                origin_netuid,
                ..
            }) => alpha_vec.push((hotkey.clone(), *origin_netuid)),
            Some(SubtensorCall::swap_stake_limit {
                hotkey,
                origin_netuid,
                ..
            }) => alpha_vec.push((hotkey.clone(), *origin_netuid)),
            Some(SubtensorCall::swap_hotkey {
                hotkey,
                new_hotkey: _,
                netuid,
            }) => match netuid {
                Some(netuid) => alpha_vec.push((hotkey.clone(), *netuid)),
                None => {
                    let netuids = OU::get_all_netuids_for_coldkey_and_hotkey(who, hotkey);
                    netuids
                        .into_iter()
                        .for_each(|netuid| alpha_vec.push((hotkey.clone(), netuid)));
                }
            },
            Some(SubtensorCall::swap_hotkey_v2 {
                hotkey,
                new_hotkey: _,
                netuid,
                keep_stake: _,
            }) => match netuid {
                Some(netuid) => alpha_vec.push((hotkey.clone(), *netuid)),
                None => {
                    let netuids = OU::get_all_netuids_for_coldkey_and_hotkey(who, hotkey);
                    netuids
                        .into_iter()
                        .for_each(|netuid| alpha_vec.push((hotkey.clone(), netuid)));
                }
            },
            Some(SubtensorCall::recycle_alpha {
                hotkey,
                amount: _,
                netuid,
            }) => alpha_vec.push((hotkey.clone(), *netuid)),
            Some(SubtensorCall::burn_alpha {
                hotkey,
                amount: _,
                netuid,
            }) => alpha_vec.push((hotkey.clone(), *netuid)),
            _ => {}
        }

        alpha_vec
    }
}

impl<T, F, OU> OnChargeTransaction<T> for SubtensorTxFeeHandler<F, OU>
where
    T: PTPConfig + pallet_subtensor::Config,
    CallOf<T>: IsSubType<pallet_subtensor::Call<T>>,
    F: Balanced<T::AccountId>,
    OU: OnUnbalanced<Credit<T::AccountId, F>> + AlphaFeeHandler<T>,
    <F as Inspect<AccountIdOf<T>>>::Balance: Into<TaoBalance> + From<TaoBalance>,
{
    type LiquidityInfo = Option<WithdrawnFee<T, F>>;
    type Balance = <F as Inspect<<T as frame_system::Config>::AccountId>>::Balance;

    fn withdraw_fee(
        who: &AccountIdOf<T>,
        call: &CallOf<T>,
        _dispatch_info: &DispatchInfoOf<CallOf<T>>,
        fee: Self::Balance,
        _tip: Self::Balance,
    ) -> Result<Self::LiquidityInfo, TransactionValidityError> {
        if fee.is_zero() {
            return Ok(None);
        }

        // Traditional fees in TAO
        match F::withdraw(
            who,
            fee,
            Precision::Exact,
            frame_support::traits::tokens::Preservation::Preserve,
            frame_support::traits::tokens::Fortitude::Polite,
        ) {
            Ok(imbalance) => Ok(Some(WithdrawnFee::Tao(imbalance))),
            Err(_) => {
                let alpha_vec = Self::fees_in_alpha::<T>(who, call);
                if let Some((hotkey, _)) = alpha_vec.first() {
                    let fee_u64: u64 = fee.saturated_into::<u64>();
                    let (alpha_fee, tao_amount, netuid) =
                        OU::withdraw_in_alpha(who, &alpha_vec, fee_u64.into())?;
                    return Ok(Some(WithdrawnFee::Alpha((
                        alpha_fee,
                        tao_amount,
                        netuid,
                        hotkey.clone(),
                    ))));
                }
                Err(InvalidTransaction::Payment.into())
            }
        }
    }

    fn can_withdraw_fee(
        who: &AccountIdOf<T>,
        call: &CallOf<T>,
        _dispatch_info: &DispatchInfoOf<CallOf<T>>,
        fee: Self::Balance,
        _tip: Self::Balance,
    ) -> Result<(), TransactionValidityError> {
        if fee.is_zero() {
            return Ok(());
        }

        // Prefer traditional fees in TAO
        match F::can_withdraw(who, fee) {
            WithdrawConsequence::Success => Ok(()),
            _ => {
                // Fallback to fees in Alpha if possible
                let alpha_vec = Self::fees_in_alpha::<T>(who, call);
                if !alpha_vec.is_empty() {
                    let fee_u64: u64 = fee.saturated_into::<u64>();
                    if OU::can_withdraw_in_alpha(who, &alpha_vec, fee_u64.into()) {
                        return Ok(());
                    }
                }
                Err(InvalidTransaction::Payment.into())
            }
        }
    }

    fn correct_and_deposit_fee(
        who: &AccountIdOf<T>,
        _dispatch_info: &DispatchInfoOf<CallOf<T>>,
        _post_info: &PostDispatchInfoOf<CallOf<T>>,
        corrected_fee: Self::Balance,
        tip: Self::Balance,
        already_withdrawn: Self::LiquidityInfo,
    ) -> Result<(), TransactionValidityError> {
        if let Some(withdrawn) = already_withdrawn {
            // Fee may be paid in TAO or in Alpha. Only refund and update total issuance for
            // TAO fees because Alpha fees are charged precisely and do not need any adjustments
            match withdrawn {
                WithdrawnFee::Tao(paid) => {
                    // Calculate how much refund we should return
                    let refund_amount = paid.peek().saturating_sub(corrected_fee);
                    // refund to the account that paid the fees if it exists. otherwise, don't refund
                    // anything.
                    let refund_imbalance = if F::total_balance(who) > F::Balance::zero() {
                        F::deposit(who, refund_amount, Precision::BestEffort)
                            .unwrap_or_else(|_| Debt::<T::AccountId, F>::zero())
                    } else {
                        Debt::<T::AccountId, F>::zero()
                    };
                    // merge the imbalance caused by paying the fees and refunding parts of it again.
                    let adjusted_paid: Credit<T::AccountId, F> =
                        paid.offset(refund_imbalance).same().map_err(|_| {
                            TransactionValidityError::Invalid(InvalidTransaction::Payment)
                        })?;
                    // Call someone else to handle the imbalance (fee and tip separately)
                    let (tip, fee) = adjusted_paid.split(tip);
                    OU::on_unbalanceds(Some(fee).into_iter().chain(Some(tip)));
                }
                WithdrawnFee::Alpha((alpha_fee, tao_amount, netuid, hotkey)) => {
                    // Spec 469: alpha payers are refunded like TAO payers. The alpha sold at
                    // withdraw covered the declared fee; what the call did not use is bought
                    // back for the payer. If the buy-back cannot run the charge stays final.
                    let refund_tao = tao_amount.saturating_sub(corrected_fee.into());
                    let alpha_back = OU::refund_in_alpha(who, &hotkey, netuid, refund_tao);
                    let (alpha_fee, tao_amount) = match alpha_back {
                        Some(alpha_back) => (
                            alpha_fee.saturating_sub(alpha_back),
                            tao_amount.saturating_sub(refund_tao),
                        ),
                        None => (alpha_fee, tao_amount),
                    };
                    frame_system::Pallet::<T>::deposit_event(
                        pallet_subtensor::Event::<T>::TransactionFeePaidWithAlpha {
                            who: who.clone(),
                            netuid,
                            alpha_fee,
                            tao_amount,
                        },
                    );
                }
            }
        }

        Ok(())
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn endow_account(who: &AccountIdOf<T>, amount: Self::Balance) {
        let _ = F::deposit(who, amount, Precision::BestEffort);
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn minimum_balance() -> Self::Balance {
        F::minimum_balance()
    }
}

impl<T, F, OU> OnChargeEVMTransaction<T> for SubtensorEvmFeeHandler<F, OU>
where
    T: EvmConfig + pallet_subtensor::Config,
    F: Balanced<T::AccountId>,
    OU: OnUnbalanced<Credit<T::AccountId, F>>,
    T::AddressMapping: AddressMapping<T::AccountId>,
    <F as Inspect<T::AccountId>>::Balance: From<TaoBalance> + Into<TaoBalance>,
{
    type LiquidityInfo = Option<Credit<T::AccountId, F>>;

    fn withdraw_fee(
        who: &H160,
        fee: EvmBalance,
    ) -> Result<Self::LiquidityInfo, pallet_evm::Error<T>> {
        if fee.into_u256().is_zero() {
            return Ok(None);
        }

        let account_id = <T::AddressMapping as AddressMapping<T::AccountId>>::into_account_id(*who);
        let fee_sub = T::BalanceConverter::into_substrate_balance(fee)
            .ok_or(pallet_evm::Error::<T>::FeeOverflow)?;

        let imbalance = F::withdraw(
            &account_id,
            TaoBalance::from(fee_sub.into_u64_saturating()).into(),
            Precision::Exact,
            frame_support::traits::tokens::Preservation::Preserve,
            frame_support::traits::tokens::Fortitude::Polite,
        )
        .map_err(|_| pallet_evm::Error::<T>::BalanceLow)?;

        Ok(Some(imbalance))
    }

    fn correct_and_deposit_fee(
        who: &H160,
        corrected_fee: EvmBalance,
        base_fee: EvmBalance,
        already_withdrawn: Self::LiquidityInfo,
    ) -> Self::LiquidityInfo {
        if let Some(paid) = already_withdrawn {
            let account_id =
                <T::AddressMapping as AddressMapping<T::AccountId>>::into_account_id(*who);
            let corrected_fee_sub = T::BalanceConverter::into_substrate_balance(corrected_fee)
                .unwrap_or_else(|| 0u64.into());
            let refund_amount = paid
                .peek()
                .saturating_sub(TaoBalance::from(corrected_fee_sub.into_u64_saturating()).into());
            let refund_imbalance = F::deposit(&account_id, refund_amount, Precision::BestEffort)
                .unwrap_or_else(|_| Debt::<T::AccountId, F>::zero());
            let adjusted_paid = paid
                .offset(refund_imbalance)
                .same()
                .unwrap_or_else(|_| Credit::<T::AccountId, F>::zero());
            let base_fee_sub = T::BalanceConverter::into_substrate_balance(base_fee)
                .unwrap_or_else(|| 0u64.into());
            let (base_fee_credit, tip) =
                adjusted_paid.split(TaoBalance::from(base_fee_sub.into_u64_saturating()).into());
            OU::on_unbalanced(base_fee_credit);
            return Some(tip);
        }

        None
    }

    fn pay_priority_fee(tip: Self::LiquidityInfo) {
        if let Some(tip) = tip {
            OU::on_unbalanced(tip);
        }
    }
}
