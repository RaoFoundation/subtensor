//! The type-string compatibility grammar.
//!
//! Portable-registry type ids are the core's native currency, but the seam
//! keeps accepting the small string grammar the SDK already speaks:
//!
//! - `scale_info::N` — a registry id directly
//! - registry names from the name map (`AccountId32`, `NeuronInfoLite`,
//!   `Vec<NeuronInfo>`, ...)
//! - structural compositions: `Vec<T>`, `Option<T>`, `[T; N]`,
//!   `(A, B, ...)`, `Compact<T>`
//! - primitive names (`u8`..`u128`, `bool`, `str`, `String`)
//! - a few special carriers: `Bytes` (= `Vec<u8>`), `AccountId` (32-byte
//!   key rendered/accepted as ss58), `Era`, `Call`, `H256`

use crate::error::CoreError;
use crate::runtime::Runtime;

/// A resolved type expression: either a registry id or a structural node
/// built over other specs.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeSpec {
    Id(u32),
    Primitive(Primitive),
    /// `Vec<T>` requested by string (not a registry id).
    Sequence(Box<TypeSpec>),
    Option(Box<TypeSpec>),
    Array(Box<TypeSpec>, u32),
    Tuple(Vec<TypeSpec>),
    Compact(Box<TypeSpec>),
    /// `Bytes` / `Vec<u8>`: utf8-or-hex string rendering.
    Bytes,
    /// 32-byte public key rendered/accepted as ss58.
    AccountId,
    /// `sp_runtime::generic::Era` ("00" / {"period","current"}).
    Era,
    /// The runtime's outer Call enum.
    Call,
    /// The full extrinsic wire format.
    Extrinsic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Primitive {
    Bool,
    Char,
    Str,
    U8,
    U16,
    U32,
    U64,
    U128,
    U256,
    I8,
    I16,
    I32,
    I64,
    I128,
    I256,
}

impl Primitive {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "bool" => Primitive::Bool,
            "char" => Primitive::Char,
            "str" | "String" => Primitive::Str,
            "u8" => Primitive::U8,
            "u16" => Primitive::U16,
            "u32" => Primitive::U32,
            "u64" => Primitive::U64,
            "u128" => Primitive::U128,
            "u256" => Primitive::U256,
            "i8" => Primitive::I8,
            "i16" => Primitive::I16,
            "i32" => Primitive::I32,
            "i64" => Primitive::I64,
            "i128" => Primitive::I128,
            "i256" => Primitive::I256,
            _ => return None,
        })
    }
}

impl Runtime {
    /// Resolve a type string to a spec, preferring structural parses over the
    /// registry name map so `Vec<u8>` and `Option<...>` always mean the same
    /// thing regardless of which names this runtime's registry produces.
    pub fn type_spec(&self, type_string: &str) -> Result<TypeSpec, CoreError> {
        let s = type_string.trim();
        if let Some(id) = s.strip_prefix("scale_info::") {
            let id: u32 = id
                .parse()
                .map_err(|_| CoreError::Codec(format!("bad scale_info id in {s:?}")))?;
            return Ok(TypeSpec::Id(id));
        }
        match s {
            "Bytes" | "Vec<u8>" => return Ok(TypeSpec::Bytes),
            "AccountId" => return Ok(TypeSpec::AccountId),
            "Era" => return Ok(TypeSpec::Era),
            "Call" | "RuntimeCall" => return Ok(TypeSpec::Call),
            "Extrinsic" => return Ok(TypeSpec::Extrinsic),
            "Compact" => {
                return Ok(TypeSpec::Compact(Box::new(TypeSpec::Primitive(
                    Primitive::U128,
                ))))
            }
            _ => {}
        }
        if let Some(inner) = strip_wrapper(s, "Vec") {
            return Ok(TypeSpec::Sequence(Box::new(self.type_spec(inner)?)));
        }
        if let Some(inner) = strip_wrapper(s, "Option") {
            return Ok(TypeSpec::Option(Box::new(self.type_spec(inner)?)));
        }
        if let Some(inner) = strip_wrapper(s, "Compact") {
            return Ok(TypeSpec::Compact(Box::new(self.type_spec(inner)?)));
        }
        if s.starts_with('[') && s.ends_with(']') {
            let body = &s[1..s.len() - 1];
            let (elem, len) = body
                .rsplit_once(';')
                .ok_or_else(|| CoreError::Codec(format!("bad array type string {s:?}")))?;
            let len: u32 = len
                .trim()
                .parse()
                .map_err(|_| CoreError::Codec(format!("bad array length in {s:?}")))?;
            return Ok(TypeSpec::Array(Box::new(self.type_spec(elem)?), len));
        }
        if s.starts_with('(') && s.ends_with(')') {
            let mut parts = Vec::new();
            for part in split_top_level(&s[1..s.len() - 1]) {
                let part = part.trim();
                if !part.is_empty() {
                    parts.push(self.type_spec(part)?);
                }
            }
            return Ok(TypeSpec::Tuple(parts));
        }
        if let Some(primitive) = Primitive::from_name(s) {
            // Prefer the registry's own primitive ids when present, so the
            // decoded shape flows through the same path either way.
            if let Some(id) = self.type_id_of(s) {
                return Ok(TypeSpec::Id(id));
            }
            return Ok(TypeSpec::Primitive(primitive));
        }
        if let Some(id) = self.type_id_of(s) {
            return Ok(TypeSpec::Id(id));
        }
        Err(CoreError::Codec(format!(
            "type {s:?} not found in this runtime's registry"
        )))
    }
}

/// `Wrapper<inner>` -> `inner` (only for the exact wrapper name).
fn strip_wrapper<'a>(s: &'a str, wrapper: &str) -> Option<&'a str> {
    let body = s.strip_prefix(wrapper)?.strip_prefix('<')?;
    body.strip_suffix('>')
}

/// Split on top-level commas (not inside <>, (), []).
fn split_top_level(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}
