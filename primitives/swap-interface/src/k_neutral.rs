//! K-neutrality burn math for lending readd (Jacob 2026-10-30; per-loan
//! readd target 2026-11-03).
//!
//! After a weight-preserving credit of a position's lift legs, Balancer
//! `L = x^{w_b} y^{w_q}` is typically above the per-loan target
//! `L(pre) + L(lift)`. Burn a common fraction of both reserves so that `L`
//! returns to that target while the post-credit spot (and weights) stay
//! fixed. Floors stop the burn from eating pre-credit reserves (user funds /
//! escrow never touched).
//!
//! Using summed `lift_tao`/`lift_alpha` across increases at different prices
//! slightly under-burns versus summing each increase's `L` (safe direction).

use substrate_fixed::{
    transcendental::{exp, ln},
    types::I64F64,
};

/// Perquintill accuracy — matches Balancer `ACCURACY` / `Perquintill` parts.
pub const WEIGHT_ACCURACY: u64 = 1_000_000_000_000_000_000;

/// Synthetic baseline reserves whose invariant equals `L(pre) + L(lift)`.
///
/// Scale pre-credit reserves by `s = 1 + L(lift)/L(pre)` (homogeneous of
/// degree 1). Pass the result to [`burn_amounts_to_invariant_baseline`] /
/// [`invariant_shortfall_tao`] as the target baseline.
pub fn loan_target_baseline_reserves(
    pre_tao: u64,
    pre_alpha: u64,
    lift_tao: u64,
    lift_alpha: u64,
    quote_weight_parts: u64,
) -> Option<(u64, u64)> {
    if pre_tao == 0 || pre_alpha == 0 {
        return None;
    }
    if lift_tao == 0 || lift_alpha == 0 {
        // Two-sided lift expected; a zero leg contributes L=0 → target = L(pre).
        return Some((pre_tao, pre_alpha));
    }
    let ratio = invariant_ratio(lift_tao, lift_alpha, pre_tao, pre_alpha, quote_weight_parts)?;
    let s = I64F64::from_num(1).saturating_add(ratio);
    // Ceil the scaled reserves so a clean proportional readd (post = pre+lift)
    // lands at-or-above target and burns 0 despite fixed-point drift. Slight
    // under-burn vs floor is the safe direction.
    let base_tao = s
        .saturating_mul(I64F64::from_num(pre_tao as i128))
        .checked_ceil()
        .map(|v| v.saturating_to_num::<u64>())
        .unwrap_or(u64::MAX)
        .max(1);
    let base_alpha = s
        .saturating_mul(I64F64::from_num(pre_alpha as i128))
        .checked_ceil()
        .map(|v| v.saturating_to_num::<u64>())
        .unwrap_or(u64::MAX)
        .max(1);
    Some((base_tao, base_alpha))
}

/// Compute `(burn_tao, burn_alpha)` that restore `L` to the baseline invariant
/// at the current spot price.
///
/// - `tao` / `alpha`: post-credit reserves (`y'`, `x'`).
/// - `quote_weight_parts`: Balancer `w_q` as `Perquintill::deconstruct()`.
/// - `baseline_tao` / `baseline_alpha`: target reserves defining
///   `L* = (x*)^{w_b}(y*)^{w_q}` (per-loan: pre scaled to `L(pre)+L(lift)`).
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

    let Some(sigma) = invariant_ratio(baseline_tao, baseline_alpha, tao, alpha, quote_weight_parts)
    else {
        return (0, 0);
    };

    // Already at or below baseline invariant (σ = L*/L' ≥ 1 ⇒ L' ≤ L*).
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

    // Ceil keep (round toward the pool) so fixed-point drift on a clean
    // proportional readd burns 0 rather than a few hundred rao of dust (N30).
    // Floor would over-burn and leave L slightly below target.
    let keep_tao = sigma_eff
        .saturating_mul(y)
        .checked_ceil()
        .map(|v| v.saturating_to_num::<u64>())
        .unwrap_or(tao)
        .clamp(floor_tao, tao);
    let keep_alpha = sigma_eff
        .saturating_mul(x)
        .checked_ceil()
        .map(|v| v.saturating_to_num::<u64>())
        .unwrap_or(alpha)
        .clamp(floor_alpha, alpha);

    (
        tao.saturating_sub(keep_tao),
        alpha.saturating_sub(keep_alpha),
    )
}

/// `L(num)/L(den) = (num_α/den_α)^{w_b} (num_τ/den_τ)^{w_q}`.
fn invariant_ratio(
    num_tao: u64,
    num_alpha: u64,
    den_tao: u64,
    den_alpha: u64,
    quote_weight_parts: u64,
) -> Option<I64F64> {
    if num_tao == 0 || num_alpha == 0 || den_tao == 0 || den_alpha == 0 {
        return None;
    }
    let acc = I64F64::from_num(WEIGHT_ACCURACY as i128);
    let w_q_parts = (quote_weight_parts as u128).min(WEIGHT_ACCURACY as u128);
    let w_b_parts = (WEIGHT_ACCURACY as u128).saturating_sub(w_q_parts);
    let w_q = I64F64::from_num(w_q_parts as i128).checked_div(acc)?;
    let w_b = I64F64::from_num(w_b_parts as i128).checked_div(acc)?;

    let ratio_x = I64F64::checked_from_num(num_alpha as i128)?
        .checked_div(I64F64::checked_from_num(den_alpha as i128)?)?;
    let ratio_y = I64F64::checked_from_num(num_tao as i128)?
        .checked_div(I64F64::checked_from_num(den_tao as i128)?)?;
    if ratio_x <= I64F64::from_num(0) || ratio_y <= I64F64::from_num(0) {
        return None;
    }
    let ln_x: I64F64 = ln(ratio_x).ok()?;
    let ln_y: I64F64 = ln(ratio_y).ok()?;
    let log_ratio = w_b
        .saturating_mul(ln_x)
        .saturating_add(w_q.saturating_mul(ln_y));
    exp(log_ratio).ok()
}

/// TAO value of a real Balancer-`L` shortfall versus the baseline invariant.
///
/// `σ = L*/L'`. When `σ ≤ 1` (`L' ≥ L*`) there is **no** shortfall — return 0 —
/// even if one reserve sits below its baseline counterpart after a price move.
/// When `σ > 1`, restoring `L*` at the live spot needs a proportional scale-up
/// of `(σ−1)` on both reserves; valued in TAO that is
/// `(σ−1)·(y + p·x)` with Balancer spot `p = (w_b/w_q)·(y/x)`.
pub fn invariant_shortfall_tao(
    tao: u64,
    alpha: u64,
    quote_weight_parts: u64,
    baseline_tao: u64,
    baseline_alpha: u64,
) -> u64 {
    if tao == 0 || alpha == 0 || baseline_tao == 0 || baseline_alpha == 0 {
        return 0;
    }
    let Some(sigma) = invariant_ratio(baseline_tao, baseline_alpha, tao, alpha, quote_weight_parts)
    else {
        return 0;
    };
    // L' ≥ L*: invariant restored (or in surplus). Per-reserve dips are not losses.
    if sigma <= I64F64::from_num(1) {
        return 0;
    }
    let acc = I64F64::from_num(WEIGHT_ACCURACY as i128);
    let w_q_parts = (quote_weight_parts as u128).min(WEIGHT_ACCURACY as u128);
    let w_b_parts = (WEIGHT_ACCURACY as u128).saturating_sub(w_q_parts);
    let Some(w_q) = I64F64::from_num(w_q_parts as i128).checked_div(acc) else {
        return 0;
    };
    let Some(w_b) = I64F64::from_num(w_b_parts as i128).checked_div(acc) else {
        return 0;
    };
    if w_q <= I64F64::from_num(0) {
        return 0;
    }
    let y = I64F64::from_num(tao as i128);
    let x = I64F64::from_num(alpha as i128);
    // p = (w_b / w_q) · (y / x)
    let Some(yx) = y.checked_div(x) else {
        return 0;
    };
    let Some(wb_over_wq) = w_b.checked_div(w_q) else {
        return 0;
    };
    let Some(price) = wb_over_wq.checked_mul(yx) else {
        return 0;
    };
    let pool_value = y.saturating_add(price.saturating_mul(x));
    let factor = sigma.saturating_sub(I64F64::from_num(1));
    factor.saturating_mul(pool_value).saturating_to_num::<u64>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_weight_burns_down_to_baseline_k() {
        // Baseline √(x y) = √(100*100) = 100.
        // Post-credit 121/121 → √ = 121; σ = 100/121; keep ≈ 100.
        let (bt, ba) =
            burn_amounts_to_invariant_baseline(121, 121, WEIGHT_ACCURACY / 2, 100, 100, 90, 90);
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
        let (bt, ba) =
            burn_amounts_to_invariant_baseline(90, 90, WEIGHT_ACCURACY / 2, 100, 100, 80, 80);
        assert_eq!((bt, ba), (0, 0));
    }

    #[test]
    fn skewed_weights_preserve_spot_ratio() {
        // w_q = 0.3, w_b = 0.7. Post-credit double both sides vs baseline →
        // L' = 2 L*; σ = 1/2; keep half.
        let w_q = (WEIGHT_ACCURACY as u128 * 3 / 10) as u64;
        let (bt, ba) = burn_amounts_to_invariant_baseline(200, 200, w_q, 100, 100, 50, 50);
        let keep_t = 200 - bt;
        let keep_a = 200 - ba;
        // Spot y/x unchanged.
        assert_eq!(keep_t, keep_a);
        // Within rounding of 100.
        assert!(keep_t.abs_diff(100) <= 1);
    }

    #[test]
    fn shortfall_zero_when_k_at_or_above_baseline() {
        // Price moved: tao below baseline, alpha above, but √(xy) = √(100·100).
        let w = WEIGHT_ACCURACY / 2;
        assert_eq!(invariant_shortfall_tao(80, 125, w, 100, 100), 0);
        assert_eq!(invariant_shortfall_tao(121, 121, w, 100, 100), 0);
    }

    #[test]
    fn shortfall_tao_valued_when_k_below_baseline() {
        // Equal weight: L' = 90, L* = 100 → σ = 100/90; V = 2·90; shortfall ≈ (σ−1)·V.
        let w = WEIGHT_ACCURACY / 2;
        let got = invariant_shortfall_tao(90, 90, w, 100, 100);
        let expect = ((100.0_f64 / 90.0) - 1.0) * 180.0;
        assert!(
            (got as f64 - expect).abs() <= 2.0,
            "got {got} expect ~{expect}"
        );
    }

    #[test]
    fn loan_target_baseline_scales_pre_by_one_plus_lift_ratio() {
        let w = WEIGHT_ACCURACY / 2;
        // Equal weight: L(lift)/L(pre) = √((10/100)*(10/100)) = 0.1 → s ≈ 1.1.
        let Some((bt, ba)) = loan_target_baseline_reserves(100, 100, 10, 10, w) else {
            panic!("loan target baseline should succeed for positive proportional inputs");
        };
        assert!((bt as i64 - 110).unsigned_abs() <= 2);
        assert!((ba as i64 - 110).unsigned_abs() <= 2);
        // N30: clean proportional readd (post = pre + lift) must burn 0 —
        // ceil keep rounds toward the pool so fixed-point drift cannot
        // dust-burn below target.
        let (burn_t, burn_a) = burn_amounts_to_invariant_baseline(110, 110, w, bt, ba, 100, 100);
        assert_eq!((burn_t, burn_a), (0, 0));
    }

    #[test]
    fn loan_target_burn_clears_surplus_above_pre_plus_lift() {
        let w = WEIGHT_ACCURACY / 2;
        let Some((bt, ba)) = loan_target_baseline_reserves(100, 100, 10, 10, w) else {
            panic!("loan target baseline should succeed for positive proportional inputs");
        };
        // Post has extra surplus (121) above target (~110).
        let (burn_t, burn_a) = burn_amounts_to_invariant_baseline(121, 121, w, bt, ba, 100, 100);
        let keep_t = 121 - burn_t;
        let keep_a = 121 - burn_a;
        assert!((keep_t as i64 - bt as i64).unsigned_abs() <= 1);
        assert!((keep_a as i64 - ba as i64).unsigned_abs() <= 1);
        assert!(burn_t >= 8 && burn_a >= 8, "must clear most of the surplus");
        assert!(burn_t <= 21 && burn_a <= 21);
    }
}
