//! Extrinsic tests: cancel order.

use super::*;

// ─────────────────────────────────────────────────────────────────────────────
// cancel_order
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn cancel_order_signer_can_cancel() {
    new_test_ext().execute_with(|| {
        let order = VersionedOrder::V1(Order {
            signer: alice(),
            hotkey: bob(),
            netuid: netuid(),
            order_type: OrderType::LimitBuy,
            amount: 1_000,
            limit_price: u64::MAX,
            expiry: FAR_FUTURE,
            fee_rate: Perbill::zero(),
            fee_recipient: fee_recipient(),
            relayer: None,
            max_slippage: None,
            chain_id: 945,
            partial_fills_enabled: false,
        });
        let id = order_id(&order);

        assert_ok!(LimitOrders::cancel_order(
            RuntimeOrigin::signed(alice()),
            order
        ));
        assert_eq!(Orders::<Test>::get(id), Some(OrderStatus::Cancelled));
        assert_event(Event::OrderCancelled {
            order_id: id,
            signer: alice(),
        });
    });
}

#[test]
fn cancel_order_non_signer_rejected() {
    new_test_ext().execute_with(|| {
        let order = VersionedOrder::V1(Order {
            signer: alice(),
            hotkey: bob(),
            netuid: netuid(),
            order_type: OrderType::LimitBuy,
            amount: 1_000,
            limit_price: u64::MAX,
            expiry: FAR_FUTURE,
            fee_rate: Perbill::zero(),
            fee_recipient: fee_recipient(),
            relayer: None,
            max_slippage: None,
            chain_id: 945,
            partial_fills_enabled: false,
        });
        // Bob tries to cancel Alice's order.
        assert_noop!(
            LimitOrders::cancel_order(RuntimeOrigin::signed(bob()), order),
            Error::<Test>::Unauthorized
        );
    });
}

#[test]
fn cancel_order_already_cancelled_rejected() {
    new_test_ext().execute_with(|| {
        let order = VersionedOrder::V1(Order {
            signer: alice(),
            hotkey: bob(),
            netuid: netuid(),
            order_type: OrderType::LimitBuy,
            amount: 1_000,
            limit_price: u64::MAX,
            expiry: FAR_FUTURE,
            fee_rate: Perbill::zero(),
            fee_recipient: fee_recipient(),
            relayer: None,
            max_slippage: None,
            chain_id: 945,
            partial_fills_enabled: false,
        });
        let id = order_id(&order);
        Orders::<Test>::insert(id, OrderStatus::Cancelled);

        assert_noop!(
            LimitOrders::cancel_order(RuntimeOrigin::signed(alice()), order),
            Error::<Test>::OrderAlreadyProcessed
        );
    });
}

#[test]
fn cancel_order_already_fulfilled_rejected() {
    new_test_ext().execute_with(|| {
        let order = VersionedOrder::V1(Order {
            signer: alice(),
            hotkey: bob(),
            netuid: netuid(),
            order_type: OrderType::LimitBuy,
            amount: 1_000,
            limit_price: u64::MAX,
            expiry: FAR_FUTURE,
            fee_rate: Perbill::zero(),
            fee_recipient: fee_recipient(),
            relayer: None,
            max_slippage: None,
            chain_id: 945,
            partial_fills_enabled: false,
        });
        let id = order_id(&order);
        Orders::<Test>::insert(id, OrderStatus::Fulfilled);

        assert_noop!(
            LimitOrders::cancel_order(RuntimeOrigin::signed(alice()), order),
            Error::<Test>::OrderAlreadyProcessed
        );
    });
}

#[test]
fn cancel_order_unsigned_rejected() {
    new_test_ext().execute_with(|| {
        let order = VersionedOrder::V1(Order {
            signer: alice(),
            hotkey: bob(),
            netuid: netuid(),
            order_type: OrderType::LimitBuy,
            amount: 1_000,
            limit_price: u64::MAX,
            expiry: FAR_FUTURE,
            fee_rate: Perbill::zero(),
            fee_recipient: fee_recipient(),
            relayer: None,
            max_slippage: None,
            chain_id: 945,
            partial_fills_enabled: false,
        });
        assert_noop!(
            LimitOrders::cancel_order(RuntimeOrigin::none(), order),
            DispatchError::BadOrigin
        );
    });
}
