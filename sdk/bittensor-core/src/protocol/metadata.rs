// Derived from RaoFoundation/subtensor sdk/bittensor-core/src/client.rs @ 850f2755bb05a3851f948489f0ab27b0645cebce (Apache-2.0); modified by rtao.
//!
//! Metadata requests and strict decoding of the V15 opaque-metadata wrapper.
//!
//! Extracted from `fetch_metadata` in `sdk/bittensor-core/src/client.rs`
//! at subtensor v478 (`850f2755bb05a3851f948489f0ab27b0645cebce`),
//! Apache-2.0. RPC execution and the missing-method fallback policy belong
//! to the caller; malformed responses remain errors.

use crate::CoreError;
use codec::Decode;
use serde_json::{json, Value as JsonValue};

use super::RpcRequest;

/// Request V15 metadata, optionally at a pinned block hash.
pub fn v15_request(block_hash: Option<&str>) -> RpcRequest {
    let mut params = vec![
        JsonValue::String("Metadata_metadata_at_version".into()),
        JsonValue::String(format!("0x{}", hex::encode(15u32.to_le_bytes()))),
    ];
    if let Some(hash) = block_hash {
        params.push(JsonValue::String(hash.into()));
    }
    RpcRequest {
        method: "state_call",
        params: JsonValue::Array(params),
    }
}

/// Request legacy metadata after the caller has established a fallback reason.
pub fn legacy_request(block_hash: Option<&str>) -> RpcRequest {
    RpcRequest {
        method: "state_getMetadata",
        params: match block_hash {
            Some(hash) => json!([hash]),
            None => json!([]),
        },
    }
}

/// Decode the entire SCALE `Option<OpaqueMetadata>` response.
///
/// `Ok(None)` means the requested version is unavailable. Wrong JSON types,
/// invalid hex, malformed SCALE and trailing bytes are errors. The returned
/// inner bytes still need to be parsed as runtime metadata by the caller.
pub fn decode_v15(result: &JsonValue) -> Result<Option<Vec<u8>>, CoreError> {
    let encoded = response_bytes(result, "Metadata_metadata_at_version result")?;
    let mut input = encoded.as_slice();
    let metadata = Option::<Vec<u8>>::decode(&mut input).map_err(|error| {
        CoreError::Codec(format!(
            "cannot decode Metadata_metadata_at_version result: {error}"
        ))
    })?;
    if !input.is_empty() {
        return Err(CoreError::Codec(format!(
            "{} trailing bytes in Metadata_metadata_at_version result",
            input.len()
        )));
    }
    Ok(metadata)
}

/// Decode an unwrapped `state_getMetadata` result without executing a request.
pub fn decode_legacy(result: &JsonValue) -> Result<Vec<u8>, CoreError> {
    response_bytes(result, "state_getMetadata result")
}

pub(super) fn response_bytes(result: &JsonValue, context: &str) -> Result<Vec<u8>, CoreError> {
    let text = result
        .as_str()
        .ok_or_else(|| CoreError::Rpc(format!("{context} is not a string")))?;
    decode_hex(text)
}

pub(super) fn decode_hex(text: &str) -> Result<Vec<u8>, CoreError> {
    hex::decode(text.trim_start_matches("0x"))
        .map_err(|error| CoreError::Codec(format!("invalid hex {text}: {error}")))
}
