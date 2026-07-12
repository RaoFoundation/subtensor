#![cfg_attr(test, allow(dead_code))]

mod digest;
mod errors;
mod keys;
#[cfg(feature = "ledger")]
mod ledger;
mod mlkem;
mod runtime;
mod timelock;
mod transaction;
mod values;

use bittensor_core::codec::value::{to_corpus_json, u256_decimal};
use bittensor_core::codec::Value;
use napi::bindgen_prelude::{BigInt, Buffer};
use napi_derive::napi;
use serde_json::Value as JsonValue;

use crate::errors::{invalid_arg, NapiResult};
use crate::values::{from_descriptor, from_wire, to_descriptor, to_wire, WIRE_TAG};

#[napi(object)]
pub struct NativeCoreValueField {
    pub name: String,
    pub value: JsonValue,
}

#[napi(object)]
pub struct NativeCoreValueEntry {
    pub key: JsonValue,
    pub value: JsonValue,
}

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

#[napi(js_name = "coreValueDescriptorRoundtrip")]
pub fn core_value_descriptor_roundtrip(value: JsonValue) -> NapiResult<JsonValue> {
    to_descriptor(&from_descriptor(value)?)
}

#[napi(js_name = "coreValueDescriptorToWire")]
pub fn core_value_descriptor_to_wire(value: JsonValue) -> NapiResult<JsonValue> {
    to_wire(&from_descriptor(value)?)
}

#[napi(js_name = "wireToCoreValueDescriptor")]
pub fn wire_to_core_value_descriptor(value: JsonValue) -> NapiResult<JsonValue> {
    to_descriptor(&from_wire(value)?)
}

#[napi(js_name = "coreValueDescriptorToCorpusJson")]
pub fn core_value_descriptor_to_corpus_json(value: JsonValue) -> NapiResult<JsonValue> {
    Ok(to_corpus_json(&from_descriptor(value)?))
}

#[napi(js_name = "coreValueDescriptorDisplay")]
pub fn core_value_descriptor_display(value: JsonValue) -> NapiResult<String> {
    Ok(from_descriptor(value)?.to_string())
}

#[napi(js_name = "coreValueNull")]
pub fn core_value_null() -> NapiResult<JsonValue> {
    to_descriptor(&Value::Null)
}

#[napi(js_name = "coreValueBool")]
pub fn core_value_bool(value: bool) -> NapiResult<JsonValue> {
    to_descriptor(&Value::Bool(value))
}

#[napi(js_name = "coreValueInt")]
pub fn core_value_int(value: BigInt) -> NapiResult<JsonValue> {
    let (negative, magnitude, lossless) = value.get_u128();
    if !lossless {
        return Err(invalid_arg("integer value must fit the Rust i128 range"));
    }
    let value = if negative {
        if magnitude == i128::MIN.unsigned_abs() {
            i128::MIN
        } else {
            i128::try_from(magnitude)
                .ok()
                .and_then(i128::checked_neg)
                .ok_or_else(|| invalid_arg("integer value must fit the Rust i128 range"))?
        }
    } else {
        i128::try_from(magnitude)
            .map_err(|_| invalid_arg("integer value must fit the Rust i128 range"))?
    };
    to_descriptor(&Value::Int(value))
}

#[napi(js_name = "coreValueUint")]
pub fn core_value_uint(value: BigInt) -> NapiResult<JsonValue> {
    let (negative, value, lossless) = value.get_u128();
    if negative || !lossless {
        return Err(invalid_arg("unsigned value must fit the Rust u128 range"));
    }
    to_descriptor(&Value::Uint(value))
}

#[napi(js_name = "coreValueU256Le")]
pub fn core_value_u256_le(raw: Buffer) -> NapiResult<JsonValue> {
    let raw: [u8; 32] = raw
        .as_ref()
        .try_into()
        .map_err(|_| invalid_arg("u256 value must be exactly 32 little-endian bytes"))?;
    to_descriptor(&Value::U256(raw))
}

#[napi(js_name = "coreValueString")]
pub fn core_value_string(value: String) -> NapiResult<JsonValue> {
    to_descriptor(&Value::str(value))
}

#[napi(js_name = "coreValueBytes")]
pub fn core_value_bytes(value: Buffer) -> NapiResult<JsonValue> {
    to_descriptor(&Value::Bytes(value.as_ref().to_vec()))
}

#[napi(js_name = "coreValueList")]
pub fn core_value_list(items: Vec<JsonValue>) -> NapiResult<JsonValue> {
    let items = items
        .into_iter()
        .map(from_descriptor)
        .collect::<NapiResult<Vec<_>>>()?;
    to_descriptor(&Value::List(items))
}

#[napi(js_name = "coreValueTuple")]
pub fn core_value_tuple(items: Vec<JsonValue>) -> NapiResult<JsonValue> {
    let items = items
        .into_iter()
        .map(from_descriptor)
        .collect::<NapiResult<Vec<_>>>()?;
    to_descriptor(&Value::Tuple(items))
}

#[napi(js_name = "coreValueDict")]
pub fn core_value_dict(entries: Vec<NativeCoreValueEntry>) -> NapiResult<JsonValue> {
    let entries = entries
        .into_iter()
        .map(|entry| Ok((from_descriptor(entry.key)?, from_descriptor(entry.value)?)))
        .collect::<NapiResult<Vec<_>>>()?;
    to_descriptor(&Value::Dict(entries))
}

#[napi(js_name = "coreValueRecord")]
pub fn core_value_record(fields: Vec<NativeCoreValueField>) -> NapiResult<JsonValue> {
    let fields = fields
        .into_iter()
        .map(|field| Ok((field.name, from_descriptor(field.value)?)))
        .collect::<NapiResult<Vec<_>>>()?;
    to_descriptor(&Value::record(fields))
}

#[napi(js_name = "coreValueHex")]
pub fn core_value_hex(value: Buffer) -> NapiResult<JsonValue> {
    to_descriptor(&Value::hex(value.as_ref()))
}
