//! PolkadotJS / substrate-interface encrypted JSON keystore import.

// Client-side code: slicing and arithmetic on locally validated buffers is
// the norm here, and this crate never runs inside the runtime.
#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

use base64::{engine::general_purpose, Engine as _};
use schnorrkel::{PublicKey, SecretKey};
use scrypt::{scrypt, Params as ScryptParams};
use serde::Deserialize;
use sodiumoxide::crypto::secretbox::{self, Key, Nonce};
use sp_core::{ed25519, Pair as PairT};
use zeroize::Zeroizing;

use crate::error::CoreError;
use crate::keys::{Keypair, KeypairInner, CRYPTO_SR25519, DEFAULT_SS58_FORMAT};

const PKCS8_HEADER: &[u8] = &[48, 83, 2, 1, 1, 48, 5, 6, 3, 43, 101, 112, 4, 34, 4, 32];
const PKCS8_DIVIDER: &[u8] = &[161, 35, 3, 33, 0];
const SEC_LENGTH: usize = 64;
const PUB_LENGTH: usize = 32;

#[derive(Deserialize)]
struct JsonStructure {
    encoded: String,
    encoding: JsonEncoding,
}

#[derive(Deserialize)]
struct JsonEncoding {
    content: Vec<String>,
    #[serde(rename = "type")]
    enc_type: Vec<String>,
    version: String,
}

fn value_err(msg: impl Into<String>) -> CoreError {
    CoreError::Crypto(msg.into())
}

fn pad_right(mut data: Vec<u8>, total_len: usize, pad_byte: u8) -> Vec<u8> {
    if data.len() < total_len {
        data.extend(vec![pad_byte; total_len - data.len()]);
    }
    data
}

fn pair_from_ed25519_secret_key(secret: &[u8]) -> Result<([u8; 64], [u8; 32]), CoreError> {
    let secret_key = SecretKey::from_ed25519_bytes(secret)
        .map_err(|_| value_err("invalid ed25519 secret key in encrypted JSON"))?;
    let public_key: PublicKey = secret_key.to_public();
    Ok((secret_key.to_bytes(), public_key.to_bytes()))
}

fn decode_pkcs8(ciphertext: &[u8]) -> Result<([u8; SEC_LENGTH], [u8; PUB_LENGTH]), CoreError> {
    let min_len = PKCS8_HEADER.len() + SEC_LENGTH + PKCS8_DIVIDER.len() + PUB_LENGTH;
    if ciphertext.len() < min_len {
        return Err(value_err("decrypted PKCS8 payload too short"));
    }
    let mut current_offset = 0;
    let header = &ciphertext[current_offset..current_offset + PKCS8_HEADER.len()];
    if header != PKCS8_HEADER {
        return Err(value_err("invalid PKCS8 header in encrypted JSON"));
    }
    current_offset += PKCS8_HEADER.len();
    let secret_key = &ciphertext[current_offset..current_offset + SEC_LENGTH];
    let mut secret_key_array = [0u8; SEC_LENGTH];
    secret_key_array.copy_from_slice(secret_key);
    current_offset += SEC_LENGTH;
    let divider = &ciphertext[current_offset..current_offset + PKCS8_DIVIDER.len()];
    if divider != PKCS8_DIVIDER {
        return Err(value_err("invalid PKCS8 divider in encrypted JSON"));
    }
    current_offset += PKCS8_DIVIDER.len();
    let public_key = &ciphertext[current_offset..current_offset + PUB_LENGTH];
    let mut public_key_array = [0u8; PUB_LENGTH];
    public_key_array.copy_from_slice(public_key);
    Ok((secret_key_array, public_key_array))
}

/// PolkadotJS keystores commonly use n=32768, r=8, p=1. Reject pathological params
/// so a malicious JSON cannot CPU-DoS the import path.
fn validate_scrypt_params(n: u32, r: u32, p: u32) -> Result<(), CoreError> {
    if n == 0 || !n.is_power_of_two() {
        return Err(value_err("scrypt n must be a non-zero power of two"));
    }
    let log_n = n.ilog2();
    if log_n > 18 {
        return Err(value_err("scrypt n exceeds maximum allowed cost"));
    }
    if r == 0 || r > 8 {
        return Err(value_err("scrypt r exceeds maximum allowed cost"));
    }
    if p == 0 || p > 1 {
        return Err(value_err("scrypt p exceeds maximum allowed cost"));
    }
    Ok(())
}

pub fn create_from_encrypted_json(json_data: &str, passphrase: &str) -> Result<Keypair, CoreError> {
    sodiumoxide::init().map_err(|_| value_err("failed to initialize libsodium"))?;

    let json_data: JsonStructure = serde_json::from_str(json_data)
        .map_err(|error| value_err(format!("invalid JSON: {error}")))?;

    if json_data.encoding.version != "3" {
        return Err(value_err("unsupported encrypted JSON format version"));
    }

    let mut encrypted = general_purpose::STANDARD
        .decode(json_data.encoded.trim())
        .map_err(|error| value_err(format!("invalid base64 in encrypted JSON: {error}")))?;

    let password = if json_data.encoding.enc_type.iter().any(|t| t == "scrypt") {
        if encrypted.len() < 44 {
            return Err(value_err(
                "encrypted JSON payload too short for scrypt params",
            ));
        }
        let salt = &encrypted[0..32];
        let n = u32::from_le_bytes(
            encrypted[32..36]
                .try_into()
                .map_err(|_| value_err("encrypted JSON payload truncated"))?,
        );
        let p = u32::from_le_bytes(
            encrypted[36..40]
                .try_into()
                .map_err(|_| value_err("encrypted JSON payload truncated"))?,
        );
        let r = u32::from_le_bytes(
            encrypted[40..44]
                .try_into()
                .map_err(|_| value_err("encrypted JSON payload truncated"))?,
        );
        validate_scrypt_params(n, r, p)?;
        let log_n: u8 = n.ilog2() as u8;
        let params = ScryptParams::new(log_n, r, p, 32)
            .map_err(|error| value_err(format!("invalid scrypt params: {error}")))?;
        let mut derived_key = Zeroizing::new(vec![0u8; 32]);
        scrypt(passphrase.as_bytes(), salt, &params, &mut derived_key)
            .map_err(|error| value_err(format!("scrypt key derivation failed: {error}")))?;
        encrypted = encrypted[44..].to_vec();
        derived_key
    } else {
        Zeroizing::new(pad_right(passphrase.as_bytes().to_vec(), 32, 0x00))
    };

    if encrypted.len() < 24 {
        return Err(value_err("encrypted JSON payload too short for nonce"));
    }
    let nonce = Nonce::from_slice(&encrypted[0..24])
        .ok_or_else(|| value_err("invalid nonce in encrypted JSON"))?;
    let message = &encrypted[24..];
    let key = Key::from_slice(&password).ok_or_else(|| value_err("invalid derived key length"))?;
    let decrypted_data = Zeroizing::new(
        secretbox::open(message, &nonce, &key)
            .map_err(|_| value_err("failed to decrypt encrypted JSON (wrong passphrase?)"))?,
    );
    let (private_key, public_key) = decode_pkcs8(&decrypted_data)?;

    if json_data.encoding.content.iter().any(|c| c == "sr25519") {
        let (secret, derived_public_key) = pair_from_ed25519_secret_key(&private_key)?;
        if public_key != derived_public_key {
            return Err(value_err("sr25519 public key mismatch in encrypted JSON"));
        }
        let secret_hex = Zeroizing::new(hex::encode(secret));
        Keypair::from_private_key(&secret_hex, CRYPTO_SR25519)
    } else if json_data.encoding.content.iter().any(|c| c == "ed25519") {
        let seed = &private_key[..32];
        let pair = ed25519::Pair::from_seed_slice(seed).map_err(|error| {
            value_err(format!("invalid ed25519 seed in encrypted JSON: {error:?}"))
        })?;
        if pair.public().0 != public_key {
            return Err(value_err("ed25519 public key mismatch in encrypted JSON"));
        }
        Ok(Keypair::from_inner(
            KeypairInner::Ed25519(pair),
            DEFAULT_SS58_FORMAT,
        ))
    } else {
        Err(value_err("unsupported keypair type in encrypted JSON"))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use sp_core::{ed25519, Pair as PairT};

    fn pkcs8_payload(secret: &[u8; SEC_LENGTH], public: &[u8; PUB_LENGTH]) -> Vec<u8> {
        let mut out =
            Vec::with_capacity(PKCS8_HEADER.len() + SEC_LENGTH + PKCS8_DIVIDER.len() + PUB_LENGTH);
        out.extend_from_slice(PKCS8_HEADER);
        out.extend_from_slice(secret);
        out.extend_from_slice(PKCS8_DIVIDER);
        out.extend_from_slice(public);
        out
    }

    fn build_passphrase_ed25519_json(uri: &str, passphrase: &str) -> String {
        sodiumoxide::init().unwrap();
        let pair = ed25519::Pair::from_string(uri, None).expect("valid uri");
        let raw = pair.to_raw_vec();
        let mut secret = [0u8; SEC_LENGTH];
        secret[..32].copy_from_slice(&raw);
        secret[32..].copy_from_slice(pair.public().as_ref());
        let mut public = [0u8; PUB_LENGTH];
        public.copy_from_slice(pair.public().as_ref());
        let plaintext = pkcs8_payload(&secret, &public);
        let password = pad_right(passphrase.as_bytes().to_vec(), 32, 0x00);
        let key = Key::from_slice(&password).expect("derived key");
        let nonce = secretbox::gen_nonce();
        let ciphertext = secretbox::seal(&plaintext, &nonce, &key);
        let mut encoded = nonce.as_ref().to_vec();
        encoded.extend_from_slice(&ciphertext);
        format!(
            r#"{{"encoded":"{}","encoding":{{"content":["pkcs8","ed25519"],"type":["xsalsa20-poly1305"],"version":"3"}}}}"#,
            general_purpose::STANDARD.encode(encoded)
        )
    }

    #[test]
    fn rejects_excessive_scrypt_cost() {
        assert!(validate_scrypt_params(1 << 19, 8, 1).is_err());
        assert!(validate_scrypt_params(32768, 9, 1).is_err());
        assert!(validate_scrypt_params(32768, 8, 2).is_err());
        assert!(validate_scrypt_params(32768, 0, 1).is_err());
    }

    #[test]
    fn accepts_standard_scrypt_cost() {
        validate_scrypt_params(32768, 8, 1).expect("standard polkadot-js params");
    }

    #[test]
    fn ed25519_passphrase_import_roundtrip() {
        let uri = "//Alice";
        let pair = ed25519::Pair::from_string(uri, None).expect("valid uri");
        let expected = Keypair::from_inner(KeypairInner::Ed25519(pair), DEFAULT_SS58_FORMAT);
        let json = build_passphrase_ed25519_json(uri, "test-passphrase");
        let kp = create_from_encrypted_json(&json, "test-passphrase").expect("import");
        assert_eq!(kp.ss58_address(), expected.ss58_address());
    }

    #[test]
    fn ed25519_passphrase_import_wrong_password() {
        let json = build_passphrase_ed25519_json("//Alice", "test-passphrase");
        assert!(create_from_encrypted_json(&json, "wrong").is_err());
    }

    #[test]
    fn rejects_unsupported_version() {
        let json = r#"{"encoded":"AA==","encoding":{"content":["pkcs8","ed25519"],"type":["xsalsa20-poly1305"],"version":"2"}}"#;
        assert!(create_from_encrypted_json(json, "pass").is_err());
    }
}
