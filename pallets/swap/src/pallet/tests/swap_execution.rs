//! Tests for swap execution paths (`do_swap`, delta conversion, rollback).

use super::*;

// cargo test --package pallet-subtensor-swap --lib -- pallet::tests::test_swap_basic --exact --nocapture
#[test]
fn test_swap_basic() {
    new_test_ext().execute_with(|| {
        fn perform_test<Order>(
            netuid: NetUid,
            order: Order,
            limit_price: f64,
            price_should_grow: bool,
        ) where
            Order: OrderT,
            BasicSwapStep<Test, Order::PaidIn, Order::PaidOut>:
                SwapStep<Test, Order::PaidIn, Order::PaidOut>,
        {
            let swap_amount = order.amount().to_u64();

            // Setup swap
            // Price is 0.25
            let initial_tao_reserve = TaoBalance::from(1_000_000_000_u64);
            let initial_alpha_reserve = AlphaBalance::from(4_000_000_000_u64);
            TaoReserve::set_mock_reserve(netuid, initial_tao_reserve);
            AlphaReserve::set_mock_reserve(netuid, initial_alpha_reserve);
            assert_ok!(Pallet::<Test>::maybe_initialize_palswap(netuid, None));

            // Get current price
            let current_price_before = Pallet::<Test>::current_price(netuid);

            // Get reserves
            let tao_reserve = TaoReserve::reserve(netuid.into()).to_u64();
            let alpha_reserve = AlphaReserve::reserve(netuid.into()).to_u64();

            // Expected fee amount
            let fee_rate = FeeRate::<Test>::get(netuid) as f64 / u16::MAX as f64;
            let expected_fee = (swap_amount as f64 * fee_rate) as u64;

            // Calculate expected output amount using f64 math
            // This is a simple case when w1 = w2 = 0.5, so there's no
            // exponentiation needed
            let x = alpha_reserve as f64;
            let y = tao_reserve as f64;
            let expected_output_amount = if price_should_grow {
                x * (1.0 - y / (y + (swap_amount - expected_fee) as f64))
            } else {
                y * (1.0 - x / (x + (swap_amount - expected_fee) as f64))
            };

            // Swap
            let limit_price_fixed = U64F64::from_num(limit_price);
            let swap_result =
                Pallet::<Test>::do_swap(netuid, order.clone(), limit_price_fixed, false, false)
                    .unwrap();
            assert_abs_diff_eq!(
                swap_result.amount_paid_out.to_u64(),
                expected_output_amount as u64,
                epsilon = 1
            );

            assert_abs_diff_eq!(
                swap_result.paid_in_reserve_delta() as u64,
                (swap_amount - expected_fee),
                epsilon = 1
            );
            assert_abs_diff_eq!(
                swap_result.paid_out_reserve_delta() as i64,
                -(expected_output_amount as i64),
                epsilon = 1
            );

            // Update reserves (because it happens outside of do_swap in stake_utils)
            if price_should_grow {
                TaoReserve::set_mock_reserve(
                    netuid,
                    TaoBalance::from(
                        (u64::from(initial_tao_reserve) as i128
                            + swap_result.paid_in_reserve_delta()) as u64,
                    ),
                );
                AlphaReserve::set_mock_reserve(
                    netuid,
                    AlphaBalance::from(
                        (u64::from(initial_alpha_reserve) as i128
                            + swap_result.paid_out_reserve_delta()) as u64,
                    ),
                );
            } else {
                TaoReserve::set_mock_reserve(
                    netuid,
                    TaoBalance::from(
                        (u64::from(initial_tao_reserve) as i128
                            + swap_result.paid_out_reserve_delta()) as u64,
                    ),
                );
                AlphaReserve::set_mock_reserve(
                    netuid,
                    AlphaBalance::from(
                        (u64::from(initial_alpha_reserve) as i128
                            + swap_result.paid_in_reserve_delta()) as u64,
                    ),
                );
            }

            // Assert that price movement is in correct direction
            let current_price_after = Pallet::<Test>::current_price(netuid);
            assert_eq!(
                current_price_after >= current_price_before,
                price_should_grow
            );
        }

        // Current price is 0.25
        // Test case is (order_type, liquidity, limit_price, output_amount)
        perform_test(1.into(), GetAlphaForTao::with_amount(1_000), 1000.0, true);
        perform_test(1.into(), GetAlphaForTao::with_amount(2_000), 1000.0, true);
        perform_test(1.into(), GetAlphaForTao::with_amount(123_456), 1000.0, true);
        perform_test(2.into(), GetTaoForAlpha::with_amount(1_000), 0.0001, false);
        perform_test(2.into(), GetTaoForAlpha::with_amount(2_000), 0.0001, false);
        perform_test(
            2.into(),
            GetTaoForAlpha::with_amount(123_456),
            0.0001,
            false,
        );
        perform_test(
            3.into(),
            GetAlphaForTao::with_amount(1_000_000_000),
            1000.0,
            true,
        );
        perform_test(
            3.into(),
            GetAlphaForTao::with_amount(10_000_000_000_u64),
            1000.0,
            true,
        );
    });
}

// cargo test --package pallet-subtensor-swap --lib -- pallet::impls::tests::test_swap_precision_edge_case --exact --show-output
#[test]
fn test_swap_precision_edge_case() {
    // Test case: tao_reserve, alpha_reserve, swap_amount
    [
        (1_000_u64, 1_000_u64, 999_500_u64),
        (1_000_000_u64, 1_000_000_u64, 999_500_000_u64),
    ]
    .into_iter()
    .for_each(|(tao_reserve, alpha_reserve, swap_amount)| {
        new_test_ext().execute_with(|| {
            let netuid = NetUid::from(1);
            let order = GetTaoForAlpha::with_amount(swap_amount);

            // Very low reserves
            TaoReserve::set_mock_reserve(netuid, TaoBalance::from(tao_reserve));
            AlphaReserve::set_mock_reserve(netuid, AlphaBalance::from(alpha_reserve));

            // Minimum possible limit price
            let limit_price: U64F64 = get_min_price();
            println!("limit_price = {:?}", limit_price);

            // Swap
            let swap_result =
                Pallet::<Test>::do_swap(netuid, order, limit_price, false, true).unwrap();

            assert!(swap_result.amount_paid_out > TaoBalance::ZERO);
        });
    });
}

#[test]
fn test_convert_deltas() {
    new_test_ext().execute_with(|| {
        for (tao, alpha, w_quote, delta_in) in [
            (1500, 1000, 0.5, 1),
            (1500, 1000, 0.5, 10000),
            (1500, 1000, 0.5, 1000000),
            (1500, 1000, 0.5, u64::MAX),
            (1, 1000000, 0.5, 1),
            (1, 1000000, 0.5, 10000),
            (1, 1000000, 0.5, 1000000),
            (1, 1000000, 0.5, u64::MAX),
            (1000000, 1, 0.5, 1),
            (1000000, 1, 0.5, 10000),
            (1000000, 1, 0.5, 1000000),
            (1000000, 1, 0.5, u64::MAX),
            (1500, 1000, 0.50000001, 1),
            (1500, 1000, 0.50000001, 10000),
            (1500, 1000, 0.50000001, 1000000),
            (1500, 1000, 0.50000001, u64::MAX),
            (1, 1000000, 0.50000001, 1),
            (1, 1000000, 0.50000001, 10000),
            (1, 1000000, 0.50000001, 1000000),
            (1, 1000000, 0.50000001, u64::MAX),
            (1000000, 1, 0.50000001, 1),
            (1000000, 1, 0.50000001, 10000),
            (1000000, 1, 0.50000001, 1000000),
            (1000000, 1, 0.50000001, u64::MAX),
            (1500, 1000, 0.49999999, 1),
            (1500, 1000, 0.49999999, 10000),
            (1500, 1000, 0.49999999, 1000000),
            (1500, 1000, 0.49999999, u64::MAX),
            (1, 1000000, 0.49999999, 1),
            (1, 1000000, 0.49999999, 10000),
            (1, 1000000, 0.49999999, 1000000),
            (1, 1000000, 0.49999999, u64::MAX),
            (1000000, 1, 0.49999999, 1),
            (1000000, 1, 0.49999999, 10000),
            (1000000, 1, 0.49999999, 1000000),
            (1000000, 1, 0.49999999, u64::MAX),
            // Low quote weight
            (1500, 1000, 0.1, 1),
            (1500, 1000, 0.1, 10000),
            (1500, 1000, 0.1, 1000000),
            (1500, 1000, 0.1, u64::MAX),
            (1, 1000000, 0.1, 1),
            (1, 1000000, 0.1, 10000),
            (1, 1000000, 0.1, 1000000),
            (1, 1000000, 0.1, u64::MAX),
            (1000000, 1, 0.1, 1),
            (1000000, 1, 0.1, 10000),
            (1000000, 1, 0.1, 1000000),
            (1000000, 1, 0.1, u64::MAX),
            // High quote weight
            (1500, 1000, 0.9, 1),
            (1500, 1000, 0.9, 10000),
            (1500, 1000, 0.9, 1000000),
            (1500, 1000, 0.9, u64::MAX),
            (1, 1000000, 0.9, 1),
            (1, 1000000, 0.9, 10000),
            (1, 1000000, 0.9, 1000000),
            (1, 1000000, 0.9, u64::MAX),
            (1000000, 1, 0.9, 1),
            (1000000, 1, 0.9, 10000),
            (1000000, 1, 0.9, 1000000),
            (1000000, 1, 0.9, u64::MAX),
        ] {
            // Initialize reserves and weights
            let netuid = NetUid::from(1);
            TaoReserve::set_mock_reserve(netuid, TaoBalance::from(tao));
            AlphaReserve::set_mock_reserve(netuid, AlphaBalance::from(alpha));
            assert_ok!(Pallet::<Test>::maybe_initialize_palswap(netuid, None));

            let w_accuracy = 1_000_000_000_f64;
            let w_quote_pt =
                Perquintill::from_rational((w_quote * w_accuracy) as u128, w_accuracy as u128);
            let bal = Balancer::new(w_quote_pt).unwrap();
            SwapBalancer::<Test>::insert(netuid, bal);

            // Calculate expected swap results (buy and sell) using f64 math
            let y = tao as f64;
            let x = alpha as f64;
            let d = delta_in as f64;
            let w1_div_w2 = (1. - w_quote) / w_quote;
            let w2_div_w1 = w_quote / (1. - w_quote);
            let expected_sell = y * (1. - (x / (x + d)).powf(w1_div_w2));
            let expected_buy = x * (1. - (y / (y + d)).powf(w2_div_w1));

            assert_abs_diff_eq!(
                u64::from(
                    BasicSwapStep::<Test, AlphaBalance, TaoBalance>::convert_deltas(
                        netuid,
                        delta_in.into()
                    )
                ),
                expected_sell as u64,
                epsilon = 2u64
            );
            assert_abs_diff_eq!(
                u64::from(
                    BasicSwapStep::<Test, TaoBalance, AlphaBalance>::convert_deltas(
                        netuid,
                        delta_in.into()
                    )
                ),
                expected_buy as u64,
                epsilon = 2u64
            );
        }
    });
}

#[test]
fn test_rollback_works() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);

        assert_eq!(
            Pallet::<Test>::do_swap(
                netuid,
                GetAlphaForTao::with_amount(1_000_000),
                u64::MAX.into(),
                false,
                true
            )
            .unwrap(),
            Pallet::<Test>::do_swap(
                netuid,
                GetAlphaForTao::with_amount(1_000_000),
                u64::MAX.into(),
                false,
                false
            )
            .unwrap()
        );
    })
}
