//! Fixed, metadata-resolved operation constructors for portable semantic intents.
//!
//! Unlike raw calls, every accepted target has constructor-derived signer, spend,
//! and subnet scope. No caller-supplied policy annotations are accepted here.

use super::*;

impl IntentCall {
    /// Construct a whitelisted leaf operation from its qualified pallet/function.
    /// Stateful convenience logic (quotes, defaults, origin resolution) belongs to
    /// the language facade; immutable policy classification is enforced here.
    pub fn operation(operation: &str, params: Value) -> Result<Self, CoreError> {
        let (pallet, function) = operation
            .split_once('.')
            .ok_or_else(|| invalid("operation must be a qualified pallet.function"))?;
        let allowed: &[&str] = match operation {
            "Balances.transfer_keep_alive" | "Balances.transfer_allow_death" => &["dest", "value"],
            "Balances.transfer_all" => &["dest", "keep_alive"],
            "SubtensorModule.add_stake" => &["hotkey", "netuid", "amount_staked"],
            "SubtensorModule.add_stake_limit" => &[
                "hotkey",
                "netuid",
                "amount_staked",
                "limit_price",
                "allow_partial",
            ],
            "SubtensorModule.remove_stake" => &["hotkey", "netuid", "amount_unstaked"],
            "SubtensorModule.remove_stake_limit" => &[
                "hotkey",
                "netuid",
                "amount_unstaked",
                "limit_price",
                "allow_partial",
            ],
            "SubtensorModule.move_stake" => &[
                "origin_hotkey",
                "destination_hotkey",
                "origin_netuid",
                "destination_netuid",
                "alpha_amount",
            ],
            "SubtensorModule.move_stake_limit" => &[
                "origin_hotkey",
                "destination_hotkey",
                "origin_netuid",
                "destination_netuid",
                "alpha_amount",
                "limit_price",
                "allow_partial",
            ],
            "SubtensorModule.swap_stake" => &[
                "hotkey",
                "origin_netuid",
                "destination_netuid",
                "alpha_amount",
            ],
            "SubtensorModule.swap_stake_limit" => &[
                "hotkey",
                "origin_netuid",
                "destination_netuid",
                "alpha_amount",
                "limit_price",
                "allow_partial",
            ],
            "SubtensorModule.transfer_stake" => &[
                "destination_coldkey",
                "hotkey",
                "origin_netuid",
                "destination_netuid",
                "alpha_amount",
            ],
            "SubtensorModule.transfer_stake_and_hotkey" => &[
                "destination_coldkey",
                "origin_hotkey",
                "destination_hotkey",
                "origin_netuid",
                "destination_netuid",
                "alpha_amount",
            ],
            "SubtensorModule.unstake_all"
            | "SubtensorModule.unstake_all_alpha"
            | "SubtensorModule.register_network"
            | "SubtensorModule.root_register"
            | "SubtensorModule.claim_root_with_hotkey"
            | "SubtensorModule.try_associate_hotkey" => &["hotkey"],
            "SubtensorModule.burned_register" | "SubtensorModule.set_coldkey_auto_stake_hotkey" => {
                &["netuid", "hotkey"]
            }
            "SubtensorModule.pow_register" => &["netuid", "work_block", "nonce", "work", "hotkey"],
            "SubtensorModule.start_call" => &["netuid"],
            "SubtensorModule.claim_root" => &["subnets"],
            "SubtensorModule.stake_into_basket" => &["hotkey", "amount_staked"],
            "SubtensorModule.swap_basket" => &[
                "hotkey",
                "origin_netuid",
                "destination_netuid",
                "amount",
                "min_amount_out",
            ],
            "SubtensorModule.swap_basket_many" => &["hotkey", "legs"],
            "SubtensorModule.swap_hotkey" => &["hotkey", "new_hotkey", "netuid"],
            "SubtensorModule.lock_stake" => &["hotkey", "netuid", "amount"],
            "SubtensorModule.set_perpetual_lock" => &["netuid", "enabled"],
            "SubtensorModule.move_lock" => &["netuid", "destination_hotkey"],
            "SubtensorModule.set_reject_locked_alpha"
            | "SubtensorModule.set_reject_staking_hotkey_growth" => &["enabled"],
            "SubtensorModule.add_collateral" => &["netuid", "hotkey", "alpha", "limit_price"],
            "SubtensorModule.set_min_collateral" => &["netuid", "hotkey", "min_locked"],
            "SubtensorModule.add_stake_burn" => &["hotkey", "netuid", "amount", "limit"],
            "SubtensorModule.set_children" => &["hotkey", "netuid", "children"],
            "SubtensorModule.set_childkey_take" => &["hotkey", "netuid", "take"],
            "SubtensorModule.increase_take" | "SubtensorModule.decrease_take" => {
                &["hotkey", "take"]
            }
            "SubtensorModule.set_weights" => &["netuid", "dests", "weights", "version_key"],
            "SubtensorModule.set_mechanism_weights" => {
                &["netuid", "mecid", "dests", "weights", "version_key"]
            }
            "SubtensorModule.commit_timelocked_mechanism_weights" => &[
                "netuid",
                "mecid",
                "commit",
                "reveal_round",
                "commit_reveal_version",
            ],
            "SubtensorModule.reveal_weights" => {
                &["netuid", "uids", "values", "salt", "version_key"]
            }
            "SubtensorModule.serve_axon" => &[
                "netuid",
                "version",
                "ip",
                "port",
                "ip_type",
                "protocol",
                "placeholder1",
                "placeholder2",
            ],
            "SubtensorModule.serve_axon_tls" => &[
                "netuid",
                "version",
                "ip",
                "port",
                "ip_type",
                "protocol",
                "placeholder1",
                "placeholder2",
                "certificate",
            ],
            "SubtensorModule.serve_prometheus" => &["netuid", "version", "ip", "port", "ip_type"],
            "SubtensorModule.associate_evm_key" => {
                &["netuid", "evm_key", "block_number", "signature"]
            }
            "SubtensorModule.set_identity" => &[
                "name",
                "url",
                "github_repo",
                "image",
                "discord",
                "description",
                "additional",
            ],
            "SubtensorModule.set_subnet_identity" => &[
                "netuid",
                "subnet_name",
                "github_repo",
                "subnet_contact",
                "subnet_url",
                "discord",
                "description",
                "logo_url",
                "additional",
            ],
            "SubtensorModule.announce_coldkey_swap_v2"
            | "SubtensorModule.swap_coldkey_announced" => &["new_coldkey"],
            "SubtensorModule.clear_coldkey_swap_announcement"
            | "SubtensorModule.dispute_coldkey_swap"
            | "SubtensorModule.release_own_coldkey_swap_destination_reservation"
            | "Proxy.remove_proxies" => &[],
            "SubtensorModule.register_leased_network" => &["emissions_share", "end_block"],
            "SubtensorModule.terminate_lease" => &["lease_id", "hotkey"],
            "SubtensorModule.update_symbol" => &["netuid", "symbol"],
            "SubtensorModule.sudo_set_root_claim_threshold" => &["netuid", "new_value"],
            "AdminUtils.sudo_trim_to_max_allowed_uids" => &["netuid", "max_n"],
            "AdminUtils.sudo_trim_null_uids_batch" => &["netuid", "target"],
            "AdminUtils.sudo_set_mechanism_count" => &["netuid", "mechanism_count"],
            "AdminUtils.sudo_set_subnet_emission_enabled" => &["netuid", "enabled"],
            "AdminUtils.sudo_set_subnet_emission_weights" => &["netuid", "uids", "values"],
            "AdminUtils.sudo_set_subnet_emission_weights_v2" => &["netuid", "split"],
            "AdminUtils.sudo_set_subnet_gamma_emission_whitelisted" => &["netuid", "whitelisted"],
            "Proxy.add_proxy" | "Proxy.remove_proxy" => &["delegate", "proxy_type", "delay"],
            "Proxy.announce" | "Proxy.remove_announcement" => &["real", "call_hash"],
            "Proxy.reject_announcement" => &["delegate", "call_hash"],
            "Proxy.create_pure" | "Proxy.create_safe_pure" | "Proxy.create_protected_pure" => {
                &["proxy_type", "delay", "index"]
            }
            "Proxy.kill_pure" | "Proxy.kill_safe_pure" => {
                &["spawner", "proxy_type", "index", "height", "ext_index"]
            }
            "HashedAccounts.register" | "HashedAccounts.check_registered" => &["descriptor"],
            "HashedAccounts.check_registration_deposit" => &["max_deposit"],
            "SubtensorModule.mint_gamma" => &[
                "hotkey",
                "netuid",
                "amount",
                "min_amount_out",
                "spender",
                "daily_spend_cap",
            ],
            "SubtensorModule.mint_gamma_with_stake" => &[
                "origin_hotkey",
                "destination_hotkey",
                "netuid",
                "alpha_amount",
                "min_amount_out",
                "spender",
                "daily_spend_cap",
            ],
            "SubtensorModule.move_gamma" => {
                &["origin_hotkey", "destination_hotkey", "netuid", "amount"]
            }
            "SubtensorModule.close_gamma_deposit" => &["hotkey", "netuid"],
            "SubtensorModule.set_gamma_deposit_spender" => &["hotkey", "netuid", "spender"],
            "SubtensorModule.set_gamma_deposit_daily_spend_cap" => {
                &["hotkey", "netuid", "daily_spend_cap"]
            }
            "SubtensorModule.spend_gamma" => &["netuid", "spends"],
            "SubtensorModule.set_service" => &["netuid", "hotkey", "url", "spender"],
            "LimitOrders.authorize_order" | "LimitOrders.cancel_order" => &["order"],
            "LimitOrders.execute_orders" => &["orders", "should_fail"],
            "LimitOrders.execute_batched_orders" => &["netuid", "orders"],
            "EVM.withdraw" | "EVM.deposit" => &["address", "value"],
            "AdminUtils.sudo_set_evm_chain_id" => &["chain_id"],
            "EVM.disable_whitelist" => &["disabled"],
            "Crowdloan.contribute" => &["crowdloan_id", "amount"],
            "Crowdloan.finalize" | "Crowdloan.refund" | "Crowdloan.dissolve"
            | "Crowdloan.withdraw" => &["crowdloan_id"],
            "Crowdloan.update_cap" => &["crowdloan_id", "new_cap"],
            "Crowdloan.update_end" => &["crowdloan_id", "new_end"],
            "Crowdloan.update_min_contribution" => &["crowdloan_id", "new_min_contribution"],
            "Crowdloan.set_max_contribution" => &["crowdloan_id", "new_max_contribution"],
            _ => return owner_hyperparameter(operation, params),
        };
        validate_fields(&params, allowed)?;
        if operation == "HashedAccounts.check_registration_deposit" {
            number(field(&params, "max_deposit")?)?;
        }
        if operation == "AdminUtils.sudo_set_subnet_gamma_emission_whitelisted"
            && !matches!(field(&params, "whitelisted")?, Value::Bool(_))
        {
            return Err(invalid("whitelisted must be a boolean"));
        }
        let mut scope = Vec::new();
        for field in ["netuid", "origin_netuid", "destination_netuid"] {
            if let Some(value) = optional_field(&params, field)? {
                if !matches!(value, Value::Null) {
                    scope.push(as_netuid(value)?);
                }
            }
        }
        if function == "root_register" {
            scope.push(0);
        }
        if pallet == "LimitOrders" {
            if let Some(order) = optional_field(&params, "order")? {
                scope.push(order_netuid(order)?);
            } else {
                for row in as_list(field(&params, "orders")?)? {
                    scope.push(order_netuid(field(row, "order")?)?);
                }
            }
        }
        if function == "claim_root" {
            for value in as_list(field(&params, "subnets")?)? {
                as_netuid(value)?;
            }
        }
        if function == "swap_basket_many" {
            let legs = as_list(field(&params, "legs")?)?;
            if legs.is_empty() || legs.len() > 128 {
                return Err(invalid("basket requires 1 to 128 legs"));
            }
            for leg in legs {
                let values = as_list(leg)?;
                if values.len() != 4 {
                    return Err(invalid("basket leg requires four values"));
                }
                scope.push(as_netuid(&values[0])?);
                scope.push(as_netuid(&values[1])?);
                number(&values[2])?;
                number(&values[3])?;
            }
        }
        let spend = match operation {
            "EVM.deposit" => match field(&params, "value")? {
                Value::Null => Spend::Unbounded,
                value => Spend::Bounded(number(value)?),
            },
            "Balances.transfer_keep_alive" | "Balances.transfer_allow_death" => {
                Spend::Bounded(number(field(&params, "value")?)?)
            }
            "SubtensorModule.add_stake"
            | "SubtensorModule.add_stake_limit"
            | "SubtensorModule.stake_into_basket" => {
                Spend::Bounded(number(field(&params, "amount_staked")?)?)
            }
            "SubtensorModule.add_stake_burn"
            | "SubtensorModule.mint_gamma"
            | "Crowdloan.contribute" => Spend::Bounded(number(field(&params, "amount")?)?),
            "Balances.transfer_all"
            | "LimitOrders.authorize_order"
            | "HashedAccounts.register"
            | "SubtensorModule.burned_register"
            | "SubtensorModule.register_network"
            | "SubtensorModule.register_leased_network"
            | "SubtensorModule.transfer_stake"
            | "SubtensorModule.transfer_stake_and_hotkey"
            | "SubtensorModule.add_collateral"
            | "SubtensorModule.mint_gamma_with_stake"
            | "SubtensorModule.move_gamma"
            | "SubtensorModule.set_gamma_deposit_spender"
            | "Crowdloan.finalize" => Spend::Unbounded,
            _ => Spend::None,
        };
        let role = if matches!(
            function,
            "set_weights"
                | "set_mechanism_weights"
                | "commit_timelocked_mechanism_weights"
                | "reveal_weights"
                | "serve_axon"
                | "serve_axon_tls"
                | "serve_prometheus"
                | "associate_evm_key"
        ) || matches!(function, "add_proxy" | "remove_proxy")
            && matches!(
                string(field(&params, "proxy_type")?)?,
                "Validate" | "Weights"
            ) {
            SignerRole::Hotkey
        } else {
            SignerRole::Coldkey
        };
        let all = matches!(
            function,
            "unstake_all"
                | "unstake_all_alpha"
                | "swap_coldkey_announced"
                | "claim_root"
                | "claim_root_with_hotkey"
                | "stake_into_basket"
        ) || operation == "Crowdloan.finalize"
            // A full split can remove targets absent from its parameters. Only
            // the state-aware descriptor constructor can prove its full scope.
            || operation == "AdminUtils.sudo_set_subnet_emission_weights_v2"
            || operation == "AdminUtils.sudo_set_subnet_gamma_emission_whitelisted"
                && matches!(field(&params, "whitelisted")?, Value::Bool(false))
            || function == "swap_hotkey" && matches!(field(&params, "netuid")?, Value::Null);
        let global = scope.is_empty() && !all;
        let constructor = if global {
            Self::trusted_global
        } else {
            Self::trusted
        };
        Ok(constructor(
            operation, role, pallet, function, params, spend, scope, all,
        ))
    }

    /// Build nested trusted descriptors, encoding each child from the same live metadata.
    /// Wrappers inherit their child's immutable policy rather than trusting annotations.
    pub fn from_descriptor(client: &Client, descriptor: &Value) -> Result<Self, CoreError> {
        descriptor_inner(client, descriptor, 0, &mut None)
    }
}

fn descriptor_inner(
    client: &Client,
    descriptor: &Value,
    depth: usize,
    scope_block: &mut Option<String>,
) -> Result<IntentCall, CoreError> {
    let mut call = descriptor_unchecked(client, descriptor, depth, scope_block)?;
    if let Some(role) = optional_field(descriptor, "signerRole")? {
        if call.pallet != "Proxy" || call.function != "add_proxy" {
            return Err(invalid(
                "explicit signer role is only supported for Proxy.add_proxy",
            ));
        }
        call.signer = match string(role)? {
            "coldkey" => SignerRole::Coldkey,
            "hotkey" => SignerRole::Hotkey,
            _ => return Err(invalid("invalid explicit signer role")),
        };
    }
    if let Some(data) = optional_field(descriptor, "callData")? {
        if call.encode(client)? != value_bytes(data)? {
            return Err(invalid("saved call bytes do not match the decoded call"));
        }
    }
    Ok(call)
}

fn descriptor_unchecked(
    client: &Client,
    descriptor: &Value,
    depth: usize,
    scope_block: &mut Option<String>,
) -> Result<IntentCall, CoreError> {
    if depth > 64 {
        return Err(invalid("intent descriptor nesting exceeds 64"));
    }
    let pallet = string(field(descriptor, "pallet")?)?;
    let function = string(field(descriptor, "call")?)?;
    let params = field(descriptor, "params")?.clone();
    let operation = format!("{pallet}.{function}");
    if operation == "Utility.batch_all" {
        validate_fields(&params, &["calls"])?;
        let children = as_list(field(&params, "calls")?)?
            .iter()
            .map(|child| descriptor_inner(client, child, depth + 1, scope_block))
            .collect::<Result<Vec<_>, _>>()?;
        let mut result = IntentCall::batch(client, children)?;
        // A composite semantic operation can contain its own atomic batch.
        result.op = "prepared_batch".into();
        return Ok(result);
    }
    let wrapper_fields: &[&str] = match operation.as_str() {
        "Sudo.sudo" => &["call"],
        "Proxy.proxy" => &["real", "force_proxy_type", "call"],
        "Proxy.proxy_announced" => &["delegate", "real", "force_proxy_type", "call"],
        "Utility.as_derivative"
        | "Utility.as_safe_derivative"
        | "Utility.as_protected_derivative" => &["index", "call"],
        "Multisig.as_multi_threshold_1" | "Multisig.as_multi_threshold_1_safe" => {
            &["other_signatories", "call"]
        }
        "Multisig.as_multi" | "Multisig.as_multi_safe" => &[
            "threshold",
            "other_signatories",
            "maybe_timepoint",
            "call",
            "max_weight",
        ],
        "Multisig.approve_as_multi" | "Multisig.approve_as_multi_safe" => &[
            "threshold",
            "other_signatories",
            "maybe_timepoint",
            "call_hash",
            "max_weight",
        ],
        "Multisig.cancel_as_multi" | "Multisig.cancel_as_multi_safe" => {
            &["threshold", "other_signatories", "timepoint", "call_hash"]
        }
        "Crowdloan.create" => &[
            "deposit",
            "min_contribution",
            "cap",
            "end",
            "call",
            "target_address",
        ],
        _ => {
            let mut call = IntentCall::operation(&operation, params)?;
            if operation == "AdminUtils.sudo_set_subnet_emission_weights_v2" {
                // Keep all replacements in a nested descriptor on one chain
                // snapshot. RPC/metadata/decoding failures must not narrow scope.
                let hash = match scope_block {
                    Some(hash) => hash,
                    None => scope_block.insert(client.block_hash(None)?),
                };
                let previous = client.query(
                    "SubtensorModule",
                    "SubnetGammaEmission",
                    &[field(&call.params, "netuid")?.clone()],
                    Some(hash),
                )?;
                call.security.netuids = emission_split_scope(&call.params, &previous)?;
                call.security.affects_all_subnets = false;
            }
            return Ok(call);
        }
    };
    validate_fields(&params, wrapper_fields)?;
    if operation == "Crowdloan.create" && matches!(field(&params, "call")?, Value::Null) {
        if matches!(field(&params, "target_address")?, Value::Null) {
            return Err(invalid("crowdloan needs target or call"));
        }
        let deposit = number(field(&params, "deposit")?)?;
        return Ok(IntentCall::trusted_global(
            &operation,
            SignerRole::Coldkey,
            pallet,
            function,
            params,
            Spend::Bounded(deposit),
            [],
            false,
        ));
    }
    let hash_only = matches!(
        operation.as_str(),
        "Multisig.approve_as_multi"
            | "Multisig.cancel_as_multi"
            | "Multisig.approve_as_multi_safe"
            | "Multisig.cancel_as_multi_safe"
    );
    let inner_descriptor = if hash_only {
        field(descriptor, "semanticCall")?
    } else {
        field(&params, "call")?
    };
    if operation == "Sudo.sudo" && !root_descriptor(inner_descriptor)? {
        return Err(invalid("trusted sudo only wraps declared root operations"));
    }
    let child = descriptor_inner(client, inner_descriptor, depth + 1, scope_block)?;
    // descriptor_inner verified saved bytes against the decoded constructor.
    // Preserve that immutable payload instead of composing it a second time.
    let encoded = match optional_field(inner_descriptor, "callData")? {
        Some(data) => value_bytes(data)?,
        None => child.encode(client)?,
    };
    let mut params = params;
    if hash_only {
        let supplied_hash = value_bytes(field(&params, "call_hash")?)?;
        if supplied_hash != sp_core::hashing::blake2_256(&encoded) {
            return Err(invalid("multisig hash does not match its semantic call"));
        }
    } else {
        replace_field(&mut params, "call", Value::Bytes(encoded))?;
    }
    let mut security = child.security.clone();
    if operation == "Crowdloan.create" {
        if !matches!(field(&params, "target_address")?, Value::Null) {
            return Err(invalid("crowdloan call and target are mutually exclusive"));
        }
        security.spend = aggregate_spend(
            security.spend,
            Spend::Bounded(number(field(&params, "deposit")?)?),
        );
        security.global = true;
    }
    if matches!(
        operation.as_str(),
        "Multisig.cancel_as_multi" | "Multisig.cancel_as_multi_safe"
    ) {
        security = SecuritySemantics::trusted(Spend::None, [], false).global();
    }
    let signer = if operation == "Proxy.proxy" {
        child.signer
    } else {
        SignerRole::Coldkey
    };
    Ok(IntentCall::from_parts(
        operation, signer, pallet, function, params, security,
    ))
}

fn emission_split_scope(params: &Value, previous: &Value) -> Result<Vec<u16>, CoreError> {
    let mut scope = BTreeSet::from([as_netuid(field(params, "netuid")?)?]);
    for row in as_list(previous)? {
        let [target, share] = as_list(row)?.as_slice() else {
            return Err(invalid("invalid stored Gamma emission share"));
        };
        scope.insert(as_netuid(target)?);
        u64::try_from(number(share)?).map_err(|_| invalid("Gamma share exceeds u64"))?;
    }
    for row in as_list(field(params, "split")?)? {
        let [slot, share] = as_list(row)?.as_slice() else {
            return Err(invalid("invalid emission split entry"));
        };
        u64::try_from(number(share)?).map_err(|_| invalid("emission share exceeds u64"))?;
        match slot {
            Value::Str(name)
                if matches!(
                    name.as_str(),
                    "Burn" | "Recycle" | "Owner" | "Validator" | "Miner"
                ) => {}
            Value::Dict(_) => {
                validate_fields(slot, &["Gamma"])?;
                let gamma = field(slot, "Gamma")?;
                validate_fields(gamma, &["netuid"])?;
                scope.insert(as_netuid(field(gamma, "netuid")?)?);
            }
            _ => return Err(invalid("invalid emission slot")),
        }
    }
    Ok(scope.into_iter().collect())
}

fn order_netuid(order: &Value) -> Result<u16, CoreError> {
    let order = if let Some(authorized) = optional_field(order, "Authorized")? {
        field(authorized, "terms")?
    } else {
        order
    };
    let terms = optional_field(order, "V1")?
        .or(optional_field(order, "V2")?)
        .ok_or_else(|| invalid("expected V1 or V2 order terms"))?;
    as_netuid(field(terms, "netuid")?)
}

fn root_descriptor(value: &Value) -> Result<bool, CoreError> {
    let operation = format!(
        "{}.{}",
        string(field(value, "pallet")?)?,
        string(field(value, "call")?)?
    );
    if matches!(
        operation.as_str(),
        "AdminUtils.sudo_set_subnet_emission_enabled"
            | "AdminUtils.sudo_set_subnet_gamma_emission_whitelisted"
            | "SubtensorModule.sudo_set_root_claim_threshold"
            | "AdminUtils.sudo_set_evm_chain_id"
            | "EVM.disable_whitelist"
    ) {
        return Ok(true);
    }
    if operation != "Utility.batch_all" {
        return Ok(false);
    }
    let children = as_list(field(field(value, "params")?, "calls")?)?;
    if children.is_empty() {
        return Ok(false);
    }
    for child in children {
        if !root_descriptor(child)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn owner_hyperparameter(operation: &str, params: Value) -> Result<IntentCall, CoreError> {
    let Some((pallet, function)) = operation.split_once('.') else {
        return Err(invalid("unknown operation"));
    };
    if pallet != "AdminUtils" {
        return Err(invalid(format!("unknown trusted operation {operation}")));
    }
    let netuid = as_netuid(field(&params, "netuid")?)?;
    // Matches Python's owner-only catalog. The old absolute activity cutoff and
    // owner-cut switch are deliberately absent: both are now derived values.
    let allowed: &[&str] = match function {
        "sudo_set_epoch_consensus" => &["netuid", "mode"],
        "sudo_set_tempo" => &["netuid", "tempo"],
        "sudo_set_immunity_period" => &["netuid", "immunity_period"],
        "sudo_set_min_allowed_weights" => &["netuid", "min_allowed_weights"],
        "sudo_set_weights_version_key" => &["netuid", "weights_version_key"],
        "sudo_set_activity_cutoff_factor" => &["netuid", "factor_milli"],
        "sudo_set_min_burn" => &["netuid", "min_burn"],
        "sudo_set_max_burn" => &["netuid", "max_burn"],
        "sudo_set_bonds_moving_average" => &["netuid", "bonds_moving_average"],
        "sudo_set_bonds_penalty" => &["netuid", "bonds_penalty"],
        "sudo_set_serving_rate_limit" => &["netuid", "serving_rate_limit"],
        "sudo_set_commit_reveal_weights_interval" => &["netuid", "interval"],
        "sudo_set_max_allowed_uids" => &["netuid", "max_allowed_uids"],
        "sudo_set_burn_increase_mult" => &["netuid", "burn_increase_mult"],
        "sudo_set_burn_half_life" => &["netuid", "burn_half_life"],
        "sudo_set_collateral_lock_share" => &["netuid", "lock_share"],
        "sudo_set_collateral_drain_ratio" => &["netuid", "drain_ratio"],
        "sudo_set_adjustment_alpha" => &["netuid", "adjustment_alpha"],
        "sudo_set_rho" => &["netuid", "rho"],
        "sudo_set_max_difficulty" => &["netuid", "max_difficulty"],
        "sudo_set_alpha_sigmoid_steepness" => &["netuid", "steepness"],
        "sudo_set_min_childkey_take_per_subnet" => &["netuid", "take"],
        "sudo_set_owner_immune_neuron_limit" => &["netuid", "immune_neurons"],
        "sudo_set_alpha_values" => &["netuid", "alpha_low", "alpha_high"],
        "sudo_set_network_registration_allowed" | "sudo_set_network_pow_registration_allowed" => {
            &["netuid", "registration_allowed"]
        }
        "sudo_set_toggle_transfer" => &["netuid", "toggle"],
        "sudo_set_commit_reveal_weights_enabled"
        | "sudo_set_liquid_alpha_enabled"
        | "sudo_set_yuma3_enabled"
        | "sudo_set_subnet_gamma_enabled"
        | "sudo_set_bonds_reset_enabled"
        | "sudo_set_owner_cut_auto_lock_enabled" => &["netuid", "enabled"],
        _ => return Err(invalid(format!("unknown trusted operation {operation}"))),
    };
    validate_fields(&params, allowed)?;
    Ok(IntentCall::trusted(
        "set_hyperparameter",
        SignerRole::Coldkey,
        pallet,
        function,
        params,
        Spend::None,
        [netuid],
        false,
    ))
}

fn invalid(message: impl Into<String>) -> CoreError {
    CoreError::Policy(message.into())
}
fn as_fields(value: &Value) -> Result<&Vec<(Value, Value)>, CoreError> {
    if let Value::Dict(fields) = value {
        Ok(fields)
    } else {
        Err(invalid("expected a named record"))
    }
}
fn optional_field<'a>(value: &'a Value, name: &str) -> Result<Option<&'a Value>, CoreError> {
    let fields = as_fields(value)?;
    let mut matches = fields
        .iter()
        .filter(|(key, _)| matches!(key, Value::Str(key) if key == name));
    let result = matches.next().map(|(_, value)| value);
    if matches.next().is_some() {
        return Err(invalid(format!("duplicate field {name}")));
    }
    Ok(result)
}
fn field<'a>(value: &'a Value, name: &str) -> Result<&'a Value, CoreError> {
    optional_field(value, name)?.ok_or_else(|| invalid(format!("missing field {name}")))
}
fn replace_field(value: &mut Value, name: &str, replacement: Value) -> Result<(), CoreError> {
    if let Value::Dict(fields) = value {
        if let Some((_, value)) = fields
            .iter_mut()
            .find(|(key, _)| matches!(key, Value::Str(key) if key == name))
        {
            *value = replacement;
            return Ok(());
        }
    }
    Err(invalid(format!("missing field {name}")))
}
fn validate_fields(value: &Value, allowed: &[&str]) -> Result<(), CoreError> {
    let fields = as_fields(value)?;
    if fields.len() != allowed.len() {
        return Err(invalid("operation has missing or unknown parameters"));
    }
    for name in allowed {
        field(value, name)?;
    }
    Ok(())
}
fn number(value: &Value) -> Result<u128, CoreError> {
    match value {
        Value::Uint(value) => Ok(*value),
        Value::Int(value) if *value >= 0 => Ok(*value as u128),
        _ => Err(invalid("expected a nonnegative SCALE integer")),
    }
}
fn as_netuid(value: &Value) -> Result<u16, CoreError> {
    u16::try_from(number(value)?).map_err(|_| invalid("netuid exceeds u16"))
}
fn string(value: &Value) -> Result<&str, CoreError> {
    if let Value::Str(value) = value {
        Ok(value)
    } else {
        Err(invalid("expected string"))
    }
}
fn as_list(value: &Value) -> Result<&Vec<Value>, CoreError> {
    match value {
        Value::List(values) | Value::Tuple(values) => Ok(values),
        _ => Err(invalid("expected list")),
    }
}
fn value_bytes(value: &Value) -> Result<Vec<u8>, CoreError> {
    match value {
        Value::Bytes(value) => Ok(value.clone()),
        Value::Str(value) => {
            hex::decode(value.trim_start_matches("0x")).map_err(|_| invalid("invalid hex"))
        }
        _ => Err(invalid("expected bytes")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(fields: &[(&str, Value)]) -> Value {
        Value::record(
            fields
                .iter()
                .map(|(key, value)| ((*key).into(), value.clone()))
                .collect(),
        )
    }

    #[test]
    fn fixed_operations_cannot_understate_spend_or_scope() -> Result<(), CoreError> {
        let call = IntentCall::operation(
            "SubtensorModule.add_stake_limit",
            record(&[
                ("hotkey", Value::str("hotkey")),
                ("netuid", Value::Int(7)),
                ("amount_staked", Value::Int(500)),
                ("limit_price", Value::Int(1000)),
                ("allow_partial", Value::Bool(false)),
            ]),
        )?;
        let policy = Policy {
            max_spend_rao: Some(499),
            allowed_netuids: Some([8].into_iter().collect()),
            ..Default::default()
        };
        assert_eq!(policy.check(&call, None).len(), 2);
        assert!(!call.security.raw);
        assert!(IntentCall::operation("Balances.force_transfer", record(&[])).is_err());
        Ok(())
    }

    #[test]
    fn delayed_crowdloan_execution_keeps_unknown_scope() -> Result<(), CoreError> {
        let call = IntentCall::operation(
            "Crowdloan.finalize",
            record(&[("crowdloan_id", Value::Int(1))]),
        )?;
        assert!(call.security.affects_all_subnets);
        assert_eq!(call.security.spend, Spend::Unbounded);
        Ok(())
    }

    #[test]
    fn basket_claim_compatibility_subnets_do_not_restrict_actual_scope() -> Result<(), CoreError> {
        let claim = IntentCall::operation(
            "SubtensorModule.claim_root",
            record(&[("subnets", Value::List(vec![Value::Int(7)]))]),
        )?;
        let policy = Policy {
            allowed_netuids: Some([7].into_iter().collect()),
            ..Default::default()
        };
        assert!(claim.security.affects_all_subnets);
        assert!(!policy.check(&claim, None).is_empty());
        for (operation, params) in [
            (
                "SubtensorModule.claim_root_with_hotkey",
                record(&[("hotkey", Value::str("validator"))]),
            ),
            (
                "SubtensorModule.stake_into_basket",
                record(&[
                    ("hotkey", Value::str("validator")),
                    ("amount_staked", Value::Uint(1)),
                ]),
            ),
        ] {
            assert!(!policy
                .check(&IntentCall::operation(operation, params)?, None)
                .is_empty());
        }
        Ok(())
    }

    #[test]
    fn gamma_operations_preserve_funds_and_subnet_policy() -> Result<(), CoreError> {
        let call = IntentCall::operation(
            "SubtensorModule.mint_gamma",
            record(&[
                ("hotkey", Value::str("hotkey")),
                ("netuid", Value::Int(7)),
                ("amount", Value::Uint(500)),
                ("min_amount_out", Value::Uint(1)),
                ("spender", Value::Null),
                ("daily_spend_cap", Value::Uint(100)),
            ]),
        )?;
        assert_eq!(call.security.spend, Spend::Bounded(500));
        let policy = Policy {
            max_spend_rao: Some(499),
            allowed_netuids: Some([8].into_iter().collect()),
            ..Default::default()
        };
        assert_eq!(policy.check(&call, None).len(), 2);
        let delegate = IntentCall::operation(
            "SubtensorModule.set_gamma_deposit_spender",
            record(&[
                ("hotkey", Value::str("hotkey")),
                ("netuid", Value::Int(7)),
                ("spender", Value::str("spender")),
            ]),
        )?;
        assert_eq!(delegate.security.spend, Spend::Unbounded);
        assert_eq!(policy.check(&delegate, None).len(), 2);
        Ok(())
    }

    #[test]
    fn malformed_policy_fields_are_rejected() {
        assert!(IntentCall::operation(
            "Balances.transfer_keep_alive",
            record(&[("dest", Value::str("target")), ("value", Value::str("1"))])
        )
        .is_err());
        assert!(IntentCall::operation(
            "Balances.transfer_keep_alive",
            record(&[
                ("dest", Value::str("target")),
                ("value", Value::Int(1)),
                ("spend", Value::Int(0))
            ])
        )
        .is_err());
    }

    #[test]
    fn release_491_operations_enforce_target_scope() -> Result<(), CoreError> {
        assert!(IntentCall::operation(
            "AdminUtils.sudo_set_subnet_gamma_emission_whitelisted",
            record(&[("netuid", Value::Int(7)), ("whitelisted", Value::Int(0))]),
        )
        .is_err());
        let policy = Policy {
            allowed_netuids: Some([7].into_iter().collect()),
            ..Default::default()
        };
        for whitelisted in [false, true] {
            let call = IntentCall::operation(
                "AdminUtils.sudo_set_subnet_gamma_emission_whitelisted",
                record(&[
                    ("netuid", Value::Int(7)),
                    ("whitelisted", Value::Bool(whitelisted)),
                ]),
            )?;
            assert_eq!(call.security.affects_all_subnets, !whitelisted);
            assert_eq!(policy.check(&call, None).is_empty(), whitelisted);
        }
        let split = IntentCall::operation(
            "AdminUtils.sudo_set_subnet_emission_weights_v2",
            record(&[("netuid", Value::Int(8)), ("split", Value::List(vec![]))]),
        )?;
        assert!(!policy.check(&split, None).is_empty());
        for (operation, account) in [
            ("Proxy.announce", "real"),
            ("Proxy.remove_announcement", "real"),
            ("Proxy.reject_announcement", "delegate"),
        ] {
            let call = IntentCall::operation(
                operation,
                record(&[
                    (account, Value::str("account")),
                    ("call_hash", Value::Bytes(vec![0; 32])),
                ]),
            )?;
            assert!(!call.security.raw);
            assert_eq!(call.security.spend, Spend::None);
        }
        Ok(())
    }

    #[test]
    fn gamma_enabled_owner_hyperparameter_retains_exact_subnet_scope() -> Result<(), CoreError> {
        for enabled in [false, true] {
            let call = IntentCall::operation(
                "AdminUtils.sudo_set_subnet_gamma_enabled",
                record(&[("netuid", Value::Int(7)), ("enabled", Value::Bool(enabled))]),
            )?;
            assert_eq!(call.security.netuids, vec![7]);
            assert!(!call.security.affects_all_subnets);
            assert_eq!(call.signer_role(), SignerRole::Coldkey);
            for (allowed, accepted) in [(7, true), (8, false)] {
                let policy = Policy {
                    allowed_netuids: Some([allowed].into_iter().collect()),
                    ..Default::default()
                };
                assert_eq!(policy.check(&call, None).is_empty(), accepted);
            }
        }
        Ok(())
    }

    #[test]
    fn gamma_v2_scope_rejects_malformed_split_or_previous_shares() {
        let params = |slot| {
            record(&[
                ("netuid", Value::Int(1)),
                (
                    "split",
                    Value::List(vec![Value::Tuple(vec![slot, Value::Uint(1)])]),
                ),
            ])
        };
        let valid = params(Value::str("Miner"));
        for previous in [
            Value::Null,
            Value::List(vec![Value::Tuple(vec![Value::Int(7)])]),
            Value::List(vec![Value::Tuple(vec![Value::Int(-1), Value::Int(1)])]),
            Value::List(vec![Value::Tuple(vec![Value::Int(65536), Value::Int(1)])]),
            Value::List(vec![Value::Tuple(vec![
                Value::Int(7),
                Value::Uint(u128::MAX),
            ])]),
        ] {
            assert!(emission_split_scope(&valid, &previous).is_err());
        }
        for slot in [
            Value::str("Unknown"),
            record(&[("Gamma", Value::Int(7))]),
            record(&[("Gamma", record(&[("netuid", Value::Int(65536))]))]),
            record(&[(
                "Gamma",
                record(&[("netuid", Value::Int(7)), ("extra", Value::Int(8))]),
            )]),
            record(&[
                ("Gamma", record(&[("netuid", Value::Int(7))])),
                ("Miner", Value::Null),
            ]),
        ] {
            assert!(emission_split_scope(&params(slot), &Value::List(vec![])).is_err());
        }
    }
}
