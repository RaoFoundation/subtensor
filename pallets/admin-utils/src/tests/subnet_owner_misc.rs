//! Subnet owner hotkey, moving alpha, EMA halving, dissolve schedule, coldkey-swap delays, max epochs.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    unused_imports
)]

use super::prelude::*;

#[test]
fn test_sudo_set_coldkey_swap_announcement_delay() {
    new_test_ext().execute_with(|| {
        // Arrange
        let root = RuntimeOrigin::root();
        let non_root = RuntimeOrigin::signed(U256::from(1));
        let new_delay = 100u32.into();

        // Act & Assert: Non-root account should fail
        assert_noop!(
            AdminUtils::sudo_set_coldkey_swap_announcement_delay(non_root, new_delay),
            DispatchError::BadOrigin
        );

        // Act: Root account should succeed
        assert_ok!(AdminUtils::sudo_set_coldkey_swap_announcement_delay(
            root.clone(),
            new_delay
        ));

        // Assert: Check if the delay was actually set
        assert_eq!(
            pallet_subtensor::ColdkeySwapAnnouncementDelay::<Test>::get(),
            new_delay
        );

        // Act & Assert: Setting the same value again should succeed (idempotent operation)
        assert_ok!(AdminUtils::sudo_set_coldkey_swap_announcement_delay(
            root, new_delay
        ));

        // You might want to check for events here if your pallet emits them
        System::assert_last_event(Event::ColdkeySwapAnnouncementDelaySet(new_delay).into());
    });
}

#[test]
fn test_sudo_set_coldkey_swap_reannouncement_delay() {
    new_test_ext().execute_with(|| {
        // Arrange
        let root = RuntimeOrigin::root();
        let non_root = RuntimeOrigin::signed(U256::from(1));
        let new_delay = 100u32.into();

        // Act & Assert: Non-root account should fail
        assert_noop!(
            AdminUtils::sudo_set_coldkey_swap_reannouncement_delay(non_root, new_delay),
            DispatchError::BadOrigin
        );

        // Act: Root account should succeed
        assert_ok!(AdminUtils::sudo_set_coldkey_swap_reannouncement_delay(
            root.clone(),
            new_delay
        ));

        // Assert: Check if the delay was actually set
        assert_eq!(
            pallet_subtensor::ColdkeySwapReannouncementDelay::<Test>::get(),
            new_delay
        );

        // Act & Assert: Setting the same value again should succeed (idempotent operation)
        assert_ok!(AdminUtils::sudo_set_coldkey_swap_reannouncement_delay(
            root, new_delay
        ));

        // You might want to check for events here if your pallet emits them
        System::assert_last_event(Event::ColdkeySwapReannouncementDelaySet(new_delay).into());
    });
}

#[test]
fn test_sudo_set_max_epochs_per_block() {
    new_test_ext().execute_with(|| {
        let root = RuntimeOrigin::root();
        let non_root = RuntimeOrigin::signed(U256::from(1));
        let init_value = SubtensorModule::get_max_epochs_per_block();
        let to_be_set: u8 = init_value.saturating_add(3);

        // Non-root is rejected and leaves the value untouched.
        assert_noop!(
            AdminUtils::sudo_set_max_epochs_per_block(non_root, to_be_set),
            DispatchError::BadOrigin
        );
        assert_eq!(SubtensorModule::get_max_epochs_per_block(), init_value);

        // Zero is rejected by the `>= 1` guard (a zero cap would halt all subnet epochs).
        assert_noop!(
            AdminUtils::sudo_set_max_epochs_per_block(root.clone(), 0u8),
            Error::<Test>::ValueNotInBounds
        );
        assert_eq!(SubtensorModule::get_max_epochs_per_block(), init_value);

        // Root succeeds: storage is updated and the event is emitted.
        assert_ok!(AdminUtils::sudo_set_max_epochs_per_block(root, to_be_set));
        assert_eq!(SubtensorModule::get_max_epochs_per_block(), to_be_set);
        System::assert_last_event(Event::MaxEpochsPerBlockSet(to_be_set).into());
    });
}

#[test]
fn test_sudo_set_max_epochs_per_block_changes_deferrals() {
    new_test_ext().execute_with(|| {
        let root = RuntimeOrigin::root();

        // Create several subnets and force each to be "due this block".
        let created: u16 = 4;
        for i in 0..created {
            let netuid = NetUid::from(i + 1);
            add_network(netuid, 100 /*tempo*/);
            pallet_subtensor::PendingEpochAt::<Test>::insert(netuid, 1);
        }

        let block = SubtensorModule::get_current_block_as_u64();
        let subnets: Vec<NetUid> = SubtensorModule::get_all_subnet_netuids()
            .into_iter()
            .filter(|x| *x != NetUid::ROOT)
            .collect();
        let due = subnets
            .iter()
            .filter(|n| SubtensorModule::should_run_epoch(**n, block))
            .count();
        assert!(due >= created as usize);

        // Tight cap (1): every due subnet beyond the first is deferred.
        assert_ok!(AdminUtils::sudo_set_max_epochs_per_block(root.clone(), 1u8));
        let deferred_tight = SubtensorModule::epochs_deferred_this_block(&subnets, block).len();
        assert_eq!(deferred_tight, due.saturating_sub(1));

        // Raising the cap above the due count clears all deferrals — proving the
        // admin-set cap directly drives which epochs are deferred.
        assert_ok!(AdminUtils::sudo_set_max_epochs_per_block(
            root,
            (due as u8).saturating_add(2)
        ));
        let deferred_loose = SubtensorModule::epochs_deferred_this_block(&subnets, block).len();
        assert_eq!(deferred_loose, 0);
        assert!(
            deferred_loose < deferred_tight,
            "raising MaxEpochsPerBlock must defer fewer epochs"
        );
    });
}

#[test]
fn test_sudo_set_dissolve_network_schedule_duration() {
    new_test_ext().execute_with(|| {
        // Arrange
        let root = RuntimeOrigin::root();
        let non_root = RuntimeOrigin::signed(U256::from(1));
        let new_duration = 200u32.into();

        // Act & Assert: Non-root account should fail
        assert_noop!(
            AdminUtils::sudo_set_dissolve_network_schedule_duration(non_root, new_duration),
            DispatchError::BadOrigin
        );

        // Act: Root account should succeed
        assert_ok!(AdminUtils::sudo_set_dissolve_network_schedule_duration(
            root.clone(),
            new_duration
        ));

        // Assert: Check if the duration was actually set
        assert_eq!(
            pallet_subtensor::DissolveNetworkScheduleDuration::<Test>::get(),
            new_duration
        );

        // Act & Assert: Setting the same value again should succeed (idempotent operation)
        assert_ok!(AdminUtils::sudo_set_dissolve_network_schedule_duration(
            root,
            new_duration
        ));

        // You might want to check for events here if your pallet emits them
        System::assert_last_event(Event::DissolveNetworkScheduleDurationSet(new_duration).into());
    });
}

#[test]
fn test_sudo_root_sets_subnet_moving_alpha() {
    new_test_ext().execute_with(|| {
        let alpha: I96F32 = I96F32::saturating_from_num(0.5);
        let initial = pallet_subtensor::SubnetMovingAlpha::<Test>::get();
        assert!(initial != alpha);

        assert_ok!(AdminUtils::sudo_set_subnet_moving_alpha(
            <<Test as Config>::RuntimeOrigin>::root(),
            alpha
        ));

        assert_eq!(pallet_subtensor::SubnetMovingAlpha::<Test>::get(), alpha);
    });
}

// cargo test --package pallet-admin-utils --lib -- tests::test_sudo_set_ema_halving --exact --show-output
#[test]
fn test_sudo_set_ema_halving() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let to_be_set: u64 = 10;
        add_network(netuid, 10);

        let value_before: u64 = pallet_subtensor::EMAPriceHalvingBlocks::<Test>::get(netuid);
        assert_eq!(
            AdminUtils::sudo_set_ema_price_halving_period(
                <<Test as Config>::RuntimeOrigin>::signed(U256::from(1)),
                netuid,
                to_be_set
            ),
            Err(DispatchError::BadOrigin)
        );
        let value_after_0: u64 = pallet_subtensor::EMAPriceHalvingBlocks::<Test>::get(netuid);
        assert_eq!(value_after_0, value_before);

        let owner = U256::from(10);
        pallet_subtensor::SubnetOwner::<Test>::insert(netuid, owner);
        assert_eq!(
            AdminUtils::sudo_set_ema_price_halving_period(
                <<Test as Config>::RuntimeOrigin>::signed(owner),
                netuid,
                to_be_set
            ),
            Err(DispatchError::BadOrigin)
        );
        let value_after_1: u64 = pallet_subtensor::EMAPriceHalvingBlocks::<Test>::get(netuid);
        assert_eq!(value_after_1, value_before);
        assert_ok!(AdminUtils::sudo_set_ema_price_halving_period(
            <<Test as Config>::RuntimeOrigin>::root(),
            netuid,
            to_be_set
        ));
        let value_after_2: u64 = pallet_subtensor::EMAPriceHalvingBlocks::<Test>::get(netuid);
        assert_eq!(value_after_2, to_be_set);
    });
}

// cargo test --package pallet-admin-utils --lib -- tests::test_set_sn_owner_hotkey --exact --show-output
#[test]
fn test_set_sn_owner_hotkey_owner() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey: U256 = U256::from(3);
        let bad_origin_coldkey: U256 = U256::from(4);
        add_network(netuid, 10);

        let owner = U256::from(10);
        pallet_subtensor::SubnetOwner::<Test>::insert(netuid, owner);

        // Non-owner and non-root cannot set the sn owner hotkey
        assert_eq!(
            AdminUtils::sudo_set_sn_owner_hotkey(
                <<Test as Config>::RuntimeOrigin>::signed(bad_origin_coldkey),
                netuid,
                hotkey
            ),
            Err(DispatchError::BadOrigin)
        );

        // SN owner can set the hotkey
        assert_ok!(AdminUtils::sudo_set_sn_owner_hotkey(
            <<Test as Config>::RuntimeOrigin>::signed(owner),
            netuid,
            hotkey
        ));

        // Check the value
        let actual_hotkey = pallet_subtensor::SubnetOwnerHotkey::<Test>::get(netuid);
        assert_eq!(actual_hotkey, hotkey);

        // Cannot set again (rate limited)
        assert_err!(
            AdminUtils::sudo_set_sn_owner_hotkey(
                <<Test as Config>::RuntimeOrigin>::signed(owner),
                netuid,
                hotkey
            ),
            pallet_subtensor::Error::<Test>::TxRateLimitExceeded
        );
    });
}

// cargo test --package pallet-admin-utils --lib -- tests::test_set_sn_owner_hotkey_root --exact --show-output
#[test]
fn test_set_sn_owner_hotkey_root() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey: U256 = U256::from(3);
        add_network(netuid, 10);

        let owner = U256::from(10);
        pallet_subtensor::SubnetOwner::<Test>::insert(netuid, owner);

        // Root can set the hotkey
        assert_ok!(AdminUtils::sudo_set_sn_owner_hotkey(
            <<Test as Config>::RuntimeOrigin>::root(),
            netuid,
            hotkey
        ));

        // Check the value
        let actual_hotkey = pallet_subtensor::SubnetOwnerHotkey::<Test>::get(netuid);
        assert_eq!(actual_hotkey, hotkey);
    });
}
