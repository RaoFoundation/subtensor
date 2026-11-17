//! Mandatory shielding follows every inspectable nested runtime call.
use super::{protected, PolicyError};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub(super) fn evaluate(input: &Value) -> Result<Value, PolicyError> {
    let call = protected::decoded_call(&input["call"], 0)?;
    let leaves = protected::evaluate("protected.call_leaves", json!({"call": call}))?;
    let operations = leaves
        .as_array()
        .ok_or_else(|| PolicyError::type_error("expected inspectable call leaves"))?
        .iter()
        .filter(|leaf| {
            leaf["pallet"] == "SubtensorModule"
                && matches!(
                    leaf["call"].as_str(),
                    Some("add_collateral" | "burned_register")
                )
        })
        .filter_map(|leaf| leaf["call"].as_str())
        .collect::<BTreeSet<_>>();
    Ok(json!({"required":!operations.is_empty(),"operations":operations}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registration() -> Value {
        json!({"pallet":"SubtensorModule","call":"burned_register","params":{"netuid":1,"hotkey":"hotkey"}})
    }

    // PROTECTED_REGRESSION: descriptor wrappers must not expose mandatory-shielded registration or collateral purchases; changes require explicit human approval.
    #[test]
    fn descriptor_shielding_checks_nested_dispatch_and_stored_calls() -> Result<(), PolicyError> {
        let child = registration();
        let wrappers = [
            child.clone(),
            json!({"pallet":"Utility","call":"batch_all","params":{"calls":[child]}}),
            json!({"pallet":"Multisig","call":"as_multi_safe","params":{"call":child}}),
            json!({"pallet":"Proxy","call":"proxy_announced","params":{"call":child}}),
            json!({"pallet":"Sudo","call":"sudo","params":{"call":child}}),
            json!({"pallet":"Crowdloan","call":"create","params":{"call":child}}),
            json!({"pallet":"Utility","call":"if_else","params":{"main":{"pallet":"System","call":"remark","params":{}},"fallback":child}}),
        ];
        for call in wrappers {
            assert_eq!(
                evaluate(&json!({"call":call}))?,
                json!({"required":true,"operations":["burned_register"]})
            );
        }
        assert_eq!(
            evaluate(
                &json!({"call":{"pallet":"SubtensorModule","call":"add_collateral","params":{}}})
            )?["required"],
            true
        );
        assert_eq!(
            evaluate(&json!({"call":{"pallet":"Balances","call":"transfer_all","params":{}}}))?
                ["required"],
            false
        );
        Ok(())
    }

    #[test]
    fn opaque_wrapped_calls_cannot_skip_shield_inspection() {
        assert!(evaluate(
            &json!({"call":{"pallet":"Multisig","call":"as_multi","params":{"call":"0x0102"}}})
        )
        .is_err());
    }
}
