//! Helper tests: `validate_and_classify`.

use super::*;

// ─────────────────────────────────────────────────────────────────────────────
// validate_and_classify
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn validate_and_classify_separates_buys_and_sells() {
    new_test_ext().execute_with(|| {
        // Current time = 1_000_000 ms; expiry = 2_000_000 ms (well in the future).
        MockTime::set(1_000_000);
        // Price = 1.0 TAO/alpha.
        MockSwap::set_price(1.0);

        let buy_order = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000u64,         // amount in TAO
            2_000_000_000u64, // limit_price: willing to pay up to 2 TAO/alpha in ×10⁹ scale (scaled=1_000_000_000 ≤ 2_000_000_000 ✓)
            2_000_000u64,     // expiry ms
            Perbill::zero(),
            fee_recipient(),
            None,
        );
        let sell_order = make_signed_order(
            AccountKeyring::Bob,
            alice(),
            netuid(),
            OrderType::TakeProfit,
            500u64,           // amount in alpha
            1_000_000_000u64, // limit_price: sell if price >= 1 TAO/alpha in ×10⁹ scale (scaled=1_000_000_000 >= 1_000_000_000 ✓)
            2_000_000u64,
            Perbill::zero(),
            fee_recipient(),
            None,
        );

        let orders = bounded(vec![buy_order, sell_order]);
        let (buys, sells) = LimitOrders::<Test>::validate_and_classify(
            netuid(),
            &orders,
            1_000_000u64,
            U64F64::from_num(1u32),
            bob(),
        )
        .expect("validate_and_classify should succeed");

        assert_eq!(buys.len(), 1, "expected 1 valid buy");
        assert_eq!(sells.len(), 1, "expected 1 valid sell");

        // Buy entry: gross=1000, net=1000 (0% fee_rate)
        let buy = &buys[0];
        assert_eq!(buy.signer, alice());
        assert_eq!(buy.gross, 1_000u64);
        assert_eq!(buy.net, 1_000u64);
        assert_eq!(buy.fee_rate, Perbill::zero());

        // Sell entry: gross=500, net=500 (fee applied on TAO output, not alpha input)
        let sell = &sells[0];
        assert_eq!(sell.signer, bob());
        assert_eq!(sell.gross, 500u64);
        assert_eq!(sell.net, 500u64);
    });
}

#[test]
fn validate_and_classify_fails_for_wrong_netuid() {
    new_test_ext().execute_with(|| {
        // An order whose netuid does not match the batch netuid must cause a hard failure.
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);

        let wrong_netuid_order = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            NetUid::from(99u16), // different netuid
            OrderType::LimitBuy,
            1_000u64,
            2_000_000_000u64, // 2.0 in ×10⁹ scale
            2_000_000u64,
            Perbill::zero(),
            fee_recipient(),
            None,
        );

        let orders = bounded(vec![wrong_netuid_order]);
        assert_noop!(
            LimitOrders::<Test>::validate_and_classify(
                netuid(), // batch is for netuid 1
                &orders,
                1_000_000u64,
                U64F64::from_num(1u32),
                bob()
            ),
            crate::Error::<Test>::OrderNetUidMismatch
        );
    });
}

#[test]
fn validate_and_classify_fails_for_expired_order() {
    new_test_ext().execute_with(|| {
        // now_ms = 2_000_001, expiry = 2_000_000 → expired → hard failure.
        MockTime::set(2_000_001);
        MockSwap::set_price(1.0);

        let expired = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000u64,
            2_000_000_000u64, // 2.0 in ×10⁹ scale
            2_000_000u64,     // expiry already past
            Perbill::zero(),
            fee_recipient(),
            None,
        );

        let orders = bounded(vec![expired]);
        assert_noop!(
            LimitOrders::<Test>::validate_and_classify(
                netuid(),
                &orders,
                2_000_001u64,
                U64F64::from_num(1u32),
                bob()
            ),
            crate::Error::<Test>::OrderExpired
        );
    });
}

#[test]
fn validate_and_classify_fails_for_price_condition_not_met_for_buy() {
    new_test_ext().execute_with(|| {
        // Price = 3.0 TAO/alpha, scaled = 3_000_000_000, buyer's limit = 2_000_000_000 (2.0 in ×10⁹) → scaled > limit → hard failure.
        MockTime::set(1_000_000);
        let order = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000u64,
            2_000_000_000u64, // 2.0 in ×10⁹ scale
            2_000_000u64,
            Perbill::zero(),
            fee_recipient(),
            None,
        );

        let orders = bounded(vec![order]);
        assert_noop!(
            LimitOrders::<Test>::validate_and_classify(
                netuid(),
                &orders,
                1_000_000u64,
                U64F64::from_num(3u32), // current price = 3 > limit 2 → fails
                bob()
            ),
            crate::Error::<Test>::PriceConditionNotMet
        );
    });
}

#[test]
fn validate_and_classify_fails_for_already_processed_order() {
    new_test_ext().execute_with(|| {
        // An order already marked Fulfilled must cause a hard failure.
        MockTime::set(1_000_000);
        let order = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000u64,
            2_000_000_000u64, // 2.0 in ×10⁹ scale
            2_000_000u64,
            Perbill::zero(),
            fee_recipient(),
            None,
        );

        // Pre-mark as fulfilled on-chain.
        let oid = LimitOrders::<Test>::derive_order_id(&order.order);
        Orders::<Test>::insert(oid, OrderStatus::Fulfilled);

        let orders = bounded(vec![order]);
        assert_noop!(
            LimitOrders::<Test>::validate_and_classify(
                netuid(),
                &orders,
                1_000_000u64,
                U64F64::from_num(1u32),
                bob()
            ),
            crate::Error::<Test>::OrderAlreadyProcessed
        );
    });
}

#[test]
fn validate_and_classify_applies_buy_fee_to_net() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        // 1_000_000 ppb = 0.1%
        // amount = 1_000_000_000, fee = 1_000_000, net = 999_000_000

        let order = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000_000_000u64,
            u64::MAX, // limit price: accept any price
            2_000_000u64,
            Perbill::from_parts(1_000_000), // 0.1% fee
            fee_recipient(),
            None,
        );

        let orders = bounded(vec![order]);
        let (buys, _) = LimitOrders::<Test>::validate_and_classify(
            netuid(),
            &orders,
            1_000_000u64,
            U64F64::from_num(1u32),
            bob(),
        )
        .expect("validate_and_classify should succeed");

        assert_eq!(buys.len(), 1);
        let entry = &buys[0];
        assert_eq!(entry.gross, 1_000_000_000u64);
        assert_eq!(entry.fee_rate, Perbill::from_parts(1_000_000));
        assert_eq!(entry.net, 999_000_000u64);
    });
}
