//! Extrinsic tests: relayer.

use super::*;

// ─────────────────────────────────────────────────────────────────────────────
// relayer enforcement
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn execute_orders_wrong_relayer_skipped() {
    new_test_ext().execute_with(|| {
        // Order locks execution to charlie(); submitting as bob() must be silently skipped.
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);

        let signed = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            u64::MAX,
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            Some(BoundedVec::truncate_from(vec![charlie()])), // only charlie may relay this order
        );
        let id = order_id(&signed.order);

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(bob()), // wrong relayer
            bounded(vec![signed]),
            false,
        ));

        // Order not stored — it was skipped.
        assert!(Orders::<Test>::get(id).is_none());
        assert_event(Event::OrderSkipped {
            order_id: id,
            reason: Error::<Test>::RelayerMissMatch.into(),
        });
    });
}

#[test]
fn execute_orders_correct_relayer_executed() {
    new_test_ext().execute_with(|| {
        // Same order submitted by the designated relayer (charlie) — must succeed.
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);

        let signed = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            u64::MAX,
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            Some(BoundedVec::truncate_from(vec![charlie()])), // charlie is the designated relayer
        );
        let id = order_id(&signed.order);

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()), // correct relayer
            bounded(vec![signed]),
            false,
        ));

        assert_eq!(Orders::<Test>::get(id), Some(OrderStatus::Fulfilled));
        assert_event(Event::OrderExecuted {
            order_id: id,
            signer: alice(),
            netuid: netuid(),
            order_type: OrderType::LimitBuy,
            amount_in: 1_000,
            amount_out: 0,
        });
    });
}

#[test]
fn execute_batched_orders_wrong_relayer_fails_entire_batch() {
    new_test_ext().execute_with(|| {
        // In execute_batched_orders a relayer mismatch is a hard failure — the
        // whole call is reverted, unlike the best-effort skip in execute_orders.
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);

        let signed = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            u64::MAX,
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            Some(BoundedVec::truncate_from(vec![charlie()])), // only charlie may relay this order
        );

        assert_noop!(
            LimitOrders::execute_batched_orders(
                RuntimeOrigin::signed(bob()), // wrong relayer
                netuid(),
                bounded(vec![signed])
            ),
            Error::<Test>::RelayerMissMatch
        );
    });
}

#[test]
fn execute_batched_orders_correct_relayer_succeeds() {
    new_test_ext().execute_with(|| {
        // Same order submitted by the designated relayer — must execute and
        // distribute alpha to the buyer.
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_buy_alpha_return(1_000);
        MockSwap::set_tao_balance(alice(), 1_000);

        let signed = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            u64::MAX,
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            Some(BoundedVec::truncate_from(vec![charlie()])), // charlie is the designated relayer
        );
        let id = order_id(&signed.order);

        assert_ok!(LimitOrders::execute_batched_orders(
            RuntimeOrigin::signed(charlie()), // correct relayer
            netuid(),
            bounded(vec![signed])
        ));

        assert_eq!(Orders::<Test>::get(id), Some(OrderStatus::Fulfilled));
    });
}
