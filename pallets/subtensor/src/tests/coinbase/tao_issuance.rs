#![allow(
    unused,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used
)]
//! TAO issuance and subnet emission-enable redistribution.

use super::helpers::*;
use super::prelude::*;

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::tao_issuance::test_hotkey_take --exact --show-output --nocapture
#[test]
fn test_hotkey_take() {
    new_test_ext(1).execute_with(|| {
        let hotkey = U256::from(1);
        Delegates::<Test>::insert(hotkey, PerU16::from_parts(u16::MAX / 2));
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::tao_issuance::test_coinbase_basecase --exact --show-output --nocapture
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::tao_issuance::test_coinbase_tao_issuance_base --exact --show-output --nocapture
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

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::tao_issuance::test_coinbase_tao_issuance_base_low --exact --show-output --nocapture
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

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::tao_issuance::test_coinbase_tao_issuance_base_low_flow --exact --show-output --nocapture
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::tao_issuance::test_coinbase_tao_issuance_multiple --exact --show-output --nocapture
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
            SubtensorModule::compute_subnet_emission_terms(&subnet_emissions);
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::tao_issuance::test_coinbase_tao_issuance_different_prices --exact --show-output --nocapture
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
