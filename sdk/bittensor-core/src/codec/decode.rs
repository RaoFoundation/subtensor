//! SCALE decode into cyscale's value shapes (see `value.rs`).
//!
//! Every convention here is pinned by the shape corpus
//! (`sdk/python/tests/fixtures/shape_corpus`): AccountId32 renders as ss58,
//! `Vec<u8>` as utf8-else-hex strings, `[u8; N]` as 0x-hex, enums as
//! `{"Variant": payload}` (unit variants as bare strings), Options unwrap,
//! newtype composites unwrap, BTreeMaps become dicts with native keys, and
//! `Era` renders as `"00"` / `(period, phase)`.

// Client-side codec, not runtime code: slices are guarded by the cursor and
// explicit length checks; arithmetic operates on lengths and era math.
#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

use scale_info::{form::PortableForm, Field, TypeDef, TypeDefPrimitive, Variant};

use crate::codec::value::Value;
use crate::error::CoreError;
use crate::keys::ss58_from_public;
use crate::runtime::type_string::{Primitive, TypeSpec};
use crate::runtime::Runtime;

/// Recursion ceiling for the decoders. Metadata comes from the connected
/// node and is untrusted: a self-referential type recurses without consuming
/// bytes, and a stack overflow is an abort (not a catchable panic) — fatal on
/// a rayon worker with no Python frame above it. Real chain values nest a
/// couple dozen levels; 256 is far above any legitimate shape.
const MAX_DECODE_DEPTH: usize = 256;

/// Floor for the per-decode element budget, so small-but-legitimate inputs
/// still decode. A collection element that consumes zero input bytes (e.g.
/// `Vec<()>`) cannot be bounded by remaining input length, so the budget caps
/// the total number of collection elements materialized across one decode.
const MIN_ELEMENT_BUDGET: u64 = 1 << 20;

/// Element-budget headroom per input byte. Every byte-consuming element needs
/// at least one input byte, so a multiple of the input length is a generous
/// ceiling for legitimate values while still bounding zero-width blow-ups.
const ELEMENT_BUDGET_PER_BYTE: u64 = 256;

pub struct Cursor<'a> {
    pub data: &'a [u8],
    pub offset: usize,
    depth: usize,
    /// Remaining collection elements this decode may still materialize (the
    /// operation budget from the DoS-hardening review). Bounds `Vec<()>`-style
    /// inputs where a huge compact length drives an allocation/CPU blow-up
    /// without consuming input bytes.
    elements_remaining: u64,
    /// Reject non-canonical/lossy encodings (bad bools, invalid UTF-8, invalid
    /// Unicode scalars, non-minimal compacts) instead of silently coercing.
    pub strict: bool,
}

impl<'a> Cursor<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        let budget = (data.len() as u64)
            .saturating_mul(ELEMENT_BUDGET_PER_BYTE)
            .max(MIN_ELEMENT_BUDGET);
        Self {
            data,
            offset: 0,
            depth: 0,
            elements_remaining: budget,
            strict: false,
        }
    }

    fn descend(&mut self) -> Result<(), CoreError> {
        self.depth += 1;
        if self.depth > MAX_DECODE_DEPTH {
            return Err(CoreError::Codec(format!(
                "decode recursion exceeds {MAX_DECODE_DEPTH} levels (malformed or malicious type definition)"
            )));
        }
        Ok(())
    }

    fn ascend(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    /// Charge one collection element against the budget before decoding it.
    fn charge_element(&mut self) -> Result<(), CoreError> {
        self.elements_remaining = self.elements_remaining.checked_sub(1).ok_or_else(|| {
            CoreError::Codec(
                "decode exceeds its element budget (malformed or malicious collection length)"
                    .into(),
            )
        })?;
        Ok(())
    }

    pub fn take(&mut self, n: usize) -> Result<&'a [u8], CoreError> {
        let end = self.offset.checked_add(n).ok_or_else(overrun)?;
        let slice = self.data.get(self.offset..end).ok_or_else(overrun)?;
        self.offset = end;
        Ok(slice)
    }

    pub fn byte(&mut self) -> Result<u8, CoreError> {
        Ok(self.take(1)?[0])
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.offset)
    }
}

fn overrun() -> CoreError {
    CoreError::Codec("unexpected end of SCALE data".into())
}

/// Compact-decoded u128 (the four SCALE compact modes). In strict mode the
/// encoding must be canonical (minimal-width for its value), matching the
/// SCALE spec: a decoder that accepts non-minimal compacts admits multiple
/// wire encodings for the same integer.
pub fn compact_u128(cursor: &mut Cursor) -> Result<u128, CoreError> {
    let strict = cursor.strict;
    let non_canonical = || CoreError::Codec("non-canonical compact encoding".into());
    let first = cursor.byte()?;
    match first & 0b11 {
        0 => Ok(u128::from(first >> 2)),
        1 => {
            let second = cursor.byte()?;
            let value = u128::from(u16::from_le_bytes([first, second])) >> 2;
            if strict && value < 0b100_0000 {
                return Err(non_canonical());
            }
            Ok(value)
        }
        2 => {
            let rest = cursor.take(3)?;
            let word = u32::from_le_bytes([first, rest[0], rest[1], rest[2]]);
            let value = u128::from(word >> 2);
            if strict && value < 0b100_0000_0000_0000 {
                return Err(non_canonical());
            }
            Ok(value)
        }
        _ => {
            let len = usize::from(first >> 2) + 4;
            if len > 16 {
                return Err(CoreError::Codec("compact value wider than u128".into()));
            }
            let bytes = cursor.take(len)?;
            // The top byte must be non-zero, otherwise a narrower mode (or
            // fewer big-mode bytes) would encode the same value.
            if strict && bytes.last() == Some(&0) {
                return Err(non_canonical());
            }
            let mut buf = [0u8; 16];
            buf[..len].copy_from_slice(bytes);
            let value = u128::from_le_bytes(buf);
            if strict && value < 0b100_0000_0000_0000_0000_0000_0000_0000 {
                return Err(non_canonical());
            }
            Ok(value)
        }
    }
}

pub fn compact_len(cursor: &mut Cursor) -> Result<usize, CoreError> {
    usize::try_from(compact_u128(cursor)?)
        .map_err(|_| CoreError::Codec("length does not fit usize".into()))
}

impl Runtime {
    /// Decode `data` as the given type spec; errors on trailing bytes when
    /// `strict`.
    pub fn decode_spec(
        &self,
        spec: &TypeSpec,
        data: &[u8],
        strict: bool,
    ) -> Result<Value, CoreError> {
        let mut cursor = Cursor::new(data);
        cursor.strict = strict;
        // cyscale's batch_decode fast path rendered a *top-level* zero-field
        // struct as () while nested ones render as {}; the corpus pins both.
        if let TypeSpec::Id(id) = spec {
            if let Ok(ty) = self.resolve(*id) {
                if matches!(&ty.type_def, TypeDef::Composite(c) if c.fields.is_empty()) {
                    // A zero-field composite consumes no bytes; strict mode
                    // must still reject trailing data on this path.
                    if strict && !data.is_empty() {
                        return Err(CoreError::Codec(format!(
                            "{} undecoded bytes remain",
                            data.len()
                        )));
                    }
                    return Ok(Value::Tuple(Vec::new()));
                }
            }
        }
        let value = self.decode_value(spec, &mut cursor)?;
        if strict && cursor.remaining() != 0 {
            return Err(CoreError::Codec(format!(
                "{} undecoded bytes remain",
                cursor.remaining()
            )));
        }
        Ok(value)
    }

    pub fn decode_value(&self, spec: &TypeSpec, cursor: &mut Cursor) -> Result<Value, CoreError> {
        cursor.descend()?;
        let value = self.decode_value_inner(spec, cursor);
        cursor.ascend();
        value
    }

    fn decode_value_inner(&self, spec: &TypeSpec, cursor: &mut Cursor) -> Result<Value, CoreError> {
        match spec {
            TypeSpec::Id(id) => self.decode_id(*id, cursor),
            TypeSpec::Primitive(p) => self.decode_primitive_spec(*p, cursor),
            TypeSpec::Sequence(inner) => {
                let len = compact_len(cursor)?;
                if matches!(**inner, TypeSpec::Primitive(Primitive::U8)) {
                    return Ok(bytes_value(cursor.take(len)?));
                }
                let mut items = Vec::with_capacity(len.min(1024));
                for _ in 0..len {
                    cursor.charge_element()?;
                    items.push(self.decode_value(inner, cursor)?);
                }
                Ok(Value::List(items))
            }
            TypeSpec::Option(inner) => match cursor.byte()? {
                0 => Ok(Value::Null),
                1 => self.decode_value(inner, cursor),
                other => Err(CoreError::Codec(format!("bad Option byte {other:#x}"))),
            },
            TypeSpec::Array(inner, len) => {
                let len = *len as usize;
                // [u8; 0] renders as [] in cyscale; hex only for len > 0.
                if len > 0 && matches!(**inner, TypeSpec::Primitive(Primitive::U8)) {
                    return Ok(Value::hex(cursor.take(len)?));
                }
                let mut items = Vec::with_capacity(len.min(1024));
                for _ in 0..len {
                    cursor.charge_element()?;
                    items.push(self.decode_value(inner, cursor)?);
                }
                Ok(Value::List(items))
            }
            TypeSpec::Tuple(parts) => {
                let mut items = Vec::with_capacity(parts.len());
                for part in parts {
                    items.push(self.decode_value(part, cursor)?);
                }
                Ok(Value::Tuple(items))
            }
            TypeSpec::Compact(_) => Ok(uint_value(compact_u128(cursor)?)),
            TypeSpec::Bytes => {
                let len = compact_len(cursor)?;
                Ok(bytes_value(cursor.take(len)?))
            }
            TypeSpec::AccountId => {
                let raw: [u8; 32] = cursor
                    .take(32)?
                    .try_into()
                    .map_err(|_| CoreError::Codec("bad AccountId length".into()))?;
                Ok(Value::str(ss58_from_public(raw, self.ss58_format)))
            }
            TypeSpec::Era => self.decode_era(cursor),
            TypeSpec::Call => self.decode_call_value(cursor),
            TypeSpec::Extrinsic => Err(CoreError::Codec(
                "extrinsics are decoded by the transport layer".into(),
            )),
        }
    }

    fn decode_primitive_spec(&self, p: Primitive, cursor: &mut Cursor) -> Result<Value, CoreError> {
        let def = match p {
            Primitive::Bool => TypeDefPrimitive::Bool,
            Primitive::Char => TypeDefPrimitive::Char,
            Primitive::Str => TypeDefPrimitive::Str,
            Primitive::U8 => TypeDefPrimitive::U8,
            Primitive::U16 => TypeDefPrimitive::U16,
            Primitive::U32 => TypeDefPrimitive::U32,
            Primitive::U64 => TypeDefPrimitive::U64,
            Primitive::U128 => TypeDefPrimitive::U128,
            Primitive::U256 => TypeDefPrimitive::U256,
            Primitive::I8 => TypeDefPrimitive::I8,
            Primitive::I16 => TypeDefPrimitive::I16,
            Primitive::I32 => TypeDefPrimitive::I32,
            Primitive::I64 => TypeDefPrimitive::I64,
            Primitive::I128 => TypeDefPrimitive::I128,
            Primitive::I256 => TypeDefPrimitive::I256,
        };
        decode_primitive(&def, cursor)
    }

    /// Decode one registry type. The special cases (paths with pinned
    /// renderings) come before the structural rules.
    pub fn decode_id(&self, id: u32, cursor: &mut Cursor) -> Result<Value, CoreError> {
        cursor.descend()?;
        let value = self.decode_id_inner(id, cursor);
        cursor.ascend();
        value
    }

    fn decode_id_inner(&self, id: u32, cursor: &mut Cursor) -> Result<Value, CoreError> {
        if self.outer_event_type == Some(id) {
            return Ok(self.decode_outer_event(cursor)?.into_value());
        }
        if self.extrinsic.call_type == Some(id) {
            return self.decode_call_value(cursor);
        }
        let ty = self.resolve(id)?;
        let segments = &ty.path.segments;
        let last = segments.last().map(String::as_str);
        if last == Some("EventRecord")
            && segments.first().map(String::as_str) == Some("frame_system")
        {
            return self.decode_event_record(ty, cursor);
        }

        match last {
            Some("AccountId32") if is_path(segments, &["sp_core", "crypto", "AccountId32"]) => {
                let raw: [u8; 32] = cursor
                    .take(32)?
                    .try_into()
                    .map_err(|_| CoreError::Codec("bad AccountId32".into()))?;
                return Ok(Value::str(ss58_from_public(raw, self.ss58_format)));
            }
            Some("Era") if segments.first().map(String::as_str) == Some("sp_runtime") => {
                return self.decode_era(cursor);
            }
            Some("MultiAddress") => {
                return self.decode_multiaddress(ty, cursor);
            }
            // cyscale registered primitive_types::U256 as a 32-byte LE int.
            Some("U256") if is_path(segments, &["primitive_types", "U256"]) => {
                return Ok(Value::U256(fixed(cursor.take(32)?)?));
            }
            Some("BTreeMap") => {
                return self.decode_btreemap(ty, cursor);
            }
            _ => {}
        }

        match &ty.type_def {
            TypeDef::Primitive(p) => decode_primitive(p, cursor),
            TypeDef::Compact(c) => {
                // Compact of a newtype (e.g. Compact<IndexU32>) unwraps like
                // the plain compact int cyscale produced. The metadata's
                // declared width is not a reliable bound on this chain (real
                // recorded compacts exceed their nominal `type_param`), so it
                // is intentionally not enforced.
                let _ = c;
                Ok(uint_value(compact_u128(cursor)?))
            }
            TypeDef::Composite(composite) => self.decode_fields_owner(&composite.fields, cursor),
            TypeDef::Variant(variant) => {
                let is_option = last == Some("Option") && segments.len() == 1;
                let index = cursor.byte()?;
                let chosen = variant
                    .variants
                    .iter()
                    .find(|v| v.index == index)
                    .ok_or_else(|| {
                        CoreError::Codec(format!("no variant with index {index} in type {id}"))
                    })?;
                if is_option {
                    return match chosen.name.as_str() {
                        "None" => Ok(Value::Null),
                        _ => self.decode_fields_payload(&chosen.fields, cursor),
                    };
                }
                self.decode_variant(chosen, cursor)
            }
            TypeDef::Sequence(s) => {
                let len = compact_len(cursor)?;
                if self.is_u8(s.type_param.id) {
                    return Ok(bytes_value(cursor.take(len)?));
                }
                let mut items = Vec::with_capacity(len.min(4096));
                for _ in 0..len {
                    cursor.charge_element()?;
                    items.push(self.decode_id(s.type_param.id, cursor)?);
                }
                Ok(Value::List(items))
            }
            TypeDef::Array(a) => {
                let len = a.len as usize;
                if len > 0 && self.is_u8(a.type_param.id) {
                    return Ok(Value::hex(cursor.take(len)?));
                }
                let mut items = Vec::with_capacity(len.min(4096));
                for _ in 0..len {
                    cursor.charge_element()?;
                    items.push(self.decode_id(a.type_param.id, cursor)?);
                }
                Ok(Value::List(items))
            }
            TypeDef::Tuple(t) => {
                let mut items = Vec::with_capacity(t.fields.len());
                for field in &t.fields {
                    items.push(self.decode_id(field.id, cursor)?);
                }
                Ok(Value::Tuple(items))
            }
            TypeDef::BitSequence(_) => Err(CoreError::Codec(
                "bit sequences are not used by this chain".into(),
            )),
        }
    }

    fn is_u8(&self, id: u32) -> bool {
        matches!(
            self.types.resolve(id).map(|t| &t.type_def),
            Some(TypeDef::Primitive(TypeDefPrimitive::U8))
        )
    }

    /// Composite-body shapes: named fields -> dict, one unnamed -> unwrap,
    /// several unnamed -> tuple, none -> empty dict (cyscale's zero-sized
    /// struct rendering).
    fn decode_fields_owner(
        &self,
        fields: &[Field<PortableForm>],
        cursor: &mut Cursor,
    ) -> Result<Value, CoreError> {
        if fields.is_empty() {
            return Ok(Value::Dict(Vec::new()));
        }
        let named = fields.iter().all(|f| f.name.is_some());
        if named {
            let mut entries = Vec::with_capacity(fields.len());
            for field in fields {
                let name = field.name.clone().unwrap_or_default();
                entries.push((Value::Str(name), self.decode_id(field.ty.id, cursor)?));
            }
            return Ok(Value::Dict(entries));
        }
        if fields.len() == 1 {
            return self.decode_id(fields[0].ty.id, cursor);
        }
        let mut items = Vec::with_capacity(fields.len());
        for field in fields {
            items.push(self.decode_id(field.ty.id, cursor)?);
        }
        Ok(Value::Tuple(items))
    }

    /// A chosen enum variant's payload (same field rules as composites).
    fn decode_fields_payload(
        &self,
        fields: &[Field<PortableForm>],
        cursor: &mut Cursor,
    ) -> Result<Value, CoreError> {
        self.decode_fields_owner(fields, cursor)
    }

    fn decode_variant(
        &self,
        chosen: &Variant<PortableForm>,
        cursor: &mut Cursor,
    ) -> Result<Value, CoreError> {
        if chosen.fields.is_empty() {
            return Ok(Value::Str(chosen.name.clone()));
        }
        let payload = self.decode_fields_payload(&chosen.fields, cursor)?;
        Ok(Value::Dict(vec![(
            Value::Str(chosen.name.clone()),
            payload,
        )]))
    }

    /// MultiAddress: `Id` renders as the bare ss58 string and `Index` as the
    /// bare integer (cyscale's GenericMultiAddress); other variants keep the
    /// standard enum shape.
    fn decode_multiaddress(
        &self,
        ty: &scale_info::Type<PortableForm>,
        cursor: &mut Cursor,
    ) -> Result<Value, CoreError> {
        let TypeDef::Variant(variant) = &ty.type_def else {
            return Err(CoreError::Codec("MultiAddress is not an enum".into()));
        };
        let index = cursor.byte()?;
        let chosen = variant
            .variants
            .iter()
            .find(|v| v.index == index)
            .ok_or_else(|| CoreError::Codec(format!("bad MultiAddress variant {index}")))?;
        match chosen.name.as_str() {
            "Id" | "Index" => self.decode_fields_payload(&chosen.fields, cursor),
            _ => self.decode_variant(chosen, cursor),
        }
    }

    /// BTreeMap<K, V>: a length-prefixed list of pairs rendered as a dict
    /// with native keys.
    fn decode_btreemap(
        &self,
        ty: &scale_info::Type<PortableForm>,
        cursor: &mut Cursor,
    ) -> Result<Value, CoreError> {
        // Portable form: composite with a single Vec<(K, V)> field.
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
        let len = compact_len(cursor)?;
        let mut entries = Vec::with_capacity(len.min(4096));
        for _ in 0..len {
            cursor.charge_element()?;
            let key = self.decode_id(key_ty, cursor)?;
            let value = self.decode_id(value_ty, cursor)?;
            entries.push((key, value));
        }
        Ok(Value::Dict(entries))
    }

    /// The outer RuntimeEvent in cyscale's GenericEvent shape:
    /// `{"event_index": "XXYY", "module_id", "event_id", "attributes"}`
    /// (unit payloads render as null).
    fn decode_outer_event(&self, cursor: &mut Cursor) -> Result<DecodedEvent, CoreError> {
        let outer_id = self
            .outer_event_type
            .ok_or_else(|| CoreError::Codec("runtime has no outer event enum".into()))?;
        let TypeDef::Variant(outer) = &self.resolve(outer_id)?.type_def else {
            return Err(CoreError::Codec("outer event type is not an enum".into()));
        };
        let pallet_index = cursor.byte()?;
        let pallet_variant = outer
            .variants
            .iter()
            .find(|v| v.index == pallet_index)
            .ok_or_else(|| CoreError::Codec(format!("no event pallet at index {pallet_index}")))?;
        let inner_id = pallet_variant
            .fields
            .first()
            .map(|f| f.ty.id)
            .ok_or_else(|| CoreError::Codec("outer event variant has no payload".into()))?;
        let TypeDef::Variant(inner) = &self.resolve(inner_id)?.type_def else {
            return Err(CoreError::Codec("pallet event type is not an enum".into()));
        };
        let event_index = cursor.byte()?;
        let event = inner
            .variants
            .iter()
            .find(|v| v.index == event_index)
            .ok_or_else(|| {
                CoreError::Codec(format!(
                    "no event at index {event_index} in pallet {}",
                    pallet_variant.name
                ))
            })?;
        let attributes = if event.fields.is_empty() {
            Value::Null
        } else {
            self.decode_fields_payload(&event.fields, cursor)?
        };
        Ok(DecodedEvent {
            pallet_index,
            event_index,
            module_id: pallet_variant.name.clone(),
            event_id: event.name.clone(),
            attributes,
        })
    }

    /// `frame_system::EventRecord` in cyscale's flattened shape: the phase
    /// split into name + extrinsic_idx, the event dict, and the event's
    /// fields copied to the top level.
    fn decode_event_record(
        &self,
        ty: &scale_info::Type<PortableForm>,
        cursor: &mut Cursor,
    ) -> Result<Value, CoreError> {
        let TypeDef::Composite(composite) = &ty.type_def else {
            return Err(CoreError::Codec("EventRecord is not a composite".into()));
        };
        let field_id = |name: &str| -> Result<u32, CoreError> {
            composite
                .fields
                .iter()
                .find(|f| f.name.as_deref() == Some(name))
                .map(|f| f.ty.id)
                .ok_or_else(|| CoreError::Codec(format!("EventRecord has no {name} field")))
        };
        // phase: ApplyExtrinsic(u32) carries the index; the other variants
        // render as their bare names.
        let phase_id = field_id("phase")?;
        let TypeDef::Variant(phase_def) = &self.resolve(phase_id)?.type_def else {
            return Err(CoreError::Codec("Phase is not an enum".into()));
        };
        let phase_index = cursor.byte()?;
        let phase = phase_def
            .variants
            .iter()
            .find(|v| v.index == phase_index)
            .ok_or_else(|| CoreError::Codec(format!("bad Phase variant {phase_index}")))?;
        let extrinsic_idx = if phase.fields.is_empty() {
            Value::Null
        } else {
            self.decode_fields_payload(&phase.fields, cursor)?
        };

        let event = self.decode_outer_event(cursor)?;

        let topics_id = field_id("topics")?;
        let topics = self.decode_id(topics_id, cursor)?;

        Ok(Value::record(vec![
            ("phase".into(), Value::Str(phase.name.clone())),
            ("extrinsic_idx".into(), extrinsic_idx),
            ("event".into(), event.clone().into_value()),
            (
                "event_index".into(),
                Value::Int(i128::from(event.pallet_index)),
            ),
            ("module_id".into(), Value::Str(event.module_id.clone())),
            ("event_id".into(), Value::Str(event.event_id.clone())),
            ("attributes".into(), event.attributes.clone()),
            ("topics".into(), topics),
        ]))
    }

    /// Era: `"00"` for immortal, `(period, phase)` for mortal — cyscale's
    /// GenericEra values.
    fn decode_era(&self, cursor: &mut Cursor) -> Result<Value, CoreError> {
        let first = cursor.byte()?;
        if first == 0 {
            return Ok(Value::str("00"));
        }
        let second = cursor.byte()?;
        let encoded = u64::from(u16::from_le_bytes([first, second]));
        let period = 2u64 << (encoded % 16);
        let quantize_factor = (period >> 12).max(1);
        let phase = (encoded >> 4) * quantize_factor;
        Ok(Value::Tuple(vec![
            Value::Int(i128::from(period)),
            Value::Int(i128::from(phase)),
        ]))
    }

    /// The runtime's outer Call in cyscale's metadata-aware shape:
    /// `{call_index, call_function, call_module, call_args: [{name, type,
    /// value}], call_hash}`.
    pub fn decode_call_value(&self, cursor: &mut Cursor) -> Result<Value, CoreError> {
        let start = cursor.offset;
        let pallet_index = cursor.byte()?;
        let call_index = cursor.byte()?;
        let pallet = self
            .pallet_at(pallet_index)
            .ok_or_else(|| CoreError::Codec(format!("no pallet at index {pallet_index}")))?;
        let calls_type = pallet
            .calls_type
            .ok_or_else(|| CoreError::Codec(format!("pallet {} has no calls", pallet.name)))?;
        let TypeDef::Variant(variant) = &self.resolve(calls_type)?.type_def else {
            return Err(CoreError::Codec("call type is not an enum".into()));
        };
        let function = variant
            .variants
            .iter()
            .find(|v| v.index == call_index)
            .ok_or_else(|| {
                CoreError::Codec(format!(
                    "no call at index {call_index} in pallet {}",
                    pallet.name
                ))
            })?;
        let mut args = Vec::with_capacity(function.fields.len());
        for field in &function.fields {
            let value = self.decode_id(field.ty.id, cursor)?;
            args.push(Value::record(vec![
                (
                    "name".into(),
                    Value::Str(field.name.clone().unwrap_or_default()),
                ),
                (
                    "type".into(),
                    Value::Str(convert_type_string(
                        field.type_name.as_deref().unwrap_or_default(),
                    )),
                ),
                ("value".into(), value),
            ]));
        }
        let call_bytes = &cursor.data[start..cursor.offset];
        let call_hash = sp_core::hashing::blake2_256(call_bytes);
        Ok(Value::record(vec![
            (
                "call_index".into(),
                Value::str(format!("0x{:02x}{:02x}", pallet_index, call_index)),
            ),
            ("call_function".into(), Value::Str(function.name.clone())),
            ("call_module".into(), Value::Str(pallet.name.clone())),
            ("call_args".into(), Value::List(args)),
            ("call_hash".into(), Value::hex(&call_hash)),
        ]))
    }
}

/// One decoded outer event, before shaping.
#[derive(Clone)]
struct DecodedEvent {
    pallet_index: u8,
    event_index: u8,
    module_id: String,
    event_id: String,
    attributes: Value,
}

impl DecodedEvent {
    fn into_value(self) -> Value {
        Value::record(vec![
            (
                "event_index".into(),
                Value::str(format!("{:02x}{:02x}", self.pallet_index, self.event_index)),
            ),
            ("module_id".into(), Value::Str(self.module_id)),
            ("event_id".into(), Value::Str(self.event_id)),
            ("attributes".into(), self.attributes),
        ])
    }
}

/// cyscale's `convert_type_string`: normalize a metadata `typeName` the way
/// the old codec rendered call-arg types (`T::Balance` -> `Balance`,
/// `Box<<T as Config>::RuntimeCall>` -> `RuntimeCall`, `Vec<u8>` -> `Bytes`).
pub fn convert_type_string(name: &str) -> String {
    fn remove_ci(haystack: &mut String, needle: &str) {
        let lower_needle = needle.to_lowercase();
        loop {
            let lower = haystack.to_lowercase();
            let Some(pos) = lower.find(&lower_needle) else {
                break;
            };
            haystack.replace_range(pos..pos.saturating_add(needle.len()), "");
        }
    }

    let mut name = name.replace("T::", "");
    if name.to_lowercase().starts_with("t::") {
        name.replace_range(..3, "");
    }
    remove_ci(&mut name, "<T>");
    remove_ci(&mut name, "<T as Trait>::");
    remove_ci(&mut name, "<T as Trait<I>>::");
    remove_ci(&mut name, "<T as Config>::");
    remove_ci(&mut name, "<T as Config<I>>::");
    name = name.replace('\n', "");
    for prefix in [
        "grandpa::",
        "session::",
        "slashing::",
        "limits::",
        "beefy_primitives::",
        "xcm::opaque::",
    ] {
        if let Some(stripped) = name.strip_prefix(prefix) {
            name = stripped.to_string();
            break;
        }
    }
    // VecDeque<...> renders as Vec<...> (case-insensitive replace).
    loop {
        let lower = name.to_lowercase();
        let Some(pos) = lower.find("vecdeque<") else {
            break;
        };
        name.replace_range(pos..pos.saturating_add("VecDeque<".len()), "Vec<");
    }
    let lower = name.to_lowercase();
    if lower.starts_with("box<") && lower.ends_with('>') && name.len() > 5 {
        name = name[4..name.len().saturating_sub(1)].to_string();
    }

    match name.to_lowercase().as_str() {
        _ if name == "()" => "Null".into(),
        "vec<u8>" | "&[u8]" | "& 'static[u8]" => "Bytes".into(),
        "<lookup as staticlookup>::source" => "LookupSource".into(),
        "<balance as hascompact>::type" => "Compact<Balance>".into(),
        "<blocknumber as hascompact>::type" => "Compact<BlockNumber>".into(),
        "<moment as hascompact>::type" => "Compact<Moment>".into(),
        "<inherentofflinereport as inherentofflinereport>::inherent" => {
            "InherentOfflineReport".into()
        }
        _ => name,
    }
}

fn is_path(segments: &[String], expected: &[&str]) -> bool {
    segments.len() == expected.len() && segments.iter().zip(expected).all(|(a, b)| a == b)
}

fn uint_value(v: u128) -> Value {
    match i128::try_from(v) {
        Ok(i) => Value::Int(i),
        Err(_) => Value::Uint(v),
    }
}

fn bytes_value(data: &[u8]) -> Value {
    match core::str::from_utf8(data) {
        Ok(s) => Value::str(s),
        Err(_) => Value::hex(data),
    }
}

fn decode_primitive(p: &TypeDefPrimitive, cursor: &mut Cursor) -> Result<Value, CoreError> {
    use TypeDefPrimitive as P;
    let strict = cursor.strict;
    Ok(match p {
        P::Bool => {
            let byte = cursor.byte()?;
            if strict && byte > 1 {
                return Err(CoreError::Codec(format!(
                    "non-canonical bool byte {byte:#x}"
                )));
            }
            Value::Bool(byte != 0)
        }
        P::Char => {
            let raw = cursor.take(4)?;
            let code = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
            match char::from_u32(code) {
                Some(c) => Value::Str(c.to_string()),
                None if strict => {
                    return Err(CoreError::Codec(format!(
                        "invalid Unicode scalar value {code:#x}"
                    )))
                }
                None => Value::Str('\u{fffd}'.to_string()),
            }
        }
        P::Str => {
            let len = compact_len(cursor)?;
            let raw = cursor.take(len)?;
            if strict {
                let s = core::str::from_utf8(raw)
                    .map_err(|e| CoreError::Codec(format!("invalid UTF-8 string: {e}")))?;
                Value::Str(s.to_owned())
            } else {
                Value::Str(String::from_utf8_lossy(raw).into_owned())
            }
        }
        P::U8 => Value::Int(i128::from(cursor.byte()?)),
        P::U16 => Value::Int(i128::from(u16::from_le_bytes(fixed(cursor.take(2)?)?))),
        P::U32 => Value::Int(i128::from(u32::from_le_bytes(fixed(cursor.take(4)?)?))),
        P::U64 => Value::Int(i128::from(u64::from_le_bytes(fixed(cursor.take(8)?)?))),
        P::U128 => uint_value(u128::from_le_bytes(fixed(cursor.take(16)?)?)),
        P::U256 => Value::U256(fixed(cursor.take(32)?)?),
        P::I8 => Value::Int(i128::from(cursor.byte()? as i8)),
        P::I16 => Value::Int(i128::from(i16::from_le_bytes(fixed(cursor.take(2)?)?))),
        P::I32 => Value::Int(i128::from(i32::from_le_bytes(fixed(cursor.take(4)?)?))),
        P::I64 => Value::Int(i128::from(i64::from_le_bytes(fixed(cursor.take(8)?)?))),
        P::I128 => Value::Int(i128::from_le_bytes(fixed(cursor.take(16)?)?)),
        P::I256 => {
            // Rendered like U256 with sign interpretation left to the corpus
            // (the chain does not use i256; kept for registry completeness).
            Value::U256(fixed(cursor.take(32)?)?)
        }
    })
}

fn fixed<const N: usize>(slice: &[u8]) -> Result<[u8; N], CoreError> {
    slice
        .try_into()
        .map_err(|_| CoreError::Codec("bad fixed-width slice".into()))
}
