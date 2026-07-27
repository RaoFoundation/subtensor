#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
//! `get_median_subnet_alpha_price` odd/even and eligibility filters.

use super::prelude::*;

#[test]
fn median_subnet_alpha_price_returns_one_when_no_eligible_subnet_prices() {
    new_test_ext(0).execute_with(|| {
        let one = U64F64::from_num(1u64);

        // Empty state.
        assert_eq!(SubtensorModule::get_median_subnet_alpha_price(), one);

        // ROOT must be ignored.
        NetworksAdded::<Test>::insert(NetUid::ROOT, true);
        assert_eq!(SubtensorModule::get_median_subnet_alpha_price(), one);

        // Zero-priced subnet must be ignored.
        let zero_cold = U256::from(101);
        let zero_hot = U256::from(102);
        let zero_netuid = add_dynamic_network(&zero_hot, &zero_cold);
        setup_reserves(zero_netuid, TaoBalance::ZERO, AlphaBalance::from(100u64));
        assert_eq!(
            <Test as pallet::Config>::SwapInterface::current_alpha_price(zero_netuid.into()),
            U64F64::from_num(0u64)
        );
        assert_eq!(SubtensorModule::get_median_subnet_alpha_price(), one);

        // added=false subnet must be ignored as well.
        let hidden_cold = U256::from(103);
        let hidden_hot = U256::from(104);
        let hidden_netuid = add_dynamic_network(&hidden_hot, &hidden_cold);
        setup_reserves(
            hidden_netuid,
            TaoBalance::from(900u64),
            AlphaBalance::from(100u64),
        );
        NetworksAdded::<Test>::insert(hidden_netuid, false);

        assert_eq!(SubtensorModule::get_median_subnet_alpha_price(), one);
    });
}

#[test]
fn median_subnet_alpha_price_returns_middle_value_for_odd_unsorted_prices() {
    new_test_ext(0).execute_with(|| {
        let n1 = add_dynamic_network(&U256::from(201), &U256::from(200));
        let n2 = add_dynamic_network(&U256::from(203), &U256::from(202));
        let n3 = add_dynamic_network(&U256::from(205), &U256::from(204));

        // Unsorted prices: 7, 2, 5 -> median should be 5.
        setup_reserves(n1, TaoBalance::from(700u64), AlphaBalance::from(100u64));
        setup_reserves(n2, TaoBalance::from(200u64), AlphaBalance::from(100u64));
        setup_reserves(n3, TaoBalance::from(500u64), AlphaBalance::from(100u64));

        assert_eq!(
            <Test as pallet::Config>::SwapInterface::current_alpha_price(n1.into()),
            U96F32::from_num(7u64)
        );
        assert_eq!(
            <Test as pallet::Config>::SwapInterface::current_alpha_price(n2.into()),
            U96F32::from_num(2u64)
        );
        assert_eq!(
            <Test as pallet::Config>::SwapInterface::current_alpha_price(n3.into()),
            U96F32::from_num(5u64)
        );

        assert_eq!(
            SubtensorModule::get_median_subnet_alpha_price(),
            U96F32::from_num(5u64)
        );
    });
}

#[test]
fn median_subnet_alpha_price_averages_even_prices_and_ignores_root_zero_and_unadded() {
    new_test_ext(0).execute_with(|| {
        // If ROOT were included, its price would be 1 and change the median.
        NetworksAdded::<Test>::insert(NetUid::ROOT, true);

        let n1 = add_dynamic_network(&U256::from(301), &U256::from(300)); // eligible, price 2
        let n2 = add_dynamic_network(&U256::from(303), &U256::from(302)); // hidden,   price 4
        let n3 = add_dynamic_network(&U256::from(305), &U256::from(304)); // eligible, price 8
        let n4 = add_dynamic_network(&U256::from(307), &U256::from(306)); // zero,     price 0

        setup_reserves(n1, TaoBalance::from(200u64), AlphaBalance::from(100u64));
        setup_reserves(n2, TaoBalance::from(400u64), AlphaBalance::from(100u64));
        setup_reserves(n3, TaoBalance::from(800u64), AlphaBalance::from(100u64));
        setup_reserves(n4, TaoBalance::ZERO, AlphaBalance::from(100u64));

        NetworksAdded::<Test>::insert(n2, false);

        assert_eq!(
            <Test as pallet::Config>::SwapInterface::current_alpha_price(n1.into()),
            U96F32::from_num(2u64)
        );
        assert_eq!(
            <Test as pallet::Config>::SwapInterface::current_alpha_price(n2.into()),
            U96F32::from_num(4u64)
        );
        assert_eq!(
            <Test as pallet::Config>::SwapInterface::current_alpha_price(n3.into()),
            U96F32::from_num(8u64)
        );
        assert_eq!(
            <Test as pallet::Config>::SwapInterface::current_alpha_price(n4.into()),
            U96F32::from_num(0u64)
        );

        // Eligible prices are only {2, 8}, so the median is (2 + 8) / 2 = 5.
        assert_eq!(
            SubtensorModule::get_median_subnet_alpha_price(),
            U96F32::from_num(5u64)
        );
    });
}
