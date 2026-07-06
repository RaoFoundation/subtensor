#![allow(clippy::unwrap_used)]

use super::{SubtensorChainExtension, SubtensorExtensionEnv, mock};
use crate::types::{ColdkeyLock, FunctionId, Output, StakeAvailability, SubnetRegistrationState};
use codec::{Decode, Encode};
use frame_support::pallet_prelude::Zero;
use frame_support::{assert_ok, weights::Weight};
use frame_system::RawOrigin;
use pallet_contracts::chain_extension::RetVal;
use pallet_subtensor::DefaultMinStake;
use pallet_subtensor::weights::WeightInfo as SubtensorWeightInfo;
use sp_core::Get;
use sp_core::U256;
use sp_runtime::DispatchError;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};
use subtensor_swap_interface::SwapHandler;

type AccountId = <mock::Test as frame_system::Config>::AccountId;

#[derive(Clone)]
struct MockEnv {
    func_id: u16,
    caller: AccountId,
    input: Vec<u8>,
    output: Vec<u8>,
    charged_weight: Option<Weight>,
    expected_weight: Option<Weight>,
}

#[allow(dead_code)]
pub fn add_balance_to_coldkey_account(coldkey: &U256, tao: TaoBalance) {
    let credit = pallet_subtensor::Pallet::<mock::Test>::mint_tao(tao);
    let _ = pallet_subtensor::Pallet::<mock::Test>::spend_tao(coldkey, credit, tao).unwrap();
}

#[test]
fn set_coldkey_auto_stake_hotkey_success_sets_destination() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(4901);
        let owner_coldkey = U256::from(4902);
        let coldkey = U256::from(5901);
        let hotkey = U256::from(5902);

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
            FunctionId::SetColdkeyAutoStakeHotkeyV1,
            coldkey,
            (netuid, hotkey).encode(),
        )
        .with_expected_weight(expected_weight);

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        assert_success(ret);
        assert_eq!(env.charged_weight(), Some(expected_weight));

        assert_eq!(
            pallet_subtensor::AutoStakeDestination::<mock::Test>::get(coldkey, netuid),
            Some(hotkey)
        );
        let coldkeys =
            pallet_subtensor::AutoStakeDestinationColdkeys::<mock::Test>::get(hotkey, netuid);
        assert!(coldkeys.contains(&coldkey));
    });
}

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
        assert_success(ret);
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
        assert_success(ret);
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
        assert_success(ret);
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
        assert_success(ret);
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
        assert_success(ret);
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
        assert_success(ret);
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
        assert_success(ret);
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
        assert_success(ret);
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
fn add_proxy_success_creates_proxy_relationship() {
    mock::new_test_ext(1).execute_with(|| {
        let delegator = U256::from(6001);
        let delegate = U256::from(6002);

        add_balance_to_coldkey_account(&delegator, 1_000_000_000.into());

        assert_eq!(
            pallet_subtensor_proxy::Proxies::<mock::Test>::get(delegator)
                .0
                .len(),
            0
        );

        let mut env = MockEnv::new(FunctionId::AddProxyV1, delegator, delegate.encode());

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        assert_success(ret);

        let proxies = pallet_subtensor_proxy::Proxies::<mock::Test>::get(delegator).0;
        assert_eq!(proxies.len(), 1);
        if let Some(proxy) = proxies.first() {
            assert_eq!(proxy.delegate, delegate);
            assert_eq!(
                proxy.proxy_type,
                subtensor_runtime_common::ProxyType::Staking
            );
            assert_eq!(proxy.delay, 0u64);
        } else {
            panic!("proxies should contain one element");
        }
    });
}

#[test]
fn remove_proxy_success_removes_proxy_relationship() {
    mock::new_test_ext(1).execute_with(|| {
        let delegator = U256::from(7001);
        let delegate = U256::from(7002);

        add_balance_to_coldkey_account(&delegator, 1_000_000_000.into());

        let mut add_env = MockEnv::new(FunctionId::AddProxyV1, delegator, delegate.encode());
        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut add_env).unwrap();
        assert_success(ret);

        let proxies_before = pallet_subtensor_proxy::Proxies::<mock::Test>::get(delegator).0;
        assert_eq!(proxies_before.len(), 1);

        let mut remove_env = MockEnv::new(FunctionId::RemoveProxyV1, delegator, delegate.encode());
        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut remove_env).unwrap();
        assert_success(ret);

        let proxies_after = pallet_subtensor_proxy::Proxies::<mock::Test>::get(delegator).0;
        assert_eq!(proxies_after.len(), 0);
    });
}

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
        assert_success(ret);
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
        assert_success(ret);
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
        assert_success(ret);
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
        assert_success(ret);
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
        assert_success(ret);

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
        assert_success(ret);

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

impl MockEnv {
    fn new(func_id: FunctionId, caller: AccountId, input: Vec<u8>) -> Self {
        Self {
            func_id: func_id as u16,
            caller,
            input,
            output: Vec::new(),
            charged_weight: None,
            expected_weight: None,
        }
    }

    fn with_expected_weight(mut self, weight: Weight) -> Self {
        self.expected_weight = Some(weight);
        self
    }

    fn charged_weight(&self) -> Option<Weight> {
        self.charged_weight
    }

    fn output(&self) -> &[u8] {
        &self.output
    }
}

impl SubtensorExtensionEnv<mock::Test> for MockEnv {
    fn func_id(&self) -> u16 {
        self.func_id
    }

    fn charge_weight(&mut self, weight: Weight) -> Result<(), DispatchError> {
        let prev = self.charged_weight.unwrap_or_default();
        let cumulative = Weight::from_parts(
            prev.ref_time().checked_add(weight.ref_time()).unwrap(),
            prev.proof_size().checked_add(weight.proof_size()).unwrap(),
        );
        if let Some(expected) = self.expected_weight
            && (cumulative.ref_time() > expected.ref_time()
                || cumulative.proof_size() > expected.proof_size())
        {
            return Err(DispatchError::Other(
                "unexpected weight charged by mock env",
            ));
        }
        self.charged_weight = Some(cumulative);
        Ok(())
    }

    fn read_as<U: codec::Decode + codec::MaxEncodedLen>(&mut self) -> Result<U, DispatchError> {
        U::decode(&mut &self.input[..]).map_err(|_| DispatchError::Other("mock env decode failure"))
    }

    fn write_output(&mut self, data: &[u8]) -> Result<(), DispatchError> {
        self.output.clear();
        self.output.extend_from_slice(data);
        Ok(())
    }

    fn caller(&mut self) -> AccountId {
        self.caller
    }

    fn origin(&mut self) -> pallet_contracts::Origin<mock::Test> {
        pallet_contracts::Origin::Signed(self.caller)
    }
}

fn assert_success(ret: RetVal) {
    match ret {
        RetVal::Converging(code) => {
            assert_eq!(code, Output::Success as u32, "expected success code")
        }
        _ => panic!("unexpected return value"),
    }
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

        assert_success(ret);
        assert_eq!(env.output(), expected.as_slice());
        assert!(env.charged_weight().is_none());
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

        assert_success(ret);
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
        assert_success(ret);
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
        assert_success(ret);
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
        assert_success(ret);
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
        assert_success(ret);

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
        let reused_netuid = mock::add_dynamic_network(&second_hotkey, &second_coldkey);
        assert_eq!(reused_netuid, netuid);

        let mut env = MockEnv::new(
            FunctionId::GetSubnetRegistrationStateV1,
            caller,
            netuid.encode(),
        );

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        assert_success(ret);

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
        assert_success(ret);
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
        assert_success(ret);
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
        assert_success(ret);
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
        assert_success(ret);
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
        assert_success(ret);
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

/// `Caller*` dispatch uses `env.origin()` via `convert_origin`; with [`MockEnv`] both match
/// `Signed(caller)`, so outcomes align with non-`Caller` arms. Weight expectations match the shared
/// `dispatch_*_v1` helpers used by each pair.
mod caller_dispatch_tests {
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
            assert_success(ret);
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
            assert_success(ret);

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
            assert_success(ret);

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
            assert_success(ret);

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
            assert_success(ret);

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
            assert_success(ret);

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
            assert_success(ret);

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
            assert_success(ret);
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
            assert_success(ret);

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
            assert_success(ret);
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
            assert_success(ret);
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
            assert_success(ret);

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

            let mut add_env =
                MockEnv::new(FunctionId::CallerAddProxyV1, delegator, delegate.encode());
            assert_success(SubtensorChainExtension::<mock::Test>::dispatch(&mut add_env).unwrap());

            let mut remove_env = MockEnv::new(
                FunctionId::CallerRemoveProxyV1,
                delegator,
                delegate.encode(),
            );
            let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut remove_env).unwrap();
            assert_success(ret);

            let proxies_after = pallet_subtensor_proxy::Proxies::<mock::Test>::get(delegator).0;
            assert_eq!(proxies_after.len(), 0);
        });
    }
}
