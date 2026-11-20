//! Bindings for `bittensor_core::timelock` and `mlkem` — the portable
//! subset. Drand fetching stays in the TS shell: extract the round with
//! `revealRound`, fetch the round's signature over HTTP, then finish with
//! `decryptWithSignature`.

use bittensor_core::timelock::epoch_schedule::EpochScheduleState;
use bittensor_core::{mlkem, timelock};
use js_sys::{Array, Uint8Array};
use wasm_bindgen::prelude::*;

use crate::errors::to_js_err;
use crate::values::u64_arg;

fn bytes_round_pair(bytes: &[u8], round: u64) -> Array {
    let out = Array::new();
    out.push(&Uint8Array::from(bytes).into());
    out.push(&JsValue::from_f64(round as f64));
    out
}

/// Timelock-encrypted weights commitment using the stateful epoch model
/// (v2): simulates the chain's block pipeline to find the reveal block, then
/// encrypts to the drand round covering it. Returns `[ciphertext, round]`.
#[allow(clippy::too_many_arguments)]
#[wasm_bindgen(js_name = getEncryptedCommitV2)]
pub fn get_encrypted_commit_v2(
    uids: Vec<u16>,
    weights: Vec<u16>,
    version_key: &JsValue,
    last_epoch_block: &JsValue,
    pending_epoch_at: &JsValue,
    subnet_epoch_index: &JsValue,
    tempo: u16,
    blocks_since_last_step: &JsValue,
    current_block: &JsValue,
    subnet_reveal_period_epochs: &JsValue,
    block_time: f64,
    hotkey: Vec<u8>,
) -> Result<Array, JsValue> {
    let state = EpochScheduleState {
        last_epoch_block: u64_arg(last_epoch_block, "lastEpochBlock")?,
        pending_epoch_at: u64_arg(pending_epoch_at, "pendingEpochAt")?,
        subnet_epoch_index: u64_arg(subnet_epoch_index, "subnetEpochIndex")?,
        tempo,
        blocks_since_last_step: u64_arg(blocks_since_last_step, "blocksSinceLastStep")?,
        current_block: u64_arg(current_block, "currentBlock")?,
    };
    let (ciphertext, round) = timelock::generate_commit_v2(
        uids,
        weights,
        u64_arg(version_key, "versionKey")?,
        state,
        u64_arg(subnet_reveal_period_epochs, "subnetRevealPeriodEpochs")?,
        block_time,
        hotkey,
    )
    .map_err(to_js_err)?;
    Ok(bytes_round_pair(&ciphertext, round))
}

/// Encrypt a string commitment to the round `blocksUntilReveal` blocks from
/// now. Returns `[ciphertext, round]`.
#[wasm_bindgen(js_name = getEncryptedCommitment)]
pub fn get_encrypted_commitment(
    data: &str,
    blocks_until_reveal: &JsValue,
    block_time: Option<f64>,
) -> Result<Array, JsValue> {
    let (ciphertext, round) = timelock::encrypt_commitment(
        data,
        u64_arg(blocks_until_reveal, "blocksUntilReveal")?,
        block_time.unwrap_or(12.0),
    )
    .map_err(to_js_err)?;
    Ok(bytes_round_pair(&ciphertext, round))
}

/// Encrypt binary data to the round `nBlocks` blocks from now, wrapped in
/// the `UserData` envelope carrying the reveal round. Returns
/// `[ciphertext, round]`.
#[wasm_bindgen(js_name = encrypt)]
pub fn encrypt(data: &[u8], n_blocks: &JsValue, block_time: Option<f64>) -> Result<Array, JsValue> {
    let (payload, round) = timelock::encrypt_n_blocks(
        data,
        u64_arg(n_blocks, "nBlocks")?,
        block_time.unwrap_or(12.0),
    )
    .map_err(to_js_err)?;
    Ok(bytes_round_pair(&payload, round))
}

/// Encrypt binary data to a specific drand reveal round, wrapped in the
/// `UserData` envelope. Returns `[ciphertext, round]`.
#[wasm_bindgen(js_name = encryptAtRound)]
pub fn encrypt_at_round(data: &[u8], reveal_round: &JsValue) -> Result<Array, JsValue> {
    let (payload, round) = timelock::encrypt_at_round(data, u64_arg(reveal_round, "revealRound")?)
        .map_err(to_js_err)?;
    Ok(bytes_round_pair(&payload, round))
}

/// The drand reveal round a `UserData` envelope was encrypted to. The shell
/// fetches this round's BLS signature and calls `decryptWithSignature`.
#[wasm_bindgen(js_name = revealRound)]
pub fn reveal_round(encrypted_data: &[u8]) -> Result<f64, JsValue> {
    timelock::reveal_round(encrypted_data)
        .map(|round| round as f64)
        .map_err(to_js_err)
}

/// Inner compressed TLE ciphertext from a `UserData` envelope, for
/// `Commitments.set_commitment` `TimelockEncrypted.encrypted`.
#[wasm_bindgen(js_name = innerCiphertext)]
pub fn inner_ciphertext(encrypted_data: &[u8]) -> Result<Vec<u8>, JsValue> {
    timelock::inner_ciphertext(encrypted_data).map_err(to_js_err)
}

/// Decrypt a `UserData` envelope with an already-fetched signature (hex).
#[wasm_bindgen(js_name = decryptWithSignature)]
pub fn decrypt_with_signature(
    encrypted_data: &[u8],
    signature_hex: &str,
) -> Result<Vec<u8>, JsValue> {
    timelock::decrypt_with_signature(encrypted_data, signature_hex).map_err(to_js_err)
}

/// Encrypt data using ML-KEM-768 + XChaCha20Poly1305. The public key is
/// rotated every block and can be queried from the NextKey storage item.
#[wasm_bindgen(js_name = encryptMlkem768)]
pub fn encrypt_mlkem768(
    pk_bytes: &[u8],
    plaintext: &[u8],
    include_key_hash: Option<bool>,
) -> Result<Vec<u8>, JsValue> {
    mlkem::seal(pk_bytes, plaintext, include_key_hash.unwrap_or(false)).map_err(to_js_err)
}

/// The KDF identifier used by ML-KEM encryption.
#[wasm_bindgen(js_name = mlkemKdfId)]
pub fn mlkem_kdf_id() -> Vec<u8> {
    mlkem::KDF_ID.to_vec()
}
