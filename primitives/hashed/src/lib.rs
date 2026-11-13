//! Fixed, versioned wire format for a hashed account's rotating authorization.
//!
//! Hashing hides an unused signing public key. It does not make sr25519
//! post-quantum secure after the key is disclosed, including in the mempool.
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

/// Original Falcon padded signatures with immutable account profiles.
pub mod falcon;
/// Quantum-resistant account protocol, independent of the immutable v1 format.
pub mod mldsa;

/// Registration-free composite address derivation.
pub mod safe;

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use scale_info::TypeInfo;

pub const VERSION: u8 = 1;
/// Classical authorization using the exact `<Bytes>...</Bytes>` message wrapper.
/// This is a signer mode only; permanent account descriptors retain version 1.
pub const WRAPPED_SIGNER_VERSION: u8 = 0x81;
pub const ACCOUNT_DOMAIN: &[u8] = b"bittensor/hashed/v1/account";
pub const KEY_DOMAIN: &[u8] = b"bittensor/hashed/v1/key";
pub const TRANSACTION_DOMAIN: &[u8] = b"bittensor/hashed/v1/transaction";

/// The single message verified for a classical account authorization.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SigningMessage {
    Raw([u8; 32]),
    Wrapped([u8; 47]),
}

impl AsRef<[u8]> for SigningMessage {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::Raw(message) => message,
            Self::Wrapped(message) => message,
        }
    }
}

/// Fixed-size authorization address carried by standard MultiAddress::Raw.
/// The call and signed-extension encodings remain unchanged.
#[derive(
    Clone, Debug, PartialEq, Eq, Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen,
)]
pub struct SignerAddress {
    // Keep the permanent AccountId first: existing explorer account displays
    // decode a Raw signer as AccountId32 and use this prefix.
    pub account: [u8; 32],
    pub version: u8,
    pub scheme: Scheme,
    pub generation: u64,
    pub public_key: [u8; 32],
    pub next_commitment: [u8; 32],
}

impl SignerAddress {
    pub const ENCODED_LEN: usize = 106;

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, codec::Error> {
        if bytes.len() != Self::ENCODED_LEN {
            return Err("Invalid hashed signer address length".into());
        }
        let address = Self::decode(&mut &bytes[..])?;
        if !matches!(address.version, VERSION | WRAPPED_SIGNER_VERSION) {
            return Err("Unsupported hashed signer address version".into());
        }
        Ok(address)
    }

    /// Bind every address field to the complete, unshortened Substrate payload
    /// (call, encoded extensions and implicit data, including genesis and era).
    /// Always hash with a dedicated domain; an ordinary signature is never an
    /// authorization to rotate a permanent account.
    pub fn signing_hash(&self, payload: &[u8]) -> [u8; 32] {
        let mut bytes = TRANSACTION_DOMAIN.to_vec();
        self.encode_to(&mut bytes);
        bytes.extend_from_slice(payload);
        sp_core::hashing::blake2_256(&bytes)
    }

    /// Select exactly one verification message without trying alternate modes.
    /// The mode is bound by `signing_hash`, which includes the signer version.
    /// Wrapped mode encloses the binary hash, not the payload or its hex text.
    pub fn signing_message(&self, payload: &[u8]) -> Result<SigningMessage, codec::Error> {
        match self.version {
            VERSION => Ok(SigningMessage::Raw(self.signing_hash(payload))),
            WRAPPED_SIGNER_VERSION => {
                let mut message = [0u8; 47];
                let (prefix, rest) = message.split_at_mut(7);
                let (hash, suffix) = rest.split_at_mut(32);
                prefix.copy_from_slice(b"<Bytes>");
                hash.copy_from_slice(&self.signing_hash(payload));
                suffix.copy_from_slice(b"</Bytes>");
                Ok(SigningMessage::Wrapped(message))
            }
            _ => Err("Unsupported hashed signer address version".into()),
        }
    }
}

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
    #[codec(index = 3)]
    Ed25519,
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
    /// Version 1 rotates the selected classical signature scheme.
    pub fn rotates(&self) -> bool {
        self.version == VERSION
    }

    pub fn is_supported(&self) -> bool {
        (self.version == VERSION) && self.initial_commitment != [0; 32]
    }
}

/// Registration API for every supported account authentication scheme.
///
/// This enum selects the registration policy only. Its discriminant is never
/// included in either protocol's permanent account ID or receiving descriptor.
/// Both schemes use the original `register` and `check_registered` dispatchables.
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
pub enum RegistrationDescriptor {
    #[codec(index = 0)]
    Classical(Descriptor),
    #[codec(index = 1)]
    MlDsa65(mldsa::Descriptor),
    #[codec(index = 2)]
    Falcon(mldsa::Descriptor),
}

impl From<Descriptor> for RegistrationDescriptor {
    fn from(descriptor: Descriptor) -> Self {
        Self::Classical(descriptor)
    }
}

impl From<mldsa::Descriptor> for RegistrationDescriptor {
    fn from(descriptor: mldsa::Descriptor) -> Self {
        if falcon::is_account_profile(descriptor.algorithm) {
            Self::Falcon(descriptor)
        } else {
            Self::MlDsa65(descriptor)
        }
    }
}

/// Bounded internal Sr25519/Ed25519 signing result; not an extrinsic format.
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

/// Internal SDK proof data, split into a standard v4 signature and raw signer address.
pub type Proof = AuthorizationProof<32, 64>;

#[cfg(test)]
mod wrapped_signer_tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn signer(version: u8) -> SignerAddress {
        SignerAddress {
            account: [8; 32],
            version,
            scheme: Scheme::Ed25519,
            generation: 9,
            public_key: [10; 32],
            next_commitment: [11; 32],
        }
    }

    #[test]
    fn signer_modes_accept_only_raw_and_wrapped_versions() {
        for version in u8::MIN..=u8::MAX {
            let address = signer(version);
            let supported = matches!(version, VERSION | WRAPPED_SIGNER_VERSION);
            assert_eq!(
                SignerAddress::from_bytes(&address.encode()).is_ok(),
                supported
            );
            assert_eq!(address.signing_message(b"payload").is_ok(), supported);
        }
        for version in [VERSION, WRAPPED_SIGNER_VERSION] {
            let mut bytes = signer(version).encode();
            bytes.push(0);
            assert!(SignerAddress::from_bytes(&bytes).is_err());
            bytes.truncate(SignerAddress::ENCODED_LEN - 1);
            assert!(SignerAddress::from_bytes(&bytes).is_err());
        }
    }

    #[test]
    fn wrapped_message_binds_mode_and_encloses_the_binary_domain_hash() {
        let payload = b"complete transaction payload";
        let raw = signer(VERSION);
        let wrapped = signer(WRAPPED_SIGNER_VERSION);
        let mut canonical = TRANSACTION_DOMAIN.to_vec();
        canonical.extend_from_slice(&raw.encode());
        canonical.extend_from_slice(payload);
        let raw_hash = sp_core::hashing::blake2_256(&canonical);
        assert_eq!(raw.signing_hash(payload), raw_hash);
        assert_eq!(
            raw.signing_message(payload).unwrap(),
            SigningMessage::Raw(raw_hash)
        );

        let wrapped_hash = wrapped.signing_hash(payload);
        assert_ne!(raw_hash, wrapped_hash);
        let mut expected = b"<Bytes>".to_vec();
        expected.extend_from_slice(&wrapped_hash);
        expected.extend_from_slice(b"</Bytes>");
        let message = wrapped.signing_message(payload).unwrap();
        assert!(matches!(message, SigningMessage::Wrapped(_)));
        assert_eq!(message.as_ref(), expected);
        assert_eq!(message.as_ref().len(), 47);
        assert_ne!(
            message,
            wrapped.signing_message(b"different payload").unwrap()
        );

        // The mode is not a registration profile and never changes an account ID.
        let descriptor = Descriptor {
            version: VERSION,
            scheme: wrapped.scheme,
            initial_commitment: key_commitment(wrapped.scheme, &wrapped.public_key),
        };
        assert!(descriptor.is_supported());
        assert!(
            !Descriptor {
                version: WRAPPED_SIGNER_VERSION,
                ..descriptor
            }
            .is_supported()
        );
    }
}

#[cfg(all(test, feature = "verify"))]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;

    #[test]
    fn unknown_schemes_and_versions_are_rejected() {
        assert!(Scheme::decode(&mut &[0u8][..]).is_err());
        assert!(Scheme::decode(&mut &[2u8][..]).is_err());
        assert!(Scheme::decode(&mut &[4u8][..]).is_err());
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
    fn domain_separation_and_wire_layout_are_fixed() {
        let signer = SignerAddress {
            account: [8; 32],
            version: VERSION,
            scheme: Scheme::Ed25519,
            generation: 9,
            public_key: [10; 32],
            next_commitment: [11; 32],
        };
        let mut expected = alloc::vec![8; 32];
        expected.extend_from_slice(&[VERSION, 3]);
        expected.extend_from_slice(&9u64.to_le_bytes());
        expected.extend_from_slice(&[10; 32]);
        expected.extend_from_slice(&[11; 32]);
        assert_eq!(signer.encode(), expected);
        assert_eq!(expected.len(), SignerAddress::ENCODED_LEN);
        assert_eq!(SignerAddress::from_bytes(&expected).unwrap(), signer);
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

#[cfg(test)]
mod registration_tests {
    use super::*;

    #[test]
    fn registration_selector_preserves_inner_descriptor_encodings() {
        let classical = Descriptor {
            version: VERSION,
            scheme: Scheme::Sr25519,
            initial_commitment: [3; 32],
        };
        let mldsa = mldsa::Descriptor {
            version: mldsa::VERSION,
            algorithm: mldsa::ALGORITHM,
            initial_commitment: [5; 64],
        };
        for (tag, inner, registration) in [
            (
                0u8,
                classical.encode(),
                RegistrationDescriptor::from(classical),
            ),
            (1u8, mldsa.encode(), RegistrationDescriptor::from(mldsa)),
        ] {
            let encoded = registration.encode();
            assert_eq!(encoded.split_first(), Some((&tag, inner.as_slice())));
            assert_eq!(
                RegistrationDescriptor::decode(&mut encoded.as_slice()),
                Ok(registration)
            );
            let mut truncated = encoded
                .split_last()
                .map(|(_, rest)| rest)
                .unwrap_or_default();
            assert!(RegistrationDescriptor::decode(&mut truncated).is_err());
        }
        assert!(RegistrationDescriptor::decode(&mut [2u8].as_slice()).is_err());
    }
}
