//! Bittensor wallet keyfile encryption — compatible with ``bittensor-wallet`` on-disk format.

use base64::{engine::general_purpose, Engine as _};
use fernet::Fernet;
use pbkdf2::pbkdf2_hmac;
use pyo3::prelude::*;
use sha2::Sha256;
use sodiumoxide::crypto::pwhash;
use sodiumoxide::crypto::secretbox;

use crate::{KeyfileError, WrongPasswordError};

const NACL_SALT: &[u8] = b"\x13q\x83\xdf\xf1Z\t\xbc\x9c\x90\xb5Q\x879\xe9\xb1";
const LEGACY_SALT: &[u8] = b"Iguesscyborgslikemyselfhaveatendencytobeparanoidaboutourorigins";

pub fn key_err(msg: impl Into<String>) -> PyErr {
    PyErr::new::<KeyfileError, _>(msg.into())
}

pub fn wrong_password_err(msg: impl Into<String>) -> PyErr {
    PyErr::new::<WrongPasswordError, _>(msg.into())
}

pub fn ensure_sodium() -> PyResult<()> {
    sodiumoxide::init().map_err(|_| key_err("failed to initialize libsodium"))?;
    Ok(())
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

fn derive_key(password: &[u8]) -> secretbox::Key {
    let salt = pwhash::argon2i13::Salt::from_slice(NACL_SALT).expect("invalid NACL salt");
    let mut key = secretbox::Key([0; secretbox::KEYBYTES]);
    pwhash::argon2i13::derive_key(
        &mut key.0,
        password,
        &salt,
        pwhash::argon2i13::OPSLIMIT_SENSITIVE,
        pwhash::argon2i13::MEMLIMIT_SENSITIVE,
    )
    .expect("failed to derive NaCl key");
    key
}

fn nacl_decrypt(keyfile_data: &[u8], key: &secretbox::Key) -> PyResult<Vec<u8>> {
    let data = &keyfile_data[5..];
    if data.len() < secretbox::NONCEBYTES {
        return Err(key_err("invalid NaCl keyfile: too short"));
    }
    let nonce = secretbox::Nonce::from_slice(&data[..secretbox::NONCEBYTES])
        .ok_or_else(|| key_err("invalid NaCl nonce"))?;
    let ciphertext = &data[secretbox::NONCEBYTES..];
    secretbox::open(ciphertext, &nonce, key)
        .map_err(|_| wrong_password_err("wrong password for NaCl decryption"))
}

pub fn encrypt_keyfile_data(keyfile_data: &[u8], password: &str) -> PyResult<Vec<u8>> {
    ensure_sodium()?;
    let key = derive_key(password.as_bytes());
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

fn decrypt_password(data: &[u8], key: &str) -> PyResult<String> {
    let decrypted_bytes = xor_with_key(data, key);
    String::from_utf8(decrypted_bytes)
        .map_err(|_| key_err("invalid wallet password env var: corrupt UTF-8"))
}

pub fn get_password_from_environment(env_var_name: &str) -> PyResult<Option<String>> {
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

pub fn save_password_to_environment(env_var_name: &str, password: &str) -> PyResult<String> {
    let encrypted = xor_with_key(password.as_bytes(), env_var_name);
    std::env::set_var(env_var_name, general_purpose::STANDARD.encode(encrypted));
    Ok(env_var_name.to_string())
}

fn legacy_decrypt(password: &str, keyfile_data: &[u8]) -> PyResult<Vec<u8>> {
    let mut key = [0u8; 32];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), LEGACY_SALT, 10_000_000, &mut key);
    let fernet_key = general_purpose::URL_SAFE.encode(key);
    let fernet = Fernet::new(&fernet_key).ok_or_else(|| key_err("invalid legacy fernet key"))?;
    let keyfile_data_str = std::str::from_utf8(keyfile_data)
        .map_err(|e| key_err(format!("legacy keyfile is not valid utf-8: {e}")))?;
    fernet
        .decrypt(keyfile_data_str)
        .map_err(|_| wrong_password_err("wrong password for legacy decryption"))
}

pub fn decrypt_keyfile_data(keyfile_data: &[u8], password: Option<&str>) -> PyResult<Vec<u8>> {
    ensure_sodium()?;
    let password = password.ok_or_else(|| key_err("password required to decrypt keyfile"))?;

    if keyfile_data_is_encrypted_nacl(keyfile_data) {
        let key = derive_key(password.as_bytes());
        return nacl_decrypt(keyfile_data, &key);
    }

    if keyfile_data_is_encrypted_ansible(keyfile_data) {
        let decrypted = ansible_vault::decrypt_vault(keyfile_data, password)
            .map_err(|_| wrong_password_err("wrong password for ansible vault decryption"))?;
        return Ok(decrypted);
    }

    if keyfile_data_is_encrypted_legacy(keyfile_data) {
        return legacy_decrypt(password, keyfile_data);
    }

    Err(key_err("invalid or unknown keyfile encryption method"))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
