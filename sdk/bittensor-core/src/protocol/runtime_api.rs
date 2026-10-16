// Derived from RaoFoundation/subtensor sdk/bittensor-core/src/client.rs @ 850f2755bb05a3851f948489f0ab27b0645cebce (Apache-2.0); modified by rtao.
//!
//! Runtime-API method lookup, positional argument encoding and result decoding.
//!
//! Extracted from `runtime_call` and `runtime_api_method` in
//! `sdk/bittensor-core/src/client.rs` at subtensor v478
//! (`850f2755bb05a3851f948489f0ab27b0645cebce`), Apache-2.0.

use crate::{
    codec::value::Value,
    runtime::{type_string::TypeSpec, Runtime, RuntimeApiMethodInfo},
    CoreError,
};
use serde_json::Value as JsonValue;

use super::{metadata::response_bytes, RpcRequest};

/// Borrow one method from the named API in live V15 metadata.
///
/// V14 metadata exposes no API signatures; archive fallback tables and API
/// version policy belong to the caller.
pub fn runtime_api_method<'a>(
    runtime: &'a Runtime,
    api: &str,
    method: &str,
) -> Result<&'a RuntimeApiMethodInfo, CoreError> {
    runtime
        .apis
        .iter()
        .find(|candidate| candidate.name == api)
        .and_then(|candidate| {
            candidate
                .methods
                .iter()
                .find(|candidate| candidate.name == method)
        })
        .ok_or_else(|| CoreError::NotInRuntime(format!("runtime API {api}.{method}")))
}

/// Encode positional arguments in the metadata's exact input order.
pub fn encode_params(
    runtime: &Runtime,
    api: &str,
    method: &RuntimeApiMethodInfo,
    params: &[Value],
) -> Result<Vec<u8>, CoreError> {
    if method.inputs.len() != params.len() {
        return Err(CoreError::Codec(format!(
            "{api}.{} expects {} params, got {}",
            method.name,
            method.inputs.len(),
            params.len()
        )));
    }
    let mut encoded = Vec::new();
    for (input, value) in method.inputs.iter().zip(params.iter()) {
        runtime.encode_id(input.ty, value, &mut encoded)?;
    }
    Ok(encoded)
}

/// Resolve and encode a `state_call` request, optionally at a pinned block.
pub fn request(
    runtime: &Runtime,
    api: &str,
    method: &str,
    params: &[Value],
    block_hash: Option<&str>,
) -> Result<RpcRequest, CoreError> {
    let info = runtime_api_method(runtime, api, method)?;
    let encoded = encode_params(runtime, api, info, params)?;
    let mut params = vec![
        JsonValue::String(format!("{api}_{method}")),
        JsonValue::String(format!("0x{}", hex::encode(encoded))),
    ];
    if let Some(hash) = block_hash {
        params.push(JsonValue::String(hash.into()));
    }
    Ok(RpcRequest {
        method: "state_call",
        params: JsonValue::Array(params),
    })
}

/// Decode a hex `state_call` result using the method's live output type.
pub fn decode_result(
    runtime: &Runtime,
    method: &RuntimeApiMethodInfo,
    result: &JsonValue,
) -> Result<Value, CoreError> {
    decode_bytes(
        runtime,
        method,
        &response_bytes(result, "state_call result")?,
    )
}

/// Decode the entire SCALE result, rejecting truncated or trailing bytes.
pub fn decode_bytes(
    runtime: &Runtime,
    method: &RuntimeApiMethodInfo,
    bytes: &[u8],
) -> Result<Value, CoreError> {
    runtime.decode_spec(&TypeSpec::Id(method.output), bytes, true)
}
