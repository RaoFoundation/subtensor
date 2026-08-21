#![allow(clippy::indexing_slicing, clippy::unwrap_used)]

use super::mock::*;
use crate::subnets::dissolution::{DissolveCleanupPhase, DissolveCleanupStatus};
use crate::*;
use frame_support::{assert_ok, weights::Weight};
use sp_core::U256;
use sp_runtime::PerU16;
use substrate_fixed::types::I64F64;
use subtensor_runtime_common::AlphaBalance;

#[test]
fn lifecycle_tracks_registration_start_dissolution_and_cleanup() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let coldkey = U256::from(10);

        SubtensorModule::init_new_network(netuid, 13);
        assert_eq!(
            SubnetState::<Test>::get(netuid),
            Some(SubnetLifecycleState::Registered)
        );

        SubnetOwner::<Test>::insert(netuid, coldkey);
        System::set_block_number(StartCallDelay::<Test>::get());
        assert_ok!(SubtensorModule::start_call(
            RuntimeOrigin::signed(coldkey),
            netuid
        ));
        assert_eq!(
            SubnetState::<Test>::get(netuid),
            Some(SubnetLifecycleState::Started)
        );

        assert_ok!(SubtensorModule::do_dissolve_network(netuid));
        assert_eq!(
            SubnetState::<Test>::get(netuid),
            Some(SubnetLifecycleState::PendingDissolution)
        );

        // Enough weight to select the queued subnet, but not to advance its first phase.
        let selection_weight = <Test as frame_system::Config>::DbWeight::get()
            .reads(1)
            .saturating_add(<Test as frame_system::Config>::DbWeight::get().writes(2));
        SubtensorModule::remove_data_for_dissolved_networks(selection_weight);
        assert_eq!(
            SubnetState::<Test>::get(netuid),
            Some(SubnetLifecycleState::Dissolving)
        );

        let mut guard = 0;
        while SubnetState::<Test>::contains_key(netuid) {
            guard += 1;
            assert!(guard < 64, "cleanup should finish");
            SubtensorModule::remove_data_for_dissolved_networks(Weight::MAX);
        }
        assert!(!SubnetState::<Test>::contains_key(netuid));

        // Once cleanup is complete, reuse starts a fresh lifecycle generation.
        SubtensorModule::init_new_network(netuid, 13);
        assert_eq!(
            SubnetState::<Test>::get(netuid),
            Some(SubnetLifecycleState::Registered)
        );
    });
}

#[test]
fn reporting_cursor_hides_paid_hotkeys_and_keeps_unpaid_hotkeys() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let coldkey = U256::from(20);
        let hotkey_a = U256::from(30);
        let hotkey_b = U256::from(31);
        let amount_a = AlphaBalance::from(100);
        let amount_b = AlphaBalance::from(200);

        SubtensorModule::init_new_network(netuid, 13);
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &coldkey, &hotkey_a
        ));
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &coldkey, &hotkey_b
        ));
        SubtensorModule::append_neuron(netuid, &hotkey_a, 0);
        SubtensorModule::append_neuron(netuid, &hotkey_b, 0);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_a, &coldkey, netuid, amount_a,
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_b, &coldkey, netuid, amount_b,
        );

        let mut ordered = [
            (
                TotalHotkeyAlpha::<Test>::hashed_key_for(hotkey_a, netuid),
                hotkey_a,
                amount_a,
            ),
            (
                TotalHotkeyAlpha::<Test>::hashed_key_for(hotkey_b, netuid),
                hotkey_b,
                amount_b,
            ),
        ];
        ordered.sort_by(|left, right| left.0.cmp(&right.0));
        let paid = &ordered[0];
        let unpaid = &ordered[1];

        SubnetState::<Test>::insert(netuid, SubnetLifecycleState::Dissolving);
        let mut status = DissolveCleanupStatus::new(netuid);
        status.set_phase(DissolveCleanupPhase::AlphaInOutStakesSettleStakes);
        status.last_key = Some(paid.0.clone());
        CurrentDissolveCleanupStatus::<Test>::set(Some(status));

        assert!(!SubtensorModule::is_hotkey_stake_reportable(
            netuid, &paid.1
        ));
        assert!(SubtensorModule::is_hotkey_stake_reportable(
            netuid, &unpaid.1
        ));
        assert_eq!(
            SubtensorModule::get_reported_total_coldkey_alpha_on_subnet(&coldkey, netuid),
            unpaid.2
        );

        let positions = SubtensorModule::get_stake_info_for_coldkey(coldkey);
        assert_eq!(positions.len(), 1, "only the unpaid hotkey remains visible");
        let availability =
            SubtensorModule::get_stake_availability_for_coldkeys(vec![coldkey], Some(vec![netuid]));
        let row = availability.get(&coldkey).unwrap().get(&netuid).unwrap();
        assert_eq!(row.total(), unpaid.2);
        assert_eq!(row.available(), AlphaBalance::ZERO);

        // Every stake-bearing reporting family accepts the dissolving subnet, while their
        // shared reporting helper zeros the paid UID and preserves the unpaid UID.
        let (_, alpha_stake, _) = SubtensorModule::get_reported_stake_weights_for_network(netuid);
        let paid_uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &paid.1).unwrap();
        let unpaid_uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &unpaid.1).unwrap();
        assert_eq!(alpha_stake[usize::from(paid_uid)], I64F64::from_num(0));
        assert!(alpha_stake[usize::from(unpaid_uid)] > I64F64::from_num(0));
        assert!(SubtensorModule::get_dynamic_info(netuid).is_some());
        assert!(SubtensorModule::get_metagraph(netuid).is_some());
        assert!(SubtensorModule::get_mechagraph(netuid, 0.into()).is_some());
        assert!(SubtensorModule::get_selective_metagraph(netuid, vec![67]).is_some());
        assert!(SubtensorModule::get_selective_mechagraph(netuid, 0.into(), vec![67]).is_some());
        assert!(SubtensorModule::get_subnet_state(netuid).is_some());
        assert_eq!(SubtensorModule::get_neurons(netuid).len(), 2);

        Delegates::<Test>::insert(paid.1, PerU16::zero());
        Delegates::<Test>::insert(unpaid.1, PerU16::zero());
        assert!(
            SubtensorModule::get_delegate(paid.1)
                .unwrap()
                .nominators
                .is_empty()
        );
        assert_eq!(
            SubtensorModule::get_delegate(unpaid.1)
                .unwrap()
                .nominators
                .len(),
            1
        );
        assert_eq!(SubtensorModule::get_delegated(coldkey).len(), 1);

        // The reporting handoff completes before physical alpha cleanup begins.
        let mut status = CurrentDissolveCleanupStatus::<Test>::get().unwrap();
        status.set_phase(DissolveCleanupPhase::AlphaInOutStakesAlpha);
        status.last_key = None;
        CurrentDissolveCleanupStatus::<Test>::set(Some(status));
        assert!(SubtensorModule::get_stake_info_for_coldkey(coldkey).is_empty());
        assert!(SubtensorModule::get_stake_availability_for_coldkeys(
            vec![coldkey],
            Some(vec![netuid]),
        )
        .get(&coldkey)
        .unwrap()
        .is_empty());
    });
}

#[test]
fn migration_classifies_and_is_idempotent() {
    new_test_ext(1).execute_with(|| {
        let root = NetUid::ROOT;
        let registered = NetUid::from(1);
        let emission_started = NetUid::from(2);
        let subtoken_started = NetUid::from(3);
        let queued = NetUid::from(4);
        let dissolving = NetUid::from(5);

        for netuid in [root, registered, emission_started, subtoken_started] {
            NetworksAdded::<Test>::insert(netuid, true);
        }
        FirstEmissionBlockNumber::<Test>::insert(emission_started, 10);
        SubtokenEnabled::<Test>::insert(subtoken_started, true);
        DissolveCleanupQueue::<Test>::set(vec![queued]);
        CurrentDissolveCleanupStatus::<Test>::set(Some(DissolveCleanupStatus::new(dissolving)));

        crate::migrations::migrate_subnet_state::migrate_subnet_state::<Test>();

        assert_eq!(
            SubnetState::<Test>::get(root),
            Some(SubnetLifecycleState::Started)
        );
        assert_eq!(
            SubnetState::<Test>::get(registered),
            Some(SubnetLifecycleState::Registered)
        );
        assert_eq!(
            SubnetState::<Test>::get(emission_started),
            Some(SubnetLifecycleState::Started)
        );
        assert_eq!(
            SubnetState::<Test>::get(subtoken_started),
            Some(SubnetLifecycleState::Started)
        );
        assert_eq!(
            SubnetState::<Test>::get(queued),
            Some(SubnetLifecycleState::PendingDissolution)
        );
        assert_eq!(
            SubnetState::<Test>::get(dissolving),
            Some(SubnetLifecycleState::Dissolving)
        );
        assert!(HasMigrationRun::<Test>::get(
            b"backfill_subnet_lifecycle_state_v1".to_vec()
        ));

        let before = SubnetState::<Test>::iter().collect::<Vec<_>>();
        crate::migrations::migrate_subnet_state::migrate_subnet_state::<Test>();
        assert_eq!(SubnetState::<Test>::iter().collect::<Vec<_>>(), before);
    });
}
