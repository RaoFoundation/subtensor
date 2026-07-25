//! Sparse/dense matmul, Hadamard products, and per-column clipping used by Yuma consensus.

use sp_std::vec;
use sp_std::vec::Vec;
use substrate_fixed::types::I32F32;

pub fn row_hadamard(matrix: &[Vec<I32F32>], vector: &[I32F32]) -> Vec<Vec<I32F32>> {
    let Some(first_row) = matrix.first() else {
        return vec![vec![]];
    };
    if first_row.is_empty() {
        return vec![vec![]];
    }

    let mut out = Vec::with_capacity(matrix.len());
    let mut vec_it = vector.iter();

    for row in matrix.iter() {
        let Some(&scale) = vec_it.next() else { break };
        let mut new_row = Vec::with_capacity(row.len());
        for m_val in row.iter() {
            new_row.push(scale.saturating_mul(*m_val));
        }
        out.push(new_row);
    }

    out
}

// Row-wise sparse matrix-vector hadamard product.
pub fn row_hadamard_sparse(
    sparse_matrix: &[Vec<(u16, I32F32)>],
    vector: &[I32F32],
) -> Vec<Vec<(u16, I32F32)>> {
    let mut out = Vec::with_capacity(sparse_matrix.len());
    let mut vec_it = vector.iter();

    for sparse_row in sparse_matrix.iter() {
        let Some(&scale) = vec_it.next() else { break };
        let mut new_row = Vec::with_capacity(sparse_row.len());
        for &(j, val) in sparse_row.iter() {
            new_row.push((j, val.saturating_mul(scale)));
        }
        out.push(new_row);
    }

    out
}

// Row-wise matrix-vector product, column-wise sum: result_j = SUM(i) vector_i * matrix_ij.
pub fn matmul(matrix: &[Vec<I32F32>], vector: &[I32F32]) -> Vec<I32F32> {
    let Some(first_row) = matrix.first() else {
        return vec![];
    };
    let cols = first_row.len();
    if cols == 0 {
        return vec![];
    }
    if matrix.len() != vector.len() {
        log::error!(
            "math error: matmul input sizes are not equal: {:?} != {:?}",
            matrix.len(),
            vector.len()
        );
    }

    let zero = I32F32::saturating_from_num(0.0);
    let mut acc = vec![zero; cols];

    let mut vec_it = vector.iter();
    for row in matrix.iter() {
        // Use 0 if the vector ran out (rows beyond vector length contribute nothing).
        let scale = vec_it.next().copied().unwrap_or(zero);

        let mut acc_it = acc.iter_mut();
        for m_val in row.iter() {
            if let Some(a) = acc_it.next() {
                *a = a.saturating_add(scale.saturating_mul(*m_val));
            } else {
                // Ignore elements beyond the accumulator width (first row’s length).
                break;
            }
        }
    }

    acc
}

// Column-wise matrix-vector product, row-wise sum: result_i = SUM(j) vector_j * matrix_ij.
pub fn matmul_transpose(matrix: &[Vec<I32F32>], vector: &[I32F32]) -> Vec<I32F32> {
    let Some(first_row) = matrix.first() else {
        return vec![];
    };
    if first_row.is_empty() {
        return vec![];
    }
    if vector.len() != first_row.len() {
        log::error!(
            "math error: matmul_transpose matrix width doesn't match to vector height: {:?} != {:?}",
            first_row.len(),
            vector.len()
        );
    }

    let zero = I32F32::saturating_from_num(0.0);
    let mut out = Vec::with_capacity(matrix.len());

    for row in matrix.iter() {
        let mut sum = zero;
        let mut v_it = vector.iter();
        for m in row.iter() {
            if let Some(&v) = v_it.next() {
                sum = sum.saturating_add(m.saturating_mul(v));
            } else {
                break;
            }
        }
        out.push(sum);
    }

    out
}

// Row-wise sparse_matrix-vector product, column-wise sum: result_j = SUM(i) vector_i * matrix_ij.
pub fn matmul_sparse(
    sparse_matrix: &[Vec<(u16, I32F32)>],
    vector: &[I32F32],
    columns: u16,
) -> Vec<I32F32> {
    let zero = I32F32::saturating_from_num(0.0);
    let mut result = vec![zero; columns as usize];

    let mut vec_it = vector.iter();
    for row in sparse_matrix.iter() {
        let scale = vec_it.next().copied().unwrap_or(zero);
        for &(j, val) in row.iter() {
            if let Some(r) = result.get_mut(j as usize) {
                *r = r.saturating_add(scale.saturating_mul(val));
            }
        }
    }

    result
}

// Column-wise sparse_matrix-vector product, row-wise sum: result_i = SUM(j) vector_j * matrix_ij.
pub fn matmul_transpose_sparse(
    sparse_matrix: &[Vec<(u16, I32F32)>],
    vector: &[I32F32],
) -> Vec<I32F32> {
    let zero = I32F32::saturating_from_num(0.0);
    let mut result = vec![zero; sparse_matrix.len()];

    let mut out_it = result.iter_mut();
    for row in sparse_matrix.iter() {
        let Some(out_cell) = out_it.next() else { break };
        let mut acc = zero;
        for &(j, val) in row.iter() {
            let v = vector.get(j as usize).copied().unwrap_or(zero);
            acc = acc.saturating_add(v.saturating_mul(val));
        }
        *out_cell = acc;
    }

    result
}

// Set inplace matrix values above column threshold to threshold value.
pub fn inplace_col_clip(x: &mut [Vec<I32F32>], col_threshold: &[I32F32]) {
    for row in x.iter_mut() {
        let mut thr_it = col_threshold.iter();
        for value in row.iter_mut() {
            if let Some(th) = thr_it.next() {
                // Clip: value = min(value, threshold)
                *value = *th.min(&*value);
            } else {
                // No more thresholds; stop for this row.
                break;
            }
        }
    }
}

// Return sparse matrix with values above column threshold set to threshold value.
pub fn col_clip_sparse(
    sparse_matrix: &[Vec<(u16, I32F32)>],
    col_threshold: &[I32F32],
) -> Vec<Vec<(u16, I32F32)>> {
    let zero = I32F32::saturating_from_num(0.0);
    let mut result = Vec::with_capacity(sparse_matrix.len());

    for row in sparse_matrix.iter() {
        let mut out_row: Vec<(u16, I32F32)> = Vec::with_capacity(row.len());
        for &(j, val) in row.iter() {
            let th = col_threshold.get(j as usize).copied().unwrap_or(zero);
            if th < val {
                if th > zero {
                    // clip down to threshold, but drop if threshold <= 0
                    out_row.push((j, th));
                }
            } else {
                // keep original
                out_row.push((j, val));
            }
        }
        result.push(out_row);
    }

    result
}

// Stake-weighted median score finding algorithm, based on a mid pivot binary search.
// Normally a random pivot is used, but to ensure full determinism the mid point is chosen instead.
// Assumes relatively random score order for efficiency, typically less than O(nlogn) complexity.
//
// # Args:
// 	* 'stake': ( &[I32F32] ):
//         - stake, assumed to be normalized.
//
// 	* 'score': ( &[I32F32] ):
//         - score for which median is sought, 0 <= score <= 1
//
// 	* 'partition_idx' ( &[usize] ):
// 		- indices as input partition
//
// 	* 'minority' ( I32F32 ):
// 		- minority_ratio = 1 - majority_ratio
//
// 	* 'partition_lo' ( I32F32 ):
// 		- lower edge of stake for partition, where partition is a segment [lo, hi] inside stake integral [0, 1].
//
// 	* 'partition_hi' ( I32F32 ):
// 		- higher edge of stake for partition, where partition is a segment [lo, hi] inside stake integral [0, 1].
//
// # Returns:
//     * 'median': ( I32F32 ):
//         - median via random pivot binary search.
//
