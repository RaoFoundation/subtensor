use super::{CallOf, DispatchableOriginOf};
use crate::weights::WeightInfo;
use crate::{Call, ColdkeySwapAnnouncements, ColdkeySwapDisputes, Config, Error};
use frame_support::{
    dispatch::{DispatchErrorWithPostInfo, DispatchExtension, DispatchInfo, PostDispatchInfo},
    pallet_prelude::*,
    traits::{IsSubType, OriginTrait},
};
use sp_runtime::traits::{Dispatchable, Zero};
use sp_std::marker::PhantomData;

/// Dispatch extension that blocks most calls when a coldkey swap is active.
///
/// When a coldkey swap has been announced for the signing account:
/// * If the swap is disputed, ALL calls are blocked.
/// * Otherwise, only swap-related calls and MEV-protected calls (`submit_encrypted`)
///   are allowed through.
///
/// After a root reset that keeps the lock, announcement, dispute, swap execution, and
/// `submit_encrypted` are allowed, even while there is no announcement. Clearing an
/// announcement cannot unlock the key. An active dispute still blocks every call,
/// including `submit_encrypted`.
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
        let dispute = ColdkeySwapDisputes::<T>::get(who);
        if dispute.is_some_and(|when| !when.is_zero()) {
            return Err(Error::<T>::ColdkeySwapDisputed);
        }
        let locked = dispute.is_some();
        if !locked && !ColdkeySwapAnnouncements::<T>::contains_key(who) {
            return Ok(());
        }

        if Self::is_allowed_during_swap(call, locked) {
            Ok(())
        } else {
            Err(Error::<T>::ColdkeySwapAnnounced)
        }
    }

    fn is_allowed_during_swap(call: &CallOf<T>, locked: bool) -> bool {
        matches!(
            call.is_sub_type(),
            Some(
                Call::announce_coldkey_swap { .. }
                    | Call::swap_coldkey_announced { .. }
                    | Call::dispute_coldkey_swap { .. }
            )
        ) || matches!(
            IsSubType::<pallet_shield::Call<T>>::is_sub_type(call),
            Some(pallet_shield::Call::submit_encrypted { .. })
        ) || (!locked
            && matches!(
                call.is_sub_type(),
                Some(Call::clear_coldkey_swap_announcement { .. })
            ))
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
            RuntimeCall::Balances(pallet_balances::Call::transfer_allow_death {
                dest: U256::from(99),
                value: 1_000u64.into(),
            }),
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
    fn reset_keeps_recovery_locked_through_reannouncement_and_redispute() {
        new_test_ext(1).execute_with(|| {
            let who = U256::from(1);
            let destination = U256::from(42);
            let hash = HashingOf::<Test>::hash_of(&destination);
            let announce = RuntimeCall::SubtensorModule(crate::Call::announce_coldkey_swap {
                new_coldkey_hash: hash,
            });
            let dispute = RuntimeCall::SubtensorModule(crate::Call::dispute_coldkey_swap {});
            let swap = RuntimeCall::SubtensorModule(crate::Call::swap_coldkey_announced {
                new_coldkey: destination,
            });
            let clear =
                RuntimeCall::SubtensorModule(crate::Call::clear_coldkey_swap_announcement {});
            let shield = RuntimeCall::Shield(pallet_shield::Call::submit_encrypted {
                ciphertext: BoundedVec::truncate_from(vec![1, 2, 3, 4]),
            });
            let signed = RuntimeOrigin::signed(who);
            add_balance_to_coldkey_account(
                &who,
                SubtensorModule::get_key_swap_cost() * 3u64.into() + ExistentialDeposit::get(),
            );
            assert_ok!(dispatch_with_ext(announce.clone(), signed.clone()));
            assert_ok!(dispatch_with_ext(dispute.clone(), signed.clone()));

            // Root can reset repeatedly without releasing funds or the recovery lock.
            for round in 0..2 {
                let balance = SubtensorModule::get_coldkey_balance(&who);
                assert_ok!(dispatch_with_ext(
                    RuntimeCall::SubtensorModule(crate::Call::reset_coldkey_swap_keep_locked {
                        coldkey: who
                    }),
                    RuntimeOrigin::root(),
                ));
                assert!(!ColdkeySwapAnnouncements::<Test>::contains_key(who));
                assert_eq!(ColdkeySwapDisputes::<Test>::get(who), Some(0));
                assert_eq!(SubtensorModule::get_coldkey_balance(&who), balance);
                assert_ok!(dispatch_with_ext(shield.clone(), signed.clone()));

                for call in forbidden_calls().into_iter().chain([clear.clone()]) {
                    assert_eq!(
                        dispatch_with_ext(call, signed.clone()).unwrap_err().error,
                        Error::<Test>::ColdkeySwapAnnounced.into(),
                    );
                }
                for call in [swap.clone(), dispute.clone()] {
                    assert_eq!(
                        dispatch_with_ext(call, signed.clone()).unwrap_err().error,
                        Error::<Test>::ColdkeySwapAnnouncementNotFound.into(),
                    );
                }

                // A fresh announcement uses the normal cost and full delay.
                assert_ok!(dispatch_with_ext(announce.clone(), signed.clone()));
                let (when, announced_hash) = ColdkeySwapAnnouncements::<Test>::get(who).unwrap();
                assert_eq!(announced_hash, hash);
                assert_eq!(
                    when,
                    System::block_number() + crate::ColdkeySwapAnnouncementDelay::<Test>::get()
                );
                assert_eq!(
                    dispatch_with_ext(swap.clone(), signed.clone())
                        .unwrap_err()
                        .error,
                    Error::<Test>::ColdkeySwapTooEarly.into(),
                );
                assert!(ColdkeySwapAnnouncements::<Test>::contains_key(who));
                assert_eq!(ColdkeySwapDisputes::<Test>::get(who), Some(0));
                assert_ok!(dispatch_with_ext(shield.clone(), signed.clone()));

                // Even once ordinary cancellation would be allowed, the root lock persists.
                System::set_block_number(
                    when + crate::ColdkeySwapReannouncementDelay::<Test>::get(),
                );
                assert_eq!(
                    dispatch_with_ext(clear.clone(), signed.clone())
                        .unwrap_err()
                        .error,
                    Error::<Test>::ColdkeySwapAnnounced.into(),
                );
                assert_ok!(dispatch_with_ext(shield.clone(), signed.clone()));
                assert_eq!(
                    dispatch_with_ext(remark_call(), signed.clone())
                        .unwrap_err()
                        .error,
                    Error::<Test>::ColdkeySwapAnnounced.into(),
                );
                if round == 0 {
                    assert_ok!(dispatch_with_ext(dispute.clone(), signed.clone()));
                    for call in [
                        announce.clone(),
                        dispute.clone(),
                        swap.clone(),
                        shield.clone(),
                        remark_call(),
                    ] {
                        assert_eq!(
                            dispatch_with_ext(call, signed.clone()).unwrap_err().error,
                            Error::<Test>::ColdkeySwapDisputed.into(),
                        );
                    }
                }
            }

            let balance = SubtensorModule::get_coldkey_balance(&who);
            assert_ok!(dispatch_with_ext(swap, signed.clone()));
            assert!(!ColdkeySwapDisputes::<Test>::contains_key(who));
            assert!(!ColdkeySwapAnnouncements::<Test>::contains_key(who));
            assert_eq!(SubtensorModule::get_coldkey_balance(&destination), balance);
            assert_ok!(dispatch_with_ext(remark_call(), signed));
            assert_ok!(dispatch_with_ext(
                remark_call(),
                RuntimeOrigin::signed(destination)
            ));
        });
    }

    #[test]
    fn reset_lock_cannot_be_cleared_by_swapping_to_self() {
        new_test_ext(1).execute_with(|| {
            let who = U256::from(1);
            add_balance_to_coldkey_account(
                &who,
                SubtensorModule::get_key_swap_cost() + ExistentialDeposit::get(),
            );
            assert_ok!(SubtensorModule::reset_coldkey_swap_keep_locked(
                RuntimeOrigin::root(),
                who
            ));
            assert_ok!(dispatch_with_ext(
                RuntimeCall::SubtensorModule(crate::Call::announce_coldkey_swap {
                    new_coldkey_hash: HashingOf::<Test>::hash_of(&who),
                }),
                RuntimeOrigin::signed(who),
            ));
            System::set_block_number(ColdkeySwapAnnouncements::<Test>::get(who).unwrap().0);
            assert_ok!(dispatch_with_ext(
                RuntimeCall::SubtensorModule(crate::Call::swap_coldkey_announced {
                    new_coldkey: who
                }),
                RuntimeOrigin::signed(who),
            ));
            assert_eq!(ColdkeySwapDisputes::<Test>::get(who), Some(0));
            assert_eq!(
                dispatch_with_ext(remark_call(), RuntimeOrigin::signed(who))
                    .unwrap_err()
                    .error,
                Error::<Test>::ColdkeySwapAnnounced.into(),
            );
        });
    }

    #[test]
    fn original_reset_still_fully_unlocks_a_recovery_locked_key() {
        new_test_ext(1).execute_with(|| {
            let who = U256::from(1);
            setup_swap_disputed(&who);
            assert_ok!(SubtensorModule::reset_coldkey_swap_keep_locked(
                RuntimeOrigin::root(),
                who
            ));
            assert_ok!(SubtensorModule::reset_coldkey_swap(
                RuntimeOrigin::root(),
                who
            ));
            assert!(!ColdkeySwapAnnouncements::<Test>::contains_key(who));
            assert!(!ColdkeySwapDisputes::<Test>::contains_key(who));
            assert_ok!(dispatch_with_ext(remark_call(), RuntimeOrigin::signed(who)));
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
            setup_swap_disputed(&real);
            assert_ok!(SubtensorModule::reset_coldkey_swap_keep_locked(
                RuntimeOrigin::root(),
                real
            ));

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
