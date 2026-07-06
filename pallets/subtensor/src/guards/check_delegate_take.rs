use super::{CallOf, DispatchableOriginOf, applicable_call};
use crate::weights::WeightInfo;
use crate::{Call, Config, Error, Pallet};
use frame_support::{
    dispatch::{DispatchErrorWithPostInfo, DispatchExtension, DispatchInfo, PostDispatchInfo},
    pallet_prelude::*,
    traits::{IsSubType, OriginTrait},
};
use sp_runtime::traits::Dispatchable;
use sp_std::marker::PhantomData;

/// Dispatch extension for delegate-take bounds and ownership preconditions.
///
/// Signed increase/decrease take calls are checked before dispatch; unrelated
/// calls and non-signed origins pass through.
pub struct CheckDelegateTake<T: Config>(PhantomData<T>);

impl<T: Config> CheckDelegateTake<T> {
    pub(crate) fn applies_to(call: &Call<T>) -> bool {
        matches!(
            call,
            Call::increase_take { .. } | Call::decrease_take { .. }
        )
    }

    pub fn check(who: &T::AccountId, call: &Call<T>) -> Result<(), Error<T>> {
        match call {
            Call::increase_take { hotkey, take } | Call::decrease_take { hotkey, take } => {
                if *take < Pallet::<T>::get_min_delegate_take() {
                    return Err(Error::<T>::DelegateTakeTooLow);
                }
                if *take > Pallet::<T>::get_max_delegate_take() {
                    return Err(Error::<T>::DelegateTakeTooHigh);
                }
                Pallet::<T>::do_take_checks(who, hotkey)
            }
            _ => Ok(()),
        }
    }
}

impl<T> DispatchExtension<CallOf<T>> for CheckDelegateTake<T>
where
    T: Config,
    CallOf<T>: Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo> + IsSubType<Call<T>>,
    DispatchableOriginOf<T>: OriginTrait<AccountId = T::AccountId>,
{
    type Pre = ();

    fn weight(call: &CallOf<T>) -> Weight {
        applicable_call(call, Self::applies_to)
            .map(|_| <T as Config>::WeightInfo::check_delegate_take_extension())
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
    use super::*;
    use crate::{Error, tests::mock::*};
    use frame_support::{
        assert_ok,
        dispatch::{DispatchExtension, DispatchResultWithPostInfo},
        traits::ExtendedDispatchable,
        weights::Weight,
    };
    use sp_core::U256;
    use sp_runtime::DispatchError;

    fn increase_take_call(hotkey: U256, take: u16) -> RuntimeCall {
        RuntimeCall::SubtensorModule(SubtensorCall::increase_take { hotkey, take })
    }

    fn decrease_take_call(hotkey: U256, take: u16) -> RuntimeCall {
        RuntimeCall::SubtensorModule(SubtensorCall::decrease_take { hotkey, take })
    }

    fn dispatch_with_ext(call: RuntimeCall, origin: RuntimeOrigin) -> DispatchResultWithPostInfo {
        <CheckDelegateTake<Test> as ExtendedDispatchable<RuntimeCall>>::dispatch_with_extension(
            origin, call,
        )
    }

    fn err(result: DispatchResultWithPostInfo) -> DispatchError {
        result.unwrap_err().error
    }

    fn add_stake_call() -> RuntimeCall {
        RuntimeCall::SubtensorModule(SubtensorCall::add_stake {
            hotkey: U256::from(1),
            netuid: 1u16.into(),
            amount_staked: 1_000u64.into(),
        })
    }

    #[test]
    fn weight_only_charges_delegate_take_calls() {
        let expected = <Test as crate::Config>::WeightInfo::check_delegate_take_extension();

        for call in [
            RuntimeCall::System(frame_system::Call::remark { remark: vec![] }),
            add_stake_call(),
        ] {
            assert_eq!(
                <CheckDelegateTake<Test> as DispatchExtension<RuntimeCall>>::weight(&call),
                Weight::zero()
            );
        }

        for call in [
            increase_take_call(U256::from(1), 0),
            decrease_take_call(U256::from(1), 0),
        ] {
            assert_eq!(
                <CheckDelegateTake<Test> as DispatchExtension<RuntimeCall>>::weight(&call),
                expected
            );
        }
    }

    #[test]
    fn accepts_owner_with_valid_take() {
        new_test_ext(0).execute_with(|| {
            let owner = U256::from(1);
            let hotkey = U256::from(2);
            crate::Owner::<Test>::insert(hotkey, owner);

            for call in [
                increase_take_call(hotkey, SubtensorModule::get_max_delegate_take()),
                decrease_take_call(hotkey, SubtensorModule::get_min_delegate_take()),
            ] {
                assert_ok!(dispatch_with_ext(call, RuntimeOrigin::signed(owner)));
            }
        });
    }

    #[test]
    fn rejects_take_too_low() {
        new_test_ext(0).execute_with(|| {
            let owner = U256::from(1);
            let hotkey = U256::from(2);
            crate::Owner::<Test>::insert(hotkey, owner);

            let take = SubtensorModule::get_min_delegate_take() - 1;

            for call in [
                increase_take_call(hotkey, take),
                decrease_take_call(hotkey, take),
            ] {
                assert_eq!(
                    err(dispatch_with_ext(call, RuntimeOrigin::signed(owner))),
                    Error::<Test>::DelegateTakeTooLow.into()
                );
            }
        });
    }

    #[test]
    fn rejects_take_too_high() {
        new_test_ext(0).execute_with(|| {
            let owner = U256::from(1);
            let hotkey = U256::from(2);
            crate::Owner::<Test>::insert(hotkey, owner);

            let take = SubtensorModule::get_max_delegate_take() + 1;

            for call in [
                increase_take_call(hotkey, take),
                decrease_take_call(hotkey, take),
            ] {
                assert_eq!(
                    err(dispatch_with_ext(call, RuntimeOrigin::signed(owner))),
                    Error::<Test>::DelegateTakeTooHigh.into()
                );
            }
        });
    }

    #[test]
    fn rejects_non_owner() {
        new_test_ext(0).execute_with(|| {
            let owner = U256::from(1);
            let other = U256::from(2);
            let hotkey = U256::from(3);
            crate::Owner::<Test>::insert(hotkey, owner);

            let take = SubtensorModule::get_max_delegate_take();

            for call in [
                increase_take_call(hotkey, take),
                decrease_take_call(hotkey, take),
            ] {
                assert_eq!(
                    err(dispatch_with_ext(call, RuntimeOrigin::signed(other))),
                    Error::<Test>::NonAssociatedColdKey.into()
                );
            }
        });
    }

    #[test]
    fn rejects_missing_hotkey_owner() {
        new_test_ext(0).execute_with(|| {
            let owner = U256::from(1);
            let hotkey = U256::from(99);
            let take = SubtensorModule::get_max_delegate_take();

            for call in [
                increase_take_call(hotkey, take),
                decrease_take_call(hotkey, take),
            ] {
                assert_eq!(
                    err(dispatch_with_ext(call, RuntimeOrigin::signed(owner))),
                    Error::<Test>::HotKeyAccountNotExists.into()
                );
            }
        });
    }
}
