//! Semantic transaction planning and policy for the native Rust SDK.
//!
//! Domain packages can construct [`IntentCall`] values without knowing pallet
//! indices. The executor resolves calls against live metadata, simulates fees,
//! enforces one policy choke point, signs, and submits through [`Client`].

#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]

use std::collections::BTreeSet;

use crate::client::{as_u128, Client, TxOutcome};
use crate::codec::Value;
use crate::error::CoreError;
use crate::keys::{Keypair, CRYPTO_SR25519};

/// Which wallet key signs an intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SignerRole {
    Coldkey,
    Hotkey,
}

/// The key material used by semantic intents.
pub struct Wallet {
    pub coldkey: Keypair,
    pub hotkey: Keypair,
}

impl Wallet {
    pub fn from_uris(coldkey_uri: &str, hotkey_uri: &str) -> Result<Self, CoreError> {
        Ok(Self {
            coldkey: Keypair::from_uri(coldkey_uri, CRYPTO_SR25519)?,
            hotkey: Keypair::from_uri(hotkey_uri, CRYPTO_SR25519)?,
        })
    }

    pub fn signer(&self, role: SignerRole) -> &Keypair {
        match role {
            SignerRole::Coldkey => &self.coldkey,
            SignerRole::Hotkey => &self.hotkey,
        }
    }
}

/// Native-currency movement declared by an intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spend {
    None,
    Bounded(u128),
    /// The exact amount depends on live state (`transfer_all`, `unstake_all`, …).
    Unbounded,
}

/// A metadata-resolved call plus its product-level transaction semantics.
#[derive(Debug, Clone)]
pub struct IntentCall {
    pub op: String,
    pub summary: String,
    pub signer: SignerRole,
    pub pallet: String,
    pub function: String,
    pub params: Value,
    pub spend: Spend,
    pub netuids: Vec<u16>,
    pub affects_all_subnets: bool,
    pub raw: bool,
}

impl IntentCall {
    pub fn new(
        op: impl Into<String>,
        signer: SignerRole,
        pallet: impl Into<String>,
        function: impl Into<String>,
        params: Value,
    ) -> Self {
        let op = op.into();
        Self {
            summary: op.replace('_', " "),
            op,
            signer,
            pallet: pallet.into(),
            function: function.into(),
            params,
            spend: Spend::None,
            netuids: Vec::new(),
            affects_all_subnets: false,
            raw: false,
        }
    }

    pub fn summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = summary.into();
        self
    }

    pub fn spend(mut self, spend: Spend) -> Self {
        self.spend = spend;
        self
    }

    pub fn touches(mut self, netuids: impl IntoIterator<Item = u16>) -> Self {
        self.netuids = netuids.into_iter().collect();
        self.netuids.sort_unstable();
        self.netuids.dedup();
        self
    }

    pub fn affects_all_subnets(mut self) -> Self {
        self.affects_all_subnets = true;
        self
    }

    pub fn raw(mut self) -> Self {
        self.raw = true;
        self
    }

    /// Construct an owner-settable subnet hyperparameter call while rejecting
    /// root-only or compound parameters before signing.
    pub fn set_hyperparameter(netuid: u16, name: &str, value: Value) -> Result<Self, CoreError> {
        let (function, is_bool) = match name {
            "immunity_period" => ("sudo_set_immunity_period", false),
            "min_allowed_weights" => ("sudo_set_min_allowed_weights", false),
            "weights_version" => ("sudo_set_weights_version_key", false),
            "activity_cutoff" => ("sudo_set_activity_cutoff", false),
            "min_burn" => ("sudo_set_min_burn", false),
            "bonds_moving_avg" => ("sudo_set_bonds_moving_average", false),
            "serving_rate_limit" => ("sudo_set_serving_rate_limit", false),
            "commit_reveal_period" => ("sudo_set_commit_reveal_weights_interval", false),
            "max_allowed_uids" => ("sudo_set_max_allowed_uids", false),
            "burn_increase_mult" => ("sudo_set_burn_increase_mult", false),
            "burn_half_life" => ("sudo_set_burn_half_life", false),
            "commit_reveal_weights_enabled" => ("sudo_set_commit_reveal_weights_enabled", true),
            "liquid_alpha_enabled" => ("sudo_set_liquid_alpha_enabled", true),
            "network_pow_registration_allowed" => {
                ("sudo_set_network_pow_registration_allowed", true)
            }
            "yuma3_enabled" => ("sudo_set_yuma3_enabled", true),
            "bonds_reset_enabled" => ("sudo_set_bonds_reset_enabled", true),
            "transfers_enabled" => ("sudo_set_toggle_transfer", true),
            "owner_cut_enabled" => ("sudo_set_owner_cut_enabled", true),
            "owner_cut_auto_lock_enabled" => ("sudo_set_owner_cut_auto_lock_enabled", true),
            other => {
                return Err(CoreError::Policy(format!(
                    "unknown or owner-unsettable hyperparameter {other}"
                )))
            }
        };
        if is_bool && !matches!(value, Value::Bool(_)) {
            return Err(CoreError::Policy(format!(
                "hyperparameter {name} requires a boolean value"
            )));
        }
        // AdminUtils owner setters are all two positional fields `(netuid,
        // value)`. Positional input deliberately avoids baking generated Rust
        // field names into the semantic layer; live metadata remains the
        // authority for both order and SCALE types.
        Ok(Self::new(
            "set_hyperparameter",
            SignerRole::Coldkey,
            "AdminUtils",
            function,
            Value::Tuple(vec![Value::Uint(u128::from(netuid)), value]),
        )
        .touches([netuid]))
    }

    /// Construct and validate the runtime's root-claim enum.
    pub fn set_root_claim_type(
        claim_type: &str,
        subnets: Option<Vec<u16>>,
    ) -> Result<Self, CoreError> {
        let value = match (claim_type, subnets) {
            ("Swap" | "Keep", None) => Value::str(claim_type),
            ("KeepSubnets", Some(mut subnets)) if !subnets.is_empty() => {
                subnets.sort_unstable();
                subnets.dedup();
                Value::Dict(vec![(
                    Value::str("KeepSubnets"),
                    Value::record(vec![(
                        "subnets".into(),
                        Value::List(
                            subnets
                                .iter()
                                .copied()
                                .map(|netuid| Value::Uint(u128::from(netuid)))
                                .collect(),
                        ),
                    )]),
                )])
            }
            ("KeepSubnets", _) => {
                return Err(CoreError::Policy(
                    "claim type KeepSubnets requires a non-empty subnets list".into(),
                ))
            }
            ("Swap" | "Keep", Some(_)) => {
                return Err(CoreError::Policy(format!(
                    "subnets are only valid for KeepSubnets, not {claim_type}"
                )))
            }
            (other, _) => {
                return Err(CoreError::Policy(format!(
                    "unknown root claim type {other}"
                )))
            }
        };
        let netuids = match &value {
            Value::Dict(_) => subnets_from_root_claim(&value),
            _ => Vec::new(),
        };
        Ok(Self::new(
            "set_root_claim_type",
            SignerRole::Coldkey,
            "SubtensorModule",
            "set_root_claim_type",
            Value::record(vec![("new_root_claim_type".into(), value)]),
        )
        .touches(netuids))
    }

    /// Set a delegate take to an absolute value, selecting the runtime's
    /// directional call from the current on-chain take. This mirrors the
    /// semantic `set_take` intent without hard-coding a direction at callers.
    pub fn set_take(
        client: &Client,
        hotkey: impl Into<String>,
        take: u16,
    ) -> Result<Self, CoreError> {
        let hotkey = hotkey.into();
        let current_value = client.query(
            "SubtensorModule",
            "Delegates",
            &[Value::str(hotkey.clone())],
            None,
        )?;
        let current = if matches!(current_value, Value::Null) {
            0
        } else {
            as_u128(&current_value).ok_or_else(|| {
                CoreError::Codec("SubtensorModule.Delegates is not an integer".into())
            })?
        };
        let target = u128::from(take);
        if target == current {
            return Err(CoreError::Policy(format!(
                "delegate take is already {take}/{}; nothing to change",
                u16::MAX
            )));
        }
        let function = if target < current {
            "decrease_take"
        } else {
            "increase_take"
        };
        Ok(Self::new(
            "set_take",
            SignerRole::Coldkey,
            "SubtensorModule",
            function,
            Value::record(vec![
                ("hotkey".into(), Value::str(hotkey)),
                ("take".into(), Value::Uint(target)),
            ]),
        )
        .summary(format!(
            "set delegate take to {:.2}% ({take} as u16)",
            f64::from(take) * 100.0 / f64::from(u16::MAX)
        )))
    }

    pub fn encode(&self, client: &Client) -> Result<Vec<u8>, CoreError> {
        client.compose_call(&self.pallet, &self.function, &self.params)
    }

    /// Build an all-or-nothing `Utility.batch_all`, aggregating signer, spend,
    /// and subnet policy metadata across every child.
    pub fn batch(client: &Client, children: Vec<Self>) -> Result<Self, CoreError> {
        let Some(first) = children.first() else {
            return Err(CoreError::Policy(
                "batch requires at least one child".into(),
            ));
        };
        if children.iter().any(|child| child.op == "batch") {
            return Err(CoreError::Policy("nested batches are not supported".into()));
        }
        if children.iter().any(|child| child.signer != first.signer) {
            return Err(CoreError::Policy(
                "all batched intents must share one signer".into(),
            ));
        }
        let mut calls = Vec::with_capacity(children.len());
        let mut spend = Spend::None;
        let mut netuids = BTreeSet::new();
        let mut affects_all_subnets = false;
        let mut summaries = Vec::with_capacity(children.len());
        for child in &children {
            calls.push(Value::Bytes(child.encode(client)?));
            spend = aggregate_spend(spend, child.spend);
            netuids.extend(child.netuids.iter().copied());
            affects_all_subnets |= child.affects_all_subnets;
            summaries.push(child.summary.clone());
        }
        Ok(Self {
            op: "batch".into(),
            summary: format!(
                "atomic batch of {}: {}",
                children.len(),
                summaries.join("; ")
            ),
            signer: first.signer,
            pallet: "Utility".into(),
            function: "batch_all".into(),
            params: Value::record(vec![("calls".into(), Value::List(calls))]),
            spend,
            netuids: netuids.into_iter().collect(),
            affects_all_subnets,
            raw: children.iter().any(|child| child.raw),
        })
    }
}

fn subnets_from_root_claim(value: &Value) -> Vec<u16> {
    let Value::Dict(variants) = value else {
        return Vec::new();
    };
    let Some((_, Value::Dict(fields))) = variants.first() else {
        return Vec::new();
    };
    let Some((_, Value::List(subnets))) = fields
        .iter()
        .find(|(key, _)| matches!(key, Value::Str(name) if name == "subnets"))
    else {
        return Vec::new();
    };
    subnets
        .iter()
        .filter_map(|value| match value {
            Value::Uint(value) => u16::try_from(*value).ok(),
            Value::Int(value) if *value >= 0 => u16::try_from(*value).ok(),
            _ => None,
        })
        .collect()
}

fn aggregate_spend(left: Spend, right: Spend) -> Spend {
    match (left, right) {
        (Spend::Unbounded, _) | (_, Spend::Unbounded) => Spend::Unbounded,
        (Spend::None, other) | (other, Spend::None) => other,
        (Spend::Bounded(left), Spend::Bounded(right)) => left
            .checked_add(right)
            .map_or(Spend::Unbounded, Spend::Bounded),
    }
}

/// Transaction guardrails enforced before any signature is created.
#[derive(Debug, Clone, Default)]
pub struct Policy {
    pub max_fee_rao: Option<u128>,
    pub max_spend_rao: Option<u128>,
    pub allowed_netuids: Option<BTreeSet<u16>>,
    pub allow_raw_calls: bool,
}

impl Policy {
    pub fn check(&self, intent: &IntentCall, fee_rao: Option<u128>) -> Vec<String> {
        let mut violations = Vec::new();
        if intent.raw && !self.allow_raw_calls {
            violations.push("raw calls are disabled by policy".into());
        }
        if let Some(max_fee) = self.max_fee_rao {
            match fee_rao {
                Some(fee) if fee > max_fee => violations.push(format!(
                    "estimated fee {fee} rao exceeds max_fee_rao {max_fee}"
                )),
                None => violations
                    .push("fee estimation failed, so max_fee_rao cannot be enforced safely".into()),
                _ => {}
            }
        }
        if let Some(max_spend) = self.max_spend_rao {
            match intent.spend {
                Spend::Bounded(spend) if spend > max_spend => violations.push(format!(
                    "spend {spend} rao exceeds max_spend_rao {max_spend}"
                )),
                Spend::Unbounded => violations
                    .push("unbounded spend is not allowed while max_spend_rao is set".into()),
                _ => {}
            }
        }
        if let Some(allowed) = &self.allowed_netuids {
            if intent.affects_all_subnets {
                violations.push(
                    "intent affects every subnet but policy only allows an explicit subset".into(),
                );
            }
            for netuid in &intent.netuids {
                if !allowed.contains(netuid) {
                    violations.push(format!("netuid {netuid} is not allowed by policy"));
                }
            }
        }
        violations
    }
}

/// Dry-run output. The exact call bytes in this plan are the bytes later signed.
#[derive(Debug, Clone)]
pub struct Plan {
    pub op: String,
    pub summary: String,
    pub signer: SignerRole,
    pub signer_address: String,
    pub fee_rao: Option<u128>,
    pub warnings: Vec<String>,
    pub violations: Vec<String>,
    pub call_data: Vec<u8>,
}

impl Plan {
    pub fn ok(&self) -> bool {
        self.violations.is_empty()
    }
}

/// The one execution choke point for plans, policy, signing, and submission.
pub struct Executor<'a> {
    client: &'a Client,
    policy: Option<Policy>,
}

impl<'a> Executor<'a> {
    pub fn new(client: &'a Client) -> Self {
        Self {
            client,
            policy: None,
        }
    }

    pub fn with_policy(client: &'a Client, policy: Policy) -> Self {
        Self {
            client,
            policy: Some(policy),
        }
    }

    pub fn plan(&self, intent: &IntentCall, wallet: &Wallet) -> Result<Plan, CoreError> {
        self.plan_with_proxy(intent, wallet, None, None, None)
    }

    pub fn plan_with_policy(
        &self,
        intent: &IntentCall,
        wallet: &Wallet,
        policy: &Policy,
    ) -> Result<Plan, CoreError> {
        self.plan_with_proxy(intent, wallet, Some(policy), None, None)
    }

    pub fn plan_with_proxy(
        &self,
        intent: &IntentCall,
        wallet: &Wallet,
        policy: Option<&Policy>,
        proxy_for: Option<&str>,
        proxy_type: Option<&str>,
    ) -> Result<Plan, CoreError> {
        let mut call_data = intent.encode(self.client)?;
        if let Some(real) = proxy_for {
            call_data = self.client.compose_call(
                "Proxy",
                "proxy",
                &Value::record(vec![
                    ("real".into(), Value::str(real)),
                    (
                        "force_proxy_type".into(),
                        proxy_type.map_or(Value::Null, Value::str),
                    ),
                    ("call".into(), Value::Bytes(call_data)),
                ]),
            )?;
        }
        let signer = wallet.signer(intent.signer);
        let mut warnings = Vec::new();
        let fee_rao = match self.client.estimate_fee(&call_data, signer) {
            Ok(fee) => Some(fee),
            Err(error) => {
                warnings.push(format!("could not estimate fee: {error}"));
                None
            }
        };
        let active = policy.or(self.policy.as_ref());
        let violations = active.map_or_else(Vec::new, |policy| policy.check(intent, fee_rao));
        Ok(Plan {
            op: intent.op.clone(),
            summary: intent.summary.clone(),
            signer: intent.signer,
            signer_address: signer.ss58_address(),
            fee_rao,
            warnings,
            violations,
            call_data,
        })
    }

    pub fn execute(&self, intent: &IntentCall, wallet: &Wallet) -> Result<TxOutcome, CoreError> {
        self.execute_with(intent, wallet, None, None, None, true)
    }

    pub fn execute_with(
        &self,
        intent: &IntentCall,
        wallet: &Wallet,
        policy: Option<&Policy>,
        proxy_for: Option<&str>,
        proxy_type: Option<&str>,
        wait_for_finalization: bool,
    ) -> Result<TxOutcome, CoreError> {
        let plan = self.plan_with_proxy(intent, wallet, policy, proxy_for, proxy_type)?;
        if !plan.ok() {
            return Err(CoreError::Policy(plan.violations.join("; ")));
        }
        self.client.submit(
            &plan.call_data,
            wallet.signer(intent.signer),
            None,
            Some(64),
            wait_for_finalization,
        )
    }

    pub fn submit_shielded(
        &self,
        intent: &IntentCall,
        wallet: &Wallet,
        policy: Option<&Policy>,
    ) -> Result<TxOutcome, CoreError> {
        let plan = self.plan_with_proxy(intent, wallet, policy, None, None)?;
        if !plan.ok() {
            return Err(CoreError::Policy(plan.violations.join("; ")));
        }
        self.client
            .submit_shielded(&plan.call_data, wallet.signer(intent.signer), false)
    }
}
