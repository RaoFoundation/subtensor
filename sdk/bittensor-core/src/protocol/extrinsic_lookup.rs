// Derived from RaoFoundation/subtensor sdk/python/bittensor/_transport/interface.py @ 850f2755bb05a3851f948489f0ab27b0645cebce (Apache-2.0); modified by rtao.
//!
//! Locate a signed extrinsic by its byte hash without fabricating inclusion.

use super::RpcRequest;
use crate::CoreError;
use serde_json::{json, Value};
use sp_core::hashing::blake2_256;

/// Fetch the complete encoded extrinsic list at one immutable block.
pub fn request(block_hash: &str) -> RpcRequest {
    RpcRequest {
        method: "chain_getBlock",
        params: json!([block_hash]),
    }
}

/// Python's exact absent-inclusion diagnostic, retaining the caller's hashes.
pub fn missing_message(extrinsic_hash: &str, block_hash: &str, block_number: u64) -> String {
    format!(
        "extrinsic {extrinsic_hash} not found in block {block_hash} (#{block_number}); the serving node may not have imported it yet"
    )
}

/// Decode the included block's hexadecimal height.
pub fn block_number(block: &Value) -> Result<u64, CoreError> {
    let text = block
        .get("block")
        .and_then(|block| block.get("header"))
        .and_then(|header| header.get("number"))
        .and_then(Value::as_str)
        .ok_or_else(|| CoreError::Rpc("chain_getBlock omitted header.number".into()))?;
    u64::from_str_radix(text.strip_prefix("0x").unwrap_or(text), 16)
        .map_err(|error| CoreError::Codec(format!("invalid block number: {error}")))
}

/// Compare BLAKE2b-256 of complete SCALE extrinsics with the wanted hash.
///
/// A missing or null list is empty, matching Python's `extrinsics or []`.
/// `None` means absent; callers must return ExtrinsicNotFound before filtering
/// events. Malformed entries remain errors rather than guessed matches.
pub fn find(block: &Value, wanted_hash: [u8; 32]) -> Result<Option<u32>, CoreError> {
    let block = block
        .get("block")
        .filter(|block| block.is_object())
        .ok_or_else(|| CoreError::Rpc("chain_getBlock omitted block object".into()))?;
    let raw = block.get("extrinsics").unwrap_or(&Value::Null);
    if raw.is_null() {
        return Ok(None);
    }
    let values = raw
        .as_array()
        .ok_or_else(|| CoreError::Rpc("chain_getBlock extrinsics is not an array".into()))?;
    for (index, value) in values.iter().enumerate() {
        let raw = value
            .as_str()
            .ok_or_else(|| CoreError::Codec("block extrinsic is not hexadecimal text".into()))?;
        let bytes = hex::decode(raw.strip_prefix("0x").unwrap_or(raw))
            .map_err(|error| CoreError::Codec(format!("invalid block extrinsic: {error}")))?;
        let hash = blake2_256(&bytes);
        if hash == wanted_hash {
            return u32::try_from(index)
                .map(Some)
                .map_err(|_| CoreError::Codec("extrinsic index exceeds u32".into()));
        }
    }
    Ok(None)
}
