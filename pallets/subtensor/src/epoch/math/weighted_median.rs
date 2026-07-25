//! Stake-weighted median consensus: scalar, dense column-wise, and sparse column-wise.

use super::{copy_at_or_default, inplace_normalize};
use safe_math::*;
use sp_std::vec;
use sp_std::vec::Vec;
use substrate_fixed::types::I32F32;

pub fn weighted_median(
    stake: &[I32F32],
    score: &[I32F32],
    partition_idx: &[usize],
    minority: I32F32,
    mut partition_lo: I32F32,
    mut partition_hi: I32F32,
) -> I32F32 {
    let zero = I32F32::saturating_from_num(0.0);
    if stake.len() != score.len() {
        log::error!(
            "math error: weighted_median stake and score have different lengths: {:?} != {:?}",
            stake.len(),
            score.len()
        );
        return zero;
    }
    let mut current_partition_index: Vec<usize> = partition_idx.to_vec();
    let mut iteration_counter: usize = 0;
    let iteration_limit = partition_idx.len();
    let mut lower: Vec<usize> = vec![];
    let mut upper: Vec<usize> = vec![];

    loop {
        let n = current_partition_index.len();
        if n == 0 {
            return zero;
        }
        if n == 1 {
            if let Some(&only_idx) = current_partition_index.first() {
                return copy_at_or_default::<I32F32>(score, only_idx);
            } else {
                return zero;
            }
        }
        let mid_idx: usize = n.safe_div(2);
        let pivot: I32F32 = copy_at_or_default::<I32F32>(
            score,
            current_partition_index.get(mid_idx).copied().unwrap_or(0),
        );
        let mut lo_stake: I32F32 = I32F32::saturating_from_num(0);
        let mut hi_stake: I32F32 = I32F32::saturating_from_num(0);

        for idx in current_partition_index.clone() {
            if copy_at_or_default::<I32F32>(score, idx) == pivot {
                continue;
            }
            if copy_at_or_default::<I32F32>(score, idx) < pivot {
                lo_stake = lo_stake.saturating_add(copy_at_or_default::<I32F32>(stake, idx));
                lower.push(idx);
            } else {
                hi_stake = hi_stake.saturating_add(copy_at_or_default::<I32F32>(stake, idx));
                upper.push(idx);
            }
        }
        if (minority < partition_lo.saturating_add(lo_stake)) && (!lower.is_empty()) {
            current_partition_index = lower.clone();
            partition_hi = partition_lo.saturating_add(lo_stake);
        } else if (partition_hi.saturating_sub(hi_stake) <= minority) && (!upper.is_empty()) {
            current_partition_index = upper.clone();
            partition_lo = partition_hi.saturating_sub(hi_stake);
        } else {
            return pivot;
        }

        lower.clear();
        upper.clear();

        // Safety limit: We should never need more than iteration_limit iterations.
        iteration_counter = iteration_counter.saturating_add(1);
        if iteration_counter > iteration_limit {
            break;
        }
    }
    zero
}

/// Column-wise weighted median, e.g. stake-weighted median scores per server (column) over all validators (rows).
pub fn weighted_median_col(
    stake: &[I32F32],
    score: &[Vec<I32F32>],
    majority: I32F32,
) -> Vec<I32F32> {
    let zero = I32F32::saturating_from_num(0.0);

    // Determine number of columns from the first row.
    let columns = score.first().map(|r| r.len()).unwrap_or(0);
    let mut median = vec![zero; columns];

    // Iterate columns into `median`.
    let mut c = 0usize;
    for med_cell in median.iter_mut() {
        let mut use_stake: Vec<I32F32> = Vec::new();
        let mut use_score: Vec<I32F32> = Vec::new();

        // Iterate rows aligned with `stake` length.
        let mut r = 0usize;
        while r < stake.len() {
            let st = copy_at_or_default::<I32F32>(stake, r);
            if st > zero {
                // Fetch row safely; if it's missing or has wrong width, push zeros to both.
                if let Some(row) = score.get(r) {
                    if row.len() == columns {
                        let val = row.get(c).copied().unwrap_or(zero);
                        use_stake.push(st);
                        use_score.push(val);
                    } else {
                        use_stake.push(zero);
                        use_score.push(zero);
                        log::error!(
                            "math error: weighted_median_col row.len() != columns: {:?} != {:?}",
                            row.len(),
                            columns
                        );
                    }
                } else {
                    // Missing row: insert zeroes.
                    use_stake.push(zero);
                    use_score.push(zero);
                }
            }
            r = r.saturating_add(1);
        }

        if !use_stake.is_empty() {
            inplace_normalize(&mut use_stake);
            let stake_sum: I32F32 = use_stake.iter().sum();
            let minority: I32F32 = stake_sum.saturating_sub(majority);

            let idxs: Vec<usize> = (0..use_stake.len()).collect();
            *med_cell = weighted_median(
                &use_stake,
                &use_score,
                idxs.as_slice(),
                minority,
                zero,
                stake_sum,
            );
        }

        c = c.saturating_add(1);
    }
    median
}

/// Column-wise weighted median, e.g. stake-weighted median scores per server (column) over all validators (rows).
pub fn weighted_median_col_sparse(
    stake: &[I32F32],
    score: &[Vec<(u16, I32F32)>],
    columns: u16,
    majority: I32F32,
) -> Vec<I32F32> {
    let zero = I32F32::saturating_from_num(0.0);

    // Keep only positive-stake rows; normalize them.
    let mut use_stake: Vec<I32F32> = stake.iter().copied().filter(|&s| s > zero).collect();
    inplace_normalize(&mut use_stake);

    let stake_sum: I32F32 = use_stake.iter().sum();
    let minority: I32F32 = stake_sum.saturating_sub(majority);
    let stake_idx: Vec<usize> = (0..use_stake.len()).collect();

    // use_score: columns x use_stake.len(), prefilled with zeros.
    let mut use_score: Vec<Vec<I32F32>> = (0..columns as usize)
        .map(|_| vec![zero; use_stake.len()])
        .collect();

    // Fill use_score by walking stake and score together, counting positives with k.
    let mut k: usize = 0;
    let mut stake_it = stake.iter();
    let mut score_it = score.iter();

    while let (Some(&s), Some(sparse_row)) = (stake_it.next(), score_it.next()) {
        if s > zero {
            for &(c, val) in sparse_row.iter() {
                if let Some(col_vec) = use_score.get_mut(c as usize)
                    && let Some(cell) = col_vec.get_mut(k)
                {
                    *cell = val;
                }
            }
            k = k.saturating_add(1);
        }
    }

    // Compute weighted median per column.
    let mut median: Vec<I32F32> = Vec::with_capacity(columns as usize);
    for col_vec in use_score.iter() {
        median.push(weighted_median(
            &use_stake,
            col_vec,
            stake_idx.as_slice(),
            minority,
            zero,
            stake_sum,
        ));
    }

    median
}

// Element-wise interpolation of two matrices: Result = A + ratio * (B - A).
// ratio has intended range [0, 1]
// ratio=0: Result = A
// ratio=1: Result = B
