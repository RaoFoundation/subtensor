#![allow(
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::indexing_slicing
)]
//! Tests for [`crate::epoch::math::vector_ops`].

use crate::epoch::math::*;
use substrate_fixed::{
    transcendental::exp,
    types::{I32F32, I64F64},
};

use super::helpers::*;

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
    assert_eq!(result, elementwise_safe_div(&x, &y));

    let x: Vec<I32F32> = vec_to_fixed(&[0., 1., 0., 1.]);
    let y: Vec<I32F32> = vec_to_fixed(&[0., 1., 1., 0.]);
    let result: Vec<I32F32> = vec_to_fixed(&[0., 1., 0., 0.]);
    assert_eq!(result, elementwise_safe_div(&x, &y));

    let x: Vec<I32F32> = vec_to_fixed(&[1., 1., 10.]);
    let y: Vec<I32F32> = vec_to_fixed(&[2., 3., 2.]);
    let result: Vec<I32F32> = vec![fixed(1.) / fixed(2.), fixed(1.) / fixed(3.), fixed(5.)];
    assert_eq!(result, elementwise_safe_div(&x, &y));
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
