//! Bind imported call bytes to their human-facing intent. Only derived values
//! (for example an approved staking price) may come from the committed call.
use super::{evaluate as policy, PolicyError};
use serde_json::{json, Value};
use std::collections::BTreeSet;

fn mismatch() -> PolicyError {
    PolicyError::invalid("saved call bytes do not match the declared semantic intent")
}

fn canonical(value: &Value) -> Value {
    match value {
        Value::Number(value) => json!(value.to_string()),
        Value::String(value) => {
            if let Ok(account) = crate::keys::public_key_from_ss58(value) {
                json!(format!("0x{}", hex::encode(account)))
            } else if value.starts_with("0x") && hex::decode(&value[2..]).is_ok() {
                json!(value.to_lowercase())
            } else {
                json!(value)
            }
        }
        Value::Array(values) => Value::Array(values.iter().map(canonical).collect()),
        Value::Object(values) if values.len() == 1 && values.contains_key("Id") => {
            canonical(&values["Id"])
        }
        Value::Object(values) => Value::Object(
            values
                .iter()
                .filter(|(key, _)| {
                    !(values.contains_key("pallet")
                        && values.contains_key("call")
                        && values.contains_key("params")
                        && matches!(key.as_str(), "callData" | "semanticCall" | "signerRole"))
                })
                .map(|(key, value)| (key.clone(), canonical(value)))
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn accounts(value: &Value, result: &mut BTreeSet<Vec<u8>>) {
    match value {
        Value::String(value) => {
            if let Ok(account) = crate::keys::public_key_from_ss58(value) {
                result.insert(account.to_vec());
            }
        }
        Value::Array(values) => values.iter().for_each(|value| accounts(value, result)),
        Value::Object(values) => values.values().for_each(|value| accounts(value, result)),
        _ => (),
    }
}

fn registration_account(value: &Value) -> Result<Vec<u8>, PolicyError> {
    let descriptor = value
        .as_object()
        .filter(|value| value.len() == 1)
        .and_then(|value| value.iter().next())
        .filter(|(name, _)| matches!(name.as_str(), "Classical" | "MlDsa65" | "Falcon"))
        .ok_or_else(mismatch)?
        .1;
    let encoded = policy("transaction.descriptor_encode", json!({"value":descriptor}))?;
    let bytes = hex::decode(
        encoded
            .as_str()
            .ok_or_else(mismatch)?
            .trim_start_matches("0x"),
    )
    .map_err(|_| mismatch())?;
    let key = crate::keys::Keypair::from_hashed_descriptor(&bytes, 42).map_err(|_| mismatch())?;
    Ok(key.public_key_bytes().to_vec())
}

// Registration envelopes are independently checked for payer, reserve and
// consent. They may only register identities actually named by this intent.
fn without_guards(
    call: &Value,
    registered: &mut BTreeSet<Vec<u8>>,
    flatten: bool,
    depth: usize,
) -> Result<Value, PolicyError> {
    if depth > 32 {
        return Err(mismatch());
    }
    let mut result = call.clone();
    if call["pallet"] == "Utility" && call["call"] == "batch_all" {
        let mut calls = vec![];
        let mut deposit = false;
        for child in call["params"]["calls"].as_array().ok_or_else(mismatch)? {
            if child["pallet"] == "HashedAccounts" && child["call"] == "check_registration_deposit"
            {
                if deposit {
                    return Err(mismatch());
                }
                deposit = true;
            } else if child["pallet"] == "HashedAccounts"
                && matches!(
                    child["call"].as_str(),
                    Some("register" | "check_registered")
                )
            {
                registered.insert(registration_account(&child["params"]["descriptor"])?);
                deposit = false;
            } else {
                if deposit {
                    return Err(mismatch());
                }
                if !flatten && child["pallet"] == "Utility" && child["call"] == "batch_all" {
                    return Err(mismatch());
                }
                let child = without_guards(child, registered, flatten, depth.saturating_add(1))?;
                if child["pallet"] == "Utility" && child["call"] == "batch_all" {
                    calls.extend(
                        child["params"]["calls"]
                            .as_array()
                            .ok_or_else(mismatch)?
                            .iter()
                            .cloned(),
                    );
                } else {
                    calls.push(child);
                }
            }
        }
        if deposit || calls.is_empty() {
            return Err(mismatch());
        }
        if calls.len() == 1 {
            return calls.pop().ok_or_else(mismatch);
        }
        result["params"]["calls"] = json!(calls);
    } else if !call["params"]["call"].is_null() {
        result["params"]["call"] = without_guards(
            &call["params"]["call"],
            registered,
            flatten,
            depth.saturating_add(1),
        )?;
    }
    Ok(result)
}

fn staking(intent: &Value, actual: &Value, origin: &Value) -> Result<Value, PolicyError> {
    let args = &intent["args"];
    let hotkey = args
        .get("hotkey_ss58")
        .filter(|value| !value.is_null())
        .unwrap_or(origin);
    let source = args
        .get("origin_hotkey_ss58")
        .filter(|value| !value.is_null())
        .unwrap_or(hotkey);
    let hotkey = if matches!(
        intent["op"].as_str(),
        Some("move_stake" | "move_swap_stake" | "swap_stake" | "transfer_stake")
    ) {
        source
    } else {
        hotkey
    };
    let mut input = json!({"op":intent["op"],"args":args,"hotkey":hotkey,"origin_hotkey":source});
    if canonical(source) == canonical(&args["dest_hotkey_ss58"]) {
        input["args"]["dest_hotkey_ss58"] = source.clone();
    }
    let route = policy("financial.staking_decision", input.clone())?;
    let mut calls = actual["params"]["calls"]
        .as_array()
        .cloned()
        .unwrap_or_else(|| vec![actual.clone()]);
    if claims_first(intent)
        && calls
            .first()
            .is_some_and(|call| call["call"] == "claim_root_with_hotkey")
    {
        calls.remove(0);
    }
    let first = calls.first().ok_or_else(mismatch)?;
    let last = calls.last().ok_or_else(mismatch)?;
    let (field, encoded) = match intent["op"].as_str() {
        Some("add_stake" | "add_stake_limit" | "stake_into_basket") => {
            ("amount_tao", "amount_staked")
        }
        Some("remove_stake" | "remove_stake_limit") => ("amount_alpha", "amount_unstaked"),
        Some("move_stake" | "move_swap_stake" | "swap_stake" | "transfer_stake") => {
            ("amount_alpha", "alpha_amount")
        }
        Some("add_collateral") => ("amount_alpha", "alpha"),
        Some("stake_burn") => ("amount_tao", "amount"),
        Some("swap_basket") => ("amount", "amount"),
        _ => ("amount_alpha", "amount"),
    };
    input["amount"] = if !route["amount"].is_null() {
        route["amount"].clone()
    } else if args[field] == "all" {
        first["params"][encoded].clone()
    } else {
        args[field].clone()
    };
    input["limit_price"] = if route["explicit_limit"] == true {
        args["limit_price_rao"].clone()
    } else if intent["op"] == "stake_burn" && !args["limit_price"].is_null() {
        args["limit_price"].clone()
    } else if intent["op"] == "stake_burn" {
        last["params"]["limit"].clone()
    } else {
        last["params"]["limit_price"].clone()
    };
    let mut call = policy("financial.staking_call", input)?["call"].clone();
    if claims_first(intent) {
        call = json!({"pallet":"Utility","call":"batch_all","params":{"calls":[{"pallet":"SubtensorModule","call":"claim_root_with_hotkey","params":{"hotkey":source}},call]}});
    }
    Ok(call)
}

fn claims_first(intent: &Value) -> bool {
    intent["args"]["claim"] == true
        && matches!(
            intent["op"].as_str(),
            Some(
                "remove_stake"
                    | "remove_stake_limit"
                    | "move_stake"
                    | "unstake_all"
                    | "unstake_all_alpha"
            )
        )
}

fn gamma(intent: &Value, actual: &Value, origin: &Value) -> Result<Value, PolicyError> {
    let op = intent["op"].as_str().ok_or_else(mismatch)?;
    let args = &intent["args"];
    let units = |value: &Value| policy("financial.gamma.units", json!({"value":value}));
    // A named service resolves an authority, not a price. Its spender must be
    // independently read by the adapter and compared, never trusted from bytes.
    let spender = args["spender_ss58"].clone();
    let cap = if args["daily_spend_cap_gamma"].is_null() || args["remove_cap"] == true {
        Value::Null
    } else {
        units(&args["daily_spend_cap_gamma"])?
    };
    let mut params = json!({"netuid":args["netuid"],"hotkey":args["hotkey_ss58"]});
    match op {
        "mint_gamma" | "mint_gamma_with_stake" => {
            params["min_amount_out"] = actual["params"]["min_amount_out"].clone();
            params["spender"] = spender;
            params["daily_spend_cap"] = cap;
            if op == "mint_gamma" {
                params["amount"] = args["amount_tao"].clone();
            } else {
                params
                    .as_object_mut()
                    .ok_or_else(mismatch)?
                    .remove("hotkey");
                params["origin_hotkey"] = args["origin_hotkey_ss58"].clone();
                params["destination_hotkey"] = args["hotkey_ss58"].clone();
                params["alpha_amount"] = if args["amount_alpha"] == "all" {
                    json!(u64::MAX.to_string())
                } else {
                    args["amount_alpha"].clone()
                };
            }
        }
        "move_gamma" => {
            params = json!({"netuid":args["netuid"],"origin_hotkey":args["origin_hotkey_ss58"],"destination_hotkey":args["destination_hotkey_ss58"],"amount":if args["amount_gamma"] == "all" { json!(u64::MAX.to_string()) } else { units(&args["amount_gamma"])? }});
        }
        "set_gamma_deposit_spender" => params["spender"] = spender,
        "set_gamma_deposit_daily_spend_cap" => params["daily_spend_cap"] = cap,
        "spend_gamma" => {
            let spends = args["spends"]
                .as_array()
                .ok_or_else(mismatch)?
                .iter()
                .map(|spend| {
                    Ok(json!([
                        spend["hotkey_ss58"],
                        units(&spend["amount_gamma"])?,
                        policy(
                            "financial.gamma.reference",
                            json!({"value":spend["reference"]})
                        )?
                    ]))
                })
                .collect::<Result<Vec<_>, PolicyError>>()?;
            params = json!({"netuid":args["netuid"],"spends":spends});
        }
        "set_service" => {
            params["hotkey"] = args
                .get("hotkey_ss58")
                .filter(|value| !value.is_null())
                .unwrap_or(origin)
                .clone();
            params["url"] = json!(format!(
                "0x{}",
                hex::encode(args["url"].as_str().ok_or_else(mismatch)?.as_bytes())
            ));
            params["spender"] = args
                .get("spender_ss58")
                .filter(|value| !value.is_null())
                .unwrap_or(origin)
                .clone();
        }
        "close_gamma_deposit" => (),
        _ => return Err(mismatch()),
    }
    Ok(json!({"pallet":"SubtensorModule","call":op,"params":params}))
}

fn multisig(
    intent: &Value,
    actual: &Value,
    origin: &Value,
    depth: usize,
) -> Result<Option<Value>, PolicyError> {
    let args = &intent["args"];
    let threshold_one = intent["op"] == "multisig_threshold_1";
    // Hash-only approvals and explicit imported commitments need the adapter's
    // SCALE hashing, metadata and pending-approval checks.
    if (intent["op"] == "multisig_approve" && !args["timepoint"].is_null())
        || ["call_data", "call_hash", "call_spec_version"]
            .iter()
            .any(|field| !args[field].is_null())
    {
        return Ok(None);
    }
    let threshold = if threshold_one {
        json!(1)
    } else {
        args["threshold"].clone()
    };
    let signer = crate::keys::public_key_from_ss58(origin.as_str().ok_or_else(mismatch)?)
        .map_err(|_| mismatch())?;
    let others = args["other_signatories"].as_array().ok_or_else(mismatch)?;
    let members = others
        .iter()
        .map(|member| {
            crate::keys::public_key_from_ss58(member.as_str().ok_or_else(mismatch)?)
                .map_err(|_| mismatch())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let selected = policy(
        "transaction.multisig_members",
        json!({"members":members.iter().map(|member| format!("0x{}",hex::encode(member))).collect::<Vec<_>>(),"signer":format!("0x{}",hex::encode(signer)),"threshold":threshold,"minimum_threshold":if threshold_one {1}else{2},"others_only":true,"deduplicate":true}),
    )?;
    let threshold = threshold
        .as_u64()
        .and_then(|value| u16::try_from(value).ok())
        .or_else(|| {
            threshold
                .as_str()
                .and_then(|value| value.parse::<u16>().ok())
        })
        .ok_or_else(mismatch)?;
    let indices = selected["indices"].as_array().ok_or_else(mismatch)?;
    let mut signatories = vec![signer];
    let mut sorted = vec![];
    for index in indices {
        let index = index
            .as_u64()
            .and_then(|index| usize::try_from(index).ok())
            .ok_or_else(mismatch)?;
        let member = members.get(index).ok_or_else(mismatch)?;
        signatories.push(*member);
        sorted.push(others.get(index).ok_or_else(mismatch)?.clone());
    }
    let mode = args
        .get("account_mode")
        .filter(|value| !value.is_null())
        .cloned()
        .unwrap_or_else(|| json!("legacy"));
    policy("transaction.account_mode", json!({"mode":mode}))?;
    let safe = mode == "safe";
    let (account, _) = if safe {
        crate::codec::extrinsic::safe_multisig_account_id(&signatories, threshold)
    } else {
        crate::codec::extrinsic::multisig_account_id(&signatories, threshold)
    }
    .map_err(|_| mismatch())?;
    let origin = json!(crate::codec::extrinsic::multisig_ss58(account, 42));
    let Some(inner) = expected(
        &args["call"],
        &actual["params"]["call"],
        &origin,
        depth.saturating_add(1),
    )?
    else {
        return Ok(None);
    };
    let mut params = json!({"other_signatories":sorted,"call":inner});
    if !threshold_one {
        params["threshold"] = json!(threshold);
        params["maybe_timepoint"] = if args["timepoint"].is_null() {
            Value::Null
        } else {
            super::transaction::multisig_timepoint(&args["timepoint"])?
        };
        params["max_weight"] = actual["params"]["max_weight"].clone();
    }
    let call = match (threshold_one, safe) {
        (true, true) => "as_multi_threshold_1_safe",
        (true, false) => "as_multi_threshold_1",
        (false, true) => "as_multi_safe",
        (false, false) => "as_multi",
    };
    Ok(Some(
        json!({"pallet":"Multisig","call":call,"params":params}),
    ))
}

fn expected(
    intent: &Value,
    actual: &Value,
    origin: &Value,
    depth: usize,
) -> Result<Option<Value>, PolicyError> {
    if depth > 32 {
        return Err(mismatch());
    }
    if !intent["args"]["service_hotkey_ss58"].is_null() {
        return Ok(None);
    }
    let root = intent["origin"] == "root";
    let actual = if root {
        if actual["pallet"] != "Sudo" || actual["call"] != "sudo" {
            return Err(mismatch());
        }
        &actual["params"]["call"]
    } else {
        actual
    };
    let mut args = intent["args"].clone();
    args["hotkey"] = args
        .get("hotkey_ss58")
        .filter(|value| !value.is_null())
        .unwrap_or(origin)
        .clone();
    let simple = policy(
        "transaction.simple_call",
        json!({"op":intent["op"],"args":args}),
    );
    let candidate = if let Ok(call) = simple {
        call
    } else {
        match intent["op"].as_str().ok_or_else(mismatch)? {
            "multisig_threshold_1" | "multisig_execute" | "multisig_approve" => {
                let Some(call) = multisig(intent, actual, origin, depth)? else {
                    return Ok(None);
                };
                call
            }
            "mint_gamma"
            | "mint_gamma_with_stake"
            | "move_gamma"
            | "close_gamma_deposit"
            | "set_gamma_deposit_spender"
            | "set_gamma_deposit_daily_spend_cap"
            | "spend_gamma"
            | "set_service" => gamma(intent, actual, origin)?,
            "evm_withdraw" => {
                let account = canonical(origin);
                let address = policy("transaction.evm_truncated", json!({"account":account}))?;
                json!({"pallet":"EVM","call":"withdraw","params":{"address":address,"value":if args["amount_tao"] == "all" { &actual["params"]["value"] } else { &args["amount_tao"] }}})
            }
            "contribute_crowdloan" => {
                json!({"pallet":"Crowdloan","call":"contribute","params":{"crowdloan_id":args["crowdloan_id"],"amount":if args["amount_tao"] == "all" { &actual["params"]["amount"] } else { &args["amount_tao"] }}})
            }
            "set_take" | "increase_take" | "decrease_take" | "set_childkey_take" => {
                let take = policy(
                    "transaction.take",
                    json!({"op":intent["op"],"take":args["take"],"approved_function":actual["call"]}),
                )?;
                let mut params = json!({"hotkey":args["hotkey"],"take":take["take"]});
                if intent["op"] == "set_childkey_take" {
                    params["netuid"] = args["netuid"].clone();
                }
                json!({"pallet":"SubtensorModule","call":take["call"],"params":params})
            }
            "add_stake"
            | "add_stake_limit"
            | "remove_stake"
            | "remove_stake_limit"
            | "move_stake"
            | "move_swap_stake"
            | "swap_stake"
            | "transfer_stake"
            | "lock_stake"
            | "add_collateral"
            | "stake_burn"
            | "stake_into_basket"
            | "swap_basket"
            | "unstake_all"
            | "unstake_all_alpha"
            | "claim_root"
            | "claim_root_with_hotkey" => staking(intent, actual, origin)?,
            "batch" => {
                let children = args["intents"].as_array().ok_or_else(mismatch)?;
                let calls = actual["params"]["calls"]
                    .as_array()
                    .cloned()
                    .unwrap_or_else(|| vec![actual.clone()]);
                let mut offset: usize = 0;
                let mut prepared = vec![];
                for child in children {
                    let width = if child["op"] == "move_swap_stake" {
                        2usize
                    } else {
                        1usize
                    }
                    .saturating_add(usize::from(claims_first(child)));
                    let end = offset.saturating_add(width);
                    let selected = calls.get(offset..end).ok_or_else(mismatch)?;
                    let selected = if selected.len() == 1 {
                        selected.first().cloned().ok_or_else(mismatch)?
                    } else {
                        json!({"pallet":"Utility","call":"batch_all","params":{"calls":selected}})
                    };
                    let Some(call) = expected(child, &selected, origin, depth.saturating_add(1))?
                    else {
                        return Ok(None);
                    };
                    prepared.push(call);
                    offset = end;
                }
                if offset != calls.len() {
                    return Err(mismatch());
                }
                json!({"pallet":"Utility","call":"batch_all","params":{"calls":prepared}})
            }
            "create_crowdloan" => {
                let inner = if args["call"].is_null() {
                    Value::Null
                } else {
                    let Some(call) = expected(
                        &args["call"],
                        &actual["params"]["call"],
                        origin,
                        depth.saturating_add(1),
                    )?
                    else {
                        return Ok(None);
                    };
                    call
                };
                json!({"pallet":"Crowdloan","call":"create","params":{"deposit":args["deposit_tao"],"min_contribution":args["min_contribution_tao"],"cap":args["cap_tao"],"end":args["end"],"call":inner,"target_address":args["target_ss58"]}})
            }
            "execute_proxy_announced" => {
                let Some(call) = expected(
                    &args["inner"],
                    &actual["params"]["call"],
                    &args["real_ss58"],
                    depth.saturating_add(1),
                )?
                else {
                    return Ok(None);
                };
                json!({"pallet":"Proxy","call":"proxy_announced","params":{"delegate":args["delegate_ss58"],"real":args["real_ss58"],"force_proxy_type":args["force_proxy_type"],"call":call}})
            }
            _ => return Ok(None),
        }
    };
    Ok(Some(if root {
        json!({"pallet":"Sudo","call":"sudo","params":{"call":candidate}})
    } else {
        candidate
    }))
}

pub(crate) fn evaluate(input: &Value) -> Result<Value, PolicyError> {
    let actual = policy("protected.decoded_call", json!({"call":input["call"]}))?;
    if opaque_weights(&actual) {
        return Err(PolicyError::invalid("saved encrypted weights cannot be verified against plaintext intent; use the original SDK-prepared call"));
    }
    let mut registered = BTreeSet::new();
    let actual = without_guards(&actual, &mut registered, false, 0)?;
    let candidate = if input["prepared"].is_null() {
        expected(&input["intent"], &actual, &input["dispatch_origin"], 0)?
    } else {
        Some(policy(
            "protected.decoded_call",
            json!({"call":input["prepared"]}),
        )?)
    };
    let Some(candidate) = candidate else {
        return Ok(json!({"verified":false}));
    };
    let candidate = without_guards(&candidate, &mut BTreeSet::new(), true, 0)?;
    let mut allowed = BTreeSet::new();
    accounts(&candidate, &mut allowed);
    accounts(&input["dispatch_origin"], &mut allowed);
    if !registered.is_subset(&allowed) {
        return Err(mismatch());
    }
    if canonical(&actual) != canonical(&candidate) {
        return Err(mismatch());
    }
    Ok(json!({"verified":true}))
}

fn opaque_weights(call: &Value) -> bool {
    (call["pallet"] == "SubtensorModule" && call["call"] == "commit_timelocked_mechanism_weights")
        || (!call["params"]["call"].is_null() && opaque_weights(&call["params"]["call"]))
        || call["params"]["calls"]
            .as_array()
            .is_some_and(|calls| calls.iter().any(opaque_weights))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transfer() -> Value {
        json!({"op":"transfer","origin":"signed","args":{"dest_ss58":"recipient","amount_tao":"1000000000","keep_alive":true}})
    }
    fn call() -> Value {
        json!({"pallet":"Balances","call":"transfer_keep_alive","params":{"dest":"recipient","value":"1000000000"}})
    }

    // PROTECTED_REGRESSION: imported payment bytes must match every displayed effect; changes require explicit human approval.
    #[test]
    fn saved_payment_rejects_all_amount_destination_and_extra_effects() {
        let mut input = json!({"intent":transfer(),"call":call(),"dispatch_origin":"sender"});
        assert!(evaluate(&input).is_ok());
        input["call"]["params"]["value"] = json!("1000000001");
        assert!(evaluate(&input).is_err());
        input["call"] = call();
        input["call"]["params"]["dest"] = json!("attacker");
        assert!(evaluate(&input).is_err());
        input["call"] = json!({"pallet":"Balances","call":"transfer_all","params":{"dest":"recipient","keep_alive":true}});
        assert!(evaluate(&input).is_err());
        input["call"] =
            json!({"pallet":"Utility","call":"batch_all","params":{"calls":[call(),call()]}});
        assert!(evaluate(&input).is_err());
    }

    // PROTECTED_REGRESSION: approved staking bytes retain their price without allowing semantic amount changes; changes require explicit human approval.
    #[test]
    fn saved_staking_uses_committed_price_but_verifies_explicit_terms() {
        let intent = json!({"op":"add_stake","args":{"hotkey_ss58":"validator","netuid":1,"amount_tao":"1000000000","slippage_protection":true,"rate_tolerance":0.1}});
        let call = json!({"pallet":"SubtensorModule","call":"add_stake_limit","params":{"hotkey":"validator","netuid":1,"amount_staked":"1000000000","limit_price":"2200000000","allow_partial":false}});
        let mut input = json!({"intent":intent,"call":call});
        assert!(evaluate(&input).is_ok());
        input["intent"]["args"]["amount_tao"] = json!("1");
        assert!(evaluate(&input).is_err());
        input["intent"]["args"]["amount_tao"] = json!("1000000000");
        input["intent"]["args"]["netuid"] = json!(2);
        assert!(evaluate(&input).is_err());
    }

    // PROTECTED_REGRESSION: nested stored calls cannot conceal a payment or controller change; changes require explicit human approval.
    #[test]
    fn saved_crowdloan_and_batch_bind_nested_call_semantics() {
        let intent = json!({"op":"create_crowdloan","args":{"deposit_tao":"10","min_contribution_tao":"1","cap_tao":"100","end":42,"target_ss58":null,"call":transfer()}});
        let stored = json!({"pallet":"Crowdloan","call":"create","params":{"deposit":"10","min_contribution":"1","cap":"100","end":42,"target_address":null,"call":call()}});
        let mut input = json!({"intent":{"op":"batch","args":{"intents":[intent,transfer()]}},"call":{"pallet":"Utility","call":"batch_all","params":{"calls":[stored,call()]}}});
        assert!(evaluate(&input).is_ok());
        input["call"]["params"]["calls"][0]["params"]["call"] =
            json!({"pallet":"Proxy","call":"remove_proxies","params":{}});
        assert!(evaluate(&input).is_err());
    }

    #[test]
    fn saved_runtime_fields_named_like_sdk_metadata_are_not_discarded() {
        let candidate =
            json!({"pallet":"Example","call":"set","params":{"value":{"callData":"first"}}});
        let mut actual = candidate.clone();
        actual["params"]["value"]["callData"] = json!("second");
        assert_ne!(canonical(&candidate), canonical(&actual));
    }

    // PROTECTED_REGRESSION: a named Gamma service must not authorize an arbitrary imported spender; changes require explicit human approval.
    #[test]
    fn saved_service_authority_requires_independent_resolution() {
        let mut input = json!({"intent":{"op":"set_gamma_deposit_spender","args":{"netuid":1,"hotkey_ss58":"deposit","service_hotkey_ss58":"service"}},"call":{"pallet":"SubtensorModule","call":"set_gamma_deposit_spender","params":{"netuid":1,"hotkey":"deposit","spender":"attacker"}}});
        assert_eq!(
            evaluate(&input).map(|value| value["verified"].clone()).ok(),
            Some(json!(false))
        );
        input["prepared"] = json!({"pallet":"SubtensorModule","call":"set_gamma_deposit_spender","params":{"netuid":1,"hotkey":"deposit","spender":"service-spender"}});
        assert!(evaluate(&input).is_err());
    }

    // PROTECTED_REGRESSION: ignored semantic metadata must not authorize unrelated account registration; changes require explicit human approval.
    #[test]
    fn registration_guards_are_bound_to_actual_recipient_fields() -> Result<(), PolicyError> {
        let encoded = [vec![1, 1], vec![7; 32]].concat();
        let unrelated =
            crate::keys::Keypair::from_hashed_descriptor(&encoded, 42).map_err(|_| mismatch())?;
        let mut intent = transfer();
        intent["args"]["ignored_extra"] = json!(unrelated.ss58_address());
        let guarded = json!({"pallet":"Utility","call":"batch_all","params":{"calls":[{"pallet":"HashedAccounts","call":"register","params":{"descriptor":{"Classical":{"version":1,"scheme":"Sr25519","initial_commitment":format!("0x{}",hex::encode([7;32]))}}}},call()]}});
        assert!(
            evaluate(&json!({"intent":intent,"call":guarded,"dispatch_origin":"sender"})).is_err()
        );
        Ok(())
    }

    // PROTECTED_REGRESSION: saved threshold-one multisig bytes must bind the derived dispatch account and every inner effect; changes require explicit human approval.
    #[test]
    fn threshold_one_derives_legacy_and_safe_inner_origins() -> Result<(), PolicyError> {
        let signer = crate::codec::extrinsic::multisig_ss58([1; 32], 42);
        let other = crate::codec::extrinsic::multisig_ss58([2; 32], 0);
        for safe in [false, true] {
            let (account, _) = if safe {
                crate::codec::extrinsic::safe_multisig_account_id(&[[1; 32], [2; 32]], 1)
            } else {
                crate::codec::extrinsic::multisig_account_id(&[[1; 32], [2; 32]], 1)
            }
            .map_err(|_| mismatch())?;
            let address = policy(
                "transaction.evm_truncated",
                json!({"account":format!("0x{}",hex::encode(account))}),
            )?;
            let mut input = json!({"dispatch_origin":signer,"intent":{"op":"multisig_threshold_1","args":{"account_mode":if safe {"safe"} else {"legacy"},"other_signatories":[other],"call":{"op":"evm_withdraw","args":{"amount_tao":"5"}}}},"call":{"pallet":"Multisig","call":if safe {"as_multi_threshold_1_safe"} else {"as_multi_threshold_1"},"params":{"other_signatories":[other],"call":{"pallet":"EVM","call":"withdraw","params":{"address":address,"value":"5"}}}}});
            assert_eq!(evaluate(&input)?["verified"], true);
            input["call"]["params"]["call"]["params"]["address"] = policy(
                "transaction.evm_truncated",
                json!({"account":format!("0x{}",hex::encode([1;32]))}),
            )?;
            assert!(evaluate(&input).is_err());
        }
        Ok(())
    }

    // PROTECTED_REGRESSION: full multisig wrappers must bind members, threshold, timepoint and all inner effects while preserving approved derived values; changes require explicit human approval.
    #[test]
    fn full_multisig_wrappers_preserve_and_verify_committed_terms() -> Result<(), PolicyError> {
        let signer = crate::codec::extrinsic::multisig_ss58([1; 32], 42);
        let other = crate::codec::extrinsic::multisig_ss58([2; 32], 42);
        for safe in [false, true] {
            for op in ["multisig_execute", "multisig_approve"] {
                let timepoint = if op == "multisig_execute" {
                    json!({"height":42,"index":3})
                } else {
                    Value::Null
                };
                let mut input = json!({"dispatch_origin":signer,"intent":{"op":op,"args":{"threshold":2,"other_signatories":[other],"account_mode":if safe {"safe"} else {"legacy"},"timepoint":timepoint,"call":transfer()}},"call":{"pallet":"Multisig","call":if safe {"as_multi_safe"} else {"as_multi"},"params":{"threshold":2,"other_signatories":[other],"maybe_timepoint":timepoint,"max_weight":{"ref_time":"12345","proof_size":"678"},"call":call()}}});
                assert_eq!(evaluate(&input)?["verified"], true);
                input["call"]["params"]["threshold"] = json!(1);
                assert!(evaluate(&input).is_err());
                input["call"]["params"]["threshold"] = json!(2);
                input["call"]["params"]["maybe_timepoint"] = json!({"height":43,"index":3});
                assert!(evaluate(&input).is_err());
                input["call"]["params"]["maybe_timepoint"] = timepoint;
                input["call"]["params"]["call"]["params"]["value"] = json!("2000000000");
                assert!(evaluate(&input).is_err());
                input["intent"]["args"]["call_hash"] = json!(format!("0x{}", "11".repeat(32)));
                assert_eq!(evaluate(&input)?["verified"], false);
            }
        }
        Ok(())
    }
}
