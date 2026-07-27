//! Shared pallet helpers that do not belong to a single feature module.
//!
//! Search anchors:
//! - [`cleanup`] — weight-metered storage cleanup during subnet dissolve / stake wipe
//! - [`evm`] — hotkey↔EVM address association (EIP-191 recover + reverse index)
//! - [`identity`] — coldkey and subnet identity validation / storage writes
//! - [`misc`] — origin guards, admin freeze window, hyperparam getters/setters, Q32 math
//! - [`rate_limiting`] — [`TransactionType`] / [`Hyperparameter`] rate-limit keys
//! - [`voting_power`] — per-subnet validator voting-power EMA tracking
//! - [`try_state`] — try-runtime stake invariants (`try-runtime` feature only)

use super::*;
pub mod cleanup;
pub mod evm;
pub mod identity;
pub mod misc;
pub mod rate_limiting;
#[cfg(feature = "try-runtime")]
pub mod try_state;
pub mod voting_power;
