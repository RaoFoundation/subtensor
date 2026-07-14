//! Drand quicknet timelock encryption (absorbed from `bittensor-drand`).
//!
//! Lives in the monorepo so the epoch-schedule simulation always matches the
//! chain: `tle` and `w3f-bls` inherit the workspace pins, so ciphertexts
//! never drift from what pallet-drand can decrypt.

// Client-side code: arithmetic on locally validated values is the norm here,
// and this crate never runs inside the runtime.
#![allow(clippy::arithmetic_side_effects)]

pub mod constants;
pub mod epoch_schedule;
#[cfg(test)]
mod epoch_schedule_vectors;

use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use codec::{Decode, Encode};
use rand_core::{OsRng, RngCore};
use serde::Deserialize;
use sha2::Digest;
// std::time::SystemTime::now() panics on wasm32-unknown-unknown; web-time is
// a Date.now()-backed drop-in with the same types on that target.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
use std::time::{SystemTime, UNIX_EPOCH};
use subtensor_macros::freeze_struct;
use tle::{
    curves::drand::TinyBLS381,
    ibe::fullident::Identity,
    stream_ciphers::AESGCMStreamCipherProvider,
    tlock::{tld, tle, TLECiphertext},
};
use w3f_bls::EngineBLS;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
use web_time::{SystemTime, UNIX_EPOCH};

use crate::error::CoreError;
#[cfg(feature = "host")]
use constants::{DRAND_ENDPOINTS, QUICKNET_CHAIN_HASH};
use constants::{DRAND_PERIOD, DRAND_PUBLIC_KEY, GENESIS_TIME, SECURITY_BLOCK_OFFSET};

fn tl_err(msg: impl Into<String>) -> CoreError {
    CoreError::Crypto(msg.into())
}

fn now_unix() -> Result<std::time::Duration, CoreError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| tl_err(format!("SystemTime error: {e:?}")))
}

#[freeze_struct("705b0f6dde3ed6e")]
#[derive(Encode, Decode, Debug, PartialEq)]
pub struct WeightsTlockPayload {
    pub hotkey: Vec<u8>,
    pub uids: Vec<u16>,
    pub values: Vec<u16>,
    pub version_key: u64,
}

#[freeze_struct("b96b617eb03c11a6")]
#[derive(Encode, Decode)]
pub struct UserData {
    pub encrypted_data: Vec<u8>,
    pub reveal_round: u64,
}

#[derive(Deserialize)]
pub struct DrandResponse {
    pub round: u64,
    pub signature: String,
}

/// The drand reveal round a `UserData` envelope was encrypted to. Portable
/// counterpart to [`decrypt`]: a host without network access in the core
/// (e.g. a browser shell) extracts the round here, fetches the signature
/// itself, and finishes with [`decrypt_with_signature`].
pub fn reveal_round(encrypted_data: &[u8]) -> Result<u64, CoreError> {
    let user_data = UserData::decode(&mut &encrypted_data[..])
        .map_err(|e| tl_err(format!("Error deserializing data: {e:?}")))?;
    Ok(user_data.reveal_round)
}

/// Timelock-encrypt `serialized_data` to the drand quicknet round
/// `reveal_round`, returning the compressed TLE ciphertext.
pub fn encrypt_and_compress(
    serialized_data: &[u8],
    reveal_round: u64,
) -> Result<Vec<u8>, CoreError> {
    let pub_key_bytes = hex::decode(DRAND_PUBLIC_KEY)
        .map_err(|e| tl_err(format!("Decoding public key failed: {e:?}")))?;
    let pub_key =
        <TinyBLS381 as EngineBLS>::PublicKeyGroup::deserialize_compressed(&*pub_key_bytes)
            .map_err(|e| tl_err(format!("Deserializing public key failed: {e:?}")))?;

    // Create identity from reveal_round
    let message = {
        let mut hasher = sha2::Sha256::new();
        hasher.update(reveal_round.to_be_bytes());
        hasher.finalize().to_vec()
    };
    let identity = Identity::new(b"", vec![message]);

    // Encrypt payload
    let mut esk = [0u8; 32];
    OsRng.fill_bytes(&mut esk);
    let ct = tle::<TinyBLS381, AESGCMStreamCipherProvider, OsRng>(
        pub_key,
        esk,
        serialized_data,
        identity,
        OsRng,
    )
    .map_err(|e| tl_err(format!("Encryption failed: {e:?}")))?;

    // Compress ciphertext
    let mut ct_bytes: Vec<u8> = Vec::new();
    ct.serialize_compressed(&mut ct_bytes)
        .map_err(|e| tl_err(format!("Ciphertext serialization failed: {e:?}")))?;

    Ok(ct_bytes)
}

/// Decrypt a compressed TLE ciphertext with the round's BLS signature.
pub fn decrypt_and_decompress(
    encrypted_data: &[u8],
    signature_bytes: &[u8],
) -> Result<Vec<u8>, CoreError> {
    let ciphertext = TLECiphertext::<TinyBLS381>::deserialize_compressed(encrypted_data)
        .map_err(|e| tl_err(format!("Error deserializing ciphertext: {e:?}")))?;

    let sign = <TinyBLS381 as EngineBLS>::SignatureGroup::deserialize_compressed(signature_bytes)
        .map_err(|e| tl_err(format!("Signature deserialization error: {e:?}")))?;

    tld::<TinyBLS381, AESGCMStreamCipherProvider>(ciphertext, sign)
        .map_err(|e| tl_err(format!("Error decrypting ciphertext: {e:?}")))
}

/// Stateful epoch model commit (v2): simulate the chain's `block_step`
/// pipeline to find the exact reveal block, then encrypt the weights payload
/// to the drand round covering it. Returns `(ciphertext, reveal_round)`.
pub fn generate_commit_v2(
    uids: Vec<u16>,
    values: Vec<u16>,
    version_key: u64,
    state: epoch_schedule::EpochScheduleState,
    subnet_reveal_period_epochs: u64,
    block_time: f64,
    hotkey: Vec<u8>,
) -> Result<(Vec<u8>, u64), CoreError> {
    let first_reveal_blk =
        epoch_schedule::predict_first_reveal_block(&state, subnet_reveal_period_epochs)
            .map_err(|e| tl_err(e.to_string()))?;

    let target_ingest_blk = first_reveal_blk.saturating_add(SECURITY_BLOCK_OFFSET);
    let blocks_until_ingest = target_ingest_blk.saturating_sub(state.current_block);
    let secs_until_ingest = blocks_until_ingest as f64 * block_time;

    let now_secs = now_unix()?.as_secs_f64();

    let target_secs = now_secs + secs_until_ingest;
    let mut reveal_round =
        ((target_secs - GENESIS_TIME as f64) / DRAND_PERIOD as f64).floor() as u64;
    if reveal_round < 1 {
        reveal_round = 1;
    }

    let payload = WeightsTlockPayload {
        hotkey,
        uids,
        values,
        version_key,
    };

    let ct_bytes = encrypt_and_compress(&payload.encode(), reveal_round)?;
    Ok((ct_bytes, reveal_round))
}

/// Encrypt a string commitment to the round `blocks_until_reveal` blocks from
/// now. Returns `(ciphertext, reveal_round)`.
pub fn encrypt_commitment(
    data: &str,
    blocks_until_reveal: u64,
    block_time: f64,
) -> Result<(Vec<u8>, u64), CoreError> {
    let serialized_data = data.encode();

    let now = now_unix()?.as_secs();
    let reveal_round = ((now - GENESIS_TIME)
        + (blocks_until_reveal as f64 * block_time).round() as u64)
        / DRAND_PERIOD;

    let ct_bytes = encrypt_and_compress(&serialized_data, reveal_round)?;
    Ok((ct_bytes, reveal_round))
}

/// Encrypt binary data to the round `n_blocks` blocks from now, wrapped in
/// the `UserData` envelope that carries the reveal round.
pub fn encrypt_n_blocks(
    data: &[u8],
    n_blocks: u64,
    block_time: f64,
) -> Result<(Vec<u8>, u64), CoreError> {
    let now = now_unix()?.as_secs_f64();
    let reveal_timestamp = (n_blocks as f64 * block_time + now).ceil() as u64 - GENESIS_TIME;
    let reveal_round = reveal_timestamp / DRAND_PERIOD;
    encrypt_at_round(data, reveal_round)
}

/// Encrypt binary data to a specific drand reveal round, wrapped in the
/// `UserData` envelope that carries the reveal round.
pub fn encrypt_at_round(data: &[u8], reveal_round: u64) -> Result<(Vec<u8>, u64), CoreError> {
    let encrypted_data = encrypt_and_compress(data, reveal_round)?;
    let encrypted_with_reveal_round = UserData {
        encrypted_data,
        reveal_round,
    }
    .encode();
    Ok((encrypted_with_reveal_round, reveal_round))
}

/// Fetch drand round info (blocking; tries each public endpoint in order).
#[cfg(feature = "host")]
pub fn get_round_info(round: Option<u64>) -> Result<DrandResponse, CoreError> {
    let mut last_error = None;

    for endpoint in DRAND_ENDPOINTS.iter() {
        let url = match round {
            Some(r) => format!("{}/{}/public/{}", endpoint, QUICKNET_CHAIN_HASH, r),
            None => format!("{}/{}/public/latest", endpoint, QUICKNET_CHAIN_HASH),
        };

        let response = match reqwest::blocking::get(&url) {
            Ok(resp) => resp,
            Err(e) => {
                last_error = Some(format!("Connection error to {}: {}", endpoint, e));
                continue;
            }
        };

        match response.json::<DrandResponse>() {
            Ok(parsed) => return Ok(parsed),
            Err(e) => {
                last_error = Some(format!("Parsing error from {}: {}", endpoint, e));
                continue;
            }
        }
    }

    Err(tl_err(last_error.unwrap_or_else(|| {
        "Failed to get data from all Drand endpoints".to_string()
    })))
}

/// The BLS signature (hex) for a reveal round, or `Ok(None)` on fetch errors
/// when `no_errors` is set.
#[cfg(feature = "host")]
pub fn get_reveal_round_signature(
    reveal_round: Option<u64>,
    no_errors: bool,
) -> Result<Option<String>, CoreError> {
    match get_round_info(reveal_round) {
        Ok(r) => Ok(Some(r.signature)),
        Err(e) if no_errors => {
            let _ = e;
            Ok(None)
        }
        Err(e) => Err(tl_err(format!(
            "Failed to get Drand round {reveal_round:?}: {e}"
        ))),
    }
}

/// Decrypt a `UserData` envelope end-to-end: extract the reveal round, fetch
/// the round's BLS signature from drand, and decrypt. With `no_errors`, a
/// malformed envelope or unavailable round yields `Ok(None)`; decryption
/// failures against a fetched signature always error (the data is present
/// but wrong, which the caller should see).
#[cfg(feature = "host")]
pub fn decrypt(encrypted_data: &[u8], no_errors: bool) -> Result<Option<Vec<u8>>, CoreError> {
    let user_data = match UserData::decode(&mut &encrypted_data[..]) {
        Ok(data) => data,
        Err(_) if no_errors => return Ok(None),
        Err(e) => return Err(tl_err(format!("Error deserializing data: {e:?}"))),
    };

    let Some(signature_hex) = get_reveal_round_signature(Some(user_data.reveal_round), no_errors)?
    else {
        return if no_errors {
            Ok(None)
        } else {
            Err(tl_err("Signature not available"))
        };
    };

    decrypt_with_signature_hex(&user_data.encrypted_data, &signature_hex).map(Some)
}

/// Decrypt a `UserData` envelope with an already-fetched signature. Useful
/// when decrypting multiple ciphertexts for the same round.
pub fn decrypt_with_signature(
    encrypted_data: &[u8],
    signature_hex: &str,
) -> Result<Vec<u8>, CoreError> {
    let user_data = UserData::decode(&mut &encrypted_data[..])
        .map_err(|e| tl_err(format!("Error deserializing data: {e:?}")))?;
    decrypt_with_signature_hex(&user_data.encrypted_data, signature_hex)
}

fn decrypt_with_signature_hex(encrypted: &[u8], signature_hex: &str) -> Result<Vec<u8>, CoreError> {
    let signature_bytes = hex::decode(signature_hex)
        .map_err(|e| tl_err(format!("Invalid hex in signature: {e:?}")))?;
    decrypt_and_decompress(encrypted, &signature_bytes)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn test_encrypt_and_decrypt_static_key() {
        let message = b"hello, bittensor!";
        let reveal_round = 17200000;

        let encrypted =
            encrypt_and_compress(message, reveal_round).expect("Encryption should succeed");

        let signature_hex = get_reveal_round_signature(Some(reveal_round), false)
            .expect("Should get signature")
            .expect("Signature should not be None");

        let signature_bytes = hex::decode(&signature_hex).expect("Hex decoding failed");

        let decrypted = decrypt_and_decompress(&encrypted, &signature_bytes)
            .expect("Decryption should succeed");

        assert_eq!(message.to_vec(), decrypted);
    }

    #[test]
    fn test_get_round_info_and_signature() {
        let round = 17200000;
        let info = get_round_info(Some(round)).expect("Drand round should be available");

        assert_eq!(info.round, round);
        assert!(!info.signature.is_empty());

        let sig = get_reveal_round_signature(Some(round), false).unwrap();
        assert!(sig.is_some());
    }

    #[test]
    fn test_encrypt_commitment_format() {
        let data = "example string";
        let (encrypted, round) = encrypt_commitment(data, 10, 12.0).expect("Encryption failed");
        assert!(!encrypted.is_empty());
        assert!(round > 0);
    }

    #[test]
    fn test_generate_commit_v2_structure() {
        let state = epoch_schedule::EpochScheduleState {
            last_epoch_block: 100,
            pending_epoch_at: 0,
            subnet_epoch_index: 0,
            tempo: 50,
            blocks_since_last_step: 0,
            current_block: 120,
        };
        let uids = vec![1, 2, 3];
        let values = vec![100, 200, 300];
        let hotkey = vec![1, 2, 3];

        let (encrypted, reveal_round) = generate_commit_v2(
            uids.clone(),
            values.clone(),
            42,
            state,
            1,
            12.0,
            hotkey.clone(),
        )
        .expect("Commit generation failed");

        assert!(!encrypted.is_empty());
        assert!(reveal_round > 0);
    }
}
