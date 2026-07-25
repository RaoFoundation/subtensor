//! Announcement delay / announce / remove / reject / proxy_announced tests.

use super::mock::*;
use crate::*;
use alloc::{boxed::Box, vec};
use frame::testing_prelude::*;

#[test]
fn announcement_works() {
    new_test_ext().execute_with(|| {
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(1),
            3,
            ProxyType::Any,
            1
        ));
        System::assert_last_event(
            ProxyEvent::ProxyAdded {
                delegator: 1,
                delegatee: 3,
                proxy_type: ProxyType::Any,
                delay: 1,
            }
            .into(),
        );
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(2),
            3,
            ProxyType::Any,
            1
        ));
        assert_eq!(Balances::reserved_balance(3), 0);

        assert_ok!(Proxy::announce(RuntimeOrigin::signed(3), 1, [1; 32].into()));
        let announcements = Announcements::<Test>::get(3);
        assert_eq!(
            announcements.0,
            vec![Announcement {
                real: 1,
                call_hash: [1; 32].into(),
                height: 1
            }]
        );
        assert_eq!(Balances::reserved_balance(3), announcements.1);

        assert_ok!(Proxy::announce(RuntimeOrigin::signed(3), 2, [2; 32].into()));
        let announcements = Announcements::<Test>::get(3);
        assert_eq!(
            announcements.0,
            vec![
                Announcement {
                    real: 1,
                    call_hash: [1; 32].into(),
                    height: 1
                },
                Announcement {
                    real: 2,
                    call_hash: [2; 32].into(),
                    height: 1
                },
            ]
        );
        assert_eq!(Balances::reserved_balance(3), announcements.1);

        assert_noop!(
            Proxy::announce(RuntimeOrigin::signed(3), 2, [3; 32].into()),
            Error::<Test>::TooMany
        );
    });
}

#[test]
fn remove_announcement_works() {
    new_test_ext().execute_with(|| {
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(1),
            3,
            ProxyType::Any,
            1
        ));
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(2),
            3,
            ProxyType::Any,
            1
        ));
        assert_ok!(Proxy::announce(RuntimeOrigin::signed(3), 1, [1; 32].into()));
        assert_ok!(Proxy::announce(RuntimeOrigin::signed(3), 2, [2; 32].into()));
        let e = Error::<Test>::NotFound;
        assert_noop!(
            Proxy::remove_announcement(RuntimeOrigin::signed(3), 1, [0; 32].into()),
            e
        );
        assert_ok!(Proxy::remove_announcement(
            RuntimeOrigin::signed(3),
            1,
            [1; 32].into()
        ));
        let announcements = Announcements::<Test>::get(3);
        assert_eq!(
            announcements.0,
            vec![Announcement {
                real: 2,
                call_hash: [2; 32].into(),
                height: 1
            }]
        );
        assert_eq!(Balances::reserved_balance(3), announcements.1);
    });
}

#[test]
fn reject_announcement_works() {
    new_test_ext().execute_with(|| {
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(1),
            3,
            ProxyType::Any,
            1
        ));
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(2),
            3,
            ProxyType::Any,
            1
        ));
        assert_ok!(Proxy::announce(RuntimeOrigin::signed(3), 1, [1; 32].into()));
        assert_ok!(Proxy::announce(RuntimeOrigin::signed(3), 2, [2; 32].into()));
        let e = Error::<Test>::NotFound;
        assert_noop!(
            Proxy::reject_announcement(RuntimeOrigin::signed(1), 3, [0; 32].into()),
            e
        );
        let e = Error::<Test>::NotFound;
        assert_noop!(
            Proxy::reject_announcement(RuntimeOrigin::signed(4), 3, [1; 32].into()),
            e
        );
        assert_ok!(Proxy::reject_announcement(
            RuntimeOrigin::signed(1),
            3,
            [1; 32].into()
        ));
        let announcements = Announcements::<Test>::get(3);
        assert_eq!(
            announcements.0,
            vec![Announcement {
                real: 2,
                call_hash: [2; 32].into(),
                height: 1
            }]
        );
        assert_eq!(Balances::reserved_balance(3), announcements.1);
    });
}

#[test]
fn announcer_must_be_proxy() {
    new_test_ext().execute_with(|| {
        assert_noop!(
            Proxy::announce(RuntimeOrigin::signed(2), 1, H256::zero()),
            Error::<Test>::NotProxy
        );
    });
}

#[test]
fn calling_proxy_doesnt_remove_announcement() {
    new_test_ext().execute_with(|| {
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(1),
            2,
            ProxyType::Any,
            0
        ));

        let call = Box::new(call_transfer(6, 1));
        let call_hash = BlakeTwo256::hash_of(&call);

        assert_ok!(Proxy::announce(RuntimeOrigin::signed(2), 1, call_hash));
        assert_ok!(Proxy::proxy(RuntimeOrigin::signed(2), 1, None, call));

        // The announcement is not removed by calling proxy.
        let announcements = Announcements::<Test>::get(2);
        assert_eq!(
            announcements.0,
            vec![Announcement {
                real: 1,
                call_hash,
                height: 1
            }]
        );
    });
}

#[test]
fn delayed_requires_pre_announcement() {
    new_test_ext().execute_with(|| {
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(1),
            2,
            ProxyType::Any,
            1
        ));
        let call = Box::new(call_transfer(6, 1));
        let e = Error::<Test>::Unannounced;
        assert_noop!(
            Proxy::proxy(RuntimeOrigin::signed(2), 1, None, call.clone()),
            e
        );
        let e = Error::<Test>::Unannounced;
        assert_noop!(
            Proxy::proxy_announced(RuntimeOrigin::signed(0), 2, 1, None, call.clone()),
            e
        );
        let call_hash = BlakeTwo256::hash_of(&call);
        assert_ok!(Proxy::announce(RuntimeOrigin::signed(2), 1, call_hash));
        frame_system::Pallet::<Test>::set_block_number(2);
        assert_ok!(Proxy::proxy_announced(
            RuntimeOrigin::signed(0),
            2,
            1,
            None,
            call.clone()
        ));
    });
}

#[test]
fn proxy_announced_removes_announcement_and_returns_deposit() {
    new_test_ext().execute_with(|| {
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(1),
            3,
            ProxyType::Any,
            1
        ));
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(2),
            3,
            ProxyType::Any,
            1
        ));
        let call = Box::new(call_transfer(6, 1));
        let call_hash = BlakeTwo256::hash_of(&call);
        assert_ok!(Proxy::announce(RuntimeOrigin::signed(3), 1, call_hash));
        assert_ok!(Proxy::announce(RuntimeOrigin::signed(3), 2, call_hash));
        // Too early to execute announced call
        let e = Error::<Test>::Unannounced;
        assert_noop!(
            Proxy::proxy_announced(RuntimeOrigin::signed(0), 3, 1, None, call.clone()),
            e
        );

        frame_system::Pallet::<Test>::set_block_number(2);
        assert_ok!(Proxy::proxy_announced(
            RuntimeOrigin::signed(0),
            3,
            1,
            None,
            call.clone()
        ));
        let announcements = Announcements::<Test>::get(3);
        assert_eq!(
            announcements.0,
            vec![Announcement {
                real: 2,
                call_hash,
                height: 1
            }]
        );
        assert_eq!(Balances::reserved_balance(3), announcements.1);
    });
}
