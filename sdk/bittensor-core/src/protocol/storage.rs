// Derived from RaoFoundation/subtensor sdk/bittensor-core/src/client.rs @ 850f2755bb05a3851f948489f0ab27b0645cebce (Apache-2.0); modified by rtao.
//!
//! Storage lookup, request construction and metadata-defined miss decoding.
//!
//! Extracted from `query`, `storage_raw`, `storage_info` and `decode_storage`
//! in `sdk/bittensor-core/src/client.rs` at subtensor v478
//! (`850f2755bb05a3851f948489f0ab27b0645cebce`), Apache-2.0.

use crate::{
    codec::value::Value,
    runtime::{type_string::TypeSpec, Runtime, StorageInfo},
    CoreError,
};
use serde_json::Value as JsonValue;

use super::{metadata::decode_hex, RpcRequest};

/// Borrow an entry from the live runtime, or report its missing pallet/item.
pub fn storage_info<'a>(
    runtime: &'a Runtime,
    pallet: &str,
    storage: &str,
) -> Result<&'a StorageInfo, CoreError> {
    runtime
        .storage_entry(pallet, storage)
        .ok_or_else(|| CoreError::NotInRuntime(format!("storage {pallet}.{storage}")))
}

/// Build a storage request using the live entry's key types and hashers.
///
/// The core permits a partial key prefix for map iteration. Callers fetching
/// one map value must supply its complete key arguments.
pub fn storage_request(
    runtime: &Runtime,
    info: &StorageInfo,
    params: &[Value],
    block_hash: Option<&str>,
) -> Result<RpcRequest, CoreError> {
    let key = runtime.storage_key(info, params)?;
    let mut params = vec![JsonValue::String(format!("0x{}", hex::encode(key)))];
    if let Some(hash) = block_hash {
        params.push(JsonValue::String(hash.into()));
    }
    Ok(RpcRequest {
        method: "state_getStorage",
        params: JsonValue::Array(params),
    })
}

/// Decode a `state_getStorage` result, rejecting values other than hex or null.
pub fn decode_result(
    runtime: &Runtime,
    info: &StorageInfo,
    result: &JsonValue,
) -> Result<Value, CoreError> {
    match result {
        JsonValue::Null => decode_storage(runtime, info, None),
        JsonValue::String(raw) => decode_storage(runtime, info, Some(raw)),
        _ => Err(CoreError::Rpc(
            "state_getStorage returned neither hex nor null".into(),
        )),
    }
}

/// Strictly decode a hex value, or apply the live entry's miss semantics.
///
/// An `Optional` miss returns `Value::Null`. A `Default` miss decodes the
/// metadata's default bytes through the same type and complete-consumption
/// checks as a present value.
pub fn decode_storage(
    runtime: &Runtime,
    info: &StorageInfo,
    raw: Option<&str>,
) -> Result<Value, CoreError> {
    match raw {
        Some(raw) => decode_storage_bytes(runtime, info, Some(&decode_hex(raw)?)),
        None => decode_storage_bytes(runtime, info, None),
    }
}

/// Decode raw SCALE storage bytes, with the same default/optional semantics.
pub fn decode_storage_bytes(
    runtime: &Runtime,
    info: &StorageInfo,
    raw: Option<&[u8]>,
) -> Result<Value, CoreError> {
    let bytes = match raw {
        Some(raw) => raw,
        None if info.modifier == "Optional" => return Ok(Value::Null),
        None => info.default_bytes.as_slice(),
    };
    runtime.decode_spec(&TypeSpec::Id(info.value_type), bytes, true)
}
