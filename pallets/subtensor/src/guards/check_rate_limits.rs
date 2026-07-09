use super::{CallOf, DispatchableOriginOf, applicable_call};
use crate::weights::WeightInfo;
use crate::{Call, Config, Error, Pallet, TransactionType};
use frame_support::{
    dispatch::{DispatchErrorWithPostInfo, DispatchExtension, DispatchInfo, PostDispatchInfo},
    pallet_prelude::*,
    traits::{IsSubType, OriginTrait},
};
use sp_runtime::traits::Dispatchable;
use sp_std::marker::PhantomData;
use subtensor_runtime_common::{NetUid, NetUidStorageIndex};

/// Dispatch extension for rate-limit checks that are safe to reject before dispatch.
///
/// Signed weight and network-registration calls are checked before dispatch;
/// unrelated calls and non-signed origins pass through.
pub struct CheckRateLimits<T: Config>(PhantomData<T>);

impl<T: Config> CheckRateLimits<T> {
    pub(crate) fn applies_to(call: &Call<T>) -> bool {
        matches!(
            call,
            Call::commit_weights { .. }
                | Call::commit_mechanism_weights { .. }
                | Call::set_weights { .. }
                | Call::set_mechanism_weights { .. }
                | Call::register_network { .. }
        )
    }

    fn check_weights_rate_limit(
        who: &T::AccountId,
        netuid: NetUid,
        netuid_index: NetUidStorageIndex,
        error: Error<T>,
    ) -> Result<(), Error<T>> {
        let Ok(neuron_uid) = Pallet::<T>::get_uid_for_net_and_hotkey(netuid, who) else {
            return Ok(());
        };

        let current_block = Pallet::<T>::get_current_block_as_u64();
        if Pallet::<T>::check_rate_limit(netuid_index, neuron_uid, current_block) {
            Ok(())
        } else {
            Err(error)
        }
    }

    pub fn check(who: &T::AccountId, call: &Call<T>) -> Result<(), Error<T>> {
        match call {
            Call::commit_weights { netuid, .. } => Self::check_weights_rate_limit(
                who,
                *netuid,
                NetUidStorageIndex::from(*netuid),
                Error::<T>::CommittingWeightsTooFast,
            ),
            Call::commit_mechanism_weights { netuid, mecid, .. } => Self::check_weights_rate_limit(
                who,
                *netuid,
                Pallet::<T>::get_mechanism_storage_index(*netuid, *mecid),
                Error::<T>::CommittingWeightsTooFast,
            ),
            Call::set_weights { netuid, .. }
                if !Pallet::<T>::get_commit_reveal_weights_enabled(*netuid) =>
            {
                Self::check_weights_rate_limit(
                    who,
                    *netuid,
                    NetUidStorageIndex::from(*netuid),
                    Error::<T>::SettingWeightsTooFast,
                )
            }
            Call::set_mechanism_weights { netuid, mecid, .. }
                if !Pallet::<T>::get_commit_reveal_weights_enabled(*netuid) =>
            {
                Self::check_weights_rate_limit(
                    who,
                    *netuid,
                    Pallet::<T>::get_mechanism_storage_index(*netuid, *mecid),
                    Error::<T>::SettingWeightsTooFast,
                )
            }
            Call::register_network { .. }
                if !TransactionType::RegisterNetwork.passes_rate_limit::<T>(who) =>
            {
                Err(Error::<T>::NetworkTxRateLimitExceeded)
            }
            _ => Ok(()),
        }
    }
}

impl<T> DispatchExtension<CallOf<T>> for CheckRateLimits<T>
where
    T: Config,
    CallOf<T>: Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo> + IsSubType<Call<T>>,
    DispatchableOriginOf<T>: OriginTrait<AccountId = T::AccountId>,
{
    type Pre = ();

    fn weight(call: &CallOf<T>) -> Weight {
        applicable_call(call, Self::applies_to)
            .map(|_| <T as Config>::WeightInfo::check_rate_limits_extension())
            .unwrap_or(Weight::zero())
    }

    fn pre_dispatch(
        origin: &DispatchableOriginOf<T>,
        call: &CallOf<T>,
    ) -> Result<Self::Pre, DispatchErrorWithPostInfo> {
        let Some(who) = origin.as_signer() else {
            return Ok(());
        };

        let Some(call) = applicable_call(call, Self::applies_to) else {
            return Ok(());
        };

        Self::check(who, call).map_err(Into::into)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::CheckRateLimits;
    use crate::{Error, tests::mock::*, weights::WeightInfo as _};
    use frame_support::{
        assert_ok,
        dispatch::{DispatchExtension, DispatchResultWithPostInfo},
        traits::ExtendedDispatchable,
        weights::Weight,
    };
    use frame_system::Call as SystemCall;
    use sp_core::U256;
    use sp_runtime::DispatchError;
    use subtensor_runtime_common::{MechId, NetUid, TaoBalance};

    fn dispatch_with_ext(call: RuntimeCall, origin: RuntimeOrigin) -> DispatchResultWithPostInfo {
        <CheckRateLimits<Test> as ExtendedDispatchable<RuntimeCall>>::dispatch_with_extension(
            origin, call,
        )
    }

    fn err(result: DispatchResultWithPostInfo) -> DispatchError {
        result.err().unwrap().error
    }

    fn register_neuron(netuid: NetUid, hotkey: U256, coldkey: U256) {
        add_network(netuid, 1, 0);
        setup_reserves(netuid, DEFAULT_RESERVE.into(), DEFAULT_RESERVE.into());
        register_ok_neuron(netuid, hotkey, coldkey, 0);
    }

    fn set_weights_call(netuid: NetUid, uid: u16) -> RuntimeCall {
        RuntimeCall::SubtensorModule(SubtensorCall::set_weights {
            netuid,
            dests: vec![uid],
            weights: vec![1],
            version_key: 0,
        })
    }

    fn register_network_call(hotkey: U256) -> RuntimeCall {
        RuntimeCall::SubtensorModule(SubtensorCall::register_network { hotkey })
    }

    fn fund(coldkey: U256, amount: TaoBalance) {
        add_balance_to_coldkey_account(&coldkey, amount);
    }

    fn add_stake_call() -> RuntimeCall {
        RuntimeCall::SubtensorModule(SubtensorCall::add_stake {
            hotkey: U256::from(1),
            netuid: 1u16.into(),
            amount_staked: 1_000u64.into(),
        })
    }

    #[test]
    fn weight_only_charges_rate_limited_calls() {
        let netuid = NetUid::from(1);
        let expected = <Test as crate::Config>::WeightInfo::check_rate_limits_extension();
        let charged_calls = [
            RuntimeCall::SubtensorModule(SubtensorCall::commit_weights {
                netuid,
                commit_hash: sp_core::H256::zero(),
            }),
            RuntimeCall::SubtensorModule(SubtensorCall::commit_mechanism_weights {
                netuid,
                mecid: MechId::MAIN,
                commit_hash: sp_core::H256::zero(),
            }),
            set_weights_call(netuid, 0),
            RuntimeCall::SubtensorModule(SubtensorCall::set_mechanism_weights {
                netuid,
                mecid: MechId::MAIN,
                dests: vec![0],
                weights: vec![1],
                version_key: 0,
            }),
            register_network_call(U256::from(1)),
        ];

        for call in [
            RuntimeCall::System(SystemCall::remark { remark: vec![] }),
            add_stake_call(),
        ] {
            assert_eq!(
                <CheckRateLimits<Test> as DispatchExtension<RuntimeCall>>::weight(&call),
                Weight::zero()
            );
        }

        for call in charged_calls {
            assert_eq!(
                <CheckRateLimits<Test> as DispatchExtension<RuntimeCall>>::weight(&call),
                expected
            );
        }
    }

    #[test]
    fn unrelated_calls_pass_through() {
        new_test_ext(0).execute_with(|| {
            let call = RuntimeCall::System(SystemCall::remark { remark: vec![] });

            assert_ok!(dispatch_with_ext(
                call,
                RuntimeOrigin::signed(U256::from(1))
            ));
        });
    }

    #[test]
    fn over_rate_set_weights_is_blocked() {
        new_test_ext(0).execute_with(|| {
            let netuid = NetUid::from(1);
            let hotkey = U256::from(1);
            let coldkey = U256::from(2);
            register_neuron(netuid, hotkey, coldkey);
            SubtensorModule::set_commit_reveal_weights_enabled(netuid, false);
            SubtensorModule::set_weights_set_rate_limit(netuid, 100);
            System::set_block_number(10_u64);
            let uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey).unwrap();
            let netuid_index = SubtensorModule::get_mechanism_storage_index(netuid, MechId::MAIN);
            SubtensorModule::set_last_update_for_uid(
                netuid_index,
                uid,
                SubtensorModule::get_current_block_as_u64(),
            );

            assert_eq!(
                err(dispatch_with_ext(
                    set_weights_call(netuid, uid),
                    RuntimeOrigin::signed(hotkey)
                )),
                Error::<Test>::SettingWeightsTooFast.into()
            );
        });
    }

    #[test]
    fn set_weights_within_rate_limit_dispatches() {
        new_test_ext(0).execute_with(|| {
            let netuid = NetUid::from(1);
            let hotkey = U256::from(1);
            let coldkey = U256::from(2);
            register_neuron(netuid, hotkey, coldkey);
            SubtensorModule::set_commit_reveal_weights_enabled(netuid, false);
            SubtensorModule::set_weights_set_rate_limit(netuid, 100);
            SubtensorModule::set_stake_threshold(0);
            System::set_block_number(200_u64);
            let uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey).unwrap();

            assert_ok!(dispatch_with_ext(
                set_weights_call(netuid, uid),
                RuntimeOrigin::signed(hotkey)
            ));
        });
    }

    #[test]
    fn over_rate_network_registration_is_blocked() {
        new_test_ext(0).execute_with(|| {
            crate::NetworkRateLimit::<Test>::put(50_u64);
            System::set_block_number(200_u64);
            SubtensorModule::set_network_last_lock_block(170);
            let coldkey = U256::from(70);

            assert_eq!(
                err(dispatch_with_ext(
                    register_network_call(U256::from(71)),
                    RuntimeOrigin::signed(coldkey)
                )),
                Error::<Test>::NetworkTxRateLimitExceeded.into()
            );
        });
    }

    #[test]
    fn network_registration_after_rate_limit_dispatches() {
        new_test_ext(0).execute_with(|| {
            crate::NetworkRateLimit::<Test>::put(50_u64);
            System::set_block_number(200_u64);
            SubtensorModule::set_network_last_lock_block(100);
            let coldkey = U256::from(70);
            fund(coldkey, SubtensorModule::get_network_lock_cost().into());

            assert_ok!(dispatch_with_ext(
                register_network_call(U256::from(71)),
                RuntimeOrigin::signed(coldkey)
            ));
        });
    }
}
