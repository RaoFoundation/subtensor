//! Extrinsic tests: max slippage.

use super::*;

// ─────────────────────────────────────────────────────────────────────────────
// max_slippage — execute_orders passes effective_swap_limit to pool
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn execute_orders_buy_no_slippage_passes_u64_max_to_pool() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);

        let signed = make_signed_order_with_slippage(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            u64::MAX,
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            None, // no slippage → u64::MAX ceiling
        );

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![signed]),
            false,
        ));

        // Pool must have been called with u64::MAX as price ceiling.
        assert_eq!(MockSwap::buy_alpha_limit_prices(), vec![u64::MAX]);
    });
}

#[test]
fn execute_orders_sell_no_slippage_passes_zero_to_pool() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(2.0);

        let signed = make_signed_order_with_slippage(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::TakeProfit,
            500,
            1_000_000_000, // 1.0 in ×10⁹ scale; price=2.0 (scaled=2_000_000_000) >= 1_000_000_000 ✓
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            None, // no slippage → 0 floor
        );

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![signed]),
            false,
        ));

        assert_eq!(MockSwap::sell_alpha_limit_prices(), vec![0]);
    });
}

#[test]
fn execute_orders_buy_one_percent_slippage_passes_ceiling_to_pool() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);

        // limit_price=1_000_000_000 (1.0 in ×10⁹), 1% slippage → ceiling = 1_010_000_000.
        let signed = make_signed_order_with_slippage(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000,
            1_000_000_000, // 1.0 in ×10⁹ scale; price=1.0 (scaled=1_000_000_000) <= 1_000_000_000 ✓
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            Some(Perbill::from_percent(1)),
        );

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![signed]),
            false,
        ));

        assert_eq!(MockSwap::buy_alpha_limit_prices(), vec![1_010_000_000]);
    });
}

#[test]
fn execute_orders_sell_one_percent_slippage_passes_floor_to_pool() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        // Price must be >= limit_price for TakeProfit to trigger.
        MockSwap::set_price(2_000.0);

        // limit_price=1_000_000_000 (1.0 in ×10⁹), 1% slippage → floor = 990_000_000.
        let signed = make_signed_order_with_slippage(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::TakeProfit,
            500,
            1_000_000_000, // 1.0 in ×10⁹ scale; price=2000.0 (scaled=2T) >= 1_000_000_000 ✓
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            Some(Perbill::from_percent(1)),
        );

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![signed]),
            false,
        ));

        assert_eq!(MockSwap::sell_alpha_limit_prices(), vec![990_000_000]);
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// max_slippage — execute_batched_orders aggregates tightest constraint
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn execute_batched_orders_buy_dominant_uses_min_ceiling() {
    new_test_ext().execute_with(|| {
        // 3 buy orders with different slippage constraints.
        //   Alice: limit=1_000_000_000, 2% → ceiling=1_020_000_000
        //   Bob:   limit=1_000_000_000, 1% → ceiling=1_010_000_000  ← tightest
        //   Charlie (as signer, not relayer): limit=1_000_000_000, 3% → ceiling=1_030_000_000
        // Expected pool price_limit = min(1_020_000_000, 1_010_000_000, 1_030_000_000) = 1_010_000_000.
        // price=1.0, scaled=1_000_000_000 <= 1_000_000_000 ✓ for all LimitBuy orders.
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_buy_alpha_return(500);
        MockSwap::set_tao_balance(alice(), 600);
        MockSwap::set_tao_balance(bob(), 200);
        MockSwap::set_tao_balance(dave(), 200);

        let alice_order = make_signed_order_with_slippage(
            AccountKeyring::Alice,
            dave(),
            netuid(),
            OrderType::LimitBuy,
            600,
            1_000_000_000, // 1.0 in ×10⁹ scale
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            Some(Perbill::from_percent(2)), // ceiling = 1_020_000_000
        );
        let bob_order = make_signed_order_with_slippage(
            AccountKeyring::Bob,
            dave(),
            netuid(),
            OrderType::LimitBuy,
            200,
            1_000_000_000, // 1.0 in ×10⁹ scale
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            Some(Perbill::from_percent(1)), // ceiling = 1_010_000_000 ← tightest
        );
        let dave_order = make_signed_order_with_slippage(
            AccountKeyring::Dave,
            dave(),
            netuid(),
            OrderType::LimitBuy,
            200,
            1_000_000_000, // 1.0 in ×10⁹ scale
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            Some(Perbill::from_percent(3)), // ceiling = 1_030_000_000
        );

        assert_ok!(LimitOrders::execute_batched_orders(
            RuntimeOrigin::signed(charlie()),
            netuid(),
            bounded(vec![alice_order, bob_order, dave_order]),
        ));

        // Net pool swap must have been called with the tightest ceiling = 1_010_000_000.
        assert_eq!(MockSwap::buy_alpha_limit_prices(), vec![1_010_000_000]);
    });
}

#[test]
fn execute_batched_orders_sell_dominant_uses_max_floor() {
    new_test_ext().execute_with(|| {
        // 3 sell orders with different slippage constraints.
        //   Alice: limit=1_000_000_000, 3% → floor=970_000_000
        //   Bob:   limit=1_000_000_000, 1% → floor=990_000_000  ← tightest (highest floor)
        //   Dave:  limit=1_000_000_000, 2% → floor=980_000_000
        // Expected pool price_limit = max(970_000_000, 990_000_000, 980_000_000) = 990_000_000.
        // Price must be >= limit_price=1_000_000_000 (1.0 in ×10⁹) for TakeProfit to trigger.
        // price=2000.0, scaled=2_000_000_000_000 >= 1_000_000_000 ✓.
        MockTime::set(1_000_000);
        MockSwap::set_price(2_000.0);
        MockSwap::set_sell_tao_return(500);
        MockSwap::set_alpha_balance(alice(), dave(), netuid(), 600);
        MockSwap::set_alpha_balance(bob(), dave(), netuid(), 200);
        MockSwap::set_alpha_balance(dave(), dave(), netuid(), 200);

        let alice_order = make_signed_order_with_slippage(
            AccountKeyring::Alice,
            dave(),
            netuid(),
            OrderType::TakeProfit,
            600,
            1_000_000_000, // 1.0 in ×10⁹ scale
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            Some(Perbill::from_percent(3)), // floor = 970_000_000
        );
        let bob_order = make_signed_order_with_slippage(
            AccountKeyring::Bob,
            dave(),
            netuid(),
            OrderType::TakeProfit,
            200,
            1_000_000_000, // 1.0 in ×10⁹ scale
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            Some(Perbill::from_percent(1)), // floor = 990_000_000 ← tightest
        );
        let dave_order = make_signed_order_with_slippage(
            AccountKeyring::Dave,
            dave(),
            netuid(),
            OrderType::TakeProfit,
            200,
            1_000_000_000, // 1.0 in ×10⁹ scale
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            Some(Perbill::from_percent(2)), // floor = 980_000_000
        );

        assert_ok!(LimitOrders::execute_batched_orders(
            RuntimeOrigin::signed(charlie()),
            netuid(),
            bounded(vec![alice_order, bob_order, dave_order]),
        ));

        // Net pool swap must have been called with the tightest floor = 990_000_000.
        assert_eq!(MockSwap::sell_alpha_limit_prices(), vec![990_000_000]);
    });
}

#[test]
fn execute_batched_orders_no_slippage_uses_unconstrained_limits() {
    new_test_ext().execute_with(|| {
        // Orders without max_slippage should pass u64::MAX (buy) or 0 (sell).
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_buy_alpha_return(500);
        MockSwap::set_tao_balance(alice(), 1_000);

        let order = make_signed_order_with_slippage(
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

        assert_ok!(LimitOrders::execute_batched_orders(
            RuntimeOrigin::signed(charlie()),
            netuid(),
            bounded(vec![order]),
        ));

        assert_eq!(MockSwap::buy_alpha_limit_prices(), vec![u64::MAX]);
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// max_slippage — mixed order type coexistence
// ─────────────────────────────────────────────────────────────────────────────

/// Sell-dominant batch: TakeProfit orders (with slippage) + StopLoss (no slippage).
///
/// TakeProfit orders set meaningful floors; StopLoss contributes 0 (no constraint).
/// pool_price_limit = max(take_floors..., 0s) = max(take_floors).
/// All three orders are fulfilled.
#[test]
fn execute_batched_orders_takeprofit_and_stoploss_coexist_sell_dominant() {
    new_test_ext().execute_with(|| {
        // Price = 2000 — scaled = 2_000_000_000_000.
        // TakeProfit triggers when scaled_price >= limit_price (2T >= 1_000_000_000 ✓).
        // StopLoss triggers when scaled_price <= limit_price (2T <= 5_000_000_000_000 ✓).
        MockTime::set(1_000_000);
        MockSwap::set_price(2_000.0);
        MockSwap::set_sell_tao_return(500);

        // Alice TakeProfit: limit=1_000_000_000 (1.0), 3% → floor=970_000_000.
        // Bob TakeProfit:   limit=1_000_000_000 (1.0), 1% → floor=990_000_000.  ← tightest
        // Dave StopLoss:    limit=5_000_000_000_000 (5000.0), None → floor=0.
        MockSwap::set_alpha_balance(alice(), dave(), netuid(), 600);
        MockSwap::set_alpha_balance(bob(), dave(), netuid(), 200);
        MockSwap::set_alpha_balance(dave(), alice(), netuid(), 200);

        let alice_order = make_signed_order_with_slippage(
            AccountKeyring::Alice,
            dave(),
            netuid(),
            OrderType::TakeProfit,
            600,
            1_000_000_000, // 1.0 in ×10⁹ scale
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            Some(Perbill::from_percent(3)),
        );
        let bob_order = make_signed_order_with_slippage(
            AccountKeyring::Bob,
            dave(),
            netuid(),
            OrderType::TakeProfit,
            200,
            1_000_000_000, // 1.0 in ×10⁹ scale
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            Some(Perbill::from_percent(1)),
        );
        let dave_stoploss = make_signed_order_with_slippage(
            AccountKeyring::Dave,
            alice(),
            netuid(),
            OrderType::StopLoss,
            200,
            5_000_000_000_000, // 5000.0 in ×10⁹ scale; scaled_price 2T <= 5T ✓
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            None, // StopLoss: no slippage → floor=0, does not constrain pool
        );

        let alice_id = order_id(&alice_order.order);
        let bob_id = order_id(&bob_order.order);
        let dave_id = order_id(&dave_stoploss.order);

        assert_ok!(LimitOrders::execute_batched_orders(
            RuntimeOrigin::signed(charlie()),
            netuid(),
            bounded(vec![alice_order, bob_order, dave_stoploss]),
        ));

        // All three fulfilled.
        assert_eq!(Orders::<Test>::get(alice_id), Some(OrderStatus::Fulfilled));
        assert_eq!(Orders::<Test>::get(bob_id), Some(OrderStatus::Fulfilled));
        assert_eq!(Orders::<Test>::get(dave_id), Some(OrderStatus::Fulfilled));

        // Pool called once with the tightest TakeProfit floor (990_000_000), not 0 from StopLoss.
        assert_eq!(MockSwap::sell_alpha_limit_prices(), vec![990_000_000]);
    });
}

/// Buy-dominant batch: LimitBuy orders (with slippage) dominant + StopLoss (no slippage) on offset side.
///
/// The offset StopLoss is settled internally at spot price; it does not contribute
/// to the pool's price ceiling (which comes only from the dominant buy side).
/// pool_price_limit = min(buy_ceilings) = 1_010_000_000.
#[test]
fn execute_batched_orders_limitbuy_and_stoploss_offset_coexist_buy_dominant() {
    new_test_ext().execute_with(|| {
        // Price = 1.0, scaled = 1_000_000_000.
        // LimitBuy triggers (scaled <= limit ✓). StopLoss triggers (scaled <= limit ✓).
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_buy_alpha_return(900);

        // Alice LimitBuy: limit=1_000_000_000 (1.0), 2% → ceiling=1_020_000_000.
        // Bob   LimitBuy: limit=1_000_000_000 (1.0), 1% → ceiling=1_010_000_000.  ← tightest
        // Dave  StopLoss: limit=2_000_000_000 (2.0), None → floor=0 (offset side, not used for pool limit).
        MockSwap::set_tao_balance(alice(), 600);
        MockSwap::set_tao_balance(bob(), 400);
        MockSwap::set_alpha_balance(dave(), alice(), netuid(), 100);

        let alice_order = make_signed_order_with_slippage(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            600,
            1_000_000_000, // 1.0 in ×10⁹ scale; scaled=1_000_000_000 <= 1_000_000_000 ✓
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            Some(Perbill::from_percent(2)),
        );
        let bob_order = make_signed_order_with_slippage(
            AccountKeyring::Bob,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            400,
            1_000_000_000, // 1.0 in ×10⁹ scale; scaled=1_000_000_000 <= 1_000_000_000 ✓
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            Some(Perbill::from_percent(1)),
        );
        let dave_stoploss = make_signed_order_with_slippage(
            AccountKeyring::Dave,
            alice(),
            netuid(),
            OrderType::StopLoss,
            100,
            2_000_000_000, // 2.0 in ×10⁹ scale; scaled=1_000_000_000 <= 2_000_000_000 ✓
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            None, // StopLoss: no slippage; settled at spot, never constrains pool ceiling
        );

        let alice_id = order_id(&alice_order.order);
        let bob_id = order_id(&bob_order.order);
        let dave_id = order_id(&dave_stoploss.order);

        assert_ok!(LimitOrders::execute_batched_orders(
            RuntimeOrigin::signed(charlie()),
            netuid(),
            bounded(vec![alice_order, bob_order, dave_stoploss]),
        ));

        // All three fulfilled.
        assert_eq!(Orders::<Test>::get(alice_id), Some(OrderStatus::Fulfilled));
        assert_eq!(Orders::<Test>::get(bob_id), Some(OrderStatus::Fulfilled));
        assert_eq!(Orders::<Test>::get(dave_id), Some(OrderStatus::Fulfilled));

        // Pool buy called with min(1_020_000_000, 1_010_000_000) = 1_010_000_000. StopLoss's floor (0) is ignored on buy side.
        assert_eq!(MockSwap::buy_alpha_limit_prices(), vec![1_010_000_000]);
    });
}

/// StopLoss with a narrow slippage sets an effective floor above the current market price,
/// making the pool swap impossible and failing the entire batch.
///
/// This demonstrates Issue 1 from the design: relayers should not apply max_slippage to
/// StopLoss orders. StopLoss triggers when price has already fallen; a floor derived from
/// the (higher) trigger threshold will almost always exceed the actual market price.
#[test]
fn execute_batched_orders_stoploss_narrow_slippage_breaks_batch() {
    new_test_ext().execute_with(|| {
        // StopLoss: limit=100_000_000_000 (100.0 in ×10⁹), triggers at price=50 (scaled=50_000_000_000 ≤ 100_000_000_000 ✓).
        // 1% slippage → floor=99_000_000_000. Market is at 50 → pool cannot deliver ≥99_000_000_000.
        MockTime::set(1_000_000);
        MockSwap::set_price(50.0);
        MockSwap::set_sell_tao_return(100); // non-zero so SwapReturnedZero is not the cause
        MockSwap::set_enforce_price_limit(true);
        MockSwap::set_alpha_balance(dave(), alice(), netuid(), 200);

        let stoploss = make_signed_order_with_slippage(
            AccountKeyring::Dave,
            alice(),
            netuid(),
            OrderType::StopLoss,
            200,
            100_000_000_000, // 100.0 in ×10⁹ scale; scaled=50_000_000_000 <= 100_000_000_000 ✓
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            Some(Perbill::from_percent(1)), // floor=99_000_000_000, but market=50 → pool rejects
        );

        assert_noop!(
            LimitOrders::execute_batched_orders(
                RuntimeOrigin::signed(charlie()),
                netuid(),
                bounded(vec![stoploss]),
            ),
            DispatchError::Other("price limit exceeded")
        );
    });
}

/// Same StopLoss scenario through execute_orders (best-effort): the order is silently
/// skipped rather than failing the whole call.
///
/// Note: `DispatchError::Other` has `#[codec(skip)]` on its string field, so the reason
/// string is lost when stored in the event log. We verify the skip via storage absence
/// and by asserting the floor (99_000_000_000 = 100_000_000_000 - 1%) was actually passed
/// to the pool — which is what caused the rejection. The `execute_batched_orders` variant
/// below uses `assert_noop!` (checks the return value directly, no storage round-trip) and
/// can verify the string.
#[test]
fn execute_orders_stoploss_narrow_slippage_skips_order() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(50.0);
        MockSwap::set_sell_tao_return(100);
        MockSwap::set_enforce_price_limit(true);

        let stoploss = make_signed_order_with_slippage(
            AccountKeyring::Dave,
            alice(),
            netuid(),
            OrderType::StopLoss,
            200,
            100_000_000_000, // 100.0 in ×10⁹ scale; scaled=50_000_000_000 <= 100_000_000_000 ✓
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            Some(Perbill::from_percent(1)), // floor=99_000_000_000, but market=50 → pool rejects
        );
        let id = order_id(&stoploss.order);

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie()),
            bounded(vec![stoploss]),
            false,
        ));

        // Order not stored — pool rejected the floor.
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

        // The sell was attempted with the correct floor (99_000_000_000 = 100_000_000_000 - 1%).
        // This is the value that exceeded the market price and caused the rejection.
        assert_eq!(MockSwap::sell_alpha_limit_prices(), vec![99_000_000_000]);
    });
}
