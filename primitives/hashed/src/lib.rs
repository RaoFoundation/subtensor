//! Fixed, versioned wire format for a hashed account's rotating authorization.
//!
//! Hashing hides an unused signing public key. It does not make sr25519
//! post-quantum secure after the key is disclosed, including in the mempool.
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use scale_info::TypeInfo;

pub const VERSION: u8 = 1;
pub const ACCOUNT_DOMAIN: &[u8] = b"bittensor/hashed/v1/account";
pub const KEY_DOMAIN: &[u8] = b"bittensor/hashed/v1/key";
pub const TRANSACTION_DOMAIN: &[u8] = b"bittensor/hashed/v1/transaction";

/// The underlying signature scheme, separate from the hashed-account wrapper.
/// New schemes require an explicit protocol addition; unknown tags fail decoding.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Encode,
    Decode,
    DecodeWithMemTracking,
    TypeInfo,
    MaxEncodedLen,
)]
pub enum Scheme {
    #[codec(index = 1)]
    Sr25519,
    #[codec(index = 2)]
    MlDsa65,
}

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Encode,
    Decode,
    DecodeWithMemTracking,
    TypeInfo,
    MaxEncodedLen,
)]
pub struct Descriptor {
    pub version: u8,
    pub scheme: Scheme,
    pub initial_commitment: [u8; 32],
}

impl Descriptor {
    pub fn is_supported(&self) -> bool {
        self.version == VERSION && self.initial_commitment != [0; 32]
    }
}

/// V1 sr25519 proof. Future key families require their own bounded proof format.
#[derive(
    Clone, Debug, PartialEq, Eq, Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen,
)]
pub struct AuthorizationProof<const PUBLIC: usize, const SIGNATURE: usize> {
    pub generation: u64,
    pub public_key: [u8; PUBLIC],
    pub next_commitment: [u8; 32],
    pub signature: [u8; SIGNATURE],
}

fn domain_hash(domain: &[u8], value: impl Encode) -> [u8; 32] {
    let mut bytes = domain.to_vec();
    value.encode_to(&mut bytes);
    sp_core::hashing::blake2_256(&bytes)
}

pub fn account_id(descriptor: &Descriptor) -> [u8; 32] {
    domain_hash(ACCOUNT_DOMAIN, descriptor)
}

pub fn key_commitment<const N: usize>(scheme: Scheme, public_key: &[u8; N]) -> [u8; 32] {
    domain_hash(KEY_DOMAIN, (VERSION, scheme, public_key))
}

/// Hash the complete transaction-extension implication. The caller must include
/// the call, nonce, era, genesis, runtime version and remaining signed extensions
/// in `implication_bytes`; none may be omitted or inferred from mutable state.
/// The returned 32 bytes are signed directly with the underlying key.
pub fn transaction_payload(
    account: &[u8; 32],
    scheme: Scheme,
    generation: u64,
    next_commitment: &[u8; 32],
    implication_bytes: &[u8],
) -> [u8; 32] {
    domain_hash(
        TRANSACTION_DOMAIN,
        (
            account,
            scheme,
            generation,
            next_commitment,
            sp_core::hashing::blake2_256(implication_bytes),
        ),
    )
}

pub const MLDSA_PUBLIC_LEN: usize = 1952;
pub const MLDSA_SIGNATURE_LEN: usize = 3309;
pub type Proof = AuthorizationProof<32, 64>;
pub type MlDsaProof = AuthorizationProof<MLDSA_PUBLIC_LEN, MLDSA_SIGNATURE_LEN>;
pub use fips204;

/// The context is part of the ML-DSA signature and cannot be repurposed for
/// off-chain messages even when both uses involve the same generation key.
pub const MLDSA_TRANSACTION_CONTEXT: &[u8] = b"bittensor/hashed/v1/transaction";

#[cfg(feature = "verify")]
pub fn verify<const P: usize, const S: usize>(
    account: &[u8; 32],
    scheme: Scheme,
    proof: &AuthorizationProof<P, S>,
    implication_bytes: &[u8],
) -> bool {
    let payload = transaction_payload(
        account,
        scheme,
        proof.generation,
        &proof.next_commitment,
        implication_bytes,
    );
    match scheme {
        Scheme::Sr25519 => {
            let (Ok(signature), Ok(public)) = (
                proof.signature.as_slice().try_into(),
                proof.public_key.as_slice().try_into(),
            ) else {
                return false;
            };
            sp_io::crypto::sr25519_verify(
                &sp_core::sr25519::Signature::from_raw(signature),
                &payload,
                &sp_core::sr25519::Public::from_raw(public),
            )
        }
        Scheme::MlDsa65 => verify_mldsa(
            &proof.public_key,
            &proof.signature,
            &payload,
            MLDSA_TRANSACTION_CONTEXT,
        ),
    }
}

pub fn verify_mldsa(public: &[u8], signature: &[u8], message: &[u8], context: &[u8]) -> bool {
    use fips204::{
        ml_dsa_65::PublicKey,
        traits::{SerDes, Verifier},
    };
    let (Ok(public), Ok(signature)) = (public.try_into(), signature.try_into()) else {
        return false;
    };
    PublicKey::try_from_bytes(public).is_ok_and(|key| key.verify(message, signature, context))
}

#[cfg(all(test, feature = "verify"))]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;
    use sp_core::Pair;

    #[test]
    fn nist_external_mldsa65_vectors_and_context_binding() {
        let vectors: serde_json::Value =
            serde_json::from_str(include_str!("../tests/data/nist-mldsa65.json")).unwrap();
        for case in vectors["tests"].as_array().unwrap() {
            let bytes = |name: &str| hex::decode(case[name].as_str().unwrap()).unwrap();
            let public = bytes("pk");
            let signature = bytes("signature");
            let message = bytes("message");
            let context = bytes("context");
            assert_eq!(
                verify_mldsa(&public, &signature, &message, &context),
                case["testPassed"].as_bool().unwrap(),
                "NIST case {}",
                case["tcId"]
            );
            assert!(!verify_mldsa(
                &public,
                &signature,
                &message,
                b"different context"
            ));
            assert!(!verify_mldsa(
                &public[..1951],
                &signature,
                &message,
                &context
            ));
            assert!(!verify_mldsa(
                &public,
                &signature[..3308],
                &message,
                &context
            ));
        }
    }

    #[test]
    fn unknown_schemes_and_versions_are_rejected() {
        assert!(Scheme::decode(&mut &[0u8][..]).is_err());
        assert!(Scheme::decode(&mut &[3u8][..]).is_err());
        assert!(
            !Descriptor {
                version: 2,
                scheme: Scheme::Sr25519,
                initial_commitment: [1; 32]
            }
            .is_supported()
        );
    }

    #[test]
    fn signature_binds_every_authorization_field() {
        let pair = sp_core::sr25519::Pair::from_seed(&[7; 32]);
        let scheme = Scheme::Sr25519;
        let account = account_id(&Descriptor {
            version: VERSION,
            scheme,
            initial_commitment: key_commitment(scheme, &pair.public().0),
        });
        let next = [9; 32];
        let payload = transaction_payload(&account, scheme, 3, &next, b"complete implication");
        let proof = Proof {
            generation: 3,
            public_key: pair.public().0,
            next_commitment: next,
            signature: pair.sign(&payload).0,
        };
        assert!(verify(&account, scheme, &proof, b"complete implication"));
        assert!(!verify(&[0; 32], scheme, &proof, b"complete implication"));
        assert!(!verify(
            &account,
            scheme,
            &proof,
            b"different call or nonce"
        ));
        let mut changed = proof.clone();
        changed.generation = 4;
        assert!(!verify(&account, scheme, &changed, b"complete implication"));
        changed = proof.clone();
        changed.next_commitment = [8; 32];
        assert!(!verify(&account, scheme, &changed, b"complete implication"));
        changed = proof;
        changed.public_key = [5; 32];
        assert!(!verify(&account, scheme, &changed, b"complete implication"));
    }

    #[test]
    fn domain_separation_and_wire_layout_are_fixed() {
        let descriptor = Descriptor {
            version: VERSION,
            scheme: Scheme::Sr25519,
            initial_commitment: [7; 32],
        };
        let mut encoded = alloc::vec![1, 1];
        encoded.extend_from_slice(&[7; 32]);
        assert_eq!(descriptor.encode(), encoded);
        assert_ne!(
            account_id(&descriptor),
            key_commitment(Scheme::Sr25519, &[7; 32])
        );
        let mut canonical = ACCOUNT_DOMAIN.to_vec();
        canonical.extend_from_slice(&encoded);
        assert_eq!(
            account_id(&descriptor),
            sp_core::hashing::blake2_256(&canonical)
        );
    }
}
