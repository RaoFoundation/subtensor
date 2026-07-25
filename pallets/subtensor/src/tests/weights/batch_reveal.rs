#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
//! `batch_reveal_weights` and related commit rate-limit coverage.

use codec::Compact;
use frame_support::{assert_err, assert_ok};
use scale_info::prelude::collections::HashMap;
use sp_core::{H256, U256};
use sp_runtime::traits::{BlakeTwo256, Hash};
use subtensor_runtime_common::NetUidStorageIndex;

use crate::tests::mock::*;
use crate::*;

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_successful_batch_reveal --exact --show-output --nocapture
#[test]
fn test_successful_batch_reveal() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey = U256::from(1);
        let version_keys: Vec<u64> = vec![0, 0, 0];
        let uids_list: Vec<Vec<u16>> = vec![vec![0, 1], vec![1, 0], vec![0, 1]];
        let weight_values_list: Vec<Vec<u16>> = vec![vec![10, 20], vec![30, 40], vec![50, 60]];
        let tempo: u16 = 100;

        System::set_block_number(0);
        add_network(netuid, tempo, 0);

        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);

        register_ok_neuron(netuid, U256::from(3), U256::from(4), 300_000);
        register_ok_neuron(netuid, hotkey, U256::from(2), 100_000);
        SubtensorModule::set_stake_threshold(0);
        SubtensorModule::set_validator_permit_for_uid(netuid, 0, true);
        SubtensorModule::set_validator_permit_for_uid(netuid, 1, true);
        add_balance_to_coldkey_account(&U256::from(0), 1.into());
        add_balance_to_coldkey_account(&U256::from(1), 1.into());
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &(U256::from(0)),
            &(U256::from(0)),
            netuid,
            1.into(),
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &(U256::from(1)),
            &(U256::from(1)),
            netuid,
            1.into(),
        );

        // 1. Commit multiple times
        let mut commit_info = Vec::new();
        for i in 0..3 {
            let salt: Vec<u16> = vec![i as u16; 8];
            let commit_hash: H256 = BlakeTwo256::hash_of(&(
                hotkey,
                netuid,
                uids_list[i].clone(),
                weight_values_list[i].clone(),
                salt.clone(),
                version_keys[i],
            ));
            commit_info.push((commit_hash, salt));
            assert_ok!(SubtensorModule::commit_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                commit_hash
            ));
        }

        step_epochs(1, netuid);

        // 2. Prepare batch reveal parameters
        let salts_list: Vec<Vec<u16>> = commit_info.iter().map(|(_, salt)| salt.clone()).collect();

        // 3. Perform batch reveal
        assert_ok!(SubtensorModule::do_batch_reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids_list.clone(),
            weight_values_list.clone(),
            salts_list.clone(),
            version_keys.clone(),
        ));

        // 4. Ensure all commits are removed
        let commits = crate::WeightCommits::<Test>::get(NetUidStorageIndex::from(netuid), hotkey);
        assert!(commits.is_none());
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_batch_reveal_with_expired_commits --exact --show-output --nocapture
#[test]
fn test_batch_reveal_with_expired_commits() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey = U256::from(1);
        let version_keys: Vec<u64> = vec![0, 0, 0];
        let uids_list: Vec<Vec<u16>> = vec![vec![0, 1], vec![1, 0], vec![0, 1]];
        let weight_values_list: Vec<Vec<u16>> = vec![vec![10, 20], vec![30, 40], vec![50, 60]];
        let tempo: u16 = 100;

        System::set_block_number(0);
        add_network(netuid, tempo, 0);

        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);

        register_ok_neuron(netuid, U256::from(3), U256::from(4), 300_000);
        register_ok_neuron(netuid, hotkey, U256::from(2), 100_000);
        SubtensorModule::set_stake_threshold(0);
        SubtensorModule::set_validator_permit_for_uid(netuid, 0, true);
        SubtensorModule::set_validator_permit_for_uid(netuid, 1, true);
        add_balance_to_coldkey_account(&U256::from(0), 1.into());
        add_balance_to_coldkey_account(&U256::from(1), 1.into());
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &(U256::from(0)),
            &(U256::from(0)),
            netuid,
            1.into(),
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &(U256::from(1)),
            &(U256::from(1)),
            netuid,
            1.into(),
        );

        let mut commit_info = Vec::new();

        // 1. Commit the first weight in epoch 0
        let salt0: Vec<u16> = vec![0u16; 8];
        let commit_hash0: H256 = BlakeTwo256::hash_of(&(
            hotkey,
            netuid,
            uids_list[0].clone(),
            weight_values_list[0].clone(),
            salt0.clone(),
            version_keys[0],
        ));
        commit_info.push((commit_hash0, salt0));
        assert_ok!(SubtensorModule::commit_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            commit_hash0
        ));

        // Advance to epoch 1
        step_epochs(1, netuid);

        // 2. Commit the next two weights in epoch 1
        for i in 1..3 {
            let salt: Vec<u16> = vec![i as u16; 8];
            let commit_hash: H256 = BlakeTwo256::hash_of(&(
                hotkey,
                netuid,
                uids_list[i].clone(),
                weight_values_list[i].clone(),
                salt.clone(),
                version_keys[i],
            ));
            commit_info.push((commit_hash, salt));
            assert_ok!(SubtensorModule::commit_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                commit_hash
            ));
        }

        // Advance to epoch 2 (after reveal period for first commit)
        step_epochs(1, netuid);

        // 3. Prepare batch reveal parameters
        let salts_list: Vec<Vec<u16>> = commit_info.iter().map(|(_, salt)| salt.clone()).collect();

        // 4. Perform batch reveal
        let result = SubtensorModule::do_batch_reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids_list.clone(),
            weight_values_list.clone(),
            salts_list.clone(),
            version_keys.clone(),
        );
        assert_err!(result, Error::<Test>::ExpiredWeightCommit);

        // 5. Expired commit is not removed until a successful call
        let commits = crate::WeightCommits::<Test>::get(NetUidStorageIndex::from(netuid), hotkey)
            .expect("Expected remaining commits");
        assert_eq!(commits.len(), 3);

        // 6. Try revealing the remaining commits
        let valid_uids_list = uids_list[1..].to_vec();
        let valid_weight_values_list = weight_values_list[1..].to_vec();
        let valid_salts_list = salts_list[1..].to_vec();
        let valid_version_keys = version_keys[1..].to_vec();

        assert_ok!(SubtensorModule::do_batch_reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            valid_uids_list,
            valid_weight_values_list,
            valid_salts_list,
            valid_version_keys,
        ));

        // 7. Ensure all commits are removed
        let commits = crate::WeightCommits::<Test>::get(NetUidStorageIndex::from(netuid), hotkey);
        assert!(commits.is_none());
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_batch_reveal_with_invalid_input_lengths --exact --show-output --nocapture
#[test]
fn test_batch_reveal_with_invalid_input_lengths() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey = U256::from(1);
        let tempo: u16 = 100;

        System::set_block_number(0);
        add_network(netuid, tempo, 0);

        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);

        // Base data for valid inputs
        let uids_list: Vec<Vec<u16>> = vec![vec![0, 1], vec![1, 0]];
        let weight_values_list: Vec<Vec<u16>> = vec![vec![10, 20], vec![30, 40]];
        let salts_list: Vec<Vec<u16>> = vec![vec![0u16; 8], vec![1u16; 8]];
        let version_keys: Vec<u64> = vec![0, 0];

        // Test cases with mismatched input lengths

        // Case 1: uids_list has an extra element
        let uids_list_case = vec![vec![0, 1], vec![1, 0], vec![2, 3]];
        let result = SubtensorModule::do_batch_reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids_list_case.clone(),
            weight_values_list.clone(),
            salts_list.clone(),
            version_keys.clone(),
        );
        assert_err!(result, Error::<Test>::InputLengthsUnequal);

        // Case 2: weight_values_list has an extra element
        let weight_values_list_case = vec![vec![10, 20], vec![30, 40], vec![50, 60]];
        let result = SubtensorModule::do_batch_reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids_list.clone(),
            weight_values_list_case.clone(),
            salts_list.clone(),
            version_keys.clone(),
        );
        assert_err!(result, Error::<Test>::InputLengthsUnequal);

        // Case 3: salts_list has an extra element
        let salts_list_case = vec![vec![0u16; 8], vec![1u16; 8], vec![2u16; 8]];
        let result = SubtensorModule::do_batch_reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids_list.clone(),
            weight_values_list.clone(),
            salts_list_case.clone(),
            version_keys.clone(),
        );
        assert_err!(result, Error::<Test>::InputLengthsUnequal);

        // Case 4: version_keys has an extra element
        let version_keys_case = vec![0, 0, 0];
        let result = SubtensorModule::do_batch_reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids_list.clone(),
            weight_values_list.clone(),
            salts_list.clone(),
            version_keys_case.clone(),
        );
        assert_err!(result, Error::<Test>::InputLengthsUnequal);

        // Case 5: All input vectors have mismatched lengths
        let uids_list_case = vec![vec![0, 1]];
        let weight_values_list_case = vec![vec![10, 20], vec![30, 40]];
        let salts_list_case = vec![vec![0u16; 8]];
        let version_keys_case = vec![0, 0, 0];
        let result = SubtensorModule::do_batch_reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids_list_case,
            weight_values_list_case,
            salts_list_case,
            version_keys_case,
        );
        assert_err!(result, Error::<Test>::InputLengthsUnequal);

        // Case 6: Valid input lengths (should not return an error)
        let result = SubtensorModule::do_batch_reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids_list.clone(),
            weight_values_list.clone(),
            salts_list.clone(),
            version_keys.clone(),
        );
        // We expect an error because no commits have been made, but it should not be InputLengthsUnequal
        assert_err!(result, Error::<Test>::NoWeightsCommitFound);
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_batch_reveal_with_no_commits --exact --show-output --nocapture
#[test]
fn test_batch_reveal_with_no_commits() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey = U256::from(1);
        let version_keys: Vec<u64> = vec![0];
        let uids_list: Vec<Vec<u16>> = vec![vec![0, 1]];
        let weight_values_list: Vec<Vec<u16>> = vec![vec![10, 20]];
        let salts_list: Vec<Vec<u16>> = vec![vec![0u16; 8]];
        let tempo: u16 = 100;

        System::set_block_number(0);
        add_network(netuid, tempo, 0);

        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);

        // 1. Attempt to perform batch reveal without any commits
        let result = SubtensorModule::do_batch_reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids_list,
            weight_values_list,
            salts_list,
            version_keys,
        );
        assert_err!(result, Error::<Test>::NoWeightsCommitFound);
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_batch_reveal_before_reveal_period --exact --show-output --nocapture
#[test]
fn test_batch_reveal_before_reveal_period() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey = U256::from(1);
        let version_keys: Vec<u64> = vec![0, 0];
        let uids_list: Vec<Vec<u16>> = vec![vec![0, 1], vec![1, 0]];
        let weight_values_list: Vec<Vec<u16>> = vec![vec![10, 20], vec![30, 40]];
        let tempo: u16 = 100;

        System::set_block_number(0);
        add_network(netuid, tempo, 0);

        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);

        register_ok_neuron(netuid, U256::from(3), U256::from(4), 300_000);
        register_ok_neuron(netuid, hotkey, U256::from(2), 100_000);
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);
        SubtensorModule::set_validator_permit_for_uid(netuid, 0, true);
        SubtensorModule::set_validator_permit_for_uid(netuid, 1, true);

        // 1. Commit multiple times in the same epoch
        let mut commit_info = Vec::new();
        for i in 0..2 {
            let salt: Vec<u16> = vec![i as u16; 8];
            let commit_hash: H256 = BlakeTwo256::hash_of(&(
                hotkey,
                netuid,
                uids_list[i].clone(),
                weight_values_list[i].clone(),
                salt.clone(),
                version_keys[i],
            ));
            commit_info.push((commit_hash, salt));
            assert_ok!(SubtensorModule::commit_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                commit_hash
            ));
        }

        // 2. Prepare batch reveal parameters
        let salts_list: Vec<Vec<u16>> = commit_info.iter().map(|(_, salt)| salt.clone()).collect();

        // 3. Attempt to reveal before reveal period
        let result = SubtensorModule::do_batch_reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids_list.clone(),
            weight_values_list.clone(),
            salts_list.clone(),
            version_keys.clone(),
        );
        assert_err!(result, Error::<Test>::RevealTooEarly);
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_batch_reveal_after_commits_expired --exact --show-output --nocapture
#[test]
fn test_batch_reveal_after_commits_expired() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey = U256::from(1);
        let version_keys: Vec<u64> = vec![0, 0];
        let uids_list: Vec<Vec<u16>> = vec![vec![0, 1], vec![1, 0]];
        let weight_values_list: Vec<Vec<u16>> = vec![vec![10, 20], vec![30, 40]];
        let tempo: u16 = 100;

        System::set_block_number(0);
        add_network(netuid, tempo, 0);

        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);

        register_ok_neuron(netuid, U256::from(3), U256::from(4), 300_000);
        register_ok_neuron(netuid, hotkey, U256::from(2), 100_000);
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);
        SubtensorModule::set_validator_permit_for_uid(netuid, 0, true);
        SubtensorModule::set_validator_permit_for_uid(netuid, 1, true);

        let mut commit_info = Vec::new();

        // 1. Commit the first weight in epoch 0
        let salt0: Vec<u16> = vec![0u16; 8];
        let commit_hash0: H256 = BlakeTwo256::hash_of(&(
            hotkey,
            netuid,
            uids_list[0].clone(),
            weight_values_list[0].clone(),
            salt0.clone(),
            version_keys[0],
        ));
        commit_info.push((commit_hash0, salt0));
        assert_ok!(SubtensorModule::commit_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            commit_hash0
        ));

        // Advance to epoch 1
        step_epochs(1, netuid);

        // 2. Commit the second weight in epoch 1
        let salt1: Vec<u16> = vec![1u16; 8];
        let commit_hash1: H256 = BlakeTwo256::hash_of(&(
            hotkey,
            netuid,
            uids_list[1].clone(),
            weight_values_list[1].clone(),
            salt1.clone(),
            version_keys[1],
        ));
        commit_info.push((commit_hash1, salt1));
        assert_ok!(SubtensorModule::commit_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            commit_hash1
        ));

        // Advance to epoch 4 to ensure both commits have expired (assuming reveal_period is 1)
        step_epochs(3, netuid);

        // 3. Prepare batch reveal parameters
        let salts_list: Vec<Vec<u16>> = commit_info.iter().map(|(_, salt)| salt.clone()).collect();

        // 4. Attempt to reveal after commits have expired
        let result = SubtensorModule::do_batch_reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids_list.clone(),
            weight_values_list.clone(),
            salts_list,
            version_keys.clone(),
        );
        assert_err!(result, Error::<Test>::ExpiredWeightCommit);
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_batch_reveal_when_commit_reveal_disabled --exact --show-output --nocapture
#[test]
fn test_batch_reveal_when_commit_reveal_disabled() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey = U256::from(1);
        let version_keys: Vec<u64> = vec![0];
        let uids_list: Vec<Vec<u16>> = vec![vec![0, 1]];
        let weight_values_list: Vec<Vec<u16>> = vec![vec![10, 20]];
        let salts_list: Vec<Vec<u16>> = vec![vec![0u16; 8]];
        let tempo: u16 = 100;

        System::set_block_number(0);
        add_network(netuid, tempo, 0);

        SubtensorModule::set_commit_reveal_weights_enabled(netuid, false);

        // 1. Attempt to perform batch reveal when commit-reveal is disabled
        let result = SubtensorModule::do_batch_reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids_list,
            weight_values_list,
            salts_list,
            version_keys,
        );
        assert_err!(result, Error::<Test>::CommitRevealDisabled);
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_batch_reveal_with_out_of_order_commits --exact --show-output --nocapture
#[test]
fn test_batch_reveal_with_out_of_order_commits() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey = U256::from(1);
        let version_keys: Vec<u64> = vec![0, 0, 0];
        let uids_list: Vec<Vec<u16>> = vec![vec![0, 1], vec![1, 0], vec![0, 1]];
        let weight_values_list: Vec<Vec<u16>> = vec![vec![10, 20], vec![30, 40], vec![50, 60]];
        let tempo: u16 = 100;

        System::set_block_number(0);
        add_network(netuid, tempo, 0);

        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);

        register_ok_neuron(netuid, U256::from(3), U256::from(4), 300_000);
        register_ok_neuron(netuid, hotkey, U256::from(2), 100_000);
        SubtensorModule::set_stake_threshold(0);
        SubtensorModule::set_validator_permit_for_uid(netuid, 0, true);
        SubtensorModule::set_validator_permit_for_uid(netuid, 1, true);
        add_balance_to_coldkey_account(&U256::from(0), 1.into());
        add_balance_to_coldkey_account(&U256::from(1), 1.into());
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &(U256::from(0)),
            &(U256::from(0)),
            netuid,
            1.into(),
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &(U256::from(1)),
            &(U256::from(1)),
            netuid,
            1.into(),
        );

        // 1. Commit multiple times (A, B, C)
        let mut commit_info = Vec::new();
        for i in 0..3 {
            let salt: Vec<u16> = vec![i as u16; 8];
            let commit_hash: H256 = BlakeTwo256::hash_of(&(
                hotkey,
                netuid,
                uids_list[i].clone(),
                weight_values_list[i].clone(),
                salt.clone(),
                version_keys[i],
            ));
            commit_info.push((commit_hash, salt));
            assert_ok!(SubtensorModule::commit_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                commit_hash
            ));
        }

        step_epochs(1, netuid);

        // 2. Prepare batch reveal parameters for commits A and C (out of order)
        let salts_list: Vec<Vec<u16>> = vec![
            commit_info[2].1.clone(), // Third commit (C)
            commit_info[0].1.clone(), // First commit (A)
        ];
        let uids_list_out_of_order = vec![
            uids_list[2].clone(), // C
            uids_list[0].clone(), // A
        ];
        let weight_values_list_out_of_order = vec![
            weight_values_list[2].clone(), // C
            weight_values_list[0].clone(), // A
        ];
        let version_keys_out_of_order = vec![
            version_keys[2], // C
            version_keys[0], // A
        ];

        // 3. Attempt batch reveal of A and C out of order
        let result = SubtensorModule::do_batch_reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids_list_out_of_order,
            weight_values_list_out_of_order,
            salts_list,
            version_keys_out_of_order,
        );

        // 4. Ensure the batch reveal succeeds
        assert_ok!(result);

        // 5. Prepare and reveal the remaining commit (B)
        let remaining_salt = commit_info[1].1.clone();
        let remaining_uids = uids_list[1].clone();
        let remaining_weights = weight_values_list[1].clone();
        let remaining_version_key = version_keys[1];

        assert_ok!(SubtensorModule::do_batch_reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            vec![remaining_uids],
            vec![remaining_weights],
            vec![remaining_salt],
            vec![remaining_version_key],
        ));

        // 6. Ensure all commits are removed
        let commits = crate::WeightCommits::<Test>::get(NetUidStorageIndex::from(netuid), hotkey);
        assert!(commits.is_none());
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_highly_concurrent_commits_and_reveals_with_multiple_hotkeys --exact --show-output --nocapture
#[test]
fn test_highly_concurrent_commits_and_reveals_with_multiple_hotkeys() {
    new_test_ext(1).execute_with(|| {
        // ==== Test Configuration ====
        let netuid = NetUid::from(1);
        let num_hotkeys: usize = 10;
        let max_unrevealed_commits: usize = 10;
        let commits_per_hotkey: usize = 20;
        let initial_reveal_period: u64 = 5;
        let initial_tempo: u16 = 100;

        // ==== Setup Network ====
        add_network(netuid, initial_tempo, 0);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);
        assert_ok!(SubtensorModule::set_reveal_period(netuid, initial_reveal_period));
        SubtensorModule::set_max_registrations_per_block(netuid, u16::MAX);
        SubtensorModule::set_target_registrations_per_interval(netuid, u16::MAX);

        // ==== Register Validators ====
        for uid in 0..5 {
            let validator_id = U256::from(100 + uid as u64);
            register_ok_neuron(netuid, validator_id, U256::from(200 + uid as u64), 300_000);
            SubtensorModule::set_validator_permit_for_uid(netuid, uid, true);
        }

        // ==== Register Hotkeys ====
        let mut hotkeys: Vec<<Test as frame_system::Config>::AccountId> = Vec::new();
        for i in 0..num_hotkeys {
            let hotkey_id = U256::from(1000 + i as u64);
            register_ok_neuron(netuid, hotkey_id, U256::from(2000 + i as u64), 100_000);
            hotkeys.push(hotkey_id);
        }

        // ==== Initialize Commit Information ====
        let mut commit_info_map: HashMap<
            <Test as frame_system::Config>::AccountId,
            Vec<(H256, Vec<u16>, Vec<u16>, Vec<u16>, u64)>,
        > = HashMap::new();

        // Initialize the map
        for hotkey in &hotkeys {
            commit_info_map.insert(*hotkey, Vec::new());
        }

        // ==== Function to Generate Unique Data ====
        fn generate_unique_data(index: usize) -> (Vec<u16>, Vec<u16>, Vec<u16>, u64) {
            let uids = vec![index as u16, (index + 1) as u16];
            let values = vec![(index * 10) as u16, ((index + 1) * 10) as u16];
            let salt = vec![(index % 100) as u16; 8];
            let version_key = index as u64;
            (uids, values, salt, version_key)
        }

        // ==== Simulate Concurrent Commits and Reveals ====
        for i in 0..commits_per_hotkey {
            for hotkey in &hotkeys {

                let current_commits = crate::WeightCommits::<Test>::get(NetUidStorageIndex::from(netuid), hotkey)
                    .unwrap_or_default();
                if current_commits.len() >= max_unrevealed_commits {
                    continue;
                }

                let (uids, values, salt, version_key) = generate_unique_data(i);
                let commit_hash: H256 = BlakeTwo256::hash_of(&(
                    *hotkey,
                    netuid,
                    uids.clone(),
                    values.clone(),
                    salt.clone(),
                    version_key,
                ));

                if let Some(commits) = commit_info_map.get_mut(hotkey) {
                    commits.push((commit_hash, salt.clone(), uids.clone(), values.clone(), version_key));
                }

            assert_ok!(SubtensorModule::commit_weights(
                    RuntimeOrigin::signed(*hotkey),
                netuid,
                    commit_hash
                ));
            }

            // ==== Reveal Phase ====
            for hotkey in &hotkeys {
                if let Some(commits) = commit_info_map.get_mut(hotkey) {
                    if commits.is_empty() {
                        continue; // No commits to reveal
                    }

                    let (_commit_hash, salt, uids, values, version_key) = commits.first().expect("expected a value");

                    let reveal_result = SubtensorModule::reveal_weights(
                        RuntimeOrigin::signed(*hotkey),
                        netuid,
                        uids.clone(),
                        values.clone(),
                        salt.clone(),
                        *version_key,
                    );

                    match reveal_result {
                        Ok(_) => {
                            commits.remove(0);
                        }
                        Err(e) => {
                            if e == Error::<Test>::RevealTooEarly.into()
                                || e == Error::<Test>::ExpiredWeightCommit.into()
                                || e == Error::<Test>::InvalidRevealCommitHashNotMatch.into()
                            {
                                log::info!("Expected error during reveal after epoch advancement: {e:?}");
                            } else {
                                panic!(
                                    "Unexpected error during reveal: {e:?}, expected RevealTooEarly, ExpiredWeightCommit, or InvalidRevealCommitHashNotMatch"
                                );
                            }
                        }
                    }
                }
            }
        }

        // ==== Modify Network Parameters During Commits ====
        SubtensorModule::set_tempo_unchecked(netuid, 150);
        assert_ok!(SubtensorModule::set_reveal_period(netuid, 7));
        log::info!("Changed tempo to 150 and reveal_period to 7 during commits.");

        step_epochs(3, netuid);

        // ==== Continue Reveals After Epoch Advancement ====
        for hotkey in &hotkeys {
            if let Some(commits) = commit_info_map.get_mut(hotkey) {
                while !commits.is_empty() {
                    let (_commit_hash, salt, uids, values, version_key) = &commits[0];

                    // Attempt to reveal
                    let reveal_result = SubtensorModule::reveal_weights(
                        RuntimeOrigin::signed(*hotkey),
                        netuid,
                        uids.clone(),
                        values.clone(),
                        salt.clone(),
                        *version_key,
                    );

                    match reveal_result {
                        Ok(_) => {
                            commits.remove(0);
                        }
                        Err(e) => {
                            // Check if the error is due to reveal being too early or commit expired
                            if e == Error::<Test>::RevealTooEarly.into()
                                || e == Error::<Test>::ExpiredWeightCommit.into()
                                || e == Error::<Test>::InvalidRevealCommitHashNotMatch.into()
                            {
                                log::info!("Expected error during reveal after epoch advancement: {e:?}");
                                break;
                            } else {
                                panic!(
                                    "Unexpected error during reveal after epoch advancement: {e:?}, expected RevealTooEarly, ExpiredWeightCommit, or InvalidRevealCommitHashNotMatch"
                                );
                            }
                        }
                    }
                }
            }
        }

        // ==== Change Network Parameters Again ====
        SubtensorModule::set_tempo_unchecked(netuid, 200);
        assert_ok!(SubtensorModule::set_reveal_period(netuid, 10));
        log::info!("Changed tempo to 200 and reveal_period to 10 after initial reveals.");

        step_epochs(10, netuid);

        // ==== Final Reveal Attempts ====
        for (hotkey, commits) in commit_info_map.iter_mut() {
            for (_commit_hash, salt, uids, values, version_key) in commits.iter() {
                let reveal_result = SubtensorModule::reveal_weights(
                    RuntimeOrigin::signed(*hotkey),
            netuid,
                    uids.clone(),
                    values.clone(),
                    salt.clone(),
                    *version_key,
                );

                assert_eq!(
                    reveal_result,
                    Err(Error::<Test>::ExpiredWeightCommit.into()),
                    "Expected ExpiredWeightCommit error, got {reveal_result:?}"
                );
            }
}

        for hotkey in &hotkeys {
            commit_info_map.insert(*hotkey, Vec::new());

            for i in 0..max_unrevealed_commits {
                let (uids, values, salt, version_key) = generate_unique_data(i + commits_per_hotkey);
                let commit_hash: H256 = BlakeTwo256::hash_of(&(
                    *hotkey,
                    netuid,
                    uids.clone(),
                    values.clone(),
                    salt.clone(),
                    version_key,
                ));

                assert_ok!(SubtensorModule::commit_weights(
                    RuntimeOrigin::signed(*hotkey),
                    netuid,
                    commit_hash
                ));
            }

            let (uids, values, salt, version_key) = generate_unique_data(max_unrevealed_commits + commits_per_hotkey);
            let commit_hash: H256 = BlakeTwo256::hash_of(&(
                *hotkey,
                netuid,
                uids.clone(),
                values.clone(),
                salt.clone(),
                version_key,
            ));

            assert_err!(
                SubtensorModule::commit_weights(
                    RuntimeOrigin::signed(*hotkey),
                    netuid,
                    commit_hash
                ),
                Error::<Test>::TooManyUnrevealedCommits
            );
        }

        // Attempt unauthorized reveal
        let unauthorized_hotkey = hotkeys[0];
        let target_hotkey = hotkeys[1];
        if let Some(commits) = commit_info_map.get(&target_hotkey)
            && let Some((_commit_hash, salt, uids, values, version_key)) = commits.first() {
                assert_err!(
                    SubtensorModule::reveal_weights(
                        RuntimeOrigin::signed(unauthorized_hotkey),
                        netuid,
                        uids.clone(),
                        values.clone(),
                        salt.clone(),
                        *version_key,
                    ),
                    Error::<Test>::InvalidRevealCommitHashNotMatch
                );
            }

        let non_committing_hotkey: <Test as frame_system::Config>::AccountId = U256::from(9999);
        assert_err!(
            SubtensorModule::reveal_weights(
                RuntimeOrigin::signed(non_committing_hotkey),
                netuid,
                vec![0, 1],
                vec![10, 20],
                vec![0; 8],
                0,
            ),
            Error::<Test>::NoWeightsCommitFound
        );

        assert_eq!(SubtensorModule::get_reveal_period(netuid), 10);
        assert_eq!(SubtensorModule::get_tempo(netuid), 200);
    })
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_commit_weights_rate_limit --exact --show-output --nocapture
#[test]
fn test_commit_weights_rate_limit() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let uids: Vec<u16> = vec![0, 1];
        let weight_values: Vec<u16> = vec![10, 10];
        let salt: Vec<u16> = vec![1, 2, 3, 4, 5, 6, 7, 8];
        let version_key: u64 = 0;
        let hotkey: U256 = U256::from(1);

        let commit_hash: H256 = BlakeTwo256::hash_of(&(
            hotkey,
            netuid,
            uids.clone(),
            weight_values.clone(),
            salt.clone(),
            version_key,
        ));
        System::set_block_number(11);

        let tempo: u16 = 5;
        add_network(netuid, tempo, 0);

        register_ok_neuron(netuid, U256::from(3), U256::from(4), 300_000);
        register_ok_neuron(netuid, U256::from(1), U256::from(2), 100_000);
        SubtensorModule::set_stake_threshold(0);
        SubtensorModule::set_weights_set_rate_limit(netuid, 10); // Rate limit is 10 blocks
        SubtensorModule::set_validator_permit_for_uid(netuid, 0, true);
        SubtensorModule::set_validator_permit_for_uid(netuid, 1, true);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        add_balance_to_coldkey_account(&U256::from(0), 1.into());
        add_balance_to_coldkey_account(&U256::from(1), 1.into());
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &(U256::from(0)),
            &(U256::from(0)),
            netuid,
            1.into(),
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &(U256::from(1)),
            &(U256::from(1)),
            netuid,
            1.into(),
        );

        let neuron_uid =
            SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey).expect("expected uid");
        SubtensorModule::set_last_update_for_uid(NetUidStorageIndex::from(netuid), neuron_uid, 0);

        assert_ok!(SubtensorModule::commit_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            commit_hash
        ));

        let new_salt: Vec<u16> = vec![9; 8];
        let new_commit_hash: H256 = BlakeTwo256::hash_of(&(
            hotkey,
            netuid,
            uids.clone(),
            weight_values.clone(),
            new_salt.clone(),
            version_key,
        ));
        assert_err!(
            SubtensorModule::commit_weights(RuntimeOrigin::signed(hotkey), netuid, new_commit_hash),
            Error::<Test>::CommittingWeightsTooFast
        );

        step_block(5);
        assert_err!(
            SubtensorModule::commit_weights(RuntimeOrigin::signed(hotkey), netuid, new_commit_hash),
            Error::<Test>::CommittingWeightsTooFast
        );

        step_block(5); // Current block is now 21

        assert_ok!(SubtensorModule::commit_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            new_commit_hash
        ));

        SubtensorModule::set_commit_reveal_weights_enabled(netuid, false);
        let weights_keys: Vec<u16> = vec![0];
        let weight_values: Vec<u16> = vec![1];

        assert_err!(
            SubtensorModule::set_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                weights_keys.clone(),
                weight_values.clone(),
                0
            ),
            Error::<Test>::SettingWeightsTooFast
        );

        step_block(10);

        assert_ok!(SubtensorModule::set_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            weights_keys.clone(),
            weight_values.clone(),
            0
        ));

        assert_err!(
            SubtensorModule::set_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                weights_keys.clone(),
                weight_values.clone(),
                0
            ),
            Error::<Test>::SettingWeightsTooFast
        );

        step_block(5);

        assert_err!(
            SubtensorModule::set_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                weights_keys.clone(),
                weight_values.clone(),
                0
            ),
            Error::<Test>::SettingWeightsTooFast
        );

        step_block(5);

        assert_ok!(SubtensorModule::set_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            weights_keys.clone(),
            weight_values.clone(),
            0
        ));
    });
}

#[test]
fn test_batch_commit_weights_item_failure_event_includes_netuid() {
    new_test_ext(1).execute_with(|| {
        let netuid_a = NetUid::from(1);
        let netuid_b = NetUid::from(2);
        add_network(netuid_a, 1, 0);
        add_network(netuid_b, 1, 0);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid_a, false);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid_b, false);

        let hotkey = U256::from(1);
        let netuids: Vec<Compact<NetUid>> = vec![netuid_a.into(), netuid_b.into()];
        let hashes: Vec<H256> = vec![H256::repeat_byte(0xAA), H256::repeat_byte(0xBB)];

        assert_ok!(SubtensorModule::do_batch_commit_weights(
            RuntimeOrigin::signed(hotkey),
            netuids,
            hashes,
        ));

        let failures: Vec<NetUid> = System::events()
            .iter()
            .filter_map(|e| match &e.event {
                RuntimeEvent::SubtensorModule(Event::BatchWeightItemFailed(netuid, _err)) => {
                    Some(*netuid)
                }
                _ => None,
            })
            .collect();

        assert_eq!(
            failures,
            vec![netuid_a, netuid_b],
            "BatchWeightItemFailed events should carry each failing netuid in batch order"
        );
    });
}

// Regression: same shape as the commit-path test, but for the set-path
// (`do_batch_set_weights`). Each failing item must emit a
// BatchWeightItemFailed carrying its netuid.
#[test]
fn test_batch_set_weights_item_failure_event_includes_netuid() {
    new_test_ext(1).execute_with(|| {
        let netuid_a = NetUid::from(3);
        let netuid_b = NetUid::from(4);
        add_network(netuid_a, 1, 0);
        add_network(netuid_b, 1, 0);
        // do_set_weights fails iff commit-reveal is ENABLED on the netuid.
        SubtensorModule::set_commit_reveal_weights_enabled(netuid_a, true);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid_b, true);

        let hotkey = U256::from(11);
        let netuids: Vec<Compact<NetUid>> = vec![netuid_a.into(), netuid_b.into()];
        let weights: Vec<Vec<(Compact<u16>, Compact<u16>)>> = vec![vec![], vec![]];
        let version_keys: Vec<Compact<u64>> = vec![0u64.into(), 0u64.into()];

        assert_ok!(SubtensorModule::do_batch_set_weights(
            RuntimeOrigin::signed(hotkey),
            netuids,
            weights,
            version_keys,
        ));

        let failures: Vec<NetUid> = System::events()
            .iter()
            .filter_map(|e| match &e.event {
                RuntimeEvent::SubtensorModule(Event::BatchWeightItemFailed(netuid, _err)) => {
                    Some(*netuid)
                }
                _ => None,
            })
            .collect();

        assert_eq!(
            failures,
            vec![netuid_a, netuid_b],
            "BatchWeightItemFailed events should carry each failing netuid in batch order"
        );
    });
}
