#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]

use approx::assert_abs_diff_eq;
use frame_support::weights::WeightMeter;
use frame_support::{assert_noop, assert_ok};
use sp_arithmetic::Perquintill;
use sp_runtime::DispatchError;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{NetUid, Token};
use subtensor_swap_interface::{Order as OrderT, SwapHandler};

use super::*;
use crate::mock::*;
use crate::pallet::swap_step::*;

// Run all tests:
// cargo test --package pallet-subtensor-swap --lib -- pallet::tests --nocapture

#[allow(dead_code)]
fn get_min_price() -> U64F64 {
    U64F64::from_num(Pallet::<Test>::min_price_inner::<TaoBalance>())
        / U64F64::from_num(1_000_000_000)
}

#[allow(dead_code)]
fn get_max_price() -> U64F64 {
    U64F64::from_num(Pallet::<Test>::max_price_inner::<TaoBalance>())
        / U64F64::from_num(1_000_000_000)
}

mod dispatchables {
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

    #[test]
    fn test_adjust_protocol_liquidity_preserves_price_and_curve_scale() {
        for (tao_delta, alpha_delta) in [
            (0, 0),
            (1, 0),
            (0, 1),
            (200_000, 1_000),
            (1_000, 200_000),
            (1_000_000_000_000, 2_000_000_000_000),
        ] {
            new_test_ext().execute_with(|| {
                let netuid = NetUid::from(1);
                let tao = 1_000_000_000_000u64;
                let alpha = 4_000_000_000_000u64;
                TaoReserve::set_mock_reserve(netuid, tao.into());
                AlphaReserve::set_mock_reserve(netuid, alpha.into());
                assert_ok!(Swap::maybe_initialize_palswap(netuid, None));
                let price = Swap::current_price(netuid);
                let before = SwapSuperellipse::<Test>::get(netuid).unwrap();
                let accepted =
                    Swap::adjust_protocol_liquidity(netuid, tao_delta.into(), alpha_delta.into())
                        .unwrap();
                assert_eq!(
                    accepted,
                    (TaoBalance::from(tao_delta), AlphaBalance::from(alpha_delta))
                );
                TaoReserve::set_mock_reserve(netuid, (tao + tao_delta).into());
                AlphaReserve::set_mock_reserve(netuid, (alpha + alpha_delta).into());
                assert_abs_diff_eq!(
                    Swap::current_price(netuid).to_num::<f64>(),
                    price.to_num::<f64>(),
                    epsilon = 1e-12
                );
                let after = SwapSuperellipse::<Test>::get(netuid).unwrap();
                // Translating back reproduces the exact original fixed curve.
                // Verify finite-trade quotes are unchanged by one-sided injections.
                let expected = before.buy_output(alpha, tao, 1_000_000).unwrap();
                let actual = after
                    .buy_output(alpha + alpha_delta, tao + tao_delta, 1_000_000)
                    .unwrap();
                assert_eq!(actual, expected);
            });
        }
    }

    #[test]
    fn test_adjust_protocol_liquidity_activates_pending_reservoirs() {
        new_test_ext().execute_with(|| {
            let netuid = NetUid::from(1);
            TaoReserve::set_mock_reserve(netuid, 1_000_000u64.into());
            AlphaReserve::set_mock_reserve(netuid, 1_000_000u64.into());
            assert_ok!(Swap::maybe_initialize_palswap(netuid, None));
            let price = Swap::current_price(netuid);
            BalancerTaoReservoir::<Test>::insert(netuid, TaoBalance::from(200_000));
            BalancerAlphaReservoir::<Test>::insert(netuid, AlphaBalance::from(20_000));
            let accepted =
                Swap::adjust_protocol_liquidity(netuid, 300u64.into(), 400u64.into()).unwrap();
            assert_eq!(
                accepted,
                (TaoBalance::from(200_300), AlphaBalance::from(20_400))
            );
            assert!(!BalancerTaoReservoir::<Test>::contains_key(netuid));
            assert!(!BalancerAlphaReservoir::<Test>::contains_key(netuid));
            TaoReserve::set_mock_reserve(netuid, 1_200_300u64.into());
            AlphaReserve::set_mock_reserve(netuid, 1_020_400u64.into());
            assert_abs_diff_eq!(
                Swap::current_price(netuid).to_num::<f64>(),
                price.to_num::<f64>(),
                epsilon = 1e-9
            );
        });
    }

    #[test]
    fn test_liquidity_translation_errors_preserve_all_storage() {
        for (tao, alpha, pending_tao, pending_alpha, tao_delta, alpha_delta) in [
            // Overflow combining already materialized pending liquidity with emissions.
            (1_000_000, 4_000_000, u64::MAX, 0, 1, 0),
            (1_000_000, 4_000_000, 0, u64::MAX, 0, 1),
            // The new physical reserve cannot be represented, in either token.
            (u64::MAX - 1, 4_000_000, 0, 0, 2, 0),
            (1_000_000, u64::MAX - 1, 0, 0, 0, 2),
        ] {
            new_test_ext().execute_with(|| {
                let netuid = NetUid::from(1);
                TaoReserve::set_mock_reserve(netuid, TaoBalance::from(tao));
                AlphaReserve::set_mock_reserve(netuid, AlphaBalance::from(alpha));
                assert_ok!(Swap::maybe_initialize_palswap(netuid, None));
                BalancerTaoReservoir::<Test>::insert(netuid, TaoBalance::from(pending_tao));
                BalancerAlphaReservoir::<Test>::insert(netuid, AlphaBalance::from(pending_alpha));
                let curve = SwapSuperellipse::<Test>::get(netuid).unwrap();
                let storage_before = sp_io::storage::root(sp_runtime::StateVersion::V1);
                assert!(
                    Swap::adjust_protocol_liquidity(netuid, tao_delta.into(), alpha_delta.into())
                        .is_err()
                );
                assert_eq!(
                    sp_io::storage::root(sp_runtime::StateVersion::V1),
                    storage_before
                );
                assert_eq!(SwapSuperellipse::<Test>::get(netuid).unwrap(), curve);
                assert_eq!(
                    u64::from(BalancerTaoReservoir::<Test>::get(netuid)),
                    pending_tao
                );
                assert_eq!(
                    u64::from(BalancerAlphaReservoir::<Test>::get(netuid)),
                    pending_alpha
                );
            });
        }
    }

    #[test]
    fn test_invalid_existing_curve_rejects_injection_without_mutation() {
        new_test_ext().execute_with(|| {
            let netuid = NetUid::from(1);
            TaoReserve::set_mock_reserve(netuid, 1_000u64.into());
            AlphaReserve::set_mock_reserve(netuid, 4_000u64.into());
            assert_ok!(Swap::maybe_initialize_palswap(netuid, None));
            // Corrupt physical accounting beyond the anchored positive branch.
            TaoReserve::set_mock_reserve(netuid, 10_000u64.into());
            let before = sp_io::storage::root(sp_runtime::StateVersion::V1);
            assert!(Swap::adjust_protocol_liquidity(netuid, 100u64.into(), 100u64.into()).is_err());
            assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), before);
        });
    }

    #[test]
    fn test_empty_pool_initializes_after_funding() {
        new_test_ext().execute_with(|| {
            let netuid = NetUid::from(1);
            TaoReserve::set_mock_reserve(netuid, 0u64.into());
            AlphaReserve::set_mock_reserve(netuid, 0u64.into());
            assert_eq!(Swap::current_price(netuid), U64F64::from_num(0));
            let accepted =
                Swap::adjust_protocol_liquidity(netuid, 1_000_000u64.into(), 4_000_000u64.into())
                    .unwrap();
            assert_eq!(
                accepted,
                (TaoBalance::from(1_000_000), AlphaBalance::from(4_000_000))
            );
            TaoReserve::set_mock_reserve(netuid, accepted.0);
            AlphaReserve::set_mock_reserve(netuid, accepted.1);
            assert_ok!(Swap::maybe_initialize_palswap(netuid, None));
            assert!(SwapSuperellipse::<Test>::contains_key(netuid));
            assert_abs_diff_eq!(
                Swap::current_price(netuid).to_num::<f64>(),
                0.25,
                epsilon = 1e-9
            );
        });
    }
}

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

        assert!(SwapSuperellipse::<Test>::contains_key(netuid));
        // Archived Balancer calibration remains available.
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
            SwapSuperellipse::<Test>::remove(netuid);
            PalSwapInitialized::<Test>::remove(netuid);
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
                ellipse_output(x as u64, y as u64, 0.5, swap_amount - expected_fee, true) as f64
            } else {
                ellipse_output(x as u64, y as u64, 0.5, swap_amount - expected_fee, false) as f64
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
            GetAlphaForTao::with_amount(10_000_000),
            1000.0,
            true,
        );
        perform_test(
            3.into(),
            GetAlphaForTao::with_amount(100_000_000_u64),
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

/// Independent floating-point reference for the migration-calibrated ellipse.
fn ellipse_output(alpha: u64, tao: u64, weight: f64, input: u64, buy: bool) -> u64 {
    let a = 2.0 * weight * alpha as f64;
    let b = 2.0 * (1.0 - weight) * tao as f64;
    let q = input as f64;
    if buy {
        (a * (2.0 - (1.0 - q / b).powi(2)).sqrt() - a) as u64
    } else {
        (b * (2.0 - (1.0 - q / a).powi(2)).sqrt() - b) as u64
    }
}

#[test]
fn test_convert_deltas_matches_independent_ellipse_reference() {
    new_test_ext().execute_with(|| {
        for (tao, alpha) in [
            (1_000_000, 1_500_000),
            (1_000_000_000, 4_000_000_000),
            (500_000_000_000_000, 10_000_000_000_000_000),
        ] {
            for weight in [0.1, 0.49999999, 0.5, 0.50000001, 0.9] {
                let netuid = NetUid::from(1);
                TaoReserve::set_mock_reserve(netuid, TaoBalance::from(tao));
                AlphaReserve::set_mock_reserve(netuid, AlphaBalance::from(alpha));
                let quote = Perquintill::from_rational((weight * 1e9) as u128, 1_000_000_000u128);
                let curve = superellipse::Superellipse::from_balancer(alpha, tao, quote).unwrap();
                SwapSuperellipse::<Test>::insert(netuid, curve);
                for input in [1, 100, tao / 100, tao / 20] {
                    let actual = BasicSwapStep::<Test, TaoBalance, AlphaBalance>::convert_deltas(
                        netuid,
                        input.into(),
                    )
                    .unwrap();
                    let expected = ellipse_output(alpha, tao, weight, input, true);
                    assert_abs_diff_eq!(u64::from(actual), expected, epsilon = 16);
                }
                for input in [1, 100, alpha / 100, alpha / 20] {
                    let actual = BasicSwapStep::<Test, AlphaBalance, TaoBalance>::convert_deltas(
                        netuid,
                        input.into(),
                    )
                    .unwrap();
                    let expected = ellipse_output(alpha, tao, weight, input, false);
                    assert_abs_diff_eq!(u64::from(actual), expected, epsilon = 16);
                }
            }
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

#[test]
fn test_swap_rejects_input_over_1000x_input_reserve() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        TaoReserve::set_mock_reserve(netuid, TaoBalance::from(1_000));
        AlphaReserve::set_mock_reserve(netuid, AlphaBalance::from(1_000));

        assert_noop!(
            Pallet::<Test>::do_swap(
                netuid,
                GetTaoForAlpha::with_amount(1_000_001),
                get_min_price(),
                true,
                false,
            ),
            Error::<Test>::SwapInputTooLarge
        );
        assert_noop!(
            Pallet::<Test>::do_swap(
                netuid,
                GetAlphaForTao::with_amount(1_000_001),
                get_max_price(),
                true,
                false,
            ),
            Error::<Test>::SwapInputTooLarge
        );
    });
}

#[test]
fn test_sim_swap_rejects_input_over_1000x_input_reserve() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        TaoReserve::set_mock_reserve(netuid, TaoBalance::from(1_000));
        AlphaReserve::set_mock_reserve(netuid, AlphaBalance::from(1_000));

        assert_noop!(
            Pallet::<Test>::sim_swap(netuid, GetTaoForAlpha::with_amount(1_001_000)),
            Error::<Test>::SwapInputTooLarge
        );
        assert_noop!(
            Pallet::<Test>::sim_swap(netuid, GetAlphaForTao::with_amount(1_001_000)),
            Error::<Test>::SwapInputTooLarge
        );
    });
}

#[test]
fn test_swap_allows_input_at_1000x_input_reserve() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        TaoReserve::set_mock_reserve(netuid, TaoBalance::from(1_000));
        AlphaReserve::set_mock_reserve(netuid, AlphaBalance::from(1_000));

        assert_ok!(Pallet::<Test>::do_swap(
            netuid,
            GetTaoForAlpha::with_amount(1_000_000),
            get_min_price(),
            true,
            true,
        ));
        assert_ok!(Pallet::<Test>::do_swap(
            netuid,
            GetAlphaForTao::with_amount(1_000_000),
            get_max_price(),
            true,
            true,
        ));
    });
}

#[test]
fn test_oversized_orders_partial_fill_before_curve_endpoint() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        TaoReserve::set_mock_reserve(netuid, 1_000_000u64.into());
        AlphaReserve::set_mock_reserve(netuid, 4_000_000u64.into());
        assert_ok!(Swap::maybe_initialize_palswap(netuid, None));
        let curve = SwapSuperellipse::<Test>::get(netuid).unwrap();
        let buy = Swap::do_swap(
            netuid,
            GetAlphaForTao::with_amount(10_000_000),
            get_max_price(),
            true,
            false,
        )
        .unwrap();
        assert!(buy.amount_paid_in > TaoBalance::ZERO);
        assert!(u64::from(buy.amount_paid_in) < 1_000_000);
        assert!(buy.amount_paid_out > AlphaBalance::ZERO);
        assert!(u64::from(buy.amount_paid_out) < 4_000_000);
        assert!(
            curve
                .calculate_price(
                    4_000_000 - u64::from(buy.amount_paid_out),
                    1_000_000 + u64::from(buy.amount_paid_in)
                )
                .is_ok()
        );
        let sell = Swap::do_swap(
            netuid,
            GetTaoForAlpha::with_amount(40_000_000),
            get_min_price(),
            true,
            false,
        )
        .unwrap();
        assert!(sell.amount_paid_in > AlphaBalance::ZERO);
        assert!(u64::from(sell.amount_paid_in) < 4_000_000);
        assert!(sell.amount_paid_out > TaoBalance::ZERO);
        assert!(u64::from(sell.amount_paid_out) < 1_000_000);
        assert!(
            curve
                .calculate_price(
                    4_000_000 + u64::from(sell.amount_paid_in),
                    1_000_000 - u64::from(sell.amount_paid_out)
                )
                .is_ok()
        );
        assert_eq!(SwapSuperellipse::<Test>::get(netuid).unwrap(), curve);
    });
}

#[test]
fn test_large_full_target_price_does_not_prevent_finite_limit_fill() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let tao = u64::MAX / 2;
        let alpha = 10u64;
        TaoReserve::set_mock_reserve(netuid, tao.into());
        AlphaReserve::set_mock_reserve(netuid, alpha.into());
        assert_ok!(Swap::maybe_initialize_palswap(netuid, None));
        let limit = Swap::current_price(netuid) * U64F64::from_num(2);
        let curve = SwapSuperellipse::<Test>::get(netuid).unwrap();
        let result =
            Swap::do_swap(netuid, GetAlphaForTao::with_amount(tao), limit, true, false).unwrap();
        assert!(result.amount_paid_out > AlphaBalance::ZERO);
        assert!(u64::from(result.amount_paid_in) < tao);
        let final_price = curve
            .calculate_price(
                alpha - u64::from(result.amount_paid_out),
                tao + u64::from(result.amount_paid_in),
            )
            .unwrap();
        assert!(final_price <= limit);
    });
}

#[test]
fn test_partial_fill_respects_buy_and_sell_price_limits_and_fees() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let tao = 1_000_000_000u64;
        let alpha = 4_000_000_000u64;
        TaoReserve::set_mock_reserve(netuid, tao.into());
        AlphaReserve::set_mock_reserve(netuid, alpha.into());
        assert_ok!(Swap::maybe_initialize_palswap(netuid, None));
        FeeRate::<Test>::insert(netuid, 1_000);
        let before = SwapSuperellipse::<Test>::get(netuid).unwrap();
        let buy = Swap::do_swap(
            netuid,
            GetAlphaForTao::with_amount(500_000_000),
            U64F64::from_num(0.3),
            false,
            false,
        )
        .unwrap();
        assert!(buy.amount_paid_in > TaoBalance::ZERO);
        assert!(u64::from(buy.amount_paid_in) + u64::from(buy.fee_paid) < 500_000_000);
        assert_eq!(buy.fee_paid, buy.fee_to_block_author);
        let buy_price = before
            .calculate_price(
                alpha - u64::from(buy.amount_paid_out),
                tao + u64::from(buy.amount_paid_in),
            )
            .unwrap();
        assert!(buy_price <= U64F64::from_num(0.3));
        assert_abs_diff_eq!(buy_price.to_num::<f64>(), 0.3, epsilon = 1e-8);
        // The reserve abstraction is updated by the caller, so this sell starts
        // from the same initial balances and independently tests the other direction.
        let sell = Swap::do_swap(
            netuid,
            GetTaoForAlpha::with_amount(2_000_000_000),
            U64F64::from_num(0.2),
            false,
            false,
        )
        .unwrap();
        assert!(sell.amount_paid_in > AlphaBalance::ZERO);
        assert!(u64::from(sell.amount_paid_in) + u64::from(sell.fee_paid) < 2_000_000_000);
        assert_eq!(sell.fee_paid, sell.fee_to_block_author);
        let sell_price = before
            .calculate_price(
                alpha + u64::from(sell.amount_paid_in),
                tao - u64::from(sell.amount_paid_out),
            )
            .unwrap();
        assert!(sell_price >= U64F64::from_num(0.2));
        assert_abs_diff_eq!(sell_price.to_num::<f64>(), 0.2, epsilon = 1e-8);
        let rate = 1_000.0 / (u16::MAX as f64 - 1_000.0);
        assert_abs_diff_eq!(
            u64::from(buy.fee_paid),
            (u64::from(buy.amount_paid_in) as f64 * rate) as u64,
            epsilon = 1
        );
        assert_abs_diff_eq!(
            u64::from(sell.fee_paid),
            (u64::from(sell.amount_paid_in) as f64 * rate) as u64,
            epsilon = 1
        );
    });
}

#[test]
fn test_simulation_does_not_initialize_or_mutate_curve() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        TaoReserve::set_mock_reserve(netuid, 1_000_000_000u64.into());
        AlphaReserve::set_mock_reserve(netuid, 4_000_000_000u64.into());
        let state_before = sp_io::storage::root(sp_runtime::StateVersion::V1);
        let quote = Swap::sim_swap(netuid, GetAlphaForTao::with_amount(1_000_000)).unwrap();
        assert!(quote.amount_paid_out > AlphaBalance::ZERO);
        assert_eq!(
            sp_io::storage::root(sp_runtime::StateVersion::V1),
            state_before
        );
        assert!(!SwapSuperellipse::<Test>::contains_key(netuid));
        assert!(!PalSwapInitialized::<Test>::get(netuid));
        assert_ok!(Swap::maybe_initialize_palswap(netuid, None));
        let state_before = sp_io::storage::root(sp_runtime::StateVersion::V1);
        assert_eq!(
            Swap::sim_swap(netuid, GetAlphaForTao::with_amount(1_000_000)).unwrap(),
            quote
        );
        assert_eq!(
            sp_io::storage::root(sp_runtime::StateVersion::V1),
            state_before
        );
    });
}

#[allow(dead_code)]
fn bbox(t: U64F64, a: U64F64, b: U64F64) -> U64F64 {
    if t < a {
        a
    } else if t > b {
        b
    } else {
        t
    }
}

#[allow(dead_code)]
fn print_current_price(netuid: NetUid) {
    let current_price = Pallet::<Test>::current_price(netuid);
    log::trace!("Current price: {current_price:.6}");
}

/// Reservoir liquidity is already materialized but not price-active; direct
/// cleanup materializes it into the reserve abstraction before clearing.
#[test]
fn test_clear_protocol_liquidity_clears_nonzero_reservoirs() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(202);

        // Insert map values
        FeeRate::<Test>::insert(netuid, 1_000);
        PalSwapInitialized::<Test>::insert(netuid, true);
        BalancerTaoReservoir::<Test>::insert(netuid, TaoBalance::from(12_345_u64));
        BalancerAlphaReservoir::<Test>::insert(netuid, AlphaBalance::from(67_890_u64));
        let w_quote_pt = Perquintill::from_rational(1u128, 2u128);
        let bal = Balancer::new(w_quote_pt).unwrap();
        SwapBalancer::<Test>::insert(netuid, bal);
        SwapSuperellipse::<Test>::insert(
            netuid,
            superellipse::Superellipse::from_balancer(
                4_000_000_000_000,
                1_000_000_000_000,
                w_quote_pt,
            )
            .unwrap(),
        );

        // Sanity: PalSwap is initialized
        assert!(PalSwapInitialized::<Test>::get(netuid));

        // ACT
        assert!(Pallet::<Test>::do_clear_protocol_liquidity(
            netuid,
            &mut WeightMeter::with_limit(Weight::from_parts(u64::MAX, u64::MAX))
        ));

        assert!(!FeeRate::<Test>::contains_key(netuid));
        assert!(!PalSwapInitialized::<Test>::contains_key(netuid));
        assert!(!SwapBalancer::<Test>::contains_key(netuid));
        assert!(!SwapSuperellipse::<Test>::contains_key(netuid));
        assert!(!BalancerTaoReservoir::<Test>::contains_key(netuid));
        assert!(!BalancerAlphaReservoir::<Test>::contains_key(netuid));
    });
}

#[test]
fn test_clear_protocol_liquidity_green_path() {
    new_test_ext().execute_with(|| {
        // --- Arrange ---
        let netuid = NetUid::from(1);

        // Initialize swap state
        assert_ok!(Pallet::<Test>::maybe_initialize_palswap(netuid, None));
        assert!(
            PalSwapInitialized::<Test>::get(netuid),
            "Swap must be initialized"
        );

        // --- Act ---
        // Green path: just clear protocol liquidity and wipe all V3 state.
        assert!(Pallet::<Test>::do_clear_protocol_liquidity(
            netuid,
            &mut WeightMeter::with_limit(Weight::from_parts(u64::MAX, u64::MAX))
        ));

        // Flags
        assert!(!PalSwapInitialized::<Test>::contains_key(netuid));

        // Knobs removed
        assert!(!FeeRate::<Test>::contains_key(netuid));

        // --- And it's idempotent ---
        assert!(Pallet::<Test>::do_clear_protocol_liquidity(
            netuid,
            &mut WeightMeter::with_limit(Weight::from_parts(u64::MAX, u64::MAX))
        ));
        assert!(!PalSwapInitialized::<Test>::contains_key(netuid));
    });
}

// cargo test --package pallet-subtensor-swap --lib -- pallet::tests::test_migrate_swapv3_to_balancer --exact --nocapture
#[test]
fn test_migrate_swapv3_to_balancer() {
    use crate::migrations::migrate_swapv3_to_balancer::deprecated_swap_maps;
    use substrate_fixed::types::U64F64;

    new_test_ext().execute_with(|| {
        let migration =
            crate::migrations::migrate_swapv3_to_balancer::migrate_swapv3_to_balancer::<Test>;
        let netuid = NetUid::from(1);

        // Insert deprecated maps values
        deprecated_swap_maps::AlphaSqrtPrice::<Test>::insert(netuid, U64F64::from_num(1.23));
        deprecated_swap_maps::ScrapReservoirTao::<Test>::insert(netuid, TaoBalance::from(9876));
        deprecated_swap_maps::ScrapReservoirAlpha::<Test>::insert(netuid, AlphaBalance::from(9876));

        // Insert reserves that do not match the 1.23 price
        TaoReserve::set_mock_reserve(netuid, TaoBalance::from(1_000_000_000));
        AlphaReserve::set_mock_reserve(netuid, AlphaBalance::from(4_000_000_000_u64));

        // Run migration
        migration();

        // Test that values are removed from state
        assert!(!deprecated_swap_maps::AlphaSqrtPrice::<Test>::contains_key(
            netuid
        ));
        assert!(!deprecated_swap_maps::ScrapReservoirAlpha::<Test>::contains_key(netuid));

        // Test that subnet price is still 1.23^2
        assert_abs_diff_eq!(
            Swap::current_price(netuid).to_num::<f64>(),
            1.23 * 1.23,
            epsilon = 0.1
        );
    });
}

#[test]
fn test_migrate_swapv3_to_balancer_falls_back_to_default_when_price_init_fails() {
    use crate::migrations::migrate_swapv3_to_balancer::deprecated_swap_maps;
    use substrate_fixed::types::U64F64;

    new_test_ext().execute_with(|| {
        let migration =
            crate::migrations::migrate_swapv3_to_balancer::migrate_swapv3_to_balancer::<Test>;
        let migration_name =
            frame_support::BoundedVec::truncate_from(b"migrate_swapv3_to_balancer".to_vec());
        let netuid = NetUid::from(1);

        deprecated_swap_maps::AlphaSqrtPrice::<Test>::insert(netuid, U64F64::from_num(1));
        deprecated_swap_maps::ScrapReservoirTao::<Test>::insert(netuid, TaoBalance::from(9876));
        deprecated_swap_maps::ScrapReservoirAlpha::<Test>::insert(netuid, AlphaBalance::from(9876));

        TaoReserve::set_mock_reserve(netuid, TaoBalance::from(1));
        AlphaReserve::set_mock_reserve(netuid, AlphaBalance::from(1_000_000_000_000_u64));

        migration();

        assert!(!deprecated_swap_maps::AlphaSqrtPrice::<Test>::contains_key(
            netuid
        ));
        assert!(!deprecated_swap_maps::ScrapReservoirTao::<Test>::contains_key(netuid));
        assert!(!deprecated_swap_maps::ScrapReservoirAlpha::<Test>::contains_key(netuid));
        assert!(PalSwapInitialized::<Test>::get(netuid));
        assert_eq!(
            SwapBalancer::<Test>::get(netuid).get_quote_weight(),
            Perquintill::from_rational(1_u64, 2_u64)
        );
        assert!(HasMigrationRun::<Test>::get(&migration_name));
    });
}

#[test]
fn test_swap_storage_cleanup_is_wired_and_cleanup_only() {
    use frame_support::traits::Hooks;
    use sp_io::hashing::twox_128;

    new_test_ext().execute_with(|| {
        let legacy_items = [
            "AlphaSqrtPrice",
            "CurrentTick",
            "EnabledUserLiquidity",
            "FeeGlobalTao",
            "FeeGlobalAlpha",
            "LastPositionId",
            "ScrapReservoirTao",
            "ScrapReservoirAlpha",
            "Ticks",
            "TickIndexBitmapWords",
            "SwapV3Initialized",
            "CurrentLiquidity",
            "Positions",
        ];
        let legacy_keys: sp_std::vec::Vec<_> = legacy_items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let mut key = [twox_128(b"Swap"), twox_128(item.as_bytes())].concat();
                key.push(index as u8);
                sp_io::storage::set(&key, &[1]);
                key
            })
            .collect();

        let zero_netuid = NetUid::from(1);
        let pending_netuid = NetUid::from(2);
        BalancerTaoReservoir::<Test>::insert(zero_netuid, TaoBalance::ZERO);
        BalancerAlphaReservoir::<Test>::insert(zero_netuid, AlphaBalance::ZERO);
        BalancerTaoReservoir::<Test>::insert(pending_netuid, TaoBalance::from(10));
        BalancerAlphaReservoir::<Test>::insert(pending_netuid, AlphaBalance::from(20));

        <Pallet<Test> as Hooks<u64>>::on_runtime_upgrade();
        for (item, key) in legacy_items.iter().zip(legacy_keys) {
            assert!(
                sp_io::storage::get(&key).is_none(),
                "legacy prefix {item} was not cleared"
            );
        }
        assert!(!BalancerTaoReservoir::<Test>::contains_key(zero_netuid));
        assert!(!BalancerAlphaReservoir::<Test>::contains_key(zero_netuid));
        assert_eq!(
            BalancerTaoReservoir::<Test>::get(pending_netuid),
            TaoBalance::from(10)
        );
        assert_eq!(
            BalancerAlphaReservoir::<Test>::get(pending_netuid),
            AlphaBalance::from(20)
        );

        let migration_name = BoundedVec::truncate_from(b"migrate_swap_storage_cleanup_v2".to_vec());
        assert!(HasMigrationRun::<Test>::get(migration_name));
    });
}
