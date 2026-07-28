//! SCALE encode with the seam's lenient input coercions: ss58 strings or
//! 0x-hex for account ids, hex/str/bytes for byte carriers, bare variant
//! names or `{"Variant": payload}` dicts for enums, `None` for Option-None,
//! ints for compacts. Byte-equality with the previous codec is pinned by the
//! golden call/payload/extrinsic vectors.

// Client-side codec, not runtime code: buffers grow as values encode and
// arithmetic operates on lengths and era math.
#![allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use std::cell::Cell;

use scale_info::{form::PortableForm, Field, TypeDef, TypeDefPrimitive};

use crate::codec::value::Value;
use crate::error::CoreError;
use crate::keys::public_key_from_ss58;
use crate::runtime::type_string::{Primitive, TypeSpec};
use crate::runtime::Runtime;

/// Recursion ceiling, mirroring `decode.rs`. Metadata is untrusted (it comes
/// from the connected node) and `encode_fields` unwraps single-field newtypes
/// without consuming value structure, so a self-referential composite would
/// otherwise recurse until the stack aborts the process. Encoding has no
/// cursor to carry depth, so a thread-local tracks it (encode never crosses
/// threads mid-value).
const MAX_ENCODE_DEPTH: usize = 256;

thread_local! {
    static ENCODE_DEPTH: Cell<usize> = const { Cell::new(0) };
}

struct DepthGuard;

impl DepthGuard {
    fn enter() -> Result<Self, CoreError> {
        let depth = ENCODE_DEPTH.with(|d| {
            let next = d.get() + 1;
            d.set(next);
            next
        });
        // The guard is constructed either way, so Drop rebalances the counter
        // on both the error path and normal unwinding.
        let guard = DepthGuard;
        if depth > MAX_ENCODE_DEPTH {
            return Err(CoreError::Codec(format!(
                "encode recursion exceeds {MAX_ENCODE_DEPTH} levels (malformed or malicious type definition)"
            )));
        }
        Ok(guard)
    }
}

impl Drop for DepthGuard {
    fn drop(&mut self) {
        ENCODE_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    }
}

/// Compact-encode an unsigned integer.
pub fn compact(value: u128, out: &mut Vec<u8>) -> Result<(), CoreError> {
    if value < 1 << 6 {
        out.push((value as u8) << 2);
    } else if value < 1 << 14 {
        out.extend_from_slice(&(((value as u16) << 2) | 0b01).to_le_bytes());
    } else if value < 1 << 30 {
        out.extend_from_slice(&(((value as u32) << 2) | 0b10).to_le_bytes());
    } else {
        let bytes = value.to_le_bytes();
        let significant = 16 - bytes.iter().rev().take_while(|&&b| b == 0).count();
        let len = significant.max(4);
        out.push((((len - 4) as u8) << 2) | 0b11);
        out.extend_from_slice(&bytes[..len]);
    }
    Ok(())
}

impl Runtime {
    pub fn encode_spec(&self, spec: &TypeSpec, value: &Value) -> Result<Vec<u8>, CoreError> {
        let mut out = Vec::new();
        self.encode_value(spec, value, &mut out)?;
        Ok(out)
    }

    pub fn encode_value(
        &self,
        spec: &TypeSpec,
        value: &Value,
        out: &mut Vec<u8>,
    ) -> Result<(), CoreError> {
        let _depth = DepthGuard::enter()?;
        match spec {
            TypeSpec::Id(id) => self.encode_id(*id, value, out),
            TypeSpec::Primitive(p) => encode_primitive_spec(*p, value, out),
            TypeSpec::Sequence(inner) => {
                if matches!(**inner, TypeSpec::Primitive(Primitive::U8)) {
                    let bytes = coerce_bytes(value, None)?;
                    compact(bytes.len() as u128, out)?;
                    out.extend_from_slice(&bytes);
                    return Ok(());
                }
                let items = coerce_list(value)?;
                compact(items.len() as u128, out)?;
                for item in items {
                    self.encode_value(inner, item, out)?;
                }
                Ok(())
            }
            TypeSpec::Option(inner) => {
                if matches!(value, Value::Null) {
                    out.push(0);
                    return Ok(());
                }
                out.push(1);
                self.encode_value(inner, value, out)
            }
            TypeSpec::Array(inner, len) => {
                if matches!(**inner, TypeSpec::Primitive(Primitive::U8)) {
                    let bytes = coerce_bytes(value, Some(*len as usize))?;
                    out.extend_from_slice(&bytes);
                    return Ok(());
                }
                let items = coerce_list(value)?;
                if items.len() != *len as usize {
                    return Err(CoreError::Codec(format!(
                        "array expects {len} elements, got {}",
                        items.len()
                    )));
                }
                for item in items {
                    self.encode_value(inner, item, out)?;
                }
                Ok(())
            }
            TypeSpec::Tuple(parts) => {
                let items = coerce_list(value)?;
                if items.len() != parts.len() {
                    return Err(CoreError::Codec(format!(
                        "tuple expects {} elements, got {}",
                        parts.len(),
                        items.len()
                    )));
                }
                for (part, item) in parts.iter().zip(items) {
                    self.encode_value(part, item, out)?;
                }
                Ok(())
            }
            TypeSpec::Compact(_) => compact(coerce_uint(value)?, out),
            TypeSpec::Bytes => {
                let bytes = coerce_bytes(value, None)?;
                compact(bytes.len() as u128, out)?;
                out.extend_from_slice(&bytes);
                Ok(())
            }
            TypeSpec::AccountId => {
                let raw = self.coerce_account_id(value)?;
                out.extend_from_slice(&raw);
                Ok(())
            }
            TypeSpec::Era => self.encode_era_value(value, out),
            TypeSpec::Call => {
                // Calls arrive as raw bytes (CallBytes) or a decoded call dict.
                self.encode_call_input(value, out)
            }
            TypeSpec::Extrinsic => Err(CoreError::Codec(
                "extrinsics are assembled by the transport layer".into(),
            )),
        }
    }

    pub fn encode_id(&self, id: u32, value: &Value, out: &mut Vec<u8>) -> Result<(), CoreError> {
        let _depth = DepthGuard::enter()?;
        let ty = self.resolve(id)?;
        let segments = &ty.path.segments;
        let last = segments.last().map(String::as_str);

        match last {
            Some("AccountId32") => {
                let raw = self.coerce_account_id(value)?;
                out.extend_from_slice(&raw);
                return Ok(());
            }
            Some("Era") if segments.first().map(String::as_str) == Some("sp_runtime") => {
                return self.encode_era_value(value, out);
            }
            Some("MultiAddress") => {
                return self.encode_multiaddress(ty, value, out);
            }
            Some("BTreeMap") => {
                return self.encode_btreemap(ty, value, out);
            }
            _ => {}
        }

        // The outer RuntimeCall (and pallet call enums embedded in params)
        // accept pre-composed raw bytes.
        if let Value::Bytes(raw) = value {
            if self.extrinsic.call_type == Some(id) {
                out.extend_from_slice(raw);
                return Ok(());
            }
        }

        match &ty.type_def {
            TypeDef::Primitive(p) => encode_primitive(p, value, out),
            TypeDef::Compact(_) => compact(coerce_uint(value)?, out),
            TypeDef::Composite(composite) => self.encode_fields(&composite.fields, value, out),
            TypeDef::Variant(variant) => {
                let is_option = last == Some("Option") && segments.len() == 1;
                if is_option {
                    if matches!(value, Value::Null) {
                        out.push(0);
                        return Ok(());
                    }
                    let some = variant
                        .variants
                        .iter()
                        .find(|v| v.name == "Some")
                        .ok_or_else(|| CoreError::Codec("Option without Some".into()))?;
                    out.push(some.index);
                    return self.encode_fields(&some.fields, value, out);
                }
                let (name, payload) = coerce_variant(value)?;
                let chosen = variant
                    .variants
                    .iter()
                    .find(|v| v.name == name)
                    .ok_or_else(|| {
                        CoreError::Codec(format!("no variant named {name:?} in type {id}"))
                    })?;
                out.push(chosen.index);
                if chosen.fields.is_empty() {
                    return Ok(());
                }
                self.encode_fields(&chosen.fields, &payload, out)
            }
            TypeDef::Sequence(s) => {
                if self.is_u8_encode(s.type_param.id) {
                    let bytes = coerce_bytes(value, None)?;
                    compact(bytes.len() as u128, out)?;
                    out.extend_from_slice(&bytes);
                    return Ok(());
                }
                let items = coerce_list(value)?;
                compact(items.len() as u128, out)?;
                for item in items {
                    self.encode_id(s.type_param.id, item, out)?;
                }
                Ok(())
            }
            TypeDef::Array(a) => {
                if self.is_u8_encode(a.type_param.id) {
                    let bytes = coerce_bytes(value, Some(a.len as usize))?;
                    out.extend_from_slice(&bytes);
                    return Ok(());
                }
                let items = coerce_list(value)?;
                if items.len() != a.len as usize {
                    return Err(CoreError::Codec(format!(
                        "array expects {} elements, got {}",
                        a.len,
                        items.len()
                    )));
                }
                for item in items {
                    self.encode_id(a.type_param.id, item, out)?;
                }
                Ok(())
            }
            TypeDef::Tuple(t) => {
                if t.fields.is_empty() {
                    return Ok(());
                }
                let items = coerce_list(value)?;
                if items.len() != t.fields.len() {
                    return Err(CoreError::Codec(format!(
                        "tuple expects {} elements, got {}",
                        t.fields.len(),
                        items.len()
                    )));
                }
                for (field, item) in t.fields.iter().zip(items) {
                    self.encode_id(field.id, item, out)?;
                }
                Ok(())
            }
            TypeDef::BitSequence(_) => Err(CoreError::Codec(
                "bit sequences are not used by this chain".into(),
            )),
        }
    }

    fn is_u8_encode(&self, id: u32) -> bool {
        matches!(
            self.types.resolve(id).map(|t| &t.type_def),
            Some(TypeDef::Primitive(TypeDefPrimitive::U8))
        )
    }

    /// Composite-body encode: named fields from a dict, single unnamed from
    /// the bare value, several unnamed from a list/tuple.
    fn encode_fields(
        &self,
        fields: &[Field<PortableForm>],
        value: &Value,
        out: &mut Vec<u8>,
    ) -> Result<(), CoreError> {
        if fields.is_empty() {
            return Ok(());
        }
        let named = fields.iter().all(|f| f.name.is_some());
        if named {
            let Value::Dict(entries) = value else {
                return Err(CoreError::Codec(format!(
                    "struct expects a dict, got {value}"
                )));
            };
            for field in fields {
                let name = field.name.as_deref().unwrap_or_default();
                let item = entries
                    .iter()
                    .find(|(k, _)| matches!(k, Value::Str(s) if s == name))
                    .map(|(_, v)| v)
                    .ok_or_else(|| CoreError::Codec(format!("missing struct field {name:?}")))?;
                self.encode_id(field.ty.id, item, out)?;
            }
            return Ok(());
        }
        if fields.len() == 1 {
            // Legacy-codec compat: scalecodec's newtype encode (a composite
            // with one unnamed field, e.g. BoundedVec) consumed one
            // list-nesting level, so legacy callers double-wrap sequence
            // payloads — `CommitmentInfo { fields: [[{"Raw5": ...}]] }`.
            // Unwrap that shape when the inner type is a sequence; the flat
            // shape stays untouched because its sole element is not a list.
            if let Value::List(items) | Value::Tuple(items) = value {
                if items.len() == 1
                    && matches!(items[0], Value::List(_))
                    && matches!(
                        self.resolve(fields[0].ty.id).map(|t| &t.type_def),
                        Ok(TypeDef::Sequence(_))
                    )
                {
                    return self.encode_id(fields[0].ty.id, &items[0], out);
                }
            }
            return self.encode_id(fields[0].ty.id, value, out);
        }
        let items = coerce_list(value)?;
        if items.len() != fields.len() {
            return Err(CoreError::Codec(format!(
                "expects {} unnamed fields, got {}",
                fields.len(),
                items.len()
            )));
        }
        for (field, item) in fields.iter().zip(items) {
            self.encode_id(field.ty.id, item, out)?;
        }
        Ok(())
    }

    fn encode_multiaddress(
        &self,
        ty: &scale_info::Type<PortableForm>,
        value: &Value,
        out: &mut Vec<u8>,
    ) -> Result<(), CoreError> {
        let TypeDef::Variant(variant) = &ty.type_def else {
            return Err(CoreError::Codec("MultiAddress is not an enum".into()));
        };
        // Bare strings/bytes mean the Id variant; explicit dicts choose.
        match value {
            Value::Str(_) | Value::Bytes(_) => {
                let id_variant = variant
                    .variants
                    .iter()
                    .find(|v| v.name == "Id")
                    .ok_or_else(|| CoreError::Codec("MultiAddress without Id".into()))?;
                out.push(id_variant.index);
                let raw = self.coerce_account_id(value)?;
                out.extend_from_slice(&raw);
                Ok(())
            }
            Value::Dict(_) => {
                let (name, payload) = coerce_variant(value)?;
                let chosen = variant
                    .variants
                    .iter()
                    .find(|v| v.name == name)
                    .ok_or_else(|| {
                        CoreError::Codec(format!("no MultiAddress variant named {name:?}"))
                    })?;
                out.push(chosen.index);
                self.encode_fields(&chosen.fields, &payload, out)
            }
            other => Err(CoreError::Codec(format!(
                "cannot encode {other} as MultiAddress"
            ))),
        }
    }

    fn encode_btreemap(
        &self,
        ty: &scale_info::Type<PortableForm>,
        value: &Value,
        out: &mut Vec<u8>,
    ) -> Result<(), CoreError> {
        let TypeDef::Composite(composite) = &ty.type_def else {
            return Err(CoreError::Codec("BTreeMap is not a composite".into()));
        };
        let inner = composite
            .fields
            .first()
            .map(|f| f.ty.id)
            .ok_or_else(|| CoreError::Codec("BTreeMap has no inner field".into()))?;
        let TypeDef::Sequence(seq) = &self.resolve(inner)?.type_def else {
            return Err(CoreError::Codec("BTreeMap inner is not a sequence".into()));
        };
        let TypeDef::Tuple(pair) = &self.resolve(seq.type_param.id)?.type_def else {
            return Err(CoreError::Codec("BTreeMap pair is not a tuple".into()));
        };
        let (key_ty, value_ty) = match pair.fields.as_slice() {
            [k, v] => (k.id, v.id),
            _ => return Err(CoreError::Codec("BTreeMap pair is not 2-ary".into())),
        };
        let Value::Dict(entries) = value else {
            return Err(CoreError::Codec("BTreeMap expects a dict".into()));
        };
        compact(entries.len() as u128, out)?;
        for (k, v) in entries {
            self.encode_id(key_ty, k, out)?;
            self.encode_id(value_ty, v, out)?;
        }
        Ok(())
    }

    /// Era inputs: `"00"` (immortal), 0/None-like handled by caller, or
    /// `{"period": N, "current": M}` / `{"period": N, "phase": P}`.
    pub fn encode_era_value(&self, value: &Value, out: &mut Vec<u8>) -> Result<(), CoreError> {
        match value {
            Value::Str(s) if s == "00" => {
                out.push(0);
                Ok(())
            }
            Value::Dict(entries) => {
                let get = |name: &str| -> Option<u64> {
                    entries.iter().find_map(|(k, v)| match (k, v) {
                        (Value::Str(s), Value::Int(i)) if s == name => u64::try_from(*i).ok(),
                        (Value::Str(s), Value::Uint(u)) if s == name => u64::try_from(*u).ok(),
                        _ => None,
                    })
                };
                let period = get("period")
                    .ok_or_else(|| CoreError::Codec("mortal era needs a period".into()))?;
                let phase = match (get("phase"), get("current")) {
                    (Some(phase), _) => phase,
                    (None, Some(current)) => {
                        let calculated = calculated_period(period);
                        let quantize_factor = (calculated >> 12).max(1);
                        (current % calculated) / quantize_factor * quantize_factor
                    }
                    (None, None) => {
                        return Err(CoreError::Codec(
                            "mortal era needs a phase or current block".into(),
                        ))
                    }
                };
                let calculated = calculated_period(period);
                let quantize_factor = (calculated >> 12).max(1);
                // The low nibble stores log2(period) - 1 (decode does
                // `2 << low`), clamped to the mortal range.
                let low = u64::from(calculated.trailing_zeros())
                    .saturating_sub(1)
                    .clamp(1, 15);
                let encoded = low | ((phase / quantize_factor) << 4);
                out.extend_from_slice(&(encoded as u16).to_le_bytes());
                Ok(())
            }
            other => Err(CoreError::Codec(format!("cannot encode {other} as Era"))),
        }
    }

    /// Coerce the seam's account-id inputs to raw key bytes: ss58 strings,
    /// 0x-hex strings, or raw 32 bytes.
    pub fn coerce_account_id(&self, value: &Value) -> Result<[u8; 32], CoreError> {
        match value {
            Value::Str(s) if s.starts_with("0x") => {
                let raw = hex::decode(s.trim_start_matches("0x"))
                    .map_err(|e| CoreError::Codec(format!("bad hex account id: {e}")))?;
                raw.as_slice()
                    .try_into()
                    .map_err(|_| CoreError::Codec("account id must be 32 bytes".into()))
            }
            Value::Str(s) => public_key_from_ss58(s),
            Value::Bytes(b) => b
                .as_slice()
                .try_into()
                .map_err(|_| CoreError::Codec("account id must be 32 bytes".into())),
            other => Err(CoreError::Codec(format!(
                "cannot interpret {other} as an account id"
            ))),
        }
    }

    fn encode_call_input(&self, value: &Value, out: &mut Vec<u8>) -> Result<(), CoreError> {
        match value {
            Value::Bytes(raw) => {
                out.extend_from_slice(raw);
                Ok(())
            }
            Value::Dict(_) => {
                let call_type = self
                    .extrinsic
                    .call_type
                    .ok_or_else(|| CoreError::Codec("runtime has no call type".into()))?;
                self.encode_id(call_type, value, out)
            }
            other => Err(CoreError::Codec(format!("cannot encode {other} as Call"))),
        }
    }

    /// Compose a call: `(pallet, function, params)` -> raw SCALE call bytes.
    ///
    /// Param values may embed pre-composed calls as `Value::Bytes` (Sudo,
    /// batches, proxies); they splice in verbatim.
    pub fn compose_call(
        &self,
        pallet: &str,
        function: &str,
        params: &Value,
    ) -> Result<Vec<u8>, CoreError> {
        let pallet_info = self
            .pallet(pallet)
            .ok_or_else(|| CoreError::Codec(format!("pallet {pallet:?} not found")))?;
        let calls_type = pallet_info
            .calls_type
            .ok_or_else(|| CoreError::Codec(format!("pallet {pallet:?} has no calls")))?;
        let TypeDef::Variant(variant) = &self.resolve(calls_type)?.type_def else {
            return Err(CoreError::Codec("call type is not an enum".into()));
        };
        let chosen = variant
            .variants
            .iter()
            .find(|v| v.name == function)
            .ok_or_else(|| CoreError::Codec(format!("call {pallet}.{function} not found")))?;
        let mut out = vec![pallet_info.index, chosen.index];
        match params {
            Value::Dict(entries) => {
                for field in &chosen.fields {
                    let name = field.name.as_deref().unwrap_or_default();
                    let item = entries
                        .iter()
                        .find(|(k, _)| matches!(k, Value::Str(s) if s == name))
                        .map(|(_, v)| v)
                        .ok_or_else(|| {
                            CoreError::Codec(format!(
                                "missing call param {name:?} for {pallet}.{function}"
                            ))
                        })?;
                    self.encode_id(field.ty.id, item, &mut out)?;
                }
            }
            Value::List(items) | Value::Tuple(items) => {
                if items.len() != chosen.fields.len() {
                    return Err(CoreError::Codec(format!(
                        "call {pallet}.{function} expects {} positional params, got {}",
                        chosen.fields.len(),
                        items.len()
                    )));
                }
                for (field, item) in chosen.fields.iter().zip(items.iter()) {
                    self.encode_id(field.ty.id, item, &mut out)?;
                }
            }
            other => {
                return Err(CoreError::Codec(format!(
                    "call params must be a dict or positional list, got {other}"
                )));
            }
        }
        Ok(out)
    }
}

fn calculated_period(period: u64) -> u64 {
    period.next_power_of_two().clamp(4, 1 << 16)
}

fn coerce_list(value: &Value) -> Result<&[Value], CoreError> {
    match value {
        Value::List(items) | Value::Tuple(items) => Ok(items),
        other => Err(CoreError::Codec(format!("expected a list, got {other}"))),
    }
}

fn coerce_uint(value: &Value) -> Result<u128, CoreError> {
    match value {
        Value::Int(i) => u128::try_from(*i)
            .map_err(|_| CoreError::Codec("negative value for unsigned type".into())),
        Value::Uint(u) => Ok(*u),
        Value::Bool(b) => Ok(u128::from(*b)),
        other => Err(CoreError::Codec(format!(
            "expected an integer, got {other}"
        ))),
    }
}

fn coerce_int(value: &Value) -> Result<i128, CoreError> {
    match value {
        Value::Int(i) => Ok(*i),
        Value::Uint(u) => i128::try_from(*u)
            .map_err(|_| CoreError::Codec("value too large for signed type".into())),
        other => Err(CoreError::Codec(format!(
            "expected an integer, got {other}"
        ))),
    }
}

/// Byte-carrier inputs: raw bytes, "0x..." hex, or plain strings (utf8) —
/// the inverse of the utf8-else-hex decode rendering. Lists of ints also
/// accepted (cyscale did).
fn coerce_bytes(value: &Value, expected_len: Option<usize>) -> Result<Vec<u8>, CoreError> {
    let bytes = match value {
        Value::Bytes(b) => b.clone(),
        Value::Str(s) if s.starts_with("0x") => hex::decode(s.trim_start_matches("0x"))
            .map_err(|e| CoreError::Codec(format!("bad hex: {e}")))?,
        Value::Str(s) => s.as_bytes().to_vec(),
        Value::List(items) | Value::Tuple(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                let v = coerce_uint(item)?;
                out.push(
                    u8::try_from(v)
                        .map_err(|_| CoreError::Codec("byte value out of range".into()))?,
                );
            }
            out
        }
        other => return Err(CoreError::Codec(format!("expected bytes, got {other}"))),
    };
    if let Some(len) = expected_len {
        if bytes.len() != len {
            return Err(CoreError::Codec(format!(
                "expected {len} bytes, got {}",
                bytes.len()
            )));
        }
    }
    Ok(bytes)
}

/// Enum inputs: bare `"Variant"` strings or single-key `{"Variant": payload}`
/// dicts. Returns the payload (unit payload as an empty tuple).
fn coerce_variant(value: &Value) -> Result<(String, Value), CoreError> {
    match value {
        Value::Str(name) => Ok((name.clone(), Value::Tuple(Vec::new()))),
        Value::Dict(entries) if entries.len() == 1 => match &entries[0] {
            (Value::Str(name), payload) => Ok((name.clone(), payload.clone())),
            _ => Err(CoreError::Codec("enum dict key must be a string".into())),
        },
        other => Err(CoreError::Codec(format!(
            "expected an enum variant, got {other}"
        ))),
    }
}

fn encode_primitive_spec(p: Primitive, value: &Value, out: &mut Vec<u8>) -> Result<(), CoreError> {
    use Primitive as P;
    let def = match p {
        P::Bool => TypeDefPrimitive::Bool,
        P::Char => TypeDefPrimitive::Char,
        P::Str => TypeDefPrimitive::Str,
        P::U8 => TypeDefPrimitive::U8,
        P::U16 => TypeDefPrimitive::U16,
        P::U32 => TypeDefPrimitive::U32,
        P::U64 => TypeDefPrimitive::U64,
        P::U128 => TypeDefPrimitive::U128,
        P::U256 => TypeDefPrimitive::U256,
        P::I8 => TypeDefPrimitive::I8,
        P::I16 => TypeDefPrimitive::I16,
        P::I32 => TypeDefPrimitive::I32,
        P::I64 => TypeDefPrimitive::I64,
        P::I128 => TypeDefPrimitive::I128,
        P::I256 => TypeDefPrimitive::I256,
    };
    encode_primitive(&def, value, out)
}

fn encode_primitive(
    p: &TypeDefPrimitive,
    value: &Value,
    out: &mut Vec<u8>,
) -> Result<(), CoreError> {
    use TypeDefPrimitive as P;
    match p {
        P::Bool => {
            let b = match value {
                Value::Bool(b) => *b,
                Value::Int(i) => *i != 0,
                other => return Err(CoreError::Codec(format!("expected bool, got {other}"))),
            };
            out.push(u8::from(b));
        }
        P::Char => {
            let Value::Str(s) = value else {
                return Err(CoreError::Codec("expected a 1-char string".into()));
            };
            let c = s
                .chars()
                .next()
                .ok_or_else(|| CoreError::Codec("expected a 1-char string".into()))?;
            out.extend_from_slice(&(c as u32).to_le_bytes());
        }
        P::Str => {
            let Value::Str(s) = value else {
                return Err(CoreError::Codec(format!("expected a string, got {value}")));
            };
            compact(s.len() as u128, out)?;
            out.extend_from_slice(s.as_bytes());
        }
        P::U8 => out.push(int_as::<u8>(value)?),
        P::U16 => out.extend_from_slice(&int_as::<u16>(value)?.to_le_bytes()),
        P::U32 => out.extend_from_slice(&int_as::<u32>(value)?.to_le_bytes()),
        P::U64 => out.extend_from_slice(&int_as::<u64>(value)?.to_le_bytes()),
        P::U128 => out.extend_from_slice(&coerce_uint(value)?.to_le_bytes()),
        P::U256 => match value {
            Value::U256(le) => out.extend_from_slice(le),
            _ => {
                let v = coerce_uint(value)?;
                let mut buf = [0u8; 32];
                buf[..16].copy_from_slice(&v.to_le_bytes());
                out.extend_from_slice(&buf);
            }
        },
        P::I8 => out.extend_from_slice(&sint_as::<i8>(value)?.to_le_bytes()),
        P::I16 => out.extend_from_slice(&sint_as::<i16>(value)?.to_le_bytes()),
        P::I32 => out.extend_from_slice(&sint_as::<i32>(value)?.to_le_bytes()),
        P::I64 => out.extend_from_slice(&sint_as::<i64>(value)?.to_le_bytes()),
        P::I128 => out.extend_from_slice(&coerce_int(value)?.to_le_bytes()),
        P::I256 => match value {
            Value::U256(le) => out.extend_from_slice(le),
            _ => {
                let v = coerce_int(value)?;
                let mut buf = if v < 0 { [0xffu8; 32] } else { [0u8; 32] };
                buf[..16].copy_from_slice(&v.to_le_bytes());
                out.extend_from_slice(&buf);
            }
        },
    }
    Ok(())
}

fn int_as<T: TryFrom<u128>>(value: &Value) -> Result<T, CoreError> {
    T::try_from(coerce_uint(value)?).map_err(|_| CoreError::Codec("integer out of range".into()))
}

fn sint_as<T: TryFrom<i128>>(value: &Value) -> Result<T, CoreError> {
    T::try_from(coerce_int(value)?).map_err(|_| CoreError::Codec("integer out of range".into()))
}
