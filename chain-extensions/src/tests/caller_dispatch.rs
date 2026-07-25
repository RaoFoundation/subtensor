//! `Caller*` function ids resolve origin via `contracts_origin_as_raw(env.origin())`.
//!
//! With [`MockEnv`] both `caller()` and `origin()` are `Signed(caller)`, so outcomes align
//! with non-`Caller` arms. Weight expectations match the shared `dispatch_*_v1` helpers.

use super::*;

#[test]
fn caller_add_stake_success_updates_stake_and_returns_success_code() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(1);
        let owner_coldkey = U256::from(2);
        let coldkey = U256::from(10101);
        let hotkey = U256::from(10202);
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

        mock::add_balance_to_coldkey_account(
            &coldkey,
            amount_raw.into(),
        );

        let expected_weight = <<mock::Test as pallet_subtensor::Config>::WeightInfo as SubtensorWeightInfo>::add_stake();

        let mut env = MockEnv::new(
            FunctionId::CallerAddStakeV1,
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
fn caller_remove_stake_with_no_stake_returns_amount_too_low() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(1);
        let owner_coldkey = U256::from(2);
        let coldkey = U256::from(30301);
        let hotkey = U256::from(30302);
        let netuid = mock::add_dynamic_network(&owner_hotkey, &owner_coldkey);
        mock::register_ok_neuron(netuid, hotkey, coldkey, 0);

        let min_stake = DefaultMinStake::<mock::Test>::get();
        let amount: AlphaBalance = AlphaBalance::from(min_stake.to_u64());

        let expected_weight = <<mock::Test as pallet_subtensor::Config>::WeightInfo as SubtensorWeightInfo>::remove_stake();
        let mut env = MockEnv::new(
            FunctionId::CallerRemoveStakeV1,
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
    });
}

#[test]
fn caller_unstake_all_success_unstakes_balance() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(40001);
        let owner_coldkey = U256::from(40002);
        let coldkey = U256::from(50001);
        let hotkey = U256::from(50002);
        let min_stake = DefaultMinStake::<mock::Test>::get();
        let stake_amount_raw = min_stake.to_u64().saturating_mul(200);
        let netuid = mock::add_dynamic_network(&owner_hotkey, &owner_coldkey);

        mock::setup_reserves(
            netuid,
            stake_amount_raw.saturating_mul(10).into(),
            AlphaBalance::from(stake_amount_raw.saturating_mul(20)),
        );

        mock::register_ok_neuron(netuid, hotkey, coldkey, 0);
        mock::add_balance_to_coldkey_account(
            &coldkey,
            (stake_amount_raw + 1_000_000_000).into(),
        );

        assert_ok!(pallet_subtensor::Pallet::<mock::Test>::add_stake(
            RawOrigin::Signed(coldkey).into(),
            hotkey,
            netuid,
            stake_amount_raw.into(),
        ));

        let expected_weight = <<mock::Test as pallet_subtensor::Config>::WeightInfo as SubtensorWeightInfo>::unstake_all();

        let pre_balance = pallet_subtensor::Pallet::<mock::Test>::get_coldkey_balance(&coldkey);

        let mut env = MockEnv::new(FunctionId::CallerUnstakeAllV1, coldkey, hotkey.encode())
            .with_expected_weight(expected_weight);

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        assert_extension_success(ret);

        let remaining_alpha =
            pallet_subtensor::Pallet::<mock::Test>::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &coldkey, netuid,
            );
        assert!(remaining_alpha <= AlphaBalance::from(1_000));

        let post_balance =
            pallet_subtensor::Pallet::<mock::Test>::get_coldkey_balance(&coldkey);
        assert!(post_balance > pre_balance);
    });
}

#[test]
fn caller_unstake_all_alpha_success_moves_stake_to_root() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(41001);
        let owner_coldkey = U256::from(41002);
        let coldkey = U256::from(51001);
        let hotkey = U256::from(51002);
        let min_stake = DefaultMinStake::<mock::Test>::get();
        let stake_amount_raw = min_stake.to_u64().saturating_mul(220);
        let netuid = mock::add_dynamic_network(&owner_hotkey, &owner_coldkey);

        mock::setup_reserves(
            netuid,
            stake_amount_raw.saturating_mul(20).into(),
            AlphaBalance::from(stake_amount_raw.saturating_mul(30)),
        );

        mock::register_ok_neuron(netuid, hotkey, coldkey, 0);
        mock::add_balance_to_coldkey_account(
            &coldkey,
            (stake_amount_raw + 1_000_000_000).into(),
        );

        assert_ok!(pallet_subtensor::Pallet::<mock::Test>::add_stake(
            RawOrigin::Signed(coldkey).into(),
            hotkey,
            netuid,
            stake_amount_raw.into(),
        ));

        let expected_weight = <<mock::Test as pallet_subtensor::Config>::WeightInfo as SubtensorWeightInfo>::unstake_all_alpha();

        let mut env = MockEnv::new(
            FunctionId::CallerUnstakeAllAlphaV1,
            coldkey,
            hotkey.encode(),
        )
        .with_expected_weight(expected_weight);

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        assert_extension_success(ret);

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
fn caller_move_stake_success_moves_alpha_between_hotkeys() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(42001);
        let owner_coldkey = U256::from(42002);
        let coldkey = U256::from(52001);
        let origin_hotkey = U256::from(52002);
        let destination_hotkey = U256::from(52003);

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

        mock::add_balance_to_coldkey_account(
            &coldkey,
            (stake_amount_raw + 1_000_000_000).into(),
        );

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
            FunctionId::CallerMoveStakeV1,
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
fn caller_transfer_stake_success_moves_between_coldkeys() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(43001);
        let owner_coldkey = U256::from(43002);
        let origin_coldkey = U256::from(53001);
        let destination_coldkey = U256::from(53002);
        let hotkey = U256::from(53003);

        let min_stake = DefaultMinStake::<mock::Test>::get();
        let stake_amount_raw = min_stake.to_u64().saturating_mul(250);

        let netuid = mock::add_dynamic_network(&owner_hotkey, &owner_coldkey);
        mock::setup_reserves(
            netuid,
            stake_amount_raw.saturating_mul(15).into(),
            AlphaBalance::from(stake_amount_raw.saturating_mul(25)),
        );

        mock::register_ok_neuron(netuid, hotkey, origin_coldkey, 0);

        mock::add_balance_to_coldkey_account(
            &origin_coldkey,
            (stake_amount_raw + 1_000_000_000).into(),
        );

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
            FunctionId::CallerTransferStakeV1,
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
fn caller_swap_stake_success_moves_between_subnets() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey_a = U256::from(44001);
        let owner_coldkey_a = U256::from(44002);
        let owner_hotkey_b = U256::from(44003);
        let owner_coldkey_b = U256::from(44004);
        let coldkey = U256::from(54001);
        let hotkey = U256::from(54002);

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

        mock::add_balance_to_coldkey_account(
            &coldkey,
            (stake_amount_raw + 1_000_000_000).into(),
        );

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
            FunctionId::CallerSwapStakeV1,
            coldkey,
            (hotkey, netuid_a, netuid_b, alpha_to_swap).encode(),
        )
        .with_expected_weight(expected_weight);

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        assert_extension_success(ret);

        let alpha_origin_after =
            pallet_subtensor::Pallet::<mock::Test>::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &coldkey, netuid_a,
            );
        let alpha_destination_after =
            pallet_subtensor::Pallet::<mock::Test>::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &coldkey, netuid_b,
            );

        assert!(alpha_origin_after < alpha_origin_before);
        assert!(alpha_destination_after > alpha_destination_before);
    });
}

#[test]
fn caller_add_stake_limit_success_executes_within_price_guard() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(45001);
        let owner_coldkey = U256::from(45002);
        let coldkey = U256::from(55001);
        let hotkey = U256::from(55002);
        let amount_raw: u64 = 900_000_000_000;
        let limit_price: TaoBalance = 24_000_000_000u64.into();

        let netuid = mock::add_dynamic_network(&owner_hotkey, &owner_coldkey);

        mock::setup_reserves(
            netuid,
            TaoBalance::from(150_000_000_000_u64),
            AlphaBalance::from(100_000_000_000_u64),
        );

        mock::register_ok_neuron(netuid, hotkey, coldkey, 0);

        mock::add_balance_to_coldkey_account(
            &coldkey,
            (amount_raw + 1_000_000_000).into(),
        );

        let stake_before =
            pallet_subtensor::Pallet::<mock::Test>::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &coldkey, netuid,
            );
        let balance_before =
            pallet_subtensor::Pallet::<mock::Test>::get_coldkey_balance(&coldkey);

        let expected_weight = <<mock::Test as pallet_subtensor::Config>::WeightInfo as SubtensorWeightInfo>::add_stake_limit();

        let mut env = MockEnv::new(
            FunctionId::CallerAddStakeLimitV1,
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

        let stake_after =
            pallet_subtensor::Pallet::<mock::Test>::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &coldkey, netuid,
            );
        let balance_after =
            pallet_subtensor::Pallet::<mock::Test>::get_coldkey_balance(&coldkey);

        assert!(stake_after > stake_before);
        assert!(stake_after > AlphaBalance::ZERO);
        assert!(balance_after < balance_before);
    });
}

#[test]
fn caller_remove_stake_limit_success_respects_price_limit() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(46001);
        let owner_coldkey = U256::from(46002);
        let coldkey = U256::from(56001);
        let hotkey = U256::from(56002);
        let stake_amount_raw: u64 = 320_000_000_000;

        let netuid = mock::add_dynamic_network(&owner_hotkey, &owner_coldkey);
        mock::setup_reserves(
            netuid,
            TaoBalance::from(120_000_000_000_u64),
            AlphaBalance::from(100_000_000_000_u64),
        );

        mock::register_ok_neuron(netuid, hotkey, coldkey, 0);

        mock::add_balance_to_coldkey_account(
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

        let balance_before =
            pallet_subtensor::Pallet::<mock::Test>::get_coldkey_balance(&coldkey);

        let mut env = MockEnv::new(
            FunctionId::CallerRemoveStakeLimitV1,
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
        let balance_after =
            pallet_subtensor::Pallet::<mock::Test>::get_coldkey_balance(&coldkey);

        assert!(alpha_after < alpha_before);
        assert!(balance_after > balance_before);
    });
}

#[test]
fn caller_swap_stake_limit_matches_standard_slippage_path() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey_a = U256::from(47001);
        let owner_coldkey_a = U256::from(47002);
        let owner_hotkey_b = U256::from(47003);
        let owner_coldkey_b = U256::from(47004);
        let coldkey = U256::from(57001);
        let hotkey = U256::from(57002);

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
            FunctionId::CallerSwapStakeLimitV1,
            coldkey,
            (hotkey, netuid_a, netuid_b, alpha_to_swap, limit_price, true).encode(),
        )
        .with_expected_weight(expected_weight);

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        assert_extension_success(ret);

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
fn caller_remove_stake_full_limit_success_with_limit_price() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(48001);
        let owner_coldkey = U256::from(48002);
        let coldkey = U256::from(58001);
        let hotkey = U256::from(58002);
        let stake_amount_raw: u64 = 340_000_000_000;

        let netuid = mock::add_dynamic_network(&owner_hotkey, &owner_coldkey);
        mock::setup_reserves(
            netuid,
            TaoBalance::from(130_000_000_000_u64),
            AlphaBalance::from(110_000_000_000_u64),
        );

        mock::register_ok_neuron(netuid, hotkey, coldkey, 0);

        mock::add_balance_to_coldkey_account(
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

        let balance_before =
            pallet_subtensor::Pallet::<mock::Test>::get_coldkey_balance(&coldkey);

        let mut env = MockEnv::new(
            FunctionId::CallerRemoveStakeFullLimitV1,
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
        let balance_after =
            pallet_subtensor::Pallet::<mock::Test>::get_coldkey_balance(&coldkey);

        assert!(alpha_after.is_zero());
        assert!(balance_after > balance_before);
    });
}

#[test]
fn caller_set_coldkey_auto_stake_hotkey_success_sets_destination() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(49001);
        let owner_coldkey = U256::from(49002);
        let coldkey = U256::from(59001);
        let hotkey = U256::from(59002);

        let netuid = mock::add_dynamic_network(&owner_hotkey, &owner_coldkey);

        pallet_subtensor::Owner::<mock::Test>::insert(hotkey, coldkey);
        pallet_subtensor::OwnedHotkeys::<mock::Test>::insert(coldkey, vec![hotkey]);
        pallet_subtensor::Uids::<mock::Test>::insert(netuid, hotkey, 0u16);

        assert_eq!(
            pallet_subtensor::AutoStakeDestination::<mock::Test>::get(coldkey, netuid),
            None
        );

        let expected_weight = <<mock::Test as pallet_subtensor::Config>::WeightInfo as SubtensorWeightInfo>::set_coldkey_auto_stake_hotkey();

        let mut env = MockEnv::new(
            FunctionId::CallerSetColdkeyAutoStakeHotkeyV1,
            coldkey,
            (netuid, hotkey).encode(),
        )
        .with_expected_weight(expected_weight);

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        assert_extension_success(ret);
        assert_eq!(env.charged_weight(), Some(expected_weight));

        assert_eq!(
            pallet_subtensor::AutoStakeDestination::<mock::Test>::get(coldkey, netuid),
            Some(hotkey)
        );
    });
}

#[test]
fn caller_add_proxy_success_creates_proxy_relationship() {
    mock::new_test_ext(1).execute_with(|| {
        let delegator = U256::from(60001);
        let delegate = U256::from(60002);

        mock::add_balance_to_coldkey_account(&delegator, 1_000_000_000.into());

        let mut env = MockEnv::new(FunctionId::CallerAddProxyV1, delegator, delegate.encode());

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        assert_extension_success(ret);

        let proxies = pallet_subtensor_proxy::Proxies::<mock::Test>::get(delegator).0;
        assert_eq!(proxies.len(), 1);
    });
}

#[test]
fn caller_remove_proxy_success_removes_proxy_relationship() {
    mock::new_test_ext(1).execute_with(|| {
        let delegator = U256::from(70001);
        let delegate = U256::from(70002);

        mock::add_balance_to_coldkey_account(&delegator, 1_000_000_000.into());

        let mut add_env = MockEnv::new(FunctionId::CallerAddProxyV1, delegator, delegate.encode());
        assert_extension_success(
            SubtensorChainExtension::<mock::Test>::dispatch(&mut add_env).unwrap(),
        );

        let mut remove_env = MockEnv::new(
            FunctionId::CallerRemoveProxyV1,
            delegator,
            delegate.encode(),
        );
        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut remove_env).unwrap();
        assert_extension_success(ret);

        let proxies_after = pallet_subtensor_proxy::Proxies::<mock::Test>::get(delegator).0;
        assert_eq!(proxies_after.len(), 0);
    });
}
