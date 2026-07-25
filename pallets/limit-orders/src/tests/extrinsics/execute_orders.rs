//! Extrinsic tests: execute orders.

use super::*;

// ─────────────────────────────────────────────────────────────────────────────
// execute_orders
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn execute_orders_buy_order_fulfilled() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        // Price = 1.0 ≤ limit = 2.0 → condition met.
        let signed = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            2_000_000_000,
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
fn execute_orders_sell_order_fulfilled() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(2.0);
        // Price = 2.0, scaled = 2_000_000_000 ≥ limit = 1_000_000_000 → condition met.
        let signed = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::TakeProfit,
            500,
            1_000_000_000, // 1.0 in ×10⁹ scale
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

        assert_eq!(Orders::<Test>::get(id), Some(OrderStatus::Fulfilled));
        assert_event(Event::OrderExecuted {
            order_id: id,
            signer: alice(),
            netuid: netuid(),
            order_type: OrderType::TakeProfit,
            amount_in: 500,
            amount_out: 0,
        });
    });
}

#[test]
fn execute_orders_stop_loss_order_fulfilled() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(0.5);
        // Price = 0.5, scaled = 500_000_000 ≤ limit = 1_000_000_000 → condition met.
        let signed = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::StopLoss,
            500,
            1_000_000_000, // 1.0 in ×10⁹ scale
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

        assert_eq!(Orders::<Test>::get(id), Some(OrderStatus::Fulfilled));
        assert_event(Event::OrderExecuted {
            order_id: id,
            signer: alice(),
            netuid: netuid(),
            order_type: OrderType::StopLoss,
            amount_in: 500,
            amount_out: 0,
        });
    });
}

#[test]
fn execute_orders_stop_loss_price_not_met_skipped() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(2.0); // price 2.0, scaled=2_000_000_000 > limit 1_000_000_000 → stop loss condition not met
        let signed = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::StopLoss,
            500,
            1_000_000_000, // 1.0 in ×10⁹ scale
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

        assert!(Orders::<Test>::get(id).is_none());
        assert_event(Event::OrderSkipped {
            order_id: id,
            reason: Error::<Test>::PriceConditionNotMet.into(),
        });
    });
}

#[test]
fn execute_orders_expired_order_skipped() {
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

#[test]
fn execute_orders_price_not_met_skipped() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(5.0); // price 5.0, scaled=5_000_000_000 > limit 2_000_000_000 → buy condition not met
        let signed = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            2_000_000_000, // 2.0 in ×10⁹ scale
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

        assert!(Orders::<Test>::get(id).is_none());
        assert_event(Event::OrderSkipped {
            order_id: id,
            reason: Error::<Test>::PriceConditionNotMet.into(),
        });
    });
}

// Regression tests: with the ×10⁹ scale fix, sub-unity prices can be meaningfully
// expressed as limit_price values.  A price of 0.5 TAO/alpha is represented as
// 500_000_000 in ×10⁹ scale, enabling fine-grained TakeProfit thresholds below 1.0.
#[test]
fn take_profit_sub_unity_price_executes_when_limit_met() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        // Market price = 0.5 TAO/alpha → scaled = 500_000_000.
        MockSwap::set_price(0.5);

        // limit_price = 400_000_000 (0.4 in ×10⁹ scale).
        // TakeProfit condition: scaled_price (500_000_000) >= limit_price (400_000_000) ✓
        let signed = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::TakeProfit,
            500,
            400_000_000, // 0.4 in ×10⁹ scale — below current price of 0.5
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

        // Executes: 500_000_000 >= 400_000_000 → condition met.
        assert_eq!(Orders::<Test>::get(id), Some(OrderStatus::Fulfilled));
    });
}

#[test]
fn take_profit_sub_unity_price_skipped_when_limit_not_met() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        // Market price = 0.5 TAO/alpha → scaled = 500_000_000.
        MockSwap::set_price(0.5);

        // limit_price = 600_000_000 (0.6 in ×10⁹ scale).
        // TakeProfit condition: scaled_price (500_000_000) >= limit_price (600_000_000) → FALSE.
        let signed = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::TakeProfit,
            500,
            600_000_000, // 0.6 in ×10⁹ scale — above current price of 0.5
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

        // Skipped: 500_000_000 >= 600_000_000 is false.
        assert!(Orders::<Test>::get(id).is_none());
        assert_event(Event::OrderSkipped {
            order_id: id,
            reason: Error::<Test>::PriceConditionNotMet.into(),
        });
    });
}

#[test]
fn execute_orders_already_processed_skipped() {
    new_test_ext().execute_with(|| {
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
            None,
        );
        let id = order_id(&signed.order);
        Orders::<Test>::insert(id, OrderStatus::Fulfilled);

        // Should succeed (batch-level) but skip this order silently.
        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![signed]),
            false,
        ));
        // Still Fulfilled (not changed).
        assert_eq!(Orders::<Test>::get(id), Some(OrderStatus::Fulfilled));
        assert_event(Event::OrderSkipped {
            order_id: id,
            reason: Error::<Test>::OrderAlreadyProcessed.into(),
        });
    });
}

#[test]
fn execute_orders_mixed_batch_valid_and_skipped() {
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

        assert_eq!(Orders::<Test>::get(valid_id), Some(OrderStatus::Fulfilled));
        assert_event(Event::OrderSkipped {
            order_id: expired_id,
            reason: Error::<Test>::OrderExpired.into(),
        });
    });
}

#[test]
fn execute_orders_unsigned_rejected() {
    new_test_ext().execute_with(|| {
        assert_noop!(
            LimitOrders::execute_orders(RuntimeOrigin::none(), bounded(vec![]), false),
            DispatchError::BadOrigin
        );
    });
}

#[test]
fn execute_orders_buy_with_fee_charges_fee() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);

        // fee_rate = 1% (10_000_000 parts-per-billion), recipient = fee_recipient().
        let signed = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            u64::MAX,
            FAR_FUTURE,
            Perbill::from_parts(10_000_000), // 1%
            fee_recipient(),
            None,
        );
        MockSwap::set_tao_balance(alice(), 1_000);
        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![signed]),
            false,
        ));

        // One buy_alpha call for the net amount (990 TAO after 1% fee).
        let buys: Vec<_> = MockSwap::log()
            .into_iter()
            .filter_map(|c| {
                if let SwapCall::BuyAlpha { tao, .. } = c {
                    Some(tao)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(buys, vec![990], "main swap must use 990 TAO after 1% fee");

        // Fee (10 TAO) forwarded directly to fee_recipient via transfer_tao.
        assert_eq!(MockSwap::tao_balance(&fee_recipient()), 10);
    });
}

#[test]
fn execute_orders_sell_with_fee_charges_fee() {
    new_test_ext().execute_with(|| {
        // fee = 1% (10_000_000 ppb).
        // Alice sells 1_000 alpha; pool returns 800 TAO.
        // fee_tao = 1% of 800 = 8 TAO, forwarded to fee_recipient via transfer_tao.
        // Alice keeps 792 TAO.
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_sell_tao_return(800);
        MockSwap::set_alpha_balance(alice(), bob(), netuid(), 1_000);

        let signed = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::TakeProfit,
            1_000,
            0,
            FAR_FUTURE,
            Perbill::from_parts(10_000_000), // 1%
            fee_recipient(),
            None,
        );
        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![signed]),
            false,
        ));

        // Full 1_000 alpha sold (no alpha deducted for fee).
        let sells: Vec<_> = MockSwap::log()
            .into_iter()
            .filter_map(|c| {
                if let SwapCall::SellAlpha { alpha, .. } = c {
                    Some(alpha)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(sells, vec![1_000], "full alpha amount must be sold");

        // fee_recipient received 8 TAO (1% of 800).
        assert_eq!(MockSwap::tao_balance(&fee_recipient()), 8);
        // Alice kept the remaining 792 TAO.
        assert_eq!(MockSwap::tao_balance(&alice()), 792);
    });
}

#[test]
fn execute_orders_empty_batch_returns_ok() {
    new_test_ext().execute_with(|| {
        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![]),
            false,
        ));
    });
}

#[test]
fn execute_orders_fee_transfer_failure_skips_order() {
    new_test_ext().execute_with(|| {
        // When the fee transfer fails the entire order is rolled back and emits OrderSkipped.
        // This prevents users from exploiting a tight balance to execute swaps fee-free.
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_buy_alpha_return(500);
        MockSwap::set_tao_balance(alice(), 10_000);

        let signed = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            u64::MAX,
            FAR_FUTURE,
            Perbill::from_parts(10_000_000), // 1%
            fee_recipient(),
            None,
        );

        FAIL_FEE_TRANSFER.with(|f| *f.borrow_mut() = true);
        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![signed.clone()]),
            false,
        ));
        FAIL_FEE_TRANSFER.with(|f| *f.borrow_mut() = false);

        // Order was skipped — not stored as Fulfilled.
        let id = crate::tests::mock::order_id(&signed.order);
        assert!(Orders::<Test>::get(id).is_none());

        // OrderSkipped was emitted with the fee-transfer error as the reason.
        assert_event(Event::OrderSkipped {
            order_id: id,
            reason: DispatchError::CannotLookup,
        });

        // fee_recipient received nothing.
        assert_eq!(MockSwap::tao_balance(&fee_recipient()), 0);
    });
}
