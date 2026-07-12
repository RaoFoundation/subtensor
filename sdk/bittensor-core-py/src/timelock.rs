//! Bindings for `bittensor_core::timelock` and `mlkem` — the
//! `bittensor_drand` surface, preserved name-for-name (minus the retired
//! `get_encrypted_commit` v1).

use bittensor_core::timelock::epoch_schedule::EpochScheduleState;
use bittensor_core::{mlkem, timelock};
use pyo3::prelude::*;
use pyo3::types::PyBytes;

use crate::errors::to_py_err;

/// Returns a timelock-encrypted commitment using the stateful epoch model (v2).
///
/// Builds an internal ``EpochScheduleState`` from the provided scalar kwargs and
/// simulates the chain's block pipeline to find the reveal block.
///
/// Args:
///     uids (List[int]): List of UID integers.
///     weights (List[int]): Corresponding list of weight values (same length as ``uids``).
///     version_key (int): A version identifier for this commitment.
///     last_epoch_block (int): Block at which the last epoch ran.
///     pending_epoch_at (int): Pending owner-triggered epoch block (0 if none).
///     subnet_epoch_index (int): Monotonic epoch counter.
///     tempo (int): Epoch duration in blocks.
///     blocks_since_last_step (int): Blocks since last step for the subnet.
///     current_block (int): Chain head block number.
///     subnet_reveal_period_epochs (int): Number of epochs before reveal.
///     block_time (float): Block time in seconds.
///     hotkey (bytes): Committer hotkey public key bytes.
///
/// Returns:
///     Tuple[bytes, int]: encrypted commitment and reveal round.
// The signature mirrors the chain's EpochScheduleState field-for-field; a
// struct would only move the argument list into the Python callers.
#[allow(clippy::too_many_arguments)]
#[pyfunction]
#[pyo3(signature = (uids, weights, version_key, last_epoch_block, pending_epoch_at, subnet_epoch_index, tempo, blocks_since_last_step, current_block, subnet_reveal_period_epochs, block_time, hotkey))]
fn get_encrypted_commit_v2(
    py: Python,
    uids: Vec<u16>,
    weights: Vec<u16>,
    version_key: u64,
    last_epoch_block: u64,
    pending_epoch_at: u64,
    subnet_epoch_index: u64,
    tempo: u16,
    blocks_since_last_step: u64,
    current_block: u64,
    subnet_reveal_period_epochs: u64,
    block_time: f64,
    hotkey: Vec<u8>,
) -> PyResult<(Py<PyBytes>, u64)> {
    let state = EpochScheduleState {
        last_epoch_block,
        pending_epoch_at,
        subnet_epoch_index,
        tempo,
        blocks_since_last_step,
        current_block,
    };
    let (ciphertext, target_round) = timelock::generate_commit_v2(
        uids,
        weights,
        version_key,
        state,
        subnet_reveal_period_epochs,
        block_time,
        hotkey,
    )
    .map_err(to_py_err)?;
    Ok((PyBytes::new(py, &ciphertext).into(), target_round))
}

/// Encrypts a string commitment with a timelock for a future Drand round.
///
/// Args:
///     data (str): The string to encrypt.
///     blocks_until_reveal (int): Number of blocks to wait before the data is decryptable.
///     block_time (float, optional): Block time in seconds (default = 12.0).
///
/// Returns:
///     Tuple[bytes, int]: the encrypted bytes and the Drand reveal round.
#[pyfunction]
#[pyo3(signature = (data, blocks_until_reveal, block_time=12.0))]
fn get_encrypted_commitment(
    py: Python,
    data: &str,
    blocks_until_reveal: u64,
    block_time: f64,
) -> PyResult<(Py<PyBytes>, u64)> {
    let (ciphertext, target_round) =
        timelock::encrypt_commitment(data, blocks_until_reveal, block_time).map_err(to_py_err)?;
    Ok((PyBytes::new(py, &ciphertext).into(), target_round))
}

/// Gets the latest revealed Drand round number.
#[pyfunction(name = "get_latest_round")]
fn get_latest_round_py(py: Python) -> PyResult<u64> {
    let response = py
        .allow_threads(|| timelock::get_round_info(None))
        .map_err(to_py_err)?;
    Ok(response.round)
}

/// Encrypts binary data for a future Drand round based on block delay.
///
/// Args:
///     data (bytes): Data to encrypt.
///     n_blocks (int): Number of blocks to wait before decryption is possible.
///     block_time (float, optional): Block time in seconds (default = 12.0).
///
/// Returns:
///     Tuple[bytes, int]: the encrypted payload and the Drand reveal round number.
#[pyfunction]
#[pyo3(signature = (data, n_blocks, block_time=12.0))]
fn encrypt(
    py: Python,
    data: &[u8],
    n_blocks: u64,
    block_time: f64,
) -> PyResult<(Py<PyBytes>, u64)> {
    let (payload, reveal_round) =
        timelock::encrypt_n_blocks(data, n_blocks, block_time).map_err(to_py_err)?;
    Ok((PyBytes::new(py, &payload).into(), reveal_round))
}

/// Encrypts binary data for a specific Drand reveal round.
///
/// Args:
///     data (bytes): Data to encrypt.
///     reveal_round (int): The specific Drand round number when decryption becomes possible.
///
/// Returns:
///     Tuple[bytes, int]: the encrypted payload and the reveal round (same as input).
#[pyfunction]
fn encrypt_at_round(py: Python, data: &[u8], reveal_round: u64) -> PyResult<(Py<PyBytes>, u64)> {
    let (payload, reveal_round) =
        timelock::encrypt_at_round(data, reveal_round).map_err(to_py_err)?;
    Ok((PyBytes::new(py, &payload).into(), reveal_round))
}

/// Attempts to decrypt data previously encrypted with Drand timelock encryption.
///
/// Automatically extracts the reveal round from the encrypted message, fetches
/// the corresponding Drand signature (if available), and decrypts the message.
///
/// Args:
///     encrypted_data (bytes): Data previously returned from `encrypt` or `get_encrypted_commit_v2`.
///     no_errors (bool, optional): If True, suppresses errors and returns None instead (default = True).
///
/// Returns:
///     Optional[bytes]: Decrypted data if successful, otherwise None or raises an error.
#[pyfunction]
#[pyo3(signature = (encrypted_data, no_errors=true))]
fn decrypt(py: Python, encrypted_data: &[u8], no_errors: bool) -> PyResult<Option<Py<PyBytes>>> {
    let decoded = py
        .allow_threads(|| timelock::decrypt(encrypted_data, no_errors))
        .map_err(to_py_err)?;
    Ok(decoded.map(|data| PyBytes::new(py, &data).into()))
}

/// Decrypts data using a provided Drand signature.
///
/// Useful when decrypting multiple ciphertexts for the same round: fetch the
/// signature once and reuse it, avoiding redundant API calls.
///
/// Args:
///     encrypted_data (bytes): Data previously returned from encrypt functions.
///     signature_hex (str): Hex-encoded Drand BLS signature for the reveal round.
///
/// Returns:
///     bytes: Decrypted data.
#[pyfunction]
fn decrypt_with_signature(
    py: Python,
    encrypted_data: &[u8],
    signature_hex: &str,
) -> PyResult<Py<PyBytes>> {
    let decoded_data =
        timelock::decrypt_with_signature(encrypted_data, signature_hex).map_err(to_py_err)?;
    Ok(PyBytes::new(py, &decoded_data).into())
}

/// Fetches the Drand signature for a specific round.
///
/// Args:
///     reveal_round (int): The Drand round number to fetch.
///
/// Returns:
///     str: Hex-encoded BLS signature for the round.
#[pyfunction]
fn get_signature_for_round(py: Python, reveal_round: u64) -> PyResult<String> {
    py.allow_threads(|| timelock::get_reveal_round_signature(Some(reveal_round), false))
        .map_err(to_py_err)?
        .ok_or_else(|| pyo3::exceptions::PyValueError::new_err("Signature not available"))
}

/// Encrypts data using ML-KEM-768 + XChaCha20Poly1305.
///
/// The public key is rotated every block and can be queried from the NextKey
/// storage item.
///
/// Blob format (include_key_hash=false): [u16 kem_len LE][kem_ct][nonce24][aead_ct]
/// Blob format (include_key_hash=true):  [key_hash(16)][u16 kem_len LE][kem_ct][nonce24][aead_ct]
///
/// Args:
///     pk_bytes (bytes): ML-KEM-768 public key bytes (from NextKey storage)
///     plaintext (bytes): Data to encrypt
///     include_key_hash (bool): If true, prepend twox_128(pk_bytes) to the output
///
/// Returns:
///     bytes: Encrypted blob
#[pyfunction]
#[pyo3(signature = (pk_bytes, plaintext, include_key_hash=false))]
fn encrypt_mlkem768(
    py: Python,
    pk_bytes: &[u8],
    plaintext: &[u8],
    include_key_hash: bool,
) -> PyResult<Py<PyBytes>> {
    let blob = mlkem::seal(pk_bytes, plaintext, include_key_hash).map_err(to_py_err)?;
    Ok(PyBytes::new(py, &blob).into())
}

/// Returns the KDF identifier used by ML-KEM encryption (b"v1": raw shared
/// secret as the AEAD key, no HKDF, empty aad).
#[pyfunction]
fn mlkem_kdf_id(py: Python) -> PyResult<Py<PyBytes>> {
    Ok(PyBytes::new(py, mlkem::KDF_ID).into())
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(get_encrypted_commit_v2, m)?)?;
    m.add_function(wrap_pyfunction!(get_encrypted_commitment, m)?)?;
    m.add_function(wrap_pyfunction!(encrypt, m)?)?;
    m.add_function(wrap_pyfunction!(encrypt_at_round, m)?)?;
    m.add_function(wrap_pyfunction!(decrypt, m)?)?;
    m.add_function(wrap_pyfunction!(decrypt_with_signature, m)?)?;
    m.add_function(wrap_pyfunction!(get_signature_for_round, m)?)?;
    m.add_function(wrap_pyfunction!(get_latest_round_py, m)?)?;
    m.add_function(wrap_pyfunction!(encrypt_mlkem768, m)?)?;
    m.add_function(wrap_pyfunction!(mlkem_kdf_id, m)?)?;
    Ok(())
}
