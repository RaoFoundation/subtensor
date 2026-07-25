//! Helper tests: `net_amount_for_event`.

use super::*;

// ─────────────────────────────────────────────────────────────────────────────
// net_amount_for_event
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn net_amount_for_event_buy_dominant() {
    new_test_ext().execute_with(|| {
        // Buys = 1000 TAO net, sells TAO-equiv = 300 TAO → net 700 TAO buy-side
        let price = U64F64::from_num(2u32); // 2 TAO/alpha
        let net = LimitOrders::<Test>::net_amount_for_event(
            &OrderSide::Buy,
            1_000u128, // total_buy_net (TAO)
            150u128,   // total_sell_net (alpha)  ← not used in Buy branch
            300u128,   // total_sell_tao_equiv
            price,
        )
        .expect("conversion does not overflow");
        assert_eq!(net, 700u64);
    });
}

#[test]
fn net_amount_for_event_sell_dominant() {
    new_test_ext().execute_with(|| {
        // Sells = 500 alpha net, buys TAO = 200 TAO at price 2 → buy_alpha_equiv = 100
        // net sell = 500 - 100 = 400 alpha
        let price = U64F64::from_num(2u32); // 2 TAO/alpha → 1 alpha = 2 TAO
        let net = LimitOrders::<Test>::net_amount_for_event(
            &OrderSide::Sell,
            200u128, // total_buy_net (TAO)
            500u128, // total_sell_net (alpha)
            400u128, // total_sell_tao_equiv (not used in Sell branch directly)
            price,
        )
        .expect("conversion does not overflow");
        // buy_alpha_equiv = 200 / 2 = 100; net = 500 - 100 = 400
        assert_eq!(net, 400u64);
    });
}

#[test]
fn net_amount_for_event_perfectly_offset() {
    new_test_ext().execute_with(|| {
        // Buys = 200 TAO, sells TAO-equiv = 200 → net = 0 (buy-side result = 0)
        let price = U64F64::from_num(2u32);
        let net = LimitOrders::<Test>::net_amount_for_event(
            &OrderSide::Buy,
            200u128,
            100u128,
            200u128,
            price,
        )
        .expect("conversion does not overflow");
        assert_eq!(net, 0u64);
    });
}

#[test]
fn net_amount_for_event_sell_overflow_returns_error() {
    new_test_ext().execute_with(|| {
        let tiny_price = U64F64::from_bits(1);
        assert_eq!(
            LimitOrders::<Test>::net_amount_for_event(
                &OrderSide::Sell,
                u128::MAX,
                500u128,
                0u128,
                tiny_price,
            ),
            Err(Error::<Test>::ArithmeticOverflow.into()),
        );
    });
}
