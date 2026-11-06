//! Portable navigation of decoded SCALE values.

use super::Value;

// The pinned codec's encode/decode recursion limits are also 256 levels.
const MAX_BYTE_WRAPPERS: usize = 256;

/// Borrowing navigation helpers for the core's decoded SCALE carrier.
///
/// Implemented directly on [`Value`], so the methods also work on `&Value`
/// through ordinary method dereferencing. The equivalent free functions are
/// useful when composing `Option::and_then` chains.
pub trait ValueExt {
    /// Borrow the first dictionary field whose string key equals `name`.
    fn field(&self, name: &str) -> Option<&Value>;

    /// Borrow a named variant's payload using the dictionary field rules.
    fn variant(&self, name: &str) -> Option<&Value>;

    /// Read a unit variant's string or a single string-keyed variant's name.
    fn variant_name(&self) -> Option<String>;

    /// Borrow a string without converting other value types.
    fn as_str(&self) -> Option<&str>;

    /// Read an unsigned integer, rejecting negative and non-integer values.
    fn as_u128(&self) -> Option<u128>;

    /// Read a boolean without converting other value types.
    fn as_bool(&self) -> Option<bool>;

    /// Extract bytes from a byte value, string, integer sequence or wrapper.
    ///
    /// Lowercase `0x` strings are decoded as hex; other strings use UTF-8.
    /// Lists and tuples accept integers in `0..=255`. A singleton sequence
    /// or dictionary can wrap another supported shape, up to 256 times.
    fn value_bytes(&self) -> Option<Vec<u8>>;

    /// Borrow an element of a tuple or list, returning `None` out of bounds.
    fn tuple_item(&self, index: usize) -> Option<&Value>;

    /// Read the first extracted byte, returning `None` for empty byte values.
    fn first_byte(&self) -> Option<u8>;
}

impl ValueExt for Value {
    fn field(&self, name: &str) -> Option<&Value> {
        field(self, name)
    }

    fn variant(&self, name: &str) -> Option<&Value> {
        variant(self, name)
    }

    fn variant_name(&self) -> Option<String> {
        variant_name(self)
    }

    fn as_str(&self) -> Option<&str> {
        as_str(self)
    }

    fn as_u128(&self) -> Option<u128> {
        as_u128(self)
    }

    fn as_bool(&self) -> Option<bool> {
        as_bool(self)
    }

    fn value_bytes(&self) -> Option<Vec<u8>> {
        value_bytes(self)
    }

    fn tuple_item(&self, index: usize) -> Option<&Value> {
        tuple_item(self, index)
    }

    fn first_byte(&self) -> Option<u8> {
        first_byte(self)
    }
}

/// Borrow the first dictionary field whose string key equals `name`.
pub fn field<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    let Value::Dict(entries) = value else {
        return None;
    };
    entries.iter().find_map(|(key, value)| match key {
        Value::Str(key) if key == name => Some(value),
        _ => None,
    })
}

/// Borrow a variant payload using the same matching rules as [`field`].
pub fn variant<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    field(value, name)
}

/// Return the name of a string variant or a single string-keyed dictionary.
pub fn variant_name(value: &Value) -> Option<String> {
    match value {
        Value::Str(name) => Some(name.clone()),
        Value::Dict(entries) if entries.len() == 1 => entries.first().and_then(|(key, _)| {
            if let Value::Str(name) = key {
                Some(name.clone())
            } else {
                None
            }
        }),
        _ => None,
    }
}

/// Borrow a string without converting other value types.
pub fn as_str(value: &Value) -> Option<&str> {
    match value {
        Value::Str(value) => Some(value),
        _ => None,
    }
}

/// Read an unsigned integer, rejecting negative and non-integer values.
pub fn as_u128(value: &Value) -> Option<u128> {
    match value {
        Value::Int(value) => u128::try_from(*value).ok(),
        Value::Uint(value) => Some(*value),
        _ => None,
    }
}

/// Read a boolean without converting other value types.
pub fn as_bool(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(value) => Some(*value),
        _ => None,
    }
}

/// Extract bytes with the v478 client's shape and integer conversion rules.
///
/// Strings with a lowercase `0x` prefix decode as hex. Other strings encode
/// as UTF-8. Integer lists and tuples accept only values in `0..=255`. If a
/// sequence cannot be read as integers, its sole element can be another byte
/// value. A singleton dictionary similarly wraps its value, ignoring the key.
/// At most 256 wrappers are followed; deeper input returns `None` without
/// recursing on the process stack.
pub fn value_bytes(mut value: &Value) -> Option<Vec<u8>> {
    for _ in 0..=MAX_BYTE_WRAPPERS {
        match value {
            Value::Bytes(bytes) => return Some(bytes.clone()),
            Value::Str(text) => {
                return if text.starts_with("0x") {
                    hex::decode(text.trim_start_matches("0x")).ok()
                } else {
                    Some(text.as_bytes().to_vec())
                };
            }
            Value::List(items) | Value::Tuple(items) => {
                let direct = items
                    .iter()
                    .map(as_u128)
                    .map(|value| value.and_then(|value| u8::try_from(value).ok()))
                    .collect::<Option<Vec<_>>>();
                if let Some(bytes) = direct {
                    return Some(bytes);
                }
                match items.as_slice() {
                    [inner] => value = inner,
                    _ => return None,
                }
            }
            Value::Dict(entries) => match entries.as_slice() {
                [(_, inner)] => value = inner,
                _ => return None,
            },
            _ => return None,
        }
    }
    None
}

/// Borrow a tuple or list element, returning `None` for other shapes or indexes.
pub fn tuple_item(value: &Value, index: usize) -> Option<&Value> {
    match value {
        Value::Tuple(items) | Value::List(items) => items.get(index),
        _ => None,
    }
}

/// Return the first byte extracted by [`value_bytes`], if one exists.
pub fn first_byte(value: &Value) -> Option<u8> {
    value_bytes(value).and_then(|bytes| bytes.first().copied())
}
