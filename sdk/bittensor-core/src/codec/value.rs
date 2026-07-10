//! The decoded-value model: the exact shapes cyscale rendered SCALE data as,
//! which the SDK's reads/intents layer pattern-matches on. The shape corpus
//! (sdk/python/tests/fixtures/shape_corpus) is the contract; this enum is its
//! Rust carrier, materialized 1:1 into Python objects by the binding crate.

#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]

use std::fmt;

/// One decoded SCALE value in cyscale's shape conventions.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// Python ``None`` (Option misses, ``Pays::No``-style unit payload holes).
    Null,
    Bool(bool),
    /// All signed integers (and any unsigned that fits).
    Int(i128),
    /// Unsigned integers above ``i128::MAX`` (u128 range).
    Uint(u128),
    /// u256: little-endian bytes, materialized as an arbitrary-size int.
    U256([u8; 32]),
    Str(String),
    /// Raw bytes (kept distinct so the binding can hand back ``bytes`` where
    /// cyscale did; most byte-like types render as 0x-hex strings instead).
    Bytes(Vec<u8>),
    /// Python ``list``.
    List(Vec<Value>),
    /// Python ``tuple`` (SCALE tuples and multi-unnamed-field shapes).
    Tuple(Vec<Value>),
    /// Python ``dict`` in insertion order; keys are values because BTreeMap
    /// keys keep their native type (ints stay ints).
    Dict(Vec<(Value, Value)>),
}

impl Value {
    pub fn str(s: impl Into<String>) -> Self {
        Value::Str(s.into())
    }

    pub fn hex(data: &[u8]) -> Self {
        Value::Str(format!("0x{}", hex::encode(data)))
    }

    /// Struct-style dict with string keys.
    pub fn record(fields: Vec<(String, Value)>) -> Self {
        Value::Dict(
            fields
                .into_iter()
                .map(|(k, v)| (Value::Str(k), v))
                .collect(),
        )
    }
}

/// Render as the JSON the corpus recorder's ``jsonable()`` produced: bytes as
/// 0x-hex, tuples as lists, dict keys stringified. Only used by tests to
/// compare against corpus fixtures.
pub fn to_corpus_json(value: &Value) -> serde_json::Value {
    use serde_json::json;
    match value {
        Value::Null => serde_json::Value::Null,
        Value::Bool(b) => json!(b),
        Value::Int(i) => serde_json::Value::Number(
            serde_json::Number::from_i128(*i).unwrap_or_else(|| serde_json::Number::from(0)),
        ),
        Value::Uint(u) => serde_json::Value::Number(
            serde_json::Number::from_u128(*u).unwrap_or_else(|| serde_json::Number::from(0)),
        ),
        Value::U256(le) => {
            // The recorder went through Python ints and json.dumps, which
            // prints arbitrary precision; serde_json's arbitrary_precision
            // feature lets the comparison carry it digit-exact.
            let number = u256_decimal(le)
                .parse::<serde_json::Number>()
                .unwrap_or_else(|_| serde_json::Number::from(0));
            serde_json::Value::Number(number)
        }
        Value::Str(s) => json!(s),
        Value::Bytes(b) => json!(format!("0x{}", hex::encode(b))),
        Value::List(items) | Value::Tuple(items) => {
            serde_json::Value::Array(items.iter().map(to_corpus_json).collect())
        }
        Value::Dict(entries) => {
            let mut map = serde_json::Map::new();
            for (k, v) in entries {
                map.insert(corpus_key(k), to_corpus_json(v));
            }
            serde_json::Value::Object(map)
        }
    }
}

/// ``str(key)`` the way Python renders dict keys in the recorder.
fn corpus_key(key: &Value) -> String {
    match key {
        Value::Str(s) => s.clone(),
        Value::Int(i) => i.to_string(),
        Value::Uint(u) => u.to_string(),
        Value::Bool(b) => (if *b { "True" } else { "False" }).to_string(),
        other => format!("{other:?}"),
    }
}

/// Decimal rendering of a little-endian u256.
pub fn u256_decimal(le: &[u8; 32]) -> String {
    // Repeated division by 10 over the big-endian limbs; 32 bytes is small
    // enough that the simple O(n * digits) loop is instant.
    let mut limbs: Vec<u8> = le.iter().rev().copied().collect(); // big-endian
    let mut digits = Vec::new();
    while limbs.iter().any(|&b| b != 0) {
        let mut remainder: u32 = 0;
        for byte in limbs.iter_mut() {
            let value = (remainder << 8) | u32::from(*byte);
            *byte = (value / 10) as u8;
            remainder = value % 10;
        }
        digits.push(char::from(b'0' + remainder as u8));
    }
    if digits.is_empty() {
        return "0".to_string();
    }
    digits.iter().rev().collect()
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", to_corpus_json(self))
    }
}
