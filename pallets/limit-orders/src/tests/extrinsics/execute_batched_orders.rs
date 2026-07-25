//! Extrinsic tests: execute batched orders.

use super::*;

// ─────────────────────────────────────────────────────────────────────────────
// execute_batched_orders
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn execute_batched_orders_unsigned_rejected() {
    new_test_ext().execute_with(|| {
        assert_noop!(
            LimitOrders::execute_batched_orders(RuntimeOrigin::none(), netuid(), bounded(vec![])),
            DispatchError::BadOrigin
        );
    });
}

#[test]
fn execute_batched_orders_all_invalid_fails() {
    new_test_ext().execute_with(|| {
        // An expired order causes the whole batch to fail.
        MockTime::set(2_000_001); // all expired
        let expired = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            u64::MAX,
            1_000_000,
            Perbill::zero(),
            fee_recipient(),
            None,
        );
        assert_noop!(
            LimitOrders::execute_batched_orders(
                RuntimeOrigin::signed(charlie()),
                netuid(),
                bounded(vec![expired]),
            ),
            Error::<Test>::OrderExpired
        );
    });
}

#[test]
fn execute_batched_orders_fails_for_wrong_netuid() {
    new_test_ext().execute_with(|| {
        // An order whose netuid does not match the batch netuid must cause the batch to fail.
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_buy_alpha_return(100);

        let wrong_net = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            NetUid::from(99u16), // wrong netuid
            OrderType::LimitBuy,
            1_000,
            u64::MAX,
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            None,
        );

        assert_noop!(
            LimitOrders::execute_batched_orders(
                RuntimeOrigin::signed(charlie()),
                netuid(), // batch targets netuid 1
                bounded(vec![wrong_net]),
            ),
            Error::<Test>::OrderNetUidMismatch
        );
    });
}

#[test]
fn execute_batched_orders_price_condition_not_met_fails_entire_batch() {
    new_test_ext().execute_with(|| {
        // Price condition not met is a hard-fail in execute_batched_orders —
        // unlike execute_orders where it silently skips the order.
        MockTime::set(1_000_000);
        MockSwap::set_price(100.0); // current price = 100, scaled = 100_000_000_000

        // LimitBuy requires scaled_price <= limit_price; with limit_price=1_000_000_000 (1.0) this fails.
        let order = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            1_000_000_000, // 1.0 in ×10⁹ scale, far below scaled price of 100_000_000_000
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            None,
        );

        assert_noop!(
            LimitOrders::execute_batched_orders(
                RuntimeOrigin::signed(charlie()),
                netuid(),
                bounded(vec![order])
            ),
            Error::<Test>::PriceConditionNotMet
        );
    });
}

#[test]
fn execute_batched_orders_buy_only_fulfills_orders_and_distributes_alpha() {
    new_test_ext().execute_with(|| {
        // Setup:
        //   Alice buys 600 TAO, Bob buys 400 TAO (total 1000 TAO net, fee=0).
        //   Pool returns 500 alpha (MOCK_BUY_ALPHA_RETURN).
        //   No sellers → total_alpha = 500.
        //   Pro-rata: Alice 500*600/1000=300, Bob 500*400/1000=200.
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_buy_alpha_return(500);
        MockSwap::set_tao_balance(alice(), 600);
        MockSwap::set_tao_balance(bob(), 400);

        let alice_order = make_signed_order(
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
        let bob_order = make_signed_order(
            AccountKeyring::Bob,
            dave(),
            netuid(),
            OrderType::LimitBuy,
            400,
            u64::MAX,
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            None,
        );
        let alice_id = order_id(&alice_order.order);
        let bob_id = order_id(&bob_order.order);

        assert_ok!(LimitOrders::execute_batched_orders(
            RuntimeOrigin::signed(charlie()),
            netuid(),
            bounded(vec![alice_order, bob_order]),
        ));

        // Both orders fulfilled.
        assert_eq!(Orders::<Test>::get(alice_id), Some(OrderStatus::Fulfilled));
        assert_eq!(Orders::<Test>::get(bob_id), Some(OrderStatus::Fulfilled));

        // Alpha distributed pro-rata.
        assert_eq!(MockSwap::alpha_balance(&alice(), &dave(), netuid()), 300);
        assert_eq!(MockSwap::alpha_balance(&bob(), &dave(), netuid()), 200);

        // Summary event.
        assert_event(Event::GroupExecutionSummary {
            netuid: netuid(),
            net_side: OrderSide::Buy,
            net_amount: 1_000,
            actual_out: 500,
            executed_count: 2,
        });
    });
}

/// Regression test for the zero-share batch fund-loss bug.
///
/// Bug (pre-fix): `collect_assets` debited every buyer's full TAO input up front,
/// then `distribute_alpha_pro_rata` floored each buyer's alpha share. When a
/// buyer's `share = floor(total_alpha * net / total_buy_net)` floored to 0, the
/// old code silently SKIPPED the alpha transfer (`if share > 0 { .. }`) yet STILL
/// marked the order `Fulfilled`. The victim therefore paid full TAO, received zero
/// alpha, and the order was permanently closed.
///
/// Fix: `distribute_alpha_pro_rata` now `ensure!(share > 0, ZeroShareInBatch)`,
/// hard-failing the whole `execute_batched_orders` call. In production FRAME's
/// per-dispatch storage layer then rolls back `collect_assets` and the pool swap,
/// so no signer is debited and no order is stored.
///
/// `assert_noop!` asserts both the error AND that no on-chain storage mutation
/// persisted — i.e. neither order is written, so neither is marked `Fulfilled`.
/// Against the old code this call returned `Ok` and wrote `Fulfilled`, so the
/// `assert_noop!` (storage-root-unchanged) would have FAILED.
///
/// NOTE: we deliberately do NOT assert the victim's TAO balance was refunded.
/// `MockSwap` keeps balances in a `thread_local!` map that lives OUTSIDE the
/// substrate storage overlay, so `collect_assets`' debit is not transactional in
/// the mock and is not rolled back here. The balance refund is a property of the
/// real `frame_system` balances under the dispatch storage layer (exercised by the
/// L2/integration PoC), not something this mock can model.
#[test]
fn execute_batched_orders_zero_share_buyer_hard_fails() {
    new_test_ext().execute_with(|| {
        // Buy-only batch, price 1.0, pool alpha output pinned to 1000.
        //   big buyer net   = 1_000_000 TAO
        //   victim buyer net = 1 TAO
        //   total_buy_net   = 1_000_001
        //   total_alpha     = actual_out(1000) + total_sell_net(0) = 1000
        //   victim share    = floor(1000 * 1 / 1_000_001) = 0 → ZeroShareInBatch
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_buy_alpha_return(1000);
        // Distinct signer coldkeys; each buyer must be able to cover its own input.
        MockSwap::set_tao_balance(alice(), 1_000_000); // big buyer (Alice)
        MockSwap::set_tao_balance(bob(), 1); // victim (Bob)

        let big_buyer = make_signed_order(
            AccountKeyring::Alice,
            dave(),
            netuid(),
            OrderType::LimitBuy,
            1_000_000,
            u64::MAX,
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            None,
        );
        let victim = make_signed_order(
            AccountKeyring::Bob,
            dave(),
            netuid(),
            OrderType::LimitBuy,
            1,
            u64::MAX,
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            None,
        );
        let big_id = order_id(&big_buyer.order);
        let victim_id = order_id(&victim.order);

        // The whole batch must hard-fail with ZeroShareInBatch. assert_noop! also asserts
        // the storage root is unchanged, so neither order was written/marked Fulfilled —
        // the core of the fix. (Old code: returned Ok and wrote Fulfilled → this fails.)
        assert_noop!(
            LimitOrders::execute_batched_orders(
                RuntimeOrigin::signed(charlie()),
                netuid(),
                bounded(vec![big_buyer, victim]),
            ),
            Error::<Test>::ZeroShareInBatch
        );

        // Explicit, redundant-with-assert_noop! statement of intent: no order is terminal.
        assert_eq!(Orders::<Test>::get(victim_id), None);
        assert_eq!(Orders::<Test>::get(big_id), None);
    });
}

/// Guards against over-restriction: the `ZeroShareInBatch` fix must NOT reject a
/// legitimate multi-buyer batch where every buyer's floored share is at least 1.
#[test]
fn execute_batched_orders_all_nonzero_shares_still_succeeds() {
    new_test_ext().execute_with(|| {
        // Buy-only, price 1.0, pool alpha output = 1000, comparable buyer nets so
        // neither share floors to zero:
        //   Alice net = 600, Bob net = 400, total_buy_net = 1000, total_alpha = 1000
        //   Alice share = floor(1000 * 600 / 1000) = 600
        //   Bob   share = floor(1000 * 400 / 1000) = 400
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_buy_alpha_return(1000);
        MockSwap::set_tao_balance(alice(), 600);
        MockSwap::set_tao_balance(bob(), 400);

        let alice_order = make_signed_order(
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
        let bob_order = make_signed_order(
            AccountKeyring::Bob,
            dave(),
            netuid(),
            OrderType::LimitBuy,
            400,
            u64::MAX,
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            None,
        );
        let alice_id = order_id(&alice_order.order);
        let bob_id = order_id(&bob_order.order);

        assert_ok!(LimitOrders::execute_batched_orders(
            RuntimeOrigin::signed(charlie()),
            netuid(),
            bounded(vec![alice_order, bob_order]),
        ));

        // Both orders fulfilled and both buyers received non-zero alpha.
        assert_eq!(Orders::<Test>::get(alice_id), Some(OrderStatus::Fulfilled));
        assert_eq!(Orders::<Test>::get(bob_id), Some(OrderStatus::Fulfilled));
        assert_eq!(MockSwap::alpha_balance(&alice(), &dave(), netuid()), 600);
        assert_eq!(MockSwap::alpha_balance(&bob(), &dave(), netuid()), 400);
        assert!(MockSwap::alpha_balance(&alice(), &dave(), netuid()) > 0);
        assert!(MockSwap::alpha_balance(&bob(), &dave(), netuid()) > 0);
    });
}

/// Sell-side analogue of the zero-share regression. A seller whose `net_share`
/// floors to 0 in `distribute_tao_pro_rata` must hard-fail the whole batch with
/// `ZeroShareInBatch`. `assert_noop!` proves no on-chain storage mutation persisted
/// (neither order is written/marked Fulfilled). As in the buy-side test, the
/// seller's collected alpha is not refunded *in the mock* (MockSwap balances are
/// thread_local, outside the storage overlay); the refund is a real-balance
/// property under the dispatch storage layer, not modelled here.
#[test]
fn execute_batched_orders_zero_share_seller_hard_fails() {
    new_test_ext().execute_with(|| {
        // Sell-only batch, price 1.0, pool TAO output pinned to 1000.
        //   big seller alpha    = 1_000_000 → sell_tao_equiv 1_000_000
        //   victim seller alpha = 1         → sell_tao_equiv 1
        //   total_sell_tao_equiv = 1_000_001
        //   total_tao = actual_out(1000) + total_buy_net(0) = 1000
        //   victim gross_share = floor(1000 * 1 / 1_000_001) = 0
        //   net_share = 0 → ZeroShareInBatch
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_sell_tao_return(1000);
        MockSwap::set_alpha_balance(alice(), dave(), netuid(), 1_000_000); // big seller
        MockSwap::set_alpha_balance(bob(), dave(), netuid(), 1); // victim seller

        let big_seller = make_signed_order(
            AccountKeyring::Alice,
            dave(),
            netuid(),
            OrderType::TakeProfit,
            1_000_000,
            0, // limit=0 → accept any price
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            None,
        );
        let victim = make_signed_order(
            AccountKeyring::Bob,
            dave(),
            netuid(),
            OrderType::TakeProfit,
            1,
            0,
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            None,
        );
        let big_id = order_id(&big_seller.order);
        let victim_id = order_id(&victim.order);

        // The whole batch must hard-fail with ZeroShareInBatch; assert_noop! also asserts
        // the storage root is unchanged, so neither order was written/marked Fulfilled.
        assert_noop!(
            LimitOrders::execute_batched_orders(
                RuntimeOrigin::signed(charlie()),
                netuid(),
                bounded(vec![big_seller, victim]),
            ),
            Error::<Test>::ZeroShareInBatch
        );

        // Explicit, redundant-with-assert_noop! statement of intent: no order is terminal.
        assert_eq!(Orders::<Test>::get(victim_id), None);
        assert_eq!(Orders::<Test>::get(big_id), None);
    });
}

#[test]
fn execute_batched_orders_sell_only_fulfills_orders_and_distributes_tao() {
    new_test_ext().execute_with(|| {
        // Setup:
        //   Alice sells 300 alpha, Bob sells 200 alpha (total 500 alpha, fee=0).
        //   Price = 2.0 → sell_tao_equiv: Alice 600, Bob 400, total 1000.
        //   Pool returns 800 TAO (MOCK_SELL_TAO_RETURN) for the net 500 alpha.
        //   No buyers → total_tao = 800 + 0 = 800.
        //   Pro-rata: Alice 800*600/1000=480, Bob 800*400/1000=320.
        MockTime::set(1_000_000);
        MockSwap::set_price(2.0);
        MockSwap::set_sell_tao_return(800);
        MockSwap::set_alpha_balance(alice(), dave(), netuid(), 300);
        MockSwap::set_alpha_balance(bob(), dave(), netuid(), 200);

        let alice_order = make_signed_order(
            AccountKeyring::Alice,
            dave(),
            netuid(),
            OrderType::TakeProfit,
            300,
            0,
            FAR_FUTURE, // limit=0 → accept any price
            Perbill::zero(),
            fee_recipient(),
            None,
        );
        let bob_order = make_signed_order(
            AccountKeyring::Bob,
            dave(),
            netuid(),
            OrderType::TakeProfit,
            200,
            0,
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            None,
        );
        let alice_id = order_id(&alice_order.order);
        let bob_id = order_id(&bob_order.order);

        assert_ok!(LimitOrders::execute_batched_orders(
            RuntimeOrigin::signed(charlie()),
            netuid(),
            bounded(vec![alice_order, bob_order]),
        ));

        assert_eq!(Orders::<Test>::get(alice_id), Some(OrderStatus::Fulfilled));
        assert_eq!(Orders::<Test>::get(bob_id), Some(OrderStatus::Fulfilled));

        // TAO distributed pro-rata.
        assert_eq!(MockSwap::tao_balance(&alice()), 480);
        assert_eq!(MockSwap::tao_balance(&bob()), 320);

        assert_event(Event::GroupExecutionSummary {
            netuid: netuid(),
            net_side: OrderSide::Sell,
            net_amount: 500,
            actual_out: 800,
            executed_count: 2,
        });
    });
}

#[test]
fn execute_batched_orders_buy_dominant_mixed() {
    new_test_ext().execute_with(|| {
        // Setup (fee=0, price=2.0 TAO/alpha):
        //   Buyers: Alice 1000 TAO, Bob 600 TAO → total_buy_net = 1600.
        //   Sellers: Charlie 200 alpha → sell_tao_equiv = 400 TAO.
        //   Net (buy-dominant): 1600 - 400 = 1200 TAO goes to pool.
        //   Pool returns 300 alpha (MOCK_BUY_ALPHA_RETURN).
        //   total_alpha for buyers = 300 (pool) + 200 (seller passthrough) = 500.
        //   Pro-rata buyers (by buy_net TAO):
        //     Alice:  500 * 1000/1600 = 312 alpha
        //     Bob:    500 *  600/1600 = 187 alpha
        //     (dust = 1 alpha stays in pallet)
        //   Sellers (buy-dominant branch): total_tao = total_sell_tao_equiv = 400.
        //     Charlie: 400 * 400/400 = 400 TAO.
        MockTime::set(1_000_000);
        MockSwap::set_price(2.0);
        MockSwap::set_buy_alpha_return(300);
        MockSwap::set_tao_balance(alice(), 1_000);
        MockSwap::set_tao_balance(bob(), 600);
        MockSwap::set_alpha_balance(charlie(), dave(), netuid(), 200);

        let alice_buy = make_signed_order(
            AccountKeyring::Alice,
            dave(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            u64::MAX,
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            None,
        );
        let bob_buy = make_signed_order(
            AccountKeyring::Bob,
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
        let charlie_sell = make_signed_order(
            AccountKeyring::Charlie,
            dave(),
            netuid(),
            OrderType::TakeProfit,
            200,
            0,
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            None,
        );

        assert_ok!(LimitOrders::execute_batched_orders(
            RuntimeOrigin::signed(dave()),
            netuid(),
            bounded(vec![alice_buy, bob_buy, charlie_sell]),
        ));

        assert_eq!(MockSwap::alpha_balance(&alice(), &dave(), netuid()), 312);
        assert_eq!(MockSwap::alpha_balance(&bob(), &dave(), netuid()), 187);
        assert_eq!(MockSwap::tao_balance(&charlie()), 400);

        assert_event(Event::GroupExecutionSummary {
            netuid: netuid(),
            net_side: OrderSide::Buy,
            net_amount: 1_200,
            actual_out: 300,
            executed_count: 3,
        });
    });
}

#[test]
fn execute_batched_orders_sell_dominant_mixed() {
    new_test_ext().execute_with(|| {
        // Setup (fee=0, price=2.0 TAO/alpha):
        //   Buyers: Alice 200 TAO → total_buy_net = 200.
        //   Sellers: Bob 300 alpha, Charlie 200 alpha → total_sell_net = 500.
        //     sell_tao_equiv: Bob 600, Charlie 400, total 1000.
        //   Net (sell-dominant): buy_alpha_equiv = 200/2 = 100 alpha;
        //     residual sell alpha = 500 - 100 = 400 alpha → pool returns 300 TAO.
        //   total_tao for sellers = 300 (pool) + 200 (buy passthrough) = 500 TAO.
        //   Pro-rata sellers (by sell_tao_equiv):
        //     Bob:     500 * 600/1000 = 300 TAO
        //     Charlie: 500 * 400/1000 = 200 TAO
        //   total_alpha for buyers = buy_net / price = 200/2 = 100 alpha.
        //   Alice: 100 * 200/200 = 100 alpha.
        MockTime::set(1_000_000);
        MockSwap::set_price(2.0);
        MockSwap::set_sell_tao_return(300);
        MockSwap::set_tao_balance(alice(), 200);
        MockSwap::set_alpha_balance(bob(), dave(), netuid(), 300);
        MockSwap::set_alpha_balance(charlie(), dave(), netuid(), 200);

        let alice_buy = make_signed_order(
            AccountKeyring::Alice,
            dave(),
            netuid(),
            OrderType::LimitBuy,
            200,
            u64::MAX,
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            None,
        );
        let bob_sell = make_signed_order(
            AccountKeyring::Bob,
            dave(),
            netuid(),
            OrderType::TakeProfit,
            300,
            0,
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            None,
        );
        let charlie_sell = make_signed_order(
            AccountKeyring::Charlie,
            dave(),
            netuid(),
            OrderType::TakeProfit,
            200,
            0,
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            None,
        );

        assert_ok!(LimitOrders::execute_batched_orders(
            RuntimeOrigin::signed(dave()),
            netuid(),
            bounded(vec![alice_buy, bob_sell, charlie_sell]),
        ));

        assert_eq!(MockSwap::alpha_balance(&alice(), &dave(), netuid()), 100);
        assert_eq!(MockSwap::tao_balance(&bob()), 300);
        assert_eq!(MockSwap::tao_balance(&charlie()), 200);

        assert_event(Event::GroupExecutionSummary {
            netuid: netuid(),
            net_side: OrderSide::Sell,
            net_amount: 400,
            actual_out: 300,
            executed_count: 3,
        });
    });
}

#[test]
fn execute_batched_orders_fee_forwarded_to_collector() {
    new_test_ext().execute_with(|| {
        // fee = 1% (10_000_000 ppb).
        // Alice buys 1000 TAO: fee = 10, net = 990.
        // Pool returns 500 alpha for 990 TAO.
        // collect_fees transfers 10 TAO (buy fee) to fee_recipient.
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_buy_alpha_return(500);

        let alice_buy = make_signed_order(
            AccountKeyring::Alice,
            dave(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            u64::MAX,
            FAR_FUTURE,
            Perbill::from_parts(10_000_000), // 1%
            fee_recipient(),
            None,
        );

        assert_ok!(LimitOrders::execute_batched_orders(
            RuntimeOrigin::signed(charlie()),
            netuid(),
            bounded(vec![alice_buy]),
        ));

        // Fee recipient received the buy-side fee.
        assert_eq!(MockSwap::tao_balance(&fee_recipient()), 10);
    });
}

#[test]
fn execute_batched_orders_fails_for_cancelled_order() {
    new_test_ext().execute_with(|| {
        // A cancelled order is already processed; including it in the batch must cause a hard failure.
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_buy_alpha_return(100);

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
        Orders::<Test>::insert(id, OrderStatus::Cancelled);

        assert_noop!(
            LimitOrders::execute_batched_orders(
                RuntimeOrigin::signed(charlie()),
                netuid(),
                bounded(vec![signed]),
            ),
            Error::<Test>::OrderCancelled
        );

        // Still cancelled, not changed to Fulfilled.
        assert_eq!(Orders::<Test>::get(id), Some(OrderStatus::Cancelled));
    });
}

#[test]
fn execute_batched_orders_fees_charged_on_both_sides_when_matched_internally() {
    new_test_ext().execute_with(|| {
        // fee = 1% (10_000_000 ppb), price = 1.0 TAO/alpha.
        //
        // Alice buys  1_000 TAO  → buy fee = 10 TAO, net = 990 TAO.
        // Bob   sells 1_000 alpha → sell_tao_equiv = 1_000 TAO.
        //
        // sell-dominant: residual = 1_000 - 990 = 10 alpha sent to pool.
        // Pool returns 9 TAO (mocked) for that residual.
        // total_tao for sellers = 9 (pool) + 990 (buy passthrough) = 999.
        // Bob gross_share = 999 * 1_000/1_000 = 999.
        // Sell fee = mul_floor(1%, 999) = floor(9.99) = 9; Bob nets 990 TAO.
        // fee_recipient total = buy_fee(10) + sell_fee(9) = 19 TAO.
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_sell_tao_return(9);
        MockSwap::set_tao_balance(alice(), 1_000);
        MockSwap::set_alpha_balance(bob(), dave(), netuid(), 1_000);

        let alice_buy = make_signed_order(
            AccountKeyring::Alice,
            dave(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            u64::MAX,
            FAR_FUTURE,
            Perbill::from_parts(10_000_000), // 1%
            fee_recipient(),
            None,
        );
        let bob_sell = make_signed_order(
            AccountKeyring::Bob,
            dave(),
            netuid(),
            OrderType::TakeProfit,
            1_000,
            0,
            FAR_FUTURE,
            Perbill::from_parts(10_000_000), // 1%
            fee_recipient(),
            None,
        );

        assert_ok!(LimitOrders::execute_batched_orders(
            RuntimeOrigin::signed(charlie()),
            netuid(),
            bounded(vec![alice_buy, bob_sell]),
        ));

        // Both sides charged: fee_recipient gets buy fee (10) + sell fee (9) = 19.
        assert_eq!(MockSwap::tao_balance(&fee_recipient()), 19);
        // Bob receives 990 TAO after sell-side fee (999 gross - 9 fee).
        assert_eq!(MockSwap::tao_balance(&bob()), 990);
    });
}
