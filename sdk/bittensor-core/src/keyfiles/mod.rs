//! Bittensor wallet keyfile encryption and JSON codec — compatible with the
//! ``bittensor-wallet`` on-disk format (absorbed from `py-sp-core`).

// Client-side code: slicing and arithmetic on locally validated buffers is
// the norm here, and this crate never runs inside the runtime.
#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

use std::collections::HashMap;

use base64::{engine::general_purpose, Engine as _};
use fernet::Fernet;
use pbkdf2::pbkdf2_hmac;
use serde_json::json;
use sha2::Sha256;
use sodiumoxide::crypto::pwhash;
use sodiumoxide::crypto::secretbox;
use zeroize::{Zeroize, Zeroizing};

use crate::error::CoreError;
use crate::keys::{ensure_sodium, is_hashed_crypto, Keypair, CRYPTO_ED25519, CRYPTO_SR25519};

const NACL_SALT: &[u8] = b"\x13q\x83\xdf\xf1Z\t\xbc\x9c\x90\xb5Q\x879\xe9\xb1";
const LEGACY_SALT: &[u8] = b"Iguesscyborgslikemyselfhaveatendencytobeparanoidaboutourorigins";

fn key_err(msg: impl Into<String>) -> CoreError {
    CoreError::Keyfile(msg.into())
}

pub fn keyfile_data_is_encrypted_nacl(keyfile_data: &[u8]) -> bool {
    keyfile_data.starts_with(b"$NACL")
}

pub fn keyfile_data_is_encrypted_ansible(keyfile_data: &[u8]) -> bool {
    keyfile_data.starts_with(b"$ANSIBLE_VAULT")
}

pub fn keyfile_data_is_encrypted_legacy(keyfile_data: &[u8]) -> bool {
    keyfile_data.starts_with(b"gAAAAA")
}

pub fn keyfile_data_is_encrypted(keyfile_data: &[u8]) -> bool {
    keyfile_data_is_encrypted_nacl(keyfile_data)
        || keyfile_data_is_encrypted_ansible(keyfile_data)
        || keyfile_data_is_encrypted_legacy(keyfile_data)
}

pub fn keyfile_data_encryption_method(keyfile_data: &[u8]) -> &'static str {
    if keyfile_data_is_encrypted_nacl(keyfile_data) {
        "NaCl"
    } else if keyfile_data_is_encrypted_ansible(keyfile_data) {
        "Ansible Vault"
    } else if keyfile_data_is_encrypted_legacy(keyfile_data) {
        "legacy"
    } else {
        "unknown"
    }
}

fn derive_key(password: &[u8]) -> Result<secretbox::Key, CoreError> {
    let salt = pwhash::argon2i13::Salt::from_slice(NACL_SALT)
        .ok_or_else(|| key_err("invalid NACL salt"))?;
    let mut key = secretbox::Key([0; secretbox::KEYBYTES]);
    pwhash::argon2i13::derive_key(
        &mut key.0,
        password,
        &salt,
        pwhash::argon2i13::OPSLIMIT_SENSITIVE,
        pwhash::argon2i13::MEMLIMIT_SENSITIVE,
    )
    .map_err(|_| key_err("failed to derive NaCl key"))?;
    Ok(key)
}

fn nacl_decrypt(keyfile_data: &[u8], key: &secretbox::Key) -> Result<Vec<u8>, CoreError> {
    let data = &keyfile_data[5..];
    if data.len() < secretbox::NONCEBYTES {
        return Err(key_err("invalid NaCl keyfile: too short"));
    }
    let nonce = secretbox::Nonce::from_slice(&data[..secretbox::NONCEBYTES])
        .ok_or_else(|| key_err("invalid NaCl nonce"))?;
    let ciphertext = &data[secretbox::NONCEBYTES..];
    secretbox::open(ciphertext, &nonce, key)
        .map_err(|_| CoreError::WrongPassword("wrong password for NaCl decryption".into()))
}

pub fn encrypt_keyfile_data(keyfile_data: &[u8], password: &str) -> Result<Vec<u8>, CoreError> {
    ensure_sodium()?;
    let key = derive_key(password.as_bytes())?;
    let nonce = secretbox::gen_nonce();
    let encrypted_data = secretbox::seal(keyfile_data, &nonce, &key);
    let mut result = b"$NACL".to_vec();
    result.extend_from_slice(&nonce.0);
    result.extend_from_slice(&encrypted_data);
    Ok(result)
}

fn xor_with_key(data: &[u8], key: &str) -> Vec<u8> {
    let key_bytes = key.as_bytes();
    data.iter()
        .enumerate()
        .map(|(index, byte)| byte ^ key_bytes[index % key_bytes.len()])
        .collect()
}

fn decrypt_password(data: &[u8], key: &str) -> Result<String, CoreError> {
    let decrypted_bytes = xor_with_key(data, key);
    String::from_utf8(decrypted_bytes)
        .map_err(|_| key_err("invalid wallet password env var: corrupt UTF-8"))
}

pub fn get_password_from_environment(env_var_name: &str) -> Result<Option<String>, CoreError> {
    if env_var_name.is_empty() {
        return Err(CoreError::Crypto("env var name must not be empty".into()));
    }
    match std::env::var(env_var_name) {
        Ok(encrypted_password_base64) => {
            let encrypted_password = general_purpose::STANDARD
                .decode(encrypted_password_base64.trim())
                .map_err(|_| key_err("invalid base64 in wallet password env var"))?;
            Ok(Some(decrypt_password(&encrypted_password, env_var_name)?))
        }
        Err(_) => Ok(None),
    }
}

pub fn save_password_to_environment(
    env_var_name: &str,
    password: &str,
) -> Result<String, CoreError> {
    if env_var_name.is_empty() {
        return Err(CoreError::Crypto("env var name must not be empty".into()));
    }
    let encrypted = xor_with_key(password.as_bytes(), env_var_name);
    // Inherited btwallet behavior: set_var is not thread-safe and can race with
    // concurrent getenv calls from other (non-GIL-holding) threads.
    std::env::set_var(env_var_name, general_purpose::STANDARD.encode(encrypted));
    Ok(env_var_name.to_string())
}

fn legacy_decrypt(password: &str, keyfile_data: &[u8]) -> Result<Vec<u8>, CoreError> {
    let mut key = [0u8; 32];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), LEGACY_SALT, 10_000_000, &mut key);
    let fernet_key = Zeroizing::new(general_purpose::URL_SAFE.encode(key));
    key.zeroize();
    let fernet = Fernet::new(&fernet_key).ok_or_else(|| key_err("invalid legacy fernet key"))?;
    let keyfile_data_str = std::str::from_utf8(keyfile_data)
        .map_err(|e| key_err(format!("legacy keyfile is not valid utf-8: {e}")))?;
    fernet
        .decrypt(keyfile_data_str)
        .map_err(|_| CoreError::WrongPassword("wrong password for legacy decryption".into()))
}

pub fn decrypt_keyfile_data(
    keyfile_data: &[u8],
    password: Option<&str>,
) -> Result<Vec<u8>, CoreError> {
    ensure_sodium()?;
    let password = password.ok_or_else(|| key_err("password required to decrypt keyfile"))?;

    if keyfile_data_is_encrypted_nacl(keyfile_data) {
        let key = derive_key(password.as_bytes())?;
        return nacl_decrypt(keyfile_data, &key);
    }

    if keyfile_data_is_encrypted_ansible(keyfile_data) {
        let decrypted = ansible_vault::decrypt_vault(keyfile_data, password).map_err(|_| {
            CoreError::WrongPassword("wrong password for ansible vault decryption".into())
        })?;
        return Ok(decrypted);
    }

    if keyfile_data_is_encrypted_legacy(keyfile_data) {
        return legacy_decrypt(password, keyfile_data);
    }

    Err(key_err("invalid or unknown keyfile encryption method"))
}

pub fn serialized_keypair_to_keyfile_data(keypair: &Keypair) -> Result<Vec<u8>, CoreError> {
    let mut data: HashMap<&str, serde_json::Value> = HashMap::new();

    let public_key = keypair.public_key_bytes();
    let public_key_str = hex::encode(public_key);
    data.insert("accountId", json!(format!("0x{public_key_str}")));
    data.insert("publicKey", json!(format!("0x{public_key_str}")));

    // Legacy btwallet keyfiles always carried secretPhrase/secretSeed, and
    // third-party parsers (subnet tooling, struct-based Rust readers) can
    // require them. Write them whenever the keypair retained them so files
    // created here stay parseable by legacy readers.
    if let Some(mnemonic) = keypair.mnemonic() {
        data.insert("secretPhrase", json!(mnemonic));
    }
    if let Some(seed) = keypair.seed_bytes() {
        data.insert("secretSeed", json!(format!("0x{}", hex::encode(seed))));
    }

    if let Some(private_key) = keypair.private_key_bytes() {
        let private_key_str = hex::encode(private_key);
        data.insert("privateKey", json!(format!("0x{private_key_str}")));
    }

    data.insert("ss58Address", json!(keypair.ss58_address()));
    data.insert("cryptoType", json!(keypair.crypto_type()));
    data.insert("accountType", json!(keypair.account_type()));
    data.insert("signingScheme", json!(keypair.signing_scheme()));
    if is_hashed_crypto(keypair.crypto_type()) {
        if let Ok(descriptor) = keypair.hashed_descriptor() {
            data.insert(
                "hashedDescriptor",
                json!(format!("0x{}", hex::encode(descriptor))),
            );
        }
        // A persisted generation would be stale after use on another machine.
        // Restore the genesis identity here; always select state from chain.
    }

    serde_json::to_string(&data)
        .map(|json_data| json_data.into_bytes())
        .map_err(|error| key_err(format!("serialization error: {error}")))
}

/// Stored ss58Address, including the legacy leading-space `" ss58Address"`
/// key some old btwallet files carry.
fn stored_ss58(keyfile_dict: &serde_json::Value) -> Option<&str> {
    keyfile_dict
        .get("ss58Address")
        .or_else(|| keyfile_dict.get(" ss58Address"))
        .and_then(|value| value.as_str())
}

/// Derive a keypair and cross-check it against the keyfile's stored
/// ss58Address. Legacy keyfiles sometimes omit or mislabel cryptoType, so on
/// a mismatch the other crypto type is tried before giving up: the stored
/// address is the ground truth for which key the file holds.
fn resolve_checked<F>(
    keyfile_dict: &serde_json::Value,
    crypto_type: u8,
    derived_from: &str,
    derive: F,
) -> Result<Keypair, CoreError>
where
    F: Fn(u8) -> Result<Keypair, CoreError>,
{
    let keypair = derive(crypto_type)?;
    let Some(stored) = stored_ss58(keyfile_dict) else {
        return Ok(keypair);
    };
    if keypair.ss58_address() == stored {
        return Ok(keypair);
    }
    let alternate = if crypto_type == CRYPTO_SR25519 {
        CRYPTO_ED25519
    } else {
        CRYPTO_SR25519
    };
    if let Ok(alternate_keypair) = derive(alternate) {
        if alternate_keypair.ss58_address() == stored {
            return Ok(alternate_keypair);
        }
    }
    Err(key_err(format!(
        "ss58Address in keyfile does not match the address derived from {derived_from} \
         (check the keyfile's cryptoType)",
    )))
}

/// Whether raw (non-JSON) keyfile content looks like a bare BIP39 phrase, as
/// written by pre-JSON-era bittensor wallets.
fn looks_like_mnemonic(text: &str) -> bool {
    let words: Vec<&str> = text.split_whitespace().collect();
    matches!(words.len(), 12 | 15 | 18 | 21 | 24)
        && words
            .iter()
            .all(|word| word.chars().all(|c| c.is_ascii_lowercase()))
}

/// Fallback for raw (non-JSON) keyfile payloads: a bare hex seed/private key
/// or a bare mnemonic, as written by pre-JSON-era bittensor wallets.
fn keypair_from_raw_text(text: &str) -> Option<Keypair> {
    let trimmed = text.trim();
    let hex_body = trimmed.strip_prefix("0x").unwrap_or(trimmed);
    if matches!(hex_body.len(), 64 | 128) && hex_body.chars().all(|c| c.is_ascii_hexdigit()) {
        if hex_body.len() == 64 {
            return hex::decode(hex_body)
                .ok()
                .and_then(|bytes| Keypair::from_seed(&bytes, CRYPTO_SR25519).ok());
        }
        return Keypair::from_private_key(trimmed, CRYPTO_SR25519).ok();
    }
    if looks_like_mnemonic(trimmed) {
        if let Ok(keypair) = Keypair::from_mnemonic(trimmed, CRYPTO_SR25519, None) {
            return Some(keypair);
        }
    }
    None
}

fn deserialize_hashed(keyfile: &serde_json::Value, crypto_type: u8) -> Result<Keypair, CoreError> {
    let public = match keyfile.get("hashedDescriptor") {
        Some(serde_json::Value::String(encoded)) => {
            let descriptor = hex::decode(encoded.trim_start_matches("0x"))
                .map_err(|_| key_err("invalid hashed descriptor encoding"))?;
            Some(Keypair::from_hashed_descriptor(&descriptor, 42)?)
        }
        Some(_) => return Err(key_err("hashedDescriptor must be a hex string")),
        None => None,
    };
    let private = if let Some(phrase) = keyfile.get("secretPhrase") {
        let phrase = phrase
            .as_str()
            .ok_or_else(|| key_err("invalid hashed secretPhrase"))?;
        Some(Keypair::from_mnemonic(phrase, crypto_type, None)?)
    } else if let Some(seed) = keyfile
        .get("secretSeed")
        .or_else(|| keyfile.get("privateKey"))
    {
        let seed = seed
            .as_str()
            .ok_or_else(|| key_err("invalid hashed master seed"))?;
        let seed = Zeroizing::new(
            hex::decode(seed.trim_start_matches("0x"))
                .map_err(|_| key_err("invalid hashed master seed encoding"))?,
        );
        Some(Keypair::from_seed(&seed, crypto_type)?)
    } else {
        None
    };
    if private.is_some() && public.is_none() {
        return Err(key_err(
            "hashed secret keyfiles require their versioned hashedDescriptor",
        ));
    }
    let keypair = match (private, public) {
        (Some(private), Some(public)) => {
            if private.hashed_descriptor()? != public.hashed_descriptor()? {
                return Err(key_err(
                    "hashedDescriptor does not match the recovered master seed",
                ));
            }
            private
        }
        (None, Some(public)) => public,
        (None, None) => Keypair::new(stored_ss58(keyfile), None, crypto_type, 42)?,
        (Some(_), None) => return Err(key_err("missing hashed descriptor")),
    };
    for (field, expected) in [
        ("accountType", keypair.account_type()),
        ("signingScheme", keypair.signing_scheme()),
    ] {
        if let Some(value) = keyfile.get(field) {
            if value.as_str() != Some(expected) {
                return Err(key_err(format!("{field} does not match hashedDescriptor")));
            }
        }
    }
    if keypair.crypto_type() != crypto_type {
        return Err(key_err("cryptoType does not match hashedDescriptor"));
    }
    if let Some(address) = stored_ss58(keyfile) {
        if keypair.ss58_address() != address {
            return Err(key_err("ss58Address does not match hashedDescriptor"));
        }
    }
    for field in ["accountId", "publicKey"] {
        if let Some(stored) = keyfile.get(field) {
            let encoded = stored
                .as_str()
                .ok_or_else(|| key_err("invalid hashed account ID"))?;
            let bytes = hex::decode(encoded.trim_start_matches("0x"))
                .map_err(|_| key_err("invalid hashed account ID encoding"))?;
            if bytes != keypair.public_key_bytes() {
                return Err(key_err("account ID does not match hashedDescriptor"));
            }
        }
    }
    Ok(keypair)
}

pub fn deserialize_keypair_from_keyfile_data(keyfile_data: &[u8]) -> Result<Keypair, CoreError> {
    let decoded = std::str::from_utf8(keyfile_data).map_err(|_| {
        if keyfile_data_is_encrypted(keyfile_data) {
            key_err("keyfile is encrypted; decrypt it with its password first")
        } else {
            key_err("failed to decode keyfile data: not utf-8 text (unknown or corrupt format)")
        }
    })?;

    let keyfile_dict: serde_json::Value = match serde_json::from_str(decoded) {
        Ok(value) => value,
        Err(_) => {
            if let Some(keypair) = keypair_from_raw_text(decoded) {
                return Ok(keypair);
            }
            return Err(key_err(
                "failed to parse keyfile data: not keyfile JSON, a raw hex seed, or a mnemonic",
            ));
        }
    };

    // A polkadot.js / mobile-app keystore export is valid JSON but a wholly
    // different (password-encrypted) format; name it instead of failing with
    // a generic parse error.
    if keyfile_dict.get("encoded").is_some() && keyfile_dict.get("encoding").is_some() {
        return Err(key_err(
            "this keyfile is a polkadot.js / mobile-app JSON export, not a btcli keyfile; \
             import it with `btcli wallet regen-coldkey --json-path <file>`",
        ));
    }

    // Historical writers disagree on the cryptoType JSON type: python btwallet
    // wrote a number, some JS tooling wrote a numeric string.
    let crypto_type = keyfile_dict
        .get("cryptoType")
        .and_then(|value| match value {
            serde_json::Value::Number(number) => number.to_string().parse::<u8>().ok(),
            serde_json::Value::String(text) => text.trim().parse::<u8>().ok(),
            _ => None,
        })
        .unwrap_or(CRYPTO_SR25519);

    if is_hashed_crypto(crypto_type) {
        return deserialize_hashed(&keyfile_dict, crypto_type);
    }
    if keyfile_dict.get("hashedDescriptor").is_some() {
        return Err(key_err(
            "hashedDescriptor requires a registered-account cryptoType",
        ));
    }

    if let Some(secret_phrase) = keyfile_dict
        .get("secretPhrase")
        .and_then(|value| value.as_str())
    {
        return resolve_checked(&keyfile_dict, crypto_type, "secretPhrase", |ct| {
            Keypair::from_mnemonic(secret_phrase, ct, None)
        });
    }

    if let Some(seed) = keyfile_dict
        .get("secretSeed")
        .and_then(|value| value.as_str())
    {
        let seed = seed.trim_start_matches("0x");
        let seed_bytes =
            hex::decode(seed).map_err(|error| key_err(format!("invalid secret seed: {error}")))?;
        return resolve_checked(&keyfile_dict, crypto_type, "secretSeed", |ct| {
            Keypair::from_seed(&seed_bytes, ct)
        });
    }

    if let Some(private_key) = keyfile_dict
        .get("privateKey")
        .and_then(|value| value.as_str())
    {
        return resolve_checked(&keyfile_dict, crypto_type, "privateKey", |ct| {
            Keypair::from_private_key(private_key, ct)
        });
    }

    if let Some(ss58) = keyfile_dict
        .get("ss58Address")
        .and_then(|value| value.as_str())
    {
        return Keypair::new(Some(ss58), None, crypto_type, 42);
    }

    let found_fields = keyfile_dict
        .as_object()
        .map(|object| object.keys().cloned().collect::<Vec<_>>().join(", "))
        .unwrap_or_else(|| "none".to_string());
    Err(key_err(format!(
        "keypair could not be created from keyfile data: no secretPhrase, secretSeed, \
         privateKey, or ss58Address field (found: {found_fields})",
    )))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::keys::CRYPTO_ED25519;
    use crate::keys::CRYPTO_HASHED;

    #[test]
    fn new_account_modes_roundtrip_and_reject_mislabeled_backups() {
        for code in [
            crate::keys::CRYPTO_HASHED_ED25519,
            crate::keys::CRYPTO_MLDSA_STANDARD,
        ] {
            let key = Keypair::from_seed(&[37; 32], code).unwrap();
            for original in [key.public_only().unwrap(), key] {
                let bytes = serialized_keypair_to_keyfile_data(&original).unwrap();
                let restored = deserialize_keypair_from_keyfile_data(&bytes).unwrap();
                assert_eq!(restored.crypto_type(), code);
                assert_eq!(
                    restored.hashed_descriptor().unwrap(),
                    original.hashed_descriptor().unwrap()
                );
                for field in ["accountType", "signingScheme"] {
                    let mut data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                    data[field] = json!("wrong");
                    assert!(deserialize_keypair_from_keyfile_data(
                        &serde_json::to_vec(&data).unwrap()
                    )
                    .is_err());
                }
                let mut legacy: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                legacy.as_object_mut().unwrap().remove("accountType");
                legacy.as_object_mut().unwrap().remove("signingScheme");
                assert_eq!(
                    deserialize_keypair_from_keyfile_data(&serde_json::to_vec(&legacy).unwrap())
                        .unwrap()
                        .crypto_type(),
                    code
                );
            }
        }
    }

    fn test_mnemonic() -> String {
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
            .to_string()
    }

    #[test]
    fn mldsa_keyfiles_preserve_identity_and_reject_scheme_substitution() {
        let key =
            Keypair::from_mnemonic(&test_mnemonic(), crate::keys::CRYPTO_MLDSA, None).unwrap();
        for original in [key.public_only().unwrap(), key] {
            let encoded = serialized_keypair_to_keyfile_data(&original).unwrap();
            let restored = deserialize_keypair_from_keyfile_data(&encoded).unwrap();
            assert_eq!(restored.crypto_type(), crate::keys::CRYPTO_MLDSA);
            assert_eq!(
                restored.hashed_descriptor().unwrap(),
                original.hashed_descriptor().unwrap()
            );
            assert_eq!(restored.private_key_bytes(), original.private_key_bytes());
            let mut changed: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
            changed["cryptoType"] = serde_json::json!(CRYPTO_HASHED);
            assert!(
                deserialize_keypair_from_keyfile_data(&serde_json::to_vec(&changed).unwrap())
                    .is_err()
            );
        }
    }

    #[test]
    fn nacl_roundtrip() {
        let message = br#"{"ss58Address":"5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY"}"#;
        let encrypted = encrypt_keyfile_data(message, "test-password").unwrap();
        assert!(keyfile_data_is_encrypted_nacl(&encrypted));
        let decrypted = decrypt_keyfile_data(&encrypted, Some("test-password")).unwrap();
        assert_eq!(decrypted, message);
    }

    #[test]
    fn env_password_roundtrip() {
        let env_var = "BT_PW_TEST_WALLET_COLDKEY";
        save_password_to_environment(env_var, "test-password").unwrap();
        let recovered = get_password_from_environment(env_var).unwrap();
        assert_eq!(recovered.as_deref(), Some("test-password"));
        std::env::remove_var(env_var);
    }

    #[test]
    fn ansible_vault_roundtrip() {
        let original = br#"{"ss58Address":"5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY"}"#;
        let encrypted =
            ansible_vault::encrypt_vault(&original[..], "test-password").expect("ansible encrypt");
        assert!(keyfile_data_is_encrypted_ansible(encrypted.as_bytes()));
        let decrypted = decrypt_keyfile_data(encrypted.as_bytes(), Some("test-password"))
            .expect("ansible decrypt");
        assert_eq!(decrypted, original);
    }

    #[test]
    fn legacy_fernet_roundtrip() {
        let original = br#"{"ss58Address":"5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY"}"#;
        let mut key = [0u8; 32];
        pbkdf2_hmac::<Sha256>(b"test-password", LEGACY_SALT, 10_000_000, &mut key);
        let fernet_key = general_purpose::URL_SAFE.encode(key);
        let fernet = Fernet::new(&fernet_key).expect("fernet key");
        let encrypted = fernet.encrypt(original);
        assert!(keyfile_data_is_encrypted_legacy(encrypted.as_bytes()));
        let decrypted = decrypt_keyfile_data(encrypted.as_bytes(), Some("test-password")).unwrap();
        assert_eq!(decrypted, original);
    }

    #[test]
    fn sr25519_keyfile_roundtrip() {
        let original = Keypair::from_mnemonic(&test_mnemonic(), CRYPTO_SR25519, None).unwrap();
        let data = serialized_keypair_to_keyfile_data(&original).unwrap();
        let restored = deserialize_keypair_from_keyfile_data(&data).unwrap();
        assert_eq!(restored.crypto_type(), CRYPTO_SR25519);
        assert_eq!(restored.ss58_address(), original.ss58_address());
    }

    #[test]
    fn hashed_private_and_public_keyfiles_preserve_recovery_descriptor() {
        let original = Keypair::from_mnemonic(&test_mnemonic(), CRYPTO_HASHED, None)
            .unwrap()
            .at_generation(72)
            .unwrap();
        let private = serialized_keypair_to_keyfile_data(&original).unwrap();
        let recovered = deserialize_keypair_from_keyfile_data(&private)
            .unwrap()
            .at_generation(72)
            .unwrap();
        assert_eq!(
            recovered.hashed_public_key().unwrap(),
            original.hashed_public_key().unwrap()
        );
        assert_eq!(
            recovered.hashed_next_commitment().unwrap(),
            original.hashed_next_commitment().unwrap()
        );
        let public = original.public_only().unwrap();
        let public_data = serialized_keypair_to_keyfile_data(&public).unwrap();
        let public_recovered = deserialize_keypair_from_keyfile_data(&public_data).unwrap();
        assert_eq!(public_recovered.ss58_address(), original.ss58_address());
        assert_eq!(
            public_recovered.hashed_descriptor().unwrap(),
            original.hashed_descriptor().unwrap()
        );
        assert!(public_recovered.private_key_bytes().is_none());
        assert!(!String::from_utf8(public_data).unwrap().contains("secret"));
    }

    #[test]
    fn hashed_encrypted_original_backup_restores_after_later_rotations() {
        use codec::Decode;
        use sp_core::{sr25519, Pair};
        use subtensor_hashed::{transaction_payload, Proof, Scheme};

        let original = Keypair::from_mnemonic(
            &test_mnemonic(),
            CRYPTO_HASHED,
            Some("mnemonic derivation passphrase"),
        )
        .unwrap();
        let backup = serialized_keypair_to_keyfile_data(&original).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&backup).unwrap();
        // The encryption password and BIP39 derivation passphrase have
        // separate purposes. A phrase alone cannot recover this wallet.
        assert!(json.get("secretPhrase").is_none());
        assert!(json.get("secretSeed").is_some());
        drop(json);
        let encrypted = encrypt_keyfile_data(&backup, "keyfile encryption password").unwrap();
        let active = original.at_generation(9_001).unwrap();
        let account = active.public_key_bytes();
        let public_key = active.hashed_public_key().unwrap();
        let commitment = active.hashed_current_commitment().unwrap();
        let next_commitment = active.hashed_next_commitment().unwrap();
        drop(active);
        drop(original);
        drop(backup);

        assert!(matches!(
            decrypt_keyfile_data(&encrypted, Some("wrong encryption password")),
            Err(CoreError::WrongPassword(_)),
        ));
        let mut altered = encrypted.clone();
        *altered.last_mut().unwrap() ^= 1;
        assert!(matches!(
            decrypt_keyfile_data(&altered, Some("keyfile encryption password")),
            Err(CoreError::WrongPassword(_)),
        ));
        let decrypted =
            decrypt_keyfile_data(&encrypted, Some("keyfile encryption password")).unwrap();
        let restored = deserialize_keypair_from_keyfile_data(&decrypted).unwrap();
        assert_eq!(restored.hashed_generation().unwrap(), 0);
        // A new device needs only the original backup and the public chain
        // generation; no locally remembered signatures or later backups.
        let restored = restored.at_generation(9_001).unwrap();
        assert_eq!(restored.public_key_bytes(), account);
        assert_eq!(restored.hashed_public_key().unwrap(), public_key);
        assert_eq!(restored.hashed_current_commitment().unwrap(), commitment);
        assert_eq!(restored.hashed_next_commitment().unwrap(), next_commitment);
        let implication = b"first transaction after restoring";
        let encoded = restored.sign_hashed(implication).unwrap();
        let proof = Proof::decode(&mut &encoded[..]).unwrap();
        assert_eq!(proof.generation, 9_001);
        assert_eq!(proof.public_key, public_key);
        assert_eq!(proof.next_commitment, next_commitment);
        let payload = transaction_payload(
            &account,
            Scheme::Sr25519,
            9_001,
            &next_commitment,
            implication,
        );
        assert!(sr25519::Pair::verify(
            &sr25519::Signature::from_raw(proof.signature),
            payload,
            &sr25519::Public::from_raw(public_key)
        ));
    }

    #[test]
    fn hashed_keyfile_import_paths_check_the_same_descriptor() {
        let original = Keypair::from_mnemonic(&test_mnemonic(), CRYPTO_HASHED, None).unwrap();
        let encoded = serialized_keypair_to_keyfile_data(&original).unwrap();
        let full: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
        for secret_field in ["secretPhrase", "secretSeed", "privateKey"] {
            let mut one_secret = full.clone();
            for other in ["secretPhrase", "secretSeed", "privateKey"] {
                if other != secret_field {
                    one_secret.as_object_mut().unwrap().remove(other);
                }
            }
            let restored =
                deserialize_keypair_from_keyfile_data(&serde_json::to_vec(&one_secret).unwrap())
                    .unwrap();
            assert_eq!(restored.ss58_address(), original.ss58_address());
            assert_eq!(
                restored.hashed_commitment(301).unwrap(),
                original.hashed_commitment(301).unwrap()
            );
            one_secret[secret_field] = if secret_field == "secretPhrase" {
                json!("legal winner thank year wave sausage worth useful legal winner thank yellow")
            } else {
                json!(format!("0x{}", "13".repeat(32)))
            };
            assert!(
                deserialize_keypair_from_keyfile_data(&serde_json::to_vec(&one_secret).unwrap())
                    .is_err(),
                "a mismatched {secret_field} must not silently select another account"
            );
        }
    }

    #[test]
    fn hashed_keyfiles_never_fall_back_to_classical_or_unknown_profiles() {
        let original = Keypair::from_seed(&[6; 32], CRYPTO_HASHED).unwrap();
        let bytes = serialized_keypair_to_keyfile_data(&original).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let sibling = Keypair::from_seed(&[7; 32], CRYPTO_HASHED).unwrap();
        for update in [
            ("cryptoType", json!(CRYPTO_SR25519)),
            ("hashedDescriptor", json!("0xff01")),
            ("accountId", json!(format!("0x{}", "00".repeat(32)))),
            ("secretSeed", json!(format!("0x{}", "01".repeat(32)))),
            (
                "hashedDescriptor",
                json!(format!(
                    "0x{}",
                    hex::encode(sibling.hashed_descriptor().unwrap())
                )),
            ),
            (
                "publicKey",
                json!(format!("0x{}", hex::encode(sibling.public_key_bytes()))),
            ),
            ("ss58Address", json!(sibling.ss58_address())),
        ] {
            let mut altered = value.clone();
            altered[update.0] = update.1;
            assert!(
                deserialize_keypair_from_keyfile_data(&serde_json::to_vec(&altered).unwrap())
                    .is_err()
            );
        }
        let mut missing = value;
        missing.as_object_mut().unwrap().remove("hashedDescriptor");
        assert!(
            deserialize_keypair_from_keyfile_data(&serde_json::to_vec(&missing).unwrap()).is_err()
        );
    }

    #[test]
    fn legacy_keyfile_without_crypto_type_defaults_sr25519() {
        let expected = Keypair::from_mnemonic(&test_mnemonic(), CRYPTO_SR25519, None).unwrap();
        let json = format!(
            r#"{{"secretPhrase":"{}","ss58Address":"{}"}}"#,
            test_mnemonic(),
            expected.ss58_address()
        );
        let keypair = deserialize_keypair_from_keyfile_data(json.as_bytes()).unwrap();
        assert_eq!(keypair.crypto_type(), CRYPTO_SR25519);
        assert_eq!(keypair.ss58_address(), expected.ss58_address());
    }

    #[test]
    fn ed25519_keyfile_roundtrip() {
        let original = Keypair::from_mnemonic(&test_mnemonic(), CRYPTO_ED25519, None).unwrap();
        let data = serialized_keypair_to_keyfile_data(&original).unwrap();
        let restored = deserialize_keypair_from_keyfile_data(&data).unwrap();
        assert_eq!(restored.crypto_type(), CRYPTO_ED25519);
        assert_eq!(restored.ss58_address(), original.ss58_address());
    }

    #[test]
    fn mnemonic_keypair_writes_legacy_secret_fields() {
        let keypair = Keypair::from_mnemonic(&test_mnemonic(), CRYPTO_SR25519, None).unwrap();
        let data = serialized_keypair_to_keyfile_data(&keypair).unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&data).unwrap();
        assert_eq!(
            parsed.get("secretPhrase").and_then(|v| v.as_str()),
            Some(test_mnemonic().as_str())
        );
        let seed = parsed
            .get("secretSeed")
            .and_then(|v| v.as_str())
            .expect("secretSeed present");
        assert!(seed.starts_with("0x") && seed.len() == 66);
        assert!(parsed.get("privateKey").is_some());
        assert!(parsed.get("accountId").is_some());

        // The seed alone re-derives the same key.
        let seed_bytes = hex::decode(seed.trim_start_matches("0x")).unwrap();
        let from_seed = Keypair::from_seed(&seed_bytes, CRYPTO_SR25519).unwrap();
        assert_eq!(from_seed.ss58_address(), keypair.ss58_address());
    }

    #[test]
    fn mnemonic_with_derivation_password_omits_phrase_keeps_seed() {
        let keypair =
            Keypair::from_mnemonic(&test_mnemonic(), CRYPTO_SR25519, Some("hunter2")).unwrap();
        let data = serialized_keypair_to_keyfile_data(&keypair).unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&data).unwrap();
        assert!(parsed.get("secretPhrase").is_none());
        assert!(parsed.get("secretSeed").is_some());
        let restored = deserialize_keypair_from_keyfile_data(&data).unwrap();
        assert_eq!(restored.ss58_address(), keypair.ss58_address());
    }

    #[test]
    fn crypto_type_as_string_is_accepted() {
        let keypair = Keypair::from_mnemonic(&test_mnemonic(), CRYPTO_ED25519, None).unwrap();
        let json = format!(
            r#"{{"secretPhrase":"{}","cryptoType":"{}","ss58Address":"{}"}}"#,
            test_mnemonic(),
            CRYPTO_ED25519,
            keypair.ss58_address()
        );
        let restored = deserialize_keypair_from_keyfile_data(json.as_bytes()).unwrap();
        assert_eq!(restored.crypto_type(), CRYPTO_ED25519);
        assert_eq!(restored.ss58_address(), keypair.ss58_address());
    }

    #[test]
    fn missing_crypto_type_recovered_from_stored_ss58() {
        // A legacy ed25519 keyfile without cryptoType: the sr25519 default
        // mismatches the stored address, so the reader retries as ed25519.
        let keypair = Keypair::from_mnemonic(&test_mnemonic(), CRYPTO_ED25519, None).unwrap();
        let json = format!(
            r#"{{"secretPhrase":"{}","ss58Address":"{}"}}"#,
            test_mnemonic(),
            keypair.ss58_address()
        );
        let restored = deserialize_keypair_from_keyfile_data(json.as_bytes()).unwrap();
        assert_eq!(restored.crypto_type(), CRYPTO_ED25519);
        assert_eq!(restored.ss58_address(), keypair.ss58_address());
    }

    #[test]
    fn stored_ss58_mismatch_is_rejected() {
        let json = format!(
            r#"{{"secretPhrase":"{}","ss58Address":"5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY"}}"#,
            test_mnemonic()
        );
        let error = deserialize_keypair_from_keyfile_data(json.as_bytes())
            .err()
            .expect("mismatch must fail");
        assert!(error.to_string().contains("does not match"));
    }

    #[test]
    fn raw_hex_seed_fallback() {
        let keypair = Keypair::from_mnemonic(&test_mnemonic(), CRYPTO_SR25519, None).unwrap();
        let seed_hex = format!("0x{}", hex::encode(keypair.seed_bytes().unwrap()));
        let restored = deserialize_keypair_from_keyfile_data(seed_hex.as_bytes()).unwrap();
        assert_eq!(restored.ss58_address(), keypair.ss58_address());
    }

    #[test]
    fn raw_hex_private_key_fallback() {
        let keypair = Keypair::from_mnemonic(&test_mnemonic(), CRYPTO_SR25519, None).unwrap();
        let private_key = keypair.private_key_bytes().unwrap();
        assert_eq!(private_key.len(), 64);
        let private_hex = format!("0x{}", hex::encode(private_key));
        let restored = deserialize_keypair_from_keyfile_data(private_hex.as_bytes()).unwrap();
        assert_eq!(restored.ss58_address(), keypair.ss58_address());
    }

    #[test]
    fn raw_mnemonic_fallback() {
        let keypair = Keypair::from_mnemonic(&test_mnemonic(), CRYPTO_SR25519, None).unwrap();
        let restored = deserialize_keypair_from_keyfile_data(test_mnemonic().as_bytes()).unwrap();
        assert_eq!(restored.ss58_address(), keypair.ss58_address());
    }

    #[test]
    fn polkadotjs_export_gets_actionable_error() {
        let json = r#"{"encoded":"abc","encoding":{"content":["pkcs8","sr25519"],"type":["scrypt","xsalsa20-poly1305"],"version":"3"},"address":"5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY","meta":{}}"#;
        let error = deserialize_keypair_from_keyfile_data(json.as_bytes())
            .err()
            .expect("polkadotjs export must fail");
        assert!(error.to_string().contains("regen-coldkey --json-path"));
    }

    #[test]
    fn unknown_json_error_names_found_fields() {
        let error = deserialize_keypair_from_keyfile_data(br#"{"foo":1}"#)
            .err()
            .expect("unknown fields must fail");
        assert!(error.to_string().contains("foo"));
    }
}
