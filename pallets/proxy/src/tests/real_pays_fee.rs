//! set_real_pays_fee preference and cleanup on proxy removal.

use super::mock::*;
use crate::*;
use frame::testing_prelude::*;

#[test]
fn set_real_pays_fee_works() {
    new_test_ext().execute_with(|| {
        // Account 1 adds account 3 as proxy
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(1),
            3,
            ProxyType::Any,
            0
        ));

        // Account 1 (real) enables real-pays-fee for delegate 3
        assert_ok!(Proxy::set_real_pays_fee(RuntimeOrigin::signed(1), 3, true));
        assert!(Proxy::is_real_pays_fee(&1, &3));
        System::assert_last_event(
            ProxyEvent::RealPaysFeeSet {
                real: 1,
                delegate: 3,
                pays_fee: true,
            }
            .into(),
        );

        // Disable it
        assert_ok!(Proxy::set_real_pays_fee(RuntimeOrigin::signed(1), 3, false));
        assert!(!Proxy::is_real_pays_fee(&1, &3));
        System::assert_last_event(
            ProxyEvent::RealPaysFeeSet {
                real: 1,
                delegate: 3,
                pays_fee: false,
            }
            .into(),
        );
    });
}

#[test]
fn set_real_pays_fee_fails_without_proxy() {
    new_test_ext().execute_with(|| {
        // No proxy relationship between 1 and 3
        assert_noop!(
            Proxy::set_real_pays_fee(RuntimeOrigin::signed(1), 3, true),
            Error::<Test>::NotProxy,
        );
    });
}

#[test]
fn set_real_pays_fee_fails_unsigned() {
    new_test_ext().execute_with(|| {
        assert_noop!(
            Proxy::set_real_pays_fee(RuntimeOrigin::none(), 3, true),
            DispatchError::BadOrigin,
        );
    });
}

#[test]
fn set_real_pays_fee_fails_root() {
    new_test_ext().execute_with(|| {
        assert_noop!(
            Proxy::set_real_pays_fee(RuntimeOrigin::root(), 3, true),
            DispatchError::BadOrigin,
        );
    });
}

#[test]
fn real_pays_fee_cleaned_on_remove_proxy() {
    new_test_ext().execute_with(|| {
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(1),
            3,
            ProxyType::Any,
            0
        ));
        assert_ok!(Proxy::set_real_pays_fee(RuntimeOrigin::signed(1), 3, true));
        assert!(Proxy::is_real_pays_fee(&1, &3));

        // Remove the proxy
        assert_ok!(Proxy::remove_proxy(
            RuntimeOrigin::signed(1),
            3,
            ProxyType::Any,
            0
        ));

        // Flag should be cleaned up
        assert!(!Proxy::is_real_pays_fee(&1, &3));
    });
}

#[test]
fn real_pays_fee_cleaned_on_remove_proxies() {
    new_test_ext().execute_with(|| {
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(1),
            2,
            ProxyType::Any,
            0
        ));
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(1),
            3,
            ProxyType::Any,
            0
        ));
        assert_ok!(Proxy::set_real_pays_fee(RuntimeOrigin::signed(1), 2, true));
        assert_ok!(Proxy::set_real_pays_fee(RuntimeOrigin::signed(1), 3, true));
        assert!(Proxy::is_real_pays_fee(&1, &2));
        assert!(Proxy::is_real_pays_fee(&1, &3));

        // Remove all proxies
        assert_ok!(Proxy::remove_proxies(RuntimeOrigin::signed(1)));

        // Both flags should be cleaned up
        assert!(!Proxy::is_real_pays_fee(&1, &2));
        assert!(!Proxy::is_real_pays_fee(&1, &3));
    });
}
