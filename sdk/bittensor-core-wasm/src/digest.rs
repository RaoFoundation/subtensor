//! Bindings for `bittensor_core::digest` — RFC-0078 merkleized metadata.

use bittensor_core::digest::{self, ChainInfo};
use wasm_bindgen::prelude::*;

use crate::errors::to_js_err;

/// The Bittensor chain constants baked into the runtime's metadata hash
/// (`runtime/build.rs` `enable_metadata_hash("TAO", 9)`, ss58 prefix 42).
/// Callers targeting another chain pass their own values.
const DEFAULT_BASE58_PREFIX: u16 = 42;
const DEFAULT_DECIMALS: u8 = 9;
const DEFAULT_TOKEN_SYMBOL: &str = "TAO";

fn chain_info(
    spec_version: u32,
    spec_name: &str,
    base58_prefix: Option<u16>,
    decimals: Option<u8>,
    token_symbol: Option<String>,
) -> ChainInfo {
    ChainInfo {
        spec_version,
        spec_name: spec_name.to_owned(),
        base58_prefix: base58_prefix.unwrap_or(DEFAULT_BASE58_PREFIX),
        decimals: decimals.unwrap_or(DEFAULT_DECIMALS),
        token_symbol: token_symbol.unwrap_or_else(|| DEFAULT_TOKEN_SYMBOL.to_owned()),
    }
}

/// The RFC-0078 metadata digest: the 32-byte hash signed via the
/// `CheckMetadataHash` extension.
#[wasm_bindgen(js_name = metadataDigest)]
pub fn metadata_digest(
    metadata_bytes: &[u8],
    spec_version: u32,
    spec_name: &str,
    base58_prefix: Option<u16>,
    decimals: Option<u8>,
    token_symbol: Option<String>,
) -> Result<Vec<u8>, JsValue> {
    let info = chain_info(
        spec_version,
        spec_name,
        base58_prefix,
        decimals,
        token_symbol,
    );
    digest::metadata_digest(metadata_bytes, &info)
        .map(|hash| hash.to_vec())
        .map_err(to_js_err)
}

/// The SCALE-encoded metadata proof an offline signer decodes on-device
/// (appended after the signature payload when signing).
#[allow(clippy::too_many_arguments)]
#[wasm_bindgen(js_name = generateExtrinsicProof)]
pub fn generate_extrinsic_proof(
    call_data: &[u8],
    included_in_extrinsic: &[u8],
    included_in_signed_data: &[u8],
    metadata_bytes: &[u8],
    spec_version: u32,
    spec_name: &str,
    base58_prefix: Option<u16>,
    decimals: Option<u8>,
    token_symbol: Option<String>,
) -> Result<Vec<u8>, JsValue> {
    let info = chain_info(
        spec_version,
        spec_name,
        base58_prefix,
        decimals,
        token_symbol,
    );
    digest::generate_extrinsic_proof(
        call_data,
        included_in_extrinsic,
        included_in_signed_data,
        metadata_bytes,
        &info,
    )
    .map_err(to_js_err)
}
