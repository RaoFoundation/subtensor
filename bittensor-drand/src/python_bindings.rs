use crate::drand;
use codec::{Decode, Encode};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyBytes;
use pyo3::{pyfunction, pymodule, wrap_pyfunction, Bound, PyResult, Python};
use std::time::{SystemTime, UNIX_EPOCH};

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
    let state = crate::epoch_schedule::EpochScheduleState {
        last_epoch_block,
        pending_epoch_at,
        subnet_epoch_index,
        tempo,
        blocks_since_last_step,
        current_block,
    };
    let result = drand::generate_commit_v2(
        uids,
        weights,
        version_key,
        state,
        subnet_reveal_period_epochs,
        block_time,
        hotkey,
    );
    match result {
        Ok((ciphertext, target_round)) => {
            let py_bytes = PyBytes::new(py, &ciphertext).into();
            Ok((py_bytes, target_round))
        }
        Err(e) => Err(PyValueError::new_err(format!("{:?}", e))),
    }
}

/// Encrypts a string commitment with a timelock for a future Drand round.
///
/// This function encrypts arbitrary string data, ensuring it will be decryptable
/// only after a number of blocks pass. Useful for general-purpose timelock encryption.
///
/// Args:
///     data (str): The string to encrypt.
///     blocks_until_reveal (int): Number of blocks to wait before the data is decryptable.
///     block_time (float, optional): Block time in seconds (default = 12.0).
///
/// Returns:
///     Tuple[bytes, int]: A tuple containing:
///         - the encrypted bytes
///         - the Drand reveal round (int)
#[pyfunction]
#[pyo3(signature = (data, blocks_until_reveal, block_time=12.0))]
fn get_encrypted_commitment(
    py: Python,
    data: &str,
    blocks_until_reveal: u64,
    block_time: f64,
) -> PyResult<(Py<PyBytes>, u64)> {
    let result = drand::encrypt_commitment(data, blocks_until_reveal, block_time);
    // matching the result
    match result {
        Ok((ciphertext, target_round)) => {
            let py_bytes = PyBytes::new(py, &ciphertext).into();
            Ok((py_bytes, target_round))
        }
        Err(e) => Err(PyValueError::new_err(format!("{:?}", e))),
    }
}

#[pyfunction(name = "get_latest_round")]
fn get_latest_round_py() -> PyResult<u64> {
    let response = drand::get_round_info(None)
        .map_err(|e| PyValueError::new_err(format!("Drand fetch error: {}", e)))?;

    Ok(response.round)
}

/// Encrypts binary data for a future Drand round based on block delay.
///
/// This method timelock-encrypts the provided binary `data`, such that it
/// becomes decryptable only after `n_blocks` have passed. Internally, it maps
/// this to a specific Drand round number.
///
/// Args:
///     data (bytes): Data to encrypt.
///     n_blocks (int): Number of blocks to wait before decryption is possible.
///     block_time (float, optional): Block time in seconds (default = 12.0).
///
/// Returns:
///     Tuple[bytes, int]: A tuple containing:
///         - the encrypted payload
///         - the Drand reveal round number
#[pyfunction]
#[pyo3(signature = (data, n_blocks, block_time=12.0))]
fn encrypt(
    py: Python,
    data: &[u8],
    n_blocks: u64,
    block_time: f64,
) -> PyResult<(Py<PyBytes>, u64)> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| PyValueError::new_err(format!("SystemTime error: {:?}", e)))?
        .as_secs_f64();

    let reveal_timestamp = (n_blocks as f64 * block_time + now).ceil() as u64 - crate::constants::GENESIS_TIME;
    let reveal_round = reveal_timestamp / crate::constants::DRAND_PERIOD;

    let encrypted_data = drand::encrypt_and_compress(data, reveal_round)
        .map_err(|e| PyValueError::new_err(format!("Encryption failed: {:?}", e)))?;

    let encrypted_with_reveal_round = drand::UserData {
        encrypted_data,
        reveal_round,
    }
    .encode();

    Ok((
        PyBytes::new(py, &encrypted_with_reveal_round).into(),
        reveal_round,
    ))
}

/// Encrypts binary data for a specific Drand reveal round.
///
/// This method timelock-encrypts the provided binary `data` to be decryptable
/// only when the specified Drand round is revealed. Unlike `encrypt()`, this
/// function directly uses the provided round number without calculating it
/// from block delays.
///
/// Args:
///     data (bytes): Data to encrypt.
///     reveal_round (int): The specific Drand round number when decryption becomes possible.
///
/// Returns:
///     Tuple[bytes, int]: A tuple containing:
///         - the encrypted payload
///         - the Drand reveal round number (same as input)
#[pyfunction]
fn encrypt_at_round(
    py: Python,
    data: &[u8],
    reveal_round: u64,
) -> PyResult<(Py<PyBytes>, u64)> {
    // Directly encrypt with the specified reveal round
    let encrypted_data = drand::encrypt_and_compress(data, reveal_round)
        .map_err(|e| PyValueError::new_err(format!("Encryption failed: {:?}", e)))?;

    let encrypted_with_reveal_round = drand::UserData {
        encrypted_data,
        reveal_round,
    }
    .encode();

    Ok((
        PyBytes::new(py, &encrypted_with_reveal_round).into(),
        reveal_round,
    ))
}

/// Attempts to decrypt data previously encrypted with Drand timelock encryption.
///
/// This function automatically extracts the reveal round from the encrypted message,
/// fetches the corresponding Drand signature (if available), and decrypts the message.
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
    let user_data = match drand::UserData::decode(&mut &encrypted_data[..]) {
        Ok(data) => data,
        Err(e) => {
            return if no_errors {
                Ok(None)
            } else {
                Err(PyValueError::new_err(format!(
                    "Error deserializing data: {:?}",
                    e
                )))
            }
        }
    };

    let signature_opt = drand::get_reveal_round_signature(Some(user_data.reveal_round), no_errors)
        .map_err(|e| PyValueError::new_err(e))?;

    let signature_str = match signature_opt {
        Some(s) => s,
        None => {
            return if no_errors {
                Ok(None)
            } else {
                Err(PyValueError::new_err("Signature not available"))
            }
        }
    };

    let signature_bytes = hex::decode(signature_str)
        .map_err(|e| PyValueError::new_err(format!("Invalid hex in signature: {:?}", e)))?;

    let decoded_data = drand::decrypt_and_decompress(&user_data.encrypted_data, &signature_bytes)
        .map_err(|e| PyValueError::new_err(e))?;

    Ok(Some(PyBytes::new(py, &decoded_data).into()))
}

/// Decrypts data using a provided Drand signature.
///
/// This function is useful when decrypting multiple ciphertexts for the same round,
/// allowing you to fetch the signature once and reuse it, avoiding redundant API calls.
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
    let user_data = drand::UserData::decode(&mut &encrypted_data[..])
        .map_err(|e| PyValueError::new_err(format!("Error deserializing data: {:?}", e)))?;

    let signature_bytes = hex::decode(signature_hex)
        .map_err(|e| PyValueError::new_err(format!("Invalid hex in signature: {:?}", e)))?;

    let decoded_data = drand::decrypt_and_decompress(&user_data.encrypted_data, &signature_bytes)
        .map_err(|e| PyValueError::new_err(e))?;

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
fn get_signature_for_round(reveal_round: u64) -> PyResult<String> {
    drand::get_reveal_round_signature(Some(reveal_round), false)
        .map_err(|e| PyValueError::new_err(e))?
        .ok_or_else(|| PyValueError::new_err("Signature not available"))
}

/// Encrypts data using ML-KEM-768 + XChaCha20Poly1305
///
/// This function encrypts plaintext using ML-KEM-768 key encapsulation followed by
/// XChaCha20Poly1305 authenticated encryption. The public key is rotated every block
/// and can be queried from the NextKey storage item.
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
///
/// Raises:
///     ValueError: If encryption fails
#[pyfunction]
#[pyo3(signature = (pk_bytes, plaintext, include_key_hash=false))]
fn encrypt_mlkem768(
    py: Python,
    pk_bytes: &[u8],
    plaintext: &[u8],
    include_key_hash: bool,
) -> PyResult<Py<PyBytes>> {
    let extra = if include_key_hash { 16 } else { 0 };
    let max_output_size = extra + 2048 + plaintext.len() + 64;
    let mut output = vec![0u8; max_output_size];
    let mut written = 0usize;

    let result = crate::ffi::mlkem768_seal_blob(
        pk_bytes.as_ptr(),
        pk_bytes.len(),
        plaintext.as_ptr(),
        plaintext.len(),
        output.as_mut_ptr(),
        output.len(),
        &mut written,
        include_key_hash,
    );

    match result {
        0 => {
            output.truncate(written);
            Ok(PyBytes::new(py, &output).into())
        }
        -1 => Err(PyValueError::new_err("Null pointer provided")),
        -2 => Err(PyValueError::new_err("Failed to decode public key")),
        -3 => Err(PyValueError::new_err("Encapsulation failed")),
        -4 => Err(PyValueError::new_err("KEM ciphertext too long")),
        -5 => Err(PyValueError::new_err("Invalid shared secret length")),
        -6 => Err(PyValueError::new_err("AEAD encryption failed")),
        -7 => Err(PyValueError::new_err("Output buffer too small")),
        code => Err(PyValueError::new_err(format!("Unknown error code: {}", code))),
    }
}

/// Returns the KDF identifier used by ML-KEM encryption
///
/// Returns "v1" indicating direct use of shared secret (no HKDF)
///
/// Returns:
///     bytes: KDF identifier (b"v1")
#[pyfunction]
fn mlkem_kdf_id(py: Python) -> PyResult<Py<PyBytes>> {
    let mut buf = vec![0u8; 10];
    let result = crate::ffi::mlkemffi_kdf_id(buf.as_mut_ptr(), buf.len());
    
    match result {
        n if n > 0 => {
            buf.truncate(n as usize);
            Ok(PyBytes::new(py, &buf).into())
        }
        _ => Err(PyValueError::new_err("Failed to get KDF ID")),
    }
}

#[pymodule]
fn bittensor_drand(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(get_encrypted_commit_v2, m)?)?;
    m.add_function(wrap_pyfunction!(get_encrypted_commitment, m)?)?;
    m.add_function(wrap_pyfunction!(encrypt, m)?)?;
    m.add_function(wrap_pyfunction!(encrypt_at_round, m)?)?;
    m.add_function(wrap_pyfunction!(decrypt, m)?)?;
    m.add_function(wrap_pyfunction!(decrypt_with_signature, m)?)?;
    m.add_function(wrap_pyfunction!(get_signature_for_round, m)?)?;
    m.add_function(wrap_pyfunction!(get_latest_round_py, m)?)?;
    // ML-KEM functions
    m.add_function(wrap_pyfunction!(encrypt_mlkem768, m)?)?;
    m.add_function(wrap_pyfunction!(mlkem_kdf_id, m)?)?;
    Ok(())
}
