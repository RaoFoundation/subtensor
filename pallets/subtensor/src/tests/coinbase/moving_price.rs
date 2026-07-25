#![allow(
    unused,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used
)]
//! Subnet moving-price updates during coinbase.

use super::helpers::*;
use super::prelude::*;

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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::moving_price::test_coinbase_moving_prices --exact --show-output --nocapture
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::moving_price::test_update_moving_price_initial --exact --show-output --nocapture
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
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::moving_price::test_update_moving_price_after_time --exact --show-output --nocapture
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
