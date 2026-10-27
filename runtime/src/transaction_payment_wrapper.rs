use crate::{NORMAL_DISPATCH_BASE_PRIORITY, OPERATIONAL_DISPATCH_PRIORITY, Weight};
use codec::{Decode, DecodeWithMemTracking, Encode};
use frame_election_provider_support::private::sp_arithmetic::traits::SaturatedConversion;
use frame_support::dispatch::{DispatchClass, DispatchInfo, PostDispatchInfo};
use frame_support::pallet_prelude::TypeInfo;
use frame_support::traits::{Get, IsSubType, IsType};
use pallet_subtensor_proxy as pallet_proxy;
use pallet_subtensor_utility as pallet_utility;
use pallet_transaction_payment::OnChargeTransaction;
use pallet_transaction_payment::{ChargeTransactionPayment, Config, Pre, Val};
use sp_runtime::DispatchResult;
use sp_runtime::traits::{
    AsSystemOriginSigner, DispatchInfoOf, DispatchOriginOf, Dispatchable, Implication,
    PostDispatchInfoOf, StaticLookup, TransactionExtension, TransactionExtensionMetadata,
    ValidateResult, Zero,
};
use sp_runtime::transaction_validity::{
    TransactionPriority, TransactionSource, TransactionValidity, TransactionValidityError,
};
use sp_std::vec::Vec;
use subtensor_macros::freeze_struct;

type BalanceOf<T> = <<T as Config>::OnChargeTransaction as OnChargeTransaction<T>>::Balance;

type RuntimeCallOf<T> = <T as frame_system::Config>::RuntimeCall;
type RuntimeOriginOf<T> = <T as frame_system::Config>::RuntimeOrigin;
type AccountIdOf<T> = <T as frame_system::Config>::AccountId;
type LookupOf<T> = <T as frame_system::Config>::Lookup;

const MAX_REAL_PAYS_FEE_PROXY_DEPTH: u8 = 3;

/// Runtime-supplied policy: which calls have their fee charged to the signing
/// hotkey's coldkey. Keeping the concrete list in the runtime (see `fee_filters`)
/// lets this generic extension stay free of a hardcoded allow-list.
pub trait ColdkeyFeeCallFilter<Call> {
    fn charges_coldkey(call: &Call) -> bool;
}

/// Weight subsidies apply only to fees, never to block admission or execution accounting.
pub trait FeeWeightDiscount<Call> {
    /// Weight to subtract from `info.call_weight` before pricing the fee.
    fn fee_weight_discount(call: &Call, info: &DispatchInfo) -> Weight;
    fn fee_discount_overhead(call: &Call) -> Weight;
}

pub fn fee_dispatch_info(info: &DispatchInfo, discount: Weight) -> DispatchInfo {
    DispatchInfo {
        call_weight: info.call_weight.saturating_sub(discount),
        ..*info
    }
}

#[freeze_struct("f003cde1f9da4a90")]
#[derive(Encode, Decode, DecodeWithMemTracking, Clone, Eq, PartialEq, TypeInfo)]
#[scale_info(skip_type_params(T))]
pub struct ChargeTransactionPaymentWrapper<T: Config> {
    inner: ChargeTransactionPayment<T>,
}

impl<T: Config> core::fmt::Debug for ChargeTransactionPaymentWrapper<T> {
    #[cfg(feature = "std")]
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "ChargeTransactionPaymentWrapper",)
    }
    #[cfg(not(feature = "std"))]
    fn fmt(&self, _: &mut core::fmt::Formatter) -> core::fmt::Result {
        Ok(())
    }
}

impl<T: Config> ChargeTransactionPaymentWrapper<T>
where
    T::RuntimeCall: Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>,
    BalanceOf<T>: Send + Sync,
{
    pub fn new(fee: BalanceOf<T>) -> Self {
        let inner = ChargeTransactionPayment::<T>::from(fee);
        Self { inner }
    }
}

impl<T: Config + pallet_proxy::Config + pallet_utility::Config + pallet_subtensor::Config>
    ChargeTransactionPaymentWrapper<T>
where
    RuntimeCallOf<T>: IsSubType<pallet_proxy::Call<T>> + IsSubType<pallet_utility::Call<T>>,
    T: ColdkeyFeeCallFilter<RuntimeCallOf<T>>,
    RuntimeOriginOf<T>: AsSystemOriginSigner<AccountIdOf<T>> + Clone,
{
    /// Resolve the real account and inner call of an ordinary `proxy`.
    /// `proxy_announced` does not participate in fee propagation.
    fn resolve_proxy_call(call: &RuntimeCallOf<T>) -> Option<(AccountIdOf<T>, &RuntimeCallOf<T>)> {
        match call.is_sub_type()? {
            pallet_proxy::Call::proxy { real, call, .. } => {
                let real = LookupOf::<T>::lookup(real.clone()).ok()?;
                Some((real, call.as_ref().into_ref()))
            }
            _ => None,
        }
    }

    /// Determine who should pay the transaction fee for a proxy call.
    ///
    /// Follow at most three proxy levels, retaining the nearest opted-in real.
    /// A homogeneous batch of proxies can supply the final level; lookup never
    /// descends into its items. Every hop requires RealPaysFee for its delegate.
    ///
    /// Returns `None` if the signer should pay (no RealPaysFee opt-in).
    fn resolve_real_fee_payer(
        call: &RuntimeCallOf<T>,
        origin: &RuntimeOriginOf<T>,
    ) -> Option<AccountIdOf<T>> {
        let signer = origin.as_system_origin_signer()?;
        let (mut real, mut inner_call) = Self::resolve_proxy_call(call)?;

        if !pallet_proxy::Pallet::<T>::is_real_pays_fee(&real, signer) {
            return None;
        }

        for _ in 1..MAX_REAL_PAYS_FEE_PROXY_DEPTH {
            let Some((inner_real, next_call)) = Self::resolve_proxy_call(inner_call) else {
                return Self::resolve_batch_proxy_payer(inner_call, &real).or(Some(real));
            };
            if !pallet_proxy::Pallet::<T>::is_real_pays_fee(&inner_real, &real) {
                break;
            }

            real = inner_real;
            inner_call = next_call;
        }

        Some(real)
    }

    /// Check if an inner call is a batch where ALL items are proxy calls with the same real
    /// account, and that real account has opted in to pay.
    /// `outer_real` is used as the implicit delegate for `proxy` calls within the batch.
    fn resolve_batch_proxy_payer(
        inner_call: &RuntimeCallOf<T>,
        outer_real: &AccountIdOf<T>,
    ) -> Option<AccountIdOf<T>> {
        let calls: &Vec<<T as pallet_utility::Config>::RuntimeCall> =
            match inner_call.is_sub_type()? {
                pallet_utility::Call::batch { calls }
                | pallet_utility::Call::batch_all { calls }
                | pallet_utility::Call::force_batch { calls } => calls,
                _ => return None,
            };

        if calls.is_empty() {
            return None;
        }

        let mut common_real: Option<AccountIdOf<T>> = None;

        for call in calls.iter() {
            let call_ref: &RuntimeCallOf<T> = call.into_ref();
            let (inner_real, _) = Self::resolve_proxy_call(call_ref)?;

            match &common_real {
                None => {
                    // Every item must have the same real and implicit delegate, so one
                    // RealPaysFee read covers the entire batch.
                    if !pallet_proxy::Pallet::<T>::is_real_pays_fee(&inner_real, outer_real) {
                        return None;
                    }
                    common_real = Some(inner_real);
                }
                // All items must share the same real account.
                Some(existing) if *existing != inner_real => return None,
                _ => {}
            }
        }

        common_real
    }

    /// Determine the coldkey that should pay the fee when a hotkey is the origin.
    ///
    /// Returns `Some(coldkey)` only when the call is runtime-marked as coldkey-paid
    /// (via [`ColdkeyFeeCallFilter`]) and the signer (hotkey) has an owner. The coldkey
    /// covers the protocol fee only; the signer-chosen tip is dropped by the caller
    /// (see `validate`) so a hotkey cannot spend coldkey funds through the tip. Returns
    /// `None` otherwise, so the signer pays.
    fn extract_coldkey_fee_payer(
        call: &RuntimeCallOf<T>,
        origin: &RuntimeOriginOf<T>,
    ) -> Option<AccountIdOf<T>> {
        if !T::charges_coldkey(call) {
            return None;
        }

        let signer = origin.as_system_origin_signer()?;
        pallet_subtensor::Pallet::<T>::maybe_coldkey_for_hotkey(signer)
    }
}

impl<T: Config + pallet_proxy::Config + pallet_utility::Config + pallet_subtensor::Config>
    TransactionExtension<RuntimeCallOf<T>> for ChargeTransactionPaymentWrapper<T>
where
    RuntimeCallOf<T>: Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>
        + IsSubType<pallet_proxy::Call<T>>
        + IsSubType<pallet_utility::Call<T>>,
    T: ColdkeyFeeCallFilter<RuntimeCallOf<T>> + FeeWeightDiscount<RuntimeCallOf<T>>,
    BalanceOf<T>: Zero + Send + Sync,
    RuntimeOriginOf<T>: AsSystemOriginSigner<AccountIdOf<T>>
        + Clone
        + From<frame_system::RawOrigin<AccountIdOf<T>>>,
{
    const IDENTIFIER: &'static str = "ChargeTransactionPaymentWrapper";
    type Implicit = ();
    type Val = Val<T>;
    type Pre = (Pre<T>, Weight);

    fn weight(&self, call: &RuntimeCallOf<T>) -> Weight {
        // Three proxy consent reads (including a final batch lookup), plus margin.
        self.inner
            .weight(call)
            .saturating_add(T::DbWeight::get().reads(4))
            .saturating_add(T::fee_discount_overhead(call))
    }

    fn validate(
        &self,
        origin: DispatchOriginOf<RuntimeCallOf<T>>,
        call: &RuntimeCallOf<T>,
        info: &DispatchInfoOf<RuntimeCallOf<T>>,
        len: usize,
        self_implicit: Self::Implicit,
        inherited_implication: &impl Implication,
        source: TransactionSource,
    ) -> ValidateResult<Self::Val, RuntimeCallOf<T>> {
        let overridden_priority = {
            let base: TransactionPriority = match info.class {
                DispatchClass::Normal => NORMAL_DISPATCH_BASE_PRIORITY,
                DispatchClass::Mandatory => NORMAL_DISPATCH_BASE_PRIORITY,
                DispatchClass::Operational => OPERATIONAL_DISPATCH_PRIORITY,
            };
            base.saturated_into::<TransactionPriority>()
        };

        // Resolve the fee payer. A proxy `RealPaysFee` opt-in takes precedence; otherwise an
        // owned hotkey's coldkey pays for allow-listed calls. Whenever the payer is not the
        // signer, only the protocol fee is redirected — the signer-chosen tip is dropped. A tip
        // does not buy priority in this wrapper (priority is overridden above), and billing it
        // to another account would let the signer spend that account's funds without limit,
        // regardless of what the proxy type allows. Only a self-paying signer keeps its tip.
        let (fee_origin, tip) = if let Some(real) = Self::resolve_real_fee_payer(call, &origin) {
            (frame_system::RawOrigin::Signed(real).into(), Zero::zero())
        } else if let Some(coldkey) = Self::extract_coldkey_fee_payer(call, &origin) {
            (
                frame_system::RawOrigin::Signed(coldkey).into(),
                Zero::zero(),
            )
        } else {
            (origin.clone(), self.inner.tip())
        };

        let fee_info = fee_dispatch_info(info, T::fee_weight_discount(call, info));
        let (mut valid_transaction, val, _fee_origin) = ChargeTransactionPayment::<T>::from(tip)
            .validate(
                fee_origin,
                call,
                &fee_info,
                len,
                self_implicit,
                inherited_implication,
                source,
            )?;

        valid_transaction.priority = overridden_priority;

        // Always return the original origin so the actual signer remains
        // the origin for dispatch and all subsequent extensions.
        Ok((valid_transaction, val, origin))
    }

    fn prepare(
        self,
        val: Self::Val,
        origin: &DispatchOriginOf<RuntimeCallOf<T>>,
        call: &RuntimeCallOf<T>,
        info: &DispatchInfoOf<RuntimeCallOf<T>>,
        len: usize,
    ) -> Result<Self::Pre, TransactionValidityError> {
        let discount = T::fee_weight_discount(call, info);
        let fee_info = fee_dispatch_info(info, discount);
        let pre = self.inner.prepare(val, origin, call, &fee_info, len)?;
        Ok((pre, discount))
    }

    fn metadata() -> Vec<TransactionExtensionMetadata> {
        ChargeTransactionPayment::<T>::metadata()
    }

    fn post_dispatch_details(
        pre: Self::Pre,
        info: &DispatchInfoOf<RuntimeCallOf<T>>,
        post_info: &PostDispatchInfoOf<RuntimeCallOf<T>>,
        len: usize,
        result: &DispatchResult,
    ) -> Result<Weight, TransactionValidityError> {
        let (pre, discount) = pre;
        let fee_info = fee_dispatch_info(info, discount);
        // Cap the fee at its discounted declaration, retaining normal refunds when
        // actual work costs less. Do not modify the execution post-info: CheckWeight
        // must still see the full scan, including work beyond four hotkeys.
        let fee_post_info = PostDispatchInfo {
            actual_weight: Some(
                post_info
                    .calc_actual_weight(info)
                    .min(fee_info.total_weight()),
            ),
            pays_fee: post_info.pays_fee,
        };
        ChargeTransactionPayment::<T>::post_dispatch_details(
            pre,
            &fee_info,
            &fee_post_info,
            len,
            result,
        )
    }

    fn bare_validate(
        call: &RuntimeCallOf<T>,
        info: &DispatchInfoOf<RuntimeCallOf<T>>,
        len: usize,
    ) -> TransactionValidity {
        ChargeTransactionPayment::<T>::bare_validate(call, info, len)
    }

    fn bare_validate_and_prepare(
        call: &RuntimeCallOf<T>,
        info: &DispatchInfoOf<RuntimeCallOf<T>>,
        len: usize,
    ) -> Result<(), TransactionValidityError> {
        ChargeTransactionPayment::<T>::bare_validate_and_prepare(call, info, len)
    }

    fn bare_post_dispatch(
        info: &DispatchInfoOf<RuntimeCallOf<T>>,
        post_info: &mut PostDispatchInfoOf<RuntimeCallOf<T>>,
        len: usize,
        result: &DispatchResult,
    ) -> Result<(), TransactionValidityError> {
        ChargeTransactionPayment::<T>::bare_post_dispatch(info, post_info, len, result)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::ChargeTransactionPaymentWrapper;
    use crate::{
        BuildStorage, NORMAL_DISPATCH_BASE_PRIORITY, OPERATIONAL_DISPATCH_PRIORITY, Proxy, Runtime,
        RuntimeCall, RuntimeGenesisConfig, RuntimeOrigin, System, SystemCall,
    };
    use frame_support::{
        assert_ok,
        dispatch::{DispatchClass, DispatchInfo, GetDispatchInfo, Pays},
    };
    use pallet_subtensor_proxy as pallet_proxy;
    use pallet_subtensor_utility as pallet_utility;
    use pallet_transaction_payment::Val;
    use sp_runtime::Saturating;
    use sp_runtime::traits::{
        AsSystemOriginSigner, DispatchTransaction, Dispatchable, TransactionExtension,
        TxBaseImplication,
    };
    use sp_runtime::transaction_validity::{
        InvalidTransaction, TransactionSource, TransactionValidityError, ValidTransaction,
    };
    use subtensor_runtime_common::{AccountId, NetUid, ProxyType, TaoBalance};

    const SIGNER: [u8; 32] = [1_u8; 32];
    const REAL_A: [u8; 32] = [2_u8; 32];
    const REAL_B: [u8; 32] = [3_u8; 32];
    const OTHER: [u8; 32] = [4_u8; 32];
    const BALANCE: TaoBalance = TaoBalance::new(1_000_000_000_000_u64);

    fn new_test_ext() -> sp_io::TestExternalities {
        sp_tracing::try_init_simple();
        let mut ext: sp_io::TestExternalities = RuntimeGenesisConfig {
            balances: pallet_balances::GenesisConfig {
                balances: vec![
                    (AccountId::from(SIGNER), BALANCE),
                    (AccountId::from(REAL_A), BALANCE),
                    (AccountId::from(REAL_B), BALANCE),
                    (AccountId::from(OTHER), BALANCE),
                ],
                dev_accounts: None,
            },
            ..Default::default()
        }
        .build_storage()
        .unwrap()
        .into();
        ext.execute_with(|| System::set_block_number(1));
        ext
    }

    fn signer() -> AccountId {
        AccountId::from(SIGNER)
    }
    fn real_a() -> AccountId {
        AccountId::from(REAL_A)
    }
    fn real_b() -> AccountId {
        AccountId::from(REAL_B)
    }
    fn other() -> AccountId {
        AccountId::from(OTHER)
    }

    // -- Call builders --

    fn call_remark() -> RuntimeCall {
        RuntimeCall::System(SystemCall::remark {
            remark: vec![1, 2, 3],
        })
    }

    fn call_set_weights() -> RuntimeCall {
        RuntimeCall::SubtensorModule(pallet_subtensor::Call::set_weights {
            netuid: NetUid::from(1),
            dests: vec![0],
            weights: vec![1],
            version_key: 0,
        })
    }

    fn call_commit_weights() -> RuntimeCall {
        RuntimeCall::SubtensorModule(pallet_subtensor::Call::commit_weights {
            netuid: NetUid::from(1),
            commit_hash: sp_core::H256::zero(),
        })
    }

    fn proxy_call(real: AccountId, inner: RuntimeCall) -> RuntimeCall {
        RuntimeCall::Proxy(pallet_proxy::Call::proxy {
            real: real.into(),
            force_proxy_type: None,
            call: Box::new(inner),
        })
    }

    fn proxy_announced_call(
        delegate: AccountId,
        real: AccountId,
        inner: RuntimeCall,
    ) -> RuntimeCall {
        RuntimeCall::Proxy(pallet_proxy::Call::proxy_announced {
            delegate: delegate.into(),
            real: real.into(),
            force_proxy_type: None,
            call: Box::new(inner),
        })
    }

    fn batch_call(calls: Vec<RuntimeCall>) -> RuntimeCall {
        RuntimeCall::Utility(pallet_utility::Call::batch { calls })
    }

    fn batch_all_call(calls: Vec<RuntimeCall>) -> RuntimeCall {
        RuntimeCall::Utility(pallet_utility::Call::batch_all { calls })
    }

    fn force_batch_call(calls: Vec<RuntimeCall>) -> RuntimeCall {
        RuntimeCall::Utility(pallet_utility::Call::force_batch { calls })
    }

    // -- Setup helpers --

    fn add_proxy(real: &AccountId, delegate: &AccountId) {
        add_proxy_of(real, delegate, ProxyType::Any);
    }

    fn add_proxy_of(real: &AccountId, delegate: &AccountId, proxy_type: ProxyType) {
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(real.clone()),
            delegate.clone().into(),
            proxy_type,
            0,
        ));
    }

    fn enable_real_pays_fee(real: &AccountId, delegate: &AccountId) {
        assert_ok!(Proxy::set_real_pays_fee(
            RuntimeOrigin::signed(real.clone()),
            delegate.clone().into(),
            true,
        ));
    }

    // -- Validate helpers --

    fn validate_call(
        origin: RuntimeOrigin,
        call: &RuntimeCall,
    ) -> Result<(ValidTransaction, Val<Runtime>), TransactionValidityError> {
        validate_call_with_info(origin, call, &call.get_dispatch_info())
    }

    fn validate_call_with_info(
        origin: RuntimeOrigin,
        call: &RuntimeCall,
        info: &DispatchInfo,
    ) -> Result<(ValidTransaction, Val<Runtime>), TransactionValidityError> {
        let ext = ChargeTransactionPaymentWrapper::<Runtime>::new(TaoBalance::new(0));
        let (valid_tx, val, _origin) = ext.validate(
            origin,
            call,
            info,
            100,
            (),
            &TxBaseImplication(()),
            TransactionSource::External,
        )?;
        Ok((valid_tx, val))
    }

    /// Extract the fee payer from the validate result.
    fn fee_payer(val: &Val<Runtime>) -> AccountId {
        match val {
            Val::Charge { who, .. } => who.clone(),
            _ => panic!("expected Val::Charge"),
        }
    }

    // ============================================================
    // Case 0: Non-proxy calls
    // ============================================================

    #[test]
    fn non_proxy_call_charges_signer() {
        new_test_ext().execute_with(|| {
            let call = call_remark();
            let (_valid_tx, val) = validate_call(RuntimeOrigin::signed(signer()), &call).unwrap();
            assert_eq!(fee_payer(&val), signer());
        });
    }

    // ============================================================
    // Case 1: Simple proxy (1 level)
    // ============================================================

    #[test]
    fn simple_proxy_charges_real_when_opted_in() {
        new_test_ext().execute_with(|| {
            add_proxy(&real_a(), &signer());
            enable_real_pays_fee(&real_a(), &signer());

            let call = proxy_call(real_a(), call_remark());
            let (_valid_tx, val) = validate_call(RuntimeOrigin::signed(signer()), &call).unwrap();
            assert_eq!(fee_payer(&val), real_a());
        });
    }

    #[test]
    fn simple_proxy_charges_signer_when_not_opted_in() {
        new_test_ext().execute_with(|| {
            add_proxy(&real_a(), &signer());
            // No enable_real_pays_fee

            let call = proxy_call(real_a(), call_remark());
            let (_valid_tx, val) = validate_call(RuntimeOrigin::signed(signer()), &call).unwrap();
            assert_eq!(fee_payer(&val), signer());
        });
    }

    #[test]
    fn proxy_announced_always_charges_signer() {
        new_test_ext().execute_with(|| {
            add_proxy(&real_a(), &real_b());
            enable_real_pays_fee(&real_a(), &real_b());

            // Fee propagation intentionally ignores proxy_announced; signer always pays.
            let call = proxy_announced_call(real_b(), real_a(), call_remark());
            let (_valid_tx, val) = validate_call(RuntimeOrigin::signed(signer()), &call).unwrap();
            assert_eq!(fee_payer(&val), signer());
        });
    }

    // ============================================================
    // Case 2: Nested proxy (2 levels)
    // ============================================================

    #[test]
    fn nested_proxy_charges_inner_real_when_both_opted_in() {
        new_test_ext().execute_with(|| {
            // Chain: signer → real_b → real_a
            add_proxy(&real_b(), &signer());
            enable_real_pays_fee(&real_b(), &signer());
            add_proxy(&real_a(), &real_b());
            enable_real_pays_fee(&real_a(), &real_b());

            let call = proxy_call(real_b(), proxy_call(real_a(), call_remark()));
            let (_valid_tx, val) = validate_call(RuntimeOrigin::signed(signer()), &call).unwrap();
            assert_eq!(fee_payer(&val), real_a());
        });
    }

    #[test]
    fn nested_proxy_charges_outer_real_when_only_outer_opted_in() {
        new_test_ext().execute_with(|| {
            add_proxy(&real_b(), &signer());
            enable_real_pays_fee(&real_b(), &signer());
            add_proxy(&real_a(), &real_b());
            // No enable_real_pays_fee for A→B

            let call = proxy_call(real_b(), proxy_call(real_a(), call_remark()));
            let (_valid_tx, val) = validate_call(RuntimeOrigin::signed(signer()), &call).unwrap();
            assert_eq!(fee_payer(&val), real_b());
        });
    }

    #[test]
    fn nested_proxy_charges_signer_when_neither_opted_in() {
        new_test_ext().execute_with(|| {
            add_proxy(&real_b(), &signer());
            add_proxy(&real_a(), &real_b());
            // No enable_real_pays_fee at all

            let call = proxy_call(real_b(), proxy_call(real_a(), call_remark()));
            let (_valid_tx, val) = validate_call(RuntimeOrigin::signed(signer()), &call).unwrap();
            assert_eq!(fee_payer(&val), signer());
        });
    }

    #[test]
    fn nested_proxy_charges_signer_when_only_inner_opted_in() {
        new_test_ext().execute_with(|| {
            add_proxy(&real_b(), &signer());
            // No enable_real_pays_fee for B→signer
            add_proxy(&real_a(), &real_b());
            enable_real_pays_fee(&real_a(), &real_b());

            let call = proxy_call(real_b(), proxy_call(real_a(), call_remark()));
            let (_valid_tx, val) = validate_call(RuntimeOrigin::signed(signer()), &call).unwrap();
            // Outer RealPaysFee not set → signer pays (inner opt-in is irrelevant)
            assert_eq!(fee_payer(&val), signer());
        });
    }

    // ============================================================
    // Case 3: Batch of proxy calls
    // ============================================================

    #[test]
    fn batch_charges_inner_real_when_all_opted_in() {
        new_test_ext().execute_with(|| {
            add_proxy(&real_b(), &signer());
            enable_real_pays_fee(&real_b(), &signer());
            add_proxy(&real_a(), &real_b());
            enable_real_pays_fee(&real_a(), &real_b());

            let batch = batch_call(vec![
                proxy_call(real_a(), call_remark()),
                proxy_call(real_a(), call_remark()),
            ]);
            let call = proxy_call(real_b(), batch);
            let (_valid_tx, val) = validate_call(RuntimeOrigin::signed(signer()), &call).unwrap();
            assert_eq!(fee_payer(&val), real_a());
        });
    }

    #[test]
    fn batch_all_charges_inner_real_when_all_opted_in() {
        new_test_ext().execute_with(|| {
            add_proxy(&real_b(), &signer());
            enable_real_pays_fee(&real_b(), &signer());
            add_proxy(&real_a(), &real_b());
            enable_real_pays_fee(&real_a(), &real_b());

            let batch = batch_all_call(vec![
                proxy_call(real_a(), call_remark()),
                proxy_call(real_a(), call_remark()),
            ]);
            let call = proxy_call(real_b(), batch);
            let (_valid_tx, val) = validate_call(RuntimeOrigin::signed(signer()), &call).unwrap();
            assert_eq!(fee_payer(&val), real_a());
        });
    }

    #[test]
    fn force_batch_charges_inner_real_when_all_opted_in() {
        new_test_ext().execute_with(|| {
            add_proxy(&real_b(), &signer());
            enable_real_pays_fee(&real_b(), &signer());
            add_proxy(&real_a(), &real_b());
            enable_real_pays_fee(&real_a(), &real_b());

            let batch = force_batch_call(vec![
                proxy_call(real_a(), call_remark()),
                proxy_call(real_a(), call_remark()),
            ]);
            let call = proxy_call(real_b(), batch);
            let (_valid_tx, val) = validate_call(RuntimeOrigin::signed(signer()), &call).unwrap();
            assert_eq!(fee_payer(&val), real_a());
        });
    }

    #[test]
    fn batch_charges_outer_real_when_only_outer_opted_in() {
        new_test_ext().execute_with(|| {
            add_proxy(&real_b(), &signer());
            enable_real_pays_fee(&real_b(), &signer());
            add_proxy(&real_a(), &real_b());
            // No enable_real_pays_fee for A→B

            let batch = batch_call(vec![
                proxy_call(real_a(), call_remark()),
                proxy_call(real_a(), call_remark()),
            ]);
            let call = proxy_call(real_b(), batch);
            let (_valid_tx, val) = validate_call(RuntimeOrigin::signed(signer()), &call).unwrap();
            assert_eq!(fee_payer(&val), real_b());
        });
    }

    #[test]
    fn batch_charges_outer_real_when_mixed_inner_reals() {
        new_test_ext().execute_with(|| {
            add_proxy(&real_b(), &signer());
            enable_real_pays_fee(&real_b(), &signer());
            add_proxy(&real_a(), &real_b());
            enable_real_pays_fee(&real_a(), &real_b());
            add_proxy(&other(), &real_b());
            enable_real_pays_fee(&other(), &real_b());

            // Different inner reals → can't push deeper
            let batch = batch_call(vec![
                proxy_call(real_a(), call_remark()),
                proxy_call(other(), call_remark()),
            ]);
            let call = proxy_call(real_b(), batch);
            let (_valid_tx, val) = validate_call(RuntimeOrigin::signed(signer()), &call).unwrap();
            assert_eq!(fee_payer(&val), real_b());
        });
    }

    #[test]
    fn batch_charges_outer_real_when_non_proxy_in_batch() {
        new_test_ext().execute_with(|| {
            add_proxy(&real_b(), &signer());
            enable_real_pays_fee(&real_b(), &signer());

            // A non-proxy batch item prevents further fee propagation.
            let batch = batch_call(vec![proxy_call(real_a(), call_remark()), call_remark()]);
            let call = proxy_call(real_b(), batch);
            let (_valid_tx, val) = validate_call(RuntimeOrigin::signed(signer()), &call).unwrap();
            assert_eq!(fee_payer(&val), real_b());
        });
    }

    #[test]
    fn batch_charges_outer_real_when_empty() {
        new_test_ext().execute_with(|| {
            add_proxy(&real_b(), &signer());
            enable_real_pays_fee(&real_b(), &signer());

            let batch = batch_call(vec![]);
            let call = proxy_call(real_b(), batch);
            let (_valid_tx, val) = validate_call(RuntimeOrigin::signed(signer()), &call).unwrap();
            assert_eq!(fee_payer(&val), real_b());
        });
    }

    #[test]
    fn batch_charges_outer_real_when_inner_real_not_opted_in() {
        new_test_ext().execute_with(|| {
            add_proxy(&real_b(), &signer());
            enable_real_pays_fee(&real_b(), &signer());
            add_proxy(&real_a(), &real_b());
            // real_a has NOT opted in to pay for real_b

            // Even with same real in all batch items, if RealPaysFee<A, B> not set → outer_real pays
            let batch = batch_call(vec![
                proxy_call(real_a(), call_remark()),
                proxy_call(real_a(), call_remark()),
            ]);
            let call = proxy_call(real_b(), batch);
            let (_valid_tx, val) = validate_call(RuntimeOrigin::signed(signer()), &call).unwrap();
            assert_eq!(fee_payer(&val), real_b());
        });
    }

    // ============================================================
    // Three-level proxy chains and propagation boundaries
    // ============================================================

    #[test]
    fn three_level_proxy_charges_deepest_real() {
        new_test_ext().execute_with(|| {
            for (real, delegate) in [
                (real_b(), signer()),
                (real_a(), real_b()),
                (other(), real_a()),
            ] {
                add_proxy(&real, &delegate);
                enable_real_pays_fee(&real, &delegate);
            }

            let call = proxy_call(
                real_b(),
                proxy_call(real_a(), proxy_call(other(), call_remark())),
            );
            let (_, val) = validate_call(RuntimeOrigin::signed(signer()), &call).unwrap();
            assert_eq!(fee_payer(&val), other());
        });
    }

    #[test]
    fn three_level_proxy_requires_consent_at_every_hop() {
        for (consent, expected) in [
            ([false, false, false], signer()),
            ([false, false, true], signer()),
            ([false, true, false], signer()),
            ([false, true, true], signer()),
            ([true, false, false], real_b()),
            ([true, false, true], real_b()),
            ([true, true, false], real_a()),
            ([true, true, true], other()),
        ] {
            new_test_ext().execute_with(|| {
                for ((real, delegate), pays_fee) in [
                    (real_b(), signer()),
                    (real_a(), real_b()),
                    (other(), real_a()),
                ]
                .into_iter()
                .zip(consent)
                {
                    add_proxy(&real, &delegate);
                    if pays_fee {
                        enable_real_pays_fee(&real, &delegate);
                    }
                }

                let inner = proxy_call(other(), call_remark());
                for last_level in [
                    inner.clone(),
                    batch_call(vec![inner.clone(), inner.clone()]),
                    batch_all_call(vec![inner.clone(), inner.clone()]),
                    force_batch_call(vec![inner.clone(), inner]),
                ] {
                    let call = proxy_call(real_b(), proxy_call(real_a(), last_level));
                    let (valid_tx, val) =
                        validate_call(RuntimeOrigin::signed(signer()), &call).unwrap();
                    assert_eq!(fee_payer(&val), expected, "consent: {consent:?}, {call:?}");
                    assert_eq!(valid_tx.priority, NORMAL_DISPATCH_BASE_PRIORITY);
                }
            });
        }
    }

    #[test]
    fn three_level_batch_keeps_middle_payer_when_items_are_unsupported() {
        new_test_ext().execute_with(|| {
            for (real, delegate) in [
                (real_b(), signer()),
                (real_a(), real_b()),
                (other(), real_a()),
                (signer(), real_a()),
            ] {
                add_proxy(&real, &delegate);
                enable_real_pays_fee(&real, &delegate);
            }

            for calls in [
                vec![],
                vec![proxy_call(other(), call_remark()), call_remark()],
                vec![call_remark(), proxy_call(other(), call_remark())],
                vec![
                    proxy_call(other(), call_remark()),
                    proxy_call(signer(), call_remark()),
                ],
                vec![
                    proxy_call(signer(), call_remark()),
                    proxy_call(other(), call_remark()),
                ],
                vec![
                    proxy_call(other(), call_remark()),
                    proxy_announced_call(real_a(), other(), call_remark()),
                ],
                vec![batch_call(vec![proxy_call(other(), call_remark())])],
            ] {
                for batch in [
                    batch_call(calls.clone()),
                    batch_all_call(calls.clone()),
                    force_batch_call(calls.clone()),
                ] {
                    let call = proxy_call(real_b(), proxy_call(real_a(), batch));
                    let (_, val) = validate_call(RuntimeOrigin::signed(signer()), &call).unwrap();
                    assert_eq!(fee_payer(&val), real_a(), "{call:?}");
                }
            }
        });
    }

    #[test]
    fn batch_fee_lookup_never_descends_into_items() {
        new_test_ext().execute_with(|| {
            for (real, delegate) in [
                (real_b(), signer()),
                (real_a(), real_b()),
                (other(), real_a()),
            ] {
                add_proxy(&real, &delegate);
                enable_real_pays_fee(&real, &delegate);
            }

            // There is still depth available, but a batch must terminate the lookup.
            let nested = proxy_call(real_a(), proxy_call(other(), call_remark()));
            for calls in [
                vec![nested.clone(), nested.clone()],
                vec![nested.clone(), proxy_call(real_a(), call_remark())],
            ] {
                for batch in [
                    batch_call(calls.clone()),
                    batch_all_call(calls.clone()),
                    force_batch_call(calls.clone()),
                ] {
                    let call = proxy_call(real_b(), batch);
                    let (_, val) = validate_call(RuntimeOrigin::signed(signer()), &call).unwrap();
                    assert_eq!(fee_payer(&val), real_a(), "{call:?}");
                }
            }
        });
    }

    #[test]
    fn proxy_fee_lookup_stops_at_three_levels() {
        new_test_ext().execute_with(|| {
            for (real, delegate) in [
                (real_b(), signer()),
                (real_a(), real_b()),
                (other(), real_a()),
                (signer(), other()),
            ] {
                add_proxy(&real, &delegate);
                enable_real_pays_fee(&real, &delegate);
            }

            let fourth = proxy_call(signer(), call_remark());
            for inner in [
                fourth.clone(),
                batch_call(vec![fourth.clone()]),
                batch_all_call(vec![fourth.clone()]),
                force_batch_call(vec![fourth]),
            ] {
                let call = proxy_call(real_b(), proxy_call(real_a(), proxy_call(other(), inner)));
                let (_, val) = validate_call(RuntimeOrigin::signed(signer()), &call).unwrap();
                assert_eq!(fee_payer(&val), other(), "{call:?}");
            }
        });
    }

    #[test]
    fn nested_announced_proxy_does_not_propagate_fees() {
        new_test_ext().execute_with(|| {
            for (real, delegate) in [
                (real_b(), signer()),
                (real_a(), real_b()),
                (other(), real_a()),
            ] {
                add_proxy(&real, &delegate);
                enable_real_pays_fee(&real, &delegate);
            }

            for (call, expected) in [
                (
                    proxy_call(
                        real_b(),
                        proxy_announced_call(
                            real_b(),
                            real_a(),
                            proxy_call(other(), call_remark()),
                        ),
                    ),
                    real_b(),
                ),
                (
                    proxy_call(
                        real_b(),
                        proxy_call(
                            real_a(),
                            proxy_announced_call(real_a(), other(), call_remark()),
                        ),
                    ),
                    real_a(),
                ),
            ] {
                let (_, val) = validate_call(RuntimeOrigin::signed(signer()), &call).unwrap();
                assert_eq!(fee_payer(&val), expected);
            }
        });
    }

    #[test]
    fn three_level_proxy_respects_revoked_consent() {
        new_test_ext().execute_with(|| {
            for (real, delegate) in [
                (real_b(), signer()),
                (real_a(), real_b()),
                (other(), real_a()),
            ] {
                add_proxy(&real, &delegate);
                enable_real_pays_fee(&real, &delegate);
            }
            let call = proxy_call(
                real_b(),
                proxy_call(real_a(), proxy_call(other(), call_remark())),
            );
            let (_, val) = validate_call(RuntimeOrigin::signed(signer()), &call).unwrap();
            assert_eq!(fee_payer(&val), other());

            assert_ok!(Proxy::set_real_pays_fee(
                RuntimeOrigin::signed(other()),
                real_a().into(),
                false,
            ));
            let (_, val) = validate_call(RuntimeOrigin::signed(signer()), &call).unwrap();
            assert_eq!(fee_payer(&val), real_a());

            assert_ok!(Proxy::remove_proxy(
                RuntimeOrigin::signed(real_a()),
                real_b().into(),
                ProxyType::Any,
                0,
            ));
            let (_, val) = validate_call(RuntimeOrigin::signed(signer()), &call).unwrap();
            assert_eq!(fee_payer(&val), real_b());
        });
    }

    #[test]
    fn three_level_proxy_does_not_fall_back_when_payer_cannot_afford_fee() {
        new_test_ext().execute_with(|| {
            for (real, delegate) in [
                (real_b(), signer()),
                (real_a(), real_b()),
                (other(), real_a()),
            ] {
                add_proxy(&real, &delegate);
                enable_real_pays_fee(&real, &delegate);
            }
            assert_ok!(pallet_balances::Pallet::<Runtime>::force_set_balance(
                RuntimeOrigin::root(),
                other().into(),
                crate::ExistentialDeposit::get(),
            ));

            let call = proxy_call(
                real_b(),
                proxy_call(real_a(), proxy_call(other(), call_remark())),
            );
            assert!(matches!(
                validate_call(RuntimeOrigin::signed(signer()), &call),
                Err(TransactionValidityError::Invalid(
                    InvalidTransaction::Payment
                ))
            ));
        });
    }

    #[test]
    fn three_level_proxy_dispatch_charges_deepest_real_without_tip() {
        new_test_ext().execute_with(|| {
            for (real, delegate) in [
                (real_b(), signer()),
                (real_a(), real_b()),
                (other(), real_a()),
            ] {
                add_proxy(&real, &delegate);
                enable_real_pays_fee(&real, &delegate);
            }

            let inner = proxy_call(other(), call_remark());
            for last_level in [
                inner.clone(),
                batch_call(vec![inner.clone(), inner.clone()]),
                batch_all_call(vec![inner.clone(), inner.clone()]),
                force_batch_call(vec![inner.clone(), inner]),
            ] {
                let call = proxy_call(real_b(), proxy_call(real_a(), last_level));
                let info = call.get_dispatch_info();
                let payer_before = pallet_balances::Pallet::<Runtime>::free_balance(other());
                let unchanged = [signer(), real_b(), real_a()].map(|account| {
                    let balance = pallet_balances::Pallet::<Runtime>::free_balance(&account);
                    (account, balance)
                });
                let tip = payer_before.saturating_sub(TaoBalance::new(1_000_000_000));
                let ext = ChargeTransactionPaymentWrapper::<Runtime>::new(tip);
                let post_info = ext
                    .test_run(
                        RuntimeOrigin::signed(signer()),
                        &call,
                        &info,
                        100,
                        0,
                        |origin| {
                            assert_eq!(origin.as_system_origin_signer(), Some(&signer()));
                            call.clone().dispatch(origin)
                        },
                    )
                    .unwrap()
                    .unwrap();

                for real in [real_b(), real_a(), other()] {
                    assert_eq!(
                        pallet_proxy::LastCallResult::<Runtime>::get(real),
                        Some(Ok(()))
                    );
                }
                let fee = pallet_transaction_payment::Pallet::<Runtime>::compute_actual_fee(
                    100,
                    &info,
                    &post_info,
                    TaoBalance::new(0),
                );
                assert!(fee > TaoBalance::new(0));
                assert_eq!(
                    payer_before
                        .saturating_sub(pallet_balances::Pallet::<Runtime>::free_balance(other())),
                    fee,
                );
                for (account, before) in unchanged {
                    assert_eq!(
                        pallet_balances::Pallet::<Runtime>::free_balance(account),
                        before
                    );
                }
            }
        });
    }

    #[test]
    fn three_level_validate_proxy_still_requires_consent_at_every_hop() {
        // Validate is a narrow filter; fee redirection is independent of proxy
        // type and must still demand RealPaysFee on every hop (#3270).
        new_test_ext().execute_with(|| {
            for (real, delegate) in [
                (real_b(), signer()),
                (real_a(), real_b()),
                (other(), real_a()),
            ] {
                add_proxy_of(&real, &delegate, ProxyType::Validate);
            }
            // Outer opt-in only → outer pays; no deeper consent means no
            // unconsenting real is charged.
            enable_real_pays_fee(&real_b(), &signer());
            let call = proxy_call(
                real_b(),
                proxy_call(real_a(), proxy_call(other(), call_set_weights())),
            );
            let (_, val) = validate_call(RuntimeOrigin::signed(signer()), &call).unwrap();
            assert_eq!(fee_payer(&val), real_b());

            enable_real_pays_fee(&real_a(), &real_b());
            enable_real_pays_fee(&other(), &real_a());
            let (_, val) = validate_call(RuntimeOrigin::signed(signer()), &call).unwrap();
            assert_eq!(fee_payer(&val), other());
        });
    }

    #[test]
    fn proxy_fee_weight_covers_three_consent_reads_and_batch_margin() {
        new_test_ext().execute_with(|| {
            let call = proxy_call(real_a(), call_remark());
            let declared =
                ChargeTransactionPaymentWrapper::<Runtime>::new(TaoBalance::new(0)).weight(&call);
            let expected_reads = <Runtime as frame_system::Config>::DbWeight::get().reads(4);
            // Hard lower bound: the wrapper adds four consent/batch reads on top
            // of the inner ChargeTransactionPayment weight.
            assert!(
                declared.proof_size() >= expected_reads.proof_size(),
                "declared={declared:?} expected at least {expected_reads:?}"
            );
        });
    }

    // ============================================================
    // Priority override
    // ============================================================

    #[test]
    fn priority_override_normal_dispatch() {
        new_test_ext().execute_with(|| {
            let call = call_remark();
            let (valid_tx, _val) = validate_call(RuntimeOrigin::signed(signer()), &call).unwrap();
            assert_eq!(valid_tx.priority, NORMAL_DISPATCH_BASE_PRIORITY);
        });
    }

    #[test]
    fn priority_override_operational_dispatch() {
        new_test_ext().execute_with(|| {
            let call = call_remark();
            let mut info = call.get_dispatch_info();
            info.class = DispatchClass::Operational;

            let (valid_tx, _val) =
                validate_call_with_info(RuntimeOrigin::signed(signer()), &call, &info).unwrap();
            assert_eq!(valid_tx.priority, OPERATIONAL_DISPATCH_PRIORITY);
        });
    }

    #[test]
    fn priority_override_mandatory_dispatch() {
        new_test_ext().execute_with(|| {
            let call = call_remark();
            let mut info = call.get_dispatch_info();
            info.class = DispatchClass::Mandatory;

            let (valid_tx, _val) =
                validate_call_with_info(RuntimeOrigin::signed(signer()), &call, &info).unwrap();
            // Mandatory uses the same base as Normal
            assert_eq!(valid_tx.priority, NORMAL_DISPATCH_BASE_PRIORITY);
        });
    }

    #[test]
    fn priority_override_applies_with_real_pays_fee() {
        new_test_ext().execute_with(|| {
            add_proxy(&real_a(), &signer());
            enable_real_pays_fee(&real_a(), &signer());

            let call = proxy_call(real_a(), call_remark());
            let (valid_tx, _val) = validate_call(RuntimeOrigin::signed(signer()), &call).unwrap();
            // Priority override should still apply when real pays fee
            assert_eq!(valid_tx.priority, NORMAL_DISPATCH_BASE_PRIORITY);
        });
    }

    // ============================================================
    // Coldkey pays the fee when a hotkey is the origin
    // ============================================================

    #[test]
    fn hotkey_with_owner_charges_coldkey() {
        new_test_ext().execute_with(|| {
            let hotkey = signer();
            let coldkey = other();

            pallet_subtensor::Owner::<Runtime>::insert(hotkey.clone(), coldkey.clone());

            let call = call_set_weights();
            let (_valid_tx, val) = validate_call(RuntimeOrigin::signed(hotkey), &call).unwrap();

            assert_eq!(fee_payer(&val), coldkey);
        });
    }

    // Guards against the gate only recognizing `set_weights` rather than the whole group.
    #[test]
    fn other_group_member_charges_coldkey() {
        new_test_ext().execute_with(|| {
            let hotkey = signer();
            let coldkey = other();

            pallet_subtensor::Owner::<Runtime>::insert(hotkey.clone(), coldkey.clone());

            let call = call_commit_weights();
            let (_valid_tx, val) = validate_call(RuntimeOrigin::signed(hotkey), &call).unwrap();

            assert_eq!(fee_payer(&val), coldkey);
        });
    }

    #[test]
    fn hotkey_without_owner_charges_signer() {
        new_test_ext().execute_with(|| {
            let hotkey = signer();

            let call = call_set_weights();
            let (_valid_tx, val) =
                validate_call(RuntimeOrigin::signed(hotkey.clone()), &call).unwrap();

            assert_eq!(fee_payer(&val), hotkey);
        });
    }

    // Full lifecycle (validate → prepare → post_dispatch) via `DispatchTransaction::test_run`,
    // which validate-only tests cannot reach: a `Pays::Yes` allow-listed call debits the coldkey
    // and leaves the hotkey untouched, and a signer-chosen tip is excluded from the debit.
    #[test]
    fn full_lifecycle_debits_coldkey_excluding_tip() {
        new_test_ext().execute_with(|| {
            let hotkey = signer();
            let coldkey = other();

            pallet_subtensor::Owner::<Runtime>::insert(hotkey.clone(), coldkey.clone());

            let call = call_set_weights();
            // Force a fee-bearing call; the real `set_weights` is `Pays::No` in this PR.
            let info = DispatchInfo {
                pays_fee: Pays::Yes,
                ..call.get_dispatch_info()
            };

            let hotkey_before = pallet_balances::Pallet::<Runtime>::free_balance(&hotkey);
            let coldkey_before = pallet_balances::Pallet::<Runtime>::free_balance(&coldkey);

            // Sign with a large tip; it must not reach the coldkey.
            let ext = ChargeTransactionPaymentWrapper::<Runtime>::new(TaoBalance::new(1_000_000));
            assert_ok!(ext.test_run(
                RuntimeOrigin::signed(hotkey.clone()),
                &call,
                &info,
                0,
                0,
                |_origin| Ok(Default::default()),
            ));

            let tipless_fee = pallet_transaction_payment::Pallet::<Runtime>::compute_fee(
                0,
                &info,
                TaoBalance::new(0),
            );
            let coldkey_after = pallet_balances::Pallet::<Runtime>::free_balance(&coldkey);

            assert_eq!(
                pallet_balances::Pallet::<Runtime>::free_balance(&hotkey),
                hotkey_before
            );
            assert!(coldkey_after < coldkey_before, "coldkey should be debited");
            assert_eq!(
                coldkey_before.saturating_sub(coldkey_after),
                tipless_fee,
                "coldkey pays the tipless fee, not the tip"
            );
        });
    }

    // Full lifecycle for the proxy opt-in path: the real pays the protocol fee for its
    // delegate, and the delegate's tip is never billed to the real. The tip is a free field
    // in the signed extrinsic, so without this bound a delegate could spend the real's whole
    // balance through the fee extension no matter how narrow its proxy type is.
    #[test]
    fn full_lifecycle_real_pays_fee_excluding_tip() {
        new_test_ext().execute_with(|| {
            add_proxy(&real_a(), &signer());
            enable_real_pays_fee(&real_a(), &signer());

            let call = proxy_call(real_a(), call_remark());
            let info = DispatchInfo {
                pays_fee: Pays::Yes,
                ..call.get_dispatch_info()
            };

            let real_before = pallet_balances::Pallet::<Runtime>::free_balance(real_a());
            let signer_before = pallet_balances::Pallet::<Runtime>::free_balance(signer());

            // A tip almost as large as the real's whole balance.
            let tip = real_before.saturating_sub(TaoBalance::new(1_000_000_000));
            let ext = ChargeTransactionPaymentWrapper::<Runtime>::new(tip);
            assert_ok!(ext.test_run(
                RuntimeOrigin::signed(signer()),
                &call,
                &info,
                0,
                0,
                |_origin| Ok(Default::default()),
            ));

            let tipless_fee = pallet_transaction_payment::Pallet::<Runtime>::compute_fee(
                0,
                &info,
                TaoBalance::new(0),
            );
            let real_after = pallet_balances::Pallet::<Runtime>::free_balance(real_a());

            assert_eq!(
                real_before.saturating_sub(real_after),
                tipless_fee,
                "the real pays the tipless fee only"
            );
            assert_eq!(
                pallet_balances::Pallet::<Runtime>::free_balance(signer()),
                signer_before,
                "the delegate is not charged when the real opted in"
            );
        });
    }

    // Owner is set, yet a call outside the group is never redirected: the signer pays.
    #[test]
    fn ineligible_call_charges_signer() {
        new_test_ext().execute_with(|| {
            let hotkey = signer();
            let coldkey = other();

            pallet_subtensor::Owner::<Runtime>::insert(hotkey.clone(), coldkey);

            let call = call_remark();
            let (_valid_tx, val) =
                validate_call(RuntimeOrigin::signed(hotkey.clone()), &call).unwrap();

            assert_eq!(fee_payer(&val), hotkey);
        });
    }
}
