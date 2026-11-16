//! Pure transaction decisions shared by Python, Node and browser clients.
use super::PolicyError;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

fn uint(value: &Value, name: &str) -> Result<u128, PolicyError> {
    let text = match value {
        Value::String(text) => text.clone(),
        Value::Number(number) if number.is_u64() => number.to_string(),
        _ => {
            return Err(PolicyError::type_error(format!(
                "{name} must be an unsigned integer"
            )))
        }
    };
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(PolicyError::type_error(format!(
            "{name} must be an unsigned integer"
        )));
    }
    text.parse()
        .map_err(|_| PolicyError::range_error(format!("{name} exceeds u128")))
}
fn optional_uint(value: &Value, name: &str) -> Result<Option<u128>, PolicyError> {
    if value.is_null() {
        Ok(None)
    } else {
        uint(value, name).map(Some)
    }
}
fn bytes(value: &Value, name: &str) -> Result<Vec<u8>, PolicyError> {
    value
        .as_str()
        .and_then(|text| text.strip_prefix("0x"))
        .and_then(|text| hex::decode(text).ok())
        .ok_or_else(|| PolicyError::type_error(format!("{name} must be hexadecimal bytes")))
}
fn checked_add(left: u128, right: u128) -> Result<u128, PolicyError> {
    left.checked_add(right)
        .ok_or_else(|| PolicyError::range_error("transaction amount exceeds u128"))
}

/// Stable machine-readable reasons; language adapters only format these for users.
pub(crate) fn policy_decisions(input: &Value) -> Result<Value, PolicyError> {
    let mut violations = Vec::new();
    if input["raw"] == true && input["allow_raw_calls"] != true {
        violations.push(json!({"code":"raw"}));
    }
    if let Some(cap) = optional_uint(&input["max_fee_rao"], "maxFeeRao")? {
        match optional_uint(&input["fee_rao"], "fee")? {
            None => violations.push(json!({"code":"fee_unknown", "cap":cap.to_string()})),
            Some(fee) if fee > cap => violations
                .push(json!({"code":"fee", "actual":fee.to_string(), "cap":cap.to_string()})),
            _ => (),
        }
    }
    let unbounded = input["spend_rao"] == "unbounded";
    let spend = if unbounded {
        0
    } else {
        checked_add(
            optional_uint(&input["spend_rao"], "spend")?.unwrap_or(0),
            optional_uint(&input["additional_spend_rao"], "additional spend")?.unwrap_or(0),
        )?
    };
    if let Some(cap) = optional_uint(&input["max_spend_rao"], "maxSpendRao")? {
        if unbounded {
            violations.push(json!({"code":"unbounded", "cap":cap.to_string()}));
        } else if spend > cap {
            violations
                .push(json!({"code":"spend", "actual":spend.to_string(), "cap":cap.to_string()}));
        }
    }
    if let Some(allowed) = input["allowed_netuids"].as_array() {
        let allowed = allowed
            .iter()
            .map(|value| uint(value, "allowed netuid"))
            .collect::<Result<BTreeSet<_>, _>>()?;
        if input["affects_all_subnets"] == true {
            violations.push(json!({"code":"all_subnets"}));
        }
        if input["global"] == true && input["allow_global"] != true {
            violations.push(json!({"code":"global"}));
        }
        for value in input["netuids"].as_array().into_iter().flatten() {
            let netuid = uint(value, "netuid")?;
            if !allowed.contains(&netuid) {
                violations.push(json!({"code":"netuid", "netuid":netuid}));
            }
        }
    } else if !input["allowed_netuids"].is_null() {
        return Err(PolicyError::type_error("allowed_netuids must be an array"));
    }
    Ok(
        json!({"violations":violations,"spend_rao":if unbounded { "unbounded".into() } else { spend.to_string() }}),
    )
}

fn account_profile(value: &Value) -> Value {
    let code = value.as_u64().and_then(|value| u8::try_from(value).ok());
    let (name, scheme, standard, hashed) = match code {
        Some(0 | 6) => (
            if code == Some(0) {
                "ed25519"
            } else {
                "hashed-ed25519"
            },
            "Ed25519",
            0,
            6,
        ),
        Some(1 | 4) => (
            if code == Some(1) {
                "sr25519"
            } else {
                "hashed-sr25519"
            },
            "Sr25519",
            1,
            4,
        ),
        Some(7 | 8) => (
            if code == Some(8) {
                "mldsa65"
            } else {
                "hashed-mldsa65"
            },
            "MlDsa65",
            8,
            7,
        ),
        Some(9 | 10) => (
            if code == Some(9) {
                "falcon512"
            } else {
                "hashed-falcon512"
            },
            "Falcon512",
            9,
            10,
        ),
        Some(11 | 12) => (
            if code == Some(11) {
                "falcon1024"
            } else {
                "hashed-falcon1024"
            },
            "Falcon1024",
            11,
            12,
        ),
        _ => {
            return json!({"known":false,"rotating":false,"pq":false,"falcon":false,"mldsa":false,"hashed":false,"fixed_pq":false,"protected":false})
        }
    };
    let Some(code) = code else {
        return json!({"known":false});
    };
    let pq = crate::keys::is_pq_crypto(code);
    let rotating = matches!(code, 4 | 6 | 7 | 10 | 12);
    let lengths = match code {
        7 | 8 => Some((1952, 3309)),
        9 | 10 => Some((897, 666)),
        11 | 12 => Some((1793, 1280)),
        _ => None,
    };
    json!({"known":true,"name":name,"scheme":scheme,"standard":standard,"hashed_type":hashed,"rotating":rotating,
        "pq":pq,"falcon":crate::keys::is_falcon_crypto(code),"mldsa":crate::keys::is_mldsa_crypto(code),"hashed":crate::keys::is_hashed_crypto(code),
        "fixed_pq":crate::keys::is_fixed_pq_crypto(code),"protected":pq || rotating,"profile":crate::keys::pq_profile(code),"lengths":lengths})
}

fn multisig_members(input: &Value) -> Result<Value, PolicyError> {
    let values = input["members"]
        .as_array()
        .ok_or_else(|| PolicyError::type_error("multisig members must be an array"))?;
    let mut members = BTreeMap::new();
    for (index, value) in values.iter().enumerate() {
        let opaque = input["opaque_members"] == true;
        let id = if opaque {
            value
                .as_str()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| {
                    PolicyError::type_error("multisig account must be a nonempty string")
                })?
                .as_bytes()
                .to_vec()
        } else {
            bytes(value, "multisig account")?
        };
        if !opaque && id.len() != 32 {
            return Err(PolicyError::type_error("multisig account must be 32 bytes"));
        }
        if members.insert(id, index).is_some() && input["deduplicate"] != true {
            return Err(PolicyError::range_error("duplicate multisig signatory"));
        }
    }
    let threshold = uint(&input["threshold"], "threshold")?;
    let minimum = optional_uint(&input["minimum_threshold"], "minimum threshold")?.unwrap_or(1);
    let others_only = input["others_only"] == true;
    if threshold < minimum || threshold > 65535 {
        return Err(PolicyError::range_error(format!(
            "invalid multisig threshold: must be at least {minimum} and fit u16"
        )));
    }
    if threshold > members.len().saturating_add(usize::from(others_only)) as u128 {
        return Err(PolicyError::range_error(
            "multisig threshold exceeds the signatory count",
        ));
    }
    if let Some(maximum) = optional_uint(&input["maximum_members"], "maximum members")? {
        if members.len() as u128 > maximum {
            return Err(PolicyError::range_error("too many multisig members"));
        }
    }
    let signer = if input["signer"].is_null() {
        None
    } else {
        Some(bytes(&input["signer"], "signer account")?)
    };
    if let Some(signer) = &signer {
        if others_only && members.contains_key(signer) {
            return Err(PolicyError::range_error(
                "other_signatories must exclude the signer's own account",
            ));
        }
        if !others_only && !members.contains_key(signer) {
            return Err(PolicyError::range_error(
                "signer is not a member of this multisig",
            ));
        }
    }
    let sorted: Vec<_> = members.values().copied().collect();
    let others: Vec<_> = members
        .iter()
        .filter(|(id, _)| signer.as_ref() != Some(*id))
        .map(|(_, index)| *index)
        .collect();
    Ok(json!({"indices":sorted,"other_indices":others,"count":members.len()}))
}

fn multisig_cancel(input: &Value) -> Result<Value, PolicyError> {
    let timepoint = |value: &Value| -> Result<Value, PolicyError> {
        let height = u32::try_from(uint(&value["height"], "timepoint height")?)
            .map_err(|_| PolicyError::range_error("timepoint height exceeds u32"))?;
        let index = u32::try_from(uint(&value["index"], "timepoint index")?)
            .map_err(|_| PolicyError::range_error("timepoint index exceeds u32"))?;
        Ok(json!({"height":height,"index":index}))
    };
    let when = timepoint(&input["timepoint"])?;
    let selected = if !input["call_hash"].is_null() {
        input["call_hash"].clone()
    } else {
        let pending = input["pending"].as_array().ok_or_else(|| {
            PolicyError::type_error("cancel requires a committed call_hash or pending operations")
        })?;
        let mut selected = None;
        for row in pending {
            if timepoint(&row["timepoint"])? == when {
                if selected.is_some() {
                    return Err(PolicyError::invalid(
                        "multiple pending operations share this timepoint; pass the committed call_hash",
                    ));
                }
                selected = Some(row["call_hash"].clone());
            }
        }
        selected.ok_or_else(|| PolicyError::invalid(
            "no pending multisig operation at this timepoint; pass the original committed call_hash",
        ))?
    };
    let hash = bytes(&selected, "multisig call hash")?;
    if hash.len() != 32 {
        return Err(PolicyError::range_error(
            "multisig call hash must be 32 bytes",
        ));
    }
    Ok(json!({"call_hash":format!("0x{}",hex::encode(hash)),"timepoint":when}))
}

pub(crate) fn nested_dispatch_error(events: &Value) -> Option<Value> {
    for row in events.as_array()? {
        let event = row.get("event").unwrap_or(row);
        let Some(module) = event
            .get("module_id")
            .or_else(|| event.get("section"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        let Some(name) = event
            .get("event_id")
            .or_else(|| event.get("method"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        let Some(field) = nested_result_field(module, name) else {
            continue;
        };
        let Some(attributes) = event.get("attributes").or_else(|| event.get("data")) else {
            continue;
        };
        let result = attributes.get(field).unwrap_or(attributes);
        // Tuple events put DispatchResult/DispatchError in their last field.
        let result = result
            .as_array()
            .and_then(|values| values.last())
            .unwrap_or(result);
        if let Some(error) = result.get("Err") {
            return Some(error.clone());
        }
        if module == "Utility" && !result.is_null() {
            return Some(result.clone());
        }
    }
    None
}

// Bounded SCALE-era shifts and arbitrary-precision nanosecond subtraction are safe.
// All user-supplied fixed-width amount addition and multiplication is checked.
#[allow(clippy::arithmetic_side_effects)]
pub(crate) fn evaluate(operation: &str, input: Value) -> Result<Value, PolicyError> {
    match operation {
        "transaction.children" => {
            let rows = input["children"]
                .as_array()
                .ok_or_else(|| PolicyError::type_error("children must be an array"))?;
            let mut normalized = Vec::new();
            let mut total = 0u128;
            for row in rows {
                let entry = row
                    .as_array()
                    .filter(|entry| entry.len() == 2)
                    .ok_or_else(|| {
                        PolicyError::type_error("child must be a [proportion, address] pair")
                    })?;
                if !entry[1].is_string() {
                    return Err(PolicyError::type_error("child address must be a string"));
                }
                let share = super::financial::evaluate(
                    "financial.intent.proportion",
                    json!({"value":entry[0],"max":u64::MAX.to_string()}),
                )?;
                total = checked_add(total, uint(&share, "child proportion")?)?;
                normalized.push(json!([share, entry[1]]));
            }
            if total > u128::from(u64::MAX) {
                return Err(PolicyError::range_error(
                    "child proportions sum exceeds 1; together they must not exceed 1.0",
                ));
            }
            Ok(json!(normalized))
        }
        "transaction.take" => {
            let take = super::financial::evaluate(
                "financial.intent.proportion",
                json!({"value":input["take"],"max":u16::MAX}),
            )?;
            let op = input["op"].as_str().unwrap_or("");
            let function = if op == "set_take" && input["approved_function"].is_null() {
                let current = uint(&input["current"], "current take")?;
                let desired = uint(&take, "take")?;
                if desired == current {
                    return Err(PolicyError::range_error("delegate take is unchanged"));
                }
                if desired < current {
                    "decrease_take"
                } else {
                    "increase_take"
                }
            } else if op == "set_take" {
                input["approved_function"]
                    .as_str()
                    .filter(|value| matches!(*value, "increase_take" | "decrease_take"))
                    .ok_or_else(|| PolicyError::type_error("invalid approved take operation"))?
            } else if matches!(op, "set_childkey_take" | "increase_take" | "decrease_take") {
                op
            } else {
                return Err(PolicyError::type_error("invalid take operation"));
            };
            Ok(json!({"call":function,"take":take}))
        }
        "transaction.serving" => serving(&input),
        "transaction.proxy_type" => {
            if !PROXY_TYPES.contains(&input["proxy_type"].as_str().unwrap_or("")) {
                return Err(PolicyError::range_error("unknown proxy type"));
            }
            Ok(input["proxy_type"].clone())
        }
        "transaction.pure_proxy" => {
            let mode=evaluate("transaction.derivative",json!({"index":input["index"],"mode":input["mode"],"crypto_type":input["crypto_type"]}))?["mode"].clone();
            let call = match (
                input["op"].as_str(),
                mode == "safe",
                input["pq_required"] == true,
            ) {
                (Some("create_pure_proxy"), true, _) => "create_safe_pure",
                (Some("create_pure_proxy"), false, true) => "create_protected_pure",
                (Some("create_pure_proxy"), false, false) => "create_pure",
                (Some("kill_pure_proxy"), true, _) => "kill_safe_pure",
                (Some("kill_pure_proxy"), false, _) => "kill_pure",
                _ => return Err(PolicyError::type_error("invalid pure proxy operation")),
            };
            Ok(json!({"mode":mode,"call":call,"protected":call=="create_protected_pure"}))
        }
        "transaction.batch_guard" => Ok(json!(if input["registration_guards"] == true {
            Some("registration_guards")
        } else if input["requires_direct"] == true {
            Some("requires_direct")
        } else if input["new_registration"] == true {
            Some("new_registration")
        } else {
            None
        })),
        "transaction.receiving_target" => {
            Ok(json!(receiving_target(input["op"].as_str().unwrap_or(""))))
        }
        "transaction.receiving_direct" => Ok(json!(
            matches!(
                input["op"].as_str(),
                Some("swap_coldkey_announced" | "pow_register" | "execute_proxy_announced")
            ) || (input["op"] == "announce_coldkey_swap" && input["registered"] == true)
        )),
        "transaction.receiving_registration" => {
            let target = receiving_target(input["op"].as_str().unwrap_or(""));
            let eligible = input["direct"] != true
                && input["op"] != "associate_hotkey"
                && target.is_some_and(|target| input["field"] == target)
                && optional_uint(&input["unique_count"], "recipient count")?.unwrap_or(1) == 1;
            Ok(
                json!({"may_register":eligible,"call":if input["direct"]==true {Value::Null}else if input["registered"]==true {json!("check_registered")}else{json!("register")}}),
            )
        }
        "transaction.protected_owner" => Ok(json!(
            matches!(input["kind"].as_str(), Some("mldsa" | "derived"))
                || (input["requires_pq"] != true && input["kind"] == "classical")
        )),
        "transaction.recipient_identity" => {
            let mismatch = if !input["explicit"].is_null()
                && input["descriptor"] != input["explicit"]
            {
                Some("descriptor")
            } else if !input["account"].is_null() && input["account"] != input["descriptor_account"]
            {
                Some("account")
            } else {
                None
            };
            Ok(json!(mismatch))
        }
        "transaction.intent_metadata" => intent_metadata(&input),
        "transaction.merge_intents" => merge_intents(&input),
        "transaction.simple_call" => simple_call(&input),
        "transaction.execution" => {
            let shielded = (input["force_required"] == true && input["shield_required"] == true)
                || input["shielded"]
                    .as_bool()
                    .unwrap_or(input["shield_default"] == true);
            let mut violations = Vec::new();
            if input["shield_required"] == true && !shielded {
                violations.push("shield_required");
            }
            if input["expected_hash"].as_str().is_some_and(|hash| {
                input["actual_hash"]
                    .as_str()
                    .is_none_or(|actual| !hash.eq_ignore_ascii_case(actual))
            }) {
                violations.push("call_hash");
            }
            if input["raw"] == true && input["allow_raw_calls"] != true {
                violations.push("raw");
            }
            Ok(json!({"shielded":shielded,"violations":violations}))
        }
        "transaction.signing_bounds" => {
            let nonce = checked_add(
                uint(&input["nonce"], "nonce")?,
                optional_uint(&input["offset"], "offset")?.unwrap_or(0),
            )?;
            if nonce >= u128::from(u32::MAX) {
                return Err(PolicyError::range_error("account nonce exhausted"));
            }
            if !input["period"].is_null() {
                let period = uint(&input["period"], "mortality period")?;
                if !(4..=65536).contains(&period) {
                    return Err(PolicyError::range_error("invalid mortality period"));
                }
                if input["shielded"] == true && period > 8 {
                    return Err(PolicyError::range_error(
                        "shielding requires a 4..8 block era",
                    ));
                }
            } else if input["shielded"] == true {
                return Err(PolicyError::range_error(
                    "shielding requires a 4..8 block era",
                ));
            }
            Ok(json!(nonce.to_string()))
        }
        "transaction.era" => {
            let raw = bytes(&input["era"], "era")?;
            if raw == [0] {
                return Ok(json!("00"));
            }
            if raw.len() != 2 {
                return Err(PolicyError::type_error("invalid transaction era encoding"));
            }
            let encoded = u16::from_le_bytes([raw[0], raw[1]]);
            let period = 2u32 << (encoded & 15);
            let phase = u32::from(encoded >> 4) * (period >> 12).max(1);
            if period < 4 || phase >= period {
                return Err(PolicyError::type_error("invalid mortal transaction era"));
            }
            Ok(json!({"period":period,"phase":phase}))
        }
        "transaction.intent_policy" => policy_decisions(&input),
        "transaction.account_profile" => Ok(account_profile(&input["crypto_type"])),
        "transaction.constants" => {
            let profiles: Vec<_> = [0, 1, 4, 6, 7, 8, 9, 10, 11, 12]
                .into_iter()
                .map(|code| {
                    let mut profile = account_profile(&json!(code));
                    profile["crypto_type"] = json!(code);
                    profile
                })
                .collect();
            Ok(json!({"profiles":profiles,"proxy_types":PROXY_TYPES}))
        }
        "transaction.account_type" => {
            let profile = account_profile(&input["crypto_type"]);
            if profile["known"] != true {
                return Err(PolicyError::range_error("unsupported crypto type"));
            }
            match input["mode"].as_str() {
                None if input["mode"].is_null() => Ok(input["crypto_type"].clone()),
                Some("standard") => Ok(profile["standard"].clone()),
                Some("hashed") => Ok(profile["hashed_type"].clone()),
                _ => Err(PolicyError::range_error(
                    "account type must be standard or hashed",
                )),
            }
        }
        "transaction.account_mode" => match input["mode"].as_str() {
            Some("legacy" | "safe") => Ok(input["mode"].clone()),
            _ => Err(PolicyError::range_error(
                "account_mode must be legacy or safe",
            )),
        },
        "transaction.multisig_members" => multisig_members(&input),
        "transaction.multisig_cancel" => multisig_cancel(&input),
        "transaction.multisig_commitment" => {
            let actual = bytes(&input["actual_hash"], "multisig call hash")?;
            if actual.len() != 32 {
                return Err(PolicyError::range_error(
                    "multisig call hash must be 32 bytes",
                ));
            }
            if !input["expected_hash"].is_null()
                && bytes(&input["expected_hash"], "expected multisig call hash")? != actual
            {
                return Err(PolicyError::invalid("multisig call differs from its original commitment; use the original call_data"));
            }
            if !input["timepoint"].is_null() {
                if input["pending"].is_null() {
                    return Err(PolicyError::invalid("no pending approval matches these call bytes; use the original multisig call_data"));
                }
                let selected = multisig_cancel(&json!({
                    "call_hash": input["actual_hash"], "timepoint": input["timepoint"]
                }))?;
                let pending = multisig_cancel(&json!({
                    "call_hash": input["actual_hash"], "timepoint": input["pending"]["when"]
                }))?;
                if selected["timepoint"] != pending["timepoint"] {
                    return Err(PolicyError::invalid(
                        "multisig timepoint does not match the committed call",
                    ));
                }
            }
            Ok(json!(format!("0x{}", hex::encode(actual))))
        }
        "transaction.multisig_deposit" => {
            let factor = uint(&input["factor"], "multisig deposit factor")?;
            let threshold = uint(&input["threshold"], "threshold")?;
            let variable = factor
                .checked_mul(threshold)
                .ok_or_else(|| PolicyError::range_error("multisig deposit exceeds u128"))?;
            Ok(json!(checked_add(
                uint(&input["base"], "multisig deposit base")?,
                variable
            )?
            .to_string()))
        }
        "transaction.multisig_funding" => {
            let deposit = uint(&input["deposit"], "deposit")?;
            let fee = uint(&input["fee"], "fee")?;
            let retained = optional_uint(&input["retained"], "retained")?.unwrap_or(0);
            let required = checked_add(checked_add(deposit, fee)?, retained)?;
            Ok(
                json!({"required":required.to_string(),"affordable":uint(&input["free"],"free")? >= required}),
            )
        }
        "transaction.signature_normalize" => {
            let mut signature = bytes(&input["signature"], "signature")?;
            let kind = uint(&input["crypto_type"], "cryptoType")?;
            let size = match kind {
                0 | 1 => 64,
                2 => 65,
                _ => {
                    return Err(PolicyError::type_error(format!(
                        "unsupported signer cryptoType {kind}"
                    )))
                }
            };
            if signature.len() == size {
                return Ok(
                    json!({"signature":format!("0x{}",hex::encode(signature)),"crypto_type":kind}),
                );
            }
            if input["strict"] == false {
                let tag = signature.first().copied();
                if matches!(tag, Some(0 | 1)) && signature.len() == 65
                    || tag == Some(2) && signature.len() == 66
                {
                    signature.remove(0);
                    return Ok(
                        json!({"signature":format!("0x{}",hex::encode(signature)),"crypto_type":tag}),
                    );
                }
                return Ok(
                    json!({"signature":format!("0x{}",hex::encode(signature)),"crypto_type":kind}),
                );
            }
            if signature.len() != size + 1 {
                return Err(PolicyError::type_error(format!(
                    "signature must be {size} bytes, or {} bytes with its scheme prefix",
                    size + 1
                )));
            }
            if signature[0] as u128 != kind {
                return Err(PolicyError::type_error(
                    "signature scheme differs from signer",
                ));
            }
            signature.remove(0);
            Ok(json!({"signature":format!("0x{}",hex::encode(signature)),"crypto_type":kind}))
        }
        "transaction.descriptor" => {
            let descriptor = bytes(&input["descriptor"], "descriptor")?;
            let (version, tag) = match descriptor.as_slice() {
                [1, tag @ (1 | 3), rest @ ..] if rest.len() == 32 => (1, *tag),
                [2, tag @ 4..=9, rest @ ..] if rest.len() == 64 => (2, *tag),
                _ => return Err(PolicyError::type_error("invalid account descriptor")),
            };
            if descriptor[2..].iter().all(|byte| *byte == 0) {
                return Err(PolicyError::type_error("zero account commitment"));
            }
            let mut value = json!({"version":version,"initial_commitment":format!("0x{}",hex::encode(&descriptor[2..]))});
            if version == 2 {
                value["algorithm"] = json!(tag);
            } else {
                value["scheme"] = json!(if tag == 1 { "Sr25519" } else { "Ed25519" });
            }
            Ok(
                json!({"value":value,"variant":if version==1 {"Classical"} else if tag>=6 {"Falcon"} else {"MlDsa65"},"fixed":version==2 && matches!(tag,5|7|9),"storage":if version==2 {"MlDsaAccounts"} else {"Accounts"}}),
            )
        }
        "transaction.fixed_descriptor" => {
            let descriptor = bytes(&input["descriptor"], "descriptor")?;
            Ok(json!(
                (descriptor.len() == 66 || input["header_only"] == true && descriptor.len() >= 2)
                    && descriptor[0] == 2
                    && if input["mldsa_only"] == true {
                        descriptor[1] == 5
                    } else {
                        matches!(descriptor[1], 5 | 7 | 9)
                    }
            ))
        }
        "transaction.descriptor_encode" => {
            let value = &input["value"];
            let version = uint(&value["version"], "descriptor version")?;
            let tag = if version == 2 {
                uint(&value["algorithm"], "descriptor algorithm")?
            } else {
                let scheme = value["scheme"].as_str().or_else(|| {
                    value["scheme"]
                        .as_object()
                        .filter(|object| object.len() == 1)
                        .and_then(|object| object.keys().next())
                        .map(String::as_str)
                });
                match scheme {
                    Some("Sr25519") => 1,
                    Some("Ed25519") => 3,
                    _ => return Err(PolicyError::type_error("unsupported account descriptor")),
                }
            };
            let mut descriptor = vec![
                u8::try_from(version)
                    .map_err(|_| PolicyError::type_error("invalid descriptor version"))?,
                u8::try_from(tag)
                    .map_err(|_| PolicyError::type_error("invalid descriptor algorithm"))?,
            ];
            descriptor.extend(bytes(&value["initial_commitment"], "initial commitment")?);
            let encoded = format!("0x{}", hex::encode(descriptor));
            evaluate("transaction.descriptor", json!({"descriptor":encoded}))?;
            Ok(json!(encoded))
        }
        "transaction.evm_alias" => {
            let alias = bytes(&input["alias"], "EVM address")?;
            let account = bytes(&input["account"], "bound account")?;
            if alias.len() != 20 || account.len() != 32 || account[..20] != alias {
                return Err(PolicyError::invalid("invalid protected EVM alias"));
            }
            if !input["descriptor_account"].is_null()
                && bytes(&input["descriptor_account"], "descriptor account")? != account
            {
                return Err(PolicyError::invalid(
                    "EVM alias descriptor differs from bound account",
                ));
            }
            Ok(Value::Null)
        }
        "transaction.evm_mirror" => {
            let address = bytes(&input["address"], "EVM address")?;
            if address.len() != 20 {
                return Err(PolicyError::type_error("expected a 20-byte EVM address"));
            }
            let preimage = [b"evm:".as_slice(), &address].concat();
            Ok(json!(format!(
                "0x{}",
                hex::encode(sp_core::hashing::blake2_256(&preimage))
            )))
        }
        "transaction.evm_truncated" => {
            let account = bytes(&input["account"], "account")?;
            if account.len() != 32 {
                return Err(PolicyError::type_error("public key must be 32 bytes"));
            }
            Ok(json!(format!("0x{}", hex::encode(&account[..20]))))
        }
        "transaction.evm_association_payload" => {
            let account = bytes(&input["hotkey"], "hotkey")?;
            let block = u64::try_from(uint(&input["block"], "block number")?)
                .map_err(|_| PolicyError::range_error("block number exceeds u64"))?;
            if account.len() != 32 {
                return Err(PolicyError::type_error("hotkey must be 32 bytes"));
            }
            let payload = [
                account.as_slice(),
                &sp_core::hashing::keccak_256(&block.to_le_bytes()),
            ]
            .concat();
            Ok(json!(format!("0x{}", hex::encode(payload))))
        }
        "transaction.signer_identity" => {
            let actual = &input["actual"];
            let expected = &input["expected"];
            let reason = if ["account", "public_key", "crypto_type"]
                .iter()
                .any(|key| actual[*key] != expected[*key])
            {
                Some("identity")
            } else if !expected["descriptor"].is_null()
                && actual["descriptor"] != expected["descriptor"]
            {
                Some("descriptor")
            } else if [
                "requires_metadata_proof",
                "requires_metadata_hash",
                "genesis_hash",
            ]
            .iter()
            .any(|key| actual[*key] != expected[*key])
            {
                Some("requirements")
            } else {
                None
            };
            Ok(json!(reason))
        }
        "transaction.authority_nonce" => {
            let finalized = uint(&input["finalized"], "finalized nonce")?;
            let pending = uint(&input["pending"], "pending nonce")?;
            let offset = optional_uint(&input["offset"], "generation offset")?.unwrap_or(0);
            let requested = optional_uint(&input["requested"], "requested nonce")?;
            Ok(json!(if pending != finalized {
                Some("pending")
            } else if requested.is_some_and(|nonce| Some(nonce) != finalized.checked_add(offset)) {
                Some("requested")
            } else {
                None
            }))
        }
        "transaction.authority_snapshot" => {
            let descriptor = bytes(&input["descriptor"], "descriptor")?;
            let expected = bytes(&input["expected_descriptor"], "wallet descriptor")?;
            let commitment = bytes(&input["commitment"], "commitment")?;
            let generation = uint(&input["generation"], "generation")?;
            let offset = optional_uint(&input["offset"], "generation offset")?.unwrap_or(0);
            let reason = if descriptor != expected {
                Some("descriptor")
            } else if generation
                .checked_add(offset)
                .is_none_or(|value| value >= u128::from(u64::MAX))
            {
                Some("generation")
            } else if commitment.len()
                != if descriptor.first() == Some(&2) {
                    64
                } else {
                    32
                }
            {
                Some("commitment")
            } else {
                None
            };
            Ok(json!(reason))
        }
        "transaction.multisig_eligible" => {
            let count = uint(&input["count"], "member count")?;
            let threshold = uint(&input["threshold"], "threshold")?;
            let weak = uint(&input["weak"], "weak members")?;
            Ok(json!(
                (2..=100).contains(&count)
                    && threshold >= 1
                    && threshold <= count
                    && weak < threshold
            ))
        }
        "transaction.derivative" => {
            if uint(&input["index"], "derivative index")? > 65535 {
                return Err(PolicyError::range_error("derivative index must fit u16"));
            }
            let mode = if input["mode"].is_null() {
                if account_profile(&input["crypto_type"])["fixed_pq"] == true {
                    "safe"
                } else {
                    "legacy"
                }
            } else {
                input["mode"].as_str().unwrap_or("")
            };
            evaluate("transaction.account_mode", json!({"mode":mode}))?;
            Ok(
                json!({"mode":mode,"call":if mode=="safe" {"as_safe_derivative"} else if input["pq_required"]==true {"as_protected_derivative"} else {"as_derivative"}}),
            )
        }
        "transaction.http_admission" => {
            let result = match input["phase"].as_str() {
                Some("trusted") => {
                    let profile = account_profile(&input["crypto_type"]);
                    if profile["pq"] == true {
                        Some("pq")
                    } else if matches!(input["crypto_type"].as_u64(), Some(0 | 1 | 4 | 6)) {
                        None
                    } else {
                        Some("unsupported")
                    }
                }
                Some("scheme") => {
                    if input["expected"].is_null() {
                        Some("missing")
                    } else if input["declared"] != input["expected"] {
                        Some("mismatch")
                    } else {
                        None
                    }
                }
                Some("receiver") => {
                    if input["receiver"].is_null() {
                        if input["required"] == true {
                            Some("missing")
                        } else {
                            None
                        }
                    } else if input["receiver"] != input["self"] {
                        Some("mismatch")
                    } else {
                        None
                    }
                }
                Some("retired") => {
                    if matches!(input["crypto_type"].as_u64(), Some(4 | 6))
                        && uint(&input["generation"], "generation")? == 0
                    {
                        Some("generation_zero")
                    } else {
                        None
                    }
                }
                Some("retention") => {
                    if input["retention"].is_null() {
                        None
                    } else {
                        let decimal = |field: &str| {
                            super::numeric::decimal(
                                &input[field]
                                    .as_str()
                                    .map(str::to_owned)
                                    .unwrap_or_else(|| input[field].to_string()),
                                1000,
                            )
                        };
                        if decimal("retention")? < decimal("max_age")? + decimal("allowed_skew")? {
                            Some("too_short")
                        } else {
                            None
                        }
                    }
                }
                _ => return Err(PolicyError::type_error("unknown HTTP admission phase")),
            };
            Ok(json!(result))
        }
        "transaction.external_signer" => Ok(json!({
            "ordinary":matches!(input["crypto_type"].as_u64(), Some(0..=2)),
            "identity_matches":input["account"]==input["public_key"],
            "metadata_required":input["requires_metadata_hash"]==true || input["requires_metadata_proof"]==true,
            "metadata_disabled":input["metadata_disabled"]==true && (input["requires_metadata_hash"]==true || input["requires_metadata_proof"]==true)
        })),
        "transaction.http_scheme" => {
            let scheme = match input["crypto_type"].as_u64() {
                Some(0) => "ed25519",
                Some(1) => "sr25519",
                Some(4) => "hashed-sr25519",
                Some(6) => "hashed-ed25519",
                _ => return Err(PolicyError::type_error("unsupported signature scheme")),
            };
            Ok(json!(scheme))
        }
        "transaction.http_crypto" => {
            let scheme = input["scheme"].as_str().unwrap_or("").to_lowercase();
            let code = match scheme.as_str() {
                "0" | "ed25519" => 0,
                "1" | "sr25519" => 1,
                "4" | "hashed-sr25519" => 4,
                "6" | "hashed-ed25519" => 6,
                _ => return Err(PolicyError::type_error("unsupported signature scheme")),
            };
            Ok(json!(code))
        }
        "transaction.http_payload" => {
            let text = |name: &str| {
                input[name]
                    .as_str()
                    .ok_or_else(|| PolicyError::type_error(format!("HTTP {name} must be a string")))
            };
            let scheme = text("scheme")?;
            let body = bytes(&input["body"], "body")?;
            let payload = [
                "btauth/1".to_string(),
                scheme.to_string(),
                text("method")?.to_uppercase(),
                text("path")?.to_string(),
                hex::encode(Sha256::digest(body)),
                text("nonce")?.to_string(),
                text("sender")?.to_string(),
                input["receiver"]
                    .as_str()
                    .filter(|value| !value.is_empty())
                    .unwrap_or("-")
                    .to_string(),
            ]
            .join("\n");
            Ok(json!(format!("0x{}", hex::encode(payload.as_bytes()))))
        }
        "transaction.http_freshness" => {
            let integer = |name: &str| -> Result<num_bigint::BigInt, PolicyError> {
                input[name]
                    .as_str()
                    .ok_or_else(|| {
                        PolicyError::type_error(format!("{name} requires decimal nanoseconds"))
                    })?
                    .parse()
                    .map_err(|_| PolicyError::type_error(format!("invalid {name}")))
            };
            let now = integer("now")?;
            let nonce = integer("nonce")?;
            let age = integer("max_age")?;
            let skew = integer("allowed_skew")?;
            if age < num_bigint::BigInt::from(0) || skew < num_bigint::BigInt::from(0) {
                return Err(PolicyError::range_error(
                    "freshness limits must be nonnegative",
                ));
            }
            Ok(json!(if &now - &nonce > age {
                "stale"
            } else if nonce - now > skew {
                "future"
            } else {
                "fresh"
            }))
        }
        "transaction.nested_dispatch_error" => {
            Ok(nested_dispatch_error(&input["events"]).unwrap_or(Value::Null))
        }
        _ => Err(PolicyError::invalid(format!(
            "unknown transaction policy operation {operation}"
        ))),
    }
}

fn intent_metadata(input: &Value) -> Result<Value, PolicyError> {
    let op = input["op"]
        .as_str()
        .ok_or_else(|| PolicyError::type_error("intent needs an op"))?;
    let args = &input["args"];
    let signer = match op {
        "add_proxy" if !args["from_hotkey"].is_null() => match args["from_hotkey"].as_bool() {
            Some(true) => "hotkey",
            Some(false) => "coldkey",
            None => return Err(PolicyError::type_error("from_hotkey must be boolean")),
        },
        "add_proxy" | "remove_proxy" => {
            if matches!(args["proxy_type"].as_str(), Some("Validate" | "Weights")) {
                "hotkey"
            } else {
                "coldkey"
            }
        }
        "remove_proxies" => {
            if args["from_hotkey"] == true {
                "hotkey"
            } else {
                "coldkey"
            }
        }
        _ => input["signer"].as_str().unwrap_or("coldkey"),
    };
    let mut netuids = Vec::new();
    let mut add = |value: &Value| -> Result<(), PolicyError> {
        let number = uint(value, "netuid")?;
        if number > 65535 {
            return Err(PolicyError::range_error("netuid exceeds 65535"));
        }
        let number = number as u16;
        if !netuids.contains(&number) {
            netuids.push(number);
        }
        Ok(())
    };
    if matches!(op, "root_register" | "set_root_claim_threshold") {
        add(&json!(0))?;
    } else if op == "set_subnet_emission_enabled" {
        for value in args["netuids"].as_array().into_iter().flatten() {
            add(value)?;
        }
    } else {
        for key in ["netuid", "origin_netuid", "dest_netuid"] {
            if !args[key].is_null() {
                add(&args[key])?;
            }
        }
    }
    let global = matches!(
        op,
        "transfer"
            | "transfer_all"
            | "fund_evm_key"
            | "evm_withdraw"
            | "register_subnet"
            | "register_leased_network"
            | "terminate_lease"
            | "set_identity"
            | "set_take"
            | "increase_take"
            | "decrease_take"
            | "set_reject_staking_hotkey_growth"
            | "set_reject_locked_alpha"
            | "associate_hotkey"
            | "remove_proxies"
    ) || op.contains("coldkey_swap")
        || op.contains("crowdloan")
        || op.contains("proxy");
    let affects_all = matches!(
        op,
        "unstake_all"
            | "unstake_all_alpha"
            | "swap_coldkey_announced"
            | "finalize_crowdloan"
            | "claim_root"
            | "claim_root_with_hotkey"
            | "stake_into_basket"
    ) || (op == "swap_hotkey" && args["netuid"].is_null())
        || (op == "set_subnet_gamma_emission_whitelisted" && args["whitelisted"] == false);
    let unbounded = matches!(
        op,
        "transfer_all"
            | "burned_register"
            | "register_subnet"
            | "register_leased_network"
            | "transfer_stake"
            | "add_collateral"
            | "finalize_crowdloan"
    );
    let field = if op == "create_crowdloan" {
        Some("deposit_tao")
    } else if matches!(
        op,
        "transfer"
            | "fund_evm_key"
            | "add_stake"
            | "add_stake_limit"
            | "stake_into_basket"
            | "stake_burn"
            | "contribute_crowdloan"
    ) {
        Some("amount_tao")
    } else {
        None
    };
    let spend = if unbounded || field.is_some_and(|field| args[field] == "all") {
        json!("unbounded")
    } else if let Some(field) = field {
        json!(uint(&args[field], field)?.to_string())
    } else {
        Value::Null
    };
    Ok(
        json!({"signer":signer,"netuids":netuids,"global":global,"affects_all_subnets":affects_all,"spend_rao":spend}),
    )
}

fn merge_intents(input: &Value) -> Result<Value, PolicyError> {
    let children = input["children"]
        .as_array()
        .ok_or_else(|| PolicyError::type_error("children must be an array"))?;
    if input["batch"] == true && children.is_empty() {
        return Err(PolicyError::range_error(
            "batch requires at least one intent",
        ));
    }
    let mut roles = BTreeSet::new();
    let mut scope = BTreeSet::new();
    let mut amount = None;
    let mut unbounded = false;
    let mut all = false;
    let mut global = false;
    let mut shield_default = false;
    let mut shield_required = false;
    for child in children {
        if let Some(role) = child["signer"].as_str() {
            roles.insert(role);
        }
        if input["batch"] == true && child["op"] == "batch" {
            return Err(PolicyError::range_error("nested batches are not supported"));
        }
        if input["batch"] == true && child["requires_direct"] == true {
            return Err(PolicyError::range_error(
                "announced proxy with an inner batch or uninspectable payload: submit it directly",
            ));
        }
        unbounded |= child["spend_rao"] == "unbounded";
        if !child["spend_rao"].is_null() && child["spend_rao"] != "unbounded" {
            amount = Some(checked_add(
                amount.unwrap_or(0),
                uint(&child["spend_rao"], "spend")?,
            )?);
        }
        for value in child["netuids"].as_array().into_iter().flatten() {
            scope.insert(uint(value, "netuid")?);
        }
        all |= child["affects_all_subnets"] == true;
        global |= child["global"] == true;
        shield_default |= child["shield_default"] == true;
        shield_required |= child["shield_required"] == true;
    }
    if input["batch"] == true && roles.len() != 1 {
        return Err(PolicyError::range_error(
            "all batch intents must share a signer",
        ));
    }
    Ok(
        json!({"signer":roles.first(),"spend_rao":if unbounded {json!("unbounded")} else {amount.map(|value|value.to_string()).into()},"netuids":scope,"affects_all_subnets":all,"global":global,"shield_default":shield_default,"shield_required":shield_required}),
    )
}

/// Deterministic call shape. Values are already unit-normalized by each language
/// edge; this structural operation deliberately preserves opaque codec scalars.
fn simple_call(input: &Value) -> Result<Value, PolicyError> {
    let op = input["op"]
        .as_str()
        .ok_or_else(|| PolicyError::type_error("intent needs an op"))?;
    let args = &input["args"];
    let mut params = serde_json::Map::new();
    let (pallet, call, fields): (&str, &str, &[(&str, &str)]) = match op {
        "transfer" | "transfer_all" => {
            let all = op == "transfer_all" || args["amount_tao"] == "all";
            if all {
                (
                    "Balances",
                    "transfer_all",
                    &[("dest", "dest_ss58"), ("keep_alive", "keep_alive")],
                )
            } else {
                (
                    "Balances",
                    if args["keep_alive"] == true {
                        "transfer_keep_alive"
                    } else {
                        "transfer_allow_death"
                    },
                    &[("dest", "dest_ss58"), ("value", "amount_tao")],
                )
            }
        }
        "set_auto_stake" => (
            "SubtensorModule",
            "set_coldkey_auto_stake_hotkey",
            &[("netuid", "netuid"), ("hotkey", "hotkey")],
        ),
        "set_reject_staking_hotkey_growth" | "set_reject_locked_alpha" => {
            ("SubtensorModule", op, &[("enabled", "enabled")])
        }
        "set_perpetual_lock" => (
            "SubtensorModule",
            op,
            &[("netuid", "netuid"), ("enabled", "enabled")],
        ),
        "move_lock" => (
            "SubtensorModule",
            op,
            &[
                ("netuid", "netuid"),
                ("destination_hotkey", "destination_hotkey_ss58"),
            ],
        ),
        "set_min_collateral" => (
            "SubtensorModule",
            op,
            &[
                ("netuid", "netuid"),
                ("hotkey", "hotkey"),
                ("min_locked", "min_alpha"),
            ],
        ),
        "burned_register" => (
            "SubtensorModule",
            op,
            &[("netuid", "netuid"), ("hotkey", "hotkey")],
        ),
        "root_register" => ("SubtensorModule", op, &[("hotkey", "hotkey")]),
        "register_subnet" => (
            "SubtensorModule",
            "register_network",
            &[("hotkey", "hotkey")],
        ),
        "start_call" => ("SubtensorModule", op, &[("netuid", "netuid")]),
        "swap_hotkey" => (
            "SubtensorModule",
            op,
            &[
                ("hotkey", "hotkey"),
                ("new_hotkey", "new_hotkey_ss58"),
                ("netuid", "netuid"),
            ],
        ),
        "associate_hotkey" => (
            "SubtensorModule",
            "try_associate_hotkey",
            &[("hotkey", "hotkey")],
        ),
        "announce_coldkey_swap" => (
            "SubtensorModule",
            "announce_coldkey_swap_v2",
            &[("new_coldkey", "new_coldkey_ss58")],
        ),
        "swap_coldkey_announced" => (
            "SubtensorModule",
            op,
            &[("new_coldkey", "new_coldkey_ss58")],
        ),
        "clear_coldkey_swap_announcement"
        | "dispute_coldkey_swap"
        | "release_own_coldkey_swap_destination_reservation" => ("SubtensorModule", op, &[]),
        "trim_subnet" => (
            "AdminUtils",
            "sudo_trim_to_max_allowed_uids",
            &[("netuid", "netuid"), ("max_n", "max_n")],
        ),
        "trim_null_subnet_batch" => (
            "AdminUtils",
            "sudo_trim_null_uids_batch",
            &[("netuid", "netuid"), ("target", "target")],
        ),
        "set_mechanism_count" => (
            "AdminUtils",
            "sudo_set_mechanism_count",
            &[("netuid", "netuid"), ("mechanism_count", "mechanism_count")],
        ),
        "set_subnet_gamma_emission_whitelisted" => (
            "AdminUtils",
            "sudo_set_subnet_gamma_emission_whitelisted",
            &[("netuid", "netuid"), ("whitelisted", "whitelisted")],
        ),
        "set_root_claim_threshold" => {
            params.insert("netuid".into(), json!(0));
            (
                "SubtensorModule",
                "sudo_set_root_claim_threshold",
                &[("new_value", "threshold")],
            )
        }
        "register_leased_network" => (
            "SubtensorModule",
            op,
            &[
                ("emissions_share", "emissions_share"),
                ("end_block", "end_block"),
            ],
        ),
        "terminate_lease" => (
            "SubtensorModule",
            op,
            &[("lease_id", "lease_id"), ("hotkey", "hotkey")],
        ),
        "add_proxy" | "remove_proxy" => (
            "Proxy",
            op,
            &[
                ("delegate", "delegate_ss58"),
                ("proxy_type", "proxy_type"),
                ("delay", "delay"),
            ],
        ),
        "remove_proxies" => ("Proxy", op, &[]),
        "announce_proxy_call" => (
            "Proxy",
            "announce",
            &[("real", "real_ss58"), ("call_hash", "call_hash")],
        ),
        "remove_proxy_announcement" => (
            "Proxy",
            "remove_announcement",
            &[("real", "real_ss58"), ("call_hash", "call_hash")],
        ),
        "reject_proxy_announcement" => (
            "Proxy",
            "reject_announcement",
            &[("delegate", "delegate_ss58"), ("call_hash", "call_hash")],
        ),
        "finalize_crowdloan" => ("Crowdloan", "finalize", &[("crowdloan_id", "crowdloan_id")]),
        "refund_crowdloan" => ("Crowdloan", "refund", &[("crowdloan_id", "crowdloan_id")]),
        "dissolve_crowdloan" => ("Crowdloan", "dissolve", &[("crowdloan_id", "crowdloan_id")]),
        "withdraw_crowdloan" => ("Crowdloan", "withdraw", &[("crowdloan_id", "crowdloan_id")]),
        "update_crowdloan_cap" => (
            "Crowdloan",
            "update_cap",
            &[("crowdloan_id", "crowdloan_id"), ("new_cap", "new_cap_tao")],
        ),
        "update_crowdloan_end" => (
            "Crowdloan",
            "update_end",
            &[("crowdloan_id", "crowdloan_id"), ("new_end", "new_end")],
        ),
        "update_crowdloan_min_contribution" => (
            "Crowdloan",
            "update_min_contribution",
            &[
                ("crowdloan_id", "crowdloan_id"),
                ("new_min_contribution", "new_min_contribution_tao"),
            ],
        ),
        "set_crowdloan_max_contribution" => (
            "Crowdloan",
            "set_max_contribution",
            &[
                ("crowdloan_id", "crowdloan_id"),
                ("new_max_contribution", "new_max_contribution_tao"),
            ],
        ),
        _ => {
            return Err(PolicyError::invalid(format!(
                "unsupported simple intent {op}"
            )))
        }
    };
    for (target, source) in fields {
        params.insert((*target).into(), args[*source].clone());
    }
    Ok(json!({"pallet":pallet,"call":call,"params":params}))
}

fn receiving_target(op: &str) -> Option<&'static str> {
    match op {
        "transfer" | "transfer_all" => Some("dest_ss58"),
        "announce_coldkey_swap" | "swap_coldkey_announced" => Some("new_coldkey_ss58"),
        "swap_hotkey" => Some("new_hotkey_ss58"),
        "burned_register" | "pow_register" | "root_register" | "register_subnet"
        | "associate_hotkey" => Some("hotkey_ss58"),
        _ => None,
    }
}

pub(crate) fn nested_result_field(module: &str, name: &str) -> Option<&'static str> {
    match (module, name) {
        ("Sudo", "Sudid" | "SudoAsDone") => Some("sudo_result"),
        ("Proxy", "ProxyExecuted") | ("Multisig", "MultisigExecuted") => Some("result"),
        ("Utility", "BatchInterrupted" | "ItemFailed") => Some("error"),
        _ => None,
    }
}

const PROXY_TYPES: &[&str] = &[
    "Any",
    "Owner",
    "NonCritical",
    "NonTransfer",
    "Senate",
    "NonFungible",
    "Triumvirate",
    "Governance",
    "Staking",
    "Registration",
    "Transfer",
    "SmallTransfer",
    "RootWeights",
    "ChildKeys",
    "SudoUncheckedSetCode",
    "SwapHotkey",
    "SubnetLeaseBeneficiary",
    "RootClaim",
    "BasketTrading",
    "Validate",
    "Weights",
    "Lock",
    "GammaSpend",
];

fn serving(input: &Value) -> Result<Value, PolicyError> {
    let op = input["op"].as_str().unwrap_or("");
    if !matches!(
        op,
        "serve_axon" | "serve_axon_tls" | "serve_prometheus" | "reset_axon"
    ) {
        return Err(PolicyError::type_error("invalid serving operation"));
    }
    let args = &input["args"];
    let reset = op == "reset_axon";
    let (ip, version) = if reset {
        (0u128, 4)
    } else {
        let address = args["ip"]
            .as_str()
            .ok_or_else(|| PolicyError::type_error("invalid IP address"))?
            .parse::<std::net::IpAddr>()
            .map_err(|_| PolicyError::type_error("invalid IP address"))?;
        match address {
            std::net::IpAddr::V4(ip) => (u128::from(u32::from(ip)), 4),
            std::net::IpAddr::V6(ip) => (u128::from(ip), 6),
        }
    };
    let bounded = |value: &Value, max: u128, name: &str| -> Result<u128, PolicyError> {
        let value = uint(value, name)?;
        if value > max {
            Err(PolicyError::range_error(format!("{name} exceeds {max}")))
        } else {
            Ok(value)
        }
    };
    let mut params = json!({"netuid":bounded(&args["netuid"],65535,"netuid")?,"version":if reset {0}else{bounded(&args["version"],u128::from(u32::MAX),"version")?},"ip":ip.to_string(),"port":if reset {1}else{bounded(&args["port"],65535,"port")?},"ip_type":version});
    if op != "serve_prometheus" {
        params["protocol"] = json!(if reset {
            4
        } else {
            bounded(&args["protocol"], 255, "protocol")?
        });
        params["placeholder1"] = json!(0);
        params["placeholder2"] = json!(0);
    }
    if op == "serve_axon_tls" {
        let certificate = bytes(&args["certificate"], "certificate")?;
        if !(1..=65).contains(&certificate.len()) {
            return Err(PolicyError::range_error(
                "neuron certificate must have one algorithm byte and at most 64 key bytes",
            ));
        }
        params["certificate"] = args["certificate"].clone();
    }
    Ok(json!({"pallet":"SubtensorModule","call":if reset {"serve_axon"}else{op},"params":params}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multisig_cancellation_selects_only_the_original_commitment() -> Result<(), PolicyError> {
        let first = format!("0x{}", "11".repeat(32));
        let second = format!("0x{}", "22".repeat(32));
        let when = json!({"height":42,"index":3});
        let pending = json!([
            {"call_hash":first,"timepoint":{"height":41,"index":3}},
            {"call_hash":second,"timepoint":when},
        ]);
        assert_eq!(
            evaluate(
                "transaction.multisig_cancel",
                json!({"timepoint":when,"pending":pending})
            )?,
            json!({"call_hash":second,"timepoint":when})
        );
        assert_eq!(
            evaluate(
                "transaction.multisig_cancel",
                json!({"timepoint":when,"call_hash":first})
            )?,
            json!({"call_hash":first,"timepoint":when})
        );
        for input in [
            json!({"timepoint":when,"pending":[]}),
            json!({"timepoint":when,"pending":[{"call_hash":first,"timepoint":when},{"call_hash":second,"timepoint":when}]}),
            json!({"timepoint":when,"call_hash":"0x01"}),
            json!({"timepoint":{"height":4294967296u64,"index":0},"call_hash":first}),
            json!({"timepoint":{"height":1,"index":-1},"call_hash":first}),
        ] {
            assert!(evaluate("transaction.multisig_cancel", input).is_err());
        }
        Ok(())
    }

    #[test]
    fn policy_preserves_unbounded_and_exact_large_spend() -> Result<(), PolicyError> {
        let result = evaluate(
            "transaction.intent_policy",
            json!({"spend_rao":"9007199254740993", "additional_spend_rao":"2", "max_spend_rao":"9007199254740994"}),
        )?;
        assert_eq!(
            result["violations"],
            json!([{"code":"spend","actual":"9007199254740995","cap":"9007199254740994"}])
        );
        assert_eq!(
            evaluate(
                "transaction.intent_policy",
                json!({"spend_rao":"unbounded","max_spend_rao":"340282366920938463463374607431768211455"})
            )?["violations"][0]["code"],
            "unbounded"
        );
        assert!(evaluate("transaction.intent_policy", json!({"spend_rao":"340282366920938463463374607431768211455", "additional_spend_rao":"1"})).is_err());
        Ok(())
    }

    #[test]
    fn identity_and_authority_cannot_drift() -> Result<(), PolicyError> {
        let identity =
            json!({"account":"0x0102","public_key":"0x0304","crypto_type":6,"descriptor":"0x0506"});
        let mut changed = identity.clone();
        changed["public_key"] = json!("0x0708");
        assert_eq!(
            evaluate(
                "transaction.signer_identity",
                json!({"actual":changed,"expected":identity})
            )?,
            "identity"
        );
        assert_eq!(
            evaluate(
                "transaction.authority_nonce",
                json!({"finalized":"4","pending":"5","requested":"4"})
            )?,
            "pending"
        );
        assert_eq!(
            evaluate(
                "transaction.authority_nonce",
                json!({"finalized":"4","pending":"4","offset":1,"requested":"5"})
            )?,
            Value::Null
        );
        assert!(evaluate(
            "transaction.signing_bounds",
            json!({"nonce":"4294967294","offset":1,"period":8})
        )
        .is_err());
        Ok(())
    }

    #[test]
    fn shared_call_shapes_and_metadata_preserve_scope() -> Result<(), PolicyError> {
        let transfer = evaluate(
            "transaction.simple_call",
            json!({"op":"transfer","args":{"dest_ss58":"recipient","amount_tao":"9007199254740993","keep_alive":true}}),
        )?;
        assert_eq!(transfer["call"], "transfer_keep_alive");
        assert_eq!(transfer["params"]["value"], "9007199254740993");
        assert_eq!(
            evaluate(
                "transaction.intent_metadata",
                json!({"op":"add_proxy","args":{"proxy_type":"Validate","from_hotkey":false}})
            )?["signer"],
            "coldkey"
        );
        assert_eq!(
            evaluate(
                "transaction.intent_metadata",
                json!({"op":"transfer_stake","args":{"origin_netuid":2,"dest_netuid":3}})
            )?["spend_rao"],
            "unbounded"
        );
        assert!(evaluate(
            "transaction.merge_intents",
            json!({"batch":true,"children":[{"signer":"hotkey"},{"signer":"coldkey"}]})
        )
        .is_err());
        Ok(())
    }

    #[test]
    fn unknown_events_do_not_hide_a_later_nested_failure() {
        let failure = json!({"Module":{"index":7,"error":"0x01000000"}});
        let events = json!([{"unrelated":true},{"event":{"module_id":"Proxy","event_id":"ProxyExecuted","attributes":[{"Err":failure.clone()}]}}]);
        assert_eq!(nested_dispatch_error(&events), Some(failure));
    }
}
