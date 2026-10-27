// Derived from RaoFoundation/subtensor sdk/python/bittensor/_transport/interface.py @ 850f2755bb05a3851f948489f0ab27b0645cebce (Apache-2.0); modified by rtao.
//!
//! Immortal zero-signature fee probes and metadata-defined dispatch-info decode.

use super::{metadata::response_bytes, runtime_api, RpcRequest};
use crate::codec::value::ValueExt;
use crate::{
    codec::{extrinsic::TxParams, value::Value},
    runtime::Runtime,
    CoreError,
};
use serde_json::{json, Value as Json};

/// Native fee and the exact runtime weight/dispatch-class values.
#[derive(Debug, Clone, PartialEq)]
pub struct FeeEstimate {
    /// Fee in native u64 rao, including checked conversion from the core carrier.
    pub partial_fee: u64,
    /// WeightV1 integer or WeightV2 `{ref_time, proof_size}` dictionary.
    pub weight: Value,
    /// Runtime dispatch class (Normal, Operational or Mandatory).
    pub class: Value,
}

/// Encode an immortal extrinsic with a zeroed 64-byte signature.
///
/// The caller supplies the uncached chain nonce; this function performs no
/// nonce allocation, signing, submission or metadata-hash enabling.
pub fn probe(
    runtime: &Runtime,
    call_data: &[u8],
    public_key: [u8; 32],
    signature_version: u8,
    nonce: u64,
    genesis: [u8; 32],
) -> Result<Vec<u8>, CoreError> {
    let params = TxParams {
        era: Value::str("00"),
        nonce,
        tip: 0,
        tip_asset_id: None,
        genesis_hash: genesis,
        era_block_hash: genesis,
        metadata_hash: None,
    };
    runtime
        .encode_signed_extrinsic(call_data, public_key, &[0; 64], signature_version, &params)
        .map(|(bytes, _)| bytes)
}

/// Runtime-API query arguments are the complete extrinsic followed by u32 length.
pub fn request(extrinsic: &[u8], block_hash: Option<&str>) -> Result<RpcRequest, CoreError> {
    let length = u32::try_from(extrinsic.len())
        .map_err(|_| CoreError::Codec("fee probe extrinsic length exceeds u32".into()))?;
    let mut args = extrinsic.to_vec();
    args.extend_from_slice(&length.to_le_bytes());
    Ok(RpcRequest {
        method: "state_call",
        params: json!([
            "TransactionPaymentApi_query_info",
            format!("0x{}", hex::encode(args)),
            block_hash
        ]),
    })
}

/// Decode a JSON hexadecimal result through its live runtime-API output type.
pub fn decode(runtime: &Runtime, result: &Json) -> Result<FeeEstimate, CoreError> {
    decode_bytes(
        runtime,
        &response_bytes(result, "TransactionPaymentApi_query_info result")?,
    )
}

/// Strictly decode the complete RuntimeDispatchInfo, rejecting trailing bytes.
pub fn decode_bytes(runtime: &Runtime, bytes: &[u8]) -> Result<FeeEstimate, CoreError> {
    let method = runtime_api::runtime_api_method(runtime, "TransactionPaymentApi", "query_info")?;
    let value = runtime_api::decode_bytes(runtime, method, bytes)?;
    let fee = value
        .field("partial_fee")
        .and_then(Value::as_u128)
        .and_then(|fee| u64::try_from(fee).ok())
        .ok_or_else(|| CoreError::Codec("partial_fee is not native u64 rao".into()))?;
    let required = |name: &str| {
        value
            .field(name)
            .cloned()
            .ok_or_else(|| CoreError::Codec(format!("RuntimeDispatchInfo omitted {name}")))
    };
    Ok(FeeEstimate {
        partial_fee: fee,
        weight: required("weight")?,
        class: required("class")?,
    })
}
