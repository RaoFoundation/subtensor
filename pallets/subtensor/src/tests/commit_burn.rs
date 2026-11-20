#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use super::mock::*;
use crate::*;
use frame_support::{assert_noop, assert_ok};
use pallet_commitments::{
    ChannelCommitmentOf, CommitBurnConfig, CommitBurnParameters, CommitBurnSequence,
    CommitmentInfo, Data, Pallet as CommitmentsPallet,
};
use sp_core::U256;
use sp_runtime::BoundedVec;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{MechId, NetUidStorageIndex, TaoBalance, Token};

fn queue_delegate_uid_move(
    netuid: NetUid,
    old_hotkey: U256,
    new_hotkey: U256,
    mecid: MechId,
    delegate: Option<U256>,
) {
    if let Some(uid) = Uids::<Test>::get(netuid, old_hotkey) {
        Uids::<Test>::remove(netuid, old_hotkey);
        Uids::<Test>::insert(netuid, new_hotkey, uid);
    }
    SubtensorModule::queue_mech_weight_delegate_move(
        netuid,
        mecid,
        &old_hotkey,
        &new_hotkey,
        delegate,
    );
}

fn raw_info() -> CommitmentInfo<<Test as pallet_commitments::Config>::MaxFields> {
    CommitmentInfo {
        fields: BoundedVec::try_from(vec![Data::Raw(
            BoundedVec::try_from(b"channel commitment".to_vec()).unwrap(),
        )])
        .unwrap(),
    }
}

fn configure_burn(netuid: NetUid, mecid: MechId, channel: u8, enabled: bool) {
    CommitBurnConfig::<Test>::insert(
        (
            SubtensorModule::get_mechanism_storage_index(netuid, mecid),
            channel,
        ),
        CommitBurnParameters {
            enabled,
            burn: TaoBalance::from(1_000u64),
            min_burn: TaoBalance::from(500u64),
            max_burn: TaoBalance::from(10_000u64),
            half_life: 10,
            increase_mult: U64F64::from_num(2),
            last_update_block: SubtensorModule::get_current_block_as_u64(),
        },
    );
}

#[test]
fn channel_commit_is_free_by_default_and_does_not_replace_legacy() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey = U256::from(11);
        let coldkey = U256::from(12);
        add_network(netuid, 100, 0);
        setup_reserves(netuid, DEFAULT_RESERVE.into(), DEFAULT_RESERVE.into());
        register_ok_neuron(netuid, hotkey, coldkey, 0);
        add_balance_to_coldkey_account(&coldkey, TaoBalance::from(1_000_000u64));
        let before = Balances::free_balance(coldkey);

        assert_ok!(CommitmentsPallet::<Test>::set_commitment(
            RuntimeOrigin::signed(hotkey),
            netuid,
            Box::new(raw_info()),
        ));
        assert_ok!(CommitmentsPallet::<Test>::set_commitment_to(
            RuntimeOrigin::signed(coldkey),
            netuid,
            MechId::MAIN,
            0,
            hotkey,
            Box::new(raw_info()),
            TaoBalance::ZERO,
        ));
        assert_eq!(Balances::free_balance(coldkey), before);
        assert!(CommitmentsPallet::<Test>::commitment_of(netuid, hotkey).is_some());
        assert!(
            ChannelCommitmentOf::<Test>::get((NetUidStorageIndex::from(netuid), hotkey, 0u8))
                .is_some()
        );
        assert_eq!(
            CommitBurnSequence::<Test>::get((NetUidStorageIndex::from(netuid), 0)),
            0
        );
    });
}

#[test]
fn paid_commit_requires_owner_and_enforces_max_burn() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey = U256::from(11);
        let coldkey = U256::from(12);
        let other = U256::from(13);
        add_network(netuid, 100, 0);
        setup_reserves(netuid, DEFAULT_RESERVE.into(), DEFAULT_RESERVE.into());
        register_ok_neuron(netuid, hotkey, coldkey, 0);
        add_balance_to_coldkey_account(&coldkey, TaoBalance::from(1_000_000u64));
        configure_burn(netuid, MechId::MAIN, 0, true);

        assert_noop!(
            CommitmentsPallet::<Test>::set_commitment_to(
                RuntimeOrigin::signed(other),
                netuid,
                MechId::MAIN,
                0,
                hotkey,
                Box::new(raw_info()),
                TaoBalance::from(1_000u64),
            ),
            pallet_commitments::Error::<Test>::AccountNotAuthorized
        );
        assert_noop!(
            CommitmentsPallet::<Test>::set_commitment_to(
                RuntimeOrigin::signed(coldkey),
                netuid,
                MechId::MAIN,
                0,
                hotkey,
                Box::new(raw_info()),
                TaoBalance::from(999u64),
            ),
            pallet_commitments::Error::<Test>::BurnPriceLimitExceeded
        );
        assert!(
            ChannelCommitmentOf::<Test>::get((NetUidStorageIndex::from(netuid), hotkey, 0u8))
                .is_none()
        );
        assert_noop!(
            CommitmentsPallet::<Test>::set_commitment(
                RuntimeOrigin::signed(hotkey),
                netuid,
                Box::new(raw_info()),
            ),
            pallet_commitments::Error::<Test>::FreeCommitmentDisabled
        );
    });
}

#[test]
fn paid_commit_charges_coldkey_and_bumps_price_with_sequence() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey = U256::from(11);
        let coldkey = U256::from(12);
        add_network(netuid, 100, 0);
        setup_reserves(netuid, DEFAULT_RESERVE.into(), DEFAULT_RESERVE.into());
        register_ok_neuron(netuid, hotkey, coldkey, 0);
        add_balance_to_coldkey_account(&coldkey, TaoBalance::from(1_000_000u64));
        configure_burn(netuid, MechId::MAIN, 0, true);
        let before = Balances::free_balance(coldkey);

        assert_ok!(CommitmentsPallet::<Test>::set_commitment_to(
            RuntimeOrigin::signed(coldkey),
            netuid,
            MechId::MAIN,
            0,
            hotkey,
            Box::new(raw_info()),
            TaoBalance::from(1_000u64),
        ));
        assert_eq!(
            before.saturating_sub(Balances::free_balance(coldkey)),
            1_000u64.into()
        );
        assert_eq!(
            CommitBurnSequence::<Test>::get((NetUidStorageIndex::from(netuid), 0)),
            1
        );
        assert_eq!(
            SubtensorModule::get_commit_burn(netuid, MechId::MAIN, 0)
                .unwrap()
                .burn,
            2_000u64.into()
        );
        assert!(
            ChannelCommitmentOf::<Test>::get((NetUidStorageIndex::from(netuid), hotkey, 0u8))
                .is_some()
        );
    });
}

#[test]
fn paid_commit_bumps_the_price_after_decay() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey = U256::from(11);
        let coldkey = U256::from(12);
        add_network(netuid, 100, 0);
        setup_reserves(netuid, DEFAULT_RESERVE.into(), DEFAULT_RESERVE.into());
        register_ok_neuron(netuid, hotkey, coldkey, 0);
        add_balance_to_coldkey_account(&coldkey, TaoBalance::from(1_000_000u64));
        configure_burn(netuid, MechId::MAIN, 0, true);
        System::set_block_number(11);
        let current = SubtensorModule::get_commit_burn(netuid, MechId::MAIN, 0)
            .unwrap()
            .burn;
        assert!(current < 1_000u64.into());
        assert_ok!(CommitmentsPallet::<Test>::set_commitment_to(
            RuntimeOrigin::signed(coldkey),
            netuid,
            MechId::MAIN,
            0,
            hotkey,
            Box::new(raw_info()),
            current,
        ));
        assert_eq!(
            SubtensorModule::get_commit_burn(netuid, MechId::MAIN, 0)
                .unwrap()
                .burn,
            current.saturating_add(current),
        );
    });
}

#[test]
fn burn_decays_lazily_without_changing_storage_on_read() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 100, 0);
        configure_burn(netuid, MechId::MAIN, 3, true);
        let key = (NetUidStorageIndex::from(netuid), 3);
        let stored = CommitBurnConfig::<Test>::get(key).unwrap();
        System::set_block_number(11);
        let current = SubtensorModule::get_commit_burn(netuid, MechId::MAIN, 3).unwrap();
        assert!(current.burn < stored.burn);
        assert!(current.burn >= stored.min_burn);
        assert_eq!(current.last_update_block, 11);
        assert_eq!(CommitBurnConfig::<Test>::get(key).unwrap(), stored);
    });
}

#[test]
fn delegated_timelocked_commit_is_queued_for_validator() {
    new_test_ext(1).execute_with(|| {
        System::set_block_number(0);
        let netuid = NetUid::from(1);
        let hotkey = U256::from(55);
        let coldkey = U256::from(66);
        let delegate = U256::from(99);
        add_network(netuid, 100, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);
        SubtensorModule::set_stake_threshold(0);
        SubtensorModule::set_weights_set_rate_limit(netuid, 5);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        add_balance_to_coldkey_account(&coldkey, TaoBalance::from(1_000_000u64));
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            1.into(),
        );
        assert_ok!(SubtensorModule::set_mech_weight_delegate(
            RuntimeOrigin::signed(hotkey),
            netuid,
            MechId::MAIN,
            Some(delegate),
        ));
        assert_ok!(SubtensorModule::commit_timelocked_mechanism_weights_for(
            RuntimeOrigin::signed(delegate),
            netuid,
            MechId::MAIN,
            hotkey,
            vec![1, 2, 3].try_into().unwrap(),
            u64::MAX,
            SubtensorModule::get_commit_reveal_weights_version(),
        ));
        let epoch = SubtensorModule::current_epoch_with_lookahead(netuid);
        let commits = TimelockedWeightCommits::<Test>::get(NetUidStorageIndex::from(netuid), epoch);
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].0, hotkey);
    });
}

#[test]
fn delegated_hash_commit_reveal_uses_validator_commit_and_uid_row() {
    new_test_ext(1).execute_with(|| {
        System::set_block_number(0);
        let netuid = NetUid::from(1);
        let hotkey = U256::from(55);
        let coldkey = U256::from(66);
        let delegate = U256::from(99);
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);
        register_ok_neuron(netuid, U256::from(77), U256::from(88), 0);
        register_ok_neuron(netuid, U256::from(99), U256::from(111), 0);
        SubtensorModule::set_stake_threshold(0);
        SubtensorModule::set_weights_set_rate_limit(netuid, 5);
        SubtensorModule::set_validator_permit_for_uid(netuid, 0, true);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        add_balance_to_coldkey_account(&coldkey, TaoBalance::from(1_000_000u64));
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            1.into(),
        );
        assert_ok!(SubtensorModule::set_mech_weight_delegate(
            RuntimeOrigin::signed(hotkey),
            netuid,
            MechId::MAIN,
            Some(delegate),
        ));
        let uids = vec![1, 2];
        let values = vec![88u16, u16::MAX];
        let salt = vec![1u16, 2, 3, 4];
        let commit_hash =
            SubtensorModule::get_commit_hash(&hotkey, netuid.into(), &uids, &values, &salt, 0);
        assert_ok!(SubtensorModule::commit_mechanism_weights_for(
            RuntimeOrigin::signed(delegate),
            netuid,
            MechId::MAIN,
            hotkey,
            commit_hash,
        ));
        assert!(WeightCommits::<Test>::get(NetUidStorageIndex::from(netuid), hotkey).is_some());
        assert!(WeightCommits::<Test>::get(NetUidStorageIndex::from(netuid), delegate).is_none());
        step_epochs(1, netuid);
        assert_ok!(SubtensorModule::reveal_mechanism_weights_for(
            RuntimeOrigin::signed(delegate),
            netuid,
            MechId::MAIN,
            hotkey,
            uids,
            values,
            salt,
            0,
        ));
        assert_eq!(
            Weights::<Test>::get(NetUidStorageIndex::from(netuid), 0),
            vec![(1, 88), (2, u16::MAX)]
        );
    });
}

#[test]
fn removing_mechanism_clears_channels_delegates_and_their_burn_config() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let mecid = MechId::from(1);
        let hotkey = U256::from(55);
        let coldkey = U256::from(66);
        let delegate = U256::from(99);
        add_network(netuid, 100, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);
        MechanismCountCurrent::<Test>::insert(netuid, MechId::from(2));
        let index = SubtensorModule::get_mechanism_storage_index(netuid, mecid);
        MechWeightDelegate::<Test>::insert((index, hotkey), delegate);
        assert_ok!(CommitmentsPallet::<Test>::set_commitment_to(
            RuntimeOrigin::signed(coldkey),
            netuid,
            mecid,
            7,
            hotkey,
            Box::new(raw_info()),
            TaoBalance::ZERO,
        ));
        configure_burn(netuid, mecid, 7, true);
        SubtensorModule::update_mechanism_counts_if_needed(netuid, MechId::MAIN);
        SubtensorModule::drain_mech_weight_delegate_lifecycle(
            &mut frame_support::weights::WeightMeter::new(),
        );
        assert_eq!(MechWeightDelegate::<Test>::get((index, hotkey)), None);
        assert!(
            pallet_commitments::ChannelMechanismPurgeQueue::<Test>::contains_key(netuid, mecid)
        );
        assert!(pallet_commitments::ChannelCommitmentOf::<Test>::get((index, hotkey, 7)).is_some());
        for block in 2..=8 {
            if !pallet_commitments::ChannelMechanismPurgeQueue::<Test>::contains_key(netuid, mecid)
            {
                break;
            }
            <pallet_commitments::Pallet<Test> as frame_support::traits::Hooks<u64>>::on_idle(
                block,
                frame_support::weights::Weight::MAX,
            );
        }
        assert!(pallet_commitments::ChannelCommitmentOf::<Test>::get((index, hotkey, 7)).is_none());
        assert!(pallet_commitments::CommitBurnConfig::<Test>::get((index, 7)).is_none());
        assert!(
            pallet_commitments::ChannelTargetsOf::<Test>::iter_key_prefix((netuid, hotkey))
                .next()
                .is_none()
        );
        assert!(
            pallet_commitments::ChannelMechanismPurgeQueue::<Test>::iter_prefix(netuid)
                .next()
                .is_none()
        );
        SubtensorModule::update_mechanism_counts_if_needed(netuid, MechId::from(2));
        assert_eq!(MechWeightDelegate::<Test>::get((index, hotkey)), None);
        assert!(pallet_commitments::ChannelCommitmentOf::<Test>::get((index, hotkey, 7)).is_none());
    });
}

#[test]
fn dissolution_clears_delegates_from_already_retired_mechanisms() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey = U256::from(55);
        let delegate = U256::from(99);
        MechanismCountCurrent::<Test>::insert(netuid, MechId::from(2));
        let retired_index = SubtensorModule::get_mechanism_storage_index(netuid, MechId::from(1));
        MechWeightDelegate::<Test>::insert((retired_index, hotkey), delegate);
        MechWeightDelegateTargets::<Test>::insert((netuid, hotkey, MechId::from(1)), ());
        SubtensorModule::update_mechanism_counts_if_needed(netuid, MechId::MAIN);
        assert!(MechWeightDelegateRetirementQueue::<Test>::contains_key(
            netuid,
            MechId::from(1)
        ));
        let mut meter = frame_support::weights::WeightMeter::new();
        assert!(SubtensorModule::remove_network_map_parameters(
            netuid, &mut meter
        ));
        assert_eq!(
            MechWeightDelegate::<Test>::get((retired_index, hotkey)),
            None
        );
        assert!(MechWeightDelegate::<Test>::iter().next().is_none());
        assert!(
            MechWeightDelegateRetirementQueue::<Test>::iter()
                .next()
                .is_none()
        );
    });
}

#[test]
fn queued_delegate_move_cannot_revive_after_revocation_or_retirement() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let mecid = MechId::from(1);
        let index = SubtensorModule::get_mechanism_storage_index(netuid, mecid);
        let hotkey = U256::from(55);
        let new_hotkey = U256::from(56);
        let delegate = U256::from(99);
        add_network(netuid, 100, 0);
        register_ok_neuron(netuid, hotkey, U256::from(66), 0);
        MechanismCountCurrent::<Test>::insert(netuid, MechId::from(2));
        assert_ok!(SubtensorModule::do_set_mech_weight_delegate(
            RuntimeOrigin::signed(hotkey),
            netuid,
            mecid,
            Some(delegate),
        ));
        assert_ok!(SubtensorModule::do_set_mech_weight_delegate(
            RuntimeOrigin::signed(hotkey),
            netuid,
            mecid,
            None,
        ));
        assert!(
            MechWeightDelegateTargets::<Test>::iter_key_prefix((netuid, hotkey))
                .next()
                .is_none()
        );
        queue_delegate_uid_move(netuid, hotkey, new_hotkey, mecid, None);
        assert!(MechWeightDelegateMoveQueue::<Test>::iter().next().is_none());
        assert_ok!(SubtensorModule::do_set_mech_weight_delegate(
            RuntimeOrigin::signed(new_hotkey),
            netuid,
            mecid,
            Some(delegate),
        ));
        SubtensorModule::drain_mech_weight_delegate_lifecycle(
            &mut frame_support::weights::WeightMeter::new(),
        );
        assert_eq!(
            MechWeightDelegate::<Test>::get((index, new_hotkey)),
            Some(delegate)
        );
        SubtensorModule::purge_mechanism_weight_delegates(netuid, mecid);
        queue_delegate_uid_move(
            netuid,
            new_hotkey,
            hotkey,
            mecid,
            MechWeightDelegate::<Test>::get((index, new_hotkey)),
        );
        SubtensorModule::drain_mech_weight_delegate_lifecycle(
            &mut frame_support::weights::WeightMeter::new(),
        );
        for validator in [hotkey, new_hotkey] {
            assert_eq!(MechWeightDelegate::<Test>::get((index, validator)), None);
            assert!(
                MechWeightDelegateTargets::<Test>::iter_key_prefix((netuid, validator))
                    .next()
                    .is_none()
            );
        }
        assert!(MechWeightDelegateMoveQueue::<Test>::iter().next().is_none());
    });
}

#[test]
fn delegate_lifecycle_rejects_destination_override_and_handles_uid_changes() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let mecid = MechId::from(1);
        let index = SubtensorModule::get_mechanism_storage_index(netuid, mecid);
        let source = U256::from(55);
        let destination = U256::from(56);
        let delegate = U256::from(99);
        add_network(netuid, 100, 0);
        MechanismCountCurrent::<Test>::insert(netuid, MechId::from(2));
        Uids::<Test>::insert(netuid, source, 0u16);
        MechWeightDelegate::<Test>::insert((index, source), delegate);
        MechWeightDelegateTargets::<Test>::insert((netuid, source, mecid), ());
        queue_delegate_uid_move(netuid, source, destination, mecid, None);
        assert_noop!(
            SubtensorModule::do_set_mech_weight_delegate(
                RuntimeOrigin::signed(destination),
                netuid,
                mecid,
                None,
            ),
            Error::<Test>::NotMechWeightDelegate
        );
        assert_noop!(
            SubtensorModule::do_set_mech_weight_delegate(
                RuntimeOrigin::signed(destination),
                netuid,
                mecid,
                Some(U256::from(100)),
            ),
            Error::<Test>::NotMechWeightDelegate
        );

        // Re-registering the old source independently must not cancel a valid
        // move, nor delete a distinct grant already owned by the destination.
        Uids::<Test>::insert(netuid, source, 1u16);
        SubtensorModule::purge_mech_weight_delegates(netuid, &source, None);
        SubtensorModule::drain_mech_weight_delegate_lifecycle(
            &mut frame_support::weights::WeightMeter::new(),
        );
        assert_eq!(
            MechWeightDelegate::<Test>::get((index, destination)),
            Some(delegate)
        );
        assert!(MechWeightDelegateMoveQueue::<Test>::iter().next().is_none());
        assert!(
            MechWeightDelegateMoveOrigins::<Test>::iter()
                .next()
                .is_none()
        );
        SubtensorModule::drain_mech_weight_delegate_lifecycle(
            &mut frame_support::weights::WeightMeter::new(),
        );

        // Once the destination UID is gone, a source purge invalidates the
        // pending move and also removes its destination target index.
        let next = U256::from(57);
        queue_delegate_uid_move(netuid, destination, next, mecid, None);
        MechWeightDelegate::<Test>::insert((index, next), delegate);
        MechWeightDelegateTargets::<Test>::insert((netuid, next, mecid), ());
        Uids::<Test>::remove(netuid, next);
        SubtensorModule::purge_mech_weight_delegates(netuid, &destination, None);
        SubtensorModule::drain_mech_weight_delegate_lifecycle(
            &mut frame_support::weights::WeightMeter::new(),
        );
        assert_eq!(MechWeightDelegate::<Test>::get((index, next)), None);
        assert!(!MechWeightDelegateTargets::<Test>::contains_key((
            netuid, next, mecid
        )));
        assert!(MechWeightDelegateMoveQueue::<Test>::iter().next().is_none());
        assert!(
            MechWeightDelegateMoveOrigins::<Test>::iter()
                .next()
                .is_none()
        );
    });
}

#[test]
fn delegate_move_remaps_uid_when_trim_compacts_surviving_registration() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let mecid = MechId::from(1);
        let index = SubtensorModule::get_mechanism_storage_index(netuid, mecid);
        let removed = U256::from(1);
        let source = U256::from(2);
        let destination = U256::from(3);
        let delegate = U256::from(99);
        add_network(netuid, 13, 0);
        MinAllowedUids::<Test>::insert(netuid, 2);
        SubtensorModule::set_immunity_period(netuid, 0);
        MechanismCountCurrent::<Test>::insert(netuid, MechId::from(2));
        for hotkey in [removed, source, destination] {
            SubtensorModule::append_neuron(netuid, &hotkey, 0);
        }
        Emission::<Test>::insert(
            netuid,
            vec![0.into(), 2.into(), 1.into()] as Vec<subtensor_runtime_common::AlphaBalance>,
        );
        MechWeightDelegate::<Test>::insert((index, source), delegate);
        MechWeightDelegateTargets::<Test>::insert((netuid, source, mecid), ());
        queue_delegate_uid_move(netuid, source, destination, mecid, None);

        assert_ok!(SubtensorModule::trim_to_max_allowed_uids(netuid, 2));
        assert_eq!(Uids::<Test>::get(netuid, destination), Some(1));
        SubtensorModule::drain_mech_weight_delegate_lifecycle(
            &mut frame_support::weights::WeightMeter::new(),
        );
        assert_eq!(
            MechWeightDelegate::<Test>::get((index, destination)),
            Some(delegate)
        );
        assert!(MechWeightDelegateMoveQueue::<Test>::iter().next().is_none());
        assert!(
            MechWeightDelegateMoveOrigins::<Test>::iter()
                .next()
                .is_none()
        );
    });
}

#[test]
fn overlapping_delegate_moves_follow_membership_without_reviving_old_hotkeys() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 100, 0);
        let mecid = MechId::from(1);
        let index = SubtensorModule::get_mechanism_storage_index(netuid, mecid);
        let first = U256::from(55);
        let intermediate = U256::from(56);
        let final_hotkey = U256::from(57);
        let delegate = U256::from(99);
        MechanismCountCurrent::<Test>::insert(netuid, MechId::from(2));
        Uids::<Test>::insert(netuid, first, 0u16);
        MechWeightDelegate::<Test>::insert((index, first), delegate);
        MechWeightDelegateTargets::<Test>::insert((netuid, first, mecid), ());
        queue_delegate_uid_move(netuid, first, intermediate, mecid, None);
        SubtensorModule::drain_mech_weight_delegate_lifecycle(
            &mut frame_support::weights::WeightMeter::new(),
        );
        assert_eq!(
            MechWeightDelegate::<Test>::get((index, intermediate)),
            Some(delegate)
        );

        // A reverse rename before either edge drains restores the grant on the
        // hotkey that currently owns the UID, with no residual move metadata.
        queue_delegate_uid_move(netuid, intermediate, final_hotkey, mecid, None);
        queue_delegate_uid_move(netuid, final_hotkey, intermediate, mecid, None);
        SubtensorModule::drain_mech_weight_delegate_lifecycle(
            &mut frame_support::weights::WeightMeter::new(),
        );
        assert_eq!(
            MechWeightDelegate::<Test>::get((index, intermediate)),
            Some(delegate)
        );
        assert!(MechWeightDelegateMoveQueue::<Test>::iter().next().is_none());
        assert!(
            MechWeightDelegateMoveOrigins::<Test>::iter()
                .next()
                .is_none()
        );

        // A second rename before the first drains redirects the grant to the
        // current UID owner rather than installing it on the intermediate key.
        queue_delegate_uid_move(netuid, first, intermediate, mecid, Some(delegate));
        queue_delegate_uid_move(netuid, intermediate, final_hotkey, mecid, None);
        SubtensorModule::drain_mech_weight_delegate_lifecycle(
            &mut frame_support::weights::WeightMeter::new(),
        );
        assert_eq!(
            MechWeightDelegate::<Test>::get((index, final_hotkey)),
            Some(delegate)
        );
        queue_delegate_uid_move(netuid, final_hotkey, intermediate, mecid, None);
        SubtensorModule::drain_mech_weight_delegate_lifecycle(
            &mut frame_support::weights::WeightMeter::new(),
        );
        assert_eq!(
            MechWeightDelegate::<Test>::get((index, intermediate)),
            Some(delegate)
        );
        for hotkey in [first, final_hotkey] {
            assert_eq!(MechWeightDelegate::<Test>::get((index, hotkey)), None);
            assert!(
                MechWeightDelegateTargets::<Test>::iter_key_prefix((netuid, hotkey))
                    .next()
                    .is_none()
            );
        }

        // Reusing the former source as an independently registered validator
        // must not inherit any lineage from the completed move.
        Uids::<Test>::insert(netuid, first, 1u16);
        MechWeightDelegate::<Test>::insert((index, first), U256::from(100));
        MechWeightDelegateTargets::<Test>::insert((netuid, first, mecid), ());
        SubtensorModule::purge_mech_weight_delegates(netuid, &final_hotkey, None);
        SubtensorModule::drain_mech_weight_delegate_lifecycle(
            &mut frame_support::weights::WeightMeter::new(),
        );
        assert_eq!(
            MechWeightDelegate::<Test>::get((index, first)),
            Some(U256::from(100))
        );
        MechWeightDelegate::<Test>::remove((index, first));
        MechWeightDelegateTargets::<Test>::remove((netuid, first, mecid));
        Uids::<Test>::remove(netuid, first);
        queue_delegate_uid_move(netuid, final_hotkey, intermediate, mecid, None);
        SubtensorModule::drain_mech_weight_delegate_lifecycle(
            &mut frame_support::weights::WeightMeter::new(),
        );

        // A grant that already landed must not survive retirement merely because
        // a newer outgoing edge supersedes its incoming edge.
        queue_delegate_uid_move(netuid, intermediate, final_hotkey, mecid, None);
        SubtensorModule::purge_mechanism_weight_delegates(netuid, mecid);
        SubtensorModule::drain_mech_weight_delegate_lifecycle(
            &mut frame_support::weights::WeightMeter::new(),
        );
        for hotkey in [first, intermediate, final_hotkey] {
            assert_eq!(MechWeightDelegate::<Test>::get((index, hotkey)), None);
            assert!(
                MechWeightDelegateTargets::<Test>::iter_key_prefix((netuid, hotkey))
                    .next()
                    .is_none()
            );
        }
        assert!(MechWeightDelegateMoveQueue::<Test>::iter().next().is_none());
        assert!(
            MechWeightDelegateMoveOrigins::<Test>::iter()
                .next()
                .is_none()
        );
        assert!(
            MechWeightDelegatePurgeQueue::<Test>::iter()
                .next()
                .is_none()
        );
        assert!(
            MechWeightDelegateMechanismMoves::<Test>::iter_keys()
                .next()
                .is_none()
        );
        assert!(
            MechWeightDelegateRetirementQueue::<Test>::iter()
                .next()
                .is_none()
        );
        SubtensorModule::update_mechanism_counts_if_needed(netuid, MechId::from(2));

        // A pending destination purge revokes grants on the pending source's
        // other mechanisms immediately, before bounded deletion completes.
        let other = MechId::MAIN;
        let other_index = SubtensorModule::get_mechanism_storage_index(netuid, other);
        Uids::<Test>::insert(netuid, first, 0u16);
        MechWeightDelegate::<Test>::insert((other_index, first), delegate);
        MechWeightDelegateTargets::<Test>::insert((netuid, first, other), ());
        queue_delegate_uid_move(netuid, first, intermediate, mecid, Some(delegate));
        SubtensorModule::purge_mech_weight_delegates(netuid, &intermediate, None);
        SubtensorModule::drain_mech_weight_delegate_lifecycle(
            &mut frame_support::weights::WeightMeter::with_limit(
                SubtensorModule::mech_weight_delegate_lifecycle_step_weight(),
            ),
        );
        assert!(SubtensorModule::mech_weight_delegate_lifecycle_pending(
            netuid, &first
        ));
        assert_noop!(
            SubtensorModule::do_set_mechanism_weights_for(
                RuntimeOrigin::signed(delegate),
                netuid,
                other,
                first,
                vec![0],
                vec![u16::MAX],
                0,
            ),
            Error::<Test>::NotMechWeightDelegate
        );
        SubtensorModule::drain_mech_weight_delegate_lifecycle(
            &mut frame_support::weights::WeightMeter::new(),
        );
        for hotkey in [first, intermediate] {
            assert_eq!(MechWeightDelegate::<Test>::get((index, hotkey)), None);
            assert_eq!(MechWeightDelegate::<Test>::get((other_index, hotkey)), None);
        }
        assert!(
            MechWeightDelegateMoveOrigins::<Test>::iter()
                .next()
                .is_none()
        );

        // Compaction leaves one edge per mechanism, so either account ordering
        // drains both grants in two metered steps without aliasing mechanisms.
        let other_delegate = U256::from(100);
        for accounts in [
            (U256::from(1), U256::from(2), U256::from(3)),
            (U256::from(3), U256::from(2), U256::from(1)),
        ] {
            let (source, middle, destination) = accounts;
            Uids::<Test>::insert(netuid, source, 0u16);
            MechWeightDelegate::<Test>::insert((index, source), delegate);
            MechWeightDelegateTargets::<Test>::insert((netuid, source, mecid), ());
            MechWeightDelegate::<Test>::insert((other_index, source), other_delegate);
            MechWeightDelegateTargets::<Test>::insert((netuid, source, other), ());
            queue_delegate_uid_move(netuid, source, middle, mecid, None);
            queue_delegate_uid_move(netuid, source, middle, other, None);
            queue_delegate_uid_move(netuid, middle, destination, mecid, None);
            queue_delegate_uid_move(netuid, middle, destination, other, None);
            for _ in 0..2 {
                SubtensorModule::drain_mech_weight_delegate_lifecycle(
                    &mut frame_support::weights::WeightMeter::with_limit(
                        SubtensorModule::mech_weight_delegate_lifecycle_step_weight(),
                    ),
                );
            }
            assert_eq!(
                MechWeightDelegate::<Test>::get((index, destination)),
                Some(delegate)
            );
            assert_eq!(
                MechWeightDelegate::<Test>::get((other_index, destination)),
                Some(other_delegate)
            );
            assert!(MechWeightDelegateMoveQueue::<Test>::iter().next().is_none());
            assert!(
                MechWeightDelegateMoveOrigins::<Test>::iter()
                    .next()
                    .is_none()
            );
            assert!(
                MechWeightDelegatePurgeQueue::<Test>::iter()
                    .next()
                    .is_none()
            );
            MechWeightDelegate::<Test>::remove((index, destination));
            MechWeightDelegateTargets::<Test>::remove((netuid, destination, mecid));
            MechWeightDelegate::<Test>::remove((other_index, destination));
            MechWeightDelegateTargets::<Test>::remove((netuid, destination, other));
            Uids::<Test>::remove(netuid, destination);
        }
    });
}

#[test]
fn reused_source_pruning_does_not_delete_its_new_registration_grant() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let mecid = MechId::from(1);
        let index = SubtensorModule::get_mechanism_storage_index(netuid, mecid);
        let source = U256::from(55);
        let intermediate = U256::from(56);
        let destination = U256::from(57);
        let displaced = U256::from(58);
        let old_delegate = U256::from(99);
        let new_delegate = U256::from(100);
        add_network(netuid, 100, 0);
        SubtensorModule::set_immunity_period(netuid, 0);
        MechanismCountCurrent::<Test>::insert(netuid, MechId::from(2));
        SubtensorModule::append_neuron(netuid, &source, 0);
        MechWeightDelegate::<Test>::insert((index, source), old_delegate);
        MechWeightDelegateTargets::<Test>::insert((netuid, source, mecid), ());
        queue_delegate_uid_move(netuid, source, intermediate, mecid, None);
        queue_delegate_uid_move(netuid, intermediate, destination, mecid, None);
        // The old source is now an independent registration with a different
        // delegate. Pruning it cancels the old pending lineage, not this grant.
        SubtensorModule::append_neuron(netuid, &displaced, 0);
        SubtensorModule::append_neuron(netuid, &source, 0);
        for offset in 200..202 {
            SubtensorModule::append_neuron(netuid, &U256::from(offset), 0);
        }
        Emission::<Test>::insert(
            netuid,
            vec![1.into(), 2.into(), 0.into(), 3.into(), 4.into()]
                as Vec<subtensor_runtime_common::AlphaBalance>,
        );
        MechWeightDelegate::<Test>::insert((index, source), new_delegate);
        MechWeightDelegateTargets::<Test>::insert((netuid, source, mecid), ());
        assert_ok!(SubtensorModule::trim_to_max_allowed_uids(netuid, 4));
        // Compaction preserves the source's surviving UID-0 registration. The
        // bounded drain must discard the obsolete purge, not its new grant.
        assert_eq!(Uids::<Test>::get(netuid, source), Some(0));
        assert!(MechWeightDelegatePurgeQueue::<Test>::contains_key(
            netuid, source
        ));
        SubtensorModule::drain_mech_weight_delegate_lifecycle(
            &mut frame_support::weights::WeightMeter::new(),
        );
        assert_eq!(
            MechWeightDelegate::<Test>::get((index, source)),
            Some(new_delegate)
        );
        assert!(MechWeightDelegateTargets::<Test>::contains_key((
            netuid, source, mecid
        )));
        assert_eq!(MechWeightDelegate::<Test>::get((index, destination)), None);
        assert!(MechWeightDelegateMoveQueue::<Test>::iter().next().is_none());
        assert!(
            MechWeightDelegateMoveOrigins::<Test>::iter()
                .next()
                .is_none()
        );
    });
}

// A reverse swap can restore a grant on the old hotkey, but that grant belongs
// to the old registration and must not authorize a later independent one.
// PROTECTED_REGRESSION: changes require explicit human approval.
#[test]
fn reverse_swap_delegate_does_not_survive_independent_reregistration() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let mecid = MechId::MAIN;
        let index = SubtensorModule::get_mechanism_storage_index(netuid, mecid);
        let old_hotkey = U256::from(55);
        let new_hotkey = U256::from(56);
        let displaced = U256::from(57);
        let delegate = U256::from(99);
        add_network_disable_commit_reveal(netuid, 100, 0);
        SubtensorModule::set_stake_threshold(0);
        SubtensorModule::set_immunity_period(netuid, 0);
        for hotkey in [old_hotkey, displaced] {
            SubtensorModule::append_neuron(netuid, &hotkey, 0);
        }
        assert_ok!(SubtensorModule::do_set_mech_weight_delegate(
            RuntimeOrigin::signed(old_hotkey),
            netuid,
            mecid,
            Some(delegate),
        ));
        queue_delegate_uid_move(netuid, old_hotkey, new_hotkey, mecid, None);
        queue_delegate_uid_move(netuid, new_hotkey, old_hotkey, mecid, None);
        assert_eq!(
            MechWeightDelegate::<Test>::get((index, old_hotkey)),
            Some(delegate)
        );

        // The second registration is independent and receives a new UID.
        SubtensorModule::append_neuron(netuid, &new_hotkey, 0);
        SubtensorModule::append_neuron(netuid, &old_hotkey, 0);
        assert_ne!(Uids::<Test>::get(netuid, old_hotkey), Some(0));
        assert!(MechWeightDelegatePurgeQueue::<Test>::contains_key(
            netuid, old_hotkey
        ));
        SubtensorModule::drain_mech_weight_delegate_lifecycle(
            &mut frame_support::weights::WeightMeter::new(),
        );

        assert_eq!(MechWeightDelegate::<Test>::get((index, old_hotkey)), None);
        assert!(!SubtensorModule::mech_weight_delegate_lifecycle_pending(
            netuid,
            &old_hotkey
        ));
        assert_noop!(
            SubtensorModule::do_set_mechanism_weights_for(
                RuntimeOrigin::signed(delegate),
                netuid,
                mecid,
                old_hotkey,
                vec![0],
                vec![u16::MAX],
                0,
            ),
            Error::<Test>::NotMechWeightDelegate
        );
        assert_ok!(SubtensorModule::do_set_mech_weight_delegate(
            RuntimeOrigin::signed(old_hotkey),
            netuid,
            mecid,
            Some(U256::from(100)),
        ));
    });
}

// Reused validator hotkeys must keep independent grants and must not leave stale
// lifecycle rows that permanently block new delegations after UID pruning.
// PROTECTED_REGRESSION: changes require explicit human approval.
#[test]
fn reused_source_keeps_new_grant_when_pending_destination_is_pruned() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let mecid = MechId::from(1);
        let index = SubtensorModule::get_mechanism_storage_index(netuid, mecid);
        let source = U256::from(55);
        let intermediate = U256::from(56);
        let destination = U256::from(57);
        let displaced = U256::from(58);
        let old_delegate = U256::from(99);
        let new_delegate = U256::from(100);
        add_network(netuid, 100, 0);
        SubtensorModule::set_immunity_period(netuid, 0);
        MechanismCountCurrent::<Test>::insert(netuid, MechId::from(2));
        SubtensorModule::append_neuron(netuid, &source, 0);
        MechWeightDelegate::<Test>::insert((index, source), old_delegate);
        MechWeightDelegateTargets::<Test>::insert((netuid, source, mecid), ());
        queue_delegate_uid_move(netuid, source, intermediate, mecid, None);
        queue_delegate_uid_move(netuid, intermediate, destination, mecid, None);

        // Source and destination now represent independent registrations. Both
        // survive compaction with their original UIDs, while a lower-emission
        // registration is removed.
        SubtensorModule::append_neuron(netuid, &displaced, 0);
        SubtensorModule::append_neuron(netuid, &source, 0);
        SubtensorModule::append_neuron(netuid, &U256::from(200), 0);
        Emission::<Test>::insert(
            netuid,
            vec![1.into(), 2.into(), 0.into(), 3.into(), 4.into()]
                as Vec<subtensor_runtime_common::AlphaBalance>,
        );
        MechWeightDelegate::<Test>::insert((index, source), new_delegate);
        MechWeightDelegateTargets::<Test>::insert((netuid, source, mecid), ());
        assert_ok!(SubtensorModule::trim_to_max_allowed_uids(netuid, 4));
        assert!(Uids::<Test>::contains_key(netuid, source));
        assert!(Uids::<Test>::contains_key(netuid, destination));
        assert_ok!(SubtensorModule::trim_to_max_allowed_uids(netuid, 3));
        assert!(Uids::<Test>::contains_key(netuid, source));
        assert_eq!(Uids::<Test>::get(netuid, destination), None);

        SubtensorModule::drain_mech_weight_delegate_lifecycle(
            &mut frame_support::weights::WeightMeter::new(),
        );

        assert_eq!(
            MechWeightDelegate::<Test>::get((index, source)),
            Some(new_delegate)
        );
        assert!(MechWeightDelegateTargets::<Test>::contains_key((
            netuid, source, mecid
        )));
        assert!(!SubtensorModule::mech_weight_delegate_lifecycle_pending(
            netuid, &source
        ));
        assert!(
            MechWeightDelegatePurgeQueue::<Test>::iter()
                .next()
                .is_none()
        );
        assert!(MechWeightDelegateMoveQueue::<Test>::iter().next().is_none());
        assert_ok!(SubtensorModule::do_set_mech_weight_delegate(
            RuntimeOrigin::signed(source),
            netuid,
            mecid,
            Some(old_delegate),
        ));
    });
}

#[test]
fn delegate_is_scoped_revocable_and_revoked_on_hotkey_swap() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey = U256::from(55);
        let new_hotkey = U256::from(56);
        let coldkey = U256::from(66);
        let delegate = U256::from(99);
        add_network_disable_commit_reveal(netuid, 100, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);
        register_ok_neuron(netuid, U256::from(77), U256::from(88), 0);
        MechanismCountCurrent::<Test>::insert(netuid, MechId::from(2u8));
        SubtensorModule::set_stake_threshold(0);
        SubtensorModule::set_validator_permit_for_uid(netuid, 0, true);
        add_balance_to_coldkey_account(&coldkey, TaoBalance::from(1_000_000u64));
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            1.into(),
        );

        assert_ok!(SubtensorModule::set_mech_weight_delegate(
            RuntimeOrigin::signed(hotkey),
            netuid,
            MechId::from(1u8),
            Some(delegate),
        ));
        assert_noop!(
            SubtensorModule::set_mechanism_weights_for(
                RuntimeOrigin::signed(delegate),
                netuid,
                MechId::MAIN,
                hotkey,
                vec![1],
                vec![u16::MAX],
                0,
            ),
            Error::<Test>::NotMechWeightDelegate
        );
        assert_noop!(
            SubtensorModule::set_mechanism_weights_for(
                RuntimeOrigin::signed(U256::from(100)),
                netuid,
                MechId::from(1u8),
                hotkey,
                vec![1],
                vec![u16::MAX],
                0,
            ),
            Error::<Test>::NotMechWeightDelegate
        );
        assert_ok!(SubtensorModule::set_mechanism_weights_for(
            RuntimeOrigin::signed(delegate),
            netuid,
            MechId::from(1u8),
            hotkey,
            vec![1],
            vec![u16::MAX],
            0,
        ));
        let index = SubtensorModule::get_mechanism_storage_index(netuid, MechId::from(1u8));
        assert_eq!(Weights::<Test>::get(index, 0), vec![(1, u16::MAX)]);
        assert_ok!(SubtensorModule::set_mech_weight_delegate(
            RuntimeOrigin::signed(hotkey),
            netuid,
            MechId::from(1u8),
            None,
        ));
        assert_noop!(
            SubtensorModule::set_mechanism_weights_for(
                RuntimeOrigin::signed(delegate),
                netuid,
                MechId::from(1u8),
                hotkey,
                vec![1],
                vec![u16::MAX],
                0,
            ),
            Error::<Test>::NotMechWeightDelegate
        );

        assert_ok!(SubtensorModule::set_mech_weight_delegate(
            RuntimeOrigin::signed(hotkey),
            netuid,
            MechId::from(1u8),
            Some(delegate),
        ));
        add_balance_to_coldkey_account(&coldkey, TaoBalance::from(1_000_000_000u64));
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(coldkey),
            &hotkey,
            &new_hotkey,
            Some(netuid),
            true,
        ));
        assert_eq!(MechWeightDelegate::<Test>::get((index, new_hotkey)), None);
        SubtensorModule::drain_mech_weight_delegate_lifecycle(
            &mut frame_support::weights::WeightMeter::new(),
        );
        assert_eq!(MechWeightDelegate::<Test>::get((index, new_hotkey)), None);
        assert_eq!(MechWeightDelegate::<Test>::get((index, hotkey)), None);
        assert_ok!(SubtensorModule::set_mech_weight_delegate(
            RuntimeOrigin::signed(new_hotkey),
            netuid,
            MechId::from(1u8),
            Some(delegate),
        ));
        assert_eq!(
            MechWeightDelegate::<Test>::get((index, new_hotkey)),
            Some(delegate)
        );
    });
}
