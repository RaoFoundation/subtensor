//! Native chain access for the Rust SDK.
//!
//! The codec/runtime modules deliberately remain transport agnostic. `Client`
//! is the small blocking JSON-RPC layer that turns their metadata-driven SCALE
//! primitives into pinned reads, signed submissions, receipts, and block
//! streams. It uses the crate's existing rustls-backed `reqwest` dependency, so
//! the native SDK adds no second async runtime and works in ordinary Rust tests,
//! command-line programs, and foreign-language bindings alike.

#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use codec::Decode;
use reqwest::blocking::Client as HttpClient;
use serde_json::{json, Value as JsonValue};

use crate::codec::extrinsic::{era_birth, TxParams};
use crate::codec::value::Value;
use crate::error::CoreError;
use crate::keys::Keypair;
use crate::mlkem;
use crate::runtime::type_string::TypeSpec;
use crate::runtime::{Runtime, RuntimeApiMethodInfo, StorageInfo};

const DEFAULT_RPC_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_RECEIPT_TIMEOUT: Duration = Duration::from_secs(120);
const DEFAULT_ERA_PERIOD: u64 = 64;
const STORAGE_PAGE_SIZE: u64 = 1_000;
const RAO_PER_TAO: u128 = 1_000_000_000;

/// A decoded block header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockHeader {
    pub hash: String,
    pub parent_hash: String,
    pub number: u64,
}

/// One subnet's small, commonly used read model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubnetInfo {
    pub netuid: u16,
    pub tempo: u16,
    pub burn_rao: u128,
    pub neuron_count: u16,
}

/// Swap simulation returned by the runtime API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwapQuote {
    pub tao_amount: u128,
    pub alpha_amount: u128,
    pub tao_fee: u128,
    pub alpha_fee: u128,
    pub tao_slippage: u128,
    pub alpha_slippage: u128,
}

/// A normalized dispatch failure. `semantic_code` is stable across wording
/// changes and is suitable for branching in callers and e2e assertions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchError {
    pub pallet: Option<String>,
    pub name: String,
    pub docs: Vec<String>,
    pub semantic_code: String,
}

impl std::fmt::Display for DispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.pallet {
            Some(pallet) => write!(f, "{pallet}.{}", self.name),
            None => write!(f, "{}", self.name),
        }
    }
}

/// Inclusion/finalization result for a submitted extrinsic.
#[derive(Debug, Clone)]
pub struct TxOutcome {
    pub success: bool,
    pub extrinsic_hash: String,
    pub block_hash: Option<String>,
    pub block_number: Option<u64>,
    pub extrinsic_index: Option<u32>,
    pub fee_rao: Option<u128>,
    pub events: Vec<Value>,
    pub error: Option<DispatchError>,
    pub message: String,
    pub data: BTreeMap<String, Value>,
}

impl TxOutcome {
    fn pool_rejection(hash: String, message: String) -> Self {
        Self {
            success: false,
            extrinsic_hash: hash,
            block_hash: None,
            block_number: None,
            extrinsic_index: None,
            fee_rao: None,
            events: Vec::new(),
            error: None,
            message,
            data: BTreeMap::new(),
        }
    }
}

// Reorg-safe receipt tracking states. An inclusion is only final when the exact
// inclusion block is still canonical after finality reaches its height.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InclusionFinalization {
    Finalized,
    Reorged,
}

fn classify_inclusion_finalization(
    inclusion_hash: &str,
    canonical_hash: Option<&str>,
    included_at: u64,
    finalized_at: u64,
) -> Option<InclusionFinalization> {
    match canonical_hash {
        Some(canonical) if canonical.eq_ignore_ascii_case(inclusion_hash) => {
            (finalized_at >= included_at).then_some(InclusionFinalization::Finalized)
        }
        _ => Some(InclusionFinalization::Reorged),
    }
}

/// The native Bittensor chain client.
pub struct Client {
    endpoint: String,
    http: HttpClient,
    next_id: AtomicU64,
    runtime: RwLock<Arc<Runtime>>,
    genesis_hash: [u8; 32],
    ss58_format: u16,
}

impl Client {
    /// Connect to a Substrate JSON-RPC endpoint. Websocket URLs are accepted for
    /// compatibility and are mapped to HTTP on the same host/port.
    pub fn connect(endpoint: impl Into<String>) -> Result<Self, CoreError> {
        let endpoint = http_endpoint(&endpoint.into());
        let http = HttpClient::builder()
            .timeout(DEFAULT_RPC_TIMEOUT)
            .build()
            .map_err(|error| CoreError::Rpc(format!("cannot build HTTP client: {error}")))?;
        let bootstrap = RpcBootstrap {
            endpoint: endpoint.clone(),
            http: http.clone(),
            next_id: AtomicU64::new(1),
        };
        let ss58_format = bootstrap.ss58_format()?;
        let version = bootstrap.runtime_version()?;
        let metadata = bootstrap.metadata()?;
        let genesis_hash = parse_h256(&bootstrap.block_hash(Some(0))?)?;
        let runtime = Runtime::parse(
            &metadata,
            version.spec_version,
            version.transaction_version,
            ss58_format,
        )?;
        Ok(Self {
            endpoint,
            http,
            next_id: AtomicU64::new(100),
            runtime: RwLock::new(Arc::new(runtime)),
            genesis_hash,
            ss58_format,
        })
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Stable names of the typed read helpers supplied by the native SDK.
    /// Generic [`Client::query`], [`Client::query_map`], and
    /// [`Client::runtime_call`] remain available for the full live metadata
    /// surface.
    pub fn read_catalog(&self) -> &'static [&'static str] {
        &[
            "balance",
            "existential_deposit",
            "subnets",
            "metagraph",
            "neurons",
            "subnet_hyperparameters",
            "stake",
            "quote_stake",
            "block_number",
            "block_time",
            "lease",
            "leases",
            "mev_shield_next_key",
            "proxies",
            "multisig",
            "identity",
            "subnet_identity",
            "children",
            "root_claim_type",
            "auto_stake",
        ]
    }

    pub fn ss58_format(&self) -> u16 {
        self.ss58_format
    }

    pub fn genesis_hash(&self) -> [u8; 32] {
        self.genesis_hash
    }

    pub fn runtime(&self) -> Result<Arc<Runtime>, CoreError> {
        self.runtime
            .read()
            .map_err(|_| CoreError::Rpc("runtime metadata lock is poisoned".into()))
            .map(|runtime| Arc::clone(&runtime))
    }

    /// Refresh metadata after a runtime upgrade. Returns true when the cached
    /// runtime changed.
    pub fn refresh_runtime(&self) -> Result<bool, CoreError> {
        let version = self.runtime_version()?;
        let current = self.runtime()?;
        if current.spec_version == version.spec_version
            && current.transaction_version == version.transaction_version
        {
            return Ok(false);
        }
        let metadata = self.metadata()?;
        let next = Runtime::parse(
            &metadata,
            version.spec_version,
            version.transaction_version,
            self.ss58_format,
        )?;
        let mut guard = self
            .runtime
            .write()
            .map_err(|_| CoreError::Rpc("runtime metadata lock is poisoned".into()))?;
        *guard = Arc::new(next);
        Ok(true)
    }

    /// Escape hatch for node RPCs not yet wrapped by the SDK.
    pub fn rpc_value(&self, method: &str, params: JsonValue) -> Result<JsonValue, CoreError> {
        rpc_request(
            &self.http,
            &self.endpoint,
            self.next_id.fetch_add(1, Ordering::Relaxed),
            method,
            params,
        )
    }

    fn runtime_version(&self) -> Result<RuntimeVersion, CoreError> {
        parse_runtime_version(self.rpc_value("state_getRuntimeVersion", json!([]))?)
    }

    fn metadata(&self) -> Result<Vec<u8>, CoreError> {
        fetch_metadata(|method, params| self.rpc_value(method, params))
    }

    pub fn block_hash(&self, block: Option<u64>) -> Result<String, CoreError> {
        let params = block.map_or_else(|| json!([]), |number| json!([number]));
        let value = self.rpc_value("chain_getBlockHash", params)?;
        Ok(json_string(&value, "chain_getBlockHash result")?.to_string())
    }

    fn canonical_block_hash(&self, block: u64) -> Result<Option<String>, CoreError> {
        let value = self.rpc_value("chain_getBlockHash", json!([block]))?;
        match value {
            JsonValue::Null => Ok(None),
            JsonValue::String(hash) => Ok(Some(hash)),
            other => Err(CoreError::Rpc(format!(
                "chain_getBlockHash returned {other}, expected a hash or null"
            ))),
        }
    }

    pub fn finalized_head(&self) -> Result<String, CoreError> {
        let value = self.rpc_value("chain_getFinalizedHead", json!([]))?;
        Ok(json_string(&value, "chain_getFinalizedHead result")?.to_string())
    }

    pub fn header(&self, block_hash: Option<&str>) -> Result<BlockHeader, CoreError> {
        let params = block_hash.map_or_else(|| json!([]), |hash| json!([hash]));
        let value = self.rpc_value("chain_getHeader", params)?;
        let (number, parent_hash) = parse_header_fields(&value)?;
        let hash = match block_hash {
            Some(hash) => hash.to_string(),
            None => self.block_hash(Some(number))?,
        };
        Ok(BlockHeader {
            hash,
            parent_hash,
            number,
        })
    }

    /// Best-head poll for [`BlockStream`]: fetch the canonical hash (a second
    /// RPC call) only when the head has advanced past `last`.
    fn best_header_past(&self, last: Option<u64>) -> Result<Option<BlockHeader>, CoreError> {
        let value = self.rpc_value("chain_getHeader", json!([]))?;
        let (number, parent_hash) = parse_header_fields(&value)?;
        if last.is_some_and(|last| number <= last) {
            return Ok(None);
        }
        let hash = self.block_hash(Some(number))?;
        Ok(Some(BlockHeader {
            hash,
            parent_hash,
            number,
        }))
    }

    pub fn block_number(&self) -> Result<u64, CoreError> {
        self.header(None).map(|header| header.number)
    }

    pub fn block_time(&self) -> Result<Duration, CoreError> {
        let millis = as_u128(&self.constant("Aura", "SlotDuration")?)
            .ok_or_else(|| CoreError::Codec("Aura.SlotDuration is not an integer".into()))?;
        let millis = u64::try_from(millis)
            .map_err(|_| CoreError::Codec("Aura.SlotDuration does not fit u64".into()))?;
        Ok(Duration::from_millis(millis.max(1)))
    }

    /// Polling block stream. HTTP and websocket endpoints therefore expose the
    /// same API; callers do not need an async runtime merely to follow heads.
    ///
    /// The poll interval adapts to the chain's slot duration (a quarter of a
    /// slot, clamped to 50ms..3s): an idle mainnet follower costs the node
    /// about one cheap RPC call every 3 seconds, while fast-blocks local
    /// chains are still polled every ~62ms.
    pub fn blocks(&self, finalized: bool) -> BlockStream<'_> {
        let poll_interval = (self.block_time().unwrap_or_else(|_| Duration::from_secs(1)) / 4)
            .clamp(Duration::from_millis(50), Duration::from_secs(3));
        BlockStream {
            client: self,
            finalized,
            last_number: None,
            last_finalized_hash: None,
            poll_interval,
        }
    }

    pub fn at(&self, block: Option<u64>) -> Result<Snapshot<'_>, CoreError> {
        let hash = self.block_hash(block)?;
        let number = self.header(Some(&hash))?.number;
        Ok(Snapshot {
            client: self,
            block_hash: hash,
            block_number: number,
        })
    }

    pub fn compose_call(
        &self,
        pallet: &str,
        function: &str,
        params: &Value,
    ) -> Result<Vec<u8>, CoreError> {
        self.runtime()?.compose_call(pallet, function, params)
    }

    pub fn decode_call(&self, data: &[u8]) -> Result<Value, CoreError> {
        self.runtime()?.decode_spec(&TypeSpec::Call, data, true)
    }

    pub fn decode_scale(&self, type_name: &str, data: &[u8]) -> Result<Value, CoreError> {
        let runtime = self.runtime()?;
        if type_name == "Call" {
            return runtime.decode_spec(&TypeSpec::Call, data, true);
        }
        let type_id = runtime
            .type_id_of(type_name)
            .ok_or_else(|| CoreError::NotInRuntime(format!("type {type_name}")))?;
        runtime.decode_spec(&TypeSpec::Id(type_id), data, true)
    }

    pub fn constant(&self, pallet: &str, name: &str) -> Result<Value, CoreError> {
        let runtime = self.runtime()?;
        let info = runtime
            .constant(pallet, name)
            .cloned()
            .ok_or_else(|| CoreError::NotInRuntime(format!("constant {pallet}.{name}")))?;
        runtime.decode_spec(&TypeSpec::Id(info.ty), &info.value, true)
    }

    pub fn query(
        &self,
        pallet: &str,
        storage: &str,
        params: &[Value],
        block_hash: Option<&str>,
    ) -> Result<Value, CoreError> {
        let runtime = self.runtime()?;
        let info = storage_info(&runtime, pallet, storage)?;
        let key = runtime.storage_key(&info, params)?;
        let raw = self.storage_raw(&key, block_hash)?;
        decode_storage(&runtime, &info, raw.as_deref())
    }

    pub fn query_batch(
        &self,
        pallet: &str,
        storage: &str,
        param_sets: &[Vec<Value>],
        block_hash: Option<&str>,
    ) -> Result<Vec<Value>, CoreError> {
        if param_sets.is_empty() {
            return Ok(Vec::new());
        }
        let runtime = self.runtime()?;
        let info = storage_info(&runtime, pallet, storage)?;
        let mut keys = Vec::with_capacity(param_sets.len());
        for params in param_sets {
            keys.push(runtime.storage_key(&info, params)?);
        }
        let values = self.storage_batch_raw(&keys, block_hash)?;
        values
            .iter()
            .map(|raw| decode_storage(&runtime, &info, raw.as_deref()))
            .collect()
    }

    pub fn query_map(
        &self,
        pallet: &str,
        storage: &str,
        fixed_params: &[Value],
        block_hash: Option<&str>,
    ) -> Result<Vec<(Value, Value)>, CoreError> {
        let runtime = self.runtime()?;
        let info = storage_info(&runtime, pallet, storage)?;
        if fixed_params.len() > info.key_types.len() {
            return Err(CoreError::Codec(format!(
                "{pallet}.{storage} has {} key parts, got {} fixed params",
                info.key_types.len(),
                fixed_params.len()
            )));
        }
        let prefix = runtime.storage_key(&info, fixed_params)?;
        let keys = self.storage_keys_paged(&prefix, block_hash)?;
        let raws = self.storage_batch_raw(&keys, block_hash)?;
        let mut rows = Vec::with_capacity(keys.len());
        for (key, raw) in keys.iter().zip(raws.iter()) {
            let decoded_key = runtime.decode_storage_key_params(&info, key, fixed_params.len())?;
            let key_value = match decoded_key.as_slice() {
                [] => Value::Tuple(Vec::new()),
                [one] => one.clone(),
                many => Value::Tuple(many.to_vec()),
            };
            rows.push((key_value, decode_storage(&runtime, &info, raw.as_deref())?));
        }
        Ok(rows)
    }

    pub fn runtime_call(
        &self,
        api: &str,
        method: &str,
        params: &[Value],
        block_hash: Option<&str>,
    ) -> Result<Value, CoreError> {
        let runtime = self.runtime()?;
        let method_info = runtime_api_method(&runtime, api, method)?.clone();
        if method_info.inputs.len() != params.len() {
            return Err(CoreError::Codec(format!(
                "{api}.{method} expects {} params, got {}",
                method_info.inputs.len(),
                params.len()
            )));
        }
        let mut encoded = Vec::new();
        for (input, value) in method_info.inputs.iter().zip(params.iter()) {
            runtime.encode_id(input.ty, value, &mut encoded)?;
        }
        let mut rpc_params = vec![
            JsonValue::String(format!("{api}_{method}")),
            JsonValue::String(hex_prefixed(&encoded)),
        ];
        if let Some(hash) = block_hash {
            rpc_params.push(JsonValue::String(hash.to_string()));
        }
        let result = self.rpc_value("state_call", JsonValue::Array(rpc_params))?;
        let bytes = decode_hex(json_string(&result, "state_call result")?)?;
        runtime.decode_spec(&TypeSpec::Id(method_info.output), &bytes, true)
    }

    pub fn account_next_index(&self, address: &str) -> Result<u64, CoreError> {
        let value = self.rpc_value("system_accountNextIndex", json!([address]))?;
        json_u64(&value)
    }

    /// Sign without submitting. Returns `(encoded extrinsic, 0x hash)`.
    pub fn sign_extrinsic(
        &self,
        call_data: &[u8],
        signer: &Keypair,
        nonce: u64,
        period: Option<u64>,
    ) -> Result<(Vec<u8>, String), CoreError> {
        let runtime = self.runtime()?;
        let current = self.block_number()?;
        let (era, era_block_hash) = match period {
            Some(period) if period > 0 => {
                let birth = era_birth(period, current);
                let hash = parse_h256(&self.block_hash(Some(birth))?)?;
                (
                    Value::record(vec![
                        ("period".into(), Value::Uint(u128::from(period))),
                        ("current".into(), Value::Uint(u128::from(current))),
                    ]),
                    hash,
                )
            }
            _ => (Value::str("00"), self.genesis_hash),
        };
        let params = TxParams {
            era,
            nonce,
            tip: 0,
            tip_asset_id: None,
            genesis_hash: self.genesis_hash,
            era_block_hash,
            metadata_hash: None,
        };
        let payload = runtime.signature_payload(call_data, &params)?;
        let signature = signer.sign(&payload)?;
        let (extrinsic, hash) = runtime.encode_signed_extrinsic(
            call_data,
            signer.public_key_bytes(),
            &signature,
            signer.crypto_type(),
            &params,
        )?;
        Ok((extrinsic, hex_prefixed(&hash)))
    }

    pub fn estimate_fee(&self, call_data: &[u8], signer: &Keypair) -> Result<u128, CoreError> {
        let nonce = self.account_next_index(&signer.ss58_address())?;
        let (extrinsic, _) =
            self.sign_extrinsic(call_data, signer, nonce, Some(DEFAULT_ERA_PERIOD))?;
        let value = self.rpc_value("payment_queryInfo", json!([hex_prefixed(&extrinsic)]))?;
        let object = value
            .as_object()
            .ok_or_else(|| CoreError::Rpc("payment_queryInfo returned a non-object".into()))?;
        let fee = object
            .get("partialFee")
            .or_else(|| object.get("partial_fee"))
            .ok_or_else(|| CoreError::Rpc("payment_queryInfo omitted partialFee".into()))?;
        json_u128(fee)
    }

    pub fn estimate_weight(&self, call_data: &[u8], signer: &Keypair) -> Result<Value, CoreError> {
        let nonce = self.account_next_index(&signer.ss58_address())?;
        let (extrinsic, _) =
            self.sign_extrinsic(call_data, signer, nonce, Some(DEFAULT_ERA_PERIOD))?;
        let value = self.rpc_value("payment_queryInfo", json!([hex_prefixed(&extrinsic)]))?;
        let weight = value
            .as_object()
            .and_then(|object| object.get("weight"))
            .ok_or_else(|| CoreError::Rpc("payment_queryInfo omitted weight".into()))?;
        json_to_value(weight)
    }

    pub fn submit(
        &self,
        call_data: &[u8],
        signer: &Keypair,
        nonce: Option<u64>,
        period: Option<u64>,
        wait_for_finalization: bool,
    ) -> Result<TxOutcome, CoreError> {
        let nonce = match nonce {
            Some(nonce) => nonce,
            None => self.account_next_index(&signer.ss58_address())?,
        };
        let (extrinsic, hash) = self.sign_extrinsic(call_data, signer, nonce, period)?;
        self.submit_encoded(&extrinsic, hash, wait_for_finalization)
    }

    pub fn submit_encoded(
        &self,
        extrinsic: &[u8],
        expected_hash: String,
        wait_for_finalization: bool,
    ) -> Result<TxOutcome, CoreError> {
        let start_block = self.block_number()?;
        let xt_hex = hex_prefixed(extrinsic);
        let submitted = match self.rpc_value("author_submitExtrinsic", json!([xt_hex])) {
            Ok(value) => value,
            Err(error) => {
                return Ok(TxOutcome::pool_rejection(expected_hash, error.to_string()));
            }
        };
        let submitted_hash = submitted
            .as_str()
            .map_or(expected_hash, ToString::to_string);
        self.wait_for_inclusion(
            extrinsic,
            submitted_hash,
            start_block,
            wait_for_finalization,
            DEFAULT_RECEIPT_TIMEOUT,
        )
    }

    /// Full MEV-shield pipeline: sign the inner call at nonce+1, seal it with
    /// the current ML-KEM-768 key, then submit the carrier at nonce.
    pub fn submit_shielded(
        &self,
        call_data: &[u8],
        signer: &Keypair,
        wait_for_finalization: bool,
    ) -> Result<TxOutcome, CoreError> {
        let next_key = self.query("MevShield", "NextKey", &[], None)?;
        let public_key = value_bytes(&next_key)
            .ok_or_else(|| CoreError::Rpc("MevShield.NextKey is unavailable".into()))?;
        let nonce = self.account_next_index(&signer.ss58_address())?;
        let (inner, inner_hash) =
            self.sign_extrinsic(call_data, signer, nonce + 1, Some(DEFAULT_ERA_PERIOD))?;
        let ciphertext = mlkem::seal(&public_key, &inner, true)?;
        let outer = self.compose_call(
            "MevShield",
            "submit_encrypted",
            &Value::record(vec![("ciphertext".into(), Value::Bytes(ciphertext))]),
        )?;
        let mut result = self.submit(
            &outer,
            signer,
            Some(nonce),
            Some(DEFAULT_ERA_PERIOD),
            wait_for_finalization,
        )?;
        if result.success {
            result.data.insert("shielded".into(), Value::Bool(true));
            result
                .data
                .insert("inner_extrinsic_hash".into(), Value::str(inner_hash));
        }
        Ok(result)
    }

    pub fn balance_rao(&self, address: &str) -> Result<u128, CoreError> {
        let account = self.query("System", "Account", &[Value::str(address)], None)?;
        field(&account, "data")
            .and_then(|data| field(data, "free"))
            .and_then(as_u128)
            .ok_or_else(|| CoreError::Codec("System.Account.data.free is missing".into()))
    }

    pub fn existential_deposit_rao(&self) -> Result<u128, CoreError> {
        as_u128(&self.constant("Balances", "ExistentialDeposit")?)
            .ok_or_else(|| CoreError::Codec("ExistentialDeposit is not an integer".into()))
    }

    pub fn subnets(&self, block_hash: Option<&str>) -> Result<Vec<SubnetInfo>, CoreError> {
        let added = self.query_map("SubtensorModule", "NetworksAdded", &[], block_hash)?;
        let tempos = value_map_u16(self.query_map("SubtensorModule", "Tempo", &[], block_hash)?);
        let burns = value_map_u128(self.query_map("SubtensorModule", "Burn", &[], block_hash)?);
        let counts =
            value_map_u16(self.query_map("SubtensorModule", "SubnetworkN", &[], block_hash)?);
        let mut out = Vec::new();
        for (key, value) in added {
            if !matches!(value, Value::Bool(true)) {
                continue;
            }
            let Some(netuid) = as_u128(&key).and_then(|n| u16::try_from(n).ok()) else {
                continue;
            };
            out.push(SubnetInfo {
                netuid,
                tempo: tempos.get(&netuid).copied().unwrap_or(0),
                burn_rao: burns.get(&netuid).copied().unwrap_or(0),
                neuron_count: counts.get(&netuid).copied().unwrap_or(0),
            });
        }
        out.sort_by_key(|subnet| subnet.netuid);
        Ok(out)
    }

    pub fn metagraph(&self, netuid: u16, block_hash: Option<&str>) -> Result<Value, CoreError> {
        self.runtime_call(
            "SubnetInfoRuntimeApi",
            "get_metagraph",
            &[Value::Uint(u128::from(netuid))],
            block_hash,
        )
    }

    /// Typed fast-path for the runtime's lite-neuron collection.
    pub fn neurons(&self, netuid: u16, block_hash: Option<&str>) -> Result<Vec<Value>, CoreError> {
        match self.runtime_call(
            "NeuronInfoRuntimeApi",
            "get_neurons_lite",
            &[Value::Uint(u128::from(netuid))],
            block_hash,
        )? {
            Value::List(neurons) => Ok(neurons),
            other => Err(CoreError::Codec(format!(
                "NeuronInfoRuntimeApi.get_neurons_lite returned {other:?}, expected a list"
            ))),
        }
    }

    pub fn subnet_hyperparameters(
        &self,
        netuid: u16,
        block_hash: Option<&str>,
    ) -> Result<Value, CoreError> {
        let entries = self.runtime_call(
            "SubnetInfoRuntimeApi",
            "get_subnet_hyperparams_v3",
            &[Value::Uint(u128::from(netuid))],
            block_hash,
        )?;
        flatten_subnet_hyperparams_v3(entries)
    }

    pub fn stake_rao(
        &self,
        coldkey: &str,
        hotkey: &str,
        netuid: u16,
        block_hash: Option<&str>,
    ) -> Result<u128, CoreError> {
        let info = self.runtime_call(
            "StakeInfoRuntimeApi",
            "get_stake_info_for_hotkey_coldkey_netuid",
            &[
                Value::str(hotkey),
                Value::str(coldkey),
                Value::Uint(u128::from(netuid)),
            ],
            block_hash,
        )?;
        if matches!(info, Value::Null) {
            return Ok(0);
        }
        field(&info, "stake")
            .and_then(as_u128)
            .ok_or_else(|| CoreError::Codec("stake runtime API omitted stake".into()))
    }

    pub fn quote_stake(
        &self,
        netuid: u16,
        amount_rao: u128,
        block_hash: Option<&str>,
    ) -> Result<SwapQuote, CoreError> {
        let quote = self.runtime_call(
            "SwapRuntimeApi",
            "sim_swap_tao_for_alpha",
            &[Value::Uint(u128::from(netuid)), Value::Uint(amount_rao)],
            block_hash,
        )?;
        Ok(SwapQuote {
            tao_amount: required_u128(&quote, "tao_amount")?,
            alpha_amount: required_u128(&quote, "alpha_amount")?,
            tao_fee: required_u128(&quote, "tao_fee")?,
            alpha_fee: required_u128(&quote, "alpha_fee")?,
            tao_slippage: required_u128(&quote, "tao_slippage")?,
            alpha_slippage: required_u128(&quote, "alpha_slippage")?,
        })
    }

    pub fn tao(value: u128) -> Result<u128, CoreError> {
        value
            .checked_mul(RAO_PER_TAO)
            .ok_or_else(|| CoreError::Codec("TAO amount overflows u128".into()))
    }

    fn storage_raw(
        &self,
        key: &[u8],
        block_hash: Option<&str>,
    ) -> Result<Option<String>, CoreError> {
        let mut params = vec![JsonValue::String(hex_prefixed(key))];
        if let Some(hash) = block_hash {
            params.push(JsonValue::String(hash.to_string()));
        }
        let value = self.rpc_value("state_getStorage", JsonValue::Array(params))?;
        match value {
            JsonValue::Null => Ok(None),
            JsonValue::String(raw) => Ok(Some(raw)),
            _ => Err(CoreError::Rpc(
                "state_getStorage returned neither hex nor null".into(),
            )),
        }
    }

    fn storage_keys_paged(
        &self,
        prefix: &[u8],
        block_hash: Option<&str>,
    ) -> Result<Vec<Vec<u8>>, CoreError> {
        let prefix_hex = hex_prefixed(prefix);
        let mut all = Vec::new();
        let mut start: Option<String> = None;
        loop {
            let mut params = vec![
                JsonValue::String(prefix_hex.clone()),
                JsonValue::from(STORAGE_PAGE_SIZE),
                start
                    .as_ref()
                    .map_or(JsonValue::Null, |key| JsonValue::String(key.clone())),
            ];
            if let Some(hash) = block_hash {
                params.push(JsonValue::String(hash.to_string()));
            }
            let value = self.rpc_value("state_getKeysPaged", JsonValue::Array(params))?;
            let page = value
                .as_array()
                .ok_or_else(|| CoreError::Rpc("state_getKeysPaged returned a non-array".into()))?;
            if page.is_empty() {
                break;
            }
            let mut last = None;
            for item in page {
                let key = json_string(item, "storage key")?.to_string();
                all.push(decode_hex(&key)?);
                last = Some(key);
            }
            if page.len() < usize::try_from(STORAGE_PAGE_SIZE).unwrap_or(usize::MAX) {
                break;
            }
            if last == start {
                return Err(CoreError::Rpc(
                    "state_getKeysPaged repeated its cursor".into(),
                ));
            }
            start = last;
        }
        Ok(all)
    }

    fn storage_batch_raw(
        &self,
        keys: &[Vec<u8>],
        block_hash: Option<&str>,
    ) -> Result<Vec<Option<String>>, CoreError> {
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        let key_hex: Vec<String> = keys.iter().map(|key| hex_prefixed(key)).collect();
        let mut params = vec![json!(key_hex)];
        if let Some(hash) = block_hash {
            params.push(JsonValue::String(hash.to_string()));
        }
        let value = self.rpc_value("state_queryStorageAt", JsonValue::Array(params))?;
        let sets = value
            .as_array()
            .ok_or_else(|| CoreError::Rpc("state_queryStorageAt returned a non-array".into()))?;
        let changes = sets
            .first()
            .and_then(JsonValue::as_object)
            .and_then(|set| set.get("changes"))
            .and_then(JsonValue::as_array)
            .ok_or_else(|| CoreError::Rpc("state_queryStorageAt omitted changes".into()))?;
        let mut by_key: HashMap<String, Option<String>> = HashMap::new();
        for change in changes {
            let pair = change
                .as_array()
                .ok_or_else(|| CoreError::Rpc("storage change is not a pair".into()))?;
            let key = pair
                .first()
                .and_then(JsonValue::as_str)
                .ok_or_else(|| CoreError::Rpc("storage change has no key".into()))?;
            let raw = pair
                .get(1)
                .and_then(JsonValue::as_str)
                .map(ToString::to_string);
            by_key.insert(key.to_ascii_lowercase(), raw);
        }
        Ok(keys
            .iter()
            .map(|key| {
                by_key
                    .remove(&hex_prefixed(key).to_ascii_lowercase())
                    .flatten()
            })
            .collect())
    }

    fn wait_for_inclusion(
        &self,
        extrinsic: &[u8],
        hash: String,
        start_block: u64,
        wait_for_finalization: bool,
        timeout: Duration,
    ) -> Result<TxOutcome, CoreError> {
        let deadline = Instant::now() + timeout;
        let xt_hex = hex_prefixed(extrinsic).to_ascii_lowercase();
        let mut next_block = start_block;
        let poll = self
            .block_time()
            .unwrap_or_else(|_| Duration::from_millis(250))
            .min(Duration::from_secs(1));
        'track_inclusion: while Instant::now() < deadline {
            let head = self.block_number()?;
            while next_block <= head {
                let included_at = next_block;
                let block_hash = self.block_hash(Some(included_at))?;
                if let Some(index) = self.find_extrinsic(&block_hash, &xt_hex)? {
                    if wait_for_finalization {
                        match self.wait_until_finalized(&block_hash, included_at, deadline, poll)? {
                            InclusionFinalization::Finalized => {}
                            InclusionFinalization::Reorged => {
                                // The old inclusion block is no longer canonical.
                                // Rescan from its height so a re-inclusion at the
                                // same or a later height can still produce a receipt.
                                next_block = included_at;
                                continue 'track_inclusion;
                            }
                        }
                    }
                    return self.decode_outcome(hash, block_hash, included_at, index);
                }
                next_block = next_block.saturating_add(1);
            }
            thread::sleep(poll);
        }
        Ok(TxOutcome::pool_rejection(
            hash,
            format!("extrinsic was not included within {}s", timeout.as_secs()),
        ))
    }

    fn find_extrinsic(&self, block_hash: &str, wanted: &str) -> Result<Option<u32>, CoreError> {
        let block = self.rpc_value("chain_getBlock", json!([block_hash]))?;
        let extrinsics = block
            .as_object()
            .and_then(|object| object.get("block"))
            .and_then(JsonValue::as_object)
            .and_then(|inner| inner.get("extrinsics"))
            .and_then(JsonValue::as_array)
            .ok_or_else(|| CoreError::Rpc("chain_getBlock omitted extrinsics".into()))?;
        for (index, extrinsic) in extrinsics.iter().enumerate() {
            if extrinsic
                .as_str()
                .is_some_and(|raw| raw.eq_ignore_ascii_case(wanted))
            {
                return u32::try_from(index)
                    .map(Some)
                    .map_err(|_| CoreError::Rpc("extrinsic index exceeds u32".into()));
            }
        }
        Ok(None)
    }

    fn wait_until_finalized(
        &self,
        inclusion_hash: &str,
        included_at: u64,
        deadline: Instant,
        poll: Duration,
    ) -> Result<InclusionFinalization, CoreError> {
        while Instant::now() < deadline {
            // Fetch finality first, then the canonical hash at the inclusion
            // height. Once finality has reached that height, the best chain must
            // contain the same finalized ancestor at that height.
            let finalized = self.finalized_head()?;
            let finalized_at = self.header(Some(&finalized))?.number;
            let canonical_hash = self.canonical_block_hash(included_at)?;
            if let Some(status) = classify_inclusion_finalization(
                inclusion_hash,
                canonical_hash.as_deref(),
                included_at,
                finalized_at,
            ) {
                return Ok(status);
            }
            thread::sleep(poll);
        }
        Err(CoreError::Rpc(format!(
            "block {included_at} ({inclusion_hash}) was included but not finalized before the receipt timeout"
        )))
    }

    fn decode_outcome(
        &self,
        extrinsic_hash: String,
        block_hash: String,
        block_number: u64,
        extrinsic_index: u32,
    ) -> Result<TxOutcome, CoreError> {
        let records = self.query("System", "Events", &[], Some(&block_hash))?;
        let mut events = Vec::new();
        if let Value::List(items) = records {
            for item in items {
                if field(&item, "extrinsic_idx")
                    .and_then(as_u128)
                    .is_some_and(|index| index == u128::from(extrinsic_index))
                {
                    events.push(item);
                }
            }
        }
        let runtime = self.runtime()?;
        let mut success = events
            .iter()
            .any(|event| event_is(event, "System", "ExtrinsicSuccess"));
        let mut error = events
            .iter()
            .find(|event| event_is(event, "System", "ExtrinsicFailed"))
            .and_then(event_attributes)
            .and_then(|attributes| field(attributes, "dispatch_error").or(Some(attributes)))
            .map(|value| dispatch_error(&runtime, value))
            .transpose()?;

        if error.is_none() {
            for (module, event_name) in [
                ("Proxy", "ProxyExecuted"),
                ("Sudo", "Sudid"),
                ("Multisig", "MultisigExecuted"),
            ] {
                let Some(attributes) = events
                    .iter()
                    .find(|event| event_is(event, module, event_name))
                    .and_then(event_attributes)
                else {
                    continue;
                };
                let result = field(attributes, "result").unwrap_or(attributes);
                if let Some(inner) = variant(result, "Err") {
                    error = Some(dispatch_error(&runtime, inner)?);
                    success = false;
                    break;
                }
            }
        }

        let fee_rao = events
            .iter()
            .find(|event| event_is(event, "TransactionPayment", "TransactionFeePaid"))
            .and_then(event_attributes)
            .and_then(|attributes| field(attributes, "actual_fee"))
            .and_then(as_u128);
        let message = match &error {
            Some(error) => error.to_string(),
            None if success => "extrinsic succeeded".into(),
            None => "extrinsic did not emit System.ExtrinsicSuccess".into(),
        };
        Ok(TxOutcome {
            success,
            extrinsic_hash,
            block_hash: Some(block_hash),
            block_number: Some(block_number),
            extrinsic_index: Some(extrinsic_index),
            fee_rao,
            events,
            error,
            message,
            data: BTreeMap::new(),
        })
    }
}

/// A read-only view pinned to one block hash.
pub struct Snapshot<'a> {
    client: &'a Client,
    pub block_hash: String,
    pub block_number: u64,
}

impl Snapshot<'_> {
    pub fn query(&self, pallet: &str, storage: &str, params: &[Value]) -> Result<Value, CoreError> {
        self.client
            .query(pallet, storage, params, Some(&self.block_hash))
    }

    pub fn query_map(
        &self,
        pallet: &str,
        storage: &str,
        fixed_params: &[Value],
    ) -> Result<Vec<(Value, Value)>, CoreError> {
        self.client
            .query_map(pallet, storage, fixed_params, Some(&self.block_hash))
    }

    pub fn runtime_call(
        &self,
        api: &str,
        method: &str,
        params: &[Value],
    ) -> Result<Value, CoreError> {
        self.client
            .runtime_call(api, method, params, Some(&self.block_hash))
    }

    pub fn balance_rao(&self, address: &str) -> Result<u128, CoreError> {
        let account = self.query("System", "Account", &[Value::str(address)])?;
        field(&account, "data")
            .and_then(|data| field(data, "free"))
            .and_then(as_u128)
            .ok_or_else(|| CoreError::Codec("System.Account.data.free is missing".into()))
    }
}

/// Iterator returned by [`Client::blocks`].
pub struct BlockStream<'a> {
    client: &'a Client,
    finalized: bool,
    last_number: Option<u64>,
    last_finalized_hash: Option<String>,
    poll_interval: Duration,
}

impl BlockStream<'_> {
    /// One poll cycle. Idle polls cost a single RPC call: the best-head path
    /// skips the hash lookup until the number advances, and the finalized path
    /// skips the header fetch while `chain_getFinalizedHead` is unchanged.
    fn poll(&mut self) -> Result<Option<BlockHeader>, CoreError> {
        let header = if self.finalized {
            let hash = self.client.finalized_head()?;
            if self.last_finalized_hash.as_deref() == Some(hash.as_str()) {
                return Ok(None);
            }
            self.last_finalized_hash = Some(hash.clone());
            let header = self.client.header(Some(&hash))?;
            (self.last_number.is_none_or(|last| header.number > last)).then_some(header)
        } else {
            self.client.best_header_past(self.last_number)?
        };
        if let Some(header) = &header {
            self.last_number = Some(header.number);
        }
        Ok(header)
    }
}

impl Iterator for BlockStream<'_> {
    type Item = Result<BlockHeader, CoreError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.poll() {
                Ok(Some(header)) => return Some(Ok(header)),
                Ok(None) => thread::sleep(self.poll_interval),
                Err(error) => return Some(Err(error)),
            }
        }
    }
}

struct RpcBootstrap {
    endpoint: String,
    http: HttpClient,
    next_id: AtomicU64,
}

impl RpcBootstrap {
    fn rpc(&self, method: &str, params: JsonValue) -> Result<JsonValue, CoreError> {
        rpc_request(
            &self.http,
            &self.endpoint,
            self.next_id.fetch_add(1, Ordering::Relaxed),
            method,
            params,
        )
    }

    fn runtime_version(&self) -> Result<RuntimeVersion, CoreError> {
        parse_runtime_version(self.rpc("state_getRuntimeVersion", json!([]))?)
    }

    fn metadata(&self) -> Result<Vec<u8>, CoreError> {
        fetch_metadata(|method, params| self.rpc(method, params))
    }

    fn block_hash(&self, block: Option<u64>) -> Result<String, CoreError> {
        let params = block.map_or_else(|| json!([]), |number| json!([number]));
        let value = self.rpc("chain_getBlockHash", params)?;
        Ok(json_string(&value, "chain_getBlockHash result")?.to_string())
    }

    fn ss58_format(&self) -> Result<u16, CoreError> {
        let properties = self.rpc("system_properties", json!([]))?;
        let value = properties
            .as_object()
            .and_then(|object| object.get("ss58Format"))
            .map(json_u64)
            .transpose()?
            .unwrap_or(42);
        u16::try_from(value)
            .map_err(|_| CoreError::Rpc(format!("ss58Format {value} does not fit u16")))
    }
}

#[derive(Debug, Clone, Copy)]
struct RuntimeVersion {
    spec_version: u32,
    transaction_version: u32,
}

fn parse_runtime_version(value: JsonValue) -> Result<RuntimeVersion, CoreError> {
    let object = value
        .as_object()
        .ok_or_else(|| CoreError::Rpc("state_getRuntimeVersion returned a non-object".into()))?;
    let spec = object
        .get("specVersion")
        .ok_or_else(|| CoreError::Rpc("runtime version has no specVersion".into()))?;
    let transaction = object
        .get("transactionVersion")
        .ok_or_else(|| CoreError::Rpc("runtime version has no transactionVersion".into()))?;
    Ok(RuntimeVersion {
        spec_version: u32::try_from(json_u64(spec)?)
            .map_err(|_| CoreError::Rpc("specVersion does not fit u32".into()))?,
        transaction_version: u32::try_from(json_u64(transaction)?)
            .map_err(|_| CoreError::Rpc("transactionVersion does not fit u32".into()))?,
    })
}

/// Fetch metadata V15 when the runtime exposes the metadata runtime API.
///
/// `state_getMetadata` returns V14 on current Substrate nodes, which omits the
/// runtime-API descriptions required by `Client::runtime_call`. Older nodes may
/// not expose `Metadata_metadata_at_version`, so retain V14 as a compatibility
/// fallback for storage reads and call composition.
fn fetch_metadata<F>(mut rpc: F) -> Result<Vec<u8>, CoreError>
where
    F: FnMut(&str, JsonValue) -> Result<JsonValue, CoreError>,
{
    let requested_version = hex_prefixed(&15u32.to_le_bytes());
    match rpc(
        "state_call",
        json!(["Metadata_metadata_at_version", requested_version]),
    ) {
        Ok(value) => {
            let encoded = decode_hex(json_string(&value, "Metadata_metadata_at_version result")?)?;
            let mut input = encoded.as_slice();
            let metadata = Option::<Vec<u8>>::decode(&mut input).map_err(|error| {
                CoreError::Codec(format!(
                    "cannot decode Metadata_metadata_at_version result: {error}"
                ))
            })?;
            if !input.is_empty() {
                return Err(CoreError::Codec(format!(
                    "{} trailing bytes in Metadata_metadata_at_version result",
                    input.len()
                )));
            }
            if let Some(metadata) = metadata {
                return Ok(metadata);
            }
        }
        Err(CoreError::Rpc(_)) => {}
        Err(error) => return Err(error),
    }

    let value = rpc("state_getMetadata", json!([]))?;
    decode_hex(json_string(&value, "state_getMetadata result")?)
}

fn rpc_request(
    client: &HttpClient,
    endpoint: &str,
    id: u64,
    method: &str,
    params: JsonValue,
) -> Result<JsonValue, CoreError> {
    let response = client
        .post(endpoint)
        .json(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))
        .send()
        .map_err(|error| CoreError::Rpc(format!("{method} request failed: {error}")))?
        .error_for_status()
        .map_err(|error| CoreError::Rpc(format!("{method} HTTP error: {error}")))?;
    let body: JsonValue = response
        .json()
        .map_err(|error| CoreError::Rpc(format!("{method} returned invalid JSON: {error}")))?;
    if let Some(error) = body.as_object().and_then(|object| object.get("error")) {
        return Err(CoreError::Rpc(format!("{method}: {error}")));
    }
    body.as_object()
        .and_then(|object| object.get("result"))
        .cloned()
        .ok_or_else(|| CoreError::Rpc(format!("{method} response omitted result")))
}

fn http_endpoint(endpoint: &str) -> String {
    if let Some(rest) = endpoint.strip_prefix("ws://") {
        return format!("http://{rest}");
    }
    if let Some(rest) = endpoint.strip_prefix("wss://") {
        return format!("https://{rest}");
    }
    endpoint.to_string()
}

fn parse_header_fields(value: &JsonValue) -> Result<(u64, String), CoreError> {
    let object = value
        .as_object()
        .ok_or_else(|| CoreError::Rpc("chain_getHeader returned a non-object".into()))?;
    let number = json_u64(
        object
            .get("number")
            .ok_or_else(|| CoreError::Rpc("header has no number".into()))?,
    )?;
    let parent_hash = object
        .get("parentHash")
        .and_then(JsonValue::as_str)
        .unwrap_or_default()
        .to_string();
    Ok((number, parent_hash))
}

fn storage_info(runtime: &Runtime, pallet: &str, storage: &str) -> Result<StorageInfo, CoreError> {
    runtime
        .storage_entry(pallet, storage)
        .cloned()
        .ok_or_else(|| CoreError::NotInRuntime(format!("storage {pallet}.{storage}")))
}

fn runtime_api_method<'a>(
    runtime: &'a Runtime,
    api: &str,
    method: &str,
) -> Result<&'a RuntimeApiMethodInfo, CoreError> {
    runtime
        .apis
        .iter()
        .find(|candidate| candidate.name == api)
        .and_then(|candidate| {
            candidate
                .methods
                .iter()
                .find(|candidate| candidate.name == method)
        })
        .ok_or_else(|| CoreError::NotInRuntime(format!("runtime API {api}.{method}")))
}

fn decode_storage(
    runtime: &Runtime,
    info: &StorageInfo,
    raw: Option<&str>,
) -> Result<Value, CoreError> {
    let bytes = match raw {
        Some(raw) => decode_hex(raw)?,
        None if info.modifier == "Optional" => return Ok(Value::Null),
        None => info.default_bytes.clone(),
    };
    runtime.decode_spec(&TypeSpec::Id(info.value_type), &bytes, true)
}

fn dispatch_error(runtime: &Runtime, value: &Value) -> Result<DispatchError, CoreError> {
    if let Some(module) = variant(value, "Module") {
        let index = field(module, "index")
            .and_then(as_u128)
            .or_else(|| tuple_item(module, 0).and_then(as_u128))
            .and_then(|value| u8::try_from(value).ok())
            .ok_or_else(|| CoreError::Codec("module error omitted pallet index".into()))?;
        let error_index = field(module, "error")
            .or_else(|| tuple_item(module, 1))
            .and_then(first_byte)
            .ok_or_else(|| CoreError::Codec("module error omitted error bytes".into()))?;
        let pallet = runtime.pallet_at(index).map(|pallet| pallet.name.clone());
        let (name, docs) = runtime
            .module_error(index, error_index)
            .unwrap_or_else(|_| (format!("Error{error_index}"), Vec::new()));
        return Ok(DispatchError {
            pallet,
            semantic_code: semantic_error_code(&name),
            name,
            docs,
        });
    }
    let name = variant_name(value).unwrap_or_else(|| "DispatchError".into());
    Ok(DispatchError {
        pallet: None,
        semantic_code: semantic_error_code(&name),
        name,
        docs: Vec::new(),
    })
}

fn semantic_error_code(name: &str) -> String {
    let canonical = match name {
        "SubNetworkDoesNotExist"
        | "SubnetDoesNotExist"
        | "SubnetNotExists"
        | "SubNetDoesNotExist" => "subnet_not_exists",
        "NotProxy" => "not_proxy",
        "NoPermission" | "NotAllowed" | "Unproxyable" => "not_allowed",
        "HotKeyNotRegisteredInNetwork" | "HotkeyNotRegisteredInSubNet" | "NotRegistered" => {
            "not_registered"
        }
        "TxRateLimitExceeded" | "SettingWeightsTooFast" => "rate_limited",
        _ => return snake_case(name),
    };
    canonical.into()
}

fn snake_case(name: &str) -> String {
    let mut out = String::new();
    for (index, ch) in name.chars().enumerate() {
        if ch.is_uppercase() {
            if index > 0 {
                out.push('_');
            }
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn event_is(record: &Value, module: &str, name: &str) -> bool {
    field(record, "module_id")
        .or_else(|| field(record, "event").and_then(|event| field(event, "module_id")))
        .and_then(as_str)
        == Some(module)
        && field(record, "event_id")
            .or_else(|| field(record, "event").and_then(|event| field(event, "event_id")))
            .and_then(as_str)
            == Some(name)
}

fn event_attributes(record: &Value) -> Option<&Value> {
    field(record, "attributes")
        .or_else(|| field(record, "event").and_then(|event| field(event, "attributes")))
}

/// Flatten `get_subnet_hyperparams_v3`'s `[{name, value}, ...]` list into a
/// `{name: raw}` map, matching the Python `subnet_hyperparameters` read.
fn flatten_subnet_hyperparams_v3(raw: Value) -> Result<Value, CoreError> {
    if matches!(raw, Value::Null) {
        return Ok(Value::Null);
    }
    let Value::List(entries) = raw else {
        return Err(CoreError::Codec(
            "SubnetInfoRuntimeApi.get_subnet_hyperparams_v3 returned a non-list".into(),
        ));
    };
    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        let name = field(&entry, "name")
            .and_then(as_str)
            .ok_or_else(|| {
                CoreError::Codec(
                    "SubnetInfoRuntimeApi.get_subnet_hyperparams_v3 entry omitted name".into(),
                )
            })?
            .to_owned();
        let tagged = field(&entry, "value").ok_or_else(|| {
            CoreError::Codec(
                "SubnetInfoRuntimeApi.get_subnet_hyperparams_v3 entry omitted value".into(),
            )
        })?;
        out.push((Value::Str(name), flatten_hyperparam_value(tagged.clone())));
    }
    Ok(Value::Dict(out))
}

/// Peel one v3 `{TypeTag: payload}` value down to the raw payload.
///
/// `U64F64` keeps the raw `bits` (what `sudo set` writes for
/// `burn_increase_mult`). `I32F32` is only used for
/// `alpha_sigmoid_steepness`, whose setter takes the integer part
/// (`bits / 2^32`), not the raw bits.
fn flatten_hyperparam_value(tagged: Value) -> Value {
    const I32F32_ONE: i128 = 1 << 32;
    let Value::Dict(entries) = &tagged else {
        return tagged;
    };
    let [(tag, payload)] = entries.as_slice() else {
        return tagged;
    };
    let Some(tag) = as_str(tag) else {
        return tagged;
    };
    if let Value::Dict(inner) = payload {
        if let [(Value::Str(key), bits)] = inner.as_slice() {
            if key == "bits" {
                if tag == "I32F32" {
                    let bits = match bits {
                        Value::Int(v) => *v,
                        Value::Uint(v) => match i128::try_from(*v) {
                            Ok(v) => v,
                            Err(_) => return tagged,
                        },
                        _ => return tagged,
                    };
                    return Value::Int(bits.div_euclid(I32F32_ONE));
                }
                return bits.clone();
            }
        }
    }
    payload.clone()
}

pub fn field<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    let Value::Dict(entries) = value else {
        return None;
    };
    entries.iter().find_map(|(key, value)| match key {
        Value::Str(key) if key == name => Some(value),
        _ => None,
    })
}

pub fn variant<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    field(value, name)
}

pub fn variant_name(value: &Value) -> Option<String> {
    match value {
        Value::Str(name) => Some(name.clone()),
        Value::Dict(entries) if entries.len() == 1 => entries.first().and_then(|(key, _)| {
            if let Value::Str(name) = key {
                Some(name.clone())
            } else {
                None
            }
        }),
        _ => None,
    }
}

pub fn as_str(value: &Value) -> Option<&str> {
    match value {
        Value::Str(value) => Some(value),
        _ => None,
    }
}

pub fn as_u128(value: &Value) -> Option<u128> {
    match value {
        Value::Int(value) => u128::try_from(*value).ok(),
        Value::Uint(value) => Some(*value),
        _ => None,
    }
}

pub fn as_bool(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(value) => Some(*value),
        _ => None,
    }
}

pub fn value_bytes(value: &Value) -> Option<Vec<u8>> {
    match value {
        Value::Bytes(bytes) => Some(bytes.clone()),
        Value::Str(value) => {
            if value.starts_with("0x") {
                decode_hex(value).ok()
            } else {
                Some(value.as_bytes().to_vec())
            }
        }
        Value::List(items) | Value::Tuple(items) => {
            let direct = items
                .iter()
                .map(as_u128)
                .map(|value| value.and_then(|value| u8::try_from(value).ok()))
                .collect::<Option<Vec<_>>>();
            direct.or_else(|| match items.as_slice() {
                [inner] => value_bytes(inner),
                _ => None,
            })
        }
        Value::Dict(entries) => match entries.as_slice() {
            [(_, inner)] => value_bytes(inner),
            _ => None,
        },
        _ => None,
    }
}

fn tuple_item(value: &Value, index: usize) -> Option<&Value> {
    match value {
        Value::Tuple(items) | Value::List(items) => items.get(index),
        _ => None,
    }
}

fn first_byte(value: &Value) -> Option<u8> {
    value_bytes(value).and_then(|bytes| bytes.first().copied())
}

fn required_u128(value: &Value, name: &str) -> Result<u128, CoreError> {
    field(value, name)
        .and_then(as_u128)
        .ok_or_else(|| CoreError::Codec(format!("runtime result omitted integer field {name}")))
}

fn value_map_u16(rows: Vec<(Value, Value)>) -> BTreeMap<u16, u16> {
    rows.into_iter()
        .filter_map(|(key, value)| {
            let key = as_u128(&key).and_then(|value| u16::try_from(value).ok())?;
            let value = as_u128(&value).and_then(|value| u16::try_from(value).ok())?;
            Some((key, value))
        })
        .collect()
}

fn value_map_u128(rows: Vec<(Value, Value)>) -> BTreeMap<u16, u128> {
    rows.into_iter()
        .filter_map(|(key, value)| {
            let key = as_u128(&key).and_then(|value| u16::try_from(value).ok())?;
            Some((key, as_u128(&value)?))
        })
        .collect()
}

fn json_string<'a>(value: &'a JsonValue, context: &str) -> Result<&'a str, CoreError> {
    match value {
        JsonValue::String(text) => Ok(text),
        _ => Err(CoreError::Rpc(format!("{context} is not a string"))),
    }
}

fn json_number_u64(value: &JsonValue) -> Option<u64> {
    match value {
        JsonValue::Number(number) => number.to_string().parse().ok(),
        _ => None,
    }
}

fn json_number_u128(value: &JsonValue) -> Option<u128> {
    match value {
        JsonValue::Number(number) => number.to_string().parse().ok(),
        _ => None,
    }
}

fn json_number_i128(value: &JsonValue) -> Option<i128> {
    match value {
        JsonValue::Number(number) => number.to_string().parse().ok(),
        _ => None,
    }
}

fn json_u64(value: &JsonValue) -> Result<u64, CoreError> {
    if let Some(value) = json_number_u64(value) {
        return Ok(value);
    }
    let text = value
        .as_str()
        .ok_or_else(|| CoreError::Rpc(format!("expected integer, got {value}")))?;
    if let Some(hex) = text.strip_prefix("0x") {
        return u64::from_str_radix(hex, 16)
            .map_err(|error| CoreError::Rpc(format!("invalid hex integer {text}: {error}")));
    }
    text.parse::<u64>()
        .map_err(|error| CoreError::Rpc(format!("invalid integer {text}: {error}")))
}

fn json_u128(value: &JsonValue) -> Result<u128, CoreError> {
    if let Some(value) = json_number_u128(value) {
        return Ok(value);
    }
    let text = value
        .as_str()
        .ok_or_else(|| CoreError::Rpc(format!("expected integer, got {value}")))?;
    if let Some(hex) = text.strip_prefix("0x") {
        return u128::from_str_radix(hex, 16)
            .map_err(|error| CoreError::Rpc(format!("invalid hex integer {text}: {error}")));
    }
    text.parse::<u128>()
        .map_err(|error| CoreError::Rpc(format!("invalid integer {text}: {error}")))
}

fn json_to_value(value: &JsonValue) -> Result<Value, CoreError> {
    Ok(match value {
        JsonValue::Null => Value::Null,
        JsonValue::Bool(value) => Value::Bool(*value),
        JsonValue::Number(value) => {
            let number = JsonValue::Number(value.clone());
            if let Some(signed) = json_number_i128(&number) {
                Value::Int(i128::from(signed))
            } else if let Some(unsigned) = json_number_u128(&number) {
                Value::Uint(unsigned)
            } else {
                return Err(CoreError::Codec(format!(
                    "non-integral JSON number cannot become SCALE Value: {value}"
                )));
            }
        }
        JsonValue::String(value) => Value::str(value),
        JsonValue::Array(items) => Value::List(
            items
                .iter()
                .map(json_to_value)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        JsonValue::Object(entries) => Value::record(
            entries
                .iter()
                .map(|(key, value)| Ok((key.clone(), json_to_value(value)?)))
                .collect::<Result<Vec<_>, CoreError>>()?,
        ),
    })
}

fn parse_h256(value: &str) -> Result<[u8; 32], CoreError> {
    let bytes = decode_hex(value)?;
    <[u8; 32]>::try_from(bytes.as_slice())
        .map_err(|_| CoreError::Rpc(format!("expected H256, got {} bytes", bytes.len())))
}

fn decode_hex(value: &str) -> Result<Vec<u8>, CoreError> {
    hex::decode(value.trim_start_matches("0x"))
        .map_err(|error| CoreError::Codec(format!("invalid hex {value}: {error}")))
}

fn hex_prefixed(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(bytes))
}

#[cfg(test)]
mod reorg_finalization_tests {
    use super::{classify_inclusion_finalization, InclusionFinalization};

    #[test]
    fn canonical_inclusion_remains_pending_below_finalized_height() {
        assert_eq!(
            classify_inclusion_finalization("0xabc", Some("0xABC"), 10, 9),
            None
        );
    }

    #[test]
    fn canonical_inclusion_finalizes_at_or_above_its_height() {
        assert_eq!(
            classify_inclusion_finalization("0xabc", Some("0xABC"), 10, 10),
            Some(InclusionFinalization::Finalized)
        );
        assert_eq!(
            classify_inclusion_finalization("0xabc", Some("0xabc"), 10, 12),
            Some(InclusionFinalization::Finalized)
        );
    }

    #[test]
    fn replaced_or_missing_inclusion_hash_is_a_reorg() {
        assert_eq!(
            classify_inclusion_finalization("0xabc", Some("0xdef"), 10, 12),
            Some(InclusionFinalization::Reorged)
        );
        assert_eq!(
            classify_inclusion_finalization("0xabc", None, 10, 9),
            Some(InclusionFinalization::Reorged)
        );
    }
}
