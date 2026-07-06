#![allow(unused, clippy::indexing_slicing, clippy::panic, clippy::unwrap_used)]

use approx::assert_abs_diff_eq;
use codec::Encode;
use frame_support::weights::Weight;
use frame_support::{assert_err, assert_noop, assert_ok};
use frame_system::{Config, RawOrigin};
use subtensor_runtime_common::{AlphaBalance, NetUidStorageIndex, TaoBalance, Token};

use super::mock::*;
use crate::*;
use share_pool::SafeFloat;
use sp_core::{Get, H160, H256, U256};
use sp_runtime::SaturatedConversion;
use std::collections::BTreeSet;
use substrate_fixed::types::{I96F32, U64F64};

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_owner --exact --nocapture
#[test]
fn test_swap_owner() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);

        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());
        Owner::<Test>::insert(old_hotkey, coldkey);
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false,
        ));

        assert_eq!(Owner::<Test>::get(old_hotkey), coldkey);
        assert_eq!(Owner::<Test>::get(new_hotkey), coldkey);
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_owned_hotkeys --exact --nocapture
#[test]
fn test_swap_owned_hotkeys() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);

        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        OwnedHotkeys::<Test>::insert(coldkey, vec![old_hotkey]);
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ));

        let hotkeys = OwnedHotkeys::<Test>::get(coldkey);
        assert!(hotkeys.contains(&old_hotkey));
        assert!(hotkeys.contains(&new_hotkey));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_total_hotkey_stake --exact --nocapture
#[test]
fn test_swap_total_hotkey_stake() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let amount = DefaultMinStake::<Test>::get().to_u64() * 10;

        let fee = (amount as f64 * 0.003) as u64;

        //add network
        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        remove_owner_registration_stake(netuid);

        // Give it some $$$ in his coldkey balance
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        // Add stake
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey),
            old_hotkey,
            netuid,
            amount.into()
        ));

        // Check if stake has increased
        assert_abs_diff_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&old_hotkey),
            (amount - fee).into(),
            epsilon = TaoBalance::from(amount / 100),
        );
        assert_abs_diff_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&new_hotkey),
            TaoBalance::ZERO,
            epsilon = 1.into(),
        );

        // Swap hotkey
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ));

        // Verify that total hotkey stake swapped
        assert_abs_diff_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&old_hotkey),
            TaoBalance::ZERO,
            epsilon = 1.into(),
        );
        assert_abs_diff_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&new_hotkey),
            TaoBalance::from(amount - fee),
            epsilon = TaoBalance::from(amount / 100),
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_delegates --exact --nocapture
#[test]
fn test_swap_delegates() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);

        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        Delegates::<Test>::insert(old_hotkey, 100);
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ));

        assert!(Delegates::<Test>::contains_key(old_hotkey));
        assert!(!Delegates::<Test>::contains_key(new_hotkey));
    });
}

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
use sp_std::collections::vec_deque::VecDeque;
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

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_staking_hotkeys --exact --nocapture
#[test]
fn test_swap_staking_hotkeys() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        StakingHotkeys::<Test>::insert(coldkey, vec![old_hotkey]);
        Alpha::<Test>::insert((old_hotkey, coldkey, netuid), U64F64::from_num(100));

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ));

        let staking_hotkeys = StakingHotkeys::<Test>::get(coldkey);
        assert!(staking_hotkeys.contains(&old_hotkey));
        assert!(staking_hotkeys.contains(&new_hotkey));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey::test_swap_hotkey_with_multiple_coldkeys --exact --show-output --nocapture
#[test]
fn test_swap_hotkey_with_multiple_coldkeys() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey1 = U256::from(3);
        let coldkey2 = U256::from(4);

        let stake = 1_000_000_000;

        StakingHotkeys::<Test>::insert(coldkey1, vec![old_hotkey]);
        StakingHotkeys::<Test>::insert(coldkey2, vec![old_hotkey]);
        SubtensorModule::create_account_if_non_existent(&coldkey1, &old_hotkey);
        add_balance_to_coldkey_account(&coldkey1, 1_000_000_000_000_u64.into());
        add_balance_to_coldkey_account(&coldkey2, 1_000_000_000_000_u64.into());

        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey1),
            old_hotkey,
            netuid,
            stake.into()
        ));
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey2),
            old_hotkey,
            netuid,
            TaoBalance::from(stake / 2)
        ));
        let stake1_before = SubtensorModule::get_total_stake_for_coldkey(&coldkey1);
        let stake2_before = SubtensorModule::get_total_stake_for_coldkey(&coldkey2);

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey1),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ));

        assert_eq!(
            SubtensorModule::get_total_stake_for_coldkey(&coldkey1),
            SubtensorModule::get_total_stake_for_coldkey(&coldkey1),
        );
        assert_eq!(
            SubtensorModule::get_total_stake_for_coldkey(&coldkey2),
            SubtensorModule::get_total_stake_for_coldkey(&coldkey2),
        );

        assert_eq!(
            SubtensorModule::get_total_stake_for_coldkey(&coldkey1),
            stake1_before
        );
        assert_eq!(
            SubtensorModule::get_total_stake_for_coldkey(&coldkey2),
            stake2_before
        );

        assert!(StakingHotkeys::<Test>::get(coldkey1).contains(&new_hotkey));
        assert!(StakingHotkeys::<Test>::get(coldkey2).contains(&new_hotkey));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_hotkey_with_multiple_subnets --exact --nocapture
#[test]
fn test_swap_hotkey_with_multiple_subnets() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let new_hotkey_2 = U256::from(3);
        let coldkey = U256::from(4);

        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        let netuid1 = add_dynamic_network(&old_hotkey, &coldkey);
        let netuid2 = add_dynamic_network(&old_hotkey, &coldkey);

        IsNetworkMember::<Test>::insert(old_hotkey, netuid1, true);
        IsNetworkMember::<Test>::insert(old_hotkey, netuid2, true);

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid1),
            false
        ));

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey_2,
            Some(netuid2),
            false
        ));

        assert!(IsNetworkMember::<Test>::get(new_hotkey, netuid1));
        assert!(IsNetworkMember::<Test>::get(new_hotkey_2, netuid2));
        assert!(!IsNetworkMember::<Test>::get(old_hotkey, netuid1));
        assert!(!IsNetworkMember::<Test>::get(old_hotkey, netuid2));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_staking_hotkeys_multiple_coldkeys --exact --nocapture
#[test]
fn test_swap_staking_hotkeys_multiple_coldkeys() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey1 = U256::from(3);
        let coldkey2 = U256::from(4);
        let staker5 = U256::from(5);

        let stake = 1_000_000_000;
        add_balance_to_coldkey_account(&coldkey1, 1_000_000_000_000_u64.into());
        add_balance_to_coldkey_account(&coldkey2, 1_000_000_000_000_u64.into());

        // Set up initial state
        StakingHotkeys::<Test>::insert(coldkey1, vec![old_hotkey]);
        StakingHotkeys::<Test>::insert(coldkey2, vec![old_hotkey, staker5]);

        SubtensorModule::create_account_if_non_existent(&coldkey1, &old_hotkey);

        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey1),
            old_hotkey,
            netuid,
            stake.into()
        ));
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey2),
            old_hotkey,
            netuid,
            stake.into()
        ));

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey1),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ));

        // Check if new_hotkey replaced old_hotkey in StakingHotkeys
        assert!(StakingHotkeys::<Test>::get(coldkey1).contains(&new_hotkey));
        assert!(StakingHotkeys::<Test>::get(coldkey1).contains(&old_hotkey));

        // Check if new_hotkey replaced old_hotkey for coldkey2 as well
        assert!(StakingHotkeys::<Test>::get(coldkey2).contains(&new_hotkey));
        assert!(StakingHotkeys::<Test>::get(coldkey2).contains(&old_hotkey));
        assert!(StakingHotkeys::<Test>::get(coldkey2).contains(&staker5));
        // Other hotkeys should remain
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_hotkey_with_no_stake --exact --nocapture
#[test]
fn test_swap_hotkey_with_no_stake() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);

        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        // Set up initial state with no stake
        Owner::<Test>::insert(old_hotkey, coldkey);

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ));

        // Check if ownership transferred
        assert!(Owner::<Test>::contains_key(old_hotkey));
        assert_eq!(Owner::<Test>::get(new_hotkey), coldkey);

        // Ensure no unexpected changes in Stake
        assert!(!Alpha::<Test>::contains_key((old_hotkey, coldkey, netuid)));
        assert!(!Alpha::<Test>::contains_key((new_hotkey, coldkey, netuid)));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey::test_swap_hotkey_with_multiple_coldkeys_and_subnets --exact --show-output
#[test]
fn test_swap_hotkey_with_multiple_coldkeys_and_subnets() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let new_hotkey_2 = U256::from(3);
        let coldkey1 = U256::from(4);
        let coldkey2 = U256::from(5);
        let netuid1 = NetUid::from(1);
        let netuid2 = NetUid::from(2);
        let stake = DefaultMinStake::<Test>::get().to_u64() * 10;

        // Set up initial state
        add_network(netuid1, 1, 1);
        add_network(netuid2, 1, 1);
        register_ok_neuron(netuid1, old_hotkey, coldkey1, 1234);
        register_ok_neuron(netuid2, old_hotkey, coldkey1, 1234);

        // Add balance to both coldkeys
        add_balance_to_coldkey_account(&coldkey1, 1_000_000_000_000_u64.into());
        add_balance_to_coldkey_account(&coldkey2, 1_000_000_000_000_u64.into());

        // Stake with coldkey1
        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey1),
            old_hotkey,
            netuid1,
            stake.into()
        ));

        // Stake with coldkey2 also
        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey2),
            old_hotkey,
            netuid2,
            stake.into()
        ));

        let ck1_stake = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &old_hotkey,
            &coldkey1,
            netuid1,
        );
        let ck2_stake = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &old_hotkey,
            &coldkey2,
            netuid2,
        );
        assert!(!ck1_stake.is_zero());
        assert!(!ck2_stake.is_zero());
        let total_hk_stake = SubtensorModule::get_total_stake_for_hotkey(&old_hotkey);
        assert!(!total_hk_stake.is_zero());
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());

        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey1),
            &old_hotkey,
            &new_hotkey,
            Some(netuid1),
            false
        ));

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey1),
            &old_hotkey,
            &new_hotkey_2,
            Some(netuid2),
            false
        ));

        // Check ownership transfer
        assert_eq!(
            SubtensorModule::get_owning_coldkey_for_hotkey(&new_hotkey),
            coldkey1
        );
        assert!(!SubtensorModule::get_owned_hotkeys(&coldkey2).contains(&new_hotkey));
        assert_eq!(
            SubtensorModule::get_owning_coldkey_for_hotkey(&new_hotkey_2),
            coldkey1
        );
        assert!(!SubtensorModule::get_owned_hotkeys(&coldkey2).contains(&new_hotkey_2));

        // Check stake transfer
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &new_hotkey,
                &coldkey1,
                netuid1
            ),
            ck1_stake
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &new_hotkey_2,
                &coldkey2,
                netuid2
            ),
            ck2_stake
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &old_hotkey,
                &coldkey1,
                netuid1
            ),
            AlphaBalance::ZERO
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &old_hotkey,
                &coldkey2,
                netuid2
            ),
            AlphaBalance::ZERO
        );

        // Check subnet membership transfer
        assert!(SubtensorModule::is_hotkey_registered_on_network(
            netuid1,
            &new_hotkey
        ));
        assert!(SubtensorModule::is_hotkey_registered_on_network(
            netuid2,
            &new_hotkey_2
        ));
        assert!(!SubtensorModule::is_hotkey_registered_on_network(
            netuid1,
            &old_hotkey
        ));
        assert!(!SubtensorModule::is_hotkey_registered_on_network(
            netuid2,
            &old_hotkey
        ));

        // Check total stake transfer
        assert_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&new_hotkey)
                + SubtensorModule::get_total_stake_for_hotkey(&new_hotkey_2),
            total_hk_stake
        );
        assert_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&old_hotkey),
            TaoBalance::ZERO
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_do_swap_hotkey_err_not_owner --exact --nocapture
#[test]
fn test_do_swap_hotkey_err_not_owner() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let not_owner_coldkey = U256::from(4);
        let swap_cost = TaoBalance::from(1_000_000_000u64);

        // Setup initial state
        add_network(netuid, tempo, 0);
        register_ok_neuron(netuid, old_hotkey, coldkey, 0);
        add_balance_to_coldkey_account(&not_owner_coldkey, swap_cost);

        // Attempt the swap with a non-owner coldkey
        assert_err!(
            SubtensorModule::do_swap_hotkey(
                RuntimeOrigin::signed(not_owner_coldkey),
                &old_hotkey,
                &new_hotkey,
                Some(netuid),
                false
            ),
            Error::<Test>::NonAssociatedColdKey
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_owner_old_hotkey_not_exist --exact --nocapture
#[test]
fn test_swap_owner_old_hotkey_not_exist() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);

        let netuid = add_dynamic_network(&new_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        // Ensure old_hotkey does not exist
        assert!(!Owner::<Test>::contains_key(old_hotkey));

        // Perform the swap
        assert_err!(
            SubtensorModule::do_swap_hotkey(
                RuntimeOrigin::signed(coldkey),
                &old_hotkey,
                &new_hotkey,
                Some(netuid),
                false
            ),
            Error::<Test>::NonAssociatedColdKey
        );

        // Verify the swap
        assert_eq!(Owner::<Test>::get(new_hotkey), coldkey);
        assert!(!Owner::<Test>::contains_key(old_hotkey));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_owner_new_hotkey_already_exists --exact --nocapture
#[test]
fn test_swap_owner_new_hotkey_already_exists() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);

        let netuid = add_dynamic_network(&new_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        // old_hotkey is owned by coldkey; new_hotkey was already registered on `netuid`
        // by add_dynamic_network (the condition under test). Do NOT reassign new_hotkey to
        // a foreign coldkey — the new_hotkey-ownership check (NonAssociatedColdKey) would
        // then fire before the already-registered-in-subnet check this test targets.
        Owner::<Test>::insert(old_hotkey, coldkey);

        // Perform the swap
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_err!(
            SubtensorModule::do_swap_hotkey(
                RuntimeOrigin::signed(coldkey),
                &old_hotkey,
                &new_hotkey,
                Some(netuid),
                false
            ),
            Error::<Test>::HotKeyAlreadyRegisteredInSubNet
        );

        // Verify the swap
        assert_eq!(Owner::<Test>::get(old_hotkey), coldkey);
        assert!(Owner::<Test>::contains_key(old_hotkey));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_swap_stake_success --exact --nocapture
#[test]
fn test_swap_stake_success() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        remove_owner_registration_stake(netuid);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());
        let amount = 10_000;
        let shares = U64F64::from_num(10_000);

        // Initialize staking variables for old_hotkey
        TotalHotkeyAlpha::<Test>::insert(old_hotkey, netuid, AlphaBalance::from(amount));
        TotalHotkeyAlphaLastEpoch::<Test>::insert(
            old_hotkey,
            netuid,
            AlphaBalance::from(amount * 2),
        );
        TotalHotkeyShares::<Test>::insert(old_hotkey, netuid, U64F64::from_num(shares));
        Alpha::<Test>::insert((old_hotkey, coldkey, netuid), U64F64::from_num(amount));
        AlphaDividendsPerSubnet::<Test>::insert(netuid, old_hotkey, AlphaBalance::from(amount));

        // Perform the swap
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ),);

        // Verify the swap
        assert_eq!(
            TotalHotkeyAlpha::<Test>::get(old_hotkey, netuid),
            AlphaBalance::ZERO
        );
        assert_eq!(
            TotalHotkeyAlpha::<Test>::get(new_hotkey, netuid),
            AlphaBalance::from(amount)
        );
        assert_eq!(
            TotalHotkeyAlphaLastEpoch::<Test>::get(old_hotkey, netuid),
            AlphaBalance::ZERO
        );
        assert_eq!(
            TotalHotkeyAlphaLastEpoch::<Test>::get(new_hotkey, netuid),
            AlphaBalance::from(amount * 2)
        );
        assert_eq!(
            TotalHotkeyShares::<Test>::get(old_hotkey, netuid),
            U64F64::from_num(0)
        );
        assert_eq!(
            TotalHotkeyShares::<Test>::get(new_hotkey, netuid),
            U64F64::from_num(0)
        );
        assert_abs_diff_eq!(
            f64::from(TotalHotkeySharesV2::<Test>::get(new_hotkey, netuid)),
            shares.to_num::<f64>(),
            epsilon = 0.0000000001
        );
        assert_eq!(
            Alpha::<Test>::get((old_hotkey, coldkey, netuid)),
            U64F64::from_num(0)
        );
        assert_eq!(
            Alpha::<Test>::get((new_hotkey, coldkey, netuid)),
            U64F64::from_num(0)
        );
        assert_eq!(
            f64::from(AlphaV2::<Test>::get((new_hotkey, coldkey, netuid))),
            amount as f64
        );
        assert_eq!(
            AlphaDividendsPerSubnet::<Test>::get(netuid, old_hotkey),
            AlphaBalance::ZERO
        );
        assert_eq!(
            AlphaDividendsPerSubnet::<Test>::get(netuid, new_hotkey),
            AlphaBalance::from(amount)
        );
    });
}

#[test]
fn test_swap_stake_v2_success() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());
        let amount = 10_000;
        let shares = U64F64::from_num(10_000);

        // Initialize staking variables for old_hotkey
        TotalHotkeyAlpha::<Test>::insert(old_hotkey, netuid, AlphaBalance::from(amount));
        TotalHotkeyAlphaLastEpoch::<Test>::insert(
            old_hotkey,
            netuid,
            AlphaBalance::from(amount * 2),
        );
        TotalHotkeySharesV2::<Test>::insert(old_hotkey, netuid, SafeFloat::from(shares));
        AlphaV2::<Test>::insert(
            (old_hotkey, coldkey, netuid),
            SafeFloat::from(U64F64::from_num(amount)),
        );
        AlphaDividendsPerSubnet::<Test>::insert(netuid, old_hotkey, AlphaBalance::from(amount));

        // Perform the swap
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false,
        ),);

        // Verify the swap
        assert_eq!(
            TotalHotkeyAlpha::<Test>::get(old_hotkey, netuid),
            AlphaBalance::ZERO
        );
        assert_eq!(
            TotalHotkeyAlpha::<Test>::get(new_hotkey, netuid),
            AlphaBalance::from(amount)
        );
        assert_eq!(
            TotalHotkeyAlphaLastEpoch::<Test>::get(old_hotkey, netuid),
            AlphaBalance::ZERO
        );
        assert_eq!(
            TotalHotkeyAlphaLastEpoch::<Test>::get(new_hotkey, netuid),
            AlphaBalance::from(amount * 2)
        );
        assert_eq!(
            f64::from(TotalHotkeySharesV2::<Test>::get(old_hotkey, netuid)),
            0_f64
        );
        assert_abs_diff_eq!(
            f64::from(TotalHotkeySharesV2::<Test>::get(new_hotkey, netuid)),
            shares.to_num::<f64>(),
            epsilon = 0.0000000001
        );
        assert_eq!(
            f64::from(AlphaV2::<Test>::get((old_hotkey, coldkey, netuid))),
            0_f64
        );
        assert_eq!(
            f64::from(AlphaV2::<Test>::get((new_hotkey, coldkey, netuid))),
            amount as f64
        );
        assert_eq!(
            AlphaDividendsPerSubnet::<Test>::get(netuid, old_hotkey),
            AlphaBalance::ZERO
        );
        assert_eq!(
            AlphaDividendsPerSubnet::<Test>::get(netuid, new_hotkey),
            AlphaBalance::from(amount)
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_hotkey_error_cases --exact --nocapture
#[test]
fn test_swap_hotkey_error_cases() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let wrong_coldkey = U256::from(4);
        let netuid = add_dynamic_network(&old_hotkey, &coldkey);

        // Set up initial state
        Owner::<Test>::insert(old_hotkey, coldkey);
        TotalNetworks::<Test>::put(1);
        SubtensorModule::set_last_tx_block(&coldkey, 0);

        // Test not enough balance
        let swap_cost = SubtensorModule::get_key_swap_cost();
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_err!(
            SubtensorModule::do_swap_hotkey(
                RuntimeOrigin::signed(coldkey),
                &old_hotkey,
                &new_hotkey,
                Some(netuid),
                false
            ),
            Error::<Test>::NotEnoughBalanceToPaySwapHotKey
        );

        let initial_balance = SubtensorModule::get_key_swap_cost() + 1000.into();
        add_balance_to_coldkey_account(&coldkey, initial_balance);

        // Test new hotkey same as old
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_noop!(
            SubtensorModule::do_swap_hotkey(
                RuntimeOrigin::signed(coldkey),
                &old_hotkey,
                &old_hotkey,
                Some(netuid),
                false
            ),
            Error::<Test>::NewHotKeyIsSameWithOld
        );

        // Test new hotkey already registered
        IsNetworkMember::<Test>::insert(new_hotkey, netuid, true);
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_noop!(
            SubtensorModule::do_swap_hotkey(
                RuntimeOrigin::signed(coldkey),
                &old_hotkey,
                &new_hotkey,
                Some(netuid),
                false
            ),
            Error::<Test>::HotKeyAlreadyRegisteredInSubNet
        );
        IsNetworkMember::<Test>::remove(new_hotkey, netuid);

        // Test non-associated coldkey
        assert_noop!(
            SubtensorModule::do_swap_hotkey(
                RuntimeOrigin::signed(wrong_coldkey),
                &old_hotkey,
                &new_hotkey,
                Some(netuid),
                false
            ),
            Error::<Test>::NonAssociatedColdKey
        );

        // Run the successful swap
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ),);
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_child_keys --exact --nocapture
#[test]
fn test_swap_child_keys() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        let children = vec![(100u64, U256::from(4)), (200u64, U256::from(5))];

        // Initialize ChildKeys for old_hotkey
        ChildKeys::<Test>::insert(old_hotkey, netuid, children.clone());

        // Perform the swap
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ),);

        // Verify the swap
        assert_eq!(ChildKeys::<Test>::get(new_hotkey, netuid), children);
        assert!(ChildKeys::<Test>::get(old_hotkey, netuid).is_empty());
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_swap_child_keys_self_loop --exact --show-output
#[test]
#[allow(deprecated)]
fn test_swap_child_keys_self_loop() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        let amount = AlphaBalance::from(12345);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        // Only for checking
        TotalHotkeyAlpha::<Test>::insert(old_hotkey, netuid, AlphaBalance::from(amount));

        let children = vec![(200u64, new_hotkey)];

        // Initialize ChildKeys for old_hotkey
        ChildKeys::<Test>::insert(old_hotkey, netuid, children.clone());

        // Perform the swap extrinsic
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_err!(
            SubtensorModule::swap_hotkey(
                RuntimeOrigin::signed(coldkey),
                old_hotkey,
                new_hotkey,
                Some(netuid),
            ),
            Error::<Test>::InvalidChild
        );

        // Verify the swap didn't happen
        assert_eq!(ChildKeys::<Test>::get(old_hotkey, netuid), children);
        assert!(ChildKeys::<Test>::get(new_hotkey, netuid).is_empty());
        assert_eq!(TotalHotkeyAlpha::<Test>::get(old_hotkey, netuid), amount);
        assert_eq!(
            TotalHotkeyAlpha::<Test>::get(new_hotkey, netuid),
            AlphaBalance::from(0)
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_parent_keys --exact --nocapture
#[test]
fn test_swap_parent_keys() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());
        let parents = vec![(100u64, U256::from(4)), (200u64, U256::from(5))];

        // Initialize ParentKeys for old_hotkey
        ParentKeys::<Test>::insert(old_hotkey, netuid, parents.clone());

        // Initialize ChildKeys for parent
        ChildKeys::<Test>::insert(U256::from(4), netuid, vec![(100u64, old_hotkey)]);
        ChildKeys::<Test>::insert(U256::from(5), netuid, vec![(200u64, old_hotkey)]);

        // Perform the swap
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ),);

        // Verify ParentKeys swap
        assert_eq!(ParentKeys::<Test>::get(new_hotkey, netuid), parents);
        assert!(ParentKeys::<Test>::get(old_hotkey, netuid).is_empty());

        // Verify ChildKeys update for parents
        assert_eq!(
            ChildKeys::<Test>::get(U256::from(4), netuid),
            vec![(100u64, new_hotkey)]
        );
        assert_eq!(
            ChildKeys::<Test>::get(U256::from(5), netuid),
            vec![(200u64, new_hotkey)]
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_multiple_subnets --exact --nocapture
#[test]
fn test_swap_multiple_subnets() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let new_hotkey_2 = U256::from(3);
        let coldkey = U256::from(4);
        let netuid1 = add_dynamic_network(&old_hotkey, &coldkey);
        let netuid2 = add_dynamic_network(&old_hotkey, &coldkey);

        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        let children1 = vec![(100u64, U256::from(4)), (200u64, U256::from(5))];
        let children2 = vec![(300u64, U256::from(6))];

        // Initialize ChildKeys for old_hotkey in multiple subnets
        ChildKeys::<Test>::insert(old_hotkey, netuid1, children1.clone());
        ChildKeys::<Test>::insert(old_hotkey, netuid2, children2.clone());

        // Perform the swap
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid1),
            false
        ),);

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey_2,
            Some(netuid2),
            false
        ),);

        // Verify the swap for both subnets
        assert_eq!(ChildKeys::<Test>::get(new_hotkey, netuid1), children1);
        assert_eq!(ChildKeys::<Test>::get(new_hotkey_2, netuid2), children2);
        assert!(ChildKeys::<Test>::get(old_hotkey, netuid1).is_empty());
        assert!(ChildKeys::<Test>::get(old_hotkey, netuid2).is_empty());
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_complex_parent_child_structure --exact --nocapture
#[test]
fn test_swap_complex_parent_child_structure() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());
        let parent1 = U256::from(4);
        let parent2 = U256::from(5);
        let child1 = U256::from(6);
        let child2 = U256::from(7);

        // Set up complex parent-child structure
        ParentKeys::<Test>::insert(
            old_hotkey,
            netuid,
            vec![(100u64, parent1), (200u64, parent2)],
        );
        ChildKeys::<Test>::insert(old_hotkey, netuid, vec![(300u64, child1), (400u64, child2)]);
        ChildKeys::<Test>::insert(
            parent1,
            netuid,
            vec![(100u64, old_hotkey), (500u64, U256::from(8))],
        );
        ChildKeys::<Test>::insert(
            parent2,
            netuid,
            vec![(200u64, old_hotkey), (600u64, U256::from(9))],
        );

        // Perform the swap
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ),);

        // Verify ParentKeys swap
        assert_eq!(
            ParentKeys::<Test>::get(new_hotkey, netuid),
            vec![(100u64, parent1), (200u64, parent2)]
        );
        assert!(ParentKeys::<Test>::get(old_hotkey, netuid).is_empty());

        // Verify ChildKeys swap
        assert_eq!(
            ChildKeys::<Test>::get(new_hotkey, netuid),
            vec![(300u64, child1), (400u64, child2)]
        );
        assert!(ChildKeys::<Test>::get(old_hotkey, netuid).is_empty());

        // Verify parent's ChildKeys update
        assert!(ChildKeys::<Test>::get(parent1, netuid).contains(&(500u64, U256::from(8))),);
        assert!(ChildKeys::<Test>::get(parent1, netuid).contains(&(100u64, new_hotkey)),);
        assert!(ChildKeys::<Test>::get(parent2, netuid).contains(&(600u64, U256::from(9))),);
        assert!(ChildKeys::<Test>::get(parent2, netuid).contains(&(200u64, new_hotkey)),);
    });
}

#[test]
fn test_swap_parent_hotkey_childkey_maps() {
    new_test_ext(1).execute_with(|| {
        let parent_old = U256::from(1);
        let coldkey = U256::from(2);
        let child = U256::from(3);
        let child_other = U256::from(4);
        let parent_new = U256::from(5);

        let netuid = add_dynamic_network(&parent_old, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        SubtensorModule::create_account_if_non_existent(&coldkey, &parent_old);

        // Set child and verify state maps
        mock_set_children(&coldkey, &parent_old, netuid, &[(u64::MAX, child)]);
        // Wait rate limit
        step_rate_limit(&TransactionType::SetChildren, netuid);
        // Schedule some pending child keys.
        mock_schedule_children(&coldkey, &parent_old, netuid, &[(u64::MAX, child_other)]);

        assert_eq!(
            ParentKeys::<Test>::get(child, netuid),
            vec![(u64::MAX, parent_old)]
        );
        assert_eq!(
            ChildKeys::<Test>::get(parent_old, netuid),
            vec![(u64::MAX, child)]
        );
        let existing_pending_child_keys = PendingChildKeys::<Test>::get(netuid, parent_old);
        assert_eq!(existing_pending_child_keys.0, vec![(u64::MAX, child_other)]);

        // Swap

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &parent_old,
            &parent_new,
            Some(netuid),
            false
        ),);

        // Verify parent and child keys updates
        assert_eq!(
            ParentKeys::<Test>::get(child, netuid),
            vec![(u64::MAX, parent_new)]
        );
        assert_eq!(
            ChildKeys::<Test>::get(parent_new, netuid),
            vec![(u64::MAX, child)]
        );
        assert_eq!(
            PendingChildKeys::<Test>::get(netuid, parent_new),
            existing_pending_child_keys // Entry under new hotkey.
        );
    })
}

#[test]
fn test_swap_child_hotkey_childkey_maps() {
    new_test_ext(1).execute_with(|| {
        let parent = U256::from(1);
        let coldkey = U256::from(2);
        let child_old = U256::from(3);
        let child_new = U256::from(4);
        let netuid = add_dynamic_network(&child_old, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        SubtensorModule::create_account_if_non_existent(&coldkey, &child_old);
        SubtensorModule::create_account_if_non_existent(&coldkey, &parent);

        // Set child and verify state maps
        mock_set_children(&coldkey, &parent, netuid, &[(u64::MAX, child_old)]);
        // Wait rate limit
        step_rate_limit(&TransactionType::SetChildren, netuid);
        // Schedule some pending child keys.
        mock_schedule_children(&coldkey, &parent, netuid, &[(u64::MAX, child_old)]);

        assert_eq!(
            ParentKeys::<Test>::get(child_old, netuid),
            vec![(u64::MAX, parent)]
        );
        assert_eq!(
            ChildKeys::<Test>::get(parent, netuid),
            vec![(u64::MAX, child_old)]
        );
        let existing_pending_child_keys = PendingChildKeys::<Test>::get(netuid, parent);
        assert_eq!(existing_pending_child_keys.0, vec![(u64::MAX, child_old)]);

        // Swap

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &child_old,
            &child_new,
            Some(netuid),
            false
        ),);

        // Verify parent and child keys updates
        assert_eq!(
            ParentKeys::<Test>::get(child_new, netuid),
            vec![(u64::MAX, parent)]
        );
        assert_eq!(
            ChildKeys::<Test>::get(parent, netuid),
            vec![(u64::MAX, child_new)]
        );
        assert_eq!(
            PendingChildKeys::<Test>::get(netuid, parent),
            (vec![(u64::MAX, child_new)], existing_pending_child_keys.1) // Same cooldown block.
        );
    })
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_hotkey_is_sn_owner_hotkey --exact --nocapture
#[test]
fn test_swap_hotkey_is_sn_owner_hotkey() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);

        // Create dynamic network
        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        // Check for SubnetOwnerHotkey
        assert_eq!(SubnetOwnerHotkey::<Test>::get(netuid), old_hotkey);

        // Perform the swap
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ),);

        // Check for SubnetOwnerHotkey
        assert_eq!(SubnetOwnerHotkey::<Test>::get(netuid), new_hotkey);
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey_with_subnet -- test_swap_hotkey_swap_rate_limits --exact --nocapture
#[test]
fn test_swap_hotkey_swap_rate_limits() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);

        let delegate_take_block = 4567;
        let child_key_take_block = 8910;

        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        // Set the last delegate take block for the old hotkey
        SubtensorModule::set_last_tx_block_delegate_take(&old_hotkey, delegate_take_block);
        // Set last childkey take block for the old hotkey
        SubtensorModule::set_last_tx_block_childkey(&old_hotkey, child_key_take_block);

        // Perform the swap
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ),);

        // Check for new hotkey (LastTxBlock is no longer transferred: the generic tx rate
        // limit was removed.
        assert_eq!(
            SubtensorModule::get_last_tx_block_delegate_take(&new_hotkey),
            delegate_take_block
        );
        assert_eq!(
            SubtensorModule::get_last_tx_block_childkey_take(&new_hotkey),
            child_key_take_block
        );
    });
}

#[test]
fn test_swap_owner_failed_interval_not_passed() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);

        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());
        Owner::<Test>::insert(old_hotkey, coldkey);
        assert_err!(
            SubtensorModule::do_swap_hotkey(
                RuntimeOrigin::signed(coldkey),
                &old_hotkey,
                &new_hotkey,
                Some(netuid),
                false
            ),
            Error::<Test>::HotKeySwapOnSubnetIntervalNotPassed,
        );
    });
}

#[test]
fn test_swap_owner_check_swap_block_set() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);

        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());
        Owner::<Test>::insert(old_hotkey, coldkey);
        let new_block_number = System::block_number() + HotkeySwapOnSubnetInterval::get();
        System::set_block_number(new_block_number);
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ));

        assert_eq!(
            LastHotkeySwapOnNetuid::<Test>::get(netuid, coldkey),
            new_block_number
        );
    });
}

#[test]
fn test_swap_owner_check_swap_record_clean_up() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());
        Owner::<Test>::insert(old_hotkey, coldkey);
        let new_block_number = System::block_number() + HotkeySwapOnSubnetInterval::get();
        System::set_block_number(new_block_number);
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ));

        assert_eq!(
            LastHotkeySwapOnNetuid::<Test>::get(netuid, coldkey),
            new_block_number
        );

        step_block((HotkeySwapOnSubnetInterval::get() as u16 + u16::from(netuid)) * 2);
        assert!(!LastHotkeySwapOnNetuid::<Test>::contains_key(
            netuid, coldkey
        ));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_revert_hotkey_swap_stake_is_not_lost --exact --nocapture
#[test]
fn test_revert_hotkey_swap_stake_is_not_lost() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let netuid2 = NetUid::from(2);
        let tempo: u16 = 13;
        let hk1 = U256::from(1);
        let hk2 = U256::from(2);
        let coldkey = U256::from(3);
        let swap_cost = 1_000_000_000u64 * 2;
        let stake2 = 1_000_000_000u64;

        // Setup
        add_network(netuid, tempo, 0);
        add_network(netuid2, tempo, 0);
        register_ok_neuron(netuid, hk1, coldkey, 0);
        register_ok_neuron(netuid2, hk1, coldkey, 0);
        add_balance_to_coldkey_account(&coldkey, swap_cost.into());

        let hk1_stake_before_increase =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hk1, &coldkey, netuid);
        assert!(
            hk1_stake_before_increase == 0.into(),
            "hk1 should have empty stake"
        );

        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hk1,
            &coldkey,
            netuid,
            1_000_000_000u64.into(),
        );

        let hk1_stake_before_swap =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hk1, &coldkey, netuid);
        assert!(
            hk1_stake_before_swap == 1_000_000_000.into(),
            "hk1 should have stake before swap"
        );

        step_block(20);

        assert_ok!(SubtensorModule::do_swap_hotkey(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            &hk1,
            &hk2,
            Some(netuid),
            false
        ));

        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hk1,
            &coldkey,
            netuid,
            stake2.into(),
        );

        step_block(20);

        let hk2_stake_before_revert =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hk2, &coldkey, netuid);
        let hk1_stake_before_revert =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hk1, &coldkey, netuid);

        assert_eq!(hk1_stake_before_revert, stake2.into());

        // Revert: hk2 -> hk1
        assert_ok!(SubtensorModule::do_swap_hotkey(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            &hk2,
            &hk1,
            Some(netuid),
            false
        ));

        let hk1_stake_after_revert =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hk1, &coldkey, netuid);
        let hk2_stake_after_revert =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hk2, &coldkey, netuid);

        assert_eq!(
            hk1_stake_after_revert,
            hk2_stake_before_revert + stake2.into(),
        );

        // hk2 should be empty
        assert_eq!(
            hk2_stake_after_revert,
            0.into(),
            "hk2 should have no stake after revert"
        );
    });
}

// Check swap hotkey with keep_stake doesn't affect stake and related storage maps
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_hotkey_swap_keep_stake --exact --nocapture
#[test]
fn test_hotkey_swap_keep_stake() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let child_key = U256::from(4);
        let coldkey = U256::from(3);
        let swap_cost = 1_000_000_000u64 * 2;
        let stake_amount = 1_000_000_000u64;
        let voting_power_value = 5_000_000_000_000_u64;

        // Setup
        add_network(netuid, tempo, 0);
        register_ok_neuron(netuid, old_hotkey, coldkey, 0);
        add_balance_to_coldkey_account(&coldkey, swap_cost.into());

        VotingPower::<Test>::insert(netuid, old_hotkey, voting_power_value);
        assert_eq!(
            SubtensorModule::get_voting_power(netuid, &old_hotkey),
            voting_power_value
        );

        ChildKeys::<Test>::insert(old_hotkey, netuid, vec![(u64::MAX, child_key)]);
        ParentKeys::<Test>::insert(child_key, netuid, vec![(u64::MAX, old_hotkey)]);

        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &old_hotkey,
            &coldkey,
            netuid,
            stake_amount.into(),
        );

        assert!(SubtensorModule::is_hotkey_registered_on_network(
            netuid,
            &old_hotkey
        ));

        step_block(20);

        let old_hotkey_stake_before_swap =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &old_hotkey,
                &coldkey,
                netuid,
            );

        assert_ok!(SubtensorModule::do_swap_hotkey(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            true
        ));

        let old_hotkey_stake_after = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &old_hotkey,
            &coldkey,
            netuid,
        );
        assert_eq!(
            old_hotkey_stake_after, old_hotkey_stake_before_swap,
            "old_hotkey stake must NOT change during keep_stake swap"
        );

        let new_hotkey_stake_after = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &new_hotkey,
            &coldkey,
            netuid,
        );
        assert_eq!(
            new_hotkey_stake_after,
            0.into(),
            "new_hotkey should have no stake"
        );

        assert!(
            SubtensorModule::is_hotkey_registered_on_network(netuid, &new_hotkey),
            "new_hotkey should be registered on netuid"
        );

        assert!(
            !SubtensorModule::is_hotkey_registered_on_network(netuid, &old_hotkey),
            "old_hotkey should NOT be registered on netuid after swap"
        );

        let root_total_alpha = TotalHotkeyAlpha::<Test>::get(old_hotkey, netuid);
        let child_total_alpha = TotalHotkeyAlpha::<Test>::get(new_hotkey, netuid);
        assert!(
            root_total_alpha > 0.into(),
            "old_hotkey should retain TotalHotkeyAlpha"
        );
        assert_eq!(
            child_total_alpha,
            0.into(),
            "new_hotkey should have zero TotalHotkeyAlpha"
        );

        let root_voting_power = VotingPower::<Test>::get(netuid, old_hotkey);
        let child_voting_power = VotingPower::<Test>::get(netuid, new_hotkey);
        assert!(
            root_voting_power > 0,
            "old_hotkey should retain VotingPower"
        );
        assert_eq!(
            child_voting_power, 0,
            "new_hotkey should have zero VotingPower"
        );

        let old_hotkey_children = ChildKeys::<Test>::get(old_hotkey, netuid);
        assert!(
            !old_hotkey_children.iter().any(|(_, c)| *c == child_key),
            "old_hotkey should NOT retain ChildKeys after swap"
        );
        let new_hotkey_children = ChildKeys::<Test>::get(new_hotkey, netuid);
        assert!(
            new_hotkey_children.iter().any(|(_, c)| *c == child_key),
            "new_hotkey should inherit ChildKeys from old_hotkey"
        );

        let child_key_parents = ParentKeys::<Test>::get(child_key, netuid);
        assert!(
            child_key_parents.iter().any(|(_, p)| *p == new_hotkey),
            "child_key should have new_hotkey as parent after swap"
        );
        assert!(
            !child_key_parents.iter().any(|(_, p)| *p == old_hotkey),
            "child_key should NOT have old_hotkey as parent after swap"
        );
    });
}
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_revert_hotkey_swap --exact --nocapture
// This test confirms, that the old hotkey can be reverted after the hotkey swap
#[test]
fn test_revert_hotkey_swap() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let netuid2 = NetUid::from(2);
        let tempo: u16 = 13;
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let swap_cost = 1_000_000_000u64 * 2;

        // Setup initial state
        add_network(netuid, tempo, 0);
        add_network(netuid2, tempo, 0);
        register_ok_neuron(netuid, old_hotkey, coldkey, 0);
        register_ok_neuron(netuid2, old_hotkey, coldkey, 0);
        add_balance_to_coldkey_account(&coldkey, swap_cost.into());
        step_block(20);

        // Perform the first swap (only on netuid)
        assert_ok!(SubtensorModule::do_swap_hotkey(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ));

        assert!(SubtensorModule::is_hotkey_registered_on_any_network(
            &old_hotkey
        ));

        step_block(20);

        assert_ok!(SubtensorModule::do_swap_hotkey(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            &new_hotkey,
            &old_hotkey,
            Some(netuid),
            false
        ));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_revert_hotkey_swap_parent_hotkey_childkey_maps --exact --nocapture
#[test]
fn test_revert_hotkey_swap_parent_hotkey_childkey_maps() {
    new_test_ext(1).execute_with(|| {
        let hk1 = U256::from(1);
        let coldkey = U256::from(2);
        let child = U256::from(3);
        let child_other = U256::from(4);
        let hk2 = U256::from(5);

        let netuid = add_dynamic_network(&hk1, &coldkey);
        let netuid2 = add_dynamic_network(&hk1, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());
        SubtensorModule::create_account_if_non_existent(&coldkey, &hk1);

        mock_set_children(&coldkey, &hk1, netuid, &[(u64::MAX, child)]);
        step_rate_limit(&TransactionType::SetChildren, netuid);
        mock_schedule_children(&coldkey, &hk1, netuid, &[(u64::MAX, child_other)]);

        assert_eq!(
            ParentKeys::<Test>::get(child, netuid),
            vec![(u64::MAX, hk1)]
        );
        assert_eq!(ChildKeys::<Test>::get(hk1, netuid), vec![(u64::MAX, child)]);
        let existing_pending_child_keys = PendingChildKeys::<Test>::get(netuid, hk1);
        assert_eq!(existing_pending_child_keys.0, vec![(u64::MAX, child_other)]);

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &hk1,
            &hk2,
            Some(netuid),
            false
        ));

        assert_eq!(
            ParentKeys::<Test>::get(child, netuid),
            vec![(u64::MAX, hk2)]
        );
        assert_eq!(ChildKeys::<Test>::get(hk2, netuid), vec![(u64::MAX, child)]);
        assert_eq!(
            PendingChildKeys::<Test>::get(netuid, hk2),
            existing_pending_child_keys
        );
        assert!(ChildKeys::<Test>::get(hk1, netuid).is_empty());
        assert!(PendingChildKeys::<Test>::get(netuid, hk1).0.is_empty());

        // Revert: hk2 -> hk1
        step_block(20);
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &hk2,
            &hk1,
            Some(netuid),
            false
        ));

        assert_eq!(
            ParentKeys::<Test>::get(child, netuid),
            vec![(u64::MAX, hk1)],
            "ParentKeys must point back to hk1 after revert"
        );
        assert_eq!(
            ChildKeys::<Test>::get(hk1, netuid),
            vec![(u64::MAX, child)],
            "ChildKeys must be restored to hk1 after revert"
        );
        assert_eq!(
            PendingChildKeys::<Test>::get(netuid, hk1),
            existing_pending_child_keys,
            "PendingChildKeys must be restored to hk1 after revert"
        );

        assert!(
            ChildKeys::<Test>::get(hk2, netuid).is_empty(),
            "hk2 must have no ChildKeys after revert"
        );
        assert!(
            PendingChildKeys::<Test>::get(netuid, hk2).0.is_empty(),
            "hk2 must have no PendingChildKeys after revert"
        );
    })
}
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_revert_hotkey_swap_uids_and_keys --exact --nocapture
#[test]
fn test_revert_hotkey_swap_uids_and_keys() {
    new_test_ext(1).execute_with(|| {
        let uid = 5u16;
        let hk1 = U256::from(1);
        let hk2 = U256::from(2);
        let coldkey = U256::from(3);

        let netuid = add_dynamic_network(&hk1, &coldkey);
        let netuid2 = add_dynamic_network(&hk1, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        IsNetworkMember::<Test>::insert(hk1, netuid, true);
        Uids::<Test>::insert(netuid, hk1, uid);
        Keys::<Test>::insert(netuid, uid, hk1);

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &hk1,
            &hk2,
            Some(netuid),
            false
        ));

        assert_eq!(Uids::<Test>::get(netuid, hk1), None);
        assert_eq!(Uids::<Test>::get(netuid, hk2), Some(uid));
        assert_eq!(Keys::<Test>::get(netuid, uid), hk2);

        // Revert: hk2 -> hk1
        step_block(20);
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &hk2,
            &hk1,
            Some(netuid),
            false
        ));

        assert_eq!(
            Uids::<Test>::get(netuid, hk2),
            None,
            "hk2 must have no uid after revert"
        );
        assert_eq!(
            Uids::<Test>::get(netuid, hk1),
            Some(uid),
            "hk1 must have its uid restored after revert"
        );
        assert_eq!(
            Keys::<Test>::get(netuid, uid),
            hk1,
            "Keys must point back to hk1 after revert"
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_revert_hotkey_swap_auto_stake_destination --exact --nocapture
#[test]
fn test_revert_hotkey_swap_auto_stake_destination() {
    new_test_ext(1).execute_with(|| {
        let hk1 = U256::from(1);
        let hk2 = U256::from(2);
        let coldkey = U256::from(3);
        let netuid = NetUid::from(2u16);
        let netuid2 = NetUid::from(3u16);
        let staker1 = U256::from(4);
        let staker2 = U256::from(5);
        let coldkeys = vec![staker1, staker2, coldkey];

        add_network(netuid, 1, 0);
        add_network(netuid2, 1, 0);
        register_ok_neuron(netuid, hk1, coldkey, 0);
        register_ok_neuron(netuid2, hk1, coldkey, 0);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        AutoStakeDestinationColdkeys::<Test>::insert(hk1, netuid, coldkeys.clone());
        AutoStakeDestination::<Test>::insert(coldkey, netuid, hk1);
        AutoStakeDestination::<Test>::insert(staker1, netuid, hk1);
        AutoStakeDestination::<Test>::insert(staker2, netuid, hk1);

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &hk1,
            &hk2,
            Some(netuid),
            false
        ));

        assert_eq!(
            AutoStakeDestinationColdkeys::<Test>::get(hk2, netuid),
            coldkeys
        );
        assert!(AutoStakeDestinationColdkeys::<Test>::get(hk1, netuid).is_empty());
        assert_eq!(
            AutoStakeDestination::<Test>::get(coldkey, netuid),
            Some(hk2)
        );
        assert_eq!(
            AutoStakeDestination::<Test>::get(staker1, netuid),
            Some(hk2)
        );
        assert_eq!(
            AutoStakeDestination::<Test>::get(staker2, netuid),
            Some(hk2)
        );

        // Revert: hk2 -> hk1
        step_block(20);
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &hk2,
            &hk1,
            Some(netuid),
            false
        ));

        assert_eq!(
            AutoStakeDestinationColdkeys::<Test>::get(hk1, netuid),
            coldkeys,
            "AutoStakeDestinationColdkeys must be restored to hk1 after revert"
        );
        assert!(
            AutoStakeDestinationColdkeys::<Test>::get(hk2, netuid).is_empty(),
            "hk2 must have no AutoStakeDestinationColdkeys after revert"
        );
        assert_eq!(
            AutoStakeDestination::<Test>::get(coldkey, netuid),
            Some(hk1),
            "coldkey AutoStakeDestination must point back to hk1 after revert"
        );
        assert_eq!(
            AutoStakeDestination::<Test>::get(staker1, netuid),
            Some(hk1),
            "staker1 AutoStakeDestination must point back to hk1 after revert"
        );
        assert_eq!(
            AutoStakeDestination::<Test>::get(staker2, netuid),
            Some(hk1),
            "staker2 AutoStakeDestination must point back to hk1 after revert"
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_revert_hotkey_swap_subnet_owner --exact --nocapture
#[test]
fn test_revert_hotkey_swap_subnet_owner() {
    new_test_ext(1).execute_with(|| {
        let hk1 = U256::from(1);
        let hk2 = U256::from(2);
        let coldkey = U256::from(3);

        let netuid = add_dynamic_network(&hk1, &coldkey);
        let netuid2 = add_dynamic_network(&hk1, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        assert_eq!(SubnetOwnerHotkey::<Test>::get(netuid), hk1);

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &hk1,
            &hk2,
            Some(netuid),
            false
        ));

        assert_eq!(
            SubnetOwnerHotkey::<Test>::get(netuid),
            hk2,
            "hk2 must be subnet owner after swap"
        );

        // Revert: hk2 -> hk1
        step_block(20);
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &hk2,
            &hk1,
            Some(netuid),
            false
        ));

        assert_eq!(
            SubnetOwnerHotkey::<Test>::get(netuid),
            hk1,
            "hk1 must be restored as subnet owner after revert"
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_revert_hotkey_swap_dividends --exact --nocapture
#[test]
fn test_revert_hotkey_swap_dividends() {
    new_test_ext(1).execute_with(|| {
        let hk1 = U256::from(1);
        let hk2 = U256::from(2);
        let coldkey = U256::from(3);

        let netuid = add_dynamic_network(&hk1, &coldkey);
        remove_owner_registration_stake(netuid);
        let netuid2 = add_dynamic_network(&hk1, &coldkey);
        remove_owner_registration_stake(netuid2);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        let amount = 10_000;
        let shares = U64F64::from_num(10_000);

        TotalHotkeyAlpha::<Test>::insert(hk1, netuid, AlphaBalance::from(amount));
        TotalHotkeyAlphaLastEpoch::<Test>::insert(hk1, netuid, AlphaBalance::from(amount * 2));
        TotalHotkeyShares::<Test>::insert(hk1, netuid, U64F64::from_num(shares));
        Alpha::<Test>::insert((hk1, coldkey, netuid), U64F64::from_num(amount));
        AlphaDividendsPerSubnet::<Test>::insert(netuid, hk1, AlphaBalance::from(amount));

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &hk1,
            &hk2,
            Some(netuid),
            false
        ));

        assert_eq!(
            TotalHotkeyAlpha::<Test>::get(hk1, netuid),
            AlphaBalance::ZERO
        );
        assert_eq!(
            TotalHotkeyAlpha::<Test>::get(hk2, netuid),
            AlphaBalance::from(amount)
        );
        assert_eq!(
            TotalHotkeyAlphaLastEpoch::<Test>::get(hk1, netuid),
            AlphaBalance::ZERO
        );
        assert_eq!(
            TotalHotkeyAlphaLastEpoch::<Test>::get(hk2, netuid),
            AlphaBalance::from(amount * 2)
        );
        assert_eq!(
            TotalHotkeyShares::<Test>::get(hk1, netuid),
            U64F64::from_num(0)
        );
        assert_eq!(
            TotalHotkeyShares::<Test>::get(hk2, netuid),
            U64F64::from_num(0)
        );
        assert_eq!(TotalHotkeySharesV2::<Test>::get(hk2, netuid), shares.into());
        assert_eq!(
            Alpha::<Test>::get((hk1, coldkey, netuid)),
            U64F64::from_num(0)
        );
        assert_eq!(
            Alpha::<Test>::get((hk2, coldkey, netuid)),
            U64F64::from_num(0)
        );
        assert_eq!(AlphaV2::<Test>::get((hk2, coldkey, netuid)), amount.into());
        assert_eq!(
            AlphaDividendsPerSubnet::<Test>::get(netuid, hk1),
            AlphaBalance::ZERO
        );
        assert_eq!(
            AlphaDividendsPerSubnet::<Test>::get(netuid, hk2),
            AlphaBalance::from(amount)
        );

        // Revert: hk2 -> hk1
        step_block(20);
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &hk2,
            &hk1,
            Some(netuid),
            false
        ));

        assert_eq!(
            TotalHotkeyAlpha::<Test>::get(hk2, netuid),
            AlphaBalance::ZERO,
            "hk2 TotalHotkeyAlpha must be zero after revert"
        );
        assert_eq!(
            TotalHotkeyAlpha::<Test>::get(hk1, netuid),
            AlphaBalance::from(amount),
            "hk1 TotalHotkeyAlpha must be restored after revert"
        );
        assert_eq!(
            TotalHotkeyAlphaLastEpoch::<Test>::get(hk2, netuid),
            AlphaBalance::ZERO,
            "hk2 TotalHotkeyAlphaLastEpoch must be zero after revert"
        );
        assert_eq!(
            TotalHotkeyAlphaLastEpoch::<Test>::get(hk1, netuid),
            AlphaBalance::from(amount * 2),
            "hk1 TotalHotkeyAlphaLastEpoch must be restored after revert"
        );
        assert_eq!(
            TotalHotkeyShares::<Test>::get(hk2, netuid),
            U64F64::from_num(0),
            "hk2 TotalHotkeyShares must be zero after revert"
        );
        assert_eq!(
            TotalHotkeyShares::<Test>::get(hk1, netuid),
            U64F64::from_num(0),
            "hk1 TotalHotkeyShares must be migrated to v2"
        );
        assert_eq!(
            TotalHotkeySharesV2::<Test>::get(hk1, netuid),
            shares.into(),
            "hk1 TotalHotkeyShares must be restored to v2 after revert"
        );
        assert_eq!(
            Alpha::<Test>::get((hk2, coldkey, netuid)),
            U64F64::from_num(0),
            "hk2 Alpha must be zero after revert"
        );
        assert_eq!(
            Alpha::<Test>::get((hk1, coldkey, netuid)),
            U64F64::from_num(0),
            "hk1 Alpha must be migrated to v2"
        );
        assert_eq!(
            AlphaV2::<Test>::get((hk1, coldkey, netuid)),
            amount.into(),
            "hk1 Alpha must be restored to v2 after revert"
        );
        assert_eq!(
            AlphaDividendsPerSubnet::<Test>::get(netuid, hk2),
            AlphaBalance::ZERO,
            "hk2 AlphaDividendsPerSubnet must be zero after revert"
        );
        assert_eq!(
            AlphaDividendsPerSubnet::<Test>::get(netuid, hk1),
            AlphaBalance::from(amount),
            "hk1 AlphaDividendsPerSubnet must be restored after revert"
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_revert_voting_power_transfers_on_hotkey_swap --exact --nocapture
#[test]
fn test_revert_voting_power_transfers_on_hotkey_swap() {
    new_test_ext(1).execute_with(|| {
        let hk1 = U256::from(1);
        let hk2 = U256::from(99);
        let coldkey = U256::from(2);
        let netuid = add_dynamic_network(&hk1, &coldkey);
        let voting_power_value = 5_000_000_000_000_u64;

        VotingPower::<Test>::insert(netuid, hk1, voting_power_value);
        assert_eq!(
            SubtensorModule::get_voting_power(netuid, &hk1),
            voting_power_value
        );
        assert_eq!(SubtensorModule::get_voting_power(netuid, &hk2), 0);

        SubtensorModule::swap_voting_power_for_hotkey(&hk1, &hk2, netuid);

        assert_eq!(SubtensorModule::get_voting_power(netuid, &hk1), 0);
        assert_eq!(
            SubtensorModule::get_voting_power(netuid, &hk2),
            voting_power_value
        );

        // Revert: hk2 -> hk1
        SubtensorModule::swap_voting_power_for_hotkey(&hk2, &hk1, netuid);

        assert_eq!(
            SubtensorModule::get_voting_power(netuid, &hk1),
            voting_power_value,
            "hk1 voting power must be fully restored after revert"
        );
        assert_eq!(
            SubtensorModule::get_voting_power(netuid, &hk2),
            0,
            "hk2 must have no voting power after revert"
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_revert_claim_root_with_swap_hotkey --exact --nocapture
#[test]
fn test_revert_claim_root_with_swap_hotkey() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let hk1 = U256::from(1002);
        let hk2 = U256::from(1003);
        let coldkey = U256::from(1004);

        let netuid = add_dynamic_network(&hk1, &owner_coldkey);
        let netuid2 = add_dynamic_network(&hk1, &owner_coldkey);

        add_balance_to_coldkey_account(&owner_coldkey, 1_000_000_000_000_u64.into());
        SubtensorModule::set_tao_weight(u64::MAX);

        let root_stake = 2_000_000u64;
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hk1,
            &coldkey,
            NetUid::ROOT,
            root_stake.into(),
        );

        let initial_total_hotkey_alpha = 10_000_000u64;
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hk1,
            &owner_coldkey,
            netuid,
            initial_total_hotkey_alpha.into(),
        );

        let pending_root_alpha = 1_000_000u64;
        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            pending_root_alpha.into(),
            AlphaBalance::ZERO,
        );

        assert_ok!(SubtensorModule::set_root_claim_type(
            RuntimeOrigin::signed(coldkey),
            RootClaimTypeEnum::Keep
        ));
        assert_ok!(SubtensorModule::claim_root(
            RuntimeOrigin::signed(coldkey),
            BTreeSet::from([netuid])
        ));

        let stake_after_claim: u64 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hk1, &coldkey, netuid)
                .into();

        let hk1_root_claimed = RootClaimed::<Test>::get((netuid, &hk1, &coldkey));
        let hk1_claimable = *RootClaimable::<Test>::get(hk1).get(&netuid).unwrap();

        assert_eq!(u128::from(stake_after_claim), hk1_root_claimed);
        assert!(!RootClaimable::<Test>::get(hk2).contains_key(&netuid));

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(owner_coldkey),
            &hk1,
            &hk2,
            Some(netuid),
            false
        ));

        assert_eq!(
            RootClaimed::<Test>::get((netuid, &hk2, &coldkey)),
            0u128,
            "hk2 RootClaimed must be zero after swap"
        );
        assert_eq!(
            RootClaimed::<Test>::get((netuid, &hk1, &coldkey)),
            hk1_root_claimed,
            "hk2 must have hk1's RootClaimed after swap"
        );
        assert!(RootClaimable::<Test>::get(hk1).contains_key(&netuid));
        assert!(!RootClaimable::<Test>::get(hk2).contains_key(&netuid));

        // Revert: hk2 -> hk1
        step_block(20);
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(owner_coldkey),
            &hk2,
            &hk1,
            Some(netuid),
            false
        ));

        assert_eq!(
            RootClaimed::<Test>::get((netuid, &hk2, &coldkey)),
            0u128,
            "hk2 RootClaimed must be zero after revert"
        );
        assert_eq!(
            RootClaimed::<Test>::get((netuid, &hk1, &coldkey)),
            hk1_root_claimed,
            "hk1 RootClaimed must be restored after revert"
        );

        assert!(!RootClaimable::<Test>::get(hk2).contains_key(&netuid));
        assert_eq!(
            *RootClaimable::<Test>::get(hk1).get(&netuid).unwrap(),
            hk1_claimable,
            "hk1 RootClaimable must be restored after revert"
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_swap_hotkey_with_existing_stake --exact --show-output
#[test]
fn test_swap_hotkey_with_existing_stake() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(4);
        let staker1 = U256::from(5);
        let staker2 = U256::from(6);
        let subnet_owner_coldkey = U256::from(1000);
        let subnet_owner_hotkey = U256::from(1001);
        let staked_tao_1 = 100_000_000;
        let staked_tao_2 = 200_000_000;
        let staked_tao_3 = 300_000_000;
        let staked_tao_4 = 500_000_000;

        // Set up initial state
        let netuid = add_dynamic_network(&subnet_owner_coldkey, &subnet_owner_hotkey);
        register_ok_neuron(netuid, old_hotkey, coldkey, 1234);
        register_ok_neuron(netuid, new_hotkey, coldkey, 1234);

        // Add balance to coldkeys
        add_balance_to_coldkey_account(&coldkey, 10_000_000_000_u64.into());
        add_balance_to_coldkey_account(&staker1, 10_000_000_000_u64.into());
        add_balance_to_coldkey_account(&staker2, 10_000_000_000_u64.into());

        // Stake with staker1 coldkey on old_hotkey
        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(staker1),
            old_hotkey,
            netuid,
            staked_tao_1.into()
        ));

        // Stake with staker2 coldkey on old_hotkey
        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(staker2),
            old_hotkey,
            netuid,
            staked_tao_2.into()
        ));

        // Stake with staker1 coldkey on new_hotkey
        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(staker1),
            new_hotkey,
            netuid,
            staked_tao_3.into()
        ));

        // Stake with staker2 coldkey on new_hotkey
        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(staker2),
            new_hotkey,
            netuid,
            staked_tao_4.into()
        ));

        // Emulate effect of emission into alpha pool - makes numerators and denominators not equal to alpha
        let emission = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_on_subnet(&old_hotkey, netuid, emission);
        SubtensorModule::increase_stake_for_hotkey_on_subnet(&new_hotkey, netuid, emission);

        // Hotkey new_hotkey gets deregistered, stake stays
        IsNetworkMember::<Test>::remove(new_hotkey, netuid);

        let hk1_stake_1 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &old_hotkey,
            &staker1,
            netuid,
        );
        let hk2_stake_1 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &new_hotkey,
            &staker1,
            netuid,
        );
        let hk1_stake_2 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &old_hotkey,
            &staker2,
            netuid,
        );
        let hk2_stake_2 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &new_hotkey,
            &staker2,
            netuid,
        );

        assert!(!hk1_stake_1.is_zero());
        assert!(!hk2_stake_1.is_zero());
        assert!(!hk1_stake_2.is_zero());
        assert!(!hk2_stake_2.is_zero());

        let total_hk1_stake = SubtensorModule::get_total_stake_for_hotkey(&old_hotkey);
        let total_hk2_stake = SubtensorModule::get_total_stake_for_hotkey(&new_hotkey);
        assert!(!total_hk1_stake.is_zero());
        assert!(!total_hk2_stake.is_zero());
        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());

        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ));

        // Check correctness of stake transfer
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &old_hotkey,
                &staker1,
                netuid
            ),
            0.into()
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &old_hotkey,
                &staker2,
                netuid
            ),
            0.into()
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &new_hotkey,
                &staker1,
                netuid
            ),
            hk2_stake_1 + hk1_stake_1
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &new_hotkey,
                &staker2,
                netuid
            ),
            hk2_stake_2 + hk1_stake_2
        );

        // Check total stake transfer
        assert_abs_diff_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&old_hotkey),
            0.into(),
            epsilon = 1.into()
        );
        assert_abs_diff_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&new_hotkey),
            total_hk1_stake + total_hk2_stake,
            epsilon = 1.into()
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_revert_hotkey_swap_with_revert_stake_the_same --exact --nocapture
#[test]
fn test_revert_hotkey_swap_with_revert_stake_the_same() {
    new_test_ext(1).execute_with(|| {
        let netuid_1 = NetUid::from(1);
        let netuid_2 = NetUid::from(2);
        let tempo: u16 = 13;
        let hk1 = U256::from(1);
        let new_hotkey = U256::from(2);
        let random_hotkey = U256::from(3);
        let coldkey = U256::from(3);
        let coldkey_2 = U256::from(4);
        let coldkey_3 = U256::from(5);
        let coldkey_4 = U256::from(6);
        let random_coldkey = U256::from(7);
        let initial_balance = 10_000_000_000u64 * 2;
        let stake1 = 500_000_000u64;
        let stake2 = 1_000_000_000u64;
        let stake_ck2 = 1_500_000_000u64;
        let stake_ck3 = 300_000_000u64;
        let stake_ck4 = 900_000_000u64;

        assert_ok!(SubtensorModule::try_associate_hotkey(
            <<Test as Config>::RuntimeOrigin>::signed(random_coldkey),
            random_hotkey
        ));

        // Setup
        super::mock::setup_reserves(netuid_1, (stake_ck4 * 100).into(), (stake_ck4 * 100).into());
        super::mock::setup_reserves(netuid_2, (stake_ck4 * 100).into(), (stake_ck4 * 100).into());

        add_network(netuid_1, tempo, 0);
        add_network(netuid_2, tempo, 0);

        SubnetMechanism::<Test>::insert(netuid_1, 1);
        SubnetMechanism::<Test>::insert(netuid_2, 1);

        register_ok_neuron(netuid_1, hk1, coldkey, 0);
        register_ok_neuron(netuid_2, hk1, coldkey, 0);

        add_balance_to_coldkey_account(&coldkey, initial_balance.into());
        add_balance_to_coldkey_account(&coldkey_4, initial_balance.into());
        add_balance_to_coldkey_account(&random_coldkey, initial_balance.into());
        step_block(20); // Waiting interval to be able to swap later

        // Checking stake for hk1 on both networks
        let hk1_stake_before_increase_sn_1 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hk1, &coldkey, netuid_1);
        assert!(
            hk1_stake_before_increase_sn_1 == 0.into(),
            "hk1 should have empty stake"
        );

        let hk1_stake_before_increase_sn_2 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hk1, &coldkey, netuid_2);
        assert!(
            hk1_stake_before_increase_sn_2 == 0.into(),
            "hk1 should have empty stake"
        );

        // Adding stake to hk1 on both networks
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hk1,
            &coldkey,
            netuid_1,
            stake1.into(),
        );
        // Adding another stake for different coldkey
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hk1,
            &coldkey_2,
            netuid_1,
            stake_ck2.into(),
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hk1,
            &coldkey_3,
            netuid_1,
            stake_ck3.into(),
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hk1,
            &coldkey,
            netuid_2,
            stake2.into(),
        );

        // The stake for validator
        let hk1_stake_before_swap_sn_1 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hk1, &coldkey, netuid_1);
        assert!(
            hk1_stake_before_swap_sn_1 == stake1.into(),
            "hk1 should have stake before swap on sn_1"
        );

        // Let's check individual stake
        let hk1_stake_before_swap_sn_1 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hk1, &coldkey_2, netuid_1);
        assert_eq!(
            hk1_stake_before_swap_sn_1,
            (stake_ck2).into(),
            "stake for ck2 should be only his stake"
        );

        let hk1_stake_before_swap_sn_2 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hk1, &coldkey, netuid_2);
        assert!(
            hk1_stake_before_swap_sn_2 == stake2.into(),
            "hk1 should have stake before swap on sn_2"
        );

        assert_ok!(SubtensorModule::do_swap_hotkey(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            &hk1,
            &new_hotkey,
            Some(netuid_1),
            false
        ));

        assert_eq!(Owner::<Test>::get(hk1), coldkey);

        SubtensorModule::do_add_stake(
            RawOrigin::Signed(random_coldkey).into(),
            hk1,
            netuid_1,
            stake_ck4.into(),
        )
        .unwrap();

        // Check stake moved to new hotkey on subnet1
        let new_hotkey_stake_after_swap_ck =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &new_hotkey,
                &coldkey,
                netuid_1,
            );
        assert_eq!(new_hotkey_stake_after_swap_ck, stake1.into());

        // Check stake moved for ck2
        let new_hotkey_stake_after_swap_ck_1 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &new_hotkey,
                &coldkey_2,
                netuid_1,
            );
        assert_eq!(new_hotkey_stake_after_swap_ck_1, stake_ck2.into());

        // Check stake moved for ck3
        let new_hotkey_stake_after_swap_ck_3 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &new_hotkey,
                &coldkey_3,
                netuid_1,
            );
        assert_eq!(new_hotkey_stake_after_swap_ck_3, stake_ck3.into());

        step_block(20);

        // Let's check individual stakes; they changed because of emissions
        let new_hotkey_stake_before_revert_ck =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &new_hotkey,
                &coldkey,
                netuid_1,
            );
        assert!(new_hotkey_stake_before_revert_ck > stake1.into());

        let new_hotkey_stake_before_revert_ck_2 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &new_hotkey,
                &coldkey_2,
                netuid_1,
            );
        assert!(new_hotkey_stake_before_revert_ck_2 > stake_ck2.into());

        let new_hotkey_stake_before_revert_ck_3 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &new_hotkey,
                &coldkey_3,
                netuid_1,
            );
        assert!(new_hotkey_stake_before_revert_ck_3 > stake_ck3.into());

        // Reverting back: hk2 -> hk1
        assert_ok!(SubtensorModule::do_swap_hotkey(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            &new_hotkey,
            &hk1,
            Some(netuid_1),
            false
        ));

        // Let's check individual stakes; they changed because of emissions
        let old_hotkey_stake_after_revert_ck =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hk1, &coldkey, netuid_1);
        assert_eq!(
            old_hotkey_stake_after_revert_ck,
            new_hotkey_stake_before_revert_ck
        );

        let old_hotkey_stake_after_revert_ck_2 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hk1, &coldkey_2, netuid_1);
        assert_eq!(
            old_hotkey_stake_after_revert_ck_2,
            new_hotkey_stake_before_revert_ck_2
        );

        let old_hotkey_stake_after_revert_ck_3 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hk1, &coldkey_3, netuid_1);
        assert_eq!(
            old_hotkey_stake_after_revert_ck_3,
            new_hotkey_stake_before_revert_ck_3
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_swap_hotkey_root_claims_unchanged_if_not_root --exact --nocapture
#[test]
fn test_swap_hotkey_root_claims_unchanged_if_not_root() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let neuron_hotkey = U256::from(1002);
        let staker_coldkey = U256::from(1003);
        let netuid = add_dynamic_network(&neuron_hotkey, &owner_coldkey);
        let new_hotkey = U256::from(10030);

        add_balance_to_coldkey_account(&owner_coldkey, 20_000_000_000_000_000_u64.into());
        SubtensorModule::set_tao_weight(u64::MAX); // Set TAO weight to 1.0

        let root_stake = 2_000_000_000u64;
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &neuron_hotkey,
            &staker_coldkey,
            NetUid::ROOT,
            root_stake.into(),
        );

        let initial_total_hotkey_alpha = 10_000_000_000u64;
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &neuron_hotkey,
            &staker_coldkey,
            netuid,
            initial_total_hotkey_alpha.into(),
        );

        let validator_stake = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &neuron_hotkey,
            &staker_coldkey,
            netuid,
        );
        assert_eq!(validator_stake, initial_total_hotkey_alpha.into());

        // Distribute pending root alpha
        let pending_root_alpha = 1_000_000_000u64;
        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            pending_root_alpha.into(),
            AlphaBalance::ZERO,
        );

        assert_ok!(SubtensorModule::claim_root(
            RuntimeOrigin::signed(staker_coldkey),
            BTreeSet::from([netuid])
        ));

        let claimable = RootClaimable::<Test>::get(neuron_hotkey)
            .get(&netuid)
            .copied();

        assert!(claimable.is_some());
        let claimable = claimable.unwrap_or_default();

        assert!(claimable > 0);

        assert!(RootClaimed::<Test>::get((netuid, &neuron_hotkey, &staker_coldkey,)) > 0u128);

        step_block(20);
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(owner_coldkey),
            &neuron_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ));

        // Claimable and claimed should stay on old hotkey
        assert_eq!(
            RootClaimable::<Test>::get(neuron_hotkey)
                .get(&netuid)
                .copied(),
            Some(claimable)
        );
        assert!(RootClaimed::<Test>::get((netuid, &neuron_hotkey, &staker_coldkey,)) > 0u128);
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_swap_hotkey_root_claims_changed_if_root --exact --nocapture
#[test]
fn test_swap_hotkey_root_claims_changed_if_root() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);

        let neuron_hotkey = U256::from(1004);
        let neuron_hotkey_new = U256::from(1005);

        let staker_coldkey = U256::from(1006);

        // Use neuron_hotkey as subnet creator so it receives root dividends
        let netuid_1 = add_dynamic_network(&neuron_hotkey, &owner_coldkey);

        add_balance_to_coldkey_account(&owner_coldkey, 20_000_000_000_000_000_u64.into());
        SubtensorModule::set_tao_weight(u64::MAX); // Set TAO weight to 1.0

        let root_stake = 2_000_000_000u64;
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &neuron_hotkey,
            &staker_coldkey,
            NetUid::ROOT,
            root_stake.into(),
        );

        let initial_total_hotkey_alpha = 10_000_000_000u64;
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &neuron_hotkey,
            &owner_coldkey,
            netuid_1,
            initial_total_hotkey_alpha.into(),
        );

        // Distribute pending root alpha
        let pending_root_alpha = 1_000_000_000u64;
        SubtensorModule::distribute_emission(
            netuid_1,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            pending_root_alpha.into(),
            AlphaBalance::ZERO,
        );

        assert_ok!(SubtensorModule::set_root_claim_type(
            RuntimeOrigin::signed(staker_coldkey),
            RootClaimTypeEnum::Keep
        ));
        assert_ok!(SubtensorModule::claim_root(
            RuntimeOrigin::signed(staker_coldkey),
            BTreeSet::from([netuid_1])
        ));

        let claimable = RootClaimable::<Test>::get(neuron_hotkey)
            .get(&netuid_1)
            .copied();
        assert!(claimable.is_some());
        let claimable = claimable.unwrap_or_default();

        assert!(claimable > 0);

        let claimed = RootClaimed::<Test>::get((netuid_1, &neuron_hotkey, &staker_coldkey));
        assert!(claimed > 0u128);

        step_block(20);
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(owner_coldkey),
            &neuron_hotkey,
            &neuron_hotkey_new,
            Some(NetUid::ROOT),
            false
        ));

        // Claimable and claimed should be transferred to new hotkey
        assert_eq!(
            RootClaimable::<Test>::get(neuron_hotkey_new)
                .get(&netuid_1)
                .copied(),
            Some(claimable)
        );
        assert_eq!(
            RootClaimed::<Test>::get((netuid_1, &neuron_hotkey_new, &staker_coldkey,)),
            claimed
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_swap_hotkey_root_claims_changed_if_all_subnets --exact --nocapture
#[test]
fn test_swap_hotkey_root_claims_changed_if_all_subnets() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let neuron_hotkey = U256::from(1004);
        let neuron_hotkey_new = U256::from(1005);

        let staker_coldkey = U256::from(1006);

        // Ensure ROOT network is registered for all-subnets swap
        SubtokenEnabled::<Test>::insert(NetUid::ROOT, true);
        NetworksAdded::<Test>::insert(NetUid::ROOT, true);

        // Use neuron_hotkey as subnet creator so it receives root dividends
        let netuid_1 = add_dynamic_network(&neuron_hotkey, &owner_coldkey);

        add_balance_to_coldkey_account(&owner_coldkey, 20_000_000_000_000_000_u64.into());
        SubtensorModule::set_tao_weight(u64::MAX); // Set TAO weight to 1.0

        let root_stake = 2_000_000_000u64;
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &neuron_hotkey,
            &staker_coldkey,
            NetUid::ROOT,
            root_stake.into(),
        );

        let initial_total_hotkey_alpha = 10_000_000_000u64;
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &neuron_hotkey,
            &owner_coldkey,
            netuid_1,
            initial_total_hotkey_alpha.into(),
        );

        // Distribute pending root alpha
        let pending_root_alpha = 1_000_000_000u64;
        SubtensorModule::distribute_emission(
            netuid_1,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            pending_root_alpha.into(),
            AlphaBalance::ZERO,
        );

        assert_ok!(SubtensorModule::set_root_claim_type(
            RuntimeOrigin::signed(staker_coldkey),
            RootClaimTypeEnum::Keep
        ));
        assert_ok!(SubtensorModule::claim_root(
            RuntimeOrigin::signed(staker_coldkey),
            BTreeSet::from([netuid_1])
        ));

        let claimable = RootClaimable::<Test>::get(neuron_hotkey)
            .get(&netuid_1)
            .copied();
        assert!(claimable.is_some());
        let claimable = claimable.unwrap_or_default();

        assert!(claimable > 0);

        let claimed = RootClaimed::<Test>::get((netuid_1, &neuron_hotkey, &staker_coldkey));
        assert!(claimed > 0u128);

        step_block(20);
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(owner_coldkey),
            &neuron_hotkey,
            &neuron_hotkey_new,
            None,
            false
        ));

        // Claimable and claimed should be transferred to new hotkey
        assert_eq!(
            RootClaimable::<Test>::get(neuron_hotkey_new)
                .get(&netuid_1)
                .copied(),
            Some(claimable)
        );
        assert_eq!(
            RootClaimed::<Test>::get((netuid_1, &neuron_hotkey_new, &staker_coldkey,)),
            claimed
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_swap_hotkey_auto_parent_delegation_transferred_on_root --exact --nocapture
#[test]
fn test_swap_hotkey_auto_parent_delegation_transferred_on_root() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let old_hotkey = U256::from(1004);
        let new_hotkey = U256::from(1005);

        let _ = add_dynamic_network(&old_hotkey, &owner_coldkey);
        add_balance_to_coldkey_account(&owner_coldkey, 20_000_000_000_000_000_u64.into());

        // Opt out of auto parent delegation on the old hotkey.
        AutoParentDelegationEnabled::<Test>::insert(old_hotkey, false);
        assert!(AutoParentDelegationEnabled::<Test>::contains_key(
            old_hotkey
        ));
        assert!(!AutoParentDelegationEnabled::<Test>::get(old_hotkey));

        step_block(20);
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(owner_coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(NetUid::ROOT),
            false
        ));

        // Flag is moved to the new hotkey, cleared from the old one.
        assert!(!AutoParentDelegationEnabled::<Test>::contains_key(
            old_hotkey
        ));
        assert!(AutoParentDelegationEnabled::<Test>::contains_key(
            new_hotkey
        ));
        assert!(!AutoParentDelegationEnabled::<Test>::get(new_hotkey));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_swap_hotkey_auto_parent_delegation_transferred_on_all_subnets --exact --nocapture
#[test]
fn test_swap_hotkey_auto_parent_delegation_transferred_on_all_subnets() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let old_hotkey = U256::from(1004);
        let new_hotkey = U256::from(1005);

        SubtokenEnabled::<Test>::insert(NetUid::ROOT, true);
        NetworksAdded::<Test>::insert(NetUid::ROOT, true);

        let _ = add_dynamic_network(&old_hotkey, &owner_coldkey);
        add_balance_to_coldkey_account(&owner_coldkey, 20_000_000_000_000_000_u64.into());

        AutoParentDelegationEnabled::<Test>::insert(old_hotkey, false);

        step_block(20);
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(owner_coldkey),
            &old_hotkey,
            &new_hotkey,
            None,
            false
        ));

        assert!(!AutoParentDelegationEnabled::<Test>::contains_key(
            old_hotkey
        ));
        assert!(AutoParentDelegationEnabled::<Test>::contains_key(
            new_hotkey
        ));
        assert!(!AutoParentDelegationEnabled::<Test>::get(new_hotkey));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_swap_hotkey_auto_parent_delegation_not_transferred_on_non_root --exact --nocapture
#[test]
fn test_swap_hotkey_auto_parent_delegation_not_transferred_on_non_root() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let old_hotkey = U256::from(1004);
        let new_hotkey = U256::from(1005);

        let netuid = add_dynamic_network(&old_hotkey, &owner_coldkey);
        add_balance_to_coldkey_account(&owner_coldkey, 20_000_000_000_000_000_u64.into());

        AutoParentDelegationEnabled::<Test>::insert(old_hotkey, false);

        System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(owner_coldkey),
            &old_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ));

        // Non-root subnet swap must not move the flag.
        assert!(AutoParentDelegationEnabled::<Test>::contains_key(
            old_hotkey
        ));
        assert!(!AutoParentDelegationEnabled::<Test>::get(old_hotkey));
        assert!(!AutoParentDelegationEnabled::<Test>::contains_key(
            new_hotkey
        ));
    });
}
