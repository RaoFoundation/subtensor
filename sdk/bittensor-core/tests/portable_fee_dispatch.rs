//! Python receipt semantics over the actual pinned portable runtime.

use bittensor_core::{codec::value::Value, runtime::Runtime};
use bittensor_core::{
    codec::value::ValueExt,
    protocol::{dispatch_error, extrinsic_lookup, fee, runtime_api},
};
use serde_json::json;
use std::error::Error;

fn runtime() -> Result<Runtime, Box<dyn Error>> {
    Ok(Runtime::parse(
        include_bytes!("fixtures/finney-v478.v15.scale"),
        478,
        1,
        42,
    )?)
}
#[test]
fn system_error_wrappers_use_exact_extracted_docs_and_python_repr() -> Result<(), Box<dyn Error>> {
    let runtime = runtime()?;
    let pallet = runtime.pallet("Balances").ok_or("Balances absent")?;
    let errors = runtime.resolve(pallet.errors_type.ok_or("Balances errors absent")?)?;
    let scale_info::TypeDef::Variant(errors) = &errors.type_def else {
        return Err("Balances error type not variant".into());
    };
    let variant = errors.variants.first().ok_or("Balances errors empty")?;
    let module = Value::record(vec![(
        "Module".into(),
        Value::record(vec![
            ("index".into(), Value::Int(i128::from(pallet.index))),
            ("error".into(), Value::Bytes(vec![variant.index, 0, 0, 0])),
        ]),
    )]);
    let module = dispatch_error::decode(&runtime, &module)?.ok_or("missing metadata error")?;
    assert_eq!(module.pallet.as_deref(), Some("Balances"));
    assert_eq!(module.name, variant.name);
    assert_eq!(
        module.docs,
        Value::List(variant.docs.iter().map(Value::str).collect())
    );
    let token = Value::record(vec![("Token".into(), Value::str("FundsUnavailable"))]);
    let error = dispatch_error::decode(&runtime, &token)?.ok_or("missing token error")?;
    assert_eq!(error.name, "FundsUnavailable");
    assert!(error
        .docs
        .as_str()
        .is_some_and(|docs| docs.contains("spendable balance")));
    let direct = dispatch_error::decode(&runtime, &Value::str("BadOrigin"))?
        .ok_or("missing direct error")?;
    assert_eq!(direct.docs, Value::str("BadOrigin"));
    let arithmetic = Value::record(vec![(
        "Arithmetic".into(),
        Value::record(vec![("Overflow".into(), Value::Null)]),
    )]);
    assert_eq!(
        dispatch_error::decode(&runtime, &arithmetic)?
            .ok_or("missing arithmetic error")?
            .name,
        "Overflow"
    );
    let unrecognized = Value::record(vec![(
        "Unknown".into(),
        Value::Tuple(vec![
            Value::Null,
            Value::str("\u{200b}"),
            Value::Bytes(vec![0, 39]),
        ]),
    )]);
    assert_eq!(
        dispatch_error::decode(&runtime, &unrecognized)?
            .ok_or("missing unknown error")?
            .docs,
        Value::str("(None, '\\u200b', b\"\\x00'\")")
    );
    assert_eq!(
        dispatch_error::normalize_module_error(&Value::record(vec![(
            "Module".into(),
            Value::Tuple(vec![Value::Int(5), Value::Int(7)])
        )]))?,
        Some((5, 7))
    );
    assert_eq!(
        dispatch_error::normalize_module_error(&Value::record(vec![(
            "Module".into(),
            Value::record(vec![
                ("index".into(), Value::Int(5)),
                ("error".into(), Value::str("0x07010203"))
            ])
        )]))?,
        Some((5, 7))
    );
    assert!(dispatch_error::decode(
        &runtime,
        &Value::record(vec![(
            "Module".into(),
            Value::record(vec![
                ("index".into(), Value::Int(255)),
                ("error".into(), Value::str("0x00000000"))
            ])
        )])
    )
    .is_err());
    Ok(())
}

#[test]
fn result_message_preserves_python_truthiness_and_shield_reasons() -> Result<(), Box<dyn Error>> {
    let mut error = dispatch_error::DispatchErrorInfo {
        kind: dispatch_error::DispatchErrorKind::MevShield,
        pallet: None,
        name: "DecryptionFailed".into(),
        docs: Value::Bytes(vec![0, 255]),
    };
    assert_eq!(error.result_message()?, "b'\\x00\\xff'");
    error.docs = Value::record(vec![(
        "reason".into(),
        Value::Tuple(vec![Value::Bool(false), Value::str("\u{200b}")]),
    )]);
    assert_eq!(error.result_message()?, "{'reason': (False, '\\u200b')}");
    error.docs = Value::List(vec![Value::str("first"), Value::str("second")]);
    assert_eq!(error.result_message()?, "first second");
    for docs in [
        Value::Null,
        Value::Bool(false),
        Value::Int(0),
        Value::Uint(0),
        Value::U256([0; 32]),
        Value::str(""),
        Value::Bytes(vec![]),
        Value::List(vec![]),
        Value::Tuple(vec![]),
        Value::Dict(vec![]),
    ] {
        error.docs = docs;
        assert_eq!(error.result_message()?, "DecryptionFailed");
    }
    error.docs = Value::Bool(true);
    assert_eq!(error.result_message()?, "True");
    error.docs = Value::List(vec![Value::str("")]);
    assert_eq!(error.result_message()?, "");
    error.docs = Value::List(vec![Value::Int(1)]);
    assert!(error.result_message().is_err());
    error.name.clear();
    error.docs = Value::List(vec![]);
    assert_eq!(
        error.result_message()?,
        "{'type': 'MevShield', 'name': '', 'docs': []}"
    );
    Ok(())
}

#[test]
fn immortal_fee_probe_and_runtime_native_fee_width() -> Result<(), Box<dyn Error>> {
    let runtime = runtime()?;
    let call = runtime.compose_call(
        "Balances",
        "transfer_keep_alive",
        &Value::record(vec![
            ("dest".into(), Value::Bytes(vec![7; 32])),
            ("value".into(), Value::Int(1)),
        ]),
    )?;
    let xt = fee::probe(&runtime, &call, [7; 32], 0, 77, [8; 32])?;
    let decoded = runtime.decode_extrinsic(&xt, true)?;
    assert_eq!(decoded.field("era").and_then(Value::as_str), Some("00"));
    assert_eq!(decoded.field("nonce").and_then(Value::as_u128), Some(77));
    assert_eq!(
        decoded
            .field("mode")
            .and_then(|mode| mode.field("mode"))
            .and_then(Value::as_str),
        Some("Disabled")
    );
    assert_eq!(
        decoded
            .field("signature")
            .and_then(|signature| signature.variant("Ed25519"))
            .and_then(Value::value_bytes),
        Some(vec![0; 64])
    );
    let request = fee::request(&xt, None)?;
    assert_eq!(request.method, "state_call");
    assert_eq!(
        request.params.get(0).ok_or("fee argument omitted")?,
        "TransactionPaymentApi_query_info"
    );
    let mut args = xt.clone();
    args.extend_from_slice(&u32::try_from(xt.len())?.to_le_bytes());
    assert_eq!(
        request.params.get(1).ok_or("fee argument omitted")?,
        &serde_json::Value::String(format!("0x{}", hex::encode(args)))
    );
    assert!(request
        .params
        .get(2)
        .ok_or("fee argument omitted")?
        .is_null());
    let output =
        runtime_api::runtime_api_method(&runtime, "TransactionPaymentApi", "query_info")?.output;
    let value = Value::record(vec![
        ("partial_fee".into(), Value::Int(i128::from(u64::MAX))),
        ("class".into(), Value::str("Normal")),
        (
            "weight".into(),
            Value::record(vec![
                ("ref_time".into(), Value::Int(42)),
                ("proof_size".into(), Value::Int(9)),
            ]),
        ),
    ]);
    let mut encoded = Vec::new();
    runtime.encode_id(output, &value, &mut encoded)?;
    assert!(encoded.ends_with(&u64::MAX.to_le_bytes()));
    let estimate = fee::decode_bytes(&runtime, &encoded)?;
    assert_eq!(estimate.partial_fee, u64::MAX);
    assert_eq!(
        estimate.weight,
        value.field("weight").ok_or("weight missing")?.clone()
    );
    encoded.push(0);
    assert!(fee::decode_bytes(&runtime, &encoded).is_err());
    Ok(())
}

#[test]
fn lookup_hashes_bytes_and_never_invents_index_for_absent_hash() -> Result<(), Box<dyn Error>> {
    let runtime = runtime()?;
    let call = runtime.compose_call(
        "System",
        "remark",
        &Value::record(vec![("remark".into(), Value::Bytes(vec![0xAB, 0xCD]))]),
    )?;
    let xt = fee::probe(&runtime, &call, [7; 32], 0, 0, [8; 32])?;
    let decoded = runtime.decode_extrinsic(&xt, true)?;
    let hash: [u8; 32] = decoded
        .field("extrinsic_hash")
        .and_then(Value::value_bytes)
        .ok_or("hash missing")?
        .try_into()
        .map_err(|_| "hash width invalid")?;
    let block = json!({"block":{"header":{"number":"0x0c"},"extrinsics":["0x00",format!("0x{}",hex::encode_upper(&xt)),"malformed later entry"]}});
    assert_eq!(extrinsic_lookup::block_number(&block)?, 12);
    assert_eq!(extrinsic_lookup::find(&block, hash)?, Some(1));
    assert_eq!(
        extrinsic_lookup::find(&json!({"block":{"extrinsics":null}}), [0; 32])?,
        None
    );
    assert!(extrinsic_lookup::find(&block, [0; 32]).is_err());
    assert_eq!(
        extrinsic_lookup::missing_message("0x0123", "0x4567", 12),
        "extrinsic 0x0123 not found in block 0x4567 (#12); the serving node may not have imported it yet"
    );
    Ok(())
}

#[test]
fn result_message_uses_cpython312_unicode15() -> Result<(), Box<dyn std::error::Error>> {
    for (value, expected) in [
        ("\u{1c89}", "('\\u1c89',)"),
        ("\u{10d40}", "('\\U00010d40',)"),
        ("\u{10d41}", "('\\U00010d41',)"),
        ("\u{11bf1}", "('\\U00011bf1',)"),
        ("\u{116d1}", "('\\U000116d1',)"),
        ("\u{1e4f1}", "('\u{1e4f1}',)"),
        ("plain", "('plain',)"),
        (" ", "(' ',)"),
        ("\n", "('\\n',)"),
        ("\t", "('\\t',)"),
        ("\r", "('\\r',)"),
        ("\\", "('\\\\',)"),
        ("'", "(\"'\",)"),
        ("\"", "('\"',)"),
        ("'\"", "('\\'\"',)"),
        ("\u{85}", "('\\x85',)"),
        ("\u{a0}", "('\\xa0',)"),
        ("\u{200b}", "('\\u200b',)"),
        ("\u{2028}", "('\\u2028',)"),
        ("\u{e000}", "('\\ue000',)"),
        ("\u{10ffff}", "('\\U0010ffff',)"),
    ] {
        let error = dispatch_error::DispatchErrorInfo {
            kind: dispatch_error::DispatchErrorKind::System,
            pallet: None,
            name: "Probe".into(),
            docs: Value::Tuple(vec![Value::str(value)]),
        };
        assert_eq!(error.result_message()?, expected, "{value:?}");
    }
    Ok(())
}
