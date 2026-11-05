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
        let pq = if crate::keys::is_pq_crypto(signature_version) {
            let (account, profile, encoded) = if crate::keys::is_falcon_crypto(signature_version) {
                let address = subtensor_hashed::falcon::SignerAddress::from_bytes(signature)
                    .map_err(|_| {
                        CoreError::Codec("invalid Falcon authorization envelope".into())
                    })?;
                (address.account, address.algorithm, address.encode())
            } else {
                let address = subtensor_hashed::mldsa::SignerAddress::from_bytes(signature)
                    .map_err(|_| {
                        CoreError::Codec("invalid ML-DSA authorization envelope".into())
                    })?;
                (address.account, address.algorithm, address.encode())
            };
            if !self.supports_mldsa_profile(profile) {
                return Err(CoreError::Codec(
                    "runtime does not advertise this post-quantum account profile".into(),
                ));
            }
            if account != public_key || crate::keys::pq_profile(signature_version) != Some(profile)
            {
                return Err(CoreError::Codec(
                    "post-quantum proof account does not match signer".into(),
                ));
            }
            Some(encoded)
        } else {
            None
        };
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
        let (address, signature, signature_version) = if let Some(address) = &pq {
            (
                Value::record(vec![("Raw".into(), Value::Bytes(address.clone()))]),
                subtensor_hashed::mldsa::CARRIER_SIGNATURE.as_slice(),
                0,
            )
        } else if let Some(proof) = &proof {
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

    /// Detect the explicit ML-DSA registration variant on the original call.
    /// Presence of HashedAccounts alone does not imply ML-DSA support.
    pub fn supports_mldsa_profile(&self, profile: u8) -> bool {
        if subtensor_hashed::falcon::is_account_profile(profile) {
            return self.supports_registration_variant("Falcon")
                && self
                    .constant("HashedAccounts", "FalconAccountProfiles")
                    .is_some_and(|constant| constant.value.contains(&profile));
        }
        self.supports_mldsa_accounts()
            && match profile {
                subtensor_hashed::mldsa::ALGORITHM => true,
                subtensor_hashed::mldsa::FIXED_ALGORITHM => self
                    .constant("HashedAccounts", "MldsaAccountProfiles")
                    .is_some_and(|constant| constant.value.contains(&profile)),
                _ => false,
            }
    }

    pub fn supports_mldsa_accounts(&self) -> bool {
        self.supports_registration_variant("MlDsa65")
    }

    fn supports_registration_variant(&self, variant: &str) -> bool {
        self.pallets
            .iter()
            .filter(|p| p.name == "HashedAccounts")
            .filter_map(|p| p.calls_type)
            .any(|id| {
                let Ok(ty) = self.resolve(id) else {
                    return false;
                };
                let TypeDef::Variant(calls) = &ty.type_def else {
                    return false;
                };
                calls.variants.iter().any(|call| {
                    let [field] = call.fields.as_slice() else {
                        return false;
                    };
                    if call.name != "register" || field.name.as_deref() != Some("descriptor") {
                        return false;
                    }
                    let Ok(descriptor) = self.resolve(field.ty.id) else {
                        return false;
                    };
                    matches!(&descriptor.type_def, TypeDef::Variant(schemes)
                        if schemes.variants.iter().any(|scheme|
                            scheme.name == variant && scheme.fields.len() == 1)
                        && schemes.variants.iter().any(|scheme|
                            scheme.name == "Classical" && scheme.fields.len() == 1))
                })
            })
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

/// Derive an opt-in multisig account that cannot be a raw Ed25519/Sr25519 key.
/// The ordinary multisig helper remains unchanged for existing accounts.
pub fn safe_multisig_account_id(
    signatories: &[[u8; 32]],
    threshold: u16,
) -> Result<([u8; 32], Vec<[u8; 32]>), CoreError> {
    let max_signatories = subtensor_hashed::safe::MAX_MULTISIG_SIGNATORIES as usize;
    if !(2..=max_signatories).contains(&signatories.len()) {
        return Err(CoreError::Codec(format!(
            "safe multisig requires 2 to {max_signatories} signatories"
        )));
    }
    if threshold == 0 || usize::from(threshold) > signatories.len() {
        return Err(CoreError::Codec("invalid multisig threshold".into()));
    }
    let mut sorted = signatories.to_vec();
    sorted.sort_unstable();
    if sorted.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(CoreError::Codec("duplicate multisig signatory".into()));
    }
    let account = subtensor_hashed::safe::safe_derived_account(
        subtensor_hashed::safe::MULTISIG_DOMAIN,
        &(sorted.as_slice(), threshold),
    )
    .ok_or_else(|| CoreError::Codec("safe multisig derivation exhausted its search".into()))?;
    Ok((account, sorted))
}

/// ss58 rendering for a derived multisig account (convenience for bindings).
pub fn multisig_ss58(account_id: [u8; 32], ss58_format: u16) -> String {
    ss58_from_public(account_id, ss58_format)
}

#[cfg(test)]
mod safe_multisig_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::{multisig_account_id, safe_multisig_account_id};

    #[test]
    fn safe_mode_is_order_independent_and_separate_from_legacy() {
        let members = [[2; 32], [1; 32], [3; 32]];
        let (account, sorted) = safe_multisig_account_id(&members, 2).expect("valid group");
        assert_eq!(sorted, vec![[1; 32], [2; 32], [3; 32]]);
        assert_eq!(
            safe_multisig_account_id(&sorted, 2)
                .expect("sorted group")
                .0,
            account
        );
        assert!(subtensor_hashed::mldsa::rejects_legacy_public_keys(
            &account
        ));
        assert_ne!(
            multisig_account_id(&members, 2).expect("legacy group").0,
            account
        );
        assert_ne!(
            safe_multisig_account_id(&members, 1)
                .expect("other threshold")
                .0,
            account
        );
    }

    #[test]
    fn safe_mode_rejects_invalid_member_manifests() {
        assert!(safe_multisig_account_id(&[], 1).is_err());
        assert!(safe_multisig_account_id(&[[1; 32]], 1).is_err());
        assert!(safe_multisig_account_id(&[[1; 32], [1; 32]], 2).is_err());
        assert!(safe_multisig_account_id(&[[1; 32], [2; 32]], 0).is_err());
        assert!(safe_multisig_account_id(&[[1; 32], [2; 32]], 3).is_err());
    }

    #[test]
    fn safe_mode_enforces_runtime_signer_count_boundaries() {
        let max = subtensor_hashed::safe::MAX_MULTISIG_SIGNATORIES as usize;
        let members: Vec<_> = (0..=max)
            .map(|index| {
                let mut account = [0u8; 32];
                account[..8].copy_from_slice(&(index as u64).to_le_bytes());
                account
            })
            .collect();
        for count in [0, 1, max + 1] {
            assert!(safe_multisig_account_id(&members[..count], 1).is_err());
        }
        for count in [2, max] {
            for threshold in [1, count as u16] {
                let (account, sorted) =
                    safe_multisig_account_id(&members[..count], threshold).expect("valid group");
                assert_eq!(sorted.len(), count);
                assert!(subtensor_hashed::mldsa::rejects_legacy_public_keys(
                    &account
                ));
                assert_eq!(
                    safe_multisig_account_id(&sorted, threshold)
                        .expect("sorted group")
                        .0,
                    account
                );
            }
        }
    }
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

    pub(crate) fn runtime_mldsa() -> Runtime {
        let mut runtime = runtime(true);
        assert!(!runtime.supports_mldsa_accounts());
        let pallet = runtime
            .pallets
            .iter_mut()
            .find(|p| p.name == "HashedAccounts")
            .unwrap();
        let source = pallet.calls_type.unwrap();
        let mut calls = runtime.types.resolve(source).unwrap().clone();
        let TypeDef::Variant(variants) = &mut calls.type_def else {
            panic!("call variants")
        };
        // Advertise the exact registration enum names without changing any
        // golden extrinsic envelope, economic call, or signing extension type.
        let mut marker = variants.variants[0].clone();
        marker.name = "register".into();
        marker.index = 0;
        marker.fields.truncate(1);
        marker.fields[0].name = Some("descriptor".into());
        let mut registration = calls.clone();
        let TypeDef::Variant(schemes) = &mut registration.type_def else {
            panic!("registration variants")
        };
        let mut scheme = marker.clone();
        scheme.name = "Classical".into();
        scheme.fields[0].name = None;
        schemes.variants = vec![scheme.clone()];
        scheme.name = "MlDsa65".into();
        scheme.index = 1;
        schemes.variants.push(scheme);
        let registration_id = runtime.types.types.len() as u32;
        runtime.types.types.push(scale_info::PortableType {
            id: registration_id,
            ty: registration,
        });
        marker.fields[0].ty = registration_id.into();
        let TypeDef::Variant(variants) = &mut calls.type_def else {
            panic!("call variants")
        };
        variants.variants = vec![marker];
        let id = runtime.types.types.len() as u32;
        runtime
            .types
            .types
            .push(scale_info::PortableType { id, ty: calls });
        pallet.calls_type = Some(id);
        pallet.constants.push(crate::runtime::ConstantInfo {
            name: "MldsaAccountProfiles".into(),
            ty: 0,
            value: vec![4, 5],
            docs: vec![],
        });
        assert!(runtime.supports_mldsa_accounts());
        runtime
    }

    pub(crate) fn runtime_falcon() -> Runtime {
        let mut runtime = runtime_mldsa();
        let pallet = runtime
            .pallets
            .iter_mut()
            .find(|p| p.name == "HashedAccounts")
            .unwrap();
        let call_id = pallet.calls_type.unwrap();
        pallet.constants.push(crate::runtime::ConstantInfo {
            name: "FalconAccountProfiles".into(),
            ty: 0,
            value: vec![6, 7, 8, 9],
            docs: vec![],
        });
        let TypeDef::Variant(calls) = &runtime.types.resolve(call_id).unwrap().type_def else {
            panic!("calls")
        };
        let descriptor_id = calls
            .variants
            .iter()
            .find(|call| call.name == "register")
            .unwrap()
            .fields[0]
            .ty
            .id;
        let descriptor = &mut runtime.types.types[descriptor_id as usize].ty;
        let TypeDef::Variant(variants) = &mut descriptor.type_def else {
            panic!("descriptor")
        };
        let mut falcon = variants.variants[1].clone();
        falcon.name = "Falcon".into();
        falcon.index = 2;
        variants.variants.push(falcon);
        runtime
    }

    #[test]
    fn fixed_profile_requires_advertised_runtime_support_before_encoding() {
        let mut runtime = runtime_mldsa();
        assert!(runtime.supports_mldsa_profile(4));
        assert!(runtime.supports_mldsa_profile(5));
        runtime
            .pallets
            .iter_mut()
            .find(|p| p.name == "HashedAccounts")
            .unwrap()
            .constants
            .retain(|c| c.name != "MldsaAccountProfiles");
        assert!(runtime.supports_mldsa_profile(4));
        assert!(!runtime.supports_mldsa_profile(5));
        let fixed = Keypair::from_seed(&[23; 32], crate::keys::CRYPTO_MLDSA65_FIXED).unwrap();
        let proof = fixed.sign_mldsa(b"call").unwrap();
        assert!(runtime
            .encode_signed_extrinsic(
                &[0, 0],
                fixed.public_key_bytes(),
                &proof,
                crate::keys::CRYPTO_MLDSA65_FIXED,
                &params()
            )
            .is_err());
    }

    #[test]
    fn mldsa_support_requires_registration_enum_not_a_call_name_marker() {
        let mut runtime = runtime_mldsa();
        let call_id = runtime
            .pallets
            .iter()
            .find(|pallet| pallet.name == "HashedAccounts")
            .unwrap()
            .calls_type
            .unwrap();
        let TypeDef::Variant(calls) = &mut runtime.types.types[call_id as usize].ty.type_def else {
            panic!("call variants")
        };
        let descriptor_id = calls.variants[0].fields[0].ty.id;
        calls.variants[0].name = "register_unsupported".into();
        assert!(!runtime.supports_mldsa_accounts());
        let TypeDef::Variant(calls) = &mut runtime.types.types[call_id as usize].ty.type_def else {
            panic!("call variants")
        };
        calls.variants[0].name = "register".into();
        assert!(runtime.supports_mldsa_accounts());
        let TypeDef::Variant(schemes) =
            &mut runtime.types.types[descriptor_id as usize].ty.type_def
        else {
            panic!("registration variants")
        };
        schemes.variants.retain(|variant| variant.name != "MlDsa65");
        assert!(!runtime.supports_mldsa_accounts());
    }

    #[test]
    fn mldsa_v4_proof_preserves_outer_types_and_call_bytes() {
        for crypto in [
            crate::keys::CRYPTO_MLDSA65,
            crate::keys::CRYPTO_MLDSA65_FIXED,
            crate::keys::CRYPTO_FALCON512,
            crate::keys::CRYPTO_FALCON512_HASHED,
            crate::keys::CRYPTO_FALCON1024,
            crate::keys::CRYPTO_FALCON1024_HASHED,
        ] {
            let legacy_runtime = runtime(false);
            let runtime = runtime_falcon();
            let call = runtime
                .compose_call(
                    "System",
                    "remark",
                    &Value::record(vec![("remark".into(), Value::Bytes(vec![7; 300]))]),
                )
                .unwrap();
            let wallet = Keypair::from_seed(&[7; 32], crypto)
                .unwrap()
                .at_generation(if crate::keys::is_rotating_crypto(crypto) {
                    3
                } else {
                    0
                })
                .unwrap();
            let (extra, additional) = runtime.signature_payload_parts(&params()).unwrap();
            let payload = [call.clone(), extra.clone(), additional].concat();
            let proof = wallet
                .sign_mldsa_with_randomness(&payload, &[0; 32])
                .unwrap();
            if crate::keys::is_falcon_crypto(crypto) {
                assert!(subtensor_hashed::falcon::SignerAddress::from_bytes(&proof)
                    .unwrap()
                    .verify(&payload));
            } else {
                assert!(subtensor_hashed::mldsa::SignerAddress::from_bytes(&proof)
                    .unwrap()
                    .verify(&payload));
            }
            let (encoded, hash) = runtime
                .encode_signed_extrinsic(
                    &call,
                    wallet.public_key_bytes(),
                    &proof,
                    crypto,
                    &params(),
                )
                .unwrap();
            assert_eq!(hash, blake2_256(&encoded));
            runtime.decode_extrinsic(&encoded, true).unwrap();
            legacy_runtime.decode_extrinsic(&encoded, true).unwrap();
            let mut body = Cursor::new(&encoded);
            let _ = compact_u128(&mut body).unwrap();
            assert_eq!(body.byte().unwrap(), 0x84);
            assert_eq!(body.byte().unwrap(), 2);
            assert_eq!(compact_u128(&mut body).unwrap(), proof.len() as u128);
            assert_eq!(body.take(proof.len()).unwrap(), proof);
            assert_eq!(body.byte().unwrap(), 0);
            assert_eq!(
                body.take(64).unwrap(),
                subtensor_hashed::mldsa::CARRIER_SIGNATURE
            );
            assert_eq!(body.take(extra.len()).unwrap(), extra);
            assert_eq!(body.take(call.len()).unwrap(), call);
            assert!(runtime
                .encode_signed_extrinsic(&call, [0; 32], &proof, crypto, &params())
                .is_err());
            assert!(legacy_runtime
                .encode_signed_extrinsic(
                    &call,
                    wallet.public_key_bytes(),
                    &proof,
                    crypto,
                    &params()
                )
                .is_err());
        }
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
