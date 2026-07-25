//! Silent-skip and should_fail behaviour for `execute_orders`.

use super::*;

// ─────────────────────────────────────────────────────────────────────────────
// execute_orders — silent-skip behaviour
// ─────────────────────────────────────────────────────────────────────────────

/// A single expired order is silently skipped: the call returns `Ok` and
/// nothing is written to the `Orders` storage map.
#[test]
fn execute_orders_skips_expired_order() {
    new_test_ext().execute_with(|| {
        MockTime::set(2_000_001); // now > expiry
        MockSwap::set_price(1.0);

        let signed = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            u64::MAX,
            2_000_000, // expiry in the past
            Perbill::zero(),
            fee_recipient(),
            None,
        );
        let id = order_id(&signed.order);

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![signed]),
            false,
        ));

        // Skipped — storage untouched.
        assert!(Orders::<Test>::get(id).is_none());
        assert_event(Event::OrderSkipped {
            order_id: id,
            reason: Error::<Test>::OrderExpired.into(),
        });
    });
}

/// A LimitBuy with `limit_price = 0` (price ceiling below current price)
/// is silently skipped: the call returns `Ok` and nothing is written to
/// the `Orders` storage map.
#[test]
fn execute_orders_skips_price_condition_not_met() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(5.0); // price 5.0 > limit 0 → buy condition not met

        let signed = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            0, // price ceiling of 0 — never satisfied at price 5.0
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            None,
        );
        let id = order_id(&signed.order);

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![signed]),
            false,
        ));

        // Skipped — storage untouched.
        assert!(Orders::<Test>::get(id).is_none());
        assert_event(Event::OrderSkipped {
            order_id: id,
            reason: Error::<Test>::PriceConditionNotMet.into(),
        });
    });
}

/// A batch containing one valid order and one expired order: the call
/// returns `Ok`, the valid order is stored as `Fulfilled`, and the expired
/// order is NOT written to storage.
#[test]
fn execute_orders_valid_and_invalid_mixed() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);

        let valid = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            u64::MAX,
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            None,
        );
        let expired = make_signed_order(
            AccountKeyring::Bob,
            alice(),
            netuid(),
            OrderType::LimitBuy,
            500,
            u64::MAX,
            500_000, // already expired
            Perbill::zero(),
            fee_recipient(),
            None,
        );
        let valid_id = order_id(&valid.order);
        let expired_id = order_id(&expired.order);

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![valid, expired]),
            false,
        ));

        // Valid order executed successfully.
        assert_eq!(Orders::<Test>::get(valid_id), Some(OrderStatus::Fulfilled));
        // Expired order silently skipped — not written to storage.
        assert!(Orders::<Test>::get(expired_id).is_none());
        assert_event(Event::OrderSkipped {
            order_id: expired_id,
            reason: Error::<Test>::OrderExpired.into(),
        });
    });
}

/// With `should_fail = true` a single expired order is NOT silently skipped:
/// the whole call fails with `OrderExpired` and storage stays untouched.
#[test]
fn execute_orders_should_fail_expired_order_reverts() {
    new_test_ext().execute_with(|| {
        MockTime::set(2_000_001); // now > expiry
        MockSwap::set_price(1.0);

        let signed = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            u64::MAX,
            2_000_000, // expiry in the past
            Perbill::zero(),
            fee_recipient(),
            None,
        );
        let id = order_id(&signed.order);

        // all-or-nothing: the failing order makes the whole call return Err
        // and assert_noop! confirms storage is unchanged.
        assert_noop!(
            LimitOrders::execute_orders(
                RuntimeOrigin::signed(charlie()),
                bounded(vec![signed]),
                true,
            ),
            Error::<Test>::OrderExpired
        );

        assert!(Orders::<Test>::get(id).is_none());
    });
}

/// With `should_fail = true` a batch containing a VALID order followed by an
/// INVALID (expired) order reverts entirely: the valid order's effects are
/// rolled back, so it is NOT recorded as `Fulfilled` and the relayer's TAO
/// is not consumed. Contrast `execute_orders_valid_and_invalid_mixed`, where
/// the same batch with `should_fail = false` keeps the valid order.
#[test]
fn execute_orders_should_fail_valid_then_invalid_reverts_whole_batch() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);

        let valid = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            u64::MAX,
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            None,
        );
        let expired = make_signed_order(
            AccountKeyring::Bob,
            alice(),
            netuid(),
            OrderType::LimitBuy,
            500,
            u64::MAX,
            500_000, // already expired
            Perbill::zero(),
            fee_recipient(),
            None,
        );
        let valid_id = order_id(&valid.order);
        let expired_id = order_id(&expired.order);

        // The expired order is the second in the batch; with should_fail = true
        // its failure reverts the already-executed valid order too.
        assert_noop!(
            LimitOrders::execute_orders(
                RuntimeOrigin::signed(charlie()),
                bounded(vec![valid, expired]),
                true,
            ),
            Error::<Test>::OrderExpired
        );

        // Neither order survived: the valid order's Fulfilled status was rolled back.
        assert!(Orders::<Test>::get(valid_id).is_none());
        assert!(Orders::<Test>::get(expired_id).is_none());
    });
}

/// With `should_fail = true` a price-condition-not-met order hard-fails the
/// whole call with `PriceConditionNotMet`, mirroring `execute_batched_orders`
/// rather than the best-effort skip path.
#[test]
fn execute_orders_should_fail_price_condition_not_met_reverts() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(5.0); // price 5.0 > limit 0 → buy condition not met

        let signed = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            0, // price ceiling of 0 — never satisfied at price 5.0
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            None,
        );
        let id = order_id(&signed.order);

        assert_noop!(
            LimitOrders::execute_orders(
                RuntimeOrigin::signed(charlie()),
                bounded(vec![signed]),
                true,
            ),
            Error::<Test>::PriceConditionNotMet
        );

        assert!(Orders::<Test>::get(id).is_none());
    });
}
