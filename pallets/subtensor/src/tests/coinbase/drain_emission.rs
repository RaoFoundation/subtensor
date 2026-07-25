#![allow(
    unused,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used
)]
//! Drain pending emission to stakers, including childkey edges.

use super::helpers::*;
use super::prelude::*;

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::drain_emission::test_drain_base --exact --show-output --nocapture
#[test]
fn test_drain_base() {
    new_test_ext(1).execute_with(|| {
        SubtensorModule::distribute_emission(
            0.into(),
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
        )
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::drain_emission::test_drain_base_with_subnet --exact --show-output --nocapture
#[test]
fn test_drain_base_with_subnet() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
        )
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::drain_emission::test_drain_base_with_subnet_with_single_staker_not_registered --exact --show-output --nocapture
#[test]
fn test_drain_base_with_subnet_with_single_staker_not_registered() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        let hotkey = U256::from(1);
        let coldkey = U256::from(2);
        let stake_before = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            stake_before,
        );
        let pending_alpha = AlphaBalance::from(1_000_000_000);
        SubtensorModule::distribute_emission(
            netuid,
            pending_alpha.saturating_div(2.into()).into(),
            pending_alpha.saturating_div(2.into()).into(),
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
        );
        let stake_after =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);
        assert_eq!(stake_before, stake_after); // Not registered.
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::drain_emission::test_drain_base_with_subnet_with_single_staker_registered --exact --show-output --nocapture
#[test]
fn test_drain_base_with_subnet_with_single_staker_registered() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        let hotkey = U256::from(1);
        let coldkey = U256::from(2);
        let stake_before = AlphaBalance::from(1_000_000_000);
        register_ok_neuron(netuid, hotkey, coldkey, 0);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            stake_before,
        );
        let pending_alpha = AlphaBalance::from(1_000_000_000);
        SubtensorModule::distribute_emission(
            netuid,
            pending_alpha.saturating_div(2.into()).into(),
            pending_alpha.saturating_div(2.into()).into(),
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
        );
        let stake_after =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);
        close(
            (stake_before + pending_alpha).into(),
            stake_after.into(),
            10,
        ); // Registered gets all emission.
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::drain_emission::test_drain_base_with_subnet_with_single_staker_registered_root_weight --exact --show-output --nocapture
#[test]
fn test_drain_base_with_subnet_with_single_staker_registered_root_weight() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        let hotkey = U256::from(1);
        let coldkey = U256::from(2);
        let stake_before = AlphaBalance::from(1_000_000_000);
        // register_ok_neuron(root, hotkey, coldkey, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);
        Delegates::<Test>::insert(hotkey, PerU16::zero());
        SubtensorModule::set_tao_weight(u64::MAX); // Set TAO weight to 1.0
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            NetUid::ROOT,
            stake_before,
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            stake_before,
        );
        let pending_alpha = AlphaBalance::from(1_000_000_000);
        let pending_root_alpha = AlphaBalance::from(1_000_000_000);
        assert_eq!(SubnetTAO::<Test>::get(NetUid::ROOT), TaoBalance::ZERO);
        SubtensorModule::distribute_emission(
            netuid,
            pending_alpha.saturating_div(2.into()).into(),
            pending_alpha.saturating_div(2.into()).into(),
            pending_root_alpha,
            AlphaBalance::ZERO,
        );
        let stake_after =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);
        let root_after = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            NetUid::ROOT,
        );
        close(
            (stake_before + pending_alpha).into(),
            stake_after.into(),
            10,
        ); // Registered gets all alpha emission.
        close(stake_before.to_u64(), root_after.into(), 10); // Registered doesn't get tao immediately
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::drain_emission::test_drain_base_with_subnet_with_two_stakers_registered --exact --show-output --nocapture
#[test]
fn test_drain_base_with_subnet_with_two_stakers_registered() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        let hotkey1 = U256::from(1);
        let hotkey2 = U256::from(2);
        let coldkey = U256::from(3);
        let stake_before = AlphaBalance::from(1_000_000_000);
        register_ok_neuron(netuid, hotkey1, coldkey, 0);
        register_ok_neuron(netuid, hotkey2, coldkey, 0);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey1,
            &coldkey,
            netuid,
            stake_before,
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey2,
            &coldkey,
            netuid,
            stake_before,
        );
        let pending_alpha = AlphaBalance::from(1_000_000_000);
        SubtensorModule::distribute_emission(
            netuid,
            pending_alpha.saturating_div(2.into()).into(),
            pending_alpha.saturating_div(2.into()).into(),
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
        );
        let stake_after1 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey1, &coldkey, netuid);
        let stake_after2 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey2, &coldkey, netuid);
        close(
            (stake_before + pending_alpha / 2.into()).into(),
            stake_after1.into(),
            10,
        ); // Registered gets 1/2 emission
        close(
            (stake_before + pending_alpha / 2.into()).into(),
            stake_after2.into(),
            10,
        ); // Registered gets 1/2 emission.
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::drain_emission::test_drain_base_with_subnet_with_two_stakers_registered_and_root --exact --show-output --nocapture
#[test]
fn test_drain_base_with_subnet_with_two_stakers_registered_and_root() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        let hotkey1 = U256::from(1);
        let hotkey2 = U256::from(2);
        let coldkey = U256::from(3);
        let stake_before = AlphaBalance::from(1_000_000_000);
        register_ok_neuron(netuid, hotkey1, coldkey, 0);
        register_ok_neuron(netuid, hotkey2, coldkey, 0);
        Delegates::<Test>::insert(hotkey1, PerU16::zero());
        Delegates::<Test>::insert(hotkey2, PerU16::zero());
        SubtensorModule::set_tao_weight(u64::MAX); // Set TAO weight to 1.0
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey1,
            &coldkey,
            netuid,
            stake_before,
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey1,
            &coldkey,
            NetUid::ROOT,
            stake_before,
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey2,
            &coldkey,
            netuid,
            stake_before,
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey2,
            &coldkey,
            NetUid::ROOT,
            stake_before,
        );
        let pending_tao = TaoBalance::from(1_000_000_000);
        let pending_alpha = AlphaBalance::from(1_000_000_000);
        assert_eq!(SubnetTAO::<Test>::get(NetUid::ROOT), TaoBalance::ZERO);
        SubtensorModule::distribute_emission(
            netuid,
            pending_alpha.saturating_div(2.into()).into(),
            pending_alpha.saturating_div(2.into()).into(),
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
        );
        let stake_after1 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey1, &coldkey, netuid);
        let root_after1 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey1,
            &coldkey,
            NetUid::ROOT,
        );
        let stake_after2 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey2, &coldkey, netuid);
        let root_after2 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey2,
            &coldkey,
            NetUid::ROOT,
        );
        close(
            (stake_before + pending_alpha / 2.into()).into(),
            stake_after1.into(),
            10,
        ); // Registered gets 1/2 emission
        close(
            (stake_before + pending_alpha / 2.into()).into(),
            stake_after2.into(),
            10,
        ); // Registered gets 1/2 emission.
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::drain_emission::test_drain_base_with_subnet_with_two_stakers_registered_and_root_different_amounts --exact --show-output --nocapture
#[test]
fn test_drain_base_with_subnet_with_two_stakers_registered_and_root_different_amounts() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        let hotkey1 = U256::from(1);
        let hotkey2 = U256::from(2);
        let coldkey = U256::from(3);
        let stake_before = AlphaBalance::from(1_000_000_000);
        Delegates::<Test>::insert(hotkey1, PerU16::zero());
        Delegates::<Test>::insert(hotkey2, PerU16::zero());
        register_ok_neuron(netuid, hotkey1, coldkey, 0);
        register_ok_neuron(netuid, hotkey2, coldkey, 0);
        SubtensorModule::set_tao_weight(u64::MAX); // Set TAO weight to 1.0
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey1,
            &coldkey,
            netuid,
            stake_before,
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey1,
            &coldkey,
            NetUid::ROOT,
            stake_before * 2.into(), // Hotkey 1 has twice as much root weight.
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey2,
            &coldkey,
            netuid,
            stake_before,
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey2,
            &coldkey,
            NetUid::ROOT,
            stake_before,
        );
        let pending_tao = TaoBalance::from(1_000_000_000);
        let pending_alpha = AlphaBalance::from(1_000_000_000);
        assert_eq!(SubnetTAO::<Test>::get(NetUid::ROOT), TaoBalance::ZERO);
        SubtensorModule::distribute_emission(
            netuid,
            pending_alpha.saturating_div(2.into()).into(),
            pending_alpha.saturating_div(2.into()).into(),
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
        );
        let stake_after1 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey1, &coldkey, netuid);
        let root_after1 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey1,
            &coldkey,
            NetUid::ROOT,
        );
        let stake_after2 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey2, &coldkey, netuid);
        let root_after2 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey2,
            &coldkey,
            NetUid::ROOT,
        );
        let expected_stake = I96F32::from_num(stake_before)
            + (I96F32::from_num(pending_alpha) * I96F32::from_num(1.0 / 2.0));
        assert_abs_diff_eq!(
            expected_stake.to_num::<u64>(),
            stake_after1.into(),
            epsilon = 10
        ); // Registered gets 50% of alpha emission
        let expected_stake2 = I96F32::from_num(stake_before)
            + I96F32::from_num(pending_alpha) * I96F32::from_num(1.0 / 2.0);
        assert_abs_diff_eq!(
            expected_stake2.to_num::<u64>(),
            stake_after2.into(),
            epsilon = 10
        ); // Registered gets 50% emission
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::drain_emission::test_drain_base_with_subnet_with_two_stakers_registered_and_root_different_amounts_half_tao_weight --exact --show-output --nocapture
#[test]
fn test_drain_base_with_subnet_with_two_stakers_registered_and_root_different_amounts_half_tao_weight()
 {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        let hotkey1 = U256::from(1);
        let hotkey2 = U256::from(2);
        let coldkey = U256::from(3);
        let stake_before = AlphaBalance::from(1_000_000_000);
        Delegates::<Test>::insert(hotkey1, PerU16::zero());
        Delegates::<Test>::insert(hotkey2, PerU16::zero());
        register_ok_neuron(netuid, hotkey1, coldkey, 0);
        register_ok_neuron(netuid, hotkey2, coldkey, 0);
        SubtensorModule::set_tao_weight(u64::MAX / 2); // Set TAO weight to 0.5
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey1,
            &coldkey,
            netuid,
            stake_before,
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey1,
            &coldkey,
            NetUid::ROOT,
            stake_before * 2.into(), // Hotkey 1 has twice as much root weight.
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey2,
            &coldkey,
            netuid,
            stake_before,
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey2,
            &coldkey,
            NetUid::ROOT,
            stake_before,
        );
        let pending_tao = TaoBalance::from(1_000_000_000);
        let pending_alpha = AlphaBalance::from(1_000_000_000);
        assert_eq!(SubnetTAO::<Test>::get(NetUid::ROOT), TaoBalance::ZERO);
        SubtensorModule::distribute_emission(
            netuid,
            pending_alpha.saturating_div(2.into()).into(),
            pending_alpha.saturating_div(2.into()).into(),
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
        );
        let stake_after1 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey1, &coldkey, netuid);
        let root_after1 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey1,
            &coldkey,
            NetUid::ROOT,
        );
        let stake_after2 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey2, &coldkey, netuid);
        let root_after2 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey2,
            &coldkey,
            NetUid::ROOT,
        );
        let expected_stake = I96F32::from_num(stake_before)
            + I96F32::from_num(pending_alpha) * I96F32::from_num(1.0 / 2.0);
        assert_abs_diff_eq!(
            expected_stake.to_num::<u64>(),
            u64::from(stake_after1),
            epsilon = 10
        );
        let expected_stake2 = I96F32::from_num(stake_before)
            + I96F32::from_num(pending_alpha) * I96F32::from_num(1.0 / 2.0);
        assert_abs_diff_eq!(
            expected_stake2.to_num::<u64>(),
            u64::from(stake_after2),
            epsilon = 10
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::drain_emission::test_drain_alpha_childkey_parentkey --exact --show-output --nocapture
#[test]
fn test_drain_alpha_childkey_parentkey() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        SubtensorModule::set_ck_burn(0);
        let parent = U256::from(1);
        let child = U256::from(2);
        let coldkey = U256::from(3);
        let stake_before = AlphaBalance::from(1_000_000_000);
        register_ok_neuron(netuid, child, coldkey, 0);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &parent,
            &coldkey,
            netuid,
            stake_before,
        );
        mock_set_children_no_epochs(netuid, &parent, &[(u64::MAX, child)]);

        // Childkey take is 10%
        ChildkeyTake::<Test>::insert(child, netuid, PerU16::from_parts(u16::MAX / 10));

        let pending_alpha = AlphaBalance::from(1_000_000_000);
        SubtensorModule::distribute_emission(
            netuid,
            pending_alpha.saturating_div(2.into()).into(),
            pending_alpha.saturating_div(2.into()).into(),
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
        );
        let parent_stake_after = SubtensorModule::get_stake_for_hotkey_on_subnet(&parent, netuid);
        let child_stake_after = SubtensorModule::get_stake_for_hotkey_on_subnet(&child, netuid);

        // Child gets 10%, parent gets 90%
        let expected = I96F32::from_num(stake_before)
            + I96F32::from_num(pending_alpha) * I96F32::from_num(9.0 / 10.0);
        log::info!(
            "expected: {:?}, parent_stake_after: {:?}",
            expected.to_num::<u64>(),
            parent_stake_after
        );
        close(expected.to_num::<u64>(), parent_stake_after.into(), 10_000);
        let expected = I96F32::from_num(u64::from(pending_alpha)) / I96F32::from_num(10);
        close(expected.to_num::<u64>(), child_stake_after.into(), 10_000);
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::drain_emission::test_drain_alpha_childkey_parentkey_with_burn --exact --show-output --nocapture
#[test]
fn test_drain_alpha_childkey_parentkey_with_burn() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        let parent = U256::from(1);
        let child = U256::from(2);
        let coldkey = U256::from(3);
        let stake_before = AlphaBalance::from(1_000_000_000);
        register_ok_neuron(netuid, child, coldkey, 0);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &parent,
            &coldkey,
            netuid,
            stake_before,
        );
        mock_set_children_no_epochs(netuid, &parent, &[(u64::MAX, child)]);

        // Childkey take is 10%
        ChildkeyTake::<Test>::insert(child, netuid, PerU16::from_parts(u16::MAX / 10));

        let burn_rate = SubtensorModule::get_ck_burn();
        let parent_stake_before = SubtensorModule::get_stake_for_hotkey_on_subnet(&parent, netuid);
        let child_stake_before = SubtensorModule::get_stake_for_hotkey_on_subnet(&child, netuid);

        let pending_alpha = AlphaBalance::from(1_000_000_000);
        SubtensorModule::distribute_emission(
            netuid,
            pending_alpha.saturating_div(2.into()).into(),
            pending_alpha.saturating_div(2.into()).into(),
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
        );
        let parent_stake_after = SubtensorModule::get_stake_for_hotkey_on_subnet(&parent, netuid);
        let child_stake_after = SubtensorModule::get_stake_for_hotkey_on_subnet(&child, netuid);

        let expected_ck_burn = I96F32::from_num(pending_alpha)
            * I96F32::from_num(9.0 / 10.0)
            * I96F32::from_num(burn_rate);

        let expected_total = I96F32::from_num(pending_alpha) - expected_ck_burn;
        let parent_ratio = (I96F32::from_num(pending_alpha) * I96F32::from_num(9.0 / 10.0)
            - expected_ck_burn)
            / expected_total;
        let child_ratio = (I96F32::from_num(pending_alpha) / I96F32::from_num(10)) / expected_total;

        let expected =
            I96F32::from_num(stake_before) + I96F32::from_num(pending_alpha) * parent_ratio;
        log::info!(
            "expected: {:?}, parent_stake_after: {:?}",
            expected.to_num::<u64>(),
            parent_stake_after
        );

        close(
            expected.to_num::<u64>(),
            parent_stake_after.into(),
            3_000_000,
        );
        let expected = I96F32::from_num(u64::from(pending_alpha)) * child_ratio;
        close(
            expected.to_num::<u64>(),
            child_stake_after.into(),
            3_000_000,
        );
    });
}
