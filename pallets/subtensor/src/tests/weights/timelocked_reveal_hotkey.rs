#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
//! Timelocked (CRv3) reveal: hotkey checks, missing pulse retry, legacy payload.

use ark_serialize::CanonicalDeserialize;
use ark_serialize::CanonicalSerialize;
use frame_support::assert_ok;
use pallet_drand::types::Pulse;
use rand_chacha::{ChaCha20Rng, rand_core::SeedableRng};
use sha2::Digest;
use sp_core::Encode;
use sp_core::U256;
use sp_runtime::{BoundedVec, traits::ConstU32};
use substrate_fixed::types::I32F32;
use subtensor_runtime_common::NetUidStorageIndex;
use tle::{
    curves::drand::TinyBLS381, ibe::fullident::Identity,
    stream_ciphers::AESGCMStreamCipherProvider, tlock::tle,
};
use w3f_bls::EngineBLS;

use crate::coinbase::reveal_commits::{LegacyWeightsTlockPayload, WeightsTlockPayload};
use crate::tests::mock::*;
use crate::*;

#[test]
fn test_reveal_crv3_commits_hotkey_check() {
    new_test_ext(100).execute_with(|| {
        // Failure case: hotkey mismatch
        let netuid = NetUid::from(1);
        let hotkey1: AccountId = U256::from(1);
        let hotkey2: AccountId = U256::from(2);
        let reveal_round: u64 = 1000;

        add_network(netuid, 5, 0);
        register_ok_neuron(netuid, hotkey1, U256::from(3), 100_000);
        register_ok_neuron(netuid, hotkey2, U256::from(4), 100_000);
        SubtensorModule::set_stake_threshold(0);
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        assert_ok!(SubtensorModule::set_reveal_period(netuid, 3));

        let neuron_uid1 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey1)
            .expect("Failed to get neuron UID for hotkey1");
        let neuron_uid2 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey2)
            .expect("Failed to get neuron UID for hotkey2");

        SubtensorModule::set_validator_permit_for_uid(netuid, neuron_uid1, true);
        SubtensorModule::set_validator_permit_for_uid(netuid, neuron_uid2, true);
        add_balance_to_coldkey_account(&U256::from(3), 1.into());
        add_balance_to_coldkey_account(&U256::from(4), 1.into());
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey1,
            &(U256::from(3)),
            netuid,
            1.into(),
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey2,
            &(U256::from(4)),
            netuid,
            1.into(),
        );

        let version_key = SubtensorModule::get_weights_version_key(netuid);

        let payload = WeightsTlockPayload {
            hotkey: hotkey2.encode(), // Mismatch: using hotkey2 instead of hotkey1
            values: vec![10, 20],
            uids: vec![neuron_uid1, neuron_uid2],
            version_key,
        };

        let serialized_payload = payload.encode();

        let esk = [2; 32];
        let rng = ChaCha20Rng::seed_from_u64(0);

        let pk_bytes = hex::decode("83cf0f2896adee7eb8b5f01fcad3912212c437e0073e911fb90022d3e760183c8c4b450b6a0a6c3ac6a5776a2d1064510d1fec758c921cc22b0e17e63aaf4bcb5ed66304de9cf809bd274ca73bab4af5a6e9c76a4bc09e76eae8991ef5ece45a")
            .expect("Failed to decode public key bytes");
        let pub_key = <TinyBLS381 as EngineBLS>::PublicKeyGroup::deserialize_compressed(&*pk_bytes)
            .expect("Failed to deserialize public key");

        let message = {
            let mut hasher = sha2::Sha256::new();
            hasher.update(reveal_round.to_be_bytes());
            hasher.finalize().to_vec()
        };
        let identity = Identity::new(b"", vec![message]);

        let ct = tle::<TinyBLS381, AESGCMStreamCipherProvider, ChaCha20Rng>(
            pub_key,
            esk,
            &serialized_payload,
            identity,
            rng,
        )
        .expect("Encryption failed");

        let mut commit_bytes = Vec::new();
        ct.serialize_compressed(&mut commit_bytes)
            .expect("Failed to serialize commit");

        assert!(
            !commit_bytes.is_empty(),
            "commit_bytes is empty after serialization"
        );

        log::debug!(
            "Commit bytes now contain {commit_bytes:#?}"
        );

        assert_ok!(SubtensorModule::do_commit_timelocked_weights(
            RuntimeOrigin::signed(hotkey1),
            netuid,
            commit_bytes.clone().try_into().expect("Failed to convert commit bytes into bounded vector"),
            reveal_round,
            SubtensorModule::get_commit_reveal_weights_version()
        ));

        let sig_bytes = hex::decode("b44679b9a59af2ec876b1a6b1ad52ea9b1615fc3982b19576350f93447cb1125e342b73a8dd2bacbe47e4b6b63ed5e39")
            .expect("Failed to decode signature bytes");

        pallet_drand::Pulses::<Test>::insert(
            reveal_round,
            Pulse {
                round: reveal_round,
                randomness: vec![0; 32].try_into().expect("Failed to convert randomness vector"),
                signature: sig_bytes.try_into().expect("Failed to convert signature bytes"),
            },
        );

        // Step epochs to run the epoch via the blockstep
        step_epochs(3, netuid);

        let weights_sparse = SubtensorModule::get_weights_sparse(netuid.into());
        let weights = weights_sparse.get(neuron_uid1 as usize).cloned().unwrap_or_default();

        assert!(
            weights.is_empty(),
            "Weights for neuron_uid1 should be empty due to hotkey mismatch."
        );
    });

    new_test_ext(100).execute_with(|| {
        // Success case: hotkey match
        let netuid = NetUid::from(1);
        let hotkey1: AccountId = U256::from(1);
        let hotkey2: AccountId = U256::from(2);
        let reveal_round: u64 = 1000;

        add_network(netuid, 5, 0);
        register_ok_neuron(netuid, hotkey1, U256::from(3), 100_000);
        register_ok_neuron(netuid, hotkey2, U256::from(4), 100_000);
        SubtensorModule::set_stake_threshold(0);
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        assert_ok!(SubtensorModule::set_reveal_period(netuid, 3));

        let neuron_uid1 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey1)
            .expect("Failed to get neuron UID for hotkey1");
        let neuron_uid2 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey2)
            .expect("Failed to get neuron UID for hotkey2");

        SubtensorModule::set_validator_permit_for_uid(netuid, neuron_uid1, true);
        SubtensorModule::set_validator_permit_for_uid(netuid, neuron_uid2, true);
        add_balance_to_coldkey_account(&U256::from(3), 1.into());
        add_balance_to_coldkey_account(&U256::from(4), 1.into());
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey1,
            &(U256::from(3)),
            netuid,
            1.into(),
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey2,
            &(U256::from(4)),
            netuid,
            1.into(),
        );

        let version_key = SubtensorModule::get_weights_version_key(netuid);

        let payload = WeightsTlockPayload {
            hotkey: hotkey1.encode(), // Match: using hotkey1
            values: vec![10, 20],
            uids: vec![neuron_uid1, neuron_uid2],
            version_key,
        };

        let serialized_payload = payload.encode();

        let esk = [2; 32];
        let rng = ChaCha20Rng::seed_from_u64(0);

        let pk_bytes = hex::decode("83cf0f2896adee7eb8b5f01fcad3912212c437e0073e911fb90022d3e760183c8c4b450b6a0a6c3ac6a5776a2d1064510d1fec758c921cc22b0e17e63aaf4bcb5ed66304de9cf809bd274ca73bab4af5a6e9c76a4bc09e76eae8991ef5ece45a")
            .expect("Failed to decode public key bytes");
        let pub_key = <TinyBLS381 as EngineBLS>::PublicKeyGroup::deserialize_compressed(&*pk_bytes)
            .expect("Failed to deserialize public key");

        let message = {
            let mut hasher = sha2::Sha256::new();
            hasher.update(reveal_round.to_be_bytes());
            hasher.finalize().to_vec()
        };
        let identity = Identity::new(b"", vec![message]);

        let ct = tle::<TinyBLS381, AESGCMStreamCipherProvider, ChaCha20Rng>(
            pub_key,
            esk,
            &serialized_payload,
            identity,
            rng,
        )
        .expect("Encryption failed");

        let mut commit_bytes = Vec::new();
        ct.serialize_compressed(&mut commit_bytes)
            .expect("Failed to serialize commit");

        assert!(
            !commit_bytes.is_empty(),
            "commit_bytes is empty after serialization"
        );

        log::debug!(
            "Commit bytes now contain {commit_bytes:#?}"
        );

        assert_ok!(SubtensorModule::do_commit_timelocked_weights(
            RuntimeOrigin::signed(hotkey1),
            netuid,
            commit_bytes.clone().try_into().expect("Failed to convert commit bytes into bounded vector"),
            reveal_round,
            SubtensorModule::get_commit_reveal_weights_version()
        ));

        let sig_bytes = hex::decode("b44679b9a59af2ec876b1a6b1ad52ea9b1615fc3982b19576350f93447cb1125e342b73a8dd2bacbe47e4b6b63ed5e39")
            .expect("Failed to decode signature bytes");

        pallet_drand::Pulses::<Test>::insert(
            reveal_round,
            Pulse {
                round: reveal_round,
                randomness: vec![0; 32].try_into().expect("Failed to convert randomness vector"),
                signature: sig_bytes.try_into().expect("Failed to convert signature bytes"),
            },
        );

        // Step epochs to run the epoch via the blockstep
        step_epochs(3, netuid);

        let weights_sparse = SubtensorModule::get_weights_sparse(netuid.into());
        let weights = weights_sparse.get(neuron_uid1 as usize).cloned().unwrap_or_default();

        assert!(
            !weights.is_empty(),
            "Weights for neuron_uid1 are empty, expected weights to be set."
        );

        let expected_weights: Vec<(u16, I32F32)> = payload
            .uids
            .iter()
            .zip(payload.values.iter())
            .map(|(&uid, &value)| (uid, I32F32::from_num(value)))
            .collect();

        let total_weight: I32F32 = weights.iter().map(|(_, w)| *w).sum();

        let normalized_weights: Vec<(u16, I32F32)> = weights
            .iter()
            .map(|&(uid, w)| (uid, w * I32F32::from_num(30) / total_weight))
            .collect();

        for ((uid_a, w_a), (uid_b, w_b)) in normalized_weights.iter().zip(expected_weights.iter()) {
            assert_eq!(uid_a, uid_b);

            let actual_weight_f64: f64 = w_a.to_num::<f64>();
            let rounded_actual_weight = actual_weight_f64.round() as i64;

            assert!(
                rounded_actual_weight != 0,
                "Actual weight for uid {uid_a} is zero"
            );

            let expected_weight = w_b.to_num::<i64>();

            assert_eq!(
                rounded_actual_weight, expected_weight,
                "Weight mismatch for uid {uid_a}: expected {expected_weight}, got {rounded_actual_weight}"
            );
        }
    });
}

#[test]
fn test_reveal_crv3_commits_retry_on_missing_pulse() {
    new_test_ext(100).execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey: AccountId = U256::from(1);
        let reveal_round: u64 = 1_000;

        // ─── network & neuron ───────────────────────────────────────────────
        add_network(netuid, 5, 0);
        register_ok_neuron(netuid, hotkey, U256::from(3), 100_000);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        assert_ok!(SubtensorModule::set_reveal_period(netuid, 3));
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);
        SubtensorModule::set_stake_threshold(0);

        let uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey).unwrap();
        SubtensorModule::set_validator_permit_for_uid(netuid, uid, true);

        // ─── craft commit ───────────────────────────────────────────────────
        let payload = WeightsTlockPayload {
            hotkey: hotkey.encode(),
            values: vec![10],
            uids: vec![uid],
            version_key: SubtensorModule::get_weights_version_key(netuid),
        };
        let esk = [2u8; 32];
        let pk_bytes = hex::decode(
            "83cf0f2896adee7eb8b5f01fcad3912212c437e0073e911fb90022d3e760183c\
             8c4b450b6a0a6c3ac6a5776a2d1064510d1fec758c921cc22b0e17e63aaf4bcb\
             5ed66304de9cf809bd274ca73bab4af5a6e9c76a4bc09e76eae8991ef5ece45a",
        )
        .unwrap();
        let pk =
            <TinyBLS381 as EngineBLS>::PublicKeyGroup::deserialize_compressed(&*pk_bytes).unwrap();
        let id_msg = {
            let mut h = sha2::Sha256::new();
            h.update(reveal_round.to_be_bytes());
            h.finalize().to_vec()
        };
        let ct = tle::<TinyBLS381, AESGCMStreamCipherProvider, ChaCha20Rng>(
            pk,
            esk,
            &payload.encode(),
            Identity::new(b"", vec![id_msg]),
            ChaCha20Rng::seed_from_u64(0),
        )
        .unwrap();
        let mut commit_bytes = Vec::new();
        ct.serialize_compressed(&mut commit_bytes).unwrap();

        // submit commit
        assert_ok!(SubtensorModule::do_commit_timelocked_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            commit_bytes.clone().try_into().unwrap(),
            reveal_round,
            SubtensorModule::get_commit_reveal_weights_version()
        ));

        // epoch in which commit was stored
        let stored_epoch =
            TimelockedWeightCommits::<Test>::iter_prefix(NetUidStorageIndex::from(netuid))
                .next()
                .map(|(e, _)| e)
                .expect("commit stored");

        // Place the subnet's epoch counter at the commit's reveal epoch
        // (`commit_epoch + reveal_period`). The counter is the canonical epoch
        // index; pin `LastEpochBlock`/`PendingEpochAt` so `should_run_epoch` stays
        // false and the look-ahead does not skip past the reveal epoch.
        let reveal_epoch = stored_epoch + SubtensorModule::get_reveal_period(netuid);
        SubnetEpochIndex::<Test>::insert(netuid, reveal_epoch);
        LastEpochBlock::<Test>::insert(netuid, SubtensorModule::get_current_block_as_u64());
        PendingEpochAt::<Test>::insert(netuid, 0);

        // run *one* block inside reveal epoch without pulse → commit should stay queued
        step_block(1);
        assert!(
            !TimelockedWeightCommits::<Test>::get(NetUidStorageIndex::from(netuid), stored_epoch)
                .is_empty(),
            "commit must remain queued when pulse is missing"
        );

        // ─── insert pulse & step one more block ─────────────────────────────
        let sig_bytes = hex::decode(
            "b44679b9a59af2ec876b1a6b1ad52ea9b1615fc3982b19576350f93447cb1125e\
             342b73a8dd2bacbe47e4b6b63ed5e39",
        )
        .unwrap();
        pallet_drand::Pulses::<Test>::insert(
            reveal_round,
            Pulse {
                round: reveal_round,
                randomness: vec![0; 32].try_into().unwrap(),
                signature: sig_bytes.try_into().unwrap(),
            },
        );

        step_block(1); // automatic reveal runs here

        let weights = SubtensorModule::get_weights_sparse(netuid.into())
            .get(uid as usize)
            .cloned()
            .unwrap_or_default();
        assert!(!weights.is_empty(), "weights must be set after pulse");

        assert!(
            TimelockedWeightCommits::<Test>::get(NetUidStorageIndex::from(netuid), stored_epoch)
                .is_empty(),
            "queue should be empty after successful reveal"
        );
    });
}

#[test]
fn test_reveal_crv3_commits_legacy_payload_success() {
    new_test_ext(100).execute_with(|| {
        // ─────────────────────────────────────
        // 1 ▸ network + neurons
        // ─────────────────────────────────────
        let netuid = NetUid::from(1);
        let hotkey1: AccountId = U256::from(1);
        let hotkey2: AccountId = U256::from(2);
        let reveal_round: u64 = 1_000;

        add_network(netuid, /*tempo*/ 5, /*modality*/ 0);
        register_ok_neuron(netuid, hotkey1, U256::from(3), 100_000);
        register_ok_neuron(netuid, hotkey2, U256::from(4), 100_000);

        SubtensorModule::set_stake_threshold(0);
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        assert_ok!(SubtensorModule::set_reveal_period(netuid, 3));

        let uid1 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey1).unwrap();
        let uid2 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey2).unwrap();

        SubtensorModule::set_validator_permit_for_uid(netuid, uid1, true);
        SubtensorModule::set_validator_permit_for_uid(netuid, uid2, true);

        add_balance_to_coldkey_account(&U256::from(3), 1.into());
        add_balance_to_coldkey_account(&U256::from(4), 1.into());
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey1,
            &U256::from(3),
            netuid,
            1.into(),
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey2,
            &U256::from(4),
            netuid,
            1.into(),
        );

        // ─────────────────────────────────────
        // 2 ▸ craft legacy payload (NO hotkey)
        // ─────────────────────────────────────
        let legacy_payload = LegacyWeightsTlockPayload {
            uids: vec![uid1, uid2],
            values: vec![10, 20],
            version_key: SubtensorModule::get_weights_version_key(netuid),
        };
        let serialized_payload = legacy_payload.encode();

        // encrypt with TLE
        let esk = [2u8; 32];
        let rng = ChaCha20Rng::seed_from_u64(0);

        let pk_bytes = hex::decode(
            "83cf0f2896adee7eb8b5f01fcad3912212c437e0073e911fb90022d3e760183c\
             8c4b450b6a0a6c3ac6a5776a2d1064510d1fec758c921cc22b0e17e63aaf4bcb\
             5ed66304de9cf809bd274ca73bab4af5a6e9c76a4bc09e76eae8991ef5ece45a",
        )
        .unwrap();
        let pk =
            <TinyBLS381 as EngineBLS>::PublicKeyGroup::deserialize_compressed(&*pk_bytes).unwrap();

        let msg_hash = {
            let mut h = sha2::Sha256::new();
            h.update(reveal_round.to_be_bytes());
            h.finalize().to_vec()
        };
        let identity = Identity::new(b"", vec![msg_hash]);

        let ct = tle::<TinyBLS381, AESGCMStreamCipherProvider, ChaCha20Rng>(
            pk,
            esk,
            &serialized_payload,
            identity,
            rng,
        )
        .expect("encryption must succeed");

        let mut commit_bytes = Vec::new();
        ct.serialize_compressed(&mut commit_bytes).unwrap();
        let bounded_commit: BoundedVec<_, ConstU32<MAX_CRV3_COMMIT_SIZE_BYTES>> =
            commit_bytes.clone().try_into().unwrap();

        // ─────────────────────────────────────
        // 3 ▸ put commit on‑chain
        // ─────────────────────────────────────
        assert_ok!(SubtensorModule::do_commit_timelocked_weights(
            RuntimeOrigin::signed(hotkey1),
            netuid,
            bounded_commit,
            reveal_round,
            SubtensorModule::get_commit_reveal_weights_version()
        ));

        // insert pulse so reveal can succeed the first time
        let sig_bytes = hex::decode(
            "b44679b9a59af2ec876b1a6b1ad52ea9b1615fc3982b19576350f93447cb1125e3\
             42b73a8dd2bacbe47e4b6b63ed5e39",
        )
        .unwrap();
        pallet_drand::Pulses::<Test>::insert(
            reveal_round,
            Pulse {
                round: reveal_round,
                randomness: vec![0; 32].try_into().unwrap(),
                signature: sig_bytes.try_into().unwrap(),
            },
        );

        let commit_block = SubtensorModule::get_current_block_as_u64();
        let commit_epoch = SubtensorModule::get_epoch_index(netuid, commit_block);

        // ─────────────────────────────────────
        // 4 ▸ advance epochs to trigger reveal
        // ─────────────────────────────────────
        step_epochs(3, netuid);

        // ─────────────────────────────────────
        // 5 ▸ assertions
        // ─────────────────────────────────────
        let weights_sparse = SubtensorModule::get_weights_sparse(netuid.into());
        let w1 = weights_sparse
            .get(uid1 as usize)
            .cloned()
            .unwrap_or_default();
        assert!(!w1.is_empty(), "weights must be set for uid1");

        // find raw values for uid1 & uid2
        let w_map: std::collections::HashMap<_, _> = w1.into_iter().collect();
        let v1 = *w_map.get(&uid1).expect("uid1 weight");
        let v2 = *w_map.get(&uid2).expect("uid2 weight");
        assert!(v2 > v1, "uid2 weight should be greater than uid1 (20 > 10)");

        // commit should be gone
        assert!(
            TimelockedWeightCommits::<Test>::get(NetUidStorageIndex::from(netuid), commit_epoch)
                .is_empty(),
            "commit storage should be cleaned after reveal"
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_subnet_owner_can_validate_without_stake_or_manual_permit --exact --show-output --nocapture
