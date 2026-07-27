//! Staking: add/remove/move stake, conviction locks, childkeys, and root claims.
//!
//! ## Search anchors
//!
//! | Module | Owns |
//! |--------|------|
//! | [`account`] | Hotkey↔coldkey association (`do_try_associate_hotkey`) |
//! | [`add_stake`] | `do_add_stake` / limit variants |
//! | [`remove_stake`] | `do_remove_stake`, unstake-all, dissolve alpha wipe |
//! | [`move_stake`] | Move / transfer / swap stake between hotkeys or subnets |
//! | [`lock`] | Conviction locks, availability, subnet-king |
//! | [`set_children`] | Parent/child hotkey graphs and childkey take |
//! | [`stake_utils`] | Prices, share pools, swaps, stake validation |
//! | [`helpers`] | Stake totals, ownership, nomination cleanup |
//! | [`claim_root`] | Root claimable dividends and auto-claim |
//! | [`increase_take`] / [`decrease_take`] | Delegate take changes |
//! | [`recycle_alpha`] | Recycle / burn alpha into subnet reserves |
//! | [`order_swap`] | Benchmark helpers for stake AMM orders |

use super::*;

pub mod account;
pub mod add_stake;
mod claim_root;
pub mod decrease_take;
pub mod helpers;
pub mod increase_take;
pub mod lock;
pub mod move_stake;
pub mod order_swap;
pub mod recycle_alpha;
pub mod remove_stake;
pub mod set_children;
pub mod stake_utils;
