//! Verify the portable code-generation IR against explicit metadata shapes.
#![allow(clippy::indexing_slicing, clippy::unwrap_used)]

use std::collections::BTreeMap;

use bittensor_core::ir::MetadataIr;
use bittensor_core::runtime::Runtime;
use codec::{Decode, Encode};
use frame_metadata::{v15, RuntimeMetadataPrefixed};
use scale_info::{meta_type, TypeInfo};

#[allow(dead_code)]
#[derive(TypeInfo)]
enum Call {
    #[codec(index = 9)]
    Put {
        count: u16,
        enabled: bool,
    },
    Empty,
}

#[allow(dead_code)]
#[derive(TypeInfo)]
enum Event {
    #[codec(index = 7)]
    Changed {
        count: u16,
    },
    Positional(u32, bool),
}

#[allow(dead_code)]
#[derive(TypeInfo)]
enum Error {
    #[codec(index = 4)]
    Denied,
}

fn fixture() -> Runtime {
    let metadata = v15::RuntimeMetadataV15::new(
        vec![v15::PalletMetadata {
            name: "Example",
            index: 12,
            calls: Some(v15::PalletCallMetadata {
                ty: meta_type::<Call>(),
            }),
            event: Some(v15::PalletEventMetadata {
                ty: meta_type::<Event>(),
            }),
            error: Some(v15::PalletErrorMetadata {
                ty: meta_type::<Error>(),
            }),
            constants: vec![v15::PalletConstantMetadata {
                name: "Limit",
                ty: meta_type::<u32>(),
                value: 500u32.encode(),
                docs: vec!["  A limit. ", " Next line.  "],
            }],
            storage: Some(v15::PalletStorageMetadata {
                prefix: "ActualPrefix",
                entries: vec![
                    v15::StorageEntryMetadata {
                        name: "Rows",
                        modifier: v15::StorageEntryModifier::Default,
                        ty: v15::StorageEntryType::Map {
                            hashers: vec![
                                v15::StorageHasher::Identity,
                                v15::StorageHasher::Twox64Concat,
                            ],
                            key: meta_type::<(u16, bool)>(),
                            value: meta_type::<u32>(),
                        },
                        default: 42u32.encode(),
                        docs: vec![],
                    },
                    v15::StorageEntryMetadata {
                        name: ":__STORAGE_VERSION__:",
                        modifier: v15::StorageEntryModifier::Default,
                        ty: v15::StorageEntryType::Plain(meta_type::<u16>()),
                        default: 1u16.encode(),
                        docs: vec![],
                    },
                ],
            }),
            docs: vec![],
        }],
        v15::ExtrinsicMetadata {
            version: 4,
            address_ty: meta_type::<[u8; 32]>(),
            call_ty: meta_type::<Call>(),
            signature_ty: meta_type::<[u8; 64]>(),
            extra_ty: meta_type::<()>(),
            signed_extensions: vec![
                v15::SignedExtensionMetadata {
                    identifier: "First",
                    ty: meta_type::<u32>(),
                    additional_signed: meta_type::<()>(),
                },
                v15::SignedExtensionMetadata {
                    identifier: "Second",
                    ty: meta_type::<()>(),
                    additional_signed: meta_type::<u16>(),
                },
            ],
        },
        meta_type::<()>(),
        vec![v15::RuntimeApiMetadata {
            name: "ExampleApi",
            docs: vec![],
            methods: vec![v15::RuntimeApiMethodMetadata {
                name: "read",
                inputs: vec![v15::RuntimeApiMethodParamMetadata {
                    name: "key",
                    ty: meta_type::<u16>(),
                }],
                output: meta_type::<Option<u32>>(),
                docs: vec![" Read a row. ", "  Returns its value.  "],
            }],
        }],
        v15::OuterEnums {
            call_enum_ty: meta_type::<Call>(),
            event_enum_ty: meta_type::<Event>(),
            error_enum_ty: meta_type::<Error>(),
        },
        v15::CustomMetadata {
            map: BTreeMap::new(),
        },
    );
    Runtime::parse(
        &RuntimeMetadataPrefixed::from(metadata).encode(),
        478,
        1,
        42,
    )
    .unwrap()
}

#[test]
fn ir_keeps_python_fields_and_adds_key_event_and_api_identities() {
    let runtime = fixture();
    let ir = runtime.metadata_ir().unwrap();
    let pallet = &ir.pallets[0];
    assert_eq!(ir.spec_version, 478);
    assert_eq!((&pallet.name, pallet.index), (&"Example".to_owned(), 12));
    assert_eq!(pallet.calls[0].name, "Put");
    assert_eq!(pallet.calls[0].index, 9);
    assert_eq!(
        pallet.calls[0]
            .args
            .iter()
            .map(|arg| (arg.name.as_str(), arg.type_ident.as_str()))
            .collect::<Vec<_>>(),
        [("count", "u16"), ("enabled", "bool")]
    );
    assert_eq!(pallet.errors[0].index, 4);
    assert_eq!(pallet.errors[0].name, "Denied");
    assert_eq!(pallet.storage.len(), 1);
    let entry = &pallet.storage[0];
    assert_eq!(entry.name, "Rows");
    assert_eq!(entry.prefix, "ActualPrefix");
    assert_eq!(entry.hashers, ["Identity", "Twox64Concat"]);
    assert_eq!(entry.key_type_idents, ["u16", "bool"]);
    assert_eq!(entry.value_type_ident, "u32");
    assert_eq!(entry.modifier, "Default");
    assert_eq!(entry.default_bytes, 42u32.encode());
    assert_eq!(pallet.constants, ["Limit"]);
    assert_eq!(pallet.constant_details[0].type_ident, "u32");
    assert_eq!(pallet.constant_details[0].value, 500u32.encode());
    assert_eq!(pallet.constant_details[0].docs, "A limit. Next line.");
    assert_eq!(pallet.events[0].index, 7);
    assert_eq!(pallet.events[0].fields[0].name, "count");
    assert_eq!(pallet.events[1].fields[0].name, "");
    assert_eq!(pallet.events[1].fields[0].type_ident, "u32");
    let api = &ir.runtime_apis[0];
    assert_eq!(api.methods, ["read"]);
    assert_eq!(api.version, None);
    assert_eq!(api.method_signatures[0].inputs[0].type_ident, "u16");
    assert_eq!(api.method_signatures[0].output_type_ident, "Option<u32>");
    assert_eq!(
        api.method_signatures[0].docs,
        "Read a row. Returns its value."
    );
    assert_eq!(
        ir.extrinsic
            .signed_extensions
            .iter()
            .map(|extension| extension.identifier.as_str())
            .collect::<Vec<_>>(),
        ["First", "Second"]
    );
    let json = serde_json::to_value(&ir).unwrap();
    assert_eq!(
        json["pallets"][0]["constants"],
        serde_json::json!(["Limit"])
    );
    assert_eq!(
        json["runtime_apis"][0]["methods"],
        serde_json::json!(["read"])
    );
    assert_eq!(serde_json::from_value::<MetadataIr>(json).unwrap(), ir);
}

#[test]
fn invalid_metadata_references_and_shapes_fail_closed() {
    let mut runtime = fixture();
    runtime.pallets[0].storage[0].key_types.clear();
    assert!(MetadataIr::from_runtime(&runtime).is_err());
    let mut runtime = fixture();
    runtime.pallets[0].storage[0].value_type = u32::MAX;
    assert!(MetadataIr::from_runtime(&runtime).is_err());
    let mut runtime = fixture();
    runtime.pallets[0].calls_type = runtime.type_id_of("u32");
    assert!(MetadataIr::from_runtime(&runtime).is_err());
    let mut runtime = fixture();
    runtime.apis[0].methods[0].inputs[0].ty = u32::MAX;
    assert!(MetadataIr::from_runtime(&runtime).is_err());
    let mut runtime = fixture();
    runtime.extrinsic.signed_extensions[0].additional_signed = u32::MAX;
    assert!(MetadataIr::from_runtime(&runtime).is_err());
}

#[test]
fn api_versions_are_explicit_and_apply_atomically() {
    let mut ir = fixture().metadata_ir().unwrap();
    let unknown = BTreeMap::from([("ExampleApi".to_owned(), 2), ("Missing".to_owned(), 1)]);
    assert!(ir.set_api_versions(&unknown).is_err());
    assert_eq!(ir.runtime_apis[0].version, None);
    let versions = BTreeMap::from([("ExampleApi".to_owned(), 2)]);
    ir.set_api_versions(&versions).unwrap();
    ir.set_api_versions(&versions).unwrap();
    assert_eq!(ir.runtime_apis[0].version, Some(2));
    assert!(ir
        .set_api_versions(&BTreeMap::from([("ExampleApi".to_owned(), 3)]))
        .is_err());
    assert_eq!(ir.runtime_apis[0].version, Some(2));
}

#[test]
fn v14_ir_has_no_runtime_api_signatures_or_invented_versions() {
    let metadata = frame_metadata::v14::RuntimeMetadataV14::new(
        vec![],
        frame_metadata::v14::ExtrinsicMetadata {
            ty: meta_type::<()>(),
            version: 4,
            signed_extensions: vec![],
        },
        meta_type::<()>(),
    );
    let runtime =
        Runtime::parse(&RuntimeMetadataPrefixed::from(metadata).encode(), 1, 1, 42).unwrap();
    let ir = runtime.metadata_ir().unwrap();
    assert!(ir.runtime_apis.is_empty());
    assert!(ir.pallets.is_empty());
}

#[test]
fn pinned_golden_metadata_exports_every_real_storage_and_api_method() {
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("../../python/tests/fixtures/golden.json")).unwrap();
    let encoded = hex::decode(
        golden["metadata"]["v15_hex"]
            .as_str()
            .unwrap()
            .trim_start_matches("0x"),
    )
    .unwrap();
    let metadata = Option::<Vec<u8>>::decode(&mut encoded.as_slice())
        .unwrap()
        .unwrap();
    let runtime = Runtime::parse(&metadata, 461, 1, 42).unwrap();
    let ir = runtime.metadata_ir().unwrap();
    assert!(ir
        .pallets
        .iter()
        .any(|pallet| pallet.name == "SubtensorModule"));
    assert_eq!(ir.pallets.len(), runtime.pallets.len());
    for (pallet_ir, pallet) in ir.pallets.iter().zip(&runtime.pallets) {
        assert_eq!(
            pallet_ir.storage.len(),
            pallet
                .storage
                .iter()
                .filter(|entry| !entry.name.contains(':'))
                .count()
        );
        for storage in &pallet_ir.storage {
            assert_eq!(storage.hashers.len(), storage.key_type_idents.len());
            assert!(!storage.value_type_ident.starts_with("scale_info::"));
        }
    }
    for (api_ir, api) in ir.runtime_apis.iter().zip(&runtime.apis) {
        assert_eq!(api_ir.method_signatures.len(), api.methods.len());
        assert!(api_ir.version.is_none());
    }
}

fn v16_fixture(
    runtime: &Runtime,
) -> Result<frame_metadata::v16::RuntimeMetadataV16, Box<dyn std::error::Error>> {
    use frame_metadata::v16;
    let prefixed = RuntimeMetadataPrefixed::decode(&mut runtime.metadata_bytes.as_slice()).unwrap();
    let frame_metadata::RuntimeMetadata::V15(metadata) = prefixed.1 else {
        return Err("fixture must be V15".into());
    };
    Ok(v16::RuntimeMetadataV16 {
        types: metadata.types,
        pallets: vec![],
        extrinsic: v16::ExtrinsicMetadata {
            versions: vec![4],
            address_ty: metadata.extrinsic.address_ty,
            call_ty: metadata.extrinsic.call_ty,
            signature_ty: metadata.extrinsic.signature_ty,
            transaction_extensions_by_version: BTreeMap::new(),
            transaction_extensions: vec![],
        },
        apis: metadata
            .apis
            .into_iter()
            .map(|api| v16::RuntimeApiMetadata {
                name: api.name,
                docs: api.docs,
                version: 2.into(),
                deprecation_info: v16::ItemDeprecationInfo::NotDeprecated,
                methods: api
                    .methods
                    .into_iter()
                    .map(|method| v16::RuntimeApiMethodMetadata {
                        name: method.name,
                        docs: method.docs,
                        output: method.output,
                        inputs: method
                            .inputs
                            .into_iter()
                            .map(|input| v16::FunctionParamMetadata {
                                name: input.name,
                                ty: input.ty,
                            })
                            .collect(),
                        deprecation_info: v16::ItemDeprecationInfo::NotDeprecated,
                    })
                    .collect(),
            })
            .collect(),
        outer_enums: metadata.outer_enums,
        custom: metadata.custom,
    })
}

#[test]
fn v16_versions_require_the_same_api_and_method_sets() {
    let runtime = fixture();
    let metadata = v16_fixture(&runtime).unwrap();
    let mut ir = runtime.metadata_ir().unwrap();
    ir.set_v16_api_versions(&metadata).unwrap();
    assert_eq!(ir.runtime_apis[0].version, Some(2));
    let mut bad = metadata.clone();
    bad.apis[0].methods[0].name = "different".into();
    assert!(ir.set_v16_api_versions(&bad).is_err());
    let mut bad = metadata.clone();
    bad.apis[0].name = "OtherApi".into();
    assert!(ir.set_v16_api_versions(&bad).is_err());
    let mut bad = metadata.clone();
    bad.apis.clear();
    assert!(ir.set_v16_api_versions(&bad).is_err());
    let mut bad = metadata.clone();
    bad.apis.push(metadata.apis[0].clone());
    assert!(ir.set_v16_api_versions(&bad).is_err());
    assert_eq!(ir.runtime_apis[0].version, Some(2));
}

#[test]
fn dangling_nested_type_and_error_field_references_are_rejected(
) -> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = fixture();
    let output = runtime.apis[0].methods[0].output;
    let option = runtime
        .types
        .types
        .iter_mut()
        .find(|ty| ty.id == output)
        .unwrap();
    let scale_info::TypeDef::Variant(variant) = &mut option.ty.type_def else {
        return Err("fixture output must be an Option variant".into());
    };
    let some = variant
        .variants
        .iter_mut()
        .find(|variant| variant.name == "Some")
        .unwrap();
    some.fields[0].ty = u32::MAX.into();
    assert!(runtime.metadata_ir().is_err());

    let mut runtime = fixture();
    let error_id = runtime.pallets[0].errors_type.unwrap();
    let field = scale_info::Field::<scale_info::form::PortableForm> {
        name: Some("broken".into()),
        ty: u32::MAX.into(),
        type_name: None,
        docs: vec![],
    };
    let error = runtime
        .types
        .types
        .iter_mut()
        .find(|ty| ty.id == error_id)
        .unwrap();
    let scale_info::TypeDef::Variant(variant) = &mut error.ty.type_def else {
        return Err("fixture error must be a variant".into());
    };
    variant.variants[0].fields.push(field);
    assert!(runtime.metadata_ir().is_err());
    Ok(())
}

#[test]
fn recursive_type_graphs_terminate_without_rejecting_valid_references(
) -> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = fixture();
    let output = runtime.apis[0].methods[0].output;
    let option = runtime
        .types
        .types
        .iter_mut()
        .find(|ty| ty.id == output)
        .unwrap();
    let scale_info::TypeDef::Variant(variant) = &mut option.ty.type_def else {
        return Err("fixture output must be an Option variant".into());
    };
    let some = variant
        .variants
        .iter_mut()
        .find(|variant| variant.name == "Some")
        .unwrap();
    some.fields[0].ty = output.into();
    assert!(runtime.metadata_ir().is_ok());
    Ok(())
}
