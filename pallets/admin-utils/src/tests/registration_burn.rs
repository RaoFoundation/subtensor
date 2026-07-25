//! Registration targets, POW toggle, burn bounds, recycled RAO, and lock-reduction interval.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    unused_imports
)]

use super::prelude::*;

#[test]
fn test_sudo_set_target_registrations_per_interval() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let to_be_set: u16 = 10;
        add_network(netuid, 10);
        let init_value: u16 = SubtensorModule::get_target_registrations_per_interval(netuid);
        assert_eq!(
            AdminUtils::sudo_set_target_registrations_per_interval(
                <<Test as Config>::RuntimeOrigin>::signed(U256::from(1)),
                netuid,
                to_be_set
            ),
            Err(DispatchError::BadOrigin)
        );
        assert_eq!(
            AdminUtils::sudo_set_target_registrations_per_interval(
                <<Test as Config>::RuntimeOrigin>::root(),
                netuid.next(),
                to_be_set
            ),
            Err(Error::<Test>::SubnetDoesNotExist.into())
        );
        assert_eq!(
            SubtensorModule::get_target_registrations_per_interval(netuid),
            init_value
        );
        assert_ok!(AdminUtils::sudo_set_target_registrations_per_interval(
            <<Test as Config>::RuntimeOrigin>::root(),
            netuid,
            to_be_set
        ));
        assert_eq!(
            SubtensorModule::get_target_registrations_per_interval(netuid),
            to_be_set
        );
    });
}

#[test]
fn test_sudo_set_rao_recycled() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let to_be_set = TaoBalance::from(10);
        add_network(netuid, 10);
        let init_value = SubtensorModule::get_rao_recycled(netuid);

        // Need to run from genesis block
        run_to_block(1);

        assert_eq!(
            AdminUtils::sudo_set_rao_recycled(
                <<Test as Config>::RuntimeOrigin>::signed(U256::from(0)),
                netuid,
                to_be_set
            ),
            Err(DispatchError::BadOrigin)
        );
        assert_eq!(
            AdminUtils::sudo_set_rao_recycled(
                <<Test as Config>::RuntimeOrigin>::root(),
                netuid.next(),
                to_be_set
            ),
            Err(Error::<Test>::SubnetDoesNotExist.into())
        );
        assert_eq!(SubtensorModule::get_rao_recycled(netuid), init_value);

        // Verify no events emitted matching the expected event
        assert_eq!(
            System::events()
                .iter()
                .filter(|r| r.event
                    == RuntimeEvent::SubtensorModule(Event::RAORecycledForRegistrationSet(
                        netuid, to_be_set
                    )))
                .count(),
            0
        );

        assert_ok!(AdminUtils::sudo_set_rao_recycled(
            <<Test as Config>::RuntimeOrigin>::root(),
            netuid,
            to_be_set
        ));
        assert_eq!(SubtensorModule::get_rao_recycled(netuid), to_be_set);

        // Verify event emitted with correct values
        assert_eq!(
            System::events()
                .last()
                .unwrap_or_else(|| panic!(
                    "Expected there to be events: {:?}",
                    System::events().to_vec()
                ))
                .event,
            RuntimeEvent::SubtensorModule(Event::RAORecycledForRegistrationSet(netuid, to_be_set))
        );
    });
}

#[test]
fn test_sudo_set_network_lock_reduction_interval() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let to_be_set: u64 = 7200;
        add_network(netuid, 10);

        let init_value: u64 = SubtensorModule::get_lock_reduction_interval();
        assert_eq!(
            AdminUtils::sudo_set_lock_reduction_interval(
                <<Test as Config>::RuntimeOrigin>::signed(U256::from(1)),
                to_be_set
            ),
            Err(DispatchError::BadOrigin)
        );
        assert_eq!(SubtensorModule::get_lock_reduction_interval(), init_value);
        assert_ok!(AdminUtils::sudo_set_lock_reduction_interval(
            <<Test as Config>::RuntimeOrigin>::root(),
            to_be_set
        ));
        assert_eq!(SubtensorModule::get_lock_reduction_interval(), to_be_set);
    });
}

#[test]
fn test_sudo_set_network_pow_registration_allowed() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let to_be_set: bool = true;
        add_network(netuid, 10);

        assert_eq!(
            AdminUtils::sudo_set_network_pow_registration_allowed(
                <<Test as Config>::RuntimeOrigin>::signed(U256::from(1)),
                netuid,
                to_be_set
            ),
            Err(Error::<Test>::POWRegistrationDisabled.into())
        );
    });
}

#[test]
fn test_sudo_set_min_burn() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let to_be_set = TaoBalance::from(1_000_000);
        add_network(netuid, 10);
        let init_value = SubtensorModule::get_min_burn(netuid);

        // Simple case
        assert_ok!(AdminUtils::sudo_set_min_burn(
            <<Test as Config>::RuntimeOrigin>::root(),
            netuid,
            TaoBalance::from(to_be_set)
        ));
        assert_ne!(SubtensorModule::get_min_burn(netuid), init_value);
        assert_eq!(SubtensorModule::get_min_burn(netuid), to_be_set);

        // Unknown subnet
        assert_err!(
            AdminUtils::sudo_set_min_burn(
                <<Test as Config>::RuntimeOrigin>::root(),
                NetUid::from(42),
                TaoBalance::from(to_be_set)
            ),
            Error::<Test>::SubnetDoesNotExist
        );

        // Non subnet owner
        assert_err!(
            AdminUtils::sudo_set_min_burn(
                <<Test as Config>::RuntimeOrigin>::signed(U256::from(1)),
                netuid,
                TaoBalance::from(to_be_set)
            ),
            DispatchError::BadOrigin
        );

        // Above upper bound
        assert_err!(
            AdminUtils::sudo_set_min_burn(
                <<Test as Config>::RuntimeOrigin>::root(),
                netuid,
                <Test as pallet_subtensor::Config>::MinBurnUpperBound::get() + 1.into()
            ),
            Error::<Test>::ValueNotInBounds
        );

        // Above max burn
        assert_err!(
            AdminUtils::sudo_set_min_burn(
                <<Test as Config>::RuntimeOrigin>::root(),
                netuid,
                SubtensorModule::get_max_burn(netuid) + 1.into()
            ),
            Error::<Test>::ValueNotInBounds
        );
    });
}

#[test]
fn test_sudo_set_max_burn() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let to_be_set = TaoBalance::from(100_000_001);
        add_network(netuid, 10);
        let init_value = SubtensorModule::get_max_burn(netuid);

        // Simple case
        assert_ok!(AdminUtils::sudo_set_max_burn(
            <<Test as Config>::RuntimeOrigin>::root(),
            netuid,
            TaoBalance::from(to_be_set)
        ));
        assert_ne!(SubtensorModule::get_max_burn(netuid), init_value);
        assert_eq!(SubtensorModule::get_max_burn(netuid), to_be_set);

        // Unknown subnet
        assert_err!(
            AdminUtils::sudo_set_max_burn(
                <<Test as Config>::RuntimeOrigin>::root(),
                NetUid::from(42),
                TaoBalance::from(to_be_set)
            ),
            Error::<Test>::SubnetDoesNotExist
        );

        // Non subnet owner
        assert_err!(
            AdminUtils::sudo_set_max_burn(
                <<Test as Config>::RuntimeOrigin>::signed(U256::from(1)),
                netuid,
                TaoBalance::from(to_be_set)
            ),
            DispatchError::BadOrigin
        );

        // Below lower bound
        assert_err!(
            AdminUtils::sudo_set_max_burn(
                <<Test as Config>::RuntimeOrigin>::root(),
                netuid,
                <Test as pallet_subtensor::Config>::MaxBurnLowerBound::get() - 1.into()
            ),
            Error::<Test>::ValueNotInBounds
        );

        // Below min burn
        assert_err!(
            AdminUtils::sudo_set_max_burn(
                <<Test as Config>::RuntimeOrigin>::root(),
                netuid,
                SubtensorModule::get_min_burn(netuid) - 1.into()
            ),
            Error::<Test>::ValueNotInBounds
        );
    });
}
