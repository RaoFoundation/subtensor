//! Mnemonic-recoverable rotating keys. This module has no mutable signing
//! counter: the chain selects a generation and a wallet derives it independently.

use codec::{Decode, Encode};
use hkdf::Hkdf;
use sha2::Sha256;
use sp_core::{crypto::Pair as _, ed25519, sr25519};
use subtensor_hashed::fips204::{
    ml_dsa_65,
    traits::{KeyGen, SerDes, Signer},
};
use subtensor_hashed::{
    account_id, key_commitment, transaction_payload, Descriptor, Proof, Scheme,
};
use zeroize::Zeroizing;

use super::{
    crypto_err, is_mldsa_crypto, Keypair, KeypairInner, CRYPTO_HASHED, CRYPTO_HASHED_ED25519,
    CRYPTO_MLDSA, CRYPTO_MLDSA_STANDARD,
};
use crate::error::CoreError;

const VERSION: u8 = 1;
const DERIVATION_SALT: &[u8] = b"bittensor/hashed/v1/derive";

/// Public metadata is sufficient to preserve an address in a watch-only wallet.
/// Signing additionally requires the original, independent master seed.
pub struct HashedKeypair {
    descriptor: Descriptor,
    master_seed: Option<Zeroizing<[u8; 32]>>,
    generation: u64,
}

impl HashedKeypair {
    pub(super) fn from_seed(seed: &[u8], crypto_type: u8) -> Result<Self, CoreError> {
        let seed: [u8; 32] = seed
            .try_into()
            .map_err(|_| crypto_err("hashed master seed must be exactly 32 bytes"))?;
        let master_seed = Zeroizing::new(seed);
        let scheme = if is_mldsa_crypto(crypto_type) {
            Scheme::MlDsa65
        } else if crypto_type == CRYPTO_HASHED_ED25519 {
            Scheme::Ed25519
        } else {
            Scheme::Sr25519
        };
        let version = if crypto_type == CRYPTO_MLDSA_STANDARD {
            2
        } else {
            VERSION
        };
        let initial_commitment = match scheme {
            Scheme::Sr25519 => key_commitment(scheme, &Self::derive(&master_seed, 0)?.public().0),
            Scheme::Ed25519 => {
                key_commitment(scheme, &Self::derive_ed(&master_seed, 0)?.public().0)
            }
            Scheme::MlDsa65 => {
                let (public, _) = Self::derive_mldsa(&master_seed, 0, version)?;
                key_commitment(scheme, &public.into_bytes())
            }
        };
        Ok(Self {
            descriptor: Descriptor {
                version,
                scheme,
                initial_commitment,
            },
            master_seed: Some(master_seed),
            generation: 0,
        })
    }

    fn derive(master: &[u8; 32], generation: u64) -> Result<sr25519::Pair, CoreError> {
        // HKDF uses the secret master independently for every generation. In
        // particular, the next seed is never derived from the current signer.
        let kdf = Hkdf::<Sha256>::new(Some(DERIVATION_SALT), master);
        let info = (VERSION, Scheme::Sr25519, generation).encode();
        let mut seed = Zeroizing::new([0u8; 32]);
        kdf.expand(&info, seed.as_mut())
            .map_err(|_| crypto_err("hashed signing-key derivation failed"))?;
        Ok(sr25519::Pair::from_seed(&seed))
    }

    fn derive_ed(master: &[u8; 32], generation: u64) -> Result<ed25519::Pair, CoreError> {
        let kdf = Hkdf::<Sha256>::new(Some(DERIVATION_SALT), master);
        let mut seed = Zeroizing::new([0u8; 32]);
        kdf.expand(
            &(VERSION, Scheme::Ed25519, generation).encode(),
            seed.as_mut(),
        )
        .map_err(|_| crypto_err("Ed25519 signing-key derivation failed"))?;
        Ok(ed25519::Pair::from_seed(&seed))
    }

    fn ed_at(&self, generation: u64) -> Result<ed25519::Pair, CoreError> {
        let master = self
            .master_seed()
            .ok_or_else(|| crypto_err("signing requires a private wallet"))?;
        Self::derive_ed(master, generation)
    }

    fn derive_mldsa(
        master: &[u8; 32],
        generation: u64,
        version: u8,
    ) -> Result<(ml_dsa_65::PublicKey, ml_dsa_65::PrivateKey), CoreError> {
        let kdf = Hkdf::<Sha256>::new(Some(DERIVATION_SALT), master);
        let mut seed = Zeroizing::new([0u8; 32]);
        kdf.expand(
            &(version, Scheme::MlDsa65, generation).encode(),
            seed.as_mut(),
        )
        .map_err(|_| crypto_err("ML-DSA signing-key derivation failed"))?;
        Ok(ml_dsa_65::KG::keygen_from_seed(&seed))
    }

    fn mldsa_at(
        &self,
        generation: u64,
    ) -> Result<(ml_dsa_65::PublicKey, ml_dsa_65::PrivateKey), CoreError> {
        let master = self
            .master_seed()
            .ok_or_else(|| crypto_err("ML-DSA signing requires a private wallet"))?;
        Self::derive_mldsa(
            master,
            if self.descriptor.rotates() {
                generation
            } else {
                0
            },
            self.descriptor.version,
        )
    }

    pub(super) fn crypto_type(&self) -> u8 {
        match self.descriptor.scheme {
            Scheme::Sr25519 => CRYPTO_HASHED,
            Scheme::MlDsa65 => {
                if self.descriptor.rotates() {
                    CRYPTO_MLDSA
                } else {
                    CRYPTO_MLDSA_STANDARD
                }
            }
            Scheme::Ed25519 => CRYPTO_HASHED_ED25519,
        }
    }

    pub(super) fn master_seed(&self) -> Option<&[u8; 32]> {
        self.master_seed.as_deref()
    }

    pub(super) fn account_id(&self) -> [u8; 32] {
        account_id(&self.descriptor)
    }

    fn pair_at(&self, generation: u64) -> Result<sr25519::Pair, CoreError> {
        let master = self.master_seed().ok_or_else(|| {
            crypto_err("hashed signing-key derivation requires the mnemonic or master seed")
        })?;
        Self::derive(master, generation)
    }
}

impl Keypair {
    fn hashed(&self) -> Result<&HashedKeypair, CoreError> {
        match &self.inner {
            KeypairInner::Hashed(hashed) => Ok(hashed),
            _ => Err(crypto_err("operation requires a hashed key descriptor")),
        }
    }

    /// Restore public account metadata without retaining any signing material.
    pub fn from_hashed_descriptor(encoded: &[u8], ss58_format: u16) -> Result<Self, CoreError> {
        let mut input = encoded;
        let descriptor = Descriptor::decode(&mut input)
            .map_err(|_| crypto_err("invalid or unsupported hashed account descriptor"))?;
        if !input.is_empty() || !descriptor.is_supported() {
            return Err(crypto_err(
                "invalid or unsupported hashed account descriptor",
            ));
        }
        Ok(Self {
            inner: KeypairInner::Hashed(HashedKeypair {
                descriptor,
                master_seed: None,
                generation: 0,
            }),
            ss58_format,
            mnemonic: None,
            seed: None,
        })
    }

    /// Clone public metadata only, including the hashed genesis descriptor.
    pub fn public_only(&self) -> Result<Self, CoreError> {
        if let KeypairInner::Hashed(hashed) = &self.inner {
            let mut public =
                Self::from_hashed_descriptor(&hashed.descriptor.encode(), self.ss58_format)?;
            if let KeypairInner::Hashed(copy) = &mut public.inner {
                copy.generation = hashed.generation;
            }
            return Ok(public);
        }
        Self::new(
            None,
            Some(&self.public_key_bytes()),
            self.crypto_type(),
            self.ss58_format,
        )
    }

    /// A new view of the same stable account at the generation read from chain.
    /// Neither this operation nor signing mutates any persisted wallet state.
    pub fn at_generation(&self, generation: u64) -> Result<Self, CoreError> {
        let hashed = self.hashed()?;
        if hashed.master_seed.is_none() {
            return Err(crypto_err(
                "hashed generation selection requires a private wallet",
            ));
        }
        Ok(Self {
            inner: KeypairInner::Hashed(HashedKeypair {
                descriptor: hashed.descriptor,
                master_seed: hashed.master_seed.clone(),
                generation,
            }),
            ss58_format: self.ss58_format,
            mnemonic: self.mnemonic.clone(),
            seed: self.seed.clone(),
        })
    }

    pub fn hashed_descriptor(&self) -> Result<Vec<u8>, CoreError> {
        Ok(self.hashed()?.descriptor.encode())
    }

    pub fn hashed_generation(&self) -> Result<u64, CoreError> {
        Ok(self.hashed()?.generation)
    }

    pub fn hashed_public_key(&self) -> Result<[u8; 32], CoreError> {
        let hashed = self.hashed()?;
        if hashed.descriptor.scheme == Scheme::Ed25519 {
            return Ok(hashed.ed_at(hashed.generation)?.public().0);
        }
        if hashed.descriptor.scheme != Scheme::Sr25519 {
            return Err(crypto_err(
                "ML-DSA public keys are 1952 bytes; use hashed_signing_public_key",
            ));
        }
        Ok(hashed.pair_at(hashed.generation)?.public().0)
    }

    pub fn hashed_commitment(&self, generation: u64) -> Result<[u8; 32], CoreError> {
        let hashed = self.hashed()?;
        if generation == 0 {
            return Ok(hashed.descriptor.initial_commitment);
        }
        Ok(match hashed.descriptor.scheme {
            Scheme::Sr25519 => {
                key_commitment(Scheme::Sr25519, &hashed.pair_at(generation)?.public().0)
            }
            Scheme::Ed25519 => {
                key_commitment(Scheme::Ed25519, &hashed.ed_at(generation)?.public().0)
            }
            Scheme::MlDsa65 => key_commitment(
                Scheme::MlDsa65,
                &hashed.mldsa_at(generation)?.0.into_bytes(),
            ),
        })
    }

    pub fn hashed_current_commitment(&self) -> Result<[u8; 32], CoreError> {
        self.hashed_commitment(self.hashed_generation()?)
    }

    pub fn hashed_next_commitment(&self) -> Result<[u8; 32], CoreError> {
        let next = self
            .hashed_generation()?
            .checked_add(1)
            .ok_or_else(|| crypto_err("hashed signing generation exhausted"))?;
        self.hashed_commitment(next)
    }

    /// Sign the full FRAME transaction implication: version byte, encoded call,
    /// transaction extensions and their implicit data. Do not pass the legacy
    /// pre-hashed signing payload: the shared protocol performs its own hash.
    /// The 136-byte SCALE proof binds the stable account, current generation,
    /// next commitment and the entire implication.
    pub fn sign_hashed(&self, implication: &[u8]) -> Result<Vec<u8>, CoreError> {
        let hashed = self.hashed()?;
        let next_commitment = self.hashed_next_commitment()?;
        let payload = transaction_payload(
            &hashed.account_id(),
            hashed.descriptor.scheme,
            hashed.generation,
            &next_commitment,
            implication,
        );
        match hashed.descriptor.scheme {
            Scheme::Sr25519 => {
                let pair = hashed.pair_at(hashed.generation)?;
                Ok(Proof {
                    generation: hashed.generation,
                    public_key: pair.public().0,
                    next_commitment,
                    signature: pair.sign(&payload).0,
                }
                .encode())
            }
            Scheme::Ed25519 => {
                let pair = hashed.ed_at(hashed.generation)?;
                Ok(Proof {
                    generation: hashed.generation,
                    public_key: pair.public().0,
                    next_commitment,
                    signature: pair.sign(&payload).0,
                }
                .encode())
            }
            Scheme::MlDsa65 => {
                let (public, private) = hashed.mldsa_at(hashed.generation)?;
                let signature = private
                    .try_sign_with_rng(
                        &mut rand_core::OsRng,
                        &payload,
                        subtensor_hashed::MLDSA_TRANSACTION_CONTEXT,
                    )
                    .map_err(crypto_err)?;
                Ok(subtensor_hashed::MlDsaProof {
                    generation: hashed.generation,
                    public_key: public.into_bytes(),
                    next_commitment,
                    signature,
                }
                .encode())
            }
        }
    }
    pub fn hashed_signing_public_key(&self) -> Result<Vec<u8>, CoreError> {
        let hashed = self.hashed()?;
        Ok(match hashed.descriptor.scheme {
            Scheme::Sr25519 => hashed.pair_at(hashed.generation)?.public().0.to_vec(),
            Scheme::MlDsa65 => hashed.mldsa_at(hashed.generation)?.0.into_bytes().to_vec(),
            Scheme::Ed25519 => hashed.ed_at(hashed.generation)?.public().0.to_vec(),
        })
    }

    /// Sign with the original identity key, never the current rotating sr25519
    /// spending key. For sr25519, the caller must select a generation obtained
    /// from finalized chain state and must have retired generation zero first.
    pub(super) fn sign_hashed_message(&self, message: &[u8]) -> Result<Vec<u8>, CoreError> {
        let hashed = self.hashed()?;
        let payload = message_payload(&hashed.account_id(), message);
        let mut envelope = MESSAGE_MAGIC.to_vec();
        envelope.extend(hashed.descriptor.encode());
        match hashed.descriptor.scheme {
            Scheme::Sr25519 => {
                if hashed.generation == 0 {
                    return Err(crypto_err("hashed message signing requires generation zero to be retired on chain first"));
                }
                let pair = hashed.pair_at(0)?;
                envelope.extend_from_slice(&pair.public().0);
                envelope.extend_from_slice(&pair.sign(&payload).0);
            }
            Scheme::Ed25519 => {
                if hashed.generation == 0 {
                    return Err(crypto_err("hashed message signing requires generation zero to be retired on chain first"));
                }
                let pair = hashed.ed_at(0)?;
                envelope.extend_from_slice(&pair.public().0);
                envelope.extend_from_slice(&pair.sign(&payload).0);
            }
            Scheme::MlDsa65 => {
                let (public, private) = hashed.mldsa_at(0)?;
                envelope.extend_from_slice(&public.into_bytes());
                envelope.extend_from_slice(
                    &private
                        .try_sign_with_rng(&mut rand_core::OsRng, &payload, MESSAGE_MAGIC)
                        .map_err(crypto_err)?,
                );
            }
        }
        Ok(envelope)
    }
}

const MESSAGE_MAGIC: &[u8] = b"BTHMSG01";

fn message_payload(account: &[u8; 32], message: &[u8]) -> Vec<u8> {
    let mut payload = b"bittensor/hashed/v1/message".to_vec();
    (account, message).encode_to(&mut payload);
    payload
}

pub(super) fn verify_message(
    crypto_type: u8,
    account: &[u8; 32],
    message: &[u8],
    envelope: &[u8],
) -> bool {
    let expected_len = if is_mldsa_crypto(crypto_type) {
        8 + 34 + 1952 + 3309
    } else {
        8 + 34 + 32 + 64
    };
    if envelope.len() != expected_len || !envelope.starts_with(MESSAGE_MAGIC) {
        return false;
    }
    let mut input = &envelope[8..];
    let Ok(descriptor) = Descriptor::decode(&mut input) else {
        return false;
    };
    let scheme = if is_mldsa_crypto(crypto_type) {
        Scheme::MlDsa65
    } else if crypto_type == CRYPTO_HASHED_ED25519 {
        Scheme::Ed25519
    } else {
        Scheme::Sr25519
    };
    if !descriptor.is_supported()
        || descriptor.rotates() == (crypto_type == CRYPTO_MLDSA_STANDARD)
        || descriptor.scheme != scheme
        || account_id(&descriptor) != *account
    {
        return false;
    }
    let payload = message_payload(account, message);
    match scheme {
        Scheme::Sr25519 => {
            let (Ok(public), Ok(signature)) = (
                <[u8; 32]>::try_from(&input[..32]),
                <[u8; 64]>::try_from(&input[32..]),
            ) else {
                return false;
            };
            descriptor.initial_commitment == key_commitment(scheme, &public)
                && sr25519::Pair::verify(
                    &sr25519::Signature::from_raw(signature),
                    &payload,
                    &sr25519::Public::from_raw(public),
                )
        }
        Scheme::Ed25519 => {
            let (Ok(public), Ok(signature)) = (
                <[u8; 32]>::try_from(&input[..32]),
                <[u8; 64]>::try_from(&input[32..]),
            ) else {
                return false;
            };
            descriptor.initial_commitment == key_commitment(scheme, &public)
                && ed25519::Pair::verify(
                    &ed25519::Signature::from_raw(signature),
                    &payload,
                    &ed25519::Public::from_raw(public),
                )
        }
        Scheme::MlDsa65 => {
            let Ok(public) = <[u8; 1952]>::try_from(&input[..1952]) else {
                return false;
            };
            descriptor.initial_commitment == key_commitment(scheme, &public)
                && subtensor_hashed::verify_mldsa(&public, &input[1952..], &payload, MESSAGE_MAGIC)
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::keys::{CRYPTO_HASHED, DEFAULT_SS58_FORMAT};

    #[test]
    fn composable_schemes_preserve_mode_recovery_and_signatures() {
        use crate::keys::{account_crypto_type, CRYPTO_ED25519, CRYPTO_SR25519};
        let phrase = "bottom drive obey lake curtain smoke basket hold race lonely fit walk";
        let mut addresses = std::collections::BTreeSet::new();
        for scheme in [CRYPTO_SR25519, CRYPTO_ED25519, CRYPTO_MLDSA] {
            for mode in ["standard", "hashed"] {
                let code = account_crypto_type(scheme, Some(mode)).unwrap();
                let key = Keypair::from_mnemonic(phrase, code, None).unwrap();
                assert_eq!(key.account_type(), mode);
                assert!(addresses.insert(key.ss58_address()));
                let recovered = Keypair::from_seed(key.seed_bytes().unwrap(), code).unwrap();
                assert_eq!(recovered.public_key_bytes(), key.public_key_bytes());
                if !super::super::is_hashed_crypto(code) {
                    let signature = key.sign(b"message").unwrap();
                    assert!(recovered.verify(b"message", &signature).unwrap());
                    continue;
                }
                let public = key.public_only().unwrap();
                assert_eq!(public.crypto_type(), code);
                assert_eq!(
                    key.hashed_descriptor().unwrap(),
                    recovered.hashed_descriptor().unwrap()
                );
                assert!(public.sign_hashed(b"implication").is_err());
                let initial = key.hashed_signing_public_key().unwrap();
                let later = key.at_generation(42).unwrap();
                assert_eq!(
                    initial == later.hashed_signing_public_key().unwrap(),
                    mode == "standard"
                );
                assert_eq!(
                    later.hashed_current_commitment().unwrap()
                        == later.hashed_next_commitment().unwrap(),
                    mode == "standard"
                );
                assert_eq!(later.public_key_bytes(), key.public_key_bytes());
                let signature = later.sign(b"message").unwrap();
                assert!(public.verify(b"message", &signature).unwrap());
                assert!(!public.verify(b"changed", &signature).unwrap());
                let wrong_mode = account_crypto_type(
                    scheme,
                    Some(if mode == "standard" {
                        "hashed"
                    } else {
                        "standard"
                    }),
                )
                .unwrap();
                assert!(!super::super::verify_with_crypto(
                    wrong_mode,
                    &key.public_key_bytes(),
                    b"message",
                    &signature
                )
                .unwrap_or(false));
            }
        }
    }

    #[test]
    fn hashed_ed_proof_is_scheme_bound_and_retires_message_key() {
        let key = Keypair::from_seed(&[31; 32], CRYPTO_HASHED_ED25519).unwrap();
        assert!(key.sign(b"message").is_err());
        let signer = key.at_generation(8).unwrap();
        let bytes = signer.sign_hashed(b"implication").unwrap();
        let proof = Proof::decode(&mut bytes.as_slice()).unwrap();
        assert_eq!(proof.generation, 8);
        let payload = transaction_payload(
            &key.public_key_bytes(),
            Scheme::Ed25519,
            8,
            &proof.next_commitment,
            b"implication",
        );
        assert!(ed25519::Pair::verify(
            &ed25519::Signature::from_raw(proof.signature),
            payload,
            &ed25519::Public::from_raw(proof.public_key)
        ));
        let wrong = transaction_payload(
            &key.public_key_bytes(),
            Scheme::Sr25519,
            8,
            &proof.next_commitment,
            b"implication",
        );
        assert!(!ed25519::Pair::verify(
            &ed25519::Signature::from_raw(proof.signature),
            wrong,
            &ed25519::Public::from_raw(proof.public_key)
        ));
    }

    #[test]
    fn mldsa_recovery_rotation_and_messages() {
        let key = Keypair::from_seed(&[43; 32], CRYPTO_MLDSA).unwrap();
        let legacy = Keypair::from_seed(&[43; 32], CRYPTO_HASHED).unwrap();
        assert_ne!(key.ss58_address(), legacy.ss58_address());
        assert_eq!(key.hashed_descriptor().unwrap()[..2], [1, 2]);
        let public = key.public_only().unwrap();
        assert_eq!(public.crypto_type(), CRYPTO_MLDSA);
        assert!(public.sign(b"message").is_err());
        for generation in [0, 1, 42, u64::MAX - 1] {
            let signer = key.at_generation(generation).unwrap();
            let encoded = signer.sign_hashed(b"complete implication").unwrap();
            assert_eq!(encoded.len(), 5301);
            let proof = subtensor_hashed::MlDsaProof::decode(&mut encoded.as_slice()).unwrap();
            assert_eq!(proof.generation, generation);
            assert_eq!(
                key_commitment(Scheme::MlDsa65, &proof.public_key),
                signer.hashed_current_commitment().unwrap()
            );
            let payload = transaction_payload(
                &key.public_key_bytes(),
                Scheme::MlDsa65,
                generation,
                &proof.next_commitment,
                b"complete implication",
            );
            assert!(subtensor_hashed::verify_mldsa(
                &proof.public_key,
                &proof.signature,
                &payload,
                subtensor_hashed::MLDSA_TRANSACTION_CONTEXT
            ));
            assert!(!subtensor_hashed::verify_mldsa(
                &proof.public_key,
                &proof.signature,
                &payload,
                MESSAGE_MAGIC
            ));
            let message = signer.sign(b"identity challenge").unwrap();
            assert!(public.verify(b"identity challenge", &message).unwrap());
            assert!(!public.verify(b"different challenge", &message).unwrap());
            assert!(!legacy.verify(b"identity challenge", &message).unwrap());
            let mut changed = message.clone();
            changed.push(0);
            assert!(!public.verify(b"identity challenge", &changed).unwrap());
            changed = message;
            changed[42] ^= 1;
            assert!(!public.verify(b"identity challenge", &changed).unwrap());
        }
        assert!(key
            .at_generation(u64::MAX)
            .unwrap()
            .sign_hashed(b"payload")
            .is_err());
        let recovered = Keypair::from_seed(&[43; 32], CRYPTO_MLDSA).unwrap();
        assert_eq!(
            key.hashed_descriptor().unwrap(),
            recovered.hashed_descriptor().unwrap()
        );
        assert_eq!(
            key.hashed_commitment(42).unwrap(),
            recovered.hashed_commitment(42).unwrap()
        );
    }

    #[test]
    fn retired_sr_message_key_never_reveals_current_spending_key() {
        let key = Keypair::from_seed(&[43; 32], CRYPTO_HASHED).unwrap();
        assert!(key.sign(b"challenge").is_err());
        let current = key.at_generation(1).unwrap();
        let envelope = current.sign(b"challenge").unwrap();
        assert_eq!(&envelope[42..74], &key.hashed_public_key().unwrap());
        assert_ne!(&envelope[42..74], &current.hashed_public_key().unwrap());
        assert!(key
            .public_only()
            .unwrap()
            .verify(b"challenge", &envelope)
            .unwrap());
        assert!(!key.verify(b"changed challenge", &envelope).unwrap());
        assert!(!Keypair::from_seed(&[44; 32], CRYPTO_HASHED)
            .unwrap()
            .verify(b"challenge", &envelope)
            .unwrap());
    }

    const PHRASE: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

    #[test]
    fn recovery_derivation_v1_is_pinned() {
        // Independently reproduced with RFC5869 HMAC-SHA256 extract/expand
        // and the ordinary sr25519 from_seed API. These values are a backup
        // compatibility contract, not a signature (sr25519 signing is randomized).
        let key = Keypair::from_seed(&[3; 32], CRYPTO_HASHED).unwrap();
        assert_eq!(
            key.ss58_address(),
            "5Du5D4jstmEn3QPytQLqgowz2huycvq7XvUFpY8u5PJTx6Dd"
        );
        assert_eq!(
            hex::encode(key.hashed_descriptor().unwrap()),
            "010123c2153ab6fc4d0a3f3cf4b2ffcb1bc0ecc5d6737ecb32addbc1f20fadb3223a"
        );
        assert_eq!(
            hex::encode(key.hashed_public_key().unwrap()),
            "4220dab7c75d2b0a2088639dc3d314d22f82493cfc86a0efa36e194bc5443612"
        );
        assert_eq!(
            hex::encode(key.at_generation(872).unwrap().hashed_public_key().unwrap()),
            "18885ee3441c25bd1c0ac0986959fcf33169ec2a6300e7a7a4572010463a7366"
        );
    }

    #[test]
    fn restore_any_generation_from_mnemonic_without_signing_history() {
        let wallet = Keypair::from_mnemonic(PHRASE, CRYPTO_HASHED, None).unwrap();
        assert_eq!(
            wallet.ss58_address(),
            "5ELvzckdbaShVHktHtSa7gH77AWawQSyfeRLXT1sbvquNjha"
        );
        let advanced = wallet.at_generation(872).unwrap();
        let restored = Keypair::from_mnemonic(PHRASE, CRYPTO_HASHED, None)
            .unwrap()
            .at_generation(872)
            .unwrap();
        assert_eq!(wallet.public_key_bytes(), advanced.public_key_bytes());
        assert_eq!(advanced.ss58_address(), restored.ss58_address());
        assert_eq!(
            advanced.hashed_public_key().unwrap(),
            restored.hashed_public_key().unwrap()
        );
        assert_eq!(
            advanced.hashed_next_commitment().unwrap(),
            restored.hashed_next_commitment().unwrap()
        );
        assert_eq!(wallet.hashed_generation().unwrap(), 0);
        assert_ne!(
            wallet.hashed_public_key().unwrap(),
            advanced.hashed_public_key().unwrap()
        );
        assert_ne!(
            wallet.hashed_current_commitment().unwrap(),
            advanced.hashed_current_commitment().unwrap()
        );
    }

    #[test]
    fn proof_binds_next_generation_and_full_transaction() {
        let key = Keypair::from_seed(&[9; 32], CRYPTO_HASHED)
            .unwrap()
            .at_generation(4)
            .unwrap();
        let implication = b"\x01call extensions genesis runtime versions";
        let encoded = key.sign_hashed(implication).unwrap();
        assert_eq!(encoded.len(), 136);
        let proof = Proof::decode(&mut &encoded[..]).unwrap();
        let payload = transaction_payload(
            &key.public_key_bytes(),
            Scheme::Sr25519,
            4,
            &proof.next_commitment,
            implication,
        );
        assert!(sr25519::Pair::verify(
            &sr25519::Signature::from_raw(proof.signature),
            payload,
            &sr25519::Public::from_raw(proof.public_key)
        ));
        for altered in [
            transaction_payload(
                &key.public_key_bytes(),
                Scheme::Sr25519,
                5,
                &proof.next_commitment,
                implication,
            ),
            transaction_payload(
                &key.public_key_bytes(),
                Scheme::Sr25519,
                4,
                &[0; 32],
                implication,
            ),
            transaction_payload(
                &[0; 32],
                Scheme::Sr25519,
                4,
                &proof.next_commitment,
                implication,
            ),
            transaction_payload(
                &key.public_key_bytes(),
                Scheme::Sr25519,
                4,
                &proof.next_commitment,
                b"different call",
            ),
        ] {
            assert!(!sr25519::Pair::verify(
                &sr25519::Signature::from_raw(proof.signature),
                altered,
                &sr25519::Public::from_raw(proof.public_key)
            ));
        }
    }

    #[test]
    fn restored_current_key_verifies_but_old_and_sibling_keys_do_not() {
        let original = Keypair::from_mnemonic(PHRASE, CRYPTO_HASHED, None).unwrap();
        let previous = original.at_generation(40).unwrap();
        let current = original.at_generation(41).unwrap();
        let recovered = Keypair::from_mnemonic(PHRASE, CRYPTO_HASHED, None)
            .unwrap()
            .at_generation(41)
            .unwrap();
        let implication = b"\x01restored device transaction";
        let proof = Proof::decode(&mut &recovered.sign_hashed(implication).unwrap()[..]).unwrap();
        let payload = transaction_payload(
            &current.public_key_bytes(),
            Scheme::Sr25519,
            proof.generation,
            &proof.next_commitment,
            implication,
        );
        assert_eq!(proof.generation, 41);
        assert_eq!(
            previous.hashed_next_commitment().unwrap(),
            current.hashed_current_commitment().unwrap()
        );
        assert_eq!(
            proof.next_commitment,
            original.hashed_commitment(42).unwrap()
        );
        let signature = sr25519::Signature::from_raw(proof.signature);
        assert!(sr25519::Pair::verify(
            &signature,
            payload,
            &sr25519::Public::from_raw(current.hashed_public_key().unwrap())
        ));
        let sibling = Keypair::from_seed(&[19; 32], CRYPTO_HASHED)
            .unwrap()
            .at_generation(41)
            .unwrap();
        for other in [previous, sibling] {
            assert_ne!(
                key_commitment(Scheme::Sr25519, &other.hashed_public_key().unwrap()),
                current.hashed_current_commitment().unwrap()
            );
            let other_proof =
                Proof::decode(&mut &other.sign_hashed(implication).unwrap()[..]).unwrap();
            let other_signature = sr25519::Signature::from_raw(other_proof.signature);
            // These are valid signatures from the old/sibling wallet, not
            // random invalid bytes. Neither controls the expected current key.
            let other_payload = transaction_payload(
                &other.public_key_bytes(),
                Scheme::Sr25519,
                other_proof.generation,
                &other_proof.next_commitment,
                implication,
            );
            assert!(sr25519::Pair::verify(
                &other_signature,
                other_payload,
                &sr25519::Public::from_raw(other_proof.public_key)
            ));
            let expected_payload = transaction_payload(
                &current.public_key_bytes(),
                Scheme::Sr25519,
                41,
                &other_proof.next_commitment,
                implication,
            );
            assert!(!sr25519::Pair::verify(
                &other_signature,
                expected_payload,
                &sr25519::Public::from_raw(current.hashed_public_key().unwrap())
            ));
        }
        // Signing alone does not claim chain acceptance or consume a counter.
        assert_eq!(recovered.hashed_generation().unwrap(), 41);
        let retry =
            Proof::decode(&mut &recovered.sign_hashed(b"replacement transaction").unwrap()[..])
                .unwrap();
        assert_eq!(retry.generation, 41);
        assert_eq!(retry.public_key, proof.public_key);
        assert_eq!(retry.next_commitment, proof.next_commitment);
    }

    #[test]
    fn recovery_requires_the_same_mnemonic_and_derivation_passphrase() {
        const OTHER_PHRASE: &str =
            "legal winner thank year wave sausage worth useful legal winner thank yellow";
        let original = Keypair::from_mnemonic(PHRASE, CRYPTO_HASHED, Some("correct passphrase"))
            .unwrap()
            .at_generation(73)
            .unwrap();
        let recovered = Keypair::from_mnemonic(PHRASE, CRYPTO_HASHED, Some("correct passphrase"))
            .unwrap()
            .at_generation(73)
            .unwrap();
        assert_eq!(
            recovered.hashed_descriptor().unwrap(),
            original.hashed_descriptor().unwrap()
        );
        assert_eq!(
            recovered.hashed_current_commitment().unwrap(),
            original.hashed_current_commitment().unwrap()
        );
        for incorrect in [
            Keypair::from_mnemonic(PHRASE, CRYPTO_HASHED, None).unwrap(),
            Keypair::from_mnemonic(PHRASE, CRYPTO_HASHED, Some("wrong passphrase")).unwrap(),
            Keypair::from_mnemonic(OTHER_PHRASE, CRYPTO_HASHED, Some("correct passphrase"))
                .unwrap(),
        ] {
            // BIP39 passphrases and other valid phrases create different
            // wallets; the chain commitment is what detects the wrong backup.
            assert_ne!(incorrect.ss58_address(), original.ss58_address());
            assert_ne!(
                incorrect.hashed_commitment(73).unwrap(),
                original.hashed_current_commitment().unwrap()
            );
        }
        assert!(Keypair::from_mnemonic("not a valid mnemonic", CRYPTO_HASHED, None).is_err());
    }

    #[test]
    fn final_usable_generation_commits_terminal_key_without_wrapping() {
        let wallet = Keypair::from_seed(&[23; 32], CRYPTO_HASHED).unwrap();
        let last = wallet.at_generation(u64::MAX - 1).unwrap();
        let terminal = wallet.at_generation(u64::MAX).unwrap();
        let proof =
            Proof::decode(&mut &last.sign_hashed(b"last authorization").unwrap()[..]).unwrap();
        assert_eq!(proof.generation, u64::MAX - 1);
        assert_eq!(
            proof.next_commitment,
            terminal.hashed_current_commitment().unwrap()
        );
        assert_ne!(
            proof.next_commitment,
            wallet.hashed_current_commitment().unwrap()
        );
        assert!(terminal.hashed_next_commitment().is_err());
        assert!(terminal
            .sign_hashed(b"must never wrap to generation zero")
            .is_err());
        assert_eq!(wallet.hashed_generation().unwrap(), 0);
    }

    #[test]
    fn public_descriptor_preserves_identity_without_exposing_signer() {
        let key = Keypair::from_seed(&[3; 32], CRYPTO_HASHED).unwrap();
        let public = key.public_only().unwrap();
        assert_eq!(key.public_key_bytes(), public.public_key_bytes());
        assert_eq!(
            key.hashed_descriptor().unwrap(),
            public.hashed_descriptor().unwrap()
        );
        assert_eq!(
            key.hashed_current_commitment().unwrap(),
            public.hashed_current_commitment().unwrap()
        );
        assert!(public.private_key_bytes().is_none());
        assert!(public.hashed_public_key().is_err());
        assert!(public.sign_hashed(b"message").is_err());
        assert!(key.sign(b"message").is_err());
    }

    #[test]
    fn descriptors_fail_closed_and_derivation_is_separated() {
        let key = Keypair::from_seed(&[3; 32], CRYPTO_HASHED).unwrap();
        let raw = Keypair::from_seed(&[3; 32], super::super::CRYPTO_SR25519).unwrap();
        assert_ne!(key.hashed_public_key().unwrap(), raw.public_key_bytes());
        for offset in [0, 1] {
            let mut descriptor = key.hashed_descriptor().unwrap();
            descriptor[offset] = 255;
            assert!(Keypair::from_hashed_descriptor(&descriptor, DEFAULT_SS58_FORMAT).is_err());
        }
        let mut trailing = key.hashed_descriptor().unwrap();
        trailing.push(0);
        assert!(Keypair::from_hashed_descriptor(&trailing, DEFAULT_SS58_FORMAT).is_err());
        assert!(Keypair::from_uri("//Alice", CRYPTO_HASHED).is_err());
    }
}
