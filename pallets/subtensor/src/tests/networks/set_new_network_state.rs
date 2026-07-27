#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
//! `set_new_network_state` pool seeding, identity, limit, and fund-locked paths.

use super::prelude::*;

#[test]
fn set_new_network_state_registers_subnet_with_expected_state() {
    new_test_ext(1).execute_with(|| {
        let cold = U256::from(9001);
        let hot = U256::from(9002);
        let lock_amount = SubtensorModule::get_network_lock_cost();
        add_balance_to_coldkey_account(&cold, lock_amount.saturating_mul(2.into()).into());
        TotalIssuance::<Test>::mutate(|total| *total = total.saturating_add(lock_amount));

        let median_price = SubtensorModule::get_median_subnet_alpha_price();
        let netuid = SubtensorModule::get_next_netuid();

        assert_ok!(SubtensorModule::set_new_network_state(
            &cold,
            &hot,
            1,
            None,
            lock_amount,
            median_price,
            None,
        ));

        assert!(SubtensorModule::subnet_exists(netuid));
        assert_eq!(SubnetOwner::<Test>::get(netuid), cold);
        assert_eq!(SubnetMechanism::<Test>::get(netuid), 1);
        assert_eq!(SubnetLocked::<Test>::get(netuid), lock_amount);
        assert_eq!(
            SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hot),
            Ok(0)
        );
    });
}

#[test]
fn set_new_network_state_fails_when_subnet_limit_reached() {
    new_test_ext(1).execute_with(|| {
        SubnetLimit::<Test>::put(1u16);
        let _n1 = add_dynamic_network(&U256::from(10_002), &U256::from(10_001));

        let cold = U256::from(10_011);
        let hot = U256::from(10_012);
        let lock_amount = SubtensorModule::get_network_lock_cost();
        add_balance_to_coldkey_account(&cold, lock_amount.saturating_mul(2.into()).into());

        assert_err!(
            SubtensorModule::set_new_network_state(
                &cold,
                &hot,
                1,
                None,
                lock_amount,
                SubtensorModule::get_median_subnet_alpha_price(),
                None,
            ),
            Error::<Test>::SubnetLimitReached
        );

        // No partial state was written.
        assert_eq!(TotalNetworks::<Test>::get(), 1);
        assert!(!SubtensorModule::hotkey_account_exists(&hot));
    });
}

#[test]
fn set_new_network_state_stores_identity_and_emits_events() {
    new_test_ext(1).execute_with(|| {
        let cold = U256::from(10_101);
        let hot = U256::from(10_102);
        let lock_amount = SubtensorModule::get_network_lock_cost();
        add_balance_to_coldkey_account(&cold, lock_amount.saturating_mul(2.into()).into());

        let identity = SubnetIdentityOfV3 {
            subnet_name: b"my subnet".to_vec(),
            github_repo: b"https://github.com/example/repo".to_vec(),
            subnet_contact: b"contact@example.com".to_vec(),
            subnet_url: b"https://example.com".to_vec(),
            discord: b"discord".to_vec(),
            description: b"description".to_vec(),
            logo_url: b"https://example.com/logo.png".to_vec(),
            additional: b"".to_vec(),
        };

        let netuid = SubtensorModule::get_next_netuid();
        System::reset_events();

        assert_ok!(SubtensorModule::set_new_network_state(
            &cold,
            &hot,
            1,
            Some(identity.clone()),
            lock_amount,
            SubtensorModule::get_median_subnet_alpha_price(),
            None,
        ));

        assert_eq!(SubnetIdentitiesV3::<Test>::get(netuid), Some(identity));
        let events = System::events();
        assert!(events.iter().any(|e| matches!(
            &e.event,
            RuntimeEvent::SubtensorModule(Event::SubnetIdentitySet(n)) if *n == netuid
        )));
        assert!(events.iter().any(|e| matches!(
            &e.event,
            RuntimeEvent::SubtensorModule(Event::NetworkAdded(n, m)) if *n == netuid && *m == 1
        )));
    });
}

#[test]
fn set_new_network_state_uses_provided_median_price_for_pool_alpha() {
    new_test_ext(1).execute_with(|| {
        let cold = U256::from(10_201);
        let hot = U256::from(10_202);

        // Lock twice the min lock so the pool is seeded from the actual lock amount.
        let min_lock = SubtensorModule::get_network_min_lock();
        let lock_amount = min_lock.saturating_mul(2.into());
        add_balance_to_coldkey_account(&cold, lock_amount.saturating_mul(2.into()).into());

        let netuid = SubtensorModule::get_next_netuid();
        let price = U64F64::from_num(2);

        assert_ok!(SubtensorModule::set_new_network_state(
            &cold,
            &hot,
            1,
            None,
            lock_amount,
            price,
            None,
        ));

        // Pool TAO equals the actual lock; alpha reserve is tao / price.
        assert_eq!(SubnetTAO::<Test>::get(netuid), lock_amount);
        let expected_alpha: u64 = u64::from(lock_amount) / 2;
        assert_eq!(
            SubnetAlphaIn::<Test>::get(netuid),
            AlphaBalance::from(expected_alpha)
        );
    });
}

#[test]
fn set_new_network_state_seeds_pool_with_min_lock_floor() {
    new_test_ext(1).execute_with(|| {
        let cold = U256::from(10_301);
        let hot = U256::from(10_302);
        add_balance_to_coldkey_account(&cold, 1_000_000_000.into());

        let netuid = SubtensorModule::get_next_netuid();
        let min_lock = SubtensorModule::get_network_min_lock();

        // Zero lock: the pool must still be seeded with the min lock floor.
        assert_ok!(SubtensorModule::set_new_network_state(
            &cold,
            &hot,
            1,
            None,
            TaoBalance::ZERO,
            U64F64::from_num(1),
            None,
        ));

        assert_eq!(SubnetTAO::<Test>::get(netuid), min_lock);
        assert_eq!(
            SubnetAlphaIn::<Test>::get(netuid),
            AlphaBalance::from(u64::from(min_lock))
        );
        assert_eq!(SubnetLocked::<Test>::get(netuid), TaoBalance::ZERO);
    });
}

#[test]
fn set_new_network_state_fund_locked_releases_balance_lock() {
    new_test_ext(1).execute_with(|| {
        let cold = U256::from(10_401);
        let hot = U256::from(10_402);
        let lock_amount = SubtensorModule::get_network_lock_cost();
        add_balance_to_coldkey_account(&cold, lock_amount.saturating_mul(2.into()).into());

        let lock_id = NetworkRegistrationLockId::<Test>::get();
        let mut identifier = [0u8; 8];
        identifier[..4].copy_from_slice(b"rglk");
        identifier[4..8].copy_from_slice(&lock_id.to_le_bytes());

        assert_ok!(SubtensorModule::lock_network_registration_cost(
            &cold,
            lock_amount.into(),
            0
        ));
        assert!(
            pallet_balances::Locks::<Test>::get(cold)
                .iter()
                .any(|l| l.id == identifier),
            "registration lock must exist before processing"
        );

        let netuid = SubtensorModule::get_next_netuid();

        assert_ok!(SubtensorModule::set_new_network_state(
            &cold,
            &hot,
            1,
            None,
            lock_amount,
            SubtensorModule::get_median_subnet_alpha_price(),
            Some(lock_id),
        ));

        assert!(
            pallet_balances::Locks::<Test>::get(cold)
                .iter()
                .all(|l| l.id != identifier),
            "registration lock must be released after processing"
        );
        assert!(SubtensorModule::subnet_exists(netuid));
        assert_eq!(SubnetLocked::<Test>::get(netuid), lock_amount);
    });
}
