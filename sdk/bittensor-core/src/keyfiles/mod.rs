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
use crate::keys::{ensure_sodium, Keypair, CRYPTO_SR25519};

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
}
