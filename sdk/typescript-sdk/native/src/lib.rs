mod digest;
mod errors;
mod keys;
#[cfg(feature = "ledger")]
mod ledger;
mod mlkem;
mod runtime;
mod timelock;
mod values;

use bittensor_core::codec::value::{to_corpus_json, u256_decimal};
use napi::bindgen_prelude::Buffer;
use napi_derive::napi;
use serde_json::Value as JsonValue;

use crate::errors::{invalid_arg, NapiResult};
use crate::values::{from_wire, to_wire, WIRE_TAG};

#[napi(js_name = "bindingVersion")]
pub fn binding_version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

#[napi(js_name = "ledgerEnabled")]
pub fn ledger_enabled() -> bool {
    cfg!(feature = "ledger")
}

#[napi(js_name = "wireTag")]
pub fn wire_tag() -> String {
    WIRE_TAG.to_owned()
}

#[napi(js_name = "wireRoundtrip")]
pub fn wire_roundtrip(value: JsonValue) -> NapiResult<JsonValue> {
    to_wire(&from_wire(value)?)
}

#[napi(js_name = "valueToCorpusJson")]
pub fn value_to_corpus_json(value: JsonValue) -> NapiResult<JsonValue> {
    Ok(to_corpus_json(&from_wire(value)?))
}

#[napi(js_name = "u256LeToDecimal")]
pub fn u256_le_to_decimal(raw: Buffer) -> NapiResult<String> {
    let bytes: [u8; 32] = raw
        .as_ref()
        .try_into()
        .map_err(|_| invalid_arg("u256 value must be exactly 32 little-endian bytes"))?;
    Ok(u256_decimal(&bytes))
}
