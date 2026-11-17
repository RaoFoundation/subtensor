//! Shared policy vectors run without host-only SDK features or unit-test modules.

use std::error::Error;

use bittensor_core::policy::{evaluate, evaluate_json};
use serde_json::{json, Value};

fn same_value(actual: &Value, expected: &Value) -> bool {
    if actual == expected {
        return true;
    }
    // JSON consumers equate 4.0 and 4. Restrict this representation adjustment
    // to safe numbers; full-width amounts retain exact string/value equality.
    match (actual, expected) {
        (Value::Number(left), Value::Number(right)) => left
            .as_f64()
            .zip(right.as_f64())
            .is_some_and(|(left, right)| {
                left.abs() <= 9_007_199_254_740_991.0
                    && right.abs() <= 9_007_199_254_740_991.0
                    && left.total_cmp(&right).is_eq()
            }),
        _ => false,
    }
}

fn check_corpus(source: &str, value_field: &str) -> Result<(), Box<dyn Error>> {
    let cases: Vec<Value> = serde_json::from_str(source)?;
    for case in cases {
        let operation = case
            .get("operation")
            .and_then(Value::as_str)
            .ok_or("policy vector is missing its operation")?;
        let input = case.get("input").ok_or("policy vector is missing input")?;
        let expected_error = case.get("error");
        let expected = expected_error
            .or_else(|| case.get(value_field))
            .ok_or("policy vector is missing its expected result")?;
        let field = if expected_error.is_some() {
            "error"
        } else {
            "value"
        };

        let (actual_field, actual) = match evaluate(operation, input.clone()) {
            Ok(value) => ("value", value),
            Err(error) => ("error", serde_json::to_value(error)?),
        };
        assert_eq!(actual_field, field, "{operation}: {case}");
        assert!(
            same_value(&actual, expected),
            "{operation}: {actual} != {expected}"
        );

        let response: Value = serde_json::from_str(&evaluate_json(operation, &input.to_string()))?;
        assert_eq!(response.get("version"), Some(&json!(1)), "{operation}");
        let encoded_result = response
            .get(field)
            .ok_or("policy response is missing its result")?;
        assert!(
            same_value(encoded_result, expected),
            "{operation}: {encoded_result} != {expected}"
        );
    }
    Ok(())
}

#[test]
fn shared_sdk_policy_corpus_is_portable() -> Result<(), Box<dyn Error>> {
    check_corpus(include_str!("fixtures/sdk-policy.json"), "value")
}

#[test]
fn financial_policy_corpus_is_portable() -> Result<(), Box<dyn Error>> {
    check_corpus(include_str!("fixtures/policy-financial.json"), "expected")
}
