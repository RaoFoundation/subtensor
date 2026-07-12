//! Bittensor wallet keyfile encryption and JSON codec — compatible with the
//! ``bittensor-wallet`` on-disk format (absorbed from `py-sp-core`).

// Client-side code: slicing and arithmetic on locally validated buffers is
// the norm here, and this crate never runs inside the runtime.
#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use base64::{engine::general_purpose, Engine as _};
use fernet::Fernet;
use pbkdf2::pbkdf2_hmac;
use serde_json::json;
use sha2::Sha256;
use sodiumoxide::crypto::pwhash;
use sodiumoxide::crypto::secretbox;
use zeroize::{Zeroize, Zeroizing};

use crate::error::CoreError;
use crate::keys::{ensure_sodium, Keypair, CRYPTO_SR25519};

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

pub fn keypair_to_keyfile_data(
    keypair: &Keypair,
    password: Option<&str>,
) -> Result<Vec<u8>, CoreError> {
    let plaintext = Zeroizing::new(serialized_keypair_to_keyfile_data(keypair)?);
    if let Some(password) = password {
        return encrypt_keyfile_data(&plaintext, password);
    }
    if keypair.has_private_key() {
        return Err(key_err(
            "plaintext private key serialization is disabled; provide a password or write through save_keypair_to_keyfile",
        ));
    }
    Ok(plaintext.to_vec())
}

pub fn deserialize_keypair_from_keyfile(
    keyfile_data: &[u8],
    password: Option<&str>,
) -> Result<Keypair, CoreError> {
    if keyfile_data_is_encrypted(keyfile_data) {
        let plaintext = Zeroizing::new(decrypt_keyfile_data(keyfile_data, password)?);
        return deserialize_keypair_from_keyfile_data(&plaintext);
    }
    deserialize_keypair_from_keyfile_data(keyfile_data)
}

pub fn read_keypair_from_keyfile(
    path: &Path,
    password: Option<&str>,
) -> Result<Keypair, CoreError> {
    let keyfile_data = read_keyfile_bytes(path)?;
    deserialize_keypair_from_keyfile(&keyfile_data, password)
}

fn read_keyfile_bytes(path: &Path) -> Result<Zeroizing<Vec<u8>>, CoreError> {
    let mut file = open_keyfile_for_read(path)?;
    let mut keyfile_data = Zeroizing::new(Vec::new());
    file.read_to_end(&mut keyfile_data).map_err(|error| {
        key_err(format!(
            "failed to read keyfile {}: {error}",
            path.display()
        ))
    })?;
    Ok(keyfile_data)
}

#[cfg(unix)]
fn open_keyfile_for_read(path: &Path) -> Result<File, CoreError> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|error| {
            key_err(format!(
                "failed to open keyfile {} without following symlinks: {error}",
                path.display()
            ))
        })?;
    let metadata = file.metadata().map_err(|error| {
        key_err(format!(
            "failed to inspect opened keyfile {}: {error}",
            path.display()
        ))
    })?;
    if !metadata.is_file() {
        return Err(key_err(format!(
            "keyfile path {} is not a regular file",
            path.display()
        )));
    }
    Ok(file)
}

#[cfg(not(unix))]
fn open_keyfile_for_read(path: &Path) -> Result<File, CoreError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        key_err(format!(
            "failed to inspect keyfile {}: {error}",
            path.display()
        ))
    })?;
    if metadata.file_type().is_symlink() {
        return Err(key_err(format!(
            "refusing to read keyfile through symlink {}",
            path.display()
        )));
    }
    if !metadata.is_file() {
        return Err(key_err(format!(
            "keyfile path {} is not a regular file",
            path.display()
        )));
    }
    File::open(path).map_err(|error| {
        key_err(format!(
            "failed to open keyfile {}: {error}",
            path.display()
        ))
    })
}

pub fn save_keypair_to_keyfile(
    keypair: &Keypair,
    path: &Path,
    password: Option<&str>,
    overwrite: bool,
    allow_plaintext: bool,
) -> Result<(), CoreError> {
    if keypair.has_private_key() && password.is_none() && !allow_plaintext {
        return Err(key_err(
            "plaintext private keyfile writes are disabled; provide a password or set allow_plaintext",
        ));
    }

    let parent = path
        .parent()
        .ok_or_else(|| key_err("keyfile path must have a parent directory"))?;
    ensure_private_directory(parent)?;
    validate_keyfile_target(path, overwrite)?;

    let plaintext = Zeroizing::new(serialized_keypair_to_keyfile_data(keypair)?);
    let data = if let Some(password) = password {
        encrypt_keyfile_data(&plaintext, password)?
    } else {
        plaintext.to_vec()
    };
    atomic_write_keyfile(path, &data, overwrite)
}

fn ensure_private_directory(path: &Path) -> Result<(), CoreError> {
    fs::create_dir_all(path).map_err(|error| {
        key_err(format!(
            "failed to create wallet directory {}: {error}",
            path.display()
        ))
    })?;
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        key_err(format!(
            "failed to inspect wallet directory {}: {error}",
            path.display()
        ))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(key_err(format!(
            "wallet path {} must be a real directory",
            path.display()
        )));
    }
    set_private_directory_permissions(path)
}

#[cfg(unix)]
fn set_private_directory_permissions(path: &Path) -> Result<(), CoreError> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|error| {
        key_err(format!(
            "failed to set wallet directory permissions on {}: {error}",
            path.display()
        ))
    })
}

#[cfg(not(unix))]
fn set_private_directory_permissions(_path: &Path) -> Result<(), CoreError> {
    Ok(())
}

fn validate_keyfile_target(path: &Path, overwrite: bool) -> Result<(), CoreError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(key_err(format!(
                    "refusing to write keyfile through symlink {}",
                    path.display()
                )));
            }
            if !metadata.is_file() {
                return Err(key_err(format!(
                    "refusing to overwrite non-file keyfile path {}",
                    path.display()
                )));
            }
            if !overwrite {
                return Err(key_err(format!(
                    "keyfile {} already exists",
                    path.display()
                )));
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(key_err(format!(
            "failed to inspect keyfile {}: {error}",
            path.display()
        ))),
    }
}

fn atomic_write_keyfile(path: &Path, data: &[u8], overwrite: bool) -> Result<(), CoreError> {
    let dir = path
        .parent()
        .ok_or_else(|| key_err("keyfile path must have a parent directory"))?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| key_err("keyfile path must be valid UTF-8"))?;
    let mut temp_path = PathBuf::new();
    let mut temp_file = None;

    for attempt in 0..128u32 {
        let candidate = dir.join(format!(
            ".{file_name}.{}.{}.tmp",
            std::process::id(),
            attempt
        ));
        match private_create_new(&candidate) {
            Ok(file) => {
                temp_path = candidate;
                temp_file = Some(file);
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(key_err(format!(
                    "failed to create temporary keyfile {}: {error}",
                    candidate.display()
                )))
            }
        }
    }

    let mut file = temp_file.ok_or_else(|| {
        key_err(format!(
            "failed to allocate temporary keyfile for {}",
            path.display()
        ))
    })?;
    let write_result = (|| -> Result<(), CoreError> {
        file.write_all(data).map_err(|error| {
            key_err(format!(
                "failed to write temporary keyfile {}: {error}",
                temp_path.display()
            ))
        })?;
        file.sync_all().map_err(|error| {
            key_err(format!(
                "failed to fsync temporary keyfile {}: {error}",
                temp_path.display()
            ))
        })?;
        drop(file);
        if overwrite {
            fs::rename(&temp_path, path).map_err(|error| {
                key_err(format!(
                    "failed to atomically replace keyfile {}: {error}",
                    path.display()
                ))
            })?;
        } else {
            fs::hard_link(&temp_path, path).map_err(|error| {
                key_err(format!(
                    "failed to atomically create keyfile {}: {error}",
                    path.display()
                ))
            })?;
            fs::remove_file(&temp_path).map_err(|error| {
                key_err(format!(
                    "failed to remove temporary keyfile {}: {error}",
                    temp_path.display()
                ))
            })?;
        }
        set_private_file_permissions(path)?;
        fsync_directory(dir);
        Ok(())
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    write_result
}

#[cfg(unix)]
fn private_create_new(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn private_create_new(path: &Path) -> std::io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}

#[cfg(unix)]
fn set_private_file_permissions(path: &Path) -> Result<(), CoreError> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|error| {
        key_err(format!(
            "failed to set keyfile permissions on {}: {error}",
            path.display()
        ))
    })
}

#[cfg(not(unix))]
fn set_private_file_permissions(_path: &Path) -> Result<(), CoreError> {
    Ok(())
}

fn fsync_directory(path: &Path) {
    if let Ok(dir) = File::open(path) {
        let _ = dir.sync_all();
    }
}

pub fn deserialize_keypair_from_keyfile_data(keyfile_data: &[u8]) -> Result<Keypair, CoreError> {
    let decoded =
        std::str::from_utf8(keyfile_data).map_err(|_| key_err("failed to decode keyfile data"))?;

    let keyfile_dict: serde_json::Value =
        serde_json::from_str(decoded).map_err(|_| key_err("failed to parse keyfile data"))?;

    let crypto_type = keyfile_dict
        .get("cryptoType")
        .and_then(|value| match value {
            serde_json::Value::Number(number) => number.to_string().parse::<u8>().ok(),
            _ => None,
        })
        .unwrap_or(CRYPTO_SR25519);

    if let Some(secret_phrase) = keyfile_dict
        .get("secretPhrase")
        .and_then(|value| value.as_str())
    {
        return Keypair::from_mnemonic(secret_phrase, crypto_type, None);
    }

    if let Some(seed) = keyfile_dict
        .get("secretSeed")
        .and_then(|value| value.as_str())
    {
        let seed = seed.trim_start_matches("0x");
        let seed_bytes =
            hex::decode(seed).map_err(|error| key_err(format!("invalid secret seed: {error}")))?;
        return Keypair::from_seed(&seed_bytes, crypto_type);
    }

    if let Some(private_key) = keyfile_dict
        .get("privateKey")
        .and_then(|value| value.as_str())
    {
        let keypair = Keypair::from_private_key(private_key, crypto_type)?;
        // Some legacy btwallet keyfiles use a leading-space " ss58Address" key.
        if let Some(stored_ss58) = keyfile_dict
            .get("ss58Address")
            .or_else(|| keyfile_dict.get(" ss58Address"))
            .and_then(|value| value.as_str())
        {
            if keypair.ss58_address() != stored_ss58 {
                return Err(key_err(
                    "ss58Address in keyfile does not match the address derived from privateKey",
                ));
            }
        }
        return Ok(keypair);
    }

    if let Some(ss58) = keyfile_dict
        .get("ss58Address")
        .and_then(|value| value.as_str())
    {
        return Keypair::new(Some(ss58), None, crypto_type, 42);
    }

    Err(key_err("keypair could not be created from keyfile data"))
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
        let json = r#"{"secretPhrase":"abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about","ss58Address":"5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY"}"#;
        let keypair = deserialize_keypair_from_keyfile_data(json.as_bytes()).unwrap();
        assert_eq!(keypair.crypto_type(), CRYPTO_SR25519);
    }

    #[test]
    fn ed25519_keyfile_roundtrip() {
        let original = Keypair::from_mnemonic(&test_mnemonic(), CRYPTO_ED25519, None).unwrap();
        let data = serialized_keypair_to_keyfile_data(&original).unwrap();
        let restored = deserialize_keypair_from_keyfile_data(&data).unwrap();
        assert_eq!(restored.crypto_type(), CRYPTO_ED25519);
        assert_eq!(restored.ss58_address(), original.ss58_address());
    }
}
