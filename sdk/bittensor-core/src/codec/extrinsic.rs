//! Signature payloads, signed-extrinsic assembly and decode, era birth, and
//! multisig account derivation — the transaction-shaped primitives on top of
//! the codec, all pinned by the golden vectors.

// Client-side codec, not runtime code.
#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

use scale_info::{form::PortableForm, TypeDef, Variant};
use sp_core::hashing::blake2_256;

use crate::codec::decode::{compact_u128, Cursor};
use crate::codec::encode::compact;
use crate::codec::value::Value;
use crate::error::CoreError;
use crate::keys::ss58_from_public;
use crate::runtime::Runtime;

/// Everything one signature payload / signed extrinsic needs beyond the call.
pub struct TxParams {
    /// `"00"` (immortal) or `{"period": N, "phase": P}` / `{"period": N,
    /// "current": M}` — the shapes the SDK has always fed the codec.
    pub era: Value,
    pub nonce: u64,
    pub tip: u128,
    pub tip_asset_id: Option<u128>,
    pub genesis_hash: [u8; 32],
    pub era_block_hash: [u8; 32],
    /// RFC-0078 metadata digest; flips `CheckMetadataHash` to Enabled.
    pub metadata_hash: Option<[u8; 32]>,
}

/// The signature payload is ``call ++ extra ++ additional``: each signed
/// extension the runtime declares contributes its "extra" bytes (signed
/// alongside the call, `ty`) and then its "additional" bytes (implied data
/// both sides must agree on, `additional_signed`). This table IS the payload
/// wire format — order matters and is pinned by the golden signing-payload
/// vectors (it mirrors the Python codec's `_PAYLOAD_FIELDS`).
const PAYLOAD_FIELDS: &[(&str, &str, Slot)] = &[
    ("era", "CheckMortality", Slot::Extrinsic),
    ("era", "CheckEra", Slot::Extrinsic),
    ("nonce", "CheckNonce", Slot::Extrinsic),
    ("tip", "ChargeTransactionPayment", Slot::Extrinsic),
    ("asset_id", "ChargeAssetTxPayment", Slot::Extrinsic),
    ("mode", "CheckMetadataHash", Slot::Extrinsic),
    ("spec_version", "CheckSpecVersion", Slot::AdditionalSigned),
    (
        "transaction_version",
        "CheckTxVersion",
        Slot::AdditionalSigned,
    ),
    ("genesis_hash", "CheckGenesis", Slot::AdditionalSigned),
    ("block_hash", "CheckMortality", Slot::AdditionalSigned),
    ("block_hash", "CheckEra", Slot::AdditionalSigned),
    ("metadata_hash", "CheckMetadataHash", Slot::AdditionalSigned),
];

#[derive(Debug, PartialEq, Clone, Copy)]
enum Slot {
    Extrinsic,
    AdditionalSigned,
}

impl Runtime {
    /// The fixed byte length a signature variant carries, when it wraps a
    /// single `[u8; N]` (the MultiSignature shape). `None` for variants whose
    /// payload is not a fixed byte array, where a length check does not apply.
    fn variant_fixed_byte_len(&self, variant: &Variant<PortableForm>) -> Option<usize> {
        let [field] = variant.fields.as_slice() else {
            return None;
        };
        let TypeDef::Array(array) = &self.resolve(field.ty.id).ok()?.type_def else {
            return None;
        };
        matches!(
            self.resolve(array.type_param.id).ok()?.type_def,
            TypeDef::Primitive(scale_info::TypeDefPrimitive::U8)
        )
        .then_some(array.len as usize)
    }

    /// Encode the payload fields whose type comes from one extension slot.
    ///
    /// Iterates the runtime's declared signed extensions in metadata order
    /// (the authoritative wire order) rather than a private table, so a
    /// reordered or newly added extension cannot silently shift the payload.
    /// `PAYLOAD_FIELDS` supplies only the semantic value for each known
    /// extension; an extension with no known encoder must be zero-sized in
    /// this slot, otherwise we fail closed.
    fn encode_payload_section(&self, slot: Slot, params: &TxParams) -> Result<Vec<u8>, CoreError> {
        let mut out = Vec::new();
        for ext in &self.extrinsic.signed_extensions {
            let ty = match slot {
                Slot::Extrinsic => ext.ty,
                Slot::AdditionalSigned => ext.additional_signed,
            };
            let field = PAYLOAD_FIELDS
                .iter()
                .find(|(_, identifier, field_slot)| {
                    *identifier == ext.identifier && *field_slot == slot
                })
                .map(|(field, _, _)| *field);
            match field {
                Some(field) => {
                    let value = self.payload_field_value(field, ty, params)?;
                    self.encode_id(ty, &value, &mut out)?;
                }
                None => {
                    if !self.type_is_zero_sized(ty)? {
                        return Err(CoreError::Codec(format!(
                            "signed extension {} contributes non-zero {slot:?} data with no known encoder",
                            ext.identifier
                        )));
                    }
                }
            }
        }
        Ok(out)
    }

    /// Whether a type encodes to zero bytes (empty composite/tuple, or a
    /// zero-length array, transitively). Used to confirm an unknown signed
    /// extension genuinely contributes nothing to the payload.
    fn type_is_zero_sized(&self, id: u32) -> Result<bool, CoreError> {
        self.type_is_zero_sized_inner(id, 0)
    }

    fn type_is_zero_sized_inner(&self, id: u32, depth: usize) -> Result<bool, CoreError> {
        // Fail closed on pathological nesting: treat as non-zero-sized.
        if depth > 32 {
            return Ok(false);
        }
        Ok(match &self.resolve(id)?.type_def {
            TypeDef::Composite(c) => {
                let mut zero_sized = true;
                for f in &c.fields {
                    if !self.type_is_zero_sized_inner(f.ty.id, depth + 1)? {
                        zero_sized = false;
                        break;
                    }
                }
                zero_sized
            }
            TypeDef::Tuple(t) => {
                let mut zero_sized = true;
                for f in &t.fields {
                    if !self.type_is_zero_sized_inner(f.id, depth + 1)? {
                        zero_sized = false;
                        break;
                    }
                }
                zero_sized
            }
            TypeDef::Array(a) => {
                a.len == 0 || self.type_is_zero_sized_inner(a.type_param.id, depth + 1)?
            }
            _ => false,
        })
    }

    /// The value for one payload field, shaped for its metadata type.
    fn payload_field_value(
        &self,
        field: &str,
        ty: u32,
        params: &TxParams,
    ) -> Result<Value, CoreError> {
        Ok(match field {
            "era" => params.era.clone(),
            "nonce" => Value::Uint(u128::from(params.nonce)),
            "tip" => Value::Uint(params.tip),
            "asset_id" => Value::record(vec![
                ("tip".into(), Value::Uint(params.tip)),
                (
                    "asset_id".into(),
                    match params.tip_asset_id {
                        Some(id) => Value::Uint(id),
                        None => Value::Null,
                    },
                ),
            ]),
            "mode" => {
                let mode = if params.metadata_hash.is_some() {
                    "Enabled"
                } else {
                    "Disabled"
                };
                // The extension type is either the Mode enum itself or the
                // CheckMetadataHash struct wrapping it in a named field.
                match &self.resolve(ty)?.type_def {
                    TypeDef::Composite(c) => {
                        let name = c
                            .fields
                            .first()
                            .and_then(|f| f.name.clone())
                            .unwrap_or_else(|| "mode".into());
                        Value::Dict(vec![(Value::Str(name), Value::str(mode))])
                    }
                    _ => Value::str(mode),
                }
            }
            "spec_version" => Value::Uint(u128::from(self.spec_version)),
            "transaction_version" => Value::Uint(u128::from(self.transaction_version)),
            "genesis_hash" => Value::Bytes(params.genesis_hash.to_vec()),
            "block_hash" => Value::Bytes(params.era_block_hash.to_vec()),
            "metadata_hash" => match params.metadata_hash {
                Some(hash) => Value::Bytes(hash.to_vec()),
                None => Value::Null,
            },
            other => return Err(CoreError::Codec(format!("unknown payload field {other:?}"))),
        })
    }

    /// The signature payload split at its wire seams:
    /// `(included_in_extrinsic, included_in_signed_data)`. Prepending the raw
    /// call bytes gives the exact unhashed payload; hardware signers that
    /// prove the runtime on-device need the parts separately.
    pub fn signature_payload_parts(
        &self,
        params: &TxParams,
    ) -> Result<(Vec<u8>, Vec<u8>), CoreError> {
        if params.metadata_hash.is_some()
            && !self
                .extrinsic
                .signed_extensions
                .iter()
                .any(|e| e.identifier == "CheckMetadataHash")
        {
            return Err(CoreError::Codec(
                "this runtime does not declare CheckMetadataHash".into(),
            ));
        }
        Ok((
            self.encode_payload_section(Slot::Extrinsic, params)?,
            self.encode_payload_section(Slot::AdditionalSigned, params)?,
        ))
    }

    /// The exact bytes a signer signs for the given raw call. Payloads longer
    /// than 256 bytes are blake2b-256 hashed, per the Substrate convention.
    pub fn signature_payload(
        &self,
        call_data: &[u8],
        params: &TxParams,
    ) -> Result<Vec<u8>, CoreError> {
        let (extra, additional) = self.signature_payload_parts(params)?;
        let mut data = call_data.to_vec();
        data.extend_from_slice(&extra);
        data.extend_from_slice(&additional);
        if data.len() > 256 {
            return Ok(blake2_256(&data).to_vec());
        }
        Ok(data)
    }

    /// Assemble the full signed extrinsic; returns `(bytes, hash)`.
    ///
    /// The metadata-hash *mode* must match what the payload was signed with
    /// (the digest itself is implied data and never travels in the
    /// extrinsic — set `params.metadata_hash` accordingly).
    pub fn encode_signed_extrinsic(
        &self,
        call_data: &[u8],
        public_key: [u8; 32],
        signature: &[u8],
        signature_version: u8,
        params: &TxParams,
    ) -> Result<(Vec<u8>, [u8; 32]), CoreError> {
        if self.extrinsic.version != 4 {
            return Err(CoreError::Codec(format!(
                "extrinsic version {} not supported",
                self.extrinsic.version
            )));
        }
        let mut body = vec![0x80 | self.extrinsic.version];
        let address_type = self
            .extrinsic
            .address_type
            .ok_or_else(|| CoreError::Codec("runtime metadata has no address type".into()))?;
        self.encode_id(address_type, &Value::Bytes(public_key.to_vec()), &mut body)?;
        // Multi-crypto chains wrap the signature in an enum carrying the
        // scheme (the MultiSignature variant byte IS the signature version).
        // Validate the variant exists and that the raw signature matches the
        // variant's fixed width, so a bad version/length fails closed here
        // rather than producing a malformed extrinsic the chain will reject.
        if let Some(signature_type) = self.extrinsic.signature_type {
            if let TypeDef::Variant(variant) = &self.resolve(signature_type)?.type_def {
                let chosen = variant
                    .variants
                    .iter()
                    .find(|v| v.index == signature_version)
                    .ok_or_else(|| {
                        CoreError::Codec(format!(
                            "no signature variant with index {signature_version} in the runtime's signature enum"
                        ))
                    })?;
                if let Some(expected) = self.variant_fixed_byte_len(chosen) {
                    if signature.len() != expected {
                        return Err(CoreError::Codec(format!(
                            "signature is {} bytes but variant {} expects {expected}",
                            signature.len(),
                            chosen.name
                        )));
                    }
                }
                body.push(signature_version);
            }
        }
        body.extend_from_slice(signature);
        body.extend_from_slice(&self.encode_payload_section(Slot::Extrinsic, params)?);
        body.extend_from_slice(call_data);

        let mut out = Vec::with_capacity(body.len().saturating_add(4));
        compact(body.len() as u128, &mut out)?;
        out.extend_from_slice(&body);
        Ok((out.clone(), blake2_256(&out)))
    }

    /// Decode one raw extrinsic into cyscale's plain value dict:
    /// `{extrinsic_hash, extrinsic_length, [address, signature, era, nonce,
    /// tip, mode,] call}`.
    pub fn decode_extrinsic(&self, data: &[u8], strict: bool) -> Result<Value, CoreError> {
        let hash = blake2_256(data);
        let mut cursor = Cursor::new(data);
        cursor.strict = strict;
        let length = compact_u128(&mut cursor)?;
        // The compact prefix frames the extrinsic body exactly: decode against
        // a cursor bounded to those bytes and require it to be fully consumed,
        // so a wrong framing length can never decode as if it were correct.
        let body_len = usize::try_from(length)
            .map_err(|_| CoreError::Codec("extrinsic length does not fit usize".into()))?;
        let body = cursor.take(body_len)?;
        let mut body_cursor = Cursor::new(body);
        body_cursor.strict = strict;

        let version_byte = body_cursor.byte()?;
        let signed = version_byte & 0x80 != 0;
        let version = version_byte & 0x7f;
        // Bare (unsigned) extrinsics are just a call regardless of format
        // version (v5 inherents included); only the signed v4 layout is known.
        if signed && version != 4 {
            return Err(CoreError::Codec(format!(
                "signed extrinsic version {version} not supported"
            )));
        }

        let mut fields: Vec<(String, Value)> = vec![
            ("extrinsic_hash".into(), Value::hex(&hash)),
            (
                "extrinsic_length".into(),
                Value::Int(i128::try_from(length).unwrap_or(0)),
            ),
        ];
        if signed {
            let address_type = self
                .extrinsic
                .address_type
                .ok_or_else(|| CoreError::Codec("runtime metadata has no address type".into()))?;
            fields.push((
                "address".into(),
                self.decode_id(address_type, &mut body_cursor)?,
            ));
            let signature_type = self
                .extrinsic
                .signature_type
                .ok_or_else(|| CoreError::Codec("runtime metadata has no signature type".into()))?;
            fields.push((
                "signature".into(),
                self.decode_id(signature_type, &mut body_cursor)?,
            ));
            for ext in &self.extrinsic.signed_extensions {
                let field = PAYLOAD_FIELDS
                    .iter()
                    .find(|(_, identifier, slot)| {
                        *identifier == ext.identifier && *slot == Slot::Extrinsic
                    })
                    .map(|(field, _, _)| (*field).to_string())
                    .unwrap_or_else(|| ext.identifier.clone());
                let value = self.decode_id(ext.ty, &mut body_cursor)?;
                // Zero-sized extras (CheckWeight & co) contribute no field.
                if matches!(&value, Value::Dict(entries) if entries.is_empty()) {
                    continue;
                }
                fields.push((field, value));
            }
        }
        let call = self.decode_call_value(&mut body_cursor)?;
        fields.push(("call".into(), call));
        // The declared body length must match what the layout actually
        // consumed — otherwise `extrinsic_length` would lie about the bytes.
        if body_cursor.remaining() != 0 {
            return Err(CoreError::Codec(format!(
                "{} undecoded bytes remain inside the extrinsic body",
                body_cursor.remaining()
            )));
        }
        if strict && cursor.remaining() != 0 {
            return Err(CoreError::Codec(format!(
                "{} undecoded bytes remain after the extrinsic",
                cursor.remaining()
            )));
        }
        Ok(Value::record(fields))
    }
}

/// The block at which a mortal era starts (cyscale's `Era.birth`).
pub fn era_birth(period: u64, current: u64) -> u64 {
    let calculated = period.next_power_of_two().clamp(4, 1 << 16);
    let quantize_factor = (calculated >> 12).max(1);
    let phase = (current % calculated) / quantize_factor * quantize_factor;
    (current.max(phase) - phase) / calculated * calculated + phase
}

/// Derive the deterministic M-of-N multisig account for a signer set.
///
/// Returns `(account_id, sorted_signatories)` — the pallet requires the
/// signatory set sorted by raw key bytes, and the derivation is
/// `blake2_256(b"modlpy/utilisuba" ++ Vec<AccountId> ++ u16)`.
pub fn multisig_account_id(
    signatories: &[[u8; 32]],
    threshold: u16,
) -> Result<([u8; 32], Vec<[u8; 32]>), CoreError> {
    if signatories.is_empty() {
        return Err(CoreError::Codec(
            "multisig needs at least one signatory".into(),
        ));
    }
    let mut sorted = signatories.to_vec();
    sorted.sort_unstable();
    let mut data = b"modlpy/utilisuba".to_vec();
    compact(sorted.len() as u128, &mut data)?;
    for signatory in &sorted {
        data.extend_from_slice(signatory);
    }
    data.extend_from_slice(&threshold.to_le_bytes());
    Ok((blake2_256(&data), sorted))
}

/// ss58 rendering for a derived multisig account (convenience for bindings).
pub fn multisig_ss58(account_id: [u8; 32], ss58_format: u16) -> String {
    ss58_from_public(account_id, ss58_format)
}
