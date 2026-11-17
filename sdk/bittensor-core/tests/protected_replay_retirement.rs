use bittensor_core::policy::evaluate;
use serde_json::{json, Value};
use std::error::Error;

fn announcement(index: u64, status: &str) -> Value {
    json!({"index":index,"status":status,"controller_wallet":"alice","delegate_ss58":"controller","real_ss58":"pure","genesis_hash":"0x01","call_hash":"0x02"})
}

// PROTECTED_REGRESSION: absent or cancelled commitments cannot authorize another payment without explicit repeat approval, including older journals; changes require explicit human approval.
#[test]
fn retired_commitments_retain_replay_guards_in_queries_and_atomic_claims(
) -> Result<(), Box<dyn Error>> {
    for status in ["gone", "cancelled", "executed"] {
        let old = announcement(1, status);
        let next = announcement(2, "pending");
        assert_eq!(
            evaluate(
                "protected.repeat_commitment_index",
                json!({"rows":[old.clone(),next.clone()],"row":next})
            )?,
            json!(1)
        );
        let mut input = json!({"action":"claim","rows":[old,next],"wallet":"alice","index":2,"kind":"execute","genesis_hash":"0x01","now":1});
        assert!(evaluate("protected.journal_transition", input.clone()).is_err());
        input
            .as_object_mut()
            .ok_or("claim must be an object")?
            .insert("allow_repeat".into(), json!(true));
        assert!(evaluate("protected.journal_transition", input).is_ok());
    }
    for status in ["gone", "cancelled", "failed"] {
        let retirement = evaluate(
            "protected.announcement_retirement",
            json!({"status":status}),
        )?;
        let uncertainty = retirement
            .get("execution_uncertain")
            .ok_or("missing execution uncertainty")?;
        assert_eq!(uncertainty, &json!(true));
        let mut old = announcement(1, status);
        old.as_object_mut()
            .ok_or("announcement must be an object")?
            .insert("execution_uncertain".into(), uncertainty.clone());
        assert_eq!(
            evaluate(
                "protected.repeat_commitment_index",
                json!({"rows":[old],"row":announcement(2,"pending")})
            )?,
            json!(1)
        );
    }
    for status in ["gone", "cancelled"] {
        let mut old = announcement(1, status);
        old.as_object_mut()
            .ok_or("announcement must be an object")?
            .insert("execution_uncertain".into(), Value::Null);
        assert_eq!(
            evaluate(
                "protected.repeat_commitment_index",
                json!({"rows":[old],"row":announcement(2,"pending")})
            )?,
            json!(1)
        );
    }
    for uncertainty in [None, Some(Value::Null)] {
        let mut old = announcement(1, "failed");
        let fields = old
            .as_object_mut()
            .ok_or("announcement must be an object")?;
        fields.insert(
            "action_receipt".into(),
            json!({"status":"finalized","success":false}),
        );
        if let Some(uncertainty) = uncertainty {
            fields.insert("execution_uncertain".into(), uncertainty);
        }
        assert_eq!(
            evaluate(
                "protected.repeat_commitment_index",
                json!({"rows":[old.clone()],"row":announcement(2,"pending")})
            )?,
            json!(1)
        );
        assert!(evaluate("protected.journal_transition", json!({"action":"claim","rows":[old,announcement(2,"pending")],"wallet":"alice","index":2,"kind":"execute","genesis_hash":"0x01","now":1})).is_err());
    }
    Ok(())
}

// PROTECTED_REGRESSION: a definitely failed announcement must remain retryable without bypassing another uncertain payment; changes require explicit human approval.
#[test]
fn definite_announcement_failure_and_proven_nonexecution_do_not_block_retry(
) -> Result<(), Box<dyn Error>> {
    let mut resolved_action = announcement(1, "failed");
    let fields = resolved_action
        .as_object_mut()
        .ok_or("announcement must be an object")?;
    fields.insert(
        "action_receipt".into(),
        json!({"status":"finalized","success":false}),
    );
    fields.insert("execution_uncertain".into(), json!(false));
    for mut old in [
        announcement(1, "failed"),
        announcement(1, "cancelled"),
        resolved_action,
    ] {
        if old.get("status") == Some(&json!("cancelled")) {
            old.as_object_mut()
                .ok_or("announcement must be an object")?
                .insert("execution_uncertain".into(), json!(false));
        }
        let next = announcement(2, "pending");
        assert_eq!(
            evaluate(
                "protected.repeat_commitment_index",
                json!({"rows":[old.clone()],"row":next})
            )?,
            Value::Null
        );
        assert!(evaluate("protected.journal_transition", json!({"action":"claim","rows":[old,next],"wallet":"alice","index":2,"kind":"execute","genesis_hash":"0x01","now":1})).is_ok());
    }
    Ok(())
}
