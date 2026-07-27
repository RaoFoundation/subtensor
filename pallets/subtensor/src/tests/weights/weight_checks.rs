#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
//! Unit tests for weight validation helpers (`check_length`, `normalize_weights`, …).

use sp_core::U256;

use crate::tests::mock::*;
use crate::*;

#[test]
fn test_check_length_allows_singleton() {
    new_test_ext(0).execute_with(|| {
        let netuid = NetUid::from(1);

        let max_allowed: u16 = 1;
        let min_allowed_weights = max_allowed;

        SubtensorModule::set_min_allowed_weights(netuid, min_allowed_weights);

        let uids: Vec<u16> = Vec::from_iter((0..max_allowed).map(|id| id + 1));
        let uid: u16 = uids[0];
        let weights: Vec<u16> = Vec::from_iter((0..max_allowed).map(|id| id + 1));

        let expected = true;
        let result = SubtensorModule::check_length(netuid, uid, &uids, &weights);

        assert_eq!(expected, result, "Failed get expected result");
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_check_length_weights_length_exceeds_min_allowed --exact --show-output --nocapture
/// Check _truthy_ path for weights within allowed range
#[test]
fn test_check_length_weights_length_exceeds_min_allowed() {
    new_test_ext(0).execute_with(|| {
        let netuid = NetUid::from(1);

        let max_allowed: u16 = 3;
        let min_allowed_weights = max_allowed;

        SubtensorModule::set_min_allowed_weights(netuid, min_allowed_weights);

        let uids: Vec<u16> = Vec::from_iter((0..max_allowed).map(|id| id + 1));
        let uid: u16 = uids[0];
        let weights: Vec<u16> = Vec::from_iter((0..max_allowed).map(|id| id + 1));

        let expected = true;
        let result = SubtensorModule::check_length(netuid, uid, &uids, &weights);

        assert_eq!(expected, result, "Failed get expected result");
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_check_length_to_few_weights --exact --show-output --nocapture
/// Check _falsey_ path for weights outside allowed range
#[test]
fn test_check_length_to_few_weights() {
    new_test_ext(0).execute_with(|| {
        let netuid = NetUid::from(1);

        let min_allowed_weights = 3;

        add_network(netuid, 1, 0);
        SubtensorModule::set_target_registrations_per_interval(netuid, 100);
        SubtensorModule::set_max_registrations_per_block(netuid, 100);
        // register morw than min allowed
        register_ok_neuron(1.into(), U256::from(1), U256::from(1), 300_000);
        register_ok_neuron(1.into(), U256::from(2), U256::from(2), 300_001);
        register_ok_neuron(1.into(), U256::from(3), U256::from(3), 300_002);
        register_ok_neuron(1.into(), U256::from(4), U256::from(4), 300_003);
        register_ok_neuron(1.into(), U256::from(5), U256::from(5), 300_004);
        register_ok_neuron(1.into(), U256::from(6), U256::from(6), 300_005);
        register_ok_neuron(1.into(), U256::from(7), U256::from(7), 300_006);
        SubtensorModule::set_min_allowed_weights(netuid, min_allowed_weights);

        let uids: Vec<u16> = Vec::from_iter((0..2).map(|id| id + 1));
        let weights: Vec<u16> = Vec::from_iter((0..2).map(|id| id + 1));
        let uid: u16 = uids[0];

        let expected = false;
        let result = SubtensorModule::check_length(netuid, uid, &uids, &weights);

        assert_eq!(expected, result, "Failed get expected result");
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_normalize_weights_does_not_mutate_when_sum_is_zero --exact --show-output --nocapture
/// Check do nothing path
#[test]
fn test_normalize_weights_does_not_mutate_when_sum_is_zero() {
    new_test_ext(0).execute_with(|| {
        let max_allowed: u16 = 3;

        let weights: Vec<u16> = Vec::from_iter((0..max_allowed).map(|_| 0));

        let expected = weights.clone();
        let result = SubtensorModule::normalize_weights(weights);

        assert_eq!(
            expected, result,
            "Failed get expected result when everything _should_ be fine"
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_normalize_weights_does_not_mutate_when_sum_not_zero --exact --show-output --nocapture
/// Check do something path
#[test]
fn test_normalize_weights_does_not_mutate_when_sum_not_zero() {
    new_test_ext(0).execute_with(|| {
        let max_allowed: u16 = 3;

        let weights: Vec<u16> = Vec::from_iter(0..max_allowed);

        let expected = weights.clone();
        let result = SubtensorModule::normalize_weights(weights);

        assert_eq!(expected.len(), result.len(), "Length of weights changed?!");
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_max_weight_limited_allow_self_weights_to_exceed_max_weight_limit --exact --show-output --nocapture
/// Check _truthy_ path for weights length
#[test]
fn test_max_weight_limited_allow_self_weights_to_exceed_max_weight_limit() {
    new_test_ext(0).execute_with(|| {
        let max_allowed: u16 = 1;

        let netuid = NetUid::from(1);
        let uids: Vec<u16> = Vec::from_iter((0..max_allowed).map(|id| id + 1));
        let uid: u16 = uids[0];
        let weights: Vec<u16> = vec![0];

        let expected = true;
        let result = SubtensorModule::max_weight_limited(netuid, uid, &uids, &weights);

        assert_eq!(
            expected, result,
            "Failed get expected result when everything _should_ be fine"
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_max_weight_limited_when_weight_limit_is_u16_max --exact --show-output --nocapture
/// Check _truthy_ path for max weight limit
#[test]
fn test_max_weight_limited_when_weight_limit_is_u16_max() {
    new_test_ext(0).execute_with(|| {
        let max_allowed: u16 = 3;

        let netuid = NetUid::from(1);
        let uids: Vec<u16> = Vec::from_iter((0..max_allowed).map(|id| id + 1));
        let uid: u16 = uids[0];
        let weights: Vec<u16> = Vec::from_iter((0..max_allowed).map(|_id| u16::MAX));

        let expected = true;
        let result = SubtensorModule::max_weight_limited(netuid, uid, &uids, &weights);

        assert_eq!(
            expected, result,
            "Failed get expected result when everything _should_ be fine"
        );
    });
}

#[test]
fn test_get_max_weight_limit_is_constant() {
    new_test_ext(0).execute_with(|| {
        assert_eq!(
            SubtensorModule::get_max_weight_limit(NetUid::from(1)),
            u16::MAX
        );
        assert_eq!(
            SubtensorModule::get_max_weight_limit(NetUid::ROOT),
            u16::MAX
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_is_self_weight_weights_length_not_one --exact --show-output --nocapture
/// Check _falsey_ path for weights length
#[test]
fn test_is_self_weight_weights_length_not_one() {
    new_test_ext(0).execute_with(|| {
        let max_allowed: u16 = 3;

        let uids: Vec<u16> = Vec::from_iter((0..max_allowed).map(|id| id + 1));
        let uid: u16 = uids[0];
        let weights: Vec<u16> = Vec::from_iter((0..max_allowed).map(|id| id + 1));

        let expected = false;
        let result = SubtensorModule::is_self_weight(uid, &uids, &weights);

        assert_eq!(
            expected, result,
            "Failed get expected result when `weights.len() != 1`"
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_is_self_weight_uid_not_in_uids --exact --show-output --nocapture
/// Check _falsey_ path for uid vs uids[0]
#[test]
fn test_is_self_weight_uid_not_in_uids() {
    new_test_ext(0).execute_with(|| {
        let max_allowed: u16 = 3;

        let uids: Vec<u16> = Vec::from_iter((0..max_allowed).map(|id| id + 1));
        let uid: u16 = uids[1];
        let weights: Vec<u16> = vec![0];

        let expected = false;
        let result = SubtensorModule::is_self_weight(uid, &uids, &weights);

        assert_eq!(
            expected, result,
            "Failed get expected result when `uid != uids[0]`"
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_is_self_weight_uid_in_uids --exact --show-output --nocapture
/// Check _truthy_ path
/// @TODO: double-check if this really be desired behavior
#[test]
fn test_is_self_weight_uid_in_uids() {
    new_test_ext(0).execute_with(|| {
        let max_allowed: u16 = 1;

        let uids: Vec<u16> = Vec::from_iter((0..max_allowed).map(|id| id + 1));
        let uid: u16 = uids[0];
        let weights: Vec<u16> = vec![0];

        let expected = true;
        let result = SubtensorModule::is_self_weight(uid, &uids, &weights);

        assert_eq!(
            expected, result,
            "Failed get expected result when everything _should_ be fine"
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_check_len_uids_within_allowed_within_network_pool --exact --show-output --nocapture
/// Check _truthy_ path
#[test]
fn test_check_len_uids_within_allowed_within_network_pool() {
    new_test_ext(0).execute_with(|| {
        let netuid = NetUid::from(1);

        let tempo: u16 = 13;
        let modality: u16 = 0;

        let max_registrations_per_block: u16 = 100;

        add_network(netuid, tempo, modality);

        /* @TODO: use a loop maybe */
        register_ok_neuron(netuid, U256::from(1), U256::from(1), 0);
        register_ok_neuron(netuid, U256::from(3), U256::from(3), 65555);
        register_ok_neuron(netuid, U256::from(5), U256::from(5), 75555);
        let max_allowed: u16 = SubtensorModule::get_subnetwork_n(netuid);

        SubtensorModule::set_max_allowed_uids(netuid, max_allowed);
        SubtensorModule::set_max_registrations_per_block(netuid, max_registrations_per_block);

        let uids: Vec<u16> = Vec::from_iter(0..max_allowed);

        let expected = true;
        let result = SubtensorModule::check_len_uids_within_allowed(netuid, &uids);
        assert_eq!(
            expected, result,
            "netuid network length and uids length incompatible"
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_check_len_uids_within_allowed_not_within_network_pool --exact --show-output --nocapture
#[test]
fn test_check_len_uids_within_allowed_not_within_network_pool() {
    new_test_ext(0).execute_with(|| {
        let netuid = NetUid::from(1);

        let tempo: u16 = 13;
        let modality: u16 = 0;

        let max_registrations_per_block: u16 = 100;

        add_network(netuid, tempo, modality);

        /* @TODO: use a loop maybe */
        register_ok_neuron(netuid, U256::from(1), U256::from(1), 0);
        register_ok_neuron(netuid, U256::from(3), U256::from(3), 65555);
        register_ok_neuron(netuid, U256::from(5), U256::from(5), 75555);
        let max_allowed: u16 = SubtensorModule::get_subnetwork_n(netuid);

        SubtensorModule::set_max_allowed_uids(netuid, max_allowed);
        SubtensorModule::set_max_registrations_per_block(netuid, max_registrations_per_block);

        let uids: Vec<u16> = Vec::from_iter(0..(max_allowed + 1));

        let expected = false;
        let result = SubtensorModule::check_len_uids_within_allowed(netuid, &uids);
        assert_eq!(
            expected, result,
            "Failed to detect incompatible uids for network"
        );
    });
}

// `get_first_block_of_epoch` is a legacy modulo helper — NOT used by live
// commit-reveal logic
#[test]
fn test_get_first_block_of_epoch_epoch_zero() {
    new_test_ext(1).execute_with(|| {
        let netuid: NetUid = NetUid::from(1);
        add_network(netuid, 10, 0);

        // 0 * 11 - 2, saturating at 0.
        assert_eq!(SubtensorModule::get_first_block_of_epoch(netuid, 0), 0);
    });
}

#[test]
fn test_get_first_block_of_epoch_small_epoch() {
    new_test_ext(1).execute_with(|| {
        let netuid: NetUid = NetUid::from(0);
        add_network(netuid, 1, 0);

        // 1 * 2 - 1 = 1.
        assert_eq!(SubtensorModule::get_first_block_of_epoch(netuid, 1), 1);
    });
}

#[test]
fn test_get_first_block_of_epoch_with_offset() {
    new_test_ext(1).execute_with(|| {
        let netuid: NetUid = NetUid::from(1);
        add_network(netuid, 10, 0);

        // 1 * 11 - 2 = 9.
        assert_eq!(SubtensorModule::get_first_block_of_epoch(netuid, 1), 9);
    });
}

#[test]
fn test_get_first_block_of_epoch_large_epoch() {
    new_test_ext(1).execute_with(|| {
        let netuid: NetUid = NetUid::from(0);
        add_network(netuid, 100, 0);

        let epoch: u64 = 1000;
        // 1000 * 101 - 1.
        assert_eq!(
            SubtensorModule::get_first_block_of_epoch(netuid, epoch),
            epoch * 101 - 1
        );
    });
}
