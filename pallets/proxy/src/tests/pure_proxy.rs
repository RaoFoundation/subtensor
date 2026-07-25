//! create_pure / kill_pure pure-account lifecycle tests.

use super::mock::*;
use crate::*;
use alloc::boxed::Box;
use frame::testing_prelude::*;

#[test]
fn pure_works() {
    new_test_ext().execute_with(|| {
        Balances::make_free_balance_be(&1, 11); // An extra one for the ED.
        assert_ok!(Proxy::create_pure(
            RuntimeOrigin::signed(1),
            ProxyType::Any,
            0,
            0
        ));
        let anon = Proxy::pure_account(&1, &ProxyType::Any, 0, None).unwrap();
        System::assert_last_event(
            ProxyEvent::PureCreated {
                pure: anon,
                who: 1,
                proxy_type: ProxyType::Any,
                disambiguation_index: 0,
            }
            .into(),
        );

        // other calls to pure allowed as long as they're not exactly the same.
        assert_ok!(Proxy::create_pure(
            RuntimeOrigin::signed(1),
            ProxyType::JustTransfer,
            0,
            0
        ));
        assert_ok!(Proxy::create_pure(
            RuntimeOrigin::signed(1),
            ProxyType::Any,
            0,
            1
        ));
        let anon2 = Proxy::pure_account(&2, &ProxyType::Any, 0, None).unwrap();
        assert_ok!(Proxy::create_pure(
            RuntimeOrigin::signed(2),
            ProxyType::Any,
            0,
            0
        ));
        assert_noop!(
            Proxy::create_pure(RuntimeOrigin::signed(1), ProxyType::Any, 0, 0),
            Error::<Test>::Duplicate
        );
        System::set_extrinsic_index(1);
        assert_ok!(Proxy::create_pure(
            RuntimeOrigin::signed(1),
            ProxyType::Any,
            0,
            0
        ));
        System::set_extrinsic_index(0);
        System::set_block_number(2);
        assert_ok!(Proxy::create_pure(
            RuntimeOrigin::signed(1),
            ProxyType::Any,
            0,
            0
        ));

        let call = Box::new(call_transfer(6, 1));
        assert_ok!(Balances::transfer_allow_death(
            RuntimeOrigin::signed(3),
            anon,
            5
        ));
        assert_ok!(Proxy::proxy(RuntimeOrigin::signed(1), anon, None, call));
        System::assert_last_event(ProxyEvent::ProxyExecuted { result: Ok(()) }.into());
        assert_eq!(Balances::free_balance(6), 1);

        let call = Box::new(RuntimeCall::Proxy(ProxyCall::new_call_variant_kill_pure(
            1,
            ProxyType::Any,
            0,
            1,
            0,
        )));
        assert_ok!(Proxy::proxy(
            RuntimeOrigin::signed(2),
            anon2,
            None,
            call.clone()
        ));
        let de = DispatchError::from(Error::<Test>::NoPermission).stripped();
        System::assert_last_event(ProxyEvent::ProxyExecuted { result: Err(de) }.into());
        assert_noop!(
            Proxy::kill_pure(RuntimeOrigin::signed(1), 1, ProxyType::Any, 0, 1, 0),
            Error::<Test>::NoPermission
        );
        assert_eq!(Balances::free_balance(1), 1);
        assert_ok!(Proxy::proxy(
            RuntimeOrigin::signed(1),
            anon,
            None,
            call.clone()
        ));
        assert_eq!(Balances::free_balance(1), 3);
        assert_noop!(
            Proxy::proxy(RuntimeOrigin::signed(1), anon, None, call.clone()),
            Error::<Test>::NotProxy
        );
    });
}
