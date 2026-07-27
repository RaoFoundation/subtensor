//! Core stake dispatch: add/remove/move/transfer/swap and unstake-all variants.

use super::*;

#[test]
fn swap_stake_success_moves_between_subnets() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey_a = U256::from(4401);
        let owner_coldkey_a = U256::from(4402);
        let owner_hotkey_b = U256::from(4403);
        let owner_coldkey_b = U256::from(4404);
        let coldkey = U256::from(5401);
        let hotkey = U256::from(5402);

        let min_stake = DefaultMinStake::<mock::Test>::get();
        let stake_amount_raw = min_stake.to_u64().saturating_mul(260);

        let netuid_a = mock::add_dynamic_network(&owner_hotkey_a, &owner_coldkey_a);
        let netuid_b = mock::add_dynamic_network(&owner_hotkey_b, &owner_coldkey_b);

        mock::setup_reserves(
            netuid_a,
            stake_amount_raw.saturating_mul(18).into(),
            AlphaBalance::from(stake_amount_raw.saturating_mul(30)),
        );
        mock::setup_reserves(
            netuid_b,
            stake_amount_raw.saturating_mul(20).into(),
            AlphaBalance::from(stake_amount_raw.saturating_mul(28)),
        );

        mock::register_ok_neuron(netuid_a, hotkey, coldkey, 0);
        mock::register_ok_neuron(netuid_b, hotkey, coldkey, 1);

        add_balance_to_coldkey_account(&coldkey, (stake_amount_raw + 1_000_000_000).into());

        assert_ok!(pallet_subtensor::Pallet::<mock::Test>::add_stake(
            RawOrigin::Signed(coldkey).into(),
            hotkey,
            netuid_a,
            stake_amount_raw.into(),
        ));

        let alpha_origin_before =
            pallet_subtensor::Pallet::<mock::Test>::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &coldkey, netuid_a,
            );
        let alpha_destination_before =
            pallet_subtensor::Pallet::<mock::Test>::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &coldkey, netuid_b,
            );
        let alpha_to_swap: AlphaBalance = (alpha_origin_before.to_u64() / 3).into();

        let expected_weight = <<mock::Test as pallet_subtensor::Config>::WeightInfo as SubtensorWeightInfo>::swap_stake();

        let mut env = MockEnv::new(
            FunctionId::SwapStakeV1,
            coldkey,
            (hotkey, netuid_a, netuid_b, alpha_to_swap).encode(),
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

        assert!(alpha_origin_after < alpha_origin_before);
        assert!(
            alpha_destination_after > alpha_destination_before,
            "destination stake should increase"
        );
    });
}

#[test]
fn transfer_stake_success_moves_between_coldkeys() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(4301);
        let owner_coldkey = U256::from(4302);
        let origin_coldkey = U256::from(5301);
        let destination_coldkey = U256::from(5302);
        let hotkey = U256::from(5303);

        let min_stake = DefaultMinStake::<mock::Test>::get();
        let stake_amount_raw = min_stake.to_u64().saturating_mul(250);

        let netuid = mock::add_dynamic_network(&owner_hotkey, &owner_coldkey);
        mock::setup_reserves(
            netuid,
            stake_amount_raw.saturating_mul(15).into(),
            AlphaBalance::from(stake_amount_raw.saturating_mul(25)),
        );

        mock::register_ok_neuron(netuid, hotkey, origin_coldkey, 0);

        add_balance_to_coldkey_account(&origin_coldkey, (stake_amount_raw + 1_000_000_000).into());

        assert_ok!(pallet_subtensor::Pallet::<mock::Test>::add_stake(
            RawOrigin::Signed(origin_coldkey).into(),
            hotkey,
            netuid,
            stake_amount_raw.into(),
        ));

        let alpha_before =
            pallet_subtensor::Pallet::<mock::Test>::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &origin_coldkey,
                netuid,
            );
        let alpha_to_transfer: AlphaBalance = (alpha_before.to_u64() / 3).into();

        let expected_weight = <<mock::Test as pallet_subtensor::Config>::WeightInfo as SubtensorWeightInfo>::transfer_stake();

        let mut env = MockEnv::new(
            FunctionId::TransferStakeV1,
            origin_coldkey,
            (
                destination_coldkey,
                hotkey,
                netuid,
                netuid,
                alpha_to_transfer,
            )
                .encode(),
        )
        .with_expected_weight(expected_weight);

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        assert_extension_success(ret);
        assert_eq!(env.charged_weight(), Some(expected_weight));

        let origin_alpha_after =
            pallet_subtensor::Pallet::<mock::Test>::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &origin_coldkey,
                netuid,
            );
        let destination_alpha_after =
            pallet_subtensor::Pallet::<mock::Test>::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &destination_coldkey,
                netuid,
            );

        assert_eq!(origin_alpha_after, alpha_before - alpha_to_transfer);
        assert_eq!(destination_alpha_after, alpha_to_transfer);
    });
}

#[test]
fn move_stake_success_moves_alpha_between_hotkeys() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(4201);
        let owner_coldkey = U256::from(4202);
        let coldkey = U256::from(5201);
        let origin_hotkey = U256::from(5202);
        let destination_hotkey = U256::from(5203);

        let min_stake = DefaultMinStake::<mock::Test>::get();
        let stake_amount_raw = min_stake.to_u64().saturating_mul(240);

        let netuid = mock::add_dynamic_network(&owner_hotkey, &owner_coldkey);
        mock::setup_reserves(
            netuid,
            stake_amount_raw.saturating_mul(15).into(),
            AlphaBalance::from(stake_amount_raw.saturating_mul(25)),
        );

        mock::register_ok_neuron(netuid, origin_hotkey, coldkey, 0);
        mock::register_ok_neuron(netuid, destination_hotkey, coldkey, 1);

        add_balance_to_coldkey_account(&coldkey, (stake_amount_raw + 1_000_000_000).into());

        assert_ok!(pallet_subtensor::Pallet::<mock::Test>::add_stake(
            RawOrigin::Signed(coldkey).into(),
            origin_hotkey,
            netuid,
            stake_amount_raw.into(),
        ));

        let alpha_before =
            pallet_subtensor::Pallet::<mock::Test>::get_stake_for_hotkey_and_coldkey_on_subnet(
                &origin_hotkey,
                &coldkey,
                netuid,
            );
        let alpha_to_move: AlphaBalance = (alpha_before.to_u64() / 2).into();

        let expected_weight = <<mock::Test as pallet_subtensor::Config>::WeightInfo as SubtensorWeightInfo>::move_stake();

        let mut env = MockEnv::new(
            FunctionId::MoveStakeV1,
            coldkey,
            (
                origin_hotkey,
                destination_hotkey,
                netuid,
                netuid,
                alpha_to_move,
            )
                .encode(),
        )
        .with_expected_weight(expected_weight);

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        assert_extension_success(ret);
        assert_eq!(env.charged_weight(), Some(expected_weight));

        let origin_alpha_after =
            pallet_subtensor::Pallet::<mock::Test>::get_stake_for_hotkey_and_coldkey_on_subnet(
                &origin_hotkey,
                &coldkey,
                netuid,
            );
        let destination_alpha_after =
            pallet_subtensor::Pallet::<mock::Test>::get_stake_for_hotkey_and_coldkey_on_subnet(
                &destination_hotkey,
                &coldkey,
                netuid,
            );

        assert_eq!(origin_alpha_after, alpha_before - alpha_to_move);
        assert_eq!(destination_alpha_after, alpha_to_move);
    });
}

#[test]
fn unstake_all_alpha_success_moves_stake_to_root() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(4101);
        let owner_coldkey = U256::from(4102);
        let coldkey = U256::from(5101);
        let hotkey = U256::from(5102);
        let min_stake = DefaultMinStake::<mock::Test>::get();
        let stake_amount_raw = min_stake.to_u64().saturating_mul(220);
        let netuid = mock::add_dynamic_network(&owner_hotkey, &owner_coldkey);

        mock::setup_reserves(
            netuid,
            stake_amount_raw.saturating_mul(20).into(),
            AlphaBalance::from(stake_amount_raw.saturating_mul(30)),
        );

        mock::register_ok_neuron(netuid, hotkey, coldkey, 0);
        add_balance_to_coldkey_account(&coldkey, (stake_amount_raw + 1_000_000_000).into());

        assert_ok!(pallet_subtensor::Pallet::<mock::Test>::add_stake(
            RawOrigin::Signed(coldkey).into(),
            hotkey,
            netuid,
            stake_amount_raw.into(),
        ));

        let expected_weight = <<mock::Test as pallet_subtensor::Config>::WeightInfo as SubtensorWeightInfo>::unstake_all_alpha();

        let mut env = MockEnv::new(FunctionId::UnstakeAllAlphaV1, coldkey, hotkey.encode())
            .with_expected_weight(expected_weight);

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        assert_extension_success(ret);
        assert_eq!(env.charged_weight(), Some(expected_weight));

        let subnet_alpha =
            pallet_subtensor::Pallet::<mock::Test>::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &coldkey, netuid,
            );
        assert!(subnet_alpha <= AlphaBalance::from(1_000));

        let root_alpha =
            pallet_subtensor::Pallet::<mock::Test>::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &coldkey,
                NetUid::ROOT,
            );
        assert!(root_alpha > AlphaBalance::ZERO);
    });
}

#[test]
fn add_stake_success_updates_stake_and_returns_success_code() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(1);
        let owner_coldkey = U256::from(2);
        let coldkey = U256::from(101);
        let hotkey = U256::from(202);
        let min_stake = DefaultMinStake::<mock::Test>::get();
        let amount_raw = min_stake.to_u64().saturating_mul(10);
        let amount: TaoBalance = amount_raw.into();

        let netuid = mock::add_dynamic_network(&owner_hotkey, &owner_coldkey);
        mock::setup_reserves(
            netuid,
            (amount_raw * 1_000_000).into(),
            AlphaBalance::from(amount_raw * 10_000_000),
        );
        mock::register_ok_neuron(netuid, hotkey, coldkey, 0);

        add_balance_to_coldkey_account(&coldkey, amount_raw.into());

        assert!(
            pallet_subtensor::Pallet::<mock::Test>::get_total_stake_for_hotkey(&hotkey).is_zero()
        );

        let expected_weight = <<mock::Test as pallet_subtensor::Config>::WeightInfo as SubtensorWeightInfo>::add_stake();

        let mut env = MockEnv::new(
            FunctionId::AddStakeV1,
            coldkey,
            (hotkey, netuid, amount).encode(),
        )
        .with_expected_weight(expected_weight);

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();

        assert_extension_success(ret);
        assert_eq!(env.charged_weight(), Some(expected_weight));

        let total_stake =
            pallet_subtensor::Pallet::<mock::Test>::get_total_stake_for_hotkey(&hotkey);
        assert!(total_stake > TaoBalance::ZERO);
    });
}

#[test]
fn remove_stake_with_no_stake_returns_amount_too_low() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(1);
        let owner_coldkey = U256::from(2);
        let coldkey = U256::from(301);
        let hotkey = U256::from(302);
        let netuid = mock::add_dynamic_network(&owner_hotkey, &owner_coldkey);
        mock::register_ok_neuron(netuid, hotkey, coldkey, 0);

        let min_stake = DefaultMinStake::<mock::Test>::get();
        let amount: AlphaBalance = AlphaBalance::from(min_stake.to_u64());

        let expected_weight = <<mock::Test as pallet_subtensor::Config>::WeightInfo as SubtensorWeightInfo>::remove_stake();
        let mut env = MockEnv::new(
            FunctionId::RemoveStakeV1,
            coldkey,
            (hotkey, netuid, amount).encode(),
        )
        .with_expected_weight(expected_weight);

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();

        match ret {
            RetVal::Converging(code) => {
                assert_eq!(code, Output::AmountTooLow as u32, "mismatched error output")
            }
            _ => panic!("unexpected return value"),
        }
        assert_eq!(env.charged_weight(), Some(expected_weight));
        assert!(
            pallet_subtensor::Pallet::<mock::Test>::get_total_stake_for_hotkey(&hotkey).is_zero()
        );
    });
}

#[test]
fn unstake_all_success_unstakes_balance() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(4001);
        let owner_coldkey = U256::from(4002);
        let coldkey = U256::from(5001);
        let hotkey = U256::from(5002);
        let min_stake = DefaultMinStake::<mock::Test>::get();
        let stake_amount_raw = min_stake.to_u64().saturating_mul(200);
        let netuid = mock::add_dynamic_network(&owner_hotkey, &owner_coldkey);

        mock::setup_reserves(
            netuid,
            stake_amount_raw.saturating_mul(10).into(),
            AlphaBalance::from(stake_amount_raw.saturating_mul(20)),
        );

        mock::register_ok_neuron(netuid, hotkey, coldkey, 0);
        add_balance_to_coldkey_account(&coldkey, (stake_amount_raw + 1_000_000_000).into());

        assert_ok!(pallet_subtensor::Pallet::<mock::Test>::add_stake(
            RawOrigin::Signed(coldkey).into(),
            hotkey,
            netuid,
            stake_amount_raw.into(),
        ));

        let expected_weight = <<mock::Test as pallet_subtensor::Config>::WeightInfo as SubtensorWeightInfo>::unstake_all();

        let pre_balance = pallet_subtensor::Pallet::<mock::Test>::get_coldkey_balance(&coldkey);

        let mut env = MockEnv::new(FunctionId::UnstakeAllV1, coldkey, hotkey.encode())
            .with_expected_weight(expected_weight);

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        assert_extension_success(ret);
        assert_eq!(env.charged_weight(), Some(expected_weight));

        let remaining_alpha =
            pallet_subtensor::Pallet::<mock::Test>::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &coldkey, netuid,
            );
        assert!(remaining_alpha <= AlphaBalance::from(1_000));

        let post_balance = pallet_subtensor::Pallet::<mock::Test>::get_coldkey_balance(&coldkey);
        assert!(post_balance > pre_balance);
    });
}
