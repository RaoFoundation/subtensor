//! Indexer-contract tests for Mentat
//! Any modification in these tests will notify the member responsible
//! for the communication between protocol and the Mentat team.


#![allow(clippy::unwrap_used)]
#![allow(clippy::arithmetic_side_effects)]

use frame_system as system;
use pallet_subtensor::*;
use pallet_subtensor_proxy::{Proxies, RealPaysFee};
use pallet_subtensor_swap::FeeRate;
use pallet_subtensor_swap_runtime_api::SwapRuntimeApi;
use sp_core::U256;
use sp_runtime::traits::Block as BlockT;
use substrate_fixed::types::I96F32;
use subtensor_custom_rpc_runtime_api::{StakeInfoRuntimeApi, SubnetInfoRuntimeApi};
use subtensor_runtime_common::{AlphaBalance, MechId, NetUid, NetUidStorageIndex, TaoBalance};

use super::helpers::*;
use super::mock::*;

// ---------------------------------------------------------------------------
// Storage queries — SubtensorModule
// ---------------------------------------------------------------------------

#[test]
fn indexer_hotkey_ownership() {
    new_test_ext(1).execute_with(|| {
        let hotkey = U256::from(1);

        let _: U256 = Owner::<Test>::get(hotkey);
    });
}

#[test]
fn indexer_staking_hotkeys() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);

        let _: Vec<U256> = StakingHotkeys::<Test>::get(coldkey);
    });
}

#[test]
fn indexer_alpha_shares_and_stake() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1u16);
        let hotkey = U256::from(1);

        let _: AlphaBalance = TotalHotkeyAlpha::<Test>::get(hotkey, netuid);
    });
}

#[test]
fn indexer_subnet_pool_data() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1u16);

        let _: u16 = TotalNetworks::<Test>::get();
        let _: TaoBalance = SubnetTAO::<Test>::get(netuid);
        let _: AlphaBalance = SubnetAlphaIn::<Test>::get(netuid);
        let _: AlphaBalance = SubnetAlphaOut::<Test>::get(netuid);
        let _: I96F32 = SubnetMovingPrice::<Test>::get(netuid);
    });
}

#[test]
fn indexer_subnet_metadata() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1u16);

        let _: MechId = MechanismCountCurrent::<Test>::get(netuid);
        let _: u64 = NetworkImmunityPeriod::<Test>::get();
        let _: u64 = NetworkRegisteredAt::<Test>::get(netuid);
        let _: Option<SubnetIdentityOfV3> = SubnetIdentitiesV3::<Test>::get(netuid);
    });
}

#[test]
fn indexer_neuron_per_subnet_vectors() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1u16);

        let _: Vec<bool> = Active::<Test>::get(netuid);
        let _: Vec<u16> = Dividends::<Test>::get(netuid);
        let _: Vec<AlphaBalance> = Emission::<Test>::get(netuid);
        let _: Vec<u16> = Incentive::<Test>::get(NetUidStorageIndex::from(netuid));
        let _: Vec<u16> = ValidatorTrust::<Test>::get(netuid);
    });
}

#[test]
fn indexer_neuron_uid_maps() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1u16);
        let uid: u16 = 0;

        let _: u16 = SubnetworkN::<Test>::get(netuid);
        let _: U256 = Keys::<Test>::get(netuid, uid);
    });
}

#[test]
fn indexer_childkey_and_parentkey_graph() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1u16);
        let hotkey = U256::from(1);

        let _: Vec<(u64, U256)> = ChildKeys::<Test>::get(hotkey, netuid);
        let _: Vec<(u64, U256)> = ParentKeys::<Test>::get(hotkey, netuid);
        let _: u16 = ChildkeyTake::<Test>::get(hotkey, netuid);
    });
}

#[test]
fn indexer_subnet_hyperparams() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1u16);

        let _: u16 = Tempo::<Test>::get(netuid);
        let _: u16 = MaxAllowedUids::<Test>::get(netuid);
    });
}

#[test]
fn indexer_validator_dividends() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1u16);
        let hotkey = U256::from(1);

        let _: AlphaBalance = AlphaDividendsPerSubnet::<Test>::get(netuid, hotkey);
    });
}

#[test]
fn indexer_network_economics() {
    new_test_ext(1).execute_with(|| {
        let _: u64 = TaoWeight::<Test>::get();
    });
}

#[test]
fn indexer_swap_fee_rate() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1u16);

        let _: u16 = FeeRate::<Test>::get(netuid);
    });
}

#[test]
fn indexer_mechanism_emission() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1u16);

        let _: MechId = MechanismCountCurrent::<Test>::get(netuid);
        let _: Option<Vec<u16>> = MechanismEmissionSplit::<Test>::get(netuid);
    });
}

#[test]
fn indexer_root_claim_type() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);

        let _: RootClaimTypeEnum = RootClaimType::<Test>::get(coldkey);
    });
}

#[test]
fn indexer_pending_childkey_cooldown() {
    new_test_ext(1).execute_with(|| {
        let _: u64 = PendingChildKeyCooldown::<Test>::get();
    });
}

#[test]
fn indexer_root_alpha_dividends_per_subnet() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1u16);
        let hotkey = U256::from(1);

        let _: AlphaBalance = RootAlphaDividendsPerSubnet::<Test>::get(netuid, hotkey);
    });
}

// ---------------------------------------------------------------------------
// Storage queries — proxy pallet
// ---------------------------------------------------------------------------

#[test]
fn indexer_proxy_proxies() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);

        let _ = Proxies::<Test>::get(coldkey);
    });
}

#[test]
fn indexer_proxy_real_pays_fee() {
    new_test_ext(1).execute_with(|| {
        let real = U256::from(1);
        let delegate = U256::from(2);

        let _: Option<()> = RealPaysFee::<Test>::get(real, delegate);
    });
}

// ---------------------------------------------------------------------------
// Extrinsics — SubtensorModule (call signatures)
//
// These run inside externalities and the Result is intentionally ignored:
// the call will typically return Err (no funds / no network), which is fine.
// We only lock that the dispatchable's argument list and types are unchanged.
// ---------------------------------------------------------------------------

#[test]
fn indexer_extrinsic_add_stake() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = NetUid::from(1u16);
        let amount = TaoBalance::from(1_000_000_000u64);

        let _ = SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            amount,
        );
    });
}

#[test]
fn indexer_extrinsic_remove_stake() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = NetUid::from(1u16);
        let amount = AlphaBalance::from(1_000_000_000u64);

        let _ = SubtensorModule::remove_stake(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            amount,
        );
    });
}

#[test]
fn indexer_extrinsic_add_stake_limit() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = NetUid::from(1u16);
        let amount = TaoBalance::from(1_000_000_000u64);

        let _ = SubtensorModule::add_stake_limit(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            amount,
            TaoBalance::from(0u64),
            false,
        );
    });
}

#[test]
fn indexer_extrinsic_remove_stake_limit() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = NetUid::from(1u16);
        let alpha_amount = AlphaBalance::from(1_000_000_000u64);

        let _ = SubtensorModule::remove_stake_limit(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            alpha_amount,
            TaoBalance::from(0u64),
            false,
        );
    });
}

#[test]
fn indexer_extrinsic_swap_stake() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid_origin = NetUid::from(1u16);
        let netuid_dest = NetUid::from(2u16);
        let alpha_amount = AlphaBalance::from(1_000_000_000u64);

        let _ = SubtensorModule::swap_stake(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid_origin,
            netuid_dest,
            alpha_amount,
        );
    });
}

#[test]
fn indexer_extrinsic_swap_stake_limit() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid_origin = NetUid::from(1u16);
        let netuid_dest = NetUid::from(2u16);
        let alpha_amount = AlphaBalance::from(1_000_000_000u64);

        let _ = SubtensorModule::swap_stake_limit(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid_origin,
            netuid_dest,
            alpha_amount,
            TaoBalance::from(0u64),
            false,
        );
    });
}

#[test]
fn indexer_extrinsic_move_stake() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey_origin = U256::from(2);
        let hotkey_dest = U256::from(3);
        let netuid = NetUid::from(1u16);
        let alpha_amount = AlphaBalance::from(1_000_000_000u64);

        let _ = SubtensorModule::move_stake(
            RuntimeOrigin::signed(coldkey),
            hotkey_origin,
            hotkey_dest,
            netuid,
            netuid,
            alpha_amount,
        );
    });
}

#[test]
fn indexer_extrinsic_set_children() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let child = U256::from(3);
        let netuid = NetUid::from(1u16);
        let children: Vec<(u64, U256)> = vec![(u64::MAX, child)];

        let _ = SubtensorModule::set_children(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            children,
        );
    });
}

#[test]
fn indexer_extrinsic_decrease_take() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let take_u16: u16 = 1000;

        let _ = SubtensorModule::decrease_take(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            take_u16,
        );
    });
}

#[test]
fn indexer_extrinsic_set_root_claim_type() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);

        let _ = SubtensorModule::set_root_claim_type(
            RuntimeOrigin::signed(coldkey),
            RootClaimTypeEnum::Swap,
        );
    });
}

// ---------------------------------------------------------------------------
// Extrinsics — proxy pallet
// ---------------------------------------------------------------------------

#[test]
fn indexer_extrinsic_proxy_add_proxy() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let delegate = U256::from(2);

        let _ = pallet_subtensor_proxy::Pallet::<Test>::add_proxy(
            RuntimeOrigin::signed(coldkey),
            delegate,
            subtensor_runtime_common::ProxyType::Any,
            0u64.into(),
        );
    });
}

#[test]
fn indexer_extrinsic_proxy_proxy() {
    new_test_ext(1).execute_with(|| {
        let delegate = U256::from(1);
        let real = U256::from(2);
        let call = Box::new(RuntimeCall::System(system::Call::remark { remark: vec![] }));

        let _ = pallet_subtensor_proxy::Pallet::<Test>::proxy(
            RuntimeOrigin::signed(delegate),
            real,
            Some(subtensor_runtime_common::ProxyType::Any),
            call,
        );
    });
}

#[test]
fn indexer_extrinsic_proxy_remove_proxy() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let delegate = U256::from(2);

        let _ = pallet_subtensor_proxy::Pallet::<Test>::remove_proxy(
            RuntimeOrigin::signed(coldkey),
            delegate,
            subtensor_runtime_common::ProxyType::Any,
            0u64.into(),
        );
    });
}

#[test]
fn indexer_extrinsic_proxy_set_real_pays_fee() {
    new_test_ext(1).execute_with(|| {
        let real = U256::from(1);
        let delegate = U256::from(2);

        let _ = pallet_subtensor_proxy::Pallet::<Test>::set_real_pays_fee(
            RuntimeOrigin::signed(real),
            delegate,
            true,
        );
    });
}

// ---------------------------------------------------------------------------
// Extrinsics — balances pallet
// ---------------------------------------------------------------------------

#[test]
fn indexer_extrinsic_balances_transfer_keep_alive() {
    new_test_ext(1).execute_with(|| {
        let from = U256::from(1);
        let dest = U256::from(2);
        let value = TaoBalance::from(1_000_000_000u64);

        let _ = Balances::transfer_keep_alive(
            RuntimeOrigin::signed(from),
            dest,
            value,
        );
    });
}

// ---------------------------------------------------------------------------
// Extrinsics — system pallet
// ---------------------------------------------------------------------------

#[test]
fn indexer_extrinsic_system_remark() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let remark = vec![0u8; 32];

        let _ = system::Pallet::<Test>::remark(
            RuntimeOrigin::signed(who),
            remark,
        );
    });
}

// ---------------------------------------------------------------------------
// Extrinsics — utility pallet
// ---------------------------------------------------------------------------

#[test]
fn indexer_extrinsic_utility_batch() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let calls: Vec<RuntimeCall> = vec![
            RuntimeCall::System(system::Call::remark { remark: vec![] }),
        ];

        let _ = pallet_subtensor_utility::Pallet::<Test>::batch(
            RuntimeOrigin::signed(who),
            calls,
        );
    });
}

#[test]
fn indexer_extrinsic_utility_batch_all() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let calls: Vec<RuntimeCall> = vec![
            RuntimeCall::System(system::Call::remark { remark: vec![] }),
        ];

        let _ = pallet_subtensor_utility::Pallet::<Test>::batch_all(
            RuntimeOrigin::signed(who),
            calls,
        );
    });
}

#[test]
fn indexer_extrinsic_utility_force_batch() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let calls: Vec<RuntimeCall> = vec![
            RuntimeCall::System(system::Call::remark { remark: vec![] }),
        ];

        let _ = pallet_subtensor_utility::Pallet::<Test>::force_batch(
            RuntimeOrigin::signed(who),
            calls,
        );
    });
}

// ---------------------------------------------------------------------------
// Runtime API signatures
// ---------------------------------------------------------------------------

#[test]
fn indexer_runtime_api_current_alpha_price() {
    let at = <Block as BlockT>::Hash::default();
    let netuid = NetUid::from(1u16);

    let _: u64 = SwapRuntimeApi::current_alpha_price(&MockApi, at, netuid).unwrap();
}

#[test]
fn indexer_runtime_api_sim_swap() {
    let at = <Block as BlockT>::Hash::default();
    let netuid = NetUid::from(1u16);
    let tao = TaoBalance::from(1_000_000_000u64);
    let alpha = AlphaBalance::from(1_000_000_000u64);

    let _: pallet_subtensor_swap_runtime_api::SimSwapResult =
        SwapRuntimeApi::sim_swap_tao_for_alpha(&MockApi, at, netuid, tao).unwrap();
    let _: pallet_subtensor_swap_runtime_api::SimSwapResult =
        SwapRuntimeApi::sim_swap_alpha_for_tao(&MockApi, at, netuid, alpha).unwrap();
}

#[test]
fn indexer_runtime_api_get_metagraph() {
    let at = <Block as BlockT>::Hash::default();
    let netuid = NetUid::from(1u16);

    let _: Option<pallet_subtensor::rpc_info::metagraph::Metagraph<sp_runtime::AccountId32>> =
        SubnetInfoRuntimeApi::get_metagraph(&MockApi, at, netuid).unwrap();
}

#[test]
fn indexer_runtime_api_get_mechagraph() {
    let at = <Block as BlockT>::Hash::default();
    let netuid = NetUid::from(1u16);
    let mecid = MechId::from(0u8);

    let _: Option<pallet_subtensor::rpc_info::metagraph::Metagraph<sp_runtime::AccountId32>> =
        SubnetInfoRuntimeApi::get_mechagraph(&MockApi, at, netuid, mecid).unwrap();
}

#[test]
fn indexer_runtime_api_stake_info_for_coldkey() {
    let at = <Block as BlockT>::Hash::default();
    let acct = sp_runtime::AccountId32::new([0u8; 32]);

    let _: Vec<pallet_subtensor::rpc_info::stake_info::StakeInfo<sp_runtime::AccountId32>> =
        StakeInfoRuntimeApi::get_stake_info_for_coldkey(&MockApi, at, acct).unwrap();
}

#[test]
fn indexer_runtime_api_stake_info_for_hotkey_coldkey_netuid() {
    let at = <Block as BlockT>::Hash::default();
    let hotkey = sp_runtime::AccountId32::new([1u8; 32]);
    let coldkey = sp_runtime::AccountId32::new([2u8; 32]);
    let netuid = NetUid::from(1u16);

    let _: Option<pallet_subtensor::rpc_info::stake_info::StakeInfo<sp_runtime::AccountId32>> =
        StakeInfoRuntimeApi::get_stake_info_for_hotkey_coldkey_netuid(
            &MockApi, at, hotkey, coldkey, netuid,
        )
        .unwrap();
}
