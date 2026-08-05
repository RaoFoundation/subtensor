#![allow(
    dead_code,
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]

use std::env;
use std::process::{Command, Output};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bittensor_core::client::{as_str, as_u128, field, Client, TxOutcome};
use bittensor_core::codec::extrinsic::{multisig_account_id, multisig_ss58};
use bittensor_core::codec::Value;
use bittensor_core::keys::{public_key_from_ss58, ss58_from_public, Keypair, CRYPTO_SR25519};
use bittensor_core::transaction::{Executor, IntentCall, SignerRole, Spend, Wallet};
use bittensor_core::CoreError;
use sp_core::hashing::blake2_256;

pub const RAO_PER_TAO: u128 = 1_000_000_000;
const DEFAULT_IMAGE: &str = "ghcr.io/raofoundation/subtensor-localnet:monorepo-sdk";
const LOCALNET_START_TIMEOUT: Duration = Duration::from_secs(180);

pub struct TestContext {
    _localnet: Localnet,
    pub client: Client,
    pub alice: Wallet,
    pub bob: Wallet,
}

impl TestContext {
    pub fn new() -> Self {
        let localnet = Localnet::start();
        let client = Client::connect(&localnet.endpoint)
            .unwrap_or_else(|error| panic!("connect {}: {error}", localnet.endpoint));
        let alice = Wallet::from_uris("//Alice", "//Alice//hot").expect("Alice dev wallet");
        let bob = Wallet::from_uris("//Bob", "//Bob//hot").expect("Bob dev wallet");
        Self {
            _localnet: localnet,
            client,
            alice,
            bob,
        }
    }

    pub fn executor(&self) -> Executor<'_> {
        Executor::new(&self.client)
    }

    pub fn owned_subnet(&self) -> u16 {
        register_subnet(&self.client, &self.alice)
    }
}

struct Localnet {
    endpoint: String,
    container_name: Option<String>,
}

impl Localnet {
    fn start() -> Self {
        if let Ok(endpoint) = env::var("E2E_ENDPOINT") {
            if !endpoint.trim().is_empty() {
                return Self {
                    endpoint,
                    container_name: None,
                };
            }
        }

        let image = env::var("LOCALNET_IMAGE_NAME")
            .or_else(|_| env::var("LOCALNET_IMAGE"))
            .unwrap_or_else(|_| DEFAULT_IMAGE.into());
        if env::var_os("SKIP_PULL").is_none() {
            checked(Command::new("docker").args(["pull", &image]), "docker pull");
        }

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let name = format!("bittensor-rust-e2e-{}-{nonce}", std::process::id());
        checked(
            Command::new("docker").args([
                "run",
                "--rm",
                "-d",
                "--name",
                &name,
                "-p",
                "127.0.0.1::9944",
                "-p",
                "127.0.0.1::9945",
                &image,
            ]),
            "docker run",
        );

        let deadline = Instant::now() + LOCALNET_START_TIMEOUT;
        loop {
            let logs = output(Command::new("docker").args(["logs", &name]), "docker logs");
            let combined = format!(
                "{}\n{}",
                String::from_utf8_lossy(&logs.stdout),
                String::from_utf8_lossy(&logs.stderr)
            );
            if combined.contains("Imported #1") {
                break;
            }
            if Instant::now() >= deadline {
                let _ = Command::new("docker").args(["rm", "-f", &name]).output();
                panic!(
                    "localnet container {name} did not import block #1 within {}s\n{combined}",
                    LOCALNET_START_TIMEOUT.as_secs()
                );
            }
            thread::sleep(Duration::from_secs(1));
        }

        let port = output(
            Command::new("docker").args(["port", &name, "9944/tcp"]),
            "docker port",
        );
        let mapping = String::from_utf8_lossy(&port.stdout);
        let port = mapping
            .lines()
            .next()
            .and_then(|line| line.rsplit(':').next())
            .map(str::trim)
            .filter(|port| !port.is_empty())
            .unwrap_or_else(|| panic!("cannot parse docker port mapping: {mapping}"));
        Self {
            endpoint: format!("ws://127.0.0.1:{port}"),
            container_name: Some(name),
        }
    }
}

impl Drop for Localnet {
    fn drop(&mut self) {
        if let Some(name) = &self.container_name {
            let _ = Command::new("docker").args(["rm", "-f", name]).output();
        }
    }
}

fn checked(command: &mut Command, label: &str) {
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("{label}: cannot start command: {error}"));
    if !output.status.success() {
        panic!(
            "{label} failed ({})\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn output(command: &mut Command, label: &str) -> Output {
    command
        .output()
        .unwrap_or_else(|error| panic!("{label}: cannot start command: {error}"))
}

pub fn assert_success(result: &TxOutcome) {
    assert!(
        result.success,
        "transaction failed: {} ({:?})\nevents: {:#?}",
        result.message, result.error, result.events
    );
}

pub fn amount_tao(tao: u128) -> u128 {
    tao.checked_mul(RAO_PER_TAO).expect("TAO amount fits u128")
}

pub fn random_wallet() -> Wallet {
    let mnemonic = Keypair::generate_mnemonic(12).expect("random mnemonic");
    Wallet {
        coldkey: Keypair::from_mnemonic(&mnemonic, CRYPTO_SR25519, None).expect("random coldkey"),
        hotkey: Keypair::from_mnemonic(&mnemonic, CRYPTO_SR25519, None).expect("random hotkey"),
    }
}

pub fn register_subnet(client: &Client, wallet: &Wallet) -> u16 {
    let intent = IntentCall::new(
        "register_subnet",
        SignerRole::Coldkey,
        "SubtensorModule",
        "register_network",
        record([("hotkey", s(wallet.hotkey.ss58_address()))]),
    );
    let result = Executor::new(client)
        .execute(&intent, wallet)
        .expect("register subnet submits");
    assert_success(&result);
    let netuid = client
        .subnets(None)
        .expect("subnets read")
        .into_iter()
        .map(|subnet| subnet.netuid)
        .max()
        .expect("at least root subnet");
    let start = IntentCall::new(
        "start_call",
        SignerRole::Coldkey,
        "SubtensorModule",
        "start_call",
        record([("netuid", u(netuid))]),
    )
    .touches([netuid]);
    let _ = Executor::new(client).execute(&start, wallet);
    netuid
}

pub fn transfer(dest: impl Into<String>, amount_rao: u128) -> IntentCall {
    IntentCall::transfer(dest, amount_rao)
}

pub fn transfer_allow_death(dest: impl Into<String>, amount_rao: u128) -> IntentCall {
    IntentCall::transfer_allow_death(dest, amount_rao)
}

pub fn add_stake(hotkey: impl Into<String>, netuid: u16, amount_rao: u128) -> IntentCall {
    IntentCall::add_stake(hotkey, netuid, amount_rao)
}

pub fn root_register(hotkey: impl Into<String>) -> IntentCall {
    IntentCall::root_register(hotkey)
}

pub fn wait_for_blocks(client: &Client, count: u64) {
    let start = client.block_number().expect("block number");
    let deadline = Instant::now() + Duration::from_secs(60);
    while client.block_number().expect("block number") < start + count {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {count} blocks"
        );
        thread::sleep(Duration::from_millis(50));
    }
}

pub fn event_attributes<'a>(result: &'a TxOutcome, module: &str, event: &str) -> Option<&'a Value> {
    result.events.iter().find_map(|record| {
        let module_id = field(record, "module_id").and_then(as_str);
        let event_id = field(record, "event_id").and_then(as_str);
        (module_id == Some(module) && event_id == Some(event))
            .then(|| field(record, "attributes"))
            .flatten()
    })
}

pub fn value_u128(value: &Value, name: &str) -> u128 {
    field(value, name)
        .and_then(as_u128)
        .unwrap_or_else(|| panic!("missing integer field {name} in {value:#?}"))
}

pub fn value_str<'a>(value: &'a Value, name: &str) -> &'a str {
    field(value, name)
        .and_then(as_str)
        .unwrap_or_else(|| panic!("missing string field {name} in {value:#?}"))
}

pub fn record<const N: usize>(fields: [(&str, Value); N]) -> Value {
    Value::record(
        fields
            .into_iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect(),
    )
}

pub fn s(value: impl Into<String>) -> Value {
    Value::str(value)
}

pub fn u(value: u16) -> Value {
    Value::Uint(u128::from(value))
}

pub fn u32v(value: u32) -> Value {
    Value::Uint(u128::from(value))
}

pub fn u64v(value: u64) -> Value {
    Value::Uint(u128::from(value))
}

pub fn u128v(value: u128) -> Value {
    Value::Uint(value)
}

pub fn list(values: impl IntoIterator<Item = Value>) -> Value {
    Value::List(values.into_iter().collect())
}

pub fn bytes(value: impl Into<Vec<u8>>) -> Value {
    Value::Bytes(value.into())
}

pub fn boolv(value: bool) -> Value {
    Value::Bool(value)
}

pub fn call_hash(call: &[u8]) -> [u8; 32] {
    blake2_256(call)
}

pub struct MultisigFixture {
    pub address: String,
    pub sorted: Vec<[u8; 32]>,
    pub threshold: u16,
}

impl MultisigFixture {
    pub fn new(client: &Client, signers: &[&str], threshold: u16) -> Self {
        let public: Vec<[u8; 32]> = signers
            .iter()
            .map(|address| public_key_from_ss58(address).expect("valid signatory address"))
            .collect();
        let (account, sorted) = multisig_account_id(&public, threshold).expect("multisig account");
        Self {
            address: multisig_ss58(account, client.ss58_format()),
            sorted,
            threshold,
        }
    }

    pub fn others(&self, signer: [u8; 32]) -> Value {
        list(
            self.sorted
                .iter()
                .copied()
                .filter(|candidate| *candidate != signer)
                .map(|candidate| bytes(candidate.to_vec())),
        )
    }
}

pub fn max_weight() -> Value {
    record([
        ("ref_time", u128v(1_000_000_000_000)),
        ("proof_size", u128v(1_000_000)),
    ])
}

pub fn proxy_type(name: &str) -> Value {
    s(name)
}

pub fn sample_intent(ctx: &TestContext, op: &str, netuid: u16) -> Result<IntentCall, CoreError> {
    let alice_cold = ctx.alice.coldkey.ss58_address();
    let alice_hot = ctx.alice.hotkey.ss58_address();
    let bob_cold = ctx.bob.coldkey.ss58_address();
    let bob_hot = ctx.bob.hotkey.ss58_address();
    let one = amount_tao(1);
    let inner_transfer = || {
        ctx.client.compose_call(
            "Balances",
            "transfer_keep_alive",
            &record([("dest", s(bob_cold.clone())), ("value", u128v(one / 2))]),
        )
    };

    let make = |signer: SignerRole, pallet: &str, function: &str, params: Value| {
        IntentCall::new(op, signer, pallet, function, params)
    };

    let intent = match op {
        "add_proxy" => make(
            SignerRole::Coldkey,
            "Proxy",
            "add_proxy",
            record([
                ("delegate", s(bob_cold.clone())),
                ("proxy_type", proxy_type("Transfer")),
                ("delay", u32v(0)),
            ]),
        ),
        "add_stake" => add_stake(bob_hot.clone(), netuid, one),
        "add_stake_limit" => make(
            SignerRole::Coldkey,
            "SubtensorModule",
            "add_stake_limit",
            record([
                ("hotkey", s(bob_hot.clone())),
                ("netuid", u(netuid)),
                ("amount_staked", u128v(one)),
                ("limit_price", u128v(one)),
                ("allow_partial", boolv(false)),
            ]),
        )
        .spend(Spend::Bounded(one))
        .touches([netuid]),
        "announce_coldkey_swap" => make(
            SignerRole::Coldkey,
            "SubtensorModule",
            "announce_coldkey_swap",
            record([(
                "new_coldkey_hash",
                bytes(blake2_256(&public_key_from_ss58(&bob_cold)?).to_vec()),
            )]),
        ),
        "associate_evm_key" => make(
            SignerRole::Hotkey,
            "SubtensorModule",
            "associate_evm_key",
            record([
                ("netuid", u(netuid)),
                ("evm_key", s(format!("0x{}", "11".repeat(20)))),
                ("block_number", u64v(ctx.client.block_number()?)),
                ("signature", s(format!("0x{}", "22".repeat(65)))),
            ]),
        )
        .touches([netuid]),
        "associate_hotkey" => make(
            SignerRole::Coldkey,
            "SubtensorModule",
            "try_associate_hotkey",
            record([("hotkey", s(bob_hot.clone()))]),
        ),
        "batch" => IntentCall::batch(
            &ctx.client,
            vec![
                transfer(bob_cold.clone(), one / 2),
                add_stake(bob_hot.clone(), netuid, one),
            ],
        )?,
        "burned_register" => make(
            SignerRole::Coldkey,
            "SubtensorModule",
            "burned_register",
            record([("netuid", u(netuid)), ("hotkey", s(alice_hot.clone()))]),
        )
        .spend(Spend::Unbounded)
        .touches([netuid]),
        "claim_root" => make(
            SignerRole::Coldkey,
            "SubtensorModule",
            "claim_root",
            record([("subnets", list([u(0)]))]),
        )
        .touches([0]),
        "claim_root_with_hotkey" => make(
            SignerRole::Coldkey,
            "SubtensorModule",
            "claim_root_with_hotkey",
            record([("hotkey", s(alice_hot.clone()))]),
        )
        .touches([0]),
        "clear_coldkey_swap_announcement" => make(
            SignerRole::Coldkey,
            "SubtensorModule",
            "clear_coldkey_swap_announcement",
            record([]),
        ),
        "commit_weights" => make(
            SignerRole::Hotkey,
            "SubtensorModule",
            "commit_timelocked_mechanism_weights",
            record([
                ("netuid", u(netuid)),
                ("mecid", u16v(0)),
                ("commit", bytes(vec![1, 2, 3, 4])),
                ("reveal_round", u64v(ctx.client.block_number()? + 100)),
                ("commit_reveal_version", u64v(4)),
            ]),
        )
        .touches([netuid]),
        "contribute_crowdloan" => make(
            SignerRole::Coldkey,
            "Crowdloan",
            "contribute",
            record([("crowdloan_id", u32v(0)), ("amount", u128v(one))]),
        )
        .spend(Spend::Bounded(one)),
        "create_crowdloan" => make(
            SignerRole::Coldkey,
            "Crowdloan",
            "create",
            record([
                ("deposit", u128v(amount_tao(100))),
                ("min_contribution", u128v(one)),
                ("cap", u128v(amount_tao(1_000))),
                ("end", u64v(ctx.client.block_number()? + 5_000)),
                ("call", Value::Null),
                ("target_address", s(bob_cold.clone())),
            ]),
        )
        .spend(Spend::Bounded(amount_tao(100))),
        "create_pure_proxy" => make(
            SignerRole::Coldkey,
            "Proxy",
            "create_pure",
            record([
                ("proxy_type", proxy_type("Any")),
                ("delay", u32v(0)),
                ("index", u16v(0)),
            ]),
        ),
        "decrease_take" => make(
            SignerRole::Coldkey,
            "SubtensorModule",
            "decrease_take",
            record([("hotkey", s(alice_hot.clone())), ("take", u16v(500))]),
        ),
        "set_take" => {
            let current = ctx
                .client
                .query(
                    "SubtensorModule",
                    "Delegates",
                    &[s(alice_hot.clone())],
                    None,
                )
                .ok()
                .and_then(|value| as_u128(&value))
                .unwrap_or_default();
            let target: u16 = if current == 500 { 501 } else { 500 };
            IntentCall::set_take(&ctx.client, alice_hot.clone(), target)?
        }
        "dispute_coldkey_swap" => make(
            SignerRole::Coldkey,
            "SubtensorModule",
            "dispute_coldkey_swap",
            record([]),
        ),
        "dissolve_crowdloan" => crowdloan_id_call(op, "dissolve", 0),
        "evm_withdraw" => make(
            SignerRole::Coldkey,
            "EVM",
            "withdraw",
            record([
                (
                    "address",
                    bytes(ctx.alice.coldkey.public_key_bytes()[..20].to_vec()),
                ),
                ("value", u128v(one)),
            ]),
        ),
        "execute_proxy_announced" => make(
            SignerRole::Coldkey,
            "Proxy",
            "proxy_announced",
            record([
                ("delegate", s(bob_cold.clone())),
                ("real", s(alice_cold.clone())),
                ("force_proxy_type", Value::Null),
                ("call", bytes(inner_transfer()?)),
            ]),
        ),
        "finalize_crowdloan" => crowdloan_id_call(op, "finalize", 0),
        "fund_evm_key" => {
            let mut input = b"evm:".to_vec();
            input.extend_from_slice(&[0x11; 20]);
            IntentCall::fund_evm_key(
                ss58_from_public(blake2_256(&input), ctx.client.ss58_format()),
                one,
            )
        }
        "increase_take" => make(
            SignerRole::Coldkey,
            "SubtensorModule",
            "increase_take",
            record([("hotkey", s(alice_hot.clone())), ("take", u16v(1_000))]),
        ),
        "kill_pure_proxy" => make(
            SignerRole::Coldkey,
            "Proxy",
            "kill_pure",
            record([
                ("spawner", s(bob_cold.clone())),
                ("proxy_type", proxy_type("Any")),
                ("index", u16v(0)),
                ("height", u32v(1)),
                ("ext_index", u32v(0)),
            ]),
        ),
        "lock_stake" => make(
            SignerRole::Coldkey,
            "SubtensorModule",
            "lock_stake",
            record([
                ("hotkey", s(alice_hot.clone())),
                ("netuid", u(netuid)),
                ("amount", u128v(one)),
            ]),
        )
        .touches([netuid]),
        "move_lock" => make(
            SignerRole::Coldkey,
            "SubtensorModule",
            "move_lock",
            record([
                ("destination_hotkey", s(bob_hot.clone())),
                ("netuid", u(netuid)),
            ]),
        )
        .touches([netuid]),
        "move_stake" => make(
            SignerRole::Coldkey,
            "SubtensorModule",
            "move_stake",
            record([
                ("origin_hotkey", s(bob_hot.clone())),
                ("destination_hotkey", s(bob_hot.clone())),
                ("origin_netuid", u(netuid)),
                ("destination_netuid", u(netuid)),
                ("alpha_amount", u128v(one)),
            ]),
        )
        .touches([netuid]),
        "multisig_approve" => make(
            SignerRole::Coldkey,
            "Multisig",
            "as_multi",
            record([
                ("threshold", u16v(2)),
                ("other_signatories", list([s(bob_cold.clone())])),
                ("maybe_timepoint", Value::Null),
                ("call", Value::Bytes(inner_transfer()?)),
                ("max_weight", max_weight()),
            ]),
        ),
        "multisig_cancel" => make(
            SignerRole::Coldkey,
            "Multisig",
            "cancel_as_multi",
            record([
                ("threshold", u16v(2)),
                ("other_signatories", list([s(bob_cold.clone())])),
                (
                    "timepoint",
                    record([("height", u32v(1)), ("index", u32v(0))]),
                ),
                ("call_hash", bytes(call_hash(&inner_transfer()?).to_vec())),
            ]),
        ),
        "multisig_execute" => make(
            SignerRole::Coldkey,
            "Multisig",
            "as_multi",
            record([
                ("threshold", u16v(2)),
                ("other_signatories", list([s(bob_cold.clone())])),
                ("maybe_timepoint", Value::Null),
                ("call", bytes(inner_transfer()?)),
                ("max_weight", max_weight()),
            ]),
        ),
        "multisig_threshold_1" => make(
            SignerRole::Coldkey,
            "Multisig",
            "as_multi_threshold_1",
            record([
                ("other_signatories", list([s(bob_cold.clone())])),
                ("call", bytes(inner_transfer()?)),
            ]),
        ),
        "refund_crowdloan" => crowdloan_id_call(op, "refund", 0),
        "register_leased_network" => make(
            SignerRole::Coldkey,
            "SubtensorModule",
            "register_leased_network",
            record([
                ("emissions_share", u16v(20)),
                ("end_block", u64v(1_000_000_000)),
            ]),
        )
        .spend(Spend::Unbounded),
        "register_subnet" => make(
            SignerRole::Coldkey,
            "SubtensorModule",
            "register_network",
            record([("hotkey", s(alice_hot.clone()))]),
        )
        .spend(Spend::Unbounded),
        "remove_proxies" => make(SignerRole::Coldkey, "Proxy", "remove_proxies", record([])),
        "remove_proxy" => make(
            SignerRole::Coldkey,
            "Proxy",
            "remove_proxy",
            record([
                ("delegate", s(bob_cold.clone())),
                ("proxy_type", proxy_type("Transfer")),
                ("delay", u32v(0)),
            ]),
        ),
        "remove_stake" => make(
            SignerRole::Coldkey,
            "SubtensorModule",
            "remove_stake",
            record([
                ("hotkey", s(bob_hot.clone())),
                ("netuid", u(netuid)),
                ("amount_unstaked", u128v(one)),
            ]),
        )
        .touches([netuid]),
        "remove_stake_limit" => make(
            SignerRole::Coldkey,
            "SubtensorModule",
            "remove_stake_limit",
            record([
                ("hotkey", s(bob_hot.clone())),
                ("netuid", u(netuid)),
                ("amount_unstaked", u128v(one)),
                ("limit_price", u128v(one)),
                ("allow_partial", boolv(false)),
            ]),
        )
        .touches([netuid]),
        "reset_axon" => make(
            SignerRole::Hotkey,
            "SubtensorModule",
            "serve_axon",
            serve_axon_params(netuid, 0, 0, 1, 4),
        )
        .touches([netuid]),
        "reveal_weights" => make(
            SignerRole::Hotkey,
            "SubtensorModule",
            "reveal_weights",
            record([
                ("netuid", u(netuid)),
                ("uids", list([u16v(0)])),
                ("values", list([u16v(u16::MAX)])),
                ("salt", list([u16v(1), u16v(2), u16v(3)])),
                ("version_key", u64v(0)),
            ]),
        )
        .touches([netuid]),
        "root_register" => root_register(alice_hot.clone()).touches([0]),
        "serve_axon" => make(
            SignerRole::Hotkey,
            "SubtensorModule",
            "serve_axon",
            serve_axon_params(netuid, 1, 3_405_803_781, 8_091, 4),
        )
        .touches([netuid]),
        "serve_axon_tls" => make(
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
        .touches([netuid]),
        "serve_prometheus" => make(
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
        .touches([netuid]),
        "set_auto_stake" => make(
            SignerRole::Coldkey,
            "SubtensorModule",
            "set_coldkey_auto_stake_hotkey",
            record([("netuid", u(netuid)), ("hotkey", s(bob_hot.clone()))]),
        )
        .touches([netuid]),
        "set_childkey_take" => make(
            SignerRole::Coldkey,
            "SubtensorModule",
            "set_childkey_take",
            record([
                ("hotkey", s(alice_hot.clone())),
                ("netuid", u(netuid)),
                ("take", u16v(1_000)),
            ]),
        )
        .touches([netuid]),
        "set_children" => make(
            SignerRole::Coldkey,
            "SubtensorModule",
            "set_children",
            record([
                ("hotkey", s(alice_hot.clone())),
                ("netuid", u(netuid)),
                (
                    "children",
                    list([Value::Tuple(vec![u128v(1_u128 << 63), s(bob_hot.clone())])]),
                ),
            ]),
        )
        .touches([netuid]),
        "set_crowdloan_max_contribution" => make(
            SignerRole::Coldkey,
            "Crowdloan",
            "set_max_contribution",
            record([
                ("crowdloan_id", u32v(0)),
                ("new_max_contribution", u128v(amount_tao(50))),
            ]),
        ),
        "set_hyperparameter" => {
            IntentCall::set_hyperparameter(netuid, "immunity_period", u16v(42))?
        }
        "set_identity" => make(
            SignerRole::Coldkey,
            "SubtensorModule",
            "set_identity",
            identity_params("verify"),
        ),
        "set_mechanism_count" => make(
            SignerRole::Coldkey,
            "AdminUtils",
            "sudo_set_mechanism_count",
            record([("netuid", u(netuid)), ("mechanism_count", u16v(2))]),
        )
        .touches([netuid]),
        "set_perpetual_lock" => make(
            SignerRole::Coldkey,
            "SubtensorModule",
            "set_perpetual_lock",
            record([("netuid", u(netuid)), ("enabled", boolv(true))]),
        )
        .touches([netuid]),
        "set_subnet_identity" => make(
            SignerRole::Coldkey,
            "SubtensorModule",
            "set_subnet_identity",
            subnet_identity_params(netuid, "verify"),
        )
        .touches([netuid]),
        "set_weights" => make(
            SignerRole::Hotkey,
            "SubtensorModule",
            "set_mechanism_weights",
            record([
                ("netuid", u(netuid)),
                ("mecid", u16v(0)),
                ("dests", list([u16v(0)])),
                ("weights", list([u16v(u16::MAX)])),
                ("version_key", u64v(0)),
            ]),
        )
        .touches([netuid]),
        "stake_burn" => make(
            SignerRole::Coldkey,
            "SubtensorModule",
            "add_stake_burn",
            record([
                ("hotkey", s(alice_hot.clone())),
                ("netuid", u(netuid)),
                ("amount", u128v(one)),
                ("limit", u128v(one)),
            ]),
        )
        .spend(Spend::Bounded(one))
        .touches([netuid]),
        "start_call" => make(
            SignerRole::Coldkey,
            "SubtensorModule",
            "start_call",
            record([("netuid", u(netuid))]),
        )
        .touches([netuid]),
        "swap_coldkey_announced" => make(
            SignerRole::Coldkey,
            "SubtensorModule",
            "swap_coldkey_announced",
            record([("new_coldkey", s(bob_cold.clone()))]),
        )
        .affects_all_subnets(),
        "swap_hotkey" => make(
            SignerRole::Coldkey,
            "SubtensorModule",
            "swap_hotkey",
            record([
                ("hotkey", s(alice_hot.clone())),
                ("new_hotkey", s(bob_hot.clone())),
                ("netuid", Value::Null),
            ]),
        )
        .affects_all_subnets(),
        "swap_stake" => make(
            SignerRole::Coldkey,
            "SubtensorModule",
            "swap_stake",
            record([
                ("hotkey", s(bob_hot.clone())),
                ("origin_netuid", u(netuid)),
                ("destination_netuid", u(netuid)),
                ("alpha_amount", u128v(one)),
            ]),
        )
        .touches([netuid]),
        "terminate_lease" => make(
            SignerRole::Coldkey,
            "SubtensorModule",
            "terminate_lease",
            record([("lease_id", u32v(0)), ("hotkey", s(alice_hot.clone()))]),
        ),
        "transfer" => transfer(bob_cold.clone(), one),
        "transfer_all" => make(
            SignerRole::Coldkey,
            "Balances",
            "transfer_all",
            record([("dest", s(bob_cold.clone())), ("keep_alive", boolv(true))]),
        )
        .spend(Spend::Unbounded),
        "transfer_stake" => make(
            SignerRole::Coldkey,
            "SubtensorModule",
            "transfer_stake",
            record([
                ("destination_coldkey", s(bob_cold.clone())),
                ("hotkey", s(bob_hot.clone())),
                ("origin_netuid", u(netuid)),
                ("destination_netuid", u(netuid)),
                ("alpha_amount", u128v(one)),
            ]),
        )
        .spend(Spend::Unbounded)
        .touches([netuid]),
        "trim_subnet" => make(
            SignerRole::Coldkey,
            "AdminUtils",
            "sudo_trim_to_max_allowed_uids",
            record([("netuid", u(netuid)), ("max_n", u16v(64))]),
        )
        .touches([netuid]),
        "unstake_all" => make(
            SignerRole::Coldkey,
            "SubtensorModule",
            "unstake_all",
            record([("hotkey", s(bob_hot.clone()))]),
        )
        .affects_all_subnets(),
        "unstake_all_alpha" => make(
            SignerRole::Coldkey,
            "SubtensorModule",
            "unstake_all_alpha",
            record([("hotkey", s(bob_hot.clone()))]),
        )
        .affects_all_subnets(),
        "update_crowdloan_cap" => make(
            SignerRole::Coldkey,
            "Crowdloan",
            "update_cap",
            record([
                ("crowdloan_id", u32v(0)),
                ("new_cap", u128v(amount_tao(2_000))),
            ]),
        ),
        "update_crowdloan_end" => make(
            SignerRole::Coldkey,
            "Crowdloan",
            "update_end",
            record([("crowdloan_id", u32v(0)), ("new_end", u64v(1_000_000_000))]),
        ),
        "update_crowdloan_min_contribution" => make(
            SignerRole::Coldkey,
            "Crowdloan",
            "update_min_contribution",
            record([
                ("crowdloan_id", u32v(0)),
                ("new_min_contribution", u128v(amount_tao(2))),
            ]),
        ),
        "update_symbol" => make(
            SignerRole::Coldkey,
            "SubtensorModule",
            "update_symbol",
            record([
                ("netuid", u(netuid)),
                ("symbol", bytes("β".as_bytes().to_vec())),
            ]),
        )
        .touches([netuid]),
        "withdraw_crowdloan" => crowdloan_id_call(op, "withdraw", 0),
        other => {
            return Err(CoreError::Policy(format!(
                "no Rust e2e sample for intent {other}"
            )))
        }
    };
    Ok(intent)
}

fn crowdloan_id_call(op: &str, function: &str, crowdloan_id: u32) -> IntentCall {
    IntentCall::new(
        op,
        SignerRole::Coldkey,
        "Crowdloan",
        function,
        record([("crowdloan_id", u32v(crowdloan_id))]),
    )
}

pub fn u8v(value: u8) -> Value {
    Value::Uint(u128::from(value))
}

pub fn u16v(value: u16) -> Value {
    Value::Uint(u128::from(value))
}

pub fn serve_axon_params(netuid: u16, version: u32, ip: u128, port: u16, protocol: u8) -> Value {
    record([
        ("netuid", u(netuid)),
        ("version", u32v(version)),
        ("ip", u128v(ip)),
        ("port", u16v(port)),
        ("ip_type", u8v(4)),
        ("protocol", u8v(protocol)),
        ("placeholder1", u8v(0)),
        ("placeholder2", u8v(0)),
    ])
}

pub fn identity_params(name: &str) -> Value {
    record([
        ("name", bytes(name.as_bytes().to_vec())),
        ("url", bytes(Vec::new())),
        ("github_repo", bytes(Vec::new())),
        ("image", bytes(Vec::new())),
        ("discord", bytes(Vec::new())),
        ("description", bytes(Vec::new())),
        ("additional", bytes(Vec::new())),
    ])
}

pub fn subnet_identity_params(netuid: u16, name: &str) -> Value {
    record([
        ("netuid", u(netuid)),
        ("subnet_name", bytes(name.as_bytes().to_vec())),
        ("github_repo", bytes(Vec::new())),
        ("subnet_contact", bytes(Vec::new())),
        ("subnet_url", bytes(Vec::new())),
        ("discord", bytes(Vec::new())),
        ("description", bytes(Vec::new())),
        ("logo_url", bytes(Vec::new())),
        ("additional", bytes(Vec::new())),
    ])
}
