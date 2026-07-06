use crate::{Call, Config, ShieldedTransaction};
use codec::{Decode, DecodeWithMemTracking, Encode};
use frame_support::dispatch::{DispatchClass, GetDispatchInfo};
use frame_support::pallet_prelude::*;
use frame_support::traits::IsSubType;
use scale_info::TypeInfo;
use sp_runtime::traits::{
    AsSystemOriginSigner, DispatchInfoOf, Dispatchable, Implication, SaturatedConversion,
    TransactionExtension, ValidateResult,
};
use sp_runtime::transaction_validity::{
    InvalidTransaction, TransactionSource, TransactionValidityError,
};
use stp_mev_shield_ibe::IbeEncryptedExtrinsicV1;
use subtensor_macros::freeze_struct;
use subtensor_runtime_common::CustomTransactionError;

#[freeze_struct("dabd89c6963de25d")]
#[derive(Default, Encode, Decode, DecodeWithMemTracking, Clone, Eq, PartialEq, TypeInfo)]
pub struct CheckShieldedTxValidity<T: Config + Send + Sync + TypeInfo>(PhantomData<T>);

impl<T: Config + Send + Sync + TypeInfo> CheckShieldedTxValidity<T> {
    pub fn new() -> Self {
        Self(Default::default())
    }

    fn shield_validation_error() -> TransactionValidityError {
        CustomTransactionError::FailedShieldedTxParsing.into()
    }

    fn validate_v2_regular_envelope(ciphertext: &[u8]) -> Result<(), TransactionValidityError> {
        let envelope = IbeEncryptedExtrinsicV1::decode_v2(ciphertext)
            .map_err(|_| Self::shield_validation_error())?;
        crate::Pallet::<T>::validate_v2_envelope_for_submission(&envelope)
            .map_err(|_| Self::shield_validation_error())
    }

    fn validate_v2_conditional_envelope(
        ciphertext: &[u8],
        condition: &crate::ConditionalIbeCondition,
        lifetime_blocks: u32,
    ) -> Result<(), TransactionValidityError> {
        let envelope = IbeEncryptedExtrinsicV1::decode_v2(ciphertext)
            .map_err(|_| Self::shield_validation_error())?;
        let now = frame_system::Pallet::<T>::block_number().saturated_into::<u64>();
        crate::Pallet::<T>::validate_conditional_ibe_envelope(
            &envelope,
            condition,
            now,
            lifetime_blocks,
        )
        .map_err(|_| Self::shield_validation_error())
    }

    fn ensure_allowed(
        origin: &<<T as Config>::RuntimeCall as Dispatchable>::RuntimeOrigin,
        call: &<T as Config>::RuntimeCall,
    ) -> Result<(), TransactionValidityError>
    where
        <T as Config>::RuntimeCall: Dispatchable + GetDispatchInfo + IsSubType<Call<T>>,
        <<T as Config>::RuntimeCall as Dispatchable>::RuntimeOrigin:
            AsSystemOriginSigner<<T as frame_system::Config>::AccountId>,
    {
        // Encrypted-admission calls are not plaintext preemption. Keep the
        // existing ciphertext sanity checks for them, then allow them through.
        if let Some(Call::submit_encrypted { ciphertext }) = IsSubType::<Call<T>>::is_sub_type(call)
        {
            if IbeEncryptedExtrinsicV1::is_v2_prefixed(ciphertext.as_slice()) {
                Self::validate_v2_regular_envelope(ciphertext.as_slice())?;
            } else {
                let Some(ShieldedTransaction { .. }) = ShieldedTransaction::parse(ciphertext)
                else {
                    return Err(CustomTransactionError::FailedShieldedTxParsing.into());
                };
            }
            return Ok(());
        }

        if let Some(Call::store_encrypted { encrypted_call }) =
            IsSubType::<Call<T>>::is_sub_type(call)
        {
            if IbeEncryptedExtrinsicV1::is_v2_prefixed(encrypted_call.as_slice()) {
                Self::validate_v2_regular_envelope(encrypted_call.as_slice())?;
            }
            return Ok(());
        }

        if let Some(Call::submit_conditional_encrypted {
            ciphertext,
            condition,
            lifetime_blocks,
        }) = IsSubType::<Call<T>>::is_sub_type(call)
        {
            Self::validate_v2_conditional_envelope(
                ciphertext.as_slice(),
                condition,
                *lifetime_blocks,
            )?;
            return Ok(());
        }

        // Non-Shield unsigned/inherent calls are outside this signed-user
        // priority extension. Shield admission calls above are still validated
        // without relying on the origin signer because transaction-pool
        // validation may not expose the signed origin yet.
        if origin.as_system_origin_signer().is_none() {
            return Ok(());
        }

        // Runtime-level no-preemption invariant. If on_initialize left a due
        // threshold-IBE queue head behind, ordinary non-operational plaintext
        // transactions are invalid for this block. The drain-in-progress guard
        // lets decrypted encrypted inner extrinsics apply in FIFO order even
        // while additional due entries remain behind them.
        let dispatch_class = call.get_dispatch_info().class;
        if !crate::Pallet::<T>::is_ibe_queue_drain_in_progress()
            && dispatch_class != DispatchClass::Operational
            && (crate::Pallet::<T>::has_due_ibe_queue_head()
                || crate::Pallet::<T>::has_fired_conditional_ibe())
        {
            return Err(InvalidTransaction::ExhaustsResources.into());
        }

        Ok(())
    }
}

impl<T: Config + Send + Sync + TypeInfo> sp_std::fmt::Debug for CheckShieldedTxValidity<T> {
    fn fmt(&self, f: &mut sp_std::fmt::Formatter) -> sp_std::fmt::Result {
        write!(f, "CheckShieldedTxValidity")
    }
}

impl<T> TransactionExtension<<T as Config>::RuntimeCall> for CheckShieldedTxValidity<T>
where
    T: Config + Send + Sync + TypeInfo,
    <T as Config>::RuntimeCall: Dispatchable + GetDispatchInfo + IsSubType<Call<T>>,
    <<T as Config>::RuntimeCall as Dispatchable>::RuntimeOrigin:
        AsSystemOriginSigner<<T as frame_system::Config>::AccountId>,
{
    const IDENTIFIER: &'static str = "CheckShieldedTxValidity";

    type Implicit = ();
    type Val = ();
    type Pre = ();

    fn weight(&self, _call: &<T as Config>::RuntimeCall) -> Weight {
        // Some arbitrary weight added to account for the cost
        // of reading the PendingKey from the proposer.
        Weight::from_parts(1_000_000, 0).saturating_add(T::DbWeight::get().reads(5))
    }

    fn prepare(
        self,
        _val: Self::Val,
        origin: &<<T as Config>::RuntimeCall as Dispatchable>::RuntimeOrigin,
        call: &<T as Config>::RuntimeCall,
        _info: &DispatchInfoOf<<T as Config>::RuntimeCall>,
        _len: usize,
    ) -> Result<Self::Pre, TransactionValidityError> {
        Self::ensure_allowed(origin, call)?;
        Ok(())
    }

    fn validate(
        &self,
        origin: <<T as Config>::RuntimeCall as Dispatchable>::RuntimeOrigin,
        call: &<T as Config>::RuntimeCall,
        _info: &DispatchInfoOf<<T as Config>::RuntimeCall>,
        _len: usize,
        _self_implicit: Self::Implicit,
        _inherited_implication: &impl Implication,
        _source: TransactionSource,
    ) -> ValidateResult<(), <T as Config>::RuntimeCall> {
        Self::ensure_allowed(&origin, call)?;
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

    fn install_v2_epoch_key(epoch: u64, key_id: [u8; stp_mev_shield_ibe::KEY_ID_LEN]) {
        crate::IbeEpochKeys::<Test>::insert(
            epoch,
            stp_mev_shield_ibe::IbeEpochPublicKey {
                epoch,
                key_id,
                master_public_key: BoundedVec::truncate_from(vec![
                    0x42;
                    stp_mev_shield_ibe::COMPRESSED_MASTER_PUBLIC_KEY_LEN
                ]),
                total_weight: 1,
                threshold_weight: 1,
                public_atoms: BoundedVec::truncate_from(Vec::new()),
                first_block: 0,
                last_block: 1_000,
            },
        );
        crate::LatestPublishedIbeEpoch::<Test>::put(epoch);
    }

    fn v2_envelope(
        epoch: u64,
        target_block: u64,
        key_id: [u8; stp_mev_shield_ibe::KEY_ID_LEN],
        commitment_byte: u8,
    ) -> BoundedVec<u8, ConstU32<8192>> {
        BoundedVec::truncate_from(
            stp_mev_shield_ibe::IbeEncryptedExtrinsicV1 {
                magic: stp_mev_shield_ibe::MEV_SHIELD_IBE_MAGIC,
                version: stp_mev_shield_ibe::MEV_SHIELD_IBE_VERSION,
                epoch,
                target_block,
                key_id,
                commitment: sp_core::H256::repeat_byte(commitment_byte),
                ciphertext: vec![commitment_byte; 32],
            }
            .encode(),
        )
    }

    fn make_v2_submit_call(target_block: u64, commitment_byte: u8) -> RuntimeCall {
        let key_id = [0x42; stp_mev_shield_ibe::KEY_ID_LEN];
        RuntimeCall::MevShield(crate::Call::submit_encrypted {
            ciphertext: v2_envelope(0, target_block, key_id, commitment_byte),
        })
    }

    fn make_v2_conditional_call(
        envelope_target: u64,
        condition_target: u64,
        lifetime_blocks: u32,
        commitment_byte: u8,
    ) -> RuntimeCall {
        let key_id = [0x42; stp_mev_shield_ibe::KEY_ID_LEN];
        RuntimeCall::MevShield(crate::Call::submit_conditional_encrypted {
            ciphertext: v2_envelope(0, envelope_target, key_id, commitment_byte),
            condition: crate::ConditionalIbeCondition::AtBlock {
                block: condition_target,
            },
            lifetime_blocks,
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

    fn prepare_ext(who: Option<u64>, call: &RuntimeCall) -> Result<(), TransactionValidityError> {
        let ext = CheckShieldedTxValidity::<Test>::new();
        let info = call.get_dispatch_info();
        let origin = match who {
            Some(id) => RuntimeOrigin::signed(id),
            None => RuntimeOrigin::none(),
        };
        ext.prepare((), &origin, call, &info, 0)
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

    #[test]
    fn v2_submit_target_window_is_rejected_from_pool() {
        new_test_ext().execute_with(|| {
            System::set_block_number(10);
            install_v2_epoch_key(0, [0x42; stp_mev_shield_ibe::KEY_ID_LEN]);
            let call = make_v2_submit_call(15, 0xA1);
            assert_eq!(
                validate_ext(Some(1), &call, TransactionSource::External),
                Err(CustomTransactionError::FailedShieldedTxParsing.into())
            );
            assert_eq!(
                prepare_ext(Some(1), &call),
                Err(CustomTransactionError::FailedShieldedTxParsing.into())
            );
        });
    }

    #[test]
    fn v2_submit_target_window_is_rejected_even_without_origin_signer() {
        new_test_ext().execute_with(|| {
            System::set_block_number(10);
            install_v2_epoch_key(0, [0x42; stp_mev_shield_ibe::KEY_ID_LEN]);
            let call = make_v2_submit_call(15, 0xA4);
            assert_eq!(
                validate_ext(None, &call, TransactionSource::External),
                Err(CustomTransactionError::FailedShieldedTxParsing.into())
            );
        });
    }

    #[test]
    fn v2_submit_valid_target_window_is_accepted_from_pool() {
        new_test_ext().execute_with(|| {
            System::set_block_number(10);
            install_v2_epoch_key(0, [0x42; stp_mev_shield_ibe::KEY_ID_LEN]);
            let call = make_v2_submit_call(12, 0xA2);
            let validity = validate_ext(Some(1), &call, TransactionSource::External).unwrap();
            assert_eq!(validity, ValidTransaction::default());
            assert!(prepare_ext(Some(1), &call).is_ok());
        });
    }

    #[test]
    fn v2_conditional_envelope_is_prevalidated_from_pool() {
        new_test_ext().execute_with(|| {
            System::set_block_number(10);
            install_v2_epoch_key(0, [0x42; stp_mev_shield_ibe::KEY_ID_LEN]);

            let valid = make_v2_conditional_call(12, 12, 4, 0xB1);
            assert!(validate_ext(Some(1), &valid, TransactionSource::External).is_ok());
            assert!(prepare_ext(Some(1), &valid).is_ok());

            let mismatched_target = make_v2_conditional_call(13, 12, 4, 0xB2);
            assert_eq!(
                validate_ext(Some(1), &mismatched_target, TransactionSource::External),
                Err(CustomTransactionError::FailedShieldedTxParsing.into())
            );

            let expired_before_fire = make_v2_conditional_call(12, 12, 1, 0xB3);
            assert_eq!(
                validate_ext(Some(1), &expired_before_fire, TransactionSource::External),
                Err(CustomTransactionError::FailedShieldedTxParsing.into())
            );
        });
    }

    fn seed_ibe_queue_head(current_block: u64, target_block: u64) {
        System::set_block_number(current_block);
        crate::PendingExtrinsics::<Test>::insert(
            0,
            crate::PendingExtrinsic::<Test> {
                who: 1,
                encrypted_call: BoundedVec::truncate_from(vec![0xA5; 32]),
                submitted_at: current_block,
            },
        );
        crate::PendingIbeMetadata::<Test>::insert(
            0,
            crate::PendingIbeMeta::<Test> {
                epoch: 0,
                target_block,
                key_id: [0u8; stp_mev_shield_ibe::KEY_ID_LEN],
                commitment: sp_core::H256::repeat_byte(0x11),
                submitted_at: current_block,
                submitted_tx_index: 0,
                submitter: 1,
            },
        );
        crate::NextPendingExtrinsicIndex::<Test>::put(1);
    }

    fn seed_fired_conditional_ibe(current_block: u64, fire_block: u64) {
        System::set_block_number(current_block);
        crate::PendingConditionalIbeQueue::<Test>::insert(
            0,
            crate::PendingConditionalIbe::<Test> {
                who: 1,
                encrypted_call: BoundedVec::truncate_from(vec![0xA5; 32]),
                condition: crate::ConditionalIbeCondition::AtBlock { block: fire_block },
                submitted_at: current_block,
                expires_at: current_block.saturating_add(10),
                epoch: 0,
                target_block: fire_block,
                key_id: [0u8; stp_mev_shield_ibe::KEY_ID_LEN],
                commitment: sp_core::H256::repeat_byte(0x22),
            },
        );
    }

    #[test]
    fn due_ibe_head_blocks_plaintext_non_operational() {
        new_test_ext().execute_with(|| {
            seed_ibe_queue_head(10, 10);
            let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![] });
            assert_eq!(
                validate_ext(Some(1), &call, TransactionSource::InBlock),
                Err(InvalidTransaction::ExhaustsResources.into())
            );
        });
    }

    #[test]
    fn due_ibe_head_blocks_plaintext_prepare() {
        new_test_ext().execute_with(|| {
            seed_ibe_queue_head(10, 10);
            let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![] });
            assert_eq!(
                prepare_ext(Some(1), &call),
                Err(InvalidTransaction::ExhaustsResources.into())
            );
        });
    }

    #[test]
    fn future_ibe_head_does_not_block_plaintext() {
        new_test_ext().execute_with(|| {
            seed_ibe_queue_head(10, 11);
            let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![] });
            assert!(validate_ext(Some(1), &call, TransactionSource::InBlock).is_ok());
            assert!(prepare_ext(Some(1), &call).is_ok());
        });
    }

    #[test]
    fn due_ibe_head_allows_signed_operational_validate_and_prepare() {
        new_test_ext().execute_with(|| {
            seed_ibe_queue_head(10, 10);
            let call = RuntimeCall::System(frame_system::Call::set_code { code: vec![] });
            assert_eq!(call.get_dispatch_info().class, DispatchClass::Operational);
            assert!(validate_ext(Some(1), &call, TransactionSource::InBlock).is_ok());
            assert!(prepare_ext(Some(1), &call).is_ok());
        });
    }

    #[test]
    fn fired_conditional_allows_signed_operational_validate_and_prepare() {
        new_test_ext().execute_with(|| {
            seed_fired_conditional_ibe(10, 10);
            let call = RuntimeCall::System(frame_system::Call::set_code { code: vec![] });
            assert_eq!(call.get_dispatch_info().class, DispatchClass::Operational);
            assert!(validate_ext(Some(1), &call, TransactionSource::InBlock).is_ok());
            assert!(prepare_ext(Some(1), &call).is_ok());
        });
    }

    #[test]
    fn fired_conditional_ibe_blocks_plaintext_non_operational() {
        new_test_ext().execute_with(|| {
            seed_fired_conditional_ibe(10, 10);
            let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![] });
            assert_eq!(
                validate_ext(Some(1), &call, TransactionSource::InBlock),
                Err(InvalidTransaction::ExhaustsResources.into())
            );
        });
    }

    #[test]
    fn fired_conditional_ibe_blocks_plaintext_prepare() {
        new_test_ext().execute_with(|| {
            seed_fired_conditional_ibe(10, 10);
            let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![] });
            assert_eq!(
                prepare_ext(Some(1), &call),
                Err(InvalidTransaction::ExhaustsResources.into())
            );
        });
    }

    #[test]
    fn due_ibe_head_allows_encrypted_admission_and_drain_context() {
        new_test_ext().execute_with(|| {
            seed_ibe_queue_head(10, 10);
            let shield_call = make_submit_call([0xFF; 16]);
            assert!(validate_ext(Some(1), &shield_call, TransactionSource::InBlock).is_ok());

            crate::IbeQueueDrainInProgress::<Test>::put(true);
            let plain_call = RuntimeCall::System(frame_system::Call::remark { remark: vec![] });
            assert!(validate_ext(Some(1), &plain_call, TransactionSource::InBlock).is_ok());
        });
    }
}
