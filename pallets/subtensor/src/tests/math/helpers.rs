#![allow(
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::indexing_slicing
)]
//! Shared fixtures and assertions for epoch math unit tests.
//!
//! Re-exported from [`super`] for callers such as `tests/epoch.rs`
//! (`assert_mat_compare`, `vec_to_fixed`, `vec_to_mat_fixed`).

use substrate_fixed::types::{I32F32, I64F64};

pub(super) fn assert_float_compare(a: I32F32, b: I32F32, epsilon: I32F32) {
    assert!(I32F32::abs(a - b) <= epsilon, "a({a:?}) != b({b:?})");
}

pub(super) fn assert_float_compare_64(a: I64F64, b: I64F64, epsilon: I64F64) {
    assert!(I64F64::abs(a - b) <= epsilon, "a({a:?}) != b({b:?})");
}

pub(super) fn assert_vec_compare(va: &[I32F32], vb: &[I32F32], epsilon: I32F32) {
    assert!(va.len() == vb.len());
    for i in 0..va.len() {
        assert_float_compare(va[i], vb[i], epsilon);
    }
}

pub(super) fn assert_vec_compare_64(va: &[I64F64], vb: &[I64F64], epsilon: I64F64) {
    assert!(va.len() == vb.len());
    for i in 0..va.len() {
        assert_float_compare_64(va[i], vb[i], epsilon);
    }
}

pub(super) fn assert_vec_compare_u16(va: &[u16], vb: &[u16]) {
    assert!(va.len() == vb.len());
    for i in 0..va.len() {
        assert_eq!(va[i], vb[i]);
    }
}

pub fn assert_mat_compare(ma: &[Vec<I32F32>], mb: &[Vec<I32F32>], epsilon: I32F32) {
    assert!(ma.len() == mb.len());
    for row in 0..ma.len() {
        assert!(ma[row].len() == mb[row].len());
        for col in 0..ma[row].len() {
            assert_float_compare(ma[row][col], mb[row][col], epsilon)
        }
    }
}

pub(super) fn assert_sparse_mat_compare(
    ma: &[Vec<(u16, I32F32)>],
    mb: &[Vec<(u16, I32F32)>],
    epsilon: I32F32,
) {
    assert!(ma.len() == mb.len());
    for row in 0..ma.len() {
        assert!(
            ma[row].len() == mb[row].len(),
            "row: {}, ma: {:?}, mb: {:?}",
            row,
            ma[row],
            mb[row]
        );
        for j in 0..ma[row].len() {
            assert!(ma[row][j].0 == mb[row][j].0); // u16
            assert_float_compare(ma[row][j].1, mb[row][j].1, epsilon) // I32F32
        }
    }
}

pub fn vec_to_fixed(vector: &[f32]) -> Vec<I32F32> {
    vector.iter().map(|x| I32F32::from_num(*x)).collect()
}

pub(super) fn mat_to_fixed(matrix: &[Vec<f32>]) -> Vec<Vec<I32F32>> {
    matrix.iter().map(|row| vec_to_fixed(row)).collect()
}

pub(super) fn assert_mat_approx_eq(left: &[Vec<I32F32>], right: &[Vec<I32F32>], epsilon: I32F32) {
    assert_eq!(left.len(), right.len());
    for (left_row, right_row) in left.iter().zip(right.iter()) {
        assert_eq!(left_row.len(), right_row.len());
        for (left_val, right_val) in left_row.iter().zip(right_row.iter()) {
            assert!(
                (left_val - right_val).abs() <= epsilon,
                "left: {left_val:?}, right: {right_val:?}"
            );
        }
    }
}

pub fn vec_to_mat_fixed(vector: &[f32], rows: usize, transpose: bool) -> Vec<Vec<I32F32>> {
    assert!(
        vector.len() % rows == 0,
        "Vector of len {:?} cannot reshape to {rows} rows.",
        vector.len()
    );
    let cols: usize = vector.len() / rows;
    let mut mat: Vec<Vec<I32F32>> = vec![];
    if transpose {
        for col in 0..cols {
            let mut vals: Vec<I32F32> = vec![];
            for row in 0..rows {
                vals.push(I32F32::from_num(vector[row * cols + col]));
            }
            mat.push(vals);
        }
    } else {
        for row in 0..rows {
            mat.push(
                vector[row * cols..(row + 1) * cols]
                    .iter()
                    .map(|v| I32F32::from_num(*v))
                    .collect(),
            );
        }
    }
    mat
}

// Reshape vector to sparse matrix with specified number of input rows, cast f32 to I32F32.
pub(super) fn vec_to_sparse_mat_fixed(
    vector: &[f32],
    rows: usize,
    transpose: bool,
) -> Vec<Vec<(u16, I32F32)>> {
    assert!(
        vector.len() % rows == 0,
        "Vector of len {:?} cannot reshape to {rows} rows.",
        vector.len()
    );
    let cols: usize = vector.len() / rows;
    let mut mat: Vec<Vec<(u16, I32F32)>> = vec![];
    if transpose {
        for col in 0..cols {
            let mut row_vec: Vec<(u16, I32F32)> = vec![];
            for row in 0..rows {
                if vector[row * cols + col] > 0. {
                    row_vec.push((row as u16, I32F32::from_num(vector[row * cols + col])));
                }
            }
            mat.push(row_vec);
        }
    } else {
        for row in 0..rows {
            let mut row_vec: Vec<(u16, I32F32)> = vec![];
            for col in 0..cols {
                if vector[row * cols + col] > 0. {
                    row_vec.push((col as u16, I32F32::from_num(vector[row * cols + col])));
                }
            }
            mat.push(row_vec);
        }
    }
    mat
}
