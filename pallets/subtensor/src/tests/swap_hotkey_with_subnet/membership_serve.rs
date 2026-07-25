#![allow(unused, clippy::indexing_slicing, clippy::panic, clippy::unwrap_used)]

use approx::assert_abs_diff_eq;
use codec::Encode;
use frame_support::weights::Weight;
use frame_support::{assert_err, assert_noop, assert_ok};
use frame_system::{Config, RawOrigin};
use subtensor_runtime_common::{AlphaBalance, NetUidStorageIndex, TaoBalance, Token};

use super::super::mock::*;
use crate::*;
use share_pool::SafeFloat;
use sp_core::{Get, H160, H256, U256};
use sp_runtime::{PerU16, SaturatedConversion};
use sp_std::collections::vec_deque::VecDeque;
use std::collections::BTreeSet;
use substrate_fixed::types::{I96F32, U64F64};

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_subnet_membership --exact --nocapture
#[test]
fn test_swap_subnet_membership() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);

        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        IsNetworkMember::<Test>::insert(old_hotkey, netuid, true);
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ));

        assert!(!IsNetworkMember::<Test>::contains_key(old_hotkey, netuid));
        assert!(IsNetworkMember::<Test>::get(new_hotkey, netuid));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_uids_and_keys --exact --nocapture
#[test]
fn test_swap_uids_and_keys() {
    new_test_ext(1).execute_with(|| {
        let uid = 5u16;
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);

        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        IsNetworkMember::<Test>::insert(old_hotkey, netuid, true);
        Uids::<Test>::insert(netuid, old_hotkey, uid);
        Keys::<Test>::insert(netuid, uid, old_hotkey);

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ));

        assert_eq!(Uids::<Test>::get(netuid, old_hotkey), None);
        assert_eq!(Uids::<Test>::get(netuid, new_hotkey), Some(uid));
        assert_eq!(Keys::<Test>::get(netuid, uid), new_hotkey);
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_prometheus --exact --nocapture
#[test]
fn test_swap_prometheus() {
    new_test_ext(1).execute_with(|| {
        let uid = 5u16;
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);

        let prometheus_info = PrometheusInfo::default();

        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        IsNetworkMember::<Test>::insert(old_hotkey, netuid, true);
        Prometheus::<Test>::insert(netuid, old_hotkey, prometheus_info.clone());

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ));

        assert!(!Prometheus::<Test>::contains_key(netuid, old_hotkey));
        assert_eq!(
            Prometheus::<Test>::get(netuid, new_hotkey),
            Some(prometheus_info)
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_axons --exact --nocapture
#[test]
fn test_swap_axons() {
    new_test_ext(1).execute_with(|| {
        let uid = 5u16;
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);

        let axon_info = AxonInfo::default();

        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        IsNetworkMember::<Test>::insert(old_hotkey, netuid, true);
        Axons::<Test>::insert(netuid, old_hotkey, axon_info.clone());

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ));

        assert!(!Axons::<Test>::contains_key(netuid, old_hotkey));
        assert_eq!(Axons::<Test>::get(netuid, new_hotkey), Some(axon_info));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_certificates --exact --nocapture
#[test]
fn test_swap_certificates() {
    new_test_ext(1).execute_with(|| {
        let uid = 5u16;
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);

        let certificate = NeuronCertificate::try_from(vec![1, 2, 3]).unwrap();

        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        IsNetworkMember::<Test>::insert(old_hotkey, netuid, true);
        NeuronCertificates::<Test>::insert(netuid, old_hotkey, certificate.clone());

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ));

        assert!(!NeuronCertificates::<Test>::contains_key(
            netuid, old_hotkey
        ));
        assert_eq!(
            NeuronCertificates::<Test>::get(netuid, new_hotkey),
            Some(certificate)
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_weight_commits --exact --nocapture
#[test]
fn test_swap_weight_commits() {
    new_test_ext(1).execute_with(|| {
        let uid = 5u16;
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);

        let mut weight_commits: VecDeque<(H256, u64, u64, u64)> = VecDeque::new();
        weight_commits.push_back((H256::from_low_u64_be(100), 200, 1, 1));

        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        IsNetworkMember::<Test>::insert(old_hotkey, netuid, true);
        WeightCommits::<Test>::insert(
            NetUidStorageIndex::from(netuid),
            old_hotkey,
            weight_commits.clone(),
        );

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ));

        assert!(!WeightCommits::<Test>::contains_key(
            NetUidStorageIndex::from(netuid),
            old_hotkey
        ));
        assert_eq!(
            WeightCommits::<Test>::get(NetUidStorageIndex::from(netuid), new_hotkey),
            Some(weight_commits)
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_loaded_emission --exact --nocapture
#[test]
fn test_swap_loaded_emission() {
    new_test_ext(1).execute_with(|| {
        let uid = 5u16;
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);

        let server_emission = 1000u64;
        let validator_emission = 1000u64;

        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        IsNetworkMember::<Test>::insert(old_hotkey, netuid, true);
        LoadedEmission::<Test>::insert(
            netuid,
            vec![(old_hotkey, server_emission, validator_emission)],
        );

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ));

        let new_loaded_emission = LoadedEmission::<Test>::get(netuid);
        assert_eq!(
            new_loaded_emission,
            Some(vec![(new_hotkey, server_emission, validator_emission)])
        );
    });
}
