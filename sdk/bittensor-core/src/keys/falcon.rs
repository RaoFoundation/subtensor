//! Falcon backend dispatch for the shared descriptor-based wallet implementation.
//! Recovery domains and the seeded backend are protocol, not implementation details.
use codec::Encode;
use subtensor_hashed::{falcon, mldsa};

use super::{crypto_err, Keypair};
use crate::error::CoreError;

pub(super) fn public_key(profile: u8, seed: &[u8; 32]) -> Result<Vec<u8>, CoreError> {
    if falcon::is_account_profile(profile) {
        falcon::public_key_from_seed(profile, seed).map_err(crypto_err)
    } else {
        mldsa::public_key_from_seed(seed)
            .map(|key| key.to_vec())
            .map_err(crypto_err)
    }
}

pub(super) fn commitment(profile: u8, public: &[u8]) -> Result<[u8; 64], CoreError> {
    if falcon::is_account_profile(profile) {
        falcon::commitment(profile, public).map_err(crypto_err)
    } else {
        let public = public
            .try_into()
            .map_err(|_| crypto_err("invalid ML-DSA public key length"))?;
        Ok(mldsa::commitment(public))
    }
}

impl Keypair {
    /// Active transaction public key, with the width selected by the wallet profile.
    pub fn pq_public_key(&self) -> Result<Vec<u8>, CoreError> {
        let key = self.mldsa()?;
        public_key(
            key.descriptor.algorithm,
            &*key.seed_at(super::ROLE_TRANSACTION, key.generation)?,
        )
    }

    pub(super) fn sign_falcon(
        &self,
        payload: &[u8],
        random: &[u8; 32],
    ) -> Result<Vec<u8>, CoreError> {
        let key = self.mldsa()?;
        let seed = key.seed_at(super::ROLE_TRANSACTION, key.generation)?;
        let header = falcon::Header {
            account: key.account_id(),
            version: falcon::VERSION,
            algorithm: key.descriptor.algorithm,
            generation: key.generation,
            public_key: public_key(key.descriptor.algorithm, &seed)?,
            next_commitment: self.mldsa_next_commitment()?,
        };
        Ok(header
            .sign(&seed, payload, random)
            .map_err(crypto_err)?
            .encode())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::keys::{
        account_crypto_type, decode_hashed_receiving_address, is_fixed_pq_crypto,
        verify_mldsa_authorization, CRYPTO_FALCON1024, CRYPTO_FALCON1024_HASHED, CRYPTO_FALCON512,
        CRYPTO_FALCON512_HASHED,
    };

    const PHRASE: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art";
    const MODES: [u8; 4] = [
        CRYPTO_FALCON512,
        CRYPTO_FALCON512_HASHED,
        CRYPTO_FALCON1024,
        CRYPTO_FALCON1024_HASHED,
    ];

    #[test]
    fn falcon_profiles_restore_sign_and_keep_permanent_addresses() {
        let mut identities = Vec::new();
        for crypto in MODES {
            let key = Keypair::from_mnemonic(PHRASE, crypto, None).unwrap();
            assert!(!identities.contains(&key.public_key_bytes()));
            identities.push(key.public_key_bytes());
            let fixed = is_fixed_pq_crypto(crypto);
            let generation = if fixed { 0 } else { 7 };
            let active = key.at_generation(generation).unwrap();
            assert_eq!(active.public_key_bytes(), key.public_key_bytes());
            assert_eq!(
                key.account_type(),
                if fixed { "standard" } else { "hashed" }
            );
            assert_eq!(
                account_crypto_type(crypto, Some(key.account_type())).unwrap(),
                crypto
            );
            let restored = Keypair::from_seed(key.seed_bytes().unwrap(), crypto)
                .unwrap()
                .at_generation(generation)
                .unwrap();
            assert_eq!(
                restored.authorization_descriptor().unwrap(),
                active.authorization_descriptor().unwrap()
            );
            assert_eq!(
                restored.authorization_current_commitment().unwrap(),
                active.authorization_current_commitment().unwrap()
            );
            if fixed {
                assert_eq!(key.hashed_receiving_address().unwrap(), key.ss58_address());
                assert!(mldsa::rejects_legacy_public_keys(&key.public_key_bytes()));
                assert!(key.at_generation(1).is_err());
                assert_eq!(
                    key.mldsa_current_commitment().unwrap(),
                    key.mldsa_next_commitment().unwrap()
                );
            } else {
                assert_eq!(
                    decode_hashed_receiving_address(&key.hashed_receiving_address().unwrap())
                        .unwrap(),
                    key.authorization_descriptor().unwrap()
                );
                assert_eq!(
                    active.hashed_receiving_address().unwrap(),
                    key.hashed_receiving_address().unwrap()
                );
                assert_ne!(
                    active.mldsa_current_commitment().unwrap(),
                    active.mldsa_next_commitment().unwrap()
                );
            }
            let payload = b"chain/call/nonce/era/metadata";
            let proof = active.sign_mldsa(payload).unwrap();
            assert_eq!(
                proof.len(),
                falcon::proof_len(active.mldsa_profile().unwrap()).unwrap()
            );
            let verify = |bytes: &[u8], message: &[u8], gen| {
                verify_mldsa_authorization(
                    bytes,
                    message,
                    &active.mldsa_descriptor().unwrap(),
                    gen,
                    &active.mldsa_current_commitment().unwrap(),
                )
                .unwrap()
            };
            assert!(verify(&proof, payload, generation));
            assert!(!verify(&proof, b"different call", generation));
            assert!(!verify(&proof, payload, generation + 1));
            let mut wrong_profile = proof.clone();
            wrong_profile[33] ^= 1;
            assert!(!verify(&wrong_profile, payload, generation));
            let mut trailing = proof.clone();
            trailing.push(0);
            assert!(!verify(&trailing, payload, generation));
            assert!(key.public_only().unwrap().sign_mldsa(payload).is_err());
            assert!(Keypair::from_uri("//Alice", crypto).is_err());
            assert!(Keypair::from_mnemonic("abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about", crypto, None).is_err());
        }
    }

    #[cfg(feature = "host")]
    #[test]
    fn falcon_keyfiles_bind_crypto_mode_descriptor_and_seed() {
        use crate::keyfiles::{
            deserialize_keypair_from_keyfile_data, serialized_keypair_to_keyfile_data,
        };
        for crypto in MODES {
            let key = Keypair::from_seed(&[31; 32], crypto).unwrap();
            for wallet in [key.public_only().unwrap(), key] {
                let encoded = serialized_keypair_to_keyfile_data(&wallet).unwrap();
                let restored = deserialize_keypair_from_keyfile_data(&encoded).unwrap();
                assert_eq!(wallet.public_key_bytes(), restored.public_key_bytes());
                assert_eq!(wallet.crypto_type(), restored.crypto_type());
                let mut json: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
                json["cryptoType"] = serde_json::json!(crypto.to_string());
                assert!(
                    deserialize_keypair_from_keyfile_data(&serde_json::to_vec(&json).unwrap())
                        .is_ok()
                );
                json["cryptoType"] = serde_json::json!(if crypto == CRYPTO_FALCON512 {
                    CRYPTO_FALCON512_HASHED
                } else {
                    CRYPTO_FALCON512
                });
                assert!(
                    deserialize_keypair_from_keyfile_data(&serde_json::to_vec(&json).unwrap())
                        .is_err()
                );
            }
        }
    }

    #[test]
    fn falcon_recovery_vectors() {
        // Immutable recovery contract for the public all-zero-entropy BIP39 fixture.
        // These literals deliberately survive backend upgrades and optimizations.
        for (crypto, descriptor, address, current, next) in [
            (9, "0207f6aa11c692344eef51a0aa4b3db0c1f1eec8e8d1693a39572ff1a155deb380fc2002ebf57dcb5b1ca239413440a73214e85027efa38fcef4597b5743da793a21", "5E4K2QqtgunUxVAq6H5aXPeVw3TSmRECei3voYM5zVNbpSYS", "f6aa11c692344eef51a0aa4b3db0c1f1eec8e8d1693a39572ff1a155deb380fc2002ebf57dcb5b1ca239413440a73214e85027efa38fcef4597b5743da793a21", "f6aa11c692344eef51a0aa4b3db0c1f1eec8e8d1693a39572ff1a155deb380fc2002ebf57dcb5b1ca239413440a73214e85027efa38fcef4597b5743da793a21"),
            (10, "02065c7137cd27990fee7911e060de0483e2bbb8932cc0213f4cbb51f1dfd4be07b06fa137850c079f83b1dcfbb0d6652395da0dd1f1243bff58f571a4f0c445fec7", "5DvQ46TL3ozmzCALgWEeZASZMqBs7YD53dRHBB4a5bbzjRSJ", "990e37be1e3dbec2d835758e5f2bb275e35a8c1a38bbb5a3aa612579dd5abe1f85a7393d3f29d94b0d17e5e996490eaf3b54c14070ea056b0533284cb75a1cf0", "9500a972b5e8fc4f32d81ba606034d26a57d147cbf1642808f478aeb179e5e043f1dd07be3ff1b53014c7da9ac840f9a464a76540224abe7c635f2685fa3213c"),
            (11, "0209193b47f776273b8a416ee35ff87560011ccd612653de9eb66c536bb36f961502ffb92256cf74422768dd08fd919fd1ac1202fb9ccbb79095d15e569620b5cdf7", "5GtkHzJ1aq8hrAzq3baviawc8tr8fPhwCrbdMiyLUo4R8kkG", "193b47f776273b8a416ee35ff87560011ccd612653de9eb66c536bb36f961502ffb92256cf74422768dd08fd919fd1ac1202fb9ccbb79095d15e569620b5cdf7", "193b47f776273b8a416ee35ff87560011ccd612653de9eb66c536bb36f961502ffb92256cf74422768dd08fd919fd1ac1202fb9ccbb79095d15e569620b5cdf7"),
            (12, "0208175720b8870ecd47bbe45a266b0b46d9d51cc39bbcf966f87b19d3bc6c1d62caae2e78352d74b466a48d36660fda4dd44f83fe86377ce3ebc8d6ad58fb04e8b9", "5DG14D3g6n5gbhBvaUysEASNGzHKRhZc4748BzsvQRbbxcSy", "a451e7283265e526714c8c115de3e86ee51a8bc94819ed3e03726a5f03e48aa0e15d075deab38e06b3e58b64aaba36dd5a973b675f5bbf25a16bcab6a41d5c51", "9e8c3e68c420d8d70f361bbcf9a268657d7d02ffb036e0799d51afbe61443ca66a477011ef81be6ce2608a779e0567bf9dafca04bbbfdba42d0b3d6aa93e272b"),
        ] {
            let key = Keypair::from_mnemonic(PHRASE, crypto, None).unwrap();
            let active = key.at_generation(if is_fixed_pq_crypto(crypto) { 0 } else { 7 }).unwrap();
            assert_eq!(hex::encode(key.mldsa_descriptor().unwrap()), descriptor);
            assert_eq!(key.ss58_address(), address);
            assert_eq!(hex::encode(active.mldsa_current_commitment().unwrap()), current);
            assert_eq!(hex::encode(active.mldsa_next_commitment().unwrap()), next);
        }
    }
}
