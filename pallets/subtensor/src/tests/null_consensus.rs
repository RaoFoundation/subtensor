#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::mock::*;
use crate::*;
use frame_support::assert_ok;
use sp_core::U256;
use subtensor_runtime_common::{MechId, NetUid, NetUidStorageIndex};

fn setup(stakes: [u64; 2]) -> NetUid {
    let netuid = NetUid::from(1);
    add_network_disable_commit_reveal(netuid, u16::MAX - 1, 0);
    SubtensorModule::set_max_allowed_uids(netuid, 4);
    SubtensorModule::set_max_allowed_validators(netuid, 2);
    SubtensorModule::set_weights_set_rate_limit(netuid, 0);
    for uid in 0..4u16 {
        let hotkey = U256::from(uid);
        SubtensorModule::append_neuron(netuid, &hotkey, 0);
        if let Some(stake) = stakes.get(uid as usize) {
            SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &hotkey,
                netuid,
                (*stake).into(),
            );
            SubtensorModule::set_validator_permit_for_uid(netuid, uid, true);
        }
    }
    System::set_block_number(1);
    assert_ok!(SubtensorModule::do_set_epoch_consensus(
        netuid,
        EpochConsensus::Null
    ));
    netuid
}

fn set_weights(netuid: NetUid, uid: u16, destinations: Vec<u16>, weights: Vec<u16>) {
    assert_ok!(SubtensorModule::set_weights(
        RuntimeOrigin::signed(U256::from(uid)),
        netuid,
        destinations,
        weights,
        0,
    ));
}

// Simulate rows left by Yuma or a previous permit holder.
fn seed_historical_weights(netuid: NetUid, uid: u16, destinations: Vec<u16>, weights: Vec<u16>) {
    Weights::<Test>::insert(
        NetUidStorageIndex::from(netuid),
        uid,
        destinations.into_iter().zip(weights).collect::<Vec<_>>(),
    );
    SubtensorModule::set_last_update_for_uid(NetUidStorageIndex::from(netuid), uid, 1);
}

#[test]
fn null_cached_stake_matches_original_delegation_arithmetic() {
    new_test_ext(1).execute_with(|| {
        let netuid = setup([300_000_000, 100_000_000]);
        let external = U256::from(99);
        let registered_parent = U256::from(1);
        let child = U256::from(0);
        // Repeated parents exercise cache reuse; external parents must still
        // contribute, and fractions must truncate per asset exactly as before.
        ParentKeys::<Test>::insert(
            child,
            netuid,
            vec![(u64::MAX / 3, registered_parent), (1, external)],
        );
        ChildKeys::<Test>::insert(child, netuid, vec![(u64::MAX / 7, external)]);
        ParentKeys::<Test>::insert(
            U256::from(2),
            netuid,
            vec![(u64::MAX / 5, registered_parent), (u64::MAX, external)],
        );
        // The original path returns zero for a missing registered key.
        Keys::<Test>::remove(netuid, 3);
        for balance in [0, 1, 987_654_321, u64::MAX] {
            TotalHotkeyAlpha::<Test>::insert(external, netuid, AlphaBalance::from(balance));
            TotalHotkeyAlpha::<Test>::insert(
                external,
                NetUid::ROOT,
                AlphaBalance::from(balance.saturating_sub(1)),
            );
            TotalHotkeyAlpha::<Test>::insert(
                registered_parent,
                NetUid::ROOT,
                AlphaBalance::from(123_456_789),
            );
            for suspended in [false, true] {
                if suspended {
                    ChildkeyThresholdSuspended::<Test>::insert(registered_parent, ());
                    ChildkeyThresholdSuspended::<Test>::insert(external, ());
                } else {
                    ChildkeyThresholdSuspended::<Test>::remove(registered_parent);
                    ChildkeyThresholdSuspended::<Test>::remove(external);
                }
                for owner in [registered_parent, external] {
                    SubnetOwnerHotkey::<Test>::insert(netuid, owner);
                    assert_eq!(
                        SubtensorModule::get_null_stake_weights_for_network(netuid),
                        SubtensorModule::get_stake_weights_for_network(netuid),
                        "balance={balance}, suspended={suspended}, owner={owner:?}",
                    );
                }
            }
        }
        // A cache lives only within one calculation, never across stake changes.
        TotalHotkeyAlpha::<Test>::insert(child, netuid, AlphaBalance::from(42));
        assert_eq!(
            SubtensorModule::get_null_stake_weights_for_network(netuid),
            SubtensorModule::get_stake_weights_for_network(netuid),
        );
    });
}

#[test]
fn null_consensus_winner_incentives_and_stake_dividends_ignore_activity() {
    new_test_ext(1).execute_with(|| {
        let netuid = setup([300_000_000, 100_000_000]);
        set_weights(netuid, 0, vec![2, 3], vec![3, 1]);
        seed_historical_weights(netuid, 1, vec![3], vec![u16::MAX]);
        // Both validators become inactive; Null must still use their stake.
        System::set_block_number(
            SubtensorModule::get_activity_cutoff_blocks(netuid).saturating_add(2),
        );
        let output = SubtensorModule::epoch_mechanism(netuid, MechId::MAIN, 1_000_000.into());
        let terms = output.as_map();
        let winner = &terms[&U256::from(0)];
        let other = &terms[&U256::from(1)];
        assert!(!winner.active);
        assert!(!other.active);
        assert!(winner.validator_emission.to_u64().abs_diff(375_000) <= 5);
        assert!(other.validator_emission.to_u64().abs_diff(125_000) <= 5);
        assert!(
            terms[&U256::from(2)]
                .server_emission
                .to_u64()
                .abs_diff(375_000)
                <= 5
        );
        assert!(
            terms[&U256::from(3)]
                .server_emission
                .to_u64()
                .abs_diff(125_000)
                <= 5
        );
        assert_eq!(winner.server_emission.to_u64(), 0);
        assert_eq!(other.server_emission.to_u64(), 0);
    });
}

#[test]
fn null_consensus_tie_selects_first_uid() {
    new_test_ext(1).execute_with(|| {
        let netuid = setup([100_000_000, 100_000_000]);
        set_weights(netuid, 0, vec![2], vec![u16::MAX]);
        seed_historical_weights(netuid, 1, vec![3], vec![u16::MAX]);
        let output = SubtensorModule::epoch_mechanism(netuid, MechId::MAIN, 1_000_000.into());
        assert!(
            output.as_map()[&U256::from(2)]
                .server_emission
                .to_u64()
                .abs_diff(500_000)
                <= 1
        );
        assert_eq!(output.as_map()[&U256::from(3)].server_emission.to_u64(), 0);
    });
}

#[test]
fn null_consensus_empty_winner_weights_share_across_every_uid() {
    new_test_ext(1).execute_with(|| {
        let netuid = setup([300_000_000, 100_000_000]);
        seed_historical_weights(netuid, 1, vec![3], vec![u16::MAX]);
        // The loser's row must not substitute for the empty winner row.
        let output = SubtensorModule::epoch_mechanism(netuid, MechId::MAIN, 1_000_000.into());
        for terms in output.as_map().values() {
            assert!(terms.server_emission.to_u64().abs_diff(125_000) <= 1);
        }
        assert!(
            output.as_map()[&U256::from(0)]
                .validator_emission
                .to_u64()
                .abs_diff(375_000)
                <= 1
        );
        assert!(
            output.as_map()[&U256::from(1)]
                .validator_emission
                .to_u64()
                .abs_diff(125_000)
                <= 1
        );
    });
}

#[test]
fn null_consensus_selects_winner_before_normalized_stake_rounding() {
    new_test_ext(1).execute_with(|| {
        let netuid = setup([1_000_000_000, 1_000_000_001]);
        seed_historical_weights(netuid, 0, vec![2], vec![u16::MAX]);
        set_weights(netuid, 1, vec![3], vec![u16::MAX]);
        let output = SubtensorModule::epoch_mechanism(netuid, MechId::MAIN, 1_000_000.into());
        assert_eq!(output.as_map()[&U256::from(2)].server_emission.to_u64(), 0);
        assert!(
            output.as_map()[&U256::from(3)]
                .server_emission
                .to_u64()
                .abs_diff(500_000)
                <= 1
        );
    });
}

#[test]
fn null_consensus_stale_winner_row_uses_equal_fallback() {
    new_test_ext(1).execute_with(|| {
        let netuid = setup([300_000_000, 100_000_000]);
        set_weights(netuid, 0, vec![2], vec![u16::MAX]);
        seed_historical_weights(netuid, 1, vec![3], vec![u16::MAX]);
        // UID 2 was replaced after the winner submitted its weights.
        BlockAtRegistration::<Test>::insert(netuid, 2, 2u64);
        let output = SubtensorModule::epoch_mechanism(netuid, MechId::MAIN, 1_000_000.into());
        for terms in output.as_map().values() {
            assert!(terms.server_emission.to_u64().abs_diff(125_000) <= 1);
        }
    });
}

#[test]
fn null_consensus_preserves_bonds_and_can_switch_back_to_yuma() {
    new_test_ext(1).execute_with(|| {
        let netuid = setup([300_000_000, 100_000_000]);
        let index = SubtensorModule::get_mechanism_storage_index(netuid, MechId::MAIN);
        Bonds::<Test>::insert(index, 0, vec![(2u16, 12345u16)]);
        Bonds::<Test>::insert(index, 1, vec![(3u16, 54321u16)]);
        let before = (Bonds::<Test>::get(index, 0), Bonds::<Test>::get(index, 1));
        SubtensorModule::epoch(netuid, 1_000_000.into());
        assert_eq!(
            before,
            (Bonds::<Test>::get(index, 0), Bonds::<Test>::get(index, 1))
        );
        SubtensorModule::epoch_dense(netuid, 1_000_000.into());
        assert_eq!(
            before,
            (Bonds::<Test>::get(index, 0), Bonds::<Test>::get(index, 1))
        );
        SubtensorModule::set_epoch_consensus(netuid, EpochConsensus::Yuma);
        assert_eq!(
            SubtensorModule::get_epoch_consensus(netuid),
            EpochConsensus::Yuma
        );
        SubtensorModule::epoch(netuid, 1_000_000.into());
    });
}

#[test]
fn null_consensus_no_stake_has_only_equal_miner_emissions() {
    new_test_ext(1).execute_with(|| {
        let netuid = setup([0, 0]);
        let output = SubtensorModule::epoch_mechanism(netuid, MechId::MAIN, 1_000_000.into());
        for terms in output.as_map().values() {
            assert_eq!(terms.server_emission.to_u64(), 250_000);
            assert_eq!(terms.validator_emission.to_u64(), 0);
        }
    });
}

#[test]
fn null_consensus_default_and_empty_subnet() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network_disable_commit_reveal(netuid, u16::MAX - 1, 0);
        assert_eq!(
            SubtensorModule::get_epoch_consensus(netuid),
            EpochConsensus::Yuma
        );
        SubtensorModule::set_epoch_consensus(netuid, EpochConsensus::Null);
        assert!(
            SubtensorModule::epoch_mechanism(netuid, MechId::MAIN, 1_000_000.into())
                .as_map()
                .is_empty()
        );
    });
}

#[test]
fn null_apportionment_conserves_large_and_subatomic_budgets() {
    use crate::epoch::run_epoch::apportion_units;
    for budget in [0, 1, 7, 4_097, 1_000_003, u64::MAX] {
        let mut shares = vec![u128::from(u16::MAX); 2_500];
        shares[0] = 1;
        shares[100] = 0;
        let payouts = apportion_units(&shares, budget);
        let sum: u128 = payouts.iter().map(|p| u128::from(u64::from(*p))).sum();
        assert_eq!(sum, u128::from(budget));
        assert_eq!(u64::from(payouts[100]), 0);
        assert_eq!(payouts, apportion_units(&shares, budget));
        let total: u128 = shares.iter().sum();
        for (share, paid) in shares.iter().zip(&payouts) {
            let floor = u128::from(budget) * share / total;
            assert!([floor, floor + 1].contains(&u128::from(u64::from(*paid))));
        }
    }
    assert_eq!(
        apportion_units(&[u128::MAX; 2_500], u64::MAX)
            .iter()
            .map(|p| u128::from(u64::from(*p)))
            .sum::<u128>(),
        u128::from(u64::MAX)
    );
}

#[test]
fn null_consensus_preserves_raw_weights_and_epoch_budget() {
    new_test_ext(1).execute_with(|| {
        let netuid = setup([300_000_000, 100_000_000]);
        set_weights(netuid, 0, vec![2, 3], vec![65_534, 1]);
        assert_eq!(
            Weights::<Test>::get(NetUidStorageIndex::from(netuid), 0),
            vec![(2, 65_534), (3, 1)]
        );
        for budget in [0, 1, 3, 131_071, u64::MAX] {
            let output = SubtensorModule::epoch_mechanism(netuid, MechId::MAIN, budget.into());
            let miner_sum: u128 = output
                .as_map()
                .values()
                .map(|t| u128::from(u64::from(t.server_emission)))
                .sum();
            let validator_sum: u128 = output
                .as_map()
                .values()
                .map(|t| u128::from(u64::from(t.validator_emission)))
                .sum();
            assert_eq!(miner_sum, u128::from(budget - budget / 2));
            assert_eq!(validator_sum, u128::from(budget / 2));
            assert_eq!(miner_sum + validator_sum, u128::from(budget));
            for term in output.as_map().values() {
                assert_eq!(
                    term.emission,
                    term.server_emission.saturating_add(term.validator_emission)
                );
            }
        }
    });
}

#[test]
fn null_consensus_full_row_timelock_reveal_to_emissions() {
    use crate::coinbase::reveal_commits::WeightsTlockPayload;
    use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
    use codec::Encode;
    use pallet_drand::types::Pulse;
    use rand_chacha::{ChaCha20Rng, rand_core::SeedableRng};
    use sha2::Digest;
    use tle::{
        curves::drand::TinyBLS381, ibe::fullident::Identity,
        stream_ciphers::AESGCMStreamCipherProvider, tlock::tle,
    };
    use w3f_bls::EngineBLS;

    new_test_ext(1).execute_with(|| {
        let netuid = setup([300_000_000, 100_000_000]);
        let index = NetUidStorageIndex::from(netuid);
        // Populate the registered-UID fixtures without 2,500 independent
        // extrinsic transactions. Submission and reveal use production paths.
        SubtensorModule::set_max_allowed_uids(netuid, 2_500);
        for uid in 4..2_500u16 {
            Keys::<Test>::insert(netuid, uid, U256::from(uid));
            Uids::<Test>::insert(netuid, U256::from(uid), uid);
        }
        SubnetworkN::<Test>::insert(netuid, 2_500);
        let mut permits = vec![false; 2_500]; permits[0] = true; permits[1] = true;
        ValidatorPermit::<Test>::insert(netuid, permits);
        LastUpdate::<Test>::insert(index, vec![0u64; 2_500]);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        assert_ok!(SubtensorModule::set_reveal_period(netuid, 1));
        let round = 1_000u64;
        let mut values = vec![1u16; 2_500]; values[2] = 65_534;
        let payload = WeightsTlockPayload { hotkey: U256::from(0).encode(),
            uids: (0..2_500u16).collect(), values: values.clone(), version_key: 0 };
        let pk_bytes = hex::decode("83cf0f2896adee7eb8b5f01fcad3912212c437e0073e911fb90022d3e760183c8c4b450b6a0a6c3ac6a5776a2d1064510d1fec758c921cc22b0e17e63aaf4bcb5ed66304de9cf809bd274ca73bab4af5a6e9c76a4bc09e76eae8991ef5ece45a").unwrap();
        let pk = <TinyBLS381 as EngineBLS>::PublicKeyGroup::deserialize_compressed(&*pk_bytes).unwrap();
        let identity = Identity::new(b"", vec![sha2::Sha256::digest(round.to_be_bytes()).to_vec()]);
        let cipher = tle::<TinyBLS381, AESGCMStreamCipherProvider, ChaCha20Rng>(
            pk, [2; 32], &payload.encode(), identity, ChaCha20Rng::seed_from_u64(0)).unwrap();
        let mut bytes = Vec::new(); cipher.serialize_compressed(&mut bytes).unwrap();
        assert!(bytes.len() > YUMA_COMMIT_SIZE_BYTES as usize);
        assert!(bytes.len() <= MAX_CRV3_COMMIT_SIZE_BYTES as usize);
        assert_ok!(SubtensorModule::do_commit_timelocked_weights(RuntimeOrigin::signed(U256::from(0)),
            netuid, bytes.try_into().unwrap(), round, SubtensorModule::get_commit_reveal_weights_version()));
        let committed_epoch = SubtensorModule::current_epoch_with_lookahead(netuid);
        let signature = hex::decode("b44679b9a59af2ec876b1a6b1ad52ea9b1615fc3982b19576350f93447cb1125e342b73a8dd2bacbe47e4b6b63ed5e39").unwrap();
        pallet_drand::Pulses::<Test>::insert(round, Pulse { round, randomness: vec![0; 32].try_into().unwrap(),
            signature: signature.try_into().unwrap() });
        SubnetEpochIndex::<Test>::insert(netuid, committed_epoch + 1);
        assert_ok!(SubtensorModule::reveal_crv3_commits_for_subnet(netuid));
        let stored = Weights::<Test>::get(index, 0);
        assert_eq!(stored.len(), 2_500);
        assert_eq!(stored.iter().map(|(_, weight)| *weight).collect::<Vec<_>>(), values);
        let output = SubtensorModule::epoch_mechanism(netuid, MechId::MAIN, 1_000_001.into());
        assert_eq!(output.as_map().len(), 2_500);
        assert!(output.as_map().values().filter(|t| t.server_emission > 0.into()).count() > 2_400);
        let miners: u128 = output.as_map().values().map(|t| u128::from(u64::from(t.server_emission))).sum();
        let validators: u128 = output.as_map().values().map(|t| u128::from(u64::from(t.validator_emission))).sum();
        assert_eq!(miners, 500_001); assert_eq!(miners + validators, 1_000_001);
    });
}

#[test]
fn null_consensus_commit_queue_budget_is_shared_by_mechanisms() {
    use frame_support::assert_noop;
    new_test_ext(1).execute_with(|| {
        let netuid = setup([300_000_000, 100_000_000]);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        assert_ok!(SubtensorModule::do_set_mechanism_count(
            netuid,
            MechId::from(2)
        ));
        let version = SubtensorModule::get_commit_reveal_weights_version();
        for uid in 0..1u16 {
            for mechanism in 0..2u8 {
                assert_ok!(SubtensorModule::do_commit_timelocked_mechanism_weights(
                    RuntimeOrigin::signed(U256::from(uid)),
                    netuid,
                    mechanism.into(),
                    vec![0u8; MAX_CRV3_COMMIT_SIZE_BYTES as usize / 2]
                        .try_into()
                        .unwrap(),
                    1000,
                    version,
                ));
            }
        }
        assert_noop!(
            SubtensorModule::do_commit_timelocked_mechanism_weights(
                RuntimeOrigin::signed(U256::from(2)),
                netuid,
                MechId::MAIN,
                vec![0u8; 1].try_into().unwrap(),
                1000,
                version
            ),
            Error::<Test>::NeuronNoValidatorPermit
        );
    });
}

#[test]
fn null_consensus_yuma_keeps_legacy_ciphertext_limit() {
    use frame_support::assert_noop;
    new_test_ext(1).execute_with(|| {
        let netuid = setup([300_000_000, 100_000_000]);
        SubtensorModule::set_epoch_consensus(netuid, EpochConsensus::Yuma);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        assert_noop!(
            SubtensorModule::do_commit_timelocked_weights(
                RuntimeOrigin::signed(U256::from(0)),
                netuid,
                vec![0u8; YUMA_COMMIT_SIZE_BYTES as usize + 1]
                    .try_into()
                    .unwrap(),
                1000,
                SubtensorModule::get_commit_reveal_weights_version()
            ),
            Error::<Test>::CommitPayloadTooLarge
        );
    });
}

#[test]
fn null_timelock_winner_preempts_junk_and_replaces_own_row() {
    use frame_support::assert_noop;
    new_test_ext(1).execute_with(|| {
        let netuid = setup([300_000_000, 100_000_000]);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        let version = SubtensorModule::get_commit_reveal_weights_version();
        let submit = |uid, byte| {
            SubtensorModule::do_commit_timelocked_mechanism_weights(
                RuntimeOrigin::signed(U256::from(uid)),
                netuid,
                MechId::MAIN,
                vec![byte; MAX_CRV3_COMMIT_SIZE_BYTES as usize]
                    .try_into()
                    .unwrap(),
                1000,
                version,
            )
        };
        assert_noop!(submit(2u16, 2u8), Error::<Test>::NeuronNoValidatorPermit);
        assert_noop!(submit(3u16, 3u8), Error::<Test>::NeuronNoValidatorPermit);
        assert_noop!(submit(1u16, 1u8), Error::<Test>::NeuronNoValidatorPermit);
        // Historical queued rows must not prevent the current winner's admission.
        let epoch = SubtensorModule::current_epoch_with_lookahead(netuid);
        TimelockedWeightCommits::<Test>::insert(
            NetUidStorageIndex::from(netuid),
            epoch,
            std::collections::VecDeque::from([
                (
                    U256::from(2),
                    1,
                    vec![2; MAX_CRV3_COMMIT_SIZE_BYTES as usize]
                        .try_into()
                        .unwrap(),
                    1000,
                ),
                (
                    U256::from(3),
                    1,
                    vec![3; MAX_CRV3_COMMIT_SIZE_BYTES as usize]
                        .try_into()
                        .unwrap(),
                    1000,
                ),
            ]),
        );
        assert_ok!(submit(0u16, 0u8));
        assert_ok!(submit(0u16, 9u8));
        let epoch = SubtensorModule::current_epoch_with_lookahead(netuid);
        let queue = TimelockedWeightCommits::<Test>::get(NetUidStorageIndex::from(netuid), epoch);
        assert_eq!(queue.len(), 2);
        assert!(
            queue
                .iter()
                .any(|(who, _, bytes, _)| *who == U256::from(0) && bytes.iter().all(|b| *b == 9))
        );
        assert!(!queue.iter().any(|(who, ..)| *who == U256::from(1)));
        // A newly highest-stake permitted validator also gets admission.
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &U256::from(2),
            &U256::from(2),
            netuid,
            500_000_000u64.into(),
        );
        let output = SubtensorModule::epoch_mechanism(netuid, MechId::MAIN, 0u64.into());
        ValidatorPermit::<Test>::insert(
            netuid,
            (0..4)
                .map(|uid| output.as_map()[&U256::from(uid)].new_validator_permit)
                .collect::<Vec<_>>(),
        );
        assert_noop!(submit(0u16, 0u8), Error::<Test>::NeuronNoValidatorPermit);
        assert_ok!(submit(2u16, 2u8));
        let queue = TimelockedWeightCommits::<Test>::get(NetUidStorageIndex::from(netuid), epoch);
        assert!(queue.iter().any(|(who, ..)| *who == U256::from(2)));
        assert!(!queue.iter().any(|(who, ..)| *who == U256::from(1)));
    });
}

#[test]
fn null_timelock_equal_stake_first_uid_preempts_at_count_limit() {
    new_test_ext(1).execute_with(|| {
        let netuid = setup([300_000_000, 300_000_000]);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        SubtensorModule::set_max_allowed_uids(netuid, 65);
        SubtensorModule::set_max_allowed_validators(netuid, 65);
        for uid in 4..65u16 {
            let hotkey = U256::from(uid);
            SubtensorModule::append_neuron(netuid, &hotkey, 0);
        }
        for uid in 2..65u16 {
            SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &U256::from(uid),
                &U256::from(uid),
                netuid,
                300_000_000u64.into(),
            );
            SubtensorModule::set_validator_permit_for_uid(netuid, uid, true);
        }
        let version = SubtensorModule::get_commit_reveal_weights_version();
        ValidatorPermit::<Test>::insert(netuid, (0..65).map(|uid| uid == 0).collect::<Vec<_>>());
        let epoch = SubtensorModule::current_epoch_with_lookahead(netuid);
        TimelockedWeightCommits::<Test>::insert(
            NetUidStorageIndex::from(netuid),
            epoch,
            (1..65u16)
                .map(|uid| (U256::from(uid), 1, vec![1u8].try_into().unwrap(), 1000))
                .collect::<std::collections::VecDeque<_>>(),
        );
        assert_ok!(SubtensorModule::do_commit_timelocked_mechanism_weights(
            RuntimeOrigin::signed(U256::from(0)),
            netuid,
            MechId::MAIN,
            vec![1u8].try_into().unwrap(),
            1000,
            version,
        ));
        let epoch = SubtensorModule::current_epoch_with_lookahead(netuid);
        let queue = TimelockedWeightCommits::<Test>::get(NetUidStorageIndex::from(netuid), epoch);
        assert_eq!(queue.len(), NULL_COMMIT_QUEUE_COUNT);
        assert!(queue.iter().any(|(who, ..)| *who == U256::from(0)));
        assert!(!queue.iter().any(|(who, ..)| *who == U256::from(64)));
    });
}

#[test]
fn null_consensus_switch_waits_for_timelock_queues_to_drain() {
    use frame_support::assert_noop;
    new_test_ext(1).execute_with(|| {
        let netuid = setup([300_000_000, 100_000_000]);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        let version = SubtensorModule::get_commit_reveal_weights_version();
        assert_ok!(SubtensorModule::do_commit_timelocked_mechanism_weights(
            RuntimeOrigin::signed(U256::from(0)),
            netuid,
            MechId::MAIN,
            vec![1u8].try_into().unwrap(),
            1000,
            version,
        ));
        assert_noop!(
            SubtensorModule::do_set_epoch_consensus(netuid, EpochConsensus::Yuma),
            Error::<Test>::InvalidValue
        );
        let index = NetUidStorageIndex::from(netuid);
        let epoch = SubtensorModule::current_epoch_with_lookahead(netuid);
        TimelockedWeightCommits::<Test>::remove(index, epoch);
        assert_ok!(SubtensorModule::do_set_epoch_consensus(
            netuid,
            EpochConsensus::Yuma
        ));
        assert_ok!(SubtensorModule::do_commit_timelocked_mechanism_weights(
            RuntimeOrigin::signed(U256::from(0)),
            netuid,
            MechId::MAIN,
            vec![1u8].try_into().unwrap(),
            1000,
            version,
        ));
        assert_noop!(
            SubtensorModule::do_set_epoch_consensus(netuid, EpochConsensus::Null),
            Error::<Test>::InvalidValue
        );
        TimelockedWeightCommits::<Test>::remove(index, epoch);
        assert_ok!(SubtensorModule::do_set_epoch_consensus(
            netuid,
            EpochConsensus::Null
        ));
    });
}

#[test]
fn null_no_validator_fallback_excludes_recycled_root_budget() {
    new_test_ext(1).execute_with(|| {
        let netuid = setup([0, 0]);
        // Ensure every participant is a miner, including UID zero.
        SubtensorModule::set_validator_permit_for_uid(netuid, 0, false);
        SubtensorModule::set_validator_permit_for_uid(netuid, 1, false);
        SubnetOwner::<Test>::insert(netuid, U256::from(100));
        SubnetAlphaOut::<Test>::insert(netuid, AlphaBalance::from(1_000u64));
        let before: u64 = (0..4u16)
            .map(|uid| {
                SubtensorModule::get_stake_for_hotkey_on_subnet(&U256::from(uid), netuid).to_u64()
            })
            .sum();
        SubtensorModule::distribute_emission(
            netuid,
            500u64.into(),
            200u64.into(),
            300u64.into(),
            0u64.into(),
        );
        let after: u64 = (0..4u16)
            .map(|uid| {
                SubtensorModule::get_stake_for_hotkey_on_subnet(&U256::from(uid), netuid).to_u64()
            })
            .sum();
        assert_eq!(after - before, 700);
        assert_eq!(SubnetAlphaOut::<Test>::get(netuid).to_u64(), 700);
        assert_eq!(
            Emission::<Test>::get(netuid)
                .iter()
                .map(|e| e.to_u64())
                .sum::<u64>(),
            700
        );
    });
}

#[test]
fn null_fallback_whole_units_respect_each_mechanism_budget() {
    new_test_ext(1).execute_with(|| {
        let netuid = setup([0, 0]);
        SubtensorModule::set_validator_permit_for_uid(netuid, 0, false);
        SubtensorModule::set_validator_permit_for_uid(netuid, 1, false);
        assert_ok!(SubtensorModule::do_set_max_mechanism_count(4.into()));
        assert_ok!(SubtensorModule::do_set_mechanism_count(netuid, 4.into()));
        for local_budget in [0u64, 1, 2, 7, 700, u64::MAX - 300] {
            let total_budget = local_budget.saturating_add(300);
            let output = SubtensorModule::epoch_with_mechanism_budgets(
                netuid,
                local_budget.into(),
                0u64.into(),
                total_budget.saturating_sub(local_budget).into(),
            );
            assert_eq!(
                output
                    .iter()
                    .map(|(_, miner, _)| miner.to_u64())
                    .sum::<u64>(),
                local_budget
            );
            assert!(output.iter().all(|(_, _, dividend)| dividend.is_zero()));
            assert_eq!(
                Emission::<Test>::get(netuid)
                    .iter()
                    .map(|amount| amount.to_u64())
                    .sum::<u64>(),
                local_budget
            );
        }
    });
}

#[test]
fn null_bond_cutoff_tracks_last_yuma_epoch_not_null_epochs() {
    new_test_ext(1).execute_with(|| {
        let netuid = setup([300_000_000, 100_000_000]);
        SubtensorModule::set_epoch_consensus(netuid, EpochConsensus::Yuma);
        LastYumaStepBlock::<Test>::remove(netuid);
        LastMechansimStepBlock::<Test>::insert(netuid, 10);
        assert_eq!(SubtensorModule::bond_cutoff_block(netuid, 20, 5), 11);
        assert_ok!(SubtensorModule::do_set_epoch_consensus(
            netuid,
            EpochConsensus::Null
        ));
        LastMechansimStepBlock::<Test>::insert(netuid, 100);
        assert_eq!(SubtensorModule::bond_cutoff_block(netuid, 100, 5), 11);
        assert_ok!(SubtensorModule::do_set_epoch_consensus(
            netuid,
            EpochConsensus::Yuma
        ));
        assert_eq!(SubtensorModule::bond_cutoff_block(netuid, 101, 5), 11);
        LastYumaStepBlock::<Test>::insert(netuid, 101);
        assert_eq!(SubtensorModule::bond_cutoff_block(netuid, 102, 5), 102);
    });
}

#[test]
fn null_replaced_miner_bonds_match_clean_baseline_after_return_to_yuma() {
    for yuma3 in [false, true] {
        let run = |stale_bonds| {
            new_test_ext(1).execute_with(|| {
                let netuid = setup([300_000_000, 300_000_000]);
                SubtensorModule::set_epoch_consensus(netuid, EpochConsensus::Yuma);
                SubtensorModule::set_yuma3_enabled(netuid, yuma3);
                LastMechansimStepBlock::<Test>::insert(netuid, 10);
                if stale_bonds {
                    Bonds::<Test>::insert(
                        NetUidStorageIndex::from(netuid),
                        0,
                        vec![(2, 32_767), (3, 32_768)],
                    );
                    Bonds::<Test>::insert(NetUidStorageIndex::from(netuid), 1, vec![(3, u16::MAX)]);
                } else {
                    Bonds::<Test>::insert(NetUidStorageIndex::from(netuid), 0, vec![(2, 32_767)]);
                }
                assert_ok!(SubtensorModule::do_set_epoch_consensus(
                    netuid,
                    EpochConsensus::Null
                ));
                // UID 3 was replaced while bonds were frozen, then Null epochs advanced.
                SubtensorModule::replace_neuron(netuid, 3, &U256::from(4), 50);
                LastMechansimStepBlock::<Test>::insert(netuid, 100);
                System::set_block_number(110);
                set_weights(netuid, 0, vec![2], vec![u16::MAX]);
                seed_historical_weights(netuid, 1, vec![2], vec![u16::MAX]);
                assert_ok!(SubtensorModule::do_set_epoch_consensus(
                    netuid,
                    EpochConsensus::Yuma
                ));
                System::set_block_number(111);
                let output =
                    SubtensorModule::epoch_mechanism(netuid, MechId::MAIN, 1_000u64.into());
                output
                    .0
                    .into_iter()
                    .map(|(hotkey, terms)| {
                        (hotkey, terms.bond, terms.dividend, terms.validator_emission)
                    })
                    .collect::<Vec<_>>()
            })
        };
        assert_eq!(run(true), run(false), "Yuma3={yuma3}");
    }
}

#[test]
fn null_multiple_mechanisms_cannot_multiply_miner_rounding_remainders() {
    new_test_ext(1).execute_with(|| {
        let netuid = setup([300_000_000, 100_000_000]);
        assert_ok!(SubtensorModule::do_set_max_mechanism_count(4.into()));
        assert_ok!(SubtensorModule::do_set_mechanism_count(netuid, 4.into()));
        for miner_budget in [0u64, 1, 2, 3, 5, 501, 1_000_001] {
            let output = SubtensorModule::epoch_with_mechanism_budgets(
                netuid,
                miner_budget.into(),
                200u64.into(),
                300u64.into(),
            );
            assert_eq!(
                output
                    .iter()
                    .map(|(_, miner, _)| miner.to_u64())
                    .sum::<u64>(),
                miner_budget
            );
            assert_eq!(
                output
                    .iter()
                    .map(|(_, _, dividend)| dividend.to_u64())
                    .sum::<u64>(),
                500
            );
            assert_eq!(
                Emission::<Test>::get(netuid)
                    .iter()
                    .map(|amount| amount.to_u64())
                    .sum::<u64>(),
                miner_budget + 500
            );
        }
        // Independently rounded 251-unit mechanism totals would pay 504 miners;
        // the public 1,002-unit epoch must pay exactly 501 instead.
        let output = SubtensorModule::epoch_with_mechanisms(netuid, 1_002u64.into());
        assert_eq!(
            output
                .iter()
                .map(|(_, miner, _)| miner.to_u64())
                .sum::<u64>(),
            501
        );
        assert_eq!(
            output
                .iter()
                .map(|(_, _, dividend)| dividend.to_u64())
                .sum::<u64>(),
            501
        );
    });
}

#[test]
fn null_single_permit_blocks_owner_self_weights_and_all_commit_paths() {
    use frame_support::assert_noop;
    use sp_core::H256;
    new_test_ext(1).execute_with(|| {
        let netuid = setup([100_000_000, 300_000_000]);
        SubnetOwnerHotkey::<Test>::insert(netuid, U256::from(0));
        assert_eq!(SubtensorModule::get_max_allowed_validators(netuid), 1);
        assert_eq!(
            ValidatorPermit::<Test>::get(netuid),
            vec![false, true, false, false]
        );
        assert_noop!(
            SubtensorModule::set_weights(
                RuntimeOrigin::signed(U256::from(0)),
                netuid,
                vec![0],
                vec![1],
                0
            ),
            Error::<Test>::NeuronNoValidatorPermit
        );
        assert_noop!(
            SubtensorModule::set_weights(
                RuntimeOrigin::signed(U256::from(0)),
                netuid,
                vec![2],
                vec![1],
                0
            ),
            Error::<Test>::NeuronNoValidatorPermit
        );
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        assert_noop!(
            SubtensorModule::do_commit_weights(
                RuntimeOrigin::signed(U256::from(0)),
                netuid,
                H256::zero()
            ),
            Error::<Test>::NeuronNoValidatorPermit
        );
        assert_noop!(
            SubtensorModule::do_commit_timelocked_weights(
                RuntimeOrigin::signed(U256::from(0)),
                netuid,
                vec![1].try_into().unwrap(),
                1000,
                SubtensorModule::get_commit_reveal_weights_version()
            ),
            Error::<Test>::NeuronNoValidatorPermit
        );
        assert_noop!(
            SubtensorModule::do_reveal_weights(
                RuntimeOrigin::signed(U256::from(0)),
                netuid,
                vec![2],
                vec![1],
                vec![1],
                0
            ),
            Error::<Test>::NeuronNoValidatorPermit
        );
    });
}

#[test]
fn null_election_replaces_permit_and_dividends_include_nonpermit_stake() {
    new_test_ext(1).execute_with(|| {
        let netuid = setup([300_000_000, 100_000_000]);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &U256::from(1),
            &U256::from(1),
            netuid,
            300_000_000u64.into(),
        );
        let output = SubtensorModule::epoch_mechanism(netuid, MechId::MAIN, 700u64.into());
        assert!(!output.as_map()[&U256::from(0)].new_validator_permit);
        assert!(output.as_map()[&U256::from(1)].new_validator_permit);
        assert_eq!(
            output
                .as_map()
                .values()
                .filter(|term| term.new_validator_permit)
                .count(),
            1
        );
        assert_eq!(
            output.as_map()[&U256::from(0)].validator_emission.to_u64(),
            150
        );
        assert_eq!(
            output.as_map()[&U256::from(1)].validator_emission.to_u64(),
            200
        );
        SubtensorModule::persist_netuid_epoch_terms(netuid, output.as_map());
        assert_eq!(
            ValidatorPermit::<Test>::get(netuid),
            vec![false, true, false, false]
        );
        set_weights(netuid, 1, vec![2, 3], vec![1, 1]);
    });
}

#[test]
fn null_invalid_historical_row_does_not_affect_rewards() {
    new_test_ext(1).execute_with(|| {
        let netuid = setup([300_000_000, 100_000_000]);
        set_weights(netuid, 0, vec![2], vec![1]);
        // A malformed historical row must not influence the winning row or rewards.
        let key = Weights::<Test>::hashed_key_for(NetUidStorageIndex::from(netuid), 1u16);
        sp_io::storage::set(&key, &[255]);
        let output = SubtensorModule::epoch_mechanism(netuid, MechId::MAIN, 100u64.into());
        assert_eq!(output.as_map()[&U256::from(2)].server_emission.to_u64(), 50);
        assert_eq!(sp_io::storage::get(&key).unwrap().as_ref(), &[255]);
    });
}

fn setup_pruning_subnet(n: u16) -> NetUid {
    let netuid = NetUid::from(1);
    add_network_disable_commit_reveal(netuid, u16::MAX - 1, 0);
    SubtensorModule::set_max_allowed_uids(netuid, n);
    SubtensorModule::set_immunity_period(netuid, 0);
    SubnetOwner::<Test>::insert(netuid, U256::from(10_000));
    SubnetOwnerHotkey::<Test>::insert(netuid, U256::from(0));
    for uid in 0..n {
        let hotkey = U256::from(uid);
        Owner::<Test>::insert(hotkey, U256::from(u64::from(uid).saturating_add(1_000)));
        SubtensorModule::append_neuron(netuid, &hotkey, 0);
    }
    SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
        &U256::from(0),
        &U256::from(1_000),
        netuid,
        1_000_000_000u64.into(),
    );
    Emission::<Test>::insert(
        netuid,
        (0..n)
            .map(|uid| AlphaBalance::from(u64::from(uid)))
            .collect::<Vec<_>>(),
    );
    System::set_block_number(10);
    assert_ok!(SubtensorModule::do_set_epoch_consensus(
        netuid,
        EpochConsensus::Null
    ));
    netuid
}

#[test]
fn null_bounded_pruning_compacts_only_tail_moves_and_finishes_target() {
    use frame_support::assert_noop;
    use sp_core::H256;
    use std::collections::VecDeque;
    new_test_ext(1).execute_with(|| {
        let netuid = setup_pruning_subnet(192);
        let index = NetUidStorageIndex::from(netuid);
        for uid in 0..192u16 {
            Weights::<Test>::insert(index, uid, vec![(1, 10), (129, 7), (65, 1)]);
        }
        Bonds::<Test>::insert(index, 0, vec![(129, 11)]);
        WeightCommits::<Test>::insert(
            index,
            U256::from(0),
            VecDeque::from([(H256::zero(), 0, 1, 0)]),
        );
        TimelockedWeightCommits::<Test>::insert(
            index,
            0,
            VecDeque::from([(U256::from(0), 1, vec![1u8].try_into().unwrap(), 1000)]),
        );
        assert_noop!(
            SubtensorModule::trim_to_max_allowed_uids(netuid, 64),
            Error::<Test>::InvalidValue
        );
        assert_ok!(SubtensorModule::trim_null_uids_batch(netuid, 64));
        assert_eq!(SubnetworkN::<Test>::get(netuid), 128);
        assert_eq!(MaxAllowedUids::<Test>::get(netuid), 128);
        assert_eq!(NullPruningTarget::<Test>::get(netuid), Some(64));
        assert_eq!(Keys::<Test>::get(netuid, 1), U256::from(128));
        assert_eq!(Keys::<Test>::get(netuid, 64), U256::from(191));
        assert_eq!(Keys::<Test>::get(netuid, 65), U256::from(65));
        assert_eq!(Uids::<Test>::get(netuid, U256::from(128)), Some(1));
        assert_eq!(Weights::<Test>::get(index, 0), vec![(2, 7), (65, 1)]);
        assert_eq!(Weights::<Test>::iter_key_prefix(index).count(), 1);
        assert_eq!(Bonds::<Test>::get(index, 0), vec![(2, 11)]);
        assert!(
            WeightCommits::<Test>::iter_key_prefix(index)
                .next()
                .is_none()
        );
        assert!(
            TimelockedWeightCommits::<Test>::iter_key_prefix(index)
                .next()
                .is_none()
        );
        assert_ok!(SubtensorModule::trim_null_uids_batch(netuid, 64));
        assert_eq!(SubnetworkN::<Test>::get(netuid), 64);
        assert_eq!(MaxAllowedUids::<Test>::get(netuid), 64);
        assert_eq!(NullPruningTarget::<Test>::get(netuid), None);
        assert_eq!(Weights::<Test>::get(index, 0), vec![(2, 7)]);
        assert_eq!(Keys::<Test>::get(netuid, 1), U256::from(191));
        assert_eq!(
            ValidatorPermit::<Test>::get(netuid)
                .iter()
                .filter(|value| **value)
                .count(),
            1
        );
        for uid in 0..64u16 {
            assert_eq!(
                Uids::<Test>::get(netuid, Keys::<Test>::get(netuid, uid)),
                Some(uid)
            );
        }
        assert_ok!(SubtensorModule::do_set_epoch_consensus(
            netuid,
            EpochConsensus::Yuma
        ));
        assert_eq!(SubtensorModule::get_max_allowed_validators(netuid), 64);
    });
}

#[test]
fn null_pruning_checks_final_immunity_before_mutation() {
    use frame_support::assert_noop;
    new_test_ext(1).execute_with(|| {
        let netuid = setup_pruning_subnet(192);
        SubtensorModule::set_immunity_period(netuid, 100);
        assert_noop!(
            SubtensorModule::trim_null_uids_batch(netuid, 64),
            Error::<Test>::TrimmingWouldExceedMaxImmunePercentage
        );
        assert_eq!(SubnetworkN::<Test>::get(netuid), 192);
        assert_eq!(MaxAllowedUids::<Test>::get(netuid), 192);
        assert_eq!(NullPruningTarget::<Test>::get(netuid), None);
    });
}

#[test]
fn null_pruning_cancels_orphan_commits_in_bounded_cleanup_steps() {
    use sp_core::H256;
    use std::collections::VecDeque;
    new_test_ext(1).execute_with(|| {
        let netuid = setup_pruning_subnet(128);
        let index = NetUidStorageIndex::from(netuid);
        for uid in 0..2_501u16 {
            WeightCommits::<Test>::insert(
                index,
                U256::from(uid),
                VecDeque::from([(H256::zero(), 0, 1, 0)]),
            );
        }
        assert_ok!(SubtensorModule::trim_null_uids_batch(netuid, 64));
        assert_eq!(WeightCommits::<Test>::iter_key_prefix(index).count(), 1);
        assert_eq!(SubnetworkN::<Test>::get(netuid), 128);
        assert_eq!(NullPruningTarget::<Test>::get(netuid), Some(64));
        assert_ok!(SubtensorModule::trim_null_uids_batch(netuid, 64));
        assert!(
            WeightCommits::<Test>::iter_key_prefix(index)
                .next()
                .is_none()
        );
        assert_eq!(SubnetworkN::<Test>::get(netuid), 64);
        assert_eq!(NullPruningTarget::<Test>::get(netuid), None);
    });
}

#[test]
fn null_commit_cleanup_allows_mechanism_changes_without_deleting_uids() {
    use frame_support::assert_noop;
    use sp_core::H256;
    use std::collections::VecDeque;
    new_test_ext(1).execute_with(|| {
        let netuid = setup_pruning_subnet(128);
        let index = NetUidStorageIndex::from(netuid);
        MaxMechanismCount::<Test>::put(MechId::from(16));
        for uid in 0..2_501u16 {
            WeightCommits::<Test>::insert(
                index,
                U256::from(uid),
                VecDeque::from([(H256::zero(), 0, 1, 0)]),
            );
        }
        assert_noop!(
            SubtensorModule::do_set_mechanism_count(netuid, MechId::from(2)),
            Error::<Test>::InvalidValue
        );
        assert_ok!(SubtensorModule::trim_null_uids_batch(netuid, 128));
        assert_eq!(SubnetworkN::<Test>::get(netuid), 128);
        assert_eq!(NullPruningTarget::<Test>::get(netuid), Some(128));
        assert_eq!(WeightCommits::<Test>::iter_key_prefix(index).count(), 1);
        assert_ok!(SubtensorModule::trim_null_uids_batch(netuid, 128));
        assert_eq!(SubnetworkN::<Test>::get(netuid), 128);
        assert_eq!(NullPruningTarget::<Test>::get(netuid), None);
        assert_ok!(SubtensorModule::do_set_mechanism_count(
            netuid,
            MechId::from(2)
        ));
        assert_eq!(MechanismCountCurrent::<Test>::get(netuid), MechId::from(2));
    });
}
