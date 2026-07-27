//! Tests for [`crate::Pallet::set_coldkey_auto_stake_hotkey`].
//!
//! Covers missing-subnet / unregistered-hotkey errors and successful set/change.

use super::mock::*;
use crate::*;
use frame_support::{assert_noop, assert_ok};
use sp_core::U256;
use subtensor_runtime_common::NetUid;

#[test]
fn test_set_coldkey_auto_stake_hotkey_subnet_not_exists() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = NetUid::from(999); // Non-existent subnet

        assert_noop!(
            SubtensorModule::set_coldkey_auto_stake_hotkey(
                RuntimeOrigin::signed(coldkey),
                netuid,
                hotkey,
            ),
            Error::<Test>::SubnetNotExists
        );
    });
}

#[test]
fn test_set_coldkey_auto_stake_hotkey_hotkey_not_registered() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_ck = U256::from(0);
        let subnet_owner_hk = U256::from(1);

        let coldkey = U256::from(10);
        let hotkey = U256::from(11); // Hotkey not registered in subnet

        let netuid = add_dynamic_network(&subnet_owner_hk, &subnet_owner_ck);

        assert_noop!(
            SubtensorModule::set_coldkey_auto_stake_hotkey(
                RuntimeOrigin::signed(coldkey),
                netuid,
                hotkey,
            ),
            Error::<Test>::HotKeyNotRegisteredInSubNet
        );
    });
}

#[test]
fn test_set_coldkey_auto_stake_hotkey_success() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_ck = U256::from(0);
        let subnet_owner_hk = U256::from(1);

        let coldkey = U256::from(10);
        let hotkey = U256::from(11);

        Owner::<Test>::insert(hotkey, coldkey);
        OwnedHotkeys::<Test>::insert(coldkey, vec![hotkey]);

        let netuid = add_dynamic_network(&subnet_owner_hk, &subnet_owner_ck);
        Uids::<Test>::insert(netuid, hotkey, 1);

        // Verify no destination is set initially
        assert_eq!(AutoStakeDestination::<Test>::get(coldkey, netuid), None);

        // Call should succeed
        assert_ok!(SubtensorModule::set_coldkey_auto_stake_hotkey(
            RuntimeOrigin::signed(coldkey),
            netuid,
            hotkey,
        ));

        // Verify destination is now set
        assert_eq!(
            AutoStakeDestination::<Test>::get(coldkey, netuid),
            Some(hotkey)
        );
    });
}

#[test]
fn test_set_coldkey_auto_stake_hotkey_same_hotkey_again() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_ck = U256::from(0);
        let subnet_owner_hk = U256::from(1);

        let coldkey = U256::from(10);
        let hotkey = U256::from(11);

        Owner::<Test>::insert(hotkey, coldkey);
        OwnedHotkeys::<Test>::insert(coldkey, vec![hotkey]);

        let netuid = add_dynamic_network(&subnet_owner_hk, &subnet_owner_ck);
        Uids::<Test>::insert(netuid, hotkey, 1);

        // First call should succeed
        assert_ok!(SubtensorModule::set_coldkey_auto_stake_hotkey(
            RuntimeOrigin::signed(coldkey),
            netuid,
            hotkey,
        ));

        // Second call with same hotkey should fail
        assert_noop!(
            SubtensorModule::set_coldkey_auto_stake_hotkey(
                RuntimeOrigin::signed(coldkey),
                netuid,
                hotkey,
            ),
            Error::<Test>::SameAutoStakeHotkeyAlreadySet
        );
    });
}

#[test]
fn test_set_coldkey_auto_stake_hotkey_change_hotkey() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_ck = U256::from(0);
        let subnet_owner_hk = U256::from(1);

        let coldkey = U256::from(10);
        let hotkey = U256::from(11);
        let new_hotkey = U256::from(12);

        Owner::<Test>::insert(hotkey, coldkey);
        OwnedHotkeys::<Test>::insert(coldkey, vec![hotkey]);

        let netuid = add_dynamic_network(&subnet_owner_hk, &subnet_owner_ck);
        Uids::<Test>::insert(netuid, hotkey, 1);
        Uids::<Test>::insert(netuid, new_hotkey, 2);

        // First call should succeed
        assert_ok!(SubtensorModule::set_coldkey_auto_stake_hotkey(
            RuntimeOrigin::signed(coldkey),
            netuid,
            hotkey,
        ));

        // Check maps
        assert_eq!(
            AutoStakeDestination::<Test>::get(coldkey, netuid),
            Some(hotkey)
        );
        assert_eq!(
            AutoStakeDestinationColdkeys::<Test>::get(hotkey, netuid),
            vec![coldkey]
        );
        assert_eq!(
            AutoStakeDestinationColdkeys::<Test>::get(new_hotkey, netuid),
            vec![]
        );

        // Second call with new hotkey should succeed
        assert_ok!(SubtensorModule::set_coldkey_auto_stake_hotkey(
            RuntimeOrigin::signed(coldkey),
            netuid,
            new_hotkey,
        ));

        // Check maps again
        assert_eq!(
            AutoStakeDestination::<Test>::get(coldkey, netuid),
            Some(new_hotkey)
        );
        assert_eq!(
            AutoStakeDestinationColdkeys::<Test>::get(hotkey, netuid),
            vec![]
        );
        assert_eq!(
            AutoStakeDestinationColdkeys::<Test>::get(new_hotkey, netuid),
            vec![coldkey]
        );
    });
}
