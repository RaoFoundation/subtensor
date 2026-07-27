//! Bonds EMA (fixed and per-edge alpha), matrix interpolate, and clamp/ln helpers.

use crate::alloc::borrow::ToOwned;
use sp_std::vec;
use sp_std::vec::Vec;
use substrate_fixed::transcendental::ln;
use substrate_fixed::types::I32F32;

pub fn interpolate(mat1: &[Vec<I32F32>], mat2: &[Vec<I32F32>], ratio: I32F32) -> Vec<Vec<I32F32>> {
    if ratio == I32F32::saturating_from_num(0.0) {
        return mat1.to_owned();
    }
    if ratio == I32F32::saturating_from_num(1.0) {
        return mat2.to_owned();
    }
    if mat1.is_empty() || mat1.first().map(|r| r.is_empty()).unwrap_or(true) {
        return vec![vec![]];
    }
    if mat1.len() != mat2.len() {
        log::error!(
            "math error: interpolate mat1.len() != mat2.len(): {:?} != {:?}",
            mat1.len(),
            mat2.len()
        );
    }

    let zero = I32F32::saturating_from_num(0.0);
    let cols = mat1.first().map(|r| r.len()).unwrap_or(0);

    // Pre-size result to mat1's shape (row count = mat1.len(), col count = first row of mat1).
    let mut result: Vec<Vec<I32F32>> = {
        let mut out = Vec::with_capacity(mat1.len());
        for _ in mat1.iter() {
            out.push(vec![zero; cols]);
        }
        out
    };

    // Walk rows of mat1, mat2, and result in lockstep; stop when any iterator ends.
    let mut m2_it = mat2.iter();
    let mut out_it = result.iter_mut();

    for row1 in mat1.iter() {
        let (Some(row2), Some(out_row)) = (m2_it.next(), out_it.next()) else {
            log::error!("math error: interpolate: No more rows in mat2");
            break;
        };
        if row1.len() != row2.len() {
            log::error!(
                "math error: interpolate row1.len() != row2.len(): {:?} != {:?}",
                row1.len(),
                row2.len()
            );
        }

        // Walk elements of row1, row2, and out_row in lockstep; stop at the shortest.
        let mut r1_it = row1.iter();
        let mut r2_it = row2.iter();
        let mut out_cell_it = out_row.iter_mut();

        while let (Some(v1), Some(v2), Some(out_cell)) =
            (r1_it.next(), r2_it.next(), out_cell_it.next())
        {
            *out_cell = (*v1).saturating_add(ratio.saturating_mul((*v2).saturating_sub(*v1)));
        }
        // Any remaining cells in `out_row` (beyond min row length) stay as zero (pre-filled).
    }

    result
}

// Element-wise interpolation of two sparse matrices: Result = A + ratio * (B - A).
// ratio has intended range [0, 1]
// ratio=0: Result = A
// ratio=1: Result = B
pub fn interpolate_sparse(
    mat1: &[Vec<(u16, I32F32)>],
    mat2: &[Vec<(u16, I32F32)>],
    columns: u16,
    ratio: I32F32,
) -> Vec<Vec<(u16, I32F32)>> {
    if ratio == I32F32::saturating_from_num(0) {
        return mat1.to_owned();
    }
    if ratio == I32F32::saturating_from_num(1) {
        return mat2.to_owned();
    }
    if mat1.len() != mat2.len() {
        // In case if sizes mismatch, return clipped weights
        log::error!(
            "math error: interpolate_sparse: mat1.len() != mat2.len(): {:?} != {:?}",
            mat1.len(),
            mat2.len()
        );
        return mat2.to_owned();
    }
    let rows = mat1.len();
    let zero: I32F32 = I32F32::saturating_from_num(0);
    let mut result: Vec<Vec<(u16, I32F32)>> = vec![vec![]; rows];
    for i in 0..rows {
        let mut row1: Vec<I32F32> = vec![zero; columns as usize];
        if let Some(row) = mat1.get(i) {
            for (j, value) in row {
                if let Some(entry) = row1.get_mut(*j as usize) {
                    *entry = *value;
                }
            }
        }
        let mut row2: Vec<I32F32> = vec![zero; columns as usize];
        if let Some(row) = mat2.get(i) {
            for (j, value) in row {
                if let Some(entry) = row2.get_mut(*j as usize) {
                    *entry = *value;
                }
            }
        }
        for j in 0..columns as usize {
            let v1 = row1.get(j).unwrap_or(&zero);
            let v2 = row2.get(j).unwrap_or(&zero);
            let interp = v1.saturating_add(ratio.saturating_mul(v2.saturating_sub(*v1)));
            if zero < interp
                && let Some(res) = result.get_mut(i)
            {
                res.push((j as u16, interp));
            }
        }
    }
    result
}

// Element-wise product of two vectors.
pub fn vec_mul(a: &[I32F32], b: &[I32F32]) -> Vec<I32F32> {
    let mut out = Vec::with_capacity(core::cmp::min(a.len(), b.len()));
    let mut ai = a.iter();
    let mut bi = b.iter();

    while let (Some(x), Some(y)) = (ai.next(), bi.next()) {
        out.push(x.checked_mul(*y).unwrap_or_default());
    }

    out
}

// Element-wise product of matrix and vector
pub fn mat_vec_mul(matrix: &[Vec<I32F32>], vector: &[I32F32]) -> Vec<Vec<I32F32>> {
    let Some(first_row) = matrix.first() else {
        return vec![vec![]];
    };
    if first_row.is_empty() {
        return vec![vec![]];
    }

    let mut out = Vec::with_capacity(matrix.len());
    for row in matrix.iter() {
        out.push(vec_mul(row, vector));
    }
    out
}

// Element-wise product of matrix and vector
pub fn mat_vec_mul_sparse(
    matrix: &[Vec<(u16, I32F32)>],
    vector: &[I32F32],
) -> Vec<Vec<(u16, I32F32)>> {
    let mut result: Vec<Vec<(u16, I32F32)>> = vec![vec![]; matrix.len()];
    for (i, matrix_row) in matrix.iter().enumerate() {
        for (j, value) in matrix_row.iter() {
            if let Some(vector_value) = vector.get(*j as usize) {
                let new_value = value.saturating_mul(*vector_value);
                if new_value != I32F32::saturating_from_num(0.0)
                    && let Some(result_row) = result.get_mut(i)
                {
                    result_row.push((*j, new_value));
                }
            }
        }
    }
    result
}

/// Clamp `value` into `[low, high]` (assumes `high > low`).
pub fn clamp_i32f32(value: I32F32, low: I32F32, high: I32F32) -> I32F32 {
    // First, clamp the value to ensure it does not exceed the upper bound (high).
    // If the value is greater than 'high', it will be set to 'high'.
    // otherwise it remains unchanged.
    value
        .min(I32F32::from_num(high))
        // Next, clamp the value to ensure it does not go below the lower bound (_low).
        // If the value (after the first clamping) is less than 'low', it will be set to 'low'.
        // otherwise it remains unchanged.
        .max(I32F32::from_num(low))
}

// Return matrix exponential moving average: `alpha * a_ij + one_minus_alpha * b_ij`.
// `alpha` is the EMA coefficient, how much to add of the new observation, typically small,
// higher alpha discounts older observations faster.
pub fn mat_ema(new: &[Vec<I32F32>], old: &[Vec<I32F32>], alpha: I32F32) -> Vec<Vec<I32F32>> {
    let Some(first_row) = new.first() else {
        return vec![vec![]];
    };
    if first_row.is_empty() {
        return vec![vec![]; 1];
    }

    let one_minus_alpha = I32F32::saturating_from_num(1.0).saturating_sub(alpha);

    let mut out = Vec::with_capacity(new.len());
    let mut old_it = old.iter();

    for new_row in new.iter() {
        let Some(old_row) = old_it.next() else { break };

        let mut row_out = Vec::with_capacity(core::cmp::min(new_row.len(), old_row.len()));
        let mut n_it = new_row.iter();
        let mut o_it = old_row.iter();

        while let (Some(&n), Some(&o)) = (n_it.next(), o_it.next()) {
            row_out.push(
                alpha
                    .saturating_mul(n)
                    .saturating_add(one_minus_alpha.saturating_mul(o)),
            );
        }

        out.push(row_out);
    }

    out
}

// Return sparse matrix exponential moving average: `alpha * a_ij + one_minus_alpha * b_ij`.
// `alpha` is the EMA coefficient, how much to add of the new observation, typically small,
// higher alpha discounts older observations faster.
pub fn mat_ema_sparse(
    new: &[Vec<(u16, I32F32)>],
    old: &[Vec<(u16, I32F32)>],
    alpha: I32F32,
) -> Vec<Vec<(u16, I32F32)>> {
    if new.len() != old.len() {
        log::error!(
            "math error: mat_ema_sparse: new.len() == old.len(): {:?} != {:?}",
            new.len(),
            old.len()
        );
    }

    let zero = I32F32::saturating_from_num(0.0);
    let one_minus_alpha = I32F32::saturating_from_num(1.0).saturating_sub(alpha);

    let n = new.len(); // assume square (rows = cols)
    if n == 0 {
        return Vec::new();
    }

    let mut result: Vec<Vec<(u16, I32F32)>> = Vec::with_capacity(n);
    let mut old_it = old.iter();

    for new_row in new.iter() {
        let mut acc_row = vec![zero; n];

        // Add alpha * new
        for &(j, v) in new_row.iter() {
            if let Some(cell) = acc_row.get_mut(j as usize) {
                *cell = cell.saturating_add(alpha.saturating_mul(v));
            }
        }

        // Add (1 - alpha) * old
        if let Some(orow) = old_it.next() {
            for &(j, v) in orow.iter() {
                if let Some(cell) = acc_row.get_mut(j as usize) {
                    *cell = cell.saturating_add(one_minus_alpha.saturating_mul(v));
                }
            }
        }

        // Densified row -> sparse (keep positives)
        let mut out_row: Vec<(u16, I32F32)> = Vec::new();
        for (j, &val) in acc_row.iter().enumerate() {
            if val > zero {
                out_row.push((j as u16, val));
            }
        }

        result.push(out_row);
    }

    result
}

/// Calculates the exponential moving average (EMA) for a sparse matrix using dynamic alpha values.
pub fn mat_ema_alpha_sparse(
    new: &[Vec<(u16, I32F32)>],
    old: &[Vec<(u16, I32F32)>],
    alpha: &[Vec<I32F32>],
) -> Vec<Vec<(u16, I32F32)>> {
    // If shapes don't match, just return `new`
    if new.len() != old.len() || new.len() != alpha.len() {
        log::error!(
            "math error: mat_ema_alpha_sparse shapes don't match: {:?} vs. {:?} vs. {:?}",
            old.len(),
            new.len(),
            alpha.len()
        );
        return new.to_owned();
    }

    let zero = I32F32::saturating_from_num(0.0);
    let one = I32F32::saturating_from_num(1.0);

    let mut result: Vec<Vec<(u16, I32F32)>> = Vec::with_capacity(new.len());
    let mut old_it = old.iter();
    let mut alf_it = alpha.iter();

    for new_row in new.iter() {
        let Some(old_row) = old_it.next() else { break };
        let Some(alpha_row) = alf_it.next() else {
            break;
        };

        // Densified accumulator sized to alpha_row length (columns outside are ignored).
        let mut decayed_values = vec![zero; alpha_row.len()];

        // Apply (1 - alpha_j) * old_ij into accumulator.
        for &(j, old_val) in old_row.iter() {
            if let (Some(&a), Some(cell)) = (
                alpha_row.get(j as usize),
                decayed_values.get_mut(j as usize),
            ) {
                *cell = one.saturating_sub(a).saturating_mul(old_val);
            }
        }

        // Add alpha_j * new_ij, clamp to [0, 1], and emit sparse entries > 0.
        let mut out_row: Vec<(u16, I32F32)> = Vec::new();
        for &(j, new_val) in new_row.iter() {
            if let (Some(&a), Some(&decayed)) =
                (alpha_row.get(j as usize), decayed_values.get(j as usize))
            {
                let inc = a.saturating_mul(new_val).max(zero);
                let val = decayed.saturating_add(inc).min(one);
                if val > zero {
                    out_row.push((j, val));
                }
            }
        }

        result.push(out_row);
    }

    result
}

/// Calculates the exponential moving average (EMA) for a dense matrix using dynamic alpha values.
pub fn mat_ema_alpha(
    new: &[Vec<I32F32>], // Weights
    old: &[Vec<I32F32>], // Bonds
    alpha: &[Vec<I32F32>],
) -> Vec<Vec<I32F32>> {
    // Empty or degenerate input
    if new.is_empty() || new.first().map(|r| r.is_empty()).unwrap_or(true) {
        return vec![vec![]];
    }

    // If outer dimensions don't match, return bonds unchanged
    if new.len() != old.len() || new.len() != alpha.len() {
        log::error!(
            "math error: mat_ema_alpha shapes don't match: {:?} vs. {:?} vs. {:?}",
            old.len(),
            new.len(),
            alpha.len()
        );
        return old.to_owned();
    }

    // Ensure each corresponding row has matching length; otherwise return `new` unchanged.
    let mut old_it = old.iter();
    let mut alp_it = alpha.iter();
    for nrow in new.iter() {
        let (Some(orow), Some(arow)) = (old_it.next(), alp_it.next()) else {
            return new.to_owned();
        };
        if nrow.len() != orow.len() || nrow.len() != arow.len() {
            return new.to_owned();
        }
    }

    let zero = I32F32::saturating_from_num(0.0);
    let one = I32F32::saturating_from_num(1.0);

    // Compute EMA: result = (1 - α) * old + α * new, clamped to [0, 1].
    let mut out: Vec<Vec<I32F32>> = Vec::with_capacity(new.len());
    let mut old_it = old.iter();
    let mut alp_it = alpha.iter();

    for nrow in new.iter() {
        let (Some(orow), Some(arow)) = (old_it.next(), alp_it.next()) else {
            break;
        };

        let mut r: Vec<I32F32> = Vec::with_capacity(nrow.len());
        let mut n_it = nrow.iter();
        let mut o_it = orow.iter();
        let mut a_it = arow.iter();

        while let (Some(&n), Some(&o), Some(&a)) = (n_it.next(), o_it.next(), a_it.next()) {
            let one_minus_a = one.saturating_sub(a);
            let decayed = one_minus_a.saturating_mul(o);
            let inc = a.saturating_mul(n).max(zero);
            r.push(decayed.saturating_add(inc).min(one));
        }

        out.push(r);
    }

    out
}

/// Natural log for positive `I32F32`; returns 0 when `value` is 0.
pub fn ln_or_zero(value: I32F32) -> I32F32 {
    ln(value).unwrap_or(I32F32::saturating_from_num(0.0))
}
