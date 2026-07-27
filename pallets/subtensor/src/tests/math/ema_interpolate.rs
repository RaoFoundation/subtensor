#![allow(
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::indexing_slicing
)]
//! Tests for [`crate::epoch::math::ema_interpolate`].

use crate::epoch::math::*;
use substrate_fixed::types::I32F32;

use super::helpers::*;

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
