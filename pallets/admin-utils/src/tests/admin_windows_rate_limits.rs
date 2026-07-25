//! Admin freeze window, owner hyperparam rate limits, and start-call delay.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    unused_imports
)]

use super::prelude::*;

#[test]
fn test_sudo_set_admin_freeze_window_and_rate() {
    new_test_ext().execute_with(|| {
        // Non-root fails
        assert_eq!(
            AdminUtils::sudo_set_admin_freeze_window(
                <<Test as Config>::RuntimeOrigin>::signed(U256::from(1)),
                7
            ),
            Err(DispatchError::BadOrigin)
        );
        // Root succeeds
        assert_ok!(AdminUtils::sudo_set_admin_freeze_window(
            <<Test as Config>::RuntimeOrigin>::root(),
            7
        ));
        assert_eq!(pallet_subtensor::AdminFreezeWindow::<Test>::get(), 7);

        // Owner hyperparam tempos setter
        assert_eq!(
            AdminUtils::sudo_set_owner_hparam_rate_limit(
                <<Test as Config>::RuntimeOrigin>::signed(U256::from(1)),
                5
            ),
            Err(DispatchError::BadOrigin)
        );
        assert_ok!(AdminUtils::sudo_set_owner_hparam_rate_limit(
            <<Test as Config>::RuntimeOrigin>::root(),
            5
        ));
        assert_eq!(pallet_subtensor::OwnerHyperparamRateLimit::<Test>::get(), 5);
    });
}

#[test]
fn test_freeze_window_blocks_root_and_owner() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let tempo: u16 = 10;
        // Create subnet with tempo 10
        add_network(netuid, tempo);
        // Set freeze window to 3 blocks
        assert_ok!(AdminUtils::sudo_set_admin_freeze_window(
            <<Test as Config>::RuntimeOrigin>::root(),
            3
        ));
        // Pin the state-based scheduler so the next auto-epoch lands at
        // `LastEpochBlock + tempo`. Freeze window covers blocks (next_auto - 3, next_auto].
        pallet_subtensor::LastEpochBlock::<Test>::insert(netuid, 0);
        let next_auto = tempo as u64;
        // Advance to a block inside the freeze window (remaining < 3).
        run_to_block(next_auto - 2);

        // Root should be blocked during freeze window
        assert_noop!(
            AdminUtils::sudo_set_min_burn(
                <<Test as Config>::RuntimeOrigin>::root(),
                netuid,
                123.into()
            ),
            SubtensorError::<Test>::AdminActionProhibitedDuringWeightsWindow
        );

        // Owner should be blocked during freeze window as well
        // Set owner
        let owner: U256 = U256::from(9);
        SubnetOwner::<Test>::insert(netuid, owner);
        assert_noop!(
            AdminUtils::sudo_set_commit_reveal_weights_interval(
                <<Test as Config>::RuntimeOrigin>::signed(owner),
                netuid,
                77
            ),
            SubtensorError::<Test>::AdminActionProhibitedDuringWeightsWindow
        );
    });
}

#[test]
fn test_owner_hyperparam_update_rate_limit_enforced() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 10);
        // Set owner
        let owner: U256 = U256::from(5);
        SubnetOwner::<Test>::insert(netuid, owner);

        // Set tempo to 1 so owner hyperparam RL = 2 tempos = 2 blocks
        SubtensorModule::set_tempo_unchecked(netuid, 1);
        // Disable admin freeze window to avoid blocking on small tempo
        assert_ok!(AdminUtils::sudo_set_admin_freeze_window(
            <<Test as Config>::RuntimeOrigin>::root(),
            0
        ));

        // First update succeeds
        assert_ok!(AdminUtils::sudo_set_commit_reveal_weights_interval(
            <<Test as Config>::RuntimeOrigin>::signed(owner),
            netuid,
            11
        ));
        // Immediate second update fails due to TxRateLimitExceeded
        assert_noop!(
            AdminUtils::sudo_set_commit_reveal_weights_interval(
                <<Test as Config>::RuntimeOrigin>::signed(owner),
                netuid,
                12
            ),
            SubtensorError::<Test>::TxRateLimitExceeded
        );

        // Advance less than limit still fails
        run_to_block(SubtensorModule::get_current_block_as_u64() + 1);
        assert_noop!(
            AdminUtils::sudo_set_commit_reveal_weights_interval(
                <<Test as Config>::RuntimeOrigin>::signed(owner),
                netuid,
                13
            ),
            SubtensorError::<Test>::TxRateLimitExceeded
        );

        // Advance one more block to pass the limit; should succeed
        run_to_block(SubtensorModule::get_current_block_as_u64() + 1);
        assert_ok!(AdminUtils::sudo_set_commit_reveal_weights_interval(
            <<Test as Config>::RuntimeOrigin>::signed(owner),
            netuid,
            14
        ));
    });
}

// Verifies owner hyperparameters are rate-limited independently per parameter.
// Setting one hyperparameter should not block setting a different hyperparameter
// during the same rate-limit window, but it should still block itself.
#[test]
fn test_owner_hyperparam_rate_limit_independent_per_param() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(7);
        add_network(netuid, 10);

        // Set subnet owner
        let owner: U256 = U256::from(123);
        SubnetOwner::<Test>::insert(netuid, owner);

        // Use small tempo to make RL short and deterministic (2 blocks when tempo=1)
        SubtensorModule::set_tempo_unchecked(netuid, 1);
        // Disable admin freeze window so it doesn't interfere with small tempo
        assert_ok!(AdminUtils::sudo_set_admin_freeze_window(
            <<Test as Config>::RuntimeOrigin>::root(),
            0
        ));

        // First update to kappa should succeed
        assert_ok!(AdminUtils::sudo_set_commit_reveal_weights_interval(
            <<Test as Config>::RuntimeOrigin>::signed(owner),
            netuid,
            10
        ));

        // Immediate second update to the SAME param (kappa) should be blocked by RL
        assert_noop!(
            AdminUtils::sudo_set_commit_reveal_weights_interval(
                <<Test as Config>::RuntimeOrigin>::signed(owner),
                netuid,
                11
            ),
            SubtensorError::<Test>::TxRateLimitExceeded
        );

        // Updating a DIFFERENT param (rho) should pass immediately — independent RL key
        assert_ok!(AdminUtils::sudo_set_rho(
            <<Test as Config>::RuntimeOrigin>::signed(owner),
            netuid,
            5
        ));

        // kappa should still be blocked until its own RL window passes
        assert_noop!(
            AdminUtils::sudo_set_commit_reveal_weights_interval(
                <<Test as Config>::RuntimeOrigin>::signed(owner),
                netuid,
                12
            ),
            SubtensorError::<Test>::TxRateLimitExceeded
        );

        // rho should also be blocked for itself immediately after being set
        assert_noop!(
            AdminUtils::sudo_set_rho(<<Test as Config>::RuntimeOrigin>::signed(owner), netuid, 6),
            SubtensorError::<Test>::TxRateLimitExceeded
        );

        // Advance enough blocks to pass the RL window (2 blocks when tempo=1 and default epochs=2)
        run_to_block(SubtensorModule::get_current_block_as_u64() + 2);

        // Now both hyperparameters can be updated again
        assert_ok!(AdminUtils::sudo_set_commit_reveal_weights_interval(
            <<Test as Config>::RuntimeOrigin>::signed(owner),
            netuid,
            13
        ));
        assert_ok!(AdminUtils::sudo_set_rho(
            <<Test as Config>::RuntimeOrigin>::signed(owner),
            netuid,
            7
        ));
    });
}

#[test]
fn test_sudo_set_start_call_delay_permissions_and_zero_delay() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        let coldkey_account_id = U256::from(0);
        let non_root_account = U256::from(1);

        // Get initial delay value (should be non-zero)
        let initial_delay = pallet_subtensor::StartCallDelay::<Test>::get();
        assert_eq!(initial_delay, 0);

        // Test 1: Non-root account should fail to set delay
        assert_noop!(
            AdminUtils::sudo_set_start_call_delay(
                <<Test as Config>::RuntimeOrigin>::signed(non_root_account),
                0
            ),
            DispatchError::BadOrigin
        );

        // Test 2: Create a subnet
        add_network(netuid, tempo);

        if pallet_subtensor::FirstEmissionBlockNumber::<Test>::get(netuid).is_some() {
            pallet_subtensor::FirstEmissionBlockNumber::<Test>::remove(netuid);
        }

        assert_eq!(
            pallet_subtensor::FirstEmissionBlockNumber::<Test>::get(netuid),
            None,
            "Emission block should not be set yet"
        );
        assert_eq!(
            pallet_subtensor::SubnetOwner::<Test>::get(netuid),
            coldkey_account_id,
            "Default owner should be account 0"
        );

        // Test 3: Can successfully start the subnet immediately
        assert_ok!(pallet_subtensor::Pallet::<Test>::start_call(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey_account_id),
            netuid
        ));

        // Verify emission has been set
        assert!(
            pallet_subtensor::FirstEmissionBlockNumber::<Test>::get(netuid).is_some(),
            "Emission should be set"
        );

        // Test 4: Root sets delay to zero
        assert_ok!(AdminUtils::sudo_set_start_call_delay(
            <<Test as Config>::RuntimeOrigin>::root(),
            0
        ));
        assert_eq!(
            pallet_subtensor::StartCallDelay::<Test>::get(),
            0,
            "Delay should now be zero"
        );

        // Verify event was emitted
        frame_system::Pallet::<Test>::assert_last_event(RuntimeEvent::SubtensorModule(
            pallet_subtensor::Event::StartCallDelaySet(0),
        ));

        // Test 5: Try to start the subnet again - should be FAILED (first emission block already set)
        assert_err!(
            pallet_subtensor::Pallet::<Test>::start_call(
                <<Test as Config>::RuntimeOrigin>::signed(coldkey_account_id),
                netuid
            ),
            pallet_subtensor::Error::<Test>::FirstEmissionBlockNumberAlreadySet
        );

        assert_eq!(
            pallet_subtensor::FirstEmissionBlockNumber::<Test>::get(netuid),
            Some(frame_system::Pallet::<Test>::block_number() + 1),
            "Emission should start at next block"
        );

        // Test 6: Try to start it a third time - should FAIL (already started)
        assert_err!(
            pallet_subtensor::Pallet::<Test>::start_call(
                <<Test as Config>::RuntimeOrigin>::signed(coldkey_account_id),
                netuid
            ),
            pallet_subtensor::Error::<Test>::FirstEmissionBlockNumberAlreadySet
        );
    });
}

// Verifies that owner hyperparameter rate limit is enforced based on tempo (2 tempos).
#[test]
fn test_hyperparam_rate_limit_enforced_by_tempo() {
    new_test_ext().execute_with(|| {
        // Setup subnet and owner
        let netuid = NetUid::from(42);
        add_network(netuid, 10);
        let owner: U256 = U256::from(77);
        SubnetOwner::<Test>::insert(netuid, owner);

        // Set tempo to 1 so RL = 2 blocks
        SubtensorModule::set_tempo_unchecked(netuid, 1);
        // Disable admin freeze window to avoid blocking on small tempo
        assert_ok!(AdminUtils::sudo_set_admin_freeze_window(
            <<Test as Config>::RuntimeOrigin>::root(),
            0
        ));

        // First owner update should succeed
        assert_ok!(AdminUtils::sudo_set_commit_reveal_weights_interval(
            <<Test as Config>::RuntimeOrigin>::signed(owner),
            netuid,
            1
        ));

        // Immediate second update should fail due to tempo-based RL
        assert_noop!(
            AdminUtils::sudo_set_commit_reveal_weights_interval(
                <<Test as Config>::RuntimeOrigin>::signed(owner),
                netuid,
                2
            ),
            SubtensorError::<Test>::TxRateLimitExceeded
        );

        // Advance 2 blocks (2 tempos with tempo=1) then succeed
        run_to_block(SubtensorModule::get_current_block_as_u64() + 2);
        assert_ok!(AdminUtils::sudo_set_commit_reveal_weights_interval(
            <<Test as Config>::RuntimeOrigin>::signed(owner),
            netuid,
            3
        ));
    });
}
