//! Stake math utilities: prices, share pools, swaps, and extrinsic validation.
//!
//! ## Search anchors
//!
//! | Module | Owns |
//! |--------|------|
//! | [`alpha_price`] | Issuance, moving/median alpha price, TAO weight |
//! | [`inherited_stake`] | Parent/child inherited stake and weight vectors |
//! | [`stake_balances`] | Get / increase / decrease hotkey–coldkey alpha |
//! | [`stake_swap`] | `stake_into_subnet`, `unstake_from_subnet`, AMM swaps |
//! | [`stake_validation`] | `validate_add_stake` / remove / transition |
//! | [`provided_reserves`] | Provided TAO/alpha reserve counters |
//! | [`alpha_share_pool`] | [`HotkeyAlphaSharePoolDataOperations`] |

use super::*;

pub mod alpha_price;
pub mod alpha_share_pool;
pub mod inherited_stake;
pub mod provided_reserves;
pub mod stake_balances;
pub mod stake_swap;
pub mod stake_validation;

pub use alpha_share_pool::HotkeyAlphaSharePoolDataOperations;
