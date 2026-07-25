//! poke_deposit deposit recomputation and fee-waiver tests.

use super::mock::*;
use crate::*;
use alloc::vec;
use frame::testing_prelude::*;
use pallet_balances::Error as BalancesError;

#[test]
fn poke_deposit_works_for_proxy_deposits() {
    new_test_ext().execute_with(|| {
        // Add a proxy and check initial deposit
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(1),
            2,
            ProxyType::Any,
            0
        ));
        assert_eq!(Balances::reserved_balance(1), 2); // Base(1) + Factor(1) * 1

        // Change the proxy deposit base to trigger deposit update
        ProxyDepositBase::set(2);
        let result = Proxy::poke_deposit(RuntimeOrigin::signed(1));
        assert_ok!(result.as_ref());
        assert_eq!(result.unwrap().pays_fee, Pays::No);
        assert_eq!(Balances::reserved_balance(1), 3); // New Base(2) + Factor(1) * 1
        System::assert_last_event(
            ProxyEvent::DepositPoked {
                who: 1,
                kind: DepositKind::Proxies,
                old_deposit: 2,
                new_deposit: 3,
            }
            .into(),
        );
        assert!(System::events().iter().any(|record| matches!(
            record.event,
            RuntimeEvent::Proxy(Event::DepositPoked { .. })
        )));
    });
}

#[test]
fn poke_deposit_works_for_announcement_deposits() {
    new_test_ext().execute_with(|| {
        // Setup proxy and make announcement
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(1),
            3,
            ProxyType::Any,
            1
        ));
        assert_eq!(Balances::reserved_balance(1), 2); // Base(1) + Factor(1) * 1
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
        let initial_deposit = Balances::reserved_balance(3);

        // Change announcement deposit base to trigger update
        AnnouncementDepositBase::set(2);
        let result = Proxy::poke_deposit(RuntimeOrigin::signed(3));
        assert_ok!(result.as_ref());
        assert_eq!(result.unwrap().pays_fee, Pays::No);
        let new_deposit = initial_deposit.saturating_add(1); // Base increased by 1
        assert_eq!(Balances::reserved_balance(3), new_deposit);
        System::assert_last_event(
            ProxyEvent::DepositPoked {
                who: 3,
                kind: DepositKind::Announcements,
                old_deposit: initial_deposit,
                new_deposit,
            }
            .into(),
        );
        assert!(System::events().iter().any(|record| matches!(
            record.event,
            RuntimeEvent::Proxy(Event::DepositPoked { .. })
        )));
    });
}

#[test]
fn poke_deposit_charges_fee_when_deposit_unchanged() {
    new_test_ext().execute_with(|| {
        // Add a proxy and check initial deposit
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(1),
            3,
            ProxyType::Any,
            0
        ));
        assert_eq!(Balances::reserved_balance(1), 2); // Base(1) + Factor(1) * 1

        // Poke the deposit without changing deposit required and check fee
        let result = Proxy::poke_deposit(RuntimeOrigin::signed(1));
        assert_ok!(result.as_ref());
        assert_eq!(result.unwrap().pays_fee, Pays::Yes); // Pays fee
        assert_eq!(Balances::reserved_balance(1), 2); // No change

        // No event emitted
        assert!(!System::events().iter().any(|record| matches!(
            record.event,
            RuntimeEvent::Proxy(Event::DepositPoked { .. })
        )));

        // Add an announcement and check initial deposit
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
        let initial_deposit = Balances::reserved_balance(3);

        // Poke the deposit without changing deposit required and check fee
        let result = Proxy::poke_deposit(RuntimeOrigin::signed(3));
        assert_ok!(result.as_ref());
        assert_eq!(result.unwrap().pays_fee, Pays::Yes); // Pays fee
        assert_eq!(Balances::reserved_balance(3), initial_deposit); // No change

        // No event emitted
        assert!(!System::events().iter().any(|record| matches!(
            record.event,
            RuntimeEvent::Proxy(Event::DepositPoked { .. })
        )));
    });
}

#[test]
fn poke_deposit_handles_insufficient_balance() {
    new_test_ext().execute_with(|| {
        // Setup with account that has minimal balance
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(5),
            3,
            ProxyType::Any,
            0
        ));
        let initial_deposit = Balances::reserved_balance(5);

        // Change deposit base to require more than available balance
        ProxyDepositBase::set(10);

        // Poking should fail due to insufficient balance
        assert_noop!(
            Proxy::poke_deposit(RuntimeOrigin::signed(5)),
            BalancesError::<Test, _>::InsufficientBalance,
        );

        // Original deposit should remain unchanged
        assert_eq!(Balances::reserved_balance(5), initial_deposit);
    });
}

#[test]
fn poke_deposit_updates_both_proxy_and_announcement_deposits() {
    new_test_ext().execute_with(|| {
        // Setup both proxy and announcement for the same account
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(1),
            2,
            ProxyType::Any,
            0
        ));
        assert_eq!(Balances::reserved_balance(1), 2); // Base(1) + Factor(1) * 1
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(2),
            3,
            ProxyType::Any,
            1
        ));
        assert_eq!(Balances::reserved_balance(2), 2); // Base(1) + Factor(1) * 1
        assert_ok!(Proxy::announce(RuntimeOrigin::signed(2), 1, [1; 32].into()));
        let announcements = Announcements::<Test>::get(2);
        assert_eq!(
            announcements.0,
            vec![Announcement {
                real: 1,
                call_hash: [1; 32].into(),
                height: 1
            }]
        );
        assert_eq!(announcements.1, 2); // Base(1) + Factor(1) * 1

        // Record initial deposits
        let initial_proxy_deposit = Proxies::<Test>::get(2).1;
        let initial_announcement_deposit = Announcements::<Test>::get(2).1;

        // Total reserved = deposit for proxy + deposit for announcement
        assert_eq!(
            Balances::reserved_balance(2),
            initial_proxy_deposit.saturating_add(initial_announcement_deposit)
        );

        // Change both deposit requirements
        ProxyDepositBase::set(2);
        AnnouncementDepositBase::set(2);

        // Poke deposits - should update both deposits and emit two events
        let result = Proxy::poke_deposit(RuntimeOrigin::signed(2));
        assert_ok!(result.as_ref());
        assert_eq!(result.unwrap().pays_fee, Pays::No);

        // Check both deposits were updated
        let (_, new_proxy_deposit) = Proxies::<Test>::get(2);
        let (_, new_announcement_deposit) = Announcements::<Test>::get(2);
        assert_eq!(new_proxy_deposit, 3); // Base(2) + Factor(1) * 1
        assert_eq!(new_announcement_deposit, 3); // Base(2) + Factor(1) * 1
        assert_eq!(
            Balances::reserved_balance(2),
            new_proxy_deposit.saturating_add(new_announcement_deposit)
        );

        // Verify both events were emitted in the correct order
        let events = System::events();
        let relevant_events: Vec<_> = events
            .iter()
            .filter(|record| {
                matches!(
                    record.event,
                    RuntimeEvent::Proxy(ProxyEvent::DepositPoked { .. })
                )
            })
            .collect();

        assert_eq!(relevant_events.len(), 2);

        // First event should be for Proxies
        assert_eq!(
            relevant_events[0].event,
            ProxyEvent::DepositPoked {
                who: 2,
                kind: DepositKind::Proxies,
                old_deposit: initial_proxy_deposit,
                new_deposit: new_proxy_deposit,
            }
            .into()
        );

        // Second event should be for Announcements
        assert_eq!(
            relevant_events[1].event,
            ProxyEvent::DepositPoked {
                who: 2,
                kind: DepositKind::Announcements,
                old_deposit: initial_announcement_deposit,
                new_deposit: new_announcement_deposit,
            }
            .into()
        );

        // Poking again should charge fee as nothing changes
        let result = Proxy::poke_deposit(RuntimeOrigin::signed(2));
        assert_ok!(result.as_ref());
        assert_eq!(result.unwrap().pays_fee, Pays::Yes);

        // Verify deposits remained the same
        assert_eq!(Proxies::<Test>::get(2).1, new_proxy_deposit);
        assert_eq!(Announcements::<Test>::get(2).1, new_announcement_deposit);
        assert_eq!(
            Balances::reserved_balance(2),
            new_proxy_deposit.saturating_add(new_announcement_deposit)
        );
    });
}

#[test]
fn poke_deposit_fails_for_unsigned_origin() {
    new_test_ext().execute_with(|| {
        assert_noop!(
            Proxy::poke_deposit(RuntimeOrigin::none()),
            DispatchError::BadOrigin,
        );
    });
}
