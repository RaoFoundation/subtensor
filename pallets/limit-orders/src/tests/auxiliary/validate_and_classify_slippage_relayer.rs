//! Helper tests: `validate_and_classify_slippage_relayer`.

use super::*;

// ─────────────────────────────────────────────────────────────────────────────
// validate_and_classify — effective_swap_limit propagation
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn validate_and_classify_stores_effective_swap_limit_for_buy() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);

        // 1% slippage on limit_price=2_000_000_000 (2.0 in ×10⁹) → ceiling = 2_020_000_000.
        // price=1.0, scaled=1_000_000_000 <= 2_000_000_000 ✓.
        let order = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            500u64,
            2_000_000_000u64, // 2.0 in ×10⁹ scale
            2_000_000u64,
            Perbill::zero(),
            fee_recipient(),
            None,
        );
        // Override max_slippage on the inner order after signing — we need to rebuild
        // the signed order so the signature covers the updated payload.
        let new_inner = {
            let mut o = order.order.inner().clone();
            o.max_slippage = Some(Perbill::from_percent(1));
            o
        };
        let versioned = crate::VersionedOrder::V1(new_inner.clone());
        let sig = AccountKeyring::Alice.pair().sign(&versioned.encode());
        let signed_with_slippage = crate::SignedOrder {
            order: versioned,
            signature: sp_runtime::MultiSignature::Sr25519(sig),
            partial_fill: None,
        };

        let orders = bounded(vec![signed_with_slippage]);
        let (buys, _) = LimitOrders::<Test>::validate_and_classify(
            netuid(),
            &orders,
            1_000_000u64,
            U64F64::from_num(1u32),
            bob(),
        )
        .expect("should succeed");

        assert_eq!(buys[0].effective_swap_limit, 2_020_000_000);
    });
}

#[test]
fn validate_and_classify_stores_effective_swap_limit_for_sell() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        // Price must be >= limit_price (in ×10⁹ scale) for TakeProfit to trigger.
        // limit_price=1_000_000_000 (1.0 in ×10⁹), 1% slippage → floor = 990_000_000.
        let new_inner = crate::Order {
            signer: AccountKeyring::Alice.to_account_id(),
            hotkey: bob(),
            netuid: netuid(),
            order_type: OrderType::TakeProfit,
            amount: 500u64,
            limit_price: 1_000_000_000u64, // 1.0 in ×10⁹ scale
            expiry: u64::MAX,
            fee_rate: Perbill::zero(),
            fee_recipient: fee_recipient(),
            relayer: None,
            max_slippage: Some(Perbill::from_percent(1)),
            chain_id: 945,
            partial_fills_enabled: false,
        };
        let versioned = crate::VersionedOrder::V1(new_inner);
        let sig = AccountKeyring::Alice.pair().sign(&versioned.encode());
        let signed = crate::SignedOrder {
            order: versioned,
            signature: sp_runtime::MultiSignature::Sr25519(sig),
            partial_fill: None,
        };

        let orders = bounded(vec![signed]);
        let (_, sells) = LimitOrders::<Test>::validate_and_classify(
            netuid(),
            &orders,
            1_000_000u64,
            U64F64::from_num(2u32), // current_price=2.0, scaled=2_000_000_000 >= limit_price=1_000_000_000 ✓
            bob(),
        )
        .expect("should succeed");

        assert_eq!(sells[0].effective_swap_limit, 990_000_000);
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// validate_and_classify — relayer enforcement
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn validate_and_classify_fails_for_wrong_relayer() {
    new_test_ext().execute_with(|| {
        // Order explicitly locks execution to charlie(); submitting as bob() must fail.
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);

        let order = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000u64,
            u64::MAX,
            2_000_000u64,
            Perbill::zero(),
            fee_recipient(),
            Some(BoundedVec::try_from(vec![charlie()]).unwrap()), // only charlie may relay this order
        );

        let orders = bounded(vec![order]);
        assert_noop!(
            LimitOrders::<Test>::validate_and_classify(
                netuid(),
                &orders,
                1_000_000u64,
                U64F64::from_num(1u32),
                bob() // wrong relayer
            ),
            crate::Error::<Test>::RelayerMissMatch
        );
    });
}

#[test]
fn validate_and_classify_succeeds_for_correct_relayer() {
    new_test_ext().execute_with(|| {
        // Same setup as above but now the correct relayer (charlie) is used.
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);

        let order = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::LimitBuy,
            1_000u64,
            u64::MAX,
            2_000_000u64,
            Perbill::zero(),
            fee_recipient(),
            Some(BoundedVec::try_from(vec![charlie()]).unwrap()), // only charlie may relay this order
        );

        let orders = bounded(vec![order]);
        let (buys, sells) = LimitOrders::<Test>::validate_and_classify(
            netuid(),
            &orders,
            1_000_000u64,
            U64F64::from_num(1u32),
            charlie(), // correct relayer
        )
        .expect("validate_and_classify should succeed");

        assert_eq!(buys.len(), 1, "expected 1 valid buy");
        assert_eq!(sells.len(), 0);
    });
}
