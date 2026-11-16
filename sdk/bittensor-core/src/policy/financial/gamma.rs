use super::*;

fn units(value: &Value) -> Result<u64> {
    u64::try_from(decimal_scaled(value, 1_000_000_000, true, 1000)?)
        .map_err(|_| range("gamma amount exceeds u64"))
}
fn render(value: u64) -> String {
    let tail = format!("{:09}", value % 1_000_000_000);
    let tail = tail.trim_end_matches('0');
    format!(
        "{}{}{}",
        value / 1_000_000_000,
        if tail.is_empty() { "" } else { "." },
        tail
    )
}
fn reference(value: &Value) -> Result<String> {
    if value.is_null() {
        return Ok(format!("0x{}", "00".repeat(32)));
    }
    let text = value
        .as_str()
        .ok_or_else(|| range("a gamma spend reference must be 32 bytes as hex"))?;
    let raw = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .unwrap_or(text);
    if raw.len() != 64 || !raw.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(range("a gamma spend reference must be 32 bytes as hex"));
    }
    Ok(format!("0x{}", raw.to_lowercase()))
}
fn present(value: &Value) -> bool {
    !value.is_null()
}
fn own(value: &Value) -> Vec<Value> {
    let hotkey = value["hotkey"].as_str().unwrap_or("");
    if !truth(&value["exists"]) {
        vec![json!(format!(
            "there is no gamma deposit at {hotkey} on this subnet"
        ))]
    } else if !truth(&value["owned"]) {
        vec![json!(format!(
            "the gamma deposit at {hotkey} belongs to another coldkey"
        ))]
    } else {
        vec![]
    }
}

pub(super) fn evaluate(op: &str, input: &Value) -> Result<Value> {
    Ok(match op {
        "burn" => {
            let stake = u64_value(&input["stake"])?;
            let amount = if input["amount"] == "all" {
                u64::MAX
            } else {
                u64_value(&input["amount"])?
            };
            json!({"sent":amount.to_string(), "burned":amount.min(stake).to_string(), "empty":stake==0, "limited":amount>stake, "all":input["amount"]=="all"})
        }
        "nonempty" => json!(truth(&input["exists"]) && u64_value(&input["balance"])? > 0),
        "amount" => {
            if input["value"]
                .as_str()
                .is_some_and(|v| v.trim().eq_ignore_ascii_case("all"))
            {
                if truth(&input["allow_all"]) {
                    json!("all")
                } else {
                    return Err(range("gamma amount does not accept all"));
                }
            } else {
                json!(render(units(&input["value"])?))
            }
        }
        "units" => json!(units(&input["value"])?.to_string()),
        "from_units" => json!(render(u64_value(&input["value"])?)),
        "u64" => json!(u64_value(&input["value"])?.to_string()),
        "minimum" => {
            let tolerance = decimal_ratio(&input["tolerance"], 1000)?;
            if tolerance >= BigRational::from_integer(BigInt::from(1)) {
                return Err(range("rate_tolerance must be in [0, 1)"));
            }
            let value = BigRational::from_integer(BigInt::from(u64_value(&input["quoted"])?))
                * (BigRational::from_integer(BigInt::from(1)) - tolerance);
            json!(value.to_integer().to_string())
        }
        "reference" => json!(reference(&input["value"])?),
        "quote" => {
            let values = array(&input["value"])?;
            if values.len() != 3 {
                return Err(ty("invalid Gamma runtime quote"));
            }
            Value::Array(
                values
                    .iter()
                    .map(|value| u64_value(value).map(|n| json!(n.to_string())))
                    .collect::<Result<_>>()?,
            )
        }
        "arguments" => {
            let op = input["op"]
                .as_str()
                .ok_or_else(|| ty("missing Gamma operation"))?;
            let mut args = object(&input["args"])?.clone();
            let value = Value::Object(args.clone());
            if ["mint_gamma", "mint_gamma_with_stake"].contains(&op)
                && present(&value["spender_ss58"])
                && present(&value["service_hotkey_ss58"])
            {
                return Err(range(format!(
                    "{op}: name a spender or a service, not both"
                )));
            }
            if op == "move_gamma" {
                if value["origin_hotkey_ss58"] == value["destination_hotkey_ss58"] {
                    return Err(range(
                        "move_gamma: origin and destination hotkeys are the same",
                    ));
                }
                if value["amount_gamma"] != "all" && units(&value["amount_gamma"])? == 0 {
                    return Err(range("move_gamma: amount must be above 0"));
                }
            }
            if op == "set_gamma_deposit_spender"
                && usize::from(present(&value["spender_ss58"]))
                    + usize::from(present(&value["service_hotkey_ss58"]))
                    + usize::from(truth(&value["default_spender"]))
                    != 1
            {
                return Err(range("set_gamma_deposit_spender: set exactly one of spender_ss58, service_hotkey_ss58 or default_spender"));
            }
            if op == "set_gamma_deposit_daily_spend_cap"
                && usize::from(present(&value["daily_spend_cap_gamma"]))
                    + usize::from(truth(&value["remove_cap"]))
                    != 1
            {
                return Err(range("set_gamma_deposit_daily_spend_cap: set exactly one of daily_spend_cap_gamma or remove_cap"));
            }
            if op == "spend_gamma" {
                let spends = array(&value["spends"])?;
                if spends.is_empty() || spends.len() > 512 {
                    return Err(range("spend_gamma takes 1 to 512 spends"));
                }
                let mut normalized = Vec::new();
                for spend in spends {
                    let row = object(spend)?;
                    if !row.contains_key("hotkey_ss58")
                        || !row.contains_key("amount_gamma")
                        || row.keys().any(|k| {
                            !["hotkey_ss58", "amount_gamma", "reference"].contains(&k.as_str())
                        })
                    {
                        return Err(ty("spend_gamma entries require hotkey_ss58, amount_gamma and optional reference"));
                    }
                    let hotkey = spend["hotkey_ss58"]
                        .as_str()
                        .filter(|v| !v.trim().is_empty())
                        .ok_or_else(|| ty("hotkey_ss58 must be an address"))?;
                    normalized.push(json!({"hotkey_ss58":hotkey,"amount_gamma":render(units(&spend["amount_gamma"])?),"reference":reference(&spend["reference"])?}));
                }
                args.insert("spends".into(), json!(normalized));
            }
            if op == "set_service" {
                let value = value["url"]
                    .as_str()
                    .ok_or_else(|| ty("service url must be a string"))?;
                if value.len() > 256 {
                    return Err(range("service url exceeds 256 UTF-8 bytes"));
                }
                if !value.is_empty() {
                    let parsed = url::Url::parse(value)
                        .map_err(|_| range("service url must be an http(s) URL"))?;
                    if !["http", "https"].contains(&parsed.scheme()) || parsed.host_str().is_none()
                    {
                        return Err(range("service url must be an http(s) URL"));
                    }
                }
            }
            Value::Object(args)
        }
        "own_deposit" => json!(own(input)),
        "new_deposit" => {
            let mut out = vec![];
            let hotkey = input["hotkey"].as_str().unwrap_or("");
            let signer = input["signer"].as_str().unwrap_or("");
            if input["owner_exists"] != false && !truth(&input["owned"]) {
                out.push(json!(if input["owner_exists"].is_null(){format!("cannot verify that {hotkey} is unassociated or owned by {signer}; use a reader with storageExists")}else{format!("the hotkey {hotkey} belongs to another coldkey; use your own or an unassociated hotkey")}));
            }
            if !input["count"].is_null()
                && count(&input["count"])? - i128::from(truth(&input["closes_origin"])) >= 64
            {
                out.push(json!("at most 64 gamma deposits per subnet are allowed"));
            }
            json!(out)
        }
        "mint_flags" => {
            let exists = truth(&input["exists"]);
            let cap_changed =
                present(&input["requested_cap"]) && input["requested_cap"] != input["existing_cap"];
            json!({"wrong_owner":exists&&!truth(&input["owned"]),"wrong_spender":exists&&truth(&input["spender_named"])&&!truth(&input["same_spender"]),"wrong_cap":exists&&cap_changed,"overflow":exists&&i128::from(u64_value(&input["balance"])?) + i128::from(u64_value(&input["minted"])?) > ONE})
        }
        "move_flags" => {
            let balance = i128::from(u64_value(&input["balance"])?);
            let requested = i128::from(u64_value(&input["amount"])?);
            let moved = requested.min(balance);
            json!({"moved":moved.to_string(),"empty":truth(&input["origin_exists"])&&balance==0,"closes":truth(&input["origin_exists"])&&moved==balance,"frees_slot":truth(&input["origin_exists"])&&moved==balance&&balance>0,"overflow":truth(&input["destination_exists"])&&i128::from(u64_value(&input["destination_balance"])?) + moved>ONE,"spender_changes":truth(&input["origin_exists"])&&truth(&input["destination_exists"])&&!truth(&input["same_spender"]),"cap_changes":truth(&input["origin_exists"])&&truth(&input["destination_exists"])&&input["origin_cap"]!=input["destination_cap"]})
        }
        "spends" => {
            let mut deposits = object(&input["deposits"])?.clone();
            let mut skipped = vec![];
            let mut total = 0i128;
            for spend in array(&input["spends"])? {
                let key = spend["identity"]
                    .as_str()
                    .ok_or_else(|| ty("spend identity missing"))?;
                let amount = i128::from(u64_value(&spend["amount"])?);
                let current = deposits.get_mut(key);
                let reason = match current.as_deref() {
                    None | Some(Value::Null) => Some("GammaDepositDoesNotExist"),
                    Some(value) if !truth(&value["owned"]) => Some("NotGammaDepositSpender"),
                    Some(_) if amount == 0 => Some("AmountTooLow"),
                    Some(value) if amount > i128::from(u64_value(&value["balance"])?) => {
                        Some("InsufficientGammaBalance")
                    }
                    Some(value) if amount > i128::from(u64_value(&value["spendable"])?) => {
                        Some("GammaDepositDailySpendCapExceeded")
                    }
                    _ => None,
                };
                if let Some(reason) = reason {
                    skipped.push(json!({"hotkey_ss58":spend["hotkey"],"amount_gamma":render(amount as u64),"reference":spend["reference"],"reason":reason}));
                } else if let Some(current) = current {
                    current["balance"] =
                        encoded(i128::from(u64_value(&current["balance"])?) - amount);
                    current["spendable"] =
                        encoded(i128::from(u64_value(&current["spendable"])?) - amount);
                    add(&mut total, amount)?;
                }
            }
            json!({"total":total.to_string(),"skipped":skipped})
        }
        "service" => {
            let mut out = vec![];
            if !truth(&input["network"]) {
                out.push(json!(format!(
                    "subnet {} does not exist",
                    text(&input["netuid"])?
                )));
            }
            if !truth(&input["owned"]) {
                out.push(json!(
                    "the dispatch coldkey does not own the service hotkey"
                ));
            }
            if truth(&input["publishing"]) {
                if !truth(&input["registered"]) {
                    out.push(json!("the service hotkey is not registered on this subnet"));
                }
                if !truth(&input["subnet_owner"]) && !truth(&input["permit"]) {
                    out.push(json!(
                        "the service hotkey needs a validator permit or subnet ownership"
                    ));
                }
            }
            json!(out)
        }
        _ => return Err(invalid(format!("unknown Gamma policy operation {op}"))),
    })
}
