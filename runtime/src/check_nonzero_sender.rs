//! Drop-in replacement for [`frame_system::CheckNonZeroSender`].
//!
//! Same `IDENTIFIER`, same empty SCALE payload. Adds a reject for small-order
//! Ed25519 public-key encodings that ZIP-215 would otherwise accept.

use codec::{Decode, DecodeWithMemTracking, Encode};
use core::marker::PhantomData;
use frame_system::CheckNonZeroSender as CheckNonZeroSenderSubstrate;
use scale_info::TypeInfo;
use sp_runtime::{
    traits::{
        AsSystemOriginSigner, DispatchInfoOf, Implication, TransactionExtension, ValidateResult,
    },
    transaction_validity::{InvalidTransaction, TransactionSource, TransactionValidityError},
};
use subtensor_macros::freeze_struct;

use crate::small_order::is_small_order_ed25519_encoding;

/// Same wire type as `frame_system::CheckNonZeroSender`, with a stricter signer check.
#[freeze_struct("ab7a937649efdd2a")]
#[derive(Encode, Decode, DecodeWithMemTracking, Clone, Eq, PartialEq, TypeInfo)]
#[scale_info(skip_type_params(T))]
pub struct CheckNonZeroSender<T>(PhantomData<T>);

impl<T> CheckNonZeroSender<T> {
    /// Create the extension. Used by clients and tests that build `TxExtension`.
    pub fn new() -> Self {
        Self(PhantomData)
    }
}

impl<T> Default for CheckNonZeroSender<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> core::fmt::Debug for CheckNonZeroSender<T> {
    #[cfg(feature = "std")]
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "CheckNonZeroSender")
    }

    #[cfg(not(feature = "std"))]
    fn fmt(&self, _: &mut core::fmt::Formatter) -> core::fmt::Result {
        Ok(())
    }
}

impl<T> TransactionExtension<<T as frame_system::Config>::RuntimeCall> for CheckNonZeroSender<T>
where
    T: frame_system::Config + Send + Sync,
    <T as frame_system::Config>::RuntimeOrigin: AsSystemOriginSigner<T::AccountId> + Clone,
{
    const IDENTIFIER: &'static str = "CheckNonZeroSender";

    type Implicit = <CheckNonZeroSenderSubstrate<T> as TransactionExtension<
        <T as frame_system::Config>::RuntimeCall,
    >>::Implicit;
    type Val = <CheckNonZeroSenderSubstrate<T> as TransactionExtension<
        <T as frame_system::Config>::RuntimeCall,
    >>::Val;
    type Pre = <CheckNonZeroSenderSubstrate<T> as TransactionExtension<
        <T as frame_system::Config>::RuntimeCall,
    >>::Pre;

    fn implicit(&self) -> Result<Self::Implicit, TransactionValidityError> {
        CheckNonZeroSenderSubstrate::<T>::new().implicit()
    }

    fn weight(&self, call: &<T as frame_system::Config>::RuntimeCall) -> sp_weights::Weight {
        CheckNonZeroSenderSubstrate::<T>::new().weight(call)
    }

    fn validate(
        &self,
        origin: <T as frame_system::Config>::RuntimeOrigin,
        call: &<T as frame_system::Config>::RuntimeCall,
        info: &DispatchInfoOf<<T as frame_system::Config>::RuntimeCall>,
        len: usize,
        self_implicit: Self::Implicit,
        inherited_implication: &impl Implication,
        source: TransactionSource,
    ) -> ValidateResult<Self::Val, <T as frame_system::Config>::RuntimeCall> {
        if let Some(who) = origin.as_system_origin_signer()
            && who.using_encoded(is_small_order_ed25519_encoding)
        {
            return Err(InvalidTransaction::BadSigner.into());
        }

        CheckNonZeroSenderSubstrate::<T>::new().validate(
            origin,
            call,
            info,
            len,
            self_implicit,
            inherited_implication,
            source,
        )
    }

    fn prepare(
        self,
        val: Self::Val,
        origin: &<T as frame_system::Config>::RuntimeOrigin,
        call: &<T as frame_system::Config>::RuntimeCall,
        info: &DispatchInfoOf<<T as frame_system::Config>::RuntimeCall>,
        len: usize,
    ) -> Result<Self::Pre, TransactionValidityError> {
        CheckNonZeroSenderSubstrate::<T>::new().prepare(val, origin, call, info, len)
    }
}

#[allow(clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Runtime, RuntimeCall, RuntimeGenesisConfig, System};
    use frame_support::dispatch::DispatchInfo;
    use frame_system::RawOrigin;
    use sp_runtime::{
        BuildStorage,
        traits::TxBaseImplication,
        transaction_validity::{InvalidTransaction, TransactionSource, TransactionValidityError},
    };
    use subtensor_runtime_common::AccountId;

    fn new_test_ext() -> sp_io::TestExternalities {
        let mut ext: sp_io::TestExternalities = RuntimeGenesisConfig {
            sudo: pallet_sudo::GenesisConfig { key: None },
            ..Default::default()
        }
        .build_storage()
        .unwrap()
        .into();
        ext.execute_with(|| System::set_block_number(1));
        ext
    }

    fn remark_call() -> RuntimeCall {
        RuntimeCall::System(frame_system::Call::remark { remark: vec![] })
    }

    fn validate(who: AccountId) -> Result<(), TransactionValidityError> {
        let call = remark_call();
        let info = DispatchInfo::default();
        CheckNonZeroSender::<Runtime>::new()
            .validate(
                RawOrigin::Signed(who).into(),
                &call,
                &info,
                0,
                (),
                &TxBaseImplication(call.clone()),
                TransactionSource::External,
            )
            .map(|_| ())
    }

    #[test]
    fn ordinary_signer_is_accepted() {
        new_test_ext().execute_with(|| {
            assert!(validate(AccountId::from([1u8; 32])).is_ok());
        });
    }

    #[test]
    fn all_zero_signer_is_rejected() {
        new_test_ext().execute_with(|| {
            assert_eq!(
                validate(AccountId::from([0u8; 32])),
                Err(InvalidTransaction::BadSigner.into())
            );
        });
    }

    #[test]
    fn small_order_sign_bit_identity_is_rejected() {
        new_test_ext().execute_with(|| {
            let mut encoding = [0u8; 32];
            encoding[31] = 0x80;
            assert_eq!(
                validate(AccountId::from(encoding)),
                Err(InvalidTransaction::BadSigner.into())
            );
        });
    }
}
