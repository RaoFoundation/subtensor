//! `sudo_set_basket_liquidity_cap`: governance setter for the `swap_basket` liquidity cap.

use super::mock::*;
use crate::Error;
use frame_support::{assert_noop, assert_ok, sp_runtime::DispatchError};
use frame_system::Config;
use sp_core::U256;

#[test]
fn test_sudo_set_basket_liquidity_cap() {
    new_test_ext().execute_with(|| {
        // Launch default: 10% of the destination pool's alpha reserve.
        assert_eq!(
            pallet_subtensor::BasketLiquidityCap::<Test>::get(),
            pallet_subtensor::DEFAULT_BASKET_LIQUIDITY_CAP
        );

        // Only root may set the cap.
        assert_noop!(
            AdminUtils::sudo_set_basket_liquidity_cap(
                <<Test as Config>::RuntimeOrigin>::signed(U256::from(1)),
                u16::MAX / 4
            ),
            DispatchError::BadOrigin
        );

        // Zero would refuse every buy.
        assert_noop!(
            AdminUtils::sudo_set_basket_liquidity_cap(<<Test as Config>::RuntimeOrigin>::root(), 0),
            Error::<Test>::ValueNotInBounds
        );

        // Root sets a looser cap (25%) and the storage + event reflect it.
        assert_ok!(AdminUtils::sudo_set_basket_liquidity_cap(
            <<Test as Config>::RuntimeOrigin>::root(),
            u16::MAX / 4
        ));
        assert_eq!(
            pallet_subtensor::BasketLiquidityCap::<Test>::get(),
            u16::MAX / 4
        );
        frame_system::Pallet::<Test>::assert_last_event(RuntimeEvent::AdminUtils(
            crate::Event::BasketLiquidityCapSet { cap: u16::MAX / 4 },
        ));
    });
}
