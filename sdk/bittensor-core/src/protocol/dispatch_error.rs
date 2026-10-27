// Derived from RaoFoundation/subtensor sdk/python/bittensor/_transport/utils/receipt.py @ 850f2755bb05a3851f948489f0ab27b0645cebce (Apache-2.0); modified by rtao.
//!
//! Python-shaped dispatch failures resolved against the included block runtime.

use crate::codec::value::ValueExt;
use crate::{codec::value::Value, runtime::Runtime, CoreError};
#[path = "python_printable.rs"]
mod python_printable;

#[path = "dispatch_error_tables.rs"]
mod tables;
use tables::DISPATCH_ERROR_DOCS;

/// Origin of a normalized failure description.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchErrorKind {
    /// A metadata-defined pallet error.
    Module,
    /// A system dispatch error, including inner Token/Arithmetic variants.
    System,
    /// A shield failure that carries its own diagnostic rather than DispatchError.
    MevShield,
}
impl DispatchErrorKind {
    /// Python's `type` field spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Module => "Module",
            Self::System => "System",
            Self::MevShield => "MevShield",
        }
    }
}

/// Failure name and exact Python-compatible description carrier.
#[derive(Debug, Clone, PartialEq)]
pub struct DispatchErrorInfo {
    /// Module, System or MevShield.
    pub kind: DispatchErrorKind,
    /// Runtime pallet name for a Module error, otherwise absent.
    pub pallet: Option<String>,
    /// Useful error variant name.
    pub name: String,
    /// Module documentation list, system description string or shield reason.
    pub docs: Value,
}
impl DispatchErrorInfo {
    /// Render the transport's `{type, name, docs}` error shape.
    pub fn to_value(&self) -> Value {
        Value::record(vec![
            ("type".into(), Value::str(self.kind.as_str())),
            ("name".into(), Value::str(&self.name)),
            ("docs".into(), self.docs.clone()),
        ])
    }
    /// Human diagnostic naming the pallet when metadata supplied one.
    pub fn message(&self) -> String {
        self.pallet.as_ref().map_or_else(
            || self.name.clone(),
            |pallet| format!("{pallet}.{}", self.name),
        )
    }

    /// Python's result message: truthy docs, then name, then the full error map.
    ///
    /// Documentation lists join strings with spaces. Other values use Python
    /// `str`, preserving byte and dictionary reasons from MevShield events.
    /// Non-string list members are invalid, as with Python's string `join`.
    pub fn result_message(&self) -> Result<String, CoreError> {
        if python_truthy(&self.docs) {
            if let Value::List(docs) = &self.docs {
                let docs = docs
                    .iter()
                    .map(|value| {
                        value.as_str().ok_or_else(|| {
                            CoreError::Codec("dispatch docs list contains a non-string".into())
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                return Ok(docs.join(" "));
            }
            return Ok(python_str(&self.docs));
        }
        if !self.name.is_empty() {
            return Ok(self.name.clone());
        }
        Ok(python_str(&self.to_value()))
    }
}

fn python_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Int(value) => *value != 0,
        Value::Uint(value) => *value != 0,
        Value::U256(bytes) => bytes.iter().any(|byte| *byte != 0),
        Value::Str(value) => !value.is_empty(),
        Value::Bytes(value) => !value.is_empty(),
        Value::List(value) | Value::Tuple(value) => !value.is_empty(),
        Value::Dict(value) => !value.is_empty(),
    }
}

/// Extract a Module dispatch error's pallet index and first error byte.
///
/// Supports the legacy tuple and named modern four-byte error layout.
pub fn normalize_module_error(value: &Value) -> Result<Option<(u8, u8)>, CoreError> {
    let Some(module) = value.variant("Module") else {
        return Ok(None);
    };
    let index = module
        .field("index")
        .or_else(|| module.tuple_item(0))
        .and_then(Value::as_u128)
        .and_then(|value| u8::try_from(value).ok())
        .ok_or_else(|| CoreError::Codec("module error omitted u8 pallet index".into()))?;
    let error = module
        .field("error")
        .or_else(|| module.tuple_item(1))
        .ok_or_else(|| CoreError::Codec("module error omitted error bytes".into()))?;
    let error = error
        .as_u128()
        .and_then(|value| u8::try_from(value).ok())
        .or_else(|| error.first_byte())
        .ok_or_else(|| CoreError::Codec("module error index is not a byte".into()))?;
    Ok(Some((index, error)))
}

fn system(name: String, docs: Value) -> DispatchErrorInfo {
    DispatchErrorInfo {
        kind: DispatchErrorKind::System,
        pallet: None,
        name,
        docs,
    }
}

/// Resolve a dispatch payload as Python's receipt decoder does.
///
/// Null has no diagnostic. Unknown module indices are errors, rather than a
/// fabricated name: the caller must pass the runtime that executed the block.
pub fn decode(runtime: &Runtime, value: &Value) -> Result<Option<DispatchErrorInfo>, CoreError> {
    if matches!(value, Value::Null) {
        return Ok(None);
    }
    if let Value::Str(name) = value {
        return Ok(Some(system(name.clone(), Value::str(name))));
    }
    let Value::Dict(entries) = value else {
        return Ok(Some(system(
            "DispatchError".into(),
            Value::str(python_str(value)),
        )));
    };
    if let Some((index, error)) = normalize_module_error(value)? {
        let (name, docs) = runtime.module_error(index, error)?;
        return Ok(Some(DispatchErrorInfo {
            kind: DispatchErrorKind::Module,
            pallet: runtime.pallet_at(index).map(|pallet| pallet.name.clone()),
            name,
            docs: Value::List(docs.into_iter().map(Value::str).collect()),
        }));
    }
    for name in [
        "BadOrigin",
        "CannotLookup",
        "Other",
        "Token",
        "Arithmetic",
        "Transactional",
        "Exhausted",
        "Corruption",
        "Unavailable",
        "RootNotAllowed",
    ] {
        let Some(variant) = value.field(name) else {
            continue;
        };
        let effective = match name {
            "Token" => match variant {
                Value::Dict(entries) if !entries.is_empty() => entries
                    .first()
                    .map(|(key, _)| python_str(key))
                    .unwrap_or_default(),
                _ => python_str(variant),
            },
            "Arithmetic" | "Transactional" => match variant {
                Value::Dict(entries) if !entries.is_empty() => entries
                    .first()
                    .map(|(key, _)| python_str(key))
                    .unwrap_or_default(),
                _ => name.into(),
            },
            _ => name.into(),
        };
        let docs = if name == "Other" {
            "Unspecified error occurred"
        } else {
            DISPATCH_ERROR_DOCS
                .iter()
                .find(|(name, _)| *name == effective)
                .map_or(effective.as_str(), |(_, docs)| *docs)
        };
        return Ok(Some(system(effective.clone(), Value::str(docs))));
    }
    match entries.as_slice() {
        [(name, detail)] => Ok(Some(system(
            python_str(name),
            Value::str(python_str(detail)),
        ))),
        _ => Ok(Some(system(
            "DispatchError".into(),
            Value::str(python_str(value)),
        ))),
    }
}

pub(super) fn python_str(value: &Value) -> String {
    match value {
        Value::Str(text) => text.clone(),
        _ => python_repr(value),
    }
}

fn string_repr(value: &str) -> String {
    let quote = if value.contains('\'') && !value.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut result = quote.to_string();
    for character in value.chars() {
        match character {
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            character if character == quote => {
                result.push('\\');
                result.push(character);
            }
            character if !python_printable::printable(character) => {
                let ordinal = u32::from(character);
                result.push_str(&if ordinal <= 0xff {
                    format!("\\x{ordinal:02x}")
                } else if ordinal <= 0xffff {
                    format!("\\u{ordinal:04x}")
                } else {
                    format!("\\U{ordinal:08x}")
                });
            }
            character => result.push(character),
        }
    }
    result.push(quote);
    result
}

fn python_repr(value: &Value) -> String {
    match value {
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::Int(value) => value.to_string(),
        Value::Uint(value) => value.to_string(),
        Value::U256(value) => crate::codec::value::u256_decimal(value),
        Value::Str(value) => string_repr(value),
        Value::Bytes(bytes) => {
            let quote = if bytes.contains(&b'\'') && !bytes.contains(&b'"') {
                '"'
            } else {
                '\''
            };
            let mut result = format!("b{quote}");
            for byte in bytes {
                match byte {
                    b'\\' => result.push_str("\\\\"),
                    b'\n' => result.push_str("\\n"),
                    b'\r' => result.push_str("\\r"),
                    b'\t' => result.push_str("\\t"),
                    byte if char::from(*byte) == quote => {
                        result.push('\\');
                        result.push(quote);
                    }
                    32..=126 => result.push(char::from(*byte)),
                    byte => result.push_str(&format!("\\x{byte:02x}")),
                }
            }
            result.push(quote);
            result
        }
        Value::List(items) => format!(
            "[{}]",
            items.iter().map(python_repr).collect::<Vec<_>>().join(", ")
        ),
        Value::Tuple(items) => format!(
            "({}{})",
            items.iter().map(python_repr).collect::<Vec<_>>().join(", "),
            if items.len() == 1 { "," } else { "" }
        ),
        Value::Dict(entries) => format!(
            "{{{}}}",
            entries
                .iter()
                .map(|(key, value)| format!("{}: {}", python_repr(key), python_repr(value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}
