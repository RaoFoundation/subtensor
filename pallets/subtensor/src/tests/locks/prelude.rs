#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
//! Shared imports for lock unit tests.

pub use approx::assert_abs_diff_eq;
pub use frame_support::dispatch::{GetDispatchInfo, Pays};
pub use frame_support::weights::Weight;
pub use frame_support::{assert_noop, assert_ok};
pub use safe_math::FixedExt;
pub use sp_core::U256;
pub use substrate_fixed::types::U64F64;
pub use subtensor_runtime_common::{AlphaBalance, NetUidStorageIndex, TaoBalance};
pub use subtensor_swap_interface::SwapHandler;

pub use super::super::mock::*;
pub use crate::staking::lock::{ConvictionModel, LockState};
pub use crate::*;
