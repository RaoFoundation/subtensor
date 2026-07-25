#![allow(clippy::unwrap_used)]
#![allow(clippy::arithmetic_side_effects)]
//! Tests for [`crate::staking::move_stake`] max-amount / limit partial paths.

use approx::assert_abs_diff_eq;
use frame_support::assert_ok;
use sp_core::U256;
use substrate_fixed::types::U96F32;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};
use subtensor_swap_interface::SwapHandler;

use super::super::mock::*;
use crate::*;

// cargo test --package pallet-subtensor --lib -- tests::staking::move_stake::test_max_amount_move_root_root --exact --show-output
#[test]
fn test_max_amount_move_root_root() {
    new_test_ext(0).execute_with(|| {
        // 0 price on (root, root) exchange => max is u64::MAX
        assert_eq!(
            SubtensorModule::get_max_amount_move(NetUid::ROOT, NetUid::ROOT, TaoBalance::ZERO),
            Ok(AlphaBalance::MAX)
        );

        // 0.5 price on (root, root) => max is u64::MAX
        assert_eq!(
            SubtensorModule::get_max_amount_move(
                NetUid::ROOT,
                NetUid::ROOT,
                TaoBalance::from(500_000_000)
            ),
            Ok(AlphaBalance::MAX)
        );

        // 0.999999... price on (root, root) => max is u64::MAX
        assert_eq!(
            SubtensorModule::get_max_amount_move(
                NetUid::ROOT,
                NetUid::ROOT,
                TaoBalance::from(999_999_999)
            ),
            Ok(AlphaBalance::MAX)
        );

        // 1.0 price on (root, root) => max is u64::MAX
        assert_eq!(
            SubtensorModule::get_max_amount_move(
                NetUid::ROOT,
                NetUid::ROOT,
                TaoBalance::from(1_000_000_000)
            ),
            Ok(AlphaBalance::MAX)
        );

        // 1.000...001 price on (root, root) => max is 0
        assert_eq!(
            SubtensorModule::get_max_amount_move(
                NetUid::ROOT,
                NetUid::ROOT,
                TaoBalance::from(1_000_000_001)
            ),
            Ok(0u64.into())
        );

        // 2.0 price on (root, root) => max is 0
        assert_eq!(
            SubtensorModule::get_max_amount_move(
                NetUid::ROOT,
                NetUid::ROOT,
                TaoBalance::from(2_000_000_000)
            ),
            Ok(0u64.into())
        );
    });
}

// cargo test --package pallet-subtensor --lib -- tests::staking::move_stake::test_max_amount_move_root_stable --exact --show-output
#[test]
fn test_max_amount_move_root_stable() {
    new_test_ext(0).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);

        // 0 price on (root, stable) exchange => max is u64::MAX
        assert_eq!(
            SubtensorModule::get_max_amount_move(NetUid::ROOT, netuid, TaoBalance::ZERO),
            Ok(AlphaBalance::MAX)
        );

        // 0.5 price on (root, stable) => max is u64::MAX
        assert_eq!(
            SubtensorModule::get_max_amount_move(
                NetUid::ROOT,
                netuid,
                TaoBalance::from(500_000_000)
            ),
            Ok(AlphaBalance::MAX)
        );

        // 0.999999... price on (root, stable) => max is u64::MAX
        assert_eq!(
            SubtensorModule::get_max_amount_move(
                NetUid::ROOT,
                netuid,
                TaoBalance::from(999_999_999)
            ),
            Ok(AlphaBalance::MAX)
        );

        // 1.0 price on (root, stable) => max is u64::MAX
        assert_eq!(
            SubtensorModule::get_max_amount_move(
                NetUid::ROOT,
                netuid,
                TaoBalance::from(1_000_000_000)
            ),
            Ok(AlphaBalance::MAX)
        );

        // 1.000...001 price on (root, stable) => max is 0
        assert_eq!(
            SubtensorModule::get_max_amount_move(
                NetUid::ROOT,
                netuid,
                TaoBalance::from(1_000_000_001)
            ),
            Ok(0u64.into())
        );

        // 2.0 price on (root, stable) => max is 0
        assert_eq!(
            SubtensorModule::get_max_amount_move(
                NetUid::ROOT,
                netuid,
                TaoBalance::from(2_000_000_000)
            ),
            Ok(0u64.into())
        );
    });
}

// cargo test --package pallet-subtensor --lib -- tests::staking::move_stake::test_max_amount_move_stable_dynamic --exact --show-output
#[test]
fn test_max_amount_move_stable_dynamic() {
    new_test_ext(0).execute_with(|| {
        // Add stable subnet
        let stable_netuid = NetUid::from(1);
        add_network(stable_netuid, 1, 0);

        // Add dynamic subnet
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let dynamic_netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        // Force-set alpha in and tao reserve to make price equal 0.5
        let tao_reserve = TaoBalance::from(50_000_000_000_u64);
        let alpha_in = AlphaBalance::from(100_000_000_000_u64);
        SubnetTAO::<Test>::insert(dynamic_netuid, tao_reserve);
        SubnetAlphaIn::<Test>::insert(dynamic_netuid, alpha_in);
        let current_price =
            <Test as pallet::Config>::SwapInterface::current_alpha_price(dynamic_netuid.into());
        assert_eq!(current_price, U96F32::from_num(0.5));

        // The tests below just mimic the add_stake_limit tests for reverted price

        // 0 price => max is u64::MAX
        assert_eq!(
            SubtensorModule::get_max_amount_move(stable_netuid, dynamic_netuid, TaoBalance::ZERO),
            Ok(AlphaBalance::MAX)
        );

        // 2.0 price => max is 0
        assert_eq!(
            SubtensorModule::get_max_amount_move(
                stable_netuid,
                dynamic_netuid,
                TaoBalance::from(2_000_000_000)
            ),
            Err(pallet_subtensor_swap::Error::<Test>::PriceLimitExceeded.into())
        );

        // 3.0 price => max is 0
        assert_eq!(
            SubtensorModule::get_max_amount_move(
                stable_netuid,
                dynamic_netuid,
                TaoBalance::from(3_000_000_000_u64)
            ),
            Err(pallet_subtensor_swap::Error::<Test>::PriceLimitExceeded.into())
        );

        // 2x price => max is 1x TAO
        assert_abs_diff_eq!(
            SubtensorModule::get_max_amount_move(
                stable_netuid,
                dynamic_netuid,
                TaoBalance::from(500_000_000)
            )
            .unwrap(),
            AlphaBalance::from(tao_reserve.to_u64() + (tao_reserve.to_u64() as f64 * 0.003) as u64),
            epsilon = AlphaBalance::from(tao_reserve.to_u64() / 100),
        );

        // Precision test:
        // 1.99999..9000 price => max > 0
        assert!(
            SubtensorModule::get_max_amount_move(
                stable_netuid,
                dynamic_netuid,
                TaoBalance::from(1_999_999_000)
            )
            .unwrap()
                > AlphaBalance::ZERO
        );

        // Max price doesn't panic and returns something meaningful
        assert_eq!(
            SubtensorModule::get_max_amount_move(stable_netuid, dynamic_netuid, TaoBalance::MAX),
            Err(pallet_subtensor_swap::Error::<Test>::PriceLimitExceeded.into())
        );
        assert_eq!(
            SubtensorModule::get_max_amount_move(
                stable_netuid,
                dynamic_netuid,
                TaoBalance::MAX - 1.into()
            ),
            Err(pallet_subtensor_swap::Error::<Test>::PriceLimitExceeded.into())
        );
        assert_eq!(
            SubtensorModule::get_max_amount_move(
                stable_netuid,
                dynamic_netuid,
                TaoBalance::MAX / 2.into()
            ),
            Err(pallet_subtensor_swap::Error::<Test>::PriceLimitExceeded.into())
        );
    });
}

// cargo test --package pallet-subtensor --lib -- tests::staking::move_stake::test_max_amount_move_dynamic_stable --exact --show-output
#[test]
fn test_max_amount_move_dynamic_stable() {
    new_test_ext(0).execute_with(|| {
        // Add stable subnet
        let stable_netuid = NetUid::from(1);
        add_network(stable_netuid, 1, 0);

        // Add dynamic subnet
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let dynamic_netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        // Forse-set alpha in and tao reserve to make price equal 1.5
        let tao_reserve = TaoBalance::from(150_000_000_000_u64);
        let alpha_in = AlphaBalance::from(100_000_000_000_u64);
        SubnetTAO::<Test>::insert(dynamic_netuid, tao_reserve);
        SubnetAlphaIn::<Test>::insert(dynamic_netuid, alpha_in);
        let current_price =
            <Test as pallet::Config>::SwapInterface::current_alpha_price(dynamic_netuid.into());
        assert_eq!(current_price, U96F32::from_num(1.5));

        // The tests below just mimic the remove_stake_limit tests

        // 0 price => max is capped at 1000x input reserve
        assert_eq!(
            SubtensorModule::get_max_amount_move(dynamic_netuid, stable_netuid, TaoBalance::ZERO),
            Ok(alpha_in.saturating_mul(1_000.into()))
        );

        // Low price values don't blow things up
        assert!(
            SubtensorModule::get_max_amount_move(dynamic_netuid, stable_netuid, 1.into()).unwrap()
                > AlphaBalance::ZERO
        );
        assert!(
            SubtensorModule::get_max_amount_move(dynamic_netuid, stable_netuid, 2.into()).unwrap()
                > AlphaBalance::ZERO
        );
        assert!(
            SubtensorModule::get_max_amount_move(dynamic_netuid, stable_netuid, 3.into()).unwrap()
                > AlphaBalance::ZERO
        );

        // 1.5000...1 price => max is 0
        assert_eq!(
            SubtensorModule::get_max_amount_move(
                dynamic_netuid,
                stable_netuid,
                1_500_000_001.into()
            ),
            Err(pallet_subtensor_swap::Error::<Test>::PriceLimitExceeded.into())
        );

        // 1.5 price => max is 0 because of non-zero slippage
        assert_abs_diff_eq!(
            SubtensorModule::get_max_amount_move(
                dynamic_netuid,
                stable_netuid,
                1_500_000_000.into()
            )
            .unwrap_or(AlphaBalance::ZERO),
            AlphaBalance::ZERO,
            epsilon = 10_000.into()
        );

        // 1/4 price => max is 1x Alpha
        assert_abs_diff_eq!(
            SubtensorModule::get_max_amount_move(dynamic_netuid, stable_netuid, 375_000_000.into())
                .unwrap(),
            alpha_in + alpha_in / 2000.into(), // + 0.05% fee
            epsilon = alpha_in / 10_000.into(),
        );

        // Precision test:
        // 1.499999.. price => max > 0
        assert!(
            SubtensorModule::get_max_amount_move(
                dynamic_netuid,
                stable_netuid,
                1_499_999_999.into()
            )
            .unwrap()
                > AlphaBalance::ZERO
        );

        // Max price doesn't panic and returns something meaningful
        assert!(
            SubtensorModule::get_max_amount_move(dynamic_netuid, stable_netuid, TaoBalance::MAX)
                .unwrap_or(AlphaBalance::ZERO)
                < 21_000_000_000_000_000_u64.into()
        );
        assert!(
            SubtensorModule::get_max_amount_move(
                dynamic_netuid,
                stable_netuid,
                TaoBalance::MAX - 1.into()
            )
            .unwrap_or(AlphaBalance::ZERO)
                < 21_000_000_000_000_000_u64.into()
        );
        assert!(
            SubtensorModule::get_max_amount_move(
                dynamic_netuid,
                stable_netuid,
                TaoBalance::MAX / 2.into()
            )
            .unwrap_or(AlphaBalance::ZERO)
                < 21_000_000_000_000_000_u64.into()
        );
    });
}

// cargo test --package pallet-subtensor --lib -- tests::staking::move_stake::test_max_amount_move_dynamic_dynamic --exact --show-output
#[test]
fn test_max_amount_move_dynamic_dynamic() {
    new_test_ext(0).execute_with(|| {
        // Add two dynamic subnets
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let origin_netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let destination_netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        // Test cases are generated with help with this limit-staking calculator:
        // https://docs.google.com/spreadsheets/d/1pfU-PVycd3I4DbJIc0GjtPohy4CbhdV6CWqgiy__jKE
        // This is for reference only; verify before use.
        //
        // CSV backup for this spreadhsheet:
        //
        // SubnetTAO 1,AlphaIn 1,SubnetTAO 2,AlphaIn 2,,initial price,limit price,max swappable
        // 150,100,100,100,,=(A2/B2)/(C2/D2),0.1,=(D2*A2-B2*C2*G2)/(G2*(A2+C2))
        //
        // tao_in_1, alpha_in_1, tao_in_2, alpha_in_2, limit_price, expected_max_swappable, precision
        [
            // Zero handling (no panics)
            (
                0_u64,
                1_000_000_000_u64,
                1_000_000_000_u64,
                1_000_000_000_u64,
                100,
                0,
                1_u64,
            ),
            (1_000_000_000, 0, 1_000_000_000, 1_000_000_000, 100, 0, 1),
            (1_000_000_000, 1_000_000_000, 0, 1_000_000_000, 100, 0, 1),
            (1_000_000_000, 1_000_000_000, 1_000_000_000, 0, 100, 0, 1),
            // Low bounds
            (1, 1, 1, 1, 0, u64::MAX, 1),
            (1, 1, 1, 1, 1, 500_000_000, 1),
            (1, 1, 1, 1, 2, 250_000_000, 1),
            (1, 1, 1, 1, 3, 166_666_666, 1),
            (1, 1, 1, 1, 4, 125_000_000, 1),
            (1, 1, 1, 1, 1_000, 500_000, 1),
            // Basic math
            (1_000, 1_000, 1_000, 1_000, 500_000_000, 500, 1),
            (1_000, 1_000, 1_000, 1_000, 100_000_000, 4_500, 1),
            // Normal range values edge cases
            (
                150_000_000_000,
                100_000_000_000,
                100_000_000_000,
                100_000_000_000,
                100_000_000,
                560_000_000_000,
                1_000_000,
            ),
            (
                150_000_000_000,
                100_000_000_000,
                100_000_000_000,
                100_000_000_000,
                500_000_000,
                80_000_000_000,
                1_000_000,
            ),
            (
                150_000_000_000,
                100_000_000_000,
                100_000_000_000,
                100_000_000_000,
                750_000_000,
                40_000_000_000,
                1_000_000,
            ),
            (
                150_000_000_000,
                100_000_000_000,
                100_000_000_000,
                100_000_000_000,
                1_000_000_000,
                20_000_000_000,
                1_000,
            ),
            (
                150_000_000_000,
                100_000_000_000,
                100_000_000_000,
                100_000_000_000,
                1_250_000_000,
                8_000_000_000,
                1_000,
            ),
            (
                150_000_000_000,
                100_000_000_000,
                100_000_000_000,
                100_000_000_000,
                1_499_999_999,
                27,
                1,
            ),
            (
                150_000_000_000,
                100_000_000_000,
                100_000_000_000,
                100_000_000_000,
                1_500_000_000,
                0,
                1,
            ),
            (
                150_000_000_000,
                100_000_000_000,
                100_000_000_000,
                100_000_000_000,
                1_500_000_001,
                0,
                1,
            ),
            (
                150_000_000_000,
                100_000_000_000,
                100_000_000_000,
                100_000_000_000,
                1_500_001_000,
                0,
                1,
            ),
            (
                150_000_000_000,
                100_000_000_000,
                100_000_000_000,
                100_000_000_000,
                2_000_000_000,
                0,
                1,
            ),
            (
                150_000_000_000,
                100_000_000_000,
                100_000_000_000,
                100_000_000_000,
                u64::MAX,
                0,
                1,
            ),
            (
                100_000_000_000,
                200_000_000_000,
                300_000_000_000,
                400_000_000_000,
                500_000_000,
                50_000_000_000,
                1_000,
            ),
            // Miscellaneous overflows
            (
                1_000_000_000,
                1_000_000_000,
                1_000_000_000,
                1_000_000_000,
                1,
                499_999_999_500_000_000,
                100_000_000,
            ),
            (
                1_000_000,
                1_000_000,
                21_000_000_000_000_000,
                1_000_000_000_000_000_000_u64,
                1,
                48_000_000_000_000_000,
                1_000_000_000_000_000,
            ),
            (
                150_000_000_000,
                100_000_000_000,
                100_000_000_000,
                100_000_000_000,
                u64::MAX,
                0,
                1,
            ),
            (
                1_000_000,
                1_000_000,
                21_000_000_000_000_000,
                1_000_000_000_000_000_000_u64,
                u64::MAX,
                0,
                1,
            ),
        ]
        .iter()
        .for_each(
            |&(
                tao_in_1,
                alpha_in_1,
                tao_in_2,
                alpha_in_2,
                limit_price,
                expected_max_swappable,
                precision,
            )| {
                let expected_max_swappable = AlphaBalance::from(expected_max_swappable);
                // Forse-set alpha in and tao reserve to achieve relative price of subnets
                SubnetTAO::<Test>::insert(origin_netuid, TaoBalance::from(tao_in_1));
                SubnetAlphaIn::<Test>::insert(origin_netuid, AlphaBalance::from(alpha_in_1));
                SubnetTAO::<Test>::insert(destination_netuid, TaoBalance::from(tao_in_2));
                SubnetAlphaIn::<Test>::insert(destination_netuid, AlphaBalance::from(alpha_in_2));

                if !alpha_in_1.is_zero() && !alpha_in_2.is_zero() {
                    let origin_price = tao_in_1 as f64 / alpha_in_1 as f64;
                    let dest_price = tao_in_2 as f64 / alpha_in_2 as f64;
                    if dest_price != 0. {
                        let expected_price = origin_price / dest_price;
                        assert_abs_diff_eq!(
                            (<Test as pallet::Config>::SwapInterface::current_alpha_price(
                                origin_netuid.into()
                            ) / <Test as pallet::Config>::SwapInterface::current_alpha_price(
                                destination_netuid.into()
                            ))
                            .to_num::<f64>(),
                            expected_price,
                            epsilon = 0.000_000_001
                        );
                    }
                }

                assert_abs_diff_eq!(
                    SubtensorModule::get_max_amount_move(
                        origin_netuid,
                        destination_netuid,
                        limit_price.into()
                    )
                    .unwrap_or(AlphaBalance::ZERO),
                    expected_max_swappable,
                    epsilon = precision.into()
                );
            },
        );
    });
}

#[test]
fn test_move_stake_limit_partial() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let stake_amount = AlphaBalance::from(150_000_000_000_u64);
        let move_amount = AlphaBalance::from(150_000_000_000_u64);

        // add network
        let origin_netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        let destination_netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        register_ok_neuron(origin_netuid, hotkey, coldkey, 192213123);
        register_ok_neuron(destination_netuid, hotkey, coldkey, 192213123);

        // Give the neuron some stake to remove
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            origin_netuid,
            stake_amount,
        );

        // Registration now goes through the burn/swap path, which initializes swap V3 state.
        // Clear that state first so the manual reserve fixture below actually controls price.
        let mut origin_weight_meter =
            frame_support::weights::WeightMeter::with_limit(Weight::from_parts(u64::MAX, u64::MAX));
        assert!(
            <Test as pallet::Config>::SwapInterface::clear_protocol_liquidity(
                origin_netuid,
                &mut origin_weight_meter
            )
        );
        let mut destination_weight_meter =
            frame_support::weights::WeightMeter::with_limit(Weight::from_parts(u64::MAX, u64::MAX));
        assert!(
            <Test as pallet::Config>::SwapInterface::clear_protocol_liquidity(
                destination_netuid,
                &mut destination_weight_meter
            )
        );

        // Force-set alpha in and tao reserve to make price equal 1.5 on both origin and destination,
        // but there's much more liquidity on destination, so its price wouldn't go up when restaked.
        let tao_reserve = TaoBalance::from(150_000_000_000_u64);
        let alpha_in = AlphaBalance::from(100_000_000_000_u64);

        SubnetTAO::<Test>::insert(origin_netuid, tao_reserve);
        SubnetAlphaIn::<Test>::insert(origin_netuid, alpha_in);

        SubnetTAO::<Test>::insert(destination_netuid, tao_reserve * 100_000.into());
        SubnetAlphaIn::<Test>::insert(destination_netuid, alpha_in * 100_000.into());

        let origin_price =
            <Test as pallet::Config>::SwapInterface::current_alpha_price(origin_netuid.into());
        let destination_price =
            <Test as pallet::Config>::SwapInterface::current_alpha_price(destination_netuid.into());

        assert_eq!(origin_price, U96F32::from_num(1.5));
        assert_eq!(destination_price, U96F32::from_num(1.5));

        // The relative price between origin and destination subnets is 1.
        // Setup limit relative price so that it doesn't drop by more than 1% from current price.
        let limit_price = TaoBalance::from(990_000_000_u64);

        // Move stake with slippage safety - executes partially
        assert_ok!(SubtensorModule::swap_stake_limit(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            origin_netuid,
            destination_netuid,
            move_amount,
            limit_price,
            true,
        ));

        let new_alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            origin_netuid,
        );

        assert_abs_diff_eq!(
            new_alpha,
            AlphaBalance::from(149_000_000_000_u64),
            epsilon = 100_000_000.into()
        );
    });
}
