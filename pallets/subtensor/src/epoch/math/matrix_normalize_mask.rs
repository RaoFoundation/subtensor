//! Row/column normalize, max-upscale, and boolean masks for dense/sparse weight matrices.

use safe_math::*;
use sp_std::vec;
use sp_std::vec::Vec;
use substrate_fixed::types::I32F32;

/// Normalizes (sum to 1 except 0) each row (dim=0) of a matrix in-place.
pub fn inplace_row_normalize(x: &mut [Vec<I32F32>]) {
    for row in x {
        let row_sum: I32F32 = row.iter().sum();
        if row_sum > I32F32::saturating_from_num(0.0_f32) {
            row.iter_mut()
                .for_each(|x_ij: &mut I32F32| *x_ij = x_ij.safe_div(row_sum));
        }
    }
}

// Normalizes (sum to 1 except 0) each row (dim=0) of a sparse matrix in-place.
pub fn inplace_row_normalize_sparse(sparse_matrix: &mut [Vec<(u16, I32F32)>]) {
    for sparse_row in sparse_matrix.iter_mut() {
        let row_sum: I32F32 = sparse_row.iter().map(|(_j, value)| *value).sum();
        if row_sum > I32F32::saturating_from_num(0.0) {
            sparse_row
                .iter_mut()
                .for_each(|(_j, value)| *value = value.safe_div(row_sum));
        }
    }
}

// Sum across each row (dim=0) of a matrix.
pub fn row_sum(x: &[Vec<I32F32>]) -> Vec<I32F32> {
    if let Some(first_row) = x.first()
        && first_row.is_empty()
    {
        return vec![];
    }
    x.iter().map(|row| row.iter().sum()).collect()
}

// Sum across each row (dim=0) of a sparse matrix.
pub fn row_sum_sparse(sparse_matrix: &[Vec<(u16, I32F32)>]) -> Vec<I32F32> {
    sparse_matrix
        .iter()
        .map(|row| row.iter().map(|(_, value)| value).sum())
        .collect()
}

// Normalizes (sum to 1 except 0) each column (dim=1) of a sparse matrix in-place.
pub fn inplace_col_normalize_sparse(sparse_matrix: &mut [Vec<(u16, I32F32)>], columns: u16) {
    let zero = I32F32::saturating_from_num(0.0);
    let mut col_sum: Vec<I32F32> = vec![zero; columns as usize];

    // Pass 1: accumulate column sums.
    for sparse_row in sparse_matrix.iter() {
        for &(j, value) in sparse_row.iter() {
            if let Some(sum) = col_sum.get_mut(j as usize) {
                *sum = sum.saturating_add(value);
            }
        }
    }

    // Pass 2: normalize by column sums where non-zero.
    for sparse_row in sparse_matrix.iter_mut() {
        for (j, value) in sparse_row.iter_mut() {
            let denom = col_sum.get(*j as usize).copied().unwrap_or(zero);
            if denom != zero {
                *value = value.safe_div(denom);
            }
        }
    }
}

// Normalizes (sum to 1 except 0) each column (dim=1) of a matrix in-place.
// If a row is shorter/longer than the accumulator, pad with zeroes accordingly.
pub fn inplace_col_normalize(x: &mut [Vec<I32F32>]) {
    let zero = I32F32::saturating_from_num(0.0);

    // Build column sums; treat missing entries as zero, but don't modify rows.
    let mut col_sums: Vec<I32F32> = Vec::new();
    for row in x.iter() {
        if col_sums.len() < row.len() {
            col_sums.resize(row.len(), zero);
        }
        let mut sums_it = col_sums.iter_mut();
        for v in row.iter() {
            if let Some(sum) = sums_it.next() {
                *sum = sum.saturating_add(*v);
            } else {
                break;
            }
        }
    }

    if col_sums.is_empty() {
        return;
    }

    // Normalize only existing elements in each row.
    for row in x.iter_mut() {
        let mut sums_it = col_sums.iter();
        for m in row.iter_mut() {
            if let Some(sum) = sums_it.next() {
                if *sum != zero {
                    *m = m.safe_div(*sum);
                }
            } else {
                break;
            }
        }
    }
}

// Max-upscale each column (dim=1) of a sparse matrix in-place.
pub fn inplace_col_max_upscale_sparse(sparse_matrix: &mut [Vec<(u16, I32F32)>], columns: u16) {
    let zero = I32F32::saturating_from_num(0.0);
    let mut col_max: Vec<I32F32> = vec![zero; columns as usize];

    // Pass 1: compute per-column max
    for sparse_row in sparse_matrix.iter() {
        for (j, value) in sparse_row.iter() {
            if let Some(m) = col_max.get_mut(*j as usize)
                && *m < *value
            {
                *m = *value;
            }
        }
    }

    // Pass 2: divide each nonzero entry by its column max
    for sparse_row in sparse_matrix.iter_mut() {
        for (j, value) in sparse_row.iter_mut() {
            let m = col_max.get(*j as usize).copied().unwrap_or(zero);
            if m != zero {
                *value = value.safe_div(m);
            }
        }
    }
}

// Max-upscale each column (dim=1) of a matrix in-place.
pub fn inplace_col_max_upscale(x: &mut [Vec<I32F32>]) {
    let zero = I32F32::saturating_from_num(0.0);

    // Find the widest row to size the column-max buffer; don't modify rows.
    let max_cols = x.iter().map(|r| r.len()).max().unwrap_or(0);
    if max_cols == 0 {
        return;
    }

    // Pass 1: compute per-column maxima across existing entries only.
    let mut col_maxes = vec![zero; max_cols];
    for row in x.iter() {
        let mut max_it = col_maxes.iter_mut();
        for v in row.iter() {
            if let Some(m) = max_it.next() {
                if *m < *v {
                    *m = *v;
                }
            } else {
                break;
            }
        }
    }

    // Pass 2: divide each existing entry by its column max (if non-zero).
    for row in x.iter_mut() {
        let mut max_it = col_maxes.iter();
        for val in row.iter_mut() {
            if let Some(&m) = max_it.next() {
                if m != zero {
                    *val = val.safe_div(m);
                }
            } else {
                break;
            }
        }
    }
}

// Apply mask to vector, mask=true will mask out, i.e. set to 0.
pub fn inplace_mask_vector(mask: &[bool], vector: &mut [I32F32]) {
    if mask.len() != vector.len() {
        log::error!(
            "math error: inplace_mask_vector input lengths are not equal: {:?} != {:?}",
            mask.len(),
            vector.len()
        );
    }

    if mask.is_empty() {
        return;
    }
    let zero: I32F32 = I32F32::saturating_from_num(0.0);
    for (i, v) in vector.iter_mut().enumerate() {
        if *mask.get(i).unwrap_or(&true) {
            *v = zero;
        }
    }
}

// Apply mask to matrix, mask=true will mask out, i.e. set to 0.
pub fn inplace_mask_matrix(mask: &[Vec<bool>], matrix: &mut [Vec<I32F32>]) {
    if mask.len() != matrix.len() {
        log::error!(
            "math error: inplace_mask_matrix input sizes are not equal: {:?} != {:?}",
            mask.len(),
            matrix.len()
        );
    }
    let Some(first_row) = mask.first() else {
        return;
    };
    if first_row.is_empty() {
        return;
    }
    let zero: I32F32 = I32F32::saturating_from_num(0.0);
    for (r, row) in matrix.iter_mut().enumerate() {
        let mask_row_opt = mask.get(r);
        for (c, val) in row.iter_mut().enumerate() {
            let should_zero = mask_row_opt
                .and_then(|mr| mr.get(c))
                .copied()
                .unwrap_or(true);
            if should_zero {
                *val = zero;
            }
        }
    }
}

// Apply row mask to matrix, mask=true will mask out, i.e. set to 0.
pub fn inplace_mask_rows(mask: &[bool], matrix: &mut [Vec<I32F32>]) {
    if mask.len() != matrix.len() {
        log::error!(
            "math error: inplace_mask_rows input sizes are not equal: {:?} != {:?}",
            mask.len(),
            matrix.len()
        );
    }
    let Some(first_row) = matrix.first() else {
        return;
    };
    let cols = first_row.len();
    let zero: I32F32 = I32F32::saturating_from_num(0);
    for (r, row) in matrix.iter_mut().enumerate() {
        if mask.get(r).copied().unwrap_or(true) {
            *row = vec![zero; cols];
        }
    }
}

// Apply column mask to matrix, mask=true will mask out, i.e. set to 0.
// Assumes each column has the same length.
pub fn inplace_mask_cols(mask: &[bool], matrix: &mut [Vec<I32F32>]) {
    if mask.len() != matrix.len() {
        log::error!(
            "math error: inplace_mask_cols input sizes are not equal: {:?} != {:?}",
            mask.len(),
            matrix.len()
        );
    }
    if matrix.is_empty() {
        return;
    };
    let zero: I32F32 = I32F32::saturating_from_num(0);
    for row in matrix.iter_mut() {
        for (c, elem) in row.iter_mut().enumerate() {
            if mask.get(c).copied().unwrap_or(true) {
                *elem = zero;
            }
        }
    }
}

// Mask out the diagonal of the input matrix in-place.
pub fn inplace_mask_diag(matrix: &mut [Vec<I32F32>]) {
    let Some(first_row) = matrix.first() else {
        return;
    };
    if first_row.is_empty() {
        return;
    }
    // Weights that we use this function for are always a square matrix.
    // If something not square is passed to this function, it's safe to return
    // with no action. Log error if this happens.
    if matrix.len() != first_row.len() {
        log::error!(
            "math error: inplace_mask_diag: matrix.len {:?} != first_row.len {:?}",
            matrix.len(),
            first_row.len()
        );
        return;
    }

    let zero: I32F32 = I32F32::saturating_from_num(0.0);
    matrix.iter_mut().enumerate().for_each(|(idx, row)| {
        let Some(elem) = row.get_mut(idx) else {
            // Should not happen since matrix is square
            return;
        };
        *elem = zero;
    });
}

// Remove cells from sparse matrix where the mask function of a scalar and a vector is true.
pub fn scalar_vec_mask_sparse_matrix(
    sparse_matrix: &[Vec<(u16, I32F32)>],
    scalar: u64,
    vector: &[u64],
    mask_fn: &dyn Fn(u64, u64) -> bool,
) -> Vec<Vec<(u16, I32F32)>> {
    let mut result: Vec<Vec<(u16, I32F32)>> = Vec::with_capacity(sparse_matrix.len());

    for row in sparse_matrix.iter() {
        let mut out_row: Vec<(u16, I32F32)> = Vec::with_capacity(row.len());
        for &(j, value) in row.iter() {
            let vj = vector.get(j as usize).copied().unwrap_or(0);
            if !mask_fn(scalar, vj) {
                out_row.push((j, value));
            }
        }
        result.push(out_row);
    }

    result
}

// Mask out the diagonal of the input matrix in-place, except for the diagonal entry at except_index.
pub fn inplace_mask_diag_except_index(matrix: &mut [Vec<I32F32>], except_index: u16) {
    let Some(first_row) = matrix.first() else {
        return;
    };
    if first_row.is_empty() {
        return;
    }
    if matrix.len() != first_row.len() {
        log::error!(
            "math error: inplace_mask_diag input matrix is now square: {:?} != {:?}",
            matrix.len(),
            first_row.len()
        );
        return;
    }
    let diag_at_index = matrix
        .get(except_index as usize)
        .and_then(|row| row.get(except_index as usize))
        .cloned();

    inplace_mask_diag(matrix);

    matrix.get_mut(except_index as usize).map(|row| {
        row.get_mut(except_index as usize).map(|value| {
            if let Some(diag_at_index) = diag_at_index {
                *value = diag_at_index;
            }
        })
    });
}

// Return a new sparse matrix that replaces masked rows with an empty vector placeholder.
pub fn mask_rows_sparse(
    mask: &[bool],
    sparse_matrix: &[Vec<(u16, I32F32)>],
) -> Vec<Vec<(u16, I32F32)>> {
    let mut out = Vec::with_capacity(sparse_matrix.len());
    for (i, sparse_row) in sparse_matrix.iter().enumerate() {
        if mask.get(i).copied().unwrap_or(true) {
            out.push(Vec::new());
        } else {
            out.push(sparse_row.clone());
        }
    }
    out
}

// Return a new sparse matrix with a masked out diagonal of input sparse matrix.
pub fn mask_diag_sparse(sparse_matrix: &[Vec<(u16, I32F32)>]) -> Vec<Vec<(u16, I32F32)>> {
    sparse_matrix
        .iter()
        .enumerate()
        .map(|(i, sparse_row)| {
            sparse_row
                .iter()
                .filter(|(j, _)| i != (*j as usize))
                .copied()
                .collect()
        })
        .collect()
}

// Return a new sparse matrix with a masked out diagonal of input sparse matrix,
// except for the diagonal entry at except_index.
pub fn mask_diag_sparse_except_index(
    sparse_matrix: &[Vec<(u16, I32F32)>],
    except_index: u16,
) -> Vec<Vec<(u16, I32F32)>> {
    sparse_matrix
        .iter()
        .enumerate()
        .map(|(i, sparse_row)| {
            sparse_row
                .iter()
                .filter(|(j, _)| {
                    // Is not a diagonal OR is the diagonal at except_index
                    i != (*j as usize) || (i == except_index as usize && *j == except_index)
                })
                .copied()
                .collect()
        })
        .collect()
}

// Remove cells from sparse matrix where the mask function of two vectors is true.
pub fn vec_mask_sparse_matrix(
    sparse_matrix: &[Vec<(u16, I32F32)>],
    first_vector: &[u64],
    second_vector: &[u64],
    mask_fn: &dyn Fn(u64, u64) -> bool,
) -> Vec<Vec<(u16, I32F32)>> {
    let mut result: Vec<Vec<(u16, I32F32)>> = Vec::with_capacity(sparse_matrix.len());
    let mut fv_it = first_vector.iter();
    for row in sparse_matrix.iter() {
        let fv = fv_it.next().copied().unwrap_or(0);
        let mut out_row: Vec<(u16, I32F32)> = Vec::with_capacity(row.len());
        for &(j, val) in row.iter() {
            let sv = second_vector.get(j as usize).copied().unwrap_or(0);
            if !mask_fn(fv, sv) {
                out_row.push((j, val));
            }
        }
        result.push(out_row);
    }
    result
}

// Row-wise matrix-vector hadamard product.
