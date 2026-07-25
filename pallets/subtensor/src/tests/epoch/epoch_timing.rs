#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
//! Blocks-since-last-step bookkeeping around epoch.

use super::super::mock::*;
use crate::*;

#[test]
fn test_blocks_since_last_step() {
    new_test_ext(1).execute_with(|| {
        System::set_block_number(0);

        let netuid = NetUid::from(1);
        let tempo: u16 = 7200;
        add_network(netuid, tempo, 0);

        let original_blocks: u64 = SubtensorModule::get_blocks_since_last_step(netuid);

        step_block(5);

        let new_blocks: u64 = SubtensorModule::get_blocks_since_last_step(netuid);

        assert!(new_blocks > original_blocks);
        assert_eq!(new_blocks, 5);

        let blocks_to_step: u16 = SubtensorModule::blocks_until_next_auto_epoch(
            netuid,
            tempo,
            SubtensorModule::get_current_block_as_u64(),
        ) as u16
            + 10;
        step_block(blocks_to_step);

        let post_blocks: u64 = SubtensorModule::get_blocks_since_last_step(netuid);

        assert_eq!(post_blocks, 10);

        let blocks_to_step: u16 = SubtensorModule::blocks_until_next_auto_epoch(
            netuid,
            tempo,
            SubtensorModule::get_current_block_as_u64(),
        ) as u16
            + 20;
        step_block(blocks_to_step);

        let new_post_blocks: u64 = SubtensorModule::get_blocks_since_last_step(netuid);

        assert_eq!(new_post_blocks, 20);

        step_block(7);

        assert_eq!(SubtensorModule::get_blocks_since_last_step(netuid), 27);
    });
}
