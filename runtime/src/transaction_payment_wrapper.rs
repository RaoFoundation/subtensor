//! Transaction-payment extension with proxy `RealPaysFee` and coldkey fee routing.
//!
//! Wraps FRAME's [`ChargeTransactionPayment`] and, before charging:
//! 1. Prefer a proxy real that opted into `RealPaysFee` (up to two nesting levels /
//!    homogeneous proxy batches).
//! 2. Else charge an owned hotkey's coldkey for allow-listed calls
//!    ([`ColdkeyFeeCallFilter`] / `fee_filters`).
//!
//! Priority is class-based ([`NORMAL_DISPATCH_BASE_PRIORITY`] /
//! [`OPERATIONAL_DISPATCH_PRIORITY`]), not tip-based. Coldkey-paid tips are
//! zeroed so a hotkey cannot drain its owner via tip. `IDENTIFIER` and SCALE
//! layout are frozen.

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
use sp_std::boxed::Box;
use sp_std::vec::Vec;
use subtensor_macros::freeze_struct;

type BalanceOf<T> = <<T as Config>::OnChargeTransaction as OnChargeTransaction<T>>::Balance;

type RuntimeCallOf<T> = <T as frame_system::Config>::RuntimeCall;
type RuntimeOriginOf<T> = <T as frame_system::Config>::RuntimeOrigin;
type AccountIdOf<T> = <T as frame_system::Config>::AccountId;
type LookupOf<T> = <T as frame_system::Config>::Lookup;

/// Runtime-supplied policy: which calls have their fee charged to the signing
/// hotkey's coldkey. Keeping the concrete list in the runtime (see `fee_filters`)
/// lets this generic extension stay free of a hardcoded allow-list.
pub trait ColdkeyFeeCallFilter<Call> {
    fn charges_coldkey(call: &Call) -> bool;
}

/// SCALE-compatible wrapper around [`ChargeTransactionPayment`] with Subtensor
/// fee-payer resolution (proxy `RealPaysFee`, then coldkey allow-list).
#[freeze_struct("808d9c4ddeec2af0")]
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
    /// Extract (real, delegate, inner_call) from a `proxy` call.
    /// `signer` is used as the delegate since it is implicit (the caller).
    /// `proxy_announced` is intentionally not handled here; fee propagation
    /// only applies to `proxy` calls to keep the logic simple.
    fn extract_proxy_parts<'a>(
        call: &'a RuntimeCallOf<T>,
        signer: &AccountIdOf<T>,
    ) -> Option<(
        AccountIdOf<T>,
        AccountIdOf<T>,
        &'a Box<<T as pallet_proxy::Config>::RuntimeCall>,
    )> {
        match call.is_sub_type()? {
            pallet_proxy::Call::proxy { real, call, .. } => {
                let real = LookupOf::<T>::lookup(real.clone()).ok()?;
                Some((real, signer.clone(), call))
            }
            _ => None,
        }
    }

    /// Determine who should pay the transaction fee for a proxy call.
    ///
    /// Follows the RealPaysFee chain up to 2 levels deep:
    /// - Case 1: `proxy(real=A, call)` → A pays if `RealPaysFee<A, signer>`
    /// - Case 2: `proxy(real=B, proxy(real=A, call))` → A pays if both
    ///   `RealPaysFee<B, signer>` and `RealPaysFee<A, B>` are set; B pays if only the former.
    /// - Case 3: `proxy(real=B, batch([proxy(real=A, ..), ..]))` → A pays if
    ///   `RealPaysFee<B, signer>`, all batch items are proxy calls with the same real A,
    ///   and `RealPaysFee<A, B>` is set; B pays if only the first condition holds.
    ///
    /// Returns `None` if the signer should pay (no RealPaysFee opt-in).
    fn extract_real_fee_payer(
        call: &RuntimeCallOf<T>,
        origin: &RuntimeOriginOf<T>,
    ) -> Option<AccountIdOf<T>> {
        let signer = origin.as_system_origin_signer()?;
        let (outer_real, delegate, inner_call) = Self::extract_proxy_parts(call, signer)?;

        // Check if the outer real account has opted in to pay for the delegate.
        if !pallet_proxy::Pallet::<T>::is_real_pays_fee(&outer_real, &delegate) {
            return None;
        }

        // outer_real pays. Try to push the fee deeper into nested proxy structures.
        let inner_call: &RuntimeCallOf<T> = (*inner_call).as_ref().into_ref();

        // Case 2: inner call is another proxy call.
        if let Some(inner_payer) = Self::extract_inner_proxy_payer(inner_call, &outer_real) {
            return Some(inner_payer);
        }

        // Case 3: inner call is a batch of proxy calls with the same real.
        if let Some(batch_payer) = Self::extract_batch_proxy_payer(inner_call, &outer_real) {
            return Some(batch_payer);
        }

        // Case 1: simple proxy, outer_real pays.
        Some(outer_real)
    }

    /// Check if an inner call is a proxy call where the inner real has opted in to pay.
    /// `outer_real` is used as the implicit delegate for `proxy` calls.
    fn extract_inner_proxy_payer(
        inner_call: &RuntimeCallOf<T>,
        outer_real: &AccountIdOf<T>,
    ) -> Option<AccountIdOf<T>> {
        let (inner_real, inner_delegate, _call) =
            Self::extract_proxy_parts(inner_call, outer_real)?;

        if pallet_proxy::Pallet::<T>::is_real_pays_fee(&inner_real, &inner_delegate) {
            Some(inner_real)
        } else {
            None
        }
    }

    /// Check if an inner call is a batch where ALL items are proxy calls with the same real
    /// account, and that real account has opted in to pay.
    /// `outer_real` is used as the implicit delegate for `proxy` calls within the batch.
    fn extract_batch_proxy_payer(
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
            let (inner_real, inner_delegate, _) = Self::extract_proxy_parts(call_ref, outer_real)?;

            match &common_real {
                None => {
                    // Check RealPaysFee once on the first item and memoize. For `proxy`
                    // calls the delegate is always `outer_real`, so a single read covers
                    // the entire batch; for `proxy_announced` it uses the explicit delegate.
                    if !pallet_proxy::Pallet::<T>::is_real_pays_fee(&inner_real, &inner_delegate) {
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
    T: ColdkeyFeeCallFilter<RuntimeCallOf<T>>,
    BalanceOf<T>: Zero + Send + Sync,
    RuntimeOriginOf<T>: AsSystemOriginSigner<AccountIdOf<T>>
        + Clone
        + From<frame_system::RawOrigin<AccountIdOf<T>>>,
{
    const IDENTIFIER: &'static str = "ChargeTransactionPaymentWrapper";
    type Implicit = ();
    type Val = Val<T>;
    type Pre = Pre<T>;

    fn weight(&self, call: &RuntimeCallOf<T>) -> Weight {
        // Account for up to 3 storage reads in the worst-case fee payer resolution
        // (outer is_real_pays_fee + inner/batch is_real_pays_fee + margin).
        self.inner
            .weight(call)
            .saturating_add(T::DbWeight::get().reads(3))
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
        // owned hotkey's coldkey pays for allow-listed calls. In that case the coldkey covers
        // the protocol fee only — the signer-chosen tip is dropped, since a tip does not buy
        // priority in this wrapper (priority is overridden above) and billing it to the coldkey
        // would let a hotkey drain coldkey funds. Other payers keep the original tip.
        let (fee_origin, tip) = if let Some(real) = Self::extract_real_fee_payer(call, &origin) {
            (
                frame_system::RawOrigin::Signed(real).into(),
                self.inner.tip(),
            )
        } else if let Some(coldkey) = Self::extract_coldkey_fee_payer(call, &origin) {
            (
                frame_system::RawOrigin::Signed(coldkey).into(),
                Zero::zero(),
            )
        } else {
            (origin.clone(), self.inner.tip())
        };

        let (mut valid_transaction, val, _fee_origin) = ChargeTransactionPayment::<T>::from(tip)
            .validate(
                fee_origin,
                call,
                info,
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
        self.inner.prepare(val, origin, call, info, len)
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
        ChargeTransactionPayment::<T>::post_dispatch_details(pre, info, post_info, len, result)
    }

    fn post_dispatch(
        pre: Self::Pre,
        info: &DispatchInfoOf<RuntimeCallOf<T>>,
        post_info: &mut PostDispatchInfoOf<RuntimeCallOf<T>>,
        len: usize,
        result: &DispatchResult,
    ) -> Result<(), TransactionValidityError> {
        ChargeTransactionPayment::<T>::post_dispatch(pre, info, post_info, len, result)
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
    use sp_runtime::traits::{DispatchTransaction, TransactionExtension, TxBaseImplication};
    use sp_runtime::transaction_validity::{
        TransactionSource, TransactionValidityError, ValidTransaction,
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
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(real.clone()),
            delegate.clone().into(),
            ProxyType::Any,
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

            // Batch contains a non-proxy call → extract_proxy_parts fails
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
