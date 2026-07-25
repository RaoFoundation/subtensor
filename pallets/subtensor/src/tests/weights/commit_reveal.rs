#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
//! Hash-based commit–reveal weights (`commit_weights` / `reveal_weights`).

use frame_support::{assert_err, assert_ok};
use sp_core::{H256, U256};
use sp_runtime::traits::{BlakeTwo256, Hash};
use subtensor_runtime_common::NetUidStorageIndex;

use crate::tests::mock::*;
use crate::*;

#[test]
fn test_set_weights_commit_reveal_enabled_error() {
    new_test_ext(0).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        register_ok_neuron(netuid, U256::from(1), U256::from(2), 10);

        let uids = vec![0];
        let weights = vec![1];
        let version_key: u64 = 0;
        let hotkey = U256::from(1);

        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);

        assert_err!(
            SubtensorModule::set_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                uids.clone(),
                weights.clone(),
                version_key
            ),
            Error::<Test>::CommitRevealEnabled
        );

        SubtensorModule::set_commit_reveal_weights_enabled(netuid, false);

        assert_ok!(SubtensorModule::set_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids,
            weights,
            version_key
        ));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_reveal_weights_when_commit_reveal_disabled --exact --show-output --nocapture
#[test]
fn test_reveal_weights_when_commit_reveal_disabled() {
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

        System::set_block_number(0);

        let tempo: u16 = 5;
        add_network(netuid, tempo, 0);

        // Register neurons and set up configurations
        register_ok_neuron(netuid, U256::from(3), U256::from(4), 300_000);
        register_ok_neuron(netuid, U256::from(1), U256::from(2), 100_000);
        SubtensorModule::set_weights_set_rate_limit(netuid, 5);
        SubtensorModule::set_validator_permit_for_uid(netuid, 0, true);
        SubtensorModule::set_validator_permit_for_uid(netuid, 1, true);

        // Enable commit-reveal and commit
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        assert_ok!(SubtensorModule::commit_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            commit_hash
        ));

        step_epochs(1, netuid);

        // Disable commit-reveal before reveal
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, false);

        // Attempt to reveal, should fail with CommitRevealDisabled
        assert_err!(
            SubtensorModule::reveal_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                uids,
                weight_values,
                salt,
                version_key,
            ),
            Error::<Test>::CommitRevealDisabled
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_commit_reveal_weights_ok --exact --show-output --nocapture
#[test]
fn test_commit_reveal_weights_ok() {
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

        System::set_block_number(0);

        let tempo: u16 = 5;
        add_network(netuid, tempo, 0);

        // Register neurons and set up configurations
        register_ok_neuron(netuid, U256::from(3), U256::from(4), 300_000);
        register_ok_neuron(netuid, U256::from(1), U256::from(2), 100_000);
        SubtensorModule::set_stake_threshold(0);
        SubtensorModule::set_weights_set_rate_limit(netuid, 5);
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

        // Commit at block 0
        assert_ok!(SubtensorModule::commit_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            commit_hash
        ));

        step_epochs(1, netuid);

        // Reveal in the next epoch
        assert_ok!(SubtensorModule::reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids,
            weight_values,
            salt,
            version_key,
        ));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_commit_reveal_tempo_interval --exact --show-output --nocapture
#[test]
fn test_commit_reveal_tempo_interval() {
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

        System::set_block_number(0);

        let tempo: u16 = 100;
        add_network(netuid, tempo, 0);

        register_ok_neuron(netuid, U256::from(3), U256::from(4), 300_000);
        register_ok_neuron(netuid, U256::from(1), U256::from(2), 100_000);
        SubtensorModule::set_stake_threshold(0);
        SubtensorModule::set_weights_set_rate_limit(netuid, 5);
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

        // Commit at block 0
        assert_ok!(SubtensorModule::commit_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            commit_hash
        ));

        // Attempt to reveal in the same epoch, should fail
        assert_err!(
            SubtensorModule::reveal_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                uids.clone(),
                weight_values.clone(),
                salt.clone(),
                version_key,
            ),
            Error::<Test>::RevealTooEarly
        );

        step_epochs(1, netuid);

        assert_ok!(SubtensorModule::reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids.clone(),
            weight_values.clone(),
            salt.clone(),
            version_key,
        ));

        step_block(6);

        assert_err!(
            SubtensorModule::reveal_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                uids.clone(),
                weight_values.clone(),
                salt.clone(),
                version_key,
            ),
            Error::<Test>::NoWeightsCommitFound
        );

        assert_ok!(SubtensorModule::commit_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            commit_hash
        ));

        // step two epochs
        step_epochs(2, netuid);

        assert_err!(
            SubtensorModule::reveal_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                uids.clone(),
                weight_values.clone(),
                salt.clone(),
                version_key,
            ),
            Error::<Test>::ExpiredWeightCommit
        );

        assert_ok!(SubtensorModule::commit_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            commit_hash
        ));

        step_block(50);

        assert_err!(
            SubtensorModule::reveal_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                uids.clone(),
                weight_values.clone(),
                salt.clone(),
                version_key,
            ),
            Error::<Test>::RevealTooEarly
        );

        step_epochs(1, netuid);

        assert_ok!(SubtensorModule::reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids,
            weight_values,
            salt,
            version_key,
        ));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_commit_reveal_hash --exact --show-output --nocapture
#[test]
fn test_commit_reveal_hash() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let uids: Vec<u16> = vec![0, 1];
        let weight_values: Vec<u16> = vec![10, 10];
        let salt: Vec<u16> = vec![1, 2, 3, 4, 5, 6, 7, 8];
        let bad_salt: Vec<u16> = vec![0, 2, 3, 4, 5, 6, 7, 8];
        let version_key: u64 = 0;
        let hotkey: U256 = U256::from(1);

        add_network(netuid, 5, 0);
        System::set_block_number(0);

        register_ok_neuron(netuid, U256::from(3), U256::from(4), 300_000);
        register_ok_neuron(netuid, U256::from(1), U256::from(2), 100_000);
        SubtensorModule::set_stake_threshold(0);
        SubtensorModule::set_weights_set_rate_limit(netuid, 5);
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

        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);

        let commit_hash: H256 = BlakeTwo256::hash_of(&(
            hotkey,
            netuid,
            uids.clone(),
            weight_values.clone(),
            salt.clone(),
            version_key,
        ));

        assert_ok!(SubtensorModule::commit_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            commit_hash
        ));

        step_epochs(1, netuid);

        // Attempt to reveal with incorrect data, should fail
        assert_err!(
            SubtensorModule::reveal_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                vec![0, 2],
                weight_values.clone(),
                salt.clone(),
                version_key
            ),
            Error::<Test>::InvalidRevealCommitHashNotMatch
        );

        assert_err!(
            SubtensorModule::reveal_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                uids.clone(),
                weight_values.clone(),
                bad_salt.clone(),
                version_key,
            ),
            Error::<Test>::InvalidRevealCommitHashNotMatch
        );

        // Correct reveal, should succeed
        assert_ok!(SubtensorModule::reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids,
            weight_values,
            salt,
            version_key,
        ));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_commit_reveal_disabled_or_enabled --exact --show-output --nocapture
#[test]
fn test_commit_reveal_disabled_or_enabled() {
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

        add_network(netuid, 5, 0);
        System::set_block_number(0);

        register_ok_neuron(netuid, U256::from(3), U256::from(4), 300_000);
        register_ok_neuron(netuid, U256::from(1), U256::from(2), 100_000);
        SubtensorModule::set_stake_threshold(0);
        SubtensorModule::set_weights_set_rate_limit(netuid, 5);
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

        // Disable commit/reveal
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, false);

        // Attempt to commit, should fail
        assert_err!(
            SubtensorModule::commit_weights(RuntimeOrigin::signed(hotkey), netuid, commit_hash),
            Error::<Test>::CommitRevealDisabled
        );

        // Enable commit/reveal
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);

        // Commit should now succeed
        assert_ok!(SubtensorModule::commit_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            commit_hash
        ));

        step_epochs(1, netuid);

        // Reveal should succeed
        assert_ok!(SubtensorModule::reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids,
            weight_values,
            salt,
            version_key,
        ));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_toggle_commit_reveal_weights_and_set_weights --exact --show-output --nocapture
#[test]
fn test_toggle_commit_reveal_weights_and_set_weights() {
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

        add_network(netuid, 5, 0);
        System::set_block_number(0);

        register_ok_neuron(netuid, U256::from(3), U256::from(4), 300_000);
        register_ok_neuron(netuid, U256::from(1), U256::from(2), 100_000);
        SubtensorModule::set_stake_threshold(0);
        SubtensorModule::set_validator_permit_for_uid(netuid, 0, true);
        SubtensorModule::set_validator_permit_for_uid(netuid, 1, true);
        SubtensorModule::set_weights_set_rate_limit(netuid, 5);
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

        // Enable commit/reveal
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);

        // Commit at block 0
        assert_ok!(SubtensorModule::commit_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            commit_hash
        ));

        step_epochs(1, netuid);

        // Reveal in the next epoch
        assert_ok!(SubtensorModule::reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids.clone(),
            weight_values.clone(),
            salt.clone(),
            version_key,
        ));

        // Disable commit/reveal
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, false);

        // Advance to allow setting weights (due to rate limit)
        step_block(5);

        // Set weights directly
        assert_ok!(SubtensorModule::set_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids,
            weight_values,
            version_key,
        ));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_tempo_change_during_commit_reveal_process --exact --show-output --nocapture
#[test]
fn test_tempo_change_during_commit_reveal_process() {
    new_test_ext(0).execute_with(|| {
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

        System::set_block_number(0);

        let tempo: u16 = 100;
        add_network(netuid, tempo, 0);

        register_ok_neuron(netuid, U256::from(3), U256::from(4), 300_000);
        register_ok_neuron(netuid, U256::from(1), U256::from(2), 100_000);
        SubtensorModule::set_stake_threshold(0);
        SubtensorModule::set_weights_set_rate_limit(netuid, 5);
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

        assert_ok!(SubtensorModule::commit_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            commit_hash
        ));
        log::info!(
            "Commit successful at block {}",
            SubtensorModule::get_current_block_as_u64()
        );

        step_block(9);
        log::info!(
            "Advanced to block {}",
            SubtensorModule::get_current_block_as_u64()
        );

        let tempo_before_next_reveal: u16 = 200;
        log::info!("Changing tempo to {tempo_before_next_reveal}");
        SubtensorModule::set_tempo_unchecked(netuid, tempo_before_next_reveal);

        step_epochs(1, netuid);
        log::info!(
            "Advanced to block {}",
            SubtensorModule::get_current_block_as_u64()
        );

        assert_ok!(SubtensorModule::reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids.clone(),
            weight_values.clone(),
            salt.clone(),
            version_key,
        ));
        log::info!(
            "Revealed at block {}",
            SubtensorModule::get_current_block_as_u64()
        );

        assert_ok!(SubtensorModule::commit_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            commit_hash
        ));
        log::info!(
            "Commit successful at block {}",
            SubtensorModule::get_current_block_as_u64()
        );

        let tempo: u16 = 150;
        log::info!("Changing tempo to {tempo}");
        SubtensorModule::set_tempo_unchecked(netuid, tempo);

        step_epochs(1, netuid);
        log::info!(
            "Advanced to block {}",
            SubtensorModule::get_current_block_as_u64()
        );

        assert_ok!(SubtensorModule::reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids.clone(),
            weight_values.clone(),
            salt.clone(),
            version_key,
        ));
        log::info!(
            "Revealed at block {}",
            SubtensorModule::get_current_block_as_u64()
        );

        let tempo: u16 = 1050;
        log::info!("Changing tempo to {tempo}");
        SubtensorModule::set_tempo_unchecked(netuid, tempo);

        assert_ok!(SubtensorModule::commit_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            commit_hash
        ));
        log::info!(
            "Commit successful at block {}",
            SubtensorModule::get_current_block_as_u64()
        );

        let tempo: u16 = 805;
        log::info!("Changing tempo to {tempo}");
        SubtensorModule::set_tempo_unchecked(netuid, tempo);

        step_epochs(1, netuid);
        log::info!(
            "Advanced to block {}",
            SubtensorModule::get_current_block_as_u64()
        );

        assert_ok!(SubtensorModule::reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids.clone(),
            weight_values.clone(),
            salt.clone(),
            version_key,
        ));
        log::info!(
            "Revealed at block {}",
            SubtensorModule::get_current_block_as_u64()
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_commit_reveal_multiple_commits --exact --show-output --nocapture
#[test]
fn test_commit_reveal_multiple_commits() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let uids: Vec<u16> = vec![0, 1];
        let weight_values: Vec<u16> = vec![10, 10];
        let version_key: u64 = 0;
        let hotkey: U256 = U256::from(1);

        System::set_block_number(0);

        let tempo: u16 = 7200;
        add_network(netuid, tempo, 0);

        // Setup the network and neurons
        register_ok_neuron(netuid, U256::from(3), U256::from(4), 300_000);
        register_ok_neuron(netuid, U256::from(1), U256::from(2), 100_000);
        SubtensorModule::set_stake_threshold(0);
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);
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

        // 1. Commit 10 times successfully
        let mut commit_info = Vec::new();
        for i in 0..10 {
            let salt_i: Vec<u16> = vec![i; 8]; // Unique salt for each commit
            let commit_hash: H256 = BlakeTwo256::hash_of(&(
                hotkey,
                netuid,
                uids.clone(),
                weight_values.clone(),
                salt_i.clone(),
                version_key,
            ));
            commit_info.push((commit_hash, salt_i));
            assert_ok!(SubtensorModule::commit_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                commit_hash
            ));
        }

        // 2. Attempt to commit an 11th time, should fail
        let salt_11: Vec<u16> = vec![11; 8];
        let commit_hash_11: H256 = BlakeTwo256::hash_of(&(
            hotkey,
            netuid,
            uids.clone(),
            weight_values.clone(),
            salt_11.clone(),
            version_key,
        ));
        assert_err!(
            SubtensorModule::commit_weights(RuntimeOrigin::signed(hotkey), netuid, commit_hash_11),
            Error::<Test>::TooManyUnrevealedCommits
        );

        // 3. Attempt to reveal out of order (reveal the second commit first)
        // Advance to the next epoch for reveals to be valid
        step_epochs(1, netuid);

        // Try to reveal the second commit first
        let (_commit_hash_2, salt_2) = &commit_info[1];
        assert_ok!(SubtensorModule::reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids.clone(),
            weight_values.clone(),
            salt_2.clone(),
            version_key,
        ));

        // Check that commits before the revealed one are removed
        let remaining_commits =
            crate::WeightCommits::<Test>::get(NetUidStorageIndex::from(netuid), hotkey)
                .expect("expected 8 remaining commits");
        assert_eq!(remaining_commits.len(), 8); // 10 commits - 2 removed (index 0 and 1)

        // 4. Reveal the last commit next
        let (_commit_hash_10, salt_10) = &commit_info[9];
        assert_ok!(SubtensorModule::reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids.clone(),
            weight_values.clone(),
            salt_10.clone(),
            version_key,
        ));

        // Remaining commits should have removed up to index 9
        let remaining_commits =
            crate::WeightCommits::<Test>::get(NetUidStorageIndex::from(netuid), hotkey);
        assert!(remaining_commits.is_none()); // All commits removed

        // After revealing all commits, attempt to commit again should now succeed
        assert_ok!(SubtensorModule::commit_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            commit_hash_11
        ));

        // 5. Test expired commits are removed and do not block reveals
        // Commit again and let the commit expire
        let salt_12: Vec<u16> = vec![12; 8];
        let commit_hash_12: H256 = BlakeTwo256::hash_of(&(
            hotkey,
            netuid,
            uids.clone(),
            weight_values.clone(),
            salt_12.clone(),
            version_key,
        ));
        assert_ok!(SubtensorModule::commit_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            commit_hash_12
        ));

        // Advance two epochs so the commit expires
        step_epochs(2, netuid);

        // Attempt to reveal the expired commit, should fail
        assert_err!(
            SubtensorModule::reveal_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                uids.clone(),
                weight_values.clone(),
                salt_12.clone(),
                version_key,
            ),
            Error::<Test>::ExpiredWeightCommit
        );

        // Commit again and reveal after advancing to next epoch
        let salt_13: Vec<u16> = vec![13; 8];
        let commit_hash_13: H256 = BlakeTwo256::hash_of(&(
            hotkey,
            netuid,
            uids.clone(),
            weight_values.clone(),
            salt_13.clone(),
            version_key,
        ));
        assert_ok!(SubtensorModule::commit_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            commit_hash_13
        ));

        step_epochs(1, netuid);

        assert_ok!(SubtensorModule::reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids.clone(),
            weight_values.clone(),
            salt_13.clone(),
            version_key,
        ));

        // 6. Ensure that attempting to reveal after the valid reveal period fails
        // Commit again
        let salt_14: Vec<u16> = vec![14; 8];
        let commit_hash_14: H256 = BlakeTwo256::hash_of(&(
            hotkey,
            netuid,
            uids.clone(),
            weight_values.clone(),
            salt_14.clone(),
            version_key,
        ));
        assert_ok!(SubtensorModule::commit_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            commit_hash_14
        ));

        // Advance beyond the valid reveal period (more than one epoch)
        step_epochs(2, netuid);

        // Attempt to reveal, should fail
        assert_err!(
            SubtensorModule::reveal_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                uids.clone(),
                weight_values.clone(),
                salt_14.clone(),
                version_key,
            ),
            Error::<Test>::ExpiredWeightCommit
        );

        // 7. Attempt to reveal a commit that is not ready yet (before the reveal period)
        // Commit again
        let salt_15: Vec<u16> = vec![15; 8];
        let commit_hash_15: H256 = BlakeTwo256::hash_of(&(
            hotkey,
            netuid,
            uids.clone(),
            weight_values.clone(),
            salt_15.clone(),
            version_key,
        ));
        assert_ok!(SubtensorModule::commit_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            commit_hash_15
        ));

        // Attempt to reveal immediately, should fail
        assert_err!(
            SubtensorModule::reveal_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                uids.clone(),
                weight_values.clone(),
                salt_15.clone(),
                version_key,
            ),
            Error::<Test>::RevealTooEarly
        );

        step_epochs(1, netuid);

        // Now reveal should succeed
        assert_ok!(SubtensorModule::reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids.clone(),
            weight_values.clone(),
            salt_15.clone(),
            version_key,
        ));

        // 8. Test that revealing with incorrect data (salt) fails
        // Commit again
        let salt_16: Vec<u16> = vec![16; 8];
        let commit_hash_16: H256 = BlakeTwo256::hash_of(&(
            hotkey,
            netuid,
            uids.clone(),
            weight_values.clone(),
            salt_16.clone(),
            version_key,
        ));
        assert_ok!(SubtensorModule::commit_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            commit_hash_16
        ));

        step_epochs(1, netuid);

        // Attempt to reveal with incorrect salt
        let wrong_salt: Vec<u16> = vec![99; 8];
        assert_err!(
            SubtensorModule::reveal_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                uids.clone(),
                weight_values.clone(),
                wrong_salt.clone(),
                version_key,
            ),
            Error::<Test>::InvalidRevealCommitHashNotMatch
        );

        // Reveal with correct data should succeed
        assert_ok!(SubtensorModule::reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids.clone(),
            weight_values.clone(),
            salt_16.clone(),
            version_key,
        ));

        // 9. Test that attempting to reveal when there are no commits fails
        assert_err!(
            SubtensorModule::reveal_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                uids.clone(),
                weight_values.clone(),
                salt_16.clone(),
                version_key,
            ),
            Error::<Test>::NoWeightsCommitFound
        );

        // 10. Commit twice and attempt to reveal out of sequence (which is now allowed)
        let salt_a: Vec<u16> = vec![21; 8];
        let commit_hash_a: H256 = BlakeTwo256::hash_of(&(
            hotkey,
            netuid,
            uids.clone(),
            weight_values.clone(),
            salt_a.clone(),
            version_key,
        ));
        assert_ok!(SubtensorModule::commit_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            commit_hash_a
        ));

        let salt_b: Vec<u16> = vec![22; 8];
        let commit_hash_b: H256 = BlakeTwo256::hash_of(&(
            hotkey,
            netuid,
            uids.clone(),
            weight_values.clone(),
            salt_b.clone(),
            version_key,
        ));
        assert_ok!(SubtensorModule::commit_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            commit_hash_b
        ));

        step_epochs(1, netuid);

        // Reveal the second commit first, should now succeed
        assert_ok!(SubtensorModule::reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids.clone(),
            weight_values.clone(),
            salt_b.clone(),
            version_key,
        ));

        // Check that the first commit has been removed
        let remaining_commits =
            crate::WeightCommits::<Test>::get(NetUidStorageIndex::from(netuid), hotkey);
        assert!(remaining_commits.is_none());

        // Attempting to reveal the first commit should fail as it was removed
        assert_err!(
            SubtensorModule::reveal_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                uids,
                weight_values,
                salt_a,
                version_key,
            ),
            Error::<Test>::NoWeightsCommitFound
        );
    });
}
