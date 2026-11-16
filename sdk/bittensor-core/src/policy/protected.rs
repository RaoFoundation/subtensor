//! Pure delayed-proxy policy shared by Node, browsers and Python.
//!
//! Address decoding and persistence are adapters. Identity-sensitive operations
//! receive canonical account identities, never display-address spellings.
use super::PolicyError;
use num_bigint::BigUint;
use serde_json::{json, Value};

type Result<T> = std::result::Result<T, PolicyError>;
fn text(value: &Value) -> &str {
    value.as_str().unwrap_or("")
}
fn rows(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn number(value: &Value) -> f64 {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|v| v.parse().ok()))
        .unwrap_or(0.0)
}
fn contains(value: &Value, name: &str) -> bool {
    rows(value).iter().any(|v| v.as_str() == Some(name))
}
fn unique(values: impl IntoIterator<Item = Value>) -> Vec<Value> {
    let mut out = Vec::new();
    for value in values {
        if !out.contains(&value) {
            out.push(value);
        }
    }
    out
}
fn depth_guard(depth: usize, message: &str) -> Result<()> {
    if depth > 64 {
        Err(PolicyError::range_error(message))
    } else {
        Ok(())
    }
}
fn uint(value: &Value, bits: usize, name: &str) -> Result<BigUint> {
    if let Value::Number(number) = value {
        let scalar = number.as_f64().unwrap_or(f64::NAN);
        if !scalar.is_finite() || scalar.fract() != 0.0 || scalar.abs() > 9_007_199_254_740_991.0 {
            return Err(PolicyError::range_error(format!(
                "{name} must be a safe integer or bigint"
            )));
        }
        if scalar < 0.0 {
            return Err(PolicyError::range_error(format!(
                "{name} must fit in u{bits}"
            )));
        }
    }
    let repr = match value {
        Value::String(v) => v.clone(),
        Value::Number(v) if v.is_u64() => v.to_string(),
        _ => {
            return Err(PolicyError::type_error(format!(
                "{name} must be an unsigned integer"
            )))
        }
    };
    if repr.is_empty() || !repr.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(PolicyError::type_error(format!(
            "{name} must be an unsigned integer"
        )));
    }
    let parsed = repr
        .parse::<BigUint>()
        .map_err(|_| PolicyError::type_error(format!("{name} must be an unsigned integer")))?;
    if parsed.bits() > bits as u64 {
        return Err(PolicyError::range_error(format!(
            "{name} must fit in u{bits}"
        )));
    }
    Ok(parsed)
}
fn decimal(value: &Value, name: &str) -> Result<BigUint> {
    uint(value, 256, name)
}
fn positive_blocks(value: f64, bounded: bool) -> Result<Value> {
    if !value.is_finite()
        || value.fract() != 0.0
        || value < 0.0
        || bounded && value > u32::MAX as f64
    {
        return Err(PolicyError::range_error("delay blocks must fit u32"));
    }
    if value == 0.0 {
        return Err(PolicyError::range_error(
            "delay must be greater than 0 blocks",
        ));
    }
    Ok(if bounded {
        json!(value as u32)
    } else {
        json!(format!("{value:.0}"))
    })
}

pub(crate) fn constants() -> Value {
    json!({
        "ALLOW_ITEM_TO_PROXY_TYPE": {"staking":"Staking","childkeys":"ChildKeys","childkey":"ChildKeys","basket":"BasketTrading","basket-swaps":"BasketTrading","basket_swaps":"BasketTrading","gamma":"GammaSpend","root-claim":"RootClaim","root_claim":"RootClaim","rootclaim":"RootClaim","validate":"Validate","weights":"Weights","owner":"Owner","swap-hotkey":"SwapHotkey","swap_hotkey":"SwapHotkey","subnet-lease":"SubnetLeaseBeneficiary","subnet_lease":"SubnetLeaseBeneficiary"},
        "FORBIDDEN_ALLOW_ITEMS": {"stake":"use 'staking' after reviewing the complete Staking authority","unstake":"use 'staking' after reviewing the complete Staking authority","registration":"Registration can burn protected-account TAO immediately","lock":"Lock can freeze protected-account stake permanently","any":"Any can transfer and change proxies immediately","transfer":"Transfer moves free balance immediately","smalltransfer":"SmallTransfer moves free balance immediately","small-transfer":"SmallTransfer moves free balance immediately","small_transfer":"SmallTransfer moves free balance immediately","nontransfer":"NonTransfer can manage proxies and defeat the delay","non-transfer":"NonTransfer can manage proxies and defeat the delay","non_transfer":"NonTransfer can manage proxies and defeat the delay","noncritical":"NonCritical can transfer free balance immediately","non-critical":"NonCritical can transfer free balance immediately","non_critical":"NonCritical can transfer free balance immediately"},
        "PROXY_TYPE_INTENT_OPS": {
            "Staking":["add_stake","remove_stake","move_stake","swap_stake","add_stake_limit","remove_stake_limit","unstake_all","unstake_all_alpha","stake_into_basket","claim_root","claim_root_with_hotkey"],
            "ChildKeys":["set_children","set_childkey_take"],"BasketTrading":["swap_basket","swap_basket_many"],"Registration":["burned_register","pow_register"],"Lock":["lock_stake","move_lock","set_perpetual_lock"],"GammaSpend":["spend_gamma"],"RootClaim":["claim_root","claim_root_with_hotkey"],"Validate":["serve_axon","serve_axon_tls","serve_prometheus","reset_axon","associate_evm_key"],"Weights":["set_weights","commit_weights","reveal_weights"],"Owner":["set_hyperparameter","set_subnet_identity"],"SwapHotkey":["swap_hotkey"],"SubnetLeaseBeneficiary":["start_call","set_subnet_identity","set_hyperparameter","set_service"]
        },
        "DEFEATING_ZERO_DELAY_TYPES":["Any","NonCritical","Transfer","SmallTransfer","NonTransfer"],
        "PROTECTED_MANAGEMENT_OPS":["add_proxy","remove_proxy","remove_proxies","kill_pure_proxy"],
        "PROTECTED_BYPASS_OPS":["announce_proxy_call","remove_proxy_announcement","reject_proxy_announcement","execute_proxy_announced","create_pure_proxy","announce_coldkey_swap","swap_coldkey_announced","clear_coldkey_swap_announcement","dispute_coldkey_swap","release_own_coldkey_swap_destination_reservation"],
        "ACTIVE_ANNOUNCEMENT_STATUSES":["pending","submitting","awaiting_approval"],
        "TRUSTED_ANNOUNCEMENT_LEAVES":TRUSTED_LEAVES
    })
}

fn delay(input: &Value) -> Result<Value> {
    let bounded = input["unbounded"] != true;
    if !bounded && input["integer_input"] == true {
        let raw = input["raw"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| input["raw"].to_string());
        let value = raw
            .parse::<num_bigint::BigInt>()
            .map_err(|_| PolicyError::type_error("invalid delay"))?;
        if value <= 0.into() {
            return Err(PolicyError::range_error(
                "delay must be greater than 0 blocks",
            ));
        }
        return Ok(json!(value.to_string()));
    }
    let block_time = input.get("block_time").map(number).unwrap_or(12.0);
    if !block_time.is_finite() || block_time <= 0.0 {
        return Err(PolicyError::range_error("block time must be positive"));
    }
    if input["raw"].is_number() {
        return positive_blocks(number(&input["raw"]), bounded);
    }
    let raw = text(&input["raw"]).trim().to_lowercase();
    let end = raw
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(raw.len());
    let digits = &raw[..end];
    let unit = raw[end..].trim();
    let valid_number = !digits.is_empty()
        && digits.split('.').count() <= 2
        && digits
            .split('.')
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()));
    let factor = match unit {
        "" | "b" | "block" | "blocks" => 0.0,
        "s" | "sec" | "secs" | "second" | "seconds" => 1.0,
        "m" | "min" | "mins" | "minute" | "minutes" => 60.0,
        "h" | "hr" | "hrs" | "hour" | "hours" => 3600.0,
        "d" | "day" | "days" => 86400.0,
        _ => -1.0,
    };
    if !valid_number || factor < 0.0 {
        return Err(PolicyError::type_error(
            "invalid delay; use blocks or a duration such as 36h, 2d or 90m",
        ));
    }
    let amount = digits
        .parse::<f64>()
        .map_err(|_| PolicyError::type_error("invalid delay"))?;
    positive_blocks(
        if factor == 0.0 {
            amount.trunc()
        } else {
            (amount * factor / block_time).round_ties_even()
        },
        bounded,
    )
}
fn allow_resolution(input: &Value) -> Value {
    let key = text(&input["item"]).trim().to_lowercase().replace(' ', "-");
    let catalog = constants();
    if let Some(reason) = catalog["FORBIDDEN_ALLOW_ITEMS"].get(&key) {
        json!({"kind":"forbidden","key":key,"reason":reason})
    } else if let Some(proxy_type) = catalog["ALLOW_ITEM_TO_PROXY_TYPE"].get(&key) {
        json!({"kind":"allowed","proxy_type":proxy_type})
    } else {
        json!({"kind":"unknown","key":key})
    }
}
fn grant_indices(input: &Value) -> Value {
    let mode = text(&input["mode"]);
    let target = &input["controller"];
    let catalog = constants();
    json!(rows(&input["grants"])
        .iter()
        .enumerate()
        .filter(|(_, row)| row["delegate"] == *target
            && match mode {
                "before_delay" => number(&row["delay"]) < number(&input["delay_blocks"]),
                "any" => row["proxy_type"] == "Any",
                "allow" => row["proxy_type"] != "Any" && number(&row["delay"]) == 0.0,
                "defeating" =>
                    number(&row["delay"]) == 0.0
                        && contains(
                            &catalog["DEFEATING_ZERO_DELAY_TYPES"],
                            text(&row["proxy_type"])
                        ),
                _ => false,
            })
        .map(|(index, _)| index)
        .collect::<Vec<_>>())
}
fn nested_intent(value: &Value) -> Value {
    let mut child = value["inner_args"].as_object().cloned().unwrap_or_default();
    child.insert("op".into(), value["inner_op"].clone());
    Value::Object(child)
}
fn management_path(value: &Value, depth: usize) -> Result<Option<String>> {
    depth_guard(depth, "protected intent nesting exceeds 64")?;
    let op = text(&value["op"]);
    if contains(&constants()["PROTECTED_MANAGEMENT_OPS"], op) {
        return Ok(Some(op.into()));
    }
    if op == "batch" {
        for (index, child) in rows(&value["intents"]).iter().enumerate() {
            if let Some(path) = management_path(child, depth.saturating_add(1))? {
                return Ok(Some(format!("batch[{index}].{path}")));
            }
        }
    }
    if op == "execute_proxy_announced" {
        if let Some(path) = management_path(&nested_intent(value), depth.saturating_add(1))? {
            return Ok(Some(format!("{op}.{path}")));
        }
    }
    if [
        "multisig_approve",
        "multisig_execute",
        "multisig_threshold_1",
        "create_crowdloan",
    ]
    .contains(&op)
        && !value["call"].is_null()
    {
        if let Some(path) = management_path(&value["call"], depth.saturating_add(1))? {
            return Ok(Some(format!("{op}.{path}")));
        }
    }
    Ok(None)
}
fn management_receipt(row: &Value, value: &Value) -> bool {
    let category = text(&row["category"]);
    let op = text(&value["op"]);
    let children = rows(&value["intents"]);
    if category.starts_with("allow:") || category.starts_with("disallow:") {
        return op
            == if category.starts_with("allow:") {
                "add_proxy"
            } else {
                "remove_proxy"
            }
            && text(&value["proxy_type"]) == category.split(':').nth(1).unwrap_or("")
            && number(&value["delay"]) == 0.0;
    }
    if category == "delay-change" {
        return op == "batch"
            && children.len() == 2
            && children[0]["op"] == "remove_proxy"
            && children[1]["op"] == "add_proxy"
            && children.iter().all(|c| c["proxy_type"] == "Any")
            && children[0]["delegate_ss58"] == children[1]["delegate_ss58"];
    }
    category == "rotate-controller"
        && op == "batch"
        && !text(&row["rotate_to_ss58"]).is_empty()
        && children.len() >= 2
        && children[0]["op"] == "remove_proxies"
        && children[1..].iter().all(|c| c["op"] == "add_proxy")
}
fn visit_grants(value: &Value, depth: usize, result: &mut Vec<Value>) -> Result<()> {
    depth_guard(depth, "protected intent nesting exceeds 64")?;
    match text(&value["op"]) {
        "add_proxy" => {
            let delay = uint(value.get("delay").unwrap_or(&json!(0)), 32, "delay blocks")?;
            let mut row = json!({"proxy_type":value.get("proxy_type").unwrap_or(&json!("Staking")),"delay":delay.to_string().parse::<u32>().unwrap_or(0)});
            if let Some(delegate) = value.get("delegate_ss58") {
                row["delegate_ss58"] = delegate.clone();
            }
            result.push(row);
        }
        "batch" => {
            for child in rows(&value["intents"]) {
                visit_grants(child, depth.saturating_add(1), result)?;
            }
        }
        "execute_proxy_announced" => {
            visit_grants(&nested_intent(value), depth.saturating_add(1), result)?
        }
        "multisig_approve" | "multisig_execute" | "multisig_threshold_1" | "create_crowdloan"
            if !value["call"].is_null() =>
        {
            visit_grants(&value["call"], depth.saturating_add(1), result)?
        }
        _ => (),
    }
    Ok(())
}
fn announcement_grants(row: &Value) -> Result<Value> {
    let mut value = nested_intent(row);
    if row["inner_op"].is_null() && text(&row["category"]).starts_with("allow:") {
        value["op"] = json!("add_proxy");
        if value["proxy_type"].is_null() {
            value["proxy_type"] = json!(&text(&row["category"])[6..]);
        }
    }
    let mut result = Vec::new();
    visit_grants(&value, 0, &mut result)?;
    Ok(json!(result))
}
// Exact arbitrary-precision amounts; subtraction is explicitly guarded.
#[allow(clippy::arithmetic_side_effects)]
fn sub(left: &BigUint, right: &BigUint) -> BigUint {
    if left > right {
        left - right
    } else {
        BigUint::default()
    }
}
// Exact arbitrary-precision amounts; subtraction is explicitly guarded.
#[allow(clippy::arithmetic_side_effects)]
fn funding_state(mut result: Value) -> Result<Value> {
    if !result.is_object() {
        return Err(PolicyError::type_error("invalid controller funding state"));
    }
    let field = |name| decimal(&result[name], name);
    let available = sub(&field("free_rao")?, &field("frozen_rao")?);
    let warning = field("warning_rao")?;
    let target = field("target_rao")?;
    let can_announce = available
        >= field("next_announcement_deposit_rao")?
            + field("transfer_fee_rao")?
            + field("existential_deposit_rao")?;
    result["available_rao"] = json!(available.to_string());
    result["low"] = json!(available < warning);
    result["top_up_rao"] = json!(sub(&target, &available).to_string());
    result["can_announce"] = json!(can_announce);
    result["estimate"] = json!(true);
    result["fee_scope"] = json!("controller; multisig signatories also need their own fees");
    Ok(result)
}
// Exact arbitrary-precision amounts; subtraction is explicitly guarded.
#[allow(clippy::arithmetic_side_effects)]
fn funding(input: &Value) -> Result<Value> {
    let amount = |name: &str, label: &str| uint(&input[name], 128, label);
    let count = uint(&input["announcement_count"], 32, "announcement count")?;
    let deposit = amount("announcement_deposit", "announcement deposit")?;
    let base = amount("deposit_base", "announcement deposit base")?;
    let factor = amount("deposit_factor", "announcement deposit factor")?;
    let ed = amount("existential_deposit", "existential deposit")?;
    let announce = amount("announce_fee", "announcement fee")?;
    let transfer = &announce + amount("transfer_execution_fee", "transfer execution fee")?;
    let staking = &announce + amount("staking_execution_fee", "staking execution fee")?;
    let reserve = |additional: u32| sub(&(&base + &factor * (&count + additional)), &deposit);
    let target = &ed + (reserve(20) + &transfer * 20u32).max(reserve(10) + &staking * 10u32);
    let warning = &ed + (reserve(5) + &transfer * 5u32).max(reserve(2) + &staking * 2u32);
    funding_state(
        json!({"controller":input["controller"],"free_rao":uint(&input["account"]["data"]["free"],128,"free balance")?.to_string(),"frozen_rao":uint(input["account"]["data"].get("frozen").unwrap_or(&json!(0)),128,"frozen balance")?.to_string(),"reserved_rao":uint(&input["account"]["data"]["reserved"],128,"reserved balance")?.to_string(),"announcement_deposit_rao":deposit.to_string(),"next_announcement_deposit_rao":reserve(1).to_string(),"transfer_fee_rao":transfer.to_string(),"staking_fee_rao":staking.to_string(),"target_rao":target.to_string(),"warning_rao":warning.to_string(),"existential_deposit_rao":ed.to_string(),"fee_payer":input["fee_payer"],"signature_estimate":input.get("signature_estimate").unwrap_or(&json!("standard sr25519 envelope; multisig members pay their own fees"))}),
    )
}
// Exact arbitrary-precision amounts; subtraction is explicitly guarded.
#[allow(clippy::arithmetic_side_effects)]
fn refill(input: &Value) -> Result<Value> {
    if input["controller"] != input["quote_controller"] {
        return Err(PolicyError::invalid(
            "funding quote belongs to another controller",
        ));
    }
    let state = &input["state"];
    let op = text(&input["op"]);
    let top = decimal(&state["top_up_rao"], "controller top-up")?;
    let available = decimal(&state["available_rao"], "available balance")?;
    let fees = decimal(&state["transfer_fee_rao"], "transfer fee")?
        .max(decimal(&state["staking_fee_rao"], "staking fee")?);
    let cost = decimal(&state["next_announcement_deposit_rao"], "next deposit")?
        + fees
        + decimal(&state["warning_rao"], "warning balance")?;
    let supported = evaluate("protected.refill_supported", json!({"op":op}))? == true;
    let reason = if input["enabled"] == false {
        "disabled"
    } else if !supported {
        "unsupported_operation"
    } else if top == BigUint::default() || state["low"] != true && available >= cost {
        "sufficient_funds"
    } else if state["can_announce"] != true {
        "external_funding_required"
    } else if input["pending"]["unknown"] == true {
        "unknown_pending_payload"
    } else if input["pending"]["unbounded"] == true
        || decimal(
            input["pending"].get("rao").unwrap_or(&json!(0)),
            "pending refill",
        )? > BigUint::default()
    {
        "already_pending"
    } else {
        "offered"
    };
    if reason == "offered"
        && !input["max_refill_rao"].is_null()
        && top > uint(&input["max_refill_rao"], 128, "maximum controller refill")?
    {
        return Err(PolicyError::range_error(
            "controller refill exceeds maxRefillRao",
        ));
    }
    Ok(
        json!({"controller_ss58":input["controller_address"],"amount_rao":if reason == "offered" {top.to_string()} else {"0".into()},"reason":reason,"offered":reason == "offered","requires_delayed":reason == "offered"}),
    )
}
fn refill_position(input: &Value) -> Result<Value> {
    let mut position = 0;
    for (index, child) in rows(&input["children"]).iter().enumerate() {
        let op = text(&child["op"]);
        if op == "batch" {
            return Err(PolicyError::range_error(
                "controller refill requires a flat atomic batch",
            ));
        }
        if [
            "add_stake",
            "add_stake_limit",
            "stake_into_basket",
            "contribute_crowdloan",
        ]
        .contains(&op)
            && text(&child["amount_tao"])
                .trim()
                .eq_ignore_ascii_case("all")
        {
            return Err(PolicyError::type_error("controller refill cannot be combined with a static free-balance amount_tao: all; use an explicit amount or fund the controller separately"));
        }
        if [
            "remove_stake",
            "remove_stake_limit",
            "unstake_all",
            "unstake_all_alpha",
            "claim_root",
            "claim_root_with_hotkey",
        ]
        .contains(&op)
        {
            position = index.saturating_add(1);
        }
    }
    Ok(json!(position))
}
fn category(op: &str) -> String {
    if ["transfer", "transfer_all"].contains(&op) {
        "transfer".into()
    } else if op.starts_with("remove_stake") || ["unstake_all", "unstake_all_alpha"].contains(&op) {
        "unstake".into()
    } else if op.starts_with("add_stake")
        || ["move_stake", "swap_stake", "move_swap_stake"].contains(&op)
    {
        "stake".into()
    } else if op.contains("basket") {
        "basket".into()
    } else if contains(&constants()["PROTECTED_MANAGEMENT_OPS"], op) {
        "proxy".into()
    } else if op.contains("coldkey") {
        "coldkey-swap".into()
    } else {
        op.replace('_', "-")
    }
}
fn lower(value: &Value) -> Value {
    value
        .as_str()
        .map(|v| json!(v.to_lowercase()))
        .unwrap_or(Value::Null)
}
fn same_commitment(left: &Value, right: &Value) -> bool {
    lower(&left["genesis_hash"]) == lower(&right["genesis_hash"])
        && ["controller_wallet", "delegate_ss58", "real_ss58"]
            .iter()
            .all(|key| left[key] == right[key])
        && lower(&left["call_hash"]) == lower(&right["call_hash"])
}
fn repeat_approval_required(row: &Value) -> bool {
    row["status"] == "executed" || row["execution_uncertain"] == true
}

const TRUSTED_LEAVES: &[&str] = &[
    "AdminUtils.sudo_set_activity_cutoff_factor",
    "AdminUtils.sudo_set_adjustment_alpha",
    "AdminUtils.sudo_set_alpha_sigmoid_steepness",
    "AdminUtils.sudo_set_alpha_values",
    "AdminUtils.sudo_set_bonds_moving_average",
    "AdminUtils.sudo_set_bonds_penalty",
    "AdminUtils.sudo_set_bonds_reset_enabled",
    "AdminUtils.sudo_set_burn_half_life",
    "AdminUtils.sudo_set_burn_increase_mult",
    "AdminUtils.sudo_set_collateral_drain_ratio",
    "AdminUtils.sudo_set_collateral_lock_share",
    "AdminUtils.sudo_set_commit_reveal_weights_enabled",
    "AdminUtils.sudo_set_commit_reveal_weights_interval",
    "AdminUtils.sudo_set_epoch_consensus",
    "AdminUtils.sudo_set_evm_chain_id",
    "AdminUtils.sudo_set_immunity_period",
    "AdminUtils.sudo_set_liquid_alpha_enabled",
    "AdminUtils.sudo_set_max_allowed_uids",
    "AdminUtils.sudo_set_max_burn",
    "AdminUtils.sudo_set_max_difficulty",
    "AdminUtils.sudo_set_mechanism_count",
    "AdminUtils.sudo_set_min_allowed_weights",
    "AdminUtils.sudo_set_min_burn",
    "AdminUtils.sudo_set_min_childkey_take_per_subnet",
    "AdminUtils.sudo_set_network_pow_registration_allowed",
    "AdminUtils.sudo_set_network_registration_allowed",
    "AdminUtils.sudo_set_owner_cut_auto_lock_enabled",
    "AdminUtils.sudo_set_owner_immune_neuron_limit",
    "AdminUtils.sudo_set_rho",
    "AdminUtils.sudo_set_serving_rate_limit",
    "AdminUtils.sudo_set_subnet_emission_enabled",
    "AdminUtils.sudo_set_subnet_emission_weights",
    "AdminUtils.sudo_set_subnet_emission_weights_v2",
    "AdminUtils.sudo_set_subnet_gamma_emission_whitelisted",
    "AdminUtils.sudo_set_subnet_gamma_enabled",
    "AdminUtils.sudo_set_tempo",
    "AdminUtils.sudo_set_toggle_transfer",
    "AdminUtils.sudo_set_weights_version_key",
    "AdminUtils.sudo_set_yuma3_enabled",
    "AdminUtils.sudo_trim_null_uids_batch",
    "AdminUtils.sudo_trim_to_max_allowed_uids",
    "Balances.transfer_all",
    "Balances.transfer_allow_death",
    "Balances.transfer_keep_alive",
    "Crowdloan.contribute",
    "Crowdloan.dissolve",
    "Crowdloan.finalize",
    "Crowdloan.refund",
    "Crowdloan.set_max_contribution",
    "Crowdloan.update_cap",
    "Crowdloan.update_end",
    "Crowdloan.update_min_contribution",
    "Crowdloan.withdraw",
    "EVM.deposit",
    "EVM.disable_whitelist",
    "EVM.withdraw",
    "HashedAccounts.check_registered",
    "HashedAccounts.check_registration_deposit",
    "HashedAccounts.register",
    "LimitOrders.authorize_order",
    "LimitOrders.cancel_order",
    "LimitOrders.execute_batched_orders",
    "LimitOrders.execute_orders",
    "Proxy.add_proxy",
    "Proxy.announce",
    "Proxy.create_protected_pure",
    "Proxy.create_pure",
    "Proxy.create_safe_pure",
    "Proxy.kill_pure",
    "Proxy.kill_safe_pure",
    "Proxy.reject_announcement",
    "Proxy.remove_announcement",
    "Proxy.remove_proxies",
    "Proxy.remove_proxy",
    "SubtensorModule.add_collateral",
    "SubtensorModule.add_stake",
    "SubtensorModule.add_stake_burn",
    "SubtensorModule.add_stake_limit",
    "SubtensorModule.announce_coldkey_swap_v2",
    "SubtensorModule.associate_evm_key",
    "SubtensorModule.burned_register",
    "SubtensorModule.claim_root",
    "SubtensorModule.claim_root_with_hotkey",
    "SubtensorModule.clear_coldkey_swap_announcement",
    "SubtensorModule.close_gamma_deposit",
    "SubtensorModule.commit_timelocked_mechanism_weights",
    "SubtensorModule.decrease_take",
    "SubtensorModule.dispute_coldkey_swap",
    "SubtensorModule.increase_take",
    "SubtensorModule.lock_stake",
    "SubtensorModule.mint_gamma",
    "SubtensorModule.mint_gamma_with_stake",
    "SubtensorModule.move_gamma",
    "SubtensorModule.move_lock",
    "SubtensorModule.move_stake",
    "SubtensorModule.move_stake_limit",
    "SubtensorModule.pow_register",
    "SubtensorModule.register_leased_network",
    "SubtensorModule.register_network",
    "SubtensorModule.release_own_coldkey_swap_destination_reservation",
    "SubtensorModule.remove_stake",
    "SubtensorModule.remove_stake_limit",
    "SubtensorModule.reveal_weights",
    "SubtensorModule.root_register",
    "SubtensorModule.serve_axon",
    "SubtensorModule.serve_axon_tls",
    "SubtensorModule.serve_prometheus",
    "SubtensorModule.set_childkey_take",
    "SubtensorModule.set_children",
    "SubtensorModule.set_coldkey_auto_stake_hotkey",
    "SubtensorModule.set_gamma_deposit_daily_spend_cap",
    "SubtensorModule.set_gamma_deposit_spender",
    "SubtensorModule.set_identity",
    "SubtensorModule.set_mechanism_weights",
    "SubtensorModule.set_min_collateral",
    "SubtensorModule.set_perpetual_lock",
    "SubtensorModule.set_reject_locked_alpha",
    "SubtensorModule.set_reject_staking_hotkey_growth",
    "SubtensorModule.set_service",
    "SubtensorModule.set_subnet_identity",
    "SubtensorModule.set_weights",
    "SubtensorModule.spend_gamma",
    "SubtensorModule.stake_into_basket",
    "SubtensorModule.start_call",
    "SubtensorModule.sudo_set_root_claim_threshold",
    "SubtensorModule.swap_basket",
    "SubtensorModule.swap_basket_many",
    "SubtensorModule.swap_coldkey_announced",
    "SubtensorModule.swap_hotkey",
    "SubtensorModule.swap_stake",
    "SubtensorModule.swap_stake_limit",
    "SubtensorModule.terminate_lease",
    "SubtensorModule.transfer_stake",
    "SubtensorModule.transfer_stake_and_hotkey",
    "SubtensorModule.try_associate_hotkey",
    "SubtensorModule.unstake_all",
    "SubtensorModule.unstake_all_alpha",
    "SubtensorModule.update_symbol",
];
const SINGLE_CALL_WRAPPERS: &[&str] = &[
    "Proxy.proxy",
    "Proxy.proxy_announced",
    "Multisig.as_multi",
    "Multisig.as_multi_safe",
    "Multisig.as_multi_threshold_1",
    "Multisig.as_multi_threshold_1_safe",
    "Utility.as_derivative",
    "Utility.as_safe_derivative",
    "Utility.as_protected_derivative",
    "Utility.dispatch_as",
    "Utility.dispatch_as_fallible",
    "Utility.with_weight",
    "Sudo.sudo",
    "Sudo.sudo_unchecked_weight",
    "Sudo.sudo_as",
];
const BATCHES: &[&str] = &["Utility.batch", "Utility.batch_all", "Utility.force_batch"];
const TRUSTED_WRAPPERS: &[&str] = &[
    "Proxy.proxy",
    "Proxy.proxy_announced",
    "Multisig.as_multi",
    "Multisig.as_multi_safe",
    "Multisig.as_multi_threshold_1",
    "Multisig.as_multi_threshold_1_safe",
    "Utility.as_derivative",
    "Utility.as_safe_derivative",
    "Utility.as_protected_derivative",
    "Sudo.sudo",
];
const ROOT_OPERATIONS: &[&str] = &[
    "AdminUtils.sudo_set_subnet_emission_enabled",
    "AdminUtils.sudo_set_subnet_gamma_emission_whitelisted",
    "SubtensorModule.sudo_set_root_claim_threshold",
    "AdminUtils.sudo_set_evm_chain_id",
    "EVM.disable_whitelist",
];
fn call_operation(call: &Value) -> String {
    format!("{}.{}", text(&call["pallet"]), text(&call["call"]))
}
pub(crate) fn decoded_call(value: &Value, depth: usize) -> Result<Value> {
    depth_guard(depth, "announced call nesting exceeds 64")?;
    let row = value
        .as_object()
        .ok_or_else(|| PolicyError::type_error("announced RuntimeCall cannot be inspected"))?;
    let (pallet, call, params) = if row.get("call_module").and_then(Value::as_str).is_some()
        && row.get("call_function").and_then(Value::as_str).is_some()
    {
        let args = value["call_args"]
            .as_array()
            .ok_or_else(|| PolicyError::type_error("announced call arguments unavailable"))?;
        let mut params = serde_json::Map::new();
        for arg in args {
            let name = arg["name"]
                .as_str()
                .ok_or_else(|| PolicyError::type_error("announced call arguments unavailable"))?;
            if params.insert(name.into(), arg["value"].clone()).is_some() {
                return Err(PolicyError::type_error(
                    "duplicate announced call arguments",
                ));
            }
        }
        (
            value["call_module"].clone(),
            value["call_function"].clone(),
            Value::Object(params),
        )
    } else if row.get("pallet").and_then(Value::as_str).is_some()
        && row.get("call").and_then(Value::as_str).is_some()
    {
        (
            value["pallet"].clone(),
            value["call"].clone(),
            value["params"].clone(),
        )
    } else {
        if row.len() != 1 {
            return Err(PolicyError::type_error(
                "announced RuntimeCall has no call identity",
            ));
        }
        let (pallet, variants) = row
            .iter()
            .next()
            .ok_or_else(|| PolicyError::type_error("announced RuntimeCall has no call identity"))?;
        let variants = variants
            .as_object()
            .filter(|v| v.len() == 1)
            .ok_or_else(|| PolicyError::type_error("announced RuntimeCall has no call identity"))?;
        let (call, params) = variants
            .iter()
            .next()
            .ok_or_else(|| PolicyError::type_error("announced RuntimeCall has no call identity"))?;
        (json!(pallet), json!(call), params.clone())
    };
    if text(&pallet).is_empty() || text(&call).is_empty() || !params.is_object() {
        return Err(PolicyError::type_error(
            "announced RuntimeCall has invalid parameters",
        ));
    }
    let mut out = json!({"pallet":pallet,"call":call,"params":params});
    let op = call_operation(&out);
    if SINGLE_CALL_WRAPPERS.contains(&op.as_str()) {
        out["params"]["call"] = decoded_call(&out["params"]["call"], depth.saturating_add(1))?;
    } else if BATCHES.contains(&op.as_str()) {
        let children = out["params"]["calls"]
            .as_array()
            .ok_or_else(|| PolicyError::type_error("announced batch has no inspectable calls"))?;
        out["params"]["calls"] = json!(children
            .iter()
            .map(|v| decoded_call(v, depth.saturating_add(1)))
            .collect::<Result<Vec<_>>>()?);
    } else if op == "Utility.if_else" {
        out["params"]["main"] = decoded_call(&out["params"]["main"], depth.saturating_add(1))?;
        out["params"]["fallback"] =
            decoded_call(&out["params"]["fallback"], depth.saturating_add(1))?;
    } else if op == "Crowdloan.create" && !out["params"]["call"].is_null() {
        out["params"]["call"] = decoded_call(&out["params"]["call"], depth.saturating_add(1))?;
    }
    Ok(out)
}
pub(crate) fn call_children(call: &Value) -> Result<Vec<Value>> {
    let op = call_operation(call);
    if SINGLE_CALL_WRAPPERS.contains(&op.as_str()) {
        return Ok(vec![call["params"]["call"].clone()]);
    }
    // Finalization dispatches the saved call as the crowdloan's creator, so
    // authority checks must inspect it even though creation only stores it.
    if op == "Crowdloan.create" && !call["params"]["call"].is_null() {
        return Ok(vec![call["params"]["call"].clone()]);
    }
    if BATCHES.contains(&op.as_str()) {
        return call["params"]["calls"]
            .as_array()
            .cloned()
            .ok_or_else(|| PolicyError::type_error("announced batch has no inspectable calls"));
    }
    if op == "Utility.if_else" {
        return Ok(vec![
            call["params"]["main"].clone(),
            call["params"]["fallback"].clone(),
        ]);
    }
    Ok(vec![])
}
fn root_call(call: &Value, depth: usize) -> Result<bool> {
    depth_guard(depth, "announced call nesting exceeds 64")?;
    let op = call_operation(call);
    if ROOT_OPERATIONS.contains(&op.as_str()) {
        return Ok(true);
    }
    if op != "Utility.batch_all" || rows(&call["params"]["calls"]).is_empty() {
        return Ok(false);
    }
    for child in rows(&call["params"]["calls"]) {
        if !root_call(
            &decoded_call(child, depth.saturating_add(1))?,
            depth.saturating_add(1),
        )? {
            return Ok(false);
        }
    }
    Ok(true)
}
fn validate_call(call: &Value, depth: usize) -> Result<()> {
    depth_guard(depth, "announced call nesting exceeds 64")?;
    let op = call_operation(call);
    if op == "Utility.batch_all" {
        let children = call["params"]["calls"]
            .as_array()
            .filter(|v| !v.is_empty())
            .ok_or_else(|| PolicyError::type_error("saved atomic batch requires calls"))?;
        for child in children {
            validate_call(
                &decoded_call(child, depth.saturating_add(1))?,
                depth.saturating_add(1),
            )?;
        }
    } else if TRUSTED_WRAPPERS.contains(&op.as_str()) {
        let child = decoded_call(&call["params"]["call"], depth.saturating_add(1))?;
        if op == "Sudo.sudo" && !root_call(&child, depth.saturating_add(1))? {
            return Err(PolicyError::type_error(
                "trusted sudo only wraps declared root operations",
            ));
        }
        validate_call(&child, depth.saturating_add(1))?;
    } else if op == "Crowdloan.create" {
        if call["params"]["call"].is_null() {
            if call["params"]["target_address"].is_null() {
                return Err(PolicyError::type_error("crowdloan needs a target or call"));
            }
        } else {
            if !call["params"]["target_address"].is_null() {
                return Err(PolicyError::type_error(
                    "crowdloan call and target are mutually exclusive",
                ));
            }
            validate_call(
                &decoded_call(&call["params"]["call"], depth.saturating_add(1))?,
                depth.saturating_add(1),
            )?;
        }
    } else if !TRUSTED_LEAVES.contains(&op.as_str()) {
        return Err(PolicyError::type_error(format!("unsupported saved announcement operation {op}; semantic replay requires a trusted operation")));
    }
    Ok(())
}
fn requires_direct(value: &Value, depth: usize) -> bool {
    if depth > 64 {
        return true;
    }
    let Ok(call) = decoded_call(value, 0) else {
        return true;
    };
    let op = call_operation(&call);
    if op == "Utility.batch_all" || op == "Crowdloan.finalize" {
        return true;
    }
    // The stored child is not dispatched during creation. It therefore cannot
    // trigger a current-origin batch restriction until finalization itself.
    if op == "Crowdloan.create" {
        return false;
    }
    match call_children(&call) {
        Ok(children) => children
            .iter()
            .any(|child| requires_direct(child, depth.saturating_add(1))),
        Err(_) => true,
    }
}
fn call_leaves(call: &Value, depth: usize, result: &mut Vec<Value>) -> Result<()> {
    depth_guard(depth, "announced call nesting exceeds 64")?;
    let children = call_children(call)?;
    if children.is_empty() {
        result.push(call.clone());
    } else {
        for child in children {
            call_leaves(
                &decoded_call(&child, depth.saturating_add(1))?,
                depth.saturating_add(1),
                result,
            )?;
        }
    }
    Ok(())
}
fn semantic_value(value: &Value, depth: usize) -> Result<Value> {
    if depth > 128 {
        return Err(PolicyError::range_error(
            "announced semantics nesting exceeds 128",
        ));
    }
    match value {
        Value::Number(number)
            if number
                .as_i64()
                .is_some_and(|v| v.unsigned_abs() <= 9_007_199_254_740_991)
                || number.as_u64().is_some_and(|v| v <= 9_007_199_254_740_991) =>
        {
            Ok(json!(number.to_string()))
        }
        Value::Array(items) => Ok(json!(items
            .iter()
            .map(|v| semantic_value(v, depth.saturating_add(1)))
            .collect::<Result<Vec<_>>>()?)),
        Value::Object(items) => Ok(Value::Object(
            items
                .iter()
                .map(|(key, v)| Ok((key.clone(), semantic_value(v, depth.saturating_add(1))?)))
                .collect::<Result<_>>()?,
        )),
        _ => Ok(value.clone()),
    }
}
fn receipt_state(receipt: &Value) -> Value {
    let events = rows(&receipt["events"]);
    let event_is = |event: &Value, module: &str, name: &str| {
        event["module_id"] == module && event["event_id"] == name
    };
    let approval = receipt["awaitingApproval"] == true
        || events.iter().any(|v| {
            event_is(v, "Multisig", "NewMultisig") || event_is(v, "Multisig", "MultisigApproval")
        }) && !events
            .iter()
            .any(|v| event_is(v, "Multisig", "MultisigExecuted"));
    let success = receipt["success"] == true
        && super::transaction::nested_dispatch_error(&receipt["events"]).is_none()
        && !events.iter().any(|v| {
            event_is(v, "System", "ExtrinsicFailed")
                || v["module_id"] == "Utility"
                    && ["BatchInterrupted", "ItemFailed", "BatchCompletedWithErrors"]
                        .contains(&text(&v["event_id"]))
        });
    let status = text(&receipt["status"]);
    let included = ![
        "uncertain",
        "unknown",
        "submitted",
        "broadcast",
        "pending",
        "dropped",
        "invalid",
        "usurped",
        "retracted",
    ]
    .contains(&status)
        && (!receipt["blockHash"].is_null()
            || !receipt["blockNumber"].is_null()
            || ["inBlock", "finalized"].contains(&status));
    json!({"successful":success,"approval_only":approval,"included":included,"confirmed":success && included && !approval && status!="failed"})
}

fn multisig_calls(call: &Value, depth: usize, result: &mut Vec<Value>) -> Result<()> {
    depth_guard(depth, "multisig recovery call nesting exceeds 64")?;
    if call["pallet"] == "Multisig" {
        result.push(call.clone());
    }
    for child in call_children(call)? {
        multisig_calls(&child, depth.saturating_add(1), result)?;
    }
    Ok(())
}

fn multisig_action_matches(action: &Value, input: &Value) -> Result<bool> {
    let expected = &input["announcement"];
    if action["pallet"] != "Proxy" || action["params"]["real"] != expected["real"] {
        return Ok(false);
    }
    Ok(match text(&input["kind"]) {
        "execute" if action["call"] == "proxy_announced" => {
            action["params"]["delegate"] == expected["delegate"]
                && semantic_value(&action["params"]["call"], 0)?
                    == semantic_value(&expected["call"], 0)?
        }
        "cancel" if action["call"] == "remove_announcement" => {
            text(&action["params"]["call_hash"]).to_lowercase()
                == text(&expected["call_hash"]).to_lowercase()
        }
        _ => false,
    })
}

fn multisig_action_result(input: &Value) -> Result<Value> {
    let invalid =
        || PolicyError::invalid("receipt does not prove the original protected multisig action");
    let receipt = &input["receipt"];
    let identity = &input["identity"];
    if receipt["status"] != "finalized"
        || identity["account"].as_str().is_none_or(str::is_empty)
        || identity["timepoint"].is_null()
    {
        return Err(invalid());
    }
    let committed = super::transaction::evaluate(
        "transaction.multisig_cancel",
        json!({"call_hash":identity["call_hash"],"timepoint":identity["timepoint"]}),
    )?;
    let events = rows(&receipt["events"]);
    let completions: Vec<_> = events
        .iter()
        .filter(|event| {
            event["module_id"] == "Multisig"
                && ["MultisigExecuted", "MultisigCancelled"].contains(&text(&event["event_id"]))
        })
        .collect();
    if completions.len() != 1
        || events
            .iter()
            .any(|event| event["module_id"] == "System" && event["event_id"] == "ExtrinsicFailed")
    {
        return Err(invalid());
    }
    let event = completions[0];
    let end = events
        .iter()
        .position(|candidate| candidate == event)
        .ok_or_else(invalid)?;
    let attrs = &event["attributes"];
    if attrs["multisig"] != identity["account"]
        || text(&attrs["call_hash"]).to_lowercase() != committed["call_hash"]
        || semantic_value(&attrs["timepoint"], 0)? != semantic_value(&committed["timepoint"], 0)?
        || (!event["extrinsic_idx"].is_null()
            && event["extrinsic_idx"] != receipt["extrinsicIndex"])
    {
        return Err(invalid());
    }
    let mut calls = Vec::new();
    multisig_calls(&decoded_call(&input["call"], 0)?, 0, &mut calls)?;
    let cancelled = event["event_id"] == "MultisigCancelled";
    let mut matches: usize = 0;
    for call in &calls {
        if cancelled {
            if ["cancel_as_multi", "cancel_as_multi_safe"].contains(&text(&call["call"]))
                && text(&call["params"]["call_hash"]).to_lowercase() == committed["call_hash"]
                && semantic_value(&call["params"]["timepoint"], 0)?
                    == semantic_value(&committed["timepoint"], 0)?
            {
                matches = matches.saturating_add(1);
            }
        } else if ["as_multi", "as_multi_safe"].contains(&text(&call["call"]))
            && semantic_value(&call["params"]["maybe_timepoint"], 0)?
                == semantic_value(&committed["timepoint"], 0)?
            && multisig_action_matches(&call["params"]["call"], input)?
        {
            matches = matches.saturating_add(1);
        }
    }
    if matches != 1 {
        return Err(invalid());
    }
    let on_chain = input["on_chain"] == true;
    if cancelled {
        let mut openings = Vec::new();
        multisig_calls(&decoded_call(&input["opening_call"], 0)?, 0, &mut openings)?;
        let mut matches: usize = 0;
        for call in &openings {
            if ["as_multi", "as_multi_safe"].contains(&text(&call["call"]))
                && call["params"]["maybe_timepoint"].is_null()
                && multisig_action_matches(&call["params"]["call"], input)?
            {
                matches = matches.saturating_add(1);
            }
        }
        if matches != 1 {
            return Err(invalid());
        }
        // Cancellation only proves this approval did not execute the action.
        // A permissionless proxy_announced may have consumed it separately.
        return if on_chain {
            Ok(json!({"status":"pending"}))
        } else {
            Err(invalid())
        };
    }
    if attrs["result"].get("Ok").is_none() && attrs["result"].get("Err").is_none() {
        return Err(invalid());
    }
    let failed = if attrs["result"].get("Err").is_some() {
        true
    } else if input["kind"] == "execute" {
        let proxies: Vec<_> = events[..=end]
            .iter()
            .filter(|event| event["module_id"] == "Proxy" && event["event_id"] == "ProxyExecuted")
            .collect();
        if proxies.len() != 1 {
            return Err(invalid());
        }
        let result = &proxies[0]["attributes"]["result"];
        if result.get("Ok").is_none() && result.get("Err").is_none() {
            return Err(invalid());
        }
        result.get("Err").is_some()
    } else {
        false
    };
    if failed {
        return if on_chain {
            Ok(json!({"status":"pending"}))
        } else {
            Err(invalid())
        };
    }
    // A later failing sibling in a non-atomic batch cannot undo this payment.
    // Earlier nested failures are ambiguous without a complete dispatch trace.
    let scoped = json!({"status":"finalized","success":true,"events":&events[..=end]});
    if on_chain || receipt_state(&scoped)["successful"] != true {
        return Err(invalid());
    }
    Ok(if input["kind"] == "execute" {
        json!({"status":"executed"})
    } else {
        // remove_announcement identifies a payload, not an announcement height.
        // A permissionless executor could have paid the original before the
        // same hash was announced again and this cancellation removed it.
        json!({"status":"cancelled","execution_uncertain":true})
    })
}
fn rotation_plan(input: &Value) -> Value {
    let record = &input["record"];
    let controller = &input["controller"];
    let delegate = input
        .get("allow_key")
        .filter(|v| !v.is_null())
        .unwrap_or(controller);
    let own = delegate == controller;
    let reviewed = record
        .get("reviewed_allows")
        .or_else(|| record.get("allows"))
        .map(rows)
        .unwrap_or(&[]);
    let mut replacements = Vec::new();
    let mut migrated = Vec::new();
    let catalog = constants();
    for grant in rows(&input["grants"]) {
        if grant["delegate"] != *delegate
            || number(&grant["delay"]) != 0.0
            || !reviewed.contains(&grant["proxy_type"])
            || catalog["PROXY_TYPE_INTENT_OPS"]
                .get(text(&grant["proxy_type"]))
                .is_none()
        {
            continue;
        }
        let replacement = json!({"delegate_ss58": if own { &input["new_address"] } else { input.get("allow_address").filter(|v|!v.is_null()).unwrap_or(&grant["address"]) },"proxy_type":grant["proxy_type"],"delay":0,"from_hotkey":false,"op":"add_proxy"});
        if !replacements.contains(&replacement) {
            replacements.push(replacement);
        }
        if own && !migrated.contains(&grant["proxy_type"]) {
            migrated.push(grant["proxy_type"].clone());
        }
    }
    let mut children = vec![
        json!({"op":"remove_proxies"}),
        json!({"op":"add_proxy","delegate_ss58":input["new_address"],"proxy_type":"Any","delay":record["delay_blocks"],"from_hotkey":false}),
    ];
    children.extend(replacements);
    json!({"children":children,"migrated":migrated})
}

fn flatten_batch(calls: &Value, output: &mut Vec<Value>, depth: usize) -> Result<()> {
    depth_guard(depth, "announced call nesting exceeds 64")?;
    for value in rows(calls) {
        let call = decoded_call(value, depth)?;
        if call_operation(&call) == "Utility.batch_all" {
            flatten_batch(&call["params"]["calls"], output, depth.saturating_add(1))?;
        } else {
            if proxy_contains_batch(&call, depth)? {
                return Err(PolicyError::range_error("atomic batches cannot contain a proxy with an inner batch; submit the proxy intent directly"));
            }
            output.push(call);
        }
    }
    Ok(())
}
fn proxy_contains_batch(call: &Value, depth: usize) -> Result<bool> {
    depth_guard(depth, "announced call nesting exceeds 64")?;
    if !["Proxy.proxy", "Proxy.proxy_announced"].contains(&call_operation(call).as_str()) {
        return Ok(false);
    }
    let child = decoded_call(&call["params"]["call"], depth.saturating_add(1))?;
    Ok(call_operation(&child) == "Utility.batch_all"
        || proxy_contains_batch(&child, depth.saturating_add(1))?)
}
fn settings_plan(input: &Value) -> Result<Value> {
    let record = &input["record"];
    let allows = rows(&input["allow"]);
    let disallows = rows(&input["disallow"]);
    let pending = rows(&input["pending"]);
    let old = &input["old_delegate"];
    let delegate = &input["delegate"];
    let address = &input["delegate_address"];
    if allows.iter().any(|kind| disallows.contains(kind)) {
        return Err(PolicyError::invalid(
            "cannot allow and disallow the same permission",
        ));
    }
    let grants = rows(&input["grants"]);
    if delegate != old
        && (grants.iter().any(|g| {
            g["delegate"] == *old && g["proxy_type"] != "Any" && number(&g["delay"]) == 0.0
        }) || pending.iter().any(|row| {
            row["plaintext_missing"] == true
                || rows(&row["grants"])
                    .iter()
                    .any(|g| g["delegate_ss58"] == *old)
        }))
    {
        return Err(PolicyError::invalid("old allow key still holds or may receive grants; cancel pending grants and execute revocations before changing the allow key"));
    }
    if !disallows.is_empty()
        && pending
            .iter()
            .any(|row| row["plaintext_missing"] == true || row["category"] == "unknown")
    {
        return Err(PolicyError::invalid(
            "cancel announcements with missing plaintext before revoking grants",
        ));
    }
    let delay = &input["delay_blocks"];
    if delay != &record["delay_blocks"]
        && rows(&input["controller_delays"])
            .iter()
            .any(|v| v != &record["delay_blocks"])
    {
        return Err(PolicyError::invalid(
            "controller has multiple Any delays; reconcile grants before changing the delay",
        ));
    }
    let row_has = |row: &Value, who: &Value, kind: &Value| {
        rows(&row["grants"]).iter().any(|g| {
            g["delegate_ss58"] == *who && g["proxy_type"] == *kind && number(&g["delay"]) == 0.0
        })
    };
    let mut actions = Vec::new();
    let mut cancel = Vec::new();
    if delay != &record["delay_blocks"] {
        actions.push(json!({"category":"delay-change","metadata":{"target_delay_blocks":delay},"intent":{"op":"batch","intents":[{"op":"remove_proxy","delegate_ss58":record["controller_ss58"],"proxy_type":"Any","delay":record["delay_blocks"]},{"op":"add_proxy","delegate_ss58":record["controller_ss58"],"proxy_type":"Any","delay":delay,"from_hotkey":false}]}}));
    }
    for kind in allows {
        if !rows(&record["allows"]).contains(kind)
            && !pending.iter().any(|row| row_has(row, delegate, kind))
        {
            actions.push(json!({"category":format!("allow:{}",text(kind)),"intent":{"op":"add_proxy","delegate_ss58":address,"proxy_type":kind,"delay":0,"from_hotkey":false}}));
        }
    }
    for kind in disallows {
        for row in pending {
            if (row_has(row, old, kind) || row["category"] == format!("allow:{}", text(kind)))
                && !cancel.contains(&row["index"])
            {
                cancel.push(row["index"].clone());
            }
        }
        if rows(&record["allows"]).contains(kind) {
            actions.push(json!({"category":format!("disallow:{}",text(kind)),"intent":{"op":"remove_proxy","delegate_ss58":address,"proxy_type":kind,"delay":0}}));
        }
    }
    Ok(json!({"actions":actions,"cancel_indices":cancel}))
}
// Exact arbitrary-precision amounts; subtraction is explicitly guarded.
#[allow(clippy::arithmetic_side_effects)]
fn pending_refills(input: &Value) -> Result<Value> {
    fn visit(
        value: &Value,
        controller: &Value,
        depth: usize,
        total: &mut num_bigint::BigInt,
        unbounded: &mut bool,
        unknown: &mut bool,
        legacy_python: bool,
    ) -> Result<()> {
        depth_guard(depth, "protected intent nesting exceeds 64")?;
        match text(&value["op"]) {
            "batch" => {
                for child in rows(&value["intents"]) {
                    visit(
                        child,
                        controller,
                        depth.saturating_add(1),
                        total,
                        unbounded,
                        unknown,
                        legacy_python,
                    )?;
                }
            }
            "execute_proxy_announced" if !legacy_python => visit(
                &nested_intent(value),
                controller,
                depth.saturating_add(1),
                total,
                unbounded,
                unknown,
                legacy_python,
            )?,
            "transfer" | "transfer_all"
                if value["dest_ss58"] == *controller
                    && (!legacy_python || value["op"] == "transfer") =>
            {
                if value["op"] == "transfer_all" || value["amount_tao"] == "all" {
                    *unbounded = true;
                } else {
                    let amount = &value["amount_tao"];
                    if legacy_python {
                        let encoded = amount
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| amount.to_string());
                        match super::validation::evaluate(
                            "validation.amount.parse",
                            json!({"text":encoded,"precision":"round"}),
                        ) {
                            Ok(parsed) => match text(&parsed).parse::<num_bigint::BigInt>() {
                                Ok(value) => *total += value,
                                Err(_) => *unknown = true,
                            },
                            Err(_) => *unknown = true,
                        }
                        return Ok(());
                    }
                    if amount.is_object() {
                        if number(&amount["netuid"]) != 0.0 {
                            *unknown = true;
                        } else {
                            match decimal(&amount["rao"], "pending refill") {
                                Ok(value) => *total += num_bigint::BigInt::from(value),
                                Err(_) => *unknown = true,
                            }
                        }
                        return Ok(());
                    }
                    let encoded = amount
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| amount.to_string());
                    let valid = encoded.trim().strip_prefix('-').unwrap_or(encoded.trim());
                    let parts: Vec<_> = valid.split('.').collect();
                    if parts.len() > 2
                        || parts
                            .iter()
                            .any(|v| v.is_empty() || !v.bytes().all(|b| b.is_ascii_digit()))
                        || parts.get(1).is_some_and(|v| v.len() > 9)
                    {
                        *unknown = true;
                        return Ok(());
                    }
                    match super::numeric::decimal(&encoded, 1000) {
                        Ok(value) => {
                            let scaled = value * num_bigint::BigInt::from(1_000_000_000u64);
                            if !scaled.is_integer()
                                || scaled < num_rational::BigRational::from_integer(0.into())
                            {
                                *unknown = true;
                            } else if let Some(amount) = scaled.to_integer().to_biguint() {
                                *total += num_bigint::BigInt::from(amount);
                            } else {
                                *unknown = true;
                            }
                        }
                        Err(_) => *unknown = true,
                    }
                }
            }
            _ => (),
        }
        Ok(())
    }
    let mut total = num_bigint::BigInt::default();
    let legacy_python = input["legacy_python"] == true;
    let mut unbounded = false;
    let mut unknown = false;
    for row in rows(&input["rows"]) {
        if !legacy_python
            && !row["status"].is_null()
            && !["pending", "awaiting_approval", "submitting", "uncertain"]
                .contains(&text(&row["status"]))
        {
            continue;
        }
        if row["plaintext_missing"] == true || (!legacy_python && text(&row["inner_op"]).is_empty())
        {
            unknown = true;
            continue;
        }
        let mut intent = nested_intent(row);
        if legacy_python {
            if let (Some(fields), Some(arguments)) =
                (intent.as_object_mut(), row["inner_args"].as_object())
            {
                fields.extend(arguments.clone());
            }
        }
        visit(
            &intent,
            &input["controller"],
            0,
            &mut total,
            &mut unbounded,
            &mut unknown,
            legacy_python,
        )?;
    }
    Ok(json!({"rao":total.to_string(),"unbounded":unbounded,"unknown":unknown}))
}

fn remove_field(value: &mut Value, key: &str) {
    if let Some(object) = value.as_object_mut() {
        object.remove(key);
    }
}
fn merged(left: &Value, right: &Value) -> Value {
    let mut out = left.as_object().cloned().unwrap_or_default();
    out.extend(right.as_object().cloned().unwrap_or_default());
    Value::Object(out)
}
fn journal_identity(row: &Value, input: &Value) -> Value {
    let mut value = row.clone();
    for key in ["delegate_ss58", "real_ss58"] {
        if let Some(id) = input["identities"].get(text(&row[key])) {
            value[key] = id.clone();
        }
    }
    value
}
fn journal_same(left: &Value, right: &Value, input: &Value, instance: bool) -> bool {
    let a = journal_identity(left, input);
    let b = journal_identity(right, input);
    same_commitment(&a, &b)
        && (!instance
            || number(&a["announced_at_block"]) == number(&b["announced_at_block"])
                && number(&a["announcement_ordinal"]) == number(&b["announcement_ordinal"]))
}
fn journal_transition(input: &Value) -> Result<Value> {
    if !input["rows"].is_array() || rows(&input["rows"]).iter().any(|row| !row.is_object()) {
        return Err(PolicyError::type_error(
            "invalid proxy_announcements journal",
        ));
    }
    let mut data = rows(&input["rows"]).to_vec();
    let action = text(&input["action"]);
    let mut candidate = input["candidate"].clone();
    if action != "claim" && !candidate.is_object() {
        return Err(PolicyError::type_error("invalid announcement record"));
    }
    let index = data
        .iter()
        .map(|r| number(&r["index"]) as u64)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .filter(|value| *value <= 9_007_199_254_740_991)
        .ok_or_else(|| {
            PolicyError::range_error("announcement index must be a nonnegative safe integer")
        })?;
    if ["add", "reserve"].contains(&action) {
        if action == "reserve" {
            if let Some(old) = data.iter().find(|row| {
                (["submitting", "awaiting_approval"].contains(&text(&row["status"]))
                    || !row["pending_action"].is_null())
                    && journal_same(row, &candidate, input, false)
            }) {
                return Err(PolicyError::invalid(format!(
                    "identical announcement #{} may still be submitting or awaiting approval",
                    number(&old["index"]) as u64
                )));
            }
            candidate["status"] = json!("submitting");
        }
        candidate["index"] = json!(index);
        if let Some(found) = data.iter().position(|old| {
            journal_same(old, &candidate, input, true)
                && (candidate["status"] != "submitting"
                    || ["submitting", "awaiting_approval"].contains(&text(&old["status"])))
        }) {
            let old = data[found].clone();
            candidate = merged(&old, &candidate);
            candidate["index"] = old["index"].clone();
            remove_field(&mut candidate, "plaintext_missing");
            remove_field(&mut candidate, "on_chain");
            if input["preserve_terminal"] != false
                && ["executed", "cancelled"].contains(&text(&old["status"]))
            {
                candidate["status"] = old["status"].clone();
            }
            data[found] = candidate.clone();
            for (i, row) in data.iter_mut().enumerate() {
                if i != found && journal_same(row, &candidate, input, true) {
                    row["status"] = json!("superseded");
                }
            }
        } else {
            data.push(candidate.clone());
        }
    } else if action == "upsert" {
        let chain = candidate.clone();
        if let Some(found) = data
            .iter()
            .position(|row| journal_same(row, &chain, input, true))
        {
            let old = data[found].clone();
            let plaintext = data
                .iter()
                .find(|row| {
                    !text(&row["inner_op"]).is_empty() && journal_same(row, &chain, input, true)
                })
                .cloned()
                .unwrap_or(Value::Null);
            candidate = merged(&merged(&old, &plaintext), &chain);
            candidate["index"] = old["index"].clone();
            candidate["plaintext_missing"] = json!(text(&candidate["inner_op"]).is_empty());
            if input["preserve_terminal"] != false
                && ["executed", "cancelled"].contains(&text(&old["status"]))
            {
                candidate["status"] = old["status"].clone();
            }
            data[found] = candidate.clone();
            for (i, row) in data.iter_mut().enumerate() {
                if i != found && journal_same(row, &chain, input, true) {
                    row["status"] = json!("superseded");
                }
            }
        } else {
            let provisional = data.iter().position(|row| {
                ["submitting", "awaiting_approval"].contains(&text(&row["status"]))
                    && journal_same(row, &chain, input, false)
            });
            let template = provisional
                .map(|i| data[i].clone())
                .or_else(|| {
                    data.iter()
                        .rev()
                        .find(|row| {
                            !text(&row["inner_op"]).is_empty()
                                && journal_same(row, &chain, input, false)
                        })
                        .cloned()
                })
                .unwrap_or(json!({}));
            candidate = merged(&template, &chain);
            candidate["index"] = provisional
                .map(|i| data[i]["index"].clone())
                .unwrap_or(json!(index));
            for (key, default) in [
                ("inner_op", json!("")),
                ("inner_args", json!({})),
                ("category", json!("unknown")),
                ("force_proxy_type", json!("Any")),
                (
                    "summary",
                    json!(format!(
                        "on-chain announcement {}",
                        text(&chain["call_hash"])
                    )),
                ),
            ] {
                if candidate.get(key).is_none()
                    || input["null_defaults"] == true && candidate[key].is_null()
                {
                    candidate[key] = default;
                }
            }
            candidate["plaintext_missing"] = json!(text(&candidate["inner_op"]).is_empty());
            candidate["created_at"] = input["now"].clone();
            if provisional.is_none() && input["clear_previous_action"] != false {
                for key in [
                    "pending_action",
                    "action_receipt",
                    "announcement_receipt",
                    "multisig_announcement",
                ] {
                    remove_field(&mut candidate, key);
                }
            }
            if let Some(i) = provisional {
                data[i] = candidate.clone();
            } else {
                data.push(candidate.clone());
            }
        }
    } else if action == "claim" {
        let Some(i) = data.iter().position(|row| {
            row["index"] == input["index"]
                && row["controller_wallet"] == input["wallet"]
                && lower(&row["genesis_hash"]) == lower(&input["genesis_hash"])
        }) else {
            return Err(PolicyError::invalid("announcement is not pending"));
        };
        let old = data[i].clone();
        if old["status"] != "pending" {
            return Err(PolicyError::invalid("announcement is not pending"));
        }
        if data.iter().any(|row| {
            row["index"] != old["index"]
                && !row["pending_action"].is_null()
                && journal_same(row, &old, input, false)
        }) {
            return Err(PolicyError::invalid(
                "an identical commitment has an uncertain action; reconcile it before retrying",
            ));
        }
        if input["kind"] == "execute" && input["allow_repeat"] != true {
            if let Some(executed) = data
                .iter()
                .find(|row| repeat_approval_required(row) && journal_same(row, &old, input, false))
            {
                return Err(PolicyError::invalid(format!(
                    "this payload {} as #{}; explicit allowRepeat is required",
                    if executed["execution_uncertain"] == true {
                        "may have executed"
                    } else {
                        "already executed"
                    },
                    number(&executed["index"]) as u64
                )));
            }
        }
        if !(old["pending_action"].is_null()
            || old["pending_action"]["kind"] == input["kind"]
                && old["pending_action"]["state"] == "awaiting_approval")
        {
            return Err(PolicyError::invalid("a previous action has an uncertain submission outcome; reconcile it before retrying"));
        }
        candidate = old;
        candidate["pending_action"] =
            json!({"kind":input["kind"],"state":"submitting","created_at":input["now"]});
        data[i] = candidate.clone();
    } else {
        return Err(PolicyError::invalid(
            "unknown announcement journal transition",
        ));
    }
    Ok(json!({"rows":data,"row":candidate}))
}
fn flatten_batch_paths(
    calls: &Value,
    prefix: Vec<usize>,
    result: &mut Vec<Vec<usize>>,
    depth: usize,
) -> Result<()> {
    depth_guard(depth, "announced call nesting exceeds 64")?;
    for (index, value) in rows(calls).iter().enumerate() {
        let call = decoded_call(value, depth)?;
        let mut path = prefix.clone();
        path.push(index);
        if call_operation(&call) == "Utility.batch_all" {
            flatten_batch_paths(
                &call["params"]["calls"],
                path,
                result,
                depth.saturating_add(1),
            )?;
        } else {
            if proxy_contains_batch(&call, depth)? {
                return Err(PolicyError::range_error("atomic batches cannot contain a proxy with an inner batch; submit the proxy intent directly"));
            }
            result.push(path);
        }
    }
    Ok(())
}

fn config_transition(input: &Value) -> Result<Value> {
    if !input["rows"].is_array() || rows(&input["rows"]).iter().any(|row| !row.is_object()) {
        return Err(PolicyError::type_error("invalid protected-account store"));
    }
    let mut entries = rows(&input["rows"]).to_vec();
    let action = text(&input["action"]);
    let saved = &input["record"];
    if ["upsert", "save_setup", "reserve_setup", "bind_created"].contains(&action)
        && !saved.is_object()
    {
        return Err(PolicyError::type_error("invalid protected-account record"));
    }
    let wallet = input.get("wallet").unwrap_or(&saved["controller_wallet"]);
    let genesis = input.get("genesis_hash").unwrap_or(&saved["genesis_hash"]);
    let matches = |row: &Value| {
        row["controller_wallet"] == *wallet && lower(&row["genesis_hash"]) == lower(genesis)
    };
    let existing = entries.iter().position(matches);
    let account = |value: &Value| {
        input["identities"]
            .get(text(value))
            .cloned()
            .unwrap_or_else(|| value.clone())
    };
    let result = match action {
        "upsert" | "save_setup" => {
            entries.retain(|row| !matches(row));
            entries.push(saved.clone());
            saved.clone()
        }
        "reserve_setup" => {
            let (reservation, created) = if let Some(i) = existing {
                (entries[i].clone(), false)
            } else {
                entries.push(saved.clone());
                (saved.clone(), true)
            };
            json!({"reservation":reservation,"created":created})
        }
        "bind_created" => {
            if let Some(i) = existing {
                let old = &entries[i];
                if account(&old["controller_ss58"]) != account(&saved["controller_ss58"])
                    || account(&old["protected_ss58"]) != account(&saved["protected_ss58"])
                {
                    return Err(PolicyError::invalid("creation recovery conflicts with an existing protected account binding; both identities preserved"));
                }
                old.clone()
            } else {
                entries.push(saved.clone());
                saved.clone()
            }
        }
        "patch" => {
            let i = existing.ok_or_else(|| {
                PolicyError::invalid("protected account binding not found on this network")
            })?;
            let old = entries[i].clone();
            let expected = &input["expected_migration"];
            if !expected.is_null()
                && (old["coldkey_swap"]["status"] != expected["status"]
                    || old["coldkey_swap"]["receipt"]["extrinsicHash"] != expected["extrinsicHash"])
            {
                old
            } else {
                let fields = &input["fields"];
                if ["controller_wallet", "genesis_hash", "protected_ss58"]
                    .iter()
                    .any(|name| contains(&input["undefined_fields"], name))
                    || fields.get("controller_wallet").is_some()
                        && fields["controller_wallet"] != old["controller_wallet"]
                    || fields.get("genesis_hash").is_some()
                        && lower(&fields["genesis_hash"]) != lower(genesis)
                    || fields.get("protected_ss58").is_some()
                        && account(&fields["protected_ss58"]) != account(&old["protected_ss58"])
                {
                    return Err(PolicyError::invalid(
                        "protected account identity cannot change in a workflow update",
                    ));
                }
                let mut updated = merged(&old, fields);
                for key in rows(&input["undefined_fields"]) {
                    remove_field(&mut updated, text(key));
                }
                for key in ["controller_wallet", "genesis_hash", "protected_ss58"] {
                    if let Some(value) = old.get(key) {
                        updated[key] = value.clone();
                    } else {
                        remove_field(&mut updated, key);
                    }
                }
                entries[i] = updated.clone();
                updated
            }
        }
        "reserve_migration" => {
            let i = existing.ok_or_else(|| {
                PolicyError::invalid("protected account binding not found on this network")
            })?;
            let mut old = entries[i].clone();
            if account(&old["protected_ss58"]) != account(&input["destination"]) {
                return Err(PolicyError::invalid(
                    "migration destination differs from the bound protected account",
                ));
            }
            if [
                "submitting",
                "uncertain",
                "awaiting_approval",
                "announced",
                "executed",
            ]
            .contains(&text(&old["coldkey_swap"]["status"]))
            {
                json!({"record":old,"created":false})
            } else {
                old["coldkey_swap"] =
                    json!({"status":"submitting","new_coldkey_ss58":input["destination"]});
                entries[i] = old.clone();
                json!({"record":old,"created":true})
            }
        }
        "remove" => {
            let before = entries.len();
            entries.retain(|row| !matches(row));
            json!(before != entries.len())
        }
        "move" => {
            if let Some(i) = existing {
                let mut record = entries[i].clone();
                let to = &input["to_wallet"];
                if let Some(other) = entries.iter().find(|row| {
                    row["controller_wallet"] == *to
                        && lower(&row["genesis_hash"]) == lower(genesis)
                        && account(&row["protected_ss58"]) != account(&record["protected_ss58"])
                }) {
                    if input["python_messages"] == true {
                        return Err(PolicyError::invalid(format!("wallet {} already controls protected account {} on this network; both bindings have been preserved",text(&input["wallet_display"]),text(&other["protected_ss58"]))));
                    }
                    return Err(PolicyError::invalid(format!("wallet {} already controls another protected account on this network; both bindings preserved",text(to))));
                }
                record["controller_wallet"] = to.clone();
                if !text(&input["controller_ss58"]).is_empty() {
                    record["controller_ss58"] = input["controller_ss58"].clone();
                }
                entries.retain(|row| {
                    !((row["controller_wallet"] == *wallet || row["controller_wallet"] == *to)
                        && lower(&row["genesis_hash"]) == lower(genesis))
                });
                entries.push(record.clone());
                record
            } else {
                Value::Null
            }
        }
        "bind_legacy" => {
            let old = entries.iter().position(|row| {
                row["controller_wallet"] == *wallet && row["genesis_hash"].is_null()
            });
            if let Some(i) = old {
                if existing.is_some() {
                    return Err(PolicyError::invalid(if input["python_messages"] == true {
                        "this wallet already has a protected account on this network; the legacy and scoped records have both been preserved"
                    } else {
                        "wallet already has a protected account on this network; legacy and scoped records preserved"
                    }));
                }
                entries[i]["genesis_hash"] = lower(genesis);
                entries[i].clone()
            } else {
                existing.map(|i| entries[i].clone()).unwrap_or(Value::Null)
            }
        }
        _ => return Err(PolicyError::invalid("unknown protected config transition")),
    };
    Ok(json!({"value":entries,"result":result}))
}

pub(crate) fn evaluate(operation: &str, input: Value) -> Result<Value> {
    match operation {
        "protected.pending_allow_grants" => {
            let mut selected = Vec::new();
            for (row_index, row) in rows(&input["rows"]).iter().enumerate() {
                let status = text(&row["status"]);
                if !(row["status"].is_null()
                    || ["pending", "awaiting_approval"].contains(&status)
                    || input["include_uncertain"] == true
                        && ["submitting", "uncertain"].contains(&status))
                {
                    continue;
                }
                for (grant_index, grant) in rows(&row["grants"]).iter().enumerate() {
                    if number(&grant["delay"]) == 0.0
                        && (input["allow_key"].is_null()
                            || grant["identity"].is_null()
                            || grant["identity"] == input["allow_key"])
                    {
                        selected.push(json!({"row_index":row_index,"grant_index":grant_index}));
                    }
                }
            }
            Ok(json!(selected))
        }
        "protected.allow_key_blockers" => {
            if input["old"] == input["new"] {
                return Ok(json!({"kinds":[],"live_indices":[]}));
            }
            let live = grant_indices(
                &json!({"mode":"allow","grants":input["grants"],"controller":input["old"]}),
            );
            let mut kinds = Vec::new();
            for kind in ["allows", "pending_allows", "pending_announcements"] {
                if !rows(&input[kind]).is_empty() {
                    kinds.push(kind);
                }
            }
            if !rows(&live).is_empty() {
                kinds.push("live");
            }
            Ok(json!({"kinds":kinds,"live_indices":live}))
        }
        "protected.registration_rules" => Ok(json!([
            ["HashedAccounts", "register"],
            ["HashedAccounts", "register_derived"],
            ["HashedAccounts", "register_derived_sponsored"],
            ["Proxy", "create_protected_pure"],
            ["Utility", "as_protected_derivative"]
        ])),
        "protected.registration_node" => {
            let pallet = text(&input["pallet"]);
            let call = text(&input["call"]);
            let defaults = evaluate("protected.registration_rules", Value::Null)?;
            let registrations = input.get("registrations").unwrap_or(&defaults);
            let found = rows(registrations).iter().any(|pair| {
                pair.get(0).and_then(Value::as_str) == Some(pallet)
                    && pair.get(1).and_then(Value::as_str) == Some(call)
            });
            let op = format!("{pallet}.{call}");
            let expanded = input["expanded"] == true;
            let single = matches!(
                op.as_str(),
                "Proxy.proxy"
                    | "Proxy.proxy_announced"
                    | "Multisig.as_multi"
                    | "Multisig.as_multi_safe"
                    | "Multisig.as_multi_threshold_1"
                    | "Multisig.as_multi_threshold_1_safe"
                    | "Utility.as_derivative"
                    | "Utility.as_safe_derivative"
                    | "Sudo.sudo"
            ) || expanded
                && matches!(
                    op.as_str(),
                    "Utility.as_protected_derivative"
                        | "Utility.dispatch_as"
                        | "Utility.dispatch_as_fallible"
                        | "Utility.with_weight"
                        | "Sudo.sudo_unchecked_weight"
                        | "Sudo.sudo_as"
                );
            let fields = if single {
                vec!["call"]
            } else if BATCHES.contains(&op.as_str()) {
                vec!["calls"]
            } else if expanded && op == "Utility.if_else" {
                vec!["main", "fallback"]
            } else {
                vec![]
            };
            Ok(json!({"found":found,"fields":fields}))
        }
        "protected.constants" => Ok(constants()),
        "protected.migration_preimages" => {
            let account = hex::decode(text(&input["account"]).trim_start_matches("0x"))
                .map_err(|_| PolicyError::type_error("invalid migration account"))?;
            if account.len() != 32 {
                return Err(PolicyError::range_error(
                    "migration account must be 32 bytes",
                ));
            }
            let mut preimages = vec![json!(format!("0x{}", hex::encode(&account)))];
            if let Some(policy) = input["policy"].as_bool() {
                let mut payload = b"coldkey-swap-v2-policy".to_vec();
                payload.extend(account);
                payload.push(u8::from(policy));
                preimages.push(json!(format!("0x{}", hex::encode(payload))));
            }
            Ok(json!(preimages))
        }
        "protected.refill_supported" => Ok(json!(
            [
                "transfer",
                "transfer_all",
                "transfer_stake",
                "transfer_stake_and_hotkey",
                "move_stake_limit",
                "swap_stake_limit",
                "move_swap_stake",
                "batch"
            ]
            .contains(&text(&input["op"]))
                || contains(
                    &constants()["PROXY_TYPE_INTENT_OPS"]["Staking"],
                    text(&input["op"])
                )
        )),
        "protected.scope_matches" => {
            Ok(json!(lower(&input["stored"]) == lower(&input["selected"])))
        }
        "protected.config_transition" => config_transition(&input),
        "protected.journal_transition" => journal_transition(&input),
        "protected.flatten_batch_paths" => {
            let mut paths = Vec::new();
            flatten_batch_paths(&input["calls"], vec![], &mut paths, 0)?;
            Ok(json!(paths))
        }
        "protected.flatten_batch" => {
            let mut result = Vec::new();
            flatten_batch(&input["calls"], &mut result, 0)?;
            Ok(json!(result))
        }
        "protected.settings_plan" => settings_plan(&input),
        "protected.pending_refills" => pending_refills(&input),
        "protected.route_gate" => {
            let hotkey = input["hotkey"] == true;
            if hotkey && input["explicit_signer"] != true {
                return Err(PolicyError::invalid("hotkey operations require an explicit signerAddress; the protected controller is a coldkey"));
            }
            if input["explicit_proxy"] == true
                || hotkey
                || contains(
                    &constants()["PROTECTED_BYPASS_OPS"],
                    text(&input["intent"]["op"]),
                )
            {
                return Ok(json!("direct"));
            }
            if let Some(path) = management_path(&input["intent"], 0)? {
                return Err(PolicyError::invalid(format!("refusing ordinary {path} on a keyless protected account; use settings or rotation")));
            }
            Ok(json!("protected"))
        }
        "protected.route" => {
            let instant = evaluate("protected.instant_proxy", input.clone())?;
            if input["op"] == "pow_register" && instant != "Registration" {
                return Err(PolicyError::invalid("PoW proof expires after five blocks; a live zero-delay Registration grant is required"));
            }
            if !instant.is_null() {
                Ok(json!({"kind":"instant","proxy_type":instant}))
            } else if input["dispatcher"] != input["controller"] {
                Err(PolicyError::invalid(
                    "delayed route requires the protected controller",
                ))
            } else {
                Ok(json!({"kind":"delayed"}))
            }
        }
        "protected.decoded_call" => decoded_call(&input["call"], number(&input["depth"]) as usize),
        "protected.validate_call" => {
            validate_call(&input["call"], number(&input["depth"]) as usize)?;
            Ok(Value::Null)
        }
        "protected.requires_direct" => Ok(json!(requires_direct(
            &input["call"],
            number(&input["depth"]) as usize
        ))),
        "protected.call_leaves" => {
            let mut result = Vec::new();
            call_leaves(
                &input["call"],
                number(&input["depth"]) as usize,
                &mut result,
            )?;
            Ok(json!(result))
        }
        "protected.same_semantics" => Ok(json!(
            semantic_value(&input["left"], 0)? == semantic_value(&input["right"], 0)?
        )),
        "protected.receipt_state" => Ok(receipt_state(&input)),
        "protected.multisig_action_result" => multisig_action_result(&input),
        "protected.rotation_plan" => Ok(rotation_plan(&input)),
        "protected.validate_setup_grants" => {
            let controller = &input["controller"];
            let delegate = input
                .get("allow_key")
                .filter(|v| !v.is_null())
                .unwrap_or(controller);
            let grants = rows(&input["grants"]);
            let delay = number(&input["delay_blocks"]);
            let any: Vec<_> = grants
                .iter()
                .filter(|row| row["delegate"] == *controller && row["proxy_type"] == "Any")
                .collect();
            if any.is_empty() || any.iter().any(|row| number(&row["delay"]) != delay) {
                return Err(PolicyError::invalid(
                    "every controller Any grant must match the requested delay",
                ));
            }
            if grants.iter().any(|row| {
                !(row["delegate"] == *controller
                    && row["proxy_type"] == "Any"
                    && number(&row["delay"]) == delay
                    || row["delegate"] == *delegate
                        && number(&row["delay"]) == 0.0
                        && rows(&input["allows"]).contains(&row["proxy_type"]))
            }) {
                return Err(PolicyError::invalid(
                    "unexpected proxy authority; refuse to migrate funds",
                ));
            }
            Ok(Value::Null)
        }
        "protected.live_delay" => {
            let delays = rows(&input["delays"]);
            if delays.is_empty() || delays.iter().any(|value| number(value) <= 0.0) {
                return Err(PolicyError::invalid(
                    "protected controller requires a live nonzero Any delay",
                ));
            }
            Ok(json!(
                delays.iter().map(number).fold(f64::INFINITY, f64::min) as u32
            ))
        }
        "protected.delay" => delay(&input),
        "protected.allow_resolution" => Ok(allow_resolution(&input)),
        "protected.grant_indices" => Ok(grant_indices(&input)),
        "protected.holds_allows" => Ok(json!(
            input["allow_key"].is_null()
                || text(&input["allow_key"]).is_empty()
                || input["allow_key"] == input["controller"]
        )),
        "protected.owned_allows" => Ok(json!(if input["allow_key"].is_null()
            || text(&input["allow_key"]).is_empty()
            || input["allow_key"] == input["controller"]
        {
            unique(rows(&input["allows"]).iter().cloned())
        } else {
            Vec::new()
        })),
        "protected.instant_proxy" => {
            let op = text(&input["op"]);
            let intent = &input["intent"];
            if [
                "remove_stake",
                "remove_stake_limit",
                "move_stake",
                "unstake_all",
            ]
            .contains(&op)
                && intent["claim"] == true
                || op == "set_hyperparameter" && intent["name"] == "registration_allowed"
                || !input["allow_key"].is_null()
                    && !input["dispatcher"].is_null()
                    && input["allow_key"] != input["dispatcher"]
            {
                return Ok(Value::Null);
            }
            Ok(rows(&input["allows"])
                .iter()
                .find(|kind| contains(&constants()["PROXY_TYPE_INTENT_OPS"][text(kind)], op))
                .cloned()
                .unwrap_or(Value::Null))
        }
        "protected.setup_pending" => Ok(json!(if !input["setup"].is_null() {
            input["setup"]["status"] != "ready"
        } else {
            input["coldkey_swap"]["status"] == "pending"
        })),
        "protected.rotation_complete" => {
            let new = &input["new_controller"];
            let old = &input["old_controller"];
            let grants = rows(&input["grants"]);
            Ok(json!(
                grants.iter().any(|row| row["delegate"] == *new
                    && row["proxy_type"] == "Any"
                    && number(&row["delay"]) == number(&input["delay_blocks"]))
                    && (old.is_null()
                        || text(old).is_empty()
                        || old == new
                        || !grants.iter().any(|row| row["delegate"] == *old))
            ))
        }
        "protected.management_path" => Ok(json!(management_path(
            &input["intent"],
            number(&input["depth"]) as usize
        )?)),
        "protected.management_receipt" => {
            Ok(json!(management_receipt(&input["row"], &input["intent"])))
        }
        "protected.announcement_grants" => announcement_grants(&input),
        "protected.funding" => funding(&input),
        "protected.funding_state" => funding_state(input),
        "protected.refill_plan" => refill(&input),
        "protected.refill_position" => refill_position(&input),
        "protected.announcement_category" => Ok(json!(category(text(&input["op"])))),
        "protected.same_commitment" => Ok(json!(same_commitment(&input["left"], &input["right"]))),
        "protected.repeat_commitment_index" => Ok(rows(&input["rows"])
            .iter()
            .find(|row| repeat_approval_required(row) && same_commitment(row, &input["row"]))
            .map(|row| row["index"].clone())
            .unwrap_or(Value::Null)),
        "protected.same_identity" => Ok(json!(
            same_commitment(&input["left"], &input["right"])
                && number(&input["left"]["announced_at_block"])
                    == number(&input["right"]["announced_at_block"])
                && number(&input["left"]["announcement_ordinal"])
                    == number(&input["right"]["announcement_ordinal"])
        )),
        "protected.pure_preimage" => {
            let spawner = hex::decode(text(&input["spawner"]).trim_start_matches("0x"))
                .map_err(|_| PolicyError::type_error("invalid spawner public key"))?;
            if spawner.len() != 32 {
                return Err(PolicyError::range_error(
                    "spawner public key must be 32 bytes",
                ));
            }
            let mut out = b"modlpy/proxy____".to_vec();
            out.extend(spawner);
            for (name, width) in [
                ("height", 4usize),
                ("ext_index", 4),
                ("proxy_type_scale", 1),
                ("index", 2),
            ] {
                let value = uint(
                    input.get(name).unwrap_or(&json!(0)),
                    width.saturating_mul(8),
                    name,
                )?;
                let mut bytes = value.to_bytes_le();
                bytes.resize(width, 0);
                out.extend(bytes);
            }
            Ok(json!(format!("0x{}", hex::encode(out))))
        }
        _ => Err(PolicyError::invalid(format!(
            "unknown SDK policy operation {operation}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // PROTECTED_REGRESSION: deferred crowdloan calls must not hide authority that can strand pure-account funds; changes require explicit human approval.
    #[test]
    fn crowdloan_authority_checks_inspect_deferred_calls() -> Result<()> {
        let intent = json!({"op":"create_crowdloan","call":{"op":"batch","intents":[{"op":"remove_proxies"}]}});
        assert_eq!(
            management_path(&intent, 0)?,
            Some("create_crowdloan.batch[0].remove_proxies".into())
        );
        assert!(evaluate("protected.route_gate", json!({"intent":intent})).is_err());
        let grant = json!({"inner_op":"create_crowdloan","inner_args":{"call":{"op":"add_proxy","delegate_ss58":"controller","proxy_type":"Any","delay":0}}});
        assert_eq!(
            announcement_grants(&grant)?,
            json!([{"delegate_ss58":"controller","proxy_type":"Any","delay":0}])
        );
        let leaf = json!({"pallet":"Proxy","call":"remove_proxies","params":{}});
        let call = json!({"pallet":"Crowdloan","call":"create","params":{"target_address":null,"call":{"pallet":"Utility","call":"batch_all","params":{"calls":[leaf]}}}});
        let mut leaves = vec![];
        call_leaves(&call, 0, &mut leaves)?;
        assert_eq!(leaves, vec![leaf]);
        assert!(!requires_direct(&call, 0));
        Ok(())
    }

    #[test]
    fn config_recovery_preserves_an_existing_claim_and_explicit_field_removal() -> Result<()> {
        let record = json!({"controller_wallet":"alice","controller_ss58":"controller","protected_ss58":"pure","genesis_hash":"chain","coldkey_swap":{"status":"submitting"},"pending_delay_blocks":50});
        let recovered = config_transition(
            &json!({"action":"bind_created","rows":[record],"record":{"controller_wallet":"alice","controller_ss58":"controller","protected_ss58":"pure","genesis_hash":"chain","coldkey_swap":{"status":"pending"}}}),
        )?;
        assert_eq!(recovered["result"]["coldkey_swap"]["status"], "submitting");
        let claimed = config_transition(
            &json!({"action":"reserve_migration","rows":recovered["value"],"wallet":"alice","genesis_hash":"chain","destination":"pure"}),
        )?;
        assert_eq!(claimed["result"]["created"], false);
        let patched = config_transition(
            &json!({"action":"patch","rows":claimed["value"],"wallet":"alice","genesis_hash":"chain","fields":{},"undefined_fields":["pending_delay_blocks"]}),
        )?;
        assert!(patched["result"].get("pending_delay_blocks").is_none());
        assert!(config_transition(&json!({"action":"patch","rows":patched["value"],"wallet":"alice","genesis_hash":"chain","fields":{},"undefined_fields":["protected_ss58"]})).is_err());
        Ok(())
    }
    #[test]
    fn delay_rounding_and_authority_are_shared() -> Result<()> {
        assert_eq!(
            evaluate("protected.delay", json!({"raw":"30s","block_time":12}))?,
            json!(2)
        );
        assert_eq!(
            evaluate("protected.delay", json!({"raw":"42s","block_time":12}))?,
            json!(4)
        );
        assert_eq!(
            evaluate(
                "protected.instant_proxy",
                json!({"op":"remove_stake","allows":["Staking"],"intent":{"claim":true}})
            )?,
            Value::Null
        );
        assert!(
            evaluate(
                "protected.management_path",
                json!({"intent":{"op":"batch","intents":[{"op":"remove_proxies"}]}})
            )? == "batch[0].remove_proxies"
        );
        Ok(())
    }
    #[test]
    fn funding_keeps_large_values_exact_and_deposits_refundable() -> Result<()> {
        let result = funding(
            &json!({"controller":"a","account":{"data":{"free":"9007199254740993","reserved":0}},"announcement_count":0,"announcement_deposit":20,"deposit_base":10,"deposit_factor":2,"existential_deposit":1,"announce_fee":3,"transfer_execution_fee":4,"staking_execution_fee":8,"fee_payer":"a"}),
        )?;
        assert_eq!(result["free_rao"], "9007199254740993");
        assert_eq!(result["target_rao"], "171");
        assert_eq!(result["next_announcement_deposit_rao"], "0");
        Ok(())
    }
}
