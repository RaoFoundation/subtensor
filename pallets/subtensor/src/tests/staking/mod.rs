#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used)]
//! Unit tests for [`crate::staking`] add/remove/move stake, take, helpers, and share pools.
//!
//! Layout mirrors `staking/` so each concept module has a matching test file where practical.
//!
//! ## Search anchors
//!
//! | Module | Owns |
//! |--------|------|
//! | [`add_stake`] | `add_stake` / stake-into-subnet / add-root |
//! | [`add_stake_limit`] | add-limit / max-amount add |
//! | [`remove_stake`] | `remove_stake` core / fees / precision |
//! | [`remove_stake_limit`] | remove-limit / max-amount remove |
//! | [`unstake`] | unstake-all / unstake-from-subnet / full unstake |
//! | [`move_stake`] | max-amount move and move-limit partial |
//! | [`delegate_take`] | increase/decrease take and rate limits |
//! | [`helpers`] | balances, ownership, nominations, delegated totals |
//! | [`stake_utils`] | swap fee correctness and large swaps |
//! | [`sharepool`] | lazy share-pool migration and Alpha data-ops |

mod add_stake;
mod add_stake_limit;
mod delegate_take;
mod helpers;
mod move_stake;
mod remove_stake;
mod remove_stake_limit;
mod sharepool;
mod stake_utils;
mod unstake;
