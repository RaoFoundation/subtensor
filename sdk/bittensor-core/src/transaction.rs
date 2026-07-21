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
    /// The exact outgoing amount depends on live state (`transfer_all`, dynamic registration, …).
    Unbounded,
}

/// Policy semantics proven by a trusted constructor, or the conservative
/// classification assigned to an arbitrary metadata call.
#[derive(Debug, Clone)]
struct SecuritySemantics {
    spend: Spend,
    netuids: Vec<u16>,
    affects_all_subnets: bool,
    raw: bool,
}

impl SecuritySemantics {
    fn arbitrary() -> Self {
        Self {
            spend: Spend::Unbounded,
            netuids: Vec::new(),
            // An arbitrary call may inspect or mutate any subnet. Treat its
            // scope as unknown/all rather than letting an empty list bypass an
            // allowlist.
            affects_all_subnets: true,
            raw: true,
        }
    }

    fn trusted(
        spend: Spend,
        netuids: impl IntoIterator<Item = u16>,
        affects_all_subnets: bool,
    ) -> Self {
        let mut netuids: Vec<u16> = netuids.into_iter().collect();
        netuids.sort_unstable();
        netuids.dedup();
        Self {
            spend,
            netuids,
            affects_all_subnets,
            raw: false,
        }
    }
}

/// A metadata-resolved call plus immutable product-level transaction semantics.
///
/// The call target, parameters, signer, and policy classification are private
/// so callers cannot change the encoded call after a trusted constructor has
/// attached its spend/subnet semantics. Use [`IntentCall::new`] only for an
/// arbitrary call: it is deliberately classified as raw, unbounded, and
/// unknown/all-subnet scope. Narrower semantics are available only through
/// fixed operation-specific constructors in this module.
#[derive(Debug, Clone)]
pub struct IntentCall {
    pub op: String,
    pub summary: String,
    signer: SignerRole,
    pallet: String,
    function: String,
    params: Value,
    security: SecuritySemantics,
}

impl IntentCall {
    /// Construct an arbitrary metadata call.
    ///
    /// Arbitrary calls are always `raw`, `Spend::Unbounded`, and treated as
    /// affecting every subnet. This fail-closed classification means they
    /// require explicit `allow_raw_calls`, cannot pass a spend cap, and cannot
    /// pass an explicit subnet allowlist. Callers cannot downgrade that
    /// classification after construction.
    pub fn new(
        op: impl Into<String>,
        signer: SignerRole,
        pallet: impl Into<String>,
        function: impl Into<String>,
        params: Value,
    ) -> Self {
        Self::from_parts(
            op,
            signer,
            pallet,
            function,
            params,
            SecuritySemantics::arbitrary(),
        )
    }

    /// Explicitly named alias for [`IntentCall::new`].
    pub fn raw_call(
        op: impl Into<String>,
        signer: SignerRole,
        pallet: impl Into<String>,
        function: impl Into<String>,
        params: Value,
    ) -> Self {
        Self::new(op, signer, pallet, function, params)
    }

    #[allow(clippy::too_many_arguments)]
    fn trusted(
        op: impl Into<String>,
        signer: SignerRole,
        pallet: impl Into<String>,
        function: impl Into<String>,
        params: Value,
        spend: Spend,
        netuids: impl IntoIterator<Item = u16>,
        affects_all_subnets: bool,
    ) -> Self {
        Self::from_parts(
            op,
            signer,
            pallet,
            function,
            params,
            SecuritySemantics::trusted(spend, netuids, affects_all_subnets),
        )
    }

    fn from_parts(
        op: impl Into<String>,
        signer: SignerRole,
        pallet: impl Into<String>,
        function: impl Into<String>,
        params: Value,
        security: SecuritySemantics,
    ) -> Self {
        let op = op.into();
        Self {
            summary: op.replace('_', " "),
            op,
            signer,
            pallet: pallet.into(),
            function: function.into(),
            params,
            security,
        }
    }

    /// Stable operation name used in plans and diagnostics.
    pub fn op(&self) -> &str {
        &self.op
    }

    /// Signer selected by the constructor. Read-only: callers cannot change it.
    pub fn signer_role(&self) -> SignerRole {
        self.signer
    }

    /// Metadata call target. Read-only: callers cannot replace the trusted call.
    pub fn pallet(&self) -> &str {
        &self.pallet
    }

    /// Metadata function target. Read-only: callers cannot replace the trusted call.
    pub fn function(&self) -> &str {
        &self.function
    }

    /// SCALE input value. Read-only: callers cannot replace trusted parameters.
    pub fn params(&self) -> &Value {
        &self.params
    }

    /// Replace presentation text only. This cannot alter policy semantics.
    pub fn summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = summary.into();
        self
    }

    /// Compatibility policy floor for older call sites. This can only make a
    /// trusted classification stricter. Arbitrary calls remain unbounded.
    pub fn spend(mut self, spend: Spend) -> Self {
        self.security.spend = strictest_spend(self.security.spend, spend);
        self
    }

    /// Compatibility policy floor for older call sites. Supplied netuids are
    /// unioned with constructor-derived scope; they can never remove scope.
    pub fn touches(mut self, netuids: impl IntoIterator<Item = u16>) -> Self {
        self.security.netuids.extend(netuids);
        self.security.netuids.sort_unstable();
        self.security.netuids.dedup();
        self
    }

    /// Broaden this call's subnet scope. There is intentionally no inverse.
    pub fn affects_all_subnets(mut self) -> Self {
        self.security.affects_all_subnets = true;
        self
    }

    /// Force the arbitrary-call classification. There is intentionally no
    /// inverse that can turn a raw call into a trusted call.
    pub fn raw(mut self) -> Self {
        self.security = SecuritySemantics::arbitrary();
        self
    }

    /// A bounded keep-alive TAO transfer.
    pub fn transfer(dest: impl Into<String>, amount_rao: u128) -> Self {
        Self::trusted(
            "transfer",
            SignerRole::Coldkey,
            "Balances",
            "transfer_keep_alive",
            Value::record(vec![
                ("dest".into(), Value::str(dest)),
                ("value".into(), Value::Uint(amount_rao)),
            ]),
            Spend::Bounded(amount_rao),
            [],
            false,
        )
    }

    /// Fund the native mirror of an EVM address with a bounded TAO amount.
    pub fn fund_evm_key(mirror: impl Into<String>, amount_rao: u128) -> Self {
        Self::trusted(
            "fund_evm_key",
            SignerRole::Coldkey,
            "Balances",
            "transfer_keep_alive",
            Value::record(vec![
                ("dest".into(), Value::str(mirror)),
                ("value".into(), Value::Uint(amount_rao)),
            ]),
            Spend::Bounded(amount_rao),
            [],
            false,
        )
    }

    /// A bounded TAO transfer that may reap the sender.
    pub fn transfer_allow_death(dest: impl Into<String>, amount_rao: u128) -> Self {
        Self::trusted(
            "transfer",
            SignerRole::Coldkey,
            "Balances",
            "transfer_allow_death",
            Value::record(vec![
                ("dest".into(), Value::str(dest)),
                ("value".into(), Value::Uint(amount_rao)),
            ]),
            Spend::Bounded(amount_rao),
            [],
            false,
        )
    }

    /// Transfer the account's full transferable balance.
    pub fn transfer_all(dest: impl Into<String>, keep_alive: bool) -> Self {
        Self::trusted(
            "transfer_all",
            SignerRole::Coldkey,
            "Balances",
            "transfer_all",
            Value::record(vec![
                ("dest".into(), Value::str(dest)),
                ("keep_alive".into(), Value::Bool(keep_alive)),
            ]),
            Spend::Unbounded,
            [],
            false,
        )
    }

    /// Stake a bounded amount of TAO on one subnet.
    pub fn add_stake(hotkey: impl Into<String>, netuid: u16, amount_rao: u128) -> Self {
        Self::trusted(
            "add_stake",
            SignerRole::Coldkey,
            "SubtensorModule",
            "add_stake",
            Value::record(vec![
                ("hotkey".into(), Value::str(hotkey)),
                ("netuid".into(), Value::Uint(u128::from(netuid))),
                ("amount_staked".into(), Value::Uint(amount_rao)),
            ]),
            Spend::Bounded(amount_rao),
            [netuid],
            false,
        )
    }

    /// Stake a bounded amount of TAO with a limit price.
    pub fn add_stake_limit(
        hotkey: impl Into<String>,
        netuid: u16,
        amount_rao: u128,
        limit_price_rao: u128,
        allow_partial: bool,
    ) -> Self {
        Self::trusted(
            "add_stake_limit",
            SignerRole::Coldkey,
            "SubtensorModule",
            "add_stake_limit",
            Value::record(vec![
                ("hotkey".into(), Value::str(hotkey)),
                ("netuid".into(), Value::Uint(u128::from(netuid))),
                ("amount_staked".into(), Value::Uint(amount_rao)),
                ("limit_price".into(), Value::Uint(limit_price_rao)),
                ("allow_partial".into(), Value::Bool(allow_partial)),
            ]),
            Spend::Bounded(amount_rao),
            [netuid],
            false,
        )
    }

    /// Remove alpha stake from one subnet. This moves value back to the signer,
    /// so it is subnet-scoped but not an outgoing spend.
    pub fn remove_stake(hotkey: impl Into<String>, netuid: u16, amount_alpha_rao: u128) -> Self {
        Self::trusted(
            "remove_stake",
            SignerRole::Coldkey,
            "SubtensorModule",
            "remove_stake",
            Value::record(vec![
                ("hotkey".into(), Value::str(hotkey)),
                ("netuid".into(), Value::Uint(u128::from(netuid))),
                ("amount_unstaked".into(), Value::Uint(amount_alpha_rao)),
            ]),
            Spend::None,
            [netuid],
            false,
        )
    }

    /// Remove alpha stake from one subnet with a limit price.
    pub fn remove_stake_limit(
        hotkey: impl Into<String>,
        netuid: u16,
        amount_alpha_rao: u128,
        limit_price_rao: u128,
        allow_partial: bool,
    ) -> Self {
        Self::trusted(
            "remove_stake_limit",
            SignerRole::Coldkey,
            "SubtensorModule",
            "remove_stake_limit",
            Value::record(vec![
                ("hotkey".into(), Value::str(hotkey)),
                ("netuid".into(), Value::Uint(u128::from(netuid))),
                ("amount_unstaked".into(), Value::Uint(amount_alpha_rao)),
                ("limit_price".into(), Value::Uint(limit_price_rao)),
                ("allow_partial".into(), Value::Bool(allow_partial)),
            ]),
            Spend::None,
            [netuid],
            false,
        )
    }

    /// Register a subnet. The live registration cost is not known locally.
    pub fn register_subnet(hotkey: impl Into<String>) -> Self {
        Self::trusted(
            "register_subnet",
            SignerRole::Coldkey,
            "SubtensorModule",
            "register_network",
            Value::record(vec![("hotkey".into(), Value::str(hotkey))]),
            Spend::Unbounded,
            [],
            false,
        )
    }

    /// Activate one owned subnet.
    pub fn start_call(netuid: u16) -> Self {
        Self::trusted(
            "start_call",
            SignerRole::Coldkey,
            "SubtensorModule",
            "start_call",
            Value::record(vec![("netuid".into(), Value::Uint(u128::from(netuid)))]),
            Spend::None,
            [netuid],
            false,
        )
    }

    /// Register a hotkey by burning the subnet's live registration cost.
    pub fn burned_register(netuid: u16, hotkey: impl Into<String>) -> Self {
        Self::trusted(
            "burned_register",
            SignerRole::Coldkey,
            "SubtensorModule",
            "burned_register",
            Value::record(vec![
                ("netuid".into(), Value::Uint(u128::from(netuid))),
                ("hotkey".into(), Value::str(hotkey)),
            ]),
            Spend::Unbounded,
            [netuid],
            false,
        )
    }

    /// Register a hotkey on root.
    pub fn root_register(hotkey: impl Into<String>) -> Self {
        Self::trusted(
            "root_register",
            SignerRole::Coldkey,
            "SubtensorModule",
            "root_register",
            Value::record(vec![("hotkey".into(), Value::str(hotkey))]),
            Spend::None,
            [0],
            false,
        )
    }

    /// Move stake between hotkeys/subnets without transferring ownership.
    pub fn move_stake(
        origin_hotkey: impl Into<String>,
        origin_netuid: u16,
        destination_hotkey: impl Into<String>,
        destination_netuid: u16,
        amount_alpha_rao: u128,
    ) -> Self {
        Self::trusted(
            "move_stake",
            SignerRole::Coldkey,
            "SubtensorModule",
            "move_stake",
            Value::record(vec![
                ("origin_hotkey".into(), Value::str(origin_hotkey)),
                ("destination_hotkey".into(), Value::str(destination_hotkey)),
                (
                    "origin_netuid".into(),
                    Value::Uint(u128::from(origin_netuid)),
                ),
                (
                    "destination_netuid".into(),
                    Value::Uint(u128::from(destination_netuid)),
                ),
                ("alpha_amount".into(), Value::Uint(amount_alpha_rao)),
            ]),
            Spend::None,
            [origin_netuid, destination_netuid],
            false,
        )
    }

    /// Swap stake between two subnets on one hotkey.
    pub fn swap_stake(
        hotkey: impl Into<String>,
        origin_netuid: u16,
        destination_netuid: u16,
        amount_alpha_rao: u128,
    ) -> Self {
        Self::trusted(
            "swap_stake",
            SignerRole::Coldkey,
            "SubtensorModule",
            "swap_stake",
            Value::record(vec![
                ("hotkey".into(), Value::str(hotkey)),
                (
                    "origin_netuid".into(),
                    Value::Uint(u128::from(origin_netuid)),
                ),
                (
                    "destination_netuid".into(),
                    Value::Uint(u128::from(destination_netuid)),
                ),
                ("alpha_amount".into(), Value::Uint(amount_alpha_rao)),
            ]),
            Spend::None,
            [origin_netuid, destination_netuid],
            false,
        )
    }

    /// Transfer stake ownership to another coldkey. The alpha value is not
    /// cheaply TAO-bounded, so a spend cap must reject it.
    pub fn transfer_stake(
        destination_coldkey: impl Into<String>,
        hotkey: impl Into<String>,
        origin_netuid: u16,
        destination_netuid: u16,
        amount_alpha_rao: u128,
    ) -> Self {
        Self::trusted(
            "transfer_stake",
            SignerRole::Coldkey,
            "SubtensorModule",
            "transfer_stake",
            Value::record(vec![
                (
                    "destination_coldkey".into(),
                    Value::str(destination_coldkey),
                ),
                ("hotkey".into(), Value::str(hotkey)),
                (
                    "origin_netuid".into(),
                    Value::Uint(u128::from(origin_netuid)),
                ),
                (
                    "destination_netuid".into(),
                    Value::Uint(u128::from(destination_netuid)),
                ),
                ("alpha_amount".into(), Value::Uint(amount_alpha_rao)),
            ]),
            Spend::Unbounded,
            [origin_netuid, destination_netuid],
            false,
        )
    }

    /// Transfer stake ownership to another coldkey, landing it on a different
    /// hotkey. Like `transfer_stake`, the alpha value is not cheaply
    /// TAO-bounded, so a spend cap must reject it.
    pub fn transfer_stake_and_hotkey(
        destination_coldkey: impl Into<String>,
        origin_hotkey: impl Into<String>,
        destination_hotkey: impl Into<String>,
        origin_netuid: u16,
        destination_netuid: u16,
        amount_alpha_rao: u128,
    ) -> Self {
        Self::trusted(
            "transfer_stake_and_hotkey",
            SignerRole::Coldkey,
            "SubtensorModule",
            "transfer_stake_and_hotkey",
            Value::record(vec![
                (
                    "destination_coldkey".into(),
                    Value::str(destination_coldkey),
                ),
                ("origin_hotkey".into(), Value::str(origin_hotkey)),
                ("destination_hotkey".into(), Value::str(destination_hotkey)),
                (
                    "origin_netuid".into(),
                    Value::Uint(u128::from(origin_netuid)),
                ),
                (
                    "destination_netuid".into(),
                    Value::Uint(u128::from(destination_netuid)),
                ),
                ("alpha_amount".into(), Value::Uint(amount_alpha_rao)),
            ]),
            Spend::Unbounded,
            [origin_netuid, destination_netuid],
            false,
        )
    }

    /// Unstake from every subnet on one hotkey.
    pub fn unstake_all(hotkey: impl Into<String>) -> Self {
        Self::trusted(
            "unstake_all",
            SignerRole::Coldkey,
            "SubtensorModule",
            "unstake_all",
            Value::record(vec![("hotkey".into(), Value::str(hotkey))]),
            Spend::None,
            [],
            true,
        )
    }

    /// Move all alpha stake to root across every subnet on one hotkey.
    pub fn unstake_all_alpha(hotkey: impl Into<String>) -> Self {
        Self::trusted(
            "unstake_all_alpha",
            SignerRole::Coldkey,
            "SubtensorModule",
            "unstake_all_alpha",
            Value::record(vec![("hotkey".into(), Value::str(hotkey))]),
            Spend::None,
            [],
            true,
        )
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
        Ok(Self::trusted(
            "set_hyperparameter",
            SignerRole::Coldkey,
            "AdminUtils",
            function,
            Value::Tuple(vec![Value::Uint(u128::from(netuid)), value]),
            Spend::None,
            [netuid],
            false,
        ))
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
        Ok(Self::trusted(
            "set_root_claim_type",
            SignerRole::Coldkey,
            "SubtensorModule",
            "set_root_claim_type",
            Value::record(vec![("new_root_claim_type".into(), value)]),
            Spend::None,
            netuids,
            false,
        ))
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
        Ok(Self::trusted(
            "set_take",
            SignerRole::Coldkey,
            "SubtensorModule",
            function,
            Value::record(vec![
                ("hotkey".into(), Value::str(hotkey)),
                ("take".into(), Value::Uint(target)),
            ]),
            Spend::None,
            [],
            false,
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
        let mut raw = false;
        for child in &children {
            calls.push(Value::Bytes(child.encode(client)?));
            spend = aggregate_spend(spend, child.security.spend);
            netuids.extend(child.security.netuids.iter().copied());
            affects_all_subnets |= child.security.affects_all_subnets;
            raw |= child.security.raw;
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
            security: SecuritySemantics {
                spend,
                netuids: netuids.into_iter().collect(),
                affects_all_subnets,
                raw,
            },
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

fn strictest_spend(left: Spend, right: Spend) -> Spend {
    match (left, right) {
        (Spend::Unbounded, _) | (_, Spend::Unbounded) => Spend::Unbounded,
        (Spend::Bounded(left), Spend::Bounded(right)) => Spend::Bounded(left.max(right)),
        (Spend::Bounded(value), Spend::None) | (Spend::None, Spend::Bounded(value)) => {
            Spend::Bounded(value)
        }
        (Spend::None, Spend::None) => Spend::None,
    }
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
        if intent.security.raw && !self.allow_raw_calls {
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
            match intent.security.spend {
                Spend::Bounded(spend) if spend > max_spend => violations.push(format!(
                    "spend {spend} rao exceeds max_spend_rao {max_spend}"
                )),
                Spend::Unbounded => violations
                    .push("unbounded spend is not allowed while max_spend_rao is set".into()),
                _ => {}
            }
        }
        if let Some(allowed) = &self.allowed_netuids {
            if intent.security.affects_all_subnets {
                violations.push(
                    "intent affects every subnet but policy only allows an explicit subset".into(),
                );
            }
            for netuid in &intent.security.netuids {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arbitrary_call_fails_closed_for_every_policy_dimension() {
        let intent = IntentCall::new(
            "caller_supplied",
            SignerRole::Coldkey,
            "Balances",
            "transfer_keep_alive",
            Value::record(vec![
                ("dest".into(), Value::str("5caller-controlled")),
                ("value".into(), Value::Uint(1)),
            ]),
        );
        let policy = Policy {
            max_spend_rao: Some(u128::MAX),
            allowed_netuids: Some(BTreeSet::from([1])),
            allow_raw_calls: false,
            ..Policy::default()
        };

        let violations = policy.check(&intent, Some(0));
        assert!(violations
            .iter()
            .any(|violation| violation == "raw calls are disabled by policy"));
        assert!(violations
            .iter()
            .any(|violation| violation
                == "unbounded spend is not allowed while max_spend_rao is set"));
        assert!(violations.iter().any(|violation| violation
            == "intent affects every subnet but policy only allows an explicit subset"));
    }

    #[test]
    fn allowing_raw_does_not_disable_spend_or_subnet_guardrails() {
        let intent = IntentCall::raw_call(
            "caller_supplied",
            SignerRole::Coldkey,
            "Sudo",
            "sudo",
            Value::Null,
        );
        let policy = Policy {
            max_spend_rao: Some(u128::MAX),
            allowed_netuids: Some(BTreeSet::from([1])),
            allow_raw_calls: true,
            ..Policy::default()
        };

        assert_eq!(
            policy.check(&intent, Some(0)),
            vec![
                String::from("unbounded spend is not allowed while max_spend_rao is set"),
                String::from(
                    "intent affects every subnet but policy only allows an explicit subset",
                ),
            ]
        );
    }

    #[test]
    fn trusted_transfer_uses_constructor_derived_bounded_spend() {
        let intent = IntentCall::transfer("5trusted-destination", 10);
        let exact = Policy {
            max_spend_rao: Some(10),
            ..Policy::default()
        };
        assert!(exact.check(&intent, Some(0)).is_empty());

        let too_small = Policy {
            max_spend_rao: Some(9),
            ..Policy::default()
        };
        assert_eq!(
            too_small.check(&intent, Some(0)),
            vec![String::from("spend 10 rao exceeds max_spend_rao 9")]
        );
    }

    #[test]
    fn trusted_move_stake_uses_constructor_derived_netuids() {
        let intent = IntentCall::move_stake("5origin", 1, "5destination", 2, 10);
        let policy = Policy {
            allowed_netuids: Some(BTreeSet::from([1])),
            ..Policy::default()
        };
        assert_eq!(
            policy.check(&intent, Some(0)),
            vec![String::from("netuid 2 is not allowed by policy")]
        );
    }

    #[test]
    fn compatibility_builders_cannot_downgrade_arbitrary_calls() {
        let intent = IntentCall::new(
            "caller_supplied",
            SignerRole::Coldkey,
            "Balances",
            "transfer_keep_alive",
            Value::record(vec![("value".into(), Value::Uint(1))]),
        )
        .spend(Spend::None)
        .touches([]);
        let policy = Policy {
            max_spend_rao: Some(u128::MAX),
            allowed_netuids: Some(BTreeSet::from([1])),
            allow_raw_calls: true,
            ..Policy::default()
        };

        assert_eq!(
            policy.check(&intent, Some(0)),
            vec![
                String::from("unbounded spend is not allowed while max_spend_rao is set"),
                String::from(
                    "intent affects every subnet but policy only allows an explicit subset",
                ),
            ]
        );
    }

    #[test]
    fn compatibility_builders_cannot_lower_trusted_spend_or_scope() {
        let intent = IntentCall::transfer("5trusted-destination", 10)
            .spend(Spend::None)
            .touches([2]);
        let policy = Policy {
            max_spend_rao: Some(9),
            allowed_netuids: Some(BTreeSet::from([1])),
            ..Policy::default()
        };

        assert_eq!(
            policy.check(&intent, Some(0)),
            vec![
                String::from("spend 10 rao exceeds max_spend_rao 9"),
                String::from("netuid 2 is not allowed by policy"),
            ]
        );
    }
}
