//! Build-and-patch workflow for producing a local test chainspec from live network state.
//!
//! This module implements the `build-patched-spec` subcommand scenario:
//!
//! 1. Start a temporary node and sync it to the requested chain.
//! 2. Wait until sync is considered stable (RPC-reported near-head status).
//! 3. Stop the temporary node and run `export-state` from the synced database.
//! 4. Apply patching to the raw chainspec:
//!    - replace validator/authority sets with selected dev authorities,
//!    - set Sudo to the first selected validator,
//!    - clear session-derived keys and localize top-level chain fields.
//! 5. Write the final patched chainspec JSON to the requested output path.
//!
//! The result is intended for local/mainnet-clone style testing where runtime state is taken from a
//! live network, but governance/validator control is reassigned to test authorities.

use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use jsonrpsee::{
    core::client::ClientT,
    http_client::{HttpClient, HttpClientBuilder},
    rpc_params,
};
use serde_json::{Value, json};
use sp_runtime::codec::Encode;

use crate::cli::CloneStateCmd;

type CloneResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

const RPC_POLL_INTERVAL: Duration = Duration::from_secs(2);
const GRANDPA_AUTHORITIES_WELL_KNOWN_KEY: &[u8] = b":grandpa_authorities";

/// Execute `build-patched-spec`: sync network state, export raw chainspec, apply clone patch.
pub fn run(cmd: &CloneStateCmd, skip_history_backfill: bool) -> sc_cli::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|err| sc_cli::Error::Application(Box::new(err)))?;

    runtime
        .block_on(async_run(cmd, skip_history_backfill))
        .map_err(sc_cli::Error::Application)
}

async fn async_run(cmd: &CloneStateCmd, skip_history_backfill: bool) -> CloneResult<()> {
    let validators = selected_validators(cmd);
    let selected_names = validators
        .iter()
        .map(|seed| seed.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(",");

    fs::create_dir_all(&cmd.base_path)?;

    if let Some(parent) = cmd.output.parent() {
        fs::create_dir_all(parent)?;
    }

    let current_exe = std::env::current_exe()?;
    let database_arg = database_arg(cmd.database);
    let sync_arg = sync_arg(cmd.sync);

    log::info!(
        "build-patched-spec: validators={} history_backfill={}",
        selected_names,
        if skip_history_backfill {
            "skip"
        } else {
            "keep"
        }
    );

    let mut sync_args = vec![
        "--base-path".to_string(),
        cmd.base_path.display().to_string(),
        "--chain".to_string(),
        cmd.chain.clone(),
        "--sync".to_string(),
        sync_arg.to_string(),
        "--database".to_string(),
        database_arg.to_string(),
        "--rpc-port".to_string(),
        cmd.rpc_port.to_string(),
        "--port".to_string(),
        cmd.port.to_string(),
        "--rpc-methods".to_string(),
        "unsafe".to_string(),
        "--no-telemetry".to_string(),
        "--no-prometheus".to_string(),
        "--no-mdns".to_string(),
        "--name".to_string(),
        "build-patched-spec-sync".to_string(),
        "--history-backfill".to_string(),
        if skip_history_backfill {
            "skip".to_string()
        } else {
            "keep".to_string()
        },
    ];

    for bootnode in &cmd.bootnodes {
        sync_args.push("--bootnodes".to_string());
        sync_args.push(bootnode.clone());
    }

    for reserved_node in &cmd.reserved_nodes {
        sync_args.push("--reserved-nodes".to_string());
        sync_args.push(reserved_node.clone());
    }

    log::info!("build-patched-spec: starting sync node");

    let mut sync_child = Command::new(&current_exe)
        .args(&sync_args)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?;

    let sync_wait_result = wait_for_sync_completion(&mut sync_child, cmd).await;
    let stop_result = stop_child_gracefully(&mut sync_child).await;

    sync_wait_result?;
    stop_result?;

    let raw_tmp = temp_raw_path()?;

    log::info!("build-patched-spec: exporting raw state");

    export_raw_state(&current_exe, cmd, database_arg, &raw_tmp)?;

    log::info!("build-patched-spec: applying clone patch");

    patch_raw_chainspec_file(&raw_tmp, &cmd.output, &validators)?;

    if let Err(err) = fs::remove_file(&raw_tmp) {
        log::warn!(
            "build-patched-spec: warning: failed to remove temp file {}: {err}",
            raw_tmp.display()
        );
    }

    log::info!("build-patched-spec: wrote {}", cmd.output.display());

    Ok(())
}

async fn wait_for_sync_completion(sync_child: &mut Child, cmd: &CloneStateCmd) -> CloneResult<()> {
    let timeout = Duration::from_secs(cmd.sync_timeout_sec);
    let start = Instant::now();
    let mut stable_ready_checks = 0u8;
    let rpc_url = format!("http://127.0.0.1:{}", cmd.rpc_port);
    let rpc_client = HttpClientBuilder::default()
        .request_timeout(Duration::from_secs(10))
        .build(rpc_url)?;

    log::info!(
        "build-patched-spec: waiting for sync completion (timeout={}s)",
        cmd.sync_timeout_sec
    );

    while sync_child
        .try_wait()
        .map_err(|err| std::io::Error::other(format!("Failed to poll sync node process: {err}")))?
        .is_none()
    {
        if start.elapsed() > timeout {
            return Err(format!(
                "Timed out waiting for sync completion after {} seconds",
                cmd.sync_timeout_sec
            )
            .into());
        }

        match query_sync_status(&rpc_client).await {
            Ok(status) => {
                let is_ready = !status.is_syncing
                    && status.peers > 0
                    && status.current > 0
                    && status.highest > 0
                    && status.current.saturating_add(cmd.sync_lag_blocks) >= status.highest;

                if is_ready {
                    stable_ready_checks = stable_ready_checks.saturating_add(1);
                    if stable_ready_checks >= 3 {
                        log::info!("build-patched-spec: sync target reached");
                        return Ok(());
                    }
                } else {
                    stable_ready_checks = 0;
                }
            }
            Err(_) => {
                // RPC may not be ready yet.
                stable_ready_checks = 0;
            }
        }

        tokio::time::sleep(RPC_POLL_INTERVAL).await;
    }

    let status = sync_child
        .try_wait()
        .map_err(|err| std::io::Error::other(format!("Failed to poll sync node process: {err}")))?
        .ok_or_else(|| std::io::Error::other("Sync node status became unavailable"))?;

    Err(format!("Sync node exited unexpectedly: {status}").into())
}

async fn stop_child_gracefully(child: &mut Child) -> CloneResult<()> {
    if child.try_wait()?.is_some() {
        return Ok(());
    }

    Command::new("kill")
        .arg("-INT")
        .arg(child.id().to_string())
        .status()?;

    for _ in 0..30 {
        if child.try_wait()?.is_some() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    child.kill()?;

    child.wait()?;

    Ok(())
}

fn export_raw_state(
    current_exe: &Path,
    cmd: &CloneStateCmd,
    database_arg: &str,
    raw_tmp: &Path,
) -> CloneResult<()> {
    let stdout = File::create(raw_tmp)?;
    let status = Command::new(current_exe)
        .args([
            "export-state",
            "--chain",
            &cmd.chain,
            "--base-path",
            &cmd.base_path.display().to_string(),
            "--database",
            database_arg,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::inherit())
        .status()?;

    if !status.success() {
        return Err(format!("export-state failed with status {status}").into());
    }

    Ok(())
}

struct SyncStatus {
    current: u64,
    highest: u64,
    peers: u64,
    is_syncing: bool,
}

async fn query_sync_status(rpc_client: &HttpClient) -> CloneResult<SyncStatus> {
    let sync = rpc_call(rpc_client, "system_syncState").await?;
    let health = rpc_call(rpc_client, "system_health").await?;

    let current = parse_u64_field(&sync, "currentBlock")
        .ok_or_else(|| "system_syncState.currentBlock missing".to_string())?;
    let highest = parse_u64_field(&sync, "highestBlock")
        .ok_or_else(|| "system_syncState.highestBlock missing".to_string())?;
    let peers = parse_u64_field(&health, "peers")
        .ok_or_else(|| "system_health.peers missing".to_string())?;
    let is_syncing = health
        .get("isSyncing")
        .and_then(Value::as_bool)
        .ok_or_else(|| "system_health.isSyncing missing".to_string())?;

    Ok(SyncStatus {
        current,
        highest,
        peers,
        is_syncing,
    })
}

async fn rpc_call(rpc_client: &HttpClient, method: &str) -> CloneResult<Value> {
    rpc_client
        .request(method, rpc_params![])
        .await
        .map_err(Into::into)
}

fn parse_u64_field(value: &Value, field: &str) -> Option<u64> {
    let field_value = value.get(field)?;

    if let Value::Number(number) = field_value {
        return number.to_string().parse::<u64>().ok();
    }

    let s = field_value.as_str()?;

    s.parse::<u64>()
        .ok()
        .or_else(|| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
}

fn temp_raw_path() -> CloneResult<PathBuf> {
    let epoch = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    Ok(std::env::temp_dir().join(format!("subtensor-clone-export-{epoch}.json")))
}

fn sync_arg(mode: sc_cli::SyncMode) -> &'static str {
    match mode {
        sc_cli::SyncMode::Full => "full",
        sc_cli::SyncMode::Fast => "fast",
        sc_cli::SyncMode::FastUnsafe => "fast-unsafe",
        sc_cli::SyncMode::Warp => "warp",
    }
}

fn database_arg(database: sc_cli::Database) -> &'static str {
    match database {
        #[cfg(feature = "rocksdb")]
        sc_cli::Database::RocksDb => "rocksdb",
        sc_cli::Database::ParityDb => "paritydb",
        sc_cli::Database::Auto => "auto",
        sc_cli::Database::ParityDbDeprecated => "paritydb-experimental",
    }
}

fn selected_validators(cmd: &CloneStateCmd) -> Vec<&'static str> {
    let explicit = cmd.alice || cmd.bob || cmd.charlie;
    let mut selected = Vec::new();

    if explicit {
        if cmd.alice {
            selected.push("Alice");
        }
        if cmd.bob {
            selected.push("Bob");
        }
        if cmd.charlie {
            selected.push("Charlie");
        }
    } else {
        selected.push("Alice"); // only alice by default
    }

    selected
}

fn patch_raw_chainspec_file(
    input: &Path,
    output: &Path,
    validators: &[&'static str],
) -> CloneResult<()> {
    let file = File::open(input)?;
    let reader = BufReader::with_capacity(64 * 1024 * 1024, file);
    let mut spec: Value = serde_json::from_reader(reader)?;
    patch_raw_spec(&mut spec, validators)?;

    let out = File::create(output)?;
    let writer = BufWriter::with_capacity(64 * 1024 * 1024, out);
    serde_json::to_writer(writer, &spec)?;
    Ok(())
}

fn patch_raw_spec(spec: &mut Value, validators: &[&'static str]) -> CloneResult<()> {
    let sudo = validators
        .first()
        .ok_or_else(|| "at least one validator must be selected".to_string())?;

    let top = spec
        .pointer_mut("/genesis/raw/top")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "missing or invalid genesis.raw.top".to_string())?;

    let aura_keys = validators
        .iter()
        .map(|seed| crate::chain_spec::authority_keys_from_seed(seed).0)
        .collect::<Vec<_>>();
    top.insert(
        storage_key("Aura", "Authorities"),
        Value::String(to_hex(&aura_keys.encode())),
    );

    let grandpa_entries = validators
        .iter()
        .map(|seed| (crate::chain_spec::authority_keys_from_seed(seed).1, 1u64))
        .collect::<Vec<_>>();
    let grandpa_encoded = grandpa_entries.encode();

    top.insert(
        storage_key("Grandpa", "Authorities"),
        Value::String(to_hex(&grandpa_encoded)),
    );

    let mut well_known = vec![0x01u8];
    well_known.extend_from_slice(&grandpa_encoded);
    top.insert(
        to_hex(GRANDPA_AUTHORITIES_WELL_KNOWN_KEY),
        Value::String(to_hex(&well_known)),
    );

    top.insert(
        storage_key("Grandpa", "CurrentSetId"),
        Value::String(to_hex(&0u64.to_le_bytes())),
    );
    top.insert(
        storage_key("Grandpa", "State"),
        Value::String("0x00".into()),
    );
    top.remove(&storage_key("Grandpa", "PendingChange"));
    top.remove(&storage_key("Grandpa", "NextForced"));
    top.remove(&storage_key("Grandpa", "Stalled"));
    remove_by_prefix(top, &storage_key("Grandpa", "SetIdSession"));

    top.insert(
        storage_key("Sudo", "Key"),
        Value::String(to_hex(
            &crate::chain_spec::get_account_id_from_seed::<sp_core::sr25519::Public>(sudo).encode(),
        )),
    );

    remove_by_prefix(top, &storage_prefix("Session"));

    // The dynamic-tempo epoch scheduler anchors on absolute block numbers
    // (`LastEpochBlock + tempo`, `PendingEpochAt`). Mainnet state carries
    // mainnet-scale values (~8.6M), but the clone restarts numbering at 0, so
    // `blocks_since = current - last` saturates to 0 and no subnet would run
    // an epoch for millions of blocks. Dropping the entries resets them to
    // their ValueQuery default (0), which restores the legacy behaviour of
    // each subnet's first epoch firing within its first `tempo` blocks.
    remove_by_prefix(top, &storage_key("SubtensorModule", "LastEpochBlock"));
    remove_by_prefix(top, &storage_key("SubtensorModule", "PendingEpochAt"));

    set_validator_balances(top, validators);

    clear_top_level(spec);
    Ok(())
}

/// Insert a `System::Account` entry for each validator seed so that dev authorities
/// have enough free balance to produce transactions on the cloned chain.
fn set_validator_balances(top: &mut serde_json::Map<String, Value>, validators: &[&'static str]) {
    const FREE_BALANCE: u64 = 1_000_000_000_000_000; // 1M TAO (9 decimals)

    let prefix = frame_support::storage::storage_prefix(b"System", b"Account");

    for seed in validators {
        let account_id =
            crate::chain_spec::get_account_id_from_seed::<sp_core::sr25519::Public>(seed);
        let encoded_id = account_id.encode();

        // Blake2_128Concat = blake2_128(encoded) ++ encoded
        let hash = sp_io::hashing::blake2_128(&encoded_id);
        let mut full_key = prefix.to_vec();
        full_key.extend_from_slice(&hash);
        full_key.extend_from_slice(&encoded_id);

        let account_info = frame_system::AccountInfo {
            nonce: 0u32,
            consumers: 0u32,
            providers: 1u32, // >=1 to keep account alive
            sufficients: 0u32,
            data: pallet_balances::AccountData {
                free: FREE_BALANCE,
                ..Default::default()
            },
        };

        top.insert(
            to_hex(&full_key),
            Value::String(to_hex(&account_info.encode())),
        );
    }
}

fn remove_by_prefix(map: &mut serde_json::Map<String, Value>, prefix: &str) {
    let mut keys_to_remove = VecDeque::new();
    for key in map.keys() {
        if key.starts_with(prefix) {
            keys_to_remove.push_back(key.clone());
        }
    }
    while let Some(key) = keys_to_remove.pop_front() {
        map.remove(&key);
    }
}

fn clear_top_level(spec: &mut Value) {
    if let Some(object) = spec.as_object_mut() {
        object.insert("bootNodes".into(), json!([]));
        object.insert("codeSubstitutes".into(), json!({}));
        object.insert("chainType".into(), json!("Local"));
    }
}

fn storage_key(pallet: &str, item: &str) -> String {
    let key = frame_support::storage::storage_prefix(pallet.as_bytes(), item.as_bytes());
    to_hex(&key)
}

fn storage_prefix(pallet: &str) -> String {
    format!(
        "0x{}",
        hex::encode(sp_io::hashing::twox_128(pallet.as_bytes()))
    )
}

fn to_hex(data: &[u8]) -> String {
    format!("0x{}", hex::encode(data))
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn target_artifact_path(name: &str) -> PathBuf {
        let target_dir = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("target"));
        target_dir.join("clone-spec-tests").join(name)
    }

    fn default_cmd() -> CloneStateCmd {
        CloneStateCmd {
            chain: "finney".to_string(),
            base_path: target_artifact_path("base"),
            output: target_artifact_path("out.json"),
            sync: sc_cli::SyncMode::Warp,
            database: sc_cli::Database::ParityDb,
            rpc_port: 9966,
            port: 30466,
            sync_timeout_sec: 10,
            sync_lag_blocks: 8,
            bootnodes: Vec::new(),
            reserved_nodes: Vec::new(),
            alice: false,
            bob: false,
            charlie: false,
        }
    }

    fn make_minimal_spec() -> Value {
        let mut top = serde_json::Map::new();
        top.insert(storage_key("Grandpa", "PendingChange"), json!("0x01"));
        top.insert(storage_key("Grandpa", "NextForced"), json!("0x02"));
        top.insert(storage_key("Grandpa", "Stalled"), json!("0x03"));
        top.insert(
            format!("{}{}", storage_key("Grandpa", "SetIdSession"), "deadbeef"),
            json!("0x04"),
        );
        top.insert(format!("{}abcd", storage_prefix("Session")), json!("0x05"));
        top.insert(storage_key("Balances", "TotalIssuance"), json!("0x06"));

        json!({
            "genesis": { "raw": { "top": top } },
            "bootNodes": ["/dns4/example.com/tcp/30333/p2p/12D3KooW..."],
            "codeSubstitutes": { "0x01": "0x02" },
            "chainType": "Live"
        })
    }

    #[test]
    fn selected_validators_defaults_to_alice() {
        let cmd = default_cmd();
        let selected = selected_validators(&cmd);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected.first(), Some(&"Alice"));
    }

    #[test]
    fn selected_validators_respects_explicit_flags() {
        let mut cmd = default_cmd();
        cmd.bob = true;
        cmd.charlie = true;

        let selected = selected_validators(&cmd);
        assert_eq!(selected, vec!["Bob", "Charlie"]);
    }

    #[test]
    fn parse_u64_field_supports_u64_decimal_and_hex_string() {
        let value = json!({
            "a": 42,
            "b": "123",
            "c": "0x2a"
        });

        assert_eq!(parse_u64_field(&value, "a"), Some(42));
        assert_eq!(parse_u64_field(&value, "b"), Some(123));
        assert_eq!(parse_u64_field(&value, "c"), Some(42));
        assert_eq!(parse_u64_field(&value, "missing"), None);
    }

    #[test]
    fn patch_raw_spec_updates_authorities_sudo_and_top_level() {
        let mut spec = make_minimal_spec();
        let validators = vec!["Alice", "Bob"];
        patch_raw_spec(&mut spec, &validators).expect("patch should succeed");

        let top = spec
            .pointer("/genesis/raw/top")
            .and_then(Value::as_object)
            .expect("top should be object");

        let aura_hex = top
            .get(&storage_key("Aura", "Authorities"))
            .and_then(Value::as_str)
            .expect("aura authorities key should exist");
        let aura_raw = hex::decode(aura_hex.trim_start_matches("0x")).expect("hex decode aura");
        let expected_aura = vec![
            crate::chain_spec::authority_keys_from_seed("Alice").0,
            crate::chain_spec::authority_keys_from_seed("Bob").0,
        ]
        .encode();
        assert_eq!(aura_raw, expected_aura);

        let sudo_hex = top
            .get(&storage_key("Sudo", "Key"))
            .and_then(Value::as_str)
            .expect("sudo key should exist");
        assert_eq!(
            sudo_hex,
            to_hex(
                &crate::chain_spec::get_account_id_from_seed::<sp_core::sr25519::Public>("Alice")
                    .encode()
            )
            .as_str()
        );

        assert!(!top.contains_key(&storage_key("Grandpa", "PendingChange")));
        assert!(!top.contains_key(&storage_key("Grandpa", "NextForced")));
        assert!(!top.contains_key(&storage_key("Grandpa", "Stalled")));
        assert!(
            top.keys()
                .all(|k| !k.starts_with(&storage_prefix("Session")))
        );

        assert_eq!(spec.get("chainType"), Some(&json!("Local")));
        assert_eq!(spec.get("bootNodes"), Some(&json!([])));
        assert_eq!(spec.get("codeSubstitutes"), Some(&json!({})));
    }

    #[test]
    fn patch_raw_spec_fails_when_top_missing() {
        let mut spec = json!({});
        let err = patch_raw_spec(&mut spec, &["Alice"]).expect_err("must fail");
        assert!(
            err.to_string()
                .contains("missing or invalid genesis.raw.top"),
            "unexpected error: {err}"
        );
    }
}
