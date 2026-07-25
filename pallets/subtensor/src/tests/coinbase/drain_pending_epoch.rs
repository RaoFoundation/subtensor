#![allow(
    unused,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used
)]
//! Drain-pending BlocksSinceLastStep and epoch deferral interaction.

use super::helpers::*;
use super::prelude::*;

#[test]
fn test_coinbase_drain_pending_increments_blockssincelaststep() {
    new_test_ext(1).execute_with(|| {
        let zero = U96F32::saturating_from_num(0);
        let netuid0 = add_dynamic_network(&U256::from(1), &U256::from(2));

        let blocks_since_last_step_before = BlocksSinceLastStep::<Test>::get(netuid0);

        // Check that blockssincelaststep is incremented
        SubtensorModule::drain_pending(&[netuid0], 1);

        let blocks_since_last_step_after = BlocksSinceLastStep::<Test>::get(netuid0);
        assert!(blocks_since_last_step_after > blocks_since_last_step_before);
        assert_eq!(
            blocks_since_last_step_after,
            blocks_since_last_step_before + 1
        );
    });
}

#[test]
fn test_coinbase_drain_pending_caps_blockssincelaststep_when_epoch_is_deferred() {
    new_test_ext(1).execute_with(|| {
        let netuid = add_dynamic_network(&U256::from(1), &U256::from(2));
        let tempo = 1;
        Tempo::<Test>::insert(netuid, tempo);
        PendingEpochAt::<Test>::insert(netuid, 1);
        SubtensorModule::set_max_epochs_per_block(0);

        for block in 1..=10 {
            SubtensorModule::drain_pending(&[netuid], block);
        }

        assert_eq!(
            BlocksSinceLastStep::<Test>::get(netuid),
            u64::from(tempo) + 1
        );
        assert!(SubtensorModule::should_run_epoch(netuid, 11));
    });
}

#[test]
fn test_coinbase_drain_pending_caps_blockssincelaststep_for_inconsistent_epoch() {
    new_test_ext(1).execute_with(|| {
        let netuid = add_dynamic_network(&U256::from(1), &U256::from(2));
        let tempo = 1;
        Tempo::<Test>::insert(netuid, tempo);
        PendingEpochAt::<Test>::insert(netuid, 1);

        let duplicate_hotkey = U256::from(99);
        Keys::<Test>::insert(netuid, 0, duplicate_hotkey);
        Keys::<Test>::insert(netuid, 1, duplicate_hotkey);
        assert!(!SubtensorModule::is_epoch_input_state_consistent(netuid));

        for block in 1..=10 {
            SubtensorModule::drain_pending(&[netuid], block);
        }

        assert_eq!(
            BlocksSinceLastStep::<Test>::get(netuid),
            u64::from(tempo) + 1
        );
        assert!(SubtensorModule::should_run_epoch(netuid, 11));
    });
}

#[test]
fn test_should_run_epoch_uses_subnet_tempo_for_step_age_safety_net() {
    new_test_ext(1).execute_with(|| {
        let netuid = add_dynamic_network(&U256::from(1), &U256::from(2));
        let tempo = 1;
        Tempo::<Test>::insert(netuid, tempo);
        LastEpochBlock::<Test>::insert(netuid, 100);
        PendingEpochAt::<Test>::insert(netuid, 0);
        BlocksSinceLastStep::<Test>::insert(netuid, u64::from(tempo) + 1);

        assert!(SubtensorModule::should_run_epoch(netuid, 2));
    });
}

#[test]
fn test_coinbase_drain_pending_resets_blockssincelaststep() {
    new_test_ext(1).execute_with(|| {
        let zero = U96F32::saturating_from_num(0);
        let netuid0 = add_dynamic_network(&U256::from(1), &U256::from(2));
        Tempo::<Test>::insert(netuid0, 100);
        LastEpochBlock::<Test>::insert(netuid0, 0);
        let block_number = 102;
        assert!(SubtensorModule::should_run_epoch(netuid0, block_number));

        let blocks_since_last_step_before = 12345678;
        BlocksSinceLastStep::<Test>::insert(netuid0, blocks_since_last_step_before);
        LastMechansimStepBlock::<Test>::insert(netuid0, 12345); // garbage value

        // Check that blockssincelaststep is reset to 0 on tempo
        SubtensorModule::drain_pending(&[netuid0], block_number);

        let blocks_since_last_step_after = BlocksSinceLastStep::<Test>::get(netuid0);
        assert_eq!(blocks_since_last_step_after, 0);
        assert_eq!(LastMechansimStepBlock::<Test>::get(netuid0), 12345);
    });
}

#[test]
fn test_coinbase_drain_pending_gets_counters_and_resets_them() {
    new_test_ext(1).execute_with(|| {
        let zero = U96F32::saturating_from_num(0);
        let netuid0 = add_dynamic_network(&U256::from(1), &U256::from(2));
        Tempo::<Test>::insert(netuid0, 100);
        LastEpochBlock::<Test>::insert(netuid0, 0);
        let block_number = 102;
        assert!(SubtensorModule::should_run_epoch(netuid0, block_number));

        let pending_server_em = AlphaBalance::from(123434534);
        let pending_validator_em = AlphaBalance::from(111111);
        let pending_root = AlphaBalance::from(12222222);
        let pending_owner_cut = AlphaBalance::from(12345678);

        PendingServerEmission::<Test>::insert(netuid0, pending_server_em);
        PendingValidatorEmission::<Test>::insert(netuid0, pending_validator_em);
        PendingRootAlphaDivs::<Test>::insert(netuid0, pending_root);
        PendingOwnerCut::<Test>::insert(netuid0, pending_owner_cut);

        let emissions_to_distribute = SubtensorModule::drain_pending(&[netuid0], block_number);
        assert_eq!(emissions_to_distribute.len(), 1);
        assert_eq!(
            emissions_to_distribute[&netuid0],
            (
                pending_server_em,
                pending_validator_em,
                pending_root,
                pending_owner_cut
            )
        );

        // Check that the pending emissions are reset
        assert_eq!(
            PendingServerEmission::<Test>::get(netuid0),
            AlphaBalance::ZERO
        );
        assert_eq!(
            PendingValidatorEmission::<Test>::get(netuid0),
            AlphaBalance::ZERO
        );
        assert_eq!(
            PendingRootAlphaDivs::<Test>::get(netuid0),
            AlphaBalance::ZERO
        );
        assert_eq!(PendingOwnerCut::<Test>::get(netuid0), AlphaBalance::ZERO);
    });
}
