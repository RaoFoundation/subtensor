//! Extrinsic tests: execute batched orders swap errors.

use super::*;

// ─────────────────────────────────────────────────────────────────────────────
// net_pool_swap – SwapReturnedZero errors
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn execute_batched_orders_buy_zero_alpha_returns_error() {
    new_test_ext().execute_with(|| {
        // buy_alpha returns 0 alpha for a non-zero TAO input → SwapReturnedZero.
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_buy_alpha_return(0); // pool gives back nothing
        MockSwap::set_tao_balance(alice(), 1_000);

        let order = make_signed_order(
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

        assert_noop!(
            LimitOrders::execute_batched_orders(
                RuntimeOrigin::signed(charlie()),
                netuid(),
                bounded(vec![order]),
            ),
            Error::<Test>::SwapReturnedZero
        );
    });
}

#[test]
fn execute_batched_orders_sell_zero_tao_returns_error() {
    new_test_ext().execute_with(|| {
        // sell_alpha returns 0 TAO for a non-zero alpha input → SwapReturnedZero.
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_sell_tao_return(0); // pool gives back nothing
        MockSwap::set_alpha_balance(alice(), bob(), netuid(), 1_000);

        let order = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::TakeProfit,
            1_000,
            0,
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            None,
        );

        assert_noop!(
            LimitOrders::execute_batched_orders(
                RuntimeOrigin::signed(charlie()),
                netuid(),
                bounded(vec![order]),
            ),
            Error::<Test>::SwapReturnedZero
        );
    });
}

#[test]
fn execute_batched_orders_sell_alpha_respects_swap_fail() {
    new_test_ext().execute_with(|| {
        // sell_alpha should propagate DispatchError when MOCK_SWAP_FAIL is set.
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        MockSwap::set_swap_fail(true);
        MockSwap::set_alpha_balance(alice(), bob(), netuid(), 1_000);

        let order = make_signed_order(
            AccountKeyring::Alice,
            bob(),
            netuid(),
            OrderType::TakeProfit,
            1_000,
            0,
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            None,
        );

        assert_noop!(
            LimitOrders::execute_batched_orders(
                RuntimeOrigin::signed(charlie()),
                netuid(),
                bounded(vec![order]),
            ),
            DispatchError::Other("pool error")
        );
    });
}
