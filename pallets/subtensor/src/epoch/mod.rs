//! Epoch consensus math and per-subnet emission scoring (Yuma).
//!
//! ## Search anchors
//!
//! - [`math`] — fixed-point vector/matrix helpers (normalize, matmul, weighted median, bonds EMA)
//! - [`run_epoch`] — [`run_epoch::epoch_mechanism`], persistence, liquid-alpha bonds
//!
//! Storage vectors written by epoch (`Incentive`, `Bonds`, `Emission`, …) live in the pallet
//! storage map; this module computes and persists them at tempo boundaries.

use super::*;
pub mod math;
pub mod run_epoch;
