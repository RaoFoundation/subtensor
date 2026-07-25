#![allow(
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::indexing_slicing
)]
//! Tests for [`crate::epoch::math::matrix_normalize_mask`].

use crate::epoch::math::*;
use substrate_fixed::types::I32F32;

use super::helpers::*;

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
