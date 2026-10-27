//! Descriptor-based post-quantum wallets with immutable recovery profiles.
//! Account identities remain 32 bytes; signing keys have algorithm-specific widths.
//! Historical ML-DSA method names remain available for client compatibility.

use codec::{Decode, Encode};
use hkdf::Hkdf;
use rand_core::{OsRng, RngCore};
use sha2::Sha512;
use subtensor_hashed::mldsa;
use zeroize::Zeroizing;

use super::{crypto_err, falcon, is_pq_crypto, Keypair, KeypairInner};
#[cfg(test)]
use super::{CRYPTO_MLDSA65, CRYPTO_MLDSA65_FIXED};
use crate::error::CoreError;

const DERIVATION_SALT: &[u8] = b"bittensor/ms/v2/derive";
const CAPABILITY_DOMAIN: &[u8] = b"bittensor/ms/v2/capability";
/// Bound wallet key generation independently of runtime transaction validation.
const STATELESS_KEY_SEARCH_LIMIT: u64 = 256;
pub const ROLE_TRANSACTION: u8 = 0;
pub const ROLE_ORDER: u8 = 1;
pub const ROLE_MESSAGE: u8 = 2;

pub struct MldsaKeypair {
    pub(super) descriptor: mldsa::Descriptor,
    master_seed: Option<Zeroizing<[u8; 32]>>,
    /// Recomputed from the master seed on recovery; never a mutable key counter.
    transaction_index: u64,
    pub(super) generation: u64,
}

impl MldsaKeypair {
    pub(super) fn from_seed(seed: &[u8], crypto_type: u8) -> Result<Self, CoreError> {
        let profile = super::pq_profile(crypto_type)
            .ok_or_else(|| crypto_err("unsupported post-quantum account profile"))?;
        let master = Zeroizing::new(
            <[u8; 32]>::try_from(seed)
                .map_err(|_| crypto_err("post-quantum master seed must be exactly 32 bytes"))?,
        );
        let (transaction_index, initial_commitment) = if super::is_fixed_pq_crypto(crypto_type) {
            Self::stateless_transaction_key_for_profile(
                &master,
                profile,
                STATELESS_KEY_SEARCH_LIMIT,
            )?
        } else {
            let initial = Self::derive_seed_for_profile(&master, profile, ROLE_TRANSACTION, 0)?;
            let public = falcon::public_key(profile, &initial)?;
            (0, falcon::commitment(profile, &public)?)
        };
        Ok(Self {
            descriptor: mldsa::Descriptor {
                version: mldsa::VERSION,
                algorithm: profile,
                initial_commitment,
            },
            master_seed: Some(master),
            transaction_index,
            generation: 0,
        })
    }

    #[cfg(test)]
    fn stateless_transaction_key(
        master: &[u8; 32],
        limit: u64,
    ) -> Result<(u64, mldsa::Commitment), CoreError> {
        Self::stateless_transaction_key_for_profile(master, mldsa::FIXED_ALGORITHM, limit)
    }

    fn stateless_transaction_key_for_profile(
        master: &[u8; 32],
        profile: u8,
        limit: u64,
    ) -> Result<(u64, mldsa::Commitment), CoreError> {
        for index in 0..limit {
            let seed = Self::derive_seed_for_profile(master, profile, ROLE_TRANSACTION, index)?;
            let public = falcon::public_key(profile, &seed)?;
            let initial_commitment = falcon::commitment(profile, &public)?;
            let descriptor = mldsa::Descriptor {
                version: mldsa::VERSION,
                algorithm: profile,
                initial_commitment,
            };
            if initial_commitment != [0; mldsa::COMMITMENT_LEN]
                && mldsa::rejects_legacy_public_keys(&mldsa::account_id(&descriptor))
            {
                return Ok((index, initial_commitment));
            }
        }
        Err(crypto_err(
            "post-quantum account derivation exhausted its bounded key search",
        ))
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
            return Err(crypto_err("unsupported post-quantum key role"));
        }
        let salt = if subtensor_hashed::falcon::is_account_profile(profile) {
            b"bittensor/falcon/v1/derive".as_slice()
        } else {
            DERIVATION_SALT
        };
        let kdf = Hkdf::<Sha512>::new(Some(salt), master);
        let mut seed = Zeroizing::new([0; 32]);
        kdf.expand(
            &(mldsa::VERSION, profile, role, index).encode(),
            seed.as_mut(),
        )
        .map_err(|_| crypto_err("post-quantum signing-key derivation failed"))?;
        Ok(seed)
    }

    pub(super) fn seed_at(&self, role: u8, index: u64) -> Result<Zeroizing<[u8; 32]>, CoreError> {
        Self::derive_seed_for_profile(
            self.master_seed()
                .ok_or_else(|| crypto_err("post-quantum derivation requires a private wallet"))?,
            self.descriptor.algorithm,
            role,
            if role == ROLE_TRANSACTION && !self.descriptor.is_rotating() {
                self.transaction_index
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

pub(super) fn randomness() -> Result<Zeroizing<[u8; 32]>, CoreError> {
    let mut random = Zeroizing::new([0; 32]);
    OsRng
        .try_fill_bytes(random.as_mut())
        .map_err(|_| crypto_err("post-quantum signing randomness unavailable"))?;
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
        .map_err(|_| crypto_err("invalid post-quantum descriptor"))?;
    if subtensor_hashed::falcon::is_account_profile(descriptor.algorithm) {
        let Ok(proof) = subtensor_hashed::falcon::SignerAddress::from_bytes(encoded) else {
            return Ok(false);
        };
        return Ok(generation != u64::MAX
            && (!descriptor.is_stateless() || generation == 0)
            && proof.account == mldsa::account_id(&descriptor)
            && proof.algorithm == descriptor.algorithm
            && proof.generation == generation
            && falcon::commitment(proof.algorithm, &proof.public_key)? == *expected_commitment
            && descriptor.accepts_next_commitment(expected_commitment, &proof.next_commitment)
            && proof.verify(payload));
    }
    let Ok(proof) = mldsa::SignerAddress::from_bytes(encoded) else {
        return Ok(false);
    };
    Ok(generation != u64::MAX
        && (!descriptor.is_stateless() || generation == 0)
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
    if encoded
        .get(1)
        .is_some_and(|profile| subtensor_hashed::falcon::is_account_profile(*profile))
    {
        return falcon::verify_capability(
            encoded,
            payload,
            owner,
            genesis_hash,
            role,
            epoch,
            expected_commitment,
        );
    }
    let context = capability_context(role)?;
    if encoded.len() != MldsaCapabilityProof::ENCODED_LEN {
        return Ok(false);
    }
    let mut input = encoded;
    let proof = MldsaCapabilityProof::decode(&mut input)
        .map_err(|_| crypto_err("invalid post-quantum capability proof"))?;
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
    pub(super) fn mldsa(&self) -> Result<&MldsaKeypair, CoreError> {
        match &self.inner {
            KeypairInner::Mldsa(key) => Ok(key),
            _ => Err(crypto_err(
                "operation requires a post-quantum account descriptor",
            )),
        }
    }

    pub fn from_mldsa_descriptor(encoded: &[u8], ss58_format: u16) -> Result<Self, CoreError> {
        let descriptor = mldsa::Descriptor::from_bytes(encoded)
            .map_err(|_| crypto_err("invalid post-quantum account descriptor"))?;
        if !descriptor.is_supported() {
            return Err(crypto_err("unsupported post-quantum account descriptor"));
        }
        Ok(Self {
            inner: KeypairInner::Mldsa(MldsaKeypair {
                descriptor,
                master_seed: None,
                transaction_index: 0,
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
                transaction_index: key.transaction_index,
                generation: key.generation,
            }),
            ss58_format: self.ss58_format,
            mnemonic: None,
            seed: None,
        })
    }

    pub(super) fn mldsa_at_generation(&self, generation: u64) -> Result<Self, CoreError> {
        let key = self.mldsa()?;
        if key.descriptor.is_stateless() && generation != 0 {
            return Err(crypto_err(
                "registration-free post-quantum accounts have no signing generation",
            ));
        }
        if key.master_seed.is_none() {
            return Err(crypto_err(
                "post-quantum generation selection requires a private wallet",
            ));
        }
        Ok(Self {
            inner: KeypairInner::Mldsa(MldsaKeypair {
                descriptor: key.descriptor,
                master_seed: key.master_seed.clone(),
                transaction_index: key.transaction_index,
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
    pub fn mldsa_profile(&self) -> Result<u8, CoreError> {
        Ok(self.mldsa()?.descriptor.algorithm)
    }
    pub fn mldsa_generation(&self) -> Result<u64, CoreError> {
        Ok(self.mldsa()?.generation)
    }
    pub fn mldsa_public_key(&self) -> Result<mldsa::PublicKeyBytes, CoreError> {
        let key = self.mldsa()?;
        if subtensor_hashed::falcon::is_account_profile(key.descriptor.algorithm) {
            return Err(crypto_err("use pq_public_key for Falcon public keys"));
        }
        mldsa::public_key_from_seed(&*key.seed_at(ROLE_TRANSACTION, key.generation)?)
            .map_err(crypto_err)
    }
    pub fn mldsa_commitment(&self, generation: u64) -> Result<[u8; 64], CoreError> {
        let key = self.mldsa()?;
        if generation == 0 || !key.descriptor.is_rotating() {
            return Ok(key.descriptor.initial_commitment);
        }
        falcon::commitment(
            key.descriptor.algorithm,
            &falcon::public_key(
                key.descriptor.algorithm,
                &*key.seed_at(ROLE_TRANSACTION, generation)?,
            )?,
        )
    }
    pub fn mldsa_current_commitment(&self) -> Result<[u8; 64], CoreError> {
        self.mldsa_commitment(self.mldsa_generation()?)
    }
    pub fn mldsa_next_commitment(&self) -> Result<[u8; 64], CoreError> {
        self.mldsa_commitment(
            self.mldsa_generation()?
                .checked_add(1)
                .ok_or_else(|| crypto_err("post-quantum signing generation exhausted"))?,
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
        if subtensor_hashed::falcon::is_account_profile(key.descriptor.algorithm) {
            return self.sign_falcon(payload, random);
        }
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
        if subtensor_hashed::falcon::is_account_profile(self.mldsa_profile()?) {
            return Err(crypto_err(
                "use pq_capability_public_key for Falcon public keys",
            ));
        }
        mldsa::public_key_from_seed(&*self.mldsa()?.seed_at(role, epoch)?).map_err(crypto_err)
    }
    pub fn mldsa_capability_commitment(&self, role: u8, epoch: u64) -> Result<[u8; 64], CoreError> {
        falcon::commitment(
            falcon::capability_profile(self.mldsa_profile()?),
            &self.pq_capability_public_key(role, epoch)?,
        )
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
        if subtensor_hashed::falcon::is_account_profile(self.mldsa_profile()?) {
            return self.sign_falcon_capability_for(owner, role, epoch, genesis_hash, payload);
        }
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
        if subtensor_hashed::falcon::is_account_profile(self.mldsa_profile()?) {
            return Err(crypto_err("use sign_pq_order_for for Falcon orders"));
        }
        let seed = self.mldsa()?.seed_at(ROLE_ORDER, epoch)?;
        let context = capability_context(ROLE_ORDER)?;
        let mut transcript = context.to_vec();
        (genesis_hash, owner, epoch, nonce).encode_to(&mut transcript);
        transcript.extend_from_slice(order);
        mldsa::sign_message(&seed, &transcript, context, &*randomness()?).map_err(crypto_err)
    }

    /// Variable-width authorization APIs support classical and post-quantum profiles without changing
    /// the historical fixed-width Rust v1 APIs.
    pub fn authorization_descriptor(&self) -> Result<Vec<u8>, CoreError> {
        if is_pq_crypto(self.crypto_type()) {
            self.mldsa_descriptor()
        } else {
            self.hashed_descriptor()
        }
    }
    pub fn authorization_generation(&self) -> Result<u64, CoreError> {
        if is_pq_crypto(self.crypto_type()) {
            self.mldsa_generation()
        } else {
            self.hashed_generation()
        }
    }
    pub fn authorization_public_key(&self) -> Result<Vec<u8>, CoreError> {
        if is_pq_crypto(self.crypto_type()) {
            self.pq_public_key()
        } else {
            self.hashed_signing_public_key()
        }
    }
    pub fn authorization_commitment(&self, generation: u64) -> Result<Vec<u8>, CoreError> {
        if is_pq_crypto(self.crypto_type()) {
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
        if is_pq_crypto(self.crypto_type()) {
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
        for crypto in [CRYPTO_MLDSA65, CRYPTO_MLDSA65_FIXED] {
            let wallet = Keypair::from_seed(&[7; 32], crypto).unwrap();
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
    }

    #[test]
    fn order_signature_binds_runtime_transcript_and_never_uses_transaction_key() {
        for crypto in [CRYPTO_MLDSA65, CRYPTO_MLDSA65_FIXED] {
            let wallet = Keypair::from_seed(&[7; 32], crypto).unwrap();
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
    }

    #[test]
    fn delegated_purpose_keys_bind_the_explicit_composite_owner() {
        for crypto in [CRYPTO_MLDSA65, CRYPTO_MLDSA65_FIXED] {
            let wallet = Keypair::from_seed(&[7; 32], crypto).unwrap();
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
    fn standard_mldsa_creation_uses_a_recoverable_stateless_key_and_ordinary_address() {
        for value in [0, 1, 7, 19, 42, 255] {
            let seed = [value; 32];
            let key = Keypair::from_seed(&seed, CRYPTO_MLDSA65_FIXED).unwrap();
            assert_eq!(key.mldsa_profile().unwrap(), mldsa::FIXED_ALGORITHM);
            assert!(mldsa::rejects_legacy_public_keys(&key.public_key_bytes()));
            assert_eq!(key.hashed_receiving_address().unwrap(), key.ss58_address());
            assert_eq!(
                encode_hashed_receiving_address(&key.mldsa_descriptor().unwrap()).unwrap(),
                key.ss58_address()
            );
            let restored = Keypair::from_seed(&seed, CRYPTO_MLDSA65_FIXED).unwrap();
            assert_eq!(
                restored.mldsa_descriptor().unwrap(),
                key.mldsa_descriptor().unwrap()
            );
            assert_eq!(
                restored.mldsa_public_key().unwrap(),
                key.mldsa_public_key().unwrap()
            );
            assert_eq!(key.mldsa_generation().unwrap(), 0);
            assert!(key.at_generation(1).is_err());
            assert!(key.at_generation(u64::MAX).is_err());
            let proof = key
                .sign_mldsa_with_randomness(b"call/nonce/era/genesis", &[0; 32])
                .unwrap();
            let commitment = key.mldsa_current_commitment().unwrap();
            assert_eq!(key.authorization_next_commitment().unwrap(), commitment);
            assert!(verify_mldsa_authorization(
                &proof,
                b"call/nonce/era/genesis",
                &key.mldsa_descriptor().unwrap(),
                0,
                &commitment
            )
            .unwrap());
            assert!(!verify_mldsa_authorization(
                &proof,
                b"call/nonce/era/genesis",
                &key.mldsa_descriptor().unwrap(),
                1,
                &commitment
            )
            .unwrap());
            assert!(!verify_mldsa_authorization(
                &proof,
                b"other call/nonce/era/genesis",
                &key.mldsa_descriptor().unwrap(),
                0,
                &commitment
            )
            .unwrap());
            let rotating = Keypair::from_seed(&seed, CRYPTO_MLDSA65).unwrap();
            assert_ne!(key.public_key_bytes(), rotating.public_key_bytes());
            assert!(!verify_mldsa_authorization(
                &proof,
                b"call/nonce/era/genesis",
                &rotating.mldsa_descriptor().unwrap(),
                0,
                &commitment
            )
            .unwrap());
            let watch =
                Keypair::new(Some(&key.ss58_address()), None, CRYPTO_MLDSA65_FIXED, 42).unwrap();
            assert_eq!(
                watch.hashed_receiving_address().unwrap(),
                key.ss58_address()
            );
            assert!(watch.mldsa_profile().is_err()); // Hash bytes cannot identify a profile.
            assert!(watch.sign_mldsa(b"call").is_err());
            let descriptor_watch = key.public_only().unwrap();
            assert_eq!(
                descriptor_watch.hashed_receiving_address().unwrap(),
                key.ss58_address()
            );
            assert_eq!(
                descriptor_watch.mldsa_profile().unwrap(),
                mldsa::FIXED_ALGORITHM
            );
            assert!(descriptor_watch.sign_mldsa(b"call").is_err());
            // Role/epoch derivation is independent of the transaction-key search.
            assert_ne!(
                key.mldsa_capability_public_key(ROLE_MESSAGE, 0).unwrap(),
                key.mldsa_capability_public_key(ROLE_MESSAGE, 1).unwrap()
            );
            assert_ne!(
                key.mldsa_capability_public_key(ROLE_MESSAGE, 0).unwrap(),
                key.mldsa_public_key().unwrap()
            );
        }
        assert!(MldsaKeypair::stateless_transaction_key(&[7; 32], 0).is_err());
    }

    #[test]
    fn fixed_account_derivation_and_recovery_vector_is_stable() {
        let key = Keypair::from_seed(&[7; 32], CRYPTO_MLDSA65_FIXED).unwrap();
        assert_eq!(
            hex::encode(key.public_key_bytes()),
            "aa4018475de4922453c911cc9b469f6c5b0c85135e77a3db98d8f6fe325064ef"
        );
        assert_eq!(hex::encode(key.mldsa_descriptor().unwrap()), "020553f8916ec9b0b73b929d66357198a2ca70b33070e3b740e00c35cfb310c1c03e7c6ec79aaa8fad9714f94504759de1bbe3bf504f81725e0c348466b05996f49b");
        assert_eq!(
            key.ss58_address(),
            "5Fuw396EmC877oPz39aopWXWdEwdJ3KkmYVzpWsDcPTsoy8d"
        );
        let index = key.mldsa().unwrap().transaction_index;
        assert!(index < STATELESS_KEY_SEARCH_LIMIT);
        // Exhausting before the chosen index must fail closed, rather than
        // selecting a different identity or returning an unsafe candidate.
        assert!(MldsaKeypair::stateless_transaction_key(&[7; 32], index).is_err());
        assert_eq!(
            MldsaKeypair::stateless_transaction_key(&[7; 32], index + 1).unwrap(),
            (index, key.mldsa_current_commitment().unwrap())
        );
    }

    #[cfg(feature = "host")]
    #[test]
    fn every_mldsa_profile_restores_its_recorded_identity_from_seed_and_mnemonic_keyfiles() {
        use crate::keyfiles::{
            deserialize_keypair_from_keyfile_data, serialized_keypair_to_keyfile_data,
        };
        let phrase = [vec!["abandon"; 23], vec!["art"]].concat().join(" ");
        for (profile, crypto) in [
            (mldsa::ALGORITHM, CRYPTO_MLDSA65),
            (mldsa::FIXED_ALGORITHM, CRYPTO_MLDSA65_FIXED),
        ] {
            for key in [
                Keypair::from_seed(&[7; 32], crypto).unwrap(),
                Keypair::from_mnemonic(&phrase, crypto, None).unwrap(),
            ] {
                for original in [key.public_only().unwrap(), key] {
                    let serialized = serialized_keypair_to_keyfile_data(&original).unwrap();
                    let restored = deserialize_keypair_from_keyfile_data(&serialized).unwrap();
                    assert_eq!(restored.mldsa_profile().unwrap(), profile);
                    assert_eq!(
                        restored.mldsa_descriptor().unwrap(),
                        original.mldsa_descriptor().unwrap()
                    );
                    assert_eq!(restored.ss58_address(), original.ss58_address());
                    if original.private_key_bytes().is_some() {
                        assert_eq!(
                            restored.mldsa_public_key().unwrap(),
                            original.mldsa_public_key().unwrap()
                        );
                    }
                    let mut corrupt: serde_json::Value =
                        serde_json::from_slice(&serialized).unwrap();
                    let mut descriptor = original.mldsa_descriptor().unwrap();
                    descriptor[1] = if profile == mldsa::FIXED_ALGORITHM {
                        mldsa::ALGORITHM
                    } else {
                        mldsa::FIXED_ALGORITHM
                    };
                    corrupt["mldsaDescriptor"] =
                        serde_json::json!(format!("0x{}", hex::encode(descriptor)));
                    assert!(deserialize_keypair_from_keyfile_data(
                        &serde_json::to_vec(&corrupt).unwrap()
                    )
                    .is_err());
                }
            }
        }
    }

    #[cfg(feature = "host")]
    #[test]
    fn fixed_address_only_watch_keyfiles_restore_without_claiming_private_authority() {
        use crate::keyfiles::{
            deserialize_keypair_from_keyfile_data, serialized_keypair_to_keyfile_data,
        };
        let wallet = Keypair::from_seed(&[7; 32], CRYPTO_MLDSA65_FIXED).unwrap();
        let watch =
            Keypair::new(Some(&wallet.ss58_address()), None, CRYPTO_MLDSA65_FIXED, 42).unwrap();
        let serialized = serialized_keypair_to_keyfile_data(&watch).unwrap();
        let mut json: serde_json::Value = serde_json::from_slice(&serialized).unwrap();
        assert_eq!(json["walletVersion"], 2);
        assert!(json.get("mldsaDescriptor").is_none());
        let restored = deserialize_keypair_from_keyfile_data(&serialized).unwrap();
        assert_eq!(restored.ss58_address(), wallet.ss58_address());
        assert_eq!(restored.crypto_type(), CRYPTO_MLDSA65_FIXED);
        assert_eq!(restored.account_type(), "standard");
        assert_eq!(
            restored.hashed_receiving_address().unwrap(),
            wallet.ss58_address()
        );
        assert!(restored.private_key_bytes().is_none());
        assert!(restored.mldsa_descriptor().is_err());
        assert!(restored.mldsa_profile().is_err());
        assert!(restored.sign_mldsa(b"call").is_err());
        json["secretSeed"] = serde_json::json!(format!("0x{}", hex::encode([7; 32])));
        assert!(
            deserialize_keypair_from_keyfile_data(&serde_json::to_vec(&json).unwrap()).is_err()
        );
    }

    #[cfg(feature = "host")]
    #[test]
    fn unsafe_pre_release_fixed_descriptor_cannot_be_reinterpreted_during_recovery() {
        use crate::keyfiles::{
            deserialize_keypair_from_keyfile_data, serialized_keypair_to_keyfile_data,
        };
        for value in 0u8..=16 {
            let master = [value; 32];
            let original_seed = MldsaKeypair::derive_seed_for_profile(
                &master,
                mldsa::FIXED_ALGORITHM,
                ROLE_TRANSACTION,
                0,
            )
            .unwrap();
            let public_key = mldsa::public_key_from_seed(&original_seed).unwrap();
            let descriptor = mldsa::Descriptor {
                version: mldsa::VERSION,
                algorithm: mldsa::FIXED_ALGORITHM,
                initial_commitment: mldsa::commitment(&public_key),
            };
            let unsafe_account = mldsa::account_id(&descriptor);
            if mldsa::rejects_legacy_public_keys(&unsafe_account) {
                continue;
            }
            let current = Keypair::from_seed(&master, CRYPTO_MLDSA65_FIXED).unwrap();
            assert_ne!(current.public_key_bytes(), unsafe_account);
            assert!(Keypair::from_mldsa_descriptor(&descriptor.encode(), 42).is_err());
            for key in [current.public_only().unwrap(), current] {
                let encoded = serialized_keypair_to_keyfile_data(&key).unwrap();
                let mut json: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
                json["mldsaDescriptor"] =
                    serde_json::json!(format!("0x{}", hex::encode(descriptor.encode())));
                json["ss58Address"] =
                    serde_json::json!(super::super::ss58_from_public(unsafe_account, 42));
                for field in ["accountId", "publicKey"] {
                    json[field] = serde_json::json!(format!("0x{}", hex::encode(unsafe_account)));
                }
                assert!(
                    deserialize_keypair_from_keyfile_data(&serde_json::to_vec(&json).unwrap())
                        .is_err()
                );
            }
            return;
        }
        panic!("test seeds did not contain a legacy-curve account");
    }
}
