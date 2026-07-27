//! Signed-extension that rejects malformed `submit_encrypted` ciphertexts before they enter the pool.

use crate::{Call, Config, ShieldedTransaction};
use codec::{Decode, DecodeWithMemTracking, Encode};
use frame_support::pallet_prelude::*;
use frame_support::traits::IsSubType;
use scale_info::TypeInfo;
use sp_runtime::impl_tx_ext_default;
use sp_runtime::traits::{
    AsSystemOriginSigner, DispatchInfoOf, Dispatchable, Implication, TransactionExtension,
    ValidateResult,
};
use sp_runtime::transaction_validity::TransactionSource;
use subtensor_macros::freeze_struct;
use subtensor_runtime_common::CustomTransactionError;

// Doc lives on the module: adding `///` here would change the freeze_struct hash.
#[freeze_struct("dabd89c6963de25d")]
#[derive(Default, Encode, Decode, DecodeWithMemTracking, Clone, Eq, PartialEq, TypeInfo)]
pub struct CheckShieldedTxValidity<T: Config + Send + Sync + TypeInfo>(PhantomData<T>);

impl<T: Config + Send + Sync + TypeInfo> CheckShieldedTxValidity<T> {
    /// Construct the zero-sized extension marker.
    pub fn new() -> Self {
        Self(Default::default())
    }
}

impl<T: Config + Send + Sync + TypeInfo> sp_std::fmt::Debug for CheckShieldedTxValidity<T> {
    fn fmt(&self, f: &mut sp_std::fmt::Formatter) -> sp_std::fmt::Result {
        write!(f, "CheckShieldedTxValidity")
    }
}

impl<T: Config + Send + Sync + TypeInfo>
    TransactionExtension<<T as frame_system::Config>::RuntimeCall> for CheckShieldedTxValidity<T>
where
    <T as frame_system::Config>::RuntimeCall: Dispatchable + IsSubType<Call<T>>,
    <T as frame_system::Config>::RuntimeOrigin: AsSystemOriginSigner<T::AccountId>,
{
    const IDENTIFIER: &'static str = "CheckShieldedTxValidity";

    type Implicit = ();
    type Val = ();
    type Pre = ();

    impl_tx_ext_default!(<T as frame_system::Config>::RuntimeCall; prepare);

    fn weight(&self, _call: &<T as frame_system::Config>::RuntimeCall) -> Weight {
        // Some arbitrary weight added to account for the cost
        // of reading the PendingKey from the proposer.
        Weight::from_parts(1_000_000, 0).saturating_add(T::DbWeight::get().reads(1))
    }

    fn validate(
        &self,
        origin: <T as frame_system::Config>::RuntimeOrigin,
        call: &<T as frame_system::Config>::RuntimeCall,
        _info: &DispatchInfoOf<<T as frame_system::Config>::RuntimeCall>,
        _len: usize,
        _self_implicit: Self::Implicit,
        _inherited_implication: &impl Implication,
        _source: TransactionSource,
    ) -> ValidateResult<Self::Val, <T as frame_system::Config>::RuntimeCall> {
        // Ensure the transaction is signed, else we just skip the extension.
        let Some(_who) = origin.as_system_origin_signer() else {
            return Ok((Default::default(), (), origin));
        };

        // Ensure the transaction is a shielded transaction, else we just skip the extension.
        let Some(Call::submit_encrypted { ciphertext }) = IsSubType::<Call<T>>::is_sub_type(call)
        else {
            return Ok((Default::default(), (), origin));
        };

        // Reject malformed ciphertext regardless of source.
        let Some(ShieldedTransaction { .. }) = ShieldedTransaction::parse(ciphertext) else {
            return Err(CustomTransactionError::FailedShieldedTxParsing.into());
        };

        Ok((Default::default(), (), origin))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::*;
    use frame_support::dispatch::GetDispatchInfo;
    use frame_support::pallet_prelude::{BoundedVec, ConstU32};
    use sp_runtime::traits::TxBaseImplication;
    use sp_runtime::transaction_validity::{TransactionValidityError, ValidTransaction};

    /// Build wire-format ciphertext with a given key_hash.
    /// Layout: key_hash(16) || kem_ct_len(2 LE) || kem_ct(N) || nonce(24) || aead_ct(rest)
    fn build_ciphertext(key_hash: [u8; 16]) -> BoundedVec<u8, ConstU32<8192>> {
        let kem_ct = [0xAA; 4];
        let nonce = [0xBB; 24];
        let aead_ct = [0xDD; 16];

        let mut buf = Vec::new();
        buf.extend_from_slice(&key_hash);
        buf.extend_from_slice(&(kem_ct.len() as u16).to_le_bytes());
        buf.extend_from_slice(&kem_ct);
        buf.extend_from_slice(&nonce);
        buf.extend_from_slice(&aead_ct);

        BoundedVec::truncate_from(buf)
    }

    fn make_submit_call(key_hash: [u8; 16]) -> RuntimeCall {
        RuntimeCall::MevShield(crate::Call::submit_encrypted {
            ciphertext: build_ciphertext(key_hash),
        })
    }

    fn validate_ext(
        who: Option<u64>,
        call: &RuntimeCall,
        source: TransactionSource,
    ) -> Result<ValidTransaction, TransactionValidityError> {
        let ext = CheckShieldedTxValidity::<Test>::new();
        let info = call.get_dispatch_info();
        let origin = match who {
            Some(id) => RuntimeOrigin::signed(id),
            None => RuntimeOrigin::none(),
        };
        ext.validate(origin, call, &info, 0, (), &TxBaseImplication(call), source)
            .map(|(validity, _, _)| validity)
    }

    #[test]
    fn non_shield_call_passes_through() {
        new_test_ext().execute_with(|| {
            let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![] });
            let validity = validate_ext(Some(1), &call, TransactionSource::InBlock).unwrap();
            assert_eq!(validity.longevity, u64::MAX);
        });
    }

    #[test]
    fn unsigned_origin_passes_through() {
        new_test_ext().execute_with(|| {
            let call = make_submit_call([0xFF; 16]);
            let validity = validate_ext(None, &call, TransactionSource::InBlock).unwrap();
            assert_eq!(validity.longevity, u64::MAX);
        });
    }

    #[test]
    fn malformed_ciphertext_rejected_inblock() {
        new_test_ext().execute_with(|| {
            let call = RuntimeCall::MevShield(crate::Call::submit_encrypted {
                ciphertext: BoundedVec::truncate_from(vec![0u8; 5]),
            });
            assert_eq!(
                validate_ext(Some(1), &call, TransactionSource::InBlock),
                Err(CustomTransactionError::FailedShieldedTxParsing.into())
            );
        });
    }

    #[test]
    fn malformed_ciphertext_rejected_from_pool() {
        new_test_ext().execute_with(|| {
            let call = RuntimeCall::MevShield(crate::Call::submit_encrypted {
                ciphertext: BoundedVec::truncate_from(vec![0u8; 5]),
            });
            assert_eq!(
                validate_ext(Some(1), &call, TransactionSource::External),
                Err(CustomTransactionError::FailedShieldedTxParsing.into())
            );
        });
    }

    #[test]
    fn wellformed_ciphertext_accepted_inblock() {
        new_test_ext().execute_with(|| {
            let call = make_submit_call([0xFF; 16]);
            let validity = validate_ext(Some(1), &call, TransactionSource::InBlock).unwrap();
            assert_eq!(validity, ValidTransaction::default());
        });
    }

    #[test]
    fn wellformed_ciphertext_accepted_external() {
        new_test_ext().execute_with(|| {
            let call = make_submit_call([0xFF; 16]);
            let validity = validate_ext(Some(1), &call, TransactionSource::External).unwrap();
            assert_eq!(validity, ValidTransaction::default());
        });
    }

    #[test]
    fn wellformed_ciphertext_accepted_local() {
        new_test_ext().execute_with(|| {
            let call = make_submit_call([0xFF; 16]);
            let validity = validate_ext(Some(1), &call, TransactionSource::Local).unwrap();
            assert_eq!(validity, ValidTransaction::default());
        });
    }
}
