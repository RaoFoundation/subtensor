//! Version-two ML-DSA wallets. Account identity and signing public keys are
//! deliberately separate: the former remains 32 bytes, the latter is 1952.

use codec::{Decode, Encode};
use hkdf::Hkdf;
use rand_core::{OsRng, RngCore};
use sha2::Sha512;
use subtensor_hashed::mldsa;
use zeroize::Zeroizing;

use super::{
    crypto_err, is_mldsa_crypto, Keypair, KeypairInner, CRYPTO_MLDSA65, CRYPTO_MLDSA65_FIXED,
};
use crate::error::CoreError;

const DERIVATION_SALT: &[u8] = b"bittensor/ms/v2/derive";
const CAPABILITY_DOMAIN: &[u8] = b"bittensor/ms/v2/capability";
pub const ROLE_TRANSACTION: u8 = 0;
pub const ROLE_ORDER: u8 = 1;
pub const ROLE_MESSAGE: u8 = 2;

pub struct MldsaKeypair {
    pub(super) descriptor: mldsa::Descriptor,
    master_seed: Option<Zeroizing<[u8; 32]>>,
    pub(super) generation: u64,
}

impl MldsaKeypair {
    pub(super) fn from_seed(seed: &[u8], crypto_type: u8) -> Result<Self, CoreError> {
        let master = Zeroizing::new(
            <[u8; 32]>::try_from(seed)
                .map_err(|_| crypto_err("ML-DSA master seed must be exactly 32 bytes"))?,
        );
        let profile = match crypto_type {
            CRYPTO_MLDSA65 => mldsa::ALGORITHM,
            CRYPTO_MLDSA65_FIXED => mldsa::FIXED_ALGORITHM,
            _ => return Err(crypto_err("unsupported ML-DSA account profile")),
        };
        let initial = Self::derive_seed_for_profile(&master, profile, ROLE_TRANSACTION, 0)?;
        let public = mldsa::public_key_from_seed(&initial).map_err(crypto_err)?;
        Ok(Self {
            descriptor: mldsa::Descriptor {
                version: mldsa::VERSION,
                algorithm: profile,
                initial_commitment: mldsa::commitment(&public),
            },
            master_seed: Some(master),
            generation: 0,
        })
    }

    #[cfg(test)]
    fn derive_seed(
        master: &[u8; 32],
        role: u8,
        index: u64,
    ) -> Result<Zeroizing<[u8; 32]>, CoreError> {
        Self::derive_seed_for_profile(master, mldsa::ALGORITHM, role, index)
    }

    fn derive_seed_for_profile(
        master: &[u8; 32],
        profile: u8,
        role: u8,
        index: u64,
    ) -> Result<Zeroizing<[u8; 32]>, CoreError> {
        if role > ROLE_MESSAGE {
            return Err(crypto_err("unsupported ML-DSA key role"));
        }
        let kdf = Hkdf::<Sha512>::new(Some(DERIVATION_SALT), master);
        let mut seed = Zeroizing::new([0; 32]);
        kdf.expand(
            &(mldsa::VERSION, profile, role, index).encode(),
            seed.as_mut(),
        )
        .map_err(|_| crypto_err("ML-DSA signing-key derivation failed"))?;
        Ok(seed)
    }

    fn seed_at(&self, role: u8, index: u64) -> Result<Zeroizing<[u8; 32]>, CoreError> {
        Self::derive_seed_for_profile(
            self.master_seed()
                .ok_or_else(|| crypto_err("ML-DSA derivation requires a private wallet"))?,
            self.descriptor.algorithm,
            role,
            if role == ROLE_TRANSACTION && !self.descriptor.is_rotating() {
                0
            } else {
                index
            },
        )
    }

    pub(super) fn master_seed(&self) -> Option<&[u8; 32]> {
        self.master_seed.as_deref()
    }
    pub(super) fn account_id(&self) -> [u8; 32] {
        mldsa::account_id(&self.descriptor)
    }
}

fn randomness() -> Result<Zeroizing<[u8; 32]>, CoreError> {
    let mut random = Zeroizing::new([0; 32]);
    OsRng
        .try_fill_bytes(random.as_mut())
        .map_err(|_| crypto_err("ML-DSA signing randomness unavailable"))?;
    Ok(random)
}

fn capability_context(role: u8) -> Result<&'static [u8], CoreError> {
    match role {
        ROLE_ORDER => Ok(b"bittensor/ms/v2/order"),
        ROLE_MESSAGE => Ok(b"bittensor/ms/v2/message"),
        _ => Err(crypto_err(
            "capability signing requires order or message role",
        )),
    }
}

/// Detached proofs are not self-authenticating: callers must resolve the
/// expected commitment and epoch from the owner's authorized chain state.
#[derive(Clone, Encode, Decode)]
pub struct MldsaCapabilityProof {
    pub version: u8,
    pub algorithm: u8,
    pub role: u8,
    pub owner: [u8; 32],
    pub genesis_hash: [u8; 32],
    pub epoch: u64,
    pub public_key: mldsa::PublicKeyBytes,
    pub signature: mldsa::SignatureBytes,
}

impl MldsaCapabilityProof {
    pub const ENCODED_LEN: usize = 5336;

    fn transcript(&self, payload: &[u8]) -> Vec<u8> {
        let mut bytes = CAPABILITY_DOMAIN.to_vec();
        (
            self.version,
            self.algorithm,
            self.role,
            self.owner,
            self.genesis_hash,
            self.epoch,
            self.public_key,
        )
            .encode_to(&mut bytes);
        bytes.extend_from_slice(payload);
        bytes
    }
}

/// Verify a complete offline proof against independently resolved authority.
/// This never accepts authority advertised only by the proof itself.
pub fn verify_mldsa_authorization(
    encoded: &[u8],
    payload: &[u8],
    descriptor: &[u8],
    generation: u64,
    expected_commitment: &[u8; 64],
) -> Result<bool, CoreError> {
    let descriptor = mldsa::Descriptor::from_bytes(descriptor)
        .map_err(|_| crypto_err("invalid ML-DSA descriptor"))?;
    let Ok(proof) = mldsa::SignerAddress::from_bytes(encoded) else {
        return Ok(false);
    };
    Ok(generation != u64::MAX
        && proof.account == mldsa::account_id(&descriptor)
        && proof.algorithm == descriptor.algorithm
        && proof.generation == generation
        && mldsa::commitment(&proof.public_key) == *expected_commitment
        && descriptor.accepts_next_commitment(expected_commitment, &proof.next_commitment)
        && proof.verify(payload))
}

pub fn verify_mldsa_capability(
    encoded: &[u8],
    payload: &[u8],
    owner: &[u8; 32],
    genesis_hash: &[u8; 32],
    role: u8,
    epoch: u64,
    expected_commitment: &[u8; 64],
) -> Result<bool, CoreError> {
    let context = capability_context(role)?;
    if encoded.len() != MldsaCapabilityProof::ENCODED_LEN {
        return Ok(false);
    }
    let mut input = encoded;
    let proof = MldsaCapabilityProof::decode(&mut input)
        .map_err(|_| crypto_err("invalid ML-DSA capability proof"))?;
    if !input.is_empty()
        || proof.version != mldsa::VERSION
        || proof.algorithm != mldsa::ALGORITHM
        || proof.role != role
        || &proof.owner != owner
        || &proof.genesis_hash != genesis_hash
        || proof.epoch != epoch
        || expected_commitment == &[0; 64]
        || mldsa::commitment(&proof.public_key) != *expected_commitment
    {
        return Ok(false);
    }
    Ok(mldsa::verify_message(
        &proof.public_key,
        &proof.transcript(payload),
        context,
        &proof.signature,
    ))
}

impl Keypair {
    fn mldsa(&self) -> Result<&MldsaKeypair, CoreError> {
        match &self.inner {
            KeypairInner::Mldsa(key) => Ok(key),
            _ => Err(crypto_err(
                "operation requires an ML-DSA account descriptor",
            )),
        }
    }

    pub fn from_mldsa_descriptor(encoded: &[u8], ss58_format: u16) -> Result<Self, CoreError> {
        let descriptor = mldsa::Descriptor::from_bytes(encoded)
            .map_err(|_| crypto_err("invalid ML-DSA account descriptor"))?;
        if !descriptor.is_supported() {
            return Err(crypto_err("unsupported ML-DSA account descriptor"));
        }
        Ok(Self {
            inner: KeypairInner::Mldsa(MldsaKeypair {
                descriptor,
                master_seed: None,
                generation: 0,
            }),
            ss58_format,
            mnemonic: None,
            seed: None,
        })
    }

    pub(super) fn mldsa_public_only(&self) -> Result<Self, CoreError> {
        let key = self.mldsa()?;
        Ok(Self {
            inner: KeypairInner::Mldsa(MldsaKeypair {
                descriptor: key.descriptor,
                master_seed: None,
                generation: key.generation,
            }),
            ss58_format: self.ss58_format,
            mnemonic: None,
            seed: None,
        })
    }

    pub(super) fn mldsa_at_generation(&self, generation: u64) -> Result<Self, CoreError> {
        let key = self.mldsa()?;
        if key.master_seed.is_none() {
            return Err(crypto_err(
                "ML-DSA generation selection requires a private wallet",
            ));
        }
        Ok(Self {
            inner: KeypairInner::Mldsa(MldsaKeypair {
                descriptor: key.descriptor,
                master_seed: key.master_seed.clone(),
                generation,
            }),
            ss58_format: self.ss58_format,
            mnemonic: self.mnemonic.clone(),
            seed: self.seed.clone(),
        })
    }

    pub fn mldsa_descriptor(&self) -> Result<Vec<u8>, CoreError> {
        Ok(self.mldsa()?.descriptor.encode())
    }
    pub fn mldsa_generation(&self) -> Result<u64, CoreError> {
        Ok(self.mldsa()?.generation)
    }
    pub fn mldsa_public_key(&self) -> Result<mldsa::PublicKeyBytes, CoreError> {
        let key = self.mldsa()?;
        mldsa::public_key_from_seed(&*key.seed_at(ROLE_TRANSACTION, key.generation)?)
            .map_err(crypto_err)
    }
    pub fn mldsa_commitment(&self, generation: u64) -> Result<[u8; 64], CoreError> {
        let key = self.mldsa()?;
        if generation == 0 || !key.descriptor.is_rotating() {
            return Ok(key.descriptor.initial_commitment);
        }
        Ok(mldsa::commitment(
            &mldsa::public_key_from_seed(&*key.seed_at(ROLE_TRANSACTION, generation)?)
                .map_err(crypto_err)?,
        ))
    }
    pub fn mldsa_current_commitment(&self) -> Result<[u8; 64], CoreError> {
        self.mldsa_commitment(self.mldsa_generation()?)
    }
    pub fn mldsa_next_commitment(&self) -> Result<[u8; 64], CoreError> {
        self.mldsa_commitment(
            self.mldsa_generation()?
                .checked_add(1)
                .ok_or_else(|| crypto_err("ML-DSA signing generation exhausted"))?,
        )
    }

    /// Returns the entire canonical MlDsaSignerAddress, not a MultiSignature payload.
    /// `payload` is the full unshortened native call + extra + implicit data.
    pub fn sign_mldsa(&self, payload: &[u8]) -> Result<Vec<u8>, CoreError> {
        self.sign_mldsa_with_randomness(payload, &*randomness()?)
    }

    /// Explicit deterministic input supports offline devices and golden vectors.
    pub fn sign_mldsa_with_randomness(
        &self,
        payload: &[u8],
        random: &[u8; 32],
    ) -> Result<Vec<u8>, CoreError> {
        let key = self.mldsa()?;
        let seed = key.seed_at(ROLE_TRANSACTION, key.generation)?;
        let header = mldsa::Header {
            account: key.account_id(),
            version: mldsa::VERSION,
            algorithm: key.descriptor.algorithm,
            generation: key.generation,
            public_key: mldsa::public_key_from_seed(&seed).map_err(crypto_err)?,
            next_commitment: self.mldsa_next_commitment()?,
        };
        Ok(header
            .sign(&seed, payload, random)
            .map_err(crypto_err)?
            .encode())
    }

    pub fn mldsa_capability_public_key(
        &self,
        role: u8,
        epoch: u64,
    ) -> Result<mldsa::PublicKeyBytes, CoreError> {
        capability_context(role)?;
        mldsa::public_key_from_seed(&*self.mldsa()?.seed_at(role, epoch)?).map_err(crypto_err)
    }
    pub fn mldsa_capability_commitment(&self, role: u8, epoch: u64) -> Result<[u8; 64], CoreError> {
        Ok(mldsa::commitment(
            &self.mldsa_capability_public_key(role, epoch)?,
        ))
    }
    pub fn sign_mldsa_capability(
        &self,
        role: u8,
        epoch: u64,
        genesis_hash: [u8; 32],
        payload: &[u8],
    ) -> Result<Vec<u8>, CoreError> {
        self.sign_mldsa_capability_for(self.public_key_bytes(), role, epoch, genesis_hash, payload)
    }

    /// Sign for an explicitly granted owner, including a composite account.
    /// Only the owner's trusted registry can authorize this purpose key.
    pub fn sign_mldsa_capability_for(
        &self,
        owner: [u8; 32],
        role: u8,
        epoch: u64,
        genesis_hash: [u8; 32],
        payload: &[u8],
    ) -> Result<Vec<u8>, CoreError> {
        if role != ROLE_MESSAGE {
            return Err(crypto_err(
                "order authorization requires sign_mldsa_order with its canonical order transcript",
            ));
        }
        let context = capability_context(role)?;
        let seed = self.mldsa()?.seed_at(role, epoch)?;
        let mut proof = MldsaCapabilityProof {
            version: mldsa::VERSION,
            algorithm: mldsa::ALGORITHM,
            role,
            owner,
            genesis_hash,
            epoch,
            public_key: mldsa::public_key_from_seed(&seed).map_err(crypto_err)?,
            signature: [0; mldsa::SIGNATURE_LEN],
        };
        proof.signature =
            mldsa::sign_message(&seed, &proof.transcript(payload), context, &*randomness()?)
                .map_err(crypto_err)?;
        Ok(proof.encode())
    }

    /// Sign the runtime's exact detached-order transcript. `order` must be the
    /// complete canonical SCALE VersionedOrder, including its version tag.
    /// The registry's owner, epoch, expiry and scope are checked at execution.
    pub fn sign_mldsa_order(
        &self,
        epoch: u64,
        nonce: u64,
        genesis_hash: [u8; 32],
        order: &[u8],
    ) -> Result<mldsa::SignatureBytes, CoreError> {
        self.sign_mldsa_order_for(self.public_key_bytes(), epoch, nonce, genesis_hash, order)
    }

    /// The issuer may be a direct, proxy, derivative or multisig owner. The
    /// current owner-issued capability must be verified separately.
    pub fn sign_mldsa_order_for(
        &self,
        owner: [u8; 32],
        epoch: u64,
        nonce: u64,
        genesis_hash: [u8; 32],
        order: &[u8],
    ) -> Result<mldsa::SignatureBytes, CoreError> {
        let seed = self.mldsa()?.seed_at(ROLE_ORDER, epoch)?;
        let context = capability_context(ROLE_ORDER)?;
        let mut transcript = context.to_vec();
        (genesis_hash, owner, epoch, nonce).encode_to(&mut transcript);
        transcript.extend_from_slice(order);
        mldsa::sign_message(&seed, &transcript, context, &*randomness()?).map_err(crypto_err)
    }

    /// Variable-width authorization APIs support v1 and ML-DSA without changing
    /// the historical fixed-width Rust v1 APIs.
    pub fn authorization_descriptor(&self) -> Result<Vec<u8>, CoreError> {
        if is_mldsa_crypto(self.crypto_type()) {
            self.mldsa_descriptor()
        } else {
            self.hashed_descriptor()
        }
    }
    pub fn authorization_generation(&self) -> Result<u64, CoreError> {
        if is_mldsa_crypto(self.crypto_type()) {
            self.mldsa_generation()
        } else {
            self.hashed_generation()
        }
    }
    pub fn authorization_public_key(&self) -> Result<Vec<u8>, CoreError> {
        if is_mldsa_crypto(self.crypto_type()) {
            Ok(self.mldsa_public_key()?.to_vec())
        } else {
            self.hashed_signing_public_key()
        }
    }
    pub fn authorization_commitment(&self, generation: u64) -> Result<Vec<u8>, CoreError> {
        if is_mldsa_crypto(self.crypto_type()) {
            Ok(self.mldsa_commitment(generation)?.to_vec())
        } else {
            Ok(self.hashed_commitment(generation)?.to_vec())
        }
    }
    pub fn authorization_current_commitment(&self) -> Result<Vec<u8>, CoreError> {
        self.authorization_commitment(self.authorization_generation()?)
    }
    pub fn authorization_next_commitment(&self) -> Result<Vec<u8>, CoreError> {
        self.authorization_commitment(
            self.authorization_generation()?
                .checked_add(1)
                .ok_or_else(|| crypto_err("signing generation exhausted"))?,
        )
    }
    pub fn sign_authorization(&self, payload: &[u8]) -> Result<Vec<u8>, CoreError> {
        if is_mldsa_crypto(self.crypto_type()) {
            self.sign_mldsa(payload)
        } else {
            self.sign_hashed(payload)
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::keys::{
        decode_hashed_receiving_address, encode_hashed_receiving_address, CRYPTO_HASHED,
        CRYPTO_SR25519,
    };

    #[test]
    fn hkdf_sha512_vectors_are_independent_by_role_and_generation() {
        // Independently computed with Python hashlib/hmac, not this KDF.
        for (role, index, expected) in [
            (
                0,
                0,
                "ee5932d8d7f72ace8a35e6a7adaf0ddc3f247fb361e4a5002653af9fbc30675d",
            ),
            (
                0,
                41,
                "226f863895c02488668a98d3bddf3796368b9616de8cea8143c65a9ee92ca379",
            ),
            (
                1,
                41,
                "4709cbcf98a580847ecae2ae32f41c2433bc1dbda286d2bc940fd635d6bd3c3f",
            ),
            (
                2,
                41,
                "4c916de3bda849119afa5df40b033cd4024d58a143ac085196591a0ca2ce9e06",
            ),
        ] {
            assert_eq!(
                hex::encode(*MldsaKeypair::derive_seed(&[7; 32], role, index).unwrap()),
                expected
            );
        }
        // Fixed-profile vectors independently computed with Python HMAC-SHA512.
        for (role, index, expected) in [
            (
                0,
                0,
                "753832bab3583b97195c3878523e70d03a5695675a62c6527a004ef1957cb736",
            ),
            (
                1,
                41,
                "df24c8ed33762468c68c864987a186a7122277293e960487014be451e644d9c4",
            ),
            (
                2,
                41,
                "99e02f7f74a0a312d3ec9df66442c6942b651474e19ec56e043ad5a4be772e6d",
            ),
        ] {
            assert_eq!(
                hex::encode(
                    *MldsaKeypair::derive_seed_for_profile(
                        &[7; 32],
                        mldsa::FIXED_ALGORITHM,
                        role,
                        index
                    )
                    .unwrap()
                ),
                expected
            );
        }
        assert!(MldsaKeypair::derive_seed(&[7; 32], 3, 0).is_err());
    }

    #[test]
    fn mnemonic_convention_requires_256_bits_and_supports_passphrase_restore() {
        let phrase = [vec!["abandon"; 23], vec!["art"]].concat().join(" ");
        let wallet = Keypair::from_mnemonic(&phrase, CRYPTO_MLDSA65, Some("TREZOR")).unwrap();
        assert_eq!(
            hex::encode(wallet.seed_bytes().unwrap()),
            "bda85446c68413707090a52022edd26a1c9462295029f2e60cd7c4f2bbd30971"
        );
        assert!(wallet.mnemonic().is_none());
        let restored = Keypair::from_seed(wallet.seed_bytes().unwrap(), CRYPTO_MLDSA65).unwrap();
        assert_eq!(
            wallet.mldsa_descriptor().unwrap(),
            restored.mldsa_descriptor().unwrap()
        );
        assert_ne!(
            wallet.public_key_bytes(),
            Keypair::from_mnemonic(&phrase, CRYPTO_MLDSA65, None)
                .unwrap()
                .public_key_bytes()
        );
        assert!(Keypair::from_mnemonic("abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about", CRYPTO_MLDSA65, None).is_err());
    }

    #[test]
    fn rotation_restore_keeps_identity_and_signs_complete_payload() {
        let wallet = Keypair::from_seed(&[7; 32], CRYPTO_MLDSA65).unwrap();
        let active = wallet.at_generation(41).unwrap();
        let restored = Keypair::from_seed(&[7; 32], CRYPTO_MLDSA65)
            .unwrap()
            .at_generation(41)
            .unwrap();
        assert_eq!(wallet.public_key_bytes(), active.public_key_bytes());
        assert_eq!(
            active.mldsa_current_commitment().unwrap(),
            restored.mldsa_current_commitment().unwrap()
        );
        assert_ne!(
            wallet.mldsa_public_key().unwrap(),
            active.mldsa_public_key().unwrap()
        );
        let payload = vec![19; 500];
        let encoded = active
            .sign_mldsa_with_randomness(&payload, &[0; 32])
            .unwrap();
        assert_eq!(encoded.len(), 5367);
        let proof = mldsa::SignerAddress::from_bytes(&encoded).unwrap();
        assert!(proof.verify(&payload));
        let descriptor = wallet.mldsa_descriptor().unwrap();
        let commitment = active.mldsa_current_commitment().unwrap();
        assert!(
            verify_mldsa_authorization(&encoded, &payload, &descriptor, 41, &commitment).unwrap()
        );
        assert!(
            !verify_mldsa_authorization(&encoded, &payload, &descriptor, 42, &commitment).unwrap()
        );
        assert!(
            !verify_mldsa_authorization(&encoded, &payload, &descriptor, 41, &[0; 64]).unwrap()
        );
        assert!(!verify_mldsa_authorization(
            &encoded[..5366],
            &payload,
            &descriptor,
            41,
            &commitment
        )
        .unwrap());
        assert_eq!(proof.account, wallet.public_key_bytes());
        assert_eq!(
            proof.next_commitment,
            restored.mldsa_commitment(42).unwrap()
        );
        assert!(!proof.verify(&sp_core::hashing::blake2_256(&payload)));
        let mut wrong = proof.clone();
        wrong.account[0] ^= 1;
        assert!(!wrong.verify(&payload));
        let mut wrong = proof.clone();
        wrong.next_commitment[0] ^= 1;
        assert!(!wrong.verify(&payload));
        let mut wrong = proof;
        wrong.generation += 1;
        assert!(!wrong.verify(&payload));
        assert_eq!(active.mldsa_generation().unwrap(), 41);
        assert!(wallet
            .at_generation(u64::MAX)
            .unwrap()
            .sign_mldsa(b"no wrap")
            .is_err());
    }

    #[test]
    fn offline_authorization_rejects_cryptographically_valid_unusable_rotations() {
        let wallet = Keypair::from_seed(&[7; 32], CRYPTO_MLDSA65).unwrap();
        let descriptor = wallet.mldsa_descriptor().unwrap();
        let payload = vec![19; 500];
        let current = wallet.mldsa_commitment(7).unwrap();
        let next = wallet.mldsa_commitment(8).unwrap();
        for (generation, next_commitment, accepted) in [
            (7, next, true),
            (7, [0; 64], false),
            (7, current, false),
            (u64::MAX, next, false),
            (
                u64::MAX - 1,
                wallet.mldsa_commitment(u64::MAX).unwrap(),
                true,
            ),
        ] {
            let active = wallet.at_generation(generation).unwrap();
            let commitment = active.mldsa_current_commitment().unwrap();
            let signing_seed = active
                .mldsa()
                .unwrap()
                .seed_at(ROLE_TRANSACTION, generation)
                .unwrap();
            // Model an external signer that does not enforce our wallet's
            // rotation policy. Every malformed header is correctly re-signed;
            // rejection must come from authorization guards, not bad crypto.
            let proof = mldsa::Header {
                account: wallet.public_key_bytes(),
                version: mldsa::VERSION,
                algorithm: mldsa::ALGORITHM,
                generation,
                public_key: active.mldsa_public_key().unwrap(),
                next_commitment,
            }
            .sign(&signing_seed, &payload, &[0; 32])
            .unwrap();
            assert!(proof.verify(&payload));
            if generation == 7 && next_commitment == [0; 64] {
                // Shared with the Python offline-attachment regression. Keep
                // the fixture cryptographically valid so a corrupted proof
                // cannot accidentally make the policy rejection test pass.
                if let Ok(path) = std::env::var("MLDSA_ZERO_NEXT_FIXTURE") {
                    std::fs::write(path, hex::encode(proof.encode())).unwrap();
                } else {
                    assert_eq!(
                        proof.encode(),
                        hex::decode(
                            include_str!(
                                "../../../python/tests/fixtures/mldsa-zero-next-proof.hex"
                            )
                            .trim()
                        )
                        .unwrap(),
                    );
                }
            }
            assert_eq!(
                verify_mldsa_authorization(
                    &proof.encode(),
                    &payload,
                    &descriptor,
                    generation,
                    &commitment,
                )
                .unwrap(),
                accepted,
                "generation {generation}, next commitment {next_commitment:?}",
            );
        }
    }

    #[test]
    fn watch_only_keeps_descriptor_but_never_signs_or_reveals_keys() {
        let wallet = Keypair::from_seed(&[7; 32], CRYPTO_MLDSA65).unwrap();
        let public = wallet.public_only().unwrap();
        assert_eq!(
            wallet.mldsa_descriptor().unwrap(),
            public.mldsa_descriptor().unwrap()
        );
        assert_eq!(wallet.public_key_bytes(), public.public_key_bytes());
        assert!(public.private_key_bytes().is_none());
        assert!(public.mldsa_public_key().is_err());
        assert!(public.sign_mldsa(b"no private material").is_err());
        assert!(wallet.sign(b"must name capability").is_err());
        assert!(Keypair::from_uri("//Alice", CRYPTO_MLDSA65).is_err());
        for crypto in [CRYPTO_HASHED, CRYPTO_SR25519] {
            assert_ne!(
                wallet.public_key_bytes(),
                Keypair::from_seed(&[7; 32], crypto)
                    .unwrap()
                    .public_key_bytes()
            );
        }
    }

    #[test]
    fn receiving_mldsa_vector_checksum_and_canonical_spelling() {
        let descriptor = mldsa::Descriptor {
            version: 2,
            algorithm: 4,
            initial_commitment: [7; 64],
        }
        .encode();
        let address = encode_hashed_receiving_address(&descriptor).unwrap();
        assert_eq!(
            address,
            "tao_AgQHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHhs8-H4mAmmA"
        );
        assert_eq!(address.len(), 103);
        for spelling in [address.clone(), address.replacen("tao_", "bth2_", 1)] {
            let decoded = decode_hashed_receiving_address(&spelling).unwrap();
            assert_eq!(decoded, descriptor);
            assert_eq!(encode_hashed_receiving_address(&decoded).unwrap(), address);
            for invalid in [
                format!("{spelling}="),
                format!(" {spelling}"),
                format!("{spelling}\n"),
                spelling.to_uppercase(),
                spelling[..spelling.len() - 1].to_string(),
            ] {
                assert!(decode_hashed_receiving_address(&invalid).is_err());
            }
            let mut bytes = spelling.into_bytes();
            bytes[30] = if bytes[30] == b'A' { b'B' } else { b'A' };
            assert!(decode_hashed_receiving_address(std::str::from_utf8(&bytes).unwrap()).is_err());
        }
        assert!(decode_hashed_receiving_address(&address.replacen("tao_", "bth1_", 1)).is_err());
    }

    #[test]
    fn capability_proofs_require_trusted_owner_role_epoch_and_commitment() {
        let wallet = Keypair::from_seed(&[7; 32], CRYPTO_MLDSA65).unwrap();
        let commitment = wallet.mldsa_capability_commitment(ROLE_MESSAGE, 3).unwrap();
        assert_ne!(commitment, wallet.mldsa_commitment(3).unwrap());
        assert_ne!(
            commitment,
            wallet.mldsa_capability_commitment(ROLE_ORDER, 3).unwrap()
        );
        let proof = wallet
            .sign_mldsa_capability(ROLE_MESSAGE, 3, [9; 32], b"audience/nonce/expiry/body")
            .unwrap();
        let verify = |role, epoch, owner, genesis, commitment| {
            verify_mldsa_capability(
                &proof,
                b"audience/nonce/expiry/body",
                &owner,
                &genesis,
                role,
                epoch,
                &commitment,
            )
            .unwrap()
        };
        assert!(verify(
            ROLE_MESSAGE,
            3,
            wallet.public_key_bytes(),
            [9; 32],
            commitment
        ));
        assert!(!verify(
            ROLE_MESSAGE,
            4,
            wallet.public_key_bytes(),
            [9; 32],
            commitment
        ));
        assert!(!verify(
            ROLE_ORDER,
            3,
            wallet.public_key_bytes(),
            [9; 32],
            commitment
        ));
        assert!(!verify(ROLE_MESSAGE, 3, [0; 32], [9; 32], commitment));
        assert!(!verify(
            ROLE_MESSAGE,
            3,
            wallet.public_key_bytes(),
            [8; 32],
            commitment
        ));
        assert!(!verify(
            ROLE_MESSAGE,
            3,
            wallet.public_key_bytes(),
            [9; 32],
            [0; 64]
        ));
        assert!(wallet
            .sign_mldsa_capability(ROLE_TRANSACTION, 0, [9; 32], b"transaction")
            .is_err());
    }

    #[test]
    fn order_signature_binds_runtime_transcript_and_never_uses_transaction_key() {
        let wallet = Keypair::from_seed(&[7; 32], CRYPTO_MLDSA65).unwrap();
        let public = wallet.mldsa_capability_public_key(ROLE_ORDER, 3).unwrap();
        let order = b"canonical SCALE VersionedOrder";
        let signature = wallet.sign_mldsa_order(3, 11, [9; 32], order).unwrap();
        let mut transcript = b"bittensor/ms/v2/order".to_vec();
        ([9_u8; 32], wallet.public_key_bytes(), 3_u64, 11_u64).encode_to(&mut transcript);
        transcript.extend_from_slice(order);
        assert!(mldsa::verify_message(
            &public,
            &transcript,
            b"bittensor/ms/v2/order",
            &signature
        ));
        assert!(!mldsa::verify_message(
            &wallet.mldsa_public_key().unwrap(),
            &transcript,
            b"bittensor/ms/v2/order",
            &signature
        ));
        assert!(!mldsa::verify_message(
            &public,
            &transcript,
            b"bittensor/ms/v2/message",
            &signature
        ));
        transcript[21 + 32 + 32 + 8] ^= 1;
        assert!(!mldsa::verify_message(
            &public,
            &transcript,
            b"bittensor/ms/v2/order",
            &signature
        ));
        assert!(wallet
            .sign_mldsa_capability(ROLE_ORDER, 3, [9; 32], order)
            .is_err());
    }

    #[test]
    fn delegated_purpose_keys_bind_the_explicit_composite_owner() {
        let wallet = Keypair::from_seed(&[7; 32], CRYPTO_MLDSA65).unwrap();
        let composite = [23; 32];
        let proof = wallet
            .sign_mldsa_capability_for(composite, ROLE_MESSAGE, 3, [9; 32], b"body")
            .unwrap();
        let commitment = wallet.mldsa_capability_commitment(ROLE_MESSAGE, 3).unwrap();
        assert!(verify_mldsa_capability(
            &proof,
            b"body",
            &composite,
            &[9; 32],
            ROLE_MESSAGE,
            3,
            &commitment
        )
        .unwrap());
        assert!(!verify_mldsa_capability(
            &proof,
            b"body",
            &wallet.public_key_bytes(),
            &[9; 32],
            ROLE_MESSAGE,
            3,
            &commitment
        )
        .unwrap());
        let signature = wallet
            .sign_mldsa_order_for(composite, 3, 11, [9; 32], b"order")
            .unwrap();
        let mut transcript = b"bittensor/ms/v2/order".to_vec();
        ([9_u8; 32], composite, 3_u64, 11_u64).encode_to(&mut transcript);
        transcript.extend_from_slice(b"order");
        assert!(mldsa::verify_message(
            &wallet.mldsa_capability_public_key(ROLE_ORDER, 3).unwrap(),
            &transcript,
            b"bittensor/ms/v2/order",
            &signature
        ));
        transcript[21] ^= 1;
        assert!(!mldsa::verify_message(
            &wallet.mldsa_capability_public_key(ROLE_ORDER, 3).unwrap(),
            &transcript,
            b"bittensor/ms/v2/order",
            &signature
        ));
    }

    #[test]
    fn order_signature_matches_independent_runtime_wire_fixture() {
        let wallet = Keypair::from_seed(&[7; 32], CRYPTO_MLDSA65).unwrap();
        let order = include_bytes!(
            "../../../../pallets/limit-orders/tests/fixtures/authorized-order.scale"
        );
        let transcript = include_bytes!(
            "../../../../pallets/limit-orders/tests/fixtures/authorized-order-transcript.bin"
        );
        let signature = wallet
            .sign_mldsa_order_for([0x11; 32], 3, 42, [0x44; 32], order)
            .unwrap();
        assert!(mldsa::verify_message(
            &wallet.mldsa_capability_public_key(ROLE_ORDER, 3).unwrap(),
            transcript,
            b"bittensor/ms/v2/order",
            &signature
        ));
    }

    #[cfg(feature = "host")]
    #[test]
    fn versioned_private_and_public_keyfiles_preserve_mldsa() {
        use crate::keyfiles::{
            deserialize_keypair_from_keyfile_data, serialized_keypair_to_keyfile_data,
        };
        for crypto in [CRYPTO_MLDSA65, CRYPTO_MLDSA65_FIXED] {
            let wallet = Keypair::from_seed(&[7; 32], crypto).unwrap();
            for key in [wallet.public_only().unwrap(), wallet] {
                let bytes = serialized_keypair_to_keyfile_data(&key).unwrap();
                let restored = deserialize_keypair_from_keyfile_data(&bytes).unwrap();
                assert_eq!(
                    key.mldsa_descriptor().unwrap(),
                    restored.mldsa_descriptor().unwrap()
                );
                assert_eq!(
                    key.private_key_bytes().is_some(),
                    restored.private_key_bytes().is_some()
                );
                let mut json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                json["cryptoType"] = serde_json::json!(if crypto == CRYPTO_MLDSA65 {
                    CRYPTO_MLDSA65_FIXED
                } else {
                    CRYPTO_MLDSA65
                });
                assert!(
                    deserialize_keypair_from_keyfile_data(&serde_json::to_vec(&json).unwrap())
                        .is_err()
                );
            }
        }
    }
    #[test]
    fn fixed_profile_is_separate_replay_safe_and_cannot_rotate_or_downgrade() {
        let fixed = Keypair::from_seed(&[19; 32], CRYPTO_MLDSA65_FIXED).unwrap();
        let rotating = Keypair::from_seed(&[19; 32], CRYPTO_MLDSA65).unwrap();
        assert_eq!(fixed.account_type(), "standard");
        assert_ne!(fixed.public_key_bytes(), rotating.public_key_bytes());
        assert_ne!(
            fixed.mldsa_public_key().unwrap(),
            rotating.mldsa_public_key().unwrap()
        );
        assert_ne!(
            fixed.mldsa_capability_public_key(ROLE_MESSAGE, 0).unwrap(),
            rotating
                .mldsa_capability_public_key(ROLE_MESSAGE, 0)
                .unwrap()
        );
        let initial = fixed.mldsa_current_commitment().unwrap();
        let public = fixed.mldsa_public_key().unwrap();
        for generation in [0, 1, 9, u64::MAX - 1] {
            let key = fixed.at_generation(generation).unwrap();
            assert_eq!(key.mldsa_public_key().unwrap(), public);
            assert_eq!(key.mldsa_next_commitment().unwrap(), initial);
            let proof = key.sign_mldsa(b"full call and extensions").unwrap();
            assert!(verify_mldsa_authorization(
                &proof,
                b"full call and extensions",
                &fixed.mldsa_descriptor().unwrap(),
                generation,
                &initial
            )
            .unwrap());
            assert!(!verify_mldsa_authorization(
                &proof,
                b"full call and extensions",
                &fixed.mldsa_descriptor().unwrap(),
                generation + 1,
                &initial
            )
            .unwrap());
            assert!(!verify_mldsa_authorization(
                &proof,
                b"full call and extensions",
                &rotating.mldsa_descriptor().unwrap(),
                generation,
                &initial
            )
            .unwrap());
        }
        assert_ne!(
            rotating
                .at_generation(1)
                .unwrap()
                .mldsa_public_key()
                .unwrap(),
            rotating.mldsa_public_key().unwrap()
        );
        let private = fixed.mldsa().unwrap().seed_at(ROLE_TRANSACTION, 0).unwrap();
        for next_commitment in [[0; 64], [9; 64]] {
            let header = mldsa::Header {
                account: fixed.public_key_bytes(),
                version: 2,
                algorithm: mldsa::FIXED_ALGORITHM,
                generation: 0,
                public_key: public,
                next_commitment,
            };
            let proof = header.sign(&private, b"call", &[0; 32]).unwrap();
            assert!(proof.verify(b"call"));
            assert!(!verify_mldsa_authorization(
                &proof.encode(),
                b"call",
                &fixed.mldsa_descriptor().unwrap(),
                0,
                &initial
            )
            .unwrap());
        }
        let watch = fixed.public_only().unwrap();
        assert_eq!(watch.crypto_type(), CRYPTO_MLDSA65_FIXED);
        assert_eq!(watch.mldsa_commitment(12).unwrap(), initial);
        assert!(watch.sign_mldsa(b"call").is_err());
        let receiving =
            encode_hashed_receiving_address(&fixed.mldsa_descriptor().unwrap()).unwrap();
        let decoded = decode_hashed_receiving_address(&receiving).unwrap();
        assert_eq!(decoded, fixed.mldsa_descriptor().unwrap());
    }
}
