use std::error::Error;

use bittensor_core::policy::evaluate;
use serde_json::json;

// PROTECTED_REGRESSION: runtime-priced burns and full coldkey migration must never bypass a caller's TAO spend cap; changes require explicit human approval.
#[test]
fn account_operations_require_uncapped_spending() -> Result<(), Box<dyn Error>> {
    for op in [
        "root_register",
        "swap_hotkey",
        "announce_coldkey_swap",
        "swap_coldkey_announced",
    ] {
        let metadata = evaluate("transaction.intent_metadata", json!({"op": op}))?;
        let spend = metadata.get("spend_rao").ok_or("missing spend metadata")?;
        assert_eq!(spend, "unbounded", "{op}");
        for maximum in ["0", "340282366920938463463374607431768211455"] {
            let policy = evaluate(
                "transaction.intent_policy",
                json!({"spend_rao": spend, "max_spend_rao": maximum, "max_fee_rao":"0", "fee_rao":"0"}),
            )?;
            assert_eq!(
                policy
                    .get("violations")
                    .and_then(|violations| violations.get(0))
                    .and_then(|violation| violation.get("code")),
                Some(&json!("unbounded")),
                "{op}"
            );
        }
        let batch = evaluate(
            "transaction.merge_intents",
            json!({"children": [metadata, {"spend_rao":"1"}]}),
        )?;
        assert_eq!(batch.get("spend_rao"), Some(&json!("unbounded")), "{op}");
    }
    for op in ["clear_coldkey_swap_announcement", "dispute_coldkey_swap"] {
        assert_eq!(
            evaluate("transaction.intent_metadata", json!({"op":op}))?.get("spend_rao"),
            Some(&serde_json::Value::Null)
        );
    }
    Ok(())
}

// PROTECTED_REGRESSION: native trusted constructors cannot bypass spend caps for burns or full coldkey migration; changes require explicit human approval.
#[cfg(feature = "host")]
#[test]
fn native_account_constructors_enforce_spend_caps() -> Result<(), Box<dyn Error>> {
    use bittensor_core::codec::Value;
    use bittensor_core::transaction::{IntentCall, Policy, Spend};

    let mut calls = vec![IntentCall::root_register("hotkey")];
    for (operation, params) in [
        (
            "SubtensorModule.root_register",
            vec![("hotkey".into(), Value::str("hotkey"))],
        ),
        (
            "SubtensorModule.swap_hotkey",
            vec![
                ("hotkey".into(), Value::str("hotkey")),
                ("new_hotkey".into(), Value::str("other")),
                ("netuid".into(), Value::Null),
            ],
        ),
        (
            "SubtensorModule.swap_hotkey",
            vec![
                ("hotkey".into(), Value::str("hotkey")),
                ("new_hotkey".into(), Value::str("other")),
                ("netuid".into(), Value::Uint(1)),
            ],
        ),
        (
            "SubtensorModule.announce_coldkey_swap_v2",
            vec![("new_coldkey".into(), Value::str("other"))],
        ),
        (
            "SubtensorModule.swap_coldkey_announced",
            vec![("new_coldkey".into(), Value::str("other"))],
        ),
    ] {
        calls.push(IntentCall::operation(operation, Value::record(params))?);
    }
    for call in calls {
        let attempted_override = call.clone().spend(Spend::None);
        for maximum in [0, u128::MAX] {
            let policy = Policy {
                max_spend_rao: Some(maximum),
                max_fee_rao: Some(0),
                ..Default::default()
            };
            for checked in [&call, &attempted_override] {
                assert_eq!(
                    policy.check(checked, Some(0)),
                    ["unbounded spend is not allowed while max_spend_rao is set"]
                );
            }
        }
        assert!(Policy::default().check(&call, Some(0)).is_empty());
    }
    Ok(())
}
