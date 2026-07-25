//! Delegate/default take, stake thresholds, nominator min stake, childkey take, and owner-cut toggles.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    unused_imports
)]

use super::prelude::*;

#[test]
fn test_sudo_set_default_take() {
    new_test_ext().execute_with(|| {
        let to_be_set = PerU16::from_parts(10);
        let init_value: u16 = SubtensorModule::get_default_delegate_take();
        assert_eq!(
            AdminUtils::sudo_set_default_take(
                <<Test as Config>::RuntimeOrigin>::signed(U256::from(0)),
                to_be_set
            ),
            Err(DispatchError::BadOrigin)
        );
        assert_eq!(SubtensorModule::get_default_delegate_take(), init_value);
        assert_ok!(AdminUtils::sudo_set_default_take(
            <<Test as Config>::RuntimeOrigin>::root(),
            to_be_set
        ));
        assert_eq!(
            SubtensorModule::get_default_delegate_take(),
            to_be_set.deconstruct()
        );
    });
}

#[test]
fn test_sudo_subnet_owner_cut() {
    new_test_ext().execute_with(|| {
        let to_be_set: u16 = 10;
        let init_value: u16 = SubtensorModule::get_subnet_owner_cut();
        assert_eq!(
            AdminUtils::sudo_set_subnet_owner_cut(
                <<Test as Config>::RuntimeOrigin>::signed(U256::from(0)),
                to_be_set
            ),
            Err(DispatchError::BadOrigin)
        );
        assert_eq!(SubtensorModule::get_subnet_owner_cut(), init_value);
        assert_ok!(AdminUtils::sudo_set_subnet_owner_cut(
            <<Test as Config>::RuntimeOrigin>::root(),
            to_be_set
        ));
        assert_eq!(SubtensorModule::get_subnet_owner_cut(), to_be_set);
    });
}

#[test]
fn test_sudo_set_stake_threshold() {
    new_test_ext().execute_with(|| {
        let to_be_set: u64 = 10;
        let init_value: u64 = SubtensorModule::get_stake_threshold();
        assert_eq!(
            AdminUtils::sudo_set_stake_threshold(
                <<Test as Config>::RuntimeOrigin>::signed(U256::from(1)),
                to_be_set
            ),
            Err(DispatchError::BadOrigin)
        );
        assert_eq!(SubtensorModule::get_stake_threshold(), init_value);
        assert_ok!(AdminUtils::sudo_set_stake_threshold(
            <<Test as Config>::RuntimeOrigin>::root(),
            to_be_set
        ));
        assert_eq!(SubtensorModule::get_stake_threshold(), to_be_set);
    });
}

mod sudo_set_nominator_min_required_stake {
    use super::*;

    #[test]
    fn can_only_be_called_by_admin() {
        new_test_ext().execute_with(|| {
            let to_be_set = SubtensorModule::get_nominator_min_required_stake() + 5;
            assert_eq!(
                AdminUtils::sudo_set_nominator_min_required_stake(
                    <<Test as Config>::RuntimeOrigin>::signed(U256::from(0)),
                    to_be_set
                ),
                Err(DispatchError::BadOrigin)
            );
        });
    }

    #[test]
    fn sets_a_lower_value() {
        new_test_ext().execute_with(|| {
            assert_ok!(AdminUtils::sudo_set_nominator_min_required_stake(
                <<Test as Config>::RuntimeOrigin>::root(),
                10
            ));
            let default_min_stake = pallet_subtensor::DefaultMinStake::<Test>::get();
            assert_eq!(
                SubtensorModule::get_nominator_min_required_stake(),
                10 * default_min_stake.to_u64() / 1_000_000
            );

            assert_ok!(AdminUtils::sudo_set_nominator_min_required_stake(
                <<Test as Config>::RuntimeOrigin>::root(),
                5
            ));
            assert_eq!(
                SubtensorModule::get_nominator_min_required_stake(),
                5 * default_min_stake.to_u64() / 1_000_000
            );
        });
    }

    #[test]
    fn sets_a_higher_value() {
        new_test_ext().execute_with(|| {
            let to_be_set = SubtensorModule::get_nominator_min_required_stake() + 5;
            let default_min_stake = pallet_subtensor::DefaultMinStake::<Test>::get();
            assert_ok!(AdminUtils::sudo_set_nominator_min_required_stake(
                <<Test as Config>::RuntimeOrigin>::root(),
                to_be_set
            ));
            assert_eq!(
                SubtensorModule::get_nominator_min_required_stake(),
                to_be_set * default_min_stake.to_u64() / 1_000_000
            );
        });
    }
}

#[test]
fn test_sudo_set_tx_delegate_take_rate_limit() {
    new_test_ext().execute_with(|| {
        let to_be_set: u64 = 10;
        let init_value: u64 = SubtensorModule::get_tx_delegate_take_rate_limit();
        assert_eq!(
            AdminUtils::sudo_set_tx_delegate_take_rate_limit(
                <<Test as Config>::RuntimeOrigin>::signed(U256::from(1)),
                to_be_set
            ),
            Err(DispatchError::BadOrigin)
        );
        assert_eq!(
            SubtensorModule::get_tx_delegate_take_rate_limit(),
            init_value
        );
        assert_ok!(AdminUtils::sudo_set_tx_delegate_take_rate_limit(
            <<Test as Config>::RuntimeOrigin>::root(),
            to_be_set
        ));
        assert_eq!(
            SubtensorModule::get_tx_delegate_take_rate_limit(),
            to_be_set
        );
    });
}

#[test]
fn test_sudo_set_min_delegate_take() {
    new_test_ext().execute_with(|| {
        let to_be_set = PerU16::from_parts(u16::MAX / 100);
        let init_value = SubtensorModule::get_min_delegate_take();
        assert_eq!(
            AdminUtils::sudo_set_min_delegate_take(
                <<Test as Config>::RuntimeOrigin>::signed(U256::from(1)),
                to_be_set
            ),
            Err(DispatchError::BadOrigin)
        );
        assert_eq!(SubtensorModule::get_min_delegate_take(), init_value);
        assert_ok!(AdminUtils::sudo_set_min_delegate_take(
            <<Test as Config>::RuntimeOrigin>::root(),
            to_be_set
        ));
        assert_eq!(
            SubtensorModule::get_min_delegate_take(),
            to_be_set.deconstruct()
        );
    });
}

#[test]
fn test_sudo_set_min_childkey_take_per_subnet() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let owner = U256::from(10);
        let non_owner = U256::from(11);
        let take = PerU16::from_parts(SubtensorModule::get_max_childkey_take() / 2);

        add_network(netuid, 10);
        SubnetOwner::<Test>::insert(netuid, owner);

        assert_eq!(
            AdminUtils::sudo_set_min_childkey_take_per_subnet(
                <<Test as Config>::RuntimeOrigin>::signed(non_owner),
                netuid,
                take
            ),
            Err(DispatchError::BadOrigin)
        );

        assert_ok!(AdminUtils::sudo_set_min_childkey_take_per_subnet(
            <<Test as Config>::RuntimeOrigin>::signed(owner),
            netuid,
            take
        ));
        assert_eq!(
            SubtensorModule::get_min_childkey_take_for_subnet(netuid),
            take.deconstruct()
        );
        assert_eq!(
            SubtensorModule::get_effective_min_childkey_take(netuid),
            take.deconstruct()
        );
    });
}

#[test]
fn test_sudo_set_min_childkey_take_per_subnet_rejects_below_global() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let global_min: u16 = 100;

        add_network(netuid, 10);
        SubtensorModule::set_min_childkey_take(PerU16::from_parts(global_min));

        assert_noop!(
            AdminUtils::sudo_set_min_childkey_take_per_subnet(
                <<Test as Config>::RuntimeOrigin>::root(),
                netuid,
                PerU16::from_parts(global_min - 1)
            ),
            Error::<Test>::InvalidValue
        );
        assert_ok!(AdminUtils::sudo_set_min_childkey_take_per_subnet(
            <<Test as Config>::RuntimeOrigin>::root(),
            netuid,
            PerU16::from_parts(global_min)
        ));
    });
}

#[test]
fn test_sets_a_lower_value_clears_small_nominations() {
    new_test_ext().execute_with(|| {
        let hotkey: U256 = U256::from(3);
        let owner_coldkey: U256 = U256::from(1);
        let staker_coldkey: U256 = U256::from(2);

        let initial_nominator_min_required_stake = 10;
        let nominator_min_required_stake_0 = 5;
        let nominator_min_required_stake_1 = 20;

        assert!(nominator_min_required_stake_0 < nominator_min_required_stake_1);
        assert!(nominator_min_required_stake_0 < initial_nominator_min_required_stake);

        let to_stake = initial_nominator_min_required_stake + 1;

        assert!(to_stake > initial_nominator_min_required_stake);
        assert!(to_stake > nominator_min_required_stake_0); // Should stay when set
        assert!(to_stake < nominator_min_required_stake_1); // Should be removed when set

        // ---- FIX: fund accounts so burn-based registration + staking doesn't fail.
        let funds: u64 = 1_000_000_000_000_000; // 1,000,000 TAO (in RAO)
        let _ = Balances::deposit_creating(&owner_coldkey, Balance::from(funds));
        let _ = Balances::deposit_creating(&staker_coldkey, Balance::from(funds));
        let _ = Balances::deposit_creating(&hotkey, Balance::from(funds)); // defensive

        // Create network
        let netuid = NetUid::from(2);
        add_network(netuid, 10);

        // Register a neuron
        register_ok_neuron(netuid, hotkey, owner_coldkey, 0);

        let default_min_stake = pallet_subtensor::DefaultMinStake::<Test>::get();
        assert_ok!(AdminUtils::sudo_set_nominator_min_required_stake(
            RuntimeOrigin::root(),
            initial_nominator_min_required_stake
        ));
        assert_eq!(
            SubtensorModule::get_nominator_min_required_stake(),
            initial_nominator_min_required_stake * default_min_stake.to_u64() / 1_000_000
        );

        // Stake to the hotkey as staker_coldkey
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &staker_coldkey,
            netuid,
            to_stake.into(),
        );

        let default_min_stake = pallet_subtensor::DefaultMinStake::<Test>::get();
        assert_ok!(AdminUtils::sudo_set_nominator_min_required_stake(
            RuntimeOrigin::root(),
            nominator_min_required_stake_0
        ));
        assert_eq!(
            SubtensorModule::get_nominator_min_required_stake(),
            nominator_min_required_stake_0 * default_min_stake.to_u64() / 1_000_000
        );

        // Check this nomination is not cleared
        assert!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &staker_coldkey,
                netuid
            ) > 0.into()
        );

        assert_ok!(AdminUtils::sudo_set_nominator_min_required_stake(
            RuntimeOrigin::root(),
            nominator_min_required_stake_1
        ));
        assert_eq!(
            SubtensorModule::get_nominator_min_required_stake(),
            nominator_min_required_stake_1 * default_min_stake.to_u64() / 1_000_000
        );

        // Check this nomination is cleared
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &staker_coldkey,
                netuid
            ),
            0.into()
        );
    });
}

#[test]
fn test_sudo_set_owner_cut_enabled() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(11);
        let owner = U256::from(1234);
        let call = RuntimeCall::AdminUtils(crate::Call::sudo_set_owner_cut_enabled {
            netuid,
            enabled: false,
        });

        add_network(netuid, 10);
        SubnetOwner::<Test>::insert(netuid, owner);

        assert_ok!(AdminUtils::sudo_set_admin_freeze_window(
            <<Test as Config>::RuntimeOrigin>::root(),
            0
        ));

        let dispatch_info = call.get_dispatch_info();
        assert_eq!(dispatch_info.pays_fee, Pays::Yes);

        assert!(SubtensorModule::get_owner_cut_enabled(netuid));
        assert_ok!(AdminUtils::sudo_set_owner_cut_enabled(
            <<Test as Config>::RuntimeOrigin>::signed(owner),
            netuid,
            false
        ));
        assert!(!SubtensorModule::get_owner_cut_enabled(netuid));

        assert_ok!(AdminUtils::sudo_set_owner_cut_enabled(
            <<Test as Config>::RuntimeOrigin>::root(),
            netuid,
            true
        ));
        assert!(SubtensorModule::get_owner_cut_enabled(netuid));
    });
}

#[test]
fn test_sudo_set_owner_cut_auto_lock_enabled() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(11);
        let owner = U256::from(1234);
        let non_owner = U256::from(4321);
        let call = RuntimeCall::AdminUtils(crate::Call::sudo_set_owner_cut_auto_lock_enabled {
            netuid,
            enabled: true,
        });

        add_network(netuid, 10);
        SubnetOwner::<Test>::insert(netuid, owner);

        assert_ok!(AdminUtils::sudo_set_admin_freeze_window(
            <<Test as Config>::RuntimeOrigin>::root(),
            0
        ));

        let dispatch_info = call.get_dispatch_info();
        assert_eq!(dispatch_info.pays_fee, Pays::Yes);

        assert!(!SubtensorModule::get_owner_cut_auto_lock_enabled(netuid));
        assert_noop!(
            AdminUtils::sudo_set_owner_cut_auto_lock_enabled(
                <<Test as Config>::RuntimeOrigin>::signed(non_owner),
                netuid,
                true
            ),
            DispatchError::BadOrigin
        );

        assert_ok!(AdminUtils::sudo_set_owner_cut_auto_lock_enabled(
            <<Test as Config>::RuntimeOrigin>::signed(owner),
            netuid,
            false
        ));
        assert!(!SubtensorModule::get_owner_cut_auto_lock_enabled(netuid));

        assert_ok!(AdminUtils::sudo_set_owner_cut_auto_lock_enabled(
            <<Test as Config>::RuntimeOrigin>::root(),
            netuid,
            true
        ));
        assert!(SubtensorModule::get_owner_cut_auto_lock_enabled(netuid));
    });
}
