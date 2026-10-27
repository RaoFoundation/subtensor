use crate::weights::WeightInfo as _;
use crate::{
    Call, CheckColdkeySwap, CheckDelegateTake, CheckEvmKeyAssociation, CheckRateLimits,
    CheckServingEndpoints, CheckWeights, Config, Error, Pallet, guards::applicable_call,
};
use codec::{Decode, DecodeWithMemTracking, Encode};
use frame_support::{
    dispatch::{DispatchExtension, DispatchInfo, GetDispatchInfo, Pays, PostDispatchInfo},
    ensure,
    traits::{Get, IsSubType, IsType, OriginTrait},
    weights::Weight,
};
use pallet_commitments::CanCommit;
use pallet_subtensor_proxy as pallet_proxy;
use pallet_subtensor_utility as pallet_utility;
use scale_info::TypeInfo;
use sp_runtime::traits::{
    DispatchInfoOf, Dispatchable, Implication, StaticLookup, TransactionExtension, ValidateResult,
    Zero,
};
use sp_runtime::{
    impl_tx_ext_default,
    transaction_validity::{TransactionSource, TransactionValidityError, ValidTransaction},
};
use sp_std::{marker::PhantomData, vec::Vec};
use subtensor_macros::freeze_struct;
use subtensor_runtime_common::{CustomTransactionError, Token};

type CallOf<T> = <T as frame_system::Config>::RuntimeCall;
type OriginOf<T> = <T as frame_system::Config>::RuntimeOrigin;
type LookupOf<T> = <T as frame_system::Config>::Lookup;
type CommitmentPolicy<T> = <T as pallet_commitments::Config>::CanCommit;

#[allow(deprecated)]
impl<T: Config> From<Error<T>> for CustomTransactionError {
    fn from(error: Error<T>) -> Self {
        match error {
            Error::<T>::AmountTooLow | Error::<T>::NotEnoughStakeToSetWeights => {
                Self::StakeAmountTooLow
            }
            Error::<T>::SubnetNotExists => Self::SubnetNotExists,
            Error::<T>::NotEnoughBalanceToStake => Self::BalanceTooLow,
            Error::<T>::HotKeyAccountNotExists => Self::HotkeyAccountDoesntExist,
            Error::<T>::NotEnoughStakeToWithdraw => Self::NotEnoughStakeToWithdraw,
            Error::<T>::InsufficientLiquidity => Self::InsufficientLiquidity,
            Error::<T>::SlippageTooHigh => Self::SlippageTooHigh,
            Error::<T>::TransferDisallowed => Self::TransferDisallowed,
            Error::<T>::HotKeyNotRegisteredInNetwork => Self::HotKeyNotRegisteredInNetwork,
            Error::<T>::InvalidIpAddress => Self::InvalidIpAddress,
            Error::<T>::ServingRateLimitExceeded => Self::ServingRateLimitExceeded,
            Error::<T>::InvalidPort => Self::InvalidPort,
            Error::<T>::NonAssociatedColdKey => Self::NonAssociatedColdKey,
            Error::<T>::DelegateTakeTooLow => Self::DelegateTakeTooLow,
            Error::<T>::DelegateTakeTooHigh => Self::DelegateTakeTooHigh,
            Error::<T>::InputLengthsUnequal => Self::InputLengthsUnequal,
            Error::<T>::NoWeightsCommitFound => Self::CommitNotFound,
            Error::<T>::RevealTooEarly => Self::CommitBlockNotInRevealRange,
            Error::<T>::InvalidRevealRound => Self::InvalidRevealRound,
            Error::<T>::CommittingWeightsTooFast
            | Error::<T>::SettingWeightsTooFast
            | Error::<T>::NetworkTxRateLimitExceeded => Self::RateLimitExceeded,
            Error::<T>::HotKeyNotRegisteredInSubNet => Self::UidNotFound,
            Error::<T>::EvmKeyAssociateRateLimitExceeded => Self::EvmKeyAssociateRateLimitExceeded,
            Error::<T>::ColdkeySwapAnnounced => Self::ColdkeyInSwapSchedule,
            Error::<T>::ColdkeySwapDisputed => Self::ColdkeySwapDisputed,
            _ => Self::BadRequest,
        }
    }
}

#[freeze_struct("2e02eb32e5cb25d3")]
#[derive(Default, Encode, Decode, DecodeWithMemTracking, Clone, Eq, PartialEq, TypeInfo)]
pub struct SubtensorTransactionExtension<T: Config + Send + Sync + TypeInfo>(pub PhantomData<T>);

impl<T: Config + Send + Sync + TypeInfo> sp_std::fmt::Debug for SubtensorTransactionExtension<T> {
    fn fmt(&self, f: &mut sp_std::fmt::Formatter) -> sp_std::fmt::Result {
        write!(f, "SubtensorTransactionExtension")
    }
}

impl<T: Config + Send + Sync + TypeInfo> SubtensorTransactionExtension<T> {
    pub fn new() -> Self {
        Self(Default::default())
    }

    fn check(origin: &OriginOf<T>, call: &CallOf<T>) -> Result<(), Error<T>>
    where
        T: pallet_commitments::Config
            + pallet_proxy::Config
            + pallet_shield::Config
            + pallet_utility::Config,
        CallOf<T>: Dispatchable<RuntimeOrigin = OriginOf<T>>
            + IsSubType<Call<T>>
            + IsSubType<pallet_commitments::Call<T>>
            + IsSubType<pallet_proxy::Call<T>>
            + IsSubType<pallet_utility::Call<T>>
            + IsSubType<pallet_shield::Call<T>>,
        OriginOf<T>: OriginTrait<AccountId = T::AccountId>,
        CommitmentPolicy<T>: CanCommit<T::AccountId, Error = Error<T>>,
    {
        let Some(who) = origin.as_signer() else {
            return Ok(());
        };

        CheckColdkeySwap::<T>::check(who, call)?;
        Self::check_basket_calls(who, call)?;
        Self::check_call_guards(who, call)
    }

    /// The per-call checks for `call` dispatched as `who`, other than the coldkey-swap gate.
    fn check_call_guards(who: &T::AccountId, call: &CallOf<T>) -> Result<(), Error<T>>
    where
        T: pallet_commitments::Config,
        CallOf<T>: Dispatchable<RuntimeOrigin = OriginOf<T>>
            + IsSubType<Call<T>>
            + IsSubType<pallet_commitments::Call<T>>,
        CommitmentPolicy<T>: CanCommit<T::AccountId, Error = Error<T>>,
    {
        if let Some(Call::pow_register {
            netuid,
            work_block,
            nonce,
            work,
            hotkey,
        }) = call.is_sub_type()
        {
            Pallet::<T>::check_pow_registration(who, *netuid, *work_block, *nonce, work, hotkey)?;
        }

        let commitment_call: Option<&pallet_commitments::Call<T>> = call.is_sub_type();
        if let Some(pallet_commitments::Call::set_commitment { netuid, .. }) = commitment_call {
            CommitmentPolicy::<T>::validate(*netuid, who)?;
        }

        if let Some(call) = applicable_call(call, CheckWeights::<T>::applies_to) {
            CheckWeights::<T>::check(who, call)?;
        }
        if let Some(call) = applicable_call(call, CheckRateLimits::<T>::applies_to) {
            CheckRateLimits::<T>::check(who, call)?;
        }
        if let Some(call) = applicable_call(call, CheckDelegateTake::<T>::applies_to) {
            CheckDelegateTake::<T>::check(who, call)?;
        }
        if let Some(call) = applicable_call(call, CheckServingEndpoints::<T>::applies_to) {
            CheckServingEndpoints::<T>::check(who, call)?;
        }
        if let Some(call) = applicable_call(call, CheckEvmKeyAssociation::<T>::applies_to) {
            CheckEvmKeyAssociation::<T>::check(who, call)?;
        }

        Ok(())
    }

    /// Validate the calls a fee-free wrapper dispatches.
    ///
    /// `Proxy::proxy` and the `Utility` batches are fee-free when every call they dispatch is
    /// (weight calls are `Pays::No`), but transaction extensions only see the outer call. Without
    /// this walk, a wrapped call that is certain to fail at dispatch would be included for free.
    /// Each dispatched call must pass the runtime's base call filter and the checks it would face
    /// as the outer call of the account it dispatches as, every proxy hop must resolve to a
    /// zero-delay delegation whose type (and every outer hop's type) admits the dispatched calls,
    /// and no two calls may share a pool tag (PoW solution or rate-limited commit). Returns those
    /// tags for the pool.
    fn check_fee_free_wrapped_calls(
        who: &T::AccountId,
        call: &CallOf<T>,
    ) -> Result<Vec<Vec<u8>>, CustomTransactionError>
    where
        T: pallet_commitments::Config
            + pallet_proxy::Config
            + pallet_shield::Config
            + pallet_utility::Config,
        CallOf<T>: Dispatchable<RuntimeOrigin = OriginOf<T>>
            + IsSubType<Call<T>>
            + IsSubType<pallet_commitments::Call<T>>
            + IsSubType<pallet_proxy::Call<T>>
            + IsSubType<pallet_utility::Call<T>>
            + IsSubType<pallet_shield::Call<T>>,
        CommitmentPolicy<T>: CanCommit<T::AccountId, Error = Error<T>>,
    {
        use frame_support::traits::{Contains, InstanceFilter};

        let mut pending = Vec::new();
        if !Self::push_dispatched_calls(who, call, &[], &mut pending)? {
            return Ok(Vec::new());
        }
        // Signed dispatch applies the runtime's base filter (safe mode, no nested batches) to
        // the wrapper and to every call it dispatches.
        ensure!(
            <T as frame_system::Config>::BaseCallFilter::contains(call),
            CustomTransactionError::BadRequest
        );

        let mut tags: Vec<Vec<u8>> = Vec::new();
        while let Some((signer, call, filters)) = pending.pop() {
            let proxied: &<T as pallet_proxy::Config>::RuntimeCall = IsType::from_ref(call);
            ensure!(
                <T as frame_system::Config>::BaseCallFilter::contains(call),
                CustomTransactionError::BadRequest
            );
            ensure!(
                filters.iter().all(|proxy_type| proxy_type.filter(proxied)),
                CustomTransactionError::InvalidRealAccount
            );
            if Self::push_dispatched_calls(&signer, call, &filters, &mut pending)? {
                continue;
            }

            CheckColdkeySwap::<T>::check(&signer, call)
                .and_then(|()| Self::check_call_guards(&signer, call))
                .map_err(CustomTransactionError::from)?;
            for tag in Self::provides_tags(&signer, call) {
                ensure!(
                    !tags.contains(&tag),
                    CustomTransactionError::RateLimitExceeded
                );
                tags.push(tag);
            }
        }
        Ok(tags)
    }

    /// Queue the calls `call` dispatches, with the account and proxy filters each dispatches
    /// under, if `call` is a `Proxy::proxy` or `Utility` wrapper. Returns whether it was one.
    #[allow(clippy::type_complexity)]
    fn push_dispatched_calls<'a>(
        signer: &T::AccountId,
        call: &'a CallOf<T>,
        filters: &[<T as pallet_proxy::Config>::ProxyType],
        pending: &mut Vec<(
            T::AccountId,
            &'a CallOf<T>,
            Vec<<T as pallet_proxy::Config>::ProxyType>,
        )>,
    ) -> Result<bool, CustomTransactionError>
    where
        T: pallet_proxy::Config + pallet_utility::Config,
        CallOf<T>: IsSubType<pallet_proxy::Call<T>> + IsSubType<pallet_utility::Call<T>>,
    {
        let utility_call: Option<&pallet_utility::Call<T>> = call.is_sub_type();
        match utility_call {
            Some(
                pallet_utility::Call::batch { calls }
                | pallet_utility::Call::batch_all { calls }
                | pallet_utility::Call::force_batch { calls },
            ) => {
                pending.extend(
                    calls
                        .iter()
                        .map(|inner| (signer.clone(), inner.into_ref(), filters.to_vec())),
                );
                return Ok(true);
            }
            Some(pallet_utility::Call::as_derivative { index, call }) => {
                let derivative =
                    pallet_utility::Pallet::<T>::derivative_account_id(signer.clone(), *index)
                        .map_err(|_| CustomTransactionError::BadRequest)?;
                pending.push((derivative, call.as_ref().into_ref(), filters.to_vec()));
                return Ok(true);
            }
            Some(pallet_utility::Call::if_else { main, fallback }) => {
                pending.push((signer.clone(), main.as_ref().into_ref(), filters.to_vec()));
                pending.push((
                    signer.clone(),
                    fallback.as_ref().into_ref(),
                    filters.to_vec(),
                ));
                return Ok(true);
            }
            _ => {}
        }

        let proxy_call: Option<&pallet_proxy::Call<T>> = call.is_sub_type();
        if let Some(pallet_proxy::Call::proxy {
            real,
            force_proxy_type,
            call: inner,
        }) = proxy_call
        {
            let real = LookupOf::<T>::lookup(real.clone())
                .map_err(|_| CustomTransactionError::InvalidRealAccount)?;
            let def =
                pallet_proxy::Pallet::<T>::find_proxy(&real, signer, force_proxy_type.clone())
                    .map_err(|_| CustomTransactionError::InvalidRealAccount)?;
            ensure!(
                def.delay.is_zero(),
                CustomTransactionError::InvalidRealAccount
            );
            let mut inner_filters = filters.to_vec();
            inner_filters.push(def.proxy_type);
            pending.push((real, inner.as_ref().into_ref(), inner_filters));
            return Ok(true);
        }
        Ok(false)
    }

    /// Validate basket trades nested in wrappers that can dispatch calls from a signed origin.
    ///
    /// Transaction extensions run only for the outer extrinsic. Walking these calls here keeps a
    /// dust trade from reaching dispatch (and charging the outer fee) through Utility or Proxy.
    fn check_basket_calls(who: &T::AccountId, call: &CallOf<T>) -> Result<(), Error<T>>
    where
        T: pallet_proxy::Config + pallet_utility::Config,
        CallOf<T>: IsSubType<Call<T>>
            + IsSubType<pallet_proxy::Call<T>>
            + IsSubType<pallet_utility::Call<T>>,
    {
        let mut pending = Vec::from([(who.clone(), call)]);

        while let Some((effective_signer, call)) = pending.pop() {
            let subtensor_call: Option<&Call<T>> = call.is_sub_type();
            match subtensor_call {
                Some(Call::swap_basket {
                    hotkey,
                    origin_netuid,
                    destination_netuid,
                    amount,
                    ..
                }) => {
                    Pallet::<T>::check_swap_basket(
                        &effective_signer,
                        hotkey,
                        *origin_netuid,
                        *destination_netuid,
                        amount.to_u64(),
                    )?;
                    Pallet::<T>::ensure_basket_trade_economic(*origin_netuid, amount.to_u64())?;
                    continue;
                }
                Some(Call::swap_basket_many { hotkey, legs }) => {
                    ensure!(!legs.is_empty(), Error::<T>::BasketSwapBatchEmpty);
                    for (origin_netuid, destination_netuid, amount, _) in legs {
                        Pallet::<T>::check_swap_basket(
                            &effective_signer,
                            hotkey,
                            *origin_netuid,
                            *destination_netuid,
                            amount.to_u64(),
                        )?;
                        Pallet::<T>::ensure_basket_trade_economic(*origin_netuid, amount.to_u64())?;
                    }
                    continue;
                }
                _ => {}
            }

            let utility_call: Option<&pallet_utility::Call<T>> = call.is_sub_type();
            match utility_call {
                Some(
                    pallet_utility::Call::batch { calls }
                    | pallet_utility::Call::batch_all { calls }
                    | pallet_utility::Call::force_batch { calls },
                ) => {
                    pending.extend(calls.iter().map(|inner| {
                        let inner: &CallOf<T> = inner.into_ref();
                        (effective_signer.clone(), inner)
                    }));
                    continue;
                }
                Some(pallet_utility::Call::as_derivative { index, call }) => {
                    let derivative = pallet_utility::Pallet::<T>::derivative_account_id(
                        effective_signer,
                        *index,
                    )
                    .map_err(|_| Error::<T>::NonAssociatedColdKey)?;
                    let inner: &CallOf<T> = call.as_ref().into_ref();
                    pending.push((derivative, inner));
                    continue;
                }
                Some(pallet_utility::Call::if_else { main, fallback }) => {
                    let main: &CallOf<T> = main.as_ref().into_ref();
                    let fallback: &CallOf<T> = fallback.as_ref().into_ref();
                    pending.push((effective_signer.clone(), main));
                    pending.push((effective_signer, fallback));
                    continue;
                }
                _ => {}
            }

            let proxy_call: Option<&pallet_proxy::Call<T>> = call.is_sub_type();
            match proxy_call {
                Some(pallet_proxy::Call::proxy {
                    real, call: inner, ..
                })
                | Some(pallet_proxy::Call::proxy_announced {
                    real, call: inner, ..
                }) => {
                    let real = LookupOf::<T>::lookup(real.clone())
                        .map_err(|_| Error::<T>::NonAssociatedColdKey)?;
                    let inner: &CallOf<T> = inner.as_ref().into_ref();
                    pending.push((real, inner));
                }
                _ => {}
            }
        }

        Ok(())
    }

    fn commitment_weight(call: &CallOf<T>) -> Weight
    where
        T: pallet_commitments::Config,
        CallOf<T>: IsSubType<pallet_commitments::Call<T>>,
    {
        let commitment_call: Option<&pallet_commitments::Call<T>> = call.is_sub_type();
        if matches!(
            commitment_call,
            Some(pallet_commitments::Call::set_commitment { .. })
        ) {
            CommitmentPolicy::<T>::validation_weight()
        } else {
            Weight::zero()
        }
    }

    fn basket_trade_weight(call: &CallOf<T>) -> Weight
    where
        T: pallet_proxy::Config + pallet_utility::Config,
        CallOf<T>: IsSubType<Call<T>>
            + IsSubType<pallet_proxy::Call<T>>
            + IsSubType<pallet_utility::Call<T>>,
    {
        let mut weight = Weight::zero();
        let mut pending = Vec::from([call]);

        while let Some(call) = pending.pop() {
            let subtensor_call: Option<&Call<T>> = call.is_sub_type();
            match subtensor_call {
                Some(Call::swap_basket { .. }) => {
                    weight = weight.saturating_add(Pallet::<T>::swap_basket_validation_weight());
                    continue;
                }
                Some(Call::swap_basket_many { legs, .. }) => {
                    weight = weight.saturating_add(
                        Pallet::<T>::swap_basket_validation_weight()
                            .saturating_mul(legs.len() as u64),
                    );
                    continue;
                }
                _ => {}
            }

            let utility_call: Option<&pallet_utility::Call<T>> = call.is_sub_type();
            match utility_call {
                Some(
                    pallet_utility::Call::batch { calls }
                    | pallet_utility::Call::batch_all { calls }
                    | pallet_utility::Call::force_batch { calls },
                ) => {
                    pending.extend(calls.iter().map(|inner| inner.into_ref()));
                    continue;
                }
                Some(pallet_utility::Call::as_derivative { call, .. }) => {
                    pending.push(call.as_ref().into_ref());
                    continue;
                }
                Some(pallet_utility::Call::if_else { main, fallback }) => {
                    pending.push(main.as_ref().into_ref());
                    pending.push(fallback.as_ref().into_ref());
                    continue;
                }
                _ => {}
            }

            let proxy_call: Option<&pallet_proxy::Call<T>> = call.is_sub_type();
            match proxy_call {
                Some(pallet_proxy::Call::proxy { call, .. })
                | Some(pallet_proxy::Call::proxy_announced { call, .. }) => {
                    pending.push(call.as_ref().into_ref());
                }
                _ => {}
            }
        }

        weight
    }

    fn pow_registration_tag(call: &CallOf<T>) -> Option<Vec<u8>>
    where
        CallOf<T>: IsSubType<Call<T>>,
    {
        match call.is_sub_type() {
            Some(Call::<T>::pow_register {
                work_block, hotkey, ..
            }) => Some((b"pow-registration", hotkey, work_block).encode()),
            _ => None,
        }
    }

    /// Pool tags for `call` dispatched as `who`: one PoW solution and one rate-limited weight
    /// commit per tag.
    fn provides_tags(who: &T::AccountId, call: &CallOf<T>) -> Vec<Vec<u8>>
    where
        CallOf<T>: IsSubType<Call<T>>,
    {
        let mut tags: Vec<Vec<u8>> = Self::pow_registration_tag(call).into_iter().collect();
        if let Some(call) = applicable_call(call, CheckRateLimits::<T>::applies_to) {
            tags.extend(CheckRateLimits::<T>::provides_tags(who, call));
        }
        tags
    }

    /// Weight of [`Self::check_call_guards`] plus the coldkey-swap gate for one call.
    fn call_guards_weight(call: &CallOf<T>) -> Weight
    where
        T: pallet_commitments::Config + pallet_shield::Config,
        CallOf<T>: Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>
            + IsSubType<Call<T>>
            + IsSubType<pallet_commitments::Call<T>>
            + IsSubType<pallet_shield::Call<T>>,
        OriginOf<T>: OriginTrait<AccountId = T::AccountId>,
    {
        use DispatchExtension as DE;
        let pow_weight = if matches!(call.is_sub_type(), Some(Call::<T>::pow_register { .. })) {
            <T as Config>::WeightInfo::check_pow_registration()
        } else {
            Weight::zero()
        };
        <CheckColdkeySwap<T> as DE<CallOf<T>>>::weight(call)
            .saturating_add(<CheckWeights<T> as DE<CallOf<T>>>::weight(call))
            .saturating_add(<CheckRateLimits<T> as DE<CallOf<T>>>::weight(call))
            .saturating_add(<CheckDelegateTake<T> as DE<CallOf<T>>>::weight(call))
            .saturating_add(<CheckServingEndpoints<T> as DE<CallOf<T>>>::weight(call))
            .saturating_add(<CheckEvmKeyAssociation<T> as DE<CallOf<T>>>::weight(call))
            .saturating_add(Self::commitment_weight(call))
            .saturating_add(pow_weight)
    }

    /// Weight of [`Self::check_fee_free_wrapped_calls`]: the per-call checks of every dispatched
    /// call, plus one proxy lookup per `Proxy::proxy` hop. Zero unless the extrinsic is fee-free.
    fn fee_free_wrapped_weight(call: &CallOf<T>) -> Weight
    where
        T: pallet_commitments::Config
            + pallet_proxy::Config
            + pallet_shield::Config
            + pallet_utility::Config,
        CallOf<T>: Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>
            + GetDispatchInfo
            + IsSubType<Call<T>>
            + IsSubType<pallet_commitments::Call<T>>
            + IsSubType<pallet_proxy::Call<T>>
            + IsSubType<pallet_utility::Call<T>>
            + IsSubType<pallet_shield::Call<T>>,
        OriginOf<T>: OriginTrait<AccountId = T::AccountId>,
    {
        if call.get_dispatch_info().pays_fee != Pays::No {
            return Weight::zero();
        }

        let mut weight = Weight::zero();
        let mut pending = Vec::from([(call, true)]);
        while let Some((call, is_outer)) = pending.pop() {
            let utility_call: Option<&pallet_utility::Call<T>> = call.is_sub_type();
            match utility_call {
                Some(
                    pallet_utility::Call::batch { calls }
                    | pallet_utility::Call::batch_all { calls }
                    | pallet_utility::Call::force_batch { calls },
                ) => {
                    pending.extend(calls.iter().map(|inner| (inner.into_ref(), false)));
                    continue;
                }
                Some(pallet_utility::Call::as_derivative { call, .. }) => {
                    pending.push((call.as_ref().into_ref(), false));
                    continue;
                }
                Some(pallet_utility::Call::if_else { main, fallback }) => {
                    pending.push((main.as_ref().into_ref(), false));
                    pending.push((fallback.as_ref().into_ref(), false));
                    continue;
                }
                _ => {}
            }

            let proxy_call: Option<&pallet_proxy::Call<T>> = call.is_sub_type();
            if let Some(pallet_proxy::Call::proxy { call, .. }) = proxy_call {
                weight = weight.saturating_add(T::DbWeight::get().reads(1));
                pending.push((call.as_ref().into_ref(), false));
                continue;
            }

            // The outer call's own checks are already counted by `weight`.
            if !is_outer {
                weight = weight.saturating_add(Self::call_guards_weight(call));
            }
        }
        weight
    }
}

impl<T> TransactionExtension<CallOf<T>> for SubtensorTransactionExtension<T>
where
    T: Config
        + pallet_commitments::Config
        + pallet_proxy::Config
        + pallet_shield::Config
        + pallet_utility::Config
        + Send
        + Sync
        + TypeInfo,
    CallOf<T>: Dispatchable<RuntimeOrigin = OriginOf<T>, Info = DispatchInfo, PostInfo = PostDispatchInfo>
        + IsSubType<Call<T>>
        + IsSubType<pallet_commitments::Call<T>>
        + IsSubType<pallet_proxy::Call<T>>
        + IsSubType<pallet_utility::Call<T>>
        + IsSubType<pallet_shield::Call<T>>,
    OriginOf<T>: Clone + OriginTrait<AccountId = T::AccountId>,
    CommitmentPolicy<T>: CanCommit<T::AccountId, Error = Error<T>>,
{
    const IDENTIFIER: &'static str = "SubtensorTransactionExtension";

    type Implicit = ();
    type Val = ();
    type Pre = ();

    fn weight(&self, call: &CallOf<T>) -> Weight {
        Self::call_guards_weight(call)
            .saturating_add(Self::basket_trade_weight(call))
            .saturating_add(Self::fee_free_wrapped_weight(call))
    }

    fn validate(
        &self,
        origin: OriginOf<T>,
        call: &CallOf<T>,
        info: &DispatchInfoOf<CallOf<T>>,
        _len: usize,
        _self_implicit: Self::Implicit,
        _inherited_implication: &impl Implication,
        _source: TransactionSource,
    ) -> ValidateResult<Self::Val, CallOf<T>> {
        Self::check(&origin, call)
            .map_err(CustomTransactionError::from)
            .and_then(|()| {
                let mut validity = ValidTransaction::default();
                if let Some(Call::<T>::pow_register { work_block, .. }) = call.is_sub_type() {
                    validity.longevity = crate::subnets::registration::POW_MAX_WORK_AGE_BLOCKS
                        .saturating_sub(
                            Pallet::<T>::get_current_block_as_u64().saturating_sub(*work_block),
                        )
                        .saturating_add(1);
                }
                match origin.as_signer() {
                    Some(who) => {
                        validity.provides.extend(Self::provides_tags(who, call));
                        if info.pays_fee == Pays::No {
                            validity
                                .provides
                                .extend(Self::check_fee_free_wrapped_calls(who, call)?);
                        }
                    }
                    None => validity.provides.extend(Self::pow_registration_tag(call)),
                }
                Ok((validity, (), origin))
            })
            .map_err(TransactionValidityError::from)
    }

    impl_tx_ext_default!(CallOf<T>; prepare);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::SubtensorTransactionExtension;
    use crate::weights::WeightInfo as _;
    use crate::{
        CheckColdkeySwap, CheckDelegateTake, CheckEvmKeyAssociation, CheckRateLimits,
        CheckServingEndpoints, CheckWeights, ColdkeySwapAnnouncements, ColdkeySwapDisputes,
        tests::mock::*,
    };
    use frame_support::{
        assert_ok,
        dispatch::{DispatchExtension, GetDispatchInfo, Pays},
    };
    use frame_system::RawOrigin;
    use sp_core::U256;
    use sp_runtime::{
        traits::{DispatchInfoOf, Hash, TransactionExtension, TxBaseImplication},
        transaction_validity::{TransactionSource, TransactionValidityError, ValidTransaction},
    };
    use subtensor_runtime_common::{CustomTransactionError, MechId, NetUid};

    fn dispatch_info()
    -> sp_runtime::traits::DispatchInfoOf<<Test as frame_system::Config>::RuntimeCall> {
        DispatchInfoOf::<<Test as frame_system::Config>::RuntimeCall>::default()
    }

    fn validate_signed(
        signer: U256,
        call: &RuntimeCall,
    ) -> Result<ValidTransaction, TransactionValidityError> {
        SubtensorTransactionExtension::<Test>::new()
            .validate(
                RawOrigin::Signed(signer).into(),
                call,
                &dispatch_info(),
                0,
                (),
                &TxBaseImplication(()),
                TransactionSource::External,
            )
            .map(|(validity, _, _)| validity)
    }

    fn expected_transaction_extension_weight(call: &RuntimeCall) -> frame_support::weights::Weight {
        use DispatchExtension as DE;
        <CheckColdkeySwap<Test> as DE<RuntimeCall>>::weight(call)
            .saturating_add(<CheckWeights<Test> as DE<RuntimeCall>>::weight(call))
            .saturating_add(<CheckRateLimits<Test> as DE<RuntimeCall>>::weight(call))
            .saturating_add(<CheckDelegateTake<Test> as DE<RuntimeCall>>::weight(call))
            .saturating_add(<CheckServingEndpoints<Test> as DE<RuntimeCall>>::weight(
                call,
            ))
            .saturating_add(<CheckEvmKeyAssociation<Test> as DE<RuntimeCall>>::weight(
                call,
            ))
            .saturating_add(SubtensorTransactionExtension::<Test>::commitment_weight(
                call,
            ))
    }

    #[test]
    fn validate_accepts_calls_allowed_by_dispatch_extensions() {
        new_test_ext(1).execute_with(|| {
            let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![] });

            assert_ok!(validate_signed(U256::from(1), &call));
        });
    }

    #[test]
    #[allow(deprecated)]
    fn validate_maps_dispatch_extension_errors_to_transaction_errors() {
        new_test_ext(1).execute_with(|| {
            let coldkey = U256::from(1);
            let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![] });
            let new_coldkey_hash =
                <Test as frame_system::Config>::Hashing::hash_of(&U256::from(99));
            let shield = RuntimeCall::Shield(pallet_shield::Call::submit_encrypted {
                ciphertext: vec![1, 2, 3, 4].try_into().unwrap(),
            });

            ColdkeySwapAnnouncements::<Test>::insert(
                coldkey,
                (System::block_number(), new_coldkey_hash),
            );
            let err = validate_signed(coldkey, &call).unwrap_err();
            assert_eq!(err, CustomTransactionError::ColdkeyInSwapSchedule.into());

            ColdkeySwapDisputes::<Test>::insert(coldkey, System::block_number());
            let err = validate_signed(coldkey, &call).unwrap_err();
            assert_eq!(err, CustomTransactionError::ColdkeySwapDisputed.into());
            assert_eq!(
                validate_signed(coldkey, &shield).unwrap_err(),
                CustomTransactionError::ColdkeySwapDisputed.into()
            );

            assert_ok!(SubtensorModule::reset_coldkey_swap_keep_locked(
                RuntimeOrigin::root(),
                coldkey
            ));
            let err = validate_signed(coldkey, &call).unwrap_err();
            assert_eq!(err, CustomTransactionError::ColdkeyInSwapSchedule.into());
            assert_ok!(validate_signed(coldkey, &shield));
            let clear =
                RuntimeCall::SubtensorModule(crate::Call::clear_coldkey_swap_announcement {});
            assert_eq!(
                validate_signed(coldkey, &clear).unwrap_err(),
                CustomTransactionError::ColdkeyInSwapSchedule.into()
            );
            let announce = RuntimeCall::SubtensorModule(crate::Call::announce_coldkey_swap {
                new_coldkey_hash,
            });
            assert_ok!(validate_signed(coldkey, &announce));
        });
    }

    #[test]
    fn pays_no_set_weights_validate_rejects_rate_limited_call() {
        new_test_ext(0).execute_with(|| {
            let netuid = NetUid::from(1);
            let hotkey = U256::from(1);
            let coldkey = U256::from(2);

            add_network_disable_commit_reveal(netuid, 1, 0);
            setup_reserves(
                netuid,
                1_000_000_000_000_u64.into(),
                1_000_000_000_000_u64.into(),
            );
            register_ok_neuron(netuid, hotkey, coldkey, 0);
            SubtensorModule::set_stake_threshold(0);

            SubtensorModule::set_weights_set_rate_limit(netuid, 100);
            System::set_block_number(10_u64);
            let uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey).unwrap();
            let netuid_index = SubtensorModule::get_mechanism_storage_index(netuid, MechId::MAIN);
            SubtensorModule::set_last_update_for_uid(
                netuid_index,
                uid,
                SubtensorModule::get_current_block_as_u64(),
            );

            let call = RuntimeCall::SubtensorModule(SubtensorCall::set_weights {
                netuid,
                dests: vec![uid],
                weights: vec![1],
                version_key: 0,
            });

            assert_eq!(call.get_dispatch_info().pays_fee, Pays::No);
            let err = validate_signed(hotkey, &call).unwrap_err();
            assert_eq!(err, CustomTransactionError::RateLimitExceeded.into());
        });
    }

    // Free (`Pays::No`) weight calls that can only fail at dispatch must be refused at
    // validation, otherwise any key can fill blocks with them at no cost.
    #[test]
    #[allow(deprecated)]
    fn pays_no_weight_calls_that_would_fail_are_rejected_at_validate() {
        new_test_ext(0).execute_with(|| {
            let netuid = NetUid::from(1);
            let stranger = U256::from(777_777);
            add_network_disable_commit_reveal(netuid, 1, 0);
            setup_reserves(
                netuid,
                1_000_000_000_000_u64.into(),
                1_000_000_000_000_u64.into(),
            );
            SubtensorModule::set_stake_threshold(0);
            assert_eq!(Balances::free_balance(stranger), 0.into());

            // Empty per-subnet batches do nothing but are admitted for free without a bound.
            for call in [
                RuntimeCall::SubtensorModule(SubtensorCall::batch_set_weights {
                    netuids: vec![],
                    weights: vec![],
                    version_keys: vec![],
                }),
                RuntimeCall::SubtensorModule(SubtensorCall::batch_commit_weights {
                    netuids: vec![],
                    commit_hashes: vec![],
                }),
                RuntimeCall::SubtensorModule(SubtensorCall::batch_reveal_weights {
                    netuid,
                    uids_list: vec![],
                    values_list: vec![],
                    salts_list: vec![],
                    version_keys: vec![],
                }),
            ] {
                assert_eq!(call.get_dispatch_info().pays_fee, Pays::No);
                assert_eq!(
                    validate_signed(stranger, &call).unwrap_err(),
                    CustomTransactionError::BadRequest.into()
                );
            }
            let oversized_batch = RuntimeCall::SubtensorModule(SubtensorCall::batch_set_weights {
                netuids: vec![codec::Compact(netuid); 1_000],
                weights: vec![vec![]; 1_000],
                version_keys: vec![codec::Compact(0_u64); 1_000],
            });
            assert_eq!(
                validate_signed(stranger, &oversized_batch).unwrap_err(),
                CustomTransactionError::BadRequest.into()
            );

            // set_weights from a hotkey without a uid is a guaranteed dispatch failure.
            let set_weights = RuntimeCall::SubtensorModule(SubtensorCall::set_weights {
                netuid,
                dests: vec![0],
                weights: vec![1],
                version_key: 0,
            });
            assert_eq!(
                validate_signed(stranger, &set_weights).unwrap_err(),
                CustomTransactionError::UidNotFound.into()
            );

            // set_weights on a commit-reveal subnet always fails at dispatch.
            let hotkey = U256::from(1);
            let coldkey = U256::from(2);
            register_ok_neuron(netuid, hotkey, coldkey, 0);
            assert_ok!(validate_signed(hotkey, &set_weights));
            SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
            assert_eq!(
                validate_signed(hotkey, &set_weights).unwrap_err(),
                CustomTransactionError::BadRequest.into()
            );
        });
    }

    // Batched weight calls declare one full per-item unit so the block scheduler books the
    // work they run instead of a constant.
    #[test]
    fn batched_weight_calls_declare_per_item_weight() {
        new_test_ext(0).execute_with(|| {
            let netuid = NetUid::from(1);
            let one_item = |items: usize| {
                RuntimeCall::SubtensorModule(SubtensorCall::batch_set_weights {
                    netuids: vec![codec::Compact(netuid); items],
                    weights: vec![vec![(codec::Compact(0_u16), codec::Compact(1_u16))]; items],
                    version_keys: vec![codec::Compact(0_u64); items],
                })
                .get_dispatch_info()
                .call_weight
            };
            let per_item = <Test as crate::Config>::WeightInfo::set_mechanism_weights(1);
            assert!(one_item(1).all_gte(per_item));
            assert!(one_item(8).all_gte(one_item(1).saturating_add(per_item.saturating_mul(7))));

            let commit_batch = |items: usize| {
                RuntimeCall::SubtensorModule(SubtensorCall::batch_commit_weights {
                    netuids: vec![codec::Compact(netuid); items],
                    commit_hashes: vec![sp_core::H256::zero(); items],
                })
                .get_dispatch_info()
                .call_weight
            };
            let per_commit = <Test as crate::Config>::WeightInfo::commit_weights();
            assert!(
                commit_batch(8)
                    .all_gte(commit_batch(1).saturating_add(per_commit.saturating_mul(7)))
            );

            let reveal = |uids: usize| {
                RuntimeCall::SubtensorModule(SubtensorCall::reveal_weights {
                    netuid,
                    uids: vec![0; uids],
                    values: vec![1; uids],
                    salt: vec![1],
                    version_key: 0,
                })
                .get_dispatch_info()
                .call_weight
            };
            assert!(
                reveal(4096)
                    .all_gte(<Test as crate::Config>::WeightInfo::reveal_mechanism_weights(4096))
            );
            assert!(reveal(4096).all_gt(reveal(1)));
        });
    }

    #[test]
    fn validate_rejects_ineligible_metadata_commitment() {
        new_test_ext(0).execute_with(|| {
            let netuid = NetUid::from(1);
            let hotkey = U256::from(1);
            let coldkey = U256::from(2);
            let commitment_call = || {
                RuntimeCall::Commitments(pallet_commitments::Call::set_commitment {
                    netuid,
                    info: Box::new(pallet_commitments::CommitmentInfo {
                        fields: frame_support::BoundedVec::default(),
                    }),
                })
            };

            assert_eq!(
                validate_signed(hotkey, &commitment_call()).unwrap_err(),
                CustomTransactionError::SubnetNotExists.into()
            );

            add_network(netuid, 1, 0);
            assert_eq!(
                validate_signed(hotkey, &commitment_call()).unwrap_err(),
                CustomTransactionError::UidNotFound.into()
            );

            setup_reserves(
                netuid,
                1_000_000_000_000_u64.into(),
                1_000_000_000_000_u64.into(),
            );
            register_ok_neuron(netuid, hotkey, coldkey, 0);
            assert_ok!(validate_signed(hotkey, &commitment_call()));
        });
    }

    #[test]
    fn timelocked_commits_reject_at_validity_and_conflict_in_pool() {
        new_test_ext(0).execute_with(|| {
            let netuid = NetUid::from(1);
            let hotkey = U256::from(1);
            let coldkey = U256::from(2);

            add_network(netuid, 1, 0);
            setup_reserves(
                netuid,
                1_000_000_000_000_u64.into(),
                1_000_000_000_000_u64.into(),
            );
            register_ok_neuron(netuid, hotkey, coldkey, 0);
            SubtensorModule::set_stake_threshold(0);
            SubtensorModule::set_weights_set_rate_limit(netuid, 100);
            System::set_block_number(10_u64);
            let uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey).unwrap();
            let netuid_index = SubtensorModule::get_mechanism_storage_index(netuid, MechId::MAIN);
            SubtensorModule::set_last_update_for_uid(netuid_index, uid, 10);

            let call =
                RuntimeCall::SubtensorModule(SubtensorCall::commit_timelocked_mechanism_weights {
                    netuid,
                    mecid: MechId::MAIN,
                    commit: Default::default(),
                    reveal_round: 1,
                    commit_reveal_version: 4,
                });
            assert_eq!(
                validate_signed(hotkey, &call).unwrap_err(),
                CustomTransactionError::RateLimitExceeded.into()
            );

            System::set_block_number(200_u64);
            let first = validate_signed(hotkey, &call).unwrap();
            let second = validate_signed(hotkey, &call).unwrap();
            assert_eq!(first.provides.len(), 1);
            assert_eq!(first.provides, second.provides);
        });
    }

    #[test]
    fn timelocked_commits_with_zero_rate_limit_do_not_conflict_in_pool() {
        new_test_ext(0).execute_with(|| {
            let netuid = NetUid::from(1);
            let hotkey = U256::from(1);
            let coldkey = U256::from(2);

            add_network(netuid, 1, 0);
            setup_reserves(
                netuid,
                1_000_000_000_000_u64.into(),
                1_000_000_000_000_u64.into(),
            );
            register_ok_neuron(netuid, hotkey, coldkey, 0);
            SubtensorModule::set_stake_threshold(0);
            SubtensorModule::set_weights_set_rate_limit(netuid, 0);

            let call =
                RuntimeCall::SubtensorModule(SubtensorCall::commit_timelocked_mechanism_weights {
                    netuid,
                    mecid: MechId::MAIN,
                    commit: Default::default(),
                    reveal_round: 1,
                    commit_reveal_version: 4,
                });

            let first = validate_signed(hotkey, &call).unwrap();
            let second = validate_signed(hotkey, &call).unwrap();
            assert!(first.provides.is_empty());
            assert!(second.provides.is_empty());
        });
    }

    #[test]
    fn weight_matches_top_level_dispatch_extension_checks() {
        new_test_ext(1).execute_with(|| {
            let extension = SubtensorTransactionExtension::<Test>::new();
            let calls = [
                RuntimeCall::System(frame_system::Call::remark { remark: vec![] }),
                RuntimeCall::SubtensorModule(SubtensorCall::set_weights {
                    netuid: NetUid::from(1),
                    dests: vec![0],
                    weights: vec![1],
                    version_key: 0,
                }),
                RuntimeCall::SubtensorModule(SubtensorCall::register_network {
                    hotkey: U256::from(9),
                }),
                RuntimeCall::Commitments(pallet_commitments::Call::set_commitment {
                    netuid: NetUid::from(1),
                    info: Box::new(pallet_commitments::CommitmentInfo {
                        fields: frame_support::BoundedVec::default(),
                    }),
                }),
            ];

            for call in calls {
                assert_eq!(
                    TransactionExtension::weight(&extension, &call),
                    expected_transaction_extension_weight(&call)
                );
            }
        });
    }
}
