#![allow(clippy::unwrap_used)]

use super::mock::*;
use crate::*;
use sp_core::U256;
use sp_runtime::BuildStorage;

#[test]
fn genesis_initializes_total_alpha_staked() {
    let mut storage = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .unwrap();
    GenesisConfig::<Test>::default()
        .assimilate_storage(&mut storage)
        .unwrap();

    sp_io::TestExternalities::new(storage).execute_with(|| {
        let netuid = NetUid::from(1);

        assert_eq!(
            TotalAlphaStaked::<Test>::get(netuid),
            AlphaBalance::from(1_000_000_000)
        );
        assert_total_alpha_staked_invariant(netuid);
    });
}

#[test]
fn hotkey_pool_mutations_keep_total_alpha_staked_in_sync() {
    new_test_ext(0).execute_with(|| {
        let netuid = NetUid::from(2);
        let hotkey_a = U256::from(10);
        let coldkey_a = U256::from(11);
        let hotkey_b = U256::from(20);
        let coldkey_b = U256::from(21);

        assert_total_alpha_staked_invariant(netuid);
        assert!(!TotalAlphaStaked::<Test>::contains_key(netuid));

        // First stake initializes both the hotkey pool and subnet aggregate.
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_a,
            &coldkey_a,
            netuid,
            100.into(),
        );
        assert_eq!(TotalAlphaStaked::<Test>::get(netuid), 100.into());
        assert_total_alpha_staked_invariant(netuid);

        // Further stake on the same hotkey contributes only its delta.
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_a,
            &coldkey_a,
            netuid,
            50.into(),
        );
        assert_eq!(TotalAlphaStaked::<Test>::get(netuid), 150.into());
        assert_total_alpha_staked_invariant(netuid);

        // A second hotkey is included independently.
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_b,
            &coldkey_b,
            netuid,
            70.into(),
        );
        assert_eq!(TotalAlphaStaked::<Test>::get(netuid), 220.into());
        assert_total_alpha_staked_invariant(netuid);

        // Coinbase's nominator-dividend path changes the shared value without
        // minting new shares; it must update the same aggregate.
        SubtensorModule::increase_stake_for_hotkey_on_subnet(&hotkey_a, netuid, 30.into());
        assert_eq!(TotalAlphaStaked::<Test>::get(netuid), 250.into());
        assert_total_alpha_staked_invariant(netuid);

        SubtensorModule::decrease_stake_for_hotkey_on_subnet(&hotkey_a, netuid, 20);
        assert_eq!(TotalAlphaStaked::<Test>::get(netuid), 230.into());
        assert_total_alpha_staked_invariant(netuid);

        // Partial burns/removals and complete removals both subtract exactly
        // what disappears from the per-hotkey total.
        SubtensorModule::decrease_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_a,
            &coldkey_a,
            netuid,
            130.into(),
        );
        assert_eq!(TotalAlphaStaked::<Test>::get(netuid), 100.into());
        assert_total_alpha_staked_invariant(netuid);

        SubtensorModule::decrease_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_b,
            &coldkey_b,
            netuid,
            70.into(),
        );
        SubtensorModule::decrease_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_a,
            &coldkey_a,
            netuid,
            30.into(),
        );
        assert_total_alpha_staked_invariant(netuid);
        assert!(TotalHotkeyAlpha::<Test>::iter().all(|(_, nu, _)| nu != netuid));
    });
}

#[test]
fn subnet_to_subnet_changes_are_accounted_independently() {
    new_test_ext(0).execute_with(|| {
        let origin_netuid = NetUid::from(2);
        let destination_netuid = NetUid::from(3);
        let hotkey = U256::from(10);
        let coldkey = U256::from(11);

        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            origin_netuid,
            100.into(),
        );
        SubtensorModule::decrease_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            origin_netuid,
            40.into(),
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            destination_netuid,
            37.into(),
        );

        assert_eq!(TotalAlphaStaked::<Test>::get(origin_netuid), 60.into());
        assert_eq!(TotalAlphaStaked::<Test>::get(destination_netuid), 37.into());
        assert_total_alpha_staked_invariant(origin_netuid);
        assert_total_alpha_staked_invariant(destination_netuid);
    });
}

#[test]
fn rejected_or_zero_mutations_do_not_change_the_aggregate() {
    new_test_ext(0).execute_with(|| {
        let netuid = NetUid::from(2);
        let hotkey = U256::from(10);
        let coldkey = U256::from(11);

        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            AlphaBalance::ZERO,
        );
        SubtensorModule::decrease_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            1.into(),
        );

        assert_total_alpha_staked_invariant(netuid);
        assert_eq!(TotalAlphaStaked::<Test>::get(netuid), AlphaBalance::ZERO);
    });
}
