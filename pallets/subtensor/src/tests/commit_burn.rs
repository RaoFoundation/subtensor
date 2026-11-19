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
        assert_eq!(MechWeightDelegate::<Test>::get((index, hotkey)), None);
        assert!(
            pallet_commitments::ChannelMechanismPurgeQueue::<Test>::contains_key(netuid, mecid)
        );
        assert!(pallet_commitments::ChannelCommitmentOf::<Test>::get((index, hotkey, 7)).is_some());
        for block in 2..=4 {
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
        SubtensorModule::update_mechanism_counts_if_needed(netuid, MechId::from(2));
        assert_eq!(MechWeightDelegate::<Test>::get((index, hotkey)), None);
        assert!(pallet_commitments::ChannelCommitmentOf::<Test>::get((index, hotkey, 7)).is_none());
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
        SubtensorModule::queue_mech_weight_delegate_move(
            netuid,
            mecid,
            &hotkey,
            &new_hotkey,
            Some(delegate),
        );
        assert_noop!(
            SubtensorModule::do_set_mech_weight_delegate(
                RuntimeOrigin::signed(hotkey),
                netuid,
                mecid,
                None,
            ),
            Error::<Test>::NotMechWeightDelegate
        );
        SubtensorModule::drain_mech_weight_delegate_lifecycle(
            &mut frame_support::weights::WeightMeter::new(),
        );
        SubtensorModule::queue_mech_weight_delegate_move(
            netuid,
            mecid,
            &hotkey,
            &new_hotkey,
            Some(delegate),
        );
        SubtensorModule::purge_mechanism_weight_delegates(netuid, mecid);
        SubtensorModule::queue_mech_weight_delegate_move(
            netuid,
            mecid,
            &new_hotkey,
            &hotkey,
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
fn overlapping_delegate_moves_follow_membership_without_reviving_old_hotkeys() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let mecid = MechId::from(1);
        let index = SubtensorModule::get_mechanism_storage_index(netuid, mecid);
        let first = U256::from(55);
        let intermediate = U256::from(56);
        let final_hotkey = U256::from(57);
        let delegate = U256::from(99);
        MechanismCountCurrent::<Test>::insert(netuid, MechId::from(2));
        SubtensorModule::queue_mech_weight_delegate_move(
            netuid,
            mecid,
            &first,
            &intermediate,
            Some(delegate),
        );
        // Reverse before the first move drains. No grant exists on the source,
        // so the incoming move must be redirected or revoked, not left to land
        // on the old hotkey later.
        SubtensorModule::queue_mech_weight_delegate_move(
            netuid,
            mecid,
            &intermediate,
            &first,
            None,
        );
        SubtensorModule::drain_mech_weight_delegate_lifecycle(
            &mut frame_support::weights::WeightMeter::new(),
        );
        assert_eq!(MechWeightDelegate::<Test>::get((index, first)), None);
        assert_eq!(MechWeightDelegate::<Test>::get((index, intermediate)), None);
        assert!(MechWeightDelegateMoveQueue::<Test>::iter().next().is_none());

        // A later swap from the intermediate key must move that same grant to
        // the new destination, not strand it on an outgoing intermediate key.
        SubtensorModule::queue_mech_weight_delegate_move(
            netuid,
            mecid,
            &first,
            &intermediate,
            Some(delegate),
        );
        SubtensorModule::drain_mech_weight_delegate_lifecycle(
            &mut frame_support::weights::WeightMeter::new(),
        );
        SubtensorModule::queue_mech_weight_delegate_move(
            netuid,
            mecid,
            &intermediate,
            &final_hotkey,
            None,
        );
        SubtensorModule::drain_mech_weight_delegate_lifecycle(
            &mut frame_support::weights::WeightMeter::new(),
        );
        assert_eq!(
            MechWeightDelegate::<Test>::get((index, final_hotkey)),
            Some(delegate)
        );
        for hotkey in [first, intermediate] {
            assert_eq!(MechWeightDelegate::<Test>::get((index, hotkey)), None);
            assert!(
                MechWeightDelegateTargets::<Test>::iter_key_prefix((netuid, hotkey))
                    .next()
                    .is_none()
            );
        }

        SubtensorModule::queue_mech_weight_delegate_move(
            netuid,
            mecid,
            &final_hotkey,
            &intermediate,
            None,
        );
        SubtensorModule::drain_mech_weight_delegate_lifecycle(
            &mut frame_support::weights::WeightMeter::new(),
        );
        for hotkey in [first, intermediate, final_hotkey] {
            assert_eq!(MechWeightDelegate::<Test>::get((index, hotkey)), None);
        }
        assert!(MechWeightDelegateMoveQueue::<Test>::iter().next().is_none());
    });
}

#[test]
fn delegate_is_scoped_revocable_and_moves_on_hotkey_swap() {
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
        assert_eq!(
            MechWeightDelegate::<Test>::get((index, new_hotkey)),
            Some(delegate)
        );
        assert_eq!(MechWeightDelegate::<Test>::get((index, hotkey)), None);
    });
}
