//! Portable SDK policy. Bindings only adapt values; decisions live here.
//!
//! Integer amounts cross the language boundary as exact decimal strings. This
//! module performs no I/O and is shared by native Node, Python, and browser WASM.

// serde_json::Value indexing is a fallible lookup returning Null, not a slice
// access. Actual byte slices below are bounded by their decoders before use.
#![allow(clippy::indexing_slicing)]

use serde::Serialize;
use serde_json::Value;

pub(crate) mod financial;
pub(crate) mod numeric;
pub(crate) mod protected;
mod saved_call;
mod shielding;
pub(crate) mod transaction;
pub(crate) mod upgrade;
pub(crate) mod validation;

#[derive(Debug, Clone, Serialize)]
pub struct PolicyError {
    pub kind: &'static str,
    pub message: String,
}

impl PolicyError {
    pub(crate) fn type_error(message: impl Into<String>) -> Self {
        Self {
            kind: "type",
            message: message.into(),
        }
    }
    pub(crate) fn range_error(message: impl Into<String>) -> Self {
        Self {
            kind: "range",
            message: message.into(),
        }
    }
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self {
            kind: "invalid",
            message: message.into(),
        }
    }
}

impl std::fmt::Display for PolicyError {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        output.write_str(&self.message)
    }
}
impl std::error::Error for PolicyError {}

pub fn evaluate(operation: &str, input: Value) -> Result<Value, PolicyError> {
    match operation.split('.').next() {
        Some("financial") => financial::evaluate(operation, input),
        Some("protected") => protected::evaluate(operation, input),
        Some("transaction") => transaction::evaluate(operation, input),
        Some("upgrade") => upgrade::evaluate(operation, input),
        Some("validation") => validation::evaluate(operation, input),
        _ => Err(PolicyError::invalid(format!(
            "unknown SDK policy operation {operation}"
        ))),
    }
}

/// Versioned, lossless JSON boundary for the three deliberately thin bindings.
pub fn evaluate_json(operation: &str, input: &str) -> String {
    let outcome = serde_json::from_str(input)
        .map_err(|error| PolicyError::type_error(format!("invalid policy input: {error}")))
        .and_then(|input| evaluate(operation, input));
    match outcome {
        Ok(value) => serde_json::json!({"version": 1, "value": value}).to_string(),
        Err(error) => serde_json::json!({"version": 1, "error": error}).to_string(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;

    #[test]
    fn shared_binding_corpus_has_exact_results_and_errors() {
        let cases: Vec<Value> =
            serde_json::from_str(include_str!("../../tests/fixtures/sdk-policy.json")).unwrap();
        for case in cases {
            let actual: Value = serde_json::from_str(&evaluate_json(
                case["operation"].as_str().unwrap(),
                &case["input"].to_string(),
            ))
            .unwrap();
            let key = if case.get("error").is_some() {
                "error"
            } else {
                "value"
            };
            assert_eq!(actual["version"], 1, "{}", case["name"]);
            assert_eq!(actual[key], case[key], "{}: {actual}", case["name"]);
        }
    }

    #[test]
    fn invalid_input_and_unknown_operations_return_typed_errors() {
        let malformed: Value =
            serde_json::from_str(&evaluate_json("validation.constants", "{")).unwrap();
        assert_eq!(malformed["error"]["kind"], "type");
        let unknown: Value =
            serde_json::from_str(&evaluate_json("absent.operation", "{}")).unwrap();
        assert_eq!(unknown["error"]["kind"], "invalid");
    }
}
