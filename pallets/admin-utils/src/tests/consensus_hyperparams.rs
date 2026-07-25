//! Consensus hyperparams: kappa, rho, activity cutoff, immunity, tempo, bonds averages/penalties.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    unused_imports
)]

use super::prelude::*;

#[test]
fn test_sudo_set_immunity_period() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let to_be_set: u16 = 10;
        add_network(netuid, 10);
        let init_value: u16 = SubtensorModule::get_immunity_period(netuid);
        assert_eq!(
            AdminUtils::sudo_set_immunity_period(
                <<Test as Config>::RuntimeOrigin>::signed(U256::from(1)),
                netuid,
                to_be_set
            ),
            Err(DispatchError::BadOrigin)
        );
        assert_eq!(
            AdminUtils::sudo_set_immunity_period(
                <<Test as Config>::RuntimeOrigin>::root(),
                netuid.next(),
                to_be_set
            ),
            Err(Error::<Test>::SubnetDoesNotExist.into())
        );
        assert_eq!(SubtensorModule::get_immunity_period(netuid), init_value);
        assert_ok!(AdminUtils::sudo_set_immunity_period(
            <<Test as Config>::RuntimeOrigin>::root(),
            netuid,
            to_be_set
        ));
        assert_eq!(SubtensorModule::get_immunity_period(netuid), to_be_set);
    });
}

#[test]
fn test_sudo_set_min_allowed_weights() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let to_be_set: u16 = 10;
        add_network(netuid, 10);
        let init_value: u16 = SubtensorModule::get_min_allowed_weights(netuid);
        assert_eq!(
            AdminUtils::sudo_set_min_allowed_weights(
                <<Test as Config>::RuntimeOrigin>::signed(U256::from(1)),
                netuid,
                to_be_set
            ),
            Err(DispatchError::BadOrigin)
        );
        assert_eq!(
            AdminUtils::sudo_set_min_allowed_weights(
                <<Test as Config>::RuntimeOrigin>::root(),
                netuid.next(),
                to_be_set
            ),
            Err(Error::<Test>::SubnetDoesNotExist.into())
        );
        assert_eq!(SubtensorModule::get_min_allowed_weights(netuid), init_value);
        assert_ok!(AdminUtils::sudo_set_min_allowed_weights(
            <<Test as Config>::RuntimeOrigin>::root(),
            netuid,
            to_be_set
        ));
        assert_eq!(SubtensorModule::get_min_allowed_weights(netuid), to_be_set);
    });
}

#[test]
fn test_sudo_set_kappa() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let to_be_set: u16 = 10;
        add_network(netuid, 10);
        let init_value: u16 = SubtensorModule::get_kappa(netuid);
        assert_eq!(
            AdminUtils::sudo_set_kappa(
                <<Test as Config>::RuntimeOrigin>::signed(U256::from(1)),
                netuid,
                to_be_set
            ),
            Err(DispatchError::BadOrigin)
        );
        assert_eq!(
            AdminUtils::sudo_set_kappa(
                <<Test as Config>::RuntimeOrigin>::root(),
                netuid.next(),
                to_be_set
            ),
            Err(Error::<Test>::SubnetDoesNotExist.into())
        );
        assert_eq!(SubtensorModule::get_kappa(netuid), init_value);
        assert_ok!(AdminUtils::sudo_set_kappa(
            <<Test as Config>::RuntimeOrigin>::root(),
            netuid,
            to_be_set
        ));
        assert_eq!(SubtensorModule::get_kappa(netuid), to_be_set);
    });
}

#[test]
fn test_sudo_set_rho() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let to_be_set: u16 = 10;
        add_network(netuid, 10);
        let init_value: u16 = SubtensorModule::get_rho(netuid);
        assert_eq!(
            AdminUtils::sudo_set_rho(
                <<Test as Config>::RuntimeOrigin>::signed(U256::from(1)),
                netuid,
                to_be_set
            ),
            Err(DispatchError::BadOrigin)
        );
        assert_eq!(
            AdminUtils::sudo_set_rho(
                <<Test as Config>::RuntimeOrigin>::root(),
                netuid.next(),
                to_be_set
            ),
            Err(Error::<Test>::SubnetDoesNotExist.into())
        );
        assert_eq!(SubtensorModule::get_rho(netuid), init_value);
        assert_ok!(AdminUtils::sudo_set_rho(
            <<Test as Config>::RuntimeOrigin>::root(),
            netuid,
            to_be_set
        ));
        assert_eq!(SubtensorModule::get_rho(netuid), to_be_set);
    });
}

#[test]
fn test_sudo_set_activity_cutoff() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let to_be_set: u16 = pallet_subtensor::MinActivityCutoff::<Test>::get();
        add_network(netuid, 10);
        let init_value: u16 = SubtensorModule::get_activity_cutoff(netuid);
        assert_eq!(
            AdminUtils::sudo_set_activity_cutoff(
                <<Test as Config>::RuntimeOrigin>::signed(U256::from(1)),
                netuid,
                to_be_set
            ),
            Err(DispatchError::BadOrigin)
        );
        assert_eq!(
            AdminUtils::sudo_set_activity_cutoff(
                <<Test as Config>::RuntimeOrigin>::root(),
                netuid.next(),
                to_be_set
            ),
            Err(Error::<Test>::SubnetDoesNotExist.into())
        );
        assert_eq!(SubtensorModule::get_activity_cutoff(netuid), init_value);
        assert_ok!(AdminUtils::sudo_set_activity_cutoff(
            <<Test as Config>::RuntimeOrigin>::root(),
            netuid,
            to_be_set
        ));
        assert_eq!(SubtensorModule::get_activity_cutoff(netuid), to_be_set);
    });
}

#[test]
fn test_sudo_set_activity_cutoff_factor() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 10);
        let owner = U256::from(5);
        SubnetOwner::<Test>::insert(netuid, owner);
        SubtensorModule::set_admin_freeze_window(0);

        // A non-owner signed origin is rejected.
        assert_eq!(
            AdminUtils::sudo_set_activity_cutoff_factor(
                <<Test as Config>::RuntimeOrigin>::signed(U256::from(1)),
                netuid,
                5_000
            ),
            Err(DispatchError::BadOrigin)
        );

        // Out-of-bounds factors are rejected for owner and root alike.
        assert_noop!(
            AdminUtils::sudo_set_activity_cutoff_factor(
                <<Test as Config>::RuntimeOrigin>::root(),
                netuid,
                MAX_ACTIVITY_CUTOFF_FACTOR_MILLI + 1
            ),
            SubtensorError::<Test>::ActivityCutoffFactorMilliOutOfBounds
        );

        // The owner can set a factor within bounds.
        assert_ok!(AdminUtils::sudo_set_activity_cutoff_factor(
            <<Test as Config>::RuntimeOrigin>::signed(owner),
            netuid,
            5_000
        ));
        assert_eq!(ActivityCutoffFactorMilli::<Test>::get(netuid), 5_000);

        // A second owner change within the rate limit is rejected; root bypasses it.
        assert_noop!(
            AdminUtils::sudo_set_activity_cutoff_factor(
                <<Test as Config>::RuntimeOrigin>::signed(owner),
                netuid,
                6_000
            ),
            SubtensorError::<Test>::TxRateLimitExceeded
        );
        assert_ok!(AdminUtils::sudo_set_activity_cutoff_factor(
            <<Test as Config>::RuntimeOrigin>::root(),
            netuid,
            6_000
        ));
        assert_eq!(ActivityCutoffFactorMilli::<Test>::get(netuid), 6_000);
    });
}

#[test]
fn test_sudo_set_tempo_owner_and_root() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 10);
        let owner = U256::from(5);
        SubnetOwner::<Test>::insert(netuid, owner);
        SubtensorModule::set_admin_freeze_window(0);

        // A non-owner signed origin is rejected.
        assert_eq!(
            AdminUtils::sudo_set_tempo(
                <<Test as Config>::RuntimeOrigin>::signed(U256::from(1)),
                netuid,
                MIN_TEMPO
            ),
            Err(DispatchError::BadOrigin)
        );

        // A nonexistent subnet is rejected for root.
        assert_noop!(
            AdminUtils::sudo_set_tempo(
                <<Test as Config>::RuntimeOrigin>::root(),
                netuid.next(),
                MIN_TEMPO
            ),
            SubtensorError::<Test>::SubnetNotExists
        );

        // The owner is bounded to [MIN_TEMPO, MAX_TEMPO].
        assert_noop!(
            AdminUtils::sudo_set_tempo(
                <<Test as Config>::RuntimeOrigin>::signed(owner),
                netuid,
                MIN_TEMPO - 1
            ),
            SubtensorError::<Test>::TempoOutOfBounds
        );
        assert_noop!(
            AdminUtils::sudo_set_tempo(
                <<Test as Config>::RuntimeOrigin>::signed(owner),
                netuid,
                MAX_TEMPO + 1
            ),
            SubtensorError::<Test>::TempoOutOfBounds
        );

        // Within bounds the owner change lands and resets the cycle.
        assert_ok!(AdminUtils::sudo_set_tempo(
            <<Test as Config>::RuntimeOrigin>::signed(owner),
            netuid,
            MIN_TEMPO
        ));
        assert_eq!(Tempo::<Test>::get(netuid), MIN_TEMPO);
        let now = SubtensorModule::get_current_block_as_u64();
        assert_eq!(LastEpochBlock::<Test>::get(netuid), now);

        // A second owner change within the MIN_TEMPO cooldown is rate-limited.
        assert_noop!(
            AdminUtils::sudo_set_tempo(
                <<Test as Config>::RuntimeOrigin>::signed(owner),
                netuid,
                MIN_TEMPO + 1
            ),
            SubtensorError::<Test>::TxRateLimitExceeded
        );

        // Root bypasses the bounds and the rate limit.
        assert_ok!(AdminUtils::sudo_set_tempo(
            <<Test as Config>::RuntimeOrigin>::root(),
            netuid,
            10
        ));
        assert_eq!(Tempo::<Test>::get(netuid), 10);
    });
}

#[test]
fn test_sudo_set_bonds_moving_average() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let to_be_set: u64 = 10;
        add_network(netuid, 10);
        let init_value: u64 = SubtensorModule::get_bonds_moving_average(netuid.into());
        assert_eq!(
            AdminUtils::sudo_set_bonds_moving_average(
                <<Test as Config>::RuntimeOrigin>::signed(U256::from(1)),
                netuid,
                to_be_set
            ),
            Err(DispatchError::BadOrigin)
        );
        assert_eq!(
            AdminUtils::sudo_set_bonds_moving_average(
                <<Test as Config>::RuntimeOrigin>::root(),
                netuid.next(),
                to_be_set
            ),
            Err(Error::<Test>::SubnetDoesNotExist.into())
        );
        assert_eq!(
            SubtensorModule::get_bonds_moving_average(netuid.into()),
            init_value
        );
        assert_ok!(AdminUtils::sudo_set_bonds_moving_average(
            <<Test as Config>::RuntimeOrigin>::root(),
            netuid,
            to_be_set
        ));
        assert_eq!(
            SubtensorModule::get_bonds_moving_average(netuid.into()),
            to_be_set
        );
    });
}

#[test]
fn test_sudo_set_bonds_penalty() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let to_be_set: u16 = 10;
        add_network(netuid, 10);
        let init_value: u16 = SubtensorModule::get_bonds_penalty(netuid);
        assert_eq!(
            AdminUtils::sudo_set_bonds_penalty(
                <<Test as Config>::RuntimeOrigin>::signed(U256::from(1)),
                netuid,
                to_be_set
            ),
            Err(DispatchError::BadOrigin)
        );
        assert_eq!(
            AdminUtils::sudo_set_bonds_penalty(
                <<Test as Config>::RuntimeOrigin>::root(),
                netuid.next(),
                to_be_set
            ),
            Err(Error::<Test>::SubnetDoesNotExist.into())
        );
        assert_eq!(SubtensorModule::get_bonds_penalty(netuid), init_value);
        assert_ok!(AdminUtils::sudo_set_bonds_penalty(
            <<Test as Config>::RuntimeOrigin>::root(),
            netuid,
            to_be_set
        ));
        assert_eq!(SubtensorModule::get_bonds_penalty(netuid), to_be_set);
    });
}
