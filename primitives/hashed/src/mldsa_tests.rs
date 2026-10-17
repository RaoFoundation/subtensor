#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use super::*;
use alloc::{vec, vec::Vec};
use serde_json::Value;

fn header(seed: &[u8; 32]) -> Header {
    let public_key = public_key_from_seed(seed).unwrap();
    let descriptor = Descriptor {
        version: VERSION,
        algorithm: ALGORITHM,
        initial_commitment: commitment(&public_key),
    };
    Header {
        account: account_id(&descriptor),
        version: VERSION,
        algorithm: ALGORITHM,
        generation: 0,
        public_key,
        next_commitment: commitment(&public_key_from_seed(&[22; 32]).unwrap()),
    }
}

#[test]
fn exact_wire_layout_and_strict_version_dispatch() {
    let header = header(&[11; 32]);
    let signed = header.sign(&[11; 32], b"payload", &[0; 32]).unwrap();
    let encoded = signed.encode();
    assert_eq!(Header::ENCODED_LEN, header.encode().len());
    assert_eq!(SignerAddress::ENCODED_LEN, encoded.len());
    assert_eq!(&encoded[..32], &header.account);
    assert_eq!(&encoded[32..34], &[VERSION, ALGORITHM]);
    assert_eq!(&encoded[34..42], &header.generation.to_le_bytes());
    assert_eq!(&encoded[42..1994], &header.public_key);
    assert_eq!(&encoded[1994..2058], &header.next_commitment);
    assert_eq!(&encoded[2058..], &signed.signature);
    assert_eq!(SignerAddress::from_bytes(&encoded).unwrap(), signed);
    assert!(SignerAddress::from_bytes(&encoded[..5366]).is_err());
    let mut overlong = encoded.clone();
    overlong.push(0);
    assert!(SignerAddress::from_bytes(&overlong).is_err());
    for (offset, value) in [(32, 1), (32, 3), (33, 1), (33, 3), (33, 255)] {
        let mut bad = encoded.clone();
        bad[offset] = value;
        assert!(SignerAddress::from_bytes(&bad).is_err());
    }
    assert_eq!(codec::Compact(5367u32).encode(), [0xdd, 0x53]);
    assert!(CARRIER_SIGNATURE.iter().all(|byte| *byte == 0x4d));
    assert_eq!(SignerAddress::max_encoded_len(), 5367);
    assert_eq!(Header::max_encoded_len(), 2058);
    assert_eq!(Descriptor::max_encoded_len(), 66);
}

#[test]
fn descriptors_have_separate_domains_and_no_implicit_version_upgrade() {
    let descriptor = Descriptor {
        version: VERSION,
        algorithm: ALGORITHM,
        initial_commitment: [7; 64],
    };
    assert_eq!(descriptor.encode().len(), Descriptor::ENCODED_LEN);
    assert_eq!(
        Descriptor::from_bytes(&descriptor.encode()).unwrap(),
        descriptor
    );
    let v1 = crate::Descriptor {
        version: 1,
        scheme: crate::Scheme::Ed25519,
        initial_commitment: [7; 32],
    };
    assert!(Descriptor::from_bytes(&v1.encode()).is_err());
    for (version, algorithm, commitment) in [(1, 4, [7; 64]), (2, 3, [7; 64]), (2, 4, [0; 64])] {
        let invalid = Descriptor {
            version,
            algorithm,
            initial_commitment: commitment,
        };
        assert!(!invalid.is_supported());
        assert!(Descriptor::from_bytes(&invalid.encode()).is_err());
    }
    let mut trailing = descriptor.encode();
    trailing.push(0);
    assert!(Descriptor::from_bytes(&trailing).is_err());
    assert_ne!(account_id(&descriptor), crate::account_id(&v1));
}

#[test]
fn pure_external_and_streamed_full_transcripts_agree() {
    let seed = [11; 32];
    let header = header(&seed);
    let payload = (vec![23u8; 1024], 17u64, [33u8; 32]);
    let payload_bytes = payload.encode();
    let signed = header.sign(&seed, &payload_bytes, &[44; 32]).unwrap();
    assert!(signed.verify(&payload_bytes));
    assert!(signed.verify_encoded(&payload));
    let mut transcript = TRANSACTION_DOMAIN.to_vec();
    header.encode_to(&mut transcript);
    transcript.extend_from_slice(&payload_bytes);
    let public = ml_dsa_65::PublicKey::try_from_bytes(header.public_key).unwrap();
    assert!(public.verify(&transcript, &signed.signature, TRANSACTION_CONTEXT));
    assert!(!public.verify(&transcript, &signed.signature, b"bittensor/ms/v2/order"));
    assert!(!signed.verify(&sp_core::hashing::blake2_256(&payload_bytes)));
    // Hedged and deterministic signatures share exactly the same verifier.
    let deterministic = header.sign(&seed, &payload_bytes, &[0; 32]).unwrap();
    assert!(deterministic.verify(&payload_bytes));
    assert_ne!(signed.signature, deterministic.signature);
    assert!(header.sign(&[99; 32], &payload_bytes, &[0; 32]).is_err());
}

#[test]
fn every_header_field_and_full_payload_are_authenticated() {
    let signed = header(&[11; 32])
        .sign(&[11; 32], &[42; 300], &[0; 32])
        .unwrap();
    for offset in [0, 31, 32, 33, 34, 41, 42, 1993, 1994, 2057, 2058, 5366] {
        let mut bytes = signed.encode();
        bytes[offset] ^= 1;
        if let Ok(mutated) = SignerAddress::from_bytes(&bytes) {
            assert!(!mutated.verify(&[42; 300]), "mutation at {offset}");
        }
    }
    let mut changed = [42u8; 300];
    changed[299] ^= 1;
    assert!(!signed.verify(&changed));
    assert!(!signed.verify(&[42; 299]));
}

#[test]
fn message_contexts_and_lengths_are_checked() {
    let seed = [11; 32];
    let public_key = public_key_from_seed(&seed).unwrap();
    let signature = sign_message(&seed, b"message", b"order", &[0; 32]).unwrap();
    assert!(verify_message(
        &public_key,
        b"message",
        b"order",
        &signature
    ));
    assert!(!verify_message(
        &public_key,
        b"message",
        b"message",
        &signature
    ));
    assert!(!verify_message(
        &public_key,
        b"changed",
        b"order",
        &signature
    ));
    assert!(!verify_message(
        &public_key,
        b"message",
        &[0; 256],
        &signature
    ));
    assert!(sign_message(&seed, b"message", &[0; 256], &[0; 32]).is_err());
}

#[test]
fn streaming_size_limit_fails_closed_without_payload_allocation() {
    struct Oversized;
    impl Encode for Oversized {
        fn encode_to<T: Output + ?Sized>(&self, dest: &mut T) {
            for _ in 0..=(MAX_PAYLOAD_LEN / 4096) {
                dest.write(&[0; 4096]);
            }
        }
    }
    assert!(header(&[11; 32]).representative(&Oversized).is_none());
}

fn decode_hex<const N: usize>(value: &Value) -> [u8; N] {
    hex::decode(value.as_str().unwrap())
        .unwrap()
        .try_into()
        .unwrap()
}

#[test]
#[allow(deprecated)]
fn pinned_nist_keygen_sign_and_positive_negative_verification_vectors() {
    let vectors: Value =
        serde_json::from_str(include_str!("../tests/fixtures/nist-mldsa65.json")).unwrap();
    for group in vectors["keygen"].as_array().unwrap() {
        for test in group["tests"].as_array().unwrap() {
            let (public, secret) = ml_dsa_65::KG::keygen_from_seed(&decode_hex(&test["seed"]));
            assert_eq!(public.into_bytes(), decode_hex::<1952>(&test["pk"]));
            assert_eq!(secret.into_bytes(), decode_hex::<4032>(&test["sk"]));
        }
    }
    for group in vectors["siggen"].as_array().unwrap() {
        for test in group["tests"].as_array().unwrap() {
            let secret = ml_dsa_65::PrivateKey::try_from_bytes(decode_hex(&test["sk"])).unwrap();
            let message = hex::decode(test["message"].as_str().unwrap()).unwrap();
            let randomness = if group["deterministic"] == true {
                [0; 32]
            } else {
                decode_hex(&test["rnd"])
            };
            let signature = ml_dsa_65::_internal_sign(&secret, &message, &[], randomness).unwrap();
            assert_eq!(signature, decode_hex::<3309>(&test["signature"]));
        }
    }
    let mut cases = 0;
    let mut positive = 0;
    for group in vectors["sigver"].as_array().unwrap() {
        let public = ml_dsa_65::PublicKey::try_from_bytes(decode_hex(&group["pk"])).unwrap();
        for test in group["tests"].as_array().unwrap() {
            let message: Vec<_> = hex::decode(test["message"].as_str().unwrap()).unwrap();
            let expected = test["testPassed"].as_bool().unwrap();
            assert_eq!(
                ml_dsa_65::_internal_verify(
                    &public,
                    &message,
                    &decode_hex(&test["signature"]),
                    &[]
                ),
                expected,
                "NIST case {}",
                test["tcId"]
            );
            cases += 1;
            positive += usize::from(expected);
        }
    }
    assert_eq!(cases, 15);
    assert!(positive > 0 && positive < cases);
}
