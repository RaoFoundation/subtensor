#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
//! Commit–reveal timing: expiry, exact epoch/block, tempo & reveal-period changes.

use frame_support::{assert_err, assert_ok};
use sp_core::{H256, U256};
use sp_runtime::traits::{BlakeTwo256, Hash};
use sp_std::collections::vec_deque::VecDeque;
use subtensor_runtime_common::NetUidStorageIndex;

use crate::tests::mock::*;
use crate::*;

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_expired_commits_handling_in_commit_and_reveal --exact --show-output --nocapture
#[test]
fn test_expired_commits_handling_in_commit_and_reveal() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey: <Test as frame_system::Config>::AccountId = U256::from(1);
        let version_key: u64 = 0;
        let uids: Vec<u16> = vec![0, 1];
        let weight_values: Vec<u16> = vec![10, 10];
        let tempo: u16 = 100;

        System::set_block_number(0);
        add_network(netuid, tempo, 0);

        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);

        // Register neurons
        register_ok_neuron(netuid, U256::from(3), U256::from(4), 300_000);
        register_ok_neuron(netuid, U256::from(1), U256::from(2), 100_000);
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

        // 1. Commit 5 times in epoch 0
        let mut commit_info = Vec::new();
        for i in 0..5 {
            let salt: Vec<u16> = vec![i; 8];
            let commit_hash: H256 = BlakeTwo256::hash_of(&(
                hotkey,
                netuid,
                uids.clone(),
                weight_values.clone(),
                salt.clone(),
                version_key,
            ));
            commit_info.push((commit_hash, salt));
            assert_ok!(SubtensorModule::commit_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                commit_hash
            ));
        }

        // Advance to epoch 1
        step_epochs(1, netuid);

        // 2. Commit another 5 times in epoch 1
        for i in 5..10 {
            let salt: Vec<u16> = vec![i; 8];
            let commit_hash: H256 = BlakeTwo256::hash_of(&(
                hotkey,
                netuid,
                uids.clone(),
                weight_values.clone(),
                salt.clone(),
                version_key,
            ));
            commit_info.push((commit_hash, salt));
            assert_ok!(SubtensorModule::commit_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                commit_hash
            ));
        }

        // 3. Attempt to commit an 11th time, should fail with TooManyUnrevealedCommits
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

        // 4. Advance to epoch 2 to expire the commits from epoch 0
        step_epochs(1, netuid); // Now at epoch 2

        // 5. Attempt to commit again; should succeed after expired commits are removed
        assert_ok!(SubtensorModule::commit_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            commit_hash_11
        ));

        // 6. Verify that the number of unrevealed, non-expired commits is now 6
        let commits: VecDeque<(H256, u64, u64, u64)> =
            crate::WeightCommits::<Test>::get(NetUidStorageIndex::from(netuid), hotkey)
                .expect("Expected a commit");
        assert_eq!(commits.len(), 6); // 5 non-expired commits from epoch 1 + new commit

        // 7. Attempt to reveal an expired commit (from epoch 0)
        // Previous commit removed expired commits
        let (_, expired_salt) = &commit_info[0];
        assert_err!(
            SubtensorModule::reveal_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                uids.clone(),
                weight_values.clone(),
                expired_salt.clone(),
                version_key,
            ),
            Error::<Test>::InvalidRevealCommitHashNotMatch
        );

        // 8. Reveal commits from epoch 1 at current_epoch = 2
        for (_, salt) in commit_info.iter().skip(5).take(5) {
            let salt = salt.clone();

            assert_ok!(SubtensorModule::reveal_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                uids.clone(),
                weight_values.clone(),
                salt.clone(),
                version_key,
            ));
        }

        // 9. Advance to epoch 3 to reveal the new commit
        step_epochs(1, netuid);

        // 10. Reveal the new commit from epoch 2
        assert_ok!(SubtensorModule::reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids.clone(),
            weight_values.clone(),
            salt_11.clone(),
            version_key,
        ));

        // 10. Verify that all commits have been revealed and the queue is empty
        let commits = crate::WeightCommits::<Test>::get(NetUidStorageIndex::from(netuid), hotkey);
        assert!(commits.is_none());

        // 11. Attempt to reveal again, should fail with NoWeightsCommitFound
        assert_err!(
            SubtensorModule::reveal_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                uids.clone(),
                weight_values.clone(),
                salt_11.clone(),
                version_key,
            ),
            Error::<Test>::NoWeightsCommitFound
        );

        // 12. Commit again to ensure we can continue after previous commits
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

        // Advance to next epoch (epoch 4) and reveal
        step_epochs(1, netuid);

        assert_ok!(SubtensorModule::reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids,
            weight_values,
            salt_12,
            version_key,
        ));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_reveal_at_exact_epoch --exact --show-output --nocapture
#[test]
fn test_reveal_at_exact_epoch() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey: <Test as frame_system::Config>::AccountId = U256::from(1);
        let version_key: u64 = 0;
        let uids: Vec<u16> = vec![0, 1];
        let weight_values: Vec<u16> = vec![10, 10];
        let tempo: u16 = 100;

        System::set_block_number(0);
        add_network(netuid, tempo, 0);

        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);

        register_ok_neuron(netuid, U256::from(3), U256::from(4), 300_000);
        register_ok_neuron(netuid, U256::from(1), U256::from(2), 100_000);
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

        let reveal_periods: Vec<u64> = vec![1, 2, 7, 40, 86, 100];

        for &reveal_period in &reveal_periods {
            assert_ok!(SubtensorModule::set_reveal_period(netuid, reveal_period));

            let salt: Vec<u16> = vec![42; 8];
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

            // Retrieve commit information
            let commit_block = SubtensorModule::get_current_block_as_u64();
            let commit_epoch = SubtensorModule::get_epoch_index(netuid, commit_block);
            let reveal_epoch = commit_epoch.saturating_add(reveal_period);

            // Attempt to reveal before the allowed epoch
            if reveal_period > 0 {
                // Advance to epoch before the reveal epoch
                if reveal_period >= 1 {
                    step_epochs((reveal_period - 1) as u16, netuid);
                }

                // Attempt to reveal too early
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
            }

            // Advance to the exact reveal epoch
            let current_epoch = SubtensorModule::get_epoch_index(
                netuid,
                SubtensorModule::get_current_block_as_u64(),
            );
            if current_epoch < reveal_epoch {
                step_epochs((reveal_epoch - current_epoch) as u16, netuid);
            }

            // Reveal at the exact allowed epoch
            assert_ok!(SubtensorModule::reveal_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                uids.clone(),
                weight_values.clone(),
                salt.clone(),
                version_key,
            ));

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

            let new_salt: Vec<u16> = vec![43; 8];
            let new_commit_hash: H256 = BlakeTwo256::hash_of(&(
                hotkey,
                netuid,
                uids.clone(),
                weight_values.clone(),
                new_salt.clone(),
                version_key,
            ));
            assert_ok!(SubtensorModule::commit_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                new_commit_hash
            ));

            // Advance past the reveal epoch to ensure commit expiration
            step_epochs((reveal_period + 1) as u16, netuid);

            // Attempt to reveal after the allowed epoch
            assert_err!(
                SubtensorModule::reveal_weights(
                    RuntimeOrigin::signed(hotkey),
                    netuid,
                    uids.clone(),
                    weight_values.clone(),
                    new_salt.clone(),
                    version_key,
                ),
                Error::<Test>::ExpiredWeightCommit
            );

            crate::WeightCommits::<Test>::remove(NetUidStorageIndex::from(netuid), hotkey);
        }
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_tempo_and_reveal_period_change_during_commit_reveal_process --exact --show-output --nocapture
#[test]
fn test_tempo_and_reveal_period_change_during_commit_reveal_process() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let uids: Vec<u16> = vec![0, 1];
        let weight_values: Vec<u16> = vec![10, 10];
        let salt: Vec<u16> = vec![42; 8];
        let version_key: u64 = 0;
        let hotkey: <Test as frame_system::Config>::AccountId = U256::from(1);

        // Compute initial commit hash
        let commit_hash: H256 = BlakeTwo256::hash_of(&(
            hotkey,
            netuid,
            uids.clone(),
            weight_values.clone(),
            salt.clone(),
            version_key,
        ));

        System::set_block_number(0);

        let initial_tempo: u16 = 100;
        let initial_reveal_period: u64 = 1;
        add_network(netuid, initial_tempo, 0);
        assert_ok!(SubtensorModule::set_reveal_period(netuid, initial_reveal_period));
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);

        SubtensorModule::set_weights_set_rate_limit(netuid, 0);

        register_ok_neuron(netuid, U256::from(3), U256::from(4), 300_000);
        register_ok_neuron(netuid, U256::from(1), U256::from(2), 100_000);
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

        // Step 1: Commit weights
        assert_ok!(SubtensorModule::commit_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            commit_hash
        ));
        log::info!(
            "Commit successful at block {}",
            SubtensorModule::get_current_block_as_u64()
        );

        // Retrieve commit block and epoch
        let commit_block = SubtensorModule::get_current_block_as_u64();
        let commit_epoch = SubtensorModule::get_epoch_index(netuid, commit_block);

        // Step 2: Change tempo and reveal period after commit
        let new_tempo: u16 = 50;
        let new_reveal_period: u64 = 2;
        SubtensorModule::set_tempo_unchecked(netuid, new_tempo);
        assert_ok!(SubtensorModule::set_reveal_period(netuid, new_reveal_period));
        log::info!(
            "Changed tempo to {new_tempo} and reveal period to {new_reveal_period}"
        );

        // Step 3: Advance blocks to reach the reveal epoch according to new tempo and reveal period
        let current_block = SubtensorModule::get_current_block_as_u64();
        let current_epoch = SubtensorModule::get_epoch_index(netuid, current_block);
        let reveal_epoch = commit_epoch.saturating_add(new_reveal_period);

        // Advance to one epoch before reveal epoch
        if current_epoch < reveal_epoch {
            let epochs_to_advance = reveal_epoch - current_epoch - 1;
            step_epochs(epochs_to_advance as u16, netuid);
        }

        // Attempt to reveal too early
        assert_err!(
            SubtensorModule::reveal_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                uids.clone(),
                weight_values.clone(),
                salt.clone(),
                version_key
            ),
            Error::<Test>::RevealTooEarly
        );
        log::info!(
            "Attempted to reveal too early at block {}",
            SubtensorModule::get_current_block_as_u64()
        );

        // Advance to reveal epoch
        step_epochs(1, netuid);

        // Attempt to reveal at the correct epoch
        assert_ok!(SubtensorModule::reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids.clone(),
            weight_values.clone(),
            salt.clone(),
            version_key
        ));
        log::info!(
            "Revealed weights at block {}",
            SubtensorModule::get_current_block_as_u64()
        );

        // Step 4: Change tempo and reveal period again after reveal
        let new_tempo_after_reveal: u16 = 200;
        let new_reveal_period_after_reveal: u64 = 1;
        SubtensorModule::set_tempo_unchecked(netuid, new_tempo_after_reveal);
        assert_ok!(SubtensorModule::set_reveal_period(
            netuid,
            new_reveal_period_after_reveal
        ));
        log::info!("Changed tempo to {new_tempo_after_reveal} and reveal period to {new_reveal_period_after_reveal} after reveal");

        // Step 5: Commit again
        let new_salt: Vec<u16> = vec![43; 8];
        let new_commit_hash: H256 = BlakeTwo256::hash_of(&(
            hotkey,
            netuid,
            uids.clone(),
            weight_values.clone(),
            new_salt.clone(),
            version_key,
        ));
        assert_ok!(SubtensorModule::commit_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            new_commit_hash
        ));
        log::info!(
            "Commit successful at block {}",
            SubtensorModule::get_current_block_as_u64()
        );

        // Retrieve new commit block and epoch
        let new_commit_block = SubtensorModule::get_current_block_as_u64();
        let new_commit_epoch = SubtensorModule::get_epoch_index(netuid, new_commit_block);
        let new_reveal_epoch = new_commit_epoch.saturating_add(new_reveal_period_after_reveal);

        // Advance to reveal epoch
        let current_block = SubtensorModule::get_current_block_as_u64();
        let current_epoch = SubtensorModule::get_epoch_index(netuid, current_block);
        if current_epoch < new_reveal_epoch {
            let epochs_to_advance = new_reveal_epoch - current_epoch;
            step_epochs(epochs_to_advance as u16, netuid);
        }

        // Attempt to reveal at the correct epoch
        assert_ok!(SubtensorModule::reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids.clone(),
            weight_values.clone(),
            new_salt.clone(),
            version_key
        ));
        log::info!(
            "Revealed weights at block {}",
            SubtensorModule::get_current_block_as_u64()
        );

        // Step 6: Attempt to reveal after the allowed epoch (commit expires)
        // Advance past the reveal epoch
        let expiration_epochs = 1;
        step_epochs(expiration_epochs as u16, netuid);

        // Attempt to reveal again (should fail due to expired commit)
        assert_err!(
            SubtensorModule::reveal_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                uids.clone(),
                weight_values.clone(),
                new_salt.clone(),
                version_key
            ),
            Error::<Test>::NoWeightsCommitFound
        );
        log::info!(
            "Attempted to reveal after expiration at block {}",
            SubtensorModule::get_current_block_as_u64()
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_commit_reveal_order_enforcement --exact --show-output --nocapture
#[test]
fn test_commit_reveal_order_enforcement() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey: <Test as frame_system::Config>::AccountId = U256::from(1);
        let version_key: u64 = 0;
        let uids: Vec<u16> = vec![0, 1];
        let weight_values: Vec<u16> = vec![10, 10];
        let tempo: u16 = 100;

        System::set_block_number(0);
        add_network(netuid, tempo, 0);

        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);

        register_ok_neuron(netuid, U256::from(3), U256::from(4), 300_000);
        register_ok_neuron(netuid, U256::from(1), U256::from(2), 100_000);
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

        // Commit three times: A, B, C
        let mut commit_info = Vec::new();
        for i in 0..3 {
            let salt: Vec<u16> = vec![i; 8];
            let commit_hash: H256 = BlakeTwo256::hash_of(&(
                hotkey,
                netuid,
                uids.clone(),
                weight_values.clone(),
                salt.clone(),
                version_key,
            ));
            commit_info.push((commit_hash, salt));
            assert_ok!(SubtensorModule::commit_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                commit_hash
            ));
        }

        step_epochs(1, netuid);

        // Attempt to reveal B first (index 1), should now succeed
        let (_commit_hash_b, salt_b) = &commit_info[1];
        assert_ok!(SubtensorModule::reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids.clone(),
            weight_values.clone(),
            salt_b.clone(),
            version_key,
        ));

        // Check that commits A and B are removed
        let remaining_commits =
            crate::WeightCommits::<Test>::get(NetUidStorageIndex::from(netuid), hotkey)
                .expect("expected 1 remaining commit");
        assert_eq!(remaining_commits.len(), 1); // Only commit C should remain

        // Attempt to reveal C (index 2), should succeed
        let (_commit_hash_c, salt_c) = &commit_info[2];
        assert_ok!(SubtensorModule::reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids.clone(),
            weight_values.clone(),
            salt_c.clone(),
            version_key,
        ));

        // Attempting to reveal A (index 0) should fail as it's been removed
        let (_commit_hash_a, salt_a) = &commit_info[0];
        assert_err!(
            SubtensorModule::reveal_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                uids,
                weight_values,
                salt_a.clone(),
                version_key,
            ),
            Error::<Test>::NoWeightsCommitFound
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_reveal_at_exact_block --exact --show-output --nocapture
#[test]
fn test_reveal_at_exact_block() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey: <Test as frame_system::Config>::AccountId = U256::from(1);
        let version_key: u64 = 0;
        let uids: Vec<u16> = vec![0, 1];
        let weight_values: Vec<u16> = vec![10, 10];
        let tempo: u16 = 360;

        System::set_block_number(0);
        add_network_disable_commit_reveal(netuid, tempo, 0);

        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);

        register_ok_neuron(netuid, U256::from(3), U256::from(4), 300_000);
        register_ok_neuron(netuid, U256::from(1), U256::from(2), 100_000);
        SubtensorModule::set_validator_permit_for_uid(netuid, 0, true);
        SubtensorModule::set_validator_permit_for_uid(netuid, 1, true);

        let reveal_periods: Vec<u64> = vec![1, 2, 5, 19, 21, 30, 77];

        for &reveal_period in &reveal_periods {
            assert_ok!(SubtensorModule::set_reveal_period(netuid, reveal_period));

            // Step 1: Commit weights
            let salt: Vec<u16> = vec![42 + (reveal_period % 100) as u16; 8];
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

            // Epoch the commit was tagged with (counter is the canonical index).
            let commit_epoch =
                crate::WeightCommits::<Test>::get(NetUidStorageIndex::from(netuid), hotkey)
                    .and_then(|q| q.back().map(|(_, e, _, _)| *e))
                    .expect("commit stored");

            // Attempt to reveal before the reveal epoch — too early.
            assert_err!(
                SubtensorModule::reveal_weights(
                    RuntimeOrigin::signed(hotkey),
                    netuid,
                    uids.clone(),
                    weight_values.clone(),
                    salt.clone(),
                    version_key
                ),
                Error::<Test>::RevealTooEarly
            );

            // Advance the epoch counter into the reveal epoch; pin the scheduler.
            SubnetEpochIndex::<Test>::insert(netuid, commit_epoch + reveal_period);
            LastEpochBlock::<Test>::insert(netuid, SubtensorModule::get_current_block_as_u64());
            PendingEpochAt::<Test>::insert(netuid, 0);

            // Reveal at the exact allowed epoch
            assert_ok!(SubtensorModule::reveal_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                uids.clone(),
                weight_values.clone(),
                salt.clone(),
                version_key
            ));

            // Attempt to reveal again; should fail with NoWeightsCommitFound
            assert_err!(
                SubtensorModule::reveal_weights(
                    RuntimeOrigin::signed(hotkey),
                    netuid,
                    uids.clone(),
                    weight_values.clone(),
                    salt.clone(),
                    version_key
                ),
                Error::<Test>::NoWeightsCommitFound
            );

            // Commit again with new salt
            let new_salt: Vec<u16> = vec![43 + (reveal_period % 100) as u16; 8];
            let new_commit_hash: H256 = BlakeTwo256::hash_of(&(
                hotkey,
                netuid,
                uids.clone(),
                weight_values.clone(),
                new_salt.clone(),
                version_key,
            ));
            assert_ok!(SubtensorModule::commit_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                new_commit_hash
            ));

            // Advance the epoch counter past the reveal epoch — commit expired.
            let new_commit_epoch =
                crate::WeightCommits::<Test>::get(NetUidStorageIndex::from(netuid), hotkey)
                    .and_then(|q| q.back().map(|(_, e, _, _)| *e))
                    .expect("commit stored");
            SubnetEpochIndex::<Test>::insert(netuid, new_commit_epoch + reveal_period + 1);
            LastEpochBlock::<Test>::insert(netuid, SubtensorModule::get_current_block_as_u64());

            // Attempt to reveal after the commit has expired
            assert_err!(
                SubtensorModule::reveal_weights(
                    RuntimeOrigin::signed(hotkey),
                    netuid,
                    uids.clone(),
                    weight_values.clone(),
                    new_salt.clone(),
                    version_key
                ),
                Error::<Test>::ExpiredWeightCommit
            );

            // Clean up for next iteration
            crate::WeightCommits::<Test>::remove(NetUidStorageIndex::from(netuid), hotkey);
        }
    });
}
