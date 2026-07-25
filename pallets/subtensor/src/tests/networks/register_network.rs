#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
//! `do_register_network` / PoW register paths, lock cost, owner-alpha seeding.

use super::helpers::*;
use super::prelude::*;

#[test]
fn test_registration_ok() {
    new_test_ext(1).execute_with(|| {
        let block_number: u64 = 0;
        let netuid = NetUid::from(2);
        let tempo: u16 = 13;
        let hotkey_account_id: U256 = U256::from(1);
        let coldkey_account_id: U256 = U256::from(0); // Neighbour of the beast, har har

        add_network(netuid, tempo, 0);

        // Ensure reserves exist for any registration path that might touch swap/burn logic.
        let reserve: u64 = 1_000_000_000_000;
        setup_reserves(
            netuid,
            TaoBalance::from(reserve),
            AlphaBalance::from(reserve),
        );

        // registration economics changed. Ensure the coldkey has enough spendable balance
        add_balance_to_coldkey_account(&coldkey_account_id, TaoBalance::from(reserve));
        add_balance_to_coldkey_account(&hotkey_account_id, TaoBalance::from(reserve));

        let (nonce, work): (u64, Vec<u8>) = SubtensorModule::create_work_for_block_number(
            netuid,
            block_number,
            129123813,
            &hotkey_account_id,
        );

        // PoW register should succeed.
        assert_ok!(SubtensorModule::register(
            <<Test as Config>::RuntimeOrigin>::signed(hotkey_account_id),
            netuid,
            block_number,
            nonce,
            work.clone(),
            hotkey_account_id,
            coldkey_account_id
        ));

        assert_ok!(SubtensorModule::do_dissolve_network(netuid));
        assert!(!SubtensorModule::subnet_exists(netuid));
    })
}

#[test]
fn register_network_skips_dissolved_netuid() {
    new_test_ext(0).execute_with(|| {
        let dissolved = NetUid::from(1);
        DissolveCleanupQueue::<Test>::put(vec![dissolved]);

        let cold = U256::from(60);
        let hot = U256::from(61);
        let needed: u64 = SubtensorModule::get_network_lock_cost().into();
        add_balance_to_coldkey_account(&cold, needed.saturating_mul(10).into());

        assert_ok!(SubtensorModule::do_register_network(
            RuntimeOrigin::signed(cold),
            &hot,
            1,
            None,
        ));

        assert!(!NetworksAdded::<Test>::get(dissolved));
        let expected = NetUid::from(2);
        assert!(NetworksAdded::<Test>::get(expected));
        assert_eq!(SubnetOwner::<Test>::get(expected), cold);
    });
}

#[test]
fn register_network_fails_before_prune_keeps_existing() {
    new_test_ext(0).execute_with(|| {
        SubnetLimit::<Test>::put(1u16);

        let n_cold = U256::from(41);
        let n_hot = U256::from(42);
        let net = add_dynamic_network(&n_hot, &n_cold);

        let imm = SubtensorModule::get_network_immunity_period();
        System::set_block_number(imm + 50);
        Emission::<Test>::insert(net, vec![AlphaBalance::from(10)]);

        let caller_cold = U256::from(50);
        let caller_hot = U256::from(51);

        assert_err!(
            SubtensorModule::do_register_network(
                RuntimeOrigin::signed(caller_cold),
                &caller_hot,
                1,
                None,
            ),
            Error::<Test>::CannotAffordLockCost
        );

        assert!(SubtensorModule::subnet_exists(net));
        assert_eq!(TotalNetworks::<Test>::get(), 1);
    });
}

#[test]
fn test_register_subnet_low_lock_cost() {
    new_test_ext(1).execute_with(|| {
        NetworkMinLockCost::<Test>::set(TaoBalance::from(1_000));
        NetworkLastLockCost::<Test>::set(TaoBalance::from(1_000));

        // Make sure lock cost is lower than 100 TAO
        let lock_cost = SubtensorModule::get_network_lock_cost();
        assert!(lock_cost < 100_000_000_000_u64.into());

        let subnet_owner_coldkey = U256::from(1);
        let subnet_owner_hotkey = U256::from(2);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        assert!(SubtensorModule::subnet_exists(netuid));

        // Ensure that both Subnet TAO and Subnet Alpha In equal to (actual) lock_cost
        assert_eq!(SubnetTAO::<Test>::get(netuid), lock_cost);
        assert_eq!(
            SubnetAlphaIn::<Test>::get(netuid),
            lock_cost.to_u64().into()
        );
    })
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::networks::test_register_subnet_high_lock_cost --exact --show-output --nocapture

#[test]
fn test_register_subnet_high_lock_cost() {
    new_test_ext(1).execute_with(|| {
        let lock_cost = TaoBalance::from(1_000_000_000_000_u64);
        NetworkMinLockCost::<Test>::set(lock_cost);
        NetworkLastLockCost::<Test>::set(lock_cost);

        // Make sure lock cost is higher than 100 TAO
        let lock_cost = SubtensorModule::get_network_lock_cost();
        assert!(lock_cost >= 1_000_000_000_000_u64.into());

        let subnet_owner_coldkey = U256::from(1);
        let subnet_owner_hotkey = U256::from(2);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        assert!(SubtensorModule::subnet_exists(netuid));

        // Ensure that both Subnet TAO and Subnet Alpha In equal to 100 TAO
        assert_eq!(SubnetTAO::<Test>::get(netuid), lock_cost);
        assert_eq!(
            SubnetAlphaIn::<Test>::get(netuid),
            lock_cost.to_u64().into()
        );
    })
}

#[test]
fn register_network_seeds_first_subnet_from_fallback_price_one_and_keeps_lock_in_pool() {
    new_test_ext(1).execute_with(|| {
        let new_cold = U256::from(1001);
        let new_hot = U256::from(1002);
        let new_netuid = SubtensorModule::get_next_netuid();

        let lock_cost_u64: u64 = SubtensorModule::get_network_lock_cost().into();
        let pre_registration_median = SubtensorModule::get_median_subnet_alpha_price();

        let pool_initial_tao = SubtensorModule::get_network_min_lock();
        let pool_initial_tao_u64 = pool_initial_tao.to_u64();
        let total_pool_tao_u64 = lock_cost_u64.max(pool_initial_tao_u64);
        let owner_alpha_tao_equivalent_u64 =
            total_pool_tao_u64.saturating_sub(pool_initial_tao_u64);

        let expected_pool_alpha_u64 =
            owner_alpha_from_lock_and_price(total_pool_tao_u64, pre_registration_median);
        let expected_pool_alpha: AlphaBalance = expected_pool_alpha_u64.into();

        let expected_owner_alpha_u64 = owner_alpha_from_lock_and_price(
            owner_alpha_tao_equivalent_u64,
            pre_registration_median,
        );
        let expected_owner_alpha: AlphaBalance = expected_owner_alpha_u64.into();

        let expected_alpha_issuance: AlphaBalance = expected_pool_alpha_u64
            .saturating_add(expected_owner_alpha_u64)
            .into();

        let expected_recycled: TaoBalance = lock_cost_u64.saturating_sub(total_pool_tao_u64).into();

        assert_eq!(pre_registration_median, U96F32::from_num(1u64));
        assert_eq!(expected_pool_alpha_u64, total_pool_tao_u64);
        assert_eq!(expected_owner_alpha_u64, owner_alpha_tao_equivalent_u64);
        assert_eq!(expected_recycled, TaoBalance::ZERO);

        add_balance_to_coldkey_account(&new_cold, lock_cost_u64.saturating_mul(2).into());

        assert_ok!(SubtensorModule::do_register_network(
            RuntimeOrigin::signed(new_cold),
            &new_hot,
            1,
            None,
        ));

        assert!(SubtensorModule::subnet_exists(new_netuid));
        assert_eq!(TotalNetworks::<Test>::get(), 1);
        assert_eq!(SubnetOwner::<Test>::get(new_netuid), new_cold);
        assert_eq!(SubnetOwnerHotkey::<Test>::get(new_netuid), new_hot);
        assert_eq!(
            SubtensorModule::get_subnet_locked_balance(new_netuid),
            TaoBalance::from(lock_cost_u64)
        );

        assert_eq!(
            SubnetTAO::<Test>::get(new_netuid),
            TaoBalance::from(total_pool_tao_u64)
        );
        assert_eq!(SubnetAlphaIn::<Test>::get(new_netuid), expected_pool_alpha);
        assert_eq!(
            SubnetAlphaOut::<Test>::get(new_netuid),
            expected_owner_alpha
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &new_hot, &new_cold, new_netuid,
            ),
            expected_owner_alpha
        );
        assert_eq!(
            TotalHotkeyAlpha::<Test>::get(new_hot, new_netuid),
            expected_owner_alpha
        );
        assert_eq!(
            SubtensorModule::get_alpha_issuance(new_netuid),
            expected_alpha_issuance
        );
        assert_eq!(
            RAORecycledForRegistration::<Test>::get(new_netuid),
            expected_recycled
        );

        assert_eq!(
            <Test as pallet::Config>::SwapInterface::current_alpha_price(new_netuid.into()),
            U96F32::from_num(1u64)
        );

        System::assert_last_event(Event::NetworkAdded(new_netuid, 1).into());
    });
}

#[test]
fn register_network_seeds_new_subnet_from_even_median_snapshot() {
    new_test_ext(0).execute_with(|| {
        let n1 = add_dynamic_network(&U256::from(1201), &U256::from(1200));
        let n2 = add_dynamic_network(&U256::from(1203), &U256::from(1202));

        // Existing prices are {5, 2} -> pre-registration median is 3.5.
        setup_reserves(n1, TaoBalance::from(500u64), AlphaBalance::from(100u64));
        setup_reserves(n2, TaoBalance::from(200u64), AlphaBalance::from(100u64));

        let pre_registration_median = SubtensorModule::get_median_subnet_alpha_price();
        assert_eq!(pre_registration_median, U96F32::from_num(3.5));

        let new_cold = U256::from(1300);
        let new_hot = U256::from(1301);
        let new_netuid = SubtensorModule::get_next_netuid();

        let lock_cost_u64: u64 = SubtensorModule::get_network_lock_cost().into();
        let pool_initial_tao_u64 = SubtensorModule::get_network_min_lock().to_u64();
        let total_pool_tao_u64 = lock_cost_u64.max(pool_initial_tao_u64);
        let owner_alpha_tao_equivalent_u64 =
            total_pool_tao_u64.saturating_sub(pool_initial_tao_u64);

        let expected_pool_alpha_u64 =
            owner_alpha_from_lock_and_price(total_pool_tao_u64, pre_registration_median);
        let expected_pool_alpha: AlphaBalance = expected_pool_alpha_u64.into();

        let expected_owner_alpha_u64 = owner_alpha_from_lock_and_price(
            owner_alpha_tao_equivalent_u64,
            pre_registration_median,
        );
        let expected_owner_alpha: AlphaBalance = expected_owner_alpha_u64.into();

        add_balance_to_coldkey_account(&new_cold, lock_cost_u64.saturating_mul(2).into());

        assert_ok!(SubtensorModule::do_register_network(
            RuntimeOrigin::signed(new_cold),
            &new_hot,
            1,
            None,
        ));

        let new_subnet_price =
            <Test as pallet::Config>::SwapInterface::current_alpha_price(new_netuid.into());
        let post_registration_median = SubtensorModule::get_median_subnet_alpha_price();

        assert!(SubtensorModule::subnet_exists(new_netuid));
        assert_eq!(SubnetOwner::<Test>::get(new_netuid), new_cold);
        assert_eq!(SubnetOwnerHotkey::<Test>::get(new_netuid), new_hot);
        assert_eq!(
            SubtensorModule::get_subnet_locked_balance(new_netuid),
            TaoBalance::from(lock_cost_u64)
        );

        assert_eq!(
            SubnetTAO::<Test>::get(new_netuid),
            TaoBalance::from(total_pool_tao_u64)
        );
        assert_eq!(SubnetAlphaIn::<Test>::get(new_netuid), expected_pool_alpha);
        assert_eq!(
            SubnetAlphaOut::<Test>::get(new_netuid),
            expected_owner_alpha
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &new_hot, &new_cold, new_netuid,
            ),
            expected_owner_alpha
        );
        assert_eq!(
            TotalHotkeyAlpha::<Test>::get(new_hot, new_netuid),
            expected_owner_alpha
        );

        // The new subnet is seeded from the pre-registration median snapshot,
        // so it is no longer initialized at the old 1:1 seed price.
        assert_ne!(new_subnet_price, U96F32::from_num(1u64));
        assert!(new_subnet_price >= pre_registration_median);

        // With prices {2, seeded_price, 5}, the live median becomes the new subnet price.
        assert_eq!(post_registration_median, new_subnet_price);

        // A 1:1 seed would have alpha_in == tao_in, which should not happen here.
        let wrong_price_one_pool_alpha: AlphaBalance = total_pool_tao_u64.into();
        assert_ne!(
            SubnetAlphaIn::<Test>::get(new_netuid),
            wrong_price_one_pool_alpha
        );
    });
}

#[test]
fn register_network_fails_without_balance_and_does_not_write_owner_alpha_state() {
    new_test_ext(0).execute_with(|| {
        let cold = U256::from(2001);
        let hot = U256::from(2002);
        let would_be_netuid = SubtensorModule::get_next_netuid();

        assert_eq!(
            SubtensorModule::get_coldkey_balance(&cold),
            TaoBalance::ZERO
        );

        assert_err!(
            SubtensorModule::do_register_network(RuntimeOrigin::signed(cold), &hot, 1, None,),
            Error::<Test>::CannotAffordLockCost
        );

        assert!(!SubtensorModule::subnet_exists(would_be_netuid));
        assert_eq!(TotalNetworks::<Test>::get(), 0);
        assert_eq!(
            SubnetAlphaIn::<Test>::get(would_be_netuid),
            AlphaBalance::ZERO
        );
        assert_eq!(
            SubnetAlphaOut::<Test>::get(would_be_netuid),
            AlphaBalance::ZERO
        );
        assert_eq!(
            SubtensorModule::get_subnet_locked_balance(would_be_netuid),
            TaoBalance::ZERO
        );
        assert_eq!(
            RAORecycledForRegistration::<Test>::get(would_be_netuid),
            TaoBalance::ZERO
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hot,
                &cold,
                would_be_netuid,
            ),
            AlphaBalance::ZERO
        );
    });
}

#[test]
fn register_network_non_associated_hotkey_does_not_withdraw_or_write_owner_alpha_state() {
    new_test_ext(0).execute_with(|| {
        let original_cold = U256::from(3001);
        let shared_hot = U256::from(3002);
        let existing_netuid = add_dynamic_network(&shared_hot, &original_cold);

        let original_stake_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &shared_hot,
            &original_cold,
            existing_netuid,
        );
        let original_alpha_out_before = SubnetAlphaOut::<Test>::get(existing_netuid);

        let attacker_cold = U256::from(3003);
        let would_be_netuid = SubtensorModule::get_next_netuid();
        let lock_cost_u64: u64 = SubtensorModule::get_network_lock_cost().into();

        add_balance_to_coldkey_account(&attacker_cold, lock_cost_u64.into());
        let attacker_balance_before = SubtensorModule::get_coldkey_balance(&attacker_cold);

        assert_err!(
            SubtensorModule::do_register_network(
                RuntimeOrigin::signed(attacker_cold),
                &shared_hot,
                1,
                None,
            ),
            Error::<Test>::NonAssociatedColdKey
        );

        // Attacker was not charged.
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&attacker_cold),
            attacker_balance_before
        );

        // Existing owner state is untouched.
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &shared_hot,
                &original_cold,
                existing_netuid,
            ),
            original_stake_before
        );
        assert_eq!(
            SubnetAlphaOut::<Test>::get(existing_netuid),
            original_alpha_out_before
        );
        assert_eq!(SubnetOwner::<Test>::get(existing_netuid), original_cold);

        // No new subnet / owner-alpha state was written.
        assert!(!SubtensorModule::subnet_exists(would_be_netuid));
        assert_eq!(TotalNetworks::<Test>::get(), 1);
        assert_eq!(
            SubnetAlphaOut::<Test>::get(would_be_netuid),
            AlphaBalance::ZERO
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &shared_hot,
                &attacker_cold,
                would_be_netuid,
            ),
            AlphaBalance::ZERO
        );
    });
}
