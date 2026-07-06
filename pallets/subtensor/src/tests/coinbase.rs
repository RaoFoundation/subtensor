#![allow(
    unused,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used
)]
use super::mock::*;

use crate::tests::mock;
use crate::*;
use alloc::collections::BTreeMap;
use approx::assert_abs_diff_eq;
use frame_support::assert_ok;
use sp_core::U256;
use substrate_fixed::{
    transcendental::sqrt,
    types::{I64F64, I96F32, U64F64, U96F32},
};
use subtensor_runtime_common::{AlphaBalance, NetUidStorageIndex};
use subtensor_swap_interface::{SwapEngine, SwapHandler};

#[allow(clippy::arithmetic_side_effects)]
fn close(value: u64, target: u64, eps: u64) {
    assert!(
        (value as i64 - target as i64).abs() < eps as i64,
        "Assertion failed: value = {value}, target = {target}, eps = {eps}"
    )
}

/// Seed a large root stake with full TAO weight so that
/// `root_proportion = tao_weight / (tao_weight + alpha_issuance)` is ~1.
/// This keeps the alpha-injection cap (`root_proportion * alpha_emission`) from
/// spuriously binding for small per-subnet emissions, preserving the liquidity
/// injection behavior these tests were written for.
fn set_full_injection_root_stake() {
    SubnetTAO::<Test>::insert(
        NetUid::ROOT,
        TaoBalance::from(1_000_000_000_000_000_000_u64),
    );
    SubtensorModule::set_tao_weight(u64::MAX);
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_hotkey_take --exact --show-output --nocapture
#[test]
fn test_hotkey_take() {
    new_test_ext(1).execute_with(|| {
        let hotkey = U256::from(1);
        Delegates::<Test>::insert(hotkey, u16::MAX / 2);
        log::info!(
            "expected: {:?}",
            SubtensorModule::get_hotkey_take_float(&hotkey)
        );
        log::info!(
            "expected: {:?}",
            SubtensorModule::get_hotkey_take_float(&hotkey)
        );
    });
}

// Test the base case of running coinbase with zero emission.
// This test verifies that the coinbase mechanism can handle the edge case
// of zero emission without errors or unexpected behavior.
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_coinbase_basecase --exact --show-output --nocapture
#[test]
fn test_coinbase_basecase() {
    new_test_ext(1).execute_with(|| {
        let zero_emission = SubtensorModule::mint_tao(0.into());
        SubtensorModule::run_coinbase(zero_emission);
    });
}

// Test the emission distribution for a single subnet.
// This test verifies that:
// - Single subnet gets cutoff by lower flow limit, so nothing is distributed
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_coinbase_tao_issuance_base --exact --show-output --nocapture
#[test]
fn test_coinbase_tao_issuance_base() {
    new_test_ext(1).execute_with(|| {
        let emission = TaoBalance::from(1_234_567);
        let subnet_owner_ck = U256::from(1001);
        let subnet_owner_hk = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hk, &subnet_owner_ck);
        // Dynamic subnets register with emission disabled by default.
        SubnetEmissionEnabled::<Test>::insert(netuid, true);
        // Price-based emission shares require a non-zero moving price.
        SubnetMovingPrice::<Test>::insert(netuid, I96F32::from_num(1));
        // Keep root_proportion ~1 so the injection cap does not bind.
        set_full_injection_root_stake();
        let total_issuance_before = TotalIssuance::<Test>::get();
        let tao_in_before = SubnetTAO::<Test>::get(netuid);
        let total_stake_before = TotalStake::<Test>::get();
        let emission_credit = SubtensorModule::mint_tao(emission);
        SubtensorModule::run_coinbase(emission_credit);
        assert_eq!(SubnetTAO::<Test>::get(netuid), tao_in_before + emission);
        assert_eq!(
            TotalIssuance::<Test>::get(),
            total_issuance_before + emission
        );
        assert_eq!(TotalStake::<Test>::get(), total_stake_before + emission);
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_coinbase_tao_issuance_base_low --exact --show-output --nocapture
#[test]
fn test_coinbase_tao_issuance_base_low() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let emission = TaoBalance::from(1);
        let emission_credit = SubtensorModule::mint_tao(emission);
        add_network(netuid, 1, 0);
        assert_eq!(SubnetTAO::<Test>::get(netuid), TaoBalance::ZERO);
        // Set subnet flow to non-zero
        SubnetTaoFlow::<Test>::insert(netuid, 33433_i64);
        SubtensorModule::run_coinbase(emission_credit);
        assert_eq!(SubnetTAO::<Test>::get(netuid), emission);
        assert_eq!(TotalIssuance::<Test>::get(), emission);
        assert_eq!(TotalStake::<Test>::get(), emission);
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_coinbase_tao_issuance_base_low_flow --exact --show-output --nocapture
// #[test]
// fn test_coinbase_tao_issuance_base_low_flow() {
//     new_test_ext(1).execute_with(|| {
//         let emission = TaoBalance::from(1_234_567);
//         let subnet_owner_ck = U256::from(1001);
//         let subnet_owner_hk = U256::from(1002);
//         let netuid = add_dynamic_network(&subnet_owner_hk, &subnet_owner_ck);
//         let emission = TaoBalance::from(1);

//         // 100% tao flow method
//         let block_num = FlowHalfLife::<Test>::get();
//         SubnetEmaTaoFlow::<Test>::insert(netuid, (block_num, I64F64::from_num(1_000_000_000)));
//         System::set_block_number(block_num);

//         let tao_in_before = SubnetTAO::<Test>::get(netuid);
//         let total_stake_before = TotalStake::<Test>::get();
//         SubtensorModule::run_coinbase(U96F32::from_num(emission));
//         assert_eq!(SubnetTAO::<Test>::get(netuid), tao_in_before + emission);
//         assert_eq!(TotalIssuance::<Test>::get(), emission);
//         assert_eq!(TotalStake::<Test>::get(), total_stake_before + emission);
//     });
// }

// Test emission distribution across multiple subnets.
// This test verifies that:
// - Multiple subnets receive equal portions of the total emission
// - Each subnet's TAO balance is updated correctly
// - Total issuance and total stake reflect the full emission amount
// - The emission is split evenly between all subnets
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_coinbase_tao_issuance_multiple --exact --show-output --nocapture
#[test]
fn test_coinbase_tao_issuance_multiple() {
    new_test_ext(1).execute_with(|| {
        let netuid1 = NetUid::from(1);
        let netuid2 = NetUid::from(2);
        let netuid3 = NetUid::from(3);
        let emission = TaoBalance::from(3_333_333);
        let emission_credit = SubtensorModule::mint_tao(emission);
        add_network(netuid1, 1, 0);
        add_network(netuid2, 1, 0);
        add_network(netuid3, 1, 0);
        assert_eq!(SubnetTAO::<Test>::get(netuid1), TaoBalance::ZERO);
        assert_eq!(SubnetTAO::<Test>::get(netuid2), TaoBalance::ZERO);
        assert_eq!(SubnetTAO::<Test>::get(netuid3), TaoBalance::ZERO);
        // Set Tao flows to equal and non-zero
        SubnetTaoFlow::<Test>::insert(netuid1, 100_000_000_i64);
        SubnetTaoFlow::<Test>::insert(netuid2, 100_000_000_i64);
        SubnetTaoFlow::<Test>::insert(netuid3, 100_000_000_i64);
        SubtensorModule::run_coinbase(emission_credit);
        assert_abs_diff_eq!(
            SubnetTAO::<Test>::get(netuid1),
            emission / 3.into(),
            epsilon = 1.into(),
        );
        assert_abs_diff_eq!(
            SubnetTAO::<Test>::get(netuid2),
            emission / 3.into(),
            epsilon = 1.into(),
        );
        assert_abs_diff_eq!(
            SubnetTAO::<Test>::get(netuid3),
            emission / 3.into(),
            epsilon = 1.into(),
        );
        assert_abs_diff_eq!(TotalIssuance::<Test>::get(), emission, epsilon = 3.into(),);
        assert_abs_diff_eq!(TotalStake::<Test>::get(), emission, epsilon = 3.into(),);
    });
}

#[test]
fn test_coinbase_disabled_subnet_emission_redistributes_tao_to_enabled_subnets() {
    new_test_ext(1).execute_with(|| {
        let netuid1 = NetUid::from(1);
        let netuid2 = NetUid::from(2);
        let netuid3 = NetUid::from(3);
        let emission = TaoBalance::from(3_333_333);

        add_network(netuid1, 1, 0);
        add_network(netuid2, 1, 0);
        add_network(netuid3, 1, 0);

        SubnetEmissionEnabled::<Test>::insert(netuid2, false);

        SubnetTaoFlow::<Test>::insert(netuid1, 100_000_000_i64);
        SubnetTaoFlow::<Test>::insert(netuid2, 100_000_000_i64);
        SubnetTaoFlow::<Test>::insert(netuid3, 100_000_000_i64);

        let subnet_emissions = SubtensorModule::get_subnet_block_emissions(
            &[netuid1, netuid2, netuid3],
            U96F32::saturating_from_num(emission.to_u64()),
        );

        assert_abs_diff_eq!(
            subnet_emissions[&netuid1].to_num::<f64>(),
            (emission.to_u64() / 2) as f64,
            epsilon = 2.0,
        );
        assert_abs_diff_eq!(
            subnet_emissions[&netuid2].to_num::<f64>(),
            0.0,
            epsilon = 1.0
        );
        assert_abs_diff_eq!(
            subnet_emissions[&netuid3].to_num::<f64>(),
            (emission.to_u64() / 2) as f64,
            epsilon = 2.0,
        );

        let (_tao_in, alpha_in, alpha_out, excess_tao) =
            SubtensorModule::get_subnet_terms(&subnet_emissions);
        assert_eq!(alpha_in[&netuid2], U96F32::from_num(0.0));
        assert_eq!(excess_tao[&netuid2], U96F32::from_num(0.0));
        assert!(alpha_out[&netuid2] > U96F32::from_num(0.0));

        let total_issuance_before = TotalIssuance::<Test>::get();
        let total_stake_before = TotalStake::<Test>::get();
        let emission_credit = SubtensorModule::mint_tao(emission);
        SubtensorModule::run_coinbase(emission_credit);

        assert_abs_diff_eq!(
            SubnetTAO::<Test>::get(netuid1),
            emission / 2.into(),
            epsilon = 2.into(),
        );
        assert_eq!(SubnetTAO::<Test>::get(netuid2), TaoBalance::ZERO);
        assert_abs_diff_eq!(
            SubnetTAO::<Test>::get(netuid3),
            emission / 2.into(),
            epsilon = 2.into(),
        );
        assert_abs_diff_eq!(
            TotalIssuance::<Test>::get(),
            total_issuance_before + emission,
            epsilon = 2.into(),
        );
        assert_abs_diff_eq!(
            TotalStake::<Test>::get(),
            total_stake_before + emission,
            epsilon = 2.into(),
        );
    });
}

#[test]
fn test_sudo_set_subnet_emission_enabled_multiple_subnets_multiple_toggles() {
    new_test_ext(1).execute_with(|| {
        let netuid1 = NetUid::from(1);
        let netuid2 = NetUid::from(2);
        let netuid3 = NetUid::from(3);
        let emission = TaoBalance::from(3_000_000);

        add_network(netuid1, 1, 0);
        add_network(netuid2, 1, 0);
        add_network(netuid3, 1, 0);

        // Keep root_proportion ~1 so TAO-side emission is injected (populating
        // SubnetTaoInEmission) rather than routed entirely to chain buys.
        set_full_injection_root_stake();

        let assert_emission_storage = |expected1: u64, expected2: u64, expected3: u64| {
            assert_abs_diff_eq!(
                SubnetTaoInEmission::<Test>::get(netuid1),
                TaoBalance::from(expected1),
                epsilon = 2.into(),
            );
            assert_abs_diff_eq!(
                SubnetTaoInEmission::<Test>::get(netuid2),
                TaoBalance::from(expected2),
                epsilon = 2.into(),
            );
            assert_abs_diff_eq!(
                SubnetTaoInEmission::<Test>::get(netuid3),
                TaoBalance::from(expected3),
                epsilon = 2.into(),
            );

            assert_eq!(
                SubnetAlphaInEmission::<Test>::get(netuid1) == AlphaBalance::from(0),
                expected1 == 0
            );
            assert_eq!(
                SubnetAlphaInEmission::<Test>::get(netuid2) == AlphaBalance::from(0),
                expected2 == 0
            );
            assert_eq!(
                SubnetAlphaInEmission::<Test>::get(netuid3) == AlphaBalance::from(0),
                expected3 == 0
            );

            assert!(SubnetAlphaOutEmission::<Test>::get(netuid1) > AlphaBalance::from(0));
            assert!(SubnetAlphaOutEmission::<Test>::get(netuid2) > AlphaBalance::from(0));
            assert!(SubnetAlphaOutEmission::<Test>::get(netuid3) > AlphaBalance::from(0));
        };

        let run_coinbase = || {
            let emission_credit = SubtensorModule::mint_tao(emission);
            SubtensorModule::run_coinbase(emission_credit);
        };

        // All enabled: split TAO-side emission equally across all three subnets.
        run_coinbase();
        assert_emission_storage(1_000_000, 1_000_000, 1_000_000);

        // Seed stale values and then disable netuid2. The next coinbase run must clear
        // netuid2's per-block TAO-side emission storage while preserving alpha_out.
        SubnetTaoInEmission::<Test>::insert(netuid2, TaoBalance::from(123));
        SubnetAlphaInEmission::<Test>::insert(netuid2, AlphaBalance::from(123));
        SubnetExcessTao::<Test>::insert(netuid2, TaoBalance::from(123));
        SubnetEmissionEnabled::<Test>::insert(netuid2, false);
        run_coinbase();
        assert_emission_storage(1_500_000, 0, 1_500_000);
        assert_eq!(SubnetExcessTao::<Test>::get(netuid2), TaoBalance::from(0));

        // Toggle a different subnet off and netuid2 back on.
        SubnetTaoInEmission::<Test>::insert(netuid1, TaoBalance::from(456));
        SubnetAlphaInEmission::<Test>::insert(netuid1, AlphaBalance::from(456));
        SubnetExcessTao::<Test>::insert(netuid1, TaoBalance::from(456));
        SubnetEmissionEnabled::<Test>::insert(netuid1, false);
        SubnetEmissionEnabled::<Test>::insert(netuid2, true);
        run_coinbase();
        assert_emission_storage(0, 1_500_000, 1_500_000);
        assert_eq!(SubnetExcessTao::<Test>::get(netuid1), TaoBalance::from(0));

        // Toggle everything back on: TAO-side emission should return to an even split.
        SubnetEmissionEnabled::<Test>::insert(netuid1, true);
        SubnetEmissionEnabled::<Test>::insert(netuid2, true);
        SubnetEmissionEnabled::<Test>::insert(netuid3, true);
        run_coinbase();
        assert_emission_storage(1_000_000, 1_000_000, 1_000_000);
    });
}

// Test emission distribution with different subnet prices.
// This test verifies that:
// - Subnets with different prices receive proportional emission shares
// - A subnet with double the price receives double the emission
// - Total issuance and total stake reflect the full emission amount
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_coinbase_tao_issuance_different_prices --exact --show-output --nocapture
#[test]
fn test_coinbase_tao_issuance_different_prices() {
    new_test_ext(1).execute_with(|| {
        let netuid1 = NetUid::from(1);
        let netuid2 = NetUid::from(2);
        let emission = 100_000_000;
        let emission_credit = SubtensorModule::mint_tao(emission.into());
        add_network(netuid1, 1, 0);
        add_network(netuid2, 1, 0);

        // Setup prices 0.1 and 0.2
        let initial_tao: u64 = 100_000_u64;
        let initial_alpha1: u64 = initial_tao * 10;
        let initial_alpha2: u64 = initial_tao * 5;
        mock::setup_reserves(netuid1, initial_tao.into(), initial_alpha1.into());
        mock::setup_reserves(netuid2, initial_tao.into(), initial_alpha2.into());

        // Force the swap to initialize
        <Test as pallet::Config>::SwapInterface::init_swap(netuid1, None);
        <Test as pallet::Config>::SwapInterface::init_swap(netuid2, None);

        // Make subnets dynamic.
        SubnetMechanism::<Test>::insert(netuid1, 1);
        SubnetMechanism::<Test>::insert(netuid2, 1);

        // Price-based shares: subnet 2 has twice the moving price of subnet 1,
        // so it should receive twice the TAO emission.
        SubnetMovingPrice::<Test>::insert(netuid1, I96F32::from_num(0.1));
        SubnetMovingPrice::<Test>::insert(netuid2, I96F32::from_num(0.2));
        // Keep root_proportion ~1 so the injection cap does not bind.
        set_full_injection_root_stake();

        // Assert initial TAO reserves.
        assert_eq!(SubnetTAO::<Test>::get(netuid1), initial_tao.into());
        assert_eq!(SubnetTAO::<Test>::get(netuid2), initial_tao.into());

        // Run the coinbase with the emission amount.
        SubtensorModule::run_coinbase(emission_credit);

        // Assert tao emission is split evenly.
        assert_abs_diff_eq!(
            SubnetTAO::<Test>::get(netuid1),
            TaoBalance::from(initial_tao + emission / 3),
            epsilon = 10.into(),
        );
        assert_abs_diff_eq!(
            SubnetTAO::<Test>::get(netuid2),
            TaoBalance::from(initial_tao + 2 * emission / 3),
            epsilon = 10.into(),
        );

        // Prices are low => we limit tao issued (buy alpha with it)
        let tao_issued = TaoBalance::from(((1.0) * emission as f64) as u64);
        assert_abs_diff_eq!(
            TotalIssuance::<Test>::get(),
            tao_issued,
            epsilon = 10.into()
        );
        assert_abs_diff_eq!(
            TotalStake::<Test>::get(),
            emission.into(),
            epsilon = 10.into()
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_coinbase_tao_issuance_different_flows --exact --show-output --nocapture
// #[test]
// fn test_coinbase_tao_issuance_different_flows() {
//     new_test_ext(1).execute_with(|| {
//         let subnet_owner_ck = U256::from(1001);
//         let subnet_owner_hk = U256::from(1002);
//         let netuid1 = add_dynamic_network(&subnet_owner_hk, &subnet_owner_ck);
//         let netuid2 = add_dynamic_network(&subnet_owner_hk, &subnet_owner_ck);
//         let emission = 100_000_000;

//         // Setup prices 0.1 and 0.2
//         let initial_tao: u64 = 100_000_u64;
//         let initial_alpha1: u64 = initial_tao * 10;
//         let initial_alpha2: u64 = initial_tao * 5;
//         mock::setup_reserves(netuid1, initial_tao.into(), initial_alpha1.into());
//         mock::setup_reserves(netuid2, initial_tao.into(), initial_alpha2.into());

//         // Force the swap to initialize
//         <Test as pallet::Config>::SwapInterface::init_swap(netuid1);
//         <Test as pallet::Config>::SwapInterface::init_swap(netuid2);

//         // Set subnet prices to reversed proportion to ensure they don't affect emissions.
//         SubnetMovingPrice::<Test>::insert(netuid1, I96F32::from_num(2));
//         SubnetMovingPrice::<Test>::insert(netuid2, I96F32::from_num(1));

//         // Set subnet tao flow ema.
//         let block_num = FlowHalfLife::<Test>::get();
//         SubnetEmaTaoFlow::<Test>::insert(netuid1, (block_num, I64F64::from_num(1)));
//         SubnetEmaTaoFlow::<Test>::insert(netuid2, (block_num, I64F64::from_num(2)));
//         System::set_block_number(block_num);

//         // Set normalization exponent to 1 for simplicity
//         FlowNormExponent::<Test>::set(U64F64::from(1_u64));

//         // Assert initial TAO reserves.
//         assert_eq!(SubnetTAO::<Test>::get(netuid1), initial_tao.into());
//         assert_eq!(SubnetTAO::<Test>::get(netuid2), initial_tao.into());
//         let total_stake_before = TotalStake::<Test>::get();

//         // Run the coinbase with the emission amount.
//         SubtensorModule::run_coinbase(U96F32::from_num(emission));

//         // Assert tao emission is split evenly.
//         assert_abs_diff_eq!(
//             SubnetTAO::<Test>::get(netuid1),
//             TaoBalance::from(initial_tao + emission / 3),
//             epsilon = 10.into(),
//         );
//         assert_abs_diff_eq!(
//             SubnetTAO::<Test>::get(netuid2),
//             TaoBalance::from(initial_tao + 2 * emission / 3),
//             epsilon = 10.into(),
//         );

//         // Prices are low => we limit tao issued (buy alpha with it)
//         let tao_issued = TaoBalance::from(((0.1 + 0.2) * emission as f64) as u64);
//         assert_abs_diff_eq!(
//             TotalIssuance::<Test>::get(),
//             tao_issued,
//             epsilon = 10.into()
//         );
//         assert_abs_diff_eq!(
//             TotalStake::<Test>::get(),
//             total_stake_before + emission.into(),
//             epsilon = 10.into()
//         );
//     });
// }

// Test moving price updates with different alpha values.
// This test verifies that:
// - Moving price stays constant when alpha is 1.0
// - Moving price converges to real price at expected rate with alpha 0.1
// - Moving price updates correctly over multiple iterations
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_coinbase_moving_prices --exact --show-output --nocapture
#[test]
fn test_coinbase_moving_prices() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        // Set price to 1.0
        SubnetTAO::<Test>::insert(netuid, TaoBalance::from(1_000_000));
        SubnetAlphaIn::<Test>::insert(netuid, AlphaBalance::from(1_000_000));
        SubnetMechanism::<Test>::insert(netuid, 1);
        SubnetMovingPrice::<Test>::insert(netuid, I96F32::from_num(1));
        FirstEmissionBlockNumber::<Test>::insert(netuid, 1);

        // Updating the moving price keeps it the same.
        assert_eq!(
            SubtensorModule::get_moving_alpha_price(netuid),
            I96F32::from_num(1)
        );
        // Skip some blocks so that EMA price is not slowed down
        System::set_block_number(7_200_000);

        SubtensorModule::update_moving_price(netuid);
        assert_eq!(
            SubtensorModule::get_moving_alpha_price(netuid),
            I96F32::from_num(1)
        );
        // Check alpha of 1.
        // Set price to zero.
        SubnetMovingPrice::<Test>::insert(netuid, I96F32::from_num(0));
        SubnetMovingAlpha::<Test>::set(I96F32::from_num(1.0));
        // Run moving 1 times.
        SubtensorModule::update_moving_price(netuid);
        // Assert price is ~ 100% of the real price.
        assert!(U64F64::from_num(1.0) - SubtensorModule::get_moving_alpha_price(netuid) < 0.05);
        // Set price to zero.
        SubnetMovingPrice::<Test>::insert(netuid, I96F32::from_num(0));
        SubnetMovingAlpha::<Test>::set(I96F32::from_num(0.1));

        // EMA price 28 days after registration
        System::set_block_number(7_200 * 28);

        // Run moving 14 times.
        for _ in 0..14 {
            SubtensorModule::update_moving_price(netuid);
        }

        // Assert price is > 50% of the real price.
        assert_abs_diff_eq!(
            0.512325,
            SubtensorModule::get_moving_alpha_price(netuid).to_num::<f64>(),
            epsilon = 0.001
        );
    });
}

// Test moving price updates slow down at the beginning.
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_update_moving_price_initial --exact --show-output --nocapture
#[test]
fn test_update_moving_price_initial() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        // Set current price to 1.0
        SubnetTAO::<Test>::insert(netuid, TaoBalance::from(1_000_000));
        SubnetAlphaIn::<Test>::insert(netuid, AlphaBalance::from(1_000_000));
        SubnetMechanism::<Test>::insert(netuid, 1);
        SubnetMovingAlpha::<Test>::set(I96F32::from_num(0.5));
        SubnetMovingPrice::<Test>::insert(netuid, I96F32::from_num(0));

        // Registered recently
        System::set_block_number(510);
        FirstEmissionBlockNumber::<Test>::insert(netuid, 500);

        SubtensorModule::update_moving_price(netuid);

        let new_price = SubnetMovingPrice::<Test>::get(netuid);
        assert!(new_price.to_num::<f64>() < 0.001);
    });
}

// Test moving price updates slow down at the beginning.
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_update_moving_price_after_time --exact --show-output --nocapture
#[test]
fn test_update_moving_price_after_time() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        // Set current price to 1.0
        SubnetTAO::<Test>::insert(netuid, TaoBalance::from(1_000_000));
        SubnetAlphaIn::<Test>::insert(netuid, AlphaBalance::from(1_000_000));
        SubnetMechanism::<Test>::insert(netuid, 1);
        SubnetMovingAlpha::<Test>::set(I96F32::from_num(0.5));
        SubnetMovingPrice::<Test>::insert(netuid, I96F32::from_num(0));

        // Registered long time ago
        System::set_block_number(144_000_500);
        FirstEmissionBlockNumber::<Test>::insert(netuid, 500);

        SubtensorModule::update_moving_price(netuid);

        let new_price = SubnetMovingPrice::<Test>::get(netuid);
        assert!((new_price.to_num::<f64>() - 0.5).abs() < 0.001);
    });
}

// Test basic alpha issuance in coinbase mechanism.
// This test verifies that:
// - Alpha issuance is initialized to 0 for new subnets
// - Alpha issuance is split evenly between subnets during coinbase
// - Each subnet receives the expected fraction of total emission
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_coinbase_alpha_issuance_base --exact --show-output --nocapture
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_coinbase_alpha_issuance_different --exact --show-output --nocapture
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

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_coinbase_alpha_issuance_with_cap_trigger --exact --show-output --nocapture
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

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_coinbase_alpha_issuance_with_cap_trigger_and_block_emission --exact --show-output --nocapture
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

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_owner_cut_base --exact --show-output --nocapture
#[test]
fn test_owner_cut_base() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        mock::setup_reserves(
            netuid,
            1_000_000_000_000_u64.into(),
            1_000_000_000_000_u64.into(),
        );
        SubtensorModule::set_tempo_unchecked(netuid, 10000); // Large number (dont drain)
        SubtensorModule::set_subnet_owner_cut(0);
        SubtensorModule::run_coinbase(SubtensorModule::mint_tao(0.into()));
        assert_eq!(PendingOwnerCut::<Test>::get(netuid), 0.into()); // No cut
        SubtensorModule::set_subnet_owner_cut(u16::MAX);
        SubtensorModule::run_coinbase(SubtensorModule::mint_tao(0.into()));
        assert_eq!(PendingOwnerCut::<Test>::get(netuid), 1_000_000_000.into()); // Full cut.
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_pending_emission --exact --show-output --nocapture
#[test]
fn test_pending_emission() {
    new_test_ext(1).execute_with(|| {
        let hotkey = U256::from(1);
        let coldkey = U256::from(2);
        let netuid = add_dynamic_network(&hotkey, &coldkey);
        remove_owner_registration_stake(netuid);
        Tempo::<Test>::insert(netuid, 1);
        FirstEmissionBlockNumber::<Test>::insert(netuid, 0);

        mock::setup_reserves(netuid, 1_000_000.into(), 1.into());
        LastEpochBlock::<Test>::insert(netuid, 0);
        System::set_block_number(10);
        SubtensorModule::run_coinbase(SubtensorModule::mint_tao(0.into()));
        SubnetTAO::<Test>::insert(NetUid::ROOT, TaoBalance::from(1_000_000_000)); // Add root weight.
        System::set_block_number(12);
        SubtensorModule::run_coinbase(SubtensorModule::mint_tao(0.into()));
        SubtensorModule::set_tempo_unchecked(netuid, 10000); // Large number (dont drain)
        SubtensorModule::set_tao_weight(u64::MAX); // Set TAO weight to 1.0

        // Set moving price > 1.0
        SubnetMovingPrice::<Test>::insert(netuid, I96F32::from_num(2));

        // Make sure we are root selling, so we have root alpha divs.
        let root_sell_flag = SubtensorModule::get_network_root_sell_flag(&[netuid]);
        assert!(root_sell_flag, "Root sell flag should be true");

        SubtensorModule::run_coinbase(SubtensorModule::mint_tao(0.into()));
        // 1 TAO / ( 1 + 3 ) = 0.25 * 1 / 2 = 125000000

        assert_abs_diff_eq!(
            u64::from(PendingServerEmission::<Test>::get(netuid)),
            500_000_000,
            epsilon = 1
        ); // 1 / 2.

        assert_abs_diff_eq!(
            u64::from(PendingValidatorEmission::<Test>::get(netuid)),
            500_000_000 - 125000000,
            epsilon = 1
        ); // 1 / 2 - swapped.

        assert_abs_diff_eq!(
            u64::from(PendingRootAlphaDivs::<Test>::get(netuid)),
            125000000,
            epsilon = 1
        ); // 1 / 2 * 0.25 --> (from root_prop)
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_drain_base --exact --show-output --nocapture
#[test]
fn test_drain_base() {
    new_test_ext(1).execute_with(|| {
        SubtensorModule::distribute_emission(
            0.into(),
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
        )
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_drain_base_with_subnet --exact --show-output --nocapture
#[test]
fn test_drain_base_with_subnet() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
        )
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_drain_base_with_subnet_with_single_staker_not_registered --exact --show-output --nocapture
#[test]
fn test_drain_base_with_subnet_with_single_staker_not_registered() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        let hotkey = U256::from(1);
        let coldkey = U256::from(2);
        let stake_before = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            stake_before,
        );
        let pending_alpha = AlphaBalance::from(1_000_000_000);
        SubtensorModule::distribute_emission(
            netuid,
            pending_alpha.saturating_div(2.into()).into(),
            pending_alpha.saturating_div(2.into()).into(),
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
        );
        let stake_after =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);
        assert_eq!(stake_before, stake_after); // Not registered.
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_drain_base_with_subnet_with_single_staker_registered --exact --show-output --nocapture
#[test]
fn test_drain_base_with_subnet_with_single_staker_registered() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        let hotkey = U256::from(1);
        let coldkey = U256::from(2);
        let stake_before = AlphaBalance::from(1_000_000_000);
        register_ok_neuron(netuid, hotkey, coldkey, 0);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            stake_before,
        );
        let pending_alpha = AlphaBalance::from(1_000_000_000);
        SubtensorModule::distribute_emission(
            netuid,
            pending_alpha.saturating_div(2.into()).into(),
            pending_alpha.saturating_div(2.into()).into(),
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
        );
        let stake_after =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);
        close(
            (stake_before + pending_alpha).into(),
            stake_after.into(),
            10,
        ); // Registered gets all emission.
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_drain_base_with_subnet_with_single_staker_registered_root_weight --exact --show-output --nocapture
#[test]
fn test_drain_base_with_subnet_with_single_staker_registered_root_weight() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        let hotkey = U256::from(1);
        let coldkey = U256::from(2);
        let stake_before = AlphaBalance::from(1_000_000_000);
        // register_ok_neuron(root, hotkey, coldkey, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);
        Delegates::<Test>::insert(hotkey, 0);
        SubtensorModule::set_tao_weight(u64::MAX); // Set TAO weight to 1.0
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            NetUid::ROOT,
            stake_before,
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            stake_before,
        );
        let pending_alpha = AlphaBalance::from(1_000_000_000);
        let pending_root_alpha = AlphaBalance::from(1_000_000_000);
        assert_eq!(SubnetTAO::<Test>::get(NetUid::ROOT), TaoBalance::ZERO);
        SubtensorModule::distribute_emission(
            netuid,
            pending_alpha.saturating_div(2.into()).into(),
            pending_alpha.saturating_div(2.into()).into(),
            pending_root_alpha,
            AlphaBalance::ZERO,
        );
        let stake_after =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);
        let root_after = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            NetUid::ROOT,
        );
        close(
            (stake_before + pending_alpha).into(),
            stake_after.into(),
            10,
        ); // Registered gets all alpha emission.
        close(stake_before.to_u64(), root_after.into(), 10); // Registered doesn't get tao immediately
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_drain_base_with_subnet_with_two_stakers_registered --exact --show-output --nocapture
#[test]
fn test_drain_base_with_subnet_with_two_stakers_registered() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        let hotkey1 = U256::from(1);
        let hotkey2 = U256::from(2);
        let coldkey = U256::from(3);
        let stake_before = AlphaBalance::from(1_000_000_000);
        register_ok_neuron(netuid, hotkey1, coldkey, 0);
        register_ok_neuron(netuid, hotkey2, coldkey, 0);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey1,
            &coldkey,
            netuid,
            stake_before,
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey2,
            &coldkey,
            netuid,
            stake_before,
        );
        let pending_alpha = AlphaBalance::from(1_000_000_000);
        SubtensorModule::distribute_emission(
            netuid,
            pending_alpha.saturating_div(2.into()).into(),
            pending_alpha.saturating_div(2.into()).into(),
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
        );
        let stake_after1 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey1, &coldkey, netuid);
        let stake_after2 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey2, &coldkey, netuid);
        close(
            (stake_before + pending_alpha / 2.into()).into(),
            stake_after1.into(),
            10,
        ); // Registered gets 1/2 emission
        close(
            (stake_before + pending_alpha / 2.into()).into(),
            stake_after2.into(),
            10,
        ); // Registered gets 1/2 emission.
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_drain_base_with_subnet_with_two_stakers_registered_and_root --exact --show-output --nocapture
#[test]
fn test_drain_base_with_subnet_with_two_stakers_registered_and_root() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        let hotkey1 = U256::from(1);
        let hotkey2 = U256::from(2);
        let coldkey = U256::from(3);
        let stake_before = AlphaBalance::from(1_000_000_000);
        register_ok_neuron(netuid, hotkey1, coldkey, 0);
        register_ok_neuron(netuid, hotkey2, coldkey, 0);
        Delegates::<Test>::insert(hotkey1, 0);
        Delegates::<Test>::insert(hotkey2, 0);
        SubtensorModule::set_tao_weight(u64::MAX); // Set TAO weight to 1.0
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey1,
            &coldkey,
            netuid,
            stake_before,
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey1,
            &coldkey,
            NetUid::ROOT,
            stake_before,
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey2,
            &coldkey,
            netuid,
            stake_before,
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey2,
            &coldkey,
            NetUid::ROOT,
            stake_before,
        );
        let pending_tao = TaoBalance::from(1_000_000_000);
        let pending_alpha = AlphaBalance::from(1_000_000_000);
        assert_eq!(SubnetTAO::<Test>::get(NetUid::ROOT), TaoBalance::ZERO);
        SubtensorModule::distribute_emission(
            netuid,
            pending_alpha.saturating_div(2.into()).into(),
            pending_alpha.saturating_div(2.into()).into(),
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
        );
        let stake_after1 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey1, &coldkey, netuid);
        let root_after1 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey1,
            &coldkey,
            NetUid::ROOT,
        );
        let stake_after2 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey2, &coldkey, netuid);
        let root_after2 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey2,
            &coldkey,
            NetUid::ROOT,
        );
        close(
            (stake_before + pending_alpha / 2.into()).into(),
            stake_after1.into(),
            10,
        ); // Registered gets 1/2 emission
        close(
            (stake_before + pending_alpha / 2.into()).into(),
            stake_after2.into(),
            10,
        ); // Registered gets 1/2 emission.
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_drain_base_with_subnet_with_two_stakers_registered_and_root_different_amounts --exact --show-output --nocapture
#[test]
fn test_drain_base_with_subnet_with_two_stakers_registered_and_root_different_amounts() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        let hotkey1 = U256::from(1);
        let hotkey2 = U256::from(2);
        let coldkey = U256::from(3);
        let stake_before = AlphaBalance::from(1_000_000_000);
        Delegates::<Test>::insert(hotkey1, 0);
        Delegates::<Test>::insert(hotkey2, 0);
        register_ok_neuron(netuid, hotkey1, coldkey, 0);
        register_ok_neuron(netuid, hotkey2, coldkey, 0);
        SubtensorModule::set_tao_weight(u64::MAX); // Set TAO weight to 1.0
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey1,
            &coldkey,
            netuid,
            stake_before,
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey1,
            &coldkey,
            NetUid::ROOT,
            stake_before * 2.into(), // Hotkey 1 has twice as much root weight.
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey2,
            &coldkey,
            netuid,
            stake_before,
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey2,
            &coldkey,
            NetUid::ROOT,
            stake_before,
        );
        let pending_tao = TaoBalance::from(1_000_000_000);
        let pending_alpha = AlphaBalance::from(1_000_000_000);
        assert_eq!(SubnetTAO::<Test>::get(NetUid::ROOT), TaoBalance::ZERO);
        SubtensorModule::distribute_emission(
            netuid,
            pending_alpha.saturating_div(2.into()).into(),
            pending_alpha.saturating_div(2.into()).into(),
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
        );
        let stake_after1 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey1, &coldkey, netuid);
        let root_after1 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey1,
            &coldkey,
            NetUid::ROOT,
        );
        let stake_after2 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey2, &coldkey, netuid);
        let root_after2 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey2,
            &coldkey,
            NetUid::ROOT,
        );
        let expected_stake = I96F32::from_num(stake_before)
            + (I96F32::from_num(pending_alpha) * I96F32::from_num(1.0 / 2.0));
        assert_abs_diff_eq!(
            expected_stake.to_num::<u64>(),
            stake_after1.into(),
            epsilon = 10
        ); // Registered gets 50% of alpha emission
        let expected_stake2 = I96F32::from_num(stake_before)
            + I96F32::from_num(pending_alpha) * I96F32::from_num(1.0 / 2.0);
        assert_abs_diff_eq!(
            expected_stake2.to_num::<u64>(),
            stake_after2.into(),
            epsilon = 10
        ); // Registered gets 50% emission
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_drain_base_with_subnet_with_two_stakers_registered_and_root_different_amounts_half_tao_weight --exact --show-output --nocapture
#[test]
fn test_drain_base_with_subnet_with_two_stakers_registered_and_root_different_amounts_half_tao_weight()
 {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        let hotkey1 = U256::from(1);
        let hotkey2 = U256::from(2);
        let coldkey = U256::from(3);
        let stake_before = AlphaBalance::from(1_000_000_000);
        Delegates::<Test>::insert(hotkey1, 0);
        Delegates::<Test>::insert(hotkey2, 0);
        register_ok_neuron(netuid, hotkey1, coldkey, 0);
        register_ok_neuron(netuid, hotkey2, coldkey, 0);
        SubtensorModule::set_tao_weight(u64::MAX / 2); // Set TAO weight to 0.5
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey1,
            &coldkey,
            netuid,
            stake_before,
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey1,
            &coldkey,
            NetUid::ROOT,
            stake_before * 2.into(), // Hotkey 1 has twice as much root weight.
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey2,
            &coldkey,
            netuid,
            stake_before,
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey2,
            &coldkey,
            NetUid::ROOT,
            stake_before,
        );
        let pending_tao = TaoBalance::from(1_000_000_000);
        let pending_alpha = AlphaBalance::from(1_000_000_000);
        assert_eq!(SubnetTAO::<Test>::get(NetUid::ROOT), TaoBalance::ZERO);
        SubtensorModule::distribute_emission(
            netuid,
            pending_alpha.saturating_div(2.into()).into(),
            pending_alpha.saturating_div(2.into()).into(),
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
        );
        let stake_after1 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey1, &coldkey, netuid);
        let root_after1 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey1,
            &coldkey,
            NetUid::ROOT,
        );
        let stake_after2 =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey2, &coldkey, netuid);
        let root_after2 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey2,
            &coldkey,
            NetUid::ROOT,
        );
        let expected_stake = I96F32::from_num(stake_before)
            + I96F32::from_num(pending_alpha) * I96F32::from_num(1.0 / 2.0);
        assert_abs_diff_eq!(
            expected_stake.to_num::<u64>(),
            u64::from(stake_after1),
            epsilon = 10
        );
        let expected_stake2 = I96F32::from_num(stake_before)
            + I96F32::from_num(pending_alpha) * I96F32::from_num(1.0 / 2.0);
        assert_abs_diff_eq!(
            expected_stake2.to_num::<u64>(),
            u64::from(stake_after2),
            epsilon = 10
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_drain_alpha_childkey_parentkey --exact --show-output --nocapture
#[test]
fn test_drain_alpha_childkey_parentkey() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        SubtensorModule::set_ck_burn(0);
        let parent = U256::from(1);
        let child = U256::from(2);
        let coldkey = U256::from(3);
        let stake_before = AlphaBalance::from(1_000_000_000);
        register_ok_neuron(netuid, child, coldkey, 0);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &parent,
            &coldkey,
            netuid,
            stake_before,
        );
        mock_set_children_no_epochs(netuid, &parent, &[(u64::MAX, child)]);

        // Childkey take is 10%
        ChildkeyTake::<Test>::insert(child, netuid, u16::MAX / 10);

        let pending_alpha = AlphaBalance::from(1_000_000_000);
        SubtensorModule::distribute_emission(
            netuid,
            pending_alpha.saturating_div(2.into()).into(),
            pending_alpha.saturating_div(2.into()).into(),
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
        );
        let parent_stake_after = SubtensorModule::get_stake_for_hotkey_on_subnet(&parent, netuid);
        let child_stake_after = SubtensorModule::get_stake_for_hotkey_on_subnet(&child, netuid);

        // Child gets 10%, parent gets 90%
        let expected = I96F32::from_num(stake_before)
            + I96F32::from_num(pending_alpha) * I96F32::from_num(9.0 / 10.0);
        log::info!(
            "expected: {:?}, parent_stake_after: {:?}",
            expected.to_num::<u64>(),
            parent_stake_after
        );
        close(expected.to_num::<u64>(), parent_stake_after.into(), 10_000);
        let expected = I96F32::from_num(u64::from(pending_alpha)) / I96F32::from_num(10);
        close(expected.to_num::<u64>(), child_stake_after.into(), 10_000);
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_get_root_children --exact --show-output --nocapture
#[test]
fn test_get_root_children() {
    new_test_ext(1).execute_with(|| {
        // Init netuid 1
        let alpha = NetUid::from(1);
        add_network(NetUid::ROOT, 1, 0);
        add_network(alpha, 1, 0);

        // Set TAO weight to 1.
        SubtensorModule::set_tao_weight(u64::MAX); // Set TAO weight to 1.

        // Create keys.
        let cold = U256::from(0);
        let alice = U256::from(1);
        let bob = U256::from(2);

        // Register Alice and Bob to the root network and alpha subnet.
        register_ok_neuron(alpha, alice, cold, 0);
        register_ok_neuron(alpha, bob, cold, 0);
        assert_ok!(SubtensorModule::root_register(
            RuntimeOrigin::signed(cold).clone(),
            alice,
        ));
        assert_ok!(SubtensorModule::root_register(
            RuntimeOrigin::signed(cold).clone(),
            bob,
        ));

        // Add stake for Alice and Bob on root.
        let alice_root_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &alice,
            &cold,
            NetUid::ROOT,
            alice_root_stake,
        );
        let bob_root_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &bob,
            &cold,
            NetUid::ROOT,
            alice_root_stake,
        );

        // Add stake for Alice and Bob on netuid.
        let alice_alpha_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &alice,
            &cold,
            alpha,
            alice_alpha_stake,
        );
        let bob_alpha_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &bob,
            &cold,
            alpha,
            bob_alpha_stake,
        );

        // Set Bob as 100% child of Alice on root.
        // mock_set_children_no_epochs( NetUid::ROOT, &alice, &[(u64::MAX, bob)]);
        mock_set_children_no_epochs(alpha, &alice, &[(u64::MAX, bob)]);

        // Assert Alice and Bob stake on root and netuid
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_on_subnet(&alice, NetUid::ROOT),
            alice_root_stake
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_on_subnet(&bob, NetUid::ROOT),
            bob_root_stake
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_on_subnet(&alice, alpha),
            alice_alpha_stake
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_on_subnet(&bob, alpha),
            bob_alpha_stake
        );

        // Assert Alice and Bob inherited stakes
        assert_eq!(
            SubtensorModule::get_inherited_for_hotkey_on_subnet(&alice, NetUid::ROOT),
            alice_root_stake
        );
        assert_eq!(
            SubtensorModule::get_inherited_for_hotkey_on_subnet(&alice, alpha),
            0.into()
        );
        assert_eq!(
            SubtensorModule::get_inherited_for_hotkey_on_subnet(&bob, NetUid::ROOT),
            bob_root_stake
        );
        assert_eq!(
            SubtensorModule::get_inherited_for_hotkey_on_subnet(&bob, alpha),
            bob_alpha_stake + alice_alpha_stake
        );

        // Assert Alice and Bob TAO inherited stakes
        assert_eq!(
            SubtensorModule::get_tao_inherited_for_hotkey_on_subnet(&alice, alpha),
            TaoBalance::ZERO
        );
        assert_eq!(
            SubtensorModule::get_tao_inherited_for_hotkey_on_subnet(&bob, alpha),
            u64::from(bob_root_stake + alice_root_stake).into()
        );

        // Get Alice stake amounts on subnet alpha.
        let (alice_total, alice_alpha, alice_tao): (I64F64, I64F64, I64F64) =
            SubtensorModule::get_stake_weights_for_hotkey_on_subnet(&alice, alpha);
        assert_eq!(alice_total, I64F64::from_num(0));

        // Get Bob stake amounts on subnet alpha.
        let (bob_total, bob_alpha, bob_tao): (I64F64, I64F64, I64F64) =
            SubtensorModule::get_stake_weights_for_hotkey_on_subnet(&bob, alpha);
        assert_eq!(
            bob_total,
            I64F64::from_num(u64::from(bob_root_stake * 4.into()))
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_get_root_children_drain --exact --show-output --nocapture
#[test]
fn test_get_root_children_drain() {
    new_test_ext(1).execute_with(|| {
        // Init netuid 1
        let alpha = NetUid::from(1);
        add_network(NetUid::ROOT, 1, 0);
        add_network(alpha, 1, 0);
        SubtensorModule::set_ck_burn(0);
        // Set TAO weight to 1.
        SubtensorModule::set_tao_weight(u64::MAX); // Set TAO weight to 1.
        // Create keys.
        let cold_alice = U256::from(0);
        let cold_bob = U256::from(1);
        let alice = U256::from(2);
        let bob = U256::from(3);
        // Register Alice and Bob to the root network and alpha subnet.
        register_ok_neuron(alpha, alice, cold_alice, 0);
        register_ok_neuron(alpha, bob, cold_bob, 0);
        assert_ok!(SubtensorModule::root_register(
            RuntimeOrigin::signed(cold_alice).clone(),
            alice,
        ));
        assert_ok!(SubtensorModule::root_register(
            RuntimeOrigin::signed(cold_bob).clone(),
            bob,
        ));
        // Add stake for Alice and Bob on root.
        let alice_root_stake = 1_000_000_000;
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &alice,
            &cold_alice,
            NetUid::ROOT,
            alice_root_stake.into(),
        );
        let bob_root_stake = 1_000_000_000;
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &bob,
            &cold_bob,
            NetUid::ROOT,
            bob_root_stake.into(),
        );
        // Add stake for Alice and Bob on netuid.
        let alice_alpha_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &alice,
            &cold_alice,
            alpha,
            alice_alpha_stake,
        );
        let bob_alpha_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &bob,
            &cold_bob,
            alpha,
            bob_alpha_stake,
        );
        // Set Bob as 100% child of Alice on root.
        mock_set_children_no_epochs(alpha, &alice, &[(u64::MAX, bob)]);
        // Set Bob childkey take to zero.
        ChildkeyTake::<Test>::insert(bob, alpha, 0);
        Delegates::<Test>::insert(alice, 0);
        Delegates::<Test>::insert(bob, 0);

        // Get Alice stake amounts on subnet alpha.
        let (alice_total, alice_alpha, alice_tao): (I64F64, I64F64, I64F64) =
            SubtensorModule::get_stake_weights_for_hotkey_on_subnet(&alice, alpha);
        assert_eq!(alice_total, I64F64::from_num(0));

        // Get Bob stake amounts on subnet alpha.
        let (bob_total, bob_alpha, bob_tao): (I64F64, I64F64, I64F64) =
            SubtensorModule::get_stake_weights_for_hotkey_on_subnet(&bob, alpha);
        assert_eq!(bob_total, I64F64::from_num(4_u64 * bob_root_stake));

        // Lets drain
        let pending_alpha = AlphaBalance::from(1_000_000_000);
        SubtensorModule::distribute_emission(
            alpha,
            pending_alpha.saturating_div(2.into()).into(),
            pending_alpha.saturating_div(2.into()).into(),
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
        );

        // Alice and Bob both made half of the dividends.
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_on_subnet(&alice, alpha),
            alice_alpha_stake + pending_alpha / 2.into()
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_on_subnet(&bob, alpha),
            bob_alpha_stake + pending_alpha / 2.into()
        );

        // There should be no TAO on the root subnet.
        assert_eq!(SubnetTAO::<Test>::get(NetUid::ROOT), TaoBalance::ZERO);

        // Lets drain
        let pending_alpha = AlphaBalance::from(1_000_000_000);
        let pending_root1 = TaoBalance::from(1_000_000_000);
        SubtensorModule::distribute_emission(
            alpha,
            pending_alpha.saturating_div(2.into()).into(),
            pending_alpha.saturating_div(2.into()).into(),
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
        );

        // Alice and Bob both made half of the dividends.
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_on_subnet(&alice, NetUid::ROOT),
            AlphaBalance::from(alice_root_stake)
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_on_subnet(&bob, NetUid::ROOT),
            AlphaBalance::from(bob_root_stake)
        );

        // Lets change the take value. (Bob is greedy.)
        ChildkeyTake::<Test>::insert(bob, alpha, u16::MAX);

        // Lets drain
        let pending_alpha = AlphaBalance::from(1_000_000_000);
        let pending_root2 = TaoBalance::from(1_000_000_000);
        SubtensorModule::distribute_emission(
            alpha,
            pending_alpha.saturating_div(2.into()).into(),
            pending_alpha.saturating_div(2.into()).into(),
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
        );

        // Alice makes nothing
        assert_eq!(
            AlphaDividendsPerSubnet::<Test>::get(alpha, alice),
            AlphaBalance::ZERO
        );
        // Bob makes it all.
        assert_abs_diff_eq!(
            AlphaDividendsPerSubnet::<Test>::get(alpha, bob),
            pending_alpha,
            epsilon = 1.into()
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_get_root_children_drain_half_proportion --exact --show-output --nocapture
#[test]
fn test_get_root_children_drain_half_proportion() {
    new_test_ext(1).execute_with(|| {
        // Init netuid 1
        let alpha = NetUid::from(1);
        add_network(NetUid::ROOT, 1, 0);
        add_network(alpha, 1, 0);
        SubtensorModule::set_ck_burn(0);
        // Set TAO weight to 1.
        SubtensorModule::set_tao_weight(u64::MAX); // Set TAO weight to 1.
        // Create keys.
        let cold_alice = U256::from(0);
        let cold_bob = U256::from(1);
        let alice = U256::from(2);
        let bob = U256::from(3);
        // Register Alice and Bob to the root network and alpha subnet.
        register_ok_neuron(alpha, alice, cold_alice, 0);
        register_ok_neuron(alpha, bob, cold_bob, 0);
        assert_ok!(SubtensorModule::root_register(
            RuntimeOrigin::signed(cold_alice).clone(),
            alice,
        ));
        assert_ok!(SubtensorModule::root_register(
            RuntimeOrigin::signed(cold_bob).clone(),
            bob,
        ));
        // Add stake for Alice and Bob on root.
        let alice_root_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &alice,
            &cold_alice,
            NetUid::ROOT,
            alice_root_stake,
        );
        let bob_root_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &bob,
            &cold_bob,
            NetUid::ROOT,
            alice_root_stake,
        );
        // Add stake for Alice and Bob on netuid.
        let alice_alpha_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &alice,
            &cold_alice,
            alpha,
            alice_alpha_stake,
        );
        let bob_alpha_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &bob,
            &cold_bob,
            alpha,
            bob_alpha_stake,
        );
        // Set Bob as 100% child of Alice on root.
        mock_set_children_no_epochs(alpha, &alice, &[(u64::MAX / 2, bob)]);

        // Set Bob childkey take to zero.
        ChildkeyTake::<Test>::insert(bob, alpha, 0);
        Delegates::<Test>::insert(alice, 0);
        Delegates::<Test>::insert(bob, 0);

        // Lets drain!
        let pending_alpha = AlphaBalance::from(1_000_000_000);
        SubtensorModule::distribute_emission(
            alpha,
            pending_alpha.saturating_div(2.into()).into(),
            pending_alpha.saturating_div(2.into()).into(),
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
        );

        // Alice and Bob make the same amount.
        close(
            AlphaDividendsPerSubnet::<Test>::get(alpha, alice).into(),
            (pending_alpha / 2.into()).into(),
            10,
        );
        close(
            AlphaDividendsPerSubnet::<Test>::get(alpha, bob).into(),
            (pending_alpha / 2.into()).into(),
            10,
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_get_root_children_drain_with_take --exact --show-output --nocapture
#[test]
fn test_get_root_children_drain_with_take() {
    new_test_ext(1).execute_with(|| {
        // Init netuid 1
        let alpha = NetUid::from(1);
        add_network(NetUid::ROOT, 1, 0);
        add_network(alpha, 1, 0);
        // Set TAO weight to 1.
        SubtensorModule::set_tao_weight(u64::MAX); // Set TAO weight to 1.
        // Create keys.
        let cold_alice = U256::from(0);
        let cold_bob = U256::from(1);
        let alice = U256::from(2);
        let bob = U256::from(3);
        // Register Alice and Bob to the root network and alpha subnet.
        register_ok_neuron(alpha, alice, cold_alice, 0);
        register_ok_neuron(alpha, bob, cold_bob, 0);
        assert_ok!(SubtensorModule::root_register(
            RuntimeOrigin::signed(cold_alice).clone(),
            alice,
        ));
        assert_ok!(SubtensorModule::root_register(
            RuntimeOrigin::signed(cold_bob).clone(),
            bob,
        ));
        // Add stake for Alice and Bob on root.
        let alice_root_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &alice,
            &cold_alice,
            NetUid::ROOT,
            alice_root_stake,
        );
        let bob_root_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &bob,
            &cold_bob,
            NetUid::ROOT,
            alice_root_stake,
        );
        // Add stake for Alice and Bob on netuid.
        let alice_alpha_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &alice,
            &cold_alice,
            alpha,
            alice_alpha_stake,
        );
        let bob_alpha_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &bob,
            &cold_bob,
            alpha,
            bob_alpha_stake,
        );
        // Set Bob as 100% child of Alice on root.
        ChildkeyTake::<Test>::insert(bob, alpha, u16::MAX);
        mock_set_children_no_epochs(alpha, &alice, &[(u64::MAX, bob)]);
        // Set Bob validator take to zero.
        Delegates::<Test>::insert(alice, 0);
        Delegates::<Test>::insert(bob, 0);

        // Lets drain!
        let pending_alpha = AlphaBalance::from(1_000_000_000);
        SubtensorModule::distribute_emission(
            alpha,
            pending_alpha.saturating_div(2.into()).into(),
            pending_alpha.saturating_div(2.into()).into(),
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
        );

        // Bob makes it all.
        close(
            AlphaDividendsPerSubnet::<Test>::get(alpha, alice).into(),
            0,
            10,
        );
        close(
            AlphaDividendsPerSubnet::<Test>::get(alpha, bob).into(),
            pending_alpha.into(),
            10,
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_get_root_children_drain_with_half_take --exact --show-output --nocapture
#[test]
fn test_get_root_children_drain_with_half_take() {
    new_test_ext(1).execute_with(|| {
        // Init netuid 1
        let alpha = NetUid::from(1);
        add_network(NetUid::ROOT, 1, 0);
        add_network(alpha, 1, 0);
        // Set TAO weight to 1.
        SubtensorModule::set_tao_weight(u64::MAX); // Set TAO weight to 1.
        SubtensorModule::set_ck_burn(0);
        // Create keys.
        let cold_alice = U256::from(0);
        let cold_bob = U256::from(1);
        let alice = U256::from(2);
        let bob = U256::from(3);
        // Register Alice and Bob to the root network and alpha subnet.
        register_ok_neuron(alpha, alice, cold_alice, 0);
        register_ok_neuron(alpha, bob, cold_bob, 0);
        assert_ok!(SubtensorModule::root_register(
            RuntimeOrigin::signed(cold_alice).clone(),
            alice,
        ));
        assert_ok!(SubtensorModule::root_register(
            RuntimeOrigin::signed(cold_bob).clone(),
            bob,
        ));
        // Add stake for Alice and Bob on root.
        let alice_root_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &alice,
            &cold_alice,
            NetUid::ROOT,
            alice_root_stake,
        );
        let bob_root_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &bob,
            &cold_bob,
            NetUid::ROOT,
            alice_root_stake,
        );
        // Add stake for Alice and Bob on netuid.
        let alice_alpha_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &alice,
            &cold_alice,
            alpha,
            alice_alpha_stake,
        );
        let bob_alpha_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &bob,
            &cold_bob,
            alpha,
            bob_alpha_stake,
        );
        // Set Bob as 100% child of Alice on root.
        ChildkeyTake::<Test>::insert(bob, alpha, u16::MAX / 2);
        mock_set_children_no_epochs(alpha, &alice, &[(u64::MAX, bob)]);
        // Set Bob childkey take to zero.
        Delegates::<Test>::insert(alice, 0);
        Delegates::<Test>::insert(bob, 0);

        // Lets drain!
        let pending_alpha = AlphaBalance::from(1_000_000_000);
        SubtensorModule::distribute_emission(
            alpha,
            pending_alpha.saturating_div(2.into()).into(),
            pending_alpha.saturating_div(2.into()).into(),
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
        );

        // Alice and Bob make the same amount.
        close(
            AlphaDividendsPerSubnet::<Test>::get(alpha, alice).into(),
            (pending_alpha / 4.into()).into(),
            10000,
        );
        close(
            AlphaDividendsPerSubnet::<Test>::get(alpha, bob).into(),
            3 * u64::from(pending_alpha / 4.into()),
            10000,
        );
    });
}

// // SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_get_root_children_with_weights --exact --show-output --nocapture
// #[test]
// fn test_get_root_children_with_weights() {
//     new_test_ext(1).execute_with(|| {
//         // Init netuid 1
//         let alpha = NetUid::from(1);
//         add_network(NetUid::ROOT, 1, 0);
//         add_network(alpha, 1, 0);
//         // Set TAO weight to 1.
//         SubtensorModule::set_tao_weight(u64::MAX); // Set TAO weight to 1.
//                                                    // Create keys.
//         let cold = U256::from(0);
//         let alice = U256::from(1);
//         let bob = U256::from(2);
//         // Register Alice and Bob to the root network and alpha subnet.
//         register_ok_neuron(alpha, alice, cold, 0);
//         register_ok_neuron(alpha, bob, cold, 0);
//         assert_ok!(SubtensorModule::root_register(
//             RuntimeOrigin::signed(cold).clone(),
//             alice,
//         ));
//         assert_ok!(SubtensorModule::root_register(
//             RuntimeOrigin::signed(cold).clone(),
//             bob,
//         ));
//         // Add stake for Alice and Bob on root.
//         let alice_root_stake = AlphaBalance::from(1_000_000_000);
//         SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
//             &alice,
//             &cold,
//             NetUid::ROOT,
//             alice_root_stake,
//         );
//         let bob_root_stake = AlphaBalance::from(1_000_000_000);
//         SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
//             &bob,
//             &cold,
//             NetUid::ROOT,
//             alice_root_stake,
//         );
//         // Add stake for Alice and Bob on netuid.
//         let alice_alpha_stake = AlphaBalance::from(1_000_000_000);
//         SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
//             &alice,
//             &cold,
//             alpha,
//             alice_alpha_stake,
//         );
//         let bob_alpha_stake = AlphaBalance::from(1_000_000_000);
//         SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
//             &bob,
//             &cold,
//             alpha,
//             bob_alpha_stake,
//         );
//         // Set Bob as 100% child of Alice on root.
//         mock_set_children_no_epochs(alpha, &alice, &[(u64::MAX, bob)]);

//         // Set Bob childkey take to zero.
//         ChildkeyTake::<Test>::insert(bob, alpha, 0);
//         Delegates::<Test>::insert(alice, 0);
//         Delegates::<Test>::insert(bob, 0);

//         // Set weights on the subnet.
//         assert_ok!(SubtensorModule::set_weights(
//             RuntimeOrigin::signed(alice),
//             alpha,
//             vec![0, 1],
//             vec![1, 1],
//             0,
//         ));
//         assert_ok!(SubtensorModule::set_weights(
//             RuntimeOrigin::signed(bob),
//             alpha,
//             vec![0, 1],
//             vec![1, 1],
//             0,
//         ));

//         // Lets drain!
//         let pending_alpha = AlphaBalance::from(1_000_000_000);
//         SubtensorModule::distribute_emission(alpha, pending_alpha, 0, 0.into(), 0.into());

//         // Alice and Bob make the same amount.
//         close(
//             AlphaDividendsPerSubnet::<Test>::get(alpha, alice),
//             pending_alpha / 2,
//             10,
//         );
//         close(
//             AlphaDividendsPerSubnet::<Test>::get(alpha, bob),
//             pending_alpha / 2,
//             10,
//         );
//     });
// }

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_incentive_to_subnet_owner_is_burned --exact --show-output --nocapture
#[test]
fn test_incentive_to_subnet_owner_is_burned() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_ck = U256::from(0);
        let subnet_owner_hk = U256::from(1);

        let other_ck = U256::from(2);
        let other_hk = U256::from(3);
        Owner::<Test>::insert(other_hk, other_ck);

        let netuid = add_dynamic_network(&subnet_owner_hk, &subnet_owner_ck);
        remove_owner_registration_stake(netuid);

        let pending_tao: u64 = 1_000_000_000;
        let pending_alpha = AlphaBalance::ZERO; // None to valis
        let owner_cut = AlphaBalance::ZERO;
        let mut incentives: BTreeMap<U256, AlphaBalance> = BTreeMap::new();

        // Give incentive to other_hk
        incentives.insert(other_hk, 10_000_000.into());

        // Give incentives to subnet_owner_hk
        incentives.insert(subnet_owner_hk, 10_000_000.into());

        // Verify stake before
        let subnet_owner_stake_before =
            SubtensorModule::get_stake_for_hotkey_on_subnet(&subnet_owner_hk, netuid);
        assert_eq!(subnet_owner_stake_before, 0.into());
        let other_stake_before = SubtensorModule::get_stake_for_hotkey_on_subnet(&other_hk, netuid);
        assert_eq!(other_stake_before, 0.into());

        // Distribute dividends and incentives
        SubtensorModule::distribute_dividends_and_incentives(
            netuid,
            owner_cut,
            incentives,
            BTreeMap::new(),
            BTreeMap::new(),
        );

        // Verify stake after
        let subnet_owner_stake_after =
            SubtensorModule::get_stake_for_hotkey_on_subnet(&subnet_owner_hk, netuid);
        assert_eq!(subnet_owner_stake_after, 0.into());
        let other_stake_after = SubtensorModule::get_stake_for_hotkey_on_subnet(&other_hk, netuid);
        assert!(other_stake_after > 0.into());
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_incentive_to_subnet_owners_hotkey_is_burned --exact --show-output --nocapture
#[test]
fn test_incentive_to_subnet_owners_hotkey_is_burned() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_ck = U256::from(0);
        let subnet_owner_hk = U256::from(1);

        // Other hk owned by owner
        let other_hk = U256::from(3);
        Owner::<Test>::insert(other_hk, subnet_owner_ck);
        OwnedHotkeys::<Test>::insert(subnet_owner_ck, vec![subnet_owner_hk, other_hk]);

        let netuid = add_dynamic_network(&subnet_owner_hk, &subnet_owner_ck);
        remove_owner_registration_stake(netuid);
        Uids::<Test>::insert(netuid, other_hk, 1);

        // Set the burn key limit to 2
        ImmuneOwnerUidsLimit::<Test>::insert(netuid, 2);

        let pending_tao: u64 = 1_000_000_000;
        let pending_alpha = AlphaBalance::ZERO; // None to valis
        let owner_cut = AlphaBalance::ZERO;
        let mut incentives: BTreeMap<U256, AlphaBalance> = BTreeMap::new();

        // Give incentive to other_hk
        incentives.insert(other_hk, 10_000_000.into());

        // Give incentives to subnet_owner_hk
        incentives.insert(subnet_owner_hk, 10_000_000.into());

        // Verify stake before
        let subnet_owner_stake_before =
            SubtensorModule::get_stake_for_hotkey_on_subnet(&subnet_owner_hk, netuid);
        assert_eq!(subnet_owner_stake_before, 0.into());
        let other_stake_before = SubtensorModule::get_stake_for_hotkey_on_subnet(&other_hk, netuid);
        assert_eq!(other_stake_before, 0.into());

        // Distribute dividends and incentives
        SubtensorModule::distribute_dividends_and_incentives(
            netuid,
            owner_cut,
            incentives,
            BTreeMap::new(),
            BTreeMap::new(),
        );

        // Verify stake after
        let subnet_owner_stake_after =
            SubtensorModule::get_stake_for_hotkey_on_subnet(&subnet_owner_hk, netuid);
        assert_eq!(subnet_owner_stake_after, 0.into());
        let other_stake_after = SubtensorModule::get_stake_for_hotkey_on_subnet(&other_hk, netuid);
        assert_eq!(other_stake_after, 0.into());
    });
}

// Test that if number of sn owner hotkeys is greater than ImmuneOwnerUidsLimit, then the ones with
// higher BlockAtRegistration are used to burn
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_burn_key_sorting --exact --show-output --nocapture
#[test]
fn test_burn_key_sorting() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_ck = U256::from(0);
        let subnet_owner_hk = U256::from(1);

        // Other hk owned by owner
        let other_hk_1 = U256::from(3);
        let other_hk_2 = U256::from(4);
        let other_hk_3 = U256::from(5);
        Owner::<Test>::insert(other_hk_1, subnet_owner_ck);
        Owner::<Test>::insert(other_hk_2, subnet_owner_ck);
        Owner::<Test>::insert(other_hk_3, subnet_owner_ck);
        OwnedHotkeys::<Test>::insert(
            subnet_owner_ck,
            vec![subnet_owner_hk, other_hk_1, other_hk_2, other_hk_3],
        );

        let netuid = add_dynamic_network(&subnet_owner_hk, &subnet_owner_ck);
        remove_owner_registration_stake(netuid);

        // Set block of registration and UIDs for other hotkeys
        // HK1 has block of registration 2
        // HK2 and HK3 have the same block of registration 1, so they are sorted by UID
        // Set HK2 UID = 3 and HK3 UID = 2 so that HK3 is burned and HK2 is not
        // Summary: HK1 and HK3 should be burned, HK2 should be not.
        // Let's test it now.
        BlockAtRegistration::<Test>::insert(netuid, 1, 2);
        BlockAtRegistration::<Test>::insert(netuid, 3, 1);
        BlockAtRegistration::<Test>::insert(netuid, 2, 1);
        Uids::<Test>::insert(netuid, other_hk_1, 1);
        Uids::<Test>::insert(netuid, other_hk_2, 3);
        Uids::<Test>::insert(netuid, other_hk_3, 2);

        let pending_tao: u64 = 1_000_000_000;
        let pending_alpha = AlphaBalance::ZERO; // None to valis
        let owner_cut = AlphaBalance::ZERO;
        let mut incentives: BTreeMap<U256, AlphaBalance> = BTreeMap::new();

        // Give incentive to hotkeys
        incentives.insert(other_hk_1, 10_000_000.into());
        incentives.insert(other_hk_2, 10_000_000.into());
        incentives.insert(other_hk_3, 10_000_000.into());

        // Give incentives to subnet_owner_hk
        incentives.insert(subnet_owner_hk, 10_000_000.into());

        // Distribute dividends and incentives
        SubtensorModule::distribute_dividends_and_incentives(
            netuid,
            owner_cut,
            incentives,
            BTreeMap::new(),
            BTreeMap::new(),
        );

        // SN owner is burned
        let subnet_owner_stake_after =
            SubtensorModule::get_stake_for_hotkey_on_subnet(&subnet_owner_hk, netuid);
        assert_eq!(subnet_owner_stake_after, 0.into());

        // No burn limits, all HKs should be burned
        let other_stake_after_1 =
            SubtensorModule::get_stake_for_hotkey_on_subnet(&other_hk_1, netuid);
        let other_stake_after_2 =
            SubtensorModule::get_stake_for_hotkey_on_subnet(&other_hk_2, netuid);
        let other_stake_after_3 =
            SubtensorModule::get_stake_for_hotkey_on_subnet(&other_hk_3, netuid);
        assert_eq!(other_stake_after_1, 0.into());
        assert_eq!(other_stake_after_2, 0.into());
        assert_eq!(other_stake_after_3, 0.into());
    });
}

#[test]
fn test_calculate_dividend_distribution_totals() {
    new_test_ext(1).execute_with(|| {
        let mut stake_map: BTreeMap<U256, (AlphaBalance, AlphaBalance)> = BTreeMap::new();
        let mut dividends: BTreeMap<U256, U96F32> = BTreeMap::new();

        let pending_validator_alpha = AlphaBalance::from(183_123_567_452_u64);
        let pending_root_alpha = AlphaBalance::from(837_120_949_872_u64);
        let tao_weight: U96F32 = U96F32::from_num(0.18); // 18%

        let hotkeys = [U256::from(0), U256::from(1)];

        // Stake map and dividends shouldn't matter for this test.
        stake_map.insert(hotkeys[0], (4_859_302.into(), 2_342_352.into()));
        stake_map.insert(hotkeys[1], (23_423.into(), 859_273.into()));
        dividends.insert(hotkeys[0], 77_783_738_u64.into());
        dividends.insert(hotkeys[1], 19_283_940_u64.into());

        let (alpha_dividends, root_alpha_dividends) =
            SubtensorModule::calculate_dividend_distribution(
                pending_validator_alpha,
                pending_root_alpha,
                tao_weight,
                stake_map,
                dividends,
            );

        // Verify the total of each dividends type is close to the inputs.
        let total_alpha_dividends = alpha_dividends.values().sum::<U96F32>();
        let total_root_alpha_dividends = root_alpha_dividends.values().sum::<U96F32>();

        assert_abs_diff_eq!(
            total_alpha_dividends.to_num::<u64>(),
            u64::from(pending_validator_alpha),
            epsilon = 1_000
        );
        assert_abs_diff_eq!(
            total_root_alpha_dividends.to_num::<u64>(),
            pending_root_alpha.to_u64(),
            epsilon = 1_000
        );
    });
}

#[test]
fn test_calculate_dividend_distribution_total_only_tao() {
    new_test_ext(1).execute_with(|| {
        let mut stake_map: BTreeMap<U256, (AlphaBalance, AlphaBalance)> = BTreeMap::new();
        let mut dividends: BTreeMap<U256, U96F32> = BTreeMap::new();

        let pending_validator_alpha = AlphaBalance::ZERO;
        let pending_root_alpha = AlphaBalance::from(837_120_949_872_u64);
        let tao_weight: U96F32 = U96F32::from_num(0.18); // 18%

        let hotkeys = [U256::from(0), U256::from(1)];

        // Stake map and dividends shouldn't matter for this test.
        stake_map.insert(hotkeys[0], (4_859_302.into(), 2_342_352.into()));
        stake_map.insert(hotkeys[1], (23_423.into(), 859_273.into()));
        dividends.insert(hotkeys[0], 77_783_738_u64.into());
        dividends.insert(hotkeys[1], 19_283_940_u64.into());

        let (alpha_dividends, root_alpha_dividends) =
            SubtensorModule::calculate_dividend_distribution(
                pending_validator_alpha,
                pending_root_alpha,
                tao_weight,
                stake_map,
                dividends,
            );

        // Verify the total of each dividends type is close to the inputs.
        let total_alpha_dividends = alpha_dividends.values().sum::<U96F32>();
        let total_root_alpha_dividends = root_alpha_dividends.values().sum::<U96F32>();

        assert_abs_diff_eq!(
            total_alpha_dividends.to_num::<u64>(),
            u64::from(pending_validator_alpha),
            epsilon = 1_000
        );
        assert_abs_diff_eq!(
            total_root_alpha_dividends.to_num::<u64>(),
            pending_root_alpha.to_u64(),
            epsilon = 1_000
        );
    });
}

#[test]
fn test_calculate_dividend_distribution_total_no_tao_weight() {
    new_test_ext(1).execute_with(|| {
        let mut stake_map: BTreeMap<U256, (AlphaBalance, AlphaBalance)> = BTreeMap::new();
        let mut dividends: BTreeMap<U256, U96F32> = BTreeMap::new();

        let pending_validator_alpha = AlphaBalance::from(183_123_567_452_u64);
        let pending_tao = TaoBalance::ZERO; // If tao weight is 0, then only alpha dividends should be input.
        let tao_weight: U96F32 = U96F32::from_num(0.0); // 0%

        let hotkeys = [U256::from(0), U256::from(1)];

        // Stake map and dividends shouldn't matter for this test.
        stake_map.insert(hotkeys[0], (4_859_302.into(), 2_342_352.into()));
        stake_map.insert(hotkeys[1], (23_423.into(), 859_273.into()));
        dividends.insert(hotkeys[0], 77_783_738_u64.into());
        dividends.insert(hotkeys[1], 19_283_940_u64.into());

        let (alpha_dividends, tao_dividends) = SubtensorModule::calculate_dividend_distribution(
            pending_validator_alpha,
            //   pending_tao,
            AlphaBalance::ZERO,
            tao_weight,
            stake_map,
            dividends,
        );

        // Verify the total of each dividends type is close to the inputs.
        let total_alpha_dividends = alpha_dividends.values().sum::<U96F32>();
        let total_tao_dividends = tao_dividends.values().sum::<U96F32>();

        assert_abs_diff_eq!(
            total_alpha_dividends.to_num::<u64>(),
            u64::from(pending_validator_alpha),
            epsilon = 1_000
        );
        assert_abs_diff_eq!(
            total_tao_dividends.to_num::<u64>(),
            pending_tao.to_u64(),
            epsilon = 1_000
        );
    });
}

#[test]
fn test_calculate_dividend_distribution_total_only_alpha() {
    new_test_ext(1).execute_with(|| {
        let mut stake_map: BTreeMap<U256, (AlphaBalance, AlphaBalance)> = BTreeMap::new();
        let mut dividends: BTreeMap<U256, U96F32> = BTreeMap::new();

        let pending_validator_alpha = AlphaBalance::from(183_123_567_452_u64);
        let pending_tao = TaoBalance::ZERO;
        let tao_weight: U96F32 = U96F32::from_num(0.18); // 18%

        let hotkeys = [U256::from(0), U256::from(1)];

        // Stake map and dividends shouldn't matter for this test.
        stake_map.insert(hotkeys[0], (4_859_302.into(), 2_342_352.into()));
        stake_map.insert(hotkeys[1], (23_423.into(), 859_273.into()));
        dividends.insert(hotkeys[0], 77_783_738_u64.into());
        dividends.insert(hotkeys[1], 19_283_940_u64.into());

        let (alpha_dividends, tao_dividends) = SubtensorModule::calculate_dividend_distribution(
            pending_validator_alpha,
            //   pending_tao,
            AlphaBalance::ZERO,
            tao_weight,
            stake_map,
            dividends,
        );

        // Verify the total of each dividends type is close to the inputs.
        let total_alpha_dividends = alpha_dividends.values().sum::<U96F32>();
        let total_tao_dividends = tao_dividends.values().sum::<U96F32>();

        assert_abs_diff_eq!(
            total_alpha_dividends.to_num::<u64>(),
            u64::from(pending_validator_alpha),
            epsilon = 1_000
        );
        assert_abs_diff_eq!(
            total_tao_dividends.to_num::<u64>(),
            pending_tao.to_u64(),
            epsilon = 1_000
        );
    });
}

#[test]
fn test_calculate_dividend_and_incentive_distribution() {
    new_test_ext(1).execute_with(|| {
        let sn_owner_hk = U256::from(0);
        let sn_owner_ck = U256::from(1);
        let netuid = add_dynamic_network(&sn_owner_hk, &sn_owner_ck);

        // Register a single neuron.
        let hotkey = U256::from(1);
        let coldkey = U256::from(2);
        register_ok_neuron(netuid, hotkey, coldkey, 0);
        // Give non-zero alpha
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            1.into(),
        );

        let pending_alpha = AlphaBalance::from(123_456_789);
        let pending_validator_alpha = pending_alpha / 2.into(); // Pay half to validators.
        let pending_tao = TaoBalance::ZERO;
        let pending_swapped = 0; // Only alpha output.
        let tao_weight: U96F32 = U96F32::from_num(0.0); // 0%

        // Hotkey, Incentive, Dividend
        let hotkey_emission = vec![(hotkey, pending_alpha / 2.into(), pending_alpha / 2.into())];

        let (incentives, (alpha_dividends, tao_dividends)) =
            SubtensorModule::calculate_dividend_and_incentive_distribution(
                netuid,
                //   pending_tao,
                AlphaBalance::ZERO,
                pending_validator_alpha,
                hotkey_emission,
                tao_weight,
            );

        let incentives_total = incentives.values().copied().map(u64::from).sum::<u64>();
        let dividends_total = alpha_dividends.values().sum::<U96F32>().to_num::<u64>();

        assert_abs_diff_eq!(
            dividends_total + incentives_total,
            u64::from(pending_alpha),
            epsilon = 2
        );
    });
}

#[test]
fn test_calculate_dividend_and_incentive_distribution_all_to_validators() {
    new_test_ext(1).execute_with(|| {
        let sn_owner_hk = U256::from(0);
        let sn_owner_ck = U256::from(1);
        let netuid = add_dynamic_network(&sn_owner_hk, &sn_owner_ck);

        // Register a single neuron.
        let hotkey = U256::from(1);
        let coldkey = U256::from(2);
        register_ok_neuron(netuid, hotkey, coldkey, 0);
        // Give non-zero alpha
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            1.into(),
        );

        let pending_alpha = AlphaBalance::from(123_456_789);
        let pending_validator_alpha = pending_alpha; // Pay all to validators.
        let pending_tao = TaoBalance::ZERO;
        let tao_weight: U96F32 = U96F32::from_num(0.0); // 0%

        // Hotkey, Incentive, Dividend
        let hotkey_emission = vec![(hotkey, 0.into(), pending_alpha)];

        let (incentives, (alpha_dividends, tao_dividends)) =
            SubtensorModule::calculate_dividend_and_incentive_distribution(
                netuid,
                //   pending_tao,
                AlphaBalance::ZERO,
                pending_validator_alpha,
                hotkey_emission,
                tao_weight,
            );

        let incentives_total = incentives.values().copied().map(u64::from).sum::<u64>();
        let dividends_total = alpha_dividends.values().sum::<U96F32>().to_num::<u64>();

        assert_eq!(
            AlphaBalance::from(dividends_total + incentives_total),
            pending_alpha
        );
    });
}

#[test]
fn test_calculate_dividends_and_incentives() {
    new_test_ext(1).execute_with(|| {
        let sn_owner_hk = U256::from(0);
        let sn_owner_ck = U256::from(1);
        let netuid = add_dynamic_network(&sn_owner_hk, &sn_owner_ck);

        // Register a single neuron.
        let hotkey = U256::from(1);
        let coldkey = U256::from(2);
        register_ok_neuron(netuid, hotkey, coldkey, 0);
        // Give non-zero alpha
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            1.into(),
        );

        let divdends = AlphaBalance::from(123_456_789);
        let incentive = AlphaBalance::from(683_051_923);
        let total_emission = divdends + incentive;

        // Hotkey, Incentive, Dividend
        let hotkey_emission = vec![(hotkey, incentive, divdends)];

        let (incentives, dividends) =
            SubtensorModule::calculate_dividends_and_incentives(netuid, hotkey_emission);

        let incentives_total = incentives
            .values()
            .copied()
            .fold(AlphaBalance::ZERO, |acc, x| acc + x);
        let dividends_total =
            AlphaBalance::from(dividends.values().sum::<U96F32>().to_num::<u64>());

        assert_eq!(dividends_total + incentives_total, total_emission);
    });
}

#[test]
fn test_calculate_dividends_and_incentives_only_validators() {
    new_test_ext(1).execute_with(|| {
        let sn_owner_hk = U256::from(0);
        let sn_owner_ck = U256::from(1);
        let netuid = add_dynamic_network(&sn_owner_hk, &sn_owner_ck);

        // Register a single neuron.
        let hotkey = U256::from(1);
        let coldkey = U256::from(2);
        register_ok_neuron(netuid, hotkey, coldkey, 0);
        // Give non-zero alpha
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            1.into(),
        );

        let divdends = AlphaBalance::from(123_456_789);
        let incentive = AlphaBalance::ZERO;

        // Hotkey, Incentive, Dividend
        let hotkey_emission = vec![(hotkey, incentive, divdends)];

        let (incentives, dividends) =
            SubtensorModule::calculate_dividends_and_incentives(netuid, hotkey_emission);

        let incentives_total = incentives
            .values()
            .copied()
            .fold(AlphaBalance::ZERO, |acc, x| acc + x);
        let dividends_total =
            AlphaBalance::from(dividends.values().sum::<U96F32>().to_num::<u64>());

        assert_eq!(dividends_total, divdends);
        assert_eq!(incentives_total, AlphaBalance::ZERO);
    });
}

#[test]
fn test_calculate_dividends_and_incentives_only_miners() {
    new_test_ext(1).execute_with(|| {
        let sn_owner_hk = U256::from(0);
        let sn_owner_ck = U256::from(1);
        let netuid = add_dynamic_network(&sn_owner_hk, &sn_owner_ck);

        // Register a single neuron.
        let hotkey = U256::from(1);
        let coldkey = U256::from(2);
        register_ok_neuron(netuid, hotkey, coldkey, 0);
        // Give non-zero alpha
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            1.into(),
        );

        let divdends = AlphaBalance::ZERO;
        let incentive = AlphaBalance::from(123_456_789);

        // Hotkey, Incentive, Dividend
        let hotkey_emission = vec![(hotkey, incentive, divdends)];

        let (incentives, dividends) =
            SubtensorModule::calculate_dividends_and_incentives(netuid, hotkey_emission);

        let incentives_total = incentives
            .values()
            .copied()
            .fold(AlphaBalance::ZERO, |acc, x| acc + x);
        let dividends_total =
            AlphaBalance::from(dividends.values().sum::<U96F32>().to_num::<u64>());

        assert_eq!(incentives_total, incentive);
        assert_eq!(dividends_total, divdends);
    });
}

#[test]
fn test_distribute_emission_no_miners_all_drained() {
    new_test_ext(1).execute_with(|| {
        let netuid = add_dynamic_network(&U256::from(1), &U256::from(2));
        remove_owner_registration_stake(netuid);
        let hotkey = U256::from(3);
        let coldkey = U256::from(4);
        let init_stake = 1;
        SubtensorModule::set_burn(netuid, TaoBalance::from(0));
        register_ok_neuron(netuid, hotkey, coldkey, 0);
        // Give non-zero stake
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            init_stake.into(),
        );
        assert_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&hotkey),
            init_stake.into()
        );

        // Set the weight of root TAO to be 0%, so only alpha is effective.
        SubtensorModule::set_tao_weight(0);

        // Set the emission to be 1 million.
        let emission = AlphaBalance::from(1_000_000);
        // Run drain pending without any miners.
        SubtensorModule::distribute_emission(
            netuid,
            emission.saturating_div(2.into()).into(),
            emission.saturating_div(2.into()).into(),
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
        );

        // Get the new stake of the hotkey.
        let new_stake = SubtensorModule::get_total_stake_for_hotkey(&hotkey);
        // We expect this neuron to get *all* the emission.
        // Slight epsilon due to rounding (hotkey_take).
        assert_abs_diff_eq!(
            new_stake,
            u64::from(emission + init_stake.into()).into(),
            epsilon = 1.into()
        );
    });
}

// cargo test --package pallet-subtensor --lib -- tests::coinbase::test_distribute_emission_zero_emission --exact --show-output
#[test]
fn test_distribute_emission_zero_emission() {
    new_test_ext(1).execute_with(|| {
        let netuid = add_dynamic_network_disable_commit_reveal(&U256::from(1), &U256::from(2));
        let hotkey = U256::from(3);
        let coldkey = U256::from(4);
        let miner_hk = U256::from(5);
        let miner_ck = U256::from(6);
        let init_stake: u64 = 100_000_000_000_000;
        let tempo = 2;
        SubtensorModule::set_tempo_unchecked(netuid, tempo);
        // Set weight-set limit to 0.
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);

        register_ok_neuron(netuid, hotkey, coldkey, 0);
        register_ok_neuron(netuid, miner_hk, miner_ck, 0);
        // Give non-zero stake
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            init_stake.into(),
        );
        assert_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&hotkey),
            init_stake.into()
        );

        // Set the weight of root TAO to be 0%, so only alpha is effective.
        SubtensorModule::set_tao_weight(0);

        run_to_block_no_epoch(netuid, 50);

        // Run epoch for initial setup.
        SubtensorModule::epoch(netuid, AlphaBalance::ZERO);

        // Set weights on miner
        assert_ok!(SubtensorModule::set_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            vec![0, 1, 2],
            vec![0, 0, 1],
            0,
        ));

        run_to_block_no_epoch(netuid, 50);

        // Clear incentive and dividends.
        Incentive::<Test>::remove(NetUidStorageIndex::from(netuid));
        Dividends::<Test>::remove(netuid);

        // Capture stake right before the zero-emission distribution so the assertion
        // isolates that call (the subnet legitimately accrues emission during the
        // preceding block runs under price-based shares).
        let stake_before_distribute = SubtensorModule::get_total_stake_for_hotkey(&hotkey);

        // Set the emission to be ZERO.
        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
        );

        // Get the new stake of the hotkey.
        let new_stake = SubtensorModule::get_total_stake_for_hotkey(&hotkey);
        // We expect the stake to remain unchanged by the zero-emission distribution.
        assert_eq!(new_stake, stake_before_distribute);

        // Check that the incentive and dividends are set by epoch.
        assert!(
            Incentive::<Test>::get(NetUidStorageIndex::from(netuid))
                .iter()
                .sum::<u16>()
                > 0
        );
        assert!(Dividends::<Test>::get(netuid).iter().sum::<u16>() > 0);
    });
}

#[test]
fn test_run_coinbase_not_started() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let tempo = 2;

        let sn_owner_hk = U256::from(7);
        let sn_owner_ck = U256::from(8);

        add_network_without_emission_block(netuid, tempo, 0);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, false);
        assert_eq!(FirstEmissionBlockNumber::<Test>::get(netuid), None);

        SubnetOwner::<Test>::insert(netuid, sn_owner_ck);
        SubnetOwnerHotkey::<Test>::insert(netuid, sn_owner_hk);

        let hotkey = U256::from(3);
        let coldkey = U256::from(4);
        let miner_hk = U256::from(5);
        let miner_ck = U256::from(6);
        let init_stake: u64 = 100_000_000_000_000;
        let tempo = 2;
        SubtensorModule::set_tempo_unchecked(netuid, tempo);
        // Set weight-set limit to 0.
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);

        let reserve = init_stake * 1000;
        mock::setup_reserves(netuid, reserve.into(), reserve.into());

        register_ok_neuron(netuid, hotkey, coldkey, 0);
        register_ok_neuron(netuid, miner_hk, miner_ck, 0);
        register_ok_neuron(netuid, sn_owner_hk, sn_owner_ck, 0);
        // Give non-zero stake
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            init_stake.into(),
        );
        assert_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&hotkey),
            init_stake.into()
        );

        // Set the weight of root TAO to be 0%, so only alpha is effective.
        SubtensorModule::set_tao_weight(0);

        run_to_block_no_epoch(netuid, 30);

        // Run epoch for initial setup.
        SubtensorModule::epoch(netuid, AlphaBalance::ZERO);

        // Set weights on miner
        assert_ok!(SubtensorModule::set_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            vec![0, 1, 2],
            vec![0, 0, 1],
            0,
        ));

        // Clear incentive and dividends.
        Incentive::<Test>::remove(NetUidStorageIndex::from(netuid));
        Dividends::<Test>::remove(netuid);

        // Step so tempo should run.
        next_block_no_epoch(netuid);
        next_block_no_epoch(netuid);
        next_block_no_epoch(netuid);
        let current_block = System::block_number();
        assert!(SubtensorModule::should_run_epoch(netuid, current_block));

        // Run coinbase with emission.
        let emission_credit = SubtensorModule::mint_tao(100_000_000.into());
        SubtensorModule::run_coinbase(emission_credit);

        // We expect that the epoch ran.
        assert_eq!(BlocksSinceLastStep::<Test>::get(netuid), 0);

        // Get the new stake of the hotkey. We expect no emissions.
        let new_stake = SubtensorModule::get_total_stake_for_hotkey(&hotkey);
        // We expect the stake to remain unchanged.
        assert_eq!(new_stake, init_stake.into());

        // Check that the incentive and dividends are set.
        assert!(
            Incentive::<Test>::get(NetUidStorageIndex::from(netuid))
                .iter()
                .sum::<u16>()
                > 0
        );
        assert!(Dividends::<Test>::get(netuid).iter().sum::<u16>() > 0);
    });
}

#[test]
fn test_run_coinbase_not_started_start_after() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let tempo = 2;

        let sn_owner_hk = U256::from(7);
        let sn_owner_ck = U256::from(8);

        add_network_without_emission_block(netuid, tempo, 0);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, false);
        assert_eq!(FirstEmissionBlockNumber::<Test>::get(netuid), None);

        SubnetOwner::<Test>::insert(netuid, sn_owner_ck);
        SubnetOwnerHotkey::<Test>::insert(netuid, sn_owner_hk);

        let hotkey = U256::from(3);
        let coldkey = U256::from(4);
        let miner_hk = U256::from(5);
        let miner_ck = U256::from(6);
        let init_stake: u64 = 100_000_000_000_000;
        let tempo = 2;
        SubtensorModule::set_tempo_unchecked(netuid, tempo);
        // Set weight-set limit to 0.
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);

        register_ok_neuron(netuid, hotkey, coldkey, 0);
        register_ok_neuron(netuid, miner_hk, miner_ck, 0);
        register_ok_neuron(netuid, sn_owner_hk, sn_owner_ck, 0);
        // Give non-zero stake
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            init_stake.into(),
        );
        assert_eq!(
            SubtensorModule::get_total_stake_for_hotkey(&hotkey),
            init_stake.into()
        );

        // Set the weight of root TAO to be 0%, so only alpha is effective.
        SubtensorModule::set_tao_weight(0);

        run_to_block_no_epoch(netuid, 30);

        // Run epoch for initial setup.
        SubtensorModule::epoch(netuid, AlphaBalance::ZERO);

        // Set weights on miner
        assert_ok!(SubtensorModule::set_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            vec![0, 1, 2],
            vec![0, 0, 1],
            0,
        ));

        // Clear incentive and dividends.
        Incentive::<Test>::remove(NetUidStorageIndex::from(netuid));
        Dividends::<Test>::remove(netuid);

        // Step so tempo should run.
        next_block_no_epoch(netuid);
        next_block_no_epoch(netuid);
        next_block_no_epoch(netuid);
        let current_block = System::block_number();
        assert!(SubtensorModule::should_run_epoch(netuid, current_block));

        // Run coinbase with emission.
        let emission_credit = SubtensorModule::mint_tao(100_000_000.into());
        SubtensorModule::run_coinbase(emission_credit);
        // We expect that the epoch ran.
        assert_eq!(BlocksSinceLastStep::<Test>::get(netuid), 0);

        let block_number = StartCallDelay::<Test>::get();
        run_to_block_no_epoch(netuid, block_number);

        let current_block = System::block_number();

        // Run start call.
        assert_ok!(SubtensorModule::start_call(
            RuntimeOrigin::signed(sn_owner_ck),
            netuid
        ));
        assert_eq!(
            FirstEmissionBlockNumber::<Test>::get(netuid),
            Some(current_block + 1)
        );

        // Advance the block past `LastEpochBlock + tempo` so the state-based
        // scheduler is due again (the previous `run_coinbase` advanced it).
        next_block_no_epoch(netuid);
        next_block_no_epoch(netuid);
        next_block_no_epoch(netuid);

        // Run coinbase with emission.
        let emission_credit = SubtensorModule::mint_tao(100_000_000.into());
        SubtensorModule::run_coinbase(emission_credit);
        // We expect that the epoch ran.
        assert_eq!(BlocksSinceLastStep::<Test>::get(netuid), 0);

        // Get the new stake of the hotkey. We expect no emissions.
        let new_stake = SubtensorModule::get_total_stake_for_hotkey(&hotkey);
        // We expect the stake to remain unchanged.
        assert!(new_stake > init_stake.into());
        log::info!("new_stake: {new_stake}");
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_drain_alpha_childkey_parentkey_with_burn --exact --show-output --nocapture
#[test]
fn test_drain_alpha_childkey_parentkey_with_burn() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        let parent = U256::from(1);
        let child = U256::from(2);
        let coldkey = U256::from(3);
        let stake_before = AlphaBalance::from(1_000_000_000);
        register_ok_neuron(netuid, child, coldkey, 0);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &parent,
            &coldkey,
            netuid,
            stake_before,
        );
        mock_set_children_no_epochs(netuid, &parent, &[(u64::MAX, child)]);

        // Childkey take is 10%
        ChildkeyTake::<Test>::insert(child, netuid, u16::MAX / 10);

        let burn_rate = SubtensorModule::get_ck_burn();
        let parent_stake_before = SubtensorModule::get_stake_for_hotkey_on_subnet(&parent, netuid);
        let child_stake_before = SubtensorModule::get_stake_for_hotkey_on_subnet(&child, netuid);

        let pending_alpha = AlphaBalance::from(1_000_000_000);
        SubtensorModule::distribute_emission(
            netuid,
            pending_alpha.saturating_div(2.into()).into(),
            pending_alpha.saturating_div(2.into()).into(),
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
        );
        let parent_stake_after = SubtensorModule::get_stake_for_hotkey_on_subnet(&parent, netuid);
        let child_stake_after = SubtensorModule::get_stake_for_hotkey_on_subnet(&child, netuid);

        let expected_ck_burn = I96F32::from_num(pending_alpha)
            * I96F32::from_num(9.0 / 10.0)
            * I96F32::from_num(burn_rate);

        let expected_total = I96F32::from_num(pending_alpha) - expected_ck_burn;
        let parent_ratio = (I96F32::from_num(pending_alpha) * I96F32::from_num(9.0 / 10.0)
            - expected_ck_burn)
            / expected_total;
        let child_ratio = (I96F32::from_num(pending_alpha) / I96F32::from_num(10)) / expected_total;

        let expected =
            I96F32::from_num(stake_before) + I96F32::from_num(pending_alpha) * parent_ratio;
        log::info!(
            "expected: {:?}, parent_stake_after: {:?}",
            expected.to_num::<u64>(),
            parent_stake_after
        );

        close(
            expected.to_num::<u64>(),
            parent_stake_after.into(),
            3_000_000,
        );
        let expected = I96F32::from_num(u64::from(pending_alpha)) * child_ratio;
        close(
            expected.to_num::<u64>(),
            child_stake_after.into(),
            3_000_000,
        );
    });
}

#[test]
fn test_incentive_is_autostaked_to_owner_destination() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_ck = U256::from(0);
        let subnet_owner_hk = U256::from(1);

        let miner_ck = U256::from(10);
        let miner_hk = U256::from(11);
        let dest_hk = U256::from(12);

        Owner::<Test>::insert(miner_hk, miner_ck);
        Owner::<Test>::insert(dest_hk, miner_ck);
        OwnedHotkeys::<Test>::insert(miner_ck, vec![miner_hk, dest_hk]);

        let netuid = add_dynamic_network(&subnet_owner_hk, &subnet_owner_ck);

        Uids::<Test>::insert(netuid, miner_hk, 1);
        Uids::<Test>::insert(netuid, dest_hk, 2);

        // Set autostake destination for the miner's coldkey
        assert_ok!(SubtensorModule::set_coldkey_auto_stake_hotkey(
            RuntimeOrigin::signed(miner_ck),
            netuid,
            dest_hk,
        ));

        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_on_subnet(&miner_hk, netuid),
            0.into()
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_on_subnet(&dest_hk, netuid),
            0.into()
        );

        // Distribute an incentive to the miner hotkey
        let mut incentives: BTreeMap<U256, AlphaBalance> = BTreeMap::new();
        let incentive: AlphaBalance = 10_000_000u64.into();
        incentives.insert(miner_hk, incentive);

        SubtensorModule::distribute_dividends_and_incentives(
            netuid,
            AlphaBalance::ZERO, // owner_cut
            incentives,
            BTreeMap::new(), // alpha_dividends
            BTreeMap::new(), // tao_dividends
        );

        // Expect the stake to land on the destination hotkey (not the original miner hotkey)
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_on_subnet(&miner_hk, netuid),
            0.into()
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_on_subnet(&dest_hk, netuid),
            incentive
        );
    });
}

#[test]
fn test_incentive_goes_to_hotkey_when_no_autostake_destination() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_ck = U256::from(0);
        let subnet_owner_hk = U256::from(1);

        let miner_ck = U256::from(20);
        let miner_hk = U256::from(21);

        Owner::<Test>::insert(miner_hk, miner_ck);
        OwnedHotkeys::<Test>::insert(miner_ck, vec![miner_hk]);

        let netuid = add_dynamic_network(&subnet_owner_hk, &subnet_owner_ck);

        Uids::<Test>::insert(netuid, miner_hk, 1);

        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_on_subnet(&miner_hk, netuid),
            0.into()
        );

        // Distribute an incentive to the miner hotkey
        let mut incentives: BTreeMap<U256, AlphaBalance> = BTreeMap::new();
        let incentive: AlphaBalance = 5_000_000u64.into();
        incentives.insert(miner_hk, incentive);

        SubtensorModule::distribute_dividends_and_incentives(
            netuid,
            AlphaBalance::ZERO, // owner_cut
            incentives,
            BTreeMap::new(), // alpha_dividends
            BTreeMap::new(), // tao_dividends
        );

        // With no autostake destination, the incentive should be staked to the original hotkey
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_on_subnet(&miner_hk, netuid),
            incentive
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_zero_shares_zero_emission --exact --show-output --nocapture
#[test]
fn test_zero_shares_zero_emission() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_ck = U256::from(0);
        let subnet_owner_hk = U256::from(1);
        let netuid1 = add_dynamic_network(&subnet_owner_hk, &subnet_owner_ck);
        let netuid2 = add_dynamic_network(&subnet_owner_hk, &subnet_owner_ck);
        let emission: u64 = 1_000_000;
        let emission_credit = SubtensorModule::mint_tao(emission.into());
        // Setup prices 1 and 1
        let initial: u64 = 1_000_000;
        SubnetTAO::<Test>::insert(netuid1, TaoBalance::from(initial));
        SubnetAlphaIn::<Test>::insert(netuid1, AlphaBalance::from(initial));
        SubnetTAO::<Test>::insert(netuid2, TaoBalance::from(initial));
        SubnetAlphaIn::<Test>::insert(netuid2, AlphaBalance::from(initial));
        // Set subnet prices so that both are
        //   - cut off by lower limit for tao flow method
        //   - zeroed out for price ema method
        SubnetMovingPrice::<Test>::insert(netuid1, I96F32::from_num(0));
        SubnetMovingPrice::<Test>::insert(netuid2, I96F32::from_num(0));
        // Run coinbase
        SubtensorModule::run_coinbase(emission_credit);
        // Netuid 1 is cut off by lower limit, all emission goes to netuid2
        assert_eq!(SubnetAlphaIn::<Test>::get(netuid1), initial.into());
        assert_eq!(SubnetAlphaIn::<Test>::get(netuid2), initial.into());
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_mining_emission_distribution_with_no_root_sell --exact --show-output --nocapture
#[test]
fn test_mining_emission_distribution_with_no_root_sell() {
    new_test_ext(1).execute_with(|| {
        let validator_coldkey = U256::from(1);
        let validator_hotkey = U256::from(2);
        let validator_miner_coldkey = U256::from(3);
        let validator_miner_hotkey = U256::from(4);
        let miner_coldkey = U256::from(5);
        let miner_hotkey = U256::from(6);
        let netuid = NetUid::from(1);
        let subnet_tempo = 10;
        let stake: u64 = 100_000_000_000;
        let root_stake: u64 = 200_000_000_000; // 200 TAO

        // Create root network
        SubtensorModule::set_tao_weight(0); // Start tao weight at 0
        SubtokenEnabled::<Test>::insert(NetUid::ROOT, true);
        NetworksAdded::<Test>::insert(NetUid::ROOT, true);

        // Add network, register hotkeys, and setup network parameters
        add_network(netuid, subnet_tempo, 0);
        SubnetMechanism::<Test>::insert(netuid, 1); // Set mechanism to 1

        // Setup large LPs to prevent slippage
        SubnetTAO::<Test>::insert(netuid, TaoBalance::from(1_000_000_000_000_000_u64));
        SubnetAlphaIn::<Test>::insert(netuid, AlphaBalance::from(1_000_000_000_000_000_u64));

        register_ok_neuron(netuid, validator_hotkey, validator_coldkey, 0);
        register_ok_neuron(netuid, validator_miner_hotkey, validator_miner_coldkey, 1);
        register_ok_neuron(netuid, miner_hotkey, miner_coldkey, 2);
        add_balance_to_coldkey_account(
            &validator_coldkey,
            TaoBalance::from(stake) + ExistentialDeposit::get(),
        );
        add_balance_to_coldkey_account(
            &validator_miner_coldkey,
            TaoBalance::from(stake) + ExistentialDeposit::get(),
        );
        add_balance_to_coldkey_account(
            &miner_coldkey,
            TaoBalance::from(stake) + ExistentialDeposit::get(),
        );
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);
        step_block(subnet_tempo);
        SubnetOwnerCut::<Test>::set(u16::MAX / 10);
        // There are two validators and three neurons
        MaxAllowedUids::<Test>::set(netuid, 3);
        SubtensorModule::set_max_allowed_validators(netuid, 2);

        // Setup stakes:
        //   Stake from validator
        //   Stake from valiminer
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(validator_coldkey),
            validator_hotkey,
            netuid,
            stake.into()
        ));
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(validator_miner_coldkey),
            validator_miner_hotkey,
            netuid,
            stake.into()
        ));

        // Setup YUMA so that it creates emissions
        Weights::<Test>::insert(NetUidStorageIndex::from(netuid), 0, vec![(1, 0xFFFF)]);
        Weights::<Test>::insert(NetUidStorageIndex::from(netuid), 1, vec![(2, 0xFFFF)]);
        BlockAtRegistration::<Test>::set(netuid, 0, 1);
        BlockAtRegistration::<Test>::set(netuid, 1, 1);
        BlockAtRegistration::<Test>::set(netuid, 2, 1);
        LastUpdate::<Test>::set(NetUidStorageIndex::from(netuid), vec![2, 2, 2]);
        Kappa::<Test>::set(netuid, u16::MAX / 5);
        ActivityCutoff::<Test>::set(netuid, u16::MAX); // makes all stake active
        ValidatorPermit::<Test>::insert(netuid, vec![true, true, false]);

        // Run run_coinbase until emissions are drained
        step_block(subnet_tempo);

        // Add stake to validator so it has root stake
        add_balance_to_coldkey_account(&validator_coldkey, root_stake.into());
        // init root
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(validator_coldkey),
            validator_hotkey,
            NetUid::ROOT,
            root_stake.into()
        ));
        // Set tao weight non zero
        SubtensorModule::set_tao_weight(u64::MAX / 10);

        // Make root sell NOT happen
        // set price very low, e.g. a lot of alpha in
        let alpha = AlphaBalance::from(1_000_000_000_000_000_000_u64);
        SubnetAlphaIn::<Test>::insert(netuid, alpha);

        // Make sure we ARE NOT root selling, so we do not have root alpha divs.
        let root_sell_flag = SubtensorModule::get_network_root_sell_flag(&[netuid]);
        assert!(!root_sell_flag, "Root sell flag should be false");

        // Run run_coinbase until emissions are drained
        step_block(subnet_tempo);

        let old_root_alpha_divs = PendingRootAlphaDivs::<Test>::get(netuid);
        let per_block_emission = SubtensorModule::get_block_emission_for_issuance(
            SubtensorModule::get_alpha_issuance(netuid).into(),
        )
        .unwrap_or(0);

        // step by one block
        step_block(1);
        // Verify that root alpha divs
        let new_root_alpha_divs = PendingRootAlphaDivs::<Test>::get(netuid);
        // Check that we are indeed NOT root selling, i.e. that root alpha divs are NOT increasing
        assert_eq!(
            new_root_alpha_divs, old_root_alpha_divs,
            "Root alpha divs should not increase"
        );
        // Check root divs are zero
        assert_eq!(
            new_root_alpha_divs,
            AlphaBalance::ZERO,
            "Root alpha divs should be zero"
        );
        step_block(1);
        // Drain to a clean epoch boundary so accumulation starts fresh.
        step_epochs(1, netuid);
        let miner_stake_before_epoch = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &miner_hotkey,
            &miner_coldkey,
            netuid,
        );
        // Run again but with some root stake
        step_block(subnet_tempo - 1);
        assert_abs_diff_eq!(
            PendingServerEmission::<Test>::get(netuid).to_u64(),
            U96F32::saturating_from_num(per_block_emission)
                .saturating_mul(U96F32::saturating_from_num((subnet_tempo - 1) as u64))
                .saturating_mul(U96F32::saturating_from_num(0.5)) // miner cut
                .saturating_mul(U96F32::saturating_from_num(0.90))
                .saturating_to_num::<u64>(),
            epsilon = 100_000_u64.into()
        );
        step_block(1);
        assert!(
            BlocksSinceLastStep::<Test>::get(netuid) == 0,
            "Blocks since last step should be 0"
        );

        let miner_uid = Uids::<Test>::get(netuid, miner_hotkey).unwrap_or(0);
        log::info!("Miner uid: {miner_uid:?}");
        let miner_incentive: AlphaBalance = {
            let miner_incentive = Incentive::<Test>::get(NetUidStorageIndex::from(netuid))
                .get(miner_uid as usize)
                .copied();

            assert!(miner_incentive.is_some());

            (miner_incentive.unwrap_or_default() as u64).into()
        };
        log::info!("Miner incentive: {miner_incentive:?}");

        // Miner emissions
        let miner_emission_1: u64 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &miner_hotkey,
            &miner_coldkey,
            netuid,
        )
        .to_u64()
            - miner_stake_before_epoch.to_u64();

        assert_abs_diff_eq!(
            Incentive::<Test>::get(NetUidStorageIndex::from(netuid))
                .iter()
                .sum::<u16>(),
            u16::MAX,
            epsilon = 10
        );

        assert_abs_diff_eq!(
            miner_emission_1,
            U96F32::saturating_from_num(miner_incentive)
                .saturating_div(u16::MAX.into())
                .saturating_mul(U96F32::saturating_from_num(per_block_emission))
                .saturating_mul(U96F32::saturating_from_num(subnet_tempo))
                .saturating_mul(U96F32::saturating_from_num(0.45)) // miner cut
                .saturating_to_num::<u64>(),
            epsilon = 1_000_000_u64
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_mining_emission_distribution_with_root_sell --exact --show-output --nocapture
#[test]
fn test_mining_emission_distribution_with_root_sell() {
    new_test_ext(1).execute_with(|| {
        let validator_coldkey = U256::from(1);
        let validator_hotkey = U256::from(2);
        let validator_miner_coldkey = U256::from(3);
        let validator_miner_hotkey = U256::from(4);
        let miner_coldkey = U256::from(5);
        let miner_hotkey = U256::from(6);
        let subnet_tempo = 10;
        let stake: u64 = 100_000_000_000;
        let root_stake: u64 = 200_000_000_000; // 200 TAO

        // Create root network
        SubtensorModule::set_tao_weight(0); // Start tao weight at 0
        SubtokenEnabled::<Test>::insert(NetUid::ROOT, true);
        NetworksAdded::<Test>::insert(NetUid::ROOT, true);

        // Add network, register hotkeys, and setup network parameters
        let owner_hotkey = U256::from(10);
        let owner_coldkey = U256::from(11);
        let netuid = add_dynamic_network(&owner_hotkey, &owner_coldkey);
        // Period is `tempo`; `tempo = 2` keeps a one-block gap between epochs so
        // pending root-alpha-divs can be observed accumulating before a drain.
        Tempo::<Test>::insert(netuid, 2);
        FirstEmissionBlockNumber::<Test>::insert(netuid, 0);

        // Setup large LPs to prevent slippage
        SubnetTAO::<Test>::insert(netuid, TaoBalance::from(1_000_000_000_000_000_u64));
        SubnetAlphaIn::<Test>::insert(netuid, AlphaBalance::from(1_000_000_000_000_000_u64));

        register_ok_neuron(netuid, validator_hotkey, validator_coldkey, 0);
        register_ok_neuron(netuid, validator_miner_hotkey, validator_miner_coldkey, 1);
        register_ok_neuron(netuid, miner_hotkey, miner_coldkey, 2);
        add_balance_to_coldkey_account(
            &validator_coldkey,
            TaoBalance::from(stake) + ExistentialDeposit::get(),
        );
        add_balance_to_coldkey_account(
            &validator_miner_coldkey,
            TaoBalance::from(stake) + ExistentialDeposit::get(),
        );
        add_balance_to_coldkey_account(
            &miner_coldkey,
            TaoBalance::from(stake) + ExistentialDeposit::get(),
        );
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);
        step_block(subnet_tempo);
        SubnetOwnerCut::<Test>::set(u16::MAX / 10);
        // There are two validators and three neurons
        MaxAllowedUids::<Test>::set(netuid, 3);
        SubtensorModule::set_max_allowed_validators(netuid, 2);

        // Setup stakes:
        //   Stake from validator
        //   Stake from valiminer
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(validator_coldkey),
            validator_hotkey,
            netuid,
            stake.into()
        ));
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(validator_miner_coldkey),
            validator_miner_hotkey,
            netuid,
            stake.into()
        ));

        // Setup YUMA so that it creates emissions
        Weights::<Test>::insert(NetUidStorageIndex::from(netuid), 0, vec![(1, 0xFFFF)]);
        Weights::<Test>::insert(NetUidStorageIndex::from(netuid), 1, vec![(2, 0xFFFF)]);
        BlockAtRegistration::<Test>::set(netuid, 0, 1);
        BlockAtRegistration::<Test>::set(netuid, 1, 1);
        BlockAtRegistration::<Test>::set(netuid, 2, 1);
        LastUpdate::<Test>::set(NetUidStorageIndex::from(netuid), vec![2, 2, 2]);
        Kappa::<Test>::set(netuid, u16::MAX / 5);
        ActivityCutoff::<Test>::set(netuid, u16::MAX); // makes all stake active
        ValidatorPermit::<Test>::insert(netuid, vec![true, true, false]);

        // Run run_coinbase until emissions are drained
        step_block(subnet_tempo);

        // Add stake to validator so it has root stake
        add_balance_to_coldkey_account(&validator_coldkey, root_stake.into());
        // init root
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(validator_coldkey),
            validator_hotkey,
            NetUid::ROOT,
            root_stake.into()
        ));
        // Set tao weight non zero
        SubtensorModule::set_tao_weight(u64::MAX / 10);

        // Make root sell happen
        // Set moving price > 1.0
        // Set price > 1.0
        let alpha = AlphaBalance::from(100_000_000_000_000_u64);
        SubnetAlphaIn::<Test>::insert(netuid, alpha);

        SubnetMovingPrice::<Test>::insert(netuid, I96F32::from_num(2));

        // Make sure we are root selling, so we have root alpha divs.
        let root_sell_flag = SubtensorModule::get_network_root_sell_flag(&[netuid]);
        assert!(root_sell_flag, "Root sell flag should be true");

        // Run run_coinbase until emissions are drained
        step_block(subnet_tempo);

        LastEpochBlock::<Test>::insert(netuid, SubtensorModule::get_current_block_as_u64());
        let old_root_alpha_divs = PendingRootAlphaDivs::<Test>::get(netuid);
        let miner_stake_before_epoch = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &miner_hotkey,
            &miner_coldkey,
            netuid,
        );

        // step by one block
        step_block(1);
        // Verify root alpha divs
        let new_root_alpha_divs = PendingRootAlphaDivs::<Test>::get(netuid);
        // Check that we ARE root selling, i.e. that root alpha divs are changing
        assert_ne!(
            new_root_alpha_divs, old_root_alpha_divs,
            "Root alpha divs should be changing"
        );
        assert!(
            new_root_alpha_divs > AlphaBalance::ZERO,
            "Root alpha divs should be greater than 0"
        );

        // Run again but with some root stake
        step_block(subnet_tempo - 1);

        let miner_uid = Uids::<Test>::get(netuid, miner_hotkey).unwrap_or(0);
        let miner_incentive: AlphaBalance = {
            let miner_incentive = Incentive::<Test>::get(NetUidStorageIndex::from(netuid))
                .get(miner_uid as usize)
                .copied();

            assert!(miner_incentive.is_some());

            (miner_incentive.unwrap_or_default() as u64).into()
        };
        log::info!("Miner incentive: {miner_incentive:?}");

        let per_block_emission = SubtensorModule::get_block_emission_for_issuance(
            SubtensorModule::get_alpha_issuance(netuid).into(),
        )
        .unwrap_or(0);

        // Miner emissions
        let miner_emission_1: u64 = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &miner_hotkey,
            &miner_coldkey,
            netuid,
        )
        .to_u64()
            - miner_stake_before_epoch.to_u64();

        assert_abs_diff_eq!(
            miner_emission_1,
            U96F32::saturating_from_num(miner_incentive)
                .saturating_div(u16::MAX.into())
                .saturating_mul(U96F32::saturating_from_num(per_block_emission))
                .saturating_mul(U96F32::saturating_from_num(subnet_tempo))
                .saturating_mul(U96F32::saturating_from_num(0.45)) // miner cut
                .saturating_to_num::<u64>(),
            epsilon = 1_000_000_u64
        );
    });
}

#[test]
fn test_coinbase_subnets_with_no_reg_get_no_emission() {
    new_test_ext(1).execute_with(|| {
        let zero = U96F32::saturating_from_num(0);
        let netuid0 = add_dynamic_network(&U256::from(1), &U256::from(2));
        let netuid1 = add_dynamic_network(&U256::from(3), &U256::from(4));

        // Setup initial state
        SubtokenEnabled::<Test>::insert(netuid0, true);
        SubtokenEnabled::<Test>::insert(netuid1, true);
        FirstEmissionBlockNumber::<Test>::insert(netuid0, 0);
        FirstEmissionBlockNumber::<Test>::insert(netuid1, 0);
        // Explicitly allow registration for both subnets
        NetworkRegistrationAllowed::<Test>::insert(netuid0, true);
        NetworkRegistrationAllowed::<Test>::insert(netuid1, true);
        NetworkPowRegistrationAllowed::<Test>::insert(netuid0, false);
        NetworkPowRegistrationAllowed::<Test>::insert(netuid1, true);

        // Note that netuid0 has only one method allowed
        // And, netuid1 has *both* methods allowed
        // Both should be in the list.
        let subnets_to_emit_to_0 = SubtensorModule::get_subnets_to_emit_to(&[netuid0, netuid1]);
        // Check that both subnets are in the list
        assert_eq!(subnets_to_emit_to_0.len(), 2);
        assert!(subnets_to_emit_to_0.contains(&netuid0));
        assert!(subnets_to_emit_to_0.contains(&netuid1));

        // Disabled registration of both methods on ONLY netuid0
        NetworkRegistrationAllowed::<Test>::insert(netuid0, false);
        NetworkPowRegistrationAllowed::<Test>::insert(netuid0, false);

        // Check that netuid0 is not in the list
        let subnets_to_emit_to_1 = SubtensorModule::get_subnets_to_emit_to(&[netuid0, netuid1]);
        assert_eq!(subnets_to_emit_to_1.len(), 1);
        assert!(!subnets_to_emit_to_1.contains(&netuid0));
        // Netuid1 still in the list
        assert!(subnets_to_emit_to_1.contains(&netuid1));
    });
}

// Tests for the excess TAO condition
#[test]
fn test_coinbase_subnet_terms_with_alpha_in_gt_alpha_emission() {
    new_test_ext(1).execute_with(|| {
        let zero = U96F32::saturating_from_num(0);
        let netuid0 = add_dynamic_network(&U256::from(1), &U256::from(2));
        mock::setup_reserves(
            netuid0,
            TaoBalance::from(1_000_000_000_000_000_u64),
            AlphaBalance::from(1_000_000_000_000_000_u64),
        );
        // Initialize swap
        Swap::maybe_initialize_palswap(netuid0, None);

        // Set netuid0 to have price tao_emission / price > alpha_emission
        let alpha_emission = U96F32::saturating_from_num(
            SubtensorModule::get_block_emission_for_issuance(
                SubtensorModule::get_alpha_issuance(netuid0).into(),
            )
            .unwrap_or(0),
        );
        let price_to_set: U64F64 = U64F64::saturating_from_num(0.01);
        let price_to_set_fixed: U96F32 = U96F32::saturating_from_num(price_to_set);

        let tao_emission: U96F32 = U96F32::saturating_from_num(alpha_emission)
            .saturating_mul(price_to_set_fixed)
            .saturating_add(U96F32::saturating_from_num(0.01));

        // Set the price
        let tao = TaoBalance::from(1_000_000_000_u64);
        let alpha = AlphaBalance::from(
            (U64F64::saturating_from_num(u64::from(tao)) / price_to_set).to_num::<u64>(),
        );
        SubnetTAO::<Test>::insert(netuid0, tao);
        SubnetAlphaIn::<Test>::insert(netuid0, alpha);

        // Check the price is set
        assert_abs_diff_eq!(
            pallet_subtensor_swap::Pallet::<Test>::current_alpha_price(netuid0).to_num::<f64>(),
            price_to_set.to_num::<f64>(),
            epsilon = 0.001
        );

        let subnet_emissions = BTreeMap::from([(netuid0, tao_emission)]);

        // The injection cap is root_proportion * alpha_emission. Seed root stake so
        // root_proportion is well-defined and the cap is positive.
        set_full_injection_root_stake();
        let root_prop: U96F32 = SubtensorModule::root_proportion(netuid0);
        let injection_cap: U96F32 = root_prop.saturating_mul(alpha_emission);

        let (tao_in, alpha_in, alpha_out, excess_tao) =
            SubtensorModule::get_subnet_terms(&subnet_emissions);

        // Check our condition is met: the raw alpha_in exceeds the cap, so it binds.
        assert!(tao_emission / price_to_set_fixed > injection_cap);

        // alpha_out should be the alpha_emission, always
        assert_abs_diff_eq!(
            alpha_out[&netuid0].to_num::<f64>(),
            alpha_emission.to_num::<f64>(),
            epsilon = 0.01
        );

        // alpha_in should be capped at root_proportion * alpha_emission
        assert_abs_diff_eq!(
            alpha_in[&netuid0].to_num::<f64>(),
            injection_cap.to_num::<f64>(),
            epsilon = injection_cap.to_num::<f64>() / 1_000.0
        );
        // tao_in should be the alpha_in at the ratio of the price
        assert_abs_diff_eq!(
            tao_in[&netuid0].to_num::<f64>(),
            alpha_in[&netuid0]
                .saturating_mul(price_to_set_fixed)
                .to_num::<f64>(),
            epsilon = 0.01
        );

        // excess_tao should be the difference between the tao_emission and the tao_in
        assert_abs_diff_eq!(
            excess_tao[&netuid0].to_num::<f64>(),
            tao_emission.to_num::<f64>() - tao_in[&netuid0].to_num::<f64>(),
            epsilon = 0.01
        );
    });
}

#[test]
fn test_coinbase_subnet_terms_with_alpha_in_lte_alpha_emission() {
    new_test_ext(1).execute_with(|| {
        let zero = U96F32::saturating_from_num(0);
        let netuid0 = add_dynamic_network(&U256::from(1), &U256::from(2));
        mock::setup_reserves(
            netuid0,
            TaoBalance::from(1_000_000_000_000_000_u64),
            AlphaBalance::from(1_000_000_000_000_000_u64),
        );
        // Initialize swap
        Swap::maybe_initialize_palswap(netuid0, None);

        let alpha_emission = U96F32::saturating_from_num(
            SubtensorModule::get_block_emission_for_issuance(
                SubtensorModule::get_alpha_issuance(netuid0).into(),
            )
            .unwrap_or(0),
        );
        let tao_emission = U96F32::saturating_from_num(34566756_u64);

        let price: U96F32 = U96F32::saturating_from_num(Swap::current_alpha_price(netuid0));

        let subnet_emissions = BTreeMap::from([(netuid0, tao_emission)]);

        // The injection cap is root_proportion * alpha_emission. Seed root stake so
        // the cap is large enough that raw alpha_in stays under it (no excess).
        set_full_injection_root_stake();
        let root_prop: U96F32 = SubtensorModule::root_proportion(netuid0);
        let injection_cap: U96F32 = root_prop.saturating_mul(alpha_emission);

        let (tao_in, alpha_in, alpha_out, excess_tao) =
            SubtensorModule::get_subnet_terms(&subnet_emissions);

        // Check our condition is met: raw alpha_in stays under the cap.
        assert!(tao_emission / price <= injection_cap);

        // alpha_out should be the alpha_emission, always
        assert_abs_diff_eq!(
            alpha_out[&netuid0].to_num::<f64>(),
            alpha_emission.to_num::<f64>(),
            epsilon = 0.1
        );

        // assuming alpha_in < alpha_emission
        // Then alpha_in should be tao_emission / price
        assert_abs_diff_eq!(
            alpha_in[&netuid0].to_num::<f64>(),
            tao_emission.to_num::<f64>() / price.to_num::<f64>(),
            epsilon = 0.01
        );

        // tao_in should be the tao_emission
        assert_abs_diff_eq!(
            tao_in[&netuid0].to_num::<f64>(),
            tao_emission.to_num::<f64>(),
            epsilon = 0.01
        );

        // excess_tao should be 0
        assert_abs_diff_eq!(
            excess_tao[&netuid0].to_num::<f64>(),
            tao_emission.to_num::<f64>() - tao_in[&netuid0].to_num::<f64>(),
            epsilon = 0.01
        );
    });
}

// Tests for the inject and swap are in the right order.
#[test]
fn test_coinbase_inject_and_maybe_swap_does_not_skew_reserves() {
    new_test_ext(1).execute_with(|| {
        let zero = U96F32::saturating_from_num(0);
        let netuid0 = add_dynamic_network(&U256::from(1), &U256::from(2));
        mock::setup_reserves(
            netuid0,
            TaoBalance::from(1_000_000_000_000_000_u64),
            AlphaBalance::from(1_000_000_000_000_000_u64),
        );
        // Initialize swap
        Swap::maybe_initialize_palswap(netuid0, None);

        let tao_in = BTreeMap::from([(netuid0, U96F32::saturating_from_num(123))]);
        let alpha_in = BTreeMap::from([(netuid0, U96F32::saturating_from_num(456))]);
        // We have excess TAO, so we will be swapping with it.
        let excess_tao = BTreeMap::from([(netuid0, U96F32::saturating_from_num(789100))]);

        // Run the inject and maybe swap
        let credit = SubtensorModule::mint_tao((123 + 789100).into());
        SubtensorModule::inject_and_maybe_swap(&[netuid0], &tao_in, &alpha_in, &excess_tao, credit);

        let tao_in_after = SubnetTAO::<Test>::get(netuid0);
        let alpha_in_after = SubnetAlphaIn::<Test>::get(netuid0);

        // Make sure that when we inject and swap, we do it in the right order.
        // Thereby not skewing the ratio away from the price.
        let ratio_after: U96F32 = U96F32::saturating_from_num(alpha_in_after.to_u64())
            .saturating_div(U96F32::saturating_from_num(tao_in_after.to_u64()));
        let price_after: U96F32 = U96F32::saturating_from_num(
            pallet_subtensor_swap::Pallet::<Test>::current_alpha_price(netuid0).to_num::<f64>(),
        );
        assert_abs_diff_eq!(
            ratio_after.to_num::<f64>(),
            price_after.to_num::<f64>(),
            epsilon = 1.0
        );
    });
}

#[test]
fn test_coinbase_failed_tao_materialization_does_not_activate_current_tao() {
    new_test_ext(1).execute_with(|| {
        let netuid = add_dynamic_network(&U256::from(1), &U256::from(2));
        let initial_reserve = TaoBalance::from(1_000_000_u64);
        let reservoir_tao = TaoBalance::from(100_u64);
        let current_tao = TaoBalance::from(200_u64);
        let current_alpha = AlphaBalance::from(100_u64);

        mock::setup_reserves(netuid, initial_reserve, AlphaBalance::from(1_000_000_u64));
        Swap::maybe_initialize_palswap(netuid, None);
        pallet_subtensor_swap::BalancerTaoReservoir::<Test>::insert(netuid, reservoir_tao);

        let tao_in = BTreeMap::from([(netuid, U96F32::saturating_from_num(current_tao))]);
        let alpha_in = BTreeMap::from([(netuid, U96F32::saturating_from_num(current_alpha))]);
        let excess_tao = BTreeMap::new();
        let credit = SubtensorModule::mint_tao(TaoBalance::ZERO);

        SubtensorModule::inject_and_maybe_swap(&[netuid], &tao_in, &alpha_in, &excess_tao, credit);

        assert_eq!(
            SubnetTAO::<Test>::get(netuid),
            initial_reserve.saturating_add(reservoir_tao)
        );
        assert_eq!(SubnetTaoInEmission::<Test>::get(netuid), reservoir_tao);
        assert_eq!(
            SubnetProtocolFlow::<Test>::get(netuid),
            reservoir_tao.to_u64() as i64
        );
        assert_eq!(
            pallet_subtensor_swap::BalancerTaoReservoir::<Test>::get(netuid),
            TaoBalance::ZERO
        );
    });
}

#[test]
fn test_alpha_reservoir_counts_toward_subnet_issuance_across_blocks() {
    new_test_ext(1).execute_with(|| {
        let netuid = add_dynamic_network(&U256::from(1), &U256::from(2));
        let alpha_in = AlphaBalance::from(10_000_u64);
        let alpha_out = AlphaBalance::from(20_000_u64);
        let reservoir_alpha = AlphaBalance::from(30_000_u64);

        SubnetAlphaIn::<Test>::insert(netuid, alpha_in);
        SubnetAlphaOut::<Test>::insert(netuid, alpha_out);
        pallet_subtensor_swap::BalancerAlphaReservoir::<Test>::insert(netuid, reservoir_alpha);

        let expected = alpha_in
            .saturating_add(alpha_out)
            .saturating_add(reservoir_alpha);
        assert_eq!(SubtensorModule::get_alpha_issuance(netuid), expected);

        System::set_block_number(System::block_number().saturating_add(1));

        assert_eq!(SubnetAlphaIn::<Test>::get(netuid), alpha_in);
        assert_eq!(
            pallet_subtensor_swap::BalancerAlphaReservoir::<Test>::get(netuid),
            reservoir_alpha
        );
        assert_eq!(SubtensorModule::get_alpha_issuance(netuid), expected);
    });
}

#[test]
fn test_coinbase_drain_pending_increments_blockssincelaststep() {
    new_test_ext(1).execute_with(|| {
        let zero = U96F32::saturating_from_num(0);
        let netuid0 = add_dynamic_network(&U256::from(1), &U256::from(2));

        let blocks_since_last_step_before = BlocksSinceLastStep::<Test>::get(netuid0);

        // Check that blockssincelaststep is incremented
        SubtensorModule::drain_pending(&[netuid0], 1);

        let blocks_since_last_step_after = BlocksSinceLastStep::<Test>::get(netuid0);
        assert!(blocks_since_last_step_after > blocks_since_last_step_before);
        assert_eq!(
            blocks_since_last_step_after,
            blocks_since_last_step_before + 1
        );
    });
}

#[test]
fn test_coinbase_drain_pending_resets_blockssincelaststep() {
    new_test_ext(1).execute_with(|| {
        let zero = U96F32::saturating_from_num(0);
        let netuid0 = add_dynamic_network(&U256::from(1), &U256::from(2));
        Tempo::<Test>::insert(netuid0, 100);
        LastEpochBlock::<Test>::insert(netuid0, 0);
        let block_number = 102;
        assert!(SubtensorModule::should_run_epoch(netuid0, block_number));

        let blocks_since_last_step_before = 12345678;
        BlocksSinceLastStep::<Test>::insert(netuid0, blocks_since_last_step_before);
        LastMechansimStepBlock::<Test>::insert(netuid0, 12345); // garbage value

        // Check that blockssincelaststep is reset to 0 on tempo
        SubtensorModule::drain_pending(&[netuid0], block_number);

        let blocks_since_last_step_after = BlocksSinceLastStep::<Test>::get(netuid0);
        assert_eq!(blocks_since_last_step_after, 0);
        assert_eq!(LastMechansimStepBlock::<Test>::get(netuid0), 12345);
    });
}

#[test]
fn test_coinbase_drain_pending_gets_counters_and_resets_them() {
    new_test_ext(1).execute_with(|| {
        let zero = U96F32::saturating_from_num(0);
        let netuid0 = add_dynamic_network(&U256::from(1), &U256::from(2));
        Tempo::<Test>::insert(netuid0, 100);
        LastEpochBlock::<Test>::insert(netuid0, 0);
        let block_number = 102;
        assert!(SubtensorModule::should_run_epoch(netuid0, block_number));

        let pending_server_em = AlphaBalance::from(123434534);
        let pending_validator_em = AlphaBalance::from(111111);
        let pending_root = AlphaBalance::from(12222222);
        let pending_owner_cut = AlphaBalance::from(12345678);

        PendingServerEmission::<Test>::insert(netuid0, pending_server_em);
        PendingValidatorEmission::<Test>::insert(netuid0, pending_validator_em);
        PendingRootAlphaDivs::<Test>::insert(netuid0, pending_root);
        PendingOwnerCut::<Test>::insert(netuid0, pending_owner_cut);

        let emissions_to_distribute = SubtensorModule::drain_pending(&[netuid0], block_number);
        assert_eq!(emissions_to_distribute.len(), 1);
        assert_eq!(
            emissions_to_distribute[&netuid0],
            (
                pending_server_em,
                pending_validator_em,
                pending_root,
                pending_owner_cut
            )
        );

        // Check that the pending emissions are reset
        assert_eq!(
            PendingServerEmission::<Test>::get(netuid0),
            AlphaBalance::ZERO
        );
        assert_eq!(
            PendingValidatorEmission::<Test>::get(netuid0),
            AlphaBalance::ZERO
        );
        assert_eq!(
            PendingRootAlphaDivs::<Test>::get(netuid0),
            AlphaBalance::ZERO
        );
        assert_eq!(PendingOwnerCut::<Test>::get(netuid0), AlphaBalance::ZERO);
    });
}

#[test]
fn test_coinbase_emit_to_subnets_with_no_root_sell() {
    new_test_ext(1).execute_with(|| {
        let zero = U96F32::saturating_from_num(0);
        let netuid0 = add_dynamic_network(&U256::from(1), &U256::from(2));
        // Set owner cut to ~10%
        SubnetOwnerCut::<Test>::set(u16::MAX / 10);
        mock::setup_reserves(
            netuid0,
            TaoBalance::from(1_000_000_000_000_000_u64),
            AlphaBalance::from(1_000_000_000_000_000_u64),
        );
        // Initialize swap
        Swap::maybe_initialize_palswap(netuid0, None);

        let tao_emission = U96F32::saturating_from_num(12345678);
        let subnet_emissions = BTreeMap::from([(netuid0, tao_emission)]);

        // NO root sell
        let root_sell_flag = false;

        let alpha_emission = U96F32::saturating_from_num(
            SubtensorModule::get_block_emission_for_issuance(
                SubtensorModule::get_alpha_issuance(netuid0).into(),
            )
            .unwrap_or(0),
        );
        let price: U96F32 = U96F32::saturating_from_num(Swap::current_alpha_price(netuid0));
        let (tao_in, alpha_in, alpha_out, excess_tao) =
            SubtensorModule::get_subnet_terms(&subnet_emissions);
        // Based on the price, we should have NO excess TAO
        assert!(tao_emission / price <= alpha_emission);

        // ==== Run the emit to subnets =====
        let credit = SubtensorModule::mint_tao(12345678.into());
        SubtensorModule::emit_to_subnets(&[netuid0], &subnet_emissions, credit, root_sell_flag);

        // Find the owner cut expected
        let owner_cut: U96F32 = SubtensorModule::get_float_subnet_owner_cut();
        let owner_cut_expected: U96F32 = owner_cut.saturating_mul(alpha_emission);
        log::info!("owner_cut_expected: {owner_cut_expected:?}");
        log::info!("alpha_emission: {alpha_emission:?}");
        log::info!("owner_cut: {owner_cut:?}");

        let alpha_issuance: U96F32 =
            U96F32::saturating_from_num(SubtensorModule::get_alpha_issuance(netuid0));
        let root_tao: U96F32 = U96F32::saturating_from_num(SubnetTAO::<Test>::get(NetUid::ROOT));
        let tao_weight: U96F32 = root_tao.saturating_mul(SubtensorModule::get_tao_weight());
        let root_prop: U96F32 = tao_weight
            .checked_div(tao_weight.saturating_add(alpha_issuance))
            .unwrap_or(U96F32::min_value());
        // Expect root alpha divs to be root prop * alpha emission
        let expected_root_alpha_divs: AlphaBalance = AlphaBalance::from(
            root_prop
                .saturating_mul(alpha_emission)
                .saturating_to_num::<u64>(),
        );

        // ===== Check that the pending emissions are set correctly =====
        // Owner cut is as expected
        assert_abs_diff_eq!(
            PendingOwnerCut::<Test>::get(netuid0).to_u64(),
            owner_cut_expected.saturating_to_num::<u64>(),
            epsilon = 200_u64
        );
        // NO root sell, so no root alpha divs
        assert_eq!(
            PendingRootAlphaDivs::<Test>::get(netuid0),
            AlphaBalance::ZERO
        );
        // Should be alpha_emission minus the owner cut,
        assert_abs_diff_eq!(
            PendingServerEmission::<Test>::get(netuid0).to_u64(),
            alpha_emission
                .saturating_sub(owner_cut_expected)
                .saturating_div(U96F32::saturating_from_num(2))
                .saturating_to_num::<u64>(),
            epsilon = 200_u64
        );
        // We ALWAYS deduct the root alpha divs
        assert_abs_diff_eq!(
            PendingValidatorEmission::<Test>::get(netuid0).to_u64(),
            alpha_emission
                .saturating_sub(owner_cut_expected)
                .saturating_div(U96F32::saturating_from_num(2))
                .saturating_sub(expected_root_alpha_divs.to_u64().into())
                .saturating_to_num::<u64>(),
            epsilon = 200_u64
        );
    });
}

#[test]
fn test_coinbase_emit_to_subnets_with_root_sell() {
    new_test_ext(1).execute_with(|| {
        let zero = U96F32::saturating_from_num(0);
        let netuid0 = add_dynamic_network(&U256::from(1), &U256::from(2));
        // Set owner cut to ~10%
        SubnetOwnerCut::<Test>::set(u16::MAX / 10);
        mock::setup_reserves(
            netuid0,
            TaoBalance::from(1_000_000_000_000_000_u64),
            AlphaBalance::from(1_000_000_000_000_000_u64),
        );
        // Initialize swap
        Swap::maybe_initialize_palswap(netuid0, None);

        let tao_emission = U96F32::saturating_from_num(12345678);
        let subnet_emissions = BTreeMap::from([(netuid0, tao_emission)]);

        // NO root sell
        let root_sell_flag = true;

        let alpha_emission: U96F32 = U96F32::saturating_from_num(
            SubtensorModule::get_block_emission_for_issuance(
                SubtensorModule::get_alpha_issuance(netuid0).into(),
            )
            .unwrap_or(0),
        );
        let price: U96F32 = U96F32::saturating_from_num(Swap::current_alpha_price(netuid0));
        let (tao_in, alpha_in, alpha_out, excess_tao) =
            SubtensorModule::get_subnet_terms(&subnet_emissions);
        // Based on the price, we should have NO excess TAO
        assert!(tao_emission / price <= alpha_emission);

        // ==== Run the emit to subnets =====
        let credit = SubtensorModule::mint_tao(12345678.into());
        SubtensorModule::emit_to_subnets(&[netuid0], &subnet_emissions, credit, root_sell_flag);

        // Find the owner cut expected
        let owner_cut: U96F32 = SubtensorModule::get_float_subnet_owner_cut();
        let owner_cut_expected: U96F32 = owner_cut.saturating_mul(alpha_emission);
        log::info!("owner_cut_expected: {owner_cut_expected:?}");
        log::info!("alpha_emission: {alpha_emission:?}");
        log::info!("owner_cut: {owner_cut:?}");

        let alpha_issuance: U96F32 =
            U96F32::saturating_from_num(SubtensorModule::get_alpha_issuance(netuid0));
        let root_tao: U96F32 = U96F32::saturating_from_num(SubnetTAO::<Test>::get(NetUid::ROOT));
        let tao_weight: U96F32 = root_tao.saturating_mul(SubtensorModule::get_tao_weight());
        let root_prop: U96F32 = tao_weight
            .checked_div(tao_weight.saturating_add(alpha_issuance))
            .unwrap_or(U96F32::min_value());
        // Expect root alpha divs to be root prop * alpha emission
        let expected_root_alpha_divs: AlphaBalance = AlphaBalance::from(
            root_prop
                .saturating_mul(alpha_emission)
                .saturating_to_num::<u64>(),
        );

        // ===== Check that the pending emissions are set correctly =====
        // Owner cut is as expected
        assert_abs_diff_eq!(
            PendingOwnerCut::<Test>::get(netuid0).to_u64(),
            owner_cut_expected.saturating_to_num::<u64>(),
            epsilon = 200_u64
        );
        // YES root sell, so we have root alpha divs
        assert_abs_diff_eq!(
            PendingRootAlphaDivs::<Test>::get(netuid0).to_u64(),
            expected_root_alpha_divs.to_u64(),
            epsilon = 200_u64
        );
        // Should be alpha_emission minus the owner cut
        assert_abs_diff_eq!(
            PendingServerEmission::<Test>::get(netuid0).to_u64(),
            alpha_emission
                .saturating_sub(owner_cut_expected)
                .saturating_div(U96F32::saturating_from_num(2))
                .saturating_to_num::<u64>(),
            epsilon = 200_u64
        );
        // Validator emission is also minus root alpha divs
        assert_abs_diff_eq!(
            PendingValidatorEmission::<Test>::get(netuid0).to_u64(),
            alpha_emission
                .saturating_sub(owner_cut_expected)
                .saturating_div(U96F32::saturating_from_num(2))
                .saturating_sub(expected_root_alpha_divs.to_u64().into())
                .saturating_to_num::<u64>(),
            epsilon = 200_u64
        );
    });
}

// cargo test --package pallet-subtensor --lib -- tests::coinbase::test_disabling_owner_cut_sends_subnet_emission_to_miners_and_validators --exact --nocapture
#[test]
fn test_disabling_owner_cut_sends_subnet_emission_to_miners_and_validators() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let validator_coldkey = U256::from(1);
        let validator_hotkey = U256::from(2);
        let miner_coldkey = U256::from(5);
        let miner_hotkey = U256::from(6);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        LastEpochBlock::<Test>::insert(netuid, SubtensorModule::get_current_block_as_u64());
        let subnet_tempo = 10;
        let stake = 100_000_000_000u64;

        SubtensorModule::set_tempo_unchecked(netuid, subnet_tempo);
        setup_reserves(netuid, (stake * 10_000).into(), (stake * 10_000).into());

        register_ok_neuron(netuid, validator_hotkey, validator_coldkey, 0);
        register_ok_neuron(netuid, miner_hotkey, miner_coldkey, 1);

        add_balance_to_coldkey_account(
            &validator_coldkey,
            TaoBalance::from(stake) + ExistentialDeposit::get(),
        );

        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(validator_coldkey),
            validator_hotkey,
            netuid,
            stake.into()
        ));

        SubtensorModule::set_weights_set_rate_limit(netuid, 0);
        SubtensorModule::set_max_allowed_validators(netuid, 1);
        step_block(subnet_tempo);

        SubnetOwnerCut::<Test>::set(u16::MAX / 10);
        SubtensorModule::set_owner_cut_enabled_flag(netuid, false);

        let owner_uid =
            SubtensorModule::get_uid_for_net_and_hotkey(netuid, &subnet_owner_hotkey).unwrap();
        let validator_uid =
            SubtensorModule::get_uid_for_net_and_hotkey(netuid, &validator_hotkey).unwrap();
        let miner_uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &miner_hotkey).unwrap();
        let uid_count = [
            owner_uid as usize,
            validator_uid as usize,
            miner_uid as usize,
        ]
        .into_iter()
        .max()
        .unwrap()
            + 1;

        Weights::<Test>::insert(
            NetUidStorageIndex::from(netuid),
            validator_uid,
            vec![(miner_uid, 0xFFFF)],
        );
        BlockAtRegistration::<Test>::set(netuid, owner_uid, 1);
        BlockAtRegistration::<Test>::set(netuid, validator_uid, 1);
        BlockAtRegistration::<Test>::set(netuid, miner_uid, 1);
        LastUpdate::<Test>::set(NetUidStorageIndex::from(netuid), vec![2; uid_count]);
        Kappa::<Test>::set(netuid, u16::MAX / 5);
        ActivityCutoff::<Test>::set(netuid, u16::MAX);
        let mut validator_permit = vec![false; uid_count];
        validator_permit[validator_uid as usize] = true;
        ValidatorPermit::<Test>::insert(netuid, validator_permit);

        let owner_stake_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &subnet_owner_hotkey,
            &subnet_owner_coldkey,
            netuid,
        );
        let validator_stake_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &validator_hotkey,
            &validator_coldkey,
            netuid,
        );
        let miner_stake_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &miner_hotkey,
            &miner_coldkey,
            netuid,
        );

        // Disabling owner cut removes the subnet owner from emission distribution, so the
        // subnet emission is fully distributed across the validator and miner paths instead.
        step_block(subnet_tempo);

        let owner_stake_after = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &subnet_owner_hotkey,
            &subnet_owner_coldkey,
            netuid,
        );
        let validator_stake_after = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &validator_hotkey,
            &validator_coldkey,
            netuid,
        );
        let miner_stake_after = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &miner_hotkey,
            &miner_coldkey,
            netuid,
        );

        assert_eq!(owner_stake_after, owner_stake_before);
        assert!(validator_stake_after > validator_stake_before);
        assert!(miner_stake_after > miner_stake_before);
        assert_eq!(PendingOwnerCut::<Test>::get(netuid), AlphaBalance::ZERO);
        assert!(
            Lock::<Test>::iter_prefix((subnet_owner_coldkey, netuid))
                .next()
                .is_none()
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_pending_emission_start_call_not_done --exact --show-output --nocapture
#[test]
fn test_pending_emission_start_call_not_done() {
    new_test_ext(1).execute_with(|| {
        let validator_coldkey = U256::from(1);
        let validator_hotkey = U256::from(2);
        let subnet_tempo = 10;
        let stake: u64 = 100_000_000_000;
        let root_stake: u64 = 200_000_000_000; // 200 TAO

        // Create root network
        NetworksAdded::<Test>::insert(NetUid::ROOT, true);
        // enabled root
        SubtokenEnabled::<Test>::insert(NetUid::ROOT, true);

        // Add network, register hotkeys, and setup network parameters
        let owner_hotkey = U256::from(10);
        let owner_coldkey = U256::from(11);
        let netuid = add_dynamic_network(&owner_hotkey, &owner_coldkey);
        // Remove FirstEmissionBlockNumber
        FirstEmissionBlockNumber::<Test>::remove(netuid);
        Tempo::<Test>::insert(netuid, subnet_tempo);

        register_ok_neuron(netuid, validator_hotkey, validator_coldkey, 0);
        add_balance_to_coldkey_account(
            &validator_coldkey,
            TaoBalance::from(stake) + ExistentialDeposit::get(),
        );
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);
        step_block(subnet_tempo);
        SubnetOwnerCut::<Test>::set(u16::MAX / 10);
        // There are two validators and three neurons
        MaxAllowedUids::<Test>::set(netuid, 3);
        SubtensorModule::set_max_allowed_validators(netuid, 2);

        // Add stake to validator so it has root stake
        add_balance_to_coldkey_account(&validator_coldkey, root_stake.into());
        // init root
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(validator_coldkey),
            validator_hotkey,
            NetUid::ROOT,
            root_stake.into()
        ));
        // Set tao weight non zero
        SubtensorModule::set_tao_weight(u64::MAX / 10);

        // Make root sell happen
        // Set moving price > 1.0
        // Set price > 1.0
        let tao = TaoBalance::from(10_000_000_000_u64);
        let alpha = AlphaBalance::from(1_000_000_000_u64);
        SubnetTAO::<Test>::insert(netuid, tao);
        SubnetAlphaIn::<Test>::insert(netuid, alpha);

        SubnetMovingPrice::<Test>::insert(netuid, I96F32::from_num(2));

        // Make sure we are root selling, so we have root alpha divs.
        let root_sell_flag = SubtensorModule::get_network_root_sell_flag(&[netuid]);
        assert!(root_sell_flag, "Root sell flag should be true");

        // !!! Check that the subnet FirstEmissionBlockNumber is None -- no entry
        assert!(FirstEmissionBlockNumber::<Test>::get(netuid).is_none());

        // Run run_coinbase until emissions are accumulated
        step_block(subnet_tempo - 2);

        // Verify that all pending emissions are zero
        assert_eq!(
            PendingServerEmission::<Test>::get(netuid),
            AlphaBalance::ZERO
        );
        assert_eq!(
            PendingValidatorEmission::<Test>::get(netuid),
            AlphaBalance::ZERO
        );
        assert_eq!(
            PendingRootAlphaDivs::<Test>::get(netuid),
            AlphaBalance::ZERO
        );
    });
}

#[test]
fn test_root_prop_filled_on_block_step() {
    new_test_ext(1).execute_with(|| {
        let hotkey = U256::from(10);
        let coldkey = U256::from(11);
        let netuid1 = add_dynamic_network(&hotkey, &coldkey);
        let netuid2 = add_dynamic_network(&hotkey, &coldkey);

        SubnetTAO::<Test>::insert(NetUid::ROOT, TaoBalance::from(1_000_000_000_000u64));
        SubtensorModule::set_tao_weight(u64::MAX); // Set TAO weight to 1.0

        let tao_reserve = TaoBalance::from(50_000_000_000_u64);
        let alpha_in = AlphaBalance::from(100_000_000_000_u64);
        SubnetTAO::<Test>::insert(netuid1, tao_reserve);
        SubnetAlphaIn::<Test>::insert(netuid1, alpha_in);
        SubnetTAO::<Test>::insert(netuid2, tao_reserve);
        SubnetAlphaIn::<Test>::insert(netuid2, alpha_in);

        assert!(!RootProp::<Test>::contains_key(netuid1));
        assert!(!RootProp::<Test>::contains_key(netuid2));

        run_to_block(2);

        assert!(RootProp::<Test>::get(netuid1) > U96F32::from_num(0));
        assert!(RootProp::<Test>::get(netuid2) > U96F32::from_num(0));
    });
}

#[test]
fn test_root_proportion() {
    new_test_ext(1).execute_with(|| {
        let hotkey = U256::from(10);
        let coldkey = U256::from(11);
        let netuid = add_dynamic_network(&hotkey, &coldkey);

        let root_tao_reserve = 1_000_000_000_000u64;
        SubnetTAO::<Test>::insert(NetUid::ROOT, TaoBalance::from(root_tao_reserve));

        let tao_weight = 3_320_413_933_267_719_290u64;
        SubtensorModule::set_tao_weight(tao_weight);

        let alpha_in = 100_000_000_000u64;
        SubnetAlphaIn::<Test>::insert(netuid, AlphaBalance::from(alpha_in));

        let actual_root_proportion = SubtensorModule::root_proportion(netuid);
        let expected_root_prop = {
            let tao_weight = SubtensorModule::get_tao_weight();
            let root_tao = U96F32::from_num(root_tao_reserve);
            let alpha_in = {
                let alpha: u64 = SubtensorModule::get_alpha_issuance(netuid).into();

                U96F32::from_num(alpha)
            };

            tao_weight * root_tao / (tao_weight * root_tao + alpha_in)
        };

        assert_eq!(actual_root_proportion, expected_root_prop);
    });
}

#[test]
fn test_get_subnet_terms_alpha_emissions_cap() {
    new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(10);
        let owner_coldkey = U256::from(11);
        let netuid = add_dynamic_network(&owner_hotkey, &owner_coldkey);

        // The injection cap is now root_proportion * alpha_emission. Seed root stake
        // so root_proportion is well-defined, and derive the cap from the live values.
        set_full_injection_root_stake();
        let alpha_emission_i: U96F32 = U96F32::saturating_from_num(
            SubtensorModule::get_block_emission_for_issuance(
                SubtensorModule::get_alpha_issuance(netuid).into(),
            )
            .unwrap_or(0),
        );
        let injection_cap: U96F32 =
            SubtensorModule::root_proportion(netuid).saturating_mul(alpha_emission_i);

        // price = 1.0, alpha_in_i (== emissions1) <= alpha_injection_cap (not capped)
        let emissions1 = U96F32::from_num(100_000_000);
        assert!(emissions1 < injection_cap);

        let subnet_emissions1 = BTreeMap::from([(netuid, emissions1)]);
        let (_, alpha_in, _, _) = SubtensorModule::get_subnet_terms(&subnet_emissions1);

        assert_eq!(alpha_in.get(&netuid).copied().unwrap(), emissions1);

        // price = 1.0, alpha_in_i (== emissions2) > alpha_injection_cap (capped)
        let emissions2 = U96F32::from_num(10_000_000_000u64);
        assert!(emissions2 > injection_cap);

        let subnet_emissions2 = BTreeMap::from([(netuid, emissions2)]);
        let (_, alpha_in, _, _) = SubtensorModule::get_subnet_terms(&subnet_emissions2);

        assert_eq!(alpha_in.get(&netuid).copied().unwrap(), injection_cap);
    });
}

#[test]
fn test_epochs_deferred_this_block_respects_cap() {
    new_test_ext(1).execute_with(|| {
        let cap = SubtensorModule::get_max_epochs_per_block() as usize;
        let n = cap + 2;

        for i in 0..n {
            let netuid = NetUid::from((i + 1) as u16);
            add_network(netuid, 100, 0);
            // Force "due this block".
            PendingEpochAt::<Test>::insert(netuid, 1);
        }

        let block = SubtensorModule::get_current_block_as_u64();
        let subnets: Vec<NetUid> = SubtensorModule::get_all_subnet_netuids()
            .into_iter()
            .filter(|x| *x != NetUid::ROOT)
            .collect();

        // All `n` subnets are due, but only `cap` may fire — the rest are deferred.
        let deferred = SubtensorModule::epochs_deferred_this_block(&subnets, block);
        assert_eq!(
            deferred.len(),
            n - cap,
            "exactly the due subnets beyond MaxEpochsPerBlock are deferred"
        );
        for netuid in &deferred {
            assert!(SubtensorModule::should_run_epoch(*netuid, block));
        }
    });
}

// Regression test for the dynamic-tempo / CR-v3 interaction: when a subnet's epoch
// is deferred by the per-block cap, its timelock reveal must be held back to the
// deferred fire-block (not run on the originally-scheduled block, which would
// surface weights before the epoch consumes them).
//
// Crypto-free probe: the reveal path removes *expired* commits only when it runs
// for a subnet, so a retained expired (epoch-0) commit means the reveal was skipped.
#[test]
fn test_reveal_crv3_defers_with_capped_epoch() {
    new_test_ext(1).execute_with(|| {
        let cap = SubtensorModule::get_max_epochs_per_block() as usize;
        let n = cap + 2;
        let mec0 = subtensor_runtime_common::MechId::from(0);

        for i in 0..n {
            let netuid = NetUid::from((i + 1) as u16);
            add_network(netuid, 100, 0);
            PendingEpochAt::<Test>::insert(netuid, 1); // due this block
            SubnetEpochIndex::<Test>::insert(netuid, 10); // cur_epoch >> reveal_period
            // Plant an expired commit at epoch 0 (field types inferred from the queue).
            let idx = SubtensorModule::get_mechanism_storage_index(netuid, mec0);
            TimelockedWeightCommits::<Test>::mutate(idx, 0u64, |q| {
                q.push_back((U256::from(1u64), 0u64, Default::default(), 0u64));
            });
        }

        let subnets: Vec<NetUid> = SubtensorModule::get_all_subnet_netuids()
            .into_iter()
            .filter(|x| *x != NetUid::ROOT)
            .collect();

        let still_holds = |netuid: NetUid| -> bool {
            let idx = SubtensorModule::get_mechanism_storage_index(netuid, mec0);
            TimelockedWeightCommits::<Test>::contains_key(idx, 0u64)
        };
        let retained = |subnets: &[NetUid]| subnets.iter().filter(|n| still_holds(**n)).count();

        // --- Phase 1: cap-deferred subnets must NOT reveal this block.
        SubtensorModule::reveal_crv3_commits();
        assert_eq!(
            retained(&subnets),
            n - cap,
            "only cap-deferred subnets keep their commit (their reveal was skipped)"
        );

        let deferred: Vec<NetUid> = subnets
            .iter()
            .copied()
            .filter(|n| still_holds(*n))
            .collect();

        // --- Phase 2: drop the cap pressure so only the deferred subnets are due;
        // they should now reveal (and clean their expired commit).
        for netuid in &subnets {
            if !deferred.contains(netuid) {
                PendingEpochAt::<Test>::insert(*netuid, 0);
                LastEpochBlock::<Test>::insert(*netuid, 1); // blocks_since < tempo => not due
            }
        }
        SubtensorModule::reveal_crv3_commits();
        assert_eq!(
            retained(&subnets),
            0,
            "deferred subnets reveal once they actually fire"
        );
    });
}
