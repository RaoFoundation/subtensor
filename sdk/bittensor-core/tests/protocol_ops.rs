//! Sans-IO protocol behavior against a compact synthetic V15 runtime.

#![deny(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]

use std::{collections::BTreeMap, error::Error};

use bittensor_core::protocol::{metadata, runtime_api, storage};
use bittensor_core::{codec::value::Value, runtime::Runtime, CoreError};
use codec::Encode;
use frame_metadata::{v15, RuntimeMetadataPrefixed};
use scale_info::meta_type;
use serde_json::{json, Value as JsonValue};

#[derive(scale_info::TypeInfo)]
enum SyntheticOuter {
    Empty,
}

fn runtime() -> Result<Runtime, CoreError> {
    let _ = SyntheticOuter::Empty;
    let metadata = v15::RuntimeMetadataV15::new(
        vec![v15::PalletMetadata {
            name: "Example",
            index: 7,
            storage: Some(v15::PalletStorageMetadata {
                prefix: "LivePrefix",
                entries: vec![
                    v15::StorageEntryMetadata {
                        name: "Counter",
                        modifier: v15::StorageEntryModifier::Default,
                        ty: v15::StorageEntryType::Plain(meta_type::<u32>()),
                        default: 42u32.encode(),
                        docs: vec![],
                    },
                    v15::StorageEntryMetadata {
                        name: "MaybeCounter",
                        modifier: v15::StorageEntryModifier::Optional,
                        ty: v15::StorageEntryType::Plain(meta_type::<u32>()),
                        default: vec![],
                        docs: vec![],
                    },
                    v15::StorageEntryMetadata {
                        name: "Counters",
                        modifier: v15::StorageEntryModifier::Default,
                        ty: v15::StorageEntryType::Map {
                            hashers: vec![v15::StorageHasher::Identity],
                            key: meta_type::<u16>(),
                            value: meta_type::<u32>(),
                        },
                        default: 0u32.encode(),
                        docs: vec![],
                    },
                ],
            }),
            calls: None,
            event: None,
            constants: vec![],
            error: None,
            docs: vec![],
        }],
        v15::ExtrinsicMetadata {
            version: 4,
            address_ty: meta_type::<()>(),
            call_ty: meta_type::<SyntheticOuter>(),
            signature_ty: meta_type::<()>(),
            extra_ty: meta_type::<()>(),
            signed_extensions: vec![],
        },
        meta_type::<()>(),
        vec![v15::RuntimeApiMetadata {
            name: "ExampleApi",
            methods: vec![
                v15::RuntimeApiMethodMetadata {
                    name: "echo",
                    inputs: vec![
                        v15::RuntimeApiMethodParamMetadata {
                            name: "number",
                            ty: meta_type::<u16>(),
                        },
                        v15::RuntimeApiMethodParamMetadata {
                            name: "flag",
                            ty: meta_type::<bool>(),
                        },
                        v15::RuntimeApiMethodParamMetadata {
                            name: "payload",
                            ty: meta_type::<Vec<u8>>(),
                        },
                    ],
                    output: meta_type::<u32>(),
                    docs: vec![],
                },
                v15::RuntimeApiMethodMetadata {
                    name: "unit",
                    inputs: vec![],
                    output: meta_type::<()>(),
                    docs: vec![],
                },
            ],
            docs: vec![],
        }],
        v15::OuterEnums {
            call_enum_ty: meta_type::<SyntheticOuter>(),
            event_enum_ty: meta_type::<SyntheticOuter>(),
            error_enum_ty: meta_type::<SyntheticOuter>(),
        },
        v15::CustomMetadata {
            map: BTreeMap::new(),
        },
    );
    let prefixed: RuntimeMetadataPrefixed = metadata.into();
    Runtime::parse(&prefixed.encode(), 478, 1, 42)
}

fn hex_result(bytes: &[u8]) -> JsonValue {
    JsonValue::String(format!("0x{}", hex::encode(bytes)))
}

fn echo_params() -> Vec<Value> {
    vec![Value::Int(513), Value::Bool(true), Value::hex(&[9, 8, 7])]
}

#[test]
fn metadata_requests_preserve_version_and_optional_block_hash() {
    let request = metadata::v15_request(None);
    assert_eq!(request.method, "state_call");
    assert_eq!(
        request.params,
        json!(["Metadata_metadata_at_version", "0x0f000000"])
    );
    assert_eq!(
        metadata::v15_request(Some("0xblock")).params,
        json!(["Metadata_metadata_at_version", "0x0f000000", "0xblock"])
    );
    assert_eq!(metadata::legacy_request(None).method, "state_getMetadata");
    assert_eq!(metadata::legacy_request(None).params, json!([]));
    assert_eq!(
        metadata::legacy_request(Some("0xblock")).params,
        json!(["0xblock"])
    );
}

#[test]
fn metadata_absence_is_distinct_from_present_empty_metadata() -> Result<(), Box<dyn Error>> {
    let payload = vec![109, 101, 116, 97, 15];
    assert_eq!(
        metadata::decode_v15(&hex_result(&Some(payload.clone()).encode()))?,
        Some(payload)
    );
    assert_eq!(
        metadata::decode_v15(&hex_result(&Option::<Vec<u8>>::None.encode()))?,
        None
    );
    assert_eq!(
        metadata::decode_v15(&hex_result(&Some(Vec::<u8>::new()).encode()))?,
        Some(vec![])
    );
    Ok(())
}

#[test]
fn metadata_wrapper_round_trips_parseable_v15_bytes() -> Result<(), Box<dyn Error>> {
    let runtime = runtime()?;
    let response = hex_result(&Some(runtime.metadata_bytes.clone()).encode());
    let decoded = metadata::decode_v15(&response)?.ok_or("V15 unexpectedly absent")?;
    let parsed = Runtime::parse(&decoded, 478, 1, 42)?;
    assert!(parsed.is_v15);
    assert!(parsed.storage_entry("Example", "Counter").is_some());
    assert!(runtime_api::runtime_api_method(&parsed, "ExampleApi", "echo").is_ok());
    Ok(())
}

#[test]
fn metadata_wrapper_rejects_malformed_and_trailing_scale() {
    for bytes in [
        vec![],
        vec![2],
        vec![1],
        vec![1, 8, 99],
        vec![1, 3],
        vec![0, 0],
        vec![1, 4, 42, 0],
    ] {
        assert!(matches!(
            metadata::decode_v15(&hex_result(&bytes)),
            Err(CoreError::Codec(_))
        ));
    }
}

#[test]
fn metadata_rejects_wrong_json_and_invalid_hex() -> Result<(), Box<dyn Error>> {
    for result in [JsonValue::Null, json!(23), json!(["0x00"])] {
        assert!(matches!(
            metadata::decode_v15(&result),
            Err(CoreError::Rpc(_))
        ));
        assert!(matches!(
            metadata::decode_legacy(&result),
            Err(CoreError::Rpc(_))
        ));
    }
    for text in ["0x0", "0xzz", "no hex"] {
        assert!(matches!(
            metadata::decode_v15(&json!(text)),
            Err(CoreError::Codec(_))
        ));
        assert!(matches!(
            metadata::decode_legacy(&json!(text)),
            Err(CoreError::Codec(_))
        ));
    }
    assert_eq!(
        metadata::decode_legacy(&json!("0x6d6574610e"))?,
        b"meta\x0e"
    );
    Ok(())
}

#[test]
fn storage_lookup_borrows_live_entry_and_reports_missing_names() -> Result<(), Box<dyn Error>> {
    let runtime = runtime()?;
    let info = storage::storage_info(&runtime, "Example", "Counter")?;
    assert!(std::ptr::eq(
        info,
        runtime
            .storage_entry("Example", "Counter")
            .ok_or("fixture has no Counter")?
    ));
    assert!(matches!(
        storage::storage_info(&runtime, "Unknown", "Counter"),
        Err(CoreError::NotInRuntime(_))
    ));
    assert!(matches!(
        storage::storage_info(&runtime, "Example", "Unknown"),
        Err(CoreError::NotInRuntime(_))
    ));
    Ok(())
}

#[test]
fn storage_keys_use_live_prefix_types_and_hashers() -> Result<(), Box<dyn Error>> {
    let runtime = runtime()?;
    let info = storage::storage_info(&runtime, "Example", "Counters")?;
    let values = [Value::Int(513)];
    let key = runtime.storage_key(info, &values)?;
    let request = storage::storage_request(&runtime, info, &values, None)?;
    assert_eq!(request.method, "state_getStorage");
    assert_eq!(request.params, json!([format!("0x{}", hex::encode(&key))]));
    assert_eq!(key.get(32..), Some([1, 2].as_slice()));
    assert_eq!(
        storage::storage_request(&runtime, info, &values, Some("0xblock"))?.params,
        json!([format!("0x{}", hex::encode(key)), "0xblock"])
    );
    let mut changed = info.clone();
    changed.hashers = vec!["Twox64Concat".into()];
    assert_ne!(
        storage::storage_request(&runtime, &changed, &values, None)?.params,
        request.params
    );
    let mut changed = info.clone();
    changed.prefix = "UpgradedPrefix".into();
    assert_ne!(
        storage::storage_request(&runtime, &changed, &values, None)?.params,
        request.params
    );
    let mut changed = info.clone();
    changed.key_types = vec![storage::storage_info(&runtime, "Example", "Counter")?.value_type];
    assert_ne!(
        storage::storage_request(&runtime, &changed, &values, None)?.params,
        request.params
    );
    assert!(storage::storage_request(&runtime, info, &[Value::Int(65536)], None).is_err());
    assert!(
        storage::storage_request(&runtime, info, &[Value::Int(1), Value::Int(2)], None).is_err()
    );
    Ok(())
}

#[test]
fn storage_default_optional_and_present_values_follow_metadata() -> Result<(), Box<dyn Error>> {
    let runtime = runtime()?;
    let default = storage::storage_info(&runtime, "Example", "Counter")?;
    let optional = storage::storage_info(&runtime, "Example", "MaybeCounter")?;
    assert_eq!(
        storage::decode_storage(&runtime, default, None)?,
        Value::Int(42)
    );
    assert_eq!(
        storage::decode_storage(&runtime, optional, None)?,
        Value::Null
    );
    for info in [default, optional] {
        assert_eq!(
            storage::decode_storage(&runtime, info, Some("0x09000000"))?,
            Value::Int(9)
        );
        assert_eq!(
            storage::decode_storage_bytes(&runtime, info, Some(&9u32.encode()))?,
            Value::Int(9)
        );
        assert_eq!(
            storage::decode_result(&runtime, info, &json!("0x09000000"))?,
            Value::Int(9)
        );
    }
    assert_eq!(
        storage::decode_result(&runtime, default, &JsonValue::Null)?,
        Value::Int(42)
    );
    let mut invalid_optional = optional.clone();
    invalid_optional.value_type = u32::MAX;
    invalid_optional.default_bytes = vec![255];
    assert_eq!(
        storage::decode_storage_bytes(&runtime, &invalid_optional, None)?,
        Value::Null
    );
    Ok(())
}

#[test]
fn storage_decoding_rejects_malformed_present_and_default_bytes() -> Result<(), Box<dyn Error>> {
    let runtime = runtime()?;
    let info = storage::storage_info(&runtime, "Example", "Counter")?;
    for raw in ["0x", "0x010000", "0x0100000000", "0xgg"] {
        assert!(matches!(
            storage::decode_storage(&runtime, info, Some(raw)),
            Err(CoreError::Codec(_))
        ));
    }
    let mut invalid_default = info.clone();
    invalid_default.default_bytes.push(0);
    assert!(matches!(
        storage::decode_storage(&runtime, &invalid_default, None),
        Err(CoreError::Codec(_))
    ));
    assert!(matches!(
        storage::decode_result(&runtime, info, &json!(8)),
        Err(CoreError::Rpc(_))
    ));
    let optional = storage::storage_info(&runtime, "Example", "MaybeCounter")?;
    assert!(storage::decode_storage(&runtime, optional, Some("0x")).is_err());
    let mut upgraded = info.clone();
    upgraded.value_type = runtime_api::runtime_api_method(&runtime, "ExampleApi", "echo")?
        .inputs
        .first()
        .ok_or("fixture input missing")?
        .ty;
    assert_eq!(
        storage::decode_storage(&runtime, &upgraded, Some("0x0102"))?,
        Value::Int(513)
    );
    assert!(storage::decode_storage(&runtime, info, Some("0x0102")).is_err());
    Ok(())
}

#[test]
fn runtime_api_lookup_borrows_the_named_api_method() -> Result<(), Box<dyn Error>> {
    let runtime = runtime()?;
    let info = runtime_api::runtime_api_method(&runtime, "ExampleApi", "echo")?;
    let first = runtime.apis.first().ok_or("fixture API missing")?;
    assert!(std::ptr::eq(
        info,
        first.methods.first().ok_or("fixture method missing")?
    ));
    assert!(matches!(
        runtime_api::runtime_api_method(&runtime, "OtherApi", "echo"),
        Err(CoreError::NotInRuntime(_))
    ));
    assert!(matches!(
        runtime_api::runtime_api_method(&runtime, "ExampleApi", "other"),
        Err(CoreError::NotInRuntime(_))
    ));
    let mut runtime = runtime;
    runtime.apis.clear();
    runtime.is_v15 = false;
    assert!(matches!(
        runtime_api::runtime_api_method(&runtime, "ExampleApi", "echo"),
        Err(CoreError::NotInRuntime(_))
    ));
    Ok(())
}

#[test]
fn runtime_api_encodes_ordered_params_and_block_hash() -> Result<(), Box<dyn Error>> {
    let runtime = runtime()?;
    let info = runtime_api::runtime_api_method(&runtime, "ExampleApi", "echo")?;
    let params = echo_params();
    let mut expected = 513u16.encode();
    expected.extend(true.encode());
    expected.extend(vec![9u8, 8, 7].encode());
    assert_eq!(
        runtime_api::encode_params(&runtime, "ExampleApi", info, &params)?,
        expected
    );
    let mut upgraded = info.clone();
    let number = upgraded.inputs.first_mut().ok_or("fixture input missing")?;
    number.ty = info.output;
    assert_eq!(
        runtime_api::encode_params(&runtime, "ExampleApi", &upgraded, &params)?,
        vec![1, 2, 0, 0, 1, 12, 9, 8, 7]
    );
    let request = runtime_api::request(&runtime, "ExampleApi", "echo", &params, None)?;
    assert_eq!(request.method, "state_call");
    assert_eq!(
        request.params,
        json!(["ExampleApi_echo", "0x0102010c090807"])
    );
    assert_eq!(
        runtime_api::request(&runtime, "ExampleApi", "echo", &params, Some("0xblock"))?.params,
        json!(["ExampleApi_echo", "0x0102010c090807", "0xblock"])
    );
    assert_eq!(
        runtime_api::request(&runtime, "ExampleApi", "unit", &[], None)?.params,
        json!(["ExampleApi_unit", "0x"])
    );
    for bytes in [
        Value::Bytes(vec![255]),
        Value::hex(&[255]),
        Value::List(vec![Value::Int(255)]),
    ] {
        let params = [Value::Int(65535), Value::Bool(false), bytes];
        assert_eq!(
            runtime_api::encode_params(&runtime, "ExampleApi", info, &params)?,
            vec![255, 255, 0, 4, 255]
        );
    }
    Ok(())
}

#[test]
fn runtime_api_argument_errors_do_not_produce_requests() -> Result<(), Box<dyn Error>> {
    let runtime = runtime()?;
    let info = runtime_api::runtime_api_method(&runtime, "ExampleApi", "echo")?;
    for params in [vec![], vec![Value::Int(1)], vec![Value::Null; 4]] {
        assert!(matches!(
            runtime_api::encode_params(&runtime, "ExampleApi", info, &params),
            Err(CoreError::Codec(_))
        ));
        assert!(runtime_api::request(&runtime, "ExampleApi", "echo", &params, None).is_err());
    }
    for bad in [
        Value::Int(-1),
        Value::Int(65536),
        Value::Str("invalid".into()),
    ] {
        let params = [bad, Value::Bool(true), Value::hex(&[])];
        assert!(matches!(
            runtime_api::encode_params(&runtime, "ExampleApi", info, &params),
            Err(CoreError::Codec(_))
        ));
    }
    let invalid_bool = [
        Value::Int(513),
        Value::Str("invalid".into()),
        Value::hex(&[]),
    ];
    assert!(runtime_api::encode_params(&runtime, "ExampleApi", info, &invalid_bool).is_err());
    let invalid_bytes = [
        Value::Int(513),
        Value::Bool(true),
        Value::Str("0xgg".into()),
    ];
    assert!(runtime_api::encode_params(&runtime, "ExampleApi", info, &invalid_bytes).is_err());
    assert!(runtime_api::request(&runtime, "MissingApi", "echo", &echo_params(), None).is_err());
    Ok(())
}

#[test]
fn runtime_api_results_require_the_entire_output_type() -> Result<(), Box<dyn Error>> {
    let runtime = runtime()?;
    let info = runtime_api::runtime_api_method(&runtime, "ExampleApi", "echo")?;
    assert_eq!(
        runtime_api::decode_bytes(&runtime, info, &99u32.encode())?,
        Value::Int(99)
    );
    assert_eq!(
        runtime_api::decode_result(&runtime, info, &json!("0x63000000"))?,
        Value::Int(99)
    );
    for raw in ["0x", "0x630000", "0x6300000000", "0xgg"] {
        assert!(matches!(
            runtime_api::decode_result(&runtime, info, &json!(raw)),
            Err(CoreError::Codec(_))
        ));
    }
    assert!(matches!(
        runtime_api::decode_result(&runtime, info, &JsonValue::Null),
        Err(CoreError::Rpc(_))
    ));
    let unit = runtime_api::runtime_api_method(&runtime, "ExampleApi", "unit")?;
    assert_eq!(
        runtime_api::decode_bytes(&runtime, unit, &[])?,
        Value::Tuple(vec![])
    );
    assert!(runtime_api::decode_bytes(&runtime, unit, &[0]).is_err());
    let mut upgraded = info.clone();
    upgraded.output = info.inputs.first().ok_or("fixture input missing")?.ty;
    assert_eq!(
        runtime_api::decode_bytes(&runtime, &upgraded, &[1, 2])?,
        Value::Int(513)
    );
    assert!(runtime_api::decode_bytes(&runtime, info, &[1, 2]).is_err());
    Ok(())
}
