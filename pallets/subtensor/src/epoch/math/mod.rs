//! Fixed-point linear algebra for Yuma consensus / epoch emission.
//!
//! These helpers operate on `I32F32` / `I64F64` stake-weight and bond matrices produced
//! by [`super::run_epoch`]. Callers outside this module typically `use crate::epoch::math::*`.
//!
//! ## Search anchors
//!
//! | Module | Owns |
//! |--------|------|
//! | [`fixed_conversions`] | u16 ↔ fixed proportions, max-upscale to u16 |
//! | [`vector_ops`] | normalize, top-k, exp/sigmoid, elementwise div |
//! | [`matrix_normalize_mask`] | row/col normalize & boolean masks (dense/sparse) |
//! | [`matmul_clip`] | matmul, Hadamard, column clip |
//! | [`weighted_median`] | stake-weighted median consensus |
//! | [`ema_interpolate`] | bonds EMA, interpolate, [`clamp_i32f32`], [`ln_or_zero`] |

mod ema_interpolate;
mod fixed_conversions;
mod matmul_clip;
mod matrix_normalize_mask;
mod vector_ops;
mod weighted_median;

pub use ema_interpolate::*;
pub use fixed_conversions::*;
pub use matmul_clip::*;
pub use matrix_normalize_mask::*;
pub use vector_ops::*;
pub use weighted_median::*;
