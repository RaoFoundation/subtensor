use super::mock::*;
use crate::weights::WeightInfo;
use crate::*;
use frame_support::{assert_noop, assert_ok, dispatch::GetDispatchInfo};
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
        assert_noop_ignore_postinfo!(
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

#[test]
fn auto_stake_refund_uses_real_reverse_index_lengths() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1);
        let owner_hotkey = U256::from(2);
        let coldkey = U256::from(10);
        let old_hotkey = U256::from(11);
        let new_hotkey = U256::from(12);
        let netuid = add_dynamic_network(&owner_hotkey, &owner_coldkey);

        Uids::<Test>::insert(netuid, new_hotkey, 0);
        AutoStakeDestination::<Test>::insert(coldkey, netuid, old_hotkey);
        AutoStakeDestinationColdkeys::<Test>::insert(
            old_hotkey,
            netuid,
            vec![U256::from(20), U256::from(21), coldkey],
        );
        AutoStakeDestinationColdkeys::<Test>::insert(
            new_hotkey,
            netuid,
            vec![U256::from(30), U256::from(31)],
        );

        let call = RuntimeCall::SubtensorModule(crate::Call::set_coldkey_auto_stake_hotkey {
            netuid,
            hotkey: new_hotkey,
        });
        let declared_weight = call.get_dispatch_info().call_weight;
        let post_info = match SubtensorModule::set_coldkey_auto_stake_hotkey(
            RuntimeOrigin::signed(coldkey),
            netuid,
            new_hotkey,
        ) {
            Ok(post_info) => post_info,
            Err(error) => panic!("auto-stake destination must change: {error:?}"),
        };
        let actual_weight = <Test as Config>::WeightInfo::set_coldkey_auto_stake_hotkey(3, 2);

        assert_eq!(post_info.actual_weight, Some(actual_weight));
        assert!(actual_weight.all_lt(declared_weight));
    });
}
