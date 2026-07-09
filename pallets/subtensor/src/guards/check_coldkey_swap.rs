use super::{CallOf, DispatchableOriginOf};
use crate::weights::WeightInfo;
use crate::{Call, ColdkeySwapAnnouncements, ColdkeySwapDisputes, Config, Error};
use frame_support::{
    dispatch::{DispatchErrorWithPostInfo, DispatchExtension, DispatchInfo, PostDispatchInfo},
    pallet_prelude::*,
    traits::{IsSubType, OriginTrait},
};
use sp_runtime::traits::Dispatchable;
use sp_std::marker::PhantomData;

/// Dispatch extension that blocks most calls when a coldkey swap is active.
///
/// When a coldkey swap has been announced for the signing account:
/// - If the swap is disputed, ALL calls are blocked.
/// - Otherwise, only swap-related calls and MEV-protected calls (`submit_encrypted`)
///   are allowed through.
///
/// Root origin bypasses this extension entirely.
/// Non-signed origins pass through.
///
/// Because this is a `DispatchExtension` (not a `TransactionExtension`), it fires at every
/// `call.dispatch(origin)` site — including inside the proxy pallet's `do_proxy()`.
/// This means nested proxies of any depth are handled automatically with the real
/// resolved origin.
pub struct CheckColdkeySwap<T: Config>(PhantomData<T>);

impl<T> CheckColdkeySwap<T>
where
    T: Config + pallet_shield::Config,
    CallOf<T>: IsSubType<Call<T>> + IsSubType<pallet_shield::Call<T>>,
{
    pub fn check(who: &T::AccountId, call: &CallOf<T>) -> Result<(), Error<T>> {
        if !ColdkeySwapAnnouncements::<T>::contains_key(who) {
            return Ok(());
        }

        if ColdkeySwapDisputes::<T>::contains_key(who) {
            return Err(Error::<T>::ColdkeySwapDisputed);
        }

        if Self::is_allowed_during_swap(call) {
            Ok(())
        } else {
            Err(Error::<T>::ColdkeySwapAnnounced)
        }
    }

    fn is_allowed_during_swap(call: &CallOf<T>) -> bool {
        matches!(
            call.is_sub_type(),
            Some(
                Call::announce_coldkey_swap { .. }
                    | Call::swap_coldkey_announced { .. }
                    | Call::dispute_coldkey_swap { .. }
                    | Call::clear_coldkey_swap_announcement { .. }
            )
        ) || matches!(
            IsSubType::<pallet_shield::Call<T>>::is_sub_type(call),
            Some(pallet_shield::Call::submit_encrypted { .. })
        )
    }
}

impl<T> DispatchExtension<<T as frame_system::Config>::RuntimeCall> for CheckColdkeySwap<T>
where
    T: Config + pallet_shield::Config,
    <T as frame_system::Config>::RuntimeCall: Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>
        + IsSubType<Call<T>>
        + IsSubType<pallet_shield::Call<T>>,
    DispatchableOriginOf<T>: OriginTrait<AccountId = T::AccountId>,
{
    type Pre = ();

    fn weight(_call: &CallOf<T>) -> Weight {
        <T as Config>::WeightInfo::check_coldkey_swap_extension()
    }

    fn pre_dispatch(
        origin: &DispatchableOriginOf<T>,
        call: &CallOf<T>,
    ) -> Result<Self::Pre, DispatchErrorWithPostInfo> {
        // Only care about signed origins.
        // Root is already bypassed by the extension before we get here.
        let Some(who) = origin.as_signer() else {
            return Ok(());
        };

        Self::check(who, call).map_err(Into::into)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::CheckColdkeySwap;
    use crate::{
        ColdkeySwapAnnouncements, ColdkeySwapDisputes, Error, tests::mock::*,
        weights::WeightInfo as _,
    };
    use frame_support::{
        BoundedVec, assert_ok,
        dispatch::{DispatchExtension, DispatchResultWithPostInfo},
        traits::ExtendedDispatchable,
    };
    use frame_system::Call as SystemCall;
    use pallet_subtensor_proxy::Call as ProxyCall;
    use sp_core::U256;
    use sp_runtime::traits::Hash;
    use subtensor_runtime_common::{ProxyType, TaoBalance};

    type HashingOf<T> = <T as frame_system::Config>::Hashing;

    /// Calls that should be blocked when a coldkey swap is active.
    fn forbidden_calls() -> Vec<RuntimeCall> {
        vec![
            RuntimeCall::System(SystemCall::remark { remark: vec![] }),
            RuntimeCall::SubtensorModule(crate::Call::add_stake {
                hotkey: U256::from(1),
                netuid: 1u16.into(),
                amount_staked: 1_000u64.into(),
            }),
            RuntimeCall::SubtensorModule(crate::Call::remove_stake {
                hotkey: U256::from(1),
                netuid: 1u16.into(),
                amount_unstaked: 1_000u64.into(),
            }),
            RuntimeCall::SubtensorModule(crate::Call::set_weights {
                netuid: 1u16.into(),
                dests: vec![],
                weights: vec![],
                version_key: 0,
            }),
            RuntimeCall::SubtensorModule(crate::Call::register_network {
                hotkey: U256::from(1),
            }),
        ]
    }

    /// Calls that should be allowed through the extension during an active (undisputed) swap.
    fn authorized_calls() -> Vec<RuntimeCall> {
        vec![
            RuntimeCall::SubtensorModule(crate::Call::announce_coldkey_swap {
                new_coldkey_hash: HashingOf::<Test>::hash_of(&U256::from(99)),
            }),
            RuntimeCall::SubtensorModule(crate::Call::swap_coldkey_announced {
                new_coldkey: U256::from(42),
            }),
            RuntimeCall::SubtensorModule(crate::Call::dispute_coldkey_swap {}),
            RuntimeCall::Shield(pallet_shield::Call::submit_encrypted {
                ciphertext: BoundedVec::truncate_from(vec![1, 2, 3, 4]),
            }),
        ]
    }

    fn setup_swap_announced(who: &U256) {
        let now = System::block_number();
        let hash = HashingOf::<Test>::hash_of(&U256::from(42));
        ColdkeySwapAnnouncements::<Test>::insert(who, (now, hash));
    }

    fn setup_swap_disputed(who: &U256) {
        setup_swap_announced(who);
        ColdkeySwapDisputes::<Test>::insert(who, System::block_number());
    }

    fn remark_call() -> RuntimeCall {
        RuntimeCall::System(SystemCall::remark { remark: vec![] })
    }

    fn add_balance_to_coldkey_account(coldkey: &U256, tao: TaoBalance) {
        let credit = SubtensorModule::mint_tao(tao);
        let _ = SubtensorModule::spend_tao(coldkey, credit, tao).unwrap();
    }

    fn dispatch_with_ext(call: RuntimeCall, origin: RuntimeOrigin) -> DispatchResultWithPostInfo {
        <CheckColdkeySwap<Test> as ExtendedDispatchable<RuntimeCall>>::dispatch_with_extension(
            origin, call,
        )
    }

    #[test]
    fn weight_charges_all_calls_because_swap_state_can_block_any_signed_call() {
        let expected = <Test as crate::Config>::WeightInfo::check_coldkey_swap_extension();

        for call in forbidden_calls().into_iter().chain(authorized_calls()) {
            assert_eq!(
                <CheckColdkeySwap<Test> as DispatchExtension<RuntimeCall>>::weight(&call),
                expected
            );
        }
    }

    #[test]
    fn no_active_swap_allows_calls() {
        new_test_ext(1).execute_with(|| {
            let who = U256::from(1);
            assert_ok!(dispatch_with_ext(remark_call(), RuntimeOrigin::signed(who)));
        });
    }

    #[test]
    fn none_bypasses_extension() {
        new_test_ext(1).execute_with(|| {
            let who = U256::from(1);
            setup_swap_disputed(&who);
            assert_ok!(dispatch_with_ext(remark_call(), RuntimeOrigin::none()));
        });
    }

    #[test]
    fn root_bypasses_extension() {
        new_test_ext(1).execute_with(|| {
            let who = U256::from(1);
            setup_swap_disputed(&who);
            assert_ok!(dispatch_with_ext(remark_call(), RuntimeOrigin::root()));
        });
    }

    #[test]
    fn active_swap_blocks_forbidden_calls() {
        new_test_ext(1).execute_with(|| {
            let who = U256::from(1);
            setup_swap_announced(&who);

            for call in forbidden_calls() {
                assert_eq!(
                    dispatch_with_ext(call, RuntimeOrigin::signed(who))
                        .unwrap_err()
                        .error,
                    Error::<Test>::ColdkeySwapAnnounced.into()
                );
            }
        });
    }

    #[test]
    fn active_swap_allows_authorized_calls() {
        new_test_ext(1).execute_with(|| {
            let who = U256::from(1);
            setup_swap_announced(&who);

            for call in authorized_calls() {
                if let Err(err) = dispatch_with_ext(call, RuntimeOrigin::signed(who)) {
                    assert_ne!(
                        err.error,
                        Error::<Test>::ColdkeySwapAnnounced.into(),
                        "Authorized call should not be blocked by the extension"
                    );
                }
            }
        });
    }

    #[test]
    fn disputed_swap_blocks_all_calls() {
        new_test_ext(1).execute_with(|| {
            let who = U256::from(1);
            setup_swap_disputed(&who);

            // Both forbidden and authorized calls should be blocked during dispute
            let all_calls = forbidden_calls()
                .into_iter()
                .chain(authorized_calls())
                .collect::<Vec<_>>();

            for call in all_calls {
                assert_eq!(
                    dispatch_with_ext(call, RuntimeOrigin::signed(who))
                        .unwrap_err()
                        .error,
                    Error::<Test>::ColdkeySwapDisputed.into()
                );
            }
        });
    }

    #[test]
    fn proxied_forbidden_call_blocked() {
        new_test_ext(1).execute_with(|| {
            let real = U256::from(1);
            let delegate = U256::from(2);
            let now = System::block_number();
            let hash = HashingOf::<Test>::hash_of(&U256::from(42));
            ColdkeySwapAnnouncements::<Test>::insert(real, (now, hash));

            // Give delegate enough balance for proxy deposit
            add_balance_to_coldkey_account(&real, 1_000_000_000.into());
            add_balance_to_coldkey_account(&delegate, 1_000_000_000.into());

            // Register proxy: delegate can act on behalf of real
            assert_ok!(Proxy::add_proxy(
                RuntimeOrigin::signed(real),
                delegate,
                ProxyType::Any,
                0
            ));

            // Dispatch a proxy call as delegate
            let proxy_call = RuntimeCall::Proxy(ProxyCall::proxy {
                real,
                force_proxy_type: None,
                call: Box::new(remark_call()),
            });

            // The outer proxy call itself succeeds
            assert_ok!(dispatch_with_ext(
                proxy_call,
                RuntimeOrigin::signed(delegate)
            ));

            // The inner call was blocked — check via LastCallResult storage.
            assert_eq!(
                pallet_subtensor_proxy::LastCallResult::<Test>::get(real),
                Some(Err(Error::<Test>::ColdkeySwapAnnounced.into()))
            );
        });
    }

    #[test]
    fn nested_proxy_blocked() {
        new_test_ext(1).execute_with(|| {
            let real = U256::from(1);
            let delegate1 = U256::from(2);
            let delegate2 = U256::from(3);
            let now = System::block_number();
            let hash = HashingOf::<Test>::hash_of(&U256::from(42));
            ColdkeySwapAnnouncements::<Test>::insert(real, (now, hash));

            add_balance_to_coldkey_account(&real, 1_000_000_000.into());
            add_balance_to_coldkey_account(&delegate1, 1_000_000_000.into());
            add_balance_to_coldkey_account(&delegate2, 1_000_000_000.into());

            // delegate1 can proxy for real, delegate2 can proxy for delegate1
            assert_ok!(Proxy::add_proxy(
                RuntimeOrigin::signed(real),
                delegate1,
                ProxyType::Any,
                0
            ));
            assert_ok!(Proxy::add_proxy(
                RuntimeOrigin::signed(delegate1),
                delegate2,
                ProxyType::Any,
                0
            ));

            // Nested: delegate2 -> delegate1 -> proxy(real, remark)
            let inner_proxy = RuntimeCall::Proxy(ProxyCall::proxy {
                real,
                force_proxy_type: None,
                call: Box::new(remark_call()),
            });
            let outer_proxy = RuntimeCall::Proxy(ProxyCall::proxy {
                real: delegate1,
                force_proxy_type: None,
                call: Box::new(inner_proxy),
            });

            assert_ok!(dispatch_with_ext(
                outer_proxy,
                RuntimeOrigin::signed(delegate2)
            ));

            // The innermost call (remark as real) was blocked.
            assert_eq!(
                pallet_subtensor_proxy::LastCallResult::<Test>::get(real),
                Some(Err(Error::<Test>::ColdkeySwapAnnounced.into()))
            );
        });
    }
}
