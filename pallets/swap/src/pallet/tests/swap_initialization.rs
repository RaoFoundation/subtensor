//! Tests for balancer pool initialization (`maybe_initialize_palswap`).

use super::*;

#[test]
fn test_swap_initialization() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);

        // Setup reserves
        let tao = TaoBalance::from(1_000_000_000u64);
        let alpha = AlphaBalance::from(4_000_000_000u64);
        TaoReserve::set_mock_reserve(netuid, tao);
        AlphaReserve::set_mock_reserve(netuid, alpha);

        assert_ok!(Pallet::<Test>::maybe_initialize_palswap(netuid, None));
        assert!(PalSwapInitialized::<Test>::get(netuid));

        // Verify current price is set
        let price = Pallet::<Test>::current_price(netuid);
        let expected_price = U64F64::from_num(0.25_f64);
        assert_abs_diff_eq!(
            price.to_num::<f64>(),
            expected_price.to_num::<f64>(),
            epsilon = 0.000000001
        );

        // Verify that swap reserve weight is initialized
        let reserve_weight = SwapBalancer::<Test>::get(netuid);
        assert_eq!(
            reserve_weight.get_quote_weight(),
            Perquintill::from_rational(1_u64, 2_u64),
        );
    });
}

#[test]
fn test_swap_initialization_with_price() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);

        // Setup reserves, tao / alpha = 0.25
        let tao = TaoBalance::from(1_000_000_000u64);
        let alpha = AlphaBalance::from(4_000_000_000u64);
        TaoReserve::set_mock_reserve(netuid, tao);
        AlphaReserve::set_mock_reserve(netuid, alpha);

        // Initialize with 0.2 price
        assert_ok!(Pallet::<Test>::maybe_initialize_palswap(
            netuid,
            Some(U64F64::from(1u16) / U64F64::from(5u16))
        ));
        assert!(PalSwapInitialized::<Test>::get(netuid));

        // Verify current price is set to 0.2
        let price = Pallet::<Test>::current_price(netuid);
        let expected_price = U64F64::from_num(0.2_f64);
        assert_abs_diff_eq!(
            price.to_num::<f64>(),
            expected_price.to_num::<f64>(),
            epsilon = 0.000000001
        );
    });
}
