use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use codec::{Decode, Encode};

use rand_core::{OsRng, RngCore};
use serde::Deserialize;
use sha2::Digest;
use std::os::raw::c_char;
use std::time::{SystemTime, UNIX_EPOCH};
use tle::{
    curves::drand::TinyBLS381,
    ibe::fullident::Identity,
    stream_ciphers::AESGCMStreamCipherProvider,
    tlock::{tld, tle, TLECiphertext},
};
use w3f_bls::EngineBLS;

use crate::constants::{
    DRAND_ENDPOINTS, DRAND_PERIOD, DRAND_PUBLIC_KEY, GENESIS_TIME, QUICKNET_CHAIN_HASH,
};

#[derive(Encode, Decode, Debug, PartialEq)]
pub struct WeightsTlockPayload {
    pub hotkey: Vec<u8>,
    pub uids: Vec<u16>,
    pub values: Vec<u16>,
    pub version_key: u64,
}

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

/// Encrypts and compresses the provided serialized data for secure transfer.
///
/// # Arguments
///
/// * `serialized_data` - A slice of bytes containing the serialized data to be encrypted.
/// * `reveal_round` - A `u64` value representing the round during which the encryption will be revealed.
///
/// # Returns
///
/// Returns a `Result` which is:
/// * `Ok(Vec<u8>)` containing the compressed and encrypted bytes if the process succeeds.
/// * `Err((std::io::Error, String))` if an error occurs during decoding, public key deserialization,
///   encryption, or ciphertext compression.
///
pub fn encrypt_and_compress(
    serialized_data: &[u8],
    reveal_round: u64,
) -> Result<Vec<u8>, (std::io::Error, String)> {
    // Deserialize public key
    let pub_key_bytes = hex::decode(DRAND_PUBLIC_KEY).map_err(|e| {
        (
            std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{:?}", e)),
            "Decoding public key failed.".to_string(),
        )
    })?;
    let pub_key =
        <TinyBLS381 as EngineBLS>::PublicKeyGroup::deserialize_compressed(&*pub_key_bytes)
            .map_err(|e| {
                (
                    std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{:?}", e)),
                    "Deserializing public key failed.".to_string(),
                )
            })?;

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
    .map_err(|e| {
        (
            std::io::Error::new(std::io::ErrorKind::Other, format!("{:?}", e)),
            "Encryption failed.".to_string(),
        )
    })?;

    // Compress ciphertext
    let mut ct_bytes: Vec<u8> = Vec::new();
    ct.serialize_compressed(&mut ct_bytes).map_err(|e| {
        (
            std::io::Error::new(std::io::ErrorKind::Other, format!("{:?}", e)),
            "Ciphertext serialization failed.".to_string(),
        )
    })?;

    Ok(ct_bytes)
}

/// Decrypts and decompresses timelock-encrypted data using the provided Drand signature.
///
/// This function reverses the encryption performed by `encrypt_and_compress`,
/// using the BLS signature from Drand as a time-based decryption key.
/// It deserializes the ciphertext, applies the signature to derive the symmetric key,
/// and decrypts the content using AES-GCM.
///
/// # Arguments
///
/// * `encrypted_data` - A byte slice containing the compressed and encrypted payload
///   produced by `encrypt_and_compress`.
/// * `signature_bytes` - A byte slice containing the BLS signature (Drand timelock reveal key),
///   usually obtained from the Drand network for the corresponding reveal round.
///
/// # Returns
///
/// Returns `Ok(Vec<u8>)` containing the decrypted plaintext bytes on success, or
/// `Err(String)` with an error message if deserialization or decryption fails.
///
/// # Errors
///
/// This function will return an error if:
/// - The ciphertext or signature cannot be deserialized
/// - The decryption operation fails (e.g. incorrect or premature signature)
///
pub fn decrypt_and_decompress(
    encrypted_data: &[u8],
    signature_bytes: &[u8],
) -> Result<Vec<u8>, String> {
    // Deserializing ciphertext
    let ciphertext = TLECiphertext::<TinyBLS381>::deserialize_compressed(encrypted_data)
        .map_err(|e| format!("Error deserializing ciphertext: {:?}", e))?;

    // Deserializing sign
    let sign = <TinyBLS381 as EngineBLS>::SignatureGroup::deserialize_compressed(signature_bytes)
        .map_err(|e| format!("Signature deserialization error: {:?}", e))?;

    // Decoding
    let decrypted_bytes = tld::<TinyBLS381, AESGCMStreamCipherProvider>(ciphertext, sign)
        .map_err(|e| format!("Error decrypting ciphertext: {:?}", e))?;

    Ok(decrypted_bytes)
}

/// Stateful epoch model commit (v2).
///
/// Instead of legacy modulo arithmetic, uses `epoch_schedule::predict_first_reveal_block`
/// to simulate the chain's `block_step` pipeline and find the exact reveal block.
pub fn generate_commit_v2(
    uids: Vec<u16>,
    values: Vec<u16>,
    version_key: u64,
    state: crate::epoch_schedule::EpochScheduleState,
    subnet_reveal_period_epochs: u64,
    block_time: f64,
    hotkey: Vec<u8>,
) -> Result<(Vec<u8>, u64), (std::io::Error, String)> {
    let first_reveal_blk =
        crate::epoch_schedule::predict_first_reveal_block(&state, subnet_reveal_period_epochs)
            .map_err(|e| {
                (
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string()),
                    e.to_string(),
                )
            })?;

    let target_ingest_blk =
        first_reveal_blk.saturating_add(crate::constants::SECURITY_BLOCK_OFFSET);
    let blocks_until_ingest = target_ingest_blk.saturating_sub(state.current_block);
    let secs_until_ingest = blocks_until_ingest as f64 * block_time;

    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();

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

/// Encrypts a string-based commitment using Drand timelock encryption for a future reveal round.
///
/// This function encodes the input `data` and calculates the corresponding Drand round number
/// based on the provided number of blocks to wait (`blocks_until_reveal`) and the time per block (`block_time`).
/// It then uses the Drand public key to encrypt the data with timelock encryption, ensuring
/// it can only be decrypted after the calculated reveal round.
///
/// # Arguments
///
/// * `data` – The string data to encrypt (e.g. a commitment or message).
/// * `blocks_until_reveal` – Number of blocks to wait before the data can be decrypted.
/// * `block_time` – Duration (in seconds) of a single block in the network (e.g. `12.0` for mainnet, `0.25` for testnet).
///
/// # Returns
///
/// * `Ok((Vec<u8>, u64))` – Tuple where:
///   - First element is the encrypted ciphertext (TLE format, compressed),
///   - Second element is the computed `reveal_round` (the Drand round when decryption becomes possible).
///
/// * `Err((std::io::Error, String))` – If encryption or public key deserialization fails, returns a tuple with the I/O error and a descriptive message.
///
pub fn encrypt_commitment(
    data: &str,
    blocks_until_reveal: u64,
    block_time: f64,
) -> Result<(Vec<u8>, u64), (std::io::Error, String)> {
    let serialized_data = data.encode();

    // revealed round calculation
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let reveal_round = ((now - GENESIS_TIME)
        + (blocks_until_reveal as f64 * block_time).round() as u64)
        / DRAND_PERIOD;

    // TLE encoding
    let ct_bytes = encrypt_and_compress(&serialized_data, reveal_round).map_err(|e| {
        (
            std::io::Error::new(std::io::ErrorKind::Other, format!("{:?}", e)),
            "Encryption failed.".to_string(),
        )
    })?;

    Ok((ct_bytes, reveal_round))
}

/// Fetches Drand round information (randomness and signature) from available public endpoints.
///
/// This function attempts to query multiple public Drand endpoints to retrieve
/// the `DrandResponse` for a specific round or for the latest round if no round is provided.
/// It uses a blocking Tokio runtime internally, so the function itself is synchronous,
/// while internally performing asynchronous HTTP operations.
///
/// # Arguments
///
/// * `round` - An `Option<u64>`:
///   - `Some(round)` – fetches data for a specific Drand round number.
///   - `None` – fetches data for the latest round.
///
/// # Returns
///
/// * `Ok(DrandResponse)` – Contains `round`, `randomness`, and `signature` values returned by the Drand network.
/// * `Err(String)` – Returns a human-readable error message if all endpoints fail to respond or return invalid data.
///
/// # Notes
///
/// * The function attempts all configured endpoints in order until a valid response is received.
/// * If all endpoints fail (connection or parsing errors), it returns the last encountered error.
///
pub fn get_round_info(round: Option<u64>) -> Result<DrandResponse, String> {
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| format!("Failed to create Tokio runtime: {}", e))?;

    rt.block_on(async {
        let mut last_error = None;

        for endpoint in DRAND_ENDPOINTS.iter() {
            let url = match round {
                Some(r) => format!("{}/{}/public/{}", endpoint, QUICKNET_CHAIN_HASH, r),
                None => format!("{}/{}/public/latest", endpoint, QUICKNET_CHAIN_HASH),
            };

            let response = match reqwest::get(&url).await {
                Ok(resp) => resp,
                Err(e) => {
                    last_error = Some(format!("Connection error to {}: {}", endpoint, e));
                    continue;
                }
            };

            match response.json::<DrandResponse>().await {
                Ok(parsed) => return Ok(parsed),
                Err(e) => {
                    last_error = Some(format!("Parsing error from {}: {}", endpoint, e));
                    continue;
                }
            }
        }

        Err(last_error.unwrap_or_else(|| "Failed to get data from all Drand endpoints".to_string()))
    })
}

/// Retrieves the Drand BLS signature for a specific reveal round.
///
/// This signature is used as a timelock decryption key for data encrypted via Drand-based timelock encryption.
/// The function queries the Drand network using the provided round number and returns the signature if available.
///
/// # Arguments
///
/// * `reveal_round` - Optional `u64` specifying the Drand round number.
///   - If `None`, the function fetches the latest available round.
///   - If `Some(round)`, the function attempts to fetch the specific round's signature.
///
/// * `no_errors` - If `true`, the function suppresses errors and returns `Ok(None)`
///   instead of returning an error when network or decoding failures occur.
///
/// # Returns
///
/// * `Ok(Some(String))` – The Drand BLS signature as a hex-encoded string, if successfully retrieved.
/// * `Ok(None)` – If `no_errors` is `true` and a fetch error occurred.
/// * `Err(String)` – An error message describing the failure, unless `no_errors` is enabled.
///
pub fn get_reveal_round_signature(
    reveal_round: Option<u64>,
    no_errors: bool,
) -> Result<Option<String>, String> {
    let response = match get_round_info(reveal_round) {
        Ok(r) => r,
        Err(e) => {
            return if no_errors {
                Ok(None)
            } else {
                Err(format!(
                    "Failed to get Drand round {:?}: {}",
                    reveal_round, e
                ))
            };
        }
    };

    Ok(Some(response.signature))
}

const PUBLIC_KEY_HEX: &[u8] = b"83cf0f2896adee7eb8b5f01fcad3912212c437e0073e911fb90022d3e760183c\
      8c4b450b6a0a6c3ac6a5776a2d1064510d1fec758c921cc22b0e17e63aaf4bcb\
      5ed66304de9cf809bd274ca73bab4af5a6e9c76a4bc09e76eae8991ef5ece45a\0";

#[no_mangle]
pub extern "C" fn drand_public_key_hex() -> *const c_char {
    PUBLIC_KEY_HEX.as_ptr() as *const c_char
}

const CHAIN_HASH: &[u8] = b"52db9ba70e0cc0f6eaf7803dd07447a1f5477735fd3f661792ba94600c84e971\0";

#[no_mangle]
pub extern "C" fn drand_chain_hash() -> *const c_char {
    CHAIN_HASH.as_ptr() as *const c_char
}

#[no_mangle]
pub extern "C" fn drand_endpoint(idx: usize) -> *const c_char {
    const ENDPOINTS: [&[u8]; 5] = [
        b"https://api.drand.sh\0",
        b"https://api2.drand.sh\0",
        b"https://api3.drand.sh\0",
        b"https://drand.cloudflare.com\0",
        b"https://api.drand.secureweb3.com:6875\0",
    ];
    if idx >= ENDPOINTS.len() {
        std::ptr::null()
    } else {
        ENDPOINTS[idx].as_ptr() as *const c_char
    }
}

#[cfg(test)]
mod tests {
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
        let state = crate::epoch_schedule::EpochScheduleState {
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
