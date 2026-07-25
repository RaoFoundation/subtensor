//! Extrinsic tests: partial fill.

use super::*;

// ─────────────────────────────────────────────────────────────────────────────
// Partial fills — execute_orders
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn execute_orders_partial_fill_sets_partially_filled_status() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_tao_balance(alice(), 1_000);

        // Order for 1000 TAO; relayer is charlie (required for partial fills).
        let signed = make_partial_fill_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            u64::MAX,
            FAR_FUTURE,
            charlie(),
            400, // fill 400 out of 1000
        );
        let id = order_id(&signed.order);

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![signed]),
            false,
        ));

        assert_eq!(
            Orders::<Test>::get(id),
            Some(OrderStatus::PartiallyFilled(400))
        );
    });
}

#[test]
fn execute_orders_second_partial_fill_completes_order() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_tao_balance(alice(), 1_000);

        let signed_first = make_partial_fill_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            u64::MAX,
            FAR_FUTURE,
            charlie(),
            600,
        );
        let id = order_id(&signed_first.order);

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![signed_first.clone()]),
            false,
        ));
        assert_eq!(
            Orders::<Test>::get(id),
            Some(OrderStatus::PartiallyFilled(600))
        );

        // Re-submit the same signed order payload with a different partial_fill amount.
        let mut signed_second = signed_first.clone();
        signed_second.partial_fill = Some(400);

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![signed_second]),
            false,
        ));
        assert_eq!(Orders::<Test>::get(id), Some(OrderStatus::Fulfilled));
    });
}

#[test]
fn execute_orders_partial_fill_without_relayer_skipped() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_tao_balance(alice(), 1_000);

        // Build an order with partial_fills_enabled but no relayer set.
        let inner = crate::Order {
            signer: alice(),
            hotkey: bob(),
            netuid: netuid(),
            order_type: OrderType::LimitBuy,
            amount: 1_000,
            limit_price: u64::MAX,
            expiry: FAR_FUTURE,
            fee_rate: Perbill::zero(),
            fee_recipient: fee_recipient(),
            relayer: None, // <-- no relayer
            max_slippage: None,
            chain_id: 945,
            partial_fills_enabled: true,
        };
        let versioned = VersionedOrder::V1(inner);
        let sig = AccountKeyring::Alice.pair().sign(&versioned.encode());
        let signed = crate::SignedOrder {
            order: versioned,
            signature: sp_runtime::MultiSignature::Sr25519(sig),
            partial_fill: Some(400),
        };
        let id = order_id(&signed.order);

        // The order is skipped (best-effort), not reverting the batch.
        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![signed]),
            false,
        ));

        // Nothing written to storage.
        assert_eq!(Orders::<Test>::get(id), None);
        assert_event(Event::OrderSkipped {
            order_id: id,
            reason: Error::<Test>::RelayerRequiredForPartialFill.into(),
        });
    });
}

#[test]
fn execute_orders_partial_fill_exceeding_remaining_is_skipped() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_tao_balance(alice(), 1_000);

        // Pre-fill 700 of 1000.
        let signed = make_partial_fill_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            u64::MAX,
            FAR_FUTURE,
            charlie(),
            700,
        );
        let id = order_id(&signed.order);
        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![signed.clone()]),
            false,
        ));
        assert_eq!(
            Orders::<Test>::get(id),
            Some(OrderStatus::PartiallyFilled(700))
        );

        // Try to fill 500 more, but only 300 remain → should be skipped.
        let mut over_fill = signed.clone();
        over_fill.partial_fill = Some(500);
        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![over_fill]),
            false,
        ));

        // Status unchanged.
        assert_eq!(
            Orders::<Test>::get(id),
            Some(OrderStatus::PartiallyFilled(700))
        );
        assert_event(Event::OrderSkipped {
            order_id: id,
            reason: Error::<Test>::IncorrectPartialFillAmount.into(),
        });
    });
}

#[test]
fn execute_orders_partial_fill_none_on_partially_filled_is_skipped() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_tao_balance(alice(), 1_000);

        // Pre-fill 700 of 1000.
        let signed = make_partial_fill_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            u64::MAX,
            FAR_FUTURE,
            charlie(),
            700,
        );
        let id = order_id(&signed.order);
        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![signed.clone()]),
            false,
        ));
        assert_eq!(
            Orders::<Test>::get(id),
            Some(OrderStatus::PartiallyFilled(700))
        );

        // Re-submit the same signed order with partial_fill = None against an
        // order already PartiallyFilled. The one-shot full-execution path must
        // not fire here: it would re-swap the full order.amount (over-debiting
        // the signer) and mark the order Fulfilled, discarding the 700 already
        // filled. The fix rejects this with IncorrectPartialFillAmount → skipped.
        let mut none_fill = signed.clone();
        none_fill.partial_fill = None;
        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![none_fill]),
            false,
        ));

        // Status unchanged — NOT over-filled and NOT marked Fulfilled.
        assert_eq!(
            Orders::<Test>::get(id),
            Some(OrderStatus::PartiallyFilled(700))
        );
        assert_event(Event::OrderSkipped {
            order_id: id,
            reason: Error::<Test>::IncorrectPartialFillAmount.into(),
        });
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// Partial fills — execute_batched_orders
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn execute_batched_orders_partial_fill_sets_partially_filled_status() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_buy_alpha_return(400);
        MockSwap::set_tao_balance(alice(), 1_000);

        let signed = make_partial_fill_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            u64::MAX,
            FAR_FUTURE,
            charlie(),
            400,
        );
        let id = order_id(&signed.order);

        assert_ok!(LimitOrders::execute_batched_orders(
            RuntimeOrigin::signed(charlie()),
            netuid(),
            bounded(vec![signed]),
        ));

        assert_eq!(
            Orders::<Test>::get(id),
            Some(OrderStatus::PartiallyFilled(400))
        );
    });
}

#[test]
fn execute_batched_orders_second_partial_fill_completes_order() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_buy_alpha_return(600);
        MockSwap::set_tao_balance(alice(), 1_000);

        let signed_first = make_partial_fill_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            u64::MAX,
            FAR_FUTURE,
            charlie(),
            600,
        );
        let id = order_id(&signed_first.order);

        assert_ok!(LimitOrders::execute_batched_orders(
            RuntimeOrigin::signed(charlie()),
            netuid(),
            bounded(vec![signed_first.clone()]),
        ));
        assert_eq!(
            Orders::<Test>::get(id),
            Some(OrderStatus::PartiallyFilled(600))
        );

        let mut signed_second = signed_first.clone();
        signed_second.partial_fill = Some(400);

        assert_ok!(LimitOrders::execute_batched_orders(
            RuntimeOrigin::signed(charlie()),
            netuid(),
            bounded(vec![signed_second]),
        ));
        assert_eq!(Orders::<Test>::get(id), Some(OrderStatus::Fulfilled));
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// In-batch order_id deduplication — regression tests
// ─────────────────────────────────────────────────────────────────────────────

/// Regression: the same fully-signed `LimitBuy` order appearing twice in one
/// batch must hard-fail with `DuplicateOrderInBatch` rather than debiting the
/// signer twice. Pre-fix, `validate_and_classify` validated each entry against
/// the same pre-batch `Orders::get(order_id)` snapshot with no in-batch tracking,
/// so the signer was charged N× their signed amount.
///
/// `assert_noop!` also asserts the storage root is unchanged, proving the
/// all-or-nothing batch rolled back. (The mock's TAO/alpha ledgers are
/// thread-local RefCell maps, not substrate storage, so we do not assert on
/// them here — see `mock.rs`.) We additionally assert `Orders::get` was never
/// written.
#[test]
fn execute_batched_orders_full_fill_duplicate_rejected() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_buy_alpha_return(500);
        MockSwap::set_tao_balance(alice(), 1_000);

        // Open-relay (relayer: None) fully-signed LimitBuy.
        let order = make_signed_order(
            AccountKeyring::Alice,
            dave(),
            netuid(),
            OrderType::LimitBuy,
            600,
            u64::MAX,
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            None,
        );
        let id = order_id(&order.order);

        // The same order twice in one batch.
        assert_noop!(
            LimitOrders::execute_batched_orders(
                RuntimeOrigin::signed(charlie()),
                netuid(),
                bounded(vec![order.clone(), order]),
            ),
            Error::<Test>::DuplicateOrderInBatch
        );

        // The batch rolled back: no order status was recorded.
        assert!(Orders::<Test>::get(id).is_none());
    });
}

/// Regression: two `SignedOrder`s that share the same inner `VersionedOrder`
/// (so the same `order_id`, since `order_id` excludes `partial_fill` and the
/// signature) but carry *different* `partial_fill` values must still collide
/// and be caught by the in-batch dedup. This exercises the partial-fill path
/// (partial_fills_enabled = true, relayer set).
#[test]
fn execute_batched_orders_partial_fill_duplicate_rejected() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_buy_alpha_return(400);
        MockSwap::set_tao_balance(alice(), 1_000);

        // Same inner VersionedOrder; only the envelope `partial_fill` differs.
        let first = make_partial_fill_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            u64::MAX,
            FAR_FUTURE,
            charlie(),
            600,
        );
        let mut second = first.clone();
        second.partial_fill = Some(400);

        // Same inner order ⇒ same order_id ⇒ caught by the dedup set.
        assert_eq!(order_id(&first.order), order_id(&second.order));
        let id = order_id(&first.order);

        assert_noop!(
            LimitOrders::execute_batched_orders(
                RuntimeOrigin::signed(charlie()),
                netuid(),
                bounded(vec![first, second]),
            ),
            Error::<Test>::DuplicateOrderInBatch
        );

        assert!(Orders::<Test>::get(id).is_none());
    });
}
