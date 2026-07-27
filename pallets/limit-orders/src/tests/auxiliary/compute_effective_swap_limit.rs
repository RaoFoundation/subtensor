//! Helper tests: `compute_effective_swap_limit`.

use super::*;

// ─────────────────────────────────────────────────────────────────────────────
// compute_effective_swap_limit
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn compute_effective_swap_limit_buy_no_slippage() {
    new_test_ext().execute_with(|| {
        // No slippage → u64::MAX (no ceiling).
        let limit = LimitOrders::<Test>::compute_effective_swap_limit(true, 1_000, None);
        assert_eq!(limit, u64::MAX);
    });
}

#[test]
fn compute_effective_swap_limit_sell_no_slippage() {
    new_test_ext().execute_with(|| {
        // No slippage → 0 (no floor).
        let limit = LimitOrders::<Test>::compute_effective_swap_limit(false, 1_000, None);
        assert_eq!(limit, 0);
    });
}

#[test]
fn compute_effective_swap_limit_buy_one_percent() {
    new_test_ext().execute_with(|| {
        // 1% slippage on a buy with limit_price=1000 → ceiling = 1010.
        let limit = LimitOrders::<Test>::compute_effective_swap_limit(
            true,
            1_000,
            Some(Perbill::from_percent(1)),
        );
        assert_eq!(limit, 1_010);
    });
}

#[test]
fn compute_effective_swap_limit_sell_one_percent() {
    new_test_ext().execute_with(|| {
        // 1% slippage on a sell with limit_price=1000 → floor = 990.
        let limit = LimitOrders::<Test>::compute_effective_swap_limit(
            false,
            1_000,
            Some(Perbill::from_percent(1)),
        );
        assert_eq!(limit, 990);
    });
}

#[test]
fn compute_effective_swap_limit_sell_saturates_at_zero() {
    new_test_ext().execute_with(|| {
        // 100% slippage on a sell with limit_price=500 → floor saturates at 0.
        let limit = LimitOrders::<Test>::compute_effective_swap_limit(
            false,
            500,
            Some(Perbill::from_percent(100)),
        );
        assert_eq!(limit, 0);
    });
}

#[test]
fn compute_effective_swap_limit_buy_saturates_at_u64_max() {
    new_test_ext().execute_with(|| {
        // 100% slippage on a buy with limit_price=u64::MAX → ceiling saturates at u64::MAX.
        let limit = LimitOrders::<Test>::compute_effective_swap_limit(
            true,
            u64::MAX,
            Some(Perbill::from_percent(100)),
        );
        assert_eq!(limit, u64::MAX);
    });
}
