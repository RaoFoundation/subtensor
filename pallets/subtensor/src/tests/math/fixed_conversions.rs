#![allow(
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::indexing_slicing
)]
//! Tests for [`crate::epoch::math::fixed_conversions`].

use crate::epoch::math::*;
use substrate_fixed::types::{I32F32, I64F64};

use super::helpers::*;
use substrate_fixed::types::{I96F32, I110F18};

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
