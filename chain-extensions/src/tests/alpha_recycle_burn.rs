//! Recycle/burn alpha and atomic add-stake+recycle/burn dispatch paths.

use super::*;

#[test]
fn recycle_alpha_success_reduces_stake_and_returns_actual_amount() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(9001);
        let owner_coldkey = U256::from(9002);
        let coldkey = U256::from(9101);
        let hotkey = U256::from(9102);
        let min_stake = DefaultMinStake::<mock::Test>::get();
        let stake_amount_raw = min_stake.to_u64().saturating_mul(200);

        let netuid = mock::add_dynamic_network(&owner_hotkey, &owner_coldkey);
        mock::setup_reserves(
            netuid,
            TaoBalance::from(130_000_000_000_u64),
            AlphaBalance::from(110_000_000_000_u64),
        );

        mock::register_ok_neuron(netuid, hotkey, coldkey, 0);

        add_balance_to_coldkey_account(
            &coldkey,
            TaoBalance::from(stake_amount_raw.saturating_add(1_000_000_000)),
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
        assert!(alpha_before > AlphaBalance::ZERO);

        let alpha_out_before = pallet_subtensor::SubnetAlphaOut::<mock::Test>::get(netuid);

        let recycle_amount: AlphaBalance = (alpha_before.to_u64() / 2).into();

        let expected_weight =
            <<mock::Test as pallet_subtensor::Config>::WeightInfo as SubtensorWeightInfo>::recycle_alpha();

        let mut env = MockEnv::new(
            FunctionId::RecycleAlphaV1,
            coldkey,
            (hotkey, netuid, recycle_amount).encode(),
        )
        .with_expected_weight(expected_weight);

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        assert_extension_success(ret);
        assert_eq!(env.charged_weight(), Some(expected_weight));

        let returned_amount = AlphaBalance::decode(&mut env.output()).unwrap();
        assert_eq!(returned_amount, recycle_amount);

        let alpha_after =
            pallet_subtensor::Pallet::<mock::Test>::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &coldkey, netuid,
            );
        assert!(alpha_after < alpha_before);

        let alpha_out_after = pallet_subtensor::SubnetAlphaOut::<mock::Test>::get(netuid);
        assert!(alpha_out_after < alpha_out_before);
    });
}

#[test]
fn recycle_alpha_on_root_subnet_returns_error() {
    mock::new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(9201);
        let hotkey = U256::from(9202);

        pallet_subtensor::Owner::<mock::Test>::insert(hotkey, coldkey);

        let expected_weight =
            <<mock::Test as pallet_subtensor::Config>::WeightInfo as SubtensorWeightInfo>::recycle_alpha();

        let mut env = MockEnv::new(
            FunctionId::RecycleAlphaV1,
            coldkey,
            (hotkey, NetUid::ROOT, AlphaBalance::from(1_000u64)).encode(),
        )
        .with_expected_weight(expected_weight);

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        match ret {
            RetVal::Converging(code) => {
                assert_ne!(
                    code,
                    Output::Success as u32,
                    "should not succeed on root subnet"
                )
            }
            _ => panic!("unexpected return value"),
        }
    });
}

#[test]
fn burn_alpha_success_reduces_stake_and_returns_actual_amount() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(9301);
        let owner_coldkey = U256::from(9302);
        let coldkey = U256::from(9401);
        let hotkey = U256::from(9402);
        let min_stake = DefaultMinStake::<mock::Test>::get();
        let stake_amount_raw = min_stake.to_u64().saturating_mul(200);

        let netuid = mock::add_dynamic_network(&owner_hotkey, &owner_coldkey);
        mock::setup_reserves(
            netuid,
            TaoBalance::from(130_000_000_000_u64),
            AlphaBalance::from(110_000_000_000_u64),
        );

        mock::register_ok_neuron(netuid, hotkey, coldkey, 0);

        add_balance_to_coldkey_account(
            &coldkey,
            TaoBalance::from(stake_amount_raw.saturating_add(1_000_000_000)),
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
        assert!(alpha_before > AlphaBalance::ZERO);

        let alpha_out_before = pallet_subtensor::SubnetAlphaOut::<mock::Test>::get(netuid);

        let burn_amount: AlphaBalance = (alpha_before.to_u64() / 2).into();

        let expected_weight =
            <<mock::Test as pallet_subtensor::Config>::WeightInfo as SubtensorWeightInfo>::burn_alpha();

        let mut env = MockEnv::new(
            FunctionId::BurnAlphaV1,
            coldkey,
            (hotkey, netuid, burn_amount).encode(),
        )
        .with_expected_weight(expected_weight);

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        assert_extension_success(ret);
        assert_eq!(env.charged_weight(), Some(expected_weight));

        let returned_amount = AlphaBalance::decode(&mut env.output()).unwrap();
        assert_eq!(returned_amount, burn_amount);

        let alpha_after =
            pallet_subtensor::Pallet::<mock::Test>::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &coldkey, netuid,
            );
        assert!(alpha_after < alpha_before);

        // Burn should NOT decrease SubnetAlphaOut (unlike recycle)
        let alpha_out_after = pallet_subtensor::SubnetAlphaOut::<mock::Test>::get(netuid);
        assert_eq!(alpha_out_after, alpha_out_before);
    });
}

#[test]
fn burn_alpha_on_nonexistent_subnet_returns_error() {
    mock::new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(9501);
        let hotkey = U256::from(9502);

        let expected_weight =
            <<mock::Test as pallet_subtensor::Config>::WeightInfo as SubtensorWeightInfo>::burn_alpha();

        let mut env = MockEnv::new(
            FunctionId::BurnAlphaV1,
            coldkey,
            (hotkey, NetUid::from(999u16), AlphaBalance::from(1_000u64)).encode(),
        )
        .with_expected_weight(expected_weight);

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        match ret {
            RetVal::Converging(code) => {
                assert_eq!(
                    code,
                    Output::SubnetNotExists as u32,
                    "expected subnet not exists error"
                )
            }
            _ => panic!("unexpected return value"),
        }
    });
}

#[test]
fn add_stake_recycle_success_atomically_stakes_and_recycles() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(9601);
        let owner_coldkey = U256::from(9602);
        let coldkey = U256::from(9701);
        let hotkey = U256::from(9702);
        let min_stake = DefaultMinStake::<mock::Test>::get();
        let tao_amount_raw = min_stake.to_u64().saturating_mul(200);

        let netuid = mock::add_dynamic_network(&owner_hotkey, &owner_coldkey);
        mock::setup_reserves(
            netuid,
            TaoBalance::from(130_000_000_000_u64),
            AlphaBalance::from(110_000_000_000_u64),
        );

        mock::register_ok_neuron(netuid, hotkey, coldkey, 0);

        add_balance_to_coldkey_account(
            &coldkey,
            TaoBalance::from(tao_amount_raw.saturating_add(1_000_000_000)),
        );

        let alpha_out_before = pallet_subtensor::SubnetAlphaOut::<mock::Test>::get(netuid);

        let expected_weight =
            <<mock::Test as pallet_subtensor::Config>::WeightInfo as SubtensorWeightInfo>::add_stake()
                .saturating_add(
                    <<mock::Test as pallet_subtensor::Config>::WeightInfo as SubtensorWeightInfo>::recycle_alpha(),
                );

        let mut env = MockEnv::new(
            FunctionId::AddStakeRecycleV1,
            coldkey,
            (hotkey, netuid, TaoBalance::from(tao_amount_raw)).encode(),
        )
        .with_expected_weight(expected_weight);

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        assert_extension_success(ret);
        assert_eq!(env.charged_weight(), Some(expected_weight));

        let returned_alpha = AlphaBalance::decode(&mut env.output()).unwrap();
        assert!(returned_alpha > AlphaBalance::ZERO);

        // After atomic add+recycle, the stake should be zero (we recycled everything we added)
        let alpha_after =
            pallet_subtensor::Pallet::<mock::Test>::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &coldkey, netuid,
            );
        assert!(alpha_after.is_zero());

        // SubnetAlphaOut should not have increased (recycle cancels out the add)
        let alpha_out_after = pallet_subtensor::SubnetAlphaOut::<mock::Test>::get(netuid);
        assert!(alpha_out_after <= alpha_out_before);
    });
}

#[test]
fn add_stake_burn_success_atomically_stakes_and_burns() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(9801);
        let owner_coldkey = U256::from(9802);
        let coldkey = U256::from(9901);
        let hotkey = U256::from(9902);
        let min_stake = DefaultMinStake::<mock::Test>::get();
        let tao_amount_raw = min_stake.to_u64().saturating_mul(200);

        let netuid = mock::add_dynamic_network(&owner_hotkey, &owner_coldkey);
        mock::setup_reserves(
            netuid,
            TaoBalance::from(130_000_000_000_u64),
            AlphaBalance::from(110_000_000_000_u64),
        );

        mock::register_ok_neuron(netuid, hotkey, coldkey, 0);

        add_balance_to_coldkey_account(
            &coldkey,
            TaoBalance::from(tao_amount_raw.saturating_add(1_000_000_000)),
        );

        let alpha_out_before = pallet_subtensor::SubnetAlphaOut::<mock::Test>::get(netuid);

        let expected_weight =
            <<mock::Test as pallet_subtensor::Config>::WeightInfo as SubtensorWeightInfo>::add_stake()
                .saturating_add(
                    <<mock::Test as pallet_subtensor::Config>::WeightInfo as SubtensorWeightInfo>::burn_alpha(),
                );

        let mut env = MockEnv::new(
            FunctionId::AddStakeBurnV1,
            coldkey,
            (hotkey, netuid, TaoBalance::from(tao_amount_raw)).encode(),
        )
        .with_expected_weight(expected_weight);

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        assert_extension_success(ret);
        assert_eq!(env.charged_weight(), Some(expected_weight));

        let returned_alpha = AlphaBalance::decode(&mut env.output()).unwrap();
        assert!(returned_alpha > AlphaBalance::ZERO);

        // After atomic add+burn, the stake should be zero
        let alpha_after =
            pallet_subtensor::Pallet::<mock::Test>::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &coldkey, netuid,
            );
        assert!(alpha_after.is_zero());

        // SubnetAlphaOut should have increased (burn does NOT reduce AlphaOut)
        let alpha_out_after = pallet_subtensor::SubnetAlphaOut::<mock::Test>::get(netuid);
        assert!(alpha_out_after > alpha_out_before);
    });
}

#[test]
fn add_stake_recycle_with_insufficient_balance_returns_error() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(10001);
        let owner_coldkey = U256::from(10002);
        let coldkey = U256::from(10101);
        let hotkey = U256::from(10102);

        let netuid = mock::add_dynamic_network(&owner_hotkey, &owner_coldkey);
        mock::setup_reserves(
            netuid,
            TaoBalance::from(130_000_000_000_u64),
            AlphaBalance::from(110_000_000_000_u64),
        );

        mock::register_ok_neuron(netuid, hotkey, coldkey, 0);

        // Don't fund the coldkey - should fail with balance error

        let expected_weight =
            <<mock::Test as pallet_subtensor::Config>::WeightInfo as SubtensorWeightInfo>::add_stake()
                .saturating_add(
                    <<mock::Test as pallet_subtensor::Config>::WeightInfo as SubtensorWeightInfo>::recycle_alpha(),
                );

        let mut env = MockEnv::new(
            FunctionId::AddStakeRecycleV1,
            coldkey,
            (hotkey, netuid, TaoBalance::from(100_000_000_000_u64)).encode(),
        )
        .with_expected_weight(expected_weight);

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        match ret {
            RetVal::Converging(code) => {
                assert_ne!(code, Output::Success as u32, "should not succeed")
            }
            _ => panic!("unexpected return value"),
        }
        assert_eq!(env.charged_weight(), Some(expected_weight));
    });
}

#[test]
fn recycle_alpha_clamps_to_available_when_amount_exceeds_stake() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(11001);
        let owner_coldkey = U256::from(11002);
        let coldkey = U256::from(11101);
        let hotkey = U256::from(11102);
        let min_stake = DefaultMinStake::<mock::Test>::get();
        let stake_amount_raw = min_stake.to_u64().saturating_mul(200);

        let netuid = mock::add_dynamic_network(&owner_hotkey, &owner_coldkey);
        mock::setup_reserves(
            netuid,
            TaoBalance::from(130_000_000_000_u64),
            AlphaBalance::from(110_000_000_000_u64),
        );

        mock::register_ok_neuron(netuid, hotkey, coldkey, 0);

        add_balance_to_coldkey_account(
            &coldkey,
            TaoBalance::from(stake_amount_raw.saturating_add(1_000_000_000)),
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
        assert!(alpha_before > AlphaBalance::ZERO);

        // Request way more than available — should clamp to alpha_before
        let huge_amount = AlphaBalance::from(u64::MAX);

        let expected_weight =
            <<mock::Test as pallet_subtensor::Config>::WeightInfo as SubtensorWeightInfo>::recycle_alpha();

        let mut env = MockEnv::new(
            FunctionId::RecycleAlphaV1,
            coldkey,
            (hotkey, netuid, huge_amount).encode(),
        )
        .with_expected_weight(expected_weight);

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        assert_extension_success(ret);

        let returned_amount = AlphaBalance::decode(&mut env.output()).unwrap();
        assert_eq!(
            returned_amount, alpha_before,
            "should return actual clamped amount, not requested amount"
        );

        let alpha_after =
            pallet_subtensor::Pallet::<mock::Test>::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &coldkey, netuid,
            );
        assert!(alpha_after.is_zero(), "all alpha should be recycled");
    });
}

#[test]
fn burn_alpha_on_root_subnet_returns_error() {
    mock::new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(11201);
        let hotkey = U256::from(11202);

        pallet_subtensor::Owner::<mock::Test>::insert(hotkey, coldkey);

        let expected_weight =
            <<mock::Test as pallet_subtensor::Config>::WeightInfo as SubtensorWeightInfo>::burn_alpha();

        let mut env = MockEnv::new(
            FunctionId::BurnAlphaV1,
            coldkey,
            (hotkey, NetUid::ROOT, AlphaBalance::from(1_000u64)).encode(),
        )
        .with_expected_weight(expected_weight);

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        match ret {
            RetVal::Converging(code) => {
                assert_ne!(
                    code,
                    Output::Success as u32,
                    "should not succeed on root subnet"
                )
            }
            _ => panic!("unexpected return value"),
        }
    });
}

#[test]
fn burn_alpha_clamps_to_available_when_amount_exceeds_stake() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(11301);
        let owner_coldkey = U256::from(11302);
        let coldkey = U256::from(11401);
        let hotkey = U256::from(11402);
        let min_stake = DefaultMinStake::<mock::Test>::get();
        let stake_amount_raw = min_stake.to_u64().saturating_mul(200);

        let netuid = mock::add_dynamic_network(&owner_hotkey, &owner_coldkey);
        mock::setup_reserves(
            netuid,
            TaoBalance::from(130_000_000_000_u64),
            AlphaBalance::from(110_000_000_000_u64),
        );

        mock::register_ok_neuron(netuid, hotkey, coldkey, 0);

        add_balance_to_coldkey_account(
            &coldkey,
            TaoBalance::from(stake_amount_raw.saturating_add(1_000_000_000)),
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
        assert!(alpha_before > AlphaBalance::ZERO);

        // Request way more than available — should clamp to alpha_before
        let huge_amount = AlphaBalance::from(u64::MAX);

        let expected_weight =
            <<mock::Test as pallet_subtensor::Config>::WeightInfo as SubtensorWeightInfo>::burn_alpha();

        let mut env = MockEnv::new(
            FunctionId::BurnAlphaV1,
            coldkey,
            (hotkey, netuid, huge_amount).encode(),
        )
        .with_expected_weight(expected_weight);

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        assert_extension_success(ret);

        let returned_amount = AlphaBalance::decode(&mut env.output()).unwrap();
        assert_eq!(
            returned_amount, alpha_before,
            "should return actual clamped amount, not requested amount"
        );

        let alpha_after =
            pallet_subtensor::Pallet::<mock::Test>::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &coldkey, netuid,
            );
        assert!(alpha_after.is_zero(), "all alpha should be burned");
    });
}
#[test]
fn add_stake_recycle_rollback_on_recycle_failure() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(12001);
        let owner_coldkey = U256::from(12002);
        let coldkey = U256::from(12101);
        let hotkey = U256::from(12102);
        let min_stake = DefaultMinStake::<mock::Test>::get();
        let tao_amount_raw = min_stake.to_u64().saturating_mul(200);

        let netuid = mock::add_dynamic_network(&owner_hotkey, &owner_coldkey);

        mock::register_ok_neuron(netuid, hotkey, coldkey, 0);
        pallet_subtensor::Pallet::<mock::Test>::insert_lock_state(
            &coldkey,
            netuid,
            &hotkey,
            pallet_subtensor::staking::lock::LockState {
                locked_mass: AlphaBalance::from(u64::MAX / 4),
                conviction: U64F64::saturating_from_num(0),
                last_update: pallet_subtensor::Pallet::<mock::Test>::get_current_block_as_u64(),
            },
        );

        // Leave enough input-side liquidity for add_stake to pass the 1000x swap input cap.
        // The lock above makes the recycle leg fail, exercising atomic rollback.
        mock::setup_reserves(
            netuid,
            TaoBalance::from(tao_amount_raw / 1000 + 1),
            AlphaBalance::from(1_000_u64),
        );

        add_balance_to_coldkey_account(
            &coldkey,
            TaoBalance::from(tao_amount_raw.saturating_add(1_000_000_000)),
        );

        let balance_before = pallet_subtensor::Pallet::<mock::Test>::get_coldkey_balance(&coldkey);
        let alpha_before =
            pallet_subtensor::Pallet::<mock::Test>::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &coldkey, netuid,
            );

        let expected_weight =
            <<mock::Test as pallet_subtensor::Config>::WeightInfo as SubtensorWeightInfo>::add_stake()
                .saturating_add(
                    <<mock::Test as pallet_subtensor::Config>::WeightInfo as SubtensorWeightInfo>::recycle_alpha(),
                );

        let mut env = MockEnv::new(
            FunctionId::AddStakeRecycleV1,
            coldkey,
            (hotkey, netuid, TaoBalance::from(tao_amount_raw)).encode(),
        )
        .with_expected_weight(expected_weight);

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        match ret {
            RetVal::Converging(code) => {
                assert_ne!(code, Output::Success as u32, "should not succeed")
            }
            _ => panic!("unexpected return value"),
        }

        // Verify full rollback: balance and stake unchanged
        let balance_after = pallet_subtensor::Pallet::<mock::Test>::get_coldkey_balance(&coldkey);
        let alpha_after =
            pallet_subtensor::Pallet::<mock::Test>::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &coldkey, netuid,
            );

        assert_eq!(
            balance_before, balance_after,
            "balance should be unchanged after rollback"
        );
        assert_eq!(
            alpha_before, alpha_after,
            "stake should be unchanged after rollback"
        );
    });
}

#[test]
fn add_stake_burn_rollback_on_burn_failure() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(12201);
        let owner_coldkey = U256::from(12202);
        let coldkey = U256::from(12301);
        let hotkey = U256::from(12302);
        let min_stake = DefaultMinStake::<mock::Test>::get();
        let tao_amount_raw = min_stake.to_u64().saturating_mul(200);

        let netuid = mock::add_dynamic_network(&owner_hotkey, &owner_coldkey);

        mock::register_ok_neuron(netuid, hotkey, coldkey, 0);
        pallet_subtensor::Pallet::<mock::Test>::insert_lock_state(
            &coldkey,
            netuid,
            &hotkey,
            pallet_subtensor::staking::lock::LockState {
                locked_mass: AlphaBalance::from(u64::MAX / 4),
                conviction: U64F64::saturating_from_num(0),
                last_update: pallet_subtensor::Pallet::<mock::Test>::get_current_block_as_u64(),
            },
        );

        // Leave enough input-side liquidity for add_stake to pass the 1000x swap input cap.
        // The lock above makes the burn leg fail, exercising atomic rollback.
        mock::setup_reserves(
            netuid,
            TaoBalance::from(tao_amount_raw / 1000 + 1),
            AlphaBalance::from(1_000_u64),
        );

        add_balance_to_coldkey_account(
            &coldkey,
            TaoBalance::from(tao_amount_raw.saturating_add(1_000_000_000)),
        );

        let balance_before = pallet_subtensor::Pallet::<mock::Test>::get_coldkey_balance(&coldkey);
        let alpha_before =
            pallet_subtensor::Pallet::<mock::Test>::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &coldkey, netuid,
            );

        let expected_weight =
            <<mock::Test as pallet_subtensor::Config>::WeightInfo as SubtensorWeightInfo>::add_stake()
                .saturating_add(
                    <<mock::Test as pallet_subtensor::Config>::WeightInfo as SubtensorWeightInfo>::burn_alpha(),
                );

        let mut env = MockEnv::new(
            FunctionId::AddStakeBurnV1,
            coldkey,
            (hotkey, netuid, TaoBalance::from(tao_amount_raw)).encode(),
        )
        .with_expected_weight(expected_weight);

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        match ret {
            RetVal::Converging(code) => {
                assert_ne!(code, Output::Success as u32, "should not succeed")
            }
            _ => panic!("unexpected return value"),
        }

        // Verify full rollback: balance and stake unchanged
        let balance_after = pallet_subtensor::Pallet::<mock::Test>::get_coldkey_balance(&coldkey);
        let alpha_after =
            pallet_subtensor::Pallet::<mock::Test>::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &coldkey, netuid,
            );

        assert_eq!(
            balance_before, balance_after,
            "balance should be unchanged after rollback"
        );
        assert_eq!(
            alpha_before, alpha_after,
            "stake should be unchanged after rollback"
        );
    });
}
