use super::*;
const OWNER_CALLS: &[&str] = &[
    "SubtensorModule.set_weights",
    "SubtensorModule.set_mechanism_weights",
    "SubtensorModule.batch_set_weights",
    "SubtensorModule.commit_weights",
    "SubtensorModule.commit_mechanism_weights",
    "SubtensorModule.batch_commit_weights",
    "SubtensorModule.commit_crv3_mechanism_weights",
    "SubtensorModule.commit_timelocked_weights",
    "SubtensorModule.commit_timelocked_mechanism_weights",
    "SubtensorModule.reveal_weights",
    "SubtensorModule.reveal_mechanism_weights",
    "SubtensorModule.batch_reveal_weights",
    "SubtensorModule.serve_axon",
    "SubtensorModule.serve_axon_tls",
    "SubtensorModule.serve_prometheus",
    "SubtensorModule.associate_evm_key",
    "Commitments.set_commitment",
];
fn name(call: &Value) -> String {
    format!(
        "{}.{}",
        call["pallet"].as_str().unwrap_or(""),
        call["call"].as_str().unwrap_or("")
    )
}
fn decoded(value: &Value) -> Result<Value> {
    // Fee inspection historically accepts codec objects and tuple descriptors.
    // Adapt representation here without broadening trusted saved-call parsing.
    super::super::protected::decoded_call(&adapt(value, 0)?, 0)
}
fn adapt(value: &Value, depth: usize) -> Result<Value> {
    if depth > 192 {
        return Err(ty("cyclic call structure or nesting exceeds 64"));
    }
    if let Some(rows) = value.as_array() {
        if rows.len() >= 2
            && rows[0].is_string()
            && rows[1].is_string()
            && rows
                .get(2)
                .is_none_or(|params| params.is_null() || params.is_object())
        {
            return Ok(
                json!({"pallet": rows[0], "call": rows[1], "params": adapt(rows.get(2).filter(|params| !params.is_null()).unwrap_or(&json!({})), depth+1)?}),
            );
        }
        return rows
            .iter()
            .map(|row| adapt(row, depth + 1))
            .collect::<Result<Vec<_>>>()
            .map(Value::Array);
    }
    let Some(row) = value.as_object() else {
        return Ok(value.clone());
    };
    let pallet = row
        .get("pallet")
        .or_else(|| row.get("module"))
        .or_else(|| row.get("call_module"));
    let call = row
        .get("call")
        .or_else(|| row.get("fn"))
        .or_else(|| row.get("function"))
        .or_else(|| row.get("call_function"));
    if let (Some(pallet), Some(call)) = (pallet, call) {
        if pallet.is_string() && call.is_string() && !row.contains_key("call_args") {
            return Ok(
                json!({"pallet":pallet,"call":call,"params":adapt(row.get("params").unwrap_or(&json!({})),depth+1)?}),
            );
        }
    }
    if row.len() == 1 {
        if let Some(inner) = row
            .get("value")
            .filter(|value| value.is_object() || value.is_array())
        {
            return adapt(inner, depth + 1);
        }
    }
    row.iter()
        .map(|(key, value)| Ok((key.clone(), adapt(value, depth + 1)?)))
        .collect::<Result<Map<_, _>>>()
        .map(Value::Object)
}
fn leaves(value: &Value, depth: usize) -> Result<Vec<Value>> {
    if depth > 64 {
        return Err(ty("cyclic call structure or nesting exceeds 64"));
    }
    let call = match decoded(value) {
        Ok(call) => call,
        Err(_) => return Ok(vec![]),
    };
    let name = name(&call);
    let children =
        if ["Utility.batch", "Utility.batch_all", "Utility.force_batch"].contains(&name.as_str()) {
            call["params"]["calls"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        } else if [
            "Proxy.proxy",
            "Proxy.proxy_announced",
            "Multisig.as_multi",
            "Multisig.as_multi_safe",
            "Multisig.as_multi_threshold_1",
            "Multisig.as_multi_threshold_1_safe",
            "Utility.as_derivative",
            "Utility.as_safe_derivative",
            "Utility.as_protected_derivative",
        ]
        .contains(&name.as_str())
        {
            if call["params"]["call"].is_null() {
                vec![]
            } else {
                vec![call["params"]["call"].clone()]
            }
        } else {
            return Ok(vec![json!([call["pallet"], call["call"]])]);
        };
    let mut out = vec![];
    for child in children {
        out.extend(leaves(&child, depth + 1)?);
    }
    Ok(out)
}
fn proxy(call: &Value) -> Option<(Value, Value)> {
    (name(call) == "Proxy.proxy").then(|| {
        (
            call["params"]["real"].clone(),
            call["params"]["call"].clone(),
        )
    })
}
fn request(kind: &str, pallet: &str, item: &str, params: Value) -> Value {
    let key = json!([kind, pallet, item, params]).to_string();
    json!({"kind":kind,"pallet":pallet,"item":item,"params":params,"key":key})
}
fn exists(state: &Value, query: &Value) -> Result<Option<bool>> {
    let value = &state[query["key"].as_str().unwrap_or("")];
    if value.is_null() {
        return Ok(None);
    }
    let count = integer(value)?;
    if count != 0 && count != 1 {
        return Err(invalid("invalid storage-key count"));
    }
    Ok(Some(count == 1))
}
fn result(address: Value, drops: bool) -> Value {
    json!({"kind":"result","address":address,"dropsTip":drops})
}
fn payer(input: &Value) -> Result<Value> {
    let call = decoded(&input["call"])?;
    let signer = input["signer"].clone();
    let state = &input["state"];
    if let Some((mut real, mut inner)) = proxy(&call) {
        let query = request(
            "exists",
            "Proxy",
            "RealPaysFeeConsentV1",
            json!([real, signer]),
        );
        match exists(state, &query)? {
            None => return Ok(query),
            Some(false) => {}
            Some(true) => {
                for _ in 1..3 {
                    let nested = decoded(&inner)?;
                    if let Some((next, next_inner)) = proxy(&nested) {
                        let query = request(
                            "exists",
                            "Proxy",
                            "RealPaysFeeConsentV1",
                            json!([next, real]),
                        );
                        match exists(state, &query)? {
                            None => return Ok(query),
                            Some(false) => break,
                            Some(true) => {
                                real = next;
                                inner = next_inner;
                            }
                        }
                    } else {
                        if ["Utility.batch", "Utility.batch_all", "Utility.force_batch"]
                            .contains(&name(&nested).as_str())
                        {
                            if let Some(calls) = nested["params"]["calls"]
                                .as_array()
                                .filter(|v| !v.is_empty())
                            {
                                let mut common: Option<Value> = None;
                                for child in calls {
                                    let Some((next, _)) = proxy(&decoded(child)?) else {
                                        return Ok(result(real, true));
                                    };
                                    if let Some(selected) = &common {
                                        if *selected != next {
                                            let query = request(
                                                "identity",
                                                "",
                                                "",
                                                json!([selected, next]),
                                            );
                                            let compared =
                                                &state[query["key"].as_str().unwrap_or("")];
                                            if compared.is_null() {
                                                return Ok(query);
                                            }
                                            if !truth(compared) {
                                                return Ok(result(real, true));
                                            }
                                        }
                                    } else {
                                        let query = request(
                                            "exists",
                                            "Proxy",
                                            "RealPaysFeeConsentV1",
                                            json!([next, real]),
                                        );
                                        match exists(state, &query)? {
                                            None => return Ok(query),
                                            Some(false) => return Ok(result(real, true)),
                                            Some(true) => common = Some(next),
                                        }
                                    }
                                }
                                if let Some(common) = common {
                                    return Ok(result(common, true));
                                }
                            }
                        }
                        return Ok(result(real, true));
                    }
                }
                return Ok(result(real, true));
            }
        }
    }
    if OWNER_CALLS.contains(&name(&call).as_str()) {
        let query = request("exists", "SubtensorModule", "Owner", json!([signer]));
        match exists(state, &query)? {
            None => return Ok(query),
            Some(false) => {}
            Some(true) => {
                let query = request("query", "SubtensorModule", "Owner", json!([signer]));
                let key = query["key"].as_str().unwrap_or("");
                if !state
                    .as_object()
                    .is_some_and(|state| state.contains_key(key))
                {
                    return Ok(query);
                }
                return Ok(result(state[key].clone(), true));
            }
        }
    }
    Ok(result(signer, false))
}
pub(super) fn evaluate(op: &str, input: &Value) -> Result<Value> {
    match op {
        "constants" => Ok(json!(OWNER_CALLS)),
        "leaves" => Ok(json!(leaves(&input["call"], 0)?)),
        "charges" => Ok(json!(leaves(&input["call"], 0)?.iter().any(|row| {
            OWNER_CALLS.contains(
                &format!(
                    "{}.{}",
                    row[0].as_str().unwrap_or(""),
                    row[1].as_str().unwrap_or("")
                )
                .as_str(),
            )
        }))),
        "payer" => payer(input),
        _ => Err(invalid(format!("unknown fee policy operation {op}"))),
    }
}
