//! A self-contained, network-independent receiving address for hashed accounts.
//!
//! The descriptor reveals only the initial commitment, never a signing key.
//! The checksum detects transcription errors; it does not authenticate a sender.
//! The payload contains only the descriptor; registration is independent per chain.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use codec::Decode;
use sp_core::crypto::{AccountId32, Ss58AddressFormat, Ss58Codec};
use sp_core::hashing::blake2_256;
use subtensor_hashed::Descriptor;

#[cfg(test)]
use super::CRYPTO_MLDSA65_FIXED;
use super::{crypto_err, is_fixed_pq_crypto, Keypair};
use crate::error::CoreError;

const PREFIX: &str = "tao_";
const DOMAIN: &[u8] = b"bittensor/hashed/v1/receiving";
const DESCRIPTOR_LEN: usize = 34;
const PAYLOAD_LEN: usize = DESCRIPTOR_LEN;
const CHECKSUM_LEN: usize = 8;
const BODY_LEN: usize = PAYLOAD_LEN + CHECKSUM_LEN;
const ADDRESS_LEN: usize = 60;
const MLDSA_LEGACY_PREFIX: &str = "bth2_";
const MLDSA_ENCODED_BODY_LEN: usize = 99;

fn validate_descriptor(bytes: &[u8]) -> Result<(), CoreError> {
    if bytes.len() != DESCRIPTOR_LEN {
        return Err(crypto_err(
            "hashed receiving descriptor must be exactly 34 bytes",
        ));
    }
    let mut input = bytes;
    let descriptor = Descriptor::decode(&mut input)
        .map_err(|_| crypto_err("invalid or unsupported hashed receiving descriptor"))?;
    if !input.is_empty() || !descriptor.is_supported() {
        return Err(crypto_err(
            "invalid or unsupported hashed receiving descriptor",
        ));
    }
    Ok(())
}

fn checksum(payload: &[u8]) -> [u8; CHECKSUM_LEN] {
    let mut input = DOMAIN.to_vec();
    input.extend_from_slice(payload);
    let hash = blake2_256(&input);
    let mut checksum = [0; CHECKSUM_LEN];
    checksum.copy_from_slice(&hash[..CHECKSUM_LEN]);
    checksum
}

/// Encode a public descriptor without a network field; addresses work on every chain.
/// This receiving address does not change the account's existing identity.
pub fn encode_hashed_receiving_address(descriptor: &[u8]) -> Result<String, CoreError> {
    if descriptor.first() == Some(&subtensor_hashed::mldsa::VERSION) {
        return encode_mldsa_receiving_address(descriptor);
    }
    validate_descriptor(descriptor)?;
    let mut body = Vec::with_capacity(BODY_LEN);
    body.extend_from_slice(descriptor);
    body.extend_from_slice(&checksum(&body));
    Ok(format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(body)))
}

/// Decode a receiving address into its original public descriptor.
pub fn decode_hashed_receiving_address(address: &str) -> Result<Vec<u8>, CoreError> {
    // Both formats share a prefix. Bound the encoded size before decoding, then
    // validate the format-specific descriptor and checksum below.
    if (address.starts_with(PREFIX) && address.len() == PREFIX.len() + MLDSA_ENCODED_BODY_LEN)
        || address.starts_with(MLDSA_LEGACY_PREFIX)
    {
        return decode_mldsa_receiving_address(address);
    }
    if address.len() != ADDRESS_LEN {
        return Err(crypto_err(
            "hashed receiving address must be exactly 60 characters",
        ));
    }
    let encoded = address
        .strip_prefix(PREFIX)
        .ok_or_else(|| crypto_err("unsupported hashed receiving address prefix"))?;
    let body = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| crypto_err("invalid hashed receiving address encoding"))?;
    if body.len() != BODY_LEN || URL_SAFE_NO_PAD.encode(&body) != encoded {
        return Err(crypto_err("noncanonical hashed receiving address"));
    }
    if body[PAYLOAD_LEN..] != checksum(&body[..PAYLOAD_LEN]) {
        return Err(crypto_err("hashed receiving address checksum mismatch"));
    }
    let descriptor = &body[..PAYLOAD_LEN];
    validate_descriptor(descriptor)?;
    Ok(descriptor.to_vec())
}

impl Keypair {
    /// Return the ordinary SS58 address for fixed ML-DSA, or the receiving
    /// descriptor for a registered account, without exposing a signing key.
    pub fn hashed_receiving_address(&self) -> Result<String, CoreError> {
        if is_fixed_pq_crypto(self.crypto_type()) {
            return Ok(self.ss58_address());
        }
        encode_hashed_receiving_address(&self.authorization_descriptor()?)
    }
}

fn mldsa_checksum(descriptor: &[u8]) -> [u8; CHECKSUM_LEN] {
    let mut bytes = b"bittensor/ms/v2/receiving".to_vec();
    bytes.extend_from_slice(descriptor);
    let mut checksum = [0; CHECKSUM_LEN];
    checksum.copy_from_slice(&blake2_256(&bytes)[..CHECKSUM_LEN]);
    checksum
}

/// Registered profiles retain their descriptor-bearing receiving address.
/// Registration-free profiles use ordinary SS58; their descriptor cannot be
/// recovered from the address and is unnecessary for receiving funds.
pub fn encode_mldsa_receiving_address(descriptor: &[u8]) -> Result<String, CoreError> {
    let parsed = subtensor_hashed::mldsa::Descriptor::from_bytes(descriptor)
        .map_err(|_| crypto_err("invalid post-quantum receiving descriptor"))?;
    if parsed.is_stateless() {
        return Ok(
            AccountId32::new(subtensor_hashed::mldsa::account_id(&parsed))
                .to_ss58check_with_version(Ss58AddressFormat::custom(super::DEFAULT_SS58_FORMAT)),
        );
    }
    let mut body = descriptor.to_vec();
    body.extend_from_slice(&mldsa_checksum(descriptor));
    Ok(format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(body)))
}

pub fn decode_mldsa_receiving_address(address: &str) -> Result<Vec<u8>, CoreError> {
    let encoded = address
        .strip_prefix(PREFIX)
        .or_else(|| address.strip_prefix(MLDSA_LEGACY_PREFIX))
        .ok_or_else(|| crypto_err("unsupported post-quantum receiving prefix"))?;
    if encoded.len() != MLDSA_ENCODED_BODY_LEN {
        return Err(crypto_err(
            "post-quantum receiving payload must be exactly 99 characters",
        ));
    }
    let body = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| crypto_err("invalid post-quantum receiving encoding"))?;
    if body.len() != 74 || URL_SAFE_NO_PAD.encode(&body) != encoded {
        return Err(crypto_err("noncanonical post-quantum receiving address"));
    }
    let descriptor = &body[..66];
    if body[66..] != mldsa_checksum(descriptor) {
        return Err(crypto_err("post-quantum receiving checksum mismatch"));
    }
    let parsed = subtensor_hashed::mldsa::Descriptor::from_bytes(descriptor)
        .map_err(|_| crypto_err("invalid post-quantum receiving descriptor"))?;
    if parsed.is_stateless() {
        return Err(crypto_err("unsupported post-quantum receiving descriptor"));
    }
    Ok(descriptor.to_vec())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use codec::Encode;
    use subtensor_hashed::{account_id, Scheme, VERSION};

    use super::*;
    use crate::keys::{CRYPTO_HASHED, CRYPTO_SR25519, DEFAULT_SS58_FORMAT};

    fn descriptor() -> Vec<u8> {
        Descriptor {
            version: VERSION,
            scheme: Scheme::Sr25519,
            initial_commitment: [7; 32],
        }
        .encode()
    }

    /// Construct a checksum-valid envelope independently of descriptor checks.
    /// Tests use this to distinguish rejecting unsupported protocol data from
    /// merely detecting an unchanged checksum after accidental damage.
    fn envelope(payload: &[u8]) -> String {
        let mut body = payload.to_vec();
        body.extend_from_slice(&checksum(payload));
        format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(body))
    }

    #[test]
    fn fixed_vector_matches_independent_blake2b_and_base64_implementation() {
        // Generated with Python hashlib.blake2b(digest_size=32) and
        // base64.urlsafe_b64encode, rather than this codec.
        const EXPECTED: &str = "tao_AQEHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHB2_GKIFP7N0l";
        let descriptor = descriptor();
        let address = encode_hashed_receiving_address(&descriptor).unwrap();
        assert_eq!(address, EXPECTED);
        assert_eq!(address.len(), ADDRESS_LEN);
        let decoded_descriptor = decode_hashed_receiving_address(&address).unwrap();
        assert_eq!(decoded_descriptor, descriptor);
        let public =
            Keypair::from_hashed_descriptor(&decoded_descriptor, DEFAULT_SS58_FORMAT).unwrap();
        let parsed = Descriptor::decode(&mut &descriptor[..]).unwrap();
        assert_eq!(public.public_key_bytes(), account_id(&parsed));
    }

    #[test]
    fn fixed_mldsa_has_only_an_ordinary_receiving_address() {
        let key = Keypair::from_seed(&[7; 32], CRYPTO_MLDSA65_FIXED).unwrap();
        let descriptor = key.mldsa_descriptor().unwrap();
        assert_eq!(
            encode_mldsa_receiving_address(&descriptor).unwrap(),
            key.ss58_address()
        );
        let mut body = descriptor.clone();
        body.extend_from_slice(&mldsa_checksum(&descriptor));
        for prefix in [PREFIX, MLDSA_LEGACY_PREFIX] {
            let raw = format!("{prefix}{}", URL_SAFE_NO_PAD.encode(&body));
            assert!(decode_mldsa_receiving_address(&raw).is_err());
        }
        let public = Keypair::from_mldsa_descriptor(&descriptor, 0).unwrap();
        assert_eq!(
            public.hashed_receiving_address().unwrap(),
            public.ss58_address()
        );
        assert_ne!(public.ss58_address(), key.ss58_address());
    }

    #[test]
    fn shared_prefix_preserves_each_rotating_scheme_and_account() {
        for crypto_type in [
            CRYPTO_HASHED,
            crate::keys::CRYPTO_HASHED_ED25519,
            crate::keys::CRYPTO_MLDSA65,
        ] {
            let key = Keypair::from_seed(&[17; 32], crypto_type).unwrap();
            let descriptor = key.authorization_descriptor().unwrap();
            let address = key.hashed_receiving_address().unwrap();
            assert!(address.starts_with(PREFIX));
            assert_eq!(
                decode_hashed_receiving_address(&address).unwrap(),
                descriptor
            );
            let restored =
                Keypair::from_hashed_descriptor(&descriptor, DEFAULT_SS58_FORMAT).unwrap();
            assert_eq!(restored.crypto_type(), crypto_type);
            assert_eq!(restored.ss58_address(), key.ss58_address());
            assert_eq!(restored.hashed_receiving_address().unwrap(), address);
            if crypto_type != crate::keys::CRYPTO_MLDSA65 {
                assert_eq!(address.len(), 60);
                assert!(decode_mldsa_receiving_address(&address).is_err());
            } else {
                assert_eq!(address.len(), 103);
            }
        }
    }

    #[test]
    fn every_payload_and_checksum_byte_is_covered() {
        let address = encode_hashed_receiving_address(&descriptor()).unwrap();
        let body = URL_SAFE_NO_PAD
            .decode(address.strip_prefix(PREFIX).unwrap())
            .unwrap();
        for index in 0..BODY_LEN {
            let mut damaged = body.clone();
            damaged[index] ^= 1;
            let encoded = format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(damaged));
            assert!(
                decode_hashed_receiving_address(&encoded).is_err(),
                "byte {index} was not protected"
            );
        }
    }

    #[test]
    fn addresses_are_stable_across_networks_and_rotation() {
        for crypto_type in [CRYPTO_HASHED, crate::keys::CRYPTO_HASHED_ED25519] {
            let key = Keypair::from_seed(&[17; 32], crypto_type).unwrap();
            let address = key.hashed_receiving_address().unwrap();
            assert_eq!(
                key.at_generation(90_001)
                    .unwrap()
                    .hashed_receiving_address()
                    .unwrap(),
                address
            );
            assert_eq!(
                key.public_only()
                    .unwrap()
                    .hashed_receiving_address()
                    .unwrap(),
                address
            );
            assert_eq!(
                decode_hashed_receiving_address(&address).unwrap(),
                key.hashed_descriptor().unwrap()
            );
        }
        let classical = Keypair::from_seed(&[17; 32], CRYPTO_SR25519).unwrap();
        assert!(classical.hashed_receiving_address().is_err());
    }

    #[test]
    fn unsupported_descriptors_are_rejected_even_with_a_valid_checksum() {
        for (offset, replacement) in [(0, 0), (0, 2), (1, 0), (1, 4), (1, 255)] {
            let mut invalid = descriptor();
            invalid[offset] = replacement;
            assert!(encode_hashed_receiving_address(&invalid).is_err());
            assert!(decode_hashed_receiving_address(&envelope(&invalid)).is_err());
        }
        let empty_commitment = Descriptor {
            version: VERSION,
            scheme: Scheme::Sr25519,
            initial_commitment: [0; 32],
        }
        .encode();
        assert!(encode_hashed_receiving_address(&empty_commitment).is_err());
        assert!(decode_hashed_receiving_address(&envelope(&empty_commitment)).is_err());
        for invalid in [vec![], vec![1; 33], vec![1; 35]] {
            assert!(encode_hashed_receiving_address(&invalid).is_err());
        }
    }

    #[test]
    fn alternate_spellings_padding_and_trailing_data_are_rejected() {
        let address = encode_hashed_receiving_address(&descriptor()).unwrap();
        for invalid in [
            address.replacen("tao_", "bth1_", 1),
            address.replacen("tao_", "bth2_", 1),
            address.replacen("tao_", "TAO_", 1),
            address.replacen("tao_", "ss58_", 1),
            format!(" {address}"),
            format!("{address}\n"),
            format!("{address}="),
            address[..ADDRESS_LEN - 1].to_string(),
        ] {
            assert!(decode_hashed_receiving_address(&invalid).is_err());
        }
        for replacement in [b' ', b'\n', b'=', b'+', b'/'] {
            let mut invalid = address.as_bytes().to_vec();
            invalid[20] = replacement;
            assert!(
                decode_hashed_receiving_address(std::str::from_utf8(&invalid).unwrap()).is_err()
            );
        }
    }

    #[test]
    fn unused_network_field_is_not_accepted() {
        for network in [[0; 32], [17; 32]] {
            let mut payload = network.to_vec();
            payload.extend_from_slice(&descriptor());
            assert!(decode_hashed_receiving_address(&envelope(&payload)).is_err());
        }
    }
}
