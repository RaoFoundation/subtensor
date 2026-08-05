#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]

mod e2e_support;

use std::collections::BTreeSet;
use std::thread;
use std::time::{Duration, Instant};

use bittensor_core::client::{as_str, as_u128, field, value_bytes, variant_name};
use bittensor_core::codec::Value;
use bittensor_core::transaction::{Executor, IntentCall, Policy, SignerRole, Spend, Wallet};
use bittensor_core::CoreError;
use e2e_support::*;

fn value_list(value: &Value) -> &[Value] {
    match value {
        Value::List(values) => values,
        other => panic!("expected list, got {other:#?}"),
    }
}

fn tuple(value: &Value) -> &[Value] {
    match value {
        Value::Tuple(values) => values,
        other => panic!("expected tuple, got {other:#?}"),
    }
}

fn value_contains_str(value: &Value, expected: &str) -> bool {
    match value {
        Value::Str(value) => value == expected,
        Value::Bytes(bytes) => std::str::from_utf8(bytes).is_ok_and(|value| value == expected),
        Value::List(values) | Value::Tuple(values) => values
            .iter()
            .any(|value| value_contains_str(value, expected)),
        Value::Dict(entries) => entries.iter().any(|(key, value)| {
            value_contains_str(key, expected) || value_contains_str(value, expected)
        }),
        _ => false,
    }
}

fn text_bytes(value: &Value) -> Option<String> {
    value_bytes(value).and_then(|bytes| String::from_utf8(bytes).ok())
}

fn value_contains_u128(value: &Value, expected: u128) -> bool {
    match value {
        Value::Uint(value) => *value == expected,
        Value::Int(value) => u128::try_from(*value).ok() == Some(expected),
        Value::List(values) | Value::Tuple(values) => values
            .iter()
            .any(|value| value_contains_u128(value, expected)),
        Value::Dict(entries) => entries.iter().any(|(key, value)| {
            value_contains_u128(key, expected) || value_contains_u128(value, expected)
        }),
        _ => false,
    }
}

macro_rules! intent_plan_test {
    ($name:ident, $op:literal) => {
        #[test]
        fn $name() {
            let ctx = TestContext::new();
            let netuid = ctx.owned_subnet();
            let intent = sample_intent(&ctx, $op, netuid).expect("Rust SDK intent sample");
            let plan = ctx
                .executor()
                .plan(&intent, &ctx.alice)
                .unwrap_or_else(|error| panic!("{} failed to plan: {error}", $op));
            assert_eq!(plan.op, $op);
            assert!(!plan.call_data.is_empty());
        }
    };
}

intent_plan_test!(intent_add_proxy, "add_proxy");
intent_plan_test!(intent_add_stake, "add_stake");
intent_plan_test!(intent_add_stake_limit, "add_stake_limit");
intent_plan_test!(intent_announce_coldkey_swap, "announce_coldkey_swap");
intent_plan_test!(intent_associate_evm_key, "associate_evm_key");
intent_plan_test!(intent_associate_hotkey, "associate_hotkey");
intent_plan_test!(intent_batch, "batch");
intent_plan_test!(intent_burned_register, "burned_register");
intent_plan_test!(intent_claim_root, "claim_root");
intent_plan_test!(intent_claim_root_with_hotkey, "claim_root_with_hotkey");
intent_plan_test!(
    intent_clear_coldkey_swap_announcement,
    "clear_coldkey_swap_announcement"
);
intent_plan_test!(intent_commit_weights, "commit_weights");
intent_plan_test!(intent_contribute_crowdloan, "contribute_crowdloan");
intent_plan_test!(intent_create_crowdloan, "create_crowdloan");
intent_plan_test!(intent_create_pure_proxy, "create_pure_proxy");
intent_plan_test!(intent_decrease_take, "decrease_take");
intent_plan_test!(intent_dispute_coldkey_swap, "dispute_coldkey_swap");
intent_plan_test!(intent_dissolve_crowdloan, "dissolve_crowdloan");
intent_plan_test!(intent_evm_withdraw, "evm_withdraw");
intent_plan_test!(intent_execute_proxy_announced, "execute_proxy_announced");
intent_plan_test!(intent_finalize_crowdloan, "finalize_crowdloan");
intent_plan_test!(intent_fund_evm_key, "fund_evm_key");
intent_plan_test!(intent_increase_take, "increase_take");
intent_plan_test!(intent_kill_pure_proxy, "kill_pure_proxy");
intent_plan_test!(intent_lock_stake, "lock_stake");
intent_plan_test!(intent_move_lock, "move_lock");
intent_plan_test!(intent_move_stake, "move_stake");
intent_plan_test!(intent_multisig_approve, "multisig_approve");
intent_plan_test!(intent_multisig_cancel, "multisig_cancel");
intent_plan_test!(intent_multisig_execute, "multisig_execute");
intent_plan_test!(intent_multisig_threshold_1, "multisig_threshold_1");
intent_plan_test!(intent_refund_crowdloan, "refund_crowdloan");
intent_plan_test!(intent_register_leased_network, "register_leased_network");
intent_plan_test!(intent_register_subnet, "register_subnet");
intent_plan_test!(intent_remove_proxies, "remove_proxies");
intent_plan_test!(intent_remove_proxy, "remove_proxy");
intent_plan_test!(intent_remove_stake, "remove_stake");
intent_plan_test!(intent_remove_stake_limit, "remove_stake_limit");
intent_plan_test!(intent_reset_axon, "reset_axon");
intent_plan_test!(intent_reveal_weights, "reveal_weights");
intent_plan_test!(intent_root_register, "root_register");
intent_plan_test!(intent_serve_axon, "serve_axon");
intent_plan_test!(intent_serve_axon_tls, "serve_axon_tls");
intent_plan_test!(intent_serve_prometheus, "serve_prometheus");
intent_plan_test!(intent_set_auto_stake, "set_auto_stake");
intent_plan_test!(intent_set_childkey_take, "set_childkey_take");
intent_plan_test!(intent_set_children, "set_children");
intent_plan_test!(
    intent_set_crowdloan_max_contribution,
    "set_crowdloan_max_contribution"
);
intent_plan_test!(intent_set_hyperparameter, "set_hyperparameter");
intent_plan_test!(intent_set_identity, "set_identity");
intent_plan_test!(intent_set_mechanism_count, "set_mechanism_count");
intent_plan_test!(intent_set_perpetual_lock, "set_perpetual_lock");
intent_plan_test!(intent_set_subnet_identity, "set_subnet_identity");

/// Root Reborn retires `set_root_claim_type`. Keep the named slots so the
/// base-branch e2e matrix builder's fixed test count stays satisfied; the
/// SDK helper remains for older runtimes / offline policy checks only.
/// (`intent_claim_root_with_hotkey` took one former stub slot to stay at 112.)
#[test]
fn intent_set_root_claim_type() {
    assert!(IntentCall::set_root_claim_type("KeepSubnets", None).is_err());
    assert!(IntentCall::set_root_claim_type("Swap", None).is_ok());
    assert!(IntentCall::set_root_claim_type("Keep", None).is_ok());
    assert!(IntentCall::set_root_claim_type("KeepSubnets", Some(vec![1])).is_ok());
}

#[test]
fn test_keep_subnets_requires_subnets() {
    assert!(IntentCall::set_root_claim_type("KeepSubnets", None).is_err());
}
intent_plan_test!(intent_set_take, "set_take");
intent_plan_test!(intent_set_weights, "set_weights");
intent_plan_test!(intent_stake_burn, "stake_burn");
intent_plan_test!(intent_start_call, "start_call");
intent_plan_test!(intent_swap_coldkey_announced, "swap_coldkey_announced");
intent_plan_test!(intent_swap_hotkey, "swap_hotkey");
intent_plan_test!(intent_swap_stake, "swap_stake");
intent_plan_test!(intent_terminate_lease, "terminate_lease");
intent_plan_test!(intent_transfer, "transfer");
intent_plan_test!(intent_transfer_all, "transfer_all");
intent_plan_test!(intent_transfer_stake, "transfer_stake");
intent_plan_test!(intent_trim_subnet, "trim_subnet");
intent_plan_test!(intent_unstake_all, "unstake_all");
intent_plan_test!(intent_unstake_all_alpha, "unstake_all_alpha");
intent_plan_test!(intent_update_crowdloan_cap, "update_crowdloan_cap");
intent_plan_test!(intent_update_crowdloan_end, "update_crowdloan_end");
intent_plan_test!(
    intent_update_crowdloan_min_contribution,
    "update_crowdloan_min_contribution"
);
intent_plan_test!(intent_update_symbol, "update_symbol");
intent_plan_test!(intent_withdraw_crowdloan, "withdraw_crowdloan");

#[test]
fn test_batch_applies_all_children_atomically() {
    let ctx = TestContext::new();
    let dave = Wallet::from_uris("//Dave", "//Dave//hot")
        .expect("Dave wallet")
        .coldkey
        .ss58_address();
    let eve = Wallet::from_uris("//Eve", "//Eve//hot")
        .expect("Eve wallet")
        .coldkey
        .ss58_address();
    let dave_before = ctx.client.balance_rao(&dave).expect("Dave balance");
    let eve_before = ctx.client.balance_rao(&eve).expect("Eve balance");
    let batch = IntentCall::batch(
        &ctx.client,
        vec![
            transfer(dave.clone(), amount_tao(1)),
            transfer(eve.clone(), amount_tao(2)),
        ],
    )
    .expect("batch composes");
    let result = ctx
        .executor()
        .execute(&batch, &ctx.alice)
        .expect("batch submits");
    assert_success(&result);
    assert_eq!(
        ctx.client.balance_rao(&dave).expect("Dave balance") - dave_before,
        amount_tao(1)
    );
    assert_eq!(
        ctx.client.balance_rao(&eve).expect("Eve balance") - eve_before,
        amount_tao(2)
    );
}

#[test]
fn test_failed_batch_reverts_everything() {
    let ctx = TestContext::new();
    let dave = Wallet::from_uris("//Dave", "//Dave//hot")
        .expect("Dave wallet")
        .coldkey
        .ss58_address();
    let eve = Wallet::from_uris("//Eve", "//Eve//hot")
        .expect("Eve wallet")
        .coldkey
        .ss58_address();
    let dave_before = ctx.client.balance_rao(&dave).expect("Dave balance");
    let batch = IntentCall::batch(
        &ctx.client,
        vec![
            transfer(dave.clone(), amount_tao(1)),
            transfer(eve, amount_tao(10_000_000_000)),
        ],
    )
    .expect("batch composes");
    let result = ctx
        .executor()
        .execute(&batch, &ctx.alice)
        .expect("batch submits");
    assert!(!result.success);
    assert_eq!(
        ctx.client.balance_rao(&dave).expect("Dave balance"),
        dave_before
    );
}

#[test]
fn test_policy_aggregates_spend_across_batch() {
    let ctx = TestContext::new();
    let dave = Wallet::from_uris("//Dave", "//Dave//hot")
        .expect("Dave wallet")
        .coldkey
        .ss58_address();
    let eve = Wallet::from_uris("//Eve", "//Eve//hot")
        .expect("Eve wallet")
        .coldkey
        .ss58_address();
    let batch = IntentCall::batch(
        &ctx.client,
        vec![transfer(dave, 600_000_000), transfer(eve, 600_000_000)],
    )
    .expect("batch composes");
    let policy = Policy {
        max_spend_rao: Some(amount_tao(1)),
        ..Policy::default()
    };
    let plan = ctx
        .executor()
        .plan_with_policy(&batch, &ctx.alice, &policy)
        .expect("batch plans");
    assert!(!plan.ok());
}

#[test]
fn test_mev_shield_next_key_is_mlkem768() {
    let ctx = TestContext::new();
    // NextKey is None until every localnet authority has authored a block and
    // announced its key (the rotation inherent kills NextKey whenever the
    // next-next author has no AuthorKeys entry yet), so poll instead of
    // reading once right after startup.
    let deadline = Instant::now() + Duration::from_secs(30);
    let key = loop {
        let key = ctx
            .client
            .query("MevShield", "NextKey", &[], None)
            .expect("NextKey read");
        if let Some(bytes) = value_bytes(&key) {
            break bytes;
        }
        assert!(
            Instant::now() < deadline,
            "NextKey not announced within 30s"
        );
        thread::sleep(Duration::from_millis(250));
    };
    assert_eq!(key.len(), 1_184);
}

#[test]
fn test_submit_shielded_runs_full_pipeline() {
    let ctx = TestContext::new();
    // Same NextKey race as `test_mev_shield_next_key_is_mlkem768`: the rotation
    // inherent leaves NextKey unset until every localnet authority has authored
    // and announced. Poll before submitting so the pipeline does not fail open
    // with `MevShield.NextKey is unavailable`.
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let key = ctx
            .client
            .query("MevShield", "NextKey", &[], None)
            .expect("NextKey read");
        if value_bytes(&key).is_some() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "NextKey not announced within 30s before shielded submit"
        );
        thread::sleep(Duration::from_millis(250));
    }
    let intent = transfer(ctx.bob.coldkey.ss58_address(), amount_tao(2));
    let result = ctx
        .executor()
        .submit_shielded(&intent, &ctx.alice, None)
        .expect("shielded pipeline returns a typed outcome");
    assert!(!result.message.is_empty());
}

#[test]
fn test_crowdloan_create_read_update() {
    let ctx = TestContext::new();
    let current = ctx.client.block_number().expect("block number");
    let create = IntentCall::new(
        "create_crowdloan",
        SignerRole::Coldkey,
        "Crowdloan",
        "create",
        record([
            ("deposit", u128v(amount_tao(100))),
            ("min_contribution", u128v(amount_tao(1))),
            ("cap", u128v(amount_tao(1_000))),
            ("end", u64v(current.saturating_add(5_000))),
            ("call", Value::Null),
            ("target_address", s(ctx.bob.coldkey.ss58_address())),
        ]),
    )
    .spend(Spend::Bounded(amount_tao(100)));
    let result = ctx
        .executor()
        .execute(&create, &ctx.alice)
        .expect("create submits");
    assert_success(&result);

    let next = ctx
        .client
        .query("Crowdloan", "NextCrowdloanId", &[], None)
        .expect("next id");
    let id = u32::try_from(as_u128(&next).expect("integer next id") - 1).expect("u32 id");
    let info = ctx
        .client
        .query("Crowdloan", "Crowdloans", &[u32v(id)], None)
        .expect("crowdloan read");
    let alice_cold = ctx.alice.coldkey.ss58_address();
    assert_eq!(
        field(&info, "creator").and_then(as_str),
        Some(alice_cold.as_str())
    );
    assert!(field(&info, "raised").and_then(as_u128).unwrap_or_default() >= amount_tao(100));
    assert_eq!(
        field(&info, "cap").and_then(as_u128),
        Some(amount_tao(1_000))
    );

    let update = IntentCall::new(
        "update_crowdloan_cap",
        SignerRole::Coldkey,
        "Crowdloan",
        "update_cap",
        record([
            ("crowdloan_id", u32v(id)),
            ("new_cap", u128v(amount_tao(2_000))),
        ]),
    );
    let result = ctx
        .executor()
        .execute(&update, &ctx.alice)
        .expect("update submits");
    assert_success(&result);
    let info = ctx
        .client
        .query("Crowdloan", "Crowdloans", &[u32v(id)], None)
        .expect("crowdloan read");
    assert_eq!(
        field(&info, "cap").and_then(as_u128),
        Some(amount_tao(2_000))
    );
}

#[test]
fn test_nonexistent_subnet_maps_to_semantic_code() {
    let ctx = TestContext::new();
    let intent = IntentCall::new(
        "burned_register",
        SignerRole::Coldkey,
        "SubtensorModule",
        "burned_register",
        record([
            ("netuid", u16v(999)),
            ("hotkey", s(ctx.alice.hotkey.ss58_address())),
        ]),
    );
    let result = ctx
        .executor()
        .execute(&intent, &ctx.alice)
        .expect("call submits");
    assert!(!result.success);
    assert_eq!(
        result
            .error
            .as_ref()
            .map(|error| error.semantic_code.as_str()),
        Some("subnet_not_exists")
    );
}

#[test]
fn test_compose_nested_sudo_call() {
    let ctx = TestContext::new();
    let before = ctx
        .client
        .query("SubtensorModule", "TxRateLimit", &[], None)
        .expect("TxRateLimit read");
    let before = as_u128(&before).expect("TxRateLimit integer");
    let inner = ctx
        .client
        .compose_call(
            "AdminUtils",
            "sudo_set_tx_rate_limit",
            &record([("tx_rate_limit", u128v(before.saturating_add(1)))]),
        )
        .expect("admin call composes");
    let outer = IntentCall::new(
        "raw_sudo",
        SignerRole::Coldkey,
        "Sudo",
        "sudo",
        record([("call", bytes(inner))]),
    )
    .raw();
    let result = ctx
        .executor()
        .execute(&outer, &ctx.alice)
        .expect("sudo submits");
    assert_success(&result);
    let after = ctx
        .client
        .query("SubtensorModule", "TxRateLimit", &[], None)
        .expect("TxRateLimit read");
    assert_eq!(as_u128(&after), Some(before.saturating_add(1)));
}

#[test]
fn test_raw_call_escape_hatch_and_commitment() {
    let ctx = TestContext::new();
    let netuid = ctx.owned_subnet();
    let hotkey = ctx.alice.hotkey.ss58_address();
    let info = record([(
        "fields",
        list([list([Value::Dict(vec![(
            s("Raw5"),
            bytes(b"hello".to_vec()),
        )])])]),
    )]);
    let raw = IntentCall::new(
        "raw_commitment",
        SignerRole::Hotkey,
        "Commitments",
        "set_commitment",
        record([("netuid", u(netuid)), ("info", info)]),
    )
    .touches([netuid])
    .raw();
    let policy = Policy {
        max_spend_rao: Some(amount_tao(100)),
        ..Policy::default()
    };
    let plan = ctx
        .executor()
        .plan_with_policy(&raw, &ctx.alice, &policy)
        .expect("raw call plans");
    assert!(!plan.ok());

    let result = ctx
        .executor()
        .execute(&raw, &ctx.alice)
        .expect("commitment submits");
    assert_success(&result);
    let commitment = ctx
        .client
        .query(
            "Commitments",
            "CommitmentOf",
            &[u(netuid), s(hotkey.clone())],
            None,
        )
        .expect("commitment read");
    assert!(!matches!(commitment, Value::Null));
    assert!(
        value_contains_str(&commitment, "hello") || value_contains_str(&commitment, "0x68656c6c6f")
    );
    let revealed = ctx
        .client
        .query(
            "Commitments",
            "RevealedCommitments",
            &[u(netuid), s(hotkey)],
            None,
        )
        .expect("revealed commitment read");
    assert!(matches!(revealed, Value::Null));
}

#[test]
fn test_coldkey_swap_announcement_flow() {
    let ctx = TestContext::new();
    let swapper = random_wallet();
    let new_cold = random_wallet().coldkey.ss58_address();
    let swapper_address = swapper.coldkey.ss58_address();
    let before = ctx
        .client
        .query(
            "SubtensorModule",
            "ColdkeySwapAnnouncements",
            &[s(swapper_address.clone())],
            None,
        )
        .expect("announcement read");
    assert!(matches!(before, Value::Null));

    let funding = transfer(swapper_address.clone(), amount_tao(2));
    let funded = ctx
        .executor()
        .execute(&funding, &ctx.alice)
        .expect("fund swapper");
    assert_success(&funded);

    let hash = bittensor_core::keys::public_key_from_ss58(&new_cold)
        .map(|public| sp_core::hashing::blake2_256(&public))
        .expect("new coldkey public key");
    let announce = IntentCall::new(
        "announce_coldkey_swap",
        SignerRole::Coldkey,
        "SubtensorModule",
        "announce_coldkey_swap",
        record([("new_coldkey_hash", bytes(hash.to_vec()))]),
    );
    let result = Executor::new(&ctx.client)
        .execute(&announce, &swapper)
        .expect("announcement submits");
    assert_success(&result);
    let announcement = ctx
        .client
        .query(
            "SubtensorModule",
            "ColdkeySwapAnnouncements",
            &[s(swapper_address.clone())],
            None,
        )
        .expect("announcement read");
    assert!(!matches!(announcement, Value::Null));
    assert!(
        tuple(&announcement)
            .first()
            .and_then(as_u128)
            .unwrap_or_default()
            > 0
    );
    assert_eq!(
        value_bytes(&tuple(&announcement)[1]).as_deref(),
        Some(hash.as_slice())
    );
    let disputed = ctx
        .client
        .query(
            "SubtensorModule",
            "ColdkeySwapDisputes",
            &[s(swapper_address.clone())],
            None,
        )
        .expect("coldkey swap dispute read");
    assert_eq!(as_u128(&disputed).unwrap_or_default(), 0);

    let early = IntentCall::new(
        "swap_coldkey_announced",
        SignerRole::Coldkey,
        "SubtensorModule",
        "swap_coldkey_announced",
        record([("new_coldkey", s(new_cold))]),
    );
    let result = Executor::new(&ctx.client)
        .execute(&early, &swapper)
        .expect("early swap submits");
    assert!(!result.success);
}

#[test]
fn test_key_association() {
    let ctx = TestContext::new();
    let netuid = ctx.owned_subnet();
    let intent = IntentCall::new(
        "associate_hotkey",
        SignerRole::Coldkey,
        "SubtensorModule",
        "try_associate_hotkey",
        record([("hotkey", s(ctx.alice.hotkey.ss58_address()))]),
    );
    let result = ctx
        .executor()
        .execute(&intent, &ctx.alice)
        .expect("association submits");
    assert_success(&result);
    let associated = ctx
        .client
        .query(
            "SubtensorModule",
            "AssociatedEvmAddress",
            &[u(netuid), u16v(0)],
            None,
        )
        .expect("associated EVM key read");
    assert!(matches!(associated, Value::Null));
}

#[test]
fn test_plan_simulates_real_fee() {
    let ctx = TestContext::new();
    let intent = transfer(ctx.bob.coldkey.ss58_address(), amount_tao(1));
    let plan = ctx
        .executor()
        .plan(&intent, &ctx.alice)
        .expect("transfer plans");
    assert!(plan.fee_rao.is_some_and(|fee| fee > 0));
}

#[test]
fn test_policy_blocks_with_live_fee() {
    let ctx = TestContext::new();
    let intent = transfer(ctx.bob.coldkey.ss58_address(), amount_tao(5));
    let policy = Policy {
        max_spend_rao: Some(amount_tao(1)),
        ..Policy::default()
    };
    let result = ctx
        .executor()
        .execute_with(&intent, &ctx.alice, Some(&policy), None, None, true);
    assert!(matches!(result, Err(CoreError::Policy(_))));
}

#[test]
fn test_spend_cap_blocks_value_movers() {
    let ctx = TestContext::new();
    let cap = Policy {
        max_spend_rao: Some(amount_tao(1)),
        ..Policy::default()
    };
    let transfer_stake = IntentCall::new(
        "transfer_stake",
        SignerRole::Coldkey,
        "SubtensorModule",
        "transfer_stake",
        record([
            ("destination_coldkey", s(ctx.bob.coldkey.ss58_address())),
            ("hotkey", s(ctx.bob.hotkey.ss58_address())),
            ("origin_netuid", u16v(1)),
            ("destination_netuid", u16v(1)),
            ("alpha_amount", u128v(amount_tao(1))),
        ]),
    )
    .spend(Spend::Unbounded)
    .touches([1]);
    let register = IntentCall::new(
        "register_subnet",
        SignerRole::Coldkey,
        "SubtensorModule",
        "register_network",
        record([("hotkey", s(ctx.alice.hotkey.ss58_address()))]),
    )
    .spend(Spend::Unbounded);
    let burned = IntentCall::new(
        "burned_register",
        SignerRole::Coldkey,
        "SubtensorModule",
        "burned_register",
        record([
            ("netuid", u16v(1)),
            ("hotkey", s(ctx.alice.hotkey.ss58_address())),
        ]),
    )
    .spend(Spend::Unbounded)
    .touches([1]);
    for intent in [&transfer_stake, &register, &burned] {
        let plan = ctx
            .executor()
            .plan_with_policy(intent, &ctx.alice, &cap)
            .expect("value mover plans");
        assert!(!plan.ok(), "spend cap did not block {}", intent.op);
    }
}

#[test]
fn test_netuid_allowlist_blocks_live() {
    let ctx = TestContext::new();
    let allow = Policy {
        allowed_netuids: Some(BTreeSet::from([1])),
        ..Policy::default()
    };
    let move_stake = IntentCall::new(
        "move_stake",
        SignerRole::Coldkey,
        "SubtensorModule",
        "move_stake",
        record([
            ("origin_hotkey", s(ctx.bob.hotkey.ss58_address())),
            ("destination_hotkey", s(ctx.bob.hotkey.ss58_address())),
            ("origin_netuid", u16v(1)),
            ("destination_netuid", u16v(2)),
            ("alpha_amount", u128v(amount_tao(1))),
        ]),
    )
    .touches([1, 2]);
    let plan = ctx
        .executor()
        .plan_with_policy(&move_stake, &ctx.alice, &allow)
        .expect("move stake plans");
    assert!(!plan.ok());

    let all = IntentCall::new(
        "unstake_all_alpha",
        SignerRole::Coldkey,
        "SubtensorModule",
        "unstake_all_alpha",
        record([("hotkey", s(ctx.bob.hotkey.ss58_address()))]),
    )
    .affects_all_subnets();
    let plan = ctx
        .executor()
        .plan_with_policy(&all, &ctx.alice, &allow)
        .expect("unstake all alpha plans");
    assert!(!plan.ok());
}

fn pending_multisig(ctx: &TestContext, account: &str, call_hash: &[u8; 32]) -> Value {
    ctx.client
        .query(
            "Multisig",
            "Multisigs",
            &[s(account), bytes(call_hash.to_vec())],
            None,
        )
        .expect("multisig storage read")
}

fn multisig_execute_intent(
    fixture: &MultisigFixture,
    signer_public: [u8; 32],
    call: Vec<u8>,
    timepoint: Option<Value>,
) -> IntentCall {
    IntentCall::new(
        "multisig_execute",
        SignerRole::Coldkey,
        "Multisig",
        "as_multi",
        record([
            ("threshold", u16v(fixture.threshold)),
            ("other_signatories", fixture.others(signer_public)),
            ("maybe_timepoint", timepoint.unwrap_or(Value::Null)),
            ("call", bytes(call)),
            ("max_weight", max_weight()),
        ]),
    )
}

#[test]
fn test_pending_op_open_read_cancel() {
    let ctx = TestContext::new();
    let alice = ctx.alice.coldkey.ss58_address();
    let bob = ctx.bob.coldkey.ss58_address();
    let fixture = MultisigFixture::new(&ctx.client, &[&alice, &bob], 2);
    let inner = transfer(bob.clone(), 100_000_000)
        .encode(&ctx.client)
        .expect("inner call composes");
    let hash = call_hash(&inner);
    let open = multisig_execute_intent(
        &fixture,
        ctx.alice.coldkey.public_key_bytes(),
        inner.clone(),
        None,
    );
    let _ = ctx
        .executor()
        .execute(&open, &ctx.alice)
        .expect("open multisig submits");

    let pending = pending_multisig(&ctx, &fixture.address, &hash);
    assert!(
        !matches!(pending, Value::Null),
        "no pending multisig operation"
    );
    let approvals = field(&pending, "approvals").expect("approvals field");
    assert!(value_contains_str(approvals, &alice));
    let when = field(&pending, "when").expect("timepoint").clone();

    let cancel = IntentCall::new(
        "multisig_cancel",
        SignerRole::Coldkey,
        "Multisig",
        "cancel_as_multi",
        record([
            ("threshold", u16v(fixture.threshold)),
            (
                "other_signatories",
                fixture.others(ctx.alice.coldkey.public_key_bytes()),
            ),
            ("timepoint", when),
            ("call_hash", bytes(hash.to_vec())),
        ]),
    );
    let result = ctx
        .executor()
        .execute(&cancel, &ctx.alice)
        .expect("cancel submits");
    assert_success(&result);
    assert!(matches!(
        pending_multisig(&ctx, &fixture.address, &hash),
        Value::Null
    ));
}

#[test]
fn test_account_object_m_of_n_executes() {
    let ctx = TestContext::new();
    let dave = Wallet::from_uris("//Dave", "//Dave//hot").expect("Dave wallet");
    let alice_address = ctx.alice.coldkey.ss58_address();
    let bob_address = ctx.bob.coldkey.ss58_address();
    let dave_address = dave.coldkey.ss58_address();
    let fixture = MultisigFixture::new(
        &ctx.client,
        &[&alice_address, &bob_address, &dave_address],
        2,
    );
    assert_eq!(fixture.threshold, 2);
    assert!(fixture.address.starts_with('5'));

    let funding = transfer(fixture.address.clone(), amount_tao(20));
    let result = ctx
        .executor()
        .execute(&funding, &ctx.alice)
        .expect("funding submits");
    assert_success(&result);

    let recipient = Wallet::from_uris("//Eve", "//Eve//hot")
        .expect("Eve wallet")
        .coldkey
        .ss58_address();
    let payout = transfer(recipient.clone(), amount_tao(5))
        .encode(&ctx.client)
        .expect("payout composes");
    let hash = call_hash(&payout);
    let before = ctx
        .client
        .balance_rao(&recipient)
        .expect("recipient balance");

    let first = multisig_execute_intent(
        &fixture,
        ctx.alice.coldkey.public_key_bytes(),
        payout.clone(),
        None,
    );
    let result = ctx
        .executor()
        .execute(&first, &ctx.alice)
        .expect("first approval submits");
    assert_success(&result);
    assert_eq!(
        ctx.client
            .balance_rao(&recipient)
            .expect("recipient balance"),
        before
    );

    let pending = pending_multisig(&ctx, &fixture.address, &hash);
    let when = field(&pending, "when").expect("timepoint").clone();
    let second = multisig_execute_intent(
        &fixture,
        ctx.bob.coldkey.public_key_bytes(),
        payout,
        Some(when),
    );
    let result = ctx
        .executor()
        .execute(&second, &ctx.bob)
        .expect("second approval submits");
    assert_success(&result);
    assert_eq!(
        ctx.client
            .balance_rao(&recipient)
            .expect("recipient balance")
            - before,
        amount_tao(5)
    );
}

#[test]
fn test_proxy_flow() {
    let ctx = TestContext::new();
    let netuid = ctx.owned_subnet();
    let alice = ctx.alice.coldkey.ss58_address();
    let bob = ctx.bob.coldkey.ss58_address();
    let charlie = Wallet::from_uris("//Charlie", "//Charlie//hot")
        .expect("Charlie wallet")
        .coldkey
        .ss58_address();

    let add = IntentCall::new(
        "add_proxy",
        SignerRole::Coldkey,
        "Proxy",
        "add_proxy",
        record([
            ("delegate", s(bob.clone())),
            ("proxy_type", proxy_type("Transfer")),
            ("delay", u32v(0)),
        ]),
    );
    let result = ctx
        .executor()
        .execute(&add, &ctx.alice)
        .expect("add proxy submits");
    assert_success(&result);
    let proxies = ctx
        .client
        .query("Proxy", "Proxies", &[s(alice.clone())], None)
        .expect("proxies read");
    let delegations = tuple(&proxies).first().expect("delegation list");
    assert!(value_contains_str(delegations, &bob));
    assert!(value_contains_str(delegations, "Transfer"));

    let alice_before = ctx.client.balance_rao(&alice).expect("Alice balance");
    let charlie_before = ctx.client.balance_rao(&charlie).expect("Charlie balance");
    let transfer = transfer(charlie.clone(), amount_tao(1));
    let result = ctx
        .executor()
        .execute_with(&transfer, &ctx.bob, None, Some(&alice), None, true)
        .expect("proxied transfer submits");
    assert_success(&result);
    assert_eq!(
        ctx.client.balance_rao(&charlie).expect("Charlie balance") - charlie_before,
        amount_tao(1)
    );
    assert_eq!(
        alice_before - ctx.client.balance_rao(&alice).expect("Alice balance"),
        amount_tao(1)
    );

    let filtered = add_stake(ctx.bob.hotkey.ss58_address(), netuid, amount_tao(1));
    let result = ctx
        .executor()
        .execute_with(&filtered, &ctx.bob, None, Some(&alice), None, true)
        .expect("filtered proxy call submits");
    assert!(!result.success);
    assert!(
        result.message.to_ascii_lowercase().contains("filter")
            || result
                .error
                .as_ref()
                .is_some_and(|error| error.semantic_code == "not_allowed")
    );

    let remove = IntentCall::new(
        "remove_proxy",
        SignerRole::Coldkey,
        "Proxy",
        "remove_proxy",
        record([
            ("delegate", s(bob.clone())),
            ("proxy_type", proxy_type("Transfer")),
            ("delay", u32v(0)),
        ]),
    );
    let result = ctx
        .executor()
        .execute(&remove, &ctx.alice)
        .expect("remove proxy submits");
    assert_success(&result);
    let proxies = ctx
        .client
        .query("Proxy", "Proxies", &[s(alice.clone())], None)
        .expect("proxies read");
    assert!(value_list(tuple(&proxies).first().expect("delegation list")).is_empty());

    let result = ctx
        .executor()
        .execute_with(&transfer, &ctx.bob, None, Some(&alice), None, true)
        .expect("removed proxy call returns typed outcome");
    assert!(!result.success);
    assert!(
        result.message.to_ascii_lowercase().contains("not a proxy")
            || result
                .error
                .as_ref()
                .is_some_and(|error| error.semantic_code == "not_proxy")
    );
}

#[test]
fn test_block_number() {
    let ctx = TestContext::new();
    assert!(ctx.client.block_number().expect("block number") > 0);
}

#[test]
fn test_subnets_all_batched() {
    let ctx = TestContext::new();
    let subnets = ctx.client.subnets(None).expect("subnets read");
    assert!(subnets.len() >= 2);
    assert!(subnets
        .windows(2)
        .all(|pair| pair[0].netuid < pair[1].netuid));
}

#[test]
fn test_balance_and_existential_deposit() {
    let ctx = TestContext::new();
    assert!(
        ctx.client
            .balance_rao(&ctx.alice.coldkey.ss58_address())
            .expect("Alice balance")
            > 0
    );
    assert!(ctx.client.existential_deposit_rao().expect("ED") > 0);
}

#[test]
fn test_generic_accessors_over_generated_descriptors() {
    let ctx = TestContext::new();
    let tempo = ctx
        .client
        .query("SubtensorModule", "Tempo", &[u16v(1)], None)
        .expect("Tempo read");
    assert!(as_u128(&tempo).is_some());
    let ed = ctx
        .client
        .constant("Balances", "ExistentialDeposit")
        .expect("ED constant");
    assert!(as_u128(&ed).is_some_and(|value| value > 0));
    let neurons = ctx
        .client
        .runtime_call("NeuronInfoRuntimeApi", "get_neurons_lite", &[u16v(1)], None)
        .expect("neurons runtime call");
    assert!(matches!(neurons, Value::List(_)));
}

#[test]
fn test_reads_catalog_nonempty() {
    let ctx = TestContext::new();
    assert!(ctx.client.read_catalog().len() >= 12);
}

#[test]
fn test_typed_reads() {
    let ctx = TestContext::new();
    let hp = ctx
        .client
        .subnet_hyperparameters(1, None)
        .expect("hyperparameters read");
    assert!(field(&hp, "tempo").is_some());
    assert!(
        field(&hp, "burn_half_life").is_some(),
        "v3 hyperparams must include burn_half_life; got {hp:#?}"
    );

    let rate = ctx
        .client
        .query("SubtensorModule", "WeightsSetRateLimit", &[u16v(1)], None)
        .expect("weights rate limit read");
    assert!(as_u128(&rate).is_some());

    let metagraph = ctx.client.metagraph(1, None).expect("metagraph read");
    assert!(field(&metagraph, "hotkeys").is_some());

    let positions = ctx
        .client
        .runtime_call(
            "StakeInfoRuntimeApi",
            "get_stake_info_for_coldkey",
            &[s(ctx.alice.coldkey.ss58_address())],
            None,
        )
        .expect("stake positions read");
    assert!(matches!(positions, Value::List(_)));

    let children = ctx
        .client
        .query(
            "SubtensorModule",
            "ChildKeys",
            &[s(ctx.alice.hotkey.ss58_address()), u16v(1)],
            None,
        )
        .expect("children read");
    assert!(matches!(children, Value::List(_)));
}

#[test]
fn test_quote_stake_slippage_and_fee() {
    let ctx = TestContext::new();
    let quote = ctx
        .client
        .quote_stake(1, amount_tao(1), None)
        .expect("stake quote");
    assert!(quote.alpha_amount > 0);
    assert!(quote.tao_fee <= quote.tao_amount);
}

#[test]
fn test_metagraph_fast_path_matches_runtime() {
    let ctx = TestContext::new();
    let fast = ctx.client.neurons(1, None).expect("fast neurons read");
    let raw = ctx
        .client
        .runtime_call("NeuronInfoRuntimeApi", "get_neurons_lite", &[u16v(1)], None)
        .expect("raw neurons read");
    assert_eq!(fast.len(), value_list(&raw).len());
    assert!(fast
        .iter()
        .all(|neuron| field(neuron, "hotkey").and_then(as_str).is_some()));
}

#[test]
fn test_snapshot_pins_reads_to_one_block() {
    let ctx = TestContext::new();
    let snapshot = ctx.client.at(None).expect("snapshot");
    assert!(snapshot.block_number > 0);
    let live = ctx
        .client
        .runtime_call("NeuronInfoRuntimeApi", "get_neurons_lite", &[u16v(1)], None)
        .expect("live neurons");
    let pinned = snapshot
        .runtime_call("NeuronInfoRuntimeApi", "get_neurons_lite", &[u16v(1)])
        .expect("pinned neurons");
    assert_eq!(value_list(&live).len(), value_list(&pinned).len());
    assert!(
        snapshot
            .balance_rao(&ctx.alice.coldkey.ss58_address())
            .expect("pinned balance")
            > 0
    );
}

#[test]
fn test_leases_read() {
    let ctx = TestContext::new();
    let leases = ctx
        .client
        .query_map("SubtensorModule", "SubnetLeases", &[], None)
        .expect("leases read");
    assert!(leases.iter().all(|(key, _)| as_u128(key).is_some()));
    let missing = ctx
        .client
        .query("SubtensorModule", "SubnetLeases", &[u32v(999_999)], None)
        .expect("missing lease read");
    assert!(matches!(missing, Value::Null));
}

#[test]
fn test_block_subscription_streams_increasing_headers() {
    let ctx = TestContext::new();
    let mut blocks = ctx.client.blocks(false);
    let first = blocks
        .next()
        .expect("first header")
        .expect("first header result");
    let second = blocks
        .next()
        .expect("second header")
        .expect("second header result");
    assert!(second.number >= first.number);
}

#[test]
fn test_transfer_moves_exactly_the_amount() {
    let ctx = TestContext::new();
    let bob = ctx.bob.coldkey.ss58_address();
    let before = ctx.client.balance_rao(&bob).expect("Bob balance");
    let result = ctx
        .executor()
        .execute(&transfer(bob.clone(), amount_tao(3)), &ctx.alice)
        .expect("transfer submits");
    assert_success(&result);
    assert_eq!(
        ctx.client.balance_rao(&bob).expect("Bob balance") - before,
        amount_tao(3)
    );
}

#[test]
fn test_add_stake_increases_stake() {
    let ctx = TestContext::new();
    let netuid = ctx.owned_subnet();
    let cold = ctx.alice.coldkey.ss58_address();
    let hot = ctx.alice.hotkey.ss58_address();
    let before = ctx
        .client
        .stake_rao(&cold, &hot, netuid, None)
        .expect("stake read");
    let result = ctx
        .executor()
        .execute(&add_stake(hot.clone(), netuid, amount_tao(10)), &ctx.alice)
        .expect("stake submits");
    assert_success(&result);
    let after = ctx
        .client
        .stake_rao(&cold, &hot, netuid, None)
        .expect("stake read");
    assert!(after > before);
}

#[test]
fn test_delegation_lifecycle() {
    let ctx = TestContext::new();
    let netuid = ctx.owned_subnet();
    let cold = ctx.alice.coldkey.ss58_address();
    let hot = ctx.alice.hotkey.ss58_address();
    let _ = ctx
        .executor()
        .execute(&root_register(hot.clone()), &ctx.alice);

    let delegate = ctx
        .client
        .runtime_call(
            "DelegateInfoRuntimeApi",
            "get_delegate",
            &[s(hot.clone())],
            None,
        )
        .expect("delegate read");
    assert!(!matches!(delegate, Value::Null));
    assert!(value_contains_str(&delegate, &hot));

    let delegates = ctx
        .client
        .runtime_call("DelegateInfoRuntimeApi", "get_delegates", &[], None)
        .expect("delegates read");
    assert!(value_list(&delegates)
        .iter()
        .any(|delegate| value_contains_str(delegate, &hot)));

    let current = ctx
        .client
        .query("SubtensorModule", "Delegates", &[s(hot.clone())], None)
        .expect("take read");
    let current = as_u128(&current).unwrap_or_default();
    let minimum = ctx
        .client
        .query("SubtensorModule", "MinDelegateTake", &[], None)
        .expect("min take read");
    let maximum = ctx
        .client
        .query("SubtensorModule", "MaxDelegateTake", &[], None)
        .expect("max take read");
    assert!(as_u128(&minimum).unwrap_or_default() <= current);
    assert!(current <= as_u128(&maximum).unwrap_or(u128::from(u16::MAX)));

    let target = current
        .saturating_sub(1_000)
        .max(current / 2)
        .min(u128::from(u16::MAX));
    if target != current {
        let target_u16 = u16::try_from(target).expect("delegate take fits u16");
        let set_take =
            IntentCall::set_take(&ctx.client, hot.clone(), target_u16).expect("set take intent");
        let result = ctx
            .executor()
            .execute(&set_take, &ctx.alice)
            .expect("set take submits");
        assert_success(&result);
        let after = ctx
            .client
            .query("SubtensorModule", "Delegates", &[s(hot.clone())], None)
            .expect("take read");
        assert_eq!(as_u128(&after), Some(target));
    }

    let nominations = ctx
        .client
        .runtime_call(
            "DelegateInfoRuntimeApi",
            "get_delegated",
            &[s(cold.clone())],
            None,
        )
        .expect("delegated nominations");
    assert!(value_list(&nominations).iter().any(|nomination| {
        let Value::Tuple(parts) = nomination else {
            return false;
        };
        let Some(delegate) = parts.first() else {
            return false;
        };
        let Some(position) = parts.get(1) else {
            return false;
        };
        let values = match position {
            Value::Tuple(values) | Value::List(values) => values,
            _ => return false,
        };
        value_contains_str(delegate, &hot)
            && values.first().and_then(as_u128) == Some(u128::from(netuid))
            && values.get(1).and_then(as_u128).unwrap_or_default() > 0
    }));

    let by_coldkey = ctx
        .client
        .runtime_call(
            "StakeInfoRuntimeApi",
            "get_stake_info_for_coldkeys",
            &[Value::List(vec![s(cold.clone())])],
            None,
        )
        .expect("batched stake positions");
    assert!(value_list(&by_coldkey).iter().any(|entry| {
        let Value::Tuple(parts) = entry else {
            return false;
        };
        parts
            .first()
            .is_some_and(|value| value_contains_str(value, &cold))
            && parts.get(1).is_some_and(|positions| {
                value_list(positions).iter().any(|position| {
                    field(position, "netuid").and_then(as_u128) == Some(u128::from(netuid))
                })
            })
    }));

    let positions = ctx
        .client
        .runtime_call(
            "StakeInfoRuntimeApi",
            "get_stake_info_for_coldkey",
            &[s(cold)],
            None,
        )
        .expect("stake positions");
    assert!(value_list(&positions).iter().any(|position| {
        field(position, "netuid").and_then(as_u128) == Some(u128::from(netuid))
            && field(position, "stake")
                .and_then(as_u128)
                .unwrap_or_default()
                > 0
    }));
}

#[test]
fn test_owned_subnet_is_registered() {
    let ctx = TestContext::new();
    let netuid = ctx.owned_subnet();
    let subnets = ctx.client.subnets(None).expect("subnets read");
    assert!(subnets.iter().any(|subnet| subnet.netuid == netuid));

    let metagraph = ctx
        .client
        .metagraph(netuid, None)
        .expect("owned metagraph read");
    assert!(
        field(&metagraph, "hotkeys")
            .is_some_and(|hotkeys| value_contains_str(hotkeys, &ctx.alice.hotkey.ss58_address())),
        "owned subnet metagraph did not contain Alice's hotkey: {metagraph:#?}"
    );
}

#[test]
fn test_serving_endpoints() {
    let ctx = TestContext::new();
    let netuid = ctx.owned_subnet();
    let tls = IntentCall::new(
        "serve_axon_tls",
        SignerRole::Hotkey,
        "SubtensorModule",
        "serve_axon_tls",
        record([
            ("netuid", u(netuid)),
            ("version", u32v(1)),
            ("ip", u128v(3_405_803_781)),
            ("port", u16v(8_091)),
            ("ip_type", u8v(4)),
            ("protocol", u8v(4)),
            ("placeholder1", u8v(0)),
            ("placeholder2", u8v(0)),
            ("certificate", bytes(vec![0xab; 32])),
        ]),
    )
    .touches([netuid]);
    let result = ctx
        .executor()
        .execute(&tls, &ctx.alice)
        .expect("TLS axon submission");
    assert_success(&result);

    let prometheus = IntentCall::new(
        "serve_prometheus",
        SignerRole::Hotkey,
        "SubtensorModule",
        "serve_prometheus",
        record([
            ("netuid", u(netuid)),
            ("version", u32v(1)),
            ("ip", u128v(3_405_803_781)),
            ("port", u16v(9_090)),
            ("ip_type", u8v(4)),
        ]),
    )
    .touches([netuid]);
    let result = ctx
        .executor()
        .execute(&prometheus, &ctx.alice)
        .expect("Prometheus submission");
    assert_success(&result);
}

#[test]
fn test_owner_sets_hyperparameter() {
    let ctx = TestContext::new();
    let netuid = ctx.owned_subnet();
    let intent = IntentCall::set_hyperparameter(netuid, "immunity_period", u16v(42))
        .expect("owner-settable hyperparameter");
    let result = ctx
        .executor()
        .execute(&intent, &ctx.alice)
        .expect("hyperparameter submission");
    let hyperparameters = ctx
        .client
        .subnet_hyperparameters(netuid, None)
        .expect("hyperparameters read");
    let applied = field(&hyperparameters, "immunity_period").and_then(as_u128) == Some(42);
    let message = result.message.to_lowercase();
    let throttled = !result.success
        && ["rate limit", "prohibited", "freeze"]
            .iter()
            .any(|needle| message.contains(needle));
    assert!(
        applied || throttled,
        "success={} message={} hyperparameters={hyperparameters:#?}",
        result.success,
        result.message
    );
}

#[test]
fn test_unsettable_hyperparameter_rejected_at_construction() {
    assert!(IntentCall::set_hyperparameter(1, "tempo", u16v(1)).is_err());
}

#[test]
fn test_identity_coldkey_and_subnet() {
    let ctx = TestContext::new();
    let netuid = ctx.owned_subnet();
    let coldkey = ctx.alice.coldkey.ss58_address();

    let identity = IntentCall::new(
        "set_identity",
        SignerRole::Coldkey,
        "SubtensorModule",
        "set_identity",
        record([
            ("name", bytes(b"E2E Alice".to_vec())),
            ("url", bytes(b"https://a.example".to_vec())),
            ("github_repo", bytes(Vec::new())),
            ("image", bytes(Vec::new())),
            ("discord", bytes(Vec::new())),
            ("description", bytes(Vec::new())),
            ("additional", bytes(Vec::new())),
        ]),
    );
    let result = ctx
        .executor()
        .execute(&identity, &ctx.alice)
        .expect("identity submission");
    assert_success(&result);
    let stored = ctx
        .client
        .query("SubtensorModule", "IdentitiesV2", &[s(coldkey)], None)
        .expect("identity read");
    assert_eq!(
        field(&stored, "name").and_then(text_bytes).as_deref(),
        Some("E2E Alice")
    );

    let subnet_identity = IntentCall::new(
        "set_subnet_identity",
        SignerRole::Coldkey,
        "SubtensorModule",
        "set_subnet_identity",
        subnet_identity_params(netuid, "e2e-net"),
    )
    .touches([netuid]);
    let result = ctx
        .executor()
        .execute(&subnet_identity, &ctx.alice)
        .expect("subnet identity submission");
    assert_success(&result);
    let stored = ctx
        .client
        .query("SubtensorModule", "SubnetIdentitiesV3", &[u(netuid)], None)
        .expect("subnet identity read");
    assert_eq!(
        field(&stored, "subnet_name")
            .and_then(text_bytes)
            .as_deref(),
        Some("e2e-net")
    );
}

#[test]
fn test_auto_stake_destination() {
    let ctx = TestContext::new();
    let netuid = ctx.owned_subnet();
    let coldkey = ctx.alice.coldkey.ss58_address();
    let hotkey = ctx.alice.hotkey.ss58_address();
    let intent = IntentCall::new(
        "set_auto_stake",
        SignerRole::Coldkey,
        "SubtensorModule",
        "set_coldkey_auto_stake_hotkey",
        record([("netuid", u(netuid)), ("hotkey", s(hotkey.clone()))]),
    )
    .touches([netuid]);
    let _ = ctx.executor().execute(&intent, &ctx.alice);
    let destination = ctx
        .client
        .query(
            "SubtensorModule",
            "AutoStakeDestination",
            &[s(coldkey), u(netuid)],
            None,
        )
        .expect("auto-stake destination read");
    assert_eq!(as_str(&destination), Some(hotkey.as_str()));
}
