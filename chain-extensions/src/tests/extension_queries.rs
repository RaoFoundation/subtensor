//! Read-only chain-extension queries: stake info, alpha price, registration, locks.

use super::*;

#[test]
fn get_stake_info_returns_encoded_runtime_value() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(1);
        let owner_coldkey = U256::from(2);
        let hotkey = U256::from(11);
        let coldkey = U256::from(22);
        let netuid = mock::add_dynamic_network(&owner_hotkey, &owner_coldkey);
        mock::register_ok_neuron(netuid, hotkey, coldkey, 0);

        let expected =
            pallet_subtensor::Pallet::<mock::Test>::get_stake_info_for_hotkey_coldkey_netuid(
                hotkey, coldkey, netuid,
            )
            .encode();

        let mut env = MockEnv::new(
            FunctionId::GetStakeInfoForHotkeyColdkeyNetuidV1,
            coldkey,
            (hotkey, coldkey, netuid).encode(),
        );

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();

        assert_extension_success(ret);
        assert_eq!(env.output(), expected.as_slice());
        assert!(env.charged_weight().is_none());
    });
}

#[test]
fn get_alpha_price_returns_encoded_price() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(8001);
        let owner_coldkey = U256::from(8002);
        let caller = U256::from(8003);

        let netuid = mock::add_dynamic_network(&owner_hotkey, &owner_coldkey);

        // Set up reserves to establish a price
        let tao_reserve = TaoBalance::from(150_000_000_000u64);
        let alpha_reserve = AlphaBalance::from(100_000_000_000u64);
        mock::setup_reserves(netuid, tao_reserve, alpha_reserve);

        // Get expected price from swap handler
        let expected_price =
            <pallet_subtensor_swap::Pallet<mock::Test> as SwapHandler>::current_alpha_price(
                netuid.into(),
            );
        let expected_price_scaled = expected_price.saturating_mul(U64F64::from_num(1_000_000_000));
        let expected_price_u64: u64 = expected_price_scaled.saturating_to_num();

        let mut env = MockEnv::new(FunctionId::GetAlphaPriceV1, caller, netuid.encode());

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        assert_extension_success(ret);
        assert!(env.charged_weight().is_none());

        // Decode the output
        let output_price: u64 = Decode::decode(&mut &env.output()[..]).unwrap();

        assert_eq!(
            output_price, expected_price_u64,
            "Price should match expected value"
        );
    });
}

#[test]
fn get_subnet_registration_state_returns_existing_subnet_counter() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(8101);
        let owner_coldkey = U256::from(8102);
        let caller = U256::from(8103);

        let netuid = mock::add_dynamic_network(&owner_hotkey, &owner_coldkey);
        let expected_counter =
            pallet_subtensor::Pallet::<mock::Test>::get_registered_subnet_counter(netuid);

        let mut env = MockEnv::new(
            FunctionId::GetSubnetRegistrationStateV1,
            caller,
            netuid.encode(),
        );

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        assert_extension_success(ret);
        assert!(env.charged_weight().is_none());

        let state = SubnetRegistrationState::decode(&mut &env.output()[..]).unwrap();
        assert_eq!(
            state,
            SubnetRegistrationState {
                netuid,
                exists: true,
                registered_subnet_counter: expected_counter,
            }
        );
    });
}

#[test]
fn get_subnet_registration_state_preserves_counter_after_dissolve() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(8201);
        let owner_coldkey = U256::from(8202);
        let caller = U256::from(8203);

        let netuid = mock::add_dynamic_network(&owner_hotkey, &owner_coldkey);
        let registered_counter =
            pallet_subtensor::Pallet::<mock::Test>::get_registered_subnet_counter(netuid);

        assert_ok!(pallet_subtensor::Pallet::<mock::Test>::do_dissolve_network(
            netuid
        ));

        let mut env = MockEnv::new(
            FunctionId::GetSubnetRegistrationStateV1,
            caller,
            netuid.encode(),
        );

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        assert_extension_success(ret);

        let state = SubnetRegistrationState::decode(&mut &env.output()[..]).unwrap();
        assert_eq!(state.netuid, netuid);
        assert!(!state.exists);
        assert_eq!(state.registered_subnet_counter, registered_counter);
    });
}

#[test]
fn get_subnet_registration_state_detects_reused_netuid_generation() {
    mock::new_test_ext(1).execute_with(|| {
        let first_hotkey = U256::from(8301);
        let first_coldkey = U256::from(8302);
        let second_hotkey = U256::from(8303);
        let second_coldkey = U256::from(8304);
        let caller = U256::from(8305);

        let netuid = mock::add_dynamic_network(&first_hotkey, &first_coldkey);
        let first_counter =
            pallet_subtensor::Pallet::<mock::Test>::get_registered_subnet_counter(netuid);

        assert_ok!(pallet_subtensor::Pallet::<mock::Test>::do_dissolve_network(
            netuid
        ));

        pallet_subtensor::Pallet::<mock::Test>::remove_data_for_dissolved_networks(
            Weight::from_parts(u64::MAX, u64::MAX),
        );

        let reused_netuid = mock::add_dynamic_network(&second_hotkey, &second_coldkey);
        assert_eq!(reused_netuid, netuid);

        let mut env = MockEnv::new(
            FunctionId::GetSubnetRegistrationStateV1,
            caller,
            netuid.encode(),
        );

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        assert_extension_success(ret);

        let state = SubnetRegistrationState::decode(&mut &env.output()[..]).unwrap();
        assert_eq!(state.netuid, netuid);
        assert!(state.exists);
        assert!(state.registered_subnet_counter > first_counter);
    });
}

#[test]
fn get_coldkey_lock_returns_none_without_lock() {
    mock::new_test_ext(1).execute_with(|| {
        let caller = U256::from(8401);
        let coldkey = U256::from(8402);
        let netuid = NetUid::from(1);

        let mut env = MockEnv::new(
            FunctionId::GetColdkeyLockV1,
            caller,
            (coldkey, netuid).encode(),
        );

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        assert_extension_success(ret);
        assert!(env.charged_weight().is_none());

        let lock = Option::<ColdkeyLock>::decode(&mut &env.output()[..]).unwrap();
        assert_eq!(lock, None);
    });
}

#[test]
fn get_coldkey_lock_returns_rolled_lock() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(8501);
        let owner_coldkey = U256::from(8502);
        let coldkey = U256::from(8503);
        let hotkey = U256::from(8504);
        let stake = AlphaBalance::from(10_000u64);
        let lock_amount = AlphaBalance::from(5_000u64);

        let netuid = mock::add_dynamic_network(&owner_hotkey, &owner_coldkey);
        mock::register_ok_neuron(netuid, hotkey, coldkey, 0);
        pallet_subtensor::Pallet::<mock::Test>::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey, &coldkey, netuid, stake,
        );

        assert_ok!(pallet_subtensor::Pallet::<mock::Test>::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey,
            lock_amount,
        ));

        frame_system::Pallet::<mock::Test>::set_block_number(1_001);
        let expected =
            pallet_subtensor::Pallet::<mock::Test>::get_coldkey_lock(&coldkey, netuid).unwrap();

        let mut env = MockEnv::new(
            FunctionId::GetColdkeyLockV1,
            coldkey,
            (coldkey, netuid).encode(),
        );

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        assert_extension_success(ret);
        assert!(env.charged_weight().is_none());

        let lock = Option::<ColdkeyLock>::decode(&mut &env.output()[..])
            .unwrap()
            .unwrap();
        assert_eq!(lock.locked_mass, expected.locked_mass);
        assert_eq!(lock.conviction_bits, expected.conviction.to_bits());
        assert!(lock.conviction_bits > 0);
        assert_eq!(
            lock.last_update,
            pallet_subtensor::Pallet::<mock::Test>::get_current_block_as_u64()
        );
    });
}

#[test]
fn get_stake_availability_returns_zeroes_without_stake_or_lock() {
    mock::new_test_ext(1).execute_with(|| {
        let caller = U256::from(8601);
        let coldkey = U256::from(8602);
        let netuid = NetUid::from(1);

        let mut env = MockEnv::new(
            FunctionId::GetStakeAvailabilityV1,
            caller,
            (coldkey, netuid).encode(),
        );

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        assert_extension_success(ret);
        assert!(env.charged_weight().is_none());

        let availability = StakeAvailability::decode(&mut &env.output()[..]).unwrap();
        assert_eq!(
            availability,
            StakeAvailability {
                netuid,
                total: AlphaBalance::ZERO,
                locked: AlphaBalance::ZERO,
                available: AlphaBalance::ZERO,
            }
        );
    });
}

#[test]
fn get_stake_availability_returns_partial_lock_breakdown() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(8701);
        let owner_coldkey = U256::from(8702);
        let coldkey = U256::from(8703);
        let hotkey = U256::from(8704);
        let stake = AlphaBalance::from(10_000u64);
        let lock_amount = AlphaBalance::from(4_000u64);

        let netuid = mock::add_dynamic_network(&owner_hotkey, &owner_coldkey);
        mock::register_ok_neuron(netuid, hotkey, coldkey, 0);
        pallet_subtensor::Pallet::<mock::Test>::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey, &coldkey, netuid, stake,
        );
        assert_ok!(pallet_subtensor::Pallet::<mock::Test>::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey,
            lock_amount,
        ));

        let mut env = MockEnv::new(
            FunctionId::GetStakeAvailabilityV1,
            coldkey,
            (coldkey, netuid).encode(),
        );

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        assert_extension_success(ret);
        assert!(env.charged_weight().is_none());

        let availability = StakeAvailability::decode(&mut &env.output()[..]).unwrap();
        assert_eq!(
            availability,
            StakeAvailability {
                netuid,
                total: stake,
                locked: lock_amount,
                available: stake.saturating_sub(lock_amount),
            }
        );
    });
}

#[test]
fn get_stake_availability_uses_rolled_forward_lock() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(8801);
        let owner_coldkey = U256::from(8802);
        let coldkey = U256::from(8803);
        let hotkey = U256::from(8804);
        let stake = AlphaBalance::from(10_000u64);
        let lock_amount = AlphaBalance::from(5_000u64);

        let netuid = mock::add_dynamic_network(&owner_hotkey, &owner_coldkey);
        mock::register_ok_neuron(netuid, hotkey, coldkey, 0);
        pallet_subtensor::Pallet::<mock::Test>::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey, &coldkey, netuid, stake,
        );
        assert_ok!(pallet_subtensor::Pallet::<mock::Test>::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey,
            lock_amount,
        ));

        frame_system::Pallet::<mock::Test>::set_block_number(1_001);
        let expected_locked =
            pallet_subtensor::Pallet::<mock::Test>::get_current_locked(&coldkey, netuid);
        assert!(expected_locked < lock_amount);

        let mut env = MockEnv::new(
            FunctionId::GetStakeAvailabilityV1,
            coldkey,
            (coldkey, netuid).encode(),
        );

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        assert_extension_success(ret);
        assert!(env.charged_weight().is_none());

        let availability = StakeAvailability::decode(&mut &env.output()[..]).unwrap();
        assert_eq!(availability.netuid, netuid);
        assert_eq!(availability.total, stake);
        assert_eq!(availability.locked, expected_locked);
        assert_eq!(
            availability.available,
            stake.saturating_sub(expected_locked)
        );
    });
}
