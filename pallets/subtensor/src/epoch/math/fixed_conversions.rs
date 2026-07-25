//! I32F32 / I64F64 / u16 conversion and max-upscale helpers for epoch consensus math.

use safe_math::*;
use sp_std::vec::Vec;
use substrate_fixed::types::{I32F32, I64F64};

/// Index into `slice`, or `T::default()` when out of bounds (used by top-k / median).
pub fn copy_at_or_default<T: Copy + Default>(slice: &[T], idx: usize) -> T {
    slice.get(idx).copied().unwrap_or_default()
}

/// Convert an `f32` literal into epoch `I32F32` fixed-point.
pub fn fixed(val: f32) -> I32F32 {
    I32F32::saturating_from_num(val)
}

pub fn fixed_to_u16(x: I32F32) -> u16 {
    x.saturating_to_num::<u16>()
}

pub fn fixed_to_u64(x: I32F32) -> u64 {
    x.saturating_to_num::<u64>()
}

pub fn fixed64_to_u64(x: I64F64) -> u64 {
    x.saturating_to_num::<u64>()
}

pub fn fixed64_to_fixed32(x: I64F64) -> I32F32 {
    I32F32::saturating_from_num(x)
}

pub fn fixed32_to_fixed64(x: I32F32) -> I64F64 {
    I64F64::saturating_from_num(x)
}

pub fn u16_to_fixed(x: u16) -> I32F32 {
    I32F32::saturating_from_num(x)
}

/// Map a raw `u16` proportion (`0..=u16::MAX`) into `I32F32` in `0..=1`.
pub fn u16_proportion_to_fixed(x: u16) -> I32F32 {
    I32F32::saturating_from_num(x).safe_div(I32F32::saturating_from_num(u16::MAX))
}

/// Scale an `I32F32` absolute value down by `u16::MAX` (bond/weight storage proportion).
pub fn i32f32_as_u16_proportion(x: I32F32) -> I32F32 {
    x.safe_div(I32F32::saturating_from_num(u16::MAX))
}

pub fn fixed_proportion_to_u16(x: I32F32) -> u16 {
    fixed_to_u16(x.saturating_mul(I32F32::saturating_from_num(u16::MAX)))
}

pub fn vec_fixed32_to_u64(vec: Vec<I32F32>) -> Vec<u64> {
    vec.into_iter().map(fixed_to_u64).collect()
}

pub fn vec_fixed64_to_fixed32(vec: Vec<I64F64>) -> Vec<I32F32> {
    vec.into_iter().map(fixed64_to_fixed32).collect()
}

pub fn vec_fixed32_to_fixed64(vec: Vec<I32F32>) -> Vec<I64F64> {
    vec.into_iter().map(fixed32_to_fixed64).collect()
}

pub fn vec_fixed64_to_u64(vec: Vec<I64F64>) -> Vec<u64> {
    vec.into_iter().map(fixed64_to_u64).collect()
}

pub fn vec_fixed_proportions_to_u16(vec: Vec<I32F32>) -> Vec<u16> {
    vec.into_iter().map(fixed_proportion_to_u16).collect()
}

/// Max-upscale a non-negative vector so the max becomes `u16::MAX`, then cast to `u16`.
pub fn vec_max_upscale_to_u16(vec: &[I32F32]) -> Vec<u16> {
    let u16_max: I32F32 = I32F32::saturating_from_num(u16::MAX);
    let threshold: I32F32 = I32F32::saturating_from_num(32768);
    let max_value: Option<&I32F32> = vec.iter().max();
    match max_value {
        Some(val) => {
            if *val == I32F32::saturating_from_num(0) {
                return vec
                    .iter()
                    .map(|e: &I32F32| e.saturating_mul(u16_max).saturating_to_num::<u16>())
                    .collect();
            }
            if *val > threshold {
                return vec
                    .iter()
                    .map(|e: &I32F32| {
                        e.saturating_mul(u16_max.safe_div(*val))
                            .round()
                            .saturating_to_num::<u16>()
                    })
                    .collect();
            }
            vec.iter()
                .map(|e: &I32F32| {
                    e.saturating_mul(u16_max)
                        .safe_div(*val)
                        .round()
                        .saturating_to_num::<u16>()
                })
                .collect()
        }
        None => {
            let sum: I32F32 = vec.iter().sum();
            vec.iter()
                .map(|e: &I32F32| {
                    e.saturating_mul(u16_max)
                        .safe_div(sum)
                        .saturating_to_num::<u16>()
                })
                .collect()
        }
    }
}

/// Max-upscale a `u16` vector so the max becomes `u16::MAX`.
pub fn vec_u16_max_upscale_to_u16(vec: &[u16]) -> Vec<u16> {
    let vec_fixed: Vec<I32F32> = vec
        .iter()
        .map(|e: &u16| I32F32::saturating_from_num(*e))
        .collect();
    vec_max_upscale_to_u16(&vec_fixed)
}
