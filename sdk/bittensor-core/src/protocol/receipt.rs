// Derived from RaoFoundation/subtensor sdk/python/bittensor/_transport/{extrinsics.py,utils/receipt.py} @ 850f2755bb05a3851f948489f0ab27b0645cebce (Apache-2.0); modified by rtao.
//!
//! Resolve ordered success, nested failures, shield errors, weight and native fees.

use super::dispatch_error::{self, DispatchErrorInfo, DispatchErrorKind};
use crate::codec::value::ValueExt;
use crate::{codec::value::Value, runtime::Runtime, CoreError};

#[path = "receipt_tables.rs"]
mod tables;

/// Python-compatible outcome for one extrinsic's triggered events.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    /// True only after an outer success with no resolved dispatch failure.
    pub success: bool,
    /// Last TransactionFeePaid amount, or checked sum of fallback deposits.
    pub total_fee_amount: u64,
    /// Last applicable WeightV1 or WeightV2 value; Null when absent.
    pub weight: Value,
    /// Exact Module, System or MevShield failure description.
    pub error: Option<DispatchErrorInfo>,
}

/// Filter complete decoded System.Events by ApplyExtrinsic and its index.
pub fn events_for_extrinsic(records: &Value, index: u32) -> Result<Vec<Value>, CoreError> {
    let Value::List(records) = records else {
        return Err(CoreError::Codec("System.Events is not a list".into()));
    };
    Ok(records
        .iter()
        .filter(|record| {
            record.field("phase").and_then(Value::as_str) == Some("ApplyExtrinsic")
                && record.field("extrinsic_idx").and_then(Value::as_u128) == Some(u128::from(index))
        })
        .cloned()
        .collect())
}

/// Select an extrinsic's events and resolve them with its parent-block runtime.
pub fn resolve(runtime: &Runtime, records: &Value, index: u32) -> Result<Outcome, CoreError> {
    resolve_outcome(runtime, &events_for_extrinsic(records, index)?)
}

fn parts(record: &Value) -> Result<(&str, &str, &Value), CoreError> {
    let event = record.field("event").unwrap_or(record);
    let required = |name: &str| {
        event
            .field(name)
            .ok_or_else(|| CoreError::Codec(format!("event omitted {name}")))
    };
    let module = required("module_id")?
        .as_str()
        .ok_or_else(|| CoreError::Codec("event module_id is not a string".into()))?;
    let event_name = required("event_id")?
        .as_str()
        .ok_or_else(|| CoreError::Codec("event event_id is not a string".into()))?;
    Ok((module, event_name, required("attributes")?))
}

fn required<'a>(attributes: &'a Value, name: &str) -> Result<&'a Value, CoreError> {
    attributes
        .field(name)
        .ok_or_else(|| CoreError::Codec(format!("event attributes omitted {name}")))
}
fn native_fee(value: &Value) -> Result<u64, CoreError> {
    value
        .as_u128()
        .and_then(|fee| u64::try_from(fee).ok())
        .ok_or_else(|| CoreError::Codec("event fee is not native u64 rao".into()))
}

/// First nested Err, with Utility's bare DispatchError supported separately.
pub fn nested_dispatch_error(events: &[Value]) -> Option<&Value> {
    for record in events {
        let Ok((module, event, attributes)) = parts(record) else {
            continue;
        };
        let Some((_, _, field)) = tables::NESTED_RESULT_EVENTS
            .iter()
            .find(|(candidate, name, _)| *candidate == module && *name == event)
        else {
            continue;
        };
        let result = if matches!(attributes, Value::Dict(_)) {
            attributes.field(field)
        } else {
            Some(attributes)
        };
        let Some(result) = result else {
            continue;
        };
        if let Some(error) = result.variant("Err") {
            return Some(error);
        }
        if module == "Utility" && !matches!(result, Value::Null) {
            return Some(result);
        }
    }
    None
}

fn shield(event: &str, reason: Value) -> DispatchErrorInfo {
    DispatchErrorInfo {
        kind: DispatchErrorKind::MevShield,
        pallet: None,
        name: event.into(),
        docs: reason,
    }
}

/// Port of Python's ordered `resolve_outcome`, including all six extracted wrappers.
///
/// No success event means false. FeePaid takes precedence over fallback
/// deposits; the last FeePaid wins. All amounts obey native checked-u64 rules.
pub fn resolve_outcome(runtime: &Runtime, events: &[Value]) -> Result<Outcome, CoreError> {
    let mut total_fee_amount = 0;
    let mut has_paid = false;
    for event in events {
        let (module, event, attributes) = parts(event)?;
        if module == "TransactionPayment" && event == "TransactionFeePaid" {
            total_fee_amount = native_fee(required(attributes, "actual_fee")?)?;
            has_paid = true;
        }
    }
    let mut possible_success = false;
    let mut success = false;
    let mut weight = Value::Null;
    let mut error = None;
    for event in events {
        let (module, event, attributes) = parts(event)?;
        if module == "System" && event == "ExtrinsicSuccess" {
            possible_success = true;
            weight = required(
                attributes.field("dispatch_info").unwrap_or(attributes),
                "weight",
            )?
            .clone();
        } else if (module == "System" && event == "ExtrinsicFailed")
            || (module == "MevShield"
                && matches!(
                    event,
                    "DecryptedRejected"
                        | "DecryptionFailed"
                        | "ExtrinsicDispatchFailed"
                        | "ExtrinsicDecodeFailed"
                        | "ExtrinsicExpired"
                ))
        {
            possible_success = false;
            success = false;
            if module == "System" {
                weight = required(required(attributes, "dispatch_info")?, "weight")?.clone();
                error = dispatch_error::decode(runtime, required(attributes, "dispatch_error")?)?;
            } else if event == "DecryptedRejected" {
                let reason = required(attributes, "reason")?;
                weight = required(required(reason, "post_info")?, "actual_weight")?.clone();
                error = dispatch_error::decode(runtime, required(reason, "error")?)?;
            } else if event == "ExtrinsicDispatchFailed" {
                error = dispatch_error::decode(runtime, required(attributes, "error")?)?;
            } else {
                error = Some(shield(
                    event,
                    attributes
                        .field("reason")
                        .cloned()
                        .unwrap_or_else(|| Value::str(event)),
                ));
            }
        } else if !has_paid {
            let fee = if module == "Treasury" && event == "Deposit" {
                Some(required(attributes, "value")?)
            } else if module == "Balances" && event == "Deposit" {
                Some(required(attributes, "amount")?)
            } else {
                None
            };
            if let Some(fee) = fee {
                total_fee_amount =
                    total_fee_amount
                        .checked_add(native_fee(fee)?)
                        .ok_or_else(|| {
                            CoreError::Codec("fallback deposit fee exceeds native u64 rao".into())
                        })?;
            }
        }
    }
    if let Some(inner) =
        nested_dispatch_error(events).filter(|_| possible_success && error.is_none())
    {
        possible_success = false;
        success = false;
        error = dispatch_error::decode(runtime, inner)?;
    }
    if possible_success && error.is_none() {
        success = true;
    }
    Ok(Outcome {
        success,
        total_fee_amount,
        weight,
        error,
    })
}
