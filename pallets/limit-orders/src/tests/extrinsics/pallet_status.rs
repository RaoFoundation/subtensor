//! `set_pallet_status` root gating and disable filtering.

use super::*;

/// Root changes the pallet status, extrinsics are filtered
#[test]
fn root_disables_and_extrinsics_are_filtered() {
    new_test_ext().execute_with(|| {
        // Disable the pallet
        assert_ok!(LimitOrders::set_pallet_status(RuntimeOrigin::root(), false));

        let sell = make_signed_order(
            AccountKeyring::Bob,
            dave(),
            netuid(),
            OrderType::TakeProfit,
            500,
            0,
            FAR_FUTURE,
            Perbill::zero(),
            fee_recipient(),
            None,
        );

        // Must succeed: collecting Bob's alpha must not rate-limit the pallet
        // intermediary, so distributing alpha to Alice is not blocked.
        assert_noop!(
            LimitOrders::execute_batched_orders(
                RuntimeOrigin::signed(charlie()),
                netuid(),
                bounded(vec![sell])
            ),
            Error::<Test>::LimitOrdersDisabled
        );
    });
}

/// Non-root origin cannot disable the pallet
#[test]
fn non_root_cannot_disable_the_pallet() {
    new_test_ext().execute_with(|| {
        // Try disabling the pallet with charlie
        assert_noop!(
            LimitOrders::set_pallet_status(RuntimeOrigin::signed(charlie()), false),
            DispatchError::BadOrigin
        );
    });
}
