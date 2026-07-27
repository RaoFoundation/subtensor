//! Proportional stake share pool with controlled-precision [`SafeFloat`] arithmetic.
//!
//! Used by subtensor staking to track each coldkey's claim on a hotkey's alpha
//! (rao) without losing 1-rao precision across large emissions and unstakes.

#![cfg_attr(not(feature = "std"), no_std)]
#![allow(clippy::result_unit_err, clippy::indexing_slicing)]

mod safe_float;
mod share_pool;

pub use safe_float::{SAFE_FLOAT_MAX, SAFE_FLOAT_MAX_EXP, SafeFloat};
pub use share_pool::{SharePool, SharePoolDataOperations};

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
