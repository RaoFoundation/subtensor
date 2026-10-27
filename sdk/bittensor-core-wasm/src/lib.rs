//! wasm-bindgen bindings for `bittensor-core`. No logic lives here:
//! constructors, method forwarding, error mapping, and JS-object
//! materialization only — the same role `bittensor-core-py` plays for Python.
//!
//! The TS shell owns all I/O (WebSocket JSON-RPC, `fetch`); this module owns
//! everything whose right answer is defined by the chain: crypto, SCALE,
//! extrinsic payloads, metadata digests. Host-only surfaces (keyfiles,
//! Ledger, drand fetching) are deliberately absent — the crate builds
//! `bittensor-core` without its `host` feature.

use wasm_bindgen::prelude::*;

mod digest;
mod errors;
mod keys;
mod runtime;
mod timelock;
mod values;

/// This binding crate's version (mirrors the Python binding's
/// `__core_version__`, which likewise reports the binding, not the core).
#[wasm_bindgen(js_name = coreVersion)]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[wasm_bindgen(js_name = blake2_256)]
pub fn blake2_256(data: &[u8]) -> Result<Vec<u8>, JsValue> {
    bittensor_core::codec::storage::hash_param("Blake2_256", data).map_err(errors::to_js_err)
}

/// Derive a storage prefix even after an item has been removed from metadata.
#[wasm_bindgen(js_name = storagePrefixFor)]
pub fn storage_prefix_for(pallet: &str, item: &str) -> Result<Vec<u8>, JsValue> {
    use bittensor_core::codec::storage::hash_param;
    let mut prefix = hash_param("Twox128", pallet.as_bytes()).map_err(errors::to_js_err)?;
    prefix.extend(hash_param("Twox128", item.as_bytes()).map_err(errors::to_js_err)?);
    Ok(prefix)
}

#[wasm_bindgen(js_name = decodeOptionalOpaqueMetadata)]
pub fn decode_optional_opaque_metadata(data: &[u8]) -> Result<Option<Vec<u8>>, JsValue> {
    use bittensor_core::codec::decode::Cursor;
    let Some((&tag, rest)) = data.split_first() else {
        return Err(errors::value_err("empty metadata response"));
    };
    match tag {
        0 if rest.is_empty() => Ok(None),
        1 => {
            let mut cursor = Cursor::new(rest);
            cursor.strict = true;
            let length = bittensor_core::codec::decode::compact_len(&mut cursor)
                .map_err(errors::to_js_err)?;
            let metadata = rest
                .get(cursor.offset..)
                .ok_or_else(|| errors::value_err("invalid metadata length"))?;
            if metadata.len() != length {
                return Err(errors::value_err(
                    "invalid metadata length or trailing bytes",
                ));
            }
            Ok(Some(metadata.to_vec()))
        }
        _ => Err(errors::value_err("invalid optional metadata response")),
    }
}
