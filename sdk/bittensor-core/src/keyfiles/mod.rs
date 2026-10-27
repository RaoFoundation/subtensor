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
use std::path::{Component, Path, PathBuf};

use base64::{engine::general_purpose, Engine as _};
use fernet::Fernet;
use pbkdf2::pbkdf2_hmac;
use serde_json::json;
use sha2::Sha256;
use sodiumoxide::crypto::pwhash;
use sodiumoxide::crypto::secretbox;
use zeroize::{Zeroize, Zeroizing};

use crate::error::CoreError;
use crate::keys::{
    ensure_sodium, is_hashed_crypto, is_pq_crypto, Keypair, CRYPTO_ED25519, CRYPTO_SR25519,
};

const NACL_SALT: &[u8] = b"\x13q\x83\xdf\xf1Z\t\xbc\x9c\x90\xb5Q\x879\xe9\xb1";
const LEGACY_SALT: &[u8] = b"Iguesscyborgslikemyselfhaveatendencytobeparanoidaboutourorigins";

fn key_err(msg: impl Into<String>) -> CoreError {
    CoreError::Keyfile(msg.into())
}

fn require_non_empty_password(password: &str) -> Result<&str, CoreError> {
    if password.is_empty() {
        return Err(key_err("keyfile password must not be empty"));
    }
    Ok(password)
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
    let password = require_non_empty_password(password)?;
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
    if is_hashed_crypto(keypair.crypto_type()) || is_pq_crypto(keypair.crypto_type()) {
        let field = if is_pq_crypto(keypair.crypto_type()) {
            data.insert("walletVersion", json!(2));
            "mldsaDescriptor"
        } else {
            "hashedDescriptor"
        };
        if let Ok(descriptor) = keypair.authorization_descriptor() {
            data.insert(field, json!(format!("0x{}", hex::encode(descriptor))));
        }
        // A persisted rotating generation would be stale after use on another
        // machine. Restore the identity; select rotating authority from chain.
    }

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

pub fn save_keypair_pair_to_keyfiles(
    private_keypair: &Keypair,
    private_path: &Path,
    private_password: Option<&str>,
    public_keypair: &Keypair,
    public_path: &Path,
    overwrite: bool,
    allow_plaintext: bool,
) -> Result<(), CoreError> {
    if private_path == public_path {
        return Err(key_err("private and public keyfile paths must be distinct"));
    }
    if private_keypair.has_private_key() && private_password.is_none() && !allow_plaintext {
        return Err(key_err(
            "plaintext private keyfile writes are disabled; provide a password or set allow_plaintext",
        ));
    }
    if public_keypair.has_private_key() {
        return Err(key_err(
            "public keyfile pair member must not contain a private key",
        ));
    }

    prepare_keyfile_target(private_path, overwrite)?;
    prepare_keyfile_target(public_path, overwrite)?;

    let private_plaintext = Zeroizing::new(serialized_keypair_to_keyfile_data(private_keypair)?);
    let private_data = if let Some(password) = private_password {
        encrypt_keyfile_data(&private_plaintext, password)?
    } else {
        private_plaintext.to_vec()
    };
    let public_data = serialized_keypair_to_keyfile_data(public_keypair)?;
    atomic_write_keyfile_pair(
        private_path,
        &private_data,
        public_path,
        &public_data,
        overwrite,
    )
}

fn prepare_keyfile_target(path: &Path, overwrite: bool) -> Result<(), CoreError> {
    let parent = path
        .parent()
        .ok_or_else(|| key_err("keyfile path must have a parent directory"))?;
    ensure_private_directory(parent)?;
    validate_keyfile_target(path, overwrite)
}

fn ensure_private_directory(path: &Path) -> Result<(), CoreError> {
    let path = normalize_directory_path(path);
    reject_symlink_ancestors(path)?;
    create_missing_private_directories(path)?;
    reject_symlink_ancestors(path)?;
    validate_wallet_directory(path)
}

fn normalize_directory_path(path: &Path) -> &Path {
    if path.as_os_str().is_empty() {
        Path::new(".")
    } else {
        path
    }
}

fn create_missing_private_directories(path: &Path) -> Result<(), CoreError> {
    let mut current = PathBuf::new();
    let mut saw_component = false;

    for component in path.components() {
        match component {
            Component::CurDir => continue,
            Component::ParentDir => {
                return Err(key_err(format!(
                    "wallet path {} must not contain parent directory components",
                    path.display()
                )));
            }
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir | Component::Normal(_) => current.push(component.as_os_str()),
        }
        saw_component = true;
        if current.as_os_str().is_empty() {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) => validate_wallet_directory_metadata(path, &current, &metadata)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match fs::create_dir(&current) {
                    Ok(()) => set_private_directory_permissions(&current)?,
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                        let metadata = fs::symlink_metadata(&current).map_err(|error| {
                            key_err(format!(
                                "failed to inspect wallet directory {}: {error}",
                                current.display()
                            ))
                        })?;
                        validate_wallet_directory_metadata(path, &current, &metadata)?;
                    }
                    Err(error) => {
                        return Err(key_err(format!(
                            "failed to create wallet directory {}: {error}",
                            current.display()
                        )));
                    }
                }
            }
            Err(error) => {
                return Err(key_err(format!(
                    "failed to inspect wallet path ancestor {}: {error}",
                    current.display()
                )));
            }
        }
    }

    if !saw_component {
        validate_wallet_directory(Path::new("."))?;
    }
    Ok(())
}

fn validate_wallet_directory(path: &Path) -> Result<(), CoreError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        key_err(format!(
            "failed to inspect wallet directory {}: {error}",
            path.display()
        ))
    })?;
    validate_wallet_directory_metadata(path, path, &metadata)
}

fn validate_wallet_directory_metadata(
    path: &Path,
    current: &Path,
    metadata: &fs::Metadata,
) -> Result<(), CoreError> {
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(key_err(format!(
            "wallet path {} must be a real directory",
            if current == path {
                path.display().to_string()
            } else {
                current.display().to_string()
            }
        )));
    }
    Ok(())
}

fn reject_symlink_ancestors(path: &Path) -> Result<(), CoreError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => continue,
            Component::ParentDir => {
                return Err(key_err(format!(
                    "wallet path {} must not contain parent directory components",
                    path.display()
                )));
            }
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir | Component::Normal(_) => current.push(component.as_os_str()),
        }
        if current.as_os_str().is_empty() {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(key_err(format!(
                        "wallet path {} must not contain symlink ancestor {}",
                        path.display(),
                        current.display()
                    )));
                }
                if !metadata.is_dir() {
                    return Err(key_err(format!(
                        "wallet path ancestor {} must be a directory",
                        current.display()
                    )));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(key_err(format!(
                    "failed to inspect wallet path ancestor {}: {error}",
                    current.display()
                )));
            }
        }
    }
    Ok(())
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
    let temp_path = write_temp_keyfile(path, data)?;
    let write_result = commit_one_keyfile(path, &temp_path, overwrite);
    if write_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    write_result
}

fn write_temp_keyfile(path: &Path, data: &[u8]) -> Result<PathBuf, CoreError> {
    let dir = path
        .parent()
        .ok_or_else(|| key_err("keyfile path must have a parent directory"))?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| key_err("keyfile path must be valid UTF-8"))?;
    let mut temp_file = None;
    let mut temp_path = None;

    for attempt in 0..128u32 {
        let candidate = dir.join(format!(
            ".{file_name}.{}.{}.tmp",
            std::process::id(),
            attempt
        ));
        match private_create_new(&candidate) {
            Ok(file) => {
                temp_path = Some(candidate);
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

    let temp_path = temp_path.ok_or_else(|| {
        key_err(format!(
            "failed to allocate temporary keyfile for {}",
            path.display()
        ))
    })?;
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
        Ok(())
    })();
    drop(file);
    if write_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    write_result.map(|()| temp_path)
}

fn commit_one_keyfile(path: &Path, temp_path: &Path, overwrite: bool) -> Result<(), CoreError> {
    if overwrite {
        return commit_overwrite_keyfile(path, temp_path);
    }
    commit_new_keyfile(path, temp_path)
}

fn commit_overwrite_keyfile(path: &Path, temp_path: &Path) -> Result<(), CoreError> {
    let dir = path
        .parent()
        .ok_or_else(|| key_err("keyfile path must have a parent directory"))?;
    let mut backup = None;
    let mut committed = false;
    let result = (|| -> Result<(), CoreError> {
        backup = move_existing_to_backup(path)?;
        fs::rename(temp_path, path).map_err(|error| {
            key_err(format!(
                "failed to atomically replace keyfile {}: {error}",
                path.display()
            ))
        })?;
        committed = true;
        set_private_file_permissions(path)?;
        fsync_directory(dir);
        Ok(())
    })();

    if result.is_err() {
        if committed {
            remove_file_if_exists(path);
        }
        restore_backup(path, backup.as_deref());
        fsync_directory(dir);
    } else if let Some(path) = backup {
        let _ = fs::remove_file(path);
    }
    result
}

fn commit_new_keyfile(path: &Path, temp_path: &Path) -> Result<(), CoreError> {
    let dir = path
        .parent()
        .ok_or_else(|| key_err("keyfile path must have a parent directory"))?;
    let mut created = false;
    let result = (|| -> Result<(), CoreError> {
        fs::hard_link(temp_path, path).map_err(|error| {
            key_err(format!(
                "failed to atomically create keyfile {}: {error}",
                path.display()
            ))
        })?;
        created = true;
        fs::remove_file(temp_path).map_err(|error| {
            key_err(format!(
                "failed to remove temporary keyfile {}: {error}",
                temp_path.display()
            ))
        })?;
        set_private_file_permissions(path)?;
        fsync_directory(dir);
        Ok(())
    })();
    if result.is_err() && created {
        remove_file_if_exists(path);
        fsync_directory(dir);
    }
    result
}

fn atomic_write_keyfile_pair(
    private_path: &Path,
    private_data: &[u8],
    public_path: &Path,
    public_data: &[u8],
    overwrite: bool,
) -> Result<(), CoreError> {
    let private_temp = write_temp_keyfile(private_path, private_data)?;
    let public_temp = match write_temp_keyfile(public_path, public_data) {
        Ok(path) => path,
        Err(error) => {
            let _ = fs::remove_file(&private_temp);
            return Err(error);
        }
    };

    let result = if overwrite {
        commit_overwrite_keyfile_pair(private_path, &private_temp, public_path, &public_temp)
    } else {
        commit_create_keyfile_pair(private_path, &private_temp, public_path, &public_temp)
    };
    if result.is_err() {
        let _ = fs::remove_file(&private_temp);
        let _ = fs::remove_file(&public_temp);
    }
    result
}

fn commit_create_keyfile_pair(
    private_path: &Path,
    private_temp: &Path,
    public_path: &Path,
    public_temp: &Path,
) -> Result<(), CoreError> {
    let mut private_created = false;
    let mut public_created = false;
    let result = (|| -> Result<(), CoreError> {
        fs::hard_link(private_temp, private_path).map_err(|error| {
            key_err(format!(
                "failed to atomically create private keyfile {}: {error}",
                private_path.display()
            ))
        })?;
        private_created = true;
        fs::remove_file(private_temp).map_err(|error| {
            key_err(format!(
                "failed to remove temporary keyfile {}: {error}",
                private_temp.display()
            ))
        })?;
        set_private_file_permissions(private_path)?;

        fs::hard_link(public_temp, public_path).map_err(|error| {
            key_err(format!(
                "failed to atomically create public keyfile {}: {error}",
                public_path.display()
            ))
        })?;
        public_created = true;
        fs::remove_file(public_temp).map_err(|error| {
            key_err(format!(
                "failed to remove temporary keyfile {}: {error}",
                public_temp.display()
            ))
        })?;
        set_private_file_permissions(public_path)?;
        fsync_parent(private_path);
        fsync_parent(public_path);
        Ok(())
    })();

    if result.is_err() {
        if public_created {
            remove_file_if_exists(public_path);
        }
        if private_created {
            remove_file_if_exists(private_path);
        }
        fsync_parent(private_path);
        fsync_parent(public_path);
    }
    result
}

fn commit_overwrite_keyfile_pair(
    private_path: &Path,
    private_temp: &Path,
    public_path: &Path,
    public_temp: &Path,
) -> Result<(), CoreError> {
    let mut private_backup = None;
    let mut public_backup = None;
    let mut private_committed = false;
    let mut public_committed = false;
    let result = (|| -> Result<(), CoreError> {
        private_backup = move_existing_to_backup(private_path)?;
        public_backup = move_existing_to_backup(public_path)?;

        fs::rename(private_temp, private_path).map_err(|error| {
            key_err(format!(
                "failed to commit private keyfile {}: {error}",
                private_path.display()
            ))
        })?;
        private_committed = true;
        set_private_file_permissions(private_path)?;

        fs::rename(public_temp, public_path).map_err(|error| {
            key_err(format!(
                "failed to commit public keyfile {}: {error}",
                public_path.display()
            ))
        })?;
        public_committed = true;
        set_private_file_permissions(public_path)?;

        fsync_parent(private_path);
        fsync_parent(public_path);
        Ok(())
    })();

    if result.is_err() {
        if public_committed {
            remove_file_if_exists(public_path);
        }
        if private_committed {
            remove_file_if_exists(private_path);
        }
        restore_backup(private_path, private_backup.as_deref());
        restore_backup(public_path, public_backup.as_deref());
        fsync_parent(private_path);
        fsync_parent(public_path);
    } else {
        if let Some(path) = private_backup {
            let _ = fs::remove_file(path);
        }
        if let Some(path) = public_backup {
            let _ = fs::remove_file(path);
        }
    }
    result
}

fn move_existing_to_backup(path: &Path) -> Result<Option<PathBuf>, CoreError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(key_err(format!(
                    "refusing to overwrite keyfile symlink {}",
                    path.display()
                )));
            }
            if !metadata.is_file() {
                return Err(key_err(format!(
                    "refusing to overwrite non-file keyfile path {}",
                    path.display()
                )));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(key_err(format!(
                "failed to inspect keyfile {}: {error}",
                path.display()
            )))
        }
    }

    let dir = path
        .parent()
        .ok_or_else(|| key_err("keyfile path must have a parent directory"))?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| key_err("keyfile path must be valid UTF-8"))?;
    for attempt in 0..128u32 {
        let backup = dir.join(format!(
            ".{file_name}.{}.{}.rollback",
            std::process::id(),
            attempt
        ));
        if backup.exists() {
            continue;
        }
        match fs::rename(path, &backup) {
            Ok(()) => return Ok(Some(backup)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(key_err(format!(
                    "failed to stage existing keyfile {} for rollback: {error}",
                    path.display()
                )))
            }
        }
    }
    Err(key_err(format!(
        "failed to allocate rollback path for {}",
        path.display()
    )))
}

fn restore_backup(path: &Path, backup: Option<&Path>) {
    if let Some(backup) = backup {
        remove_file_if_exists(path);
        let _ = fs::rename(backup, path);
    }
}

fn remove_file_if_exists(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => {}
    }
}

fn fsync_parent(path: &Path) {
    if let Some(parent) = path.parent() {
        fsync_directory(parent);
    }
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
    let descriptor_field = if is_pq_crypto(crypto_type) {
        if keyfile
            .get("walletVersion")
            .and_then(serde_json::Value::as_u64)
            != Some(2)
            || keyfile.get("hashedDescriptor").is_some()
        {
            return Err(key_err(
                "post-quantum keyfiles require walletVersion 2 and mldsaDescriptor",
            ));
        }
        "mldsaDescriptor"
    } else {
        if keyfile.get("mldsaDescriptor").is_some() {
            return Err(key_err("v2 descriptor requires post-quantum cryptoType"));
        }
        "hashedDescriptor"
    };
    let public = match keyfile.get(descriptor_field) {
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
            if private.authorization_descriptor()? != public.authorization_descriptor()? {
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

    if is_hashed_crypto(crypto_type) || is_pq_crypto(crypto_type) {
        return deserialize_hashed(&keyfile_dict, crypto_type);
    }
    if keyfile_dict.get("hashedDescriptor").is_some()
        || keyfile_dict.get("mldsaDescriptor").is_some()
    {
        return Err(key_err(
            "wallet descriptors require a hashed or post-quantum cryptoType",
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
        for code in [crate::keys::CRYPTO_HASHED_ED25519] {
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

    fn temp_test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "bittensor-core-keyfiles-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
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
        use subtensor_hashed::Proof;

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
        assert!(sr25519::Pair::verify(
            &sr25519::Signature::from_raw(proof.signature),
            subtensor_hashed::SignerAddress {
                version: 1,
                scheme: subtensor_hashed::Scheme::Sr25519,
                account,
                generation: proof.generation,
                public_key,
                next_commitment,
            }
            .signing_hash(implication),
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
    fn keypair_pair_keyfile_roundtrip() {
        let dir = temp_test_dir("pair-roundtrip");
        let private_path = dir.join("hotkey");
        let public_path = dir.join("hotkeypub.txt");
        let original = Keypair::from_mnemonic(&test_mnemonic(), CRYPTO_SR25519, None).unwrap();
        let public = Keypair::new(
            Some(&original.ss58_address()),
            None,
            original.crypto_type(),
            original.ss58_format(),
        )
        .unwrap();

        save_keypair_pair_to_keyfiles(
            &original,
            &private_path,
            Some("test-password"),
            &public,
            &public_path,
            false,
            false,
        )
        .unwrap();

        let restored_private =
            read_keypair_from_keyfile(&private_path, Some("test-password")).unwrap();
        let restored_public = read_keypair_from_keyfile(&public_path, None).unwrap();
        assert_eq!(restored_private.ss58_address(), original.ss58_address());
        assert_eq!(restored_public.ss58_address(), original.ss58_address());
        assert!(!restored_public.has_private_key());
        assert!(keyfile_data_is_encrypted(&fs::read(&private_path).unwrap()));
        assert!(!keyfile_data_is_encrypted(&fs::read(&public_path).unwrap()));

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn empty_passwords_are_rejected_by_low_level_keyfile_apis() {
        let keypair = Keypair::from_mnemonic(&test_mnemonic(), CRYPTO_SR25519, None).unwrap();
        let public_keypair = Keypair::new(
            Some(&keypair.ss58_address()),
            None,
            keypair.crypto_type(),
            keypair.ss58_format(),
        )
        .unwrap();
        let dir = temp_test_dir("empty-password");

        assert!(encrypt_keyfile_data(b"{}", "").is_err());
        assert!(keypair_to_keyfile_data(&keypair, Some("")).is_err());
        assert!(
            save_keypair_to_keyfile(&keypair, &dir.join("hotkey"), Some(""), false, false,)
                .is_err()
        );
        assert!(save_keypair_pair_to_keyfiles(
            &keypair,
            &dir.join("hotkey-pair"),
            Some(""),
            &public_keypair,
            &dir.join("hotkeypub.txt"),
            false,
            false,
        )
        .is_err());

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn writing_keyfile_does_not_chmod_existing_parent_directory() {
        use std::os::unix::fs::PermissionsExt;

        let dir = temp_test_dir("existing-parent-permissions");
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        let nested = dir.join("wallet");
        let keypair = Keypair::from_mnemonic(&test_mnemonic(), CRYPTO_SR25519, None).unwrap();

        save_keypair_to_keyfile(
            &keypair,
            &nested.join("hotkey"),
            Some("test-password"),
            false,
            false,
        )
        .unwrap();

        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o755
        );
        assert_eq!(
            fs::metadata(&nested).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(nested.join("hotkey"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn rejects_symlink_wallet_directory_ancestor() {
        use std::os::unix::fs::symlink;

        let dir = temp_test_dir("symlink-ancestor");
        let real = dir.join("real");
        let link = dir.join("link");
        fs::create_dir(&real).unwrap();
        symlink(&real, &link).unwrap();
        let keypair = Keypair::from_mnemonic(&test_mnemonic(), CRYPTO_SR25519, None).unwrap();

        let err = save_keypair_to_keyfile(
            &keypair,
            &link.join("wallet").join("hotkey"),
            Some("test-password"),
            false,
            false,
        )
        .expect_err("symlink ancestors must be rejected");
        assert!(
            err.to_string().contains("symlink ancestor"),
            "unexpected error: {err}"
        );

        fs::remove_dir_all(dir).unwrap();
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
    fn derived_keyfile_does_not_restore_parent_mnemonic_or_seed() {
        for crypto_type in [CRYPTO_SR25519, CRYPTO_ED25519] {
            let parent = Keypair::from_mnemonic(&test_mnemonic(), crypto_type, None).unwrap();
            let child = parent.derive("//child").unwrap();
            assert_ne!(child.ss58_address(), parent.ss58_address());

            let data = serialized_keypair_to_keyfile_data(&child).unwrap();
            let parsed: serde_json::Value = serde_json::from_slice(&data).unwrap();
            assert!(parsed.get("secretPhrase").is_none());
            assert!(parsed.get("secretSeed").is_none());
            let restored = deserialize_keypair_from_keyfile_data(&data).unwrap();
            assert_eq!(restored.ss58_address(), child.ss58_address());
            let signature = restored.sign(b"derived keyfile roundtrip").unwrap();
            assert!(child
                .verify(b"derived keyfile roundtrip", &signature)
                .unwrap());
        }
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
