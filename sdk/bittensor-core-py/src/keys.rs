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

/// A native keypair or versioned hashed account backed by workspace crypto.
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
    #[pyo3(signature = (mnemonic, crypto_type=CRYPTO_SR25519, password=None, account_type=None))]
    fn from_mnemonic(
        mnemonic: &str,
        crypto_type: u8,
        password: Option<&str>,
        account_type: Option<&str>,
    ) -> PyResult<Self> {
        Self::create_from_mnemonic(mnemonic, crypto_type, password, account_type)
    }

    /// Btwallet-compatible alias for :meth:`from_mnemonic`.
    #[staticmethod]
    #[pyo3(signature = (mnemonic, crypto_type=CRYPTO_SR25519, password=None, account_type=None))]
    fn create_from_mnemonic(
        mnemonic: &str,
        crypto_type: u8,
        password: Option<&str>,
        account_type: Option<&str>,
    ) -> PyResult<Self> {
        let crypto_type =
            keys::account_crypto_type(crypto_type, account_type).map_err(to_py_err)?;
        let inner =
            keys::Keypair::from_mnemonic(mnemonic, crypto_type, password).map_err(to_py_err)?;
        Ok(Self { inner })
    }

    /// Derive a keypair from a 32-byte seed.
    #[staticmethod]
    #[pyo3(signature = (seed, crypto_type=CRYPTO_SR25519, account_type=None))]
    fn from_seed(seed: &[u8], crypto_type: u8, account_type: Option<&str>) -> PyResult<Self> {
        Self::create_from_seed(seed, crypto_type, account_type)
    }

    /// Btwallet-compatible alias for :meth:`from_seed`.
    #[staticmethod]
    #[pyo3(signature = (seed, crypto_type=CRYPTO_SR25519, account_type=None))]
    fn create_from_seed(
        seed: &[u8],
        crypto_type: u8,
        account_type: Option<&str>,
    ) -> PyResult<Self> {
        let crypto_type =
            keys::account_crypto_type(crypto_type, account_type).map_err(to_py_err)?;
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
    #[pyo3(signature = (private_key, crypto_type=CRYPTO_SR25519, account_type=None))]
    fn create_from_private_key(
        private_key: &str,
        crypto_type: u8,
        account_type: Option<&str>,
    ) -> PyResult<Self> {
        let crypto_type =
            keys::account_crypto_type(crypto_type, account_type).map_err(to_py_err)?;
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
    fn account_type(&self) -> &'static str {
        self.inner.account_type()
    }

    #[getter]
    fn signing_scheme(&self) -> &'static str {
        self.inner.signing_scheme()
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

    /// Restore a public hashed account descriptor without private material.
    #[staticmethod]
    #[pyo3(signature = (descriptor, ss58_format=DEFAULT_SS58_FORMAT))]
    fn from_hashed_descriptor(descriptor: &[u8], ss58_format: u16) -> PyResult<Self> {
        Ok(Self {
            inner: keys::Keypair::from_hashed_descriptor(descriptor, ss58_format)
                .map_err(to_py_err)?,
        })
    }

    fn public_only(&self) -> PyResult<Self> {
        Ok(Self {
            inner: self.inner.public_only().map_err(to_py_err)?,
        })
    }

    fn at_generation(&self, generation: u64) -> PyResult<Self> {
        Ok(Self {
            inner: self.inner.at_generation(generation).map_err(to_py_err)?,
        })
    }

    #[getter]
    fn hashed_generation(&self) -> PyResult<u64> {
        self.inner.authorization_generation().map_err(to_py_err)
    }

    #[getter]
    fn hashed_descriptor<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        Ok(PyBytes::new(
            py,
            &self.inner.authorization_descriptor().map_err(to_py_err)?,
        ))
    }

    /// Network-independent receiving address containing only the descriptor and checksum.
    fn hashed_receiving_address(&self) -> PyResult<String> {
        self.inner.hashed_receiving_address().map_err(to_py_err)
    }

    #[getter]
    fn hashed_public_key<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        Ok(PyBytes::new(
            py,
            &self.inner.authorization_public_key().map_err(to_py_err)?,
        ))
    }

    #[getter]
    fn hashed_signing_public_key<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        Ok(PyBytes::new(
            py,
            &self.inner.authorization_public_key().map_err(to_py_err)?,
        ))
    }

    #[getter]
    fn hashed_current_commitment<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        Ok(PyBytes::new(
            py,
            &self
                .inner
                .authorization_current_commitment()
                .map_err(to_py_err)?,
        ))
    }

    #[getter]
    fn hashed_next_commitment<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        Ok(PyBytes::new(
            py,
            &self
                .inner
                .authorization_next_commitment()
                .map_err(to_py_err)?,
        ))
    }

    fn hashed_commitment<'py>(
        &self,
        py: Python<'py>,
        generation: u64,
    ) -> PyResult<Bound<'py, PyBytes>> {
        Ok(PyBytes::new(
            py,
            &self
                .inner
                .authorization_commitment(generation)
                .map_err(to_py_err)?,
        ))
    }

    /// Sign a fully composed standard v4 payload for a rotating account.
    fn sign_hashed<'py>(
        &self,
        py: Python<'py>,
        implication: &[u8],
    ) -> PyResult<Bound<'py, PyBytes>> {
        Ok(PyBytes::new(
            py,
            &self
                .inner
                .sign_authorization(implication)
                .map_err(to_py_err)?,
        ))
    }

    #[staticmethod]
    #[pyo3(signature = (descriptor, ss58_format=DEFAULT_SS58_FORMAT))]
    fn from_mldsa_descriptor(descriptor: &[u8], ss58_format: u16) -> PyResult<Self> {
        Ok(Self {
            inner: keys::Keypair::from_mldsa_descriptor(descriptor, ss58_format)
                .map_err(to_py_err)?,
        })
    }

    #[getter]
    fn mldsa_descriptor<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        Ok(PyBytes::new(
            py,
            &self.inner.mldsa_descriptor().map_err(to_py_err)?,
        ))
    }
    #[getter]
    fn mldsa_generation(&self) -> PyResult<u64> {
        self.inner.mldsa_generation().map_err(to_py_err)
    }
    #[getter]
    fn mldsa_public_key<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        Ok(PyBytes::new(
            py,
            &self.inner.mldsa_public_key().map_err(to_py_err)?,
        ))
    }
    #[getter]
    fn mldsa_current_commitment<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        Ok(PyBytes::new(
            py,
            &self.inner.mldsa_current_commitment().map_err(to_py_err)?,
        ))
    }
    #[getter]
    fn mldsa_next_commitment<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        Ok(PyBytes::new(
            py,
            &self.inner.mldsa_next_commitment().map_err(to_py_err)?,
        ))
    }
    fn mldsa_commitment<'py>(
        &self,
        py: Python<'py>,
        generation: u64,
    ) -> PyResult<Bound<'py, PyBytes>> {
        Ok(PyBytes::new(
            py,
            &self.inner.mldsa_commitment(generation).map_err(to_py_err)?,
        ))
    }
    fn sign_mldsa<'py>(&self, py: Python<'py>, payload: &[u8]) -> PyResult<Bound<'py, PyBytes>> {
        Ok(PyBytes::new(
            py,
            &self.inner.sign_mldsa(payload).map_err(to_py_err)?,
        ))
    }
    fn mldsa_receiving_address(&self) -> PyResult<String> {
        keys::encode_mldsa_receiving_address(&self.inner.mldsa_descriptor().map_err(to_py_err)?)
            .map_err(to_py_err)
    }
    fn mldsa_capability_public_key<'py>(
        &self,
        py: Python<'py>,
        role: u8,
        epoch: u64,
    ) -> PyResult<Bound<'py, PyBytes>> {
        Ok(PyBytes::new(
            py,
            &self
                .inner
                .mldsa_capability_public_key(role, epoch)
                .map_err(to_py_err)?,
        ))
    }
    fn mldsa_capability_commitment<'py>(
        &self,
        py: Python<'py>,
        role: u8,
        epoch: u64,
    ) -> PyResult<Bound<'py, PyBytes>> {
        Ok(PyBytes::new(
            py,
            &self
                .inner
                .mldsa_capability_commitment(role, epoch)
                .map_err(to_py_err)?,
        ))
    }
    #[pyo3(signature = (role, epoch, genesis_hash, payload, owner=None))]
    fn sign_mldsa_capability<'py>(
        &self,
        py: Python<'py>,
        role: u8,
        epoch: u64,
        genesis_hash: &[u8],
        payload: &[u8],
        owner: Option<&[u8]>,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let owner = owner
            .map(|bytes| {
                bytes
                    .try_into()
                    .map_err(|_| value_err("owner must be 32 bytes"))
            })
            .transpose()?
            .unwrap_or_else(|| self.inner.public_key_bytes());
        let genesis = genesis_hash
            .try_into()
            .map_err(|_| value_err("genesis hash must be 32 bytes"))?;
        Ok(PyBytes::new(
            py,
            &self
                .inner
                .sign_mldsa_capability_for(owner, role, epoch, genesis, payload)
                .map_err(to_py_err)?,
        ))
    }

    #[pyo3(signature = (epoch, nonce, genesis_hash, order, owner=None))]
    fn sign_mldsa_order<'py>(
        &self,
        py: Python<'py>,
        epoch: u64,
        nonce: u64,
        genesis_hash: &[u8],
        order: &[u8],
        owner: Option<&[u8]>,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let owner = owner
            .map(|bytes| {
                bytes
                    .try_into()
                    .map_err(|_| value_err("owner must be 32 bytes"))
            })
            .transpose()?
            .unwrap_or_else(|| self.inner.public_key_bytes());
        let genesis = genesis_hash
            .try_into()
            .map_err(|_| value_err("genesis hash must be 32 bytes"))?;
        Ok(PyBytes::new(
            py,
            &self
                .inner
                .sign_mldsa_order_for(owner, epoch, nonce, genesis, order)
                .map_err(to_py_err)?,
        ))
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

/// Decode an SS58 address to its 32-byte account identity.
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

/// Encode the initial public descriptor without a network field.
#[pyfunction]
fn encode_hashed_receiving_address(descriptor: &[u8]) -> PyResult<String> {
    keys::encode_hashed_receiving_address(descriptor).map_err(to_py_err)
}

/// Return the original public descriptor after validating the address.
#[pyfunction]
fn decode_hashed_receiving_address<'py>(
    py: Python<'py>,
    address: &str,
) -> PyResult<Bound<'py, PyBytes>> {
    let descriptor = keys::decode_hashed_receiving_address(address).map_err(to_py_err)?;
    Ok(PyBytes::new(py, &descriptor))
}

#[pyfunction]
fn verify_mldsa_authorization(
    proof: &[u8],
    payload: &[u8],
    descriptor: &[u8],
    generation: u64,
    expected_commitment: &[u8],
) -> PyResult<bool> {
    let commitment = expected_commitment
        .try_into()
        .map_err(|_| value_err("ML-DSA commitment must be 64 bytes"))?;
    keys::verify_mldsa_authorization(proof, payload, descriptor, generation, commitment)
        .map_err(to_py_err)
}

#[pyfunction]
fn verify_mldsa_capability(
    proof: &[u8],
    payload: &[u8],
    owner: &[u8],
    genesis_hash: &[u8],
    role: u8,
    epoch: u64,
    expected_commitment: &[u8],
) -> PyResult<bool> {
    let owner = owner
        .try_into()
        .map_err(|_| value_err("owner must be 32 bytes"))?;
    let genesis = genesis_hash
        .try_into()
        .map_err(|_| value_err("genesis hash must be 32 bytes"))?;
    let commitment = expected_commitment
        .try_into()
        .map_err(|_| value_err("capability commitment must be 64 bytes"))?;
    keys::verify_mldsa_capability(proof, payload, owner, genesis, role, epoch, commitment)
        .map_err(to_py_err)
}

#[pyfunction]
fn encode_mldsa_receiving_address(descriptor: &[u8]) -> PyResult<String> {
    keys::encode_mldsa_receiving_address(descriptor).map_err(to_py_err)
}

#[pyfunction]
fn decode_mldsa_receiving_address<'py>(
    py: Python<'py>,
    address: &str,
) -> PyResult<Bound<'py, PyBytes>> {
    Ok(PyBytes::new(
        py,
        &keys::decode_mldsa_receiving_address(address).map_err(to_py_err)?,
    ))
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
    m.add_function(wrap_pyfunction!(encode_hashed_receiving_address, m)?)?;
    m.add_function(wrap_pyfunction!(decode_hashed_receiving_address, m)?)?;
    m.add_function(wrap_pyfunction!(encode_mldsa_receiving_address, m)?)?;
    m.add_function(wrap_pyfunction!(decode_mldsa_receiving_address, m)?)?;
    m.add_function(wrap_pyfunction!(verify_mldsa_capability, m)?)?;
    m.add_function(wrap_pyfunction!(verify_mldsa_authorization, m)?)?;
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
    m.add("CRYPTO_HASHED", keys::CRYPTO_HASHED)?;
    m.add("CRYPTO_HASHED_ED25519", keys::CRYPTO_HASHED_ED25519)?;
    m.add("CRYPTO_MLDSA65", keys::CRYPTO_MLDSA65)?;
    m.add("CRYPTO_MLDSA65_FIXED", keys::CRYPTO_MLDSA65_FIXED)?;
    m.add("ROLE_ORDER", keys::ROLE_ORDER)?;
    m.add("ROLE_MESSAGE", keys::ROLE_MESSAGE)?;
    Ok(())
}
