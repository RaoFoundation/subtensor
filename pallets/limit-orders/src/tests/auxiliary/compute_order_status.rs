//! Helper tests: `compute_order_status`.

use super::*;

// ─────────────────────────────────────────────────────────────────────────────
// compute_order_status
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn compute_order_status_no_partial_fill_returns_fulfilled() {
    new_test_ext().execute_with(|| {
        let id = H256::repeat_byte(1);
        // No existing state, no partial fill → Fulfilled immediately.
        let status = LimitOrders::<Test>::compute_order_status(id, None, 1_000);
        assert_eq!(status, OrderStatus::Fulfilled);
    });
}

#[test]
fn compute_order_status_partial_fill_below_total_returns_partially_filled() {
    new_test_ext().execute_with(|| {
        let id = H256::repeat_byte(2);
        // First partial fill of 400 on a 1000-unit order → PartiallyFilled(400).
        let status = LimitOrders::<Test>::compute_order_status(id, Some(400), 1_000);
        assert_eq!(status, OrderStatus::PartiallyFilled(400));
    });
}

#[test]
fn compute_order_status_partial_fill_exact_total_returns_fulfilled() {
    new_test_ext().execute_with(|| {
        let id = H256::repeat_byte(3);
        // Single partial fill that equals the full order amount → Fulfilled.
        let status = LimitOrders::<Test>::compute_order_status(id, Some(1_000), 1_000);
        assert_eq!(status, OrderStatus::Fulfilled);
    });
}

#[test]
fn compute_order_status_accumulates_previous_partial_fill() {
    new_test_ext().execute_with(|| {
        let id = H256::repeat_byte(4);
        // Pre-seed storage as if a prior partial fill of 300 already happened.
        Orders::<Test>::insert(id, OrderStatus::PartiallyFilled(300));

        // Second fill of 400 → 300 + 400 = 700, still below 1000.
        let status = LimitOrders::<Test>::compute_order_status(id, Some(400), 1_000);
        assert_eq!(status, OrderStatus::PartiallyFilled(700));
    });
}

#[test]
fn compute_order_status_completes_order_when_accumulated_total_reaches_amount() {
    new_test_ext().execute_with(|| {
        let id = H256::repeat_byte(5);
        Orders::<Test>::insert(id, OrderStatus::PartiallyFilled(600));

        // Fill the remaining 400 → 600 + 400 = 1000 = order_amount → Fulfilled.
        let status = LimitOrders::<Test>::compute_order_status(id, Some(400), 1_000);
        assert_eq!(status, OrderStatus::Fulfilled);
    });
}

#[test]
fn compute_order_status_ignores_fulfilled_storage_when_no_partial_fill() {
    new_test_ext().execute_with(|| {
        let id = H256::repeat_byte(6);
        // If somehow called with no partial_fill regardless of what's in storage
        // (should not happen in practice) it still returns Fulfilled.
        Orders::<Test>::insert(id, OrderStatus::PartiallyFilled(500));
        let status = LimitOrders::<Test>::compute_order_status(id, None, 1_000);
        assert_eq!(status, OrderStatus::Fulfilled);
    });
}
