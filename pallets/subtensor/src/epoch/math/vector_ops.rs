//! Vector sum/normalize, top-k masks, and clamped exp/sigmoid for epoch scoring.

use super::copy_at_or_default;
use safe_math::*;
use sp_runtime::traits::CheckedAdd;
use sp_std::vec;
use sp_std::vec::Vec;
use substrate_fixed::transcendental::exp;
use substrate_fixed::types::{I32F32, I64F64};

/// After normalizing `vec` as proportions, true iff no entry exceeds `max_limit / u16::MAX`.
pub fn check_vec_max_limited(vec: &[u16], max_limit: u16) -> bool {
    let max_limit_fixed: I32F32 =
        I32F32::saturating_from_num(max_limit).safe_div(I32F32::saturating_from_num(u16::MAX));
    let mut vec_fixed: Vec<I32F32> = vec
        .iter()
        .map(|e: &u16| I32F32::saturating_from_num(*e))
        .collect();
    inplace_normalize(&mut vec_fixed);
    let max_value: Option<&I32F32> = vec_fixed.iter().max();
    max_value.is_none_or(|v| *v <= max_limit_fixed)
}

pub fn sum(x: &[I32F32]) -> I32F32 {
    x.iter().sum()
}

// Sums a Vector of type that has CheckedAdd trait.
// Returns None if overflow occurs during sum using T::checked_add.
// Returns Some(T::default()) if input vector is empty.
pub fn checked_sum<T>(x: &[T]) -> Option<T>
where
    T: Copy + Default + CheckedAdd,
{
    let mut iter = x.iter();
    let Some(mut sum) = iter.next().copied() else {
        return Some(T::default());
    };
    for i in iter {
        sum = sum.checked_add(i)?;
    }
    Some(sum)
}

// Return true when vector sum is zero.
pub fn is_zero(vector: &[I32F32]) -> bool {
    let vector_sum: I32F32 = sum(vector);
    vector_sum == I32F32::saturating_from_num(0)
}

/// `exp(input)` with input clamped to `[-20, 20]` to avoid fixed-point overflow.
pub fn exp_safe(input: I32F32) -> I32F32 {
    let min_input: I32F32 = I32F32::saturating_from_num(-20); // <= 1/exp(-20) = 485 165 195,4097903
    let max_input: I32F32 = I32F32::saturating_from_num(20); // <= exp(20) = 485 165 195,4097903
    let mut safe_input: I32F32 = input;
    if input < min_input {
        safe_input = min_input;
    } else if max_input < input {
        safe_input = max_input;
    }
    let output: I32F32;
    match exp(safe_input) {
        Ok(val) => {
            output = val;
        }
        Err(_err) => {
            if safe_input <= 0 {
                output = I32F32::saturating_from_num(0);
            } else {
                output = I32F32::max_value();
            }
        }
    }
    output
}

/// Consensus sigmoid: `1 / (1 + exp(-rho * (input - kappa)))` using [`exp_safe`].
pub fn sigmoid_safe(input: I32F32, rho: I32F32, kappa: I32F32) -> I32F32 {
    let one: I32F32 = I32F32::saturating_from_num(1);
    let offset: I32F32 = input.saturating_sub(kappa); // (input - kappa)
    let neg_rho: I32F32 = rho.saturating_mul(one.saturating_neg()); // -rho
    let exp_input: I32F32 = neg_rho.saturating_mul(offset); // -rho*(input-kappa)
    let exp_output: I32F32 = exp_safe(exp_input); // exp(-rho*(input-kappa))
    let denominator: I32F32 = exp_output.saturating_add(one); // 1 + exp(-rho*(input-kappa))
    let sigmoid_output: I32F32 = one.safe_div(denominator); // 1 / (1 + exp(-rho*(input-kappa)))
    sigmoid_output
}

// Returns a bool vector where an item is true if the vector item is in topk values.
pub fn is_topk(vector: &[I32F32], k: usize) -> Vec<bool> {
    let n: usize = vector.len();
    let mut result: Vec<bool> = vec![true; n];
    if n < k {
        return result;
    }
    let mut idxs: Vec<usize> = (0..n).collect();
    idxs.sort_by_key(|&idx| copy_at_or_default(vector, idx)); // ascending stable sort
    for &idx in idxs.iter().take(n.saturating_sub(k)) {
        if let Some(cell) = result.get_mut(idx) {
            *cell = false;
        }
    }
    result
}

// Returns a bool vector where an item is true if the vector item is in topk values and is non-zero.
pub fn is_topk_nonzero_i32f32(vector: &[I32F32], k: usize) -> Vec<bool> {
    let n: usize = vector.len();
    let mut result: Vec<bool> = vector.iter().map(|&elem| elem != I32F32::from(0)).collect();
    if n < k {
        return result;
    }
    let mut idxs: Vec<usize> = (0..n).collect();
    idxs.sort_by_key(|&idx| copy_at_or_default(vector, idx)); // ascending stable sort
    for &idx in idxs.iter().take(n.saturating_sub(k)) {
        if let Some(cell) = result.get_mut(idx) {
            *cell = false;
        }
    }
    result
}

// Returns a normalized (sum to 1 except 0) copy of the input vector.
pub fn normalize(x: &[I32F32]) -> Vec<I32F32> {
    let x_sum: I32F32 = sum(x);
    if x_sum != I32F32::saturating_from_num(0.0_f32) {
        x.iter().map(|xi| xi.safe_div(x_sum)).collect()
    } else {
        x.to_vec()
    }
}

// Normalizes (sum to 1 except 0) the input vector directly in-place.
pub fn inplace_normalize(x: &mut [I32F32]) {
    let x_sum: I32F32 = x.iter().sum();
    if x_sum == I32F32::saturating_from_num(0.0_f32) {
        return;
    }
    x.iter_mut()
        .for_each(|value| *value = value.safe_div(x_sum));
}

// Normalizes (sum to 1 except 0) the input vector directly in-place, using the sum arg.
pub fn inplace_normalize_i32f32_with_sum(x: &mut [I32F32], x_sum: I32F32) {
    if x_sum == I32F32::saturating_from_num(0.0_f32) {
        return;
    }
    x.iter_mut()
        .for_each(|value| *value = value.safe_div(x_sum));
}

// Normalizes (sum to 1 except 0) the I64F64 input vector directly in-place.
pub fn inplace_normalize_64(x: &mut [I64F64]) {
    let x_sum: I64F64 = x.iter().sum();
    if x_sum == I64F64::saturating_from_num(0) {
        return;
    }
    x.iter_mut()
        .for_each(|value| *value = value.safe_div(x_sum));
}

/// Normalizes (sum to 1 except 0) each row (dim=0) of a I64F64 matrix in-place.
pub fn inplace_row_normalize_64(x: &mut [Vec<I64F64>]) {
    for row in x {
        let row_sum: I64F64 = row.iter().sum();
        if row_sum > I64F64::saturating_from_num(0.0_f64) {
            row.iter_mut()
                .for_each(|x_ij: &mut I64F64| *x_ij = x_ij.safe_div(row_sum));
        }
    }
}

/// Returns x / y for input vectors x and y, if y == 0 return 0.
pub fn vecdiv(x: &[I32F32], y: &[I32F32]) -> Vec<I32F32> {
    if x.len() != y.len() {
        log::error!(
            "math error: vecdiv input lengths are not equal: {:?} != {:?}",
            x.len(),
            y.len()
        );
    }

    let zero = I32F32::saturating_from_num(0);

    let mut out = Vec::with_capacity(x.len());
    for (i, x_i) in x.iter().enumerate() {
        let y_i = y.get(i).copied().unwrap_or(zero);
        out.push(x_i.safe_div(y_i));
    }
    out
}
