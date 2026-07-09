//! Python bindings for Substrate `sp-core` key primitives.
//!
//! This crate is the monorepo's replacement for the parts of the external
//! `bittensor-wallet` (btwallet) package that wrap sp-core: keypair
//! derivation, signing, verification, and SS58 encoding. Because it builds
//! against the same `sp-core` revision as the runtime, the SDK's crypto can
//! never drift from the chain's.

use pyo3::exceptions::{PyException, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyBytes};
use sodiumoxide::crypto::box_;
use sodiumoxide::crypto::sealedbox;
use sodiumoxide::crypto::sign::ed25519 as sign_ed25519;
use sp_core::crypto::{AccountId32, Pair as PairT, Ss58AddressFormat, Ss58Codec};
use sp_core::{ed25519, sr25519, ByteArray};

mod encrypted_json;
mod keyfile;
mod keyfile_codec;

pyo3::create_exception!(py_sp_core, WrongPasswordError, PyException);
pyo3::create_exception!(py_sp_core, KeyfileError, PyException);

/// Crypto type codes, matching the py-substrate-interface / btwallet convention.
pub const CRYPTO_ED25519: u8 = 0;
pub const CRYPTO_SR25519: u8 = 1;

pub(crate) const DEFAULT_SS58_FORMAT: u16 = 42;

fn value_err(msg: impl Into<String>) -> PyErr {
    PyValueError::new_err(msg.into())
}

fn coerce_message_bytes(message: &Bound<'_, PyAny>) -> PyResult<Vec<u8>> {
    if let Ok(text) = message.extract::<&str>() {
        if let Some(hex_part) = text.strip_prefix("0x") {
            return hex::decode(hex_part)
                .map_err(|error| value_err(format!("invalid hex message: {error}")));
        }
        return Ok(text.as_bytes().to_vec());
    }
    if let Ok(bytes) = message.extract::<&[u8]>() {
        return Ok(bytes.to_vec());
    }
    Err(value_err("message must be str or bytes"))
}

fn ensure_sodium() -> PyResult<()> {
    sodiumoxide::init().map_err(|_| value_err("failed to initialize libsodium"))?;
    Ok(())
}

// sp-core's CryptoBytes has several AsRef impls; pin down the byte view.
fn as_bytes<T: AsRef<[u8]>>(value: &T) -> Vec<u8> {
    value.as_ref().to_vec()
}

fn public_key_from_ss58(ss58_address: &str) -> PyResult<[u8; 32]> {
    let account = AccountId32::from_ss58check(ss58_address)
        .map_err(|e| value_err(format!("invalid ss58 address: {e:?}")))?;
    Ok(account.into())
}

fn ss58_from_public(public_key: [u8; 32], ss58_format: u16) -> String {
    AccountId32::from(public_key).to_ss58check_with_version(Ss58AddressFormat::custom(ss58_format))
}

fn verify_with_crypto(
    crypto_type: u8,
    public_key: &[u8; 32],
    message: &[u8],
    signature: &[u8],
) -> PyResult<bool> {
    match crypto_type {
        CRYPTO_SR25519 => {
            let public = sr25519::Public::from_raw(*public_key);
            let sig = sr25519::Signature::from_slice(signature)
                .map_err(|_| value_err("invalid sr25519 signature length"))?;
            Ok(sr25519::Pair::verify(&sig, message, &public))
        }
        CRYPTO_ED25519 => {
            let public = ed25519::Public::from_raw(*public_key);
            let sig = ed25519::Signature::from_slice(signature)
                .map_err(|_| value_err("invalid ed25519 signature length"))?;
            Ok(ed25519::Pair::verify(&sig, message, &public))
        }
        other => Err(value_err(format!("unknown crypto type {other}"))),
    }
}

fn ed25519_to_x25519_pk(public_key: &[u8; 32]) -> PyResult<box_::PublicKey> {
    ensure_sodium()?;
    let ed25519_pk = sign_ed25519::PublicKey::from_slice(public_key)
        .ok_or_else(|| value_err("invalid ed25519 public key"))?;
    sign_ed25519::to_curve25519_pk(&ed25519_pk)
        .map_err(|_| value_err("failed to convert ed25519 public key to x25519"))
}

fn ed25519_x25519_from_pair(pair: &ed25519::Pair) -> PyResult<(box_::PublicKey, box_::SecretKey)> {
    ensure_sodium()?;
    let seed_bytes = pair.to_raw_vec();
    let seed = sign_ed25519::Seed::from_slice(&seed_bytes)
        .or_else(|| sign_ed25519::Seed::from_slice(&seed_bytes[..32.min(seed_bytes.len())]))
        .ok_or_else(|| value_err("failed to derive x25519 keypair for decryption"))?;
    let (pk, sk) = sign_ed25519::keypair_from_seed(&seed);
    let x25519_pk = sign_ed25519::to_curve25519_pk(&pk)
        .map_err(|_| value_err("failed to derive x25519 keypair for decryption"))?;
    let x25519_sk = sign_ed25519::to_curve25519_sk(&sk)
        .map_err(|_| value_err("failed to derive x25519 keypair for decryption"))?;
    Ok((x25519_pk, x25519_sk))
}

// Each Keypair is a Python-heap object holding exactly one variant; the
// size skew between ed25519 and public-only is irrelevant here.
#[allow(clippy::large_enum_variant)]
pub(crate) enum KeypairInner {
    Ed25519(ed25519::Pair),
    Sr25519(sr25519::Pair),
    PublicOnly {
        public_key: [u8; 32],
        crypto_type: u8,
    },
}

impl KeypairInner {
    fn private_key_bytes(&self) -> Option<Vec<u8>> {
        match self {
            KeypairInner::Ed25519(pair) => Some(pair.to_raw_vec()),
            KeypairInner::Sr25519(pair) => Some(pair.to_raw_vec()),
            KeypairInner::PublicOnly { .. } => None,
        }
    }
}

/// An sr25519 or ed25519 keypair backed by the workspace's sp-core.
#[pyclass]
pub struct Keypair {
    inner: KeypairInner,
    ss58_format: u16,
}

#[pymethods]
impl Keypair {
    /// Public-only or full keypair from SS58 address and/or raw public key bytes.
    #[new]
    #[pyo3(signature = (ss58_address=None, public_key=None, crypto_type=CRYPTO_SR25519, ss58_format=DEFAULT_SS58_FORMAT))]
    fn py_new(
        ss58_address: Option<&str>,
        public_key: Option<&[u8]>,
        crypto_type: u8,
        ss58_format: u16,
    ) -> PyResult<Self> {
        match crypto_type {
            CRYPTO_SR25519 | CRYPTO_ED25519 => {}
            other => return Err(value_err(format!("unknown crypto type {other}"))),
        }

        let public_key = match (ss58_address, public_key) {
            (Some(addr), None) => public_key_from_ss58(addr)?,
            (None, Some(pk)) => <[u8; 32]>::try_from(pk)
                .map_err(|_| value_err("public key must be exactly 32 bytes"))?,
            (Some(addr), Some(pk)) => {
                let from_addr = public_key_from_ss58(addr)?;
                let from_pk = <[u8; 32]>::try_from(pk)
                    .map_err(|_| value_err("public key must be exactly 32 bytes"))?;
                if from_addr != from_pk {
                    return Err(value_err(
                        "ss58 address and public key refer to different accounts",
                    ));
                }
                from_pk
            }
            (None, None) => {
                return Err(value_err(
                    "no ss58 formatted address or public key provided",
                ));
            }
        };

        Ok(Self {
            inner: KeypairInner::PublicOnly {
                public_key,
                crypto_type,
            },
            ss58_format,
        })
    }

    /// Derive a keypair from a BIP39 mnemonic (with optional password).
    #[staticmethod]
    #[pyo3(signature = (mnemonic, crypto_type=CRYPTO_SR25519, password=None))]
    fn from_mnemonic(mnemonic: &str, crypto_type: u8, password: Option<&str>) -> PyResult<Self> {
        Self::create_from_mnemonic(mnemonic, crypto_type, password)
    }

    /// Btwallet-compatible alias for :meth:`from_mnemonic`.
    #[staticmethod]
    #[pyo3(signature = (mnemonic, crypto_type=CRYPTO_SR25519, password=None))]
    fn create_from_mnemonic(
        mnemonic: &str,
        crypto_type: u8,
        password: Option<&str>,
    ) -> PyResult<Self> {
        let inner = match crypto_type {
            CRYPTO_SR25519 => {
                let (pair, _seed) = sr25519::Pair::from_phrase(mnemonic, password)
                    .map_err(|e| value_err(format!("invalid mnemonic: {e:?}")))?;
                KeypairInner::Sr25519(pair)
            }
            CRYPTO_ED25519 => {
                let (pair, _seed) = ed25519::Pair::from_phrase(mnemonic, password)
                    .map_err(|e| value_err(format!("invalid mnemonic: {e:?}")))?;
                KeypairInner::Ed25519(pair)
            }
            other => return Err(value_err(format!("unknown crypto type {other}"))),
        };
        Ok(Self {
            inner,
            ss58_format: DEFAULT_SS58_FORMAT,
        })
    }

    /// Derive a keypair from a 32-byte seed.
    #[staticmethod]
    #[pyo3(signature = (seed, crypto_type=CRYPTO_SR25519))]
    fn from_seed(seed: &[u8], crypto_type: u8) -> PyResult<Self> {
        Self::create_from_seed(seed, crypto_type)
    }

    /// Btwallet-compatible alias for :meth:`from_seed`.
    #[staticmethod]
    #[pyo3(signature = (seed, crypto_type=CRYPTO_SR25519))]
    fn create_from_seed(seed: &[u8], crypto_type: u8) -> PyResult<Self> {
        let inner = match crypto_type {
            CRYPTO_SR25519 => KeypairInner::Sr25519(
                sr25519::Pair::from_seed_slice(seed)
                    .map_err(|e| value_err(format!("invalid seed: {e:?}")))?,
            ),
            CRYPTO_ED25519 => KeypairInner::Ed25519(
                ed25519::Pair::from_seed_slice(seed)
                    .map_err(|e| value_err(format!("invalid seed: {e:?}")))?,
            ),
            other => return Err(value_err(format!("unknown crypto type {other}"))),
        };
        Ok(Self {
            inner,
            ss58_format: DEFAULT_SS58_FORMAT,
        })
    }

    /// Derive a keypair from a secret URI (e.g. "//Alice" or "<mnemonic>//hard/soft").
    #[staticmethod]
    #[pyo3(signature = (uri, crypto_type=CRYPTO_SR25519))]
    fn from_uri(uri: &str, crypto_type: u8) -> PyResult<Self> {
        Self::create_from_uri(uri, crypto_type)
    }

    /// Btwallet-compatible alias for :meth:`from_uri`.
    #[staticmethod]
    #[pyo3(signature = (uri, crypto_type=CRYPTO_SR25519))]
    fn create_from_uri(uri: &str, crypto_type: u8) -> PyResult<Self> {
        let inner = match crypto_type {
            CRYPTO_SR25519 => KeypairInner::Sr25519(
                sr25519::Pair::from_string(uri, None)
                    .map_err(|e| value_err(format!("invalid secret uri: {e:?}")))?,
            ),
            CRYPTO_ED25519 => KeypairInner::Ed25519(
                ed25519::Pair::from_string(uri, None)
                    .map_err(|e| value_err(format!("invalid secret uri: {e:?}")))?,
            ),
            other => return Err(value_err(format!("unknown crypto type {other}"))),
        };
        Ok(Self {
            inner,
            ss58_format: DEFAULT_SS58_FORMAT,
        })
    }

    /// Derive a keypair from a hex-encoded private key or seed bytes.
    #[staticmethod]
    #[pyo3(signature = (private_key, crypto_type=CRYPTO_SR25519))]
    fn create_from_private_key(private_key: &str, crypto_type: u8) -> PyResult<Self> {
        let private_key_vec = hex::decode(private_key.trim_start_matches("0x"))
            .map_err(|error| value_err(format!("invalid private_key string: {error}")))?;

        let inner = match crypto_type {
            CRYPTO_SR25519 => {
                KeypairInner::Sr25519(sr25519::Pair::from_seed_slice(&private_key_vec).map_err(
                    |error| value_err(format!("invalid sr25519 private key: {error:?}")),
                )?)
            }
            CRYPTO_ED25519 => {
                let seed = if private_key_vec.len() >= 32 {
                    &private_key_vec[..32]
                } else {
                    return Err(value_err("ed25519 private key must be at least 32 bytes"));
                };
                KeypairInner::Ed25519(ed25519::Pair::from_seed_slice(seed).map_err(|error| {
                    value_err(format!("invalid ed25519 private key: {error:?}"))
                })?)
            }
            other => return Err(value_err(format!("unknown crypto type {other}"))),
        };

        Ok(Self {
            inner,
            ss58_format: DEFAULT_SS58_FORMAT,
        })
    }

    /// Generate a fresh mnemonic with ``n_words`` (12 or 24).
    #[staticmethod]
    #[pyo3(signature = (n_words=12))]
    fn generate_mnemonic(n_words: usize) -> PyResult<String> {
        use bip39::{Language, Mnemonic};

        if !matches!(n_words, 12 | 15 | 18 | 21 | 24) {
            return Err(value_err(format!(
                "unsupported mnemonic length {n_words}; expected 12, 15, 18, 21, or 24"
            )));
        }

        Mnemonic::generate_in(Language::English, n_words)
            .map(|mnemonic| mnemonic.to_string())
            .map_err(|error| value_err(format!("failed to generate mnemonic: {error}")))
    }

    /// Import a keypair from a PolkadotJS encrypted JSON keystore (v3).
    #[staticmethod]
    #[pyo3(signature = (json_data, passphrase))]
    fn create_from_encrypted_json(json_data: &str, passphrase: &str) -> PyResult<Self> {
        encrypted_json::create_from_encrypted_json(json_data, passphrase)
    }

    #[getter]
    fn crypto_type(&self) -> u8 {
        match &self.inner {
            KeypairInner::Ed25519(_) => CRYPTO_ED25519,
            KeypairInner::Sr25519(_) => CRYPTO_SR25519,
            KeypairInner::PublicOnly { crypto_type, .. } => *crypto_type,
        }
    }

    #[getter]
    fn public_key<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, &self.public_key_bytes())
    }

    #[getter]
    fn ss58_address(&self) -> String {
        ss58_from_public(self.public_key_bytes(), self.ss58_format)
    }

    #[getter]
    fn ss58_format(&self) -> u16 {
        self.ss58_format
    }

    /// Sign a message; returns the raw 64-byte signature.
    #[pyo3(signature = (message))]
    fn sign<'py>(
        &self,
        py: Python<'py>,
        message: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let message = coerce_message_bytes(message)?;
        let signature = match &self.inner {
            KeypairInner::Ed25519(pair) => as_bytes(&pair.sign(&message)),
            KeypairInner::Sr25519(pair) => as_bytes(&pair.sign(&message)),
            KeypairInner::PublicOnly { .. } => {
                return Err(value_err("no private key set to create signatures"));
            }
        };
        Ok(PyBytes::new(py, &signature))
    }

    /// Verify a signature. Matches btwallet's ``<Bytes>`` wrapping fallback.
    fn verify(&self, message: &[u8], signature: &[u8]) -> PyResult<bool> {
        let public_key = self.public_key_bytes();
        let crypto_type = self.crypto_type();
        if verify_with_crypto(crypto_type, &public_key, message, signature)? {
            return Ok(true);
        }
        let wrapped = [b"<Bytes>", message, b"</Bytes>"].concat();
        verify_with_crypto(crypto_type, &public_key, &wrapped, signature)
    }

    /// Encrypt a message to this keypair's ed25519 public key (sealed box).
    fn encrypt<'py>(&self, py: Python<'py>, message: &[u8]) -> PyResult<Bound<'py, PyBytes>> {
        if self.crypto_type() != CRYPTO_ED25519 {
            return Err(value_err(
                "encrypt/decrypt is only supported for ed25519 keypairs",
            ));
        }
        ensure_sodium()?;
        let x25519_pk = ed25519_to_x25519_pk(&self.public_key_bytes())?;
        let ciphertext = sealedbox::seal(message, &x25519_pk);
        Ok(PyBytes::new(py, &ciphertext))
    }

    /// Decrypt a sealed-box ciphertext with this ed25519 keypair's private key.
    fn decrypt<'py>(&self, py: Python<'py>, ciphertext: &[u8]) -> PyResult<Bound<'py, PyBytes>> {
        if self.crypto_type() != CRYPTO_ED25519 {
            return Err(value_err(
                "encrypt/decrypt is only supported for ed25519 keypairs",
            ));
        }
        let KeypairInner::Ed25519(pair) = &self.inner else {
            return Err(value_err(
                "decryption requires a keypair with a private key",
            ));
        };
        ensure_sodium()?;
        let (x25519_pk, x25519_sk) = ed25519_x25519_from_pair(pair)?;
        let plaintext = sealedbox::open(ciphertext, &x25519_pk, &x25519_sk)
            .map_err(|_| value_err("decryption failed: invalid ciphertext or wrong key"))?;
        Ok(PyBytes::new(py, &plaintext))
    }

    /// Encrypt a message for a recipient SS58 address (ed25519 sealed box).
    #[staticmethod]
    #[pyo3(signature = (ss58_address, message, crypto_type=CRYPTO_ED25519))]
    fn encrypt_for<'py>(
        py: Python<'py>,
        ss58_address: &str,
        message: &[u8],
        crypto_type: u8,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let recipient = Self::py_new(Some(ss58_address), None, crypto_type, DEFAULT_SS58_FORMAT)?;
        recipient.encrypt(py, message)
    }
}

impl Keypair {
    fn public_key_bytes(&self) -> [u8; 32] {
        match &self.inner {
            KeypairInner::Ed25519(pair) => pair.public().0,
            KeypairInner::Sr25519(pair) => pair.public().0,
            KeypairInner::PublicOnly { public_key, .. } => *public_key,
        }
    }

    pub(crate) fn private_key_bytes(&self) -> Option<Vec<u8>> {
        self.inner.private_key_bytes()
    }

    pub(crate) fn from_inner(inner: KeypairInner, ss58_format: u16) -> Self {
        Self { inner, ss58_format }
    }
}

/// Verify a signature against an SS58 address without holding the secret key.
#[pyfunction]
#[pyo3(signature = (message, signature, ss58_address, crypto_type=CRYPTO_SR25519))]
fn verify(message: &[u8], signature: &[u8], ss58_address: &str, crypto_type: u8) -> PyResult<bool> {
    let public_key = public_key_from_ss58(ss58_address)?;
    if verify_with_crypto(crypto_type, &public_key, message, signature)? {
        return Ok(true);
    }
    let wrapped = [b"<Bytes>", message, b"</Bytes>"].concat();
    verify_with_crypto(crypto_type, &public_key, &wrapped, signature)
}

/// Decode an SS58 address to its raw 32-byte public key.
#[pyfunction]
fn ss58_decode<'py>(py: Python<'py>, ss58_address: &str) -> PyResult<Bound<'py, PyBytes>> {
    let public_key = public_key_from_ss58(ss58_address)?;
    Ok(PyBytes::new(py, &public_key))
}

/// Encode a raw 32-byte public key as an SS58 address.
#[pyfunction]
#[pyo3(signature = (public_key, ss58_format=DEFAULT_SS58_FORMAT))]
fn ss58_encode(public_key: &[u8], ss58_format: u16) -> PyResult<String> {
    let public_key = <[u8; 32]>::try_from(public_key)
        .map_err(|_| value_err("public key must be exactly 32 bytes"))?;
    Ok(ss58_from_public(public_key, ss58_format))
}

#[pyfunction]
fn serialized_keypair_to_keyfile_data<'py>(
    py: Python<'py>,
    keypair: &Keypair,
) -> PyResult<Bound<'py, PyBytes>> {
    let bytes = keyfile_codec::serialized_keypair_to_keyfile_data(keypair)?;
    Ok(PyBytes::new(py, &bytes))
}

#[pyfunction]
fn deserialize_keypair_from_keyfile_data(keyfile_data: &[u8]) -> PyResult<Keypair> {
    keyfile_codec::deserialize_keypair_from_keyfile_data(keyfile_data)
}

#[pyfunction]
#[pyo3(signature = (keyfile_data, password))]
fn encrypt_keyfile_data<'py>(
    py: Python<'py>,
    keyfile_data: &[u8],
    password: &str,
) -> PyResult<Bound<'py, PyBytes>> {
    let bytes = keyfile::encrypt_keyfile_data(keyfile_data, password)?;
    Ok(PyBytes::new(py, &bytes))
}

#[pyfunction]
#[pyo3(signature = (keyfile_data, password=None))]
fn decrypt_keyfile_data<'py>(
    py: Python<'py>,
    keyfile_data: &[u8],
    password: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let bytes = keyfile::decrypt_keyfile_data(keyfile_data, password)?;
    Ok(PyBytes::new(py, &bytes))
}

#[pyfunction]
fn keyfile_data_is_encrypted(keyfile_data: &[u8]) -> bool {
    keyfile::keyfile_data_is_encrypted(keyfile_data)
}

#[pyfunction]
fn keyfile_data_is_encrypted_nacl(keyfile_data: &[u8]) -> bool {
    keyfile::keyfile_data_is_encrypted_nacl(keyfile_data)
}

#[pyfunction]
fn keyfile_data_is_encrypted_ansible(keyfile_data: &[u8]) -> bool {
    keyfile::keyfile_data_is_encrypted_ansible(keyfile_data)
}

#[pyfunction]
fn keyfile_data_is_encrypted_legacy(keyfile_data: &[u8]) -> bool {
    keyfile::keyfile_data_is_encrypted_legacy(keyfile_data)
}

#[pyfunction]
fn keyfile_data_encryption_method(keyfile_data: &[u8]) -> &'static str {
    keyfile::keyfile_data_encryption_method(keyfile_data)
}

#[pyfunction]
fn get_password_from_environment(env_var_name: &str) -> PyResult<Option<String>> {
    keyfile::get_password_from_environment(env_var_name)
}

#[pyfunction]
fn save_password_to_environment(env_var_name: &str, password: &str) -> PyResult<String> {
    keyfile::save_password_to_environment(env_var_name, password)
}

#[pymodule]
fn py_sp_core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    ensure_sodium()?;
    m.add_class::<Keypair>()?;
    m.add(
        "WrongPasswordError",
        m.py().get_type::<WrongPasswordError>(),
    )?;
    m.add("KeyfileError", m.py().get_type::<KeyfileError>())?;
    m.add_function(wrap_pyfunction!(verify, m)?)?;
    m.add_function(wrap_pyfunction!(ss58_decode, m)?)?;
    m.add_function(wrap_pyfunction!(ss58_encode, m)?)?;
    // Backwards-compatible aliases for pre-migration bittensor.sp_core names.
    m.add("verify_signature", m.getattr("verify")?)?;
    m.add("decode_ss58", m.getattr("ss58_decode")?)?;
    m.add("encode_ss58", m.getattr("ss58_encode")?)?;
    m.add_function(wrap_pyfunction!(serialized_keypair_to_keyfile_data, m)?)?;
    m.add_function(wrap_pyfunction!(deserialize_keypair_from_keyfile_data, m)?)?;
    m.add_function(wrap_pyfunction!(encrypt_keyfile_data, m)?)?;
    m.add_function(wrap_pyfunction!(decrypt_keyfile_data, m)?)?;
    m.add_function(wrap_pyfunction!(keyfile_data_is_encrypted, m)?)?;
    m.add_function(wrap_pyfunction!(keyfile_data_is_encrypted_nacl, m)?)?;
    m.add_function(wrap_pyfunction!(keyfile_data_is_encrypted_ansible, m)?)?;
    m.add_function(wrap_pyfunction!(keyfile_data_is_encrypted_legacy, m)?)?;
    m.add_function(wrap_pyfunction!(keyfile_data_encryption_method, m)?)?;
    m.add_function(wrap_pyfunction!(get_password_from_environment, m)?)?;
    m.add_function(wrap_pyfunction!(save_password_to_environment, m)?)?;
    m.add("CRYPTO_ED25519", CRYPTO_ED25519)?;
    m.add("CRYPTO_SR25519", CRYPTO_SR25519)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alice_uri_matches_known_address() {
        let kp = Keypair::create_from_uri("//Alice", CRYPTO_SR25519).unwrap();
        assert_eq!(
            kp.ss58_address(),
            "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY"
        );
    }

    #[test]
    fn public_only_from_ss58_matches_full_key() {
        let full = Keypair::create_from_uri("//Alice", CRYPTO_SR25519).unwrap();
        let public = Keypair::py_new(Some(&full.ss58_address()), None, CRYPTO_SR25519, 42).unwrap();
        assert_eq!(public.ss58_address(), full.ss58_address());
        assert_eq!(public.public_key_bytes(), full.public_key_bytes());
    }

    #[test]
    fn sr25519_verify_roundtrip() {
        let kp = Keypair::create_from_uri("//Alice", CRYPTO_SR25519).unwrap();
        let message = b"hello";
        let sig = match &kp.inner {
            KeypairInner::Sr25519(pair) => pair.sign(message).0.to_vec(),
            _ => panic!("expected sr25519"),
        };
        assert!(kp.verify(message, &sig).unwrap());
    }

    #[test]
    fn sr25519_verify_bytes_wrapping() {
        let kp = Keypair::create_from_uri("//Alice", CRYPTO_SR25519).unwrap();
        let sig = match &kp.inner {
            KeypairInner::Sr25519(pair) => pair.sign(b"<Bytes>hello</Bytes>").0.to_vec(),
            _ => panic!("expected sr25519"),
        };
        assert!(kp.verify(b"hello", &sig).unwrap());
    }
}
