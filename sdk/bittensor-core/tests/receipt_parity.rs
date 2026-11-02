//! Python receipt semantics over the actual pinned portable runtime.

use bittensor_core::{codec::value::Value, runtime::Runtime};
use bittensor_core::{
    codec::value::ValueExt,
    protocol::{dispatch_error, receipt},
};
use std::error::Error;

fn runtime() -> Result<Runtime, Box<dyn Error>> {
    Ok(Runtime::parse(
        include_bytes!("fixtures/finney-v478.v15.scale"),
        478,
        1,
        42,
    )?)
}
fn record(module: &str, event: &str, attributes: Value) -> Value {
    Value::record(vec![(
        "event".into(),
        Value::record(vec![
            ("module_id".into(), Value::str(module)),
            ("event_id".into(), Value::str(event)),
            ("attributes".into(), attributes),
        ]),
    )])
}
fn success() -> Value {
    record(
        "System",
        "ExtrinsicSuccess",
        Value::record(vec![(
            "dispatch_info".into(),
            Value::record(vec![(
                "weight".into(),
                Value::record(vec![
                    ("ref_time".into(), Value::Int(42)),
                    ("proof_size".into(), Value::Int(9)),
                ]),
            )]),
        )]),
    )
}

#[test]
fn every_nested_wrapper_overrides_outer_success() -> Result<(), Box<dyn Error>> {
    let runtime = runtime()?;
    for (module, event, field, bare) in [
        ("Sudo", "Sudid", "sudo_result", false),
        ("Sudo", "SudoAsDone", "sudo_result", false),
        ("Proxy", "ProxyExecuted", "result", false),
        ("Multisig", "MultisigExecuted", "result", false),
        ("Utility", "BatchInterrupted", "error", true),
        ("Utility", "ItemFailed", "error", true),
    ] {
        let error = if bare {
            Value::str("BadOrigin")
        } else {
            Value::record(vec![("Err".into(), Value::str("BadOrigin"))])
        };
        let events = [
            record(module, event, Value::record(vec![(field.into(), error)])),
            success(),
        ];
        let outcome = receipt::resolve_outcome(&runtime, &events)?;
        assert!(!outcome.success, "missed {module}.{event}");
        assert_eq!(
            outcome.error.as_ref().map(|error| error.name.as_str()),
            Some("BadOrigin")
        );
    }
    let ok = record(
        "Sudo",
        "Sudid",
        Value::record(vec![(
            "sudo_result".into(),
            Value::record(vec![("Ok".into(), Value::Null)]),
        )]),
    );
    assert!(receipt::resolve_outcome(&runtime, &[ok, success()])?.success);
    assert!(!receipt::resolve_outcome(&runtime, &[])?.success);
    Ok(())
}

#[test]
fn fee_paid_last_wins_and_prevents_native_fallback_overflow() -> Result<(), Box<dyn Error>> {
    let runtime = runtime()?;
    let deposit = record(
        "Balances",
        "Deposit",
        Value::record(vec![("amount".into(), Value::Int(i128::from(u64::MAX)))]),
    );
    let treasury = record(
        "Treasury",
        "Deposit",
        Value::record(vec![("value".into(), Value::Int(2))]),
    );
    assert!(receipt::resolve_outcome(&runtime, &[deposit.clone(), treasury.clone()]).is_err());
    let paid = |amount| {
        record(
            "TransactionPayment",
            "TransactionFeePaid",
            Value::record(vec![("actual_fee".into(), Value::Int(amount))]),
        )
    };
    assert_eq!(
        receipt::resolve_outcome(&runtime, &[deposit, paid(7), treasury, paid(9), success()])?
            .total_fee_amount,
        9
    );
    assert_eq!(
        receipt::resolve_outcome(
            &runtime,
            &[
                record(
                    "Balances",
                    "Deposit",
                    Value::record(vec![("amount".into(), Value::Int(3))])
                ),
                record(
                    "Treasury",
                    "Deposit",
                    Value::record(vec![("value".into(), Value::Int(5))])
                )
            ]
        )?
        .total_fee_amount,
        8
    );
    Ok(())
}

#[test]
fn all_shield_failure_names_weight_and_reason_shapes() -> Result<(), Box<dyn Error>> {
    let runtime = runtime()?;
    for name in [
        "DecryptionFailed",
        "ExtrinsicDecodeFailed",
        "ExtrinsicExpired",
    ] {
        let outcome = receipt::resolve_outcome(
            &runtime,
            &[
                success(),
                record(
                    "MevShield",
                    name,
                    Value::record(vec![("reason".into(), Value::Bytes(vec![0, 255]))]),
                ),
            ],
        )?;
        assert!(!outcome.success);
        let error = outcome.error.ok_or("shield diagnostic missing")?;
        assert_eq!(error.kind, dispatch_error::DispatchErrorKind::MevShield);
        assert_eq!(error.docs, Value::Bytes(vec![0, 255]));
    }
    for name in ["DecryptedRejected", "ExtrinsicDispatchFailed"] {
        let attributes = if name == "DecryptedRejected" {
            Value::record(vec![(
                "reason".into(),
                Value::record(vec![
                    (
                        "post_info".into(),
                        Value::record(vec![("actual_weight".into(), Value::Int(99))]),
                    ),
                    ("error".into(), Value::str("BadOrigin")),
                ]),
            )])
        } else {
            Value::record(vec![("error".into(), Value::str("BadOrigin"))])
        };
        let outcome = receipt::resolve_outcome(
            &runtime,
            &[success(), record("MevShield", name, attributes)],
        )?;
        assert!(!outcome.success);
        assert_eq!(
            outcome.error.as_ref().map(|error| error.name.as_str()),
            Some("BadOrigin")
        );
        if name == "DecryptedRejected" {
            assert_eq!(outcome.weight, Value::Int(99));
        }
    }
    Ok(())
}

#[test]
fn scale_decoded_sudid_failure_overrides_the_outer_success() -> Result<(), Box<dyn Error>> {
    let runtime = runtime()?;
    let raw_event = |pallet: &str, event: &str, attributes: Value| {
        Value::record(vec![
            (
                "phase".into(),
                Value::record(vec![("ApplyExtrinsic".into(), Value::Int(1))]),
            ),
            (
                "event".into(),
                Value::record(vec![(
                    pallet.into(),
                    Value::record(vec![(event.into(), attributes)]),
                )]),
            ),
            ("topics".into(), Value::List(vec![])),
        ])
    };
    let raw = Value::List(vec![
        raw_event(
            "Sudo",
            "Sudid",
            Value::record(vec![(
                "sudo_result".into(),
                Value::record(vec![("Err".into(), Value::str("BadOrigin"))]),
            )]),
        ),
        raw_event(
            "System",
            "ExtrinsicSuccess",
            Value::record(vec![(
                "dispatch_info".into(),
                Value::record(vec![
                    (
                        "weight".into(),
                        Value::record(vec![
                            ("ref_time".into(), Value::Int(42)),
                            ("proof_size".into(), Value::Int(9)),
                        ]),
                    ),
                    ("class".into(), Value::str("Normal")),
                    ("pays_fee".into(), Value::str("Yes")),
                ]),
            )]),
        ),
    ]);
    let info = bittensor_core::protocol::storage::storage_info(&runtime, "System", "Events")?;
    let mut encoded = Vec::new();
    runtime.encode_id(info.value_type, &raw, &mut encoded)?;
    let records =
        bittensor_core::protocol::storage::decode_storage_bytes(&runtime, info, Some(&encoded))?;
    let triggered = receipt::events_for_extrinsic(&records, 1)?;
    assert_eq!(triggered.len(), 2);
    let attributes = triggered
        .first()
        .and_then(|record| record.field("attributes"))
        .ok_or("Sudid attributes missing")?;
    assert!(attributes.field("result").is_none());
    assert!(attributes
        .field("sudo_result")
        .and_then(|result| result.variant("Err"))
        .is_some());
    // Pin the v478 false-success cause against these actual decoder attributes:
    // the old client tested Err on result or the complete attribute map.
    assert!(attributes.variant("Err").is_none());
    let outcome = receipt::resolve(&runtime, &records, 1)?;
    assert!(!outcome.success);
    assert_eq!(
        outcome.error.as_ref().map(|error| error.name.as_str()),
        Some("BadOrigin")
    );
    assert!(receipt::events_for_extrinsic(&records, 0)?.is_empty());
    Ok(())
}
