use crate::{
    Call, CheckColdkeySwap, CheckDelegateTake, CheckEvmKeyAssociation, CheckRateLimits,
    CheckServingEndpoints, CheckWeights, Config, Error, guards::applicable_call,
};
use codec::{Decode, DecodeWithMemTracking, Encode};
use frame_support::{
    dispatch::{DispatchExtension, DispatchInfo, PostDispatchInfo},
    traits::{IsSubType, OriginTrait},
    weights::Weight,
};
use scale_info::TypeInfo;
use sp_runtime::traits::{
    DispatchInfoOf, Dispatchable, Implication, TransactionExtension, ValidateResult,
};
use sp_runtime::{
    impl_tx_ext_default,
    transaction_validity::{TransactionSource, TransactionValidityError},
};
use sp_std::marker::PhantomData;
use subtensor_macros::freeze_struct;
use subtensor_runtime_common::CustomTransactionError;

type CallOf<T> = <T as frame_system::Config>::RuntimeCall;
type OriginOf<T> = <T as frame_system::Config>::RuntimeOrigin;

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
        T: pallet_shield::Config,
        CallOf<T>: Dispatchable<RuntimeOrigin = OriginOf<T>>
            + IsSubType<Call<T>>
            + IsSubType<pallet_shield::Call<T>>,
        OriginOf<T>: OriginTrait<AccountId = T::AccountId>,
    {
        let Some(who) = origin.as_signer() else {
            return Ok(());
        };

        CheckColdkeySwap::<T>::check(who, call)?;

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
}

impl<T> TransactionExtension<CallOf<T>> for SubtensorTransactionExtension<T>
where
    T: Config + pallet_shield::Config + Send + Sync + TypeInfo,
    CallOf<T>: Dispatchable<RuntimeOrigin = OriginOf<T>, Info = DispatchInfo, PostInfo = PostDispatchInfo>
        + IsSubType<Call<T>>
        + IsSubType<pallet_shield::Call<T>>,
    OriginOf<T>: Clone + OriginTrait<AccountId = T::AccountId>,
{
    const IDENTIFIER: &'static str = "SubtensorTransactionExtension";

    type Implicit = ();
    type Val = ();
    type Pre = ();

    fn weight(&self, call: &CallOf<T>) -> Weight {
        use DispatchExtension as DE;
        <CheckColdkeySwap<T> as DE<CallOf<T>>>::weight(call)
            .saturating_add(<CheckWeights<T> as DE<CallOf<T>>>::weight(call))
            .saturating_add(<CheckRateLimits<T> as DE<CallOf<T>>>::weight(call))
            .saturating_add(<CheckDelegateTake<T> as DE<CallOf<T>>>::weight(call))
            .saturating_add(<CheckServingEndpoints<T> as DE<CallOf<T>>>::weight(call))
            .saturating_add(<CheckEvmKeyAssociation<T> as DE<CallOf<T>>>::weight(call))
    }

    fn validate(
        &self,
        origin: OriginOf<T>,
        call: &CallOf<T>,
        _info: &DispatchInfoOf<CallOf<T>>,
        _len: usize,
        _self_implicit: Self::Implicit,
        _inherited_implication: &impl Implication,
        _source: TransactionSource,
    ) -> ValidateResult<Self::Val, CallOf<T>> {
        Self::check(&origin, call)
            .map(|()| (Default::default(), (), origin))
            .map_err(|error| TransactionValidityError::from(CustomTransactionError::from(error)))
    }

    impl_tx_ext_default!(CallOf<T>; prepare);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::SubtensorTransactionExtension;
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

            ColdkeySwapAnnouncements::<Test>::insert(
                coldkey,
                (System::block_number(), new_coldkey_hash),
            );
            let err = validate_signed(coldkey, &call).unwrap_err();
            assert_eq!(err, CustomTransactionError::ColdkeyInSwapSchedule.into());

            ColdkeySwapDisputes::<Test>::insert(coldkey, System::block_number());
            let err = validate_signed(coldkey, &call).unwrap_err();
            assert_eq!(err, CustomTransactionError::ColdkeySwapDisputed.into());
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
