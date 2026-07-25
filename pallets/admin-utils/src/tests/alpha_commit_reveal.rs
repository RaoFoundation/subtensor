//! Commit-reveal, liquid alpha, alpha values/sigmoid, Yuma3, and bonds-reset toggles.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    unused_imports
)]

use super::prelude::*;

#[test]
fn test_sudo_set_commit_reveal_weights_enabled() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 10);

        let to_be_set: bool = false;
        let init_value: bool = SubtensorModule::get_commit_reveal_weights_enabled(netuid);

        assert_ok!(AdminUtils::sudo_set_commit_reveal_weights_enabled(
            <<Test as Config>::RuntimeOrigin>::root(),
            netuid,
            to_be_set
        ));

        assert!(init_value != to_be_set);
        assert_eq!(
            SubtensorModule::get_commit_reveal_weights_enabled(netuid),
            to_be_set
        );
    });
}

#[test]
fn test_sudo_set_liquid_alpha_enabled() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let enabled: bool = true;
        NetworksAdded::<Test>::insert(netuid, true);
        assert_eq!(!enabled, SubtensorModule::get_liquid_alpha_enabled(netuid));

        assert_ok!(AdminUtils::sudo_set_liquid_alpha_enabled(
            <<Test as Config>::RuntimeOrigin>::root(),
            netuid,
            enabled
        ));

        assert_eq!(enabled, SubtensorModule::get_liquid_alpha_enabled(netuid));
    });
}

#[test]
fn test_sudo_set_alpha_sigmoid_steepness() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let to_be_set: i16 = 5000;
        add_network(netuid, 10);
        let init_value = SubtensorModule::get_alpha_sigmoid_steepness(netuid);
        assert_eq!(
            AdminUtils::sudo_set_alpha_sigmoid_steepness(
                <<Test as Config>::RuntimeOrigin>::signed(U256::from(1)),
                netuid,
                to_be_set
            ),
            Err(DispatchError::BadOrigin)
        );
        assert_eq!(
            AdminUtils::sudo_set_alpha_sigmoid_steepness(
                <<Test as Config>::RuntimeOrigin>::root(),
                netuid.next(),
                to_be_set
            ),
            Err(Error::<Test>::SubnetDoesNotExist.into())
        );

        let owner = U256::from(10);
        pallet_subtensor::SubnetOwner::<Test>::insert(netuid, owner);
        assert_eq!(
            AdminUtils::sudo_set_alpha_sigmoid_steepness(
                <<Test as Config>::RuntimeOrigin>::signed(owner),
                netuid,
                -to_be_set
            ),
            Err(Error::<Test>::NegativeSigmoidSteepness.into())
        );
        assert_eq!(
            SubtensorModule::get_alpha_sigmoid_steepness(netuid),
            init_value
        );
        assert_ok!(AdminUtils::sudo_set_alpha_sigmoid_steepness(
            <<Test as Config>::RuntimeOrigin>::root(),
            netuid,
            to_be_set
        ));
        assert_eq!(
            SubtensorModule::get_alpha_sigmoid_steepness(netuid),
            to_be_set
        );
        assert_ok!(AdminUtils::sudo_set_alpha_sigmoid_steepness(
            <<Test as Config>::RuntimeOrigin>::root(),
            netuid,
            -to_be_set
        ));
        assert_eq!(
            SubtensorModule::get_alpha_sigmoid_steepness(netuid),
            -to_be_set
        );
    });
}

#[test]
fn test_set_alpha_values_dispatch_info_ok() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let alpha_low: u16 = 1638_u16;
        let alpha_high: u16 = u16::MAX - 10;
        let call = RuntimeCall::AdminUtils(crate::Call::sudo_set_alpha_values {
            netuid,
            alpha_low,
            alpha_high,
        });

        let dispatch_info = call.get_dispatch_info();

        assert_eq!(dispatch_info.class, DispatchClass::Normal);
        assert_eq!(dispatch_info.pays_fee, Pays::Yes);
    });
}

#[test]
fn test_sudo_get_set_alpha() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let alpha_low: u16 = 1638_u16;
        let alpha_high: u16 = u16::MAX - 10;

        let hotkey: U256 = U256::from(1);
        let coldkey: U256 = U256::from(1 + 456);
        let signer = <<Test as Config>::RuntimeOrigin>::signed(coldkey);

        // Enable Liquid Alpha and setup
        SubtensorModule::set_liquid_alpha_enabled(netuid, true);
        pallet_subtensor::migrations::migrate_create_root_network::migrate_create_root_network::<
            Test,
        >();
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000_000_000_u64.into());
        assert_ok!(SubtensorModule::root_register(signer.clone(), hotkey,));

        // Should fail as signer does not own the subnet
        assert_err!(
            AdminUtils::sudo_set_alpha_values(signer.clone(), netuid, alpha_low, alpha_high),
            DispatchError::BadOrigin
        );

        assert_ok!(SubtensorModule::register_network(signer.clone(), hotkey));

        assert_ok!(AdminUtils::sudo_set_alpha_values(
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
            AdminUtils::sudo_set_alpha_values(signer.clone(), netuid, alpha_low, alpha_high),
            SubtensorError::<Test>::LiquidAlphaDisabled
        );
        // Correct scenario after error
        SubtensorModule::set_liquid_alpha_enabled(netuid, true); // Re-enable for further tests
        assert_ok!(AdminUtils::sudo_set_alpha_values(
            signer.clone(),
            netuid,
            alpha_low,
            alpha_high
        ));

        // 2. Alpha high too low
        let alpha_high_too_low = (u16::MAX as u32 / 40) as u16 - 1; // One less than the minimum acceptable value
        assert_err!(
            AdminUtils::sudo_set_alpha_values(
                signer.clone(),
                netuid,
                alpha_low,
                alpha_high_too_low
            ),
            SubtensorError::<Test>::AlphaHighTooLow
        );
        // Correct scenario after error
        assert_ok!(AdminUtils::sudo_set_alpha_values(
            signer.clone(),
            netuid,
            alpha_low,
            alpha_high
        ));

        // 3. Alpha low too low or too high
        let alpha_low_too_low = 0_u16;
        assert_err!(
            AdminUtils::sudo_set_alpha_values(
                signer.clone(),
                netuid,
                alpha_low_too_low,
                alpha_high
            ),
            SubtensorError::<Test>::AlphaLowOutOfRange
        );
        // Correct scenario after error
        assert_ok!(AdminUtils::sudo_set_alpha_values(
            signer.clone(),
            netuid,
            alpha_low,
            alpha_high
        ));

        let alpha_low_too_high = alpha_high + 1;
        assert_err!(
            AdminUtils::sudo_set_alpha_values(
                signer.clone(),
                netuid,
                alpha_low_too_high,
                alpha_high
            ),
            SubtensorError::<Test>::AlphaLowOutOfRange
        );
        // Correct scenario after error
        assert_ok!(AdminUtils::sudo_set_alpha_values(
            signer.clone(),
            netuid,
            alpha_low,
            alpha_high
        ));
    });
}

#[test]
fn sudo_set_commit_reveal_weights_interval() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 10);

        let too_high = 101;
        assert_err!(
            AdminUtils::sudo_set_commit_reveal_weights_interval(
                <<Test as Config>::RuntimeOrigin>::root(),
                netuid,
                too_high
            ),
            pallet_subtensor::Error::<Test>::RevealPeriodTooLarge
        );

        let to_be_set = 55;
        let init_value = SubtensorModule::get_reveal_period(netuid);

        assert_ok!(AdminUtils::sudo_set_commit_reveal_weights_interval(
            <<Test as Config>::RuntimeOrigin>::root(),
            netuid,
            to_be_set
        ));

        assert!(init_value != to_be_set);
        assert_eq!(SubtensorModule::get_reveal_period(netuid), to_be_set);
    });
}

#[test]
fn test_sudo_set_bonds_reset_enabled() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let to_be_set: bool = true;
        let sn_owner = U256::from(1);
        add_network(netuid, 10);
        let init_value: bool = SubtensorModule::get_bonds_reset(netuid);

        assert_eq!(
            AdminUtils::sudo_set_bonds_reset_enabled(
                <<Test as Config>::RuntimeOrigin>::signed(U256::from(1)),
                netuid,
                to_be_set
            ),
            Err(DispatchError::BadOrigin)
        );

        assert_ok!(AdminUtils::sudo_set_bonds_reset_enabled(
            <<Test as Config>::RuntimeOrigin>::root(),
            netuid,
            to_be_set
        ));
        assert_eq!(SubtensorModule::get_bonds_reset(netuid), to_be_set);
        assert_ne!(SubtensorModule::get_bonds_reset(netuid), init_value);

        pallet_subtensor::SubnetOwner::<Test>::insert(netuid, sn_owner);

        assert_ok!(AdminUtils::sudo_set_bonds_reset_enabled(
            <<Test as Config>::RuntimeOrigin>::signed(sn_owner),
            netuid,
            !to_be_set
        ));
        assert_eq!(SubtensorModule::get_bonds_reset(netuid), !to_be_set);
    });
}

#[test]
fn test_sudo_set_yuma3_enabled() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let to_be_set: bool = false;
        let sn_owner = U256::from(1);
        add_network(netuid, 10);
        let init_value: bool = SubtensorModule::get_yuma3_enabled(netuid);

        assert_eq!(
            AdminUtils::sudo_set_yuma3_enabled(
                <<Test as Config>::RuntimeOrigin>::signed(U256::from(1)),
                netuid,
                to_be_set
            ),
            Err(DispatchError::BadOrigin)
        );

        assert_ok!(AdminUtils::sudo_set_yuma3_enabled(
            <<Test as Config>::RuntimeOrigin>::root(),
            netuid,
            to_be_set
        ));
        assert_eq!(SubtensorModule::get_yuma3_enabled(netuid), to_be_set);
        assert_ne!(SubtensorModule::get_yuma3_enabled(netuid), init_value);

        pallet_subtensor::SubnetOwner::<Test>::insert(netuid, sn_owner);

        assert_ok!(AdminUtils::sudo_set_yuma3_enabled(
            <<Test as Config>::RuntimeOrigin>::signed(sn_owner),
            netuid,
            !to_be_set
        ));
        assert_eq!(SubtensorModule::get_yuma3_enabled(netuid), !to_be_set);
    });
}

#[test]
fn test_sudo_set_commit_reveal_version() {
    new_test_ext().execute_with(|| {
        add_network(NetUid::from(1), 10);

        let to_be_set: u16 = 5;
        let init_value: u16 = SubtensorModule::get_commit_reveal_weights_version();

        assert_ok!(AdminUtils::sudo_set_commit_reveal_version(
            <<Test as Config>::RuntimeOrigin>::root(),
            to_be_set
        ));

        assert!(init_value != to_be_set);
        assert_eq!(
            SubtensorModule::get_commit_reveal_weights_version(),
            to_be_set
        );
    });
}
