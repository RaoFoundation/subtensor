//! ProxyType InstanceFilter behavior through `proxy` (including nested utility).

use super::mock::*;
use crate::*;
use alloc::{boxed::Box, vec};
use frame::testing_prelude::*;
use pallet_balances::Event as BalancesEvent;
use pallet_subtensor_utility::{Call as UtilityCall, Event as UtilityEvent};

#[test]
fn filtering_works() {
    new_test_ext().execute_with(|| {
        Balances::make_free_balance_be(&1, 1000);
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(1),
            2,
            ProxyType::Any,
            0
        ));
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(1),
            3,
            ProxyType::JustTransfer,
            0
        ));
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(1),
            4,
            ProxyType::JustUtility,
            0
        ));

        let call = Box::new(call_transfer(6, 1));
        assert_ok!(Proxy::proxy(
            RuntimeOrigin::signed(2),
            1,
            None,
            call.clone()
        ));
        System::assert_last_event(ProxyEvent::ProxyExecuted { result: Ok(()) }.into());
        assert_ok!(Proxy::proxy(
            RuntimeOrigin::signed(3),
            1,
            None,
            call.clone()
        ));
        System::assert_last_event(ProxyEvent::ProxyExecuted { result: Ok(()) }.into());
        assert_ok!(Proxy::proxy(
            RuntimeOrigin::signed(4),
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

        let derivative_id = Utility::derivative_account_id(1, 0).unwrap();
        Balances::make_free_balance_be(&derivative_id, 1000);
        let inner = Box::new(call_transfer(6, 1));

        let call = Box::new(RuntimeCall::Utility(UtilityCall::as_derivative {
            index: 0,
            call: inner.clone(),
        }));
        assert_ok!(Proxy::proxy(
            RuntimeOrigin::signed(2),
            1,
            None,
            call.clone()
        ));
        System::assert_last_event(ProxyEvent::ProxyExecuted { result: Ok(()) }.into());
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
        assert_ok!(Proxy::proxy(
            RuntimeOrigin::signed(4),
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

        let call = Box::new(RuntimeCall::Utility(UtilityCall::batch {
            calls: vec![*inner],
        }));
        assert_ok!(Proxy::proxy(
            RuntimeOrigin::signed(2),
            1,
            None,
            call.clone()
        ));
        expect_events(vec![
            UtilityEvent::BatchCompleted.into(),
            ProxyEvent::ProxyExecuted { result: Ok(()) }.into(),
        ]);
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
        assert_ok!(Proxy::proxy(
            RuntimeOrigin::signed(4),
            1,
            None,
            call.clone()
        ));
        expect_events(vec![
            UtilityEvent::BatchInterrupted {
                index: 0,
                error: SystemError::CallFiltered.into(),
            }
            .into(),
            ProxyEvent::ProxyExecuted { result: Ok(()) }.into(),
        ]);

        let inner = Box::new(RuntimeCall::Proxy(ProxyCall::new_call_variant_add_proxy(
            5,
            ProxyType::Any,
            0,
        )));
        let call = Box::new(RuntimeCall::Utility(UtilityCall::batch {
            calls: vec![*inner],
        }));
        assert_ok!(Proxy::proxy(
            RuntimeOrigin::signed(2),
            1,
            None,
            call.clone()
        ));
        expect_events(vec![
            UtilityEvent::BatchCompleted.into(),
            ProxyEvent::ProxyExecuted { result: Ok(()) }.into(),
        ]);
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
        assert_ok!(Proxy::proxy(
            RuntimeOrigin::signed(4),
            1,
            None,
            call.clone()
        ));
        expect_events(vec![
            UtilityEvent::BatchInterrupted {
                index: 0,
                error: SystemError::CallFiltered.into(),
            }
            .into(),
            ProxyEvent::ProxyExecuted { result: Ok(()) }.into(),
        ]);

        let call = Box::new(RuntimeCall::Proxy(ProxyCall::remove_proxies {}));
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
        assert_ok!(Proxy::proxy(
            RuntimeOrigin::signed(4),
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
        assert_ok!(Proxy::proxy(
            RuntimeOrigin::signed(2),
            1,
            None,
            call.clone()
        ));
        expect_events(vec![
            BalancesEvent::<Test>::Unreserved { who: 1, amount: 5 }.into(),
            ProxyEvent::ProxyExecuted { result: Ok(()) }.into(),
        ]);
    });
}
