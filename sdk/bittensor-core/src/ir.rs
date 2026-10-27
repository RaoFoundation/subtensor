//! Serializable metadata IR shared by code generation and runtime inspection.
//!
//! Existing Python IR fields retain their names and shapes. Additional fields
//! expose storage key identities, indexed events, runtime API signatures, and
//! signed extensions. Registry IDs describe this runtime only; generated public
//! descriptors should use names and structural hashes instead.

use std::collections::{BTreeMap, BTreeSet};

use crate::{runtime::Runtime, CoreError};
use scale_info::{form::PortableForm, TypeDef};
use serde::{Deserialize, Serialize};

/// A runtime's serializable code-generation view.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetadataIr {
    /// Runtime specification version supplied when parsing the metadata.
    pub spec_version: u32,
    /// Pallets, in metadata order.
    pub pallets: Vec<PalletIr>,
    /// Runtime APIs; empty for V14 metadata.
    pub runtime_apis: Vec<RuntimeApiIr>,
    /// Extrinsic version and signed extension identities.
    pub extrinsic: ExtrinsicIr,
}

/// One pallet, with the fields exposed by Python's metadata IR.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PalletIr {
    /// Pallet name.
    pub name: String,
    /// Outer enum index of this pallet.
    pub index: u8,
    /// Callable variants, in metadata order.
    pub calls: Vec<CallIr>,
    /// Module errors, including their variant indices.
    pub errors: Vec<ErrorIr>,
    /// Real storage entries; pseudo-entries containing `:` are excluded.
    pub storage: Vec<StorageIr>,
    /// Constant names, preserving Python's original IR shape.
    pub constants: Vec<String>,
    /// Constant types and encoded values.
    pub constant_details: Vec<ConstantIr>,
    /// Event variants and their fields.
    pub events: Vec<EventIr>,
}

/// One call variant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallIr {
    /// Metadata name used for dynamic lookup.
    pub name: String,
    /// Variant index, for drift reports rather than baked call encoding.
    pub index: u8,
    /// Arguments, in encoding order.
    pub args: Vec<FieldIr>,
    /// Trimmed documentation lines joined with spaces, as in Python.
    pub docs: String,
}

/// A call argument, event field, or runtime API input.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldIr {
    /// Field name; empty for unnamed fields, as in Python's call IR.
    pub name: String,
    /// Identity rendered by the core's `Runtime::type_ident`.
    pub type_ident: String,
    /// Portable registry ID, valid only within the source runtime.
    pub type_id: u32,
}

/// An indexed dispatch module error.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorIr {
    /// Variant index within the pallet's error enum.
    pub index: u8,
    /// Error variant name.
    pub name: String,
    /// Joined documentation, matching Python's IR.
    pub docs: String,
}

/// An indexed event, with named or positional field identities.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventIr {
    /// Variant index within the pallet's event enum.
    pub index: u8,
    /// Event variant name.
    pub name: String,
    /// Fields, in encoding order.
    pub fields: Vec<FieldIr>,
    /// Joined documentation.
    pub docs: String,
}

/// A storage entry's identities, hashers, and missing-value semantics.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageIr {
    /// Storage item name.
    pub name: String,
    /// Value identity, retaining Python's original IR field.
    pub value_type_ident: String,
    /// Storage prefix from metadata, which may differ from the pallet name.
    pub prefix: String,
    /// `Default` or `Optional`.
    pub modifier: String,
    /// One metadata-named hasher per unhashed key component.
    pub hashers: Vec<String>,
    /// Unhashed key component identities, in encoding order.
    pub key_type_idents: Vec<String>,
    /// Unhashed key component registry IDs.
    pub key_types: Vec<u32>,
    /// Value registry ID.
    pub value_type: u32,
    /// SCALE bytes used when a `Default` entry is absent.
    pub default_bytes: Vec<u8>,
}

/// A constant's encoded value and type identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConstantIr {
    /// Constant name.
    pub name: String,
    /// Identity rendered by the core.
    pub type_ident: String,
    /// Portable registry ID.
    pub type_id: u32,
    /// Encoded constant value.
    pub value: Vec<u8>,
    /// Joined documentation.
    pub docs: String,
}

/// A runtime API trait, retaining the original list of method names.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeApiIr {
    /// API trait name.
    pub name: String,
    /// Method names in metadata order, as in Python's IR.
    pub methods: Vec<String>,
    /// Complete method signatures, in the same order.
    pub method_signatures: Vec<RuntimeApiMethodIr>,
    /// Advertised API version. V15 has no versions, so callers supply these
    /// from the pinned runtime version or V16 metadata through `set_api_versions`.
    pub version: Option<u32>,
}

/// A metadata-described runtime API method.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeApiMethodIr {
    /// Method name.
    pub name: String,
    /// Input fields, in encoding order.
    pub inputs: Vec<FieldIr>,
    /// Output identity rendered by the core.
    pub output_type_ident: String,
    /// Output registry ID.
    pub output: u32,
    /// Joined documentation.
    pub docs: String,
}

/// Extrinsic information needed for generated extension guards.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtrinsicIr {
    /// Extrinsic encoding version.
    pub version: u8,
    /// Signed extensions, in the exact runtime encoding order.
    pub signed_extensions: Vec<SignedExtensionIr>,
}

/// One signed extension's explicit and implied payload types.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedExtensionIr {
    /// Extension identifier.
    pub identifier: String,
    /// Explicit payload identity.
    pub type_ident: String,
    /// Explicit payload registry ID.
    pub type_id: u32,
    /// Implied payload identity.
    pub additional_signed_type_ident: String,
    /// Implied payload registry ID.
    pub additional_signed: u32,
}

impl Runtime {
    /// Build the serializable metadata IR without host I/O.
    ///
    /// Includes storage key identities, events, runtime API signatures, and
    /// signed extensions. Registry IDs are local to this runtime; public
    /// generated descriptors should use names and structural identities.
    pub fn metadata_ir(&self) -> Result<MetadataIr, CoreError> {
        MetadataIr::from_runtime(self)
    }
}

fn join_docs(docs: &[String]) -> String {
    docs.iter()
        .map(|line| line.trim())
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

fn fields(
    runtime: &Runtime,
    fields: &[scale_info::Field<PortableForm>],
) -> Result<Vec<FieldIr>, CoreError> {
    fields
        .iter()
        .map(|field| {
            runtime.resolve(field.ty.id)?;
            Ok(FieldIr {
                name: field.name.clone().unwrap_or_default(),
                type_ident: runtime.type_ident(field.ty.id),
                type_id: field.ty.id,
            })
        })
        .collect()
}

fn variants<'a>(
    runtime: &'a Runtime,
    id: Option<u32>,
    kind: &str,
) -> Result<&'a [scale_info::Variant<PortableForm>], CoreError> {
    let Some(id) = id else {
        return Ok(&[]);
    };
    let TypeDef::Variant(variant) = &runtime.resolve(id)?.type_def else {
        return Err(CoreError::Codec(format!("{kind} type {id} is not an enum")));
    };
    Ok(&variant.variants)
}

// Validate every reachable reference before rendering identities. The core's
// type_ident deliberately substitutes scale_info::<id> for an unresolved ID;
// code generation must distinguish a valid structural fallback from a broken
// graph. Keep the walk iterative and cycle-safe for recursive runtime types.
fn validate_referenced_types(runtime: &Runtime) -> Result<(), CoreError> {
    let mut pending = Vec::new();
    for pallet in &runtime.pallets {
        pending.extend(
            [pallet.calls_type, pallet.events_type, pallet.errors_type]
                .into_iter()
                .flatten(),
        );
        pending.extend(pallet.constants.iter().map(|constant| constant.ty));
        for entry in pallet
            .storage
            .iter()
            .filter(|entry| !entry.name.contains(':'))
        {
            pending.push(entry.value_type);
            pending.extend(&entry.key_types);
        }
    }
    pending.extend(
        [
            runtime.extrinsic.address_type,
            runtime.extrinsic.call_type,
            runtime.extrinsic.signature_type,
            runtime.outer_event_type,
        ]
        .into_iter()
        .flatten(),
    );
    for extension in &runtime.extrinsic.signed_extensions {
        pending.extend([extension.ty, extension.additional_signed]);
    }
    for api in &runtime.apis {
        for method in &api.methods {
            pending.push(method.output);
            pending.extend(method.inputs.iter().map(|input| input.ty));
        }
    }
    let mut visited = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        let ty = runtime.resolve(id)?;
        pending.extend(
            ty.type_params
                .iter()
                .filter_map(|param| param.ty.map(|ty| ty.id)),
        );
        match &ty.type_def {
            TypeDef::Composite(composite) => {
                pending.extend(composite.fields.iter().map(|field| field.ty.id))
            }
            TypeDef::Variant(variant) => pending.extend(
                variant
                    .variants
                    .iter()
                    .flat_map(|variant| variant.fields.iter().map(|field| field.ty.id)),
            ),
            TypeDef::Sequence(sequence) => pending.push(sequence.type_param.id),
            TypeDef::Array(array) => pending.push(array.type_param.id),
            TypeDef::Tuple(tuple) => pending.extend(tuple.fields.iter().map(|field| field.id)),
            TypeDef::Primitive(_) => {}
            TypeDef::Compact(compact) => pending.push(compact.type_param.id),
            TypeDef::BitSequence(bits) => {
                pending.extend([bits.bit_store_type.id, bits.bit_order_type.id])
            }
        }
    }
    Ok(())
}

impl MetadataIr {
    /// Extract the core's parsed metadata, preserving Python's existing IR.
    ///
    /// V15 runtime API versions remain `None` until populated separately.
    /// Invalid referenced IDs and malformed enum descriptions fail closed.
    pub fn from_runtime(runtime: &Runtime) -> Result<Self, CoreError> {
        validate_referenced_types(runtime)?;
        let mut pallets = Vec::with_capacity(runtime.pallets.len());
        for pallet in &runtime.pallets {
            let calls = variants(runtime, pallet.calls_type, "call")?
                .iter()
                .map(|call| {
                    Ok(CallIr {
                        name: call.name.clone(),
                        index: call.index,
                        args: fields(runtime, &call.fields)?,
                        docs: join_docs(&call.docs),
                    })
                })
                .collect::<Result<Vec<_>, CoreError>>()?;
            let errors = variants(runtime, pallet.errors_type, "error")?
                .iter()
                .map(|error| ErrorIr {
                    name: error.name.clone(),
                    index: error.index,
                    docs: join_docs(&error.docs),
                })
                .collect();
            let events = variants(runtime, pallet.events_type, "event")?
                .iter()
                .map(|event| {
                    Ok(EventIr {
                        name: event.name.clone(),
                        index: event.index,
                        fields: fields(runtime, &event.fields)?,
                        docs: join_docs(&event.docs),
                    })
                })
                .collect::<Result<Vec<_>, CoreError>>()?;
            let mut storage = Vec::new();
            for entry in pallet
                .storage
                .iter()
                .filter(|entry| !entry.name.contains(':'))
            {
                if entry.hashers.len() != entry.key_types.len() {
                    return Err(CoreError::Codec(format!(
                        "storage {}.{} has {} hashers for {} keys",
                        pallet.name,
                        entry.name,
                        entry.hashers.len(),
                        entry.key_types.len()
                    )));
                }
                runtime.resolve(entry.value_type)?;
                for id in &entry.key_types {
                    runtime.resolve(*id)?;
                }
                storage.push(StorageIr {
                    name: entry.name.clone(),
                    value_type_ident: runtime.type_ident(entry.value_type),
                    prefix: entry.prefix.clone(),
                    modifier: entry.modifier.clone(),
                    hashers: entry.hashers.clone(),
                    key_type_idents: entry
                        .key_types
                        .iter()
                        .map(|id| runtime.type_ident(*id))
                        .collect(),
                    key_types: entry.key_types.clone(),
                    value_type: entry.value_type,
                    default_bytes: entry.default_bytes.clone(),
                });
            }
            let constant_details = pallet
                .constants
                .iter()
                .map(|constant| {
                    runtime.resolve(constant.ty)?;
                    Ok(ConstantIr {
                        name: constant.name.clone(),
                        type_ident: runtime.type_ident(constant.ty),
                        type_id: constant.ty,
                        value: constant.value.clone(),
                        docs: join_docs(&constant.docs),
                    })
                })
                .collect::<Result<Vec<_>, CoreError>>()?;
            pallets.push(PalletIr {
                name: pallet.name.clone(),
                index: pallet.index,
                calls,
                errors,
                storage,
                constants: pallet
                    .constants
                    .iter()
                    .map(|constant| constant.name.clone())
                    .collect(),
                constant_details,
                events,
            });
        }
        let runtime_apis = runtime
            .apis
            .iter()
            .map(|api| {
                let method_signatures = api
                    .methods
                    .iter()
                    .map(|method| {
                        runtime.resolve(method.output)?;
                        let inputs = method
                            .inputs
                            .iter()
                            .map(|input| {
                                runtime.resolve(input.ty)?;
                                Ok(FieldIr {
                                    name: input.name.clone(),
                                    type_ident: runtime.type_ident(input.ty),
                                    type_id: input.ty,
                                })
                            })
                            .collect::<Result<Vec<_>, CoreError>>()?;
                        Ok(RuntimeApiMethodIr {
                            name: method.name.clone(),
                            inputs,
                            output_type_ident: runtime.type_ident(method.output),
                            output: method.output,
                            docs: join_docs(&method.docs),
                        })
                    })
                    .collect::<Result<Vec<_>, CoreError>>()?;
                Ok(RuntimeApiIr {
                    name: api.name.clone(),
                    methods: api
                        .methods
                        .iter()
                        .map(|method| method.name.clone())
                        .collect(),
                    method_signatures,
                    version: None,
                })
            })
            .collect::<Result<Vec<_>, CoreError>>()?;
        let signed_extensions = runtime
            .extrinsic
            .signed_extensions
            .iter()
            .map(|extension| {
                runtime.resolve(extension.ty)?;
                runtime.resolve(extension.additional_signed)?;
                Ok(SignedExtensionIr {
                    identifier: extension.identifier.clone(),
                    type_ident: runtime.type_ident(extension.ty),
                    type_id: extension.ty,
                    additional_signed_type_ident: runtime.type_ident(extension.additional_signed),
                    additional_signed: extension.additional_signed,
                })
            })
            .collect::<Result<Vec<_>, CoreError>>()?;
        Ok(Self {
            spec_version: runtime.spec_version,
            pallets,
            runtime_apis,
            extrinsic: ExtrinsicIr {
                version: runtime.extrinsic.version,
                signed_extensions,
            },
        })
    }

    /// Populate known runtime API versions by trait name.
    ///
    /// The caller correlates advertised API IDs or supplies names from V16 at
    /// the same pinned runtime. Unlisted APIs remain unknown. Unknown trait
    /// names and conflicting replacements are errors, and an error applies
    /// none of the supplied changes.
    pub fn set_api_versions(&mut self, versions: &BTreeMap<String, u32>) -> Result<(), CoreError> {
        for (name, version) in versions {
            let api = self
                .runtime_apis
                .iter()
                .find(|api| &api.name == name)
                .ok_or_else(|| CoreError::NotInRuntime(format!("runtime API {name}")))?;
            if api.version.is_some_and(|previous| previous != *version) {
                return Err(CoreError::Codec(format!(
                    "conflicting runtime API version for {name}"
                )));
            }
        }
        for api in &mut self.runtime_apis {
            if let Some(version) = versions.get(&api.name) {
                api.version = Some(*version);
            }
        }
        Ok(())
    }

    /// Populate API versions from already-decoded V16 metadata fetched at the
    /// same block as this IR. Method names must match the V15 descriptions.
    ///
    /// The caller owns block/spec pinning; this helper does no I/O and does not
    /// infer a version from a method's presence or from the chain spec number.
    pub fn set_v16_api_versions(
        &mut self,
        metadata: &frame_metadata::v16::RuntimeMetadataV16,
    ) -> Result<(), CoreError> {
        let mut versions = BTreeMap::new();
        for api in &metadata.apis {
            let existing = self
                .runtime_apis
                .iter()
                .find(|existing| existing.name == api.name)
                .ok_or_else(|| CoreError::NotInRuntime(format!("runtime API {}", api.name)))?;
            let mut expected = existing.methods.clone();
            let mut actual: Vec<_> = api
                .methods
                .iter()
                .map(|method| method.name.clone())
                .collect();
            expected.sort();
            actual.sort();
            if expected != actual {
                return Err(CoreError::Codec(format!(
                    "V15/V16 method mismatch for runtime API {}",
                    api.name
                )));
            }
            if versions.insert(api.name.clone(), api.version.0).is_some() {
                return Err(CoreError::Codec(format!(
                    "duplicate V16 runtime API {}",
                    api.name
                )));
            }
        }
        if versions.len() != self.runtime_apis.len() {
            return Err(CoreError::Codec("V15/V16 runtime API sets differ".into()));
        }
        self.set_api_versions(&versions)
    }
}
