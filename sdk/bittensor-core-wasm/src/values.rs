//! Value <-> JS materialization — the wasm counterpart of the Python
//! binding's `values.rs`. Shapes mirror it exactly: ints in the f64-safe
//! range become JS numbers, bigger ones BigInt; bytes become `Uint8Array`;
//! tuples become arrays (JS has none); dicts become plain objects when every
//! key is a string and `Map` otherwise (BTreeMap keys keep their type).

// Client-side conversion code: arithmetic on locally validated digits and
// in-bounds limb loops are the norm here, as in the core's codec.
#![allow(clippy::arithmetic_side_effects)]

use bittensor_core::codec::value::{u256_decimal, Value};
use js_sys::{Array, BigInt, Map, Object, Reflect, Uint8Array};
use wasm_bindgen::{JsCast, JsValue};

use crate::errors::value_err;

/// Integers with |x| <= 2^53 - 1 round-trip exactly through f64.
const MAX_SAFE_INT: i128 = 9_007_199_254_740_991;

/// Matches the core codec's recursion ceiling (and the Python binding's).
const MAX_JS_VALUE_DEPTH: usize = 256;

fn bigint_from_decimal(decimal: &str) -> Result<JsValue, JsValue> {
    BigInt::new(&JsValue::from_str(decimal))
        .map(Into::into)
        .map_err(|_| value_err(format!("invalid integer literal: {decimal}")))
}

fn int_to_js(value: i128) -> Result<JsValue, JsValue> {
    if (-MAX_SAFE_INT..=MAX_SAFE_INT).contains(&value) {
        Ok(JsValue::from_f64(value as f64))
    } else {
        bigint_from_decimal(&value.to_string())
    }
}

fn uint_to_js(value: u128) -> Result<JsValue, JsValue> {
    if value <= MAX_SAFE_INT as u128 {
        Ok(JsValue::from_f64(value as f64))
    } else {
        bigint_from_decimal(&value.to_string())
    }
}

/// Own-property assignment that cannot reach the inherited `__proto__`
/// setter. `Reflect.set(obj, "__proto__", v)` replaces the object's
/// prototype instead of defining a property — a spoofing vector when keys
/// come from chain state (BTreeMap keys) or RPC-supplied metadata (field
/// and API names). The define-property path is taken only for that key, so
/// the hot materialization path stays a plain [[Set]].
pub fn set_own(target: &Object, key: &str, value: &JsValue) -> Result<(), JsValue> {
    if key == "__proto__" {
        let descriptor = Object::new();
        for (k, v) in [
            ("value", value.clone()),
            ("writable", JsValue::TRUE),
            ("enumerable", JsValue::TRUE),
            ("configurable", JsValue::TRUE),
        ] {
            Reflect::set(&descriptor, &JsValue::from_str(k), &v)
                .map_err(|_| value_err("failed to build property descriptor"))?;
        }
        Object::define_property(target, &JsValue::from_str(key), &descriptor);
        return Ok(());
    }
    Reflect::set(target, &JsValue::from_str(key), value)
        .map(|_| ())
        .map_err(|_| value_err("failed to set object property"))
}

/// Materialize a decoded value as plain JS values.
pub fn value_to_js(value: &Value) -> Result<JsValue, JsValue> {
    Ok(match value {
        Value::Null => JsValue::NULL,
        Value::Bool(b) => JsValue::from_bool(*b),
        Value::Int(i) => int_to_js(*i)?,
        Value::Uint(u) => uint_to_js(*u)?,
        Value::U256(le) => bigint_from_decimal(&u256_decimal(le))?,
        Value::Str(s) => JsValue::from_str(s),
        Value::Bytes(b) => Uint8Array::from(b.as_slice()).into(),
        Value::List(items) | Value::Tuple(items) => {
            let array = Array::new();
            for item in items {
                array.push(&value_to_js(item)?);
            }
            array.into()
        }
        Value::Dict(entries) => {
            let all_string_keys = entries.iter().all(|(k, _)| matches!(k, Value::Str(_)));
            if all_string_keys {
                let object = Object::new();
                for (key, val) in entries {
                    let Value::Str(name) = key else { continue };
                    set_own(&object, name, &value_to_js(val)?)?;
                }
                object.into()
            } else {
                let map = Map::new();
                for (key, val) in entries {
                    map.set(&value_to_js(key)?, &value_to_js(val)?);
                }
                map.into()
            }
        }
    })
}

/// Parse a decimal string into little-endian u256 bytes.
fn decimal_to_u256_le(decimal: &str) -> Option<[u8; 32]> {
    if decimal.is_empty() {
        return None;
    }
    let mut le = [0u8; 32];
    for ch in decimal.bytes() {
        let digit = ch.wrapping_sub(b'0');
        if digit > 9 {
            return None;
        }
        let mut carry = u16::from(digit);
        for byte in le.iter_mut() {
            let value = u16::from(*byte) * 10 + carry;
            *byte = (value & 0xff) as u8;
            carry = value >> 8;
        }
        if carry != 0 {
            return None;
        }
    }
    Some(le)
}

fn bigint_to_value(value: &JsValue) -> Result<Value, JsValue> {
    let big = BigInt::new(value).map_err(|_| value_err("invalid BigInt"))?;
    let decimal: String = big
        .to_string(10)
        .map_err(|_| value_err("BigInt rendering failed"))?
        .into();
    if let Ok(i) = decimal.parse::<i128>() {
        return Ok(Value::Int(i));
    }
    if let Ok(u) = decimal.parse::<u128>() {
        return Ok(Value::Uint(u));
    }
    if decimal.starts_with('-') {
        return Err(value_err("integer below i128 range"));
    }
    decimal_to_u256_le(&decimal)
        .map(Value::U256)
        .ok_or_else(|| value_err("integer out of u256 range"))
}

/// Accept the lenient JS inputs the codec seam takes everywhere else:
/// numbers/BigInt, strings, Uint8Array, arrays, Maps, and plain objects.
pub fn js_to_value(value: &JsValue) -> Result<Value, JsValue> {
    js_to_value_at(value, 0)
}

fn js_to_value_at(value: &JsValue, depth: usize) -> Result<Value, JsValue> {
    if depth > MAX_JS_VALUE_DEPTH {
        return Err(value_err(format!(
            "value nesting exceeds {MAX_JS_VALUE_DEPTH} levels"
        )));
    }
    if value.is_null() || value.is_undefined() {
        return Ok(Value::Null);
    }
    if let Some(b) = value.as_bool() {
        return Ok(Value::Bool(b));
    }
    if let Some(f) = value.as_f64() {
        if !f.is_finite() || f.fract() != 0.0 || f.abs() > MAX_SAFE_INT as f64 {
            return Err(value_err(
                "cannot encode a non-integer or unsafe-range number as SCALE; use BigInt",
            ));
        }
        return Ok(Value::Int(f as i128));
    }
    if value.is_bigint() {
        return bigint_to_value(value);
    }
    if let Some(s) = value.as_string() {
        return Ok(Value::Str(s));
    }
    if let Some(bytes) = value.dyn_ref::<Uint8Array>() {
        return Ok(Value::Bytes(bytes.to_vec()));
    }
    let deeper = depth.saturating_add(1);
    if Array::is_array(value) {
        let array = Array::from(value);
        let mut items = Vec::with_capacity(array.length() as usize);
        for item in array.iter() {
            items.push(js_to_value_at(&item, deeper)?);
        }
        return Ok(Value::List(items));
    }
    if let Some(map) = value.dyn_ref::<Map>() {
        let mut entries = Vec::with_capacity(map.size() as usize);
        for pair in Array::from(&map.entries()).iter() {
            let pair = Array::from(&pair);
            entries.push((
                js_to_value_at(&pair.get(0), deeper)?,
                js_to_value_at(&pair.get(1), deeper)?,
            ));
        }
        return Ok(Value::Dict(entries));
    }
    if value.is_object() {
        let object = Object::from(value.clone());
        let pairs = Object::entries(&object);
        let mut entries = Vec::with_capacity(pairs.length() as usize);
        for pair in pairs.iter() {
            let pair = Array::from(&pair);
            let key = pair
                .get(0)
                .as_string()
                .ok_or_else(|| value_err("object keys must be strings"))?;
            entries.push((Value::Str(key), js_to_value_at(&pair.get(1), deeper)?));
        }
        return Ok(Value::Dict(entries));
    }
    Err(value_err("cannot encode this JS value as SCALE"))
}

/// Materialize decoded map pages as `[key, value]` pair arrays: single free
/// key yields a scalar key, multiple yield an array (the JS tuple).
pub fn materialize_pairs(decoded: &[(Vec<Value>, Value)]) -> Result<Array, JsValue> {
    let out = Array::new();
    for (params, value) in decoded {
        let key = if let [single] = params.as_slice() {
            value_to_js(single)?
        } else {
            let parts = Array::new();
            for param in params {
                parts.push(&value_to_js(param)?);
            }
            parts.into()
        };
        let pair = Array::new();
        pair.push(&key);
        pair.push(&value_to_js(value)?);
        out.push(&pair);
    }
    Ok(out)
}

/// Coerce a JS number or BigInt argument to u64 (block numbers, nonces,
/// rounds — semantically small, so plain numbers must work).
pub fn u64_arg(value: &JsValue, name: &str) -> Result<u64, JsValue> {
    match js_to_value(value)? {
        Value::Int(i) => u64::try_from(i).ok(),
        Value::Uint(u) => u64::try_from(u).ok(),
        _ => None,
    }
    .ok_or_else(|| value_err(format!("{name} must be a non-negative integer within u64")))
}

/// Coerce a JS number or BigInt argument to u128 (tips, balances).
pub fn u128_arg(value: &JsValue, name: &str) -> Result<u128, JsValue> {
    match js_to_value(value)? {
        Value::Int(i) => u128::try_from(i).ok(),
        Value::Uint(u) => Some(u),
        _ => None,
    }
    .ok_or_else(|| value_err(format!("{name} must be a non-negative integer within u128")))
}

/// Like `u128_arg` but treats null/undefined as None.
pub fn optional_u128_arg(value: &JsValue, name: &str) -> Result<Option<u128>, JsValue> {
    if value.is_null() || value.is_undefined() {
        return Ok(None);
    }
    u128_arg(value, name).map(Some)
}
