#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::mock::*;
use crate::subnets::mechanism::{
    MAX_MECHANISM_COUNT_PER_SUBNET, NULL_UID_BUDGET, TOTAL_NULL_WEIGHT_BUDGET,
};
use crate::*;
use codec::Compact;
use frame_support::{assert_noop, assert_ok};
use sp_core::U256;
use subtensor_runtime_common::{MechId, NetUid, NetUidStorageIndex};

fn setup_null_subnet(mechanisms: u8, uid_count: u16) -> NetUid {
    let netuid = NetUid::from(1);
    add_network_disable_commit_reveal(netuid, u16::MAX - 1, 0);
    SubtensorModule::set_max_allowed_uids(netuid, NULL_UID_BUDGET);
    SubtensorModule::set_max_allowed_validators(netuid, 1);
    SubtensorModule::set_weights_set_rate_limit(netuid, 0);
    for uid in 0..uid_count {
        let hotkey = U256::from(uid);
        SubtensorModule::append_neuron(netuid, &hotkey, 0);
    }
    SubnetworkN::<Test>::insert(netuid, uid_count);
    SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
        &U256::zero(),
        &U256::zero(),
        netuid,
        1_000_000_000u64.into(),
    );
    SubtensorModule::set_validator_permit_for_uid(netuid, 0, true);
    System::set_block_number(1);
    assert_ok!(SubtensorModule::do_set_epoch_consensus(
        netuid,
        EpochConsensus::Null
    ));
    if mechanisms != 1 {
        assert_ok!(SubtensorModule::do_set_max_mechanism_count(
            mechanisms.into()
        ));
        assert_ok!(SubtensorModule::do_set_mechanism_count(
            netuid,
            mechanisms.into()
        ));
    }
    netuid
}

fn row(row_len: u16) -> (Vec<u16>, Vec<u16>) {
    ((0..row_len).collect(), vec![1; usize::from(row_len)])
}

fn mechanism_index(netuid: NetUid, mechanism: u8) -> NetUidStorageIndex {
    SubtensorModule::get_mechanism_storage_index(netuid, mechanism.into())
}

#[test]
fn null_row_cap_uses_full_budget_at_one_mechanism() {
    new_test_ext(1).execute_with(|| {
        let netuid = setup_null_subnet(1, TOTAL_NULL_WEIGHT_BUDGET);
        assert_eq!(
            SubtensorModule::max_null_weights_per_row(netuid),
            TOTAL_NULL_WEIGHT_BUDGET
        );
        assert_eq!(TOTAL_NULL_WEIGHT_BUDGET, NULL_UID_BUDGET);
        let (uids, values) = row(TOTAL_NULL_WEIGHT_BUDGET);
        assert_ok!(SubtensorModule::set_weights(
            RuntimeOrigin::signed(U256::zero()),
            netuid,
            uids,
            values,
            0,
        ));
        let (uids, values) = row(TOTAL_NULL_WEIGHT_BUDGET.saturating_add(1));
        assert_noop!(
            SubtensorModule::set_weights(
                RuntimeOrigin::signed(U256::zero()),
                netuid,
                uids,
                values,
                0,
            ),
            Error::<Test>::UidsLengthExceedUidsInSubNet
        );
    });
}

#[test]
fn null_row_cap_scales_with_mechanism_count_across_write_paths() {
    for mechanisms in [2u8, 16] {
        new_test_ext(1).execute_with(|| {
            let cap = TOTAL_NULL_WEIGHT_BUDGET / u16::from(mechanisms);
            let netuid = setup_null_subnet(mechanisms, cap.saturating_add(1));
            assert_eq!(SubtensorModule::max_null_weights_per_row(netuid), cap);
            let (uids, values) = row(cap);
            let (over_uids, over_values) = row(cap.saturating_add(1));
            let index = mechanism_index(netuid, 1u8.saturating_sub(mechanisms));
            assert_ok!(SubtensorModule::set_weights(
                RuntimeOrigin::signed(U256::zero()),
                netuid,
                uids.clone(),
                values.clone(),
                0,
            ));
            assert_noop!(
                SubtensorModule::set_weights(
                    RuntimeOrigin::signed(U256::zero()),
                    netuid,
                    over_uids.clone(),
                    over_values.clone(),
                    0,
                ),
                Error::<Test>::TooManyWeightsForNullMechanism
            );
            assert_ok!(SubtensorModule::set_mechanism_weights(
                RuntimeOrigin::signed(U256::zero()),
                netuid,
                MechId::MAIN,
                uids.clone(),
                values.clone(),
                0,
            ));
            assert_noop!(
                SubtensorModule::set_mechanism_weights(
                    RuntimeOrigin::signed(U256::zero()),
                    netuid,
                    MechId::MAIN,
                    over_uids.clone(),
                    over_values.clone(),
                    0,
                ),
                Error::<Test>::TooManyWeightsForNullMechanism
            );
            assert_ok!(SubtensorModule::do_batch_set_weights(
                RuntimeOrigin::signed(U256::zero()),
                vec![Compact(netuid)],
                vec![
                    uids.iter()
                        .copied()
                        .zip(values.iter().copied())
                        .map(|(uid, value)| (Compact(uid), Compact(value)))
                        .collect()
                ],
                vec![Compact(0)],
            ));
            System::reset_events();
            assert_ok!(SubtensorModule::do_batch_set_weights(
                RuntimeOrigin::signed(U256::zero()),
                vec![Compact(netuid)],
                vec![
                    over_uids
                        .iter()
                        .copied()
                        .zip(over_values.iter().copied())
                        .map(|(uid, value)| (Compact(uid), Compact(value)))
                        .collect()
                ],
                vec![Compact(0)],
            ));
            assert!(System::events().iter().any(|record| {
                record.event
                    == RuntimeEvent::SubtensorModule(Event::BatchWeightItemFailed(
                        netuid,
                        Error::<Test>::TooManyWeightsForNullMechanism.into(),
                    ))
            }));
            assert_eq!(Weights::<Test>::get(index, 0).len(), usize::from(cap));
        });
    }
}

#[test]
fn null_delegated_weight_paths_enforce_derived_cap() {
    let mechanisms = 16u8;
    let cap = TOTAL_NULL_WEIGHT_BUDGET / u16::from(mechanisms);
    new_test_ext(1).execute_with(|| {
        let netuid = setup_null_subnet(mechanisms, cap.saturating_add(1));
        let hotkey = U256::zero();
        let delegate = U256::from(999);
        let (uids, values) = row(cap);
        let (over_uids, over_values) = row(cap.saturating_add(1));
        for mechanism in 0..mechanisms {
            let mecid = MechId::from(mechanism);
            assert_ok!(SubtensorModule::set_mech_weight_delegate(
                RuntimeOrigin::signed(hotkey),
                netuid,
                mecid,
                Some(delegate),
            ));
            assert_ok!(SubtensorModule::set_mechanism_weights_for(
                RuntimeOrigin::signed(delegate),
                netuid,
                mecid,
                hotkey,
                uids.clone(),
                values.clone(),
                0,
            ));
            assert_eq!(
                Weights::<Test>::get(mechanism_index(netuid, mechanism), 0).len(),
                usize::from(cap)
            );
        }
        let mecid = MechId::MAIN;
        assert_noop!(
            SubtensorModule::set_mechanism_weights_for(
                RuntimeOrigin::signed(delegate),
                netuid,
                mecid,
                hotkey,
                over_uids.clone(),
                over_values.clone(),
                0,
            ),
            Error::<Test>::TooManyWeightsForNullMechanism
        );

        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        assert_ok!(SubtensorModule::set_reveal_period(netuid, 1));
        let salt = vec![1, 2, 3];
        let reveal_epoch = |uids: &[u16], values: &[u16]| {
            let hash = SubtensorModule::get_commit_hash(
                &hotkey,
                mechanism_index(netuid, 0),
                uids,
                values,
                &salt,
                0,
            );
            assert_ok!(SubtensorModule::commit_mechanism_weights_for(
                RuntimeOrigin::signed(delegate),
                netuid,
                mecid,
                hotkey,
                hash,
            ));
            let epoch = SubtensorModule::current_epoch_with_lookahead(netuid);
            SubnetEpochIndex::<Test>::insert(netuid, epoch.saturating_add(1));
        };
        reveal_epoch(&uids, &values);
        assert_ok!(SubtensorModule::reveal_mechanism_weights_for(
            RuntimeOrigin::signed(delegate),
            netuid,
            mecid,
            hotkey,
            uids,
            values,
            salt.clone(),
            0,
        ));
        reveal_epoch(&over_uids, &over_values);
        assert_noop!(
            SubtensorModule::reveal_mechanism_weights_for(
                RuntimeOrigin::signed(delegate),
                netuid,
                mecid,
                hotkey,
                over_uids,
                over_values,
                salt,
                0,
            ),
            Error::<Test>::TooManyWeightsForNullMechanism
        );
    });
}

#[test]
fn null_hash_and_batch_reveals_enforce_derived_cap() {
    new_test_ext(1).execute_with(|| {
        let netuid = setup_null_subnet(2, 1_251);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        assert_ok!(SubtensorModule::set_reveal_period(netuid, 1));
        let hotkey = U256::zero();
        let index = mechanism_index(netuid, 0);
        let salt = vec![9, 8];
        let (uids, values) = row(1_250);
        let (over_uids, over_values) = row(1_251);
        let commit = |uids: &[u16], values: &[u16]| {
            let hash = SubtensorModule::get_commit_hash(&hotkey, index, uids, values, &salt, 0);
            assert_ok!(SubtensorModule::do_commit_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                hash,
            ));
            let epoch = SubtensorModule::current_epoch_with_lookahead(netuid);
            SubnetEpochIndex::<Test>::insert(netuid, epoch.saturating_add(1));
        };
        commit(&uids, &values);
        assert_ok!(SubtensorModule::do_reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            uids.clone(),
            values.clone(),
            salt.clone(),
            0,
        ));
        commit(&uids, &values);
        assert_ok!(SubtensorModule::do_batch_reveal_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            vec![uids],
            vec![values],
            vec![salt.clone()],
            vec![0],
        ));
        commit(&over_uids, &over_values);
        assert_noop!(
            SubtensorModule::do_reveal_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                over_uids.clone(),
                over_values.clone(),
                salt.clone(),
                0,
            ),
            Error::<Test>::TooManyWeightsForNullMechanism
        );
        commit(&over_uids, &over_values);
        assert_noop!(
            SubtensorModule::do_batch_reveal_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                vec![over_uids],
                vec![over_values],
                vec![salt],
                vec![0],
            ),
            Error::<Test>::TooManyWeightsForNullMechanism
        );
    });
}

#[test]
fn null_signed_reveal_extension_rejects_over_cap_rows() {
    new_test_ext(1).execute_with(|| {
        let netuid = setup_null_subnet(2, 1_251);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        let hotkey = U256::zero();
        let delegate = U256::from(999);
        assert_ok!(SubtensorModule::set_mech_weight_delegate(
            RuntimeOrigin::signed(hotkey),
            netuid,
            MechId::MAIN,
            Some(delegate),
        ));
        let (over_uids, over_values) = row(1_251);
        assert_eq!(
            CheckWeights::<Test>::check(
                &delegate,
                &Call::reveal_mechanism_weights_for {
                    netuid,
                    mecid: MechId::MAIN,
                    hotkey,
                    uids: over_uids.clone(),
                    values: over_values.clone(),
                    salt: vec![1],
                    version_key: 0,
                },
            ),
            Err(Error::<Test>::TooManyWeightsForNullMechanism)
        );
        assert_eq!(
            CheckWeights::<Test>::check(
                &hotkey,
                &Call::batch_reveal_weights {
                    netuid,
                    uids_list: vec![over_uids.clone()],
                    values_list: vec![over_values.clone()],
                    salts_list: vec![vec![1]],
                    version_keys: vec![0],
                },
            ),
            Err(Error::<Test>::TooManyWeightsForNullMechanism)
        );
        assert_eq!(
            CheckWeights::<Test>::check(
                &hotkey,
                &Call::reveal_weights {
                    netuid,
                    uids: over_uids,
                    values: over_values,
                    salt: vec![1],
                    version_key: 0,
                },
            ),
            Err(Error::<Test>::TooManyWeightsForNullMechanism)
        );
    });
}

#[test]
fn null_timelocked_over_cap_row_is_skipped_without_stalling() {
    use crate::coinbase::reveal_commits::WeightsTlockPayload;
    use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
    use pallet_drand::types::Pulse;
    use rand_chacha::{ChaCha20Rng, rand_core::SeedableRng};
    use sha2::Digest;
    use tle::{
        curves::drand::TinyBLS381, ibe::fullident::Identity,
        stream_ciphers::AESGCMStreamCipherProvider, tlock::tle,
    };
    use w3f_bls::EngineBLS;

    new_test_ext(1).execute_with(|| {
        let netuid = setup_null_subnet(2, 1_251);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        assert_ok!(SubtensorModule::set_reveal_period(netuid, 1));
        let index = mechanism_index(netuid, 0);
        let hotkey = U256::zero();
        let round = 1_000u64;
        let (uids, values) = row(1_251);
        let payload = WeightsTlockPayload {
            hotkey: hotkey.encode(),
            uids,
            values,
            version_key: 0,
        };
        let pk_bytes = hex::decode("83cf0f2896adee7eb8b5f01fcad3912212c437e0073e911fb90022d3e760183c8c4b450b6a0a6c3ac6a5776a2d1064510d1fec758c921cc22b0e17e63aaf4bcb5ed66304de9cf809bd274ca73bab4af5a6e9c76a4bc09e76eae8991ef5ece45a").unwrap();
        let pk = <TinyBLS381 as EngineBLS>::PublicKeyGroup::deserialize_compressed(&*pk_bytes)
            .unwrap();
        let identity = Identity::new(b"", vec![sha2::Sha256::digest(round.to_be_bytes()).to_vec()]);
        let cipher = tle::<TinyBLS381, AESGCMStreamCipherProvider, ChaCha20Rng>(
            pk,
            [2; 32],
            &payload.encode(),
            identity,
            ChaCha20Rng::seed_from_u64(0),
        )
        .unwrap();
        let mut bytes = Vec::new();
        cipher.serialize_compressed(&mut bytes).unwrap();
        assert_ok!(SubtensorModule::do_commit_timelocked_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            bytes.try_into().unwrap(),
            round,
            SubtensorModule::get_commit_reveal_weights_version(),
        ));
        let committed_epoch = SubtensorModule::current_epoch_with_lookahead(netuid);
        let signature = hex::decode("b44679b9a59af2ec876b1a6b1ad52ea9b1615fc3982b19576350f93447cb1125e342b73a8dd2bacbe47e4b6b63ed5e39").unwrap();
        pallet_drand::Pulses::<Test>::insert(
            round,
            Pulse {
                round,
                randomness: vec![0; 32].try_into().unwrap(),
                signature: signature.try_into().unwrap(),
            },
        );
        SubnetEpochIndex::<Test>::insert(netuid, committed_epoch.saturating_add(1));
        assert_ok!(SubtensorModule::reveal_crv3_commits_for_subnet(netuid));
        assert!(Weights::<Test>::get(index, 0).is_empty());
        assert!(TimelockedWeightCommits::<Test>::get(index, committed_epoch).is_empty());
        SubtensorModule::reveal_crv3_commits();
    });
}

#[test]
fn null_mechanism_count_change_preserves_legacy_oversized_rows() {
    new_test_ext(1).execute_with(|| {
        let netuid = setup_null_subnet(1, 2_500);
        let (uids, values) = row(2_500);
        assert_ok!(SubtensorModule::set_weights(
            RuntimeOrigin::signed(U256::zero()),
            netuid,
            uids,
            values,
            0,
        ));
        assert_ok!(SubtensorModule::do_set_max_mechanism_count(16.into()));
        assert_ok!(SubtensorModule::do_set_mechanism_count(netuid, 16.into()));
        assert_eq!(
            Weights::<Test>::get(mechanism_index(netuid, 0), 0).len(),
            2_500
        );
        let output = SubtensorModule::epoch(netuid, 1_000_001u64.into());
        assert_eq!(output.len(), 2_500);
        assert_noop!(
            SubtensorModule::set_weights(
                RuntimeOrigin::signed(U256::zero()),
                netuid,
                vec![0; 157],
                vec![1; 157],
                0,
            ),
            Error::<Test>::TooManyWeightsForNullMechanism
        );
        assert_eq!(
            Weights::<Test>::get(mechanism_index(netuid, 0), 0).len(),
            2_500
        );
    });
}

#[test]
fn null_supports_16_delegated_mechanisms_and_epoch_at_256_uids() {
    new_test_ext(1).execute_with(|| {
        let netuid = setup_null_subnet(16, 256);
        SubtensorModule::set_max_allowed_uids(netuid, 256);
        assert_eq!(SubtensorModule::get_max_allowed_uids(netuid), 256);
        SubtensorModule::set_max_allowed_uids(netuid, 2_500);
        assert_eq!(SubtensorModule::get_max_allowed_uids(netuid), 2_500);
        assert_eq!(
            SubtensorModule::get_current_mechanism_count(netuid),
            16.into()
        );
        let hotkey = U256::zero();
        let delegate = U256::from(999);
        for mechanism in 0..16u8 {
            let mecid = MechId::from(mechanism);
            assert_ok!(SubtensorModule::set_mech_weight_delegate(
                RuntimeOrigin::signed(hotkey),
                netuid,
                mecid,
                Some(delegate),
            ));
            assert_ok!(SubtensorModule::set_mechanism_weights_for(
                RuntimeOrigin::signed(delegate),
                netuid,
                mecid,
                hotkey,
                vec![u16::from(mechanism)],
                vec![1],
                0,
            ));
        }
        let output = SubtensorModule::epoch(netuid, 1_000_001u64.into());
        assert_eq!(output.len(), 256);
        for mechanism in 0..16u8 {
            assert_eq!(
                Weights::<Test>::get(mechanism_index(netuid, mechanism), 0),
                vec![(u16::from(mechanism), 1)]
            );
        }
    });
}

#[test]
fn yuma_to_null_switch_does_not_clamp_for_mechanism_count() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network_disable_commit_reveal(netuid, u16::MAX - 1, 0);
        SubtensorModule::set_max_allowed_uids(netuid, 128);
        assert_ok!(SubtensorModule::do_set_mechanism_count(netuid, 2.into()));
        SubtensorModule::set_epoch_consensus(netuid, EpochConsensus::Null);
        SubtensorModule::set_max_allowed_uids(netuid, 2_500);
        SubtensorModule::set_epoch_consensus(netuid, EpochConsensus::Yuma);
        SubnetworkN::<Test>::insert(netuid, 0);
        assert_ok!(SubtensorModule::do_set_epoch_consensus(
            netuid,
            EpochConsensus::Null
        ));
        assert_eq!(
            SubtensorModule::get_max_allowed_uids(netuid),
            NULL_UID_BUDGET
        );
    });
}

#[test]
fn null_max_mechanism_count_defaults_to_two_and_allows_sixteen() {
    new_test_ext(1).execute_with(|| {
        assert_eq!(DefaultMaxMechanismCount::<Test>::get(), 2.into());
        assert_eq!(MAX_MECHANISM_COUNT_PER_SUBNET, 16);
        assert_noop!(
            SubtensorModule::do_set_max_mechanism_count(17.into()),
            Error::<Test>::InvalidValue
        );
        assert_ok!(SubtensorModule::do_set_max_mechanism_count(16.into()));
        assert_eq!(MaxMechanismCount::<Test>::get(), 16.into());
    });
}
