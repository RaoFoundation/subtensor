//! Pure staking routes and call construction. Reads and approved amounts are
//! supplied by adapters; this module never resolves `all` from mutable state.
use super::*;

fn subnet(value: &Value) -> Result<u16> {
    u16::try_from(integer(value)?).map_err(|_| range("invalid staking netuid"))
}
fn optional_subnet(value: &Value, default: u16) -> Result<u16> {
    if value.is_null() {
        Ok(default)
    } else {
        subnet(value)
    }
}
fn op(input: &Value) -> Result<&str> {
    input["op"]
        .as_str()
        .ok_or_else(|| ty("staking operation is required"))
}
fn root_limit(value: &Value) -> Result<()> {
    if !(0..=1_000_000_000).contains(&integer(value)?) {
        return Err(range(
            "root remove_stake_limit requires limit_price_rao in [0, 1000000000]",
        ));
    }
    Ok(())
}
fn claim(input: &Value) -> Result<Value> {
    if !truth(&input["claim"]) {
        return Ok(json!({"claim":false}));
    }
    if !input["netuid"].is_null() && subnet(&input["netuid"])? != 0 {
        return Ok(json!({"claim":false,"error":"non_root"}));
    }
    if !input["interval"].is_null() && count(&input["interval"])? > 0 {
        return Ok(json!({"claim":false,"error":"hold"}));
    }
    Ok(json!({"claim":true}))
}
fn decision(input: &Value) -> Result<Value> {
    let op = op(input)?;
    let args = &input["args"];
    for field in ["claim", "slippage_protection", "allow_partial"] {
        if !args[field].is_null() && !args[field].is_boolean() {
            return Err(ty(format!("{field} must be a boolean")));
        }
    }
    let netuid = optional_subnet(&args["netuid"], 0)?;
    let origin = optional_subnet(&args["origin_netuid"], netuid)?;
    let destination = optional_subnet(&args["dest_netuid"], origin)?;
    let changes_subnet = origin != destination;
    let source = input
        .get("origin_hotkey")
        .filter(|v| !v.is_null())
        .unwrap_or(&input["hotkey"]);
    let dest = &args["dest_hotkey_ss58"];
    let changes_hotkey = !dest.is_null() && source != dest;
    let wants_claim = truth(&args["claim"]);
    let all = args["amount_alpha"] == "all";
    let removal = ["remove_stake", "remove_stake_limit"].contains(&op);
    let moving = [
        "move_stake",
        "move_swap_stake",
        "swap_stake",
        "transfer_stake",
    ]
    .contains(&op);
    let claim_netuid = if moving { origin } else { netuid };
    if wants_claim && claim_netuid != 0 {
        return Err(range(
            "claim is only valid when the origin is root (netuid 0)",
        ));
    }
    let will_claim = wants_claim && claim_netuid == 0;
    let claim_all = will_claim && all;
    let protected = truth(&args["slippage_protection"]);
    let explicit = ["add_stake_limit", "remove_stake_limit"].contains(&op);
    let (function, direction, needs_limit) = match op {
        "add_stake" | "add_stake_limit" => {
            if explicit || protected {
                ("add_stake_limit", Some("buy"), true)
            } else {
                ("add_stake", None, false)
            }
        }
        "remove_stake" | "remove_stake_limit" => {
            if !claim_all && (explicit || protected) {
                ("remove_stake_limit", Some("sell"), true)
            } else {
                ("remove_stake", None, false)
            }
        }
        "move_stake" => {
            if protected && changes_subnet {
                ("move_stake_limit", Some("ratio"), true)
            } else {
                ("move_stake", None, false)
            }
        }
        "swap_stake" => {
            if protected {
                ("swap_stake_limit", Some("ratio"), true)
            } else {
                ("swap_stake", None, false)
            }
        }
        "move_swap_stake" => {
            if !changes_hotkey || !changes_subnet {
                return Err(range(
                    "move_swap_stake requires different hotkeys and subnets",
                ));
            }
            (
                "batch_all",
                if protected { Some("ratio") } else { None },
                protected,
            )
        }
        "transfer_stake" => (
            if changes_hotkey {
                "transfer_stake_and_hotkey"
            } else {
                "transfer_stake"
            },
            None,
            false,
        ),
        "add_collateral" | "stake_burn" => (
            if op == "stake_burn" {
                "add_stake_burn"
            } else {
                op
            },
            Some("buy"),
            true,
        ),
        "stake_into_basket"
        | "lock_stake"
        | "unstake_all"
        | "unstake_all_alpha"
        | "swap_basket"
        | "claim_root"
        | "claim_root_with_hotkey" => (op, None, false),
        _ => return Err(invalid(format!("unknown staking operation {op}"))),
    };
    if claim_all && op == "remove_stake_limit" && !args["limit_price_rao"].is_null() {
        root_limit(&args["limit_price_rao"])?;
    }
    let amount = if !input["amount"].is_null() {
        Some(u64_value(&input["amount"])?.to_string())
    } else if claim_all && (removal || moving) {
        Some(u64::MAX.to_string())
    } else {
        None
    };
    Ok(
        json!({"function":function,"needs_limit":needs_limit,"limit_direction":direction,"explicit_limit":explicit&&needs_limit,"allow_partial":explicit&&needs_limit&&truth(&args["allow_partial"]),"claim":will_claim,"claim_all":claim_all,"changes_subnet":changes_subnet,"changes_hotkey":changes_hotkey,"move_then_swap":op=="move_swap_stake","amount":amount}),
    )
}
fn amount(input: &Value) -> Result<Value> {
    let value = i128::from(u64_value(&input["amount"])?);
    let kind = input["kind"]
        .as_str()
        .ok_or_else(|| ty("staking amount kind is required"))?;
    let (value, message) = match kind {
        "stake" => (value, "no stake in the selected position"),
        "lock" => (value, "no unlocked stake available"),
        "collateral" => (
            value
                - i128::from(if input["locked"].is_null() {
                    0
                } else {
                    u64_value(&input["locked"])?
                }),
            "no free alpha to lock as collateral",
        ),
        "move_swap" => (
            value - 1,
            "move_swap_stake needs more than 1 rao to cover share-pool rounding",
        ),
        _ => return Err(invalid("unknown staking amount kind")),
    };
    if value < 0 || value == 0 && !truth(&input["allow_empty"]) {
        return Err(range(message));
    }
    Ok(encoded(value))
}
fn call(input: &Value) -> Result<Value> {
    let route = decision(input)?;
    let op = op(input)?;
    let args = &input["args"];
    if op == "remove_stake_limit" && truth(&route["claim_all"]) {
        root_limit(&args["limit_price_rao"])?;
    }
    let name = route["function"]
        .as_str()
        .ok_or_else(|| ty("missing staking route"))?;
    let hotkey = input["hotkey"].clone();
    let origin_hotkey = input
        .get("origin_hotkey")
        .filter(|v| !v.is_null())
        .cloned()
        .unwrap_or_else(|| hotkey.clone());
    let amount = route["amount"].clone();
    let descriptor =
        |name: &str, params: Value| json!({"pallet":"SubtensorModule","call":name,"params":params});
    let mut integer_paths: Vec<Vec<Value>> = vec![];
    let mut add_amount = |field: &str, params: &mut Value| -> Result<()> {
        if amount.is_null() {
            return Err(ty("resolved staking amount is required"));
        }
        params[field] = amount.clone();
        integer_paths.push(vec![json!("params"), json!(field)]);
        Ok(())
    };
    let mut params = json!({});
    match op {
        "add_stake" | "add_stake_limit" | "stake_into_basket" => {
            params["hotkey"] = hotkey;
            if op != "stake_into_basket" {
                params["netuid"] = args["netuid"].clone();
            }
            add_amount("amount_staked", &mut params)?;
        }
        "remove_stake" | "remove_stake_limit" => {
            params = json!({"hotkey":hotkey,"netuid":args["netuid"]});
            add_amount("amount_unstaked", &mut params)?;
        }
        "move_stake" | "swap_stake" | "transfer_stake" | "move_swap_stake" => {
            params = json!({"origin_netuid":args["origin_netuid"],"destination_netuid":args["dest_netuid"]});
            add_amount("alpha_amount", &mut params)?;
            if op == "transfer_stake" {
                params["destination_coldkey"] = args["dest_coldkey_ss58"].clone();
                if truth(&route["changes_hotkey"]) {
                    params["origin_hotkey"] = hotkey;
                    params["destination_hotkey"] = args["dest_hotkey_ss58"].clone();
                } else {
                    params["hotkey"] = hotkey;
                }
            } else if op == "swap_stake" {
                params["hotkey"] = hotkey;
            } else {
                params["origin_hotkey"] = origin_hotkey;
                params["destination_hotkey"] = args["dest_hotkey_ss58"].clone();
            }
        }
        "lock_stake" => {
            params = json!({"hotkey":hotkey,"netuid":args["netuid"]});
            add_amount("amount", &mut params)?;
        }
        "add_collateral" => {
            params = json!({"hotkey":hotkey,"netuid":args["netuid"]});
            add_amount("alpha", &mut params)?;
        }
        "stake_burn" => {
            params = json!({"hotkey":hotkey,"netuid":args["netuid"]});
            add_amount("amount", &mut params)?;
        }
        "swap_basket" => {
            if !truth(&route["changes_subnet"]) {
                return Err(range("basket origin and destination must differ"));
            }
            params = json!({"hotkey":hotkey,"origin_netuid":args["origin_netuid"],"destination_netuid":args["dest_netuid"],"min_amount_out":u64_value(&args["min_amount_out"])?.to_string()});
            add_amount("amount", &mut params)?;
            integer_paths.push(vec![json!("params"), json!("min_amount_out")]);
        }
        "claim_root" => {
            let rows = array(&args["subnets"])?;
            if rows.is_empty() {
                return Err(range("claim_root requires a nonempty subnet list"));
            }
            params["subnets"] = json!(rows.iter().map(subnet).collect::<Result<Vec<_>>>()?);
        }
        "claim_root_with_hotkey" | "unstake_all" | "unstake_all_alpha" => params["hotkey"] = hotkey,
        _ => return Err(invalid("unsupported staking call")),
    }
    if op == "move_swap_stake" {
        let mut movement = params.clone();
        movement["destination_netuid"] = args["origin_netuid"].clone();
        let mut swap = json!({"hotkey":args["dest_hotkey_ss58"],"origin_netuid":args["origin_netuid"],"destination_netuid":args["dest_netuid"],"alpha_amount":self::amount(&json!({"kind":"move_swap","amount":amount}))?});
        let swap_name = if truth(&route["needs_limit"]) {
            swap["limit_price"] = json!(u64_value(&input["limit_price"])?.to_string());
            swap["allow_partial"] = json!(false);
            "swap_stake_limit"
        } else {
            "swap_stake"
        };
        let mut paths = vec![
            vec![
                json!("params"),
                json!("calls"),
                json!(0),
                json!("params"),
                json!("alpha_amount"),
            ],
            vec![
                json!("params"),
                json!("calls"),
                json!(1),
                json!("params"),
                json!("alpha_amount"),
            ],
        ];
        if truth(&route["needs_limit"]) {
            paths.push(vec![
                json!("params"),
                json!("calls"),
                json!(1),
                json!("params"),
                json!("limit_price"),
            ]);
        }
        return Ok(
            json!({"call":{"pallet":"Utility","call":"batch_all","params":{"calls":[descriptor("move_stake",movement),descriptor(swap_name,swap)]}},"integer_paths":paths}),
        );
    }
    if truth(&route["needs_limit"]) {
        let field = if op == "stake_burn" {
            "limit"
        } else {
            "limit_price"
        };
        params[field] = json!(u64_value(&input["limit_price"])?.to_string());
        integer_paths.push(vec![json!("params"), json!(field)]);
        if !["add_collateral", "stake_burn"].contains(&op) {
            params["allow_partial"] = route["allow_partial"].clone();
        }
    }
    Ok(json!({"call":descriptor(name,params),"integer_paths":integer_paths}))
}
fn basket_legs(input: &Value) -> Result<Value> {
    let legs = array(&input["legs"])?;
    if legs.is_empty() || legs.len() > 128 {
        return Err(range("swap_basket_many requires 1 to 128 legs"));
    }
    let mut values = vec![];
    let mut netuids = vec![];
    for row in legs {
        if !row["fields"].is_null() {
            let fields = array(&row["fields"])?;
            for field in ["origin_netuid", "dest_netuid", "amount"] {
                if !fields.contains(&json!(field)) {
                    return Err(ty(format!("missing basket leg field {field}")));
                }
            }
            for field in fields {
                let field = field
                    .as_str()
                    .ok_or_else(|| ty("invalid basket leg field"))?;
                if !["origin_netuid", "dest_netuid", "amount", "min_amount_out"].contains(&field) {
                    return Err(ty(format!("unknown basket leg field {field}")));
                }
            }
        }
        let origin = subnet(&row["origin_netuid"])?;
        let destination = subnet(&row["dest_netuid"])?;
        if origin == destination {
            return Err(range("basket leg origin and destination must differ"));
        }
        for id in [origin, destination] {
            if !netuids.contains(&id) {
                netuids.push(id);
            }
        }
        values.push(json!([
            origin,
            destination,
            u64_value(&row["amount_rao"])?.to_string(),
            u64_value(&row["min_out_rao"])?.to_string()
        ]));
    }
    Ok(json!({"legs":values,"netuids":netuids}))
}

pub(super) fn evaluate(operation: &str, input: &Value) -> Result<Value> {
    match operation {
        "basket_legs" => basket_legs(input),
        "staking_decision" => decision(input),
        "staking_amount" => amount(input),
        "staking_claim" => claim(input),
        "staking_call" => call(input),
        _ => Err(invalid("unknown staking policy operation")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn run(op: &str, input: Value) -> Value {
        evaluate(op, &input).unwrap_or_else(|e| panic!("{}", e.message))
    }

    #[test]
    fn root_claim_all_uses_plain_unstake_and_live_balance_sentinel() {
        let input = json!({"op":"remove_stake_limit","hotkey":"validator","args":{"netuid":0,"claim":true,"amount_alpha":"all","allow_partial":true,"limit_price_rao":"1000000000"}});
        let route = run("staking_decision", input.clone());
        let mut invalid_boolean = input.clone();
        invalid_boolean["args"]["slippage_protection"] = json!(1);
        assert!(evaluate("staking_decision", &invalid_boolean).is_err());
        assert_eq!(route["function"], "remove_stake");
        assert_eq!(route["needs_limit"], false);
        assert_eq!(route["amount"], u64::MAX.to_string());
        let mut pinned = input.clone();
        pinned["amount"] = json!("100");
        assert_eq!(
            run("staking_call", pinned)["call"]["params"]["amount_unstaked"],
            "100"
        );
        let mut invalid_limit = input.clone();
        invalid_limit["args"]["limit_price_rao"] = json!("1000000001");
        assert!(evaluate("staking_call", &invalid_limit).is_err());
        let call = run("staking_call", input);
        assert_eq!(
            call["call"],
            json!({"pallet":"SubtensorModule","call":"remove_stake","params":{"hotkey":"validator","netuid":0,"amount_unstaked":u64::MAX.to_string()}})
        );
        assert_eq!(
            run(
                "staking_claim",
                json!({"claim":true,"netuid":0,"interval":1})
            )["error"],
            "hold"
        );
        assert_eq!(
            run("staking_claim", json!({"claim":true,"netuid":1}))["error"],
            "non_root"
        );
    }

    #[test]
    fn move_then_swap_keeps_atomicity_and_one_rao_rounding_allowance() {
        let input = json!({"op":"move_swap_stake","origin_hotkey":"first","hotkey":"first","amount":"1000","limit_price":"2000000000","args":{"origin_netuid":1,"dest_netuid":2,"dest_hotkey_ss58":"second","slippage_protection":true}});
        let value = run("staking_call", input);
        assert_eq!(value["call"]["pallet"], "Utility");
        assert_eq!(value["call"]["call"], "batch_all");
        assert_eq!(
            value["call"]["params"]["calls"][0]["params"]["destination_netuid"],
            1
        );
        assert_eq!(
            value["call"]["params"]["calls"][0]["params"]["alpha_amount"],
            "1000"
        );
        assert_eq!(
            value["call"]["params"]["calls"][1]["call"],
            "swap_stake_limit"
        );
        assert_eq!(
            value["call"]["params"]["calls"][1]["params"]["alpha_amount"],
            "999"
        );
        assert_eq!(
            value["call"]["params"]["calls"][1]["params"]["allow_partial"],
            false
        );
        assert!(evaluate("staking_amount", &json!({"kind":"move_swap","amount":"1"})).is_err());
        assert!(evaluate(
            "staking_amount",
            &json!({"kind":"collateral","amount":"9","locked":"10"})
        )
        .is_err());
        assert_eq!(
            run(
                "staking_amount",
                json!({"kind":"collateral","amount":"20","locked":"3"})
            ),
            "17"
        );
    }
}
