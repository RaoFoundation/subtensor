#![allow(
    unused,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used
)]
//! Alpha issuance and emission-cap triggers.

use super::helpers::*;
use super::prelude::*;

// Test basic alpha issuance in coinbase mechanism.
// This test verifies that:
// - Alpha issuance is initialized to 0 for new subnets
// - Alpha issuance is split evenly between subnets during coinbase
// - Each subnet receives the expected fraction of total emission
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::alpha_issuance::test_coinbase_alpha_issuance_base --exact --show-output --nocapture
#[test]
fn test_coinbase_alpha_issuance_base() {
    new_test_ext(1).execute_with(|| {
        let netuid1 = NetUid::from(1);
        let netuid2 = NetUid::from(2);
        let emission: u64 = 1_000_000;
        let emission_credit = SubtensorModule::mint_tao(emission.into());
        add_network(netuid1, 1, 0);
        add_network(netuid2, 1, 0);
        // Set up prices 1 and 1
        let initial: u64 = 1_000_000;
        SubnetTAO::<Test>::insert(netuid1, TaoBalance::from(initial));
        SubnetAlphaIn::<Test>::insert(netuid1, AlphaBalance::from(initial));
        SubnetTAO::<Test>::insert(netuid2, TaoBalance::from(initial));
        SubnetAlphaIn::<Test>::insert(netuid2, AlphaBalance::from(initial));
        // Keep root_proportion ~1 so the injection cap does not bind.
        set_full_injection_root_stake();
        // Check initial
        SubtensorModule::run_coinbase(emission_credit);
        // tao_in = 500_000
        // alpha_in = 500_000/price = 500_000
        assert_eq!(
            SubnetAlphaIn::<Test>::get(netuid1),
            (initial + emission / 2).into()
        );
        assert_eq!(
            SubnetAlphaIn::<Test>::get(netuid2),
            (initial + emission / 2).into()
        );
    });
}

// Test alpha issuance with different subnet flows.
// This test verifies that:
// - Alpha issuance is proportional to subnet flows
// - Higher priced subnets receive more TAO emission
// - Alpha issuance is correctly calculated based on price ratios
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::alpha_issuance::test_coinbase_alpha_issuance_different --exact --show-output --nocapture
#[test]
fn test_coinbase_alpha_issuance_different() {
    new_test_ext(1).execute_with(|| {
        let netuid1 = NetUid::from(1);
        let netuid2 = NetUid::from(2);
        let emission: u64 = 1_000_000;
        let emission_credit = SubtensorModule::mint_tao(emission.into());
        add_network(netuid1, 1, 0);
        add_network(netuid2, 1, 0);
        // Make subnets dynamic.
        SubnetMechanism::<Test>::insert(netuid1, 1);
        SubnetMechanism::<Test>::insert(netuid2, 1);
        // Setup prices 1 and 2
        let initial: u64 = 1_000_000;
        SubnetTAO::<Test>::insert(netuid1, TaoBalance::from(initial));
        SubnetAlphaIn::<Test>::insert(netuid1, AlphaBalance::from(initial));
        SubnetTAO::<Test>::insert(netuid2, TaoBalance::from(2 * initial));
        SubnetAlphaIn::<Test>::insert(netuid2, AlphaBalance::from(initial));
        // Price-based shares with prices 1 and 2 (1:2 ratio).
        SubnetMovingPrice::<Test>::insert(netuid1, I96F32::from_num(1));
        SubnetMovingPrice::<Test>::insert(netuid2, I96F32::from_num(2));
        // Keep root_proportion ~1 so the injection cap does not bind.
        set_full_injection_root_stake();
        // Run coinbase
        SubtensorModule::run_coinbase(emission_credit);
        // tao_in = 333_333
        // alpha_in = 333_333/price = 333_333 + initial
        assert_eq!(
            SubnetAlphaIn::<Test>::get(netuid1),
            (initial + emission / 3).into()
        );
        // tao_in = 666_666
        // alpha_in = 666_666/price = 333_333 + initial
        assert_eq!(
            SubnetAlphaIn::<Test>::get(netuid2),
            (initial + (emission * 2 / 3) / 2).into()
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::alpha_issuance::test_coinbase_alpha_issuance_with_cap_trigger --exact --show-output --nocapture
#[test]
fn test_coinbase_alpha_issuance_with_cap_trigger() {
    new_test_ext(1).execute_with(|| {
        let netuid1 = NetUid::from(1);
        let netuid2 = NetUid::from(2);
        let emission: u64 = 1_000_000;
        let emission_credit = SubtensorModule::mint_tao(emission.into());
        add_network(netuid1, 1, 0);
        add_network(netuid2, 1, 0);
        // Make subnets dynamic.
        SubnetMechanism::<Test>::insert(netuid1, 1);
        SubnetMechanism::<Test>::insert(netuid2, 1);
        // Setup prices 1000000
        let initial: u64 = 1_000;
        let initial_alpha: u64 = initial * 1000000;
        SubnetTAO::<Test>::insert(netuid1, TaoBalance::from(initial));
        SubnetAlphaIn::<Test>::insert(netuid1, AlphaBalance::from(initial_alpha)); // Make price extremely low.
        SubnetTAO::<Test>::insert(netuid2, TaoBalance::from(initial));
        SubnetAlphaIn::<Test>::insert(netuid2, AlphaBalance::from(initial_alpha)); // Make price extremely low.
        // Set subnet prices.
        SubnetMovingPrice::<Test>::insert(netuid1, I96F32::from_num(1));
        SubnetMovingPrice::<Test>::insert(netuid2, I96F32::from_num(2));
        // Keep root_proportion ~1 so the injection cap binds at alpha_emission.
        set_full_injection_root_stake();
        // Run coinbase
        SubtensorModule::run_coinbase(emission_credit);
        // alpha_in is capped at the injection cap, so injected alpha stays below
        // a full block emission on top of the initial reserve.
        assert!(SubnetAlphaIn::<Test>::get(netuid1) < (initial_alpha + 1_000_000_000).into());
        // Per-block alpha emission is the full block emission regardless of the cap.
        assert_eq!(
            SubnetAlphaOutEmission::<Test>::get(netuid1),
            1_000_000_000.into()
        );
        assert!(SubnetAlphaIn::<Test>::get(netuid2) < (initial_alpha + 1_000_000_000).into());
        assert_eq!(
            SubnetAlphaOutEmission::<Test>::get(netuid2),
            1_000_000_000.into()
        ); // Gets full block emission.
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::alpha_issuance::test_coinbase_alpha_issuance_with_cap_trigger_and_block_emission --exact --show-output --nocapture
#[test]
fn test_coinbase_alpha_issuance_with_cap_trigger_and_block_emission() {
    new_test_ext(1).execute_with(|| {
        let netuid1 = NetUid::from(1);
        let netuid2 = NetUid::from(2);
        let emission: u64 = 1_000_000;
        let emission_credit = SubtensorModule::mint_tao(emission.into());
        add_network(netuid1, 1, 0);
        add_network(netuid2, 1, 0);

        // Make subnets dynamic.
        SubnetMechanism::<Test>::insert(netuid1, 1);
        SubnetMechanism::<Test>::insert(netuid2, 1);

        // Setup prices 0.000001
        let initial_tao: u64 = 10_000_u64;
        let initial_alpha: u64 = initial_tao * 100_000_u64;
        mock::setup_reserves(netuid1, initial_tao.into(), initial_alpha.into());
        mock::setup_reserves(netuid2, initial_tao.into(), initial_alpha.into());

        // Enable emission
        FirstEmissionBlockNumber::<Test>::insert(netuid1, 0);
        FirstEmissionBlockNumber::<Test>::insert(netuid2, 0);
        // Price-based shares (1:2 ratio). Low pool prices mean alpha_in exceeds the
        // injection cap, so the surplus TAO is spent on chain buys.
        SubnetMovingPrice::<Test>::insert(netuid1, I96F32::from_num(1));
        SubnetMovingPrice::<Test>::insert(netuid2, I96F32::from_num(2));

        // Force the swap to initialize
        <Test as pallet::Config>::SwapInterface::init_swap(netuid1, None);
        <Test as pallet::Config>::SwapInterface::init_swap(netuid2, None);

        // Get the prices before the run_coinbase
        let price_1_before = <Test as pallet::Config>::SwapInterface::current_alpha_price(netuid1);
        let price_2_before = <Test as pallet::Config>::SwapInterface::current_alpha_price(netuid2);

        // Set issuance at 21M
        SubnetAlphaOut::<Test>::insert(netuid1, AlphaBalance::from(21_000_000_000_000_000_u64)); // Set issuance above 21M
        SubnetAlphaOut::<Test>::insert(netuid2, AlphaBalance::from(21_000_000_000_000_000_u64)); // Set issuance above 21M

        // Run coinbase
        SubtensorModule::run_coinbase(emission_credit);

        // New behavior: chain-bought alpha is cached instead of recycled.
        // The cached amount remains part of outstanding alpha supply.
        assert!(
            !SubnetProtocolAlpha::<Test>::get(netuid1).is_zero()
                || !SubnetProtocolAlpha::<Test>::get(netuid2).is_zero()
        );

        // Get the prices after the run_coinbase
        let price_1_after = <Test as pallet::Config>::SwapInterface::current_alpha_price(netuid1);
        let price_2_after = <Test as pallet::Config>::SwapInterface::current_alpha_price(netuid2);

        // AlphaIn gets decreased beacuse of a buy
        assert!(u64::from(SubnetAlphaIn::<Test>::get(netuid1)) < initial_alpha);
        assert_eq!(
            u64::from(SubnetAlphaOut::<Test>::get(netuid2)),
            21_000_000_000_000_000_u64
                .saturating_add(u64::from(SubnetProtocolAlpha::<Test>::get(netuid2)))
        );
        assert!(u64::from(SubnetAlphaIn::<Test>::get(netuid2)) < initial_alpha);
        assert_eq!(
            u64::from(SubnetAlphaOut::<Test>::get(netuid2)),
            21_000_000_000_000_000_u64
                .saturating_add(u64::from(SubnetProtocolAlpha::<Test>::get(netuid2)))
        );

        assert!(price_1_after > price_1_before);
        assert!(price_2_after > price_2_before);
    });
}
