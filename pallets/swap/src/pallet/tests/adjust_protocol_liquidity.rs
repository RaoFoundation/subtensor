//! Tests for protocol liquidity injection via [`Pallet::adjust_protocol_liquidity`].

use super::*;

fn perquintill_to_f64(p: Perquintill) -> f64 {
    let parts = p.deconstruct() as f64;
    parts / 1_000_000_000_000_000_000_f64
}

/// cargo test --package pallet-subtensor-swap --lib -- pallet::tests::adjust_protocol_liquidity::test_adjust_protocol_liquidity_happy --exact --nocapture
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

#[test]
fn test_adjust_protocol_liquidity_materializes_tao_when_reservoiring_tao() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);

        let tao = TaoBalance::from(1_000_u64);
        let alpha = AlphaBalance::from(1_000_u64);
        TaoReserve::set_mock_reserve(netuid, tao);
        AlphaReserve::set_mock_reserve(netuid, alpha);

        let (price_active_tao, price_active_alpha) = Swap::adjust_protocol_liquidity(
            netuid,
            TaoBalance::from(200_000_u64),
            AlphaBalance::from(1_000_u64),
        );

        assert_eq!(price_active_tao, TaoBalance::ZERO);
        assert_eq!(price_active_alpha, AlphaBalance::from(1_000_u64));
        assert_eq!(
            BalancerTaoReservoir::<Test>::get(netuid),
            TaoBalance::from(200_000_u64)
        );
        assert_eq!(
            BalancerAlphaReservoir::<Test>::get(netuid),
            AlphaBalance::ZERO
        );
    });
}

#[test]
fn test_adjust_protocol_liquidity_materializes_alpha_when_reservoiring_alpha() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);

        let tao = TaoBalance::from(1_000_u64);
        let alpha = AlphaBalance::from(1_000_u64);
        TaoReserve::set_mock_reserve(netuid, tao);
        AlphaReserve::set_mock_reserve(netuid, alpha);

        let (price_active_tao, price_active_alpha) = Swap::adjust_protocol_liquidity(
            netuid,
            TaoBalance::from(1_000_u64),
            AlphaBalance::from(200_000_u64),
        );

        assert_eq!(price_active_tao, TaoBalance::from(1_000_u64));
        assert_eq!(price_active_alpha, AlphaBalance::ZERO);
        assert_eq!(BalancerTaoReservoir::<Test>::get(netuid), TaoBalance::ZERO);
        assert_eq!(
            BalancerAlphaReservoir::<Test>::get(netuid),
            AlphaBalance::from(200_000_u64)
        );
    });
}

#[test]
fn test_adjust_protocol_liquidity_retries_reservoir_with_new_injection() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);

        let mut tao = TaoBalance::from(1_000_u64);
        let mut alpha = AlphaBalance::from(1_000_u64);
        TaoReserve::set_mock_reserve(netuid, tao);
        AlphaReserve::set_mock_reserve(netuid, alpha);

        let (price_active_tao, price_active_alpha) = Swap::adjust_protocol_liquidity(
            netuid,
            TaoBalance::from(200_000_u64),
            AlphaBalance::from(1_000_u64),
        );
        assert_eq!(price_active_tao, TaoBalance::ZERO);
        assert_eq!(price_active_alpha, AlphaBalance::from(1_000_u64));
        tao += price_active_tao;
        alpha += price_active_alpha;
        TaoReserve::set_mock_reserve(netuid, tao);
        AlphaReserve::set_mock_reserve(netuid, alpha);

        let (price_active_tao, price_active_alpha) = Swap::adjust_protocol_liquidity(
            netuid,
            TaoBalance::from(1_000_u64),
            AlphaBalance::from(200_000_u64),
        );

        assert!(price_active_tao >= TaoBalance::from(1_000_u64));
        assert!(price_active_alpha >= AlphaBalance::from(200_000_u64));
        assert_eq!(BalancerTaoReservoir::<Test>::get(netuid), TaoBalance::ZERO);
        assert_eq!(
            BalancerAlphaReservoir::<Test>::get(netuid),
            AlphaBalance::ZERO
        );
    });
}

#[test]
fn test_adjust_protocol_liquidity_activates_reservoir_amounts() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);

        TaoReserve::set_mock_reserve(netuid, TaoBalance::from(1_000_000_u64));
        AlphaReserve::set_mock_reserve(netuid, AlphaBalance::from(1_000_000_u64));
        BalancerTaoReservoir::<Test>::insert(netuid, TaoBalance::from(10_000_u64));
        BalancerAlphaReservoir::<Test>::insert(netuid, AlphaBalance::from(20_000_u64));

        let tao_delta = TaoBalance::from(300_u64);
        let alpha_delta = AlphaBalance::from(400_u64);
        let (price_active_tao, price_active_alpha) =
            Swap::adjust_protocol_liquidity(netuid, tao_delta, alpha_delta);

        assert_eq!(price_active_tao, TaoBalance::from(10_300_u64));
        assert_eq!(price_active_alpha, AlphaBalance::from(20_400_u64));
        assert_eq!(BalancerTaoReservoir::<Test>::get(netuid), TaoBalance::ZERO);
        assert_eq!(
            BalancerAlphaReservoir::<Test>::get(netuid),
            AlphaBalance::ZERO
        );
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
/// cargo test --package pallet-subtensor-swap --lib -- pallet::tests::adjust_protocol_liquidity::test_adjust_protocol_liquidity_deltas --exact --nocapture
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
/// cargo test --package pallet-subtensor-swap --lib -- pallet::tests::adjust_protocol_liquidity::test_adjust_protocol_liquidity_zero_alpha --exact --nocapture
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
