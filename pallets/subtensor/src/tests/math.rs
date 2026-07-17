#![allow(
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::indexing_slicing
)]
use substrate_fixed::types::{I32F32, I64F64};

use crate::epoch::math::*;
use rand::{RngExt, seq::SliceRandom};
use substrate_fixed::{
    transcendental::exp,
    types::{I96F32, I110F18},
};

fn assert_float_compare(a: I32F32, b: I32F32, epsilon: I32F32) {
    assert!(I32F32::abs(a - b) <= epsilon, "a({a:?}) != b({b:?})");
}

fn assert_float_compare_64(a: I64F64, b: I64F64, epsilon: I64F64) {
    assert!(I64F64::abs(a - b) <= epsilon, "a({a:?}) != b({b:?})");
}

fn assert_vec_compare(va: &[I32F32], vb: &[I32F32], epsilon: I32F32) {
    assert!(va.len() == vb.len());
    for i in 0..va.len() {
        assert_float_compare(va[i], vb[i], epsilon);
    }
}

fn assert_vec_compare_64(va: &[I64F64], vb: &[I64F64], epsilon: I64F64) {
    assert!(va.len() == vb.len());
    for i in 0..va.len() {
        assert_float_compare_64(va[i], vb[i], epsilon);
    }
}

fn assert_vec_compare_u16(va: &[u16], vb: &[u16]) {
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

fn assert_sparse_mat_compare(
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

fn mat_to_fixed(matrix: &[Vec<f32>]) -> Vec<Vec<I32F32>> {
    matrix.iter().map(|row| vec_to_fixed(row)).collect()
}

fn assert_mat_approx_eq(left: &[Vec<I32F32>], right: &[Vec<I32F32>], epsilon: I32F32) {
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

#[test]
fn test_vec_max_upscale_to_u16() {
    let vector: Vec<I32F32> = vec_to_fixed(&[]);
    let target: Vec<u16> = vec![];
    let result: Vec<u16> = vec_max_upscale_to_u16(&vector);
    assert_vec_compare_u16(&result, &target);
    let vector: Vec<I32F32> = vec_to_fixed(&[0.]);
    let target: Vec<u16> = vec![0];
    let result: Vec<u16> = vec_max_upscale_to_u16(&vector);
    assert_vec_compare_u16(&result, &target);
    let vector: Vec<I32F32> = vec_to_fixed(&[0., 0.]);
    let target: Vec<u16> = vec![0, 0];
    let result: Vec<u16> = vec_max_upscale_to_u16(&vector);
    assert_vec_compare_u16(&result, &target);
    let vector: Vec<I32F32> = vec_to_fixed(&[0., 1.]);
    let target: Vec<u16> = vec![0, 65535];
    let result: Vec<u16> = vec_max_upscale_to_u16(&vector);
    assert_vec_compare_u16(&result, &target);
    let vector: Vec<I32F32> = vec_to_fixed(&[0., 0.000000001]);
    let target: Vec<u16> = vec![0, 65535];
    let result: Vec<u16> = vec_max_upscale_to_u16(&vector);
    assert_vec_compare_u16(&result, &target);
    let vector: Vec<I32F32> = vec_to_fixed(&[0., 0.000016, 1.]);
    let target: Vec<u16> = vec![0, 1, 65535];
    let result: Vec<u16> = vec_max_upscale_to_u16(&vector);
    assert_vec_compare_u16(&result, &target);
    let vector: Vec<I32F32> = vec_to_fixed(&[0.000000001, 0.000000001]);
    let target: Vec<u16> = vec![65535, 65535];
    let result: Vec<u16> = vec_max_upscale_to_u16(&vector);
    assert_vec_compare_u16(&result, &target);
    let vector: Vec<I32F32> = vec_to_fixed(&[
        0.000001, 0.000006, 0.000007, 0.0001, 0.001, 0.01, 0.1, 0.2, 0.3, 0.4,
    ]);
    let target: Vec<u16> = vec![0, 1, 1, 16, 164, 1638, 16384, 32768, 49151, 65535];
    let result: Vec<u16> = vec_max_upscale_to_u16(&vector);
    assert_vec_compare_u16(&result, &target);
    let vector: Vec<I32F32> = vec![I32F32::from_num(16384)];
    let target: Vec<u16> = vec![65535];
    let result: Vec<u16> = vec_max_upscale_to_u16(&vector);
    assert_vec_compare_u16(&result, &target);
    let vector: Vec<I32F32> = vec![I32F32::from_num(32768)];
    let target: Vec<u16> = vec![65535];
    let result: Vec<u16> = vec_max_upscale_to_u16(&vector);
    assert_vec_compare_u16(&result, &target);
    let vector: Vec<I32F32> = vec![I32F32::from_num(32769)];
    let target: Vec<u16> = vec![65535];
    let result: Vec<u16> = vec_max_upscale_to_u16(&vector);
    assert_vec_compare_u16(&result, &target);
    let vector: Vec<I32F32> = vec![I32F32::from_num(65535)];
    let target: Vec<u16> = vec![65535];
    let result: Vec<u16> = vec_max_upscale_to_u16(&vector);
    assert_vec_compare_u16(&result, &target);
    let vector: Vec<I32F32> = vec![I32F32::max_value()];
    let target: Vec<u16> = vec![65535];
    let result: Vec<u16> = vec_max_upscale_to_u16(&vector);
    assert_vec_compare_u16(&result, &target);
    let vector: Vec<I32F32> = vec_to_fixed(&[0., 1., 65535.]);
    let target: Vec<u16> = vec![0, 1, 65535];
    let result: Vec<u16> = vec_max_upscale_to_u16(&vector);
    assert_vec_compare_u16(&result, &target);
    let vector: Vec<I32F32> = vec_to_fixed(&[0., 0.5, 1., 1.5, 2., 32768.]);
    let target: Vec<u16> = vec![0, 1, 2, 3, 4, 65535];
    let result: Vec<u16> = vec_max_upscale_to_u16(&vector);
    assert_vec_compare_u16(&result, &target);
    let vector: Vec<I32F32> = vec_to_fixed(&[0., 0.5, 1., 1.5, 2., 32768., 32769.]);
    let target: Vec<u16> = vec![0, 1, 2, 3, 4, 65533, 65535];
    let result: Vec<u16> = vec_max_upscale_to_u16(&vector);
    assert_vec_compare_u16(&result, &target);
    let vector: Vec<I32F32> = vec![
        I32F32::from_num(0),
        I32F32::from_num(1),
        I32F32::from_num(32768),
        I32F32::from_num(32769),
        I32F32::max_value(),
    ];
    let target: Vec<u16> = vec![0, 0, 1, 1, 65535];
    let result: Vec<u16> = vec_max_upscale_to_u16(&vector);
    assert_vec_compare_u16(&result, &target);
}

#[test]
fn test_vec_u16_max_upscale_to_u16() {
    let vector: Vec<u16> = vec![];
    let result: Vec<u16> = vec_u16_max_upscale_to_u16(&vector);
    assert_vec_compare_u16(&result, &vector);
    let vector: Vec<u16> = vec![0];
    let result: Vec<u16> = vec_u16_max_upscale_to_u16(&vector);
    assert_vec_compare_u16(&result, &vector);
    let vector: Vec<u16> = vec![0, 0];
    let result: Vec<u16> = vec_u16_max_upscale_to_u16(&vector);
    assert_vec_compare_u16(&result, &vector);
    let vector: Vec<u16> = vec![1];
    let target: Vec<u16> = vec![65535];
    let result: Vec<u16> = vec_u16_max_upscale_to_u16(&vector);
    assert_vec_compare_u16(&result, &target);
    let vector: Vec<u16> = vec![0, 1];
    let target: Vec<u16> = vec![0, 65535];
    let result: Vec<u16> = vec_u16_max_upscale_to_u16(&vector);
    assert_vec_compare_u16(&result, &target);
    let vector: Vec<u16> = vec![65534];
    let target: Vec<u16> = vec![65535];
    let result: Vec<u16> = vec_u16_max_upscale_to_u16(&vector);
    assert_vec_compare_u16(&result, &target);
    let vector: Vec<u16> = vec![65535];
    let target: Vec<u16> = vec![65535];
    let result: Vec<u16> = vec_u16_max_upscale_to_u16(&vector);
    assert_vec_compare_u16(&result, &target);
    let vector: Vec<u16> = vec![65535, 65535];
    let target: Vec<u16> = vec![65535, 65535];
    let result: Vec<u16> = vec_u16_max_upscale_to_u16(&vector);
    assert_vec_compare_u16(&result, &target);
    let vector: Vec<u16> = vec![0, 1, 65534];
    let target: Vec<u16> = vec![0, 1, 65535];
    let result: Vec<u16> = vec_u16_max_upscale_to_u16(&vector);
    assert_vec_compare_u16(&result, &target);
    let vector: Vec<u16> = vec![0, 1, 2, 3, 4, 65533, 65535];
    let result: Vec<u16> = vec_u16_max_upscale_to_u16(&vector);
    assert_vec_compare_u16(&result, &vector);
}

#[test]
fn test_check_vec_max_limited() {
    let vector: Vec<u16> = vec![];
    let max_limit: u16 = 0;
    assert!(check_vec_max_limited(&vector, max_limit));
    let vector: Vec<u16> = vec![];
    let max_limit: u16 = u16::MAX;
    assert!(check_vec_max_limited(&vector, max_limit));
    let vector: Vec<u16> = vec![u16::MAX];
    let max_limit: u16 = u16::MAX;
    assert!(check_vec_max_limited(&vector, max_limit));
    let vector: Vec<u16> = vec![u16::MAX];
    let max_limit: u16 = u16::MAX - 1;
    assert!(!check_vec_max_limited(&vector, max_limit));
    let vector: Vec<u16> = vec![u16::MAX];
    let max_limit: u16 = 0;
    assert!(!check_vec_max_limited(&vector, max_limit));
    let vector: Vec<u16> = vec![0];
    let max_limit: u16 = u16::MAX;
    assert!(check_vec_max_limited(&vector, max_limit));
    let vector: Vec<u16> = vec![0, u16::MAX];
    let max_limit: u16 = u16::MAX;
    assert!(check_vec_max_limited(&vector, max_limit));
    let vector: Vec<u16> = vec![0, u16::MAX, u16::MAX];
    let max_limit: u16 = u16::MAX / 2;
    assert!(!check_vec_max_limited(&vector, max_limit));
    let vector: Vec<u16> = vec![0, u16::MAX, u16::MAX];
    let max_limit: u16 = u16::MAX / 2 + 1;
    assert!(check_vec_max_limited(&vector, max_limit));
    let vector: Vec<u16> = vec![0, u16::MAX, u16::MAX, u16::MAX];
    let max_limit: u16 = u16::MAX / 3 - 1;
    assert!(!check_vec_max_limited(&vector, max_limit));
    let vector: Vec<u16> = vec![0, u16::MAX, u16::MAX, u16::MAX];
    let max_limit: u16 = u16::MAX / 3;
    assert!(check_vec_max_limited(&vector, max_limit));
}

#[test]
fn test_math_fixed_overflow() {
    let max_32: I32F32 = I32F32::max_value();
    let max_u64: u64 = u64::MAX;
    let _prod_96: I96F32 = I96F32::from_num(max_32) * I96F32::from_num(max_u64);
    // let one: I96F32 = I96F32::from_num(1);
    // let prod_96: I96F32 = (I96F32::from_num(max_32) + one) * I96F32::from_num(max_u64); // overflows
    let _prod_110: I110F18 = I110F18::from_num(max_32) * I110F18::from_num(max_u64);

    let bonds_moving_average_val: u64 = 900_000_u64;
    let bonds_moving_average: I64F64 =
        I64F64::from_num(bonds_moving_average_val) / I64F64::from_num(1_000_000);
    let alpha: I32F32 = I32F32::from_num(1) - I32F32::from_num(bonds_moving_average);
    assert_eq!(I32F32::from_num(0.1), alpha);

    let bonds_moving_average: I64F64 = I64F64::from_num(max_32) / I64F64::from_num(max_32);
    let alpha: I32F32 = I32F32::from_num(1) - I32F32::from_num(bonds_moving_average);
    assert_eq!(I32F32::from_num(0), alpha);
}

#[test]
fn test_math_u64_normalization() {
    let min: u64 = 1;
    let min32: u64 = 4_889_444; // 21_000_000_000_000_000 / 4_294_967_296
    let mid: u64 = 10_500_000_000_000_000;
    let max: u64 = 21_000_000_000_000_000;
    let min_64: I64F64 = I64F64::from_num(min);
    let min32_64: I64F64 = I64F64::from_num(min32);
    let mid_64: I64F64 = I64F64::from_num(mid);
    let max_64: I64F64 = I64F64::from_num(max);
    let max_sum: I64F64 = I64F64::from_num(max);
    let min_frac: I64F64 = min_64 / max_sum;
    assert_eq!(min_frac, I64F64::from_num(0.0000000000000000476));
    let min_frac_32: I32F32 = I32F32::from_num(min_frac);
    assert_eq!(min_frac_32, I32F32::from_num(0));
    let min32_frac: I64F64 = min32_64 / max_sum;
    assert_eq!(min32_frac, I64F64::from_num(0.00000000023283066664));
    let min32_frac_32: I32F32 = I32F32::from_num(min32_frac);
    assert_eq!(min32_frac_32, I32F32::from_num(0.0000000002));
    let half: I64F64 = mid_64 / max_sum;
    assert_eq!(half, I64F64::from_num(0.5));
    let half_32: I32F32 = I32F32::from_num(half);
    assert_eq!(half_32, I32F32::from_num(0.5));
    let one: I64F64 = max_64 / max_sum;
    assert_eq!(one, I64F64::from_num(1));
    let one_32: I32F32 = I32F32::from_num(one);
    assert_eq!(one_32, I32F32::from_num(1));
}

#[test]
fn test_math_to_num() {
    let val: I32F32 = I32F32::from_num(u16::MAX);
    let res: u16 = val.to_num::<u16>();
    assert_eq!(res, u16::MAX);
    let vector: Vec<I32F32> = vec![val; 1000];
    let target: Vec<u16> = vec![u16::MAX; 1000];
    let output: Vec<u16> = vector.iter().map(|e: &I32F32| e.to_num::<u16>()).collect();
    assert_eq!(output, target);
    let output: Vec<u16> = vector
        .iter()
        .map(|e: &I32F32| (*e).to_num::<u16>())
        .collect();
    assert_eq!(output, target);
    let val: I32F32 = I32F32::max_value();
    let res: u64 = val.to_num::<u64>();
    let vector: Vec<I32F32> = vec![val; 1000];
    let target: Vec<u64> = vec![res; 1000];
    let output: Vec<u64> = vector.iter().map(|e: &I32F32| e.to_num::<u64>()).collect();
    assert_eq!(output, target);
    let output: Vec<u64> = vector
        .iter()
        .map(|e: &I32F32| (*e).to_num::<u64>())
        .collect();
    assert_eq!(output, target);
    let val: I32F32 = I32F32::from_num(0);
    let res: u64 = val.to_num::<u64>();
    let vector: Vec<I32F32> = vec![val; 1000];
    let target: Vec<u64> = vec![res; 1000];
    let output: Vec<u64> = vector.iter().map(|e: &I32F32| e.to_num::<u64>()).collect();
    assert_eq!(output, target);
    let output: Vec<u64> = vector
        .iter()
        .map(|e: &I32F32| (*e).to_num::<u64>())
        .collect();
    assert_eq!(output, target);
    let val: I96F32 = I96F32::from_num(u64::MAX);
    let res: u64 = val.to_num::<u64>();
    assert_eq!(res, u64::MAX);
    let vector: Vec<I96F32> = vec![val; 1000];
    let target: Vec<u64> = vec![u64::MAX; 1000];
    let output: Vec<u64> = vector.iter().map(|e: &I96F32| e.to_num::<u64>()).collect();
    assert_eq!(output, target);
    let output: Vec<u64> = vector
        .iter()
        .map(|e: &I96F32| (*e).to_num::<u64>())
        .collect();
    assert_eq!(output, target);
}

#[test]
fn test_math_vec_to_fixed() {
    let vector: Vec<f32> = vec![0., 1., 2., 3.];
    let target: Vec<I32F32> = vec![
        I32F32::from_num(0.),
        I32F32::from_num(1.),
        I32F32::from_num(2.),
        I32F32::from_num(3.),
    ];
    let result = vec_to_fixed(&vector);
    assert_vec_compare(&result, &target, I32F32::from_num(0));
}

// Reshape vector to matrix with specified number of rows, cast to I32F32.
pub fn vec_to_mat_fixed(vector: &[f32], rows: usize, transpose: bool) -> Vec<Vec<I32F32>> {
    assert!(
        vector.len().is_multiple_of(rows),
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

#[test]
fn test_math_vec_to_mat_fixed() {
    let vector: Vec<f32> = vec![0., 1., 2., 0., 10., 100.];
    let target: Vec<Vec<I32F32>> = vec![
        vec![
            I32F32::from_num(0.),
            I32F32::from_num(1.),
            I32F32::from_num(2.),
        ],
        vec![
            I32F32::from_num(0.),
            I32F32::from_num(10.),
            I32F32::from_num(100.),
        ],
    ];
    let mat = vec_to_mat_fixed(&vector, 2, false);
    assert_mat_compare(&mat, &target, I32F32::from_num(0));
}

// Reshape vector to sparse matrix with specified number of input rows, cast f32 to I32F32.
fn vec_to_sparse_mat_fixed(
    vector: &[f32],
    rows: usize,
    transpose: bool,
) -> Vec<Vec<(u16, I32F32)>> {
    assert!(
        vector.len().is_multiple_of(rows),
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

#[test]
fn test_math_vec_to_sparse_mat_fixed() {
    let vector: Vec<f32> = vec![0., 1., 2., 0., 10., 100.];
    let target: Vec<Vec<(u16, I32F32)>> = vec![
        vec![(1_u16, I32F32::from_num(1.)), (2_u16, I32F32::from_num(2.))],
        vec![
            (1_u16, I32F32::from_num(10.)),
            (2_u16, I32F32::from_num(100.)),
        ],
    ];
    let mat = vec_to_sparse_mat_fixed(&vector, 2, false);
    assert_sparse_mat_compare(&mat, &target, I32F32::from_num(0));
    let vector: Vec<f32> = vec![0., 0.];
    let target: Vec<Vec<(u16, I32F32)>> = vec![vec![], vec![]];
    let mat = vec_to_sparse_mat_fixed(&vector, 2, false);
    assert_sparse_mat_compare(&mat, &target, I32F32::from_num(0));
    let vector: Vec<f32> = vec![0., 1., 2., 0., 10., 100.];
    let target: Vec<Vec<(u16, I32F32)>> = vec![
        vec![],
        vec![
            (0_u16, I32F32::from_num(1.)),
            (1_u16, I32F32::from_num(10.)),
        ],
        vec![
            (0_u16, I32F32::from_num(2.)),
            (1_u16, I32F32::from_num(100.)),
        ],
    ];
    let mat = vec_to_sparse_mat_fixed(&vector, 2, true);
    assert_sparse_mat_compare(&mat, &target, I32F32::from_num(0));
    let vector: Vec<f32> = vec![0., 0.];
    let target: Vec<Vec<(u16, I32F32)>> = vec![vec![]];
    let mat = vec_to_sparse_mat_fixed(&vector, 2, true);
    assert_sparse_mat_compare(&mat, &target, I32F32::from_num(0));
}

#[test]
fn test_math_exp_safe() {
    let zero: I32F32 = I32F32::from_num(0);
    let one: I32F32 = I32F32::from_num(1);
    let target: I32F32 = exp(zero).unwrap();
    assert_eq!(exp_safe(zero), target);
    let target: I32F32 = exp(one).unwrap();
    assert_eq!(exp_safe(one), target);
    let min_input: I32F32 = I32F32::from_num(-20); // <= 1/exp(-20) = 485 165 195,4097903
    let max_input: I32F32 = I32F32::from_num(20); // <= exp(20) = 485 165 195,4097903
    let target: I32F32 = exp(min_input).unwrap();
    assert_eq!(exp_safe(min_input), target);
    assert_eq!(exp_safe(min_input - one), target);
    assert_eq!(exp_safe(I32F32::min_value()), target);
    let target: I32F32 = exp(max_input).unwrap();
    assert_eq!(exp_safe(max_input), target);
    assert_eq!(exp_safe(max_input + one), target);
    assert_eq!(exp_safe(I32F32::max_value()), target);
}

#[test]
fn test_math_sigmoid_safe() {
    let trust: Vec<I32F32> = vec![
        I32F32::min_value(),
        I32F32::from_num(0),
        I32F32::from_num(0.4),
        I32F32::from_num(0.5),
        I32F32::from_num(0.6),
        I32F32::from_num(1),
        I32F32::max_value(),
    ];
    let consensus: Vec<I32F32> = trust
        .iter()
        .map(|t: &I32F32| sigmoid_safe(*t, I32F32::max_value(), I32F32::max_value()))
        .collect();
    let target: Vec<I32F32> = vec_to_fixed(&[
        0.0000000019,
        0.0000000019,
        0.0000000019,
        0.0000000019,
        0.0000000019,
        0.0000000019,
        0.5,
    ]);
    assert_eq!(&consensus, &target);
    let consensus: Vec<I32F32> = trust
        .iter()
        .map(|t: &I32F32| sigmoid_safe(*t, I32F32::min_value(), I32F32::min_value()))
        .collect();
    let target: Vec<I32F32> = vec_to_fixed(&[
        0.5,
        0.0000000019,
        0.0000000019,
        0.0000000019,
        0.0000000019,
        0.0000000019,
        0.0000000019,
    ]);
    assert_eq!(&consensus, &target);
    let consensus: Vec<I32F32> = trust
        .iter()
        .map(|t: &I32F32| sigmoid_safe(*t, I32F32::from_num(30), I32F32::from_num(0.5)))
        .collect();
    let target: Vec<f64> = vec![
        0.0000000019,
        0.0000003057,
        0.0474258729,
        0.5,
        0.952574127,
        0.9999996943,
        0.9999999981,
    ];
    let target: Vec<I32F32> = target.iter().map(|c: &f64| I32F32::from_num(*c)).collect();
    assert_eq!(&consensus, &target);
    let trust: Vec<I32F32> = vec_to_fixed(&[0., 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.]);
    let consensus: Vec<I32F32> = trust
        .iter()
        .map(|t: &I32F32| sigmoid_safe(*t, I32F32::from_num(40), I32F32::from_num(0.5)))
        .collect();
    let target: Vec<f64> = vec![
        0.0000000019,
        0.0000001125,
        0.0000061442,
        0.0003353502,
        0.017986214,
        0.5,
        0.9820138067,
        0.9996646498,
        0.9999938558,
        0.9999998875,
        0.9999999981,
    ];
    let target: Vec<I32F32> = target.iter().map(|c: &f64| I32F32::from_num(*c)).collect();
    assert_eq!(&consensus, &target);
}

#[test]
fn test_math_is_topk() {
    let vector: Vec<I32F32> = vec_to_fixed(&[]);
    let result = is_topk(&vector, 5);
    let target: Vec<bool> = vec![];
    assert_eq!(&result, &target);
    let vector: Vec<I32F32> = vec_to_fixed(&[0., 1., 2., 3., 4., 5., 6., 7., 8., 9.]);
    let result = is_topk(&vector, 0);
    let target: Vec<bool> = vec![
        false, false, false, false, false, false, false, false, false, false,
    ];
    assert_eq!(&result, &target);
    let result = is_topk(&vector, 5);
    let target: Vec<bool> = vec![
        false, false, false, false, false, true, true, true, true, true,
    ];
    assert_eq!(&result, &target);
    let result = is_topk(&vector, 10);
    let target: Vec<bool> = vec![true, true, true, true, true, true, true, true, true, true];
    assert_eq!(&result, &target);
    let result = is_topk(&vector, 100);
    assert_eq!(&result, &target);
    let vector: Vec<I32F32> = vec_to_fixed(&[9., 8., 7., 6., 5., 4., 3., 2., 1., 0.]);
    let result = is_topk(&vector, 5);
    let target: Vec<bool> = vec![
        true, true, true, true, true, false, false, false, false, false,
    ];
    assert_eq!(&result, &target);
    let vector: Vec<I32F32> = vec_to_fixed(&[9., 0., 8., 1., 7., 2., 6., 3., 5., 4.]);
    let result = is_topk(&vector, 5);
    let target: Vec<bool> = vec![
        true, false, true, false, true, false, true, false, true, false,
    ];
    assert_eq!(&result, &target);
    let vector: Vec<I32F32> = vec_to_fixed(&[0.9, 0., 0.8, 0.1, 0.7, 0.2, 0.6, 0.3, 0.5, 0.4]);
    let result = is_topk(&vector, 5);
    let target: Vec<bool> = vec![
        true, false, true, false, true, false, true, false, true, false,
    ];
    assert_eq!(&result, &target);
    let vector: Vec<I32F32> = vec_to_fixed(&[0., 1., 2., 3., 4., 5., 5., 5., 5., 6.]);
    let result = is_topk(&vector, 5);
    let target: Vec<bool> = vec![
        false, false, false, false, false, true, true, true, true, true,
    ];
    assert_eq!(&result, &target);
}

#[test]
fn test_math_sum() {
    assert!(sum(&[]) == I32F32::from_num(0));
    assert!(
        sum(&[
            I32F32::from_num(1.0),
            I32F32::from_num(10.0),
            I32F32::from_num(30.0)
        ]) == I32F32::from_num(41)
    );
    assert!(
        sum(&[
            I32F32::from_num(-1.0),
            I32F32::from_num(10.0),
            I32F32::from_num(30.0)
        ]) == I32F32::from_num(39)
    );
}

#[test]
fn test_math_normalize() {
    let epsilon: I32F32 = I32F32::from_num(0.0001);
    let x: Vec<I32F32> = vec![];
    let y: Vec<I32F32> = normalize(&x);
    assert_vec_compare(&x, &y, epsilon);
    let x: Vec<I32F32> = vec![
        I32F32::from_num(1.0),
        I32F32::from_num(10.0),
        I32F32::from_num(30.0),
    ];
    let y: Vec<I32F32> = normalize(&x);
    assert_vec_compare(
        &y,
        &[
            I32F32::from_num(0.0243902437),
            I32F32::from_num(0.243902439),
            I32F32::from_num(0.7317073171),
        ],
        epsilon,
    );
    assert_float_compare(sum(&y), I32F32::from_num(1.0), epsilon);
    let x: Vec<I32F32> = vec![
        I32F32::from_num(-1.0),
        I32F32::from_num(10.0),
        I32F32::from_num(30.0),
    ];
    let y: Vec<I32F32> = normalize(&x);
    assert_vec_compare(
        &y,
        &[
            I32F32::from_num(-0.0256410255),
            I32F32::from_num(0.2564102563),
            I32F32::from_num(0.769230769),
        ],
        epsilon,
    );
    assert_float_compare(sum(&y), I32F32::from_num(1.0), epsilon);
}

#[test]
fn test_math_inplace_normalize() {
    let epsilon: I32F32 = I32F32::from_num(0.0001);
    let mut x1: Vec<I32F32> = vec![
        I32F32::from_num(1.0),
        I32F32::from_num(10.0),
        I32F32::from_num(30.0),
    ];
    inplace_normalize(&mut x1);
    assert_vec_compare(
        &x1,
        &[
            I32F32::from_num(0.0243902437),
            I32F32::from_num(0.243902439),
            I32F32::from_num(0.7317073171),
        ],
        epsilon,
    );
    let mut x2: Vec<I32F32> = vec![
        I32F32::from_num(-1.0),
        I32F32::from_num(10.0),
        I32F32::from_num(30.0),
    ];
    inplace_normalize(&mut x2);
    assert_vec_compare(
        &x2,
        &[
            I32F32::from_num(-0.0256410255),
            I32F32::from_num(0.2564102563),
            I32F32::from_num(0.769230769),
        ],
        epsilon,
    );
}

#[test]
fn test_math_inplace_normalize_64() {
    let epsilon: I64F64 = I64F64::from_num(0.0001);
    let mut x1: Vec<I64F64> = vec![
        I64F64::from_num(1.0),
        I64F64::from_num(10.0),
        I64F64::from_num(30.0),
    ];
    inplace_normalize_64(&mut x1);
    assert_vec_compare_64(
        &x1,
        &[
            I64F64::from_num(0.0243902437),
            I64F64::from_num(0.243902439),
            I64F64::from_num(0.7317073171),
        ],
        epsilon,
    );
    let mut x2: Vec<I64F64> = vec![
        I64F64::from_num(-1.0),
        I64F64::from_num(10.0),
        I64F64::from_num(30.0),
    ];
    inplace_normalize_64(&mut x2);
    assert_vec_compare_64(
        &x2,
        &[
            I64F64::from_num(-0.0256410255),
            I64F64::from_num(0.2564102563),
            I64F64::from_num(0.769230769),
        ],
        epsilon,
    );
}

#[test]
fn test_math_vecdiv() {
    let x: Vec<I32F32> = vec_to_fixed(&[]);
    let y: Vec<I32F32> = vec_to_fixed(&[]);
    let result: Vec<I32F32> = vec_to_fixed(&[]);
    assert_eq!(result, vecdiv(&x, &y));

    let x: Vec<I32F32> = vec_to_fixed(&[0., 1., 0., 1.]);
    let y: Vec<I32F32> = vec_to_fixed(&[0., 1., 1., 0.]);
    let result: Vec<I32F32> = vec_to_fixed(&[0., 1., 0., 0.]);
    assert_eq!(result, vecdiv(&x, &y));

    let x: Vec<I32F32> = vec_to_fixed(&[1., 1., 10.]);
    let y: Vec<I32F32> = vec_to_fixed(&[2., 3., 2.]);
    let result: Vec<I32F32> = vec![fixed(1.) / fixed(2.), fixed(1.) / fixed(3.), fixed(5.)];
    assert_eq!(result, vecdiv(&x, &y));
}

#[test]
fn test_math_inplace_row_normalize() {
    let epsilon: I32F32 = I32F32::from_num(0.0001);
    let vector: Vec<f32> = vec![
        0., 1., 2., 3., 4., 0., 10., 100., 1000., 10000., 0., 0., 0., 0., 0., 1., 1., 1., 1., 1.,
    ];
    let mut mat = vec_to_mat_fixed(&vector, 4, false);
    inplace_row_normalize(&mut mat);
    let target: Vec<f32> = vec![
        0., 0.1, 0.2, 0.3, 0.4, 0., 0.0009, 0.009, 0.09, 0.9, 0., 0., 0., 0., 0., 0.2, 0.2, 0.2,
        0.2, 0.2,
    ];
    assert_mat_compare(&mat, &vec_to_mat_fixed(&target, 4, false), epsilon);
}

#[test]
fn test_math_inplace_row_normalize_sparse() {
    let epsilon: I32F32 = I32F32::from_num(0.0001);
    let vector: Vec<f32> = vec![
        0., 1., 0., 2., 0., 3., 4., 0., 1., 0., 2., 0., 3., 0., 1., 0., 0., 2., 0., 3., 4., 0.,
        10., 0., 100., 1000., 0., 10000., 0., 0., 0., 0., 0., 0., 0., 1., 1., 1., 1., 1., 1., 1.,
    ];
    let mut mat = vec_to_sparse_mat_fixed(&vector, 6, false);
    inplace_row_normalize_sparse(&mut mat);
    let target: Vec<f32> = vec![
        0., 0.1, 0., 0.2, 0., 0.3, 0.4, 0., 0.166666, 0., 0.333333, 0., 0.5, 0., 0.1, 0., 0., 0.2,
        0., 0.3, 0.4, 0., 0.0009, 0., 0.009, 0.09, 0., 0.9, 0., 0., 0., 0., 0., 0., 0., 0.142857,
        0.142857, 0.142857, 0.142857, 0.142857, 0.142857, 0.142857,
    ];
    assert_sparse_mat_compare(&mat, &vec_to_sparse_mat_fixed(&target, 6, false), epsilon);
    let vector: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let target: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let mut mat = vec_to_sparse_mat_fixed(&vector, 3, false);
    inplace_row_normalize_sparse(&mut mat);
    assert_sparse_mat_compare(
        &mat,
        &vec_to_sparse_mat_fixed(&target, 3, false),
        I32F32::from_num(0),
    );
}

#[test]
fn test_math_inplace_col_normalize() {
    let epsilon: I32F32 = I32F32::from_num(0.0001);
    let vector: Vec<f32> = vec![
        0., 1., 2., 3., 4., 0., 10., 100., 1000., 10000., 0., 0., 0., 0., 0., 1., 1., 1., 1., 1.,
    ];
    let mut mat = vec_to_mat_fixed(&vector, 4, true);
    inplace_col_normalize(&mut mat);
    let target: Vec<f32> = vec![
        0., 0.1, 0.2, 0.3, 0.4, 0., 0.0009, 0.009, 0.09, 0.9, 0., 0., 0., 0., 0., 0.2, 0.2, 0.2,
        0.2, 0.2,
    ];
    assert_mat_compare(&mat, &vec_to_mat_fixed(&target, 4, true), epsilon);
}

#[test]
fn test_math_inplace_col_normalize_sparse() {
    let epsilon: I32F32 = I32F32::from_num(0.0001);
    let vector: Vec<f32> = vec![
        0., 1., 0., 2., 0., 3., 4., 0., 1., 0., 2., 0., 3., 0., 1., 0., 0., 2., 0., 3., 4., 0.,
        10., 0., 100., 1000., 0., 10000., 0., 0., 0., 0., 0., 0., 0., 1., 1., 1., 1., 1., 1., 1.,
    ];
    let mut mat = vec_to_sparse_mat_fixed(&vector, 6, true);
    inplace_col_normalize_sparse(&mut mat, 6);
    let target: Vec<f32> = vec![
        0., 0.1, 0., 0.2, 0., 0.3, 0.4, 0., 0.166666, 0., 0.333333, 0., 0.5, 0., 0.1, 0., 0., 0.2,
        0., 0.3, 0.4, 0., 0.0009, 0., 0.009, 0.09, 0., 0.9, 0., 0., 0., 0., 0., 0., 0., 0.142857,
        0.142857, 0.142857, 0.142857, 0.142857, 0.142857, 0.142857,
    ];
    assert_sparse_mat_compare(&mat, &vec_to_sparse_mat_fixed(&target, 6, true), epsilon);
    let vector: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let target: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let mut mat = vec_to_sparse_mat_fixed(&vector, 3, false);
    inplace_col_normalize_sparse(&mut mat, 6);
    assert_sparse_mat_compare(
        &mat,
        &vec_to_sparse_mat_fixed(&target, 3, false),
        I32F32::from_num(0),
    );
    let mut mat: Vec<Vec<(u16, I32F32)>> = vec![];
    let target: Vec<Vec<(u16, I32F32)>> = vec![];
    inplace_col_normalize_sparse(&mut mat, 0);
    assert_sparse_mat_compare(&mat, &target, epsilon);
}

#[test]
fn test_math_inplace_col_max_upscale() {
    let mut mat: Vec<Vec<I32F32>> = vec![vec![]];
    let target: Vec<Vec<I32F32>> = vec![vec![]];
    inplace_col_max_upscale(&mut mat);
    assert_eq!(&mat, &target);
    let mut mat: Vec<Vec<I32F32>> = vec![vec![I32F32::from_num(0)]];
    let target: Vec<Vec<I32F32>> = vec![vec![I32F32::from_num(0)]];
    inplace_col_max_upscale(&mut mat);
    assert_eq!(&mat, &target);
    let epsilon: I32F32 = I32F32::from_num(0.0001);
    let vector: Vec<f32> = vec![
        0., 1., 2., 3., 4., 0., 10., 100., 1000., 10000., 0., 0., 0., 0., 0., 1., 1., 1., 1., 1.,
    ];
    let mut mat: Vec<Vec<I32F32>> = vec_to_mat_fixed(&vector, 4, true);
    inplace_col_max_upscale(&mut mat);
    let target: Vec<f32> = vec![
        0., 0.25, 0.5, 0.75, 1., 0., 0.001, 0.01, 0.1, 1., 0., 0., 0., 0., 0., 1., 1., 1., 1., 1.,
    ];
    assert_mat_compare(&mat, &vec_to_mat_fixed(&target, 4, true), epsilon);
}

#[test]
fn test_math_inplace_col_max_upscale_sparse() {
    let mut mat: Vec<Vec<(u16, I32F32)>> = vec![vec![]];
    let target: Vec<Vec<(u16, I32F32)>> = vec![vec![]];
    inplace_col_max_upscale_sparse(&mut mat, 0);
    assert_eq!(&mat, &target);
    let mut mat: Vec<Vec<(u16, I32F32)>> = vec![vec![(0, I32F32::from_num(0))]];
    let target: Vec<Vec<(u16, I32F32)>> = vec![vec![(0, I32F32::from_num(0))]];
    inplace_col_max_upscale_sparse(&mut mat, 1);
    assert_eq!(&mat, &target);
    let epsilon: I32F32 = I32F32::from_num(0.0001);
    let vector: Vec<f32> = vec![
        0., 1., 0., 2., 0., 3., 4., 0., 1., 0., 2., 0., 3., 0., 1., 0., 0., 2., 0., 3., 4., 0.,
        10., 0., 100., 1000., 0., 10000., 0., 0., 0., 0., 0., 0., 0., 1., 1., 1., 1., 1., 1., 1.,
    ];
    let mut mat = vec_to_sparse_mat_fixed(&vector, 6, true);
    inplace_col_max_upscale_sparse(&mut mat, 6);
    let target: Vec<f32> = vec![
        0., 0.25, 0., 0.5, 0., 0.75, 1., 0., 0.333333, 0., 0.666666, 0., 1., 0., 0.25, 0., 0., 0.5,
        0., 0.75, 1., 0., 0.001, 0., 0.01, 0.1, 0., 1., 0., 0., 0., 0., 0., 0., 0., 1., 1., 1., 1.,
        1., 1., 1.,
    ];
    assert_sparse_mat_compare(&mat, &vec_to_sparse_mat_fixed(&target, 6, true), epsilon);
    let vector: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let target: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let mut mat = vec_to_sparse_mat_fixed(&vector, 3, false);
    inplace_col_max_upscale_sparse(&mut mat, 6);
    assert_sparse_mat_compare(
        &mat,
        &vec_to_sparse_mat_fixed(&target, 3, false),
        I32F32::from_num(0),
    );
    let mut mat: Vec<Vec<(u16, I32F32)>> = vec![];
    let target: Vec<Vec<(u16, I32F32)>> = vec![];
    inplace_col_max_upscale_sparse(&mut mat, 0);
    assert_sparse_mat_compare(&mat, &target, epsilon);
}

#[test]
fn test_math_inplace_mask_vector() {
    let mask: Vec<bool> = vec![false, false, false];
    let mut vector: Vec<I32F32> = vec_to_fixed(&[0., 1., 2.]);
    let target: Vec<I32F32> = vec_to_fixed(&[0., 1., 2.]);
    inplace_mask_vector(&mask, &mut vector);
    assert_vec_compare(&vector, &target, I32F32::from_num(0));
    let mask: Vec<bool> = vec![false, true, false];
    let mut vector: Vec<I32F32> = vec_to_fixed(&[0., 1., 2.]);
    let target: Vec<I32F32> = vec_to_fixed(&[0., 0., 2.]);
    inplace_mask_vector(&mask, &mut vector);
    assert_vec_compare(&vector, &target, I32F32::from_num(0));
    let mask: Vec<bool> = vec![true, true, true];
    let mut vector: Vec<I32F32> = vec_to_fixed(&[0., 1., 2.]);
    let target: Vec<I32F32> = vec_to_fixed(&[0., 0., 0.]);
    inplace_mask_vector(&mask, &mut vector);
    assert_vec_compare(&vector, &target, I32F32::from_num(0));
}

#[test]
fn test_math_inplace_mask_matrix() {
    let mask: Vec<Vec<bool>> = vec![
        vec![false, false, false],
        vec![false, false, false],
        vec![false, false, false],
    ];
    let vector: Vec<f32> = vec![0., 1., 2., 3., 4., 5., 6., 7., 8.];
    let mut mat = vec_to_mat_fixed(&vector, 3, false);
    inplace_mask_matrix(&mask, &mut mat);
    assert_mat_compare(
        &mat,
        &vec_to_mat_fixed(&vector, 3, false),
        I32F32::from_num(0),
    );
    let mask: Vec<Vec<bool>> = vec![
        vec![true, false, false],
        vec![false, true, false],
        vec![false, false, true],
    ];
    let target: Vec<f32> = vec![0., 1., 2., 3., 0., 5., 6., 7., 0.];
    let mut mat = vec_to_mat_fixed(&vector, 3, false);
    inplace_mask_matrix(&mask, &mut mat);
    assert_mat_compare(
        &mat,
        &vec_to_mat_fixed(&target, 3, false),
        I32F32::from_num(0),
    );
    let mask: Vec<Vec<bool>> = vec![
        vec![true, true, true],
        vec![true, true, true],
        vec![true, true, true],
    ];
    let target: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let mut mat = vec_to_mat_fixed(&vector, 3, false);
    inplace_mask_matrix(&mask, &mut mat);
    assert_mat_compare(
        &mat,
        &vec_to_mat_fixed(&target, 3, false),
        I32F32::from_num(0),
    );
}

#[test]
fn test_math_inplace_mask_rows() {
    let input: Vec<f32> = vec![1., 2., 3., 4., 5., 6., 7., 8., 9.];
    let mask: Vec<bool> = vec![false, false, false];
    let target: Vec<f32> = vec![1., 2., 3., 4., 5., 6., 7., 8., 9.];
    let mut mat = vec_to_mat_fixed(&input, 3, false);
    inplace_mask_rows(&mask, &mut mat);
    assert_mat_compare(
        &mat,
        &vec_to_mat_fixed(&target, 3, false),
        I32F32::from_num(0),
    );
    let mask: Vec<bool> = vec![true, true, true];
    let target: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let mut mat = vec_to_mat_fixed(&input, 3, false);
    inplace_mask_rows(&mask, &mut mat);
    assert_mat_compare(
        &mat,
        &vec_to_mat_fixed(&target, 3, false),
        I32F32::from_num(0),
    );
    let mask: Vec<bool> = vec![true, false, true];
    let target: Vec<f32> = vec![0., 0., 0., 4., 5., 6., 0., 0., 0.];
    let mut mat = vec_to_mat_fixed(&input, 3, false);
    inplace_mask_rows(&mask, &mut mat);
    assert_mat_compare(
        &mat,
        &vec_to_mat_fixed(&target, 3, false),
        I32F32::from_num(0),
    );
    let input: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let mut mat = vec_to_mat_fixed(&input, 3, false);
    let mask: Vec<bool> = vec![false, false, false];
    let target: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0.];
    inplace_mask_rows(&mask, &mut mat);
    assert_mat_compare(
        &mat,
        &vec_to_mat_fixed(&target, 3, false),
        I32F32::from_num(0),
    );
}

#[test]
fn test_math_inplace_mask_diag() {
    let vector: Vec<f32> = vec![1., 2., 3., 4., 5., 6., 7., 8., 9.];
    let target: Vec<f32> = vec![0., 2., 3., 4., 0., 6., 7., 8., 0.];
    let mut mat = vec_to_mat_fixed(&vector, 3, false);
    inplace_mask_diag(&mut mat);
    assert_mat_compare(
        &mat,
        &vec_to_mat_fixed(&target, 3, false),
        I32F32::from_num(0),
    );
}

#[test]
fn test_math_inplace_mask_diag_except_index() {
    let vector: Vec<f32> = vec![1., 2., 3., 4., 5., 6., 7., 8., 9.];
    let rows = 3;

    for i in 0..rows {
        let mut target: Vec<f32> = vec![0., 2., 3., 4., 0., 6., 7., 8., 0.];
        let row = i * rows;
        let col = i;
        target[row + col] = vector[row + col];

        let mut mat = vec_to_mat_fixed(&vector, rows, false);
        inplace_mask_diag_except_index(&mut mat, i as u16);
        assert_mat_compare(
            &mat,
            &vec_to_mat_fixed(&target, rows, false),
            I32F32::from_num(0),
        );
    }
}

#[test]
fn test_math_mask_rows_sparse() {
    let input: Vec<f32> = vec![1., 2., 3., 4., 5., 6., 7., 8., 9.];
    let mat = vec_to_sparse_mat_fixed(&input, 3, false);
    let mask: Vec<bool> = vec![false, false, false];
    let target: Vec<f32> = vec![1., 2., 3., 4., 5., 6., 7., 8., 9.];
    let result = mask_rows_sparse(&mask, &mat);
    assert_sparse_mat_compare(
        &result,
        &vec_to_sparse_mat_fixed(&target, 3, false),
        I32F32::from_num(0),
    );
    let mask: Vec<bool> = vec![true, true, true];
    let target: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let result = mask_rows_sparse(&mask, &mat);
    assert_sparse_mat_compare(
        &result,
        &vec_to_sparse_mat_fixed(&target, 3, false),
        I32F32::from_num(0),
    );
    let mask: Vec<bool> = vec![true, false, true];
    let target: Vec<f32> = vec![0., 0., 0., 4., 5., 6., 0., 0., 0.];
    let result = mask_rows_sparse(&mask, &mat);
    assert_sparse_mat_compare(
        &result,
        &vec_to_sparse_mat_fixed(&target, 3, false),
        I32F32::from_num(0),
    );
    let input: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let mat = vec_to_sparse_mat_fixed(&input, 3, false);
    let mask: Vec<bool> = vec![false, false, false];
    let target: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let result = mask_rows_sparse(&mask, &mat);
    assert_sparse_mat_compare(
        &result,
        &vec_to_sparse_mat_fixed(&target, 3, false),
        I32F32::from_num(0),
    );
}

#[test]
fn test_math_mask_diag_sparse() {
    let vector: Vec<f32> = vec![1., 2., 3., 4., 5., 6., 7., 8., 9.];
    let target: Vec<f32> = vec![0., 2., 3., 4., 0., 6., 7., 8., 0.];
    let mat = vec_to_sparse_mat_fixed(&vector, 3, false);
    let result = mask_diag_sparse(&mat);
    assert_sparse_mat_compare(
        &result,
        &vec_to_sparse_mat_fixed(&target, 3, false),
        I32F32::from_num(0),
    );
    let vector: Vec<f32> = vec![1., 0., 0., 0., 5., 0., 0., 0., 9.];
    let target: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let mat = vec_to_sparse_mat_fixed(&vector, 3, false);
    let result = mask_diag_sparse(&mat);
    assert_sparse_mat_compare(
        &result,
        &vec_to_sparse_mat_fixed(&target, 3, false),
        I32F32::from_num(0),
    );
    let vector: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let target: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let mat = vec_to_sparse_mat_fixed(&vector, 3, false);
    let result = mask_diag_sparse(&mat);
    assert_sparse_mat_compare(
        &result,
        &vec_to_sparse_mat_fixed(&target, 3, false),
        I32F32::from_num(0),
    );
}

#[test]
fn test_math_mask_diag_sparse_except_index() {
    let rows = 3;

    let vector: Vec<f32> = vec![1., 2., 3., 4., 5., 6., 7., 8., 9.];
    let mat = vec_to_sparse_mat_fixed(&vector, rows, false);

    for i in 0..rows {
        let mut target: Vec<f32> = vec![0., 2., 3., 4., 0., 6., 7., 8., 0.];
        let row = i * rows;
        let col = i;
        target[row + col] = vector[row + col];

        let result = mask_diag_sparse_except_index(&mat, i as u16);
        let target_as_mat = vec_to_sparse_mat_fixed(&target, rows, false);

        assert_sparse_mat_compare(&result, &target_as_mat, I32F32::from_num(0));
    }

    let vector: Vec<f32> = vec![1., 0., 0., 0., 5., 0., 0., 0., 9.];
    let mat = vec_to_sparse_mat_fixed(&vector, rows, false);

    for i in 0..rows {
        let mut target: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0.];
        let row = i * rows;
        let col = i;
        target[row + col] = vector[row + col];

        let result = mask_diag_sparse_except_index(&mat, i as u16);
        let target_as_mat = vec_to_sparse_mat_fixed(&target, rows, false);
        assert_eq!(result.len(), target_as_mat.len());

        assert_sparse_mat_compare(&result, &target_as_mat, I32F32::from_num(0));
    }

    for i in 0..rows {
        let vector: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0.];
        let mat = vec_to_sparse_mat_fixed(&vector, rows, false);

        let mut target: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0.];
        let row = i * rows;
        let col = i;
        target[row + col] = vector[row + col];

        let result = mask_diag_sparse_except_index(&mat, i as u16);
        let target_as_mat = vec_to_sparse_mat_fixed(&target, rows, false);
        assert_eq!(result.len(), target_as_mat.len());

        assert_sparse_mat_compare(&result, &target_as_mat, I32F32::from_num(0));
    }
}

#[test]
fn test_math_vec_mask_sparse_matrix() {
    let vector: Vec<f32> = vec![1., 2., 3., 4., 5., 6., 7., 8., 9.];
    let target: Vec<f32> = vec![0., 2., 3., 4., 0., 6., 7., 8., 0.];
    let mat = vec_to_sparse_mat_fixed(&vector, 3, false);
    let first_vector: Vec<u64> = vec![1, 2, 3];
    let second_vector: Vec<u64> = vec![1, 2, 3];
    let result = vec_mask_sparse_matrix(&mat, &first_vector, &second_vector, &|a, b| a == b);
    assert_sparse_mat_compare(
        &result,
        &vec_to_sparse_mat_fixed(&target, 3, false),
        I32F32::from_num(0),
    );
    let target: Vec<f32> = vec![1., 0., 0., 4., 5., 0., 7., 8., 9.];
    let mat = vec_to_sparse_mat_fixed(&vector, 3, false);
    let first_vector: Vec<u64> = vec![1, 2, 3];
    let second_vector: Vec<u64> = vec![1, 2, 3];
    let result = vec_mask_sparse_matrix(&mat, &first_vector, &second_vector, &|a, b| a < b);
    assert_sparse_mat_compare(
        &result,
        &vec_to_sparse_mat_fixed(&target, 3, false),
        I32F32::from_num(0),
    );
    let vector: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let target: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let mat = vec_to_sparse_mat_fixed(&vector, 3, false);
    let first_vector: Vec<u64> = vec![1, 2, 3];
    let second_vector: Vec<u64> = vec![1, 2, 3];
    let result = vec_mask_sparse_matrix(&mat, &first_vector, &second_vector, &|a, b| a == b);
    assert_sparse_mat_compare(
        &result,
        &vec_to_sparse_mat_fixed(&target, 3, false),
        I32F32::from_num(0),
    );
}

#[test]
fn test_math_vec_mul() {
    let vector: Vec<I32F32> = vec_to_fixed(&[1., 2., 3., 4.]);
    let target: Vec<I32F32> = vec_to_fixed(&[1., 4., 9., 16.]);
    let result = vec_mul(&vector, &vector);
    assert_vec_compare(&result, &target, I32F32::from_num(0));
    let vector_empty: Vec<I32F32> = vec_to_fixed(&[]);
    let result = vec_mul(&vector_empty, &vector);
    let target: Vec<I32F32> = vec![];
    assert_vec_compare(&result, &target, I32F32::from_num(0));
    let vector_zero: Vec<I32F32> = vec_to_fixed(&[0., 0., 0., 0., 0., 0., 0., 0.]);
    let result = vec_mul(&vector_zero, &vector);
    let target: Vec<I32F32> = vec![I32F32::from_num(0); 4];
    assert_vec_compare(&result, &target, I32F32::from_num(0));
}

#[test]
fn test_math_mat_vec_mul() {
    let matrix: Vec<f32> = vec![1., 2., 3., 4., 5., 6., 7., 8., 9., 10., 11., 12.];
    let matrix = vec_to_mat_fixed(&matrix, 4, false);
    let vector: Vec<I32F32> = vec_to_fixed(&[1., 2., 3.]);
    let target: Vec<f32> = vec![1., 4., 9., 4., 10., 18., 7., 16., 27., 10., 22., 36.];
    let target = vec_to_mat_fixed(&target, 4, false);
    let result = mat_vec_mul(&matrix, &vector);
    assert_mat_compare(&result, &target, I32F32::from_num(0));
    let vector_one: Vec<I32F32> = vec_to_fixed(&[1., 0., 0.]);
    let target: Vec<f32> = vec![1., 0., 0., 4., 0., 0., 7., 0., 0., 10., 0., 0.];
    let target = vec_to_mat_fixed(&target, 4, false);
    let result = mat_vec_mul(&matrix, &vector_one);
    assert_mat_compare(&result, &target, I32F32::from_num(0));
    let vector_empty: Vec<I32F32> = vec_to_fixed(&[]);
    let result = mat_vec_mul(&matrix, &vector_empty);
    let target: Vec<Vec<I32F32>> = vec![vec![]; 4];
    assert_mat_compare(&result, &target, I32F32::from_num(0));
}

#[test]
fn test_math_mat_vec_mul_sparse() {
    let matrix: Vec<f32> = vec![1., 2., 3., 4., 5., 6., 7., 8., 9., 10., 11., 12.];
    let matrix = vec_to_sparse_mat_fixed(&matrix, 4, false);
    let vector: Vec<I32F32> = vec_to_fixed(&[1., 2., 3.]);
    let target: Vec<f32> = vec![1., 4., 9., 4., 10., 18., 7., 16., 27., 10., 22., 36.];
    let target = vec_to_sparse_mat_fixed(&target, 4, false);
    let result = mat_vec_mul_sparse(&matrix, &vector);
    assert_sparse_mat_compare(&result, &target, I32F32::from_num(0));
    let vector_one: Vec<I32F32> = vec_to_fixed(&[1., 0., 0.]);
    let target: Vec<f32> = vec![1., 0., 0., 4., 0., 0., 7., 0., 0., 10., 0., 0.];
    let target = vec_to_sparse_mat_fixed(&target, 4, false);
    let result = mat_vec_mul_sparse(&matrix, &vector_one);
    assert_sparse_mat_compare(&result, &target, I32F32::from_num(0));
    let vector_empty: Vec<I32F32> = vec_to_fixed(&[]);
    let result = mat_vec_mul_sparse(&matrix, &vector_empty);
    let target = vec![vec![]; 4];
    assert_sparse_mat_compare(&result, &target, I32F32::from_num(0));
}

#[test]
fn test_math_row_hadamard() {
    let vector: Vec<I32F32> = vec_to_fixed(&[1., 2., 3., 4.]);
    let matrix: Vec<f32> = vec![1., 2., 3., 4., 5., 6., 7., 8., 9., 10., 11., 12.];
    let matrix = vec_to_mat_fixed(&matrix, 4, false);
    let result = row_hadamard(&matrix, &vector);
    let target: Vec<f32> = vec![1., 2., 3., 8., 10., 12., 21., 24., 27., 40., 44., 48.];
    let target = vec_to_mat_fixed(&target, 4, false);
    assert_mat_compare(&result, &target, I32F32::from_num(0));
}

#[test]
fn test_math_row_hadamard_sparse() {
    let vector: Vec<I32F32> = vec_to_fixed(&[1., 2., 3., 4.]);
    let matrix: Vec<f32> = vec![1., 2., 3., 4., 5., 6., 7., 8., 9., 10., 11., 12.];
    let matrix = vec_to_sparse_mat_fixed(&matrix, 4, false);
    let result = row_hadamard_sparse(&matrix, &vector);
    let target: Vec<f32> = vec![1., 2., 3., 8., 10., 12., 21., 24., 27., 40., 44., 48.];
    let target = vec_to_sparse_mat_fixed(&target, 4, false);
    assert_sparse_mat_compare(&result, &target, I32F32::from_num(0));
    let matrix: Vec<f32> = vec![0., 2., 3., 4., 0., 6., 7., 8., 0., 10., 11., 12.];
    let matrix = vec_to_sparse_mat_fixed(&matrix, 4, false);
    let result = row_hadamard_sparse(&matrix, &vector);
    let target: Vec<f32> = vec![0., 2., 3., 8., 0., 12., 21., 24., 0., 40., 44., 48.];
    let target = vec_to_sparse_mat_fixed(&target, 4, false);
    assert_sparse_mat_compare(&result, &target, I32F32::from_num(0));
    let matrix: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let matrix = vec_to_sparse_mat_fixed(&matrix, 4, false);
    let result = row_hadamard_sparse(&matrix, &vector);
    let target: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let target = vec_to_sparse_mat_fixed(&target, 4, false);
    assert_sparse_mat_compare(&result, &target, I32F32::from_num(0));
}

#[test]
fn test_math_row_sum() {
    let matrix: Vec<f32> = vec![1., 2., 3., 4., 5., 6., 7., 8., 9., 10., 11., 12.];
    let matrix = vec_to_mat_fixed(&matrix, 4, false);
    let result = row_sum(&matrix);
    let target: Vec<I32F32> = vec_to_fixed(&[6., 15., 24., 33.]);
    assert_vec_compare(&result, &target, I32F32::from_num(0));
}

#[test]
fn test_math_row_sum_sparse() {
    let matrix: Vec<f32> = vec![1., 2., 3., 4., 5., 6., 7., 8., 9., 10., 11., 12.];
    let matrix = vec_to_sparse_mat_fixed(&matrix, 4, false);
    let result = row_sum_sparse(&matrix);
    let target: Vec<I32F32> = vec_to_fixed(&[6., 15., 24., 33.]);
    assert_vec_compare(&result, &target, I32F32::from_num(0));
    let matrix: Vec<f32> = vec![0., 2., 3., 4., 0., 6., 7., 8., 0., 10., 11., 12.];
    let matrix = vec_to_sparse_mat_fixed(&matrix, 4, false);
    let result = row_sum_sparse(&matrix);
    let target: Vec<I32F32> = vec_to_fixed(&[5., 10., 15., 33.]);
    assert_vec_compare(&result, &target, I32F32::from_num(0));
    let matrix: Vec<f32> = vec![1., 2., 3., 0., 0., 0., 7., 8., 9., 10., 11., 12.];
    let matrix = vec_to_sparse_mat_fixed(&matrix, 4, false);
    let result = row_sum_sparse(&matrix);
    let target: Vec<I32F32> = vec_to_fixed(&[6., 0., 24., 33.]);
    assert_vec_compare(&result, &target, I32F32::from_num(0));
    let matrix: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let matrix = vec_to_sparse_mat_fixed(&matrix, 4, false);
    let result = row_sum_sparse(&matrix);
    let target: Vec<I32F32> = vec_to_fixed(&[0., 0., 0., 0.]);
    assert_vec_compare(&result, &target, I32F32::from_num(0));
}

#[test]
fn test_math_matmul() {
    let vector: Vec<I32F32> = vec_to_fixed(&[1., 2., 3., 4.]);
    let matrix: Vec<f32> = vec![1., 2., 3., 4., 5., 6., 7., 8., 9., 10., 11., 12.];
    let matrix = vec_to_mat_fixed(&matrix, 4, false);
    let result = matmul(&matrix, &vector);
    let target: Vec<I32F32> = vec_to_fixed(&[70., 80., 90.]);
    assert_vec_compare(&result, &target, I32F32::from_num(0));
}

#[test]
fn test_math_matmul_transpose() {
    let vector: Vec<I32F32> = vec_to_fixed(&[1., 2., 3.]);
    let matrix: Vec<f32> = vec![1., 2., 3., 4., 5., 6., 7., 8., 9., 10., 11., 12.];
    let matrix = vec_to_mat_fixed(&matrix, 4, false);
    let result = matmul_transpose(&matrix, &vector);
    let target: Vec<I32F32> = vec_to_fixed(&[14., 32., 50., 68.]);
    assert_vec_compare(&result, &target, I32F32::from_num(0));
}

#[test]
fn test_math_sparse_matmul() {
    let vector: Vec<I32F32> = vec_to_fixed(&[1., 2., 3., 4.]);
    let matrix: Vec<f32> = vec![1., 2., 3., 4., 5., 6., 7., 8., 9., 10., 11., 12.];
    let matrix = vec_to_sparse_mat_fixed(&matrix, 4, false);
    let result = matmul_sparse(&matrix, &vector, 3);
    let target: Vec<I32F32> = vec_to_fixed(&[70., 80., 90.]);
    assert_vec_compare(&result, &target, I32F32::from_num(0));
    let matrix: Vec<f32> = vec![0., 2., 3., 4., 0., 6., 7., 8., 0., 10., 11., 12.];
    let matrix = vec_to_sparse_mat_fixed(&matrix, 4, false);
    let result = matmul_sparse(&matrix, &vector, 3);
    let target: Vec<I32F32> = vec_to_fixed(&[69., 70., 63.]);
    assert_vec_compare(&result, &target, I32F32::from_num(0));
    let matrix: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let matrix = vec_to_sparse_mat_fixed(&matrix, 4, false);
    let result = matmul_sparse(&matrix, &vector, 3);
    let target: Vec<I32F32> = vec_to_fixed(&[0., 0., 0.]);
    assert_vec_compare(&result, &target, I32F32::from_num(0));
}

#[test]
fn test_math_sparse_matmul_transpose() {
    let vector: Vec<I32F32> = vec_to_fixed(&[1., 2., 3.]);
    let matrix: Vec<f32> = vec![1., 2., 3., 4., 5., 6., 7., 8., 9., 10., 11., 12.];
    let matrix = vec_to_sparse_mat_fixed(&matrix, 4, false);
    let result = matmul_transpose_sparse(&matrix, &vector);
    let target: Vec<I32F32> = vec_to_fixed(&[14., 32., 50., 68.]);
    assert_vec_compare(&result, &target, I32F32::from_num(0));
    let matrix: Vec<f32> = vec![0., 2., 3., 4., 0., 6., 7., 8., 0., 10., 11., 12.];
    let matrix = vec_to_sparse_mat_fixed(&matrix, 4, false);
    let result = matmul_transpose_sparse(&matrix, &vector);
    let target: Vec<I32F32> = vec_to_fixed(&[13., 22., 23., 68.]);
    assert_vec_compare(&result, &target, I32F32::from_num(0));
    let matrix: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let matrix = vec_to_sparse_mat_fixed(&matrix, 4, false);
    let result = matmul_transpose_sparse(&matrix, &vector);
    let target: Vec<I32F32> = vec_to_fixed(&[0., 0., 0., 0.]);
    assert_vec_compare(&result, &target, I32F32::from_num(0));
}

#[test]
fn test_math_inplace_col_clip() {
    let vector: Vec<I32F32> = vec_to_fixed(&[0., 5., 12.]);
    let matrix: Vec<f32> = vec![0., 2., 3., 4., 5., 6., 7., 8., 9., 10., 11., 12.];
    let mut matrix = vec_to_mat_fixed(&matrix, 4, false);
    let target: Vec<f32> = vec![0., 2., 3., 0., 5., 6., 0., 5., 9., 0., 5., 12.];
    let target = vec_to_mat_fixed(&target, 4, false);
    inplace_col_clip(&mut matrix, &vector);
    assert_mat_compare(&matrix, &target, I32F32::from_num(0));
}

#[test]
fn test_math_col_clip_sparse() {
    let vector: Vec<I32F32> = vec_to_fixed(&[0., 5., 12.]);
    let matrix: Vec<f32> = vec![0., 2., 3., 4., 5., 6., 7., 8., 9., 10., 11., 12.];
    let matrix = vec_to_sparse_mat_fixed(&matrix, 4, false);
    let target: Vec<f32> = vec![0., 2., 3., 0., 5., 6., 0., 5., 9., 0., 5., 12.];
    let target = vec_to_sparse_mat_fixed(&target, 4, false);
    let result = col_clip_sparse(&matrix, &vector);
    assert_sparse_mat_compare(&result, &target, I32F32::from_num(0));
    let matrix: Vec<f32> = vec![0., 2., 3., 4., 5., 6., 0., 0., 0., 10., 11., 12.];
    let matrix = vec_to_sparse_mat_fixed(&matrix, 4, false);
    let target: Vec<f32> = vec![0., 2., 3., 0., 5., 6., 0., 0., 0., 0., 5., 12.];
    let target = vec_to_sparse_mat_fixed(&target, 4, false);
    let result = col_clip_sparse(&matrix, &vector);
    assert_sparse_mat_compare(&result, &target, I32F32::from_num(0));
    let matrix: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let matrix = vec_to_sparse_mat_fixed(&matrix, 4, false);
    let target: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let target = vec_to_sparse_mat_fixed(&target, 4, false);
    let result = col_clip_sparse(&matrix, &vector);
    assert_sparse_mat_compare(&result, &target, I32F32::from_num(0));
}

#[test]
fn test_math_weighted_median() {
    let mut rng = rand::rng();
    let zero: I32F32 = fixed(0.);
    let one: I32F32 = fixed(1.);
    for _ in 0..100 {
        let stake: Vec<I32F32> = vec_to_fixed(&[]);
        let score: Vec<I32F32> = vec_to_fixed(&[]);
        let majority: I32F32 = fixed(0.51);
        assert_eq!(
            zero,
            weighted_median(
                &stake,
                &score,
                (0..stake.len()).collect::<Vec<_>>().as_slice(),
                one - majority,
                zero,
                stake.iter().sum()
            )
        );

        let stake: Vec<I32F32> = normalize(&vec_to_fixed(&[0.51]));
        let score: Vec<I32F32> = vec_to_fixed(&[1.]);
        let majority: I32F32 = fixed(0.51);
        assert_eq!(
            one,
            weighted_median(
                &stake,
                &score,
                (0..stake.len()).collect::<Vec<_>>().as_slice(),
                one - majority,
                zero,
                stake.iter().sum()
            )
        );

        let stake: Vec<I32F32> = vec_to_fixed(&[0.49, 0.51]);
        let score: Vec<I32F32> = vec_to_fixed(&[0.5, 1.]);
        let majority: I32F32 = fixed(0.51);
        assert_eq!(
            one,
            weighted_median(
                &stake,
                &score,
                (0..stake.len()).collect::<Vec<_>>().as_slice(),
                one - majority,
                zero,
                stake.iter().sum()
            )
        );

        let stake: Vec<I32F32> = vec_to_fixed(&[0.51, 0.49]);
        let score: Vec<I32F32> = vec_to_fixed(&[0.5, 1.]);
        let majority: I32F32 = fixed(0.51);
        assert_eq!(
            fixed(0.5),
            weighted_median(
                &stake,
                &score,
                (0..stake.len()).collect::<Vec<_>>().as_slice(),
                one - majority,
                zero,
                stake.iter().sum()
            )
        );

        let stake: Vec<I32F32> = vec_to_fixed(&[0.49, 0., 0.51]);
        let score: Vec<I32F32> = vec_to_fixed(&[0.5, 0.7, 1.]);
        let majority: I32F32 = fixed(0.51);
        assert_eq!(
            one,
            weighted_median(
                &stake,
                &score,
                (0..stake.len()).collect::<Vec<_>>().as_slice(),
                one - majority,
                zero,
                stake.iter().sum()
            )
        );

        let stake: Vec<I32F32> = vec_to_fixed(&[0.49, 0.01, 0.5]);
        let score: Vec<I32F32> = vec_to_fixed(&[0.5, 0.7, 1.]);
        let majority: I32F32 = fixed(0.51);
        assert_eq!(
            fixed(0.7),
            weighted_median(
                &stake,
                &score,
                (0..stake.len()).collect::<Vec<_>>().as_slice(),
                one - majority,
                zero,
                stake.iter().sum()
            )
        );

        let stake: Vec<I32F32> = vec_to_fixed(&[0.49, 0.51, 0.0]);
        let score: Vec<I32F32> = vec_to_fixed(&[0.5, 0.7, 1.]);
        let majority: I32F32 = fixed(0.51);
        assert_eq!(
            fixed(0.7),
            weighted_median(
                &stake,
                &score,
                (0..stake.len()).collect::<Vec<_>>().as_slice(),
                one - majority,
                zero,
                stake.iter().sum()
            )
        );

        let stake: Vec<I32F32> = vec_to_fixed(&[0.0, 0.49, 0.51]);
        let score: Vec<I32F32> = vec_to_fixed(&[0.5, 0.7, 1.]);
        let majority: I32F32 = fixed(0.51);
        assert_eq!(
            one,
            weighted_median(
                &stake,
                &score,
                (0..stake.len()).collect::<Vec<_>>().as_slice(),
                one - majority,
                zero,
                stake.iter().sum()
            )
        );

        let stake: Vec<I32F32> = vec_to_fixed(&[0.0, 0.49, 0.0, 0.51]);
        let score: Vec<I32F32> = vec_to_fixed(&[0.5, 0.5, 1., 1.]);
        let majority: I32F32 = fixed(0.51);
        assert_eq!(
            one,
            weighted_median(
                &stake,
                &score,
                (0..stake.len()).collect::<Vec<_>>().as_slice(),
                one - majority,
                zero,
                stake.iter().sum()
            )
        );

        let stake: Vec<I32F32> = vec_to_fixed(&[0.0, 0.49, 0.0, 0.51, 0.0]);
        let score: Vec<I32F32> = vec_to_fixed(&[0.5, 0.5, 1., 1., 0.5]);
        let majority: I32F32 = fixed(0.51);
        assert_eq!(
            one,
            weighted_median(
                &stake,
                &score,
                (0..stake.len()).collect::<Vec<_>>().as_slice(),
                one - majority,
                zero,
                stake.iter().sum()
            )
        );

        let stake: Vec<I32F32> = vec_to_fixed(&[0.2, 0.2, 0.2, 0.2, 0.2]);
        let score: Vec<I32F32> = vec_to_fixed(&[0.8, 0.2, 1., 0.6, 0.4]);
        let majority: I32F32 = fixed(0.51);
        assert_eq!(
            fixed(0.6),
            weighted_median(
                &stake,
                &score,
                (0..stake.len()).collect::<Vec<_>>().as_slice(),
                one - majority,
                zero,
                stake.iter().sum()
            )
        );

        let stake: Vec<I32F32> = vec_to_fixed(&[0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1]);
        let score: Vec<I32F32> = vec_to_fixed(&[0.8, 0.8, 0.2, 0.2, 1.0, 1.0, 0.6, 0.6, 0.4, 0.4]);
        let majority: I32F32 = fixed(0.51);
        assert_eq!(
            fixed(0.6),
            weighted_median(
                &stake,
                &score,
                (0..stake.len()).collect::<Vec<_>>().as_slice(),
                one - majority,
                zero,
                stake.iter().sum()
            )
        );

        let n: usize = 100;
        for majority in vec_to_fixed(&[
            0., 0.0000001, 0.25, 0.49, 0.49, 0.49, 0.5, 0.51, 0.51, 0.51, 0.9999999, 1.,
        ]) {
            for allow_equal in [false, true] {
                let mut stake: Vec<I32F32> = vec![];
                let mut score: Vec<I32F32> = vec![];
                let mut last_score: I32F32 = zero;
                for i in 0..n {
                    if allow_equal {
                        match rng.random_range(0..2) {
                            1 => stake.push(one),
                            _ => stake.push(zero),
                        }
                        if rng.random_range(0..2) == 1 {
                            last_score += one
                        }
                        score.push(last_score);
                    } else {
                        stake.push(one);
                        score.push(I32F32::from_num(i));
                    }
                }
                inplace_normalize(&mut stake);
                let total_stake: I32F32 = stake.iter().sum();
                let mut minority: I32F32 = total_stake - majority;
                if minority < zero {
                    minority = zero;
                }
                let mut medians: Vec<I32F32> = vec![];
                let mut median_stake: I32F32 = zero;
                let mut median_set = false;
                let mut stake_sum: I32F32 = zero;
                for i in 0..n {
                    stake_sum += stake[i];
                    if !median_set && stake_sum >= minority {
                        median_stake = stake_sum;
                        median_set = true;
                    }
                    if median_set {
                        if median_stake < stake_sum {
                            if median_stake == minority && !medians.contains(&score[i]) {
                                medians.push(score[i]);
                            }
                            break;
                        }
                        if !medians.contains(&score[i]) {
                            medians.push(score[i]);
                        }
                    }
                }
                if medians.is_empty() {
                    medians.push(zero);
                }
                let stake_idx: Vec<usize> = (0..stake.len()).collect();
                let result: I32F32 =
                    weighted_median(&stake, &score, &stake_idx, minority, zero, total_stake);
                assert!(medians.contains(&result));
                for _ in 0..10 {
                    let mut permuted_uids: Vec<usize> = (0..n).collect();
                    permuted_uids.shuffle(&mut rng);
                    stake = permuted_uids.iter().map(|&i| stake[i]).collect();
                    score = permuted_uids.iter().map(|&i| score[i]).collect();
                    let result: I32F32 =
                        weighted_median(&stake, &score, &stake_idx, minority, zero, total_stake);
                    assert!(medians.contains(&result));
                }
            }
        }
    }
}

#[test]
fn test_math_weighted_median_col() {
    let stake: Vec<I32F32> = vec_to_fixed(&[]);
    let weights: Vec<Vec<I32F32>> = vec![vec![]];
    let median: Vec<I32F32> = vec_to_fixed(&[]);
    assert_eq!(median, weighted_median_col(&stake, &weights, fixed(0.5)));

    let stake: Vec<I32F32> = vec_to_fixed(&[0., 0.]);
    let weights: Vec<f32> = vec![0., 0., 0., 0.];
    let weights: Vec<Vec<I32F32>> = vec_to_mat_fixed(&weights, 2, false);
    let median: Vec<I32F32> = vec_to_fixed(&[0., 0.]);
    assert_eq!(median, weighted_median_col(&stake, &weights, fixed(0.5)));

    let stake: Vec<I32F32> = vec_to_fixed(&[0., 0.75, 0.25, 0.]);
    let weights: Vec<f32> = vec![0., 0.1, 0., 0., 0.2, 0.4, 0., 0.3, 0.1, 0., 0.4, 0.5];
    let weights: Vec<Vec<I32F32>> = vec_to_mat_fixed(&weights, 4, false);
    let median: Vec<I32F32> = vec_to_fixed(&[0., 0.3, 0.4]);
    assert_eq!(median, weighted_median_col(&stake, &weights, fixed(0.24)));
    let median: Vec<I32F32> = vec_to_fixed(&[0., 0.2, 0.4]);
    assert_eq!(median, weighted_median_col(&stake, &weights, fixed(0.26)));
    let median: Vec<I32F32> = vec_to_fixed(&[0., 0.2, 0.1]);
    assert_eq!(median, weighted_median_col(&stake, &weights, fixed(0.76)));

    let stake: Vec<I32F32> = vec_to_fixed(&[0., 0.3, 0.2, 0.5]);
    let weights: Vec<f32> = vec![0., 0.1, 0., 0., 0.2, 0.4, 0., 0.3, 0.1, 0., 0., 0.5];
    let weights: Vec<Vec<I32F32>> = vec_to_mat_fixed(&weights, 4, false);
    let median: Vec<I32F32> = vec_to_fixed(&[0., 0., 0.4]);
    assert_eq!(median, weighted_median_col(&stake, &weights, fixed(0.51)));
}

#[test]
fn test_math_weighted_median_col_sparse() {
    let stake: Vec<I32F32> = vec_to_fixed(&[]);
    let weights: Vec<Vec<(u16, I32F32)>> = vec![vec![]];
    let median: Vec<I32F32> = vec_to_fixed(&[]);
    assert_eq!(
        median,
        weighted_median_col_sparse(&stake, &weights, 0, fixed(0.5))
    );

    let stake: Vec<I32F32> = vec_to_fixed(&[0., 0.]);
    let weights: Vec<f32> = vec![0., 0., 0., 0.];
    let weights: Vec<Vec<(u16, I32F32)>> = vec_to_sparse_mat_fixed(&weights, 2, false);
    let median: Vec<I32F32> = vec_to_fixed(&[0., 0.]);
    assert_eq!(
        median,
        weighted_median_col_sparse(&stake, &weights, 2, fixed(0.5))
    );

    let stake: Vec<I32F32> = vec_to_fixed(&[0., 0.75, 0.25, 0.]);
    let weights: Vec<f32> = vec![0., 0.1, 0., 0., 0.2, 0.4, 0., 0.3, 0.1, 0., 0.4, 0.5];
    let weights: Vec<Vec<(u16, I32F32)>> = vec_to_sparse_mat_fixed(&weights, 4, false);
    let median: Vec<I32F32> = vec_to_fixed(&[0., 0.3, 0.4]);
    assert_eq!(
        median,
        weighted_median_col_sparse(&stake, &weights, 3, fixed(0.24))
    );
    let median: Vec<I32F32> = vec_to_fixed(&[0., 0.2, 0.4]);
    assert_eq!(
        median,
        weighted_median_col_sparse(&stake, &weights, 3, fixed(0.26))
    );
    let median: Vec<I32F32> = vec_to_fixed(&[0., 0.2, 0.1]);
    assert_eq!(
        median,
        weighted_median_col_sparse(&stake, &weights, 3, fixed(0.76))
    );

    let stake: Vec<I32F32> = vec_to_fixed(&[0., 0.3, 0.2, 0.5]);
    let weights: Vec<f32> = vec![0., 0.1, 0., 0., 0.2, 0.4, 0., 0.3, 0.1, 0., 0., 0.5];
    let weights: Vec<Vec<(u16, I32F32)>> = vec_to_sparse_mat_fixed(&weights, 4, false);
    let median: Vec<I32F32> = vec_to_fixed(&[0., 0., 0.4]);
    assert_eq!(
        median,
        weighted_median_col_sparse(&stake, &weights, 3, fixed(0.51))
    );
}

#[test]
fn test_math_interpolate() {
    let mat1: Vec<Vec<I32F32>> = vec![vec![]];
    let mat2: Vec<Vec<I32F32>> = vec![vec![]];
    let target: Vec<Vec<I32F32>> = vec![vec![]];
    let ratio = I32F32::from_num(0);
    let result = interpolate(&mat1, &mat2, ratio);
    assert_mat_compare(&result, &target, I32F32::from_num(0));

    let mat1: Vec<Vec<I32F32>> = vec![vec![I32F32::from_num(0)]];
    let mat2: Vec<Vec<I32F32>> = vec![vec![I32F32::from_num(1)]];
    let target: Vec<Vec<I32F32>> = vec![vec![I32F32::from_num(0)]];
    let ratio = I32F32::from_num(0);
    let result = interpolate(&mat1, &mat2, ratio);
    assert_mat_compare(&result, &target, I32F32::from_num(0));

    let target: Vec<Vec<I32F32>> = vec![vec![I32F32::from_num(1)]];
    let ratio = I32F32::from_num(1);
    let result = interpolate(&mat1, &mat2, ratio);
    assert_mat_compare(&result, &target, I32F32::from_num(0));

    let mat1: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let mat2: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let target: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let mat1 = vec_to_mat_fixed(&mat1, 4, false);
    let mat2 = vec_to_mat_fixed(&mat2, 4, false);
    let ratio = I32F32::from_num(0);
    let target = vec_to_mat_fixed(&target, 4, false);
    let result = interpolate(&mat1, &mat2, ratio);
    assert_mat_compare(&result, &target, I32F32::from_num(0));

    let ratio = I32F32::from_num(1);
    let result = interpolate(&mat1, &mat2, ratio);
    assert_mat_compare(&result, &target, I32F32::from_num(0));

    let mat1: Vec<f32> = vec![1., 10., 100., 1000., 10000., 100000.];
    let mat2: Vec<f32> = vec![10., 100., 1000., 10000., 100000., 1000000.];
    let target: Vec<f32> = vec![1., 10., 100., 1000., 10000., 100000.];
    let mat1 = vec_to_mat_fixed(&mat1, 3, false);
    let mat2 = vec_to_mat_fixed(&mat2, 3, false);
    let ratio = I32F32::from_num(0);
    let target = vec_to_mat_fixed(&target, 3, false);
    let result = interpolate(&mat1, &mat2, ratio);
    assert_mat_compare(&result, &target, I32F32::from_num(0));

    let target: Vec<f32> = vec![9.1, 91., 910., 9100., 91000., 910000.];
    let ratio = I32F32::from_num(0.9);
    let target = vec_to_mat_fixed(&target, 3, false);
    let result = interpolate(&mat1, &mat2, ratio);
    assert_mat_compare(&result, &target, I32F32::from_num(0.0001));

    let mat1: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let mat2: Vec<f32> = vec![1., 1., 1., 1., 1., 1., 1., 1., 1., 1., 1., 1.];
    let target: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let mat1 = vec_to_mat_fixed(&mat1, 4, false);
    let mat2 = vec_to_mat_fixed(&mat2, 4, false);
    let ratio = I32F32::from_num(0);
    let target = vec_to_mat_fixed(&target, 4, false);
    let result = interpolate(&mat1, &mat2, ratio);
    assert_mat_compare(&result, &target, I32F32::from_num(0));

    let target: Vec<f32> = vec![
        0.000000001,
        0.000000001,
        0.000000001,
        0.000000001,
        0.000000001,
        0.000000001,
        0.000000001,
        0.000000001,
        0.000000001,
        0.000000001,
        0.000000001,
        0.000000001,
    ];
    let ratio = I32F32::from_num(0.000000001);
    let target = vec_to_mat_fixed(&target, 4, false);
    let result = interpolate(&mat1, &mat2, ratio);
    assert_mat_compare(&result, &target, I32F32::from_num(0));

    let target: Vec<f32> = vec![0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5];
    let ratio = I32F32::from_num(0.5);
    let target = vec_to_mat_fixed(&target, 4, false);
    let result = interpolate(&mat1, &mat2, ratio);
    assert_mat_compare(&result, &target, I32F32::from_num(0));

    let target: Vec<f32> = vec![
        0.999_999_9,
        0.999_999_9,
        0.999_999_9,
        0.999_999_9,
        0.999_999_9,
        0.999_999_9,
        0.999_999_9,
        0.999_999_9,
        0.999_999_9,
        0.999_999_9,
        0.999_999_9,
        0.999_999_9,
    ];
    let ratio = I32F32::from_num(0.9999998808);
    let target = vec_to_mat_fixed(&target, 4, false);
    let result = interpolate(&mat1, &mat2, ratio);
    assert_mat_compare(&result, &target, I32F32::from_num(0));

    let target: Vec<f32> = vec![1., 1., 1., 1., 1., 1., 1., 1., 1., 1., 1., 1.];
    let ratio = I32F32::from_num(1);
    let target = vec_to_mat_fixed(&target, 4, false);
    let result = interpolate(&mat1, &mat2, ratio);
    assert_mat_compare(&result, &target, I32F32::from_num(0));
}

#[test]
fn test_math_interpolate_sparse() {
    let mat1: Vec<Vec<(u16, I32F32)>> = vec![vec![]];
    let mat2: Vec<Vec<(u16, I32F32)>> = vec![vec![]];
    let target: Vec<Vec<(u16, I32F32)>> = vec![vec![]];
    let ratio = I32F32::from_num(0);
    let result = interpolate_sparse(&mat1, &mat2, 0, ratio);
    assert_sparse_mat_compare(&result, &target, I32F32::from_num(0));

    let mat1: Vec<f32> = vec![0.];
    let mat2: Vec<f32> = vec![1.];
    let target: Vec<f32> = vec![0.];
    let mat1 = vec_to_sparse_mat_fixed(&mat1, 1, false);
    let mat2 = vec_to_sparse_mat_fixed(&mat2, 1, false);
    let ratio = I32F32::from_num(0);
    let target = vec_to_sparse_mat_fixed(&target, 1, false);
    let result = interpolate_sparse(&mat1, &mat2, 1, ratio);
    assert_sparse_mat_compare(&result, &target, I32F32::from_num(0));

    let target: Vec<f32> = vec![0.5];
    let ratio = I32F32::from_num(0.5);
    let target = vec_to_sparse_mat_fixed(&target, 1, false);
    let result = interpolate_sparse(&mat1, &mat2, 1, ratio);
    assert_sparse_mat_compare(&result, &target, I32F32::from_num(0));

    let target: Vec<f32> = vec![1.];
    let ratio = I32F32::from_num(1);
    let target = vec_to_sparse_mat_fixed(&target, 1, false);
    let result = interpolate_sparse(&mat1, &mat2, 1, ratio);
    assert_sparse_mat_compare(&result, &target, I32F32::from_num(0));

    let mat1: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let mat2: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let target: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let mat1 = vec_to_sparse_mat_fixed(&mat1, 4, false);
    let mat2 = vec_to_sparse_mat_fixed(&mat2, 4, false);
    let ratio = I32F32::from_num(0);
    let target = vec_to_sparse_mat_fixed(&target, 4, false);
    let result = interpolate_sparse(&mat1, &mat2, 3, ratio);
    assert_sparse_mat_compare(&result, &target, I32F32::from_num(0));

    let ratio = I32F32::from_num(1);
    let result = interpolate_sparse(&mat1, &mat2, 3, ratio);
    assert_sparse_mat_compare(&result, &target, I32F32::from_num(0));

    let mat1: Vec<f32> = vec![1., 0., 100., 1000., 10000., 100000.];
    let mat2: Vec<f32> = vec![10., 100., 1000., 10000., 100000., 0.];
    let target: Vec<f32> = vec![1., 0., 100., 1000., 10000., 100000.];
    let mat1 = vec_to_sparse_mat_fixed(&mat1, 3, false);
    let mat2 = vec_to_sparse_mat_fixed(&mat2, 3, false);
    let ratio = I32F32::from_num(0);
    let target = vec_to_sparse_mat_fixed(&target, 3, false);
    let result = interpolate_sparse(&mat1, &mat2, 2, ratio);
    assert_sparse_mat_compare(&result, &target, I32F32::from_num(0));

    let target: Vec<f32> = vec![9.1, 90., 910., 9100., 91000., 10000.];
    let ratio = I32F32::from_num(0.9);
    let target = vec_to_sparse_mat_fixed(&target, 3, false);
    let result = interpolate_sparse(&mat1, &mat2, 2, ratio);
    assert_sparse_mat_compare(&result, &target, I32F32::from_num(0.0001));

    let mat1: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let mat2: Vec<f32> = vec![1., 1., 1., 1., 1., 1., 1., 1., 1., 1., 1., 1.];
    let target: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let mat1 = vec_to_sparse_mat_fixed(&mat1, 4, false);
    let mat2 = vec_to_sparse_mat_fixed(&mat2, 4, false);
    let ratio = I32F32::from_num(0);
    let target = vec_to_sparse_mat_fixed(&target, 4, false);
    let result = interpolate_sparse(&mat1, &mat2, 3, ratio);
    assert_sparse_mat_compare(&result, &target, I32F32::from_num(0));

    let target: Vec<f32> = vec![
        0.000000001,
        0.000000001,
        0.000000001,
        0.000000001,
        0.000000001,
        0.000000001,
        0.000000001,
        0.000000001,
        0.000000001,
        0.000000001,
        0.000000001,
        0.000000001,
    ];
    let ratio = I32F32::from_num(0.000000001);
    let target = vec_to_sparse_mat_fixed(&target, 4, false);
    let result = interpolate_sparse(&mat1, &mat2, 3, ratio);
    assert_sparse_mat_compare(&result, &target, I32F32::from_num(0));

    let target: Vec<f32> = vec![0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5];
    let ratio = I32F32::from_num(0.5);
    let target = vec_to_sparse_mat_fixed(&target, 4, false);
    let result = interpolate_sparse(&mat1, &mat2, 3, ratio);
    assert_sparse_mat_compare(&result, &target, I32F32::from_num(0));

    let target: Vec<f32> = vec![
        0.999_999_9,
        0.999_999_9,
        0.999_999_9,
        0.999_999_9,
        0.999_999_9,
        0.999_999_9,
        0.999_999_9,
        0.999_999_9,
        0.999_999_9,
        0.999_999_9,
        0.999_999_9,
        0.999_999_9,
    ];
    let ratio = I32F32::from_num(0.9999998808);
    let target = vec_to_sparse_mat_fixed(&target, 4, false);
    let result = interpolate_sparse(&mat1, &mat2, 3, ratio);
    assert_sparse_mat_compare(&result, &target, I32F32::from_num(0));

    let target: Vec<f32> = vec![1., 1., 1., 1., 1., 1., 1., 1., 1., 1., 1., 1.];
    let ratio = I32F32::from_num(1);
    let target = vec_to_sparse_mat_fixed(&target, 4, false);
    let result = interpolate_sparse(&mat1, &mat2, 3, ratio);
    assert_sparse_mat_compare(&result, &target, I32F32::from_num(0));
}

#[test]
fn test_math_mat_ema_alpha() {
    let old: Vec<f32> = vec![
        0.1, 0.2, 3., 0.04, 0.05, 0.06, 0.07, 0.08, 0.09, 0.10, 0.11, 0.12,
    ];
    let new: Vec<f32> = vec![1., 2., 3., 4., 5., 6., 7., 8., 9., 10., 11., 12.];
    let target: Vec<f32> = vec![
        0.19, 0.38, 1., 0.436, 0.545, 0.6539, 0.763, 0.8719, 0.981, 1., 1., 1.,
    ];

    let old = vec_to_mat_fixed(&old, 4, false);
    let new = vec_to_mat_fixed(&new, 4, false);
    let target = vec_to_mat_fixed(&target, 4, false);
    let alphas = vec_to_mat_fixed(&[0.1; 12], 4, false);
    let result = mat_ema_alpha(&new, &old, &alphas);
    assert_mat_compare(&result, &target, I32F32::from_num(1e-4));
    let old: Vec<f32> = vec![
        0.1, 0.2, 3., 0.04, 0.05, 0.06, 0.07, 0.08, 0.09, 0.10, 0.11, 0.12,
    ];
    let new: Vec<f32> = vec![
        10., 20., 30., 40., 50., 60., 70., 80., 90., 100., 110., 120.,
    ];
    let target: Vec<f32> = vec![
        0.10, 0.2, 1., 0.0399, 0.05, 0.0599, 0.07, 0.07999, 0.09, 0.1, 0.10999, 0.11999,
    ];
    let old = vec_to_mat_fixed(&old, 4, false);
    let new = vec_to_mat_fixed(&new, 4, false);
    let target = vec_to_mat_fixed(&target, 4, false);
    let alphas = vec_to_mat_fixed(&[0.; 12], 4, false);
    let result = mat_ema_alpha(&new, &old, &alphas);
    assert_mat_compare(&result, &target, I32F32::from_num(1e-4));
    let old: Vec<f32> = vec![
        0.001, 0.002, 0.003, 0.004, 0.05, 0.006, 0.007, 0.008, 0.009, 0.010, 0.011, 0.012,
    ];
    let new: Vec<f32> = vec![
        0.1, 0.2, 3., 0.04, 0.05, 0.06, 0.07, 0.08, 0.09, 0.10, 0.11, 0.12,
    ];
    let target: Vec<f32> = vec![
        0.10, 0.2, 1., 0.0399, 0.05, 0.0599, 0.07, 0.07999, 0.09, 0.1, 0.10999, 0.11999,
    ];

    let old = vec_to_mat_fixed(&old, 4, false);
    let new = vec_to_mat_fixed(&new, 4, false);
    let target = vec_to_mat_fixed(&target, 4, false);
    let alphas = vec_to_mat_fixed(&[1.; 12], 4, false);
    let result = mat_ema_alpha(&new, &old, &alphas);
    assert_mat_compare(&result, &target, I32F32::from_num(1e-4));
}

#[test]
fn test_math_sparse_mat_ema_alpha() {
    let old: Vec<f32> = vec![
        0.1, 0.2, 3., 0.04, 0.05, 0.06, 0.07, 0.08, 0.09, 0.10, 0.11, 0.12,
    ];
    let new: Vec<f32> = vec![1., 2., 3., 4., 5., 6., 7., 8., 9., 10., 11., 12.];
    let target: Vec<f32> = vec![
        0.19, 0.38, 1., 0.43599, 0.545, 0.65399, 0.763, 0.87199, 0.981, 1., 1., 1.,
    ];
    let old = vec_to_sparse_mat_fixed(&old, 4, false);
    let new = vec_to_sparse_mat_fixed(&new, 4, false);
    let target = vec_to_sparse_mat_fixed(&target, 4, false);
    let alphas = vec_to_mat_fixed(&[0.1; 12], 4, false);
    let result = mat_ema_alpha_sparse(&new, &old, &alphas);
    assert_sparse_mat_compare(&result, &target, I32F32::from_num(1e-4));
    let old: Vec<f32> = vec![
        0.001, 0.002, 0.003, 0.004, 0.05, 0.006, 0.007, 0.008, 0.009, 0.010, 0.011, 0.012,
    ];
    let new: Vec<f32> = vec![
        0.1, 0.2, 3., 0.04, 0.05, 0.06, 0.07, 0.08, 0.09, 0.10, 0.11, 0.12,
    ];
    let target: Vec<f32> = vec![
        0.0109, 0.0218, 0.30270, 0.007599, 0.05, 0.01139, 0.0133, 0.01519, 0.017, 0.01899, 0.02089,
        0.0227,
    ];
    let old = vec_to_sparse_mat_fixed(&old, 4, false);
    let new = vec_to_sparse_mat_fixed(&new, 4, false);
    let target = vec_to_sparse_mat_fixed(&target, 4, false);
    let alphas = vec_to_mat_fixed(&[0.1; 12], 4, false);
    let result = mat_ema_alpha_sparse(&new, &old, &alphas);
    assert_sparse_mat_compare(&result, &target, I32F32::from_num(1e-4));
    let old: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let new: Vec<f32> = vec![
        0.1, 0.2, 3., 0.04, 0.05, 0.06, 0.07, 0.08, 0.09, 0.10, 0.11, 0.12,
    ];
    let target: Vec<f32> = vec![
        0.01, 0.02, 0.3, 0.00399, 0.005, 0.00599, 0.007, 0.00799, 0.009, 0.01, 0.011, 0.01199,
    ];
    let old = vec_to_sparse_mat_fixed(&old, 4, false);
    let new = vec_to_sparse_mat_fixed(&new, 4, false);
    let target = vec_to_sparse_mat_fixed(&target, 4, false);
    let alphas = vec_to_mat_fixed(&[0.1; 12], 4, false);
    let result = mat_ema_alpha_sparse(&new, &old, &alphas);
    assert_sparse_mat_compare(&result, &target, I32F32::from_num(1e-4));
    let old: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let new: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let target: Vec<f32> = vec![0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let old = vec_to_sparse_mat_fixed(&old, 4, false);
    let new = vec_to_sparse_mat_fixed(&new, 4, false);
    let target = vec_to_sparse_mat_fixed(&target, 4, false);
    let alphas = vec_to_mat_fixed(&[0.1; 12], 4, false);
    let result = mat_ema_alpha_sparse(&new, &old, &alphas);
    assert_sparse_mat_compare(&result, &target, I32F32::from_num(1e-4));
    let old: Vec<f32> = vec![1., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.];
    let new: Vec<f32> = vec![0., 0., 0., 0., 2., 0., 0., 0., 0., 0., 0., 0.];
    let target: Vec<f32> = vec![0.0, 0., 0., 0., 0.2, 0., 0., 0., 0., 0., 0., 0.];
    let old = vec_to_sparse_mat_fixed(&old, 4, false);
    let new = vec_to_sparse_mat_fixed(&new, 4, false);
    let target = vec_to_sparse_mat_fixed(&target, 4, false);
    let alphas = vec_to_mat_fixed(&[0.1; 12], 4, false);
    let result = mat_ema_alpha_sparse(&new, &old, &alphas);
    assert_sparse_mat_compare(&result, &target, I32F32::from_num(1e-1));
}

#[test]
fn test_math_matmul2() {
    let epsilon: I32F32 = I32F32::from_num(0.0001);
    let w: Vec<Vec<I32F32>> = vec![vec![I32F32::from_num(1.0); 3]; 3];
    assert_vec_compare(
        &matmul(&w, &[I32F32::from_num(1.0); 3]),
        &[
            I32F32::from_num(3),
            I32F32::from_num(3),
            I32F32::from_num(3),
        ],
        epsilon,
    );
    assert_vec_compare(
        &matmul(&w, &[I32F32::from_num(2.0); 3]),
        &[
            I32F32::from_num(6),
            I32F32::from_num(6),
            I32F32::from_num(6),
        ],
        epsilon,
    );
    assert_vec_compare(
        &matmul(&w, &[I32F32::from_num(3.0); 3]),
        &[
            I32F32::from_num(9),
            I32F32::from_num(9),
            I32F32::from_num(9),
        ],
        epsilon,
    );
    assert_vec_compare(
        &matmul(&w, &[I32F32::from_num(-1.0); 3]),
        &[
            I32F32::from_num(-3),
            I32F32::from_num(-3),
            I32F32::from_num(-3),
        ],
        epsilon,
    );
    let w: Vec<Vec<I32F32>> = vec![vec![I32F32::from_num(-1.0); 3]; 3];
    assert_vec_compare(
        &matmul(&w, &[I32F32::from_num(1.0); 3]),
        &[
            I32F32::from_num(-3),
            I32F32::from_num(-3),
            I32F32::from_num(-3),
        ],
        epsilon,
    );
    assert_vec_compare(
        &matmul(&w, &[I32F32::from_num(2.0); 3]),
        &[
            I32F32::from_num(-6),
            I32F32::from_num(-6),
            I32F32::from_num(-6),
        ],
        epsilon,
    );
    assert_vec_compare(
        &matmul(&w, &[I32F32::from_num(3.0); 3]),
        &[
            I32F32::from_num(-9),
            I32F32::from_num(-9),
            I32F32::from_num(-9),
        ],
        epsilon,
    );
    assert_vec_compare(
        &matmul(&w, &[I32F32::from_num(-1.0); 3]),
        &[
            I32F32::from_num(3),
            I32F32::from_num(3),
            I32F32::from_num(3),
        ],
        epsilon,
    );
    let w: Vec<Vec<I32F32>> = vec![
        vec![I32F32::from_num(1.0); 3],
        vec![I32F32::from_num(2.0); 3],
        vec![I32F32::from_num(3.0); 3],
    ];
    assert_vec_compare(
        &matmul(&w, &[I32F32::from_num(0.0); 3]),
        &[
            I32F32::from_num(0.0),
            I32F32::from_num(0.0),
            I32F32::from_num(0.0),
        ],
        epsilon,
    );
    assert_vec_compare(
        &matmul(&w, &[I32F32::from_num(2.0); 3]),
        &[
            I32F32::from_num(12),
            I32F32::from_num(12),
            I32F32::from_num(12),
        ],
        epsilon,
    );
    let w: Vec<Vec<I32F32>> = vec![
        vec![
            I32F32::from_num(1),
            I32F32::from_num(2),
            I32F32::from_num(3)
        ];
        3
    ];
    assert_vec_compare(
        &matmul(&w, &[I32F32::from_num(0.0); 3]),
        &[
            I32F32::from_num(0.0),
            I32F32::from_num(0.0),
            I32F32::from_num(0.0),
        ],
        epsilon,
    );
    assert_vec_compare(
        &matmul(&w, &[I32F32::from_num(2.0); 3]),
        &[
            I32F32::from_num(6),
            I32F32::from_num(12),
            I32F32::from_num(18),
        ],
        epsilon,
    );
}

#[test]
fn test_math_fixed_to_u16() {
    let expected = u16::MIN;
    assert_eq!(fixed_to_u16(I32F32::from_num(expected)), expected);

    let expected = u16::MAX / 2;
    assert_eq!(fixed_to_u16(I32F32::from_num(expected)), expected);

    let expected = u16::MAX;
    assert_eq!(fixed_to_u16(I32F32::from_num(expected)), expected);
}

#[test]
#[should_panic(expected = "overflow")]
fn test_math_fixed_to_u16_panics() {
    let bad_input = I32F32::from_num(u32::MAX);
    fixed_to_u16(bad_input);

    let bad_input = I32F32::from_num(-1);
    fixed_to_u16(bad_input);
}

// TODO: Investigate why `I32F32` and not `I64F64`
#[test]
fn test_math_fixed_to_u64() {
    let expected = u64::MIN;
    assert_eq!(fixed_to_u64(I32F32::from_num(expected)), expected);

    // let expected = u64::MAX / 2;
    // assert_eq!(fixed_to_u64(I32F32::from_num(expected)), expected);

    // let expected = u64::MAX;
    // assert_eq!(fixed_to_u64(I32F32::from_num(expected)), expected);
}

#[test]
fn test_math_fixed_to_u64_saturates() {
    let bad_input = I32F32::from_num(-1);
    let expected = 0;
    assert_eq!(fixed_to_u64(bad_input), expected);
}

#[test]
fn test_math_fixed64_to_u64() {
    let expected = u64::MIN;
    let input = I64F64::from_num(expected);
    assert_eq!(fixed64_to_u64(input), expected);

    let input = i64::MAX / 2;
    let expected = u64::try_from(input).unwrap();
    assert_eq!(fixed64_to_u64(I64F64::from_num(input)), expected);

    let input = i64::MAX;
    let expected = u64::try_from(input).unwrap();
    assert_eq!(fixed64_to_u64(I64F64::from_num(input)), expected);
}

#[test]
fn test_math_fixed64_to_u64_saturates() {
    let bad_input = I64F64::from_num(-1);
    let expected = 0;
    assert_eq!(fixed64_to_u64(bad_input), expected);
}

/* @TODO: find the _true_ max, and half, input values */
#[test]
fn test_math_fixed64_to_fixed32() {
    let input = u64::MIN;
    let expected = u32::try_from(input).unwrap();
    assert_eq!(fixed64_to_fixed32(I64F64::from_num(expected)), expected);

    let expected = u32::MAX / 2;
    let input = u64::from(expected);
    assert_eq!(fixed64_to_fixed32(I64F64::from_num(input)), expected);
}

#[test]
fn test_math_fixed64_to_fixed32_saturates() {
    let bad_input = I64F64::from_num(u32::MAX);
    assert_eq!(fixed64_to_fixed32(bad_input), I32F32::max_value());
}

#[test]
fn test_math_u16_to_fixed() {
    let input = u16::MIN;
    let expected = I32F32::from_num(input);
    assert_eq!(u16_to_fixed(input), expected);

    let input = u16::MAX / 2;
    let expected = I32F32::from_num(input);
    assert_eq!(u16_to_fixed(input), expected);

    let input = u16::MAX;
    let expected = I32F32::from_num(input);
    assert_eq!(u16_to_fixed(input), expected);
}

#[test]
fn test_math_u16_proportion_to_fixed() {
    let input = u16::MIN;
    let expected = I32F32::from_num(input);
    assert_eq!(u16_proportion_to_fixed(input), expected);
}

#[test]
fn test_fixed_proportion_to_u16() {
    let expected = u16::MIN;
    let input = I32F32::from_num(expected);
    assert_eq!(fixed_proportion_to_u16(input), expected);
}

#[test]
fn test_fixed_proportion_to_u16_saturates() {
    let expected = u16::MAX;
    let input = I32F32::from_num(expected);
    log::trace!("Testing with input: {input:?}"); // Debug output
    let result = fixed_proportion_to_u16(input);
    log::trace!("Testing with result: {result:?}"); // Debug output
    assert_eq!(result, expected);
}

#[test]
fn test_vec_fixed64_to_fixed32() {
    let input = vec![I64F64::from_num(i32::MIN)];
    let expected = vec![I32F32::from_num(i32::MIN)];
    assert_eq!(vec_fixed64_to_fixed32(input), expected);

    let input = vec![I64F64::from_num(i32::MAX)];
    let expected = vec![I32F32::from_num(i32::MAX)];
    assert_eq!(vec_fixed64_to_fixed32(input), expected);
}

#[test]
fn test_vec_fixed64_to_fixed32_saturates() {
    let bad_input = vec![I64F64::from_num(i64::MAX)];
    assert_eq!(vec_fixed64_to_fixed32(bad_input), [I32F32::max_value()]);
}

#[test]
#[allow(arithmetic_overflow)]
fn test_checked_sum() {
    let overflowing_input = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, u64::MAX];
    // Expect None when overflow occurs
    assert_eq!(checked_sum(&overflowing_input), None);

    let normal_input = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
    // Expect Some when no overflow occurs
    assert_eq!(checked_sum(&normal_input), Some(55));

    let empty_input: Vec<u16> = vec![];
    // Expect Some(u16::default()) when input is empty
    assert_eq!(checked_sum(&empty_input), Some(u16::default()));

    let single_input = vec![1];
    // Expect Some(...) when input is a single value
    assert_eq!(checked_sum(&single_input), Some(1));
}

#[test]
fn test_mat_ema_alpha_sparse_empty() {
    let new: Vec<Vec<(u16, I32F32)>> = Vec::new();
    let old: Vec<Vec<(u16, I32F32)>> = Vec::new();
    let alpha: Vec<Vec<I32F32>> = Vec::new();
    let result = mat_ema_alpha_sparse(&new, &old, &alpha);
    assert_eq!(result, Vec::<Vec<(u16, I32F32)>>::new());
}

#[test]
fn test_mat_ema_alpha_sparse_single_element() {
    let new: Vec<Vec<(u16, I32F32)>> = vec![vec![(0, I32F32::from_num(1.0))]];
    let old: Vec<Vec<(u16, I32F32)>> = vec![vec![(0, I32F32::from_num(2.0))]];
    let alpha = vec![vec![I32F32::from_num(0.5)]];
    let result = mat_ema_alpha_sparse(&new, &old, &alpha);
    assert_eq!(result, vec![vec![(0, I32F32::from_num(1.0))]]);
}

#[test]
fn test_mat_ema_alpha_sparse_multiple_elements() {
    let new: Vec<Vec<(u16, I32F32)>> = vec![
        vec![(0, I32F32::from_num(1.0)), (1, I32F32::from_num(2.0))],
        vec![(0, I32F32::from_num(3.0)), (1, I32F32::from_num(4.0))],
    ];
    let old: Vec<Vec<(u16, I32F32)>> = vec![
        vec![(0, I32F32::from_num(5.0)), (1, I32F32::from_num(6.0))],
        vec![(0, I32F32::from_num(7.0)), (1, I32F32::from_num(8.0))],
    ];
    let alpha = vec![vec![I32F32::from_num(0.1), I32F32::from_num(0.2)]; 2];
    let result = mat_ema_alpha_sparse(&new, &old, &alpha);
    let expected = vec![
        vec![(0, I32F32::from_num(1.0)), (1, I32F32::from_num(1.0))],
        vec![(0, I32F32::from_num(1.0)), (1, I32F32::from_num(1.0))],
    ];
    assert_sparse_mat_compare(&result, &expected, I32F32::from_num(0.000001));
}

#[test]
fn test_mat_ema_alpha_sparse_zero_alpha() {
    let new: Vec<Vec<(u16, I32F32)>> = vec![vec![(0, I32F32::from_num(1.0))]];
    let old: Vec<Vec<(u16, I32F32)>> = vec![vec![(0, I32F32::from_num(2.0))]];
    let alpha = vec![vec![I32F32::from_num(0.1), I32F32::from_num(0.0)]];
    let result = mat_ema_alpha_sparse(&new, &old, &alpha);
    assert_eq!(result, vec![vec![(0, I32F32::from_num(1.0))]]);
}

#[test]
fn test_mat_ema_alpha_sparse_one_alpha() {
    let new: Vec<Vec<(u16, I32F32)>> = vec![vec![(0, I32F32::from_num(1.0))]];
    let old: Vec<Vec<(u16, I32F32)>> = vec![vec![(0, I32F32::from_num(2.0))]];
    let alpha = vec![vec![I32F32::from_num(1.0), I32F32::from_num(0.0)]];
    let result = mat_ema_alpha_sparse(&new, &old, &alpha);
    assert_eq!(result, vec![vec![(0, I32F32::from_num(1.0))]]);
}

#[test]
fn test_mat_ema_alpha_sparse_mixed_alpha() {
    let new: Vec<Vec<(u16, I32F32)>> = vec![
        vec![(0, I32F32::from_num(1.0)), (1, I32F32::from_num(2.0))],
        vec![(0, I32F32::from_num(3.0)), (1, I32F32::from_num(4.0))],
    ];
    let old: Vec<Vec<(u16, I32F32)>> = vec![
        vec![(0, I32F32::from_num(5.0)), (1, I32F32::from_num(6.0))],
        vec![(0, I32F32::from_num(7.0)), (1, I32F32::from_num(8.0))],
    ];
    let alpha = vec![vec![I32F32::from_num(0.3), I32F32::from_num(0.7)]; 2];
    let result = mat_ema_alpha_sparse(&new, &old, &alpha);
    assert_sparse_mat_compare(
        &result,
        &[
            vec![(0, I32F32::from_num(1.0)), (1, I32F32::from_num(1.0))],
            vec![(0, I32F32::from_num(1.0)), (1, I32F32::from_num(1.0))],
        ],
        I32F32::from_num(0.000001),
    );
}

#[test]
fn test_mat_ema_alpha_sparse_sparse_matrix() {
    let new: Vec<Vec<(u16, I32F32)>> = vec![
        vec![(0, I32F32::from_num(1.0))],
        vec![(1, I32F32::from_num(4.0))],
    ];
    let old: Vec<Vec<(u16, I32F32)>> = vec![
        vec![(0, I32F32::from_num(5.0))],
        vec![(1, I32F32::from_num(8.0))],
    ];
    let alpha = vec![vec![I32F32::from_num(0.5), I32F32::from_num(0.5)]; 2];
    let result = mat_ema_alpha_sparse(&new, &old, &alpha);
    assert_eq!(
        result,
        vec![
            vec![(0, I32F32::from_num(1.0))],
            vec![(1, I32F32::from_num(1.0))]
        ]
    );
}

#[test]
fn test_mat_ema_alpha_basic() {
    let new = mat_to_fixed(&[vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]);
    let old = mat_to_fixed(&[vec![0.5, 1.5, 2.5], vec![3.5, 4.5, 5.5]]);
    let alpha = vec![
        vec![
            I32F32::from_num(0.5),
            I32F32::from_num(0.5),
            I32F32::from_num(0.5),
        ];
        2
    ];
    let expected = mat_to_fixed(&[vec![0.75, 1.0, 1.0], vec![1.0, 1.0, 1.0]]);
    let result = mat_ema_alpha(&new, &old, &alpha);
    assert_eq!(result, expected);
}

#[test]
fn test_mat_ema_alpha_varying_alpha() {
    let new = mat_to_fixed(&[vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]);
    let old = mat_to_fixed(&[vec![0.5, 1.5, 2.5], vec![3.5, 4.5, 5.5]]);
    let alpha = vec![
        vec![
            I32F32::from_num(0.2),
            I32F32::from_num(0.5),
            I32F32::from_num(0.8),
        ];
        2
    ];
    let expected = mat_to_fixed(&[vec![0.6, 1.0, 1.0], vec![1.0, 1.0, 1.0]]);
    let result = mat_ema_alpha(&new, &old, &alpha);
    assert_mat_approx_eq(&result, &expected, I32F32::from_num(1e-6));
}

#[test]
fn test_mat_ema_alpha_sparse_varying_alpha() {
    let weights = vec![
        vec![(0, I32F32::from_num(0.1)), (1, I32F32::from_num(0.2))],
        vec![(0, I32F32::from_num(0.3)), (1, I32F32::from_num(0.4))],
    ];
    let bonds = vec![
        vec![(0, I32F32::from_num(0.5)), (1, I32F32::from_num(0.6))],
        vec![(0, I32F32::from_num(0.7)), (1, I32F32::from_num(0.8))],
    ];
    let alpha = vec![
        vec![I32F32::from_num(0.9), I32F32::from_num(0.8)],
        vec![I32F32::from_num(0.5), I32F32::from_num(0.7)],
    ];

    let expected = vec![
        vec![(0, I32F32::from_num(0.14)), (1, I32F32::from_num(0.28))],
        vec![
            (0, I32F32::from_num(0.499999)),
            (1, I32F32::from_num(0.519999)),
        ],
    ];

    let result = mat_ema_alpha_sparse(&weights, &bonds, &alpha);
    // Assert the results with an epsilon for approximate equality
    assert_sparse_mat_compare(&result, &expected, I32F32::from_num(1e-6));
}

#[test]
fn test_mat_ema_alpha_empty_matrices() {
    let new: Vec<Vec<I32F32>> = vec![];
    let old: Vec<Vec<I32F32>> = vec![];
    let alpha = vec![];
    let expected: Vec<Vec<I32F32>> = vec![vec![]; 1];
    let result = mat_ema_alpha(&new, &old, &alpha);
    assert_eq!(result, expected);
}

#[test]
fn test_mat_ema_alpha_single_element() {
    let new = mat_to_fixed(&[vec![1.0]]);
    let old = mat_to_fixed(&[vec![0.5]]);
    let alpha = vec![vec![I32F32::from_num(0.5)]];
    let expected = mat_to_fixed(&[vec![0.75]]);
    let result = mat_ema_alpha(&new, &old, &alpha);
    assert_eq!(result, expected);
}

#[test]
fn test_mat_ema_alpha_mismatched_dimensions() {
    let new = mat_to_fixed(&[vec![1.0, 2.0], vec![3.0, 4.0]]);
    let old = mat_to_fixed(&[vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]);
    let alpha = vec![
        vec![
            I32F32::from_num(0.5),
            I32F32::from_num(0.5),
            I32F32::from_num(0.5),
        ];
        2
    ];
    let result = mat_ema_alpha(&new, &old, &alpha);
    assert_eq!(result[0][0], old[0][0])
}
