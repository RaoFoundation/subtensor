//! Signature payloads, signed-extrinsic assembly and decode, era birth, and
//! multisig account derivation — the transaction-shaped primitives on top of
//! the codec, all pinned by the golden vectors.

// Client-side codec, not runtime code.
#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

use codec::{Decode, Encode};
use scale_info::{form::PortableForm, TypeDef, Variant};
use sp_core::hashing::blake2_256;

use crate::codec::decode::{compact_u128, Cursor};
use crate::codec::encode::compact;
use crate::codec::value::Value;
use crate::error::CoreError;
use crate::keys::{is_hashed_crypto, ss58_from_public};
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
        let proof = if is_hashed_crypto(signature_version) {
            if signature.len() != 136 {
                return Err(CoreError::Codec(
                    "hashed authorization proof must be 136 bytes".into(),
                ));
            }
            if !self.pallets.iter().any(|p| p.name == "HashedAccounts") {
                return Err(CoreError::Codec(
                    "runtime does not support hashed accounts".into(),
                ));
            }
            Some(
                subtensor_hashed::Proof::decode(&mut &signature[..])
                    .map_err(|_| CoreError::Codec("invalid hashed authorization proof".into()))?,
            )
        } else {
            None
        };
        let (address, signature, signature_version) = if let Some(proof) = &proof {
            let scheme = if signature_version == crate::keys::CRYPTO_HASHED {
                subtensor_hashed::Scheme::Sr25519
            } else {
                subtensor_hashed::Scheme::Ed25519
            };
            let address = subtensor_hashed::SignerAddress {
                version: subtensor_hashed::VERSION,
                scheme,
                account: public_key,
                generation: proof.generation,
                public_key: proof.public_key,
                next_commitment: proof.next_commitment,
            };
            (
                Value::record(vec![("Raw".into(), Value::Bytes(address.encode()))]),
                proof.signature.as_slice(),
                if signature_version == crate::keys::CRYPTO_HASHED {
                    1
                } else {
                    0
                },
            )
        } else {
            (
                Value::Bytes(public_key.to_vec()),
                signature,
                signature_version,
            )
        };
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
        self.encode_id(address_type, &address, &mut body)?;
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
        let general = version_byte == 0x45;
        let version = version_byte & 0x7f;
        // Bare v4/v5 extrinsics contain a call; signed v4 and General v5
        // carry distinct authorization/extension envelopes.
        if signed && version != 4 {
            return Err(CoreError::Codec(format!(
                "signed extrinsic version {version} not supported"
            )));
        }
        if !matches!(version_byte, 4 | 5 | 0x84 | 0x45) {
            return Err(CoreError::Codec(format!(
                "unsupported extrinsic format {version_byte}"
            )));
        }
        if general && body_cursor.byte()? != 0 {
            return Err(CoreError::Codec(
                "unsupported general extension version".into(),
            ));
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
        }
        if signed || general {
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

#[cfg(test)]
pub(crate) mod hashed_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use codec::{Decode, Encode};
    use frame_metadata::{RuntimeMetadata, RuntimeMetadataPrefixed};

    use super::*;
    use crate::keys::Keypair;
    use crate::keys::{CRYPTO_HASHED, CRYPTO_HASHED_ED25519};

    pub(crate) fn runtime(hashed: bool) -> Runtime {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../../python/tests/fixtures/golden.json"))
                .unwrap();
        let encoded = hex::decode(
            fixture["metadata"]["v15_hex"]
                .as_str()
                .unwrap()
                .trim_start_matches("0x"),
        )
        .unwrap();
        let metadata = Option::<Vec<u8>>::decode(&mut &encoded[..])
            .unwrap()
            .unwrap();
        let mut metadata = RuntimeMetadataPrefixed::decode(&mut &metadata[..]).unwrap();
        if hashed {
            let RuntimeMetadata::V15(inner) = &mut metadata.1 else {
                panic!("v15 fixture")
            };
            // Only the advertised availability is added. The existing golden
            // call and extension metadata stays identical to a legacy runtime.
            let mut pallet = inner.pallets[0].clone();
            pallet.name = "HashedAccounts".into();
            pallet.index = 250;
            inner.pallets.push(pallet);
        }
        Runtime::parse(&metadata.encode(), 1, 1, 42).unwrap()
    }

    fn params() -> TxParams {
        TxParams {
            era: Value::str("00"),
            nonce: 4,
            tip: 0,
            tip_asset_id: None,
            genesis_hash: [8; 32],
            era_block_hash: [8; 32],
            metadata_hash: None,
        }
    }

    #[test]
    fn hashed_v4_roundtrip_has_standard_signature_and_real_call() {
        use sp_core::Pair;
        for crypto in [CRYPTO_HASHED, CRYPTO_HASHED_ED25519] {
            let runtime = runtime(true);
            let call = runtime
                .compose_call(
                    "System",
                    "remark",
                    &Value::record(vec![("remark".into(), Value::Bytes(vec![7; 300]))]),
                )
                .unwrap();
            let key = Keypair::from_seed(&[4; 32], crypto)
                .unwrap()
                .at_generation(12)
                .unwrap();
            let (extra, additional) = runtime.signature_payload_parts(&params()).unwrap();
            let mut payload = call.clone();
            payload.extend_from_slice(&extra);
            payload.extend_from_slice(&additional);
            let proof_bytes = key.sign_hashed(&payload).unwrap();
            let proof = subtensor_hashed::Proof::decode(&mut proof_bytes.as_slice()).unwrap();
            let (encoded, _) = runtime
                .encode_signed_extrinsic(
                    &call,
                    key.public_key_bytes(),
                    &proof_bytes,
                    crypto,
                    &params(),
                )
                .unwrap();
            let address = subtensor_hashed::SignerAddress {
                version: 1,
                account: key.public_key_bytes(),
                scheme: if crypto == CRYPTO_HASHED {
                    subtensor_hashed::Scheme::Sr25519
                } else {
                    subtensor_hashed::Scheme::Ed25519
                },
                generation: proof.generation,
                public_key: proof.public_key,
                next_commitment: proof.next_commitment,
            };
            let payload = address.signing_hash(&payload);
            let valid = if crypto == CRYPTO_HASHED {
                sp_core::sr25519::Pair::verify(
                    &sp_core::sr25519::Signature::from_raw(proof.signature),
                    payload,
                    &sp_core::sr25519::Public::from_raw(proof.public_key),
                )
            } else {
                sp_core::ed25519::Pair::verify(
                    &sp_core::ed25519::Signature::from_raw(proof.signature),
                    payload,
                    &sp_core::ed25519::Public::from_raw(proof.public_key),
                )
            };
            assert!(valid);
            let mut body = Cursor::new(&encoded);
            let _ = compact_u128(&mut body).unwrap();
            assert_eq!(body.byte().unwrap(), 0x84);
            let decoded = runtime.decode_extrinsic(&encoded, true).unwrap();
            let Value::Dict(fields) = decoded else {
                panic!("record")
            };
            // The direct call survives; the signer carries only fixed bounded metadata.
            assert!(fields.iter().any(|(name, _)| name == &Value::str("call")));
            assert_eq!(body.byte().unwrap(), 2); // MultiAddress::Raw
            assert_eq!(compact_u128(&mut body).unwrap(), 106);
            assert_eq!(body.take(106).unwrap(), address.encode());
            assert!(encoded.ends_with(&call));
            let mut malformed = encoded;
            malformed.push(0);
            assert!(runtime.decode_extrinsic(&malformed, true).is_err());
            assert!(runtime
                .encode_signed_extrinsic(
                    &call,
                    key.public_key_bytes(),
                    &proof_bytes[..64],
                    crypto,
                    &params()
                )
                .is_err());
        }
    }

    #[test]
    fn legacy_metadata_cannot_silently_accept_hashed_authorization() {
        let runtime = runtime(false);
        assert!(runtime
            .encode_signed_extrinsic(&[0, 0], [0; 32], &[0; 136], CRYPTO_HASHED, &params())
            .is_err());
    }

    #[test]
    fn historical_general_zero_decodes_without_hashed_authorization() {
        let runtime = runtime(false);
        let call = runtime
            .compose_call(
                "System",
                "remark",
                &Value::record(vec![("remark".into(), Value::Bytes(vec![]))]),
            )
            .unwrap();
        let (extra, _) = runtime.signature_payload_parts(&params()).unwrap();
        let mut body = vec![0x45, 0];
        body.extend_from_slice(&extra);
        body.extend_from_slice(&call);
        let mut encoded = Vec::new();
        compact(body.len() as u128, &mut encoded).unwrap();
        encoded.extend_from_slice(&body);
        let decoded = runtime.decode_extrinsic(&encoded, true).unwrap();
        let Value::Dict(fields) = decoded else {
            panic!("record expected")
        };
        assert!(!fields
            .iter()
            .any(|(name, _)| name == &Value::str("signature")));
        assert!(fields
            .iter()
            .any(|(name, value)| name == &Value::str("nonce")
                && matches!(value, Value::Uint(4) | Value::Int(4))));
    }
}
