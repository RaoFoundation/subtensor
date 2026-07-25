//! Unit tests for alpha/TAO transaction fee charging.
//!
//! Concept modules mirror fee paths: remove-stake, unstake-all, hotkey swap, move/transfer/swap,
//! burn/recycle, block-author sinks, and miner-collateral guards.

#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]

mod helpers;
mod mock;

mod alpha_fee_collateral;
mod block_builder_fees;
mod burn_recycle_alpha_fees;
mod move_transfer_swap_stake_fees;
mod remove_stake_fees;
mod swap_hotkey_fees;
mod unstake_all_fees;
