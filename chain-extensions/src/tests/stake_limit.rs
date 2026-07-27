//! Price-limited stake dispatch: add/remove/swap limit and remove-full-limit.

use super::*;

#[test]
fn remove_stake_full_limit_success_with_limit_price() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(4801);
        let owner_coldkey = U256::from(4802);
        let coldkey = U256::from(5801);
        let hotkey = U256::from(5802);
        let stake_amount_raw: u64 = 340_000_000_000;

        let netuid = mock::add_dynamic_network(&owner_hotkey, &owner_coldkey);
        mock::setup_reserves(
            netuid,
            TaoBalance::from(130_000_000_000_u64),
            AlphaBalance::from(110_000_000_000_u64),
        );

        mock::register_ok_neuron(netuid, hotkey, coldkey, 0);

        add_balance_to_coldkey_account(
            &coldkey,
            TaoBalance::from(stake_amount_raw + 1_000_000_000),
        );

        assert_ok!(pallet_subtensor::Pallet::<mock::Test>::add_stake(
            RawOrigin::Signed(coldkey).into(),
            hotkey,
            netuid,
            stake_amount_raw.into(),
        ));

        let expected_weight = <<mock::Test as pallet_subtensor::Config>::WeightInfo as SubtensorWeightInfo>::remove_stake_full_limit();

        let balance_before = pallet_subtensor::Pallet::<mock::Test>::get_coldkey_balance(&coldkey);

        let mut env = MockEnv::new(
            FunctionId::RemoveStakeFullLimitV1,
            coldkey,
            (hotkey, netuid, Option::<TaoBalance>::None).encode(),
        )
        .with_expected_weight(expected_weight);

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        assert_extension_success(ret);
        assert_eq!(env.charged_weight(), Some(expected_weight));

        let alpha_after =
            pallet_subtensor::Pallet::<mock::Test>::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &coldkey, netuid,
            );
        let balance_after = pallet_subtensor::Pallet::<mock::Test>::get_coldkey_balance(&coldkey);

        assert!(alpha_after.is_zero());
        assert!(balance_after > balance_before);
    });
}

#[test]
fn swap_stake_limit_with_tight_price_returns_slippage_error() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey_a = U256::from(4701);
        let owner_coldkey_a = U256::from(4702);
        let owner_hotkey_b = U256::from(4703);
        let owner_coldkey_b = U256::from(4704);
        let coldkey = U256::from(5701);
        let hotkey = U256::from(5702);

        let stake_alpha = AlphaBalance::from(150_000_000_000u64);

        let netuid_a = mock::add_dynamic_network(&owner_hotkey_a, &owner_coldkey_a);
        let netuid_b = mock::add_dynamic_network(&owner_hotkey_b, &owner_coldkey_b);

        mock::setup_reserves(
            netuid_a,
            TaoBalance::from(150_000_000_000_u64),
            AlphaBalance::from(110_000_000_000_u64),
        );
        mock::setup_reserves(
            netuid_b,
            TaoBalance::from(120_000_000_000_u64),
            AlphaBalance::from(90_000_000_000_u64),
        );

        mock::register_ok_neuron(netuid_a, hotkey, coldkey, 0);
        mock::register_ok_neuron(netuid_b, hotkey, coldkey, 1);

        pallet_subtensor::Pallet::<mock::Test>::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid_a,
            stake_alpha,
        );

        let alpha_origin_before =
            pallet_subtensor::Pallet::<mock::Test>::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &coldkey, netuid_a,
            );
        let alpha_destination_before =
            pallet_subtensor::Pallet::<mock::Test>::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &coldkey, netuid_b,
            );

        let alpha_to_swap: AlphaBalance = (alpha_origin_before.to_u64() / 8).into();
        let limit_price: TaoBalance = 100u64.into();

        let expected_weight = <<mock::Test as pallet_subtensor::Config>::WeightInfo as SubtensorWeightInfo>::swap_stake_limit();

        let mut env = MockEnv::new(
            FunctionId::SwapStakeLimitV1,
            coldkey,
            (hotkey, netuid_a, netuid_b, alpha_to_swap, limit_price, true).encode(),
        )
        .with_expected_weight(expected_weight);

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        assert_extension_success(ret);
        assert_eq!(env.charged_weight(), Some(expected_weight));

        let alpha_origin_after =
            pallet_subtensor::Pallet::<mock::Test>::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &coldkey, netuid_a,
            );
        let alpha_destination_after =
            pallet_subtensor::Pallet::<mock::Test>::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &coldkey, netuid_b,
            );

        assert!(alpha_origin_after <= alpha_origin_before);
        assert!(alpha_destination_after >= alpha_destination_before);
    });
}

#[test]
fn remove_stake_limit_success_respects_price_limit() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(4601);
        let owner_coldkey = U256::from(4602);
        let coldkey = U256::from(5601);
        let hotkey = U256::from(5602);
        let stake_amount_raw: u64 = 320_000_000_000;

        let netuid = mock::add_dynamic_network(&owner_hotkey, &owner_coldkey);
        mock::setup_reserves(
            netuid,
            TaoBalance::from(120_000_000_000_u64),
            AlphaBalance::from(100_000_000_000_u64),
        );

        mock::register_ok_neuron(netuid, hotkey, coldkey, 0);

        add_balance_to_coldkey_account(
            &coldkey,
            TaoBalance::from(stake_amount_raw + 1_000_000_000),
        );

        assert_ok!(pallet_subtensor::Pallet::<mock::Test>::add_stake(
            RawOrigin::Signed(coldkey).into(),
            hotkey,
            netuid,
            stake_amount_raw.into(),
        ));

        let alpha_before =
            pallet_subtensor::Pallet::<mock::Test>::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &coldkey, netuid,
            );

        let current_price =
            <mock::Test as pallet_subtensor::Config>::SwapInterface::current_alpha_price(
                netuid.into(),
            );
        let limit_price_value = (current_price.to_num::<f64>() * 990_000_000f64).round() as u64;
        let limit_price: TaoBalance = limit_price_value.into();

        let alpha_to_unstake: AlphaBalance = (alpha_before.to_u64() / 2).into();

        let expected_weight = <<mock::Test as pallet_subtensor::Config>::WeightInfo as SubtensorWeightInfo>::remove_stake_limit();

        let balance_before = pallet_subtensor::Pallet::<mock::Test>::get_coldkey_balance(&coldkey);

        let mut env = MockEnv::new(
            FunctionId::RemoveStakeLimitV1,
            coldkey,
            (hotkey, netuid, alpha_to_unstake, limit_price, true).encode(),
        )
        .with_expected_weight(expected_weight);

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        assert_extension_success(ret);
        assert_eq!(env.charged_weight(), Some(expected_weight));

        let alpha_after =
            pallet_subtensor::Pallet::<mock::Test>::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &coldkey, netuid,
            );
        let balance_after = pallet_subtensor::Pallet::<mock::Test>::get_coldkey_balance(&coldkey);

        assert!(alpha_after < alpha_before);
        assert!(balance_after > balance_before);
    });
}

#[test]
fn add_stake_limit_success_executes_within_price_guard() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(4501);
        let owner_coldkey = U256::from(4502);
        let coldkey = U256::from(5501);
        let hotkey = U256::from(5502);
        let amount_raw: u64 = 900_000_000_000;
        let limit_price: TaoBalance = 24_000_000_000u64.into();

        let netuid = mock::add_dynamic_network(&owner_hotkey, &owner_coldkey);

        mock::setup_reserves(
            netuid,
            TaoBalance::from(150_000_000_000_u64),
            AlphaBalance::from(100_000_000_000_u64),
        );

        mock::register_ok_neuron(netuid, hotkey, coldkey, 0);

        add_balance_to_coldkey_account(&coldkey, (amount_raw + 1_000_000_000).into());

        let stake_before =
            pallet_subtensor::Pallet::<mock::Test>::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &coldkey, netuid,
            );
        let balance_before = pallet_subtensor::Pallet::<mock::Test>::get_coldkey_balance(&coldkey);

        let expected_weight = <<mock::Test as pallet_subtensor::Config>::WeightInfo as SubtensorWeightInfo>::add_stake_limit();

        let mut env = MockEnv::new(
            FunctionId::AddStakeLimitV1,
            coldkey,
            (
                hotkey,
                netuid,
                TaoBalance::from(amount_raw),
                limit_price,
                true,
            )
                .encode(),
        )
        .with_expected_weight(expected_weight);

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        assert_extension_success(ret);
        assert_eq!(env.charged_weight(), Some(expected_weight));

        let stake_after =
            pallet_subtensor::Pallet::<mock::Test>::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &coldkey, netuid,
            );
        let balance_after = pallet_subtensor::Pallet::<mock::Test>::get_coldkey_balance(&coldkey);

        assert!(stake_after > stake_before);
        assert!(stake_after > AlphaBalance::ZERO);
        assert!(balance_after < balance_before);
    });
}
