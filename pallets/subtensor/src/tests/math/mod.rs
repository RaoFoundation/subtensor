#![allow(
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::indexing_slicing
)]
//! Unit tests for [`crate::epoch::math`] fixed-point helpers.
//!
//! Layout mirrors `epoch/math/` so each concept module has a matching test file.
//!
//! ## Search anchors
//!
//! | Module | Owns |
//! |--------|------|
//! | [`helpers`] | assert/compare fixtures; `vec_to_fixed` / `vec_to_mat_fixed` |
//! | [`fixed_conversions`] | u16 ↔ fixed proportions, max-upscale, overflow |
//! | [`vector_ops`] | normalize, top-k, exp/sigmoid, elementwise div |
//! | [`matrix_normalize_mask`] | row/col normalize & boolean masks (dense/sparse) |
//! | [`matmul_clip`] | matmul, Hadamard, column clip |
//! | [`weighted_median`] | stake-weighted median consensus |
//! | [`ema_interpolate`] | bonds EMA, interpolate, vec/mat-vector mul |

mod ema_interpolate;
mod fixed_conversions;
mod helpers;
mod matmul_clip;
mod matrix_normalize_mask;
mod vector_ops;
mod weighted_median;

pub use helpers::{assert_mat_compare, vec_to_fixed, vec_to_mat_fixed};
