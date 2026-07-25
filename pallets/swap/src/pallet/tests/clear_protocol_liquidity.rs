//! Tests for clearing protocol-owned swap liquidity and related state.

use super::*;

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

        // Sanity: PalSwap is not initialized
        assert!(PalSwapInitialized::<Test>::get(netuid));

        // ACT
        assert!(Pallet::<Test>::do_clear_protocol_liquidity(
            netuid,
            &mut WeightMeter::with_limit(Weight::from_parts(u64::MAX, u64::MAX))
        ));

        assert!(!FeeRate::<Test>::contains_key(netuid));
        assert!(!PalSwapInitialized::<Test>::contains_key(netuid));
        assert!(!SwapBalancer::<Test>::contains_key(netuid));
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
