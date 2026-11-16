//! Recover sponsorship from committed calls rather than descriptive intents.
use super::{array, invalid, u64_value, Result};
use serde_json::{json, Value};

fn visit(
    call: &Value,
    payer: Option<&str>,
    guard: Option<u64>,
    depth: usize,
    out: &mut Vec<Value>,
) -> Result<Option<u64>> {
    if depth > 64 {
        return Err(invalid("committed registration nesting exceeds 64"));
    }
    let params = &call["params"];
    let operation = format!(
        "{}.{}",
        call["pallet"].as_str().unwrap_or(""),
        call["call"].as_str().unwrap_or("")
    );
    match operation.as_str() {
        "Utility.batch_all" => {
            let mut next = guard;
            for child in array(&params["calls"])? {
                next = visit(child, payer, next, depth + 1, out)?;
            }
            return Ok(next);
        }
        "HashedAccounts.check_registration_deposit" => {
            return Ok(Some(u64_value(&params["max_deposit"])?))
        }
        "HashedAccounts.register" => {
            let payer = payer.ok_or_else(|| {
                invalid("cannot determine committed registration sponsor through this wrapper")
            })?;
            let descriptor = params["descriptor"]
                .as_object()
                .filter(|value| value.len() == 1)
                .and_then(|value| value.iter().next())
                .filter(|(kind, _)| ["Classical", "MlDsa65", "Falcon"].contains(&kind.as_str()))
                .map(|(_, value)| value)
                .ok_or_else(|| invalid("invalid committed registration descriptor"))?;
            let encoded = super::super::transaction::evaluate(
                "transaction.descriptor_encode",
                json!({"value": descriptor}),
            )?;
            out.push(json!({"descriptor": encoded, "payer": payer, "maximum": guard.map(|value| value.to_string())}));
        }
        "Proxy.proxy" | "Proxy.proxy_announced" => {
            let real = params["real"]
                .as_str()
                .or_else(|| params["real"]["Id"].as_str());
            visit(&params["call"], real, None, depth + 1, out)?;
        }
        _ => {
            // Unknown origin-changing wrappers must not silently drop a
            // registration. Inspect children, refusing only if one registers.
            for field in ["call", "main", "fallback"] {
                if params[field].is_object() {
                    visit(&params[field], None, None, depth + 1, out)?;
                }
            }
            if let Some(children) = params["calls"].as_array() {
                for child in children {
                    visit(child, None, None, depth + 1, out)?;
                }
            }
        }
    }
    // Only an immediately preceding guard in the same atomic sequence bounds
    // this reserve; an intervening call could change the configurable amount.
    Ok(None)
}

pub(super) fn evaluate(operation: &str, input: &Value) -> Result<Value> {
    match operation {
        "registration.committed_calls" => {
            let call = super::super::protected::evaluate(
                "protected.decoded_call",
                json!({"call": input["call"]}),
            )?;
            let mut result = Vec::new();
            visit(&call, input["payer"].as_str(), None, 0, &mut result)?;
            Ok(json!(result))
        }
        "registration.committed_quote" => {
            let current = u64_value(&input["current"])?;
            let maximum = if input["maximum"].is_null() {
                if input["configurable"] == true {
                    return Err(invalid("committed registration has no deposit guard; cancel and prepare a bounded call"));
                }
                current
            } else {
                let maximum = u64_value(&input["maximum"])?;
                if current > maximum {
                    return Err(invalid("current registration deposit exceeds the committed guard; cancel and prepare again"));
                }
                maximum
            };
            Ok(json!(maximum.to_string()))
        }
        _ => Err(invalid("unknown committed registration policy")),
    }
}
