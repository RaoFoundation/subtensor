//! Chain-extension dispatch for `SetColdkeyAutoStakeHotkeyV1`.

use super::*;

#[test]
fn set_coldkey_auto_stake_hotkey_success_sets_destination() {
    mock::new_test_ext(1).execute_with(|| {
        let owner_hotkey = U256::from(4901);
        let owner_coldkey = U256::from(4902);
        let coldkey = U256::from(5901);
        let hotkey = U256::from(5902);

        let netuid = mock::add_dynamic_network(&owner_hotkey, &owner_coldkey);

        pallet_subtensor::Owner::<mock::Test>::insert(hotkey, coldkey);
        pallet_subtensor::OwnedHotkeys::<mock::Test>::insert(coldkey, vec![hotkey]);
        pallet_subtensor::Uids::<mock::Test>::insert(netuid, hotkey, 0u16);

        assert_eq!(
            pallet_subtensor::AutoStakeDestination::<mock::Test>::get(coldkey, netuid),
            None
        );

        let expected_weight = <<mock::Test as pallet_subtensor::Config>::WeightInfo as SubtensorWeightInfo>::set_coldkey_auto_stake_hotkey();

        let mut env = MockEnv::new(
            FunctionId::SetColdkeyAutoStakeHotkeyV1,
            coldkey,
            (netuid, hotkey).encode(),
        )
        .with_expected_weight(expected_weight);

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        assert_extension_success(ret);
        assert_eq!(env.charged_weight(), Some(expected_weight));

        assert_eq!(
            pallet_subtensor::AutoStakeDestination::<mock::Test>::get(coldkey, netuid),
            Some(hotkey)
        );
        let coldkeys =
            pallet_subtensor::AutoStakeDestinationColdkeys::<mock::Test>::get(hotkey, netuid);
        assert!(coldkeys.contains(&coldkey));
    });
}
