use subtensor_runtime_common::NetUid;

use super::mock::*;
use crate::LastEpochBlock;

// 1. Test Zero Tempo
// Description: Verify that when tempo is 0, the function returns u64::MAX.
//  SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::emission::test_zero_tempo --exact --show-output --nocapture
#[test]
fn test_zero_tempo() {
    new_test_ext(1).execute_with(|| {
        assert_eq!(
            SubtensorModule::blocks_until_next_auto_epoch(1.into(), 0, 100),
            u64::MAX
        );
    });
}

// 2. Test Regular Case
// Description: Check if the function correctly calculates the blocks until the next epoch for various combinations of netuid, tempo, and block_number.
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::emission::test_regular_case --exact --show-output --nocapture
#[test]
fn test_regular_case() {
    new_test_ext(1).execute_with(|| {
        LastEpochBlock::<Test>::insert(NetUid::from(1), 0);
        LastEpochBlock::<Test>::insert(NetUid::from(2), 0);
        LastEpochBlock::<Test>::insert(NetUid::from(3), 0);
        // (LastEpochBlock + tempo) - block.
        assert_eq!(
            SubtensorModule::blocks_until_next_auto_epoch(1.into(), 10, 5),
            5
        );
        assert_eq!(
            SubtensorModule::blocks_until_next_auto_epoch(2.into(), 20, 15),
            5
        );
        assert_eq!(
            SubtensorModule::blocks_until_next_auto_epoch(3.into(), 30, 25),
            5
        );
    });
}

// 3. Test Boundary Conditions
// Description: Ensure the function handles edge cases like maximum u16 values for netuid and tempo, and maximum u64 value for block_number.
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::emission::test_boundary_conditions --exact --show-output --nocapture
#[test]
fn test_boundary_conditions() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(u16::MAX);
        LastEpochBlock::<Test>::insert(netuid, 0);
        // Far past the next-auto block — saturating to 0.
        assert_eq!(
            SubtensorModule::blocks_until_next_auto_epoch(netuid, u16::MAX, u64::MAX),
            0
        );
        // Block 0 — full period until next auto epoch.
        assert_eq!(
            SubtensorModule::blocks_until_next_auto_epoch(netuid, u16::MAX, 0),
            u16::MAX as u64
        );
    });
}

// 4. Test Overflow Handling
// Description: Verify that the function correctly handles potential overflows in intermediate calculations.
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::emission::test_overflow_handling --exact --show-output --nocapture
#[test]
fn test_overflow_handling() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(u16::MAX);
        LastEpochBlock::<Test>::insert(netuid, 0);
        assert_eq!(
            SubtensorModule::blocks_until_next_auto_epoch(netuid, u16::MAX, u64::MAX - 1),
            0
        );
    });
}

// 5. Test Epoch Alignment
// Description: Check if the function returns 0 when the current block is exactly at an epoch boundary.
//  SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::emission::test_epoch_alignment --exact --show-output --nocapture
#[test]
fn test_epoch_alignment() {
    new_test_ext(1).execute_with(|| {
        LastEpochBlock::<Test>::insert(NetUid::from(1), 0);
        LastEpochBlock::<Test>::insert(NetUid::from(2), 0);
        // (LastEpochBlock + tempo) - block_number.
        assert_eq!(
            SubtensorModule::blocks_until_next_auto_epoch(1.into(), 10, 9),
            1
        );
        // Block exactly at next-auto — returns 0.
        assert_eq!(
            SubtensorModule::blocks_until_next_auto_epoch(2.into(), 20, 21),
            0
        );
    });
}

// 7. Test Different Network IDs
// Description: Verify that the function behaves correctly for different network IDs (netuids).
//  SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::emission::test_different_network_ids --exact --show-output --nocapture
#[test]
fn test_different_network_ids() {
    new_test_ext(1).execute_with(|| {
        // Anchor each subnet identically — proves the new formula does NOT
        // depend on `netuid` (only on the per-subnet `LastEpochBlock`).
        LastEpochBlock::<Test>::insert(NetUid::from(1), 0);
        LastEpochBlock::<Test>::insert(NetUid::from(2), 0);
        LastEpochBlock::<Test>::insert(NetUid::from(3), 0);
        assert_eq!(
            SubtensorModule::blocks_until_next_auto_epoch(1.into(), 10, 5),
            5
        );
        assert_eq!(
            SubtensorModule::blocks_until_next_auto_epoch(2.into(), 10, 5),
            5
        );
        assert_eq!(
            SubtensorModule::blocks_until_next_auto_epoch(3.into(), 10, 5),
            5
        );
    });
}

// 8. Test Large Tempo Values
// Description: Check if the function works correctly with large tempo values close to u16::MAX.
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::emission::test_large_tempo_values --exact --show-output --nocapture
#[test]
fn test_large_tempo_values() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        LastEpochBlock::<Test>::insert(netuid, 0);
        assert_eq!(
            SubtensorModule::blocks_until_next_auto_epoch(netuid, u16::MAX - 1, 100),
            (u16::MAX as u64).saturating_sub(1).saturating_sub(100)
        );
    });
}

// 9. Test Consecutive Blocks
// Description: Ensure that the function returns expected decreasing values for consecutive block numbers within an epoch.
//  SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::emission::test_consecutive_blocks --exact --show-output --nocapture
#[test]
fn test_consecutive_blocks() {
    new_test_ext(1).execute_with(|| {
        let tempo = 10;
        let netuid = NetUid::from(1);
        LastEpochBlock::<Test>::insert(netuid, 0);
        let mut last_result = SubtensorModule::blocks_until_next_auto_epoch(netuid, tempo, 0);
        for i in 1..tempo - 1 {
            let current_result =
                SubtensorModule::blocks_until_next_auto_epoch(netuid, tempo, i as u64);
            assert_eq!(current_result, last_result - 1);
            last_result = current_result;
        }
    });
}

// 10. Test Wrap-around Behavior
// Description: Verify that the function correctly handles the wrap-around case when block_number is close to u64::MAX.
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::emission::test_wrap_around_behavior --exact --show-output --nocapture
#[test]
fn test_wrap_around_behavior() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        LastEpochBlock::<Test>::insert(netuid, 0);
        // `next_auto - block_number` saturates to 0 for far-future blocks.
        assert_eq!(
            SubtensorModule::blocks_until_next_auto_epoch(netuid, 10, u64::MAX),
            0
        );
        assert_eq!(
            SubtensorModule::blocks_until_next_auto_epoch(netuid, 10, u64::MAX - 1),
            0
        );
    });
}
