#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
//! Liquid alpha hyperparameters and equal-alpha self-consistency.

use frame_support::{assert_err, assert_ok};
use sp_core::U256;
use substrate_fixed::types::I32F32;
use subtensor_runtime_common::TaoBalance;
use subtensor_swap_interface::SwapHandler;

use super::super::mock::*;
use crate::tests::math::{assert_mat_compare, vec_to_fixed, vec_to_mat_fixed};
use crate::*;

#[test]
fn test_set_alpha_disabled() {
    new_test_ext(1).execute_with(|| {
        let hotkey = U256::from(1);
        let coldkey = U256::from(1 + 456);
        let netuid = add_dynamic_network(&hotkey, &coldkey);
        let signer = RuntimeOrigin::signed(coldkey);

        // Enable Liquid Alpha and setup
        SubtensorModule::set_liquid_alpha_enabled(netuid, true);
        migrations::migrate_create_root_network::migrate_create_root_network::<Test>();
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_000_u64.into());
        assert_ok!(SubtensorModule::root_register(signer.clone(), hotkey,));
        let fee = <Test as pallet::Config>::SwapInterface::approx_fee_amount(
            netuid.into(),
            DefaultMinStake::<Test>::get(),
        );
        assert_ok!(SubtensorModule::add_stake(
            signer.clone(),
            hotkey,
            netuid,
            TaoBalance::from(5) * DefaultMinStake::<Test>::get() + fee
        ));
        // Only owner can set alpha values
        assert_ok!(SubtensorModule::register_network(signer.clone(), hotkey));

        // Explicitly set to false
        SubtensorModule::set_liquid_alpha_enabled(netuid, false);
        assert_err!(
            SubtensorModule::do_set_alpha_values(signer.clone(), netuid, 1638_u16, u16::MAX),
            Error::<Test>::LiquidAlphaDisabled
        );

        SubtensorModule::set_liquid_alpha_enabled(netuid, true);
        assert_ok!(SubtensorModule::do_set_alpha_values(
            signer.clone(),
            netuid,
            1638_u16,
            u16::MAX
        ));
    });
}

/// cargo test --package pallet-subtensor --lib -- tests::epoch::liquid_alpha::test_get_set_alpha --exact --show-output
#[test]
fn test_get_set_alpha() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let alpha_low: u16 = 1638_u16;
        let alpha_high: u16 = u16::MAX - 10;

        let hotkey: U256 = U256::from(1);
        let coldkey: U256 = U256::from(1 + 456);
        let signer = RuntimeOrigin::signed(coldkey);

        // Enable Liquid Alpha and setup
        SubtensorModule::set_liquid_alpha_enabled(netuid, true);
        migrations::migrate_create_root_network::migrate_create_root_network::<Test>();
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_000_u64.into());
        assert_ok!(SubtensorModule::root_register(signer.clone(), hotkey,));

        // Should fail as signer does not own the subnet
        assert_err!(
            SubtensorModule::do_set_alpha_values(signer.clone(), netuid, alpha_low, alpha_high),
            DispatchError::BadOrigin
        );

        assert_ok!(SubtensorModule::register_network(signer.clone(), hotkey));
        SubtokenEnabled::<Test>::insert(netuid, true);

        let fee = <Test as pallet::Config>::SwapInterface::approx_fee_amount(
            netuid.into(),
            DefaultMinStake::<Test>::get(),
        );

        assert_ok!(SubtensorModule::add_stake(
            signer.clone(),
            hotkey,
            netuid,
            DefaultMinStake::<Test>::get() + fee * 2.into()
        ));

        assert_ok!(SubtensorModule::do_set_alpha_values(
            signer.clone(),
            netuid,
            alpha_low,
            alpha_high
        ));
        let (grabbed_alpha_low, grabbed_alpha_high): (u16, u16) =
            SubtensorModule::get_alpha_values(netuid);

        log::info!("alpha_low: {grabbed_alpha_low:?} alpha_high: {grabbed_alpha_high:?}");
        assert_eq!(grabbed_alpha_low, alpha_low);
        assert_eq!(grabbed_alpha_high, alpha_high);

        // Convert the u16 values to decimal values
        fn unnormalize_u16_to_float(normalized_value: u16) -> f32 {
            const MAX_U16: u16 = 65535;
            normalized_value as f32 / MAX_U16 as f32
        }

        let alpha_low_decimal = unnormalize_u16_to_float(alpha_low);
        let alpha_high_decimal = unnormalize_u16_to_float(alpha_high);

        let (alpha_low_32, alpha_high_32) = SubtensorModule::get_alpha_values_32(netuid);

        let tolerance: f32 = 1e-6; // 0.000001

        // Check if the values are equal to the sixth decimal
        assert!(
            (alpha_low_32.to_num::<f32>() - alpha_low_decimal).abs() < tolerance,
            "alpha_low mismatch: {} != {}",
            alpha_low_32.to_num::<f32>(),
            alpha_low_decimal
        );
        assert!(
            (alpha_high_32.to_num::<f32>() - alpha_high_decimal).abs() < tolerance,
            "alpha_high mismatch: {} != {}",
            alpha_high_32.to_num::<f32>(),
            alpha_high_decimal
        );

        // 1. Liquid alpha disabled
        SubtensorModule::set_liquid_alpha_enabled(netuid, false);
        assert_err!(
            SubtensorModule::do_set_alpha_values(signer.clone(), netuid, alpha_low, alpha_high),
            Error::<Test>::LiquidAlphaDisabled
        );
        // Correct scenario after error
        SubtensorModule::set_liquid_alpha_enabled(netuid, true); // Re-enable for further tests
        assert_ok!(SubtensorModule::do_set_alpha_values(
            signer.clone(),
            netuid,
            alpha_low,
            alpha_high
        ));

        // 2. Alpha high too low
        let alpha_high_too_low = (u16::MAX as u32 / 40) as u16 - 1; // One less than the minimum acceptable value
        assert_err!(
            SubtensorModule::do_set_alpha_values(
                signer.clone(),
                netuid,
                alpha_low,
                alpha_high_too_low
            ),
            Error::<Test>::AlphaHighTooLow
        );
        // Correct scenario after error
        assert_ok!(SubtensorModule::do_set_alpha_values(
            signer.clone(),
            netuid,
            alpha_low,
            alpha_high
        ));

        // 3. Alpha low too low or too high
        let alpha_low_too_low = 0_u16;
        assert_err!(
            SubtensorModule::do_set_alpha_values(
                signer.clone(),
                netuid,
                alpha_low_too_low,
                alpha_high
            ),
            Error::<Test>::AlphaLowOutOfRange
        );
        // Correct scenario after error
        assert_ok!(SubtensorModule::do_set_alpha_values(
            signer.clone(),
            netuid,
            alpha_low,
            alpha_high
        ));

        let alpha_low_too_high = alpha_high + 1; // alpha_low should be <= alpha_high
        assert_err!(
            SubtensorModule::do_set_alpha_values(
                signer.clone(),
                netuid,
                alpha_low_too_high,
                alpha_high
            ),
            Error::<Test>::AlphaLowOutOfRange
        );
        // Correct scenario after error
        assert_ok!(SubtensorModule::do_set_alpha_values(
            signer.clone(),
            netuid,
            alpha_low,
            alpha_high
        ));
    });
}

#[test]
fn test_liquid_alpha_equal_values_against_itself() {
    new_test_ext(1).execute_with(|| {
        // check Liquid alpha disabled against Liquid Alpha enabled with alpha_low == alpha_high
        let netuid: NetUid = NetUid::from(1);
        let alpha_low = u16::MAX / 10;
        let alpha_high = u16::MAX / 10;
        let epsilon = I32F32::from_num(1e-3);
        let weights: Vec<Vec<I32F32>> = vec_to_mat_fixed(
            &[0., 0.1, 0., 0., 0.2, 0.4, 0., 0.3, 0.1, 0., 0.4, 0.5],
            4,
            false,
        );
        let bonds: Vec<Vec<I32F32>> = vec_to_mat_fixed(
            &[0.1, 0.1, 0.5, 0., 0., 0.4, 0.5, 0.1, 0.1, 0., 0.4, 0.2],
            4,
            false,
        );
        let consensus: Vec<I32F32> = vec_to_fixed(&[0.3, 0.2, 0.1, 0.4]);

        // set both alpha values to 0.1 and bonds moving average to 0.9
        AlphaValues::<Test>::insert(netuid, (alpha_low, alpha_high));
        SubtensorModule::set_bonds_moving_average(netuid.into(), 900_000);

        // compute bonds with liquid alpha enabled
        SubtensorModule::set_liquid_alpha_enabled(netuid.into(), true);
        let new_bonds_liquid_alpha_on =
            SubtensorModule::compute_bonds(netuid.into(), &weights, &bonds, &consensus);

        // compute bonds with liquid alpha disabled
        SubtensorModule::set_liquid_alpha_enabled(netuid.into(), false);
        let new_bonds_liquid_alpha_off =
            SubtensorModule::compute_bonds(netuid.into(), &weights, &bonds, &consensus);

        assert_mat_compare(
            &new_bonds_liquid_alpha_on,
            &new_bonds_liquid_alpha_off,
            epsilon,
        );
    });
}
