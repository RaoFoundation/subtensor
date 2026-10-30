//! K-neutrality burn math for lending readd (Jacob 2026-10-30).
//!
//! After a weight-preserving full credit of lift legs, Balancer
//! `L = x^{w_b} y^{w_q}` is typically above the pre-lending baseline. Burn a
//! common fraction of both reserves so that `L` returns to the baseline while
//! the post-credit spot (and weights) stay fixed. Floors stop the burn from
//! eating pre-credit reserves (user funds / escrow never touched).

use substrate_fixed::{
    transcendental::{exp, ln},
    types::I64F64,
};

/// Perquintill accuracy — matches Balancer `ACCURACY` / `Perquintill` parts.
pub const WEIGHT_ACCURACY: u64 = 1_000_000_000_000_000_000;

/// Compute `(burn_tao, burn_alpha)` that restore `L` to the baseline invariant
/// at the current spot price.
///
/// - `tao` / `alpha`: post-credit reserves (`y'`, `x'`).
/// - `quote_weight_parts`: Balancer `w_q` as `Perquintill::deconstruct()`.
/// - `baseline_tao` / `baseline_alpha`: reserves snapshotted when the lift book
///   last went idle→busy (defines `L* = (x*)^{w_b}(y*)^{w_q}`).
/// - `floor_tao` / `floor_alpha`: pre-credit reserves; burn never takes either
///   side below these.
///
/// Formula: `σ = (x*/x')^{w_b} (y*/y')^{w_q}`; keep `σ` of each reserve
/// (clamped so keep ≥ floor); burn the rest. If `σ ≥ 1` (`L' ≤ L*`), burn
/// nothing.
pub fn burn_amounts_to_invariant_baseline(
    tao: u64,
    alpha: u64,
    quote_weight_parts: u64,
    baseline_tao: u64,
    baseline_alpha: u64,
    floor_tao: u64,
    floor_alpha: u64,
) -> (u64, u64) {
    if tao == 0 || alpha == 0 || baseline_tao == 0 || baseline_alpha == 0 {
        return (0, 0);
    }
    let floor_tao = floor_tao.min(tao);
    let floor_alpha = floor_alpha.min(alpha);
    if tao == floor_tao && alpha == floor_alpha {
        return (0, 0);
    }

    let Some(sigma) = sigma_to_baseline(
        tao,
        alpha,
        quote_weight_parts,
        baseline_tao,
        baseline_alpha,
    ) else {
        return (0, 0);
    };

    // Already at or below baseline invariant.
    if sigma >= I64F64::from_num(1) {
        return (0, 0);
    }

    let y = I64F64::from_num(tao as i128);
    let x = I64F64::from_num(alpha as i128);
    // Cap σ so proportional keep stays ≥ floors (preserves spot).
    let sigma_floor_t = I64F64::from_num(floor_tao as i128)
        .checked_div(y)
        .unwrap_or_else(|| I64F64::from_num(1));
    let sigma_floor_a = I64F64::from_num(floor_alpha as i128)
        .checked_div(x)
        .unwrap_or_else(|| I64F64::from_num(1));
    let mut sigma_eff = sigma;
    if sigma_floor_t > sigma_eff {
        sigma_eff = sigma_floor_t;
    }
    if sigma_floor_a > sigma_eff {
        sigma_eff = sigma_floor_a;
    }
    if sigma_eff >= I64F64::from_num(1) {
        return (0, 0);
    }

    let keep_tao = sigma_eff
        .saturating_mul(y)
        .saturating_to_num::<u64>()
        .clamp(floor_tao, tao);
    let keep_alpha = sigma_eff
        .saturating_mul(x)
        .saturating_to_num::<u64>()
        .clamp(floor_alpha, alpha);

    (
        tao.saturating_sub(keep_tao),
        alpha.saturating_sub(keep_alpha),
    )
}

fn sigma_to_baseline(
    tao: u64,
    alpha: u64,
    quote_weight_parts: u64,
    baseline_tao: u64,
    baseline_alpha: u64,
) -> Option<I64F64> {
    let acc = I64F64::from_num(WEIGHT_ACCURACY as i128);
    let w_q_parts = (quote_weight_parts as u128).min(WEIGHT_ACCURACY as u128);
    let w_b_parts = (WEIGHT_ACCURACY as u128).saturating_sub(w_q_parts);
    let w_q = I64F64::from_num(w_q_parts as i128).checked_div(acc)?;
    let w_b = I64F64::from_num(w_b_parts as i128).checked_div(acc)?;

    let ratio_x = I64F64::checked_from_num(baseline_alpha as i128)?
        .checked_div(I64F64::checked_from_num(alpha as i128)?)?;
    let ratio_y = I64F64::checked_from_num(baseline_tao as i128)?
        .checked_div(I64F64::checked_from_num(tao as i128)?)?;
    if ratio_x <= I64F64::from_num(0) || ratio_y <= I64F64::from_num(0) {
        return None;
    }
    let ln_x: I64F64 = ln(ratio_x).ok()?;
    let ln_y: I64F64 = ln(ratio_y).ok()?;
    let log_sigma = w_b.saturating_mul(ln_x).saturating_add(w_q.saturating_mul(ln_y));
    exp(log_sigma).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_weight_burns_down_to_baseline_k() {
        // Baseline √(x y) = √(100*100) = 100.
        // Post-credit 121/121 → √ = 121; σ = 100/121; keep ≈ 100.
        let (bt, ba) = burn_amounts_to_invariant_baseline(
            121,
            121,
            WEIGHT_ACCURACY / 2,
            100,
            100,
            90,
            90,
        );
        // Floor of σ·reserve may undershoot by 1 U64F64 quantum.
        assert!((121 - bt).abs_diff(100) <= 1);
        assert!((121 - ba).abs_diff(100) <= 1);
        assert_eq!(bt, ba, "equal-weight proportional burn");
    }

    #[test]
    fn floor_prevents_burning_precredit_reserves() {
        // Ideal keep would be below floor → burn nothing past floor.
        let (bt, ba) = burn_amounts_to_invariant_baseline(
            110,
            110,
            WEIGHT_ACCURACY / 2,
            50,
            50,
            100, // floor leaves only 10 of credit
            100,
        );
        assert!(bt <= 10);
        assert!(ba <= 10);
        assert_eq!(110 - bt, 100);
        assert_eq!(110 - ba, 100);
    }

    #[test]
    fn no_burn_when_already_at_or_below_baseline() {
        let (bt, ba) = burn_amounts_to_invariant_baseline(
            90,
            90,
            WEIGHT_ACCURACY / 2,
            100,
            100,
            80,
            80,
        );
        assert_eq!((bt, ba), (0, 0));
    }

    #[test]
    fn skewed_weights_preserve_spot_ratio() {
        // w_q = 0.3, w_b = 0.7. Post-credit double both sides vs baseline →
        // L' = 2 L*; σ = 1/2; keep half.
        let w_q = (WEIGHT_ACCURACY as u128 * 3 / 10) as u64;
        let (bt, ba) = burn_amounts_to_invariant_baseline(
            200,
            200,
            w_q,
            100,
            100,
            50,
            50,
        );
        let keep_t = 200 - bt;
        let keep_a = 200 - ba;
        // Spot y/x unchanged.
        assert_eq!(keep_t, keep_a);
        // Within rounding of 100.
        assert!(keep_t.abs_diff(100) <= 1);
    }
}
