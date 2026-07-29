use crate::{
    Call, CheckColdkeySwap, CheckDelegateTake, CheckEvmKeyAssociation, CheckRateLimits,
    CheckServingEndpoints, CheckWeights, Config, Error, guards::applicable_call,
    weights::WeightInfo,
};
use codec::{Decode, DecodeWithMemTracking, Encode};
use frame_support::{
    dispatch::{DispatchExtension, DispatchInfo, GetDispatchInfo, PostDispatchInfo},
    traits::{IsSubType, OriginTrait},
    weights::Weight,
};
use scale_info::TypeInfo;
use sp_runtime::traits::{
    DispatchInfoOf, Dispatchable, Implication, TransactionExtension, ValidateResult,
};
use sp_runtime::transaction_validity::{TransactionSource, TransactionValidityError};
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

    /// Return a call-independent upper bound for the validation work performed
    /// by this extension.
    ///
    /// Individual calls enable different guard combinations, and some calls
    /// enable more than one guard. Summing every guard is deliberately
    /// conservative.
    pub fn maximum_weight() -> Weight {
        <T as Config>::WeightInfo::check_coldkey_swap_extension()
            .saturating_add(<T as Config>::WeightInfo::check_weights_extension())
            .saturating_add(<T as Config>::WeightInfo::check_rate_limits_extension())
            .saturating_add(<T as Config>::WeightInfo::check_delegate_take_extension())
            .saturating_add(<T as Config>::WeightInfo::check_serving_endpoints_extension())
            .saturating_add(<T as Config>::WeightInfo::check_evm_key_association_extension())
    }

    /// Weight consumed by the validation guards themselves, excluding any
    /// top-level dispatch reserve.
    pub fn validation_weight(call: &CallOf<T>) -> Weight
    where
        T: pallet_shield::Config,
        CallOf<T>: Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>
            + IsSubType<Call<T>>
            + IsSubType<pallet_shield::Call<T>>,
    {
        use DispatchExtension as DE;
        <CheckColdkeySwap<T> as DE<CallOf<T>>>::weight(call)
            .saturating_add(<CheckWeights<T> as DE<CallOf<T>>>::weight(call))
            .saturating_add(<CheckRateLimits<T> as DE<CallOf<T>>>::weight(call))
            .saturating_add(<CheckDelegateTake<T> as DE<CallOf<T>>>::weight(call))
            .saturating_add(<CheckServingEndpoints<T> as DE<CallOf<T>>>::weight(call))
            .saturating_add(<CheckEvmKeyAssociation<T> as DE<CallOf<T>>>::weight(call))
    }

    fn reserves_maximum_dispatch_weight(call: &CallOf<T>) -> bool
    where
        CallOf<T>: IsSubType<Call<T>>,
    {
        let Some(call) = call.is_sub_type() else {
            return false;
        };
        matches!(
            call,
            Call::set_weights { .. }
                | Call::set_mechanism_weights { .. }
                | Call::batch_set_weights { .. }
                | Call::commit_weights { .. }
                | Call::commit_mechanism_weights { .. }
                | Call::batch_commit_weights { .. }
                | Call::reveal_weights { .. }
                | Call::reveal_mechanism_weights { .. }
                | Call::commit_crv3_mechanism_weights { .. }
                | Call::batch_reveal_weights { .. }
                | Call::serve_axon_tls { .. }
                | Call::swap_hotkey { .. }
                | Call::swap_hotkey_v2 { .. }
                | Call::swap_coldkey { .. }
                | Call::set_children { .. }
                | Call::set_identity { .. }
                | Call::set_subnet_identity { .. }
                | Call::register_network_with_identity { .. }
                | Call::unstake_all { .. }
                | Call::unstake_all_alpha { .. }
                | Call::commit_timelocked_weights { .. }
                | Call::set_coldkey_auto_stake_hotkey { .. }
                | Call::commit_timelocked_mechanism_weights { .. }
                | Call::claim_root { .. }
                | Call::set_root_claim_type { .. }
                | Call::swap_coldkey_announced { .. }
        )
    }

    fn maximum_dispatch_reserve(call: &CallOf<T>) -> Weight
    where
        CallOf<T>: GetDispatchInfo + IsSubType<Call<T>>,
    {
        if Self::reserves_maximum_dispatch_weight(call) {
            crate::Pallet::<T>::max_normal_dispatch_weight()
                .saturating_sub(call.get_dispatch_info().call_weight)
        } else {
            Weight::zero()
        }
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
        + GetDispatchInfo
        + IsSubType<Call<T>>
        + IsSubType<pallet_shield::Call<T>>,
    OriginOf<T>: Clone + OriginTrait<AccountId = T::AccountId>,
{
    const IDENTIFIER: &'static str = "SubtensorTransactionExtension";

    type Implicit = ();
    type Val = Weight;
    type Pre = Weight;

    fn weight(&self, call: &CallOf<T>) -> Weight {
        Self::validation_weight(call).saturating_add(Self::maximum_dispatch_reserve(call))
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
            .map(|()| {
                (
                    Default::default(),
                    Self::maximum_dispatch_reserve(call),
                    origin,
                )
            })
            .map_err(|error| TransactionValidityError::from(CustomTransactionError::from(error)))
    }

    fn prepare(
        self,
        reserve: Self::Val,
        _origin: &OriginOf<T>,
        _call: &CallOf<T>,
        _info: &DispatchInfoOf<CallOf<T>>,
        _len: usize,
    ) -> Result<Self::Pre, TransactionValidityError> {
        Ok(reserve)
    }

    fn post_dispatch_details(
        reserve: Self::Pre,
        _info: &DispatchInfoOf<CallOf<T>>,
        _post_info: &PostDispatchInfo,
        _len: usize,
        _result: &sp_runtime::DispatchResult,
    ) -> Result<Weight, TransactionValidityError> {
        // FRAME adds the complete extension precharge to the raw call-reported
        // weight before running transaction-extension post-dispatch hooks. The
        // state-dependent call overage is therefore already present in
        // `post_info.actual_weight`; refunding less than the full reserve would
        // charge that overage twice. Runtime ordering must keep transaction
        // payment after this extension and WeightReclaim last so neither
        // consumer caps the aggregate before this refund is applied.
        Ok(reserve)
    }
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
        dispatch::{DispatchExtension, GetDispatchInfo, Pays, PostDispatchInfo},
        weights::Weight,
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
                    expected_transaction_extension_weight(&call).saturating_add(
                        SubtensorTransactionExtension::<Test>::maximum_dispatch_reserve(&call)
                    )
                );
            }
        });
    }

    #[test]
    fn dynamic_call_and_extension_reserve_the_maximum_dispatch_weight() {
        new_test_ext(1).execute_with(|| {
            let call = RuntimeCall::SubtensorModule(SubtensorCall::set_weights {
                netuid: NetUid::from(1),
                dests: vec![0],
                weights: vec![1],
                version_key: 0,
            });
            let call_weight = call.get_dispatch_info().call_weight;
            let reserve = SubtensorTransactionExtension::<Test>::maximum_dispatch_reserve(&call);

            assert_eq!(
                call_weight.saturating_add(reserve),
                SubtensorModule::max_normal_dispatch_weight()
            );
            assert_eq!(
                TransactionExtension::weight(&SubtensorTransactionExtension::<Test>::new(), &call),
                expected_transaction_extension_weight(&call).saturating_add(reserve)
            );
        });
    }

    #[test]
    fn fixed_weight_call_has_no_maximum_dispatch_reserve() {
        new_test_ext(1).execute_with(|| {
            let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![] });

            assert_eq!(
                SubtensorTransactionExtension::<Test>::maximum_dispatch_reserve(&call),
                Weight::zero()
            );
        });
    }

    #[test]
    fn maximum_dispatch_reserve_refund_preserves_state_dependent_call_weight() {
        new_test_ext(1).execute_with(|| {
            let call = RuntimeCall::SubtensorModule(SubtensorCall::set_weights {
                netuid: NetUid::from(1),
                dests: vec![0],
                weights: vec![1],
                version_key: 0,
            });
            let reserve = SubtensorTransactionExtension::<Test>::maximum_dispatch_reserve(&call);
            let declared_call_weight = call.get_dispatch_info().call_weight;
            let other_extension_weight = Weight::from_parts(17, 19);
            let info = frame_support::dispatch::DispatchInfo {
                call_weight: declared_call_weight,
                extension_weight: reserve.saturating_add(other_extension_weight),
                ..call.get_dispatch_info()
            };

            // `set_extension_weight` adds the complete extension precharge to
            // the raw weight reported by the call. The reserve must therefore
            // be refunded in full: retaining the call's excess over its
            // declaration happens automatically because that raw excess is
            // still present and has not yet been capped.
            for actual_call_weight in [
                declared_call_weight / 2,
                declared_call_weight,
                declared_call_weight.saturating_add(reserve / 2),
            ] {
                let mut post_info = PostDispatchInfo {
                    actual_weight: Some(actual_call_weight.saturating_add(info.extension_weight)),
                    ..Default::default()
                };

                assert_ok!(<SubtensorTransactionExtension<Test> as TransactionExtension<
                    RuntimeCall,
                >>::post_dispatch(
                    reserve, &info, &mut post_info, 0, &Ok(())
                ));

                assert_eq!(
                    post_info.actual_weight,
                    Some(actual_call_weight.saturating_add(other_extension_weight))
                );
            }
        });
    }

    #[test]
    fn maximum_dispatch_reserve_is_refunded_after_failure() {
        new_test_ext(1).execute_with(|| {
            let call = RuntimeCall::SubtensorModule(SubtensorCall::set_weights {
                netuid: NetUid::from(1),
                dests: vec![0],
                weights: vec![1],
                version_key: 0,
            });
            let reserve = SubtensorTransactionExtension::<Test>::maximum_dispatch_reserve(&call);
            let info = call.get_dispatch_info();
            let post_info = PostDispatchInfo::default();

            let refunded = <SubtensorTransactionExtension<Test> as TransactionExtension<
                RuntimeCall,
            >>::post_dispatch_details(
                reserve,
                &info,
                &post_info,
                0,
                &Err(sp_runtime::DispatchError::Other("expected failure")),
            )
            .unwrap();

            assert_eq!(refunded, reserve);
        });
    }
}
