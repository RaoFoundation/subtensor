//! Lossless boundary representation for dynamic SCALE values.
//!
//! Node-API itself handles ordinary scalars and objects, while the tiny tagged
//! representation below preserves JavaScript `bigint`, bytes, and dictionaries
//! whose keys are not strings. The public TypeScript layer applies/removes
//! these tags. All actual SCALE semantics remain in `bittensor-core`.

#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

use std::collections::HashSet;

use bittensor_core::codec::value::{u256_decimal, Value};
use serde_json::{Map, Number, Value as JsonValue};

use crate::errors::{invalid_arg, NapiResult};

pub const WIRE_TAG: &str = "__bittensor_core_wire__";
const TAG_BIGINT: &str = "bigint";
const TAG_BYTES: &str = "bytes";
const TAG_DICT: &str = "dict";
const MAX_SAFE_INTEGER: i128 = 9_007_199_254_740_991;
const MIN_SAFE_INTEGER: i128 = -MAX_SAFE_INTEGER;
const MAX_WIRE_DEPTH: usize = 256;

pub fn from_wire(value: JsonValue) -> NapiResult<Value> {
    from_wire_at(value, 0)
}

fn from_wire_at(value: JsonValue, depth: usize) -> NapiResult<Value> {
    if depth > MAX_WIRE_DEPTH {
        return Err(invalid_arg("value nesting exceeds 256 levels"));
    }
    match value {
        JsonValue::Null => Ok(Value::Null),
        JsonValue::Bool(value) => Ok(Value::Bool(value)),
        JsonValue::Number(number) => number_to_value(number),
        JsonValue::String(value) => Ok(Value::Str(value)),
        JsonValue::Array(values) => values
            .into_iter()
            .map(|value| from_wire_at(value, depth + 1))
            .collect::<NapiResult<Vec<_>>>()
            .map(Value::List),
        JsonValue::Object(map) => object_from_wire(map, depth + 1),
    }
}

fn number_to_value(number: Number) -> NapiResult<Value> {
    if let Some(value) = number.as_i64() {
        return Ok(Value::Int(i128::from(value)));
    }
    if let Some(value) = number.as_u64() {
        let value = u128::from(value);
        return Ok(if value <= i128::MAX as u128 {
            Value::Int(value as i128)
        } else {
            Value::Uint(value)
        });
    }
    Err(invalid_arg(format!(
        "SCALE values must use integers; got {number}"
    )))
}

fn object_from_wire(mut map: Map<String, JsonValue>, depth: usize) -> NapiResult<Value> {
    let tag = map.get(WIRE_TAG).and_then(JsonValue::as_str);
    match tag {
        Some(TAG_BIGINT) => {
            let decimal = map
                .remove("value")
                .and_then(|value| value.as_str().map(str::to_owned))
                .ok_or_else(|| invalid_arg("bigint wire value is missing decimal `value`"))?;
            bigint_value(&decimal)
        }
        Some(TAG_BYTES) => {
            let hex_value = map
                .remove("hex")
                .and_then(|value| value.as_str().map(str::to_owned))
                .ok_or_else(|| invalid_arg("bytes wire value is missing `hex`"))?;
            let raw = hex::decode(hex_value.trim_start_matches("0x"))
                .map_err(|error| invalid_arg(format!("invalid bytes hex: {error}")))?;
            Ok(Value::Bytes(raw))
        }
        Some(TAG_DICT) => {
            let entries = map
                .remove("entries")
                .and_then(|value| value.as_array().cloned())
                .ok_or_else(|| invalid_arg("dict wire value is missing `entries`"))?;
            let mut output = Vec::with_capacity(entries.len());
            for entry in entries {
                let JsonValue::Array(pair) = entry else {
                    return Err(invalid_arg("dict entry must be a [key, value] pair"));
                };
                if pair.len() != 2 {
                    return Err(invalid_arg("dict entry must contain exactly two values"));
                }
                let mut pair = pair.into_iter();
                let key = pair
                    .next()
                    .ok_or_else(|| invalid_arg("dict entry is missing its key"))?;
                let value = pair
                    .next()
                    .ok_or_else(|| invalid_arg("dict entry is missing its value"))?;
                output.push((
                    from_wire_at(key, depth + 1)?,
                    from_wire_at(value, depth + 1)?,
                ));
            }
            Ok(Value::Dict(output))
        }
        _ => map
            .into_iter()
            .map(|(key, value)| {
                Ok((
                    Value::Str(key),
                    from_wire_at(value, depth + 1)?,
                ))
            })
            .collect::<NapiResult<Vec<_>>>()
            .map(Value::Dict),
    }
}

fn bigint_value(decimal: &str) -> NapiResult<Value> {
    let decimal = decimal.trim();
    if decimal.is_empty() {
        return Err(invalid_arg("empty bigint decimal string"));
    }
    if decimal.starts_with('-') {
        let value = decimal
            .parse::<i128>()
            .map_err(|_| invalid_arg("negative bigint is outside the i128 SCALE range"))?;
        return Ok(Value::Int(value));
    }
    let unsigned = decimal.strip_prefix('+').unwrap_or(decimal);
    if unsigned.is_empty() || !unsigned.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid_arg(format!(
            "invalid bigint decimal string {decimal:?}"
        )));
    }
    if let Ok(value) = unsigned.parse::<i128>() {
        return Ok(Value::Int(value));
    }
    if let Ok(value) = unsigned.parse::<u128>() {
        return Ok(Value::Uint(value));
    }
    Ok(Value::U256(decimal_to_u256_le(unsigned)?))
}

fn decimal_to_u256_le(decimal: &str) -> NapiResult<[u8; 32]> {
    let mut output = [0u8; 32];
    for digit in decimal.bytes() {
        let digit = u16::from(digit - b'0');
        let mut carry = digit;
        for byte in &mut output {
            let value = u16::from(*byte) * 10 + carry;
            *byte = (value & 0xff) as u8;
            carry = value >> 8;
        }
        if carry != 0 {
            return Err(invalid_arg("bigint is outside the unsigned 256-bit range"));
        }
    }
    Ok(output)
}

pub fn to_wire(value: &Value) -> NapiResult<JsonValue> {
    to_wire_at(value, 0)
}

fn to_wire_at(value: &Value, depth: usize) -> NapiResult<JsonValue> {
    if depth > MAX_WIRE_DEPTH {
        return Err(invalid_arg("decoded value nesting exceeds 256 levels"));
    }
    match value {
        Value::Null => Ok(JsonValue::Null),
        Value::Bool(value) => Ok(JsonValue::Bool(*value)),
        Value::Int(value) if (MIN_SAFE_INTEGER..=MAX_SAFE_INTEGER).contains(value) => {
            let integer = i64::try_from(*value)
                .map_err(|_| invalid_arg("safe integer conversion failed"))?;
            Ok(JsonValue::Number(Number::from(integer)))
        }
        Value::Int(value) => Ok(tagged_bigint(value.to_string())),
        Value::Uint(value) if *value <= MAX_SAFE_INTEGER as u128 => {
            let integer = u64::try_from(*value)
                .map_err(|_| invalid_arg("safe unsigned integer conversion failed"))?;
            Ok(JsonValue::Number(Number::from(integer)))
        }
        Value::Uint(value) => Ok(tagged_bigint(value.to_string())),
        Value::U256(value) => Ok(tagged_bigint(u256_decimal(value))),
        Value::Str(value) => Ok(JsonValue::String(value.clone())),
        Value::Bytes(value) => {
            let mut map = Map::new();
            map.insert(WIRE_TAG.into(), JsonValue::String(TAG_BYTES.into()));
            map.insert("hex".into(), JsonValue::String(hex::encode(value)));
            Ok(JsonValue::Object(map))
        }
        Value::List(values) | Value::Tuple(values) => values
            .iter()
            .map(|value| to_wire_at(value, depth + 1))
            .collect::<NapiResult<Vec<_>>>()
            .map(JsonValue::Array),
        Value::Dict(entries) => dict_to_wire(entries, depth + 1),
    }
}

fn tagged_bigint(decimal: String) -> JsonValue {
    let mut map = Map::new();
    map.insert(WIRE_TAG.into(), JsonValue::String(TAG_BIGINT.into()));
    map.insert("value".into(), JsonValue::String(decimal));
    JsonValue::Object(map)
}

fn dict_to_wire(entries: &[(Value, Value)], depth: usize) -> NapiResult<JsonValue> {
    let mut names = HashSet::with_capacity(entries.len());
    let plain_object = entries.iter().all(|(key, _)| match key {
        Value::Str(name) => name != WIRE_TAG && names.insert(name.clone()),
        _ => false,
    });

    if plain_object {
        let mut map = Map::new();
        for (key, value) in entries {
            let Value::Str(name) = key else {
                return Err(invalid_arg("internal string-key conversion failed"));
            };
            map.insert(name.clone(), to_wire_at(value, depth + 1)?);
        }
        return Ok(JsonValue::Object(map));
    }

    let mut pairs = Vec::with_capacity(entries.len());
    for (key, value) in entries {
        pairs.push(JsonValue::Array(vec![
            to_wire_at(key, depth + 1)?,
            to_wire_at(value, depth + 1)?,
        ]));
    }
    let mut map = Map::new();
    map.insert(WIRE_TAG.into(), JsonValue::String(TAG_DICT.into()));
    map.insert("entries".into(), JsonValue::Array(pairs));
    Ok(JsonValue::Object(map))
}

pub fn values_from_wire(value: JsonValue) -> NapiResult<Vec<Value>> {
    let JsonValue::Array(values) = value else {
        return Err(invalid_arg("expected an array of SCALE values"));
    };
    values.into_iter().map(from_wire).collect()
}

pub fn values_to_wire(values: &[Value]) -> NapiResult<JsonValue> {
    values
        .iter()
        .map(to_wire)
        .collect::<NapiResult<Vec<_>>>()
        .map(JsonValue::Array)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn bigint_roundtrip_covers_u256() {
        let decimal = "115792089237316195423570985008687907853269984665640564039457584007913129639935";
        let value = bigint_value(decimal).expect("u256 parses");
        assert!(matches!(value, Value::U256(_)));
        let wire = to_wire(&value).expect("u256 renders");
        assert_eq!(wire["value"], decimal);
    }

    #[test]
    fn bare_plus_is_not_a_bigint() {
        assert!(bigint_value("+").is_err());
    }

    #[test]
    fn non_string_dict_keys_use_entry_wire_shape() {
        let value = Value::Dict(vec![(Value::Int(7), Value::Str("seven".into()))]);
        let wire = to_wire(&value).expect("dict renders");
        assert_eq!(wire[WIRE_TAG], TAG_DICT);
        let decoded = from_wire(wire).expect("dict parses");
        assert_eq!(decoded, value);
    }

    #[test]
    fn bytes_roundtrip_without_json_arrays() {
        let value = Value::Bytes(vec![0, 1, 0xfe, 0xff]);
        let wire = to_wire(&value).expect("bytes render");
        assert_eq!(wire["hex"], "0001feff");
        assert_eq!(from_wire(wire).expect("bytes parse"), value);
    }
}
