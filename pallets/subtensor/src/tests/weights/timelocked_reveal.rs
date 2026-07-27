#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
//! Timelocked (CRv3) reveal failure modes and multi-commit processing.

use ark_serialize::CanonicalDeserialize;
use ark_serialize::CanonicalSerialize;
use frame_support::{assert_ok, dispatch::DispatchResult};
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

use crate::coinbase::reveal_commits::WeightsTlockPayload;
use crate::tests::mock::*;
use crate::*;

#[test]
fn test_reveal_crv3_commits_decryption_failure() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey: AccountId = U256::from(1);
        let reveal_round: u64 = 1000;

        add_network(netuid, 5, 0);
        register_ok_neuron(netuid, hotkey, U256::from(2), 100_000);
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);

        let commit_bytes: Vec<u8> = vec![0xff; 100];
        let bounded_commit_bytes = commit_bytes
            .clone()
            .try_into()
            .expect("Failed to convert commit bytes into bounded vector");

        assert_ok!(SubtensorModule::do_commit_timelocked_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            bounded_commit_bytes,
            reveal_round,
            SubtensorModule::get_commit_reveal_weights_version()
        ));

        step_epochs(1, netuid);

        pallet_drand::Pulses::<Test>::insert(
            reveal_round,
            Pulse {
                round: reveal_round,
                randomness: vec![0; 32]
                    .try_into()
                    .expect("Failed to convert randomness vector"),
                signature: vec![0; 128]
                    .try_into()
                    .expect("Failed to convert signature vector"),
            },
        );

        assert_ok!(SubtensorModule::reveal_crv3_commits_for_subnet(netuid));

        let neuron_uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey)
            .expect("Failed to get neuron UID for hotkey") as usize;
        let weights_matrix = SubtensorModule::get_weights(netuid.into());
        let weights = weights_matrix.get(neuron_uid).cloned().unwrap_or_default();
        assert!(weights.iter().all(|&w| w == I32F32::from_num(0)));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_reveal_crv3_commits_multiple_commits_some_fail_some_succeed --exact --show-output --nocapture
#[test]
fn test_reveal_crv3_commits_multiple_commits_some_fail_some_succeed() {
    new_test_ext(100).execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey1: AccountId = U256::from(1);
        let hotkey2: AccountId = U256::from(2);
        let reveal_round: u64 = 1000;

        add_network(netuid, 5, 0);
        register_ok_neuron(netuid, hotkey1, U256::from(3), 100_000);
        register_ok_neuron(netuid, hotkey2, U256::from(4), 100_000);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        assert_ok!(SubtensorModule::set_reveal_period(netuid, 1));
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);

        // Prepare a valid payload for hotkey1
        let neuron_uid1 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey1)
            .expect("Failed to get neuron UID for hotkey1");
        let version_key = SubtensorModule::get_weights_version_key(netuid);
        let valid_payload = WeightsTlockPayload {
            hotkey: hotkey1.encode(),
            values: vec![10],
            uids: vec![neuron_uid1],
            version_key,
        };
        let serialized_valid_payload = valid_payload.encode();

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

        let ct_valid = tle::<TinyBLS381, AESGCMStreamCipherProvider, ChaCha20Rng>(
            pub_key,
            esk,
            &serialized_valid_payload,
            identity.clone(),
            rng.clone(),
        )
        .expect("Encryption failed");

        let mut commit_bytes_valid = Vec::new();
        ct_valid
            .serialize_compressed(&mut commit_bytes_valid)
            .expect("Failed to serialize valid commit");

        // Prepare an invalid payload for hotkey2
        let invalid_payload = vec![0u8; 10]; // Invalid payload
        let ct_invalid = tle::<TinyBLS381, AESGCMStreamCipherProvider, ChaCha20Rng>(
            pub_key,
            esk,
            &invalid_payload,
            identity,
            rng,
        )
        .expect("Encryption failed");

        let mut commit_bytes_invalid = Vec::new();
        ct_invalid
            .serialize_compressed(&mut commit_bytes_invalid)
            .expect("Failed to serialize invalid commit");

        // Insert both commits
        assert_ok!(SubtensorModule::do_commit_timelocked_weights(
            RuntimeOrigin::signed(hotkey1),
            netuid,
            commit_bytes_valid.try_into().expect("Failed to convert valid commit data"),
            reveal_round,
            SubtensorModule::get_commit_reveal_weights_version()
        ));
        assert_ok!(SubtensorModule::do_commit_timelocked_weights(
            RuntimeOrigin::signed(hotkey2),
            netuid,
            commit_bytes_invalid.try_into().expect("Failed to convert invalid commit data"),
            reveal_round,
            SubtensorModule::get_commit_reveal_weights_version()
        ));

        // Insert the pulse
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

        step_epochs(1, netuid);

        // Verify that weights are set for hotkey1
        let neuron_uid1 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey1)
            .expect("Failed to get neuron UID for hotkey1") as usize;
        let weights_sparse = SubtensorModule::unnormalized_weights_sparse(netuid.into());
        let weights1 = weights_sparse.get(neuron_uid1).cloned().unwrap_or_default();
        assert!(
            !weights1.is_empty(),
            "Weights for neuron_uid1 should be set"
        );

        // Verify that weights are not set for hotkey2
        let neuron_uid2 = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey2)
            .expect("Failed to get neuron UID for hotkey2") as usize;
        let weights2 = weights_sparse.get(neuron_uid2).cloned().unwrap_or_default();
        assert!(
            weights2.is_empty(),
            "Weights for neuron_uid2 should be empty as commit could not be revealed"
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_reveal_crv3_commits_do_set_weights_failure --exact --show-output --nocapture
#[test]
fn test_reveal_crv3_commits_do_set_weights_failure() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey: AccountId = U256::from(1);
        let reveal_round: u64 = 1000;

        add_network(netuid, 5, 0);
        register_ok_neuron(netuid, hotkey, U256::from(2), 100_000);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        assert_ok!(SubtensorModule::set_reveal_period(netuid, 3));
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);

        // Prepare payload with mismatched uids and values lengths
        let version_key = SubtensorModule::get_weights_version_key(netuid);
        let payload = WeightsTlockPayload {
            hotkey: hotkey.encode(),
            values: vec![10, 20], // Length 2
            uids: vec![0],        // Length 1
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
            RuntimeOrigin::signed(hotkey),
            netuid,
            commit_bytes.try_into().expect("Failed to convert commit data into bounded vector"),
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

        step_epochs(3, netuid);

        // Verify that weights are not set due to `do_set_weights` failure
        let neuron_uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey)
            .expect("Failed to get neuron UID for hotkey") as usize;
        let weights_sparse = SubtensorModule::unnormalized_weights_sparse(netuid.into());
        let weights = weights_sparse.get(neuron_uid).cloned().unwrap_or_default();
        assert!(
            weights.is_empty(),
            "Weights for neuron_uid should be empty as do_set_weights should have failed"
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_reveal_crv3_commits_payload_decoding_failure --exact --show-output --nocapture
#[test]
fn test_reveal_crv3_commits_payload_decoding_failure() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey: AccountId = U256::from(1);
        let reveal_round: u64 = 1000;

        add_network(netuid, 5, 0);
        register_ok_neuron(netuid, hotkey, U256::from(2), 100_000);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        assert_ok!(SubtensorModule::set_reveal_period(netuid, 3));
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);

        let invalid_payload = vec![0u8; 10]; // Not a valid encoding of WeightsTlockPayload

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
            &invalid_payload,
            identity,
            rng,
        )
        .expect("Encryption failed");

        let mut commit_bytes = Vec::new();
        ct.serialize_compressed(&mut commit_bytes)
            .expect("Failed to serialize commit");

        assert_ok!(SubtensorModule::do_commit_timelocked_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            commit_bytes.try_into().expect("Failed to convert commit data into bounded vector"),
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

        step_epochs(3, netuid);

        // Verify that weights are not set
        let neuron_uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey)
            .expect("Failed to get neuron UID for hotkey") as usize;
        let weights_sparse = SubtensorModule::unnormalized_weights_sparse(netuid.into());
        let weights = weights_sparse.get(neuron_uid).cloned().unwrap_or_default();
        assert!(
            weights.is_empty(),
            "Weights for neuron_uid should be empty as the payload could not be decoded"
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_reveal_crv3_commits_signature_deserialization_failure --exact --show-output --nocapture
#[test]
fn test_reveal_crv3_commits_signature_deserialization_failure() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey: AccountId = U256::from(1);
        let reveal_round: u64 = 1000;

        add_network(netuid, 5, 0);
        register_ok_neuron(netuid, hotkey, U256::from(2), 100_000);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        assert_ok!(SubtensorModule::set_reveal_period(netuid, 3));
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);

        let version_key = SubtensorModule::get_weights_version_key(netuid);
        let payload = WeightsTlockPayload {
            hotkey: hotkey.encode(),
            values: vec![10, 20],
            uids: vec![0, 1],
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
            RuntimeOrigin::signed(hotkey),
            netuid,
            commit_bytes.try_into().expect("Failed to convert commit data into bounded vector"),
            reveal_round,
            SubtensorModule::get_commit_reveal_weights_version()
        ));

        pallet_drand::Pulses::<Test>::insert(
            reveal_round,
            Pulse {
                round: reveal_round,
                randomness: vec![0; 32].try_into().expect("Failed to convert randomness vector"),
                signature: vec![0; 10].try_into().expect("Failed to create invalid signature"), // Invalid signature length
            },
        );

        step_epochs(3, netuid);

        // Verify that weights are not set
        let neuron_uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey)
            .expect("Failed to get neuron UID for hotkey") as usize;
        let weights_sparse = SubtensorModule::unnormalized_weights_sparse(netuid.into());
        let weights = weights_sparse.get(neuron_uid).cloned().unwrap_or_default();
        assert!(
            weights.is_empty(),
            "Weights for neuron_uid should be empty as the signature could not be deserialized"
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_do_commit_crv3_weights_commit_size_exceeds_limit --exact --show-output --nocapture
#[test]
fn test_do_commit_crv3_weights_commit_size_exceeds_limit() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey: AccountId = U256::from(1);
        let reveal_round: u64 = 1000;

        add_network(netuid, 5, 0);
        register_ok_neuron(netuid, hotkey, U256::from(2), 100_000);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);

        let max_commit_size = MAX_CRV3_COMMIT_SIZE_BYTES as usize;
        let commit_data_exceeding: Vec<u8> = vec![0u8; max_commit_size + 1]; // Exceeds max size

        // Attempt to create a BoundedVec; this should fail
        let bounded_commit_data_result =
            BoundedVec::<u8, ConstU32<MAX_CRV3_COMMIT_SIZE_BYTES>>::try_from(
                commit_data_exceeding.clone(),
            );

        assert!(
            bounded_commit_data_result.is_err(),
            "Expected error when converting commit data exceeding max size into BoundedVec"
        );

        let commit_data_max_size: Vec<u8> = vec![0u8; max_commit_size]; // Exactly at max size
        let bounded_commit_data = BoundedVec::<u8, ConstU32<MAX_CRV3_COMMIT_SIZE_BYTES>>::try_from(
            commit_data_max_size.clone(),
        )
        .expect("Failed to create BoundedVec with data at max size");

        // Now call the function with valid data at max size
        assert_ok!(SubtensorModule::do_commit_timelocked_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            bounded_commit_data,
            reveal_round,
            SubtensorModule::get_commit_reveal_weights_version()
        ));
    });
}

//  SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_reveal_crv3_commits_with_empty_commit_queue --exact --show-output --nocapture
#[test]
fn test_reveal_crv3_commits_with_empty_commit_queue() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);

        add_network(netuid, 5, 0);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);

        step_epochs(2, netuid);

        let weights_sparse = SubtensorModule::unnormalized_weights_sparse(netuid.into());
        assert!(
            weights_sparse.is_empty(),
            "Weights should be empty as there were no commits to reveal"
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_reveal_crv3_commits_with_incorrect_identity_message --exact --show-output --nocapture
#[test]
fn test_reveal_crv3_commits_with_incorrect_identity_message() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey: AccountId = U256::from(1);
        let reveal_round: u64 = 1000;

        add_network(netuid, 5, 0);
        register_ok_neuron(netuid, hotkey, U256::from(2), 100_000);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        assert_ok!(SubtensorModule::set_reveal_period(netuid, 1));
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);

        // Prepare a valid payload but use incorrect identity message during encryption
        let neuron_uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey)
            .expect("Failed to get neuron UID for hotkey");
        let version_key = SubtensorModule::get_weights_version_key(netuid);
        let payload = WeightsTlockPayload {
            hotkey: hotkey.encode(),
            values: vec![10],
            uids: vec![neuron_uid],
            version_key,
        };
        let serialized_payload = payload.encode();

        let esk = [2; 32];
        let rng = ChaCha20Rng::seed_from_u64(0);

        let pk_bytes = hex::decode("83cf0f2896adee7eb8b5f01fcad3912212c437e0073e911fb90022d3e760183c8c4b450b6a0a6c3ac6a5776a2d1064510d1fec758c921cc22b0e17e63aaf4bcb5ed66304de9cf809bd274ca73bab4af5a6e9c76a4bc09e76eae8991ef5ece45a")
            .expect("Failed to decode public key bytes");
        let pub_key = <TinyBLS381 as EngineBLS>::PublicKeyGroup::deserialize_compressed(&*pk_bytes)
            .expect("Failed to deserialize public key");

        // Use incorrect message for identity (e.g., reveal_round + 1)
        let incorrect_message = {
            let mut hasher = sha2::Sha256::new();
            hasher.update((reveal_round + 1).to_be_bytes());
            hasher.finalize().to_vec()
        };
        let identity = Identity::new(b"", vec![incorrect_message]);

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
            RuntimeOrigin::signed(hotkey),
            netuid,
            commit_bytes.try_into().expect("Failed to convert commit data into bounded vector"),
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

        step_epochs(1, netuid);

        // Verify that weights are not set due to decryption failure
        let neuron_uid = neuron_uid as usize;
        let weights_sparse = SubtensorModule::unnormalized_weights_sparse(netuid.into());
        let weights = weights_sparse.get(neuron_uid).cloned().unwrap_or_default();
        assert!(
            weights.is_empty(),
            "Weights for neuron_uid should be empty due to incorrect identity message"
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_multiple_commits_by_same_hotkey_within_limit --exact --show-output --nocapture
#[test]
fn test_multiple_commits_by_same_hotkey_within_limit() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey: AccountId = U256::from(1);
        let reveal_round: u64 = 1000;

        add_network(netuid, 5, 0);
        register_ok_neuron(netuid, hotkey, U256::from(2), 100_000);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        assert_ok!(SubtensorModule::set_reveal_period(netuid, 1));
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);

        for i in 0..10 {
            let commit_data: Vec<u8> = vec![i; 5];
            assert_ok!(SubtensorModule::do_commit_timelocked_weights(
                RuntimeOrigin::signed(hotkey),
                netuid,
                commit_data
                    .try_into()
                    .expect("Failed to convert commit data into bounded vector"),
                reveal_round + i as u64,
                SubtensorModule::get_commit_reveal_weights_version()
            ));
        }

        let cur_epoch =
            SubtensorModule::get_epoch_index(netuid, SubtensorModule::get_current_block_as_u64());
        let commits =
            TimelockedWeightCommits::<Test>::get(NetUidStorageIndex::from(netuid), cur_epoch);
        assert_eq!(
            commits.len(),
            10,
            "Expected 10 commits stored for the hotkey"
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_reveal_crv3_commits_removes_past_epoch_commits --exact --show-output --nocapture
#[test]
fn test_reveal_crv3_commits_removes_past_epoch_commits() {
    new_test_ext(100).execute_with(|| {
        let netuid = NetUid::from(1);
        let hotkey: AccountId = U256::from(1);
        let reveal_round: u64 = 1_000;

        add_network(netuid, /*tempo*/ 5, 0);
        register_ok_neuron(netuid, hotkey, U256::from(2), 100_000);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        assert_ok!(SubtensorModule::set_reveal_period(netuid, 1)); // reveal_period = 1 epoch
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);

        // ---------------------------------------------------------------------
        // Put dummy commits into the two epochs immediately *before* current.
        // ---------------------------------------------------------------------
        // Establish a non-zero epoch counter and pin the scheduler so the reveal
        // pass sees exactly this epoch (no look-ahead increment).
        let cur_epoch: u64 = 10;
        SubnetEpochIndex::<Test>::insert(netuid, cur_epoch);
        LastEpochBlock::<Test>::insert(netuid, SubtensorModule::get_current_block_as_u64());
        PendingEpochAt::<Test>::insert(netuid, 0);
        let cur_block = SubtensorModule::get_current_block_as_u64();
        let past_epoch = cur_epoch.saturating_sub(2); // definitely < reveal_epoch
        let reveal_epoch = cur_epoch.saturating_sub(1); // == cur_epoch - reveal_period

        for &epoch in &[past_epoch, reveal_epoch] {
            let bounded_commit = vec![epoch as u8; 5].try_into().expect("bounded vec");

            assert_ok!(TimelockedWeightCommits::<Test>::try_mutate(
                NetUidStorageIndex::from(netuid),
                epoch,
                |q| -> DispatchResult {
                    q.push_back((hotkey, cur_block, bounded_commit, reveal_round));
                    Ok(())
                }
            ));
        }

        // Sanity – both epochs presently hold a commit.
        assert!(
            !TimelockedWeightCommits::<Test>::get(NetUidStorageIndex::from(netuid), past_epoch)
                .is_empty()
        );
        assert!(
            !TimelockedWeightCommits::<Test>::get(NetUidStorageIndex::from(netuid), reveal_epoch)
                .is_empty()
        );

        // ---------------------------------------------------------------------
        // Run the reveal pass WITHOUT a pulse – only expiry housekeeping runs.
        // ---------------------------------------------------------------------
        assert_ok!(SubtensorModule::reveal_crv3_commits_for_subnet(netuid));

        // past_epoch (< reveal_epoch) must be gone
        assert!(
            TimelockedWeightCommits::<Test>::get(NetUidStorageIndex::from(netuid), past_epoch)
                .is_empty(),
            "expired epoch {past_epoch} should be cleared"
        );

        // reveal_epoch queue is *kept* because its commit could still be revealed later.
        assert!(
            !TimelockedWeightCommits::<Test>::get(NetUidStorageIndex::from(netuid), reveal_epoch)
                .is_empty(),
            "reveal-epoch {reveal_epoch} must be retained until commit can be revealed"
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_reveal_crv3_commits_multiple_valid_commits_all_processed --exact --show-output --nocapture
#[test]
fn test_reveal_crv3_commits_multiple_valid_commits_all_processed() {
    new_test_ext(100).execute_with(|| {
        let netuid = NetUid::from(1);
        let reveal_round: u64 = 1_000;

        // ───── network parameters ───────────────────────────────────────────
        add_network(netuid, 5, 0);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        assert_ok!(SubtensorModule::set_reveal_period(netuid, 1));
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);
        SubtensorModule::set_stake_threshold(0);
        SubtensorModule::set_max_registrations_per_block(netuid, 100);
        SubtensorModule::set_target_registrations_per_interval(netuid, 100);

        // Insert the pulse
        let sig_bytes = hex::decode("b44679b9a59af2ec876b1a6b1ad52ea9b1615fc3982b19576350f93447cb1125e342b73a8dd2bacbe47e4b6b63ed5e39")
            .expect("Failed to decode signature bytes");

        // pulse for round 1000
        // let sig_bytes = hex::decode(
        //     "b44679b9a59af2ec876b1a6b1ad52ea9b1615fc3982b19576350f93447cb1125e\
        //      342b73a8dd2bacbe47e4b6b63ed5e39",
        // )
        // .unwrap();
        pallet_drand::Pulses::<Test>::insert(
            reveal_round,
            Pulse {
                round: reveal_round,
                randomness: vec![0; 32].try_into().unwrap(),
                signature: sig_bytes.try_into().unwrap(),
            },
        );

        // ───── five neurons (hotkeys 1‑5) ───────────────────────────────────
        let hotkeys: Vec<_> = (1..=5).map(U256::from).collect();
        for (i, hk) in hotkeys.iter().enumerate() {
            let cold: AccountId = U256::from(i + 100);

            register_ok_neuron(netuid, *hk, cold, 100_000);
            SubtensorModule::set_validator_permit_for_uid(netuid, i as u16, true);

            // add minimal stake so `do_set_weights` will succeed
            add_balance_to_coldkey_account(&cold, 1.into());
            SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                hk,
                &cold,
                netuid,
                1.into(),
            );

            step_block(1); // avoids TooManyRegistrationsThisBlock
        }


        // ───── create & submit commits for each hotkey ──────────────────────
        let esk = [2u8; 32];
        let pk_bytes = hex::decode(
            "83cf0f2896adee7eb8b5f01fcad3912212c437e0073e911fb90022d3e760183c\
             8c4b450b6a0a6c3ac6a5776a2d1064510d1fec758c921cc22b0e17e63aaf4bcb\
             5ed66304de9cf809bd274ca73bab4af5a6e9c76a4bc09e76eae8991ef5ece45a",
        )
        .unwrap();
        let pk =
            <TinyBLS381 as EngineBLS>::PublicKeyGroup::deserialize_compressed(&*pk_bytes).unwrap();

        for (i, hk) in hotkeys.iter().enumerate() {
            let payload = WeightsTlockPayload {
                hotkey: hk.encode(),
                values: vec![10, 20, 30, 40, 50],
                uids: (0..5).map(|u| u as u16).collect(),
                version_key: SubtensorModule::get_weights_version_key(netuid),
            };

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
                ChaCha20Rng::seed_from_u64(i as u64),
            )
            .unwrap();

            let mut commit_bytes = Vec::new();
            ct.serialize_compressed(&mut commit_bytes).unwrap();

            assert_ok!(SubtensorModule::do_commit_timelocked_weights(
                RuntimeOrigin::signed(*hk),
                netuid,
                commit_bytes.try_into().unwrap(),
                reveal_round,
                SubtensorModule::get_commit_reveal_weights_version()
            ));
        }

        // advance reveal_period + 1 epochs → 2 epochs
        step_epochs(2, netuid);

        // ───── assertions ───────────────────────────────────────────────────
        let w_sparse = SubtensorModule::unnormalized_weights_sparse(netuid.into());
        for hk in hotkeys {
            let uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hk).unwrap() as usize;
            assert!(
                !w_sparse.get(uid).unwrap_or(&Vec::new()).is_empty(),
                "weights for uid {uid} should be set"
            );
        }
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::weights::test_reveal_crv3_commits_max_neurons --exact --show-output --nocapture
#[test]
fn test_reveal_crv3_commits_max_neurons() {
    new_test_ext(100).execute_with(|| {
        let netuid = NetUid::from(1);
        let reveal_round: u64 = 1_000;

        // ───── network parameters ───────────────────────────────────────────
        add_network(netuid, 5, 0);
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        assert_ok!(SubtensorModule::set_reveal_period(netuid, 1));
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);
        SubtensorModule::set_stake_threshold(0);
        SubtensorModule::set_max_registrations_per_block(netuid, 10_000);
        SubtensorModule::set_target_registrations_per_interval(netuid, 10_000);
        SubtensorModule::set_max_allowed_uids(netuid, 10_024);

        // ───── register 1 024 neurons ───────────────────────────────────────
        for i in 0..1_024u16 {
            let hk: AccountId = U256::from(i as u64 + 1);
            let cold: AccountId = U256::from(i as u64 + 10_000);

            register_ok_neuron(netuid, hk, cold, 100_000);
            SubtensorModule::set_validator_permit_for_uid(netuid, i, true);

            // give each neuron a nominal stake (safe even if not needed)
            add_balance_to_coldkey_account(&cold, 1.into());
            SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &hk,
                &cold,
                netuid,
                1.into(),
            );

            step_block(1); // avoid registration‑limit panic
        }

        // ───── pulse for round 1000 ─────────────────────────────────────────
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

        // ───── three committing hotkeys ─────────────────────────────────────
        let esk = [2u8; 32];
        let pk_bytes = hex::decode(
            "83cf0f2896adee7eb8b5f01fcad3912212c437e0073e911fb90022d3e760183c\
             8c4b450b6a0a6c3ac6a5776a2d1064510d1fec758c921cc22b0e17e63aaf4bcb\
             5ed66304de9cf809bd274ca73bab4af5a6e9c76a4bc09e76eae8991ef5ece45a",
        )
        .unwrap();
        let pk =
            <TinyBLS381 as EngineBLS>::PublicKeyGroup::deserialize_compressed(&*pk_bytes).unwrap();
        let committing_hotkeys = [U256::from(1), U256::from(2), U256::from(3)];
        let mut commits = Vec::new();
        for (i, hk) in committing_hotkeys.iter().enumerate() {
            let payload = WeightsTlockPayload {
                hotkey: hk.encode(),
                values: vec![10u16; 1_024],
                uids: (0..1_024).collect(),
                version_key: SubtensorModule::get_weights_version_key(netuid),
            };
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
                ChaCha20Rng::seed_from_u64(i as u64),
            )
            .unwrap();
            let mut commit_bytes = Vec::new();
            ct.serialize_compressed(&mut commit_bytes).unwrap();
            // Submit the commit
            assert_ok!(SubtensorModule::do_commit_timelocked_weights(
                RuntimeOrigin::signed(*hk),
                netuid,
                commit_bytes
                    .try_into()
                    .expect("Failed to convert commit data"),
                reveal_round,
                SubtensorModule::get_commit_reveal_weights_version()
            ));

            // Store the expected weights for later comparison
            commits.push((hk, payload));
        }
        // ───── advance reveal_period + 1 epochs ─────────────────────────────
        step_epochs(2, netuid);

        // ───── verify weights ───────────────────────────────────────────────
        let w_sparse = SubtensorModule::unnormalized_weights_sparse(netuid.into());
        for hk in &committing_hotkeys {
            let uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, hk).unwrap() as usize;
            assert!(
                !w_sparse.get(uid).unwrap_or(&Vec::new()).is_empty(),
                "weights for uid {uid} should be set"
            );
        }
    });
}
