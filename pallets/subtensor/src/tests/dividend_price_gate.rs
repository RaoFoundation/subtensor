//! Admission arithmetic for the basket-wide dividend price-impact budget.
use crate::tests::mock::*;
use frame_support::{assert_err, assert_ok};

#[test]
fn weighted_dividend_gate_accepts_small_net_nav_effect() {
    // The report's example: 15 unstable holdings have a net +0.073% effect.
    assert_ok!(SubtensorModule::ensure_basket_dividend_price_impact(
        1_000_730, 1_000_000, 100_000, 100_000, 0,
    ));
    assert_err!(
        SubtensorModule::ensure_basket_dividend_price_impact(
            1_002_000, 1_000_000, 100_000, 100_000, 0,
        ),
        crate::Error::<Test>::SlippageTooHigh
    );
}

#[test]
fn weighted_dividend_gate_checks_joint_mint_error_and_origin_cancellation() {
    // Both independent budgets pass, but opposing moves exceed the 2% mint
    // budget. A denominator-only gate would incorrectly admit it.
    assert_err!(
        SubtensorModule::ensure_basket_dividend_price_impact(
            999_200, 1_000_000, 101_950, 100_000, 1_950,
        ),
        crate::Error::<Test>::SlippageTooHigh
    );
    // Opposing origin moves net to zero but redistribute deferred entitlements.
    assert_err!(
        SubtensorModule::ensure_basket_dividend_price_impact(
            1_000_000, 1_000_000, 100_000, 100_000, 2_001,
        ),
        crate::Error::<Test>::SlippageTooHigh
    );
}

#[test]
fn weighted_dividend_gate_accepts_reported_taostats_batch_shape() {
    // Mainnet 9,448,000: ~+8 bps NAV, -8.47 bps incoming net and
    // ~169.10 bps gross incoming impact. A 10-bps mint budget would still
    // freeze this batch even though the requested NAV gate admits it.
    assert_ok!(SubtensorModule::ensure_basket_dividend_price_impact(
        1_000_800, 1_000_000, 99_915, 100_000, 1_691,
    ));
}

#[test]
fn weighted_dividend_gate_handles_boundaries_and_large_references() {
    assert_ok!(SubtensorModule::ensure_basket_dividend_price_impact(
        1_001_000, 1_000_000, 100_000, 100_000, 0,
    ));
    assert_err!(
        SubtensorModule::ensure_basket_dividend_price_impact(
            1_001_001, 1_000_000, 100_000, 100_000, 0,
        ),
        crate::Error::<Test>::SlippageTooHigh
    );
    assert_ok!(SubtensorModule::ensure_basket_dividend_price_impact(
        u64::MAX,
        u128::from(u64::MAX),
        u64::MAX,
        u128::from(u64::MAX),
        0,
    ));
    assert_err!(
        SubtensorModule::ensure_basket_dividend_price_impact(
            u64::MAX,
            u128::MAX,
            u64::MAX,
            u128::MAX,
            0,
        ),
        crate::Error::<Test>::SlippageTooHigh
    );
    assert_ok!(SubtensorModule::ensure_basket_dividend_price_impact(
        0, 0, 1_000_000, 1_000_000, 0,
    ));
    assert_err!(
        SubtensorModule::ensure_basket_dividend_price_impact(0, 0, 2_000, 1_000, 1_000),
        crate::Error::<Test>::SlippageTooHigh
    );
}
