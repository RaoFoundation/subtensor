//! Tests for Uniswap-v3 → balancer storage migration.

use super::*;

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
