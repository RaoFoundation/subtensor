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
use crate::keys::{ensure_sodium, Keypair, CRYPTO_ED25519, CRYPTO_SR25519};

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

    fn test_mnemonic() -> String {
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
            .to_string()
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
