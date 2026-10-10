//! A self-contained, network-independent receiving address for hashed accounts.
//!
//! The descriptor reveals only the initial commitment, never a signing key.
//! The checksum detects transcription errors; it does not authenticate a sender.
//! The former 32-byte network field is reserved and zero in new encodings.
//! Legacy addresses remain decodable; their embedded network is not a restriction.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use codec::Decode;
use sp_core::hashing::blake2_256;
use subtensor_hashed::Descriptor;

use super::{crypto_err, Keypair};
use crate::error::CoreError;

const PREFIX: &str = "bth1_";
const DOMAIN: &[u8] = b"bittensor/hashed/v1/receiving";
const RESERVED_LEN: usize = 32;
const DESCRIPTOR_LEN: usize = 34;
const PAYLOAD_LEN: usize = RESERVED_LEN + DESCRIPTOR_LEN;
const CHECKSUM_LEN: usize = 8;
const BODY_LEN: usize = PAYLOAD_LEN + CHECKSUM_LEN;
const ADDRESS_LEN: usize = 104;

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

/// Encode a public descriptor with a zeroed, reserved compatibility field.
/// The legacy genesis_hash argument is ignored; addresses work on every chain.
/// This receiving address does not change the account's existing identity.
pub fn encode_hashed_receiving_address(
    descriptor: &[u8],
    _genesis_hash: &[u8],
) -> Result<String, CoreError> {
    validate_descriptor(descriptor)?;
    let mut body = Vec::with_capacity(BODY_LEN);
    body.extend_from_slice(&[0; RESERVED_LEN]);
    body.extend_from_slice(descriptor);
    body.extend_from_slice(&checksum(&body));
    Ok(format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(body)))
}

/// Decode a receiving address into `(legacy_network_field, descriptor)`.
/// The first field is returned only for wire compatibility. It must not restrict
/// where the account can be used; registration is independently checked per chain.
pub fn decode_hashed_receiving_address(address: &str) -> Result<([u8; 32], Vec<u8>), CoreError> {
    if address.len() != ADDRESS_LEN {
        return Err(crypto_err(
            "hashed receiving address must be exactly 104 characters",
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
    let descriptor = &body[RESERVED_LEN..PAYLOAD_LEN];
    validate_descriptor(descriptor)?;
    let mut legacy_network_field = [0; RESERVED_LEN];
    legacy_network_field.copy_from_slice(&body[..RESERVED_LEN]);
    Ok((legacy_network_field, descriptor.to_vec()))
}

impl Keypair {
    /// Share this account's initial descriptor without disclosing
    /// its current signing key. Also works for descriptor-bearing public keys.
    pub fn hashed_receiving_address(&self, genesis_hash: &[u8]) -> Result<String, CoreError> {
        encode_hashed_receiving_address(&self.hashed_descriptor()?, genesis_hash)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use codec::Encode;
    use subtensor_hashed::{account_id, Scheme, VERSION};

    use super::*;
    use crate::keys::{CRYPTO_HASHED, CRYPTO_MLDSA, CRYPTO_SR25519, DEFAULT_SS58_FORMAT};

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
        const EXPECTED: &str = "bth1_AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8BAQcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHbgCS2rCAxcU";
        let genesis: Vec<u8> = (0..32).collect();
        let descriptor = descriptor();
        let address = encode_hashed_receiving_address(&descriptor, &genesis).unwrap();
        const CANONICAL: &str = "bth1_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABAQcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHnKscYQ9BfVA";
        assert_eq!(address, CANONICAL);
        assert_eq!(
            address,
            encode_hashed_receiving_address(&descriptor, &[]).unwrap()
        );
        let (reserved, canonical_descriptor) = decode_hashed_receiving_address(&address).unwrap();
        assert_eq!(reserved, [0; 32]);
        assert_eq!(canonical_descriptor, descriptor);
        assert_eq!(address.len(), ADDRESS_LEN);
        let (decoded_genesis, decoded_descriptor) =
            decode_hashed_receiving_address(EXPECTED).unwrap();
        assert_eq!(decoded_genesis.as_slice(), genesis);
        assert_eq!(decoded_descriptor, descriptor);
        let public =
            Keypair::from_hashed_descriptor(&decoded_descriptor, DEFAULT_SS58_FORMAT).unwrap();
        let parsed = Descriptor::decode(&mut &descriptor[..]).unwrap();
        assert_eq!(public.public_key_bytes(), account_id(&parsed));
    }

    #[test]
    fn every_payload_and_checksum_byte_is_covered() {
        let address = encode_hashed_receiving_address(&descriptor(), &[9; 32]).unwrap();
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
        for crypto_type in [
            CRYPTO_HASHED,
            CRYPTO_MLDSA,
            crate::keys::CRYPTO_HASHED_ED25519,
            crate::keys::CRYPTO_MLDSA_STANDARD,
        ] {
            let key = Keypair::from_seed(&[17; 32], crypto_type).unwrap();
            let genesis = [11; 32];
            let address = key.hashed_receiving_address(&genesis).unwrap();
            assert_eq!(
                key.at_generation(90_001)
                    .unwrap()
                    .hashed_receiving_address(&genesis)
                    .unwrap(),
                address
            );
            assert_eq!(
                key.public_only()
                    .unwrap()
                    .hashed_receiving_address(&genesis)
                    .unwrap(),
                address
            );
            for index in 0..RESERVED_LEN {
                let mut other_network = genesis;
                other_network[index] ^= 1;
                let other = key.hashed_receiving_address(&other_network).unwrap();
                assert_eq!(other, address);
                let (decoded, descriptor) = decode_hashed_receiving_address(&other).unwrap();
                assert_eq!(decoded, [0; 32]);
                assert_eq!(descriptor, key.hashed_descriptor().unwrap());
            }
        }
        let classical = Keypair::from_seed(&[17; 32], CRYPTO_SR25519).unwrap();
        assert!(classical.hashed_receiving_address(&[]).is_err());
    }

    #[test]
    fn unsupported_descriptors_are_rejected_even_with_a_valid_checksum() {
        for (offset, replacement) in [(0, 0), (0, 2), (1, 0), (1, 4), (1, 255)] {
            let mut invalid = descriptor();
            invalid[offset] = replacement;
            assert!(encode_hashed_receiving_address(&invalid, &[0; 32]).is_err());
            let mut payload = vec![0; 32];
            payload.extend_from_slice(&invalid);
            assert!(decode_hashed_receiving_address(&envelope(&payload)).is_err());
        }
        let empty_commitment = Descriptor {
            version: VERSION,
            scheme: Scheme::Sr25519,
            initial_commitment: [0; 32],
        }
        .encode();
        assert!(encode_hashed_receiving_address(&empty_commitment, &[0; 32]).is_err());
        let mut payload = vec![0; 32];
        payload.extend_from_slice(&empty_commitment);
        assert!(decode_hashed_receiving_address(&envelope(&payload)).is_err());
        for invalid in [vec![], vec![1; 33], vec![1; 35]] {
            assert!(encode_hashed_receiving_address(&invalid, &[0; 32]).is_err());
        }
        for length in [0, 31, 33] {
            assert_eq!(
                encode_hashed_receiving_address(&descriptor(), &vec![0; length]).unwrap(),
                encode_hashed_receiving_address(&descriptor(), &[]).unwrap(),
            );
        }
    }

    #[test]
    fn alternate_spellings_padding_and_trailing_data_are_rejected() {
        let address = encode_hashed_receiving_address(&descriptor(), &[9; 32]).unwrap();
        for invalid in [
            address.replacen("bth1_", "bth2_", 1),
            address.replacen("bth1_", "BTH1_", 1),
            address.replacen("bth1_", "ss58_", 1),
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
        // 74 bytes leave two unused bits in the last base64 character. Set
        // one without changing the represented payload: it must not be accepted.
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut invalid = address.into_bytes();
        let last = *invalid.last().unwrap();
        let value = ALPHABET.iter().position(|byte| *byte == last).unwrap();
        *invalid.last_mut().unwrap() = ALPHABET[value | 1];
        assert!(decode_hashed_receiving_address(std::str::from_utf8(&invalid).unwrap()).is_err());
    }
}
