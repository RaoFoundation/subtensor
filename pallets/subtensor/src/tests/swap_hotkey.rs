#![allow(unused, clippy::indexing_slicing, clippy::panic, clippy::unwrap_used)]

use approx::assert_abs_diff_eq;
use codec::Encode;
use frame_support::weights::Weight;
use frame_support::{assert_err, assert_noop, assert_ok};
use frame_system::{Config, RawOrigin};
use share_pool::SafeFloat;
use sp_core::{Get, H160, H256, U256};
use sp_runtime::SaturatedConversion;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{AlphaBalance, NetUidStorageIndex, TaoBalance};
use subtensor_swap_interface::{SwapEngine, SwapHandler};

use super::mock;
use super::mock::*;
use crate::*;

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey -- test_swap_owner --exact --nocapture
#[test]
fn test_swap_owner() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let mut weight = Weight::zero();

        Owner::<Test>::insert(old_hotkey, coldkey);
        assert_ok!(SubtensorModule::perform_hotkey_swap_on_all_subnets(
            &old_hotkey,
            &new_hotkey,
            &coldkey,
            &mut weight,
            false
        ));

        assert!(!Owner::<Test>::contains_key(old_hotkey));
        assert_eq!(Owner::<Test>::get(new_hotkey), coldkey);
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey -- test_swap_owned_hotkeys --exact --nocapture
#[test]
fn test_swap_owned_hotkeys() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let mut weight = Weight::zero();

        OwnedHotkeys::<Test>::insert(coldkey, vec![old_hotkey]);
        assert_ok!(SubtensorModule::perform_hotkey_swap_on_all_subnets(
            &old_hotkey,
            &new_hotkey,
            &coldkey,
            &mut weight,
            false
        ));

        let hotkeys = OwnedHotkeys::<Test>::get(coldkey);
        assert!(!hotkeys.contains(&old_hotkey));
        assert!(hotkeys.contains(&new_hotkey));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey -- test_swap_total_hotkey_stake --exact --nocapture
#[test]
fn test_swap_total_hotkey_stake() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let amount = DefaultMinStake::<Test>::get() * 10.into();
        let mut weight = Weight::zero();

        //add network
        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        remove_owner_registration_stake(netuid);

        let reserve = u64::from(amount) * 100;
        mock::setup_reserves(netuid, reserve.into(), reserve.into());

        // Give it some $$$ in his coldkey balance
        add_balance_to_coldkey_account(&coldkey, amount);

        // Add stake
        let (expected_alpha, _) = mock::swap_tao_to_alpha(netuid, amount);
        assert!(!expected_alpha.is_zero());
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey),
            old_hotkey,
            netuid,
            amount
        ));

        // Check if stake has increased
        assert_eq!(
            TotalHotkeyAlpha::<Test>::get(old_hotkey, netuid),
            expected_alpha
        );
        assert_abs_diff_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&new_hotkey),
            TaoBalance::ZERO,
            epsilon = 1.into(),
        );

        // Swap hotkey
        assert_ok!(SubtensorModule::perform_hotkey_swap_on_all_subnets(
            &old_hotkey,
            &new_hotkey,
            &coldkey,
            &mut weight,
            false
        ));

        // Verify that total hotkey stake swapped
        assert_abs_diff_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&old_hotkey),
            TaoBalance::ZERO,
            epsilon = 1.into(),
        );
        assert_eq!(
            TotalHotkeyAlpha::<Test>::get(new_hotkey, netuid),
            expected_alpha
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey -- test_swap_delegates --exact --nocapture
#[test]
fn test_swap_delegates() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let mut weight = Weight::zero();

        Delegates::<Test>::insert(old_hotkey, 100);
        assert_ok!(SubtensorModule::perform_hotkey_swap_on_all_subnets(
            &old_hotkey,
            &new_hotkey,
            &coldkey,
            &mut weight,
            false
        ));

        assert!(!Delegates::<Test>::contains_key(old_hotkey));
        assert_eq!(Delegates::<Test>::get(new_hotkey), 100);
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey -- test_swap_subnet_membership --exact --nocapture
#[test]
fn test_swap_subnet_membership() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let netuid = NetUid::from(0u16);
        let mut weight = Weight::zero();

        add_network(netuid, 1, 1);
        IsNetworkMember::<Test>::insert(old_hotkey, netuid, true);
        assert_ok!(SubtensorModule::perform_hotkey_swap_on_all_subnets(
            &old_hotkey,
            &new_hotkey,
            &coldkey,
            &mut weight,
            false
        ));

        assert!(!IsNetworkMember::<Test>::contains_key(old_hotkey, netuid));
        assert!(IsNetworkMember::<Test>::get(new_hotkey, netuid));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey -- test_swap_uids_and_keys --exact --nocapture
#[test]
fn test_swap_uids_and_keys() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let netuid = NetUid::from(0u16);
        let uid = 5u16;
        let mut weight = Weight::zero();

        add_network(netuid, 1, 1);
        IsNetworkMember::<Test>::insert(old_hotkey, netuid, true);
        Uids::<Test>::insert(netuid, old_hotkey, uid);
        Keys::<Test>::insert(netuid, uid, old_hotkey);

        assert_ok!(SubtensorModule::perform_hotkey_swap_on_all_subnets(
            &old_hotkey,
            &new_hotkey,
            &coldkey,
            &mut weight,
            false
        ));

        assert_eq!(Uids::<Test>::get(netuid, old_hotkey), None);
        assert_eq!(Uids::<Test>::get(netuid, new_hotkey), Some(uid));
        assert_eq!(Keys::<Test>::get(netuid, uid), new_hotkey);
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey -- test_swap_prometheus --exact --nocapture
#[test]
fn test_swap_prometheus() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let netuid = NetUid::from(0u16);
        let prometheus_info = PrometheusInfo::default();
        let mut weight = Weight::zero();

        add_network(netuid, 1, 1);
        IsNetworkMember::<Test>::insert(old_hotkey, netuid, true);
        Prometheus::<Test>::insert(netuid, old_hotkey, prometheus_info.clone());

        assert_ok!(SubtensorModule::perform_hotkey_swap_on_all_subnets(
            &old_hotkey,
            &new_hotkey,
            &coldkey,
            &mut weight,
            false
        ));

        assert!(!Prometheus::<Test>::contains_key(netuid, old_hotkey));
        assert_eq!(
            Prometheus::<Test>::get(netuid, new_hotkey),
            Some(prometheus_info)
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey -- test_swap_axons --exact --nocapture
#[test]
fn test_swap_axons() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let netuid = NetUid::from(0u16);
        let axon_info = AxonInfo::default();
        let mut weight = Weight::zero();

        add_network(netuid, 1, 1);
        IsNetworkMember::<Test>::insert(old_hotkey, netuid, true);
        Axons::<Test>::insert(netuid, old_hotkey, axon_info.clone());

        assert_ok!(SubtensorModule::perform_hotkey_swap_on_all_subnets(
            &old_hotkey,
            &new_hotkey,
            &coldkey,
            &mut weight,
            false
        ));

        assert!(!Axons::<Test>::contains_key(netuid, old_hotkey));
        assert_eq!(Axons::<Test>::get(netuid, new_hotkey), Some(axon_info));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey -- test_swap_certificates --exact --nocapture
#[test]
fn test_swap_certificates() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let netuid = NetUid::from(0u16);
        let certificate = NeuronCertificate::try_from(vec![1, 2, 3]).unwrap();
        let mut weight = Weight::zero();

        add_network(netuid, 1, 1);
        IsNetworkMember::<Test>::insert(old_hotkey, netuid, true);
        NeuronCertificates::<Test>::insert(netuid, old_hotkey, certificate.clone());

        assert_ok!(SubtensorModule::perform_hotkey_swap_on_all_subnets(
            &old_hotkey,
            &new_hotkey,
            &coldkey,
            &mut weight,
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey -- test_swap_weight_commits --exact --nocapture
#[test]
fn test_swap_weight_commits() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let netuid = NetUid::from(0u16);
        let mut weight_commits: VecDeque<(H256, u64, u64, u64)> = VecDeque::new();
        weight_commits.push_back((H256::from_low_u64_be(100), 200, 1, 1));
        let mut weight = Weight::zero();

        add_network(netuid, 1, 1);
        IsNetworkMember::<Test>::insert(old_hotkey, netuid, true);
        WeightCommits::<Test>::insert(
            NetUidStorageIndex::from(netuid),
            old_hotkey,
            weight_commits.clone(),
        );

        assert_ok!(SubtensorModule::perform_hotkey_swap_on_all_subnets(
            &old_hotkey,
            &new_hotkey,
            &coldkey,
            &mut weight,
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

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey -- test_swap_loaded_emission --exact --nocapture
#[test]
fn test_swap_loaded_emission() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let netuid = NetUid::from(0u16);
        let server_emission = 1000u64;
        let validator_emission = 1000u64;
        let mut weight = Weight::zero();

        add_network(netuid, 1, 1);
        IsNetworkMember::<Test>::insert(old_hotkey, netuid, true);
        LoadedEmission::<Test>::insert(
            netuid,
            vec![(old_hotkey, server_emission, validator_emission)],
        );

        assert_ok!(SubtensorModule::perform_hotkey_swap_on_all_subnets(
            &old_hotkey,
            &new_hotkey,
            &coldkey,
            &mut weight,
            false
        ));

        let new_loaded_emission = LoadedEmission::<Test>::get(netuid);
        assert_eq!(
            new_loaded_emission,
            Some(vec![(new_hotkey, server_emission, validator_emission)])
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey -- test_swap_staking_hotkeys --exact --nocapture
#[test]
fn test_swap_staking_hotkeys() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);

        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let mut weight = Weight::zero();
        let netuid = NetUid::from(1);

        StakingHotkeys::<Test>::insert(coldkey, vec![old_hotkey]);
        Alpha::<Test>::insert((old_hotkey, coldkey, netuid), U64F64::from_num(100));

        assert_ok!(SubtensorModule::perform_hotkey_swap_on_all_subnets(
            &old_hotkey,
            &new_hotkey,
            &coldkey,
            &mut weight,
            false
        ));

        let staking_hotkeys = StakingHotkeys::<Test>::get(coldkey);
        assert!(!staking_hotkeys.contains(&old_hotkey));
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
        let mut weight = Weight::zero();
        let stake = TaoBalance::from(1_000_000_000_u64);

        StakingHotkeys::<Test>::insert(coldkey1, vec![old_hotkey]);
        StakingHotkeys::<Test>::insert(coldkey2, vec![old_hotkey]);
        SubtensorModule::create_account_if_non_existent(&coldkey1, &old_hotkey);
        add_balance_to_coldkey_account(&coldkey1, stake + ExistentialDeposit::get());
        add_balance_to_coldkey_account(&coldkey2, stake + ExistentialDeposit::get());

        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey1),
            old_hotkey,
            netuid,
            stake
        ));
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey2),
            old_hotkey,
            netuid,
            stake / 2.into()
        ));
        let stake1_before = SubtensorModule::get_total_stake_for_coldkey(&coldkey1);
        let stake2_before = SubtensorModule::get_total_stake_for_coldkey(&coldkey2);

        assert_ok!(SubtensorModule::perform_hotkey_swap_on_all_subnets(
            &old_hotkey,
            &new_hotkey,
            &coldkey1,
            &mut weight,
            false
        ));

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

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey -- test_swap_hotkey_with_multiple_subnets --exact --nocapture
#[test]
fn test_swap_hotkey_with_multiple_subnets() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let netuid1 = NetUid::from(0);
        let netuid2 = NetUid::from(1);
        let mut weight = Weight::zero();

        add_network(netuid1, 1, 1);
        add_network(netuid2, 1, 1);
        IsNetworkMember::<Test>::insert(old_hotkey, netuid1, true);
        IsNetworkMember::<Test>::insert(old_hotkey, netuid2, true);

        assert_ok!(SubtensorModule::perform_hotkey_swap_on_all_subnets(
            &old_hotkey,
            &new_hotkey,
            &coldkey,
            &mut weight,
            false
        ));

        assert!(IsNetworkMember::<Test>::get(new_hotkey, netuid1));
        assert!(IsNetworkMember::<Test>::get(new_hotkey, netuid2));
        assert!(!IsNetworkMember::<Test>::get(old_hotkey, netuid1));
        assert!(!IsNetworkMember::<Test>::get(old_hotkey, netuid2));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey -- test_swap_staking_hotkeys_multiple_coldkeys --exact --nocapture
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
        let mut weight = Weight::zero();
        let stake = TaoBalance::from(1_000_000_000);

        // Set up initial state
        StakingHotkeys::<Test>::insert(coldkey1, vec![old_hotkey]);
        StakingHotkeys::<Test>::insert(coldkey2, vec![old_hotkey, staker5]);
        Alpha::<Test>::insert((old_hotkey, coldkey1, netuid), U64F64::from_num(100));
        Alpha::<Test>::insert((old_hotkey, coldkey2, netuid), U64F64::from_num(100));

        SubtensorModule::create_account_if_non_existent(&coldkey1, &old_hotkey);
        add_balance_to_coldkey_account(&coldkey1, stake + ExistentialDeposit::get());
        add_balance_to_coldkey_account(&coldkey2, stake + ExistentialDeposit::get());
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey1),
            old_hotkey,
            netuid,
            stake
        ));
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey2),
            old_hotkey,
            netuid,
            stake
        ));

        assert_ok!(SubtensorModule::perform_hotkey_swap_on_all_subnets(
            &old_hotkey,
            &new_hotkey,
            &coldkey1,
            &mut weight,
            false
        ));

        // Check if new_hotkey replaced old_hotkey in StakingHotkeys
        assert!(StakingHotkeys::<Test>::get(coldkey1).contains(&new_hotkey));
        assert!(!StakingHotkeys::<Test>::get(coldkey1).contains(&old_hotkey));

        // Check if new_hotkey replaced old_hotkey for coldkey2 as well
        assert!(StakingHotkeys::<Test>::get(coldkey2).contains(&new_hotkey));
        assert!(!StakingHotkeys::<Test>::get(coldkey2).contains(&old_hotkey));
        assert!(StakingHotkeys::<Test>::get(coldkey2).contains(&staker5));
        // Other hotkeys should remain
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey -- test_swap_hotkey_with_no_stake --exact --nocapture
#[test]
fn test_swap_hotkey_with_no_stake() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let mut weight = Weight::zero();

        // Set up initial state with no stake
        Owner::<Test>::insert(old_hotkey, coldkey);

        assert_ok!(SubtensorModule::perform_hotkey_swap_on_all_subnets(
            &old_hotkey,
            &new_hotkey,
            &coldkey,
            &mut weight,
            false
        ));

        // Check if ownership transferred
        assert!(!Owner::<Test>::contains_key(old_hotkey));
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
        let coldkey1 = U256::from(3);
        let coldkey2 = U256::from(4);
        let netuid1 = NetUid::from(1);
        let netuid2 = NetUid::from(2);
        let stake = DefaultMinStake::<Test>::get() * 10.into();
        let mut weight = Weight::zero();

        // Set up initial state
        add_network(netuid1, 1, 1);
        add_network(netuid2, 1, 1);
        register_ok_neuron(netuid1, old_hotkey, coldkey1, 1234);
        register_ok_neuron(netuid2, old_hotkey, coldkey1, 1234);

        let reserve = u64::from(stake) * 100;
        mock::setup_reserves(netuid1, reserve.into(), reserve.into());
        mock::setup_reserves(netuid2, reserve.into(), reserve.into());

        // Add balance to both coldkeys
        add_balance_to_coldkey_account(&coldkey1, stake + 1_000.into());
        add_balance_to_coldkey_account(&coldkey2, stake + 1_000.into());

        // Stake with coldkey1
        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey1),
            old_hotkey,
            netuid1,
            stake
        ));

        // Stake with coldkey2 also
        assert_ok!(SubtensorModule::add_stake(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey2),
            old_hotkey,
            netuid2,
            stake
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

        assert_ok!(SubtensorModule::perform_hotkey_swap_on_all_subnets(
            &old_hotkey,
            &new_hotkey,
            &coldkey1,
            &mut weight,
            false
        ));

        // Check ownership transfer
        assert_eq!(
            SubtensorModule::get_owning_coldkey_for_hotkey(&new_hotkey),
            coldkey1
        );
        assert!(!SubtensorModule::get_owned_hotkeys(&coldkey2).contains(&new_hotkey));

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
                &new_hotkey,
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
            &new_hotkey
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
            SubtensorModule::get_total_stake_for_hotkey(&new_hotkey),
            total_hk_stake
        );
        assert_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&old_hotkey),
            TaoBalance::ZERO
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey -- test_swap_hotkey_no_tx_rate_limit --exact --nocapture
#[test]
fn test_swap_hotkey_no_tx_rate_limit() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        let old_hotkey = U256::from(1);
        let new_hotkey_1 = U256::from(2);
        let new_hotkey_2 = U256::from(4);
        let coldkey = U256::from(3);
        let swap_cost = SubtensorModule::get_key_swap_cost() * 2.into();

        let interval: u64 = <Test as crate::Config>::HotkeySwapOnSubnetInterval::get();

        // Set a tx rate limit far larger than the gap we leave between the two swaps below.
        // The generic tx rate limit no longer gates hotkey swaps, so the second swap must
        // succeed even though the old behaviour would have rejected it. Only the per-subnet
        // `HotkeySwapOnSubnetInterval` cooldown still applies.
        let tx_rate_limit = interval.saturating_mul(100);
        SubtensorModule::set_tx_rate_limit(tx_rate_limit);
        assert_eq!(SubtensorModule::get_tx_rate_limit(), tx_rate_limit);

        // Setup initial state
        add_network(netuid, tempo, 0);
        register_ok_neuron(netuid, old_hotkey, coldkey, 0);
        add_balance_to_coldkey_account(&coldkey, swap_cost + ExistentialDeposit::get());

        // Perform the first swap
        assert_ok!(SubtensorModule::do_swap_hotkey(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            &old_hotkey,
            &new_hotkey_1,
            None,
            false,
        ));

        // Advance just past the per-subnet swap cooldown, but far less than the tx rate
        // limit set above. Under the old rules the generic tx rate limit would reject this
        // second swap; with that limit removed it succeeds.
        step_block(interval as u16 + 1);
        assert_ok!(SubtensorModule::do_swap_hotkey(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            &new_hotkey_1,
            &new_hotkey_2,
            None,
            false,
        ));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey -- test_do_swap_hotkey_err_not_owner --exact --nocapture
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
                <<Test as Config>::RuntimeOrigin>::signed(not_owner_coldkey),
                &old_hotkey,
                &new_hotkey,
                None,
                false,
            ),
            Error::<Test>::NonAssociatedColdKey
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey -- test_swap_owner_old_hotkey_not_exist --exact --nocapture
#[test]
fn test_swap_owner_old_hotkey_not_exist() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let mut weight = Weight::zero();

        // Ensure old_hotkey does not exist
        assert!(!Owner::<Test>::contains_key(old_hotkey));

        // Perform the swap
        SubtensorModule::perform_hotkey_swap_on_all_subnets(
            &old_hotkey,
            &new_hotkey,
            &coldkey,
            &mut weight,
            false,
        );

        // Verify the swap
        assert_eq!(Owner::<Test>::get(new_hotkey), coldkey);
        assert!(!Owner::<Test>::contains_key(old_hotkey));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey -- test_swap_owner_new_hotkey_already_exists --exact --nocapture
#[test]
fn test_swap_owner_new_hotkey_already_exists() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let another_coldkey = U256::from(4);
        let mut weight = Weight::zero();

        // Initialize Owner for old_hotkey and new_hotkey
        Owner::<Test>::insert(old_hotkey, coldkey);
        Owner::<Test>::insert(new_hotkey, another_coldkey);

        // Perform the swap
        SubtensorModule::perform_hotkey_swap_on_all_subnets(
            &old_hotkey,
            &new_hotkey,
            &coldkey,
            &mut weight,
            false,
        );

        // Verify the swap
        assert_eq!(Owner::<Test>::get(new_hotkey), coldkey);
        assert!(!Owner::<Test>::contains_key(old_hotkey));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey::test_swap_stake_success --exact --nocapture
#[test]
fn test_swap_stake_success() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let amount = 10_000;
        let shares = U64F64::from_num(10_000);
        let mut weight = Weight::zero();

        // Initialize staking variables for old_hotkey
        TotalHotkeyAlpha::<Test>::insert(old_hotkey, netuid, AlphaBalance::from(amount));
        TotalHotkeyAlphaLastEpoch::<Test>::insert(
            old_hotkey,
            netuid,
            AlphaBalance::from(amount * 2),
        );
        TotalHotkeyShares::<Test>::insert(old_hotkey, netuid, shares);
        Alpha::<Test>::insert((old_hotkey, coldkey, netuid), U64F64::from_num(amount));
        AlphaDividendsPerSubnet::<Test>::insert(netuid, old_hotkey, AlphaBalance::from(amount));

        // Perform the swap
        SubtensorModule::perform_hotkey_swap_on_all_subnets(
            &old_hotkey,
            &new_hotkey,
            &coldkey,
            &mut weight,
            false,
        );

        // Verify the swap
        assert_eq!(
            TotalHotkeyAlpha::<Test>::get(old_hotkey, netuid),
            AlphaBalance::ZERO
        );
        assert_eq!(
            TotalHotkeyAlpha::<Test>::get(new_hotkey, netuid),
            amount.into()
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
        assert_eq!(
            TotalHotkeySharesV2::<Test>::get(new_hotkey, netuid),
            shares.into()
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
            amount.into()
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
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let amount = 10_000;
        let shares = U64F64::from_num(10_000);
        let mut weight = Weight::zero();

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
        SubtensorModule::perform_hotkey_swap_on_all_subnets(
            &old_hotkey,
            &new_hotkey,
            &coldkey,
            &mut weight,
            false,
        );

        // Verify the swap
        assert_eq!(
            TotalHotkeyAlpha::<Test>::get(old_hotkey, netuid),
            AlphaBalance::ZERO
        );
        assert_eq!(
            TotalHotkeyAlpha::<Test>::get(new_hotkey, netuid),
            amount.into()
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
            amount.into()
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey::test_swap_stake_old_hotkey_not_exist --exact --nocapture
#[test]
fn test_swap_stake_old_hotkey_not_exist() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let alpha = AlphaBalance::from(1000);
        let alpha_share = U64F64::from_num(1000);
        let mut weight = Weight::zero();
        let netuid = NetUid::from(1);

        // Initialize Stake for old_hotkey
        Alpha::<Test>::insert((old_hotkey, coldkey, netuid), alpha_share);
        TotalHotkeyAlpha::<Test>::insert(old_hotkey, netuid, alpha);
        TotalHotkeyShares::<Test>::insert(old_hotkey, netuid, alpha_share);

        // Ensure old_hotkey has a stake
        assert!(Alpha::<Test>::contains_key((old_hotkey, coldkey, netuid)));

        // Perform the swap
        SubtensorModule::perform_hotkey_swap_on_all_subnets(
            &old_hotkey,
            &new_hotkey,
            &coldkey,
            &mut weight,
            false,
        );

        // Verify that new_hotkey has the stake (in new AlphaV2 map) and old_hotkey does not
        assert!(AlphaV2::<Test>::contains_key((new_hotkey, coldkey, netuid)));
        assert!(!Alpha::<Test>::contains_key((old_hotkey, coldkey, netuid)));
    });
}

// // SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey -- test_swap_total_hotkey_coldkey_stakes_this_interval_success --exact --nocapture
// #[test]
// fn test_swap_total_hotkey_coldkey_stakes_this_interval_success() {
//     new_test_ext(1).execute_with(|| {
//         let old_hotkey = U256::from(1);
//         let new_hotkey = U256::from(2);
//         let coldkey = U256::from(3);
//         let stake = (1000u64, 42u64); // Example tuple value
//         let mut weight = Weight::zero();

//         // Initialize TotalHotkeyColdkeyStakesThisInterval for old_hotkey
//         TotalHotkeyColdkeyStakesThisInterval::<Test>::insert(old_hotkey, coldkey, stake);

//         // Perform the swap
//         SubtensorModule::perform_hotkey_swap_on_all_subnets(&old_hotkey, &new_hotkey, &coldkey, &mut weight);

//         // Verify the swap
//         assert_eq!(
//             TotalHotkeyColdkeyStakesThisInterval::<Test>::get(new_hotkey, coldkey),
//             stake
//         );
//         assert!(!TotalHotkeyColdkeyStakesThisInterval::<Test>::contains_key(
//             old_hotkey, coldkey
//         ));
//     });
// }

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey -- test_swap_hotkey_error_cases --exact --nocapture
#[test]
fn test_swap_hotkey_error_cases() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let wrong_coldkey = U256::from(4);

        // Set up initial state
        Owner::<Test>::insert(old_hotkey, coldkey);
        TotalNetworks::<Test>::put(1);
        SubtensorModule::set_last_tx_block(&coldkey, 0);

        // Test not enough balance
        let swap_cost = SubtensorModule::get_key_swap_cost();
        assert_err!(
            SubtensorModule::do_swap_hotkey(
                RuntimeOrigin::signed(coldkey),
                &old_hotkey,
                &new_hotkey,
                None,
                false,
            ),
            Error::<Test>::NotEnoughBalanceToPaySwapHotKey
        );

        let initial_balance = SubtensorModule::get_key_swap_cost() + 1000.into();
        add_balance_to_coldkey_account(&coldkey, initial_balance);

        // Test new hotkey same as old
        assert_noop!(
            SubtensorModule::do_swap_hotkey(
                RuntimeOrigin::signed(coldkey),
                &old_hotkey,
                &old_hotkey,
                None,
                false,
            ),
            Error::<Test>::NewHotKeyIsSameWithOld
        );

        // Test new hotkey already registered
        IsNetworkMember::<Test>::insert(new_hotkey, NetUid::ROOT, true);
        assert_noop!(
            SubtensorModule::do_swap_hotkey(
                RuntimeOrigin::signed(coldkey),
                &old_hotkey,
                &new_hotkey,
                None,
                false,
            ),
            Error::<Test>::HotKeyAlreadyRegisteredInSubNet
        );
        IsNetworkMember::<Test>::remove(new_hotkey, NetUid::ROOT);

        // Test non-associated coldkey
        assert_noop!(
            SubtensorModule::do_swap_hotkey(
                RuntimeOrigin::signed(wrong_coldkey),
                &old_hotkey,
                &new_hotkey,
                None,
                false,
            ),
            Error::<Test>::NonAssociatedColdKey
        );

        // Run the successful swap
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            None,
            false,
        ));

        // Check balance after swap
        assert_eq!(Balances::free_balance(coldkey), initial_balance - swap_cost);
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey::test_do_swap_hotkey_err_new_hotkey_not_clean_for_root --exact --nocapture
#[test]
fn test_do_swap_hotkey_err_new_hotkey_not_clean_for_root() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let other_coldkey = U256::from(4);

        Owner::<Test>::insert(old_hotkey, coldkey);
        NetworksAdded::<Test>::insert(NetUid::ROOT, true);
        TotalNetworks::<Test>::put(1);
        SubtensorModule::set_last_tx_block(&coldkey, 0);

        let initial_balance = SubtensorModule::get_key_swap_cost() + 1000.into();
        add_balance_to_coldkey_account(&coldkey, initial_balance);

        // new_hotkey is NOT registered on any network, but some other coldkey
        // has staked to it on root. This must block a root-touching swap.
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &new_hotkey,
            &other_coldkey,
            NetUid::ROOT,
            AlphaBalance::from(1_000_000u64),
        );

        assert!(!SubtensorModule::is_hotkey_registered_on_any_network(
            &new_hotkey
        ));
        assert!(
            SubtensorModule::get_stake_for_hotkey_on_subnet(&new_hotkey, NetUid::ROOT)
                > AlphaBalance::ZERO
        );

        // Full swap (netuid = None) — touches root, must fail.
        assert_noop!(
            SubtensorModule::do_swap_hotkey(
                RuntimeOrigin::signed(coldkey),
                &old_hotkey,
                &new_hotkey,
                None,
                false,
            ),
            Error::<Test>::NewHotKeyNotCleanForRootSwap
        );

        // Explicit root-subnet swap — also must fail.
        assert_noop!(
            SubtensorModule::do_swap_hotkey(
                RuntimeOrigin::signed(coldkey),
                &old_hotkey,
                &new_hotkey,
                Some(NetUid::ROOT),
                false,
            ),
            Error::<Test>::NewHotKeyNotCleanForRootSwap
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey -- test_swap_child_keys --exact --nocapture
#[test]
fn test_swap_child_keys() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let netuid = NetUid::from(0u16);
        let children = vec![(100u64, U256::from(4)), (200u64, U256::from(5))];
        let mut weight = Weight::zero();

        // Initialize ChildKeys for old_hotkey
        add_network(netuid, 1, 0);
        ChildKeys::<Test>::insert(old_hotkey, netuid, children.clone());

        // Perform the swap
        SubtensorModule::perform_hotkey_swap_on_all_subnets(
            &old_hotkey,
            &new_hotkey,
            &coldkey,
            &mut weight,
            false,
        );

        // Verify the swap
        assert_eq!(ChildKeys::<Test>::get(new_hotkey, netuid), children);
        assert!(ChildKeys::<Test>::get(old_hotkey, netuid).is_empty());
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey -- test_swap_parent_keys --exact --nocapture
#[test]
fn test_swap_parent_keys() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let netuid = NetUid::from(0u16);
        let parents = vec![(100u64, U256::from(4)), (200u64, U256::from(5))];
        let mut weight = Weight::zero();

        // Initialize ParentKeys for old_hotkey
        add_network(netuid, 1, 0);
        ParentKeys::<Test>::insert(old_hotkey, netuid, parents.clone());

        // Initialize ChildKeys for parent
        ChildKeys::<Test>::insert(U256::from(4), netuid, vec![(100u64, old_hotkey)]);
        ChildKeys::<Test>::insert(U256::from(5), netuid, vec![(200u64, old_hotkey)]);

        // Perform the swap
        SubtensorModule::perform_hotkey_swap_on_all_subnets(
            &old_hotkey,
            &new_hotkey,
            &coldkey,
            &mut weight,
            false,
        );

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

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey -- test_swap_multiple_subnets --exact --nocapture
#[test]
fn test_swap_multiple_subnets() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let netuid1 = NetUid::from(0);
        let netuid2 = NetUid::from(1);
        let children1 = vec![(100u64, U256::from(4)), (200u64, U256::from(5))];
        let children2 = vec![(300u64, U256::from(6))];
        let mut weight = Weight::zero();

        add_network(netuid1, 1, 0);
        add_network(netuid2, 1, 0);

        // Initialize ChildKeys for old_hotkey in multiple subnets
        ChildKeys::<Test>::insert(old_hotkey, netuid1, children1.clone());
        ChildKeys::<Test>::insert(old_hotkey, netuid2, children2.clone());

        // Perform the swap
        SubtensorModule::perform_hotkey_swap_on_all_subnets(
            &old_hotkey,
            &new_hotkey,
            &coldkey,
            &mut weight,
            false,
        );

        // Verify the swap for both subnets
        assert_eq!(ChildKeys::<Test>::get(new_hotkey, netuid1), children1);
        assert_eq!(ChildKeys::<Test>::get(new_hotkey, netuid2), children2);
        assert!(ChildKeys::<Test>::get(old_hotkey, netuid1).is_empty());
        assert!(ChildKeys::<Test>::get(old_hotkey, netuid2).is_empty());
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey -- test_swap_complex_parent_child_structure --exact --nocapture
#[test]
fn test_swap_complex_parent_child_structure() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let netuid = NetUid::from(0u16);
        let parent1 = U256::from(4);
        let parent2 = U256::from(5);
        let child1 = U256::from(6);
        let child2 = U256::from(7);
        let mut weight = Weight::zero();

        add_network(netuid, 1, 0);

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
        SubtensorModule::perform_hotkey_swap_on_all_subnets(
            &old_hotkey,
            &new_hotkey,
            &coldkey,
            &mut weight,
            false,
        );

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
        let netuid = NetUid::from(1);
        let parent_old = U256::from(1);
        let coldkey = U256::from(2);
        let child = U256::from(3);
        let child_other = U256::from(4);
        let parent_new = U256::from(5);
        add_network(netuid, 1, 0);
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
        let mut weight = Weight::zero();
        assert_ok!(SubtensorModule::perform_hotkey_swap_on_all_subnets(
            &parent_old,
            &parent_new,
            &coldkey,
            &mut weight,
            false
        ));

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
        let netuid = NetUid::from(1);
        let parent = U256::from(1);
        let coldkey = U256::from(2);
        let child_old = U256::from(3);
        let child_new = U256::from(4);
        add_network(netuid, 1, 0);
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
        let mut weight = Weight::zero();
        assert_ok!(SubtensorModule::perform_hotkey_swap_on_all_subnets(
            &child_old,
            &child_new,
            &coldkey,
            &mut weight,
            false
        ));

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

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey -- test_swap_hotkey_is_sn_owner_hotkey --exact --nocapture
#[test]
fn test_swap_hotkey_is_sn_owner_hotkey() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let mut weight = Weight::zero();

        // Create dynamic network
        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        // Check for SubnetOwnerHotkey
        assert_eq!(SubnetOwnerHotkey::<Test>::get(netuid), old_hotkey);

        // Perform the swap
        SubtensorModule::perform_hotkey_swap_on_all_subnets(
            &old_hotkey,
            &new_hotkey,
            &coldkey,
            &mut weight,
            false,
        );

        // Check for SubnetOwnerHotkey
        assert_eq!(SubnetOwnerHotkey::<Test>::get(netuid), new_hotkey);
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test swap_hotkey -- test_swap_hotkey_swap_rate_limits --exact --nocapture
#[test]
fn test_swap_hotkey_swap_rate_limits() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let netuid = add_dynamic_network(&old_hotkey, &coldkey);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        let delegate_take_block = 4567;
        let child_key_take_block = 8910;

        // Set the last delegate take block for the old hotkey
        SubtensorModule::set_last_tx_block_delegate_take(&old_hotkey, delegate_take_block);
        // Set last childkey take block for the old hotkey
        SubtensorModule::set_last_tx_block_childkey(&old_hotkey, child_key_take_block);

        // Perform the swap
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            None,
            false,
        ));

        // Check for new hotkey (LastTxBlock is no longer transferred: the generic tx rate
        // limit was removed, so it is dead state). The take rate limits are still active.
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
fn test_swap_parent_hotkey_self_loops_in_pending() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let parent_old = U256::from(1);
        let coldkey = U256::from(2);
        let child = U256::from(3);
        let child_other = U256::from(4);

        // Same as child_other, so it will self-loop when pending is set. Should fail.
        let parent_new = U256::from(4);
        add_network(netuid, 1, 0);
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
        let mut weight = Weight::zero();
        assert_err!(
            SubtensorModule::perform_hotkey_swap_on_all_subnets(
                &parent_old,
                &parent_new,
                &coldkey,
                &mut weight,
                false
            ),
            Error::<Test>::InvalidChild
        );
    })
}

#[test]
fn test_swap_auto_stake_destination_coldkeys() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        let netuid = NetUid::from(2u16); // Can't be root
        let coldkeys = vec![U256::from(4), U256::from(5), coldkey];
        let mut weight = Weight::zero();

        // Initialize ChildKeys for old_hotkey
        add_network(netuid, 1, 0);
        AutoStakeDestinationColdkeys::<Test>::insert(old_hotkey, netuid, coldkeys.clone());
        AutoStakeDestination::<Test>::insert(coldkey, netuid, old_hotkey);

        // Perform the swap
        SubtensorModule::perform_hotkey_swap_on_all_subnets(
            &old_hotkey,
            &new_hotkey,
            &coldkey,
            &mut weight,
            false,
        );

        // Verify the swap
        assert_eq!(
            AutoStakeDestinationColdkeys::<Test>::get(new_hotkey, netuid),
            coldkeys
        );
        assert!(AutoStakeDestinationColdkeys::<Test>::get(old_hotkey, netuid).is_empty());
        assert_eq!(
            AutoStakeDestination::<Test>::get(coldkey, netuid),
            Some(new_hotkey)
        );
    });
}

// ============================================================
// GHSA-2026-011 regression test — security audit (June 2026)
// Fails on the vulnerable code; passes with the fix in this PR.
// ============================================================
use crate::staking::lock::LockState;

#[test]
fn ghsa_2026_011_subnet_swap_interval_bypassed_by_all_subnets_path() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1); // not root (root == 0)
        let tempo: u16 = 13;
        let coldkey = U256::from(3);
        let old_hotkey = U256::from(1);
        let hk_a = U256::from(2); // result of the per-subnet swap
        let hk_contrast = U256::from(6); // attempted per-subnet re-swap (must fail)
        let hk_b = U256::from(7); // attempted all-subnets bypass swap (must now also fail)

        // The per-subnet cooldown configured in the mock.
        let interval: u64 = <Test as crate::Config>::HotkeySwapOnSubnetInterval::get();
        assert_eq!(interval, 15);

        // Setup: coldkey owns old_hotkey, registered on subnet N.
        add_network(netuid, tempo, 0);
        register_ok_neuron(netuid, old_hotkey, coldkey, 0);
        // Fund the coldkey generously for both per-subnet and all-subnets swap costs.
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        // Advance the block past the interval so the FIRST per-subnet swap is allowed
        // (LastHotkeySwapOnNetuid defaults to 0; the check is 0 + interval < block).
        // Do NOT step further afterwards: the on_finalize cleanup hook would otherwise
        // purge stale LastHotkeySwapOnNetuid rows once the interval elapses.
        step_block(20);
        let block = SubtensorModule::get_current_block_as_u64();
        assert!(block > interval);

        // Precondition sanity: no swap record yet for (netuid, coldkey).
        assert_eq!(LastHotkeySwapOnNetuid::<Test>::get(netuid, coldkey), 0);

        // 1. Per-subnet swap old_hotkey -> hk_a on subnet N. This stamps
        //    LastHotkeySwapOnNetuid(N, coldkey) = current block, opening the cooldown.
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &hk_a,
            Some(netuid),
            false,
        ));
        assert_eq!(
            LastHotkeySwapOnNetuid::<Test>::get(netuid, coldkey),
            block,
            "per-subnet swap must record the swap block for the cooldown"
        );

        // 2. CONTRAST (the rate limit works on the per-subnet path):
        //    Immediately re-swapping on the SAME subnet within the interval fails.
        assert_err!(
            SubtensorModule::do_swap_hotkey(
                RuntimeOrigin::signed(coldkey),
                &hk_a,
                &hk_contrast,
                Some(netuid),
                false,
            ),
            Error::<Test>::HotKeySwapOnSubnetIntervalNotPassed
        );
        // State unchanged by the rejected per-subnet swap.
        assert!(SubtensorModule::is_hotkey_registered_on_specific_network(
            &hk_a, netuid
        ));
        assert!(!SubtensorModule::is_hotkey_registered_on_specific_network(
            &hk_contrast,
            netuid
        ));

        // 3. FIXED (GHSA-2026-011): the all-subnets path (netuid=None) now also consults
        //    the per-subnet interval for every subnet the old hotkey is a member of, so an
        //    immediate swap via netuid=None within the cooldown is rejected with the same
        //    error and CANNOT bypass the per-subnet cooldown.
        assert_err!(
            SubtensorModule::do_swap_hotkey(
                RuntimeOrigin::signed(coldkey),
                &hk_a,
                &hk_b,
                None,
                false,
            ),
            Error::<Test>::HotKeySwapOnSubnetIntervalNotPassed
        );

        // The bypass swap did NOT take effect: ownership stays with hk_a (from step 1),
        // and hk_b never became an owner.
        assert_eq!(Owner::<Test>::get(hk_a), coldkey);
        assert!(!Owner::<Test>::contains_key(hk_b));
        // The per-subnet cooldown record is unchanged (still the step-1 block).
        assert_eq!(LastHotkeySwapOnNetuid::<Test>::get(netuid, coldkey), block);

        // 4. After the cooldown elapses, the all-subnets swap is allowed again and now
        //    correctly re-stamps the per-subnet cooldown for the affected subnet.
        step_block((interval + 1) as u16);
        let block_after = SubtensorModule::get_current_block_as_u64();
        assert!(block_after > block.saturating_add(interval));
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &hk_a,
            &hk_b,
            None,
            false,
        ));
        assert_eq!(Owner::<Test>::get(hk_b), coldkey);
        assert!(!Owner::<Test>::contains_key(hk_a));
        // The all-subnets path now records the per-subnet cooldown.
        assert_eq!(
            LastHotkeySwapOnNetuid::<Test>::get(netuid, coldkey),
            block_after,
            "all-subnets swap must record the per-subnet cooldown block"
        );
    });
}

// ============================================================
// GHSA-2026-011 follow-up (review): the all-subnets cooldown must cover subnets where the
// old hotkey is a PARENT (has childkeys) — those are migrated even on subnets it is not a
// member of — but must NOT gate on the CHILD side (ParentKeys), since a third party can set
// any hotkey as its child without consent (that would be a griefing vector).
// Fails on the member-only filter; passes with the membership-or-parent filter in this PR.
// ============================================================
#[test]
fn ghsa_2026_011_all_subnets_swap_covers_parent_key_subnets_not_child_side() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(3);
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let child = U256::from(8);
        let foreign_parent = U256::from(9);

        // member_netuid: old_hotkey is a registered member here.
        let member_netuid = NetUid::from(1);
        // parent_netuid: old_hotkey is ONLY a parent here (has a childkey), NOT a member.
        let parent_netuid = NetUid::from(2);
        // child_netuid: old_hotkey is ONLY a child here (some other hotkey's child), NOT a
        // member and not a parent. This must NOT be cooldown-gated (anti-griefing).
        let child_netuid = NetUid::from(3);

        let interval: u64 = <Test as crate::Config>::HotkeySwapOnSubnetInterval::get();

        add_network(member_netuid, 13, 0);
        add_network(parent_netuid, 13, 0);
        add_network(child_netuid, 13, 0);
        register_ok_neuron(member_netuid, old_hotkey, coldkey, 0);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_u64.into());

        // old_hotkey is a PARENT on parent_netuid (has a child) but NOT a network member there.
        ChildKeys::<Test>::insert(old_hotkey, parent_netuid, vec![(u64::MAX, child)]);
        assert!(!IsNetworkMember::<Test>::get(old_hotkey, parent_netuid));
        // old_hotkey is only a CHILD on child_netuid (set by foreign_parent, no consent).
        ParentKeys::<Test>::insert(old_hotkey, child_netuid, vec![(u64::MAX, foreign_parent)]);
        assert!(!IsNetworkMember::<Test>::get(old_hotkey, child_netuid));
        assert!(ChildKeys::<Test>::get(old_hotkey, child_netuid).is_empty());

        // Advance past the interval so the swap itself is allowed (first swap on each subnet).
        step_block(20);
        let block = SubtensorModule::get_current_block_as_u64();
        assert!(block > interval);

        // Preconditions: no cooldown recorded on the parent-only or child-only subnets.
        assert_eq!(LastHotkeySwapOnNetuid::<Test>::get(parent_netuid, coldkey), 0);
        assert_eq!(LastHotkeySwapOnNetuid::<Test>::get(child_netuid, coldkey), 0);

        // All-subnets swap (netuid = None).
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &old_hotkey,
            &new_hotkey,
            None,
            false,
        ));

        // The relationships are re-homed onto new_hotkey regardless of membership.
        assert_eq!(
            ChildKeys::<Test>::get(new_hotkey, parent_netuid),
            vec![(u64::MAX, child)],
            "parent relationship must migrate to new_hotkey on the parent-only subnet"
        );
        assert_eq!(
            ParentKeys::<Test>::get(new_hotkey, child_netuid),
            vec![(u64::MAX, foreign_parent)],
            "child relationship must still migrate to new_hotkey on the child-only subnet"
        );

        // FIXED: the per-subnet cooldown is recorded on the member subnet AND the parent-only
        // subnet (the member-only filter would have left the latter at 0).
        assert_eq!(LastHotkeySwapOnNetuid::<Test>::get(member_netuid, coldkey), block);
        assert_eq!(
            LastHotkeySwapOnNetuid::<Test>::get(parent_netuid, coldkey),
            block,
            "all-subnets swap must record the cooldown on parent-key subnets, not just member subnets"
        );

        // Anti-griefing: the child-only subnet is NOT cooldown-gated. A third party (the
        // foreign parent) set old_hotkey as its child without consent; gating on that would
        // let it impose swap-cooldowns on the victim.
        assert_eq!(
            LastHotkeySwapOnNetuid::<Test>::get(child_netuid, coldkey),
            0,
            "child-only subnet must NOT be cooldown-gated (no-consent child assignment is a griefing vector)"
        );
    });
}

// ============================================================
// GHSA-2026-014 regression test — security audit (June 2026)
// Fails on the vulnerable code; passes with the fix in this PR.
// ============================================================

#[test]
fn ghsa_2026_014_childkey_take_not_migrated_on_hotkey_swap() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let coldkey = U256::from(2);
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(5);
        let mut weight = Weight::zero();

        add_network(netuid, 1, 0);
        // Establish coldkey ownership of old_hotkey (Owner(old_hotkey) = coldkey).
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &coldkey,
            &old_hotkey
        ));

        // The effective minimum (floor) childkey take in this mock is 0.
        let floor_take = SubtensorModule::get_effective_min_childkey_take(netuid);
        assert_eq!(floor_take, 0);

        // Configure a NON-minimum childkey take on the old hotkey for this subnet.
        // 5000 is well above the floor (0) and below the max (11_796).
        let configured_childkey_take: u16 = 5000;
        assert!(configured_childkey_take > floor_take);
        ChildkeyTake::<Test>::insert(old_hotkey, netuid, configured_childkey_take);

        // Configure a NON-default delegate take on the old hotkey for contrast.
        // (Default delegate take in this mock is 11_796.)
        let configured_delegate_take: u16 = 100;
        Delegates::<Test>::insert(old_hotkey, configured_delegate_take);

        // Sanity: pre-swap the old hotkey carries the configured values.
        assert_eq!(
            ChildkeyTake::<Test>::get(old_hotkey, netuid),
            configured_childkey_take
        );
        assert_eq!(
            SubtensorModule::get_childkey_take(&old_hotkey, netuid),
            configured_childkey_take
        );
        assert_eq!(Delegates::<Test>::get(old_hotkey), configured_delegate_take);

        // Perform the real hotkey swap on all subnets.
        assert_ok!(SubtensorModule::perform_hotkey_swap_on_all_subnets(
            &old_hotkey,
            &new_hotkey,
            &coldkey,
            &mut weight,
            false
        ));

        // CONTRAST (safe behavior): Delegates take IS migrated to the new hotkey.
        assert!(!Delegates::<Test>::contains_key(old_hotkey));
        assert_eq!(Delegates::<Test>::get(new_hotkey), configured_delegate_take);

        // FIXED (GHSA-2026-014): ChildkeyTake IS now migrated to the new hotkey.
        // The new hotkey carries over the configured take (5000) instead of
        // silently dropping to the storage default (0) / floor take.
        assert_eq!(
            ChildkeyTake::<Test>::get(new_hotkey, netuid),
            configured_childkey_take,
            "ChildkeyTake(new_hotkey) should inherit the configured take after swap"
        );
        // The effective getter for the new hotkey returns the configured take, not the floor.
        assert_eq!(
            SubtensorModule::get_childkey_take(&new_hotkey, netuid),
            configured_childkey_take
        );

        // FIXED: the old hotkey's ChildkeyTake row is removed (no orphan left behind).
        assert!(!ChildkeyTake::<Test>::contains_key(old_hotkey, netuid));
        assert_eq!(ChildkeyTake::<Test>::get(old_hotkey, netuid), 0);
    });
}
