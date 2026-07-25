#![allow(
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::indexing_slicing
)]
//! Tests for [`crate::epoch::math::matmul_clip`].

use crate::epoch::math::*;
use substrate_fixed::types::I32F32;

use super::helpers::*;

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
