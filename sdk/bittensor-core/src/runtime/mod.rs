//! `Runtime`: one immutable object per spec version, parsed from the raw
//! `MetadataVersioned` bytes the transport already downloads and caches.
//!
//! Every metadata question the SDK asks — storage entries, call composition,
//! constants, module errors, runtime APIs, the codegen IR — is answered here,
//! keyed by portable-registry type *ids* internally and by *names* at the
//! seam (ids are not stable across spec versions, so they never leave the
//! process; see the spec's §4.2).
//!
//! Immutable and `Send + Sync`: decoded values are materialized fresh per
//! call and nothing mutates after `parse`.

// Client-side metadata views, not runtime code.
#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

pub mod type_string;

use std::collections::HashMap;

use codec::Decode;
use frame_metadata::{v14, v15, RuntimeMetadata, RuntimeMetadataPrefixed};
use scale_info::{form::PortableForm, PortableRegistry, TypeDef};

use crate::error::CoreError;

/// One storage item's full description: everything needed to build keys for
/// and decode values of it.
#[derive(Debug, Clone)]
pub struct StorageInfo {
    pub name: String,
    /// The pallet's storage prefix string (usually the pallet name).
    pub prefix: String,
    /// "Default" | "Optional"
    pub modifier: String,
    pub hashers: Vec<String>,
    /// Unhashed key component type ids (one per hasher).
    pub key_types: Vec<u32>,
    pub value_type: u32,
    pub default_bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct ConstantInfo {
    pub name: String,
    pub ty: u32,
    pub value: Vec<u8>,
    pub docs: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PalletInfo {
    pub name: String,
    pub index: u8,
    /// The pallet's `Call` enum type id (variant type), when it has calls.
    pub calls_type: Option<u32>,
    /// The pallet's `Event` enum type id, when it has events.
    pub events_type: Option<u32>,
    /// The pallet's `Error` enum type id, when it has errors.
    pub errors_type: Option<u32>,
    pub constants: Vec<ConstantInfo>,
    pub storage: Vec<StorageInfo>,
}

#[derive(Debug, Clone)]
pub struct SignedExtensionInfo {
    pub identifier: String,
    /// Type id of the bytes that travel in the extrinsic ("extra").
    pub ty: u32,
    /// Type id of the implied bytes both sides sign ("additional_signed").
    pub additional_signed: u32,
}

#[derive(Debug, Clone)]
pub struct ExtrinsicInfo {
    pub version: u8,
    pub address_type: Option<u32>,
    pub call_type: Option<u32>,
    pub signature_type: Option<u32>,
    pub signed_extensions: Vec<SignedExtensionInfo>,
}

#[derive(Debug, Clone)]
pub struct RuntimeApiParamInfo {
    pub name: String,
    pub ty: u32,
}

#[derive(Debug, Clone)]
pub struct RuntimeApiMethodInfo {
    pub name: String,
    pub inputs: Vec<RuntimeApiParamInfo>,
    pub output: u32,
    pub docs: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RuntimeApiInfo {
    pub name: String,
    pub methods: Vec<RuntimeApiMethodInfo>,
}

/// V14 and V15 pallet metadata share the fields we read but are distinct
/// types; the macro is the shared conversion.
macro_rules! pallet_info {
    ($pallet:expr, $types:expr) => {{
        let pallet = $pallet;
        let storage = match &pallet.storage {
            Some(s) => s
                .entries
                .iter()
                .map(|e| storage_info(&s.prefix, e, $types))
                .collect(),
            None => Vec::new(),
        };
        PalletInfo {
            name: pallet.name.clone(),
            index: pallet.index,
            calls_type: pallet.calls.as_ref().map(|c| c.ty.id),
            events_type: pallet.event.as_ref().map(|e| e.ty.id),
            errors_type: pallet.error.as_ref().map(|e| e.ty.id),
            constants: pallet
                .constants
                .iter()
                .map(|c| ConstantInfo {
                    name: c.name.clone(),
                    ty: c.ty.id,
                    value: c.value.clone(),
                    docs: c.docs.clone(),
                })
                .collect(),
            storage,
        }
    }};
}

/// One runtime's complete metadata view.
pub struct Runtime {
    pub spec_version: u32,
    pub transaction_version: u32,
    pub ss58_format: u16,
    pub is_v15: bool,
    /// The raw `MetadataVersioned` blob this was parsed from (fed to the
    /// RFC-0078 digest without re-downloading).
    pub metadata_bytes: Vec<u8>,
    pub types: PortableRegistry,
    pub pallets: Vec<PalletInfo>,
    pub extrinsic: ExtrinsicInfo,
    pub apis: Vec<RuntimeApiInfo>,
    /// The outer RuntimeEvent enum type id (drives the flattened event shape).
    pub outer_event_type: Option<u32>,
    pallet_by_name: HashMap<String, usize>,
    pallet_by_index: HashMap<u8, usize>,
    /// Registry type-name maps (the `_name_maps` logic from the Python codec):
    /// unambiguous last-path-segment names plus derived Vec<...>/(...) names.
    name_to_id: HashMap<String, u32>,
    id_to_name: HashMap<u32, String>,
}

impl Runtime {
    /// Parse a raw `MetadataVersioned` blob (magic `meta` + version byte +
    /// V14/V15 payload — what `state_getMetadata` / `Metadata_metadata_at_version`
    /// return, unwrapped).
    pub fn parse(
        metadata_bytes: &[u8],
        spec_version: u32,
        transaction_version: u32,
        ss58_format: u16,
    ) -> Result<Self, CoreError> {
        let prefixed = RuntimeMetadataPrefixed::decode(&mut &metadata_bytes[..])
            .map_err(|e| CoreError::Codec(format!("cannot decode runtime metadata: {e}")))?;
        let (types, pallets, extrinsic, apis, outer_event_type, is_v15) = match prefixed.1 {
            RuntimeMetadata::V14(m) => {
                let pallets = m
                    .pallets
                    .iter()
                    .map(|p| pallet_info!(p, &m.types))
                    .collect();
                let extrinsic = extrinsic_info_v14(&m.extrinsic, &m.types);
                let outer_event = find_outer_event_v14(&m.types);
                (m.types, pallets, extrinsic, Vec::new(), outer_event, false)
            }
            RuntimeMetadata::V15(m) => {
                let pallets = m
                    .pallets
                    .iter()
                    .map(|p| pallet_info!(p, &m.types))
                    .collect();
                let extrinsic = extrinsic_info_v15(&m.extrinsic);
                let apis = m.apis.iter().map(api_info_v15).collect();
                let outer_event = Some(m.outer_enums.event_enum_ty.id);
                (m.types, pallets, extrinsic, apis, outer_event, true)
            }
            other => {
                return Err(CoreError::Codec(format!(
                    "unsupported metadata version V{}",
                    other.version()
                )))
            }
        };
        let pallets: Vec<PalletInfo> = pallets;
        let pallet_by_name = pallets
            .iter()
            .enumerate()
            .map(|(i, p)| (p.name.clone(), i))
            .collect();
        let pallet_by_index = pallets
            .iter()
            .enumerate()
            .map(|(i, p)| (p.index, i))
            .collect();
        let (name_to_id, id_to_name) = build_name_maps(&types);
        Ok(Self {
            spec_version,
            transaction_version,
            ss58_format,
            is_v15,
            metadata_bytes: metadata_bytes.to_vec(),
            types,
            pallets,
            extrinsic,
            apis,
            outer_event_type,
            pallet_by_name,
            pallet_by_index,
            name_to_id,
            id_to_name,
        })
    }

    pub fn pallet(&self, name: &str) -> Option<&PalletInfo> {
        self.pallet_by_name.get(name).map(|&i| &self.pallets[i])
    }

    pub fn pallet_at(&self, index: u8) -> Option<&PalletInfo> {
        self.pallet_by_index.get(&index).map(|&i| &self.pallets[i])
    }

    pub fn storage_entry(&self, pallet: &str, name: &str) -> Option<&StorageInfo> {
        self.pallet(pallet)?.storage.iter().find(|s| s.name == name)
    }

    pub fn constant(&self, pallet: &str, name: &str) -> Option<&ConstantInfo> {
        self.pallet(pallet)?
            .constants
            .iter()
            .find(|c| c.name == name)
    }

    pub fn type_id_of(&self, name: &str) -> Option<u32> {
        self.name_to_id.get(name).copied()
    }

    pub fn type_name_of(&self, id: u32) -> Option<&str> {
        self.id_to_name.get(&id).map(String::as_str)
    }

    pub fn resolve(&self, id: u32) -> Result<&scale_info::Type<PortableForm>, CoreError> {
        self.types
            .resolve(id)
            .ok_or_else(|| CoreError::Codec(format!("unknown type id {id}")))
    }

    /// A short, human-facing identity for a registry type: the last segment
    /// of its path (e.g. "TaoBalance", "NetUid", "AccountId32"), with
    /// `Option`'s payload kept (`Option<Timepoint>`). Path-less types render
    /// structurally: "u64", "Vec<u16>", "[u8; 32]", "(u16, u16)",
    /// "Compact<u64>".
    ///
    /// This is what the codegen IR carries per call parameter, so generated
    /// builders can surface newtype identity (a TaoBalance argument vs a
    /// bare u64) even though the wire encoding is the inner value.
    pub fn type_ident(&self, id: u32) -> String {
        self.type_ident_bounded(id, 0)
    }

    fn type_ident_bounded(&self, id: u32, depth: usize) -> String {
        // The registry comes from the connected node and is untrusted; bound
        // recursion like the decoders do.
        if depth > 8 {
            return format!("scale_info::{id}");
        }
        let Ok(ty) = self.resolve(id) else {
            return format!("scale_info::{id}");
        };
        if let Some(last) = ty.path.segments.last() {
            if last == "Option" && ty.path.segments.len() == 1 {
                if let TypeDef::Variant(variant) = &ty.type_def {
                    if let Some(field) = variant
                        .variants
                        .iter()
                        .find(|v| v.name == "Some")
                        .and_then(|v| v.fields.first())
                    {
                        return format!(
                            "Option<{}>",
                            self.type_ident_bounded(field.ty.id, depth + 1)
                        );
                    }
                }
            }
            return last.clone();
        }
        match &ty.type_def {
            TypeDef::Primitive(p) => primitive_name(p).to_string(),
            TypeDef::Compact(c) => format!(
                "Compact<{}>",
                self.type_ident_bounded(c.type_param.id, depth + 1)
            ),
            TypeDef::Sequence(s) => format!(
                "Vec<{}>",
                self.type_ident_bounded(s.type_param.id, depth + 1)
            ),
            TypeDef::Array(a) => format!(
                "[{}; {}]",
                self.type_ident_bounded(a.type_param.id, depth + 1),
                a.len
            ),
            TypeDef::Tuple(t) => {
                let parts: Vec<String> = t
                    .fields
                    .iter()
                    .map(|f| self.type_ident_bounded(f.id, depth + 1))
                    .collect();
                format!("({})", parts.join(", "))
            }
            _ => format!("scale_info::{id}"),
        }
    }

    /// The portable registry as JSON (`{"types": [{"id", "type": {...}}]}`).
    ///
    /// For registry-walking tooling (the shape-corpus recorder); not a hot
    /// path.
    pub fn registry_json(&self) -> Result<String, CoreError> {
        serde_json::to_string(&self.types)
            .map_err(|e| CoreError::Codec(format!("registry serialization failed: {e}")))
    }

    /// `{"type": "Module", "name", "docs"}` inputs for a dispatch module error.
    pub fn module_error(
        &self,
        module_index: u8,
        error_index: u8,
    ) -> Result<(String, Vec<String>), CoreError> {
        let pallet = self
            .pallet_at(module_index)
            .ok_or_else(|| CoreError::Codec(format!("no pallet at index {module_index}")))?;
        let errors_type = pallet
            .errors_type
            .ok_or_else(|| CoreError::Codec(format!("pallet {} has no errors", pallet.name)))?;
        let ty = self.resolve(errors_type)?;
        let TypeDef::Variant(variant) = &ty.type_def else {
            return Err(CoreError::Codec("error type is not an enum".into()));
        };
        let error = variant
            .variants
            .iter()
            .find(|v| v.index == error_index)
            .ok_or_else(|| {
                CoreError::Codec(format!(
                    "no error at index {error_index} in pallet {}",
                    pallet.name
                ))
            })?;
        Ok((error.name.clone(), error.docs.clone()))
    }
}

fn storage_info(
    prefix: &str,
    entry: &v14::StorageEntryMetadata<PortableForm>,
    types: &PortableRegistry,
) -> StorageInfo {
    let modifier = match entry.modifier {
        v14::StorageEntryModifier::Optional => "Optional",
        v14::StorageEntryModifier::Default => "Default",
    };
    let (hashers, key_types, value_type) = match &entry.ty {
        v14::StorageEntryType::Plain(ty) => (Vec::new(), Vec::new(), ty.id),
        v14::StorageEntryType::Map {
            hashers,
            key,
            value,
        } => {
            let hasher_names: Vec<String> = hashers.iter().map(|h| format!("{h:?}")).collect();
            // One hasher, one key type; several hashers hash the components
            // of a key tuple separately.
            let key_ids: Vec<u32> = if hasher_names.len() == 1 {
                vec![key.id]
            } else {
                match types.resolve(key.id).map(|t| &t.type_def) {
                    Some(TypeDef::Tuple(tuple)) => tuple.fields.iter().map(|f| f.id).collect(),
                    _ => vec![key.id],
                }
            };
            (hasher_names, key_ids, value.id)
        }
    };
    StorageInfo {
        name: entry.name.clone(),
        prefix: prefix.to_string(),
        modifier: modifier.to_string(),
        hashers,
        key_types,
        value_type,
        default_bytes: entry.default.clone(),
    }
}

fn extrinsic_info_v14(
    extrinsic: &v14::ExtrinsicMetadata<PortableForm>,
    types: &PortableRegistry,
) -> ExtrinsicInfo {
    // V14 carries only the UncheckedExtrinsic type; its generic params name
    // the Address / Call / Signature / Extra types.
    let mut address_type = None;
    let mut call_type = None;
    let mut signature_type = None;
    if let Some(ty) = types.resolve(extrinsic.ty.id) {
        for param in &ty.type_params {
            let target = match param.name.as_str() {
                "Address" => &mut address_type,
                "Call" => &mut call_type,
                "Signature" => &mut signature_type,
                _ => continue,
            };
            *target = param.ty.map(|t| t.id);
        }
    }
    ExtrinsicInfo {
        version: extrinsic.version,
        address_type,
        call_type,
        signature_type,
        signed_extensions: extrinsic
            .signed_extensions
            .iter()
            .map(|e| SignedExtensionInfo {
                identifier: e.identifier.clone(),
                ty: e.ty.id,
                additional_signed: e.additional_signed.id,
            })
            .collect(),
    }
}

fn extrinsic_info_v15(extrinsic: &v15::ExtrinsicMetadata<PortableForm>) -> ExtrinsicInfo {
    ExtrinsicInfo {
        version: extrinsic.version,
        address_type: Some(extrinsic.address_ty.id),
        call_type: Some(extrinsic.call_ty.id),
        signature_type: Some(extrinsic.signature_ty.id),
        signed_extensions: extrinsic
            .signed_extensions
            .iter()
            .map(|e| SignedExtensionInfo {
                identifier: e.identifier.clone(),
                ty: e.ty.id,
                additional_signed: e.additional_signed.id,
            })
            .collect(),
    }
}

/// V14 has no outer_enums section; the outer event enum is the `event` field
/// of `frame_system::EventRecord`.
fn find_outer_event_v14(types: &PortableRegistry) -> Option<u32> {
    let record = types.types.iter().find(|t| {
        t.ty.path.segments.len() == 2
            && t.ty.path.segments[0] == "frame_system"
            && t.ty.path.segments[1] == "EventRecord"
    })?;
    let TypeDef::Composite(composite) = &record.ty.type_def else {
        return None;
    };
    composite
        .fields
        .iter()
        .find(|f| f.name.as_deref() == Some("event"))
        .map(|f| f.ty.id)
}

fn api_info_v15(api: &v15::RuntimeApiMetadata<PortableForm>) -> RuntimeApiInfo {
    RuntimeApiInfo {
        name: api.name.clone(),
        methods: api
            .methods
            .iter()
            .map(|m| RuntimeApiMethodInfo {
                name: m.name.clone(),
                inputs: m
                    .inputs
                    .iter()
                    .map(|p| RuntimeApiParamInfo {
                        name: p.name.clone(),
                        ty: p.ty.id,
                    })
                    .collect(),
                output: m.output.id,
                docs: m.docs.clone(),
            })
            .collect(),
    }
}

/// The registry name maps: unambiguous last-path-segment names for concrete
/// types, plus `Vec<...>` / `[T; N]` / `Compact<...>` / tuple names derived
/// bottom-up — the same fixed-point the Python `_name_maps` computed. Used by
/// the legacy runtime-call registry, which addresses types by name.
fn build_name_maps(types: &PortableRegistry) -> (HashMap<String, u32>, HashMap<u32, String>) {
    let mut name_to_id: HashMap<String, u32> = HashMap::new();
    let mut id_to_name: HashMap<u32, String> = HashMap::new();

    for ty in types.types.iter() {
        let def = &ty.ty.type_def;
        if !ty.ty.type_params.is_empty() || matches!(def, TypeDef::Variant(_)) {
            continue;
        }
        if let Some(segment) = ty.ty.path.segments.last() {
            name_to_id.insert(segment.clone(), ty.id);
            id_to_name.insert(ty.id, segment.clone());
        } else if let TypeDef::Primitive(p) = def {
            let name = primitive_name(p);
            name_to_id.insert(name.to_string(), ty.id);
            id_to_name.insert(ty.id, name.to_string());
        }
    }

    let mut progressed = true;
    while progressed {
        progressed = false;
        for ty in types.types.iter() {
            if id_to_name.contains_key(&ty.id) {
                continue;
            }
            let resolved: Option<String> = (|| {
                let def = &ty.ty.type_def;
                if let Some(segment) = ty.ty.path.segments.last() {
                    if !ty.ty.type_params.is_empty() {
                        let mut inner = Vec::new();
                        for param in &ty.ty.type_params {
                            let dep = param.ty?.id;
                            inner.push(id_to_name.get(&dep)?.clone());
                        }
                        return Some(format!("{segment}<{}>", inner.join(", ")));
                    }
                    if matches!(def, TypeDef::Variant(_)) {
                        return None;
                    }
                    return Some(segment.clone());
                }
                match def {
                    TypeDef::Sequence(s) => {
                        Some(format!("Vec<{}>", id_to_name.get(&s.type_param.id)?))
                    }
                    TypeDef::Array(a) => Some(format!(
                        "[{}; {}]",
                        id_to_name.get(&a.type_param.id)?,
                        a.len
                    )),
                    TypeDef::Compact(c) => {
                        Some(format!("Compact<{}>", id_to_name.get(&c.type_param.id)?))
                    }
                    TypeDef::Tuple(t) => {
                        let mut names = Vec::new();
                        for field in &t.fields {
                            names.push(id_to_name.get(&field.id)?.clone());
                        }
                        Some(format!("({})", names.join(", ")))
                    }
                    _ => None,
                }
            })();
            if let Some(name) = resolved {
                name_to_id.insert(name.clone(), ty.id);
                id_to_name.insert(ty.id, name);
                progressed = true;
            }
        }
    }
    (name_to_id, id_to_name)
}

pub(crate) fn primitive_name(p: &scale_info::TypeDefPrimitive) -> &'static str {
    use scale_info::TypeDefPrimitive as P;
    match p {
        P::Bool => "bool",
        P::Char => "char",
        P::Str => "str",
        P::U8 => "u8",
        P::U16 => "u16",
        P::U32 => "u32",
        P::U64 => "u64",
        P::U128 => "u128",
        P::U256 => "u256",
        P::I8 => "i8",
        P::I16 => "i16",
        P::I32 => "i32",
        P::I64 => "i64",
        P::I128 => "i128",
        P::I256 => "i256",
    }
}
