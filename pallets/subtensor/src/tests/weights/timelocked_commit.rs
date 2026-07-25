#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
//! Timelocked (CRv3) weight commits and tlock encrypt/decrypt smoke test.

use ark_serialize::CanonicalDeserialize;
use ark_serialize::CanonicalSerialize;
use frame_support::{assert_err, assert_ok};
use pallet_drand::types::Pulse;
use rand_chacha::{ChaCha20Rng, rand_core::SeedableRng};
use sha2::Digest;
use sp_core::Encode;
use sp_core::U256;
use substrate_fixed::types::I32F32;
use subtensor_runtime_common::NetUidStorageIndex;
use tle::{
    curves::drand::TinyBLS381,
    ibe::fullident::Identity,
    stream_ciphers::AESGCMStreamCipherProvider,
    tlock::{tld, tle},
};
use w3f_bls::EngineBLS;

use crate::coinbase::reveal_commits::WeightsTlockPayload;
use crate::tests::mock::*;
use crate::*;

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::tlock_encrypt_decrypt_drand_quicknet_works --exact --show-output --nocapture
#[test]
pub fn tlock_encrypt_decrypt_drand_quicknet_works() {
    // using a pulse from drand's QuickNet
    // https://api.drand.sh/52db9ba70e0cc0f6eaf7803dd07447a1f5477735fd3f661792ba94600c84e971/public/1000
    // the beacon public key
    let pk_bytes =
        b"83cf0f2896adee7eb8b5f01fcad3912212c437e0073e911fb90022d3e760183c8c4b450b6a0a6c3ac6a5776a2d1064510d1fec758c921cc22b0e17e63aaf4bcb5ed66304de9cf809bd274ca73bab4af5a6e9c76a4bc09e76eae8991ef5ece45a"
    ; // a round number that we know a signature for
    let round: u64 = 1000;
    // the signature produced in that round
    let signature =
        b"b44679b9a59af2ec876b1a6b1ad52ea9b1615fc3982b19576350f93447cb1125e342b73a8dd2bacbe47e4b6b63ed5e39"
    ;

    // Convert hex string to bytes
    let pub_key_bytes = hex::decode(pk_bytes).expect("Failed to decode public key bytes");
    // Deserialize to G1Affine
    let pub_key =
        <TinyBLS381 as EngineBLS>::PublicKeyGroup::deserialize_compressed(&*pub_key_bytes)
            .expect("Failed to deserialize public key");

    // then we tlock a message for the pubkey
    let plaintext = b"this is a test".as_slice();
    let esk = [2; 32];

    let sig_bytes = hex::decode(signature).expect("Failed to decode signature bytes");
    let sig = <TinyBLS381 as EngineBLS>::SignatureGroup::deserialize_compressed(&*sig_bytes)
        .expect("Failed to deserialize signature");

    let message = {
        let mut hasher = sha2::Sha256::new();
        hasher.update(round.to_be_bytes());
        hasher.finalize().to_vec()
    };

    let identity = Identity::new(b"", vec![message]);

    let rng = ChaCha20Rng::seed_from_u64(0);
    let ct = tle::<TinyBLS381, AESGCMStreamCipherProvider, ChaCha20Rng>(
        pub_key, esk, plaintext, identity, rng,
    )
    .expect("Encryption failed");

    // then we can decrypt the ciphertext using the signature
    let result = tld::<TinyBLS381, AESGCMStreamCipherProvider>(ct, sig).expect("Decryption failed");
    assert!(result == plaintext);
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_reveal_crv3_commits_success --exact --show-output --nocapture

#[test]
fn test_reveal_crv3_commits_success() {
    new_test_ext(100).execute_with(|| {
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
            hotkey: hotkey1.encode(),
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

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_reveal_crv3_commits_cannot_reveal_after_reveal_epoch --exact --show-output --nocapture
#[test]
fn test_reveal_crv3_commits_cannot_reveal_after_reveal_epoch() {
    new_test_ext(100).execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey1: AccountId = U256::from(1);
        let hotkey2: AccountId = U256::from(2);
        let reveal_round: u64 = 1000;

        add_network(netuid, 5, 0);
        register_ok_neuron(netuid, hotkey1, U256::from(3), 100_000);
        register_ok_neuron(netuid, hotkey2, U256::from(4), 100_000);
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        assert_ok!(SubtensorModule::set_reveal_period(netuid, 3));

        let neuron_uid1 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey1)
            .expect("Failed to get neuron UID for hotkey1");
        let neuron_uid2 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey2)
            .expect("Failed to get neuron UID for hotkey2");

        SubtensorModule::set_validator_permit_for_uid(netuid, neuron_uid1, true);
        SubtensorModule::set_validator_permit_for_uid(netuid, neuron_uid2, true);

        let version_key = SubtensorModule::get_weights_version_key(netuid);

        let payload = WeightsTlockPayload {
            hotkey: hotkey1.encode(),
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

        assert_ok!(SubtensorModule::do_commit_timelocked_weights(
            RuntimeOrigin::signed(hotkey1),
            netuid,
            commit_bytes
                .clone()
                .try_into()
                .expect("Failed to convert commit bytes into bounded vector"),
            reveal_round,
            SubtensorModule::get_commit_reveal_weights_version()
        ));

        // Do NOT insert the pulse at this time; this simulates the missing pulse during the reveal epoch
        // Advance epochs to reach the reveal epoch (3 epochs as reveal_period is 3)
        step_epochs(3, netuid);

        // Verify that weights are not set
        let weights_sparse = SubtensorModule::get_weights_sparse(netuid.into());
        let weights = weights_sparse
            .get(neuron_uid1 as usize)
            .cloned()
            .unwrap_or_default();

        assert!(
            weights.is_empty(),
            "Weights for neuron_uid1 should be empty as the commit cannot be revealed without the pulse."
        );

        // Now, after the reveal epoch has passed, insert the pulse
        let sig_bytes = hex::decode("b44679b9a59af2ec876b1a6b1ad52ea9b1615fc3982b19576350f93447cb1125e342b73a8dd2bacbe47e4b6b63ed5e39")
            .expect("Failed to decode signature bytes");

        pallet_drand::Pulses::<Test>::insert(
            reveal_round,
            Pulse {
                round: reveal_round,
                randomness: vec![0; 32]
                    .try_into()
                    .expect("Failed to convert randomness vector"),
                signature: sig_bytes
                    .try_into()
                    .expect("Failed to convert signature bytes"),
            },
        );

        // Advance one more epoch to be after the reveal epoch
        step_epochs(1, netuid);

        // Attempt to reveal commits after the reveal epoch has passed
        assert_ok!(SubtensorModule::reveal_crv3_commits_for_subnet(netuid));

        // Verify that the weights for the neuron have not been set
        let weights_sparse = SubtensorModule::get_weights_sparse(netuid.into());
        let weights = weights_sparse
            .get(neuron_uid1 as usize)
            .cloned()
            .unwrap_or_default();

        assert!(
            weights.is_empty(),
            "Weights for neuron_uid1 should be empty as the commit cannot be revealed after the reveal epoch."
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_do_commit_crv3_weights_success --exact --show-output --nocapture
#[test]
fn test_do_commit_crv3_weights_success() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey: AccountId = U256::from(1);
        let commit_data: Vec<u8> = vec![1, 2, 3, 4, 5];
        let reveal_round: u64 = 1000;

        add_network(netuid, 5, 0);
        register_ok_neuron(netuid, hotkey, U256::from(2), 100_000);
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);

        assert_ok!(SubtensorModule::do_commit_timelocked_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            commit_data
                .clone()
                .try_into()
                .expect("Failed to convert commit data into bounded vector"),
            reveal_round,
            SubtensorModule::get_commit_reveal_weights_version()
        ));

        let cur_epoch =
            SubtensorModule::get_epoch_index(netuid, SubtensorModule::get_current_block_as_u64());
        let commits =
            TimelockedWeightCommits::<Test>::get(NetUidStorageIndex::from(netuid), cur_epoch);
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].0, hotkey);
        assert_eq!(commits[0].2, commit_data);
        assert_eq!(commits[0].3, reveal_round);
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_do_commit_crv3_weights_disabled --exact --show-output --nocapture
#[test]
fn test_do_commit_crv3_weights_disabled() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey: AccountId = U256::from(1);
        let commit_data: Vec<u8> = vec![1, 2, 3, 4, 5];
        let reveal_round: u64 = 1000;

        add_network(netuid, 5, 0);
        register_ok_neuron(netuid, hotkey, U256::from(2), 100_000);
        SubtensorModule::set_weights_set_rate_limit(netuid, 5);

        SubtensorModule::set_commit_reveal_weights_enabled(netuid, false);
        assert_err!(
            SubtensorModule::do_commit_timelocked_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                commit_data
                    .try_into()
                    .expect("Failed to convert commit data into bounded vector"),
                reveal_round,
                SubtensorModule::get_commit_reveal_weights_version()
            ),
            Error::<Test>::CommitRevealDisabled
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_do_commit_crv3_weights_hotkey_not_registered --exact --show-output --nocapture
#[test]
fn test_do_commit_crv3_weights_hotkey_not_registered() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let unregistered_hotkey: AccountId = U256::from(99);
        let commit_data: Vec<u8> = vec![1, 2, 3, 4, 5];
        let reveal_round: u64 = 1000;
        let hotkey: AccountId = U256::from(1);

        add_network(netuid, 5, 0);
        register_ok_neuron(netuid, hotkey, U256::from(2), 100_000);
        SubtensorModule::set_weights_set_rate_limit(netuid, 5);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);

        assert_err!(
            SubtensorModule::do_commit_timelocked_weights(
                RuntimeOrigin::signed(unregistered_hotkey),
                netuid,
                commit_data
                    .try_into()
                    .expect("Failed to convert commit data into bounded vector"),
                reveal_round,
                SubtensorModule::get_commit_reveal_weights_version()
            ),
            Error::<Test>::HotKeyNotRegisteredInSubNet
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_do_commit_crv3_weights_committing_too_fast --exact --show-output --nocapture
#[test]
fn test_do_commit_crv3_weights_committing_too_fast() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey: AccountId = U256::from(1);
        let commit_data_1: Vec<u8> = vec![1, 2, 3];
        let commit_data_2: Vec<u8> = vec![4, 5, 6];
        let reveal_round: u64 = 1000;

        add_network(netuid, 5, 0);
        register_ok_neuron(netuid, hotkey, U256::from(2), 100_000);
        SubtensorModule::set_weights_set_rate_limit(netuid, 5);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        let neuron_uid =
            SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey).expect("Expected uid");
        SubtensorModule::set_last_update_for_uid(NetUidStorageIndex::from(netuid), neuron_uid, 0);

        assert_ok!(SubtensorModule::do_commit_timelocked_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            commit_data_1
                .clone()
                .try_into()
                .expect("Failed to convert commit data into bounded vector"),
            reveal_round,
            SubtensorModule::get_commit_reveal_weights_version()
        ));

        assert_err!(
            SubtensorModule::do_commit_timelocked_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                commit_data_2
                    .clone()
                    .try_into()
                    .expect("Failed to convert commit data into bounded vector"),
                reveal_round,
                SubtensorModule::get_commit_reveal_weights_version()
            ),
            Error::<Test>::CommittingWeightsTooFast
        );

        step_block(2);

        assert_err!(
            SubtensorModule::do_commit_timelocked_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                commit_data_2
                    .clone()
                    .try_into()
                    .expect("Failed to convert commit data into bounded vector"),
                reveal_round,
                SubtensorModule::get_commit_reveal_weights_version()
            ),
            Error::<Test>::CommittingWeightsTooFast
        );

        step_block(3);

        assert_ok!(SubtensorModule::do_commit_timelocked_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            commit_data_2
                .try_into()
                .expect("Failed to convert commit data into bounded vector"),
            reveal_round,
            SubtensorModule::get_commit_reveal_weights_version()
        ));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_do_commit_crv3_weights_too_many_unrevealed_commits --exact --show-output --nocapture
#[test]
fn test_do_commit_crv3_weights_too_many_unrevealed_commits() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey1: AccountId = U256::from(1);
        let hotkey2: AccountId = U256::from(2);
        let reveal_round: u64 = 1000;

        add_network(netuid, 5, 0);
        register_ok_neuron(netuid, hotkey1, U256::from(2), 100_000);
        register_ok_neuron(netuid, hotkey2, U256::from(3), 100_000);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);

        // Hotkey1 submits 10 commits successfully
        for i in 0..10 {
            let commit_data: Vec<u8> = vec![i as u8; 5];
            let bounded_commit_data = commit_data
                .try_into()
                .expect("Failed to convert commit data into bounded vector");

            assert_ok!(SubtensorModule::do_commit_timelocked_weights(
                RuntimeOrigin::signed(hotkey1),
                netuid,
                bounded_commit_data,
                reveal_round,
                SubtensorModule::get_commit_reveal_weights_version()
            ));
        }

        // Hotkey1 attempts to commit an 11th time, should fail with TooManyUnrevealedCommits
        let new_commit_data: Vec<u8> = vec![11; 5];
        let bounded_new_commit_data = new_commit_data
            .try_into()
            .expect("Failed to convert new commit data into bounded vector");

        assert_err!(
            SubtensorModule::do_commit_timelocked_weights(
                RuntimeOrigin::signed(hotkey1),
                netuid,
                bounded_new_commit_data,
                reveal_round,
                SubtensorModule::get_commit_reveal_weights_version()
            ),
            Error::<Test>::TooManyUnrevealedCommits
        );

        // Hotkey2 can still submit commits independently
        let commit_data_hotkey2: Vec<u8> = vec![0; 5];
        let bounded_commit_data_hotkey2 = commit_data_hotkey2
            .try_into()
            .expect("Failed to convert commit data into bounded vector");

        assert_ok!(SubtensorModule::do_commit_timelocked_weights(
            RuntimeOrigin::signed(hotkey2),
            netuid,
            bounded_commit_data_hotkey2,
            reveal_round,
            SubtensorModule::get_commit_reveal_weights_version()
        ));

        // Hotkey2 can submit up to 10 commits
        for i in 1..10 {
            let commit_data: Vec<u8> = vec![i as u8; 5];
            let bounded_commit_data = commit_data
                .try_into()
                .expect("Failed to convert commit data into bounded vector");

            assert_ok!(SubtensorModule::do_commit_timelocked_weights(
                RuntimeOrigin::signed(hotkey2),
                netuid,
                bounded_commit_data,
                reveal_round,
                SubtensorModule::get_commit_reveal_weights_version()
            ));
        }

        // Hotkey2 attempts to commit an 11th time, should fail
        let new_commit_data: Vec<u8> = vec![11; 5];
        let bounded_new_commit_data = new_commit_data
            .try_into()
            .expect("Failed to convert new commit data into bounded vector");

        assert_err!(
            SubtensorModule::do_commit_timelocked_weights(
                RuntimeOrigin::signed(hotkey2),
                netuid,
                bounded_new_commit_data,
                reveal_round,
                SubtensorModule::get_commit_reveal_weights_version()
            ),
            Error::<Test>::TooManyUnrevealedCommits
        );

        step_epochs(10, netuid);

        let new_commit_data: Vec<u8> = vec![11; 5];
        let bounded_new_commit_data = new_commit_data
            .try_into()
            .expect("Failed to convert new commit data into bounded vector");
        assert_ok!(SubtensorModule::do_commit_timelocked_weights(
            RuntimeOrigin::signed(hotkey1),
            netuid,
            bounded_new_commit_data,
            reveal_round,
            SubtensorModule::get_commit_reveal_weights_version()
        ));
    });
}
