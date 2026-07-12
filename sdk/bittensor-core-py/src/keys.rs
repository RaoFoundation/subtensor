//! Bindings for `bittensor_core::keys` and `keyfiles` — the `py_sp_core`
//! surface, preserved name-for-name.

use bittensor_core::keyfiles;
use bittensor_core::keys::{self, CRYPTO_SR25519, DEFAULT_SS58_FORMAT};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyBytes};

use crate::errors::to_py_err;

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

/// An sr25519 or ed25519 keypair backed by the workspace's sp-core.
#[pyclass]
pub struct Keypair {
    pub(crate) inner: keys::Keypair,
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
        let inner = keys::Keypair::new(ss58_address, public_key, crypto_type, ss58_format)
            .map_err(to_py_err)?;
        Ok(Self { inner })
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
        let inner =
            keys::Keypair::from_mnemonic(mnemonic, crypto_type, password).map_err(to_py_err)?;
        Ok(Self { inner })
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
        let inner = keys::Keypair::from_seed(seed, crypto_type).map_err(to_py_err)?;
        Ok(Self { inner })
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
        let inner = keys::Keypair::from_uri(uri, crypto_type).map_err(to_py_err)?;
        Ok(Self { inner })
    }

    /// Derive a keypair from a hex-encoded private key or seed bytes.
    #[staticmethod]
    #[pyo3(signature = (private_key, crypto_type=CRYPTO_SR25519))]
    fn create_from_private_key(private_key: &str, crypto_type: u8) -> PyResult<Self> {
        let inner = keys::Keypair::from_private_key(private_key, crypto_type).map_err(to_py_err)?;
        Ok(Self { inner })
    }

    /// Generate a fresh mnemonic with ``n_words`` (12 or 24).
    #[staticmethod]
    #[pyo3(signature = (n_words=12))]
    fn generate_mnemonic(n_words: usize) -> PyResult<String> {
        keys::Keypair::generate_mnemonic(n_words).map_err(to_py_err)
    }

    /// Import a keypair from a PolkadotJS encrypted JSON keystore (v3).
    #[staticmethod]
    #[pyo3(signature = (json_data, passphrase))]
    fn create_from_encrypted_json(json_data: &str, passphrase: &str) -> PyResult<Self> {
        let inner = keys::Keypair::from_encrypted_json(json_data, passphrase).map_err(to_py_err)?;
        Ok(Self { inner })
    }

    #[getter]
    fn crypto_type(&self) -> u8 {
        self.inner.crypto_type()
    }

    #[getter]
    fn public_key<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, &self.inner.public_key_bytes())
    }

    #[getter]
    fn ss58_address(&self) -> String {
        self.inner.ss58_address()
    }

    #[getter]
    fn ss58_format(&self) -> u16 {
        self.inner.ss58_format()
    }

    /// Sign a message; returns the raw 64-byte signature.
    #[pyo3(signature = (message))]
    fn sign<'py>(
        &self,
        py: Python<'py>,
        message: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let message = coerce_message_bytes(message)?;
        let signature = self.inner.sign(&message).map_err(to_py_err)?;
        Ok(PyBytes::new(py, &signature))
    }

    /// Verify a signature. Matches btwallet's ``<Bytes>`` wrapping fallback.
    fn verify(&self, message: &[u8], signature: &[u8]) -> PyResult<bool> {
        self.inner.verify(message, signature).map_err(to_py_err)
    }

    /// Encrypt a message to this keypair's ed25519 public key (sealed box).
    fn encrypt<'py>(&self, py: Python<'py>, message: &[u8]) -> PyResult<Bound<'py, PyBytes>> {
        let ciphertext = self.inner.encrypt(message).map_err(to_py_err)?;
        Ok(PyBytes::new(py, &ciphertext))
    }

    /// Decrypt a sealed-box ciphertext with this ed25519 keypair's private key.
    fn decrypt<'py>(&self, py: Python<'py>, ciphertext: &[u8]) -> PyResult<Bound<'py, PyBytes>> {
        let plaintext = self.inner.decrypt(ciphertext).map_err(to_py_err)?;
        Ok(PyBytes::new(py, &plaintext))
    }

    /// Encrypt a message for a recipient SS58 address (ed25519 sealed box).
    #[staticmethod]
    #[pyo3(signature = (ss58_address, message, crypto_type=keys::CRYPTO_ED25519))]
    fn encrypt_for<'py>(
        py: Python<'py>,
        ss58_address: &str,
        message: &[u8],
        crypto_type: u8,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let ciphertext =
            keys::Keypair::encrypt_for(ss58_address, message, crypto_type).map_err(to_py_err)?;
        Ok(PyBytes::new(py, &ciphertext))
    }
}

/// Verify a signature against an SS58 address without holding the secret key.
#[pyfunction]
#[pyo3(signature = (message, signature, ss58_address, crypto_type=CRYPTO_SR25519))]
fn verify(message: &[u8], signature: &[u8], ss58_address: &str, crypto_type: u8) -> PyResult<bool> {
    keys::verify(message, signature, ss58_address, crypto_type).map_err(to_py_err)
}

/// Decode an SS58 address to its raw 32-byte public key.
#[pyfunction]
fn ss58_decode<'py>(py: Python<'py>, ss58_address: &str) -> PyResult<Bound<'py, PyBytes>> {
    let public_key = keys::public_key_from_ss58(ss58_address).map_err(to_py_err)?;
    Ok(PyBytes::new(py, &public_key))
}

/// Encode a raw 32-byte public key as an SS58 address.
#[pyfunction]
#[pyo3(signature = (public_key, ss58_format=DEFAULT_SS58_FORMAT))]
fn ss58_encode(public_key: &[u8], ss58_format: u16) -> PyResult<String> {
    let public_key = <[u8; 32]>::try_from(public_key)
        .map_err(|_| value_err("public key must be exactly 32 bytes"))?;
    Ok(keys::ss58_from_public(public_key, ss58_format))
}

#[pyfunction]
fn serialized_keypair_to_keyfile_data<'py>(
    py: Python<'py>,
    keypair: &Keypair,
) -> PyResult<Bound<'py, PyBytes>> {
    let bytes = keyfiles::serialized_keypair_to_keyfile_data(&keypair.inner).map_err(to_py_err)?;
    Ok(PyBytes::new(py, &bytes))
}

#[pyfunction]
fn deserialize_keypair_from_keyfile_data(keyfile_data: &[u8]) -> PyResult<Keypair> {
    let inner = keyfiles::deserialize_keypair_from_keyfile_data(keyfile_data).map_err(to_py_err)?;
    Ok(Keypair { inner })
}

#[pyfunction]
#[pyo3(signature = (keyfile_data, password))]
fn encrypt_keyfile_data<'py>(
    py: Python<'py>,
    keyfile_data: &[u8],
    password: &str,
) -> PyResult<Bound<'py, PyBytes>> {
    let bytes = keyfiles::encrypt_keyfile_data(keyfile_data, password).map_err(to_py_err)?;
    Ok(PyBytes::new(py, &bytes))
}

#[pyfunction]
#[pyo3(signature = (keyfile_data, password=None))]
fn decrypt_keyfile_data<'py>(
    py: Python<'py>,
    keyfile_data: &[u8],
    password: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let bytes = keyfiles::decrypt_keyfile_data(keyfile_data, password).map_err(to_py_err)?;
    Ok(PyBytes::new(py, &bytes))
}

#[pyfunction]
fn keyfile_data_is_encrypted(keyfile_data: &[u8]) -> bool {
    keyfiles::keyfile_data_is_encrypted(keyfile_data)
}

#[pyfunction]
fn keyfile_data_is_encrypted_nacl(keyfile_data: &[u8]) -> bool {
    keyfiles::keyfile_data_is_encrypted_nacl(keyfile_data)
}

#[pyfunction]
fn keyfile_data_is_encrypted_ansible(keyfile_data: &[u8]) -> bool {
    keyfiles::keyfile_data_is_encrypted_ansible(keyfile_data)
}

#[pyfunction]
fn keyfile_data_is_encrypted_legacy(keyfile_data: &[u8]) -> bool {
    keyfiles::keyfile_data_is_encrypted_legacy(keyfile_data)
}

#[pyfunction]
fn keyfile_data_encryption_method(keyfile_data: &[u8]) -> &'static str {
    keyfiles::keyfile_data_encryption_method(keyfile_data)
}

#[pyfunction]
fn get_password_from_environment(env_var_name: &str) -> PyResult<Option<String>> {
    keyfiles::get_password_from_environment(env_var_name).map_err(to_py_err)
}

#[pyfunction]
fn save_password_to_environment(env_var_name: &str, password: &str) -> PyResult<String> {
    keyfiles::save_password_to_environment(env_var_name, password).map_err(to_py_err)
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Keypair>()?;
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
    m.add("CRYPTO_ED25519", keys::CRYPTO_ED25519)?;
    m.add("CRYPTO_SR25519", keys::CRYPTO_SR25519)?;
    Ok(())
}
