use bittensor_core::policy::evaluate;
use serde_json::{json, Value};
use std::error::Error;

// PROTECTED_REGRESSION: replacing Gamma targets must not redirect emission outside a caller's subnet allowlist; changes require explicit human approval.
#[test]
fn gamma_replacement_requires_unrestricted_subnet_scope() -> Result<(), Box<dyn Error>> {
    for gamma in [json!({}), json!({"7": "0"}), json!({"7": "5%"})] {
        let metadata = evaluate(
            "transaction.intent_metadata",
            json!({"op":"set_subnet_emission_weights", "args":{"netuid":1,"gamma":gamma}}),
        )?;
        assert_eq!(
            metadata.get("affects_all_subnets"),
            Some(&Value::Bool(true))
        );
        let batch = evaluate(
            "transaction.merge_intents",
            json!({"children":[metadata.clone()]}),
        )?;
        for intent in [metadata, batch] {
            let mut input = intent.clone();
            input
                .as_object_mut()
                .ok_or("intent metadata must be an object")?
                .insert("allowed_netuids".into(), json!([1, 7]));
            let result = evaluate("transaction.intent_policy", input)?;
            assert!(result
                .get("violations")
                .and_then(Value::as_array)
                .ok_or("missing violations")?
                .iter()
                .any(|item| item.get("code") == Some(&json!("all_subnets"))));
            assert_eq!(
                evaluate("transaction.intent_policy", intent)?.get("violations"),
                Some(&json!([]))
            );
        }
    }
    Ok(())
}
