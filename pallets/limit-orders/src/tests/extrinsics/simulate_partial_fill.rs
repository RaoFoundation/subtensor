//! Extrinsic tests: simulate partial fill.

use super::*;

// ─────────────────────────────────────────────────────────────────────────────
// MOCK_SIMULATE_PARTIAL_FILL — sim-swap detects partial fill before funds move
// ─────────────────────────────────────────────────────────────────────────────

/// `execute_batched_orders` hard-fails the whole batch when the sim-swap for a
/// `LimitBuy` order detects a partial fill (price limit would stop the AMM
/// before consuming the full input).
#[test]
fn execute_batched_orders_buy_partial_fill_fails_batch() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_simulate_partial_fill(true);

        let order = make_signed_order_with_slippage(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            u64::MAX, // limit_price always passes for a buy
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            Some(Perbill::from_parts(1)), // slippage field set; mock ignores value
        );

        assert_noop!(
            LimitOrders::execute_batched_orders(
                RuntimeOrigin::signed(charlie()),
                netuid(),
                bounded(vec![order]),
            ),
            DispatchError::Other("slippage too high")
        );
    });
}

/// `execute_orders` silently skips a `LimitBuy` order when the sim-swap detects
/// a partial fill: the order must not appear in storage and an `OrderSkipped`
/// event must be emitted.
#[test]
fn execute_orders_buy_partial_fill_skips_order() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_simulate_partial_fill(true);

        let order = make_signed_order_with_slippage(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            u64::MAX, // limit_price always passes for a buy
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            Some(Perbill::from_parts(1)), // slippage field set; mock ignores value
        );
        let id = order_id(&order.order);

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![order]),
            false,
        ));

        // Order must not be stored — it was skipped, not fulfilled.
        assert!(Orders::<Test>::get(id).is_none());

        // An OrderSkipped event must have been emitted for this order.
        assert!(
            System::events().iter().any(|r| matches!(
                &r.event,
                RuntimeEvent::LimitOrders(Event::OrderSkipped { order_id, .. })
                    if *order_id == id
            )),
            "expected OrderSkipped event for this order"
        );
    });
}

/// `execute_batched_orders` hard-fails the whole batch when the sim-swap for a
/// `TakeProfit` (sell) order detects a partial fill.
#[test]
fn execute_batched_orders_sell_partial_fill_fails_batch() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_simulate_partial_fill(true);
        // Seed alpha so the order passes the balance check before reaching the swap.
        MockSwap::set_alpha_balance(alice(), bob(), netuid(), 1_000);

        let order = make_signed_order_with_slippage(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::TakeProfit,
            1_000,
            0, // limit_price = 0 → floor always passes for a TakeProfit
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            Some(Perbill::from_parts(1)), // slippage field set; mock ignores value
        );

        assert_noop!(
            LimitOrders::execute_batched_orders(
                RuntimeOrigin::signed(charlie()),
                netuid(),
                bounded(vec![order]),
            ),
            DispatchError::Other("slippage too high")
        );
    });
}

/// `execute_orders` silently skips a `TakeProfit` order when the sim-swap
/// detects a partial fill: the order must not appear in storage and an
/// `OrderSkipped` event must be emitted.
#[test]
fn execute_orders_sell_partial_fill_skips_order() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_simulate_partial_fill(true);
        // Seed alpha so the order passes the balance check before reaching the swap.
        MockSwap::set_alpha_balance(alice(), bob(), netuid(), 1_000);

        let order = make_signed_order_with_slippage(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::TakeProfit,
            1_000,
            0, // limit_price = 0 → floor always passes for a TakeProfit
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            Some(Perbill::from_parts(1)), // slippage field set; mock ignores value
        );
        let id = order_id(&order.order);

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![order]),
            false,
        ));

        // Order must not be stored — it was skipped, not fulfilled.
        assert!(Orders::<Test>::get(id).is_none());

        // An OrderSkipped event must have been emitted for this order.
        assert!(
            System::events().iter().any(|r| matches!(
                &r.event,
                RuntimeEvent::LimitOrders(Event::OrderSkipped { order_id, .. })
                    if *order_id == id
            )),
            "expected OrderSkipped event for this order"
        );
    });
}
