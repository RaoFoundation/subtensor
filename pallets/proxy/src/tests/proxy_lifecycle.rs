//! add_proxy / remove_proxy / deposit and basic `proxy` dispatch tests.

use super::mock::*;
use crate::*;
use alloc::boxed::Box;
use frame::testing_prelude::*;
use frame_system::Call as SystemCall;
use pallet_balances::Call as BalancesCall;

#[test]
fn add_remove_proxies_works() {
    new_test_ext().execute_with(|| {
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(1),
            2,
            ProxyType::Any,
            0
        ));
        assert_noop!(
            Proxy::add_proxy(RuntimeOrigin::signed(1), 2, ProxyType::Any, 0),
            Error::<Test>::Duplicate
        );
        assert_eq!(Balances::reserved_balance(1), 2);
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(1),
            2,
            ProxyType::JustTransfer,
            0
        ));
        assert_eq!(Balances::reserved_balance(1), 3);
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(1),
            3,
            ProxyType::Any,
            0
        ));
        assert_eq!(Balances::reserved_balance(1), 4);
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(1),
            4,
            ProxyType::JustUtility,
            0
        ));
        assert_eq!(Balances::reserved_balance(1), 5);
        assert_noop!(
            Proxy::add_proxy(RuntimeOrigin::signed(1), 4, ProxyType::Any, 0),
            Error::<Test>::TooMany
        );
        assert_noop!(
            Proxy::remove_proxy(RuntimeOrigin::signed(1), 3, ProxyType::JustTransfer, 0),
            Error::<Test>::NotFound
        );
        assert_ok!(Proxy::remove_proxy(
            RuntimeOrigin::signed(1),
            4,
            ProxyType::JustUtility,
            0
        ));
        System::assert_last_event(
            ProxyEvent::ProxyRemoved {
                delegator: 1,
                delegatee: 4,
                proxy_type: ProxyType::JustUtility,
                delay: 0,
            }
            .into(),
        );
        assert_eq!(Balances::reserved_balance(1), 4);
        assert_ok!(Proxy::remove_proxy(
            RuntimeOrigin::signed(1),
            3,
            ProxyType::Any,
            0
        ));
        assert_eq!(Balances::reserved_balance(1), 3);
        System::assert_last_event(
            ProxyEvent::ProxyRemoved {
                delegator: 1,
                delegatee: 3,
                proxy_type: ProxyType::Any,
                delay: 0,
            }
            .into(),
        );
        assert_ok!(Proxy::remove_proxy(
            RuntimeOrigin::signed(1),
            2,
            ProxyType::Any,
            0
        ));
        assert_eq!(Balances::reserved_balance(1), 2);
        System::assert_last_event(
            ProxyEvent::ProxyRemoved {
                delegator: 1,
                delegatee: 2,
                proxy_type: ProxyType::Any,
                delay: 0,
            }
            .into(),
        );
        assert_ok!(Proxy::remove_proxy(
            RuntimeOrigin::signed(1),
            2,
            ProxyType::JustTransfer,
            0
        ));
        assert_eq!(Balances::reserved_balance(1), 0);
        System::assert_last_event(
            ProxyEvent::ProxyRemoved {
                delegator: 1,
                delegatee: 2,
                proxy_type: ProxyType::JustTransfer,
                delay: 0,
            }
            .into(),
        );
        assert_noop!(
            Proxy::add_proxy(RuntimeOrigin::signed(1), 1, ProxyType::Any, 0),
            Error::<Test>::NoSelfProxy
        );
    });
}

#[test]
fn cannot_add_proxy_without_balance() {
    new_test_ext().execute_with(|| {
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(5),
            3,
            ProxyType::Any,
            0
        ));
        assert_eq!(Balances::reserved_balance(5), 2);
        assert_noop!(
            Proxy::add_proxy(RuntimeOrigin::signed(5), 4, ProxyType::Any, 0),
            DispatchError::ConsumerRemaining,
        );
    });
}

#[test]
fn proxying_works() {
    new_test_ext().execute_with(|| {
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(1),
            2,
            ProxyType::JustTransfer,
            0
        ));
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(1),
            3,
            ProxyType::Any,
            0
        ));

        let call = Box::new(call_transfer(6, 1));
        assert_noop!(
            Proxy::proxy(RuntimeOrigin::signed(4), 1, None, call.clone()),
            Error::<Test>::NotProxy
        );
        assert_noop!(
            Proxy::proxy(
                RuntimeOrigin::signed(2),
                1,
                Some(ProxyType::Any),
                call.clone()
            ),
            Error::<Test>::NotProxy
        );
        assert_ok!(Proxy::proxy(
            RuntimeOrigin::signed(2),
            1,
            None,
            call.clone()
        ));
        System::assert_last_event(ProxyEvent::ProxyExecuted { result: Ok(()) }.into());
        assert_eq!(Balances::free_balance(6), 1);

        let call = Box::new(RuntimeCall::System(SystemCall::set_code { code: vec![] }));
        assert_ok!(Proxy::proxy(
            RuntimeOrigin::signed(3),
            1,
            None,
            call.clone()
        ));
        System::assert_last_event(
            ProxyEvent::ProxyExecuted {
                result: Err(SystemError::CallFiltered.into()),
            }
            .into(),
        );

        let call = Box::new(RuntimeCall::Balances(BalancesCall::transfer_keep_alive {
            dest: 6,
            value: 1,
        }));
        assert_ok!(
            RuntimeCall::Proxy(ProxyCall::new_call_variant_proxy(1, None, call.clone()))
                .dispatch(RuntimeOrigin::signed(2))
        );
        System::assert_last_event(
            ProxyEvent::ProxyExecuted {
                result: Err(SystemError::CallFiltered.into()),
            }
            .into(),
        );
        assert_ok!(Proxy::proxy(
            RuntimeOrigin::signed(3),
            1,
            None,
            call.clone()
        ));
        System::assert_last_event(ProxyEvent::ProxyExecuted { result: Ok(()) }.into());
        assert_eq!(Balances::free_balance(6), 2);
    });
}
