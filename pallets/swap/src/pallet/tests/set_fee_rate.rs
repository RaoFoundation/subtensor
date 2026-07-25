//! Tests for root [`Pallet::set_fee_rate`].

use super::*;

#[test]
fn test_set_fee_rate() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let fee_rate = 500; // 0.76% fee

        assert_noop!(
            Swap::set_fee_rate(RuntimeOrigin::signed(666), netuid, fee_rate),
            DispatchError::BadOrigin
        );

        assert_ok!(Swap::set_fee_rate(RuntimeOrigin::root(), netuid, fee_rate));

        // Check that fee rate was set correctly
        assert_eq!(FeeRate::<Test>::get(netuid), fee_rate);

        // Verify fee rate validation - should fail if too high
        let too_high_fee = MaxFeeRate::get() + 1;
        assert_noop!(
            Swap::set_fee_rate(RuntimeOrigin::root(), netuid, too_high_fee),
            Error::<Test>::FeeRateTooHigh
        );
    });
}
