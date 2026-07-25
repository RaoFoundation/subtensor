#![allow(
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::indexing_slicing
)]
//! Tests for [`crate::epoch::math::weighted_median`].

use crate::epoch::math::*;
use substrate_fixed::types::I32F32;

use super::helpers::*;
use rand::{RngExt, seq::SliceRandom};

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
