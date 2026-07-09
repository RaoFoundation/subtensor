//! ML-KEM-768 + XChaCha20Poly1305 sealing (the MEV shield), absorbed from
//! `bittensor-drand` and rewritten as safe Rust (the old C ABI is gone).
//!
//! Blob layout (`include_key_hash = false`):
//!   `[u16 kem_len LE][kem_ct (kem_len bytes)][nonce24][aead_ct]`
//!
//! Blob layout (`include_key_hash = true`):
//!   `[key_hash(16)][u16 kem_len LE][kem_ct][nonce24][aead_ct]`
//!   where `key_hash = twox_128(pk_bytes)`
//!
//! AEAD parameters: key = raw ML-KEM shared secret (32 bytes, no HKDF),
//! nonce = 24 random bytes, aad = [] — the canonical "v1" KDF.

// Client-side code: arithmetic on locally validated values is the norm here,
// and this crate never runs inside the runtime.
#![allow(clippy::arithmetic_side_effects)]

use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};
use core::hash::Hasher;
use ml_kem::kem::{Encapsulate, EncapsulationKey};
use ml_kem::{Encoded, EncodedSizeUser, MlKem768Params};
use rand_core::{OsRng, RngCore};
use twox_hash::XxHash64;

use crate::error::CoreError;

/// XChaCha20Poly1305 nonce length used in ML-KEM seal/open.
pub const MLKEM_NONCE_LEN: usize = 24;

/// KDF identifier: key = raw ML-KEM shared secret, aad = [] (empty).
pub const KDF_ID: &[u8] = b"v1";

/// Computes Substrate-compatible `twox_128` hash (xxHash64 with seeds 0 and 1).
pub fn twox_128(data: &[u8]) -> [u8; 16] {
    let mut h0 = XxHash64::with_seed(0);
    h0.write(data);
    let r0 = h0.finish();

    let mut h1 = XxHash64::with_seed(1);
    h1.write(data);
    let r1 = h1.finish();

    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&r0.to_le_bytes());
    out[8..].copy_from_slice(&r1.to_le_bytes());
    out
}

/// Encrypt `plaintext` to the ML-KEM-768 public key `pk_bytes`.
pub fn seal(
    pk_bytes: &[u8],
    plaintext: &[u8],
    include_key_hash: bool,
) -> Result<Vec<u8>, CoreError> {
    let enc_pk = Encoded::<EncapsulationKey<MlKem768Params>>::try_from(pk_bytes)
        .map_err(|_| CoreError::Crypto("Failed to decode public key".into()))?;
    let pk = EncapsulationKey::<MlKem768Params>::from_bytes(&enc_pk);

    let (ct, ss) = pk
        .encapsulate(&mut OsRng)
        .map_err(|_| CoreError::Crypto("Encapsulation failed".into()))?;

    let kem_ct_bytes: &[u8] = ct.as_ref();
    let kem_ct_len = kem_ct_bytes.len();
    if kem_ct_len > u16::MAX as usize {
        return Err(CoreError::Crypto("KEM ciphertext too long".into()));
    }

    let ss_bytes: &[u8] = ss.as_ref();
    if ss_bytes.len() != 32 {
        return Err(CoreError::Crypto("Invalid shared secret length".into()));
    }
    let mut aead_key = [0u8; 32];
    aead_key.copy_from_slice(ss_bytes);

    // AEAD encrypt plaintext with XChaCha20-Poly1305, AAD = [].
    let aead = XChaCha20Poly1305::new((&aead_key).into());
    let mut nonce = [0u8; MLKEM_NONCE_LEN];
    OsRng.fill_bytes(&mut nonce);

    let nonce_x = XNonce::from_slice(&nonce);
    let aead_ct = aead
        .encrypt(
            nonce_x,
            Payload {
                msg: plaintext,
                aad: &[],
            },
        )
        .map_err(|_| CoreError::Crypto("AEAD encryption failed".into()))?;

    let key_hash_prefix_len = if include_key_hash { 16 } else { 0 };
    let mut out =
        Vec::with_capacity(key_hash_prefix_len + 2 + kem_ct_len + MLKEM_NONCE_LEN + aead_ct.len());

    if include_key_hash {
        out.extend_from_slice(&twox_128(pk_bytes));
    }
    out.extend_from_slice(&(kem_ct_len as u16).to_le_bytes());
    out.extend_from_slice(kem_ct_bytes);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&aead_ct);
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;
    use ml_kem::kem::Decapsulate;
    use ml_kem::{KemCore, MlKem768};

    #[test]
    fn twox_128_matches_substrate() {
        // twox_128("System") — a well-known Substrate storage prefix.
        assert_eq!(
            hex::encode(twox_128(b"System")),
            "26aa394eea5630e07c48ae0c9558cef7"
        );
    }

    #[test]
    fn seal_roundtrip_with_decapsulation() {
        let (dk, ek) = MlKem768::generate(&mut OsRng);
        let pk_bytes = ek.as_bytes();
        let plaintext = b"mev shield payload";

        let blob = seal(&pk_bytes, plaintext, false).expect("seal");

        let kem_len = u16::from_le_bytes([blob[0], blob[1]]) as usize;
        let kem_ct = &blob[2..2 + kem_len];
        let nonce = &blob[2 + kem_len..2 + kem_len + MLKEM_NONCE_LEN];
        let aead_ct = &blob[2 + kem_len + MLKEM_NONCE_LEN..];

        let ct = ml_kem::Ciphertext::<MlKem768>::try_from(kem_ct).expect("ct size");
        let ss = dk.decapsulate(&ct).expect("decapsulate");
        let ss_bytes: &[u8] = ss.as_ref();
        let mut key = [0u8; 32];
        key.copy_from_slice(ss_bytes);
        let aead = XChaCha20Poly1305::new((&key).into());
        let opened = aead
            .decrypt(
                XNonce::from_slice(nonce),
                Payload {
                    msg: aead_ct,
                    aad: &[],
                },
            )
            .expect("open");
        assert_eq!(opened, plaintext);
    }

    #[test]
    fn seal_with_key_hash_prefixes_twox() {
        let (_dk, ek) = MlKem768::generate(&mut OsRng);
        let pk_bytes = ek.as_bytes();
        let blob = seal(&pk_bytes, b"x", true).expect("seal");
        assert_eq!(&blob[..16], &twox_128(&pk_bytes));
    }

    #[test]
    fn seal_rejects_bad_public_key() {
        assert!(seal(b"short", b"data", false).is_err());
    }
}
