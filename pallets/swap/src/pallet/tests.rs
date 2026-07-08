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

    fn perquintill_to_f64(p: Perquintill) -> f64 {
        let parts = p.deconstruct() as f64;
        parts / 1_000_000_000_000_000_000_f64
    }

    /// cargo test --package pallet-subtensor-swap --lib -- pallet::tests::dispatchables::test_adjust_protocol_liquidity_happy --exact --nocapture
    #[test]
    fn test_adjust_protocol_liquidity_happy() {
        // test case: tao_delta, alpha_delta
        [
            (0_u64, 0_u64),
            (0_u64, 1_u64),
            (1_u64, 0_u64),
            (1_u64, 1_u64),
            (0_u64, 10_u64),
            (10_u64, 0_u64),
            (10_u64, 10_u64),
            (0_u64, 100_u64),
            (100_u64, 0_u64),
            (100_u64, 100_u64),
            (0_u64, 1_000_u64),
            (1_000_u64, 0_u64),
            (1_000_u64, 1_000_u64),
            (1_000_000_u64, 0_u64),
            (0_u64, 1_000_000_u64),
            (1_000_000_u64, 1_000_000_u64),
            (1_000_000_000_u64, 0_u64),
            (0_u64, 1_000_000_000_u64),
            (1_000_000_000_u64, 1_000_000_000_u64),
            (1_000_000_000_000_u64, 0_u64),
            (0_u64, 1_000_000_000_000_u64),
            (1_000_000_000_000_u64, 1_000_000_000_000_u64),
            (1_u64, 2_u64),
            (2_u64, 1_u64),
            (10_u64, 20_u64),
            (20_u64, 10_u64),
            (100_u64, 200_u64),
            (200_u64, 100_u64),
            (1_000_u64, 2_000_u64),
            (2_000_u64, 1_000_u64),
            (1_000_000_u64, 2_000_000_u64),
            (2_000_000_u64, 1_000_000_u64),
            (1_000_000_000_u64, 2_000_000_000_u64),
            (2_000_000_000_u64, 1_000_000_000_u64),
            (1_000_000_000_000_u64, 2_000_000_000_000_u64),
            (2_000_000_000_000_u64, 1_000_000_000_000_u64),
            (1_234_567_u64, 2_432_765_u64),
            (1_234_567_u64, 2_432_765_890_u64),
        ]
        .into_iter()
        .for_each(|(tao_delta, alpha_delta)| {
            new_test_ext().execute_with(|| {
                let netuid = NetUid::from(1);
                let tao_delta = TaoBalance::from(tao_delta);
                let alpha_delta = AlphaBalance::from(alpha_delta);

                // Initialize reserves and price
                let tao = TaoBalance::from(1_000_000_000_000_u64);
                let alpha = AlphaBalance::from(4_000_000_000_000_u64);
                TaoReserve::set_mock_reserve(netuid, tao);
                AlphaReserve::set_mock_reserve(netuid, alpha);
                let price_before = Swap::current_price(netuid);

                // Adjust reserves
                Swap::adjust_protocol_liquidity(netuid, tao_delta, alpha_delta);
                TaoReserve::set_mock_reserve(netuid, tao + tao_delta);
                AlphaReserve::set_mock_reserve(netuid, alpha + alpha_delta);

                // Check that price didn't change
                let price_after = Swap::current_price(netuid);
                assert_abs_diff_eq!(
                    price_before.to_num::<f64>(),
                    price_after.to_num::<f64>(),
                    epsilon = price_before.to_num::<f64>() / 1_000_000_000_000.
                );

                // Check that reserve weight was properly updated
                let new_tao = u64::from(tao + tao_delta) as f64;
                let new_alpha = u64::from(alpha + alpha_delta) as f64;
                let expected_quote_weight =
                    new_tao / (new_alpha * price_before.to_num::<f64>() + new_tao);
                let expected_quote_weight_delta = expected_quote_weight - 0.5;
                let res_weights = SwapBalancer::<Test>::get(netuid);
                let actual_quote_weight_delta =
                    perquintill_to_f64(res_weights.get_quote_weight()) - 0.5;
                let eps = expected_quote_weight / 1_000_000_000_000.;
                assert_abs_diff_eq!(
                    expected_quote_weight_delta,
                    actual_quote_weight_delta,
                    epsilon = eps
                );
            });
        });
    }

    /// This test case verifies that small gradual injections (like emissions in every block)
    /// in the worst case
    ///   - Do not cause price to change
    ///   - Result in the same weight change as one large injection
    ///
    /// This is a long test that only tests validity of weights math. Run again if changing
    /// Balancer::update_weights_for_added_liquidity
    ///
    /// cargo test --package pallet-subtensor-swap --lib -- pallet::tests::dispatchables::test_adjust_protocol_liquidity_deltas --exact --nocapture
    #[ignore]
    #[test]
    fn test_adjust_protocol_liquidity_deltas() {
        // The number of times (blocks) over which gradual injections will be made
        // One year price drift due to precision is under 1e-6
        const ITERATIONS: u64 = 2_700_000;
        const PRICE_PRECISION: f64 = 0.000_001;
        const PREC_LARGE_DELTA: f64 = 0.001;
        const WEIGHT_PRECISION: f64 = 0.000_000_000_000_000_001;

        let initial_tao_reserve = TaoBalance::from(1_000_000_000_000_000_u64);
        let initial_alpha_reserve = AlphaBalance::from(10_000_000_000_000_000_u64);

        // test case: tao_delta, alpha_delta, price_precision
        [
            (0_u64, 0_u64, PRICE_PRECISION),
            (0_u64, 1_u64, PRICE_PRECISION),
            (1_u64, 0_u64, PRICE_PRECISION),
            (1_u64, 1_u64, PRICE_PRECISION),
            (0_u64, 10_u64, PRICE_PRECISION),
            (10_u64, 0_u64, PRICE_PRECISION),
            (10_u64, 10_u64, PRICE_PRECISION),
            (0_u64, 100_u64, PRICE_PRECISION),
            (100_u64, 0_u64, PRICE_PRECISION),
            (100_u64, 100_u64, PRICE_PRECISION),
            (0_u64, 987_u64, PRICE_PRECISION),
            (987_u64, 0_u64, PRICE_PRECISION),
            (876_u64, 987_u64, PRICE_PRECISION),
            (0_u64, 1_000_u64, PRICE_PRECISION),
            (1_000_u64, 0_u64, PRICE_PRECISION),
            (1_000_u64, 1_000_u64, PRICE_PRECISION),
            (0_u64, 1_234_u64, PRICE_PRECISION),
            (1_234_u64, 0_u64, PRICE_PRECISION),
            (1_234_u64, 4_321_u64, PRICE_PRECISION),
            (1_234_000_u64, 4_321_000_u64, PREC_LARGE_DELTA),
            (1_234_u64, 4_321_000_u64, PREC_LARGE_DELTA),
        ]
        .into_iter()
        .for_each(|(tao_delta, alpha_delta, price_precision)| {
            new_test_ext().execute_with(|| {
                let netuid1 = NetUid::from(1);

                let tao_delta = TaoBalance::from(tao_delta);
                let alpha_delta = AlphaBalance::from(alpha_delta);

                // Initialize realistically large reserves
                let mut tao = initial_tao_reserve;
                let mut alpha = initial_alpha_reserve;
                TaoReserve::set_mock_reserve(netuid1, tao);
                AlphaReserve::set_mock_reserve(netuid1, alpha);
                let price_before = Swap::current_price(netuid1);

                // Adjust reserves gradually
                for _ in 0..ITERATIONS {
                    Swap::adjust_protocol_liquidity(netuid1, tao_delta, alpha_delta);
                    tao += tao_delta;
                    alpha += alpha_delta;
                    TaoReserve::set_mock_reserve(netuid1, tao);
                    AlphaReserve::set_mock_reserve(netuid1, alpha);
                }
                // Check that price didn't change
                let price_after = Swap::current_price(netuid1);
                assert_abs_diff_eq!(
                    price_before.to_num::<f64>(),
                    price_after.to_num::<f64>(),
                    epsilon = price_precision
                );

                /////////////////////////
                // Now do one-time big injection with another netuid and compare weights
                let netuid2 = NetUid::from(2);

                // Initialize same large reserves
                TaoReserve::set_mock_reserve(netuid2, initial_tao_reserve);
                AlphaReserve::set_mock_reserve(netuid2, initial_alpha_reserve);

                // Adjust reserves by one large amount at once
                let tao_delta_once = TaoBalance::from(ITERATIONS * u64::from(tao_delta));
                let alpha_delta_once = AlphaBalance::from(ITERATIONS * u64::from(alpha_delta));
                Swap::adjust_protocol_liquidity(netuid2, tao_delta_once, alpha_delta_once);
                TaoReserve::set_mock_reserve(netuid2, initial_tao_reserve + tao_delta_once);
                AlphaReserve::set_mock_reserve(netuid2, initial_alpha_reserve + alpha_delta_once);

                // Compare reserve weights for netuid 1 and 2
                let res_weights1 = SwapBalancer::<Test>::get(netuid1);
                let res_weights2 = SwapBalancer::<Test>::get(netuid2);
                let actual_quote_weight1 = perquintill_to_f64(res_weights1.get_quote_weight());
                let actual_quote_weight2 = perquintill_to_f64(res_weights2.get_quote_weight());
                assert_abs_diff_eq!(
                    actual_quote_weight1,
                    actual_quote_weight2,
                    epsilon = WEIGHT_PRECISION
                );
            });
        });
    }

    /// Should work ok when initial alpha is zero
    /// cargo test --package pallet-subtensor-swap --lib -- pallet::tests::dispatchables::test_adjust_protocol_liquidity_zero_alpha --exact --nocapture
    #[test]
    fn test_adjust_protocol_liquidity_zero_alpha() {
        // test case: tao_delta, alpha_delta
        [
            (0_u64, 0_u64),
            (0_u64, 1_u64),
            (1_u64, 0_u64),
            (1_u64, 1_u64),
            (0_u64, 10_u64),
            (10_u64, 0_u64),
            (10_u64, 10_u64),
            (0_u64, 100_u64),
            (100_u64, 0_u64),
            (100_u64, 100_u64),
            (0_u64, 1_000_u64),
            (1_000_u64, 0_u64),
            (1_000_u64, 1_000_u64),
            (1_000_000_u64, 0_u64),
            (0_u64, 1_000_000_u64),
            (1_000_000_u64, 1_000_000_u64),
            (1_000_000_000_u64, 0_u64),
            (0_u64, 1_000_000_000_u64),
            (1_000_000_000_u64, 1_000_000_000_u64),
            (1_000_000_000_000_u64, 0_u64),
            (0_u64, 1_000_000_000_000_u64),
            (1_000_000_000_000_u64, 1_000_000_000_000_u64),
            (1_u64, 2_u64),
            (2_u64, 1_u64),
            (10_u64, 20_u64),
            (20_u64, 10_u64),
            (100_u64, 200_u64),
            (200_u64, 100_u64),
            (1_000_u64, 2_000_u64),
            (2_000_u64, 1_000_u64),
            (1_000_000_u64, 2_000_000_u64),
            (2_000_000_u64, 1_000_000_u64),
            (1_000_000_000_u64, 2_000_000_000_u64),
            (2_000_000_000_u64, 1_000_000_000_u64),
            (1_000_000_000_000_u64, 2_000_000_000_000_u64),
            (2_000_000_000_000_u64, 1_000_000_000_000_u64),
            (1_234_567_u64, 2_432_765_u64),
            (1_234_567_u64, 2_432_765_890_u64),
        ]
        .into_iter()
        .for_each(|(tao_delta, alpha_delta)| {
            new_test_ext().execute_with(|| {
                let netuid = NetUid::from(1);

                let tao_delta = TaoBalance::from(tao_delta);
                let alpha_delta = AlphaBalance::from(alpha_delta);

                // Initialize reserves and price
                // broken state: Zero price because of zero alpha reserve
                let tao = TaoBalance::from(1_000_000_000_000_u64);
                let alpha = AlphaBalance::from(0_u64);
                TaoReserve::set_mock_reserve(netuid, tao);
                AlphaReserve::set_mock_reserve(netuid, alpha);
                let price_before = Swap::current_price(netuid);
                assert_eq!(price_before, U64F64::from_num(0));
                let new_tao = u64::from(tao + tao_delta) as f64;
                let new_alpha = u64::from(alpha + alpha_delta) as f64;

                // Adjust reserves
                Swap::adjust_protocol_liquidity(netuid, tao_delta, alpha_delta);
                TaoReserve::set_mock_reserve(netuid, tao + tao_delta);
                AlphaReserve::set_mock_reserve(netuid, alpha + alpha_delta);

                let res_weights = SwapBalancer::<Test>::get(netuid);
                let actual_quote_weight = perquintill_to_f64(res_weights.get_quote_weight());

                // Check that price didn't change
                let price_after = Swap::current_price(netuid);
                if new_alpha == 0. {
                    // If the pool state is still broken (∆x = 0), no change
                    assert_eq!(actual_quote_weight, 0.5);
                    assert_eq!(price_after, U64F64::from_num(0));
                } else {
                    // Price got fixed
                    let expected_price = new_tao / new_alpha;
                    assert_abs_diff_eq!(
                        expected_price,
                        price_after.to_num::<f64>(),
                        epsilon = price_before.to_num::<f64>() / 1_000_000_000_000.
                    );
                    assert_eq!(actual_quote_weight, 0.5);
                }
            });
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

/// Simple palswap path: PalSwap is initialized, but no positions, only protocol; function
/// must still clear any residual storages and succeed.
/// TODO: Revise when user liquidity is available
#[test]
fn test_liquidate_pal_simple_ok_and_clears() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(202);

        // Insert map values
        FeeRate::<Test>::insert(netuid, 1_000);
        PalSwapInitialized::<Test>::insert(netuid, true);
        let w_quote_pt = Perquintill::from_rational(1u128, 2u128);
        let bal = Balancer::new(w_quote_pt).unwrap();
        SwapBalancer::<Test>::insert(netuid, bal);

        // Sanity: PalSwap is not initialized
        assert!(PalSwapInitialized::<Test>::get(netuid));

        // ACT
        assert!(Pallet::<Test>::do_clear_protocol_liquidity(
            netuid,
            &mut WeightMeter::with_limit(Weight::from_parts(u64::MAX, u64::MAX))
        ));

        // All single-key maps should not have the key after liquidation
        assert!(!FeeRate::<Test>::contains_key(netuid));
        assert!(!PalSwapInitialized::<Test>::contains_key(netuid));
        assert!(!SwapBalancer::<Test>::contains_key(netuid));
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
