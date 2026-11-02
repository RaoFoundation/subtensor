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

use codec::{Decode, Encode, MaxEncodedLen};
use reqwest::blocking::Client as HttpClient;
use serde_json::{json, Value as JsonValue};

use crate::codec::extrinsic::{era_birth, TxParams};
use crate::codec::value::Value;
use crate::error::CoreError;
use crate::keys::{is_hashed_crypto, is_pq_crypto, is_rotating_crypto, Keypair};
use crate::mlkem;
use crate::runtime::type_string::TypeSpec;
use crate::runtime::{Runtime, RuntimeApiMethodInfo, StorageInfo};

const DEFAULT_RPC_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_RECEIPT_TIMEOUT: Duration = Duration::from_secs(120);
const DEFAULT_ERA_PERIOD: u64 = 64;
// Runtime CheckMortality limits encrypted carriers to eight blocks.
const SHIELD_ERA_PERIOD: u64 = 8;
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

/// One immutable authority/nonce selection for an ordinary signature or both
/// signatures of a Shield submission. Authority is never reloaded between the
/// encrypted inner proof and its carrier proof.
struct SigningContext {
    runtime: Arc<Runtime>,
    params: TxParams,
    registered: Option<RegisteredSigningAuthority>,
}

struct RegisteredSigningAuthority {
    key: Keypair,
    generation: u64,
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
        self.query_with_runtime(
            self.runtime()?.as_ref(),
            pallet,
            storage,
            params,
            block_hash,
        )
    }

    fn query_with_runtime(
        &self,
        runtime: &Runtime,
        pallet: &str,
        storage: &str,
        params: &[Value],
        block_hash: Option<&str>,
    ) -> Result<Value, CoreError> {
        let info = storage_info(runtime, pallet, storage)?;
        let key = runtime.storage_key(&info, params)?;
        let raw = self.storage_raw(&key, block_hash)?;
        decode_storage(runtime, &info, raw.as_deref())
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
    /// Registered accounts require the current confirmed nonce and no pending
    /// transaction. ML-DSA authority is selected from finalized state.
    pub fn sign_extrinsic(
        &self,
        call_data: &[u8],
        signer: &Keypair,
        nonce: u64,
        period: Option<u64>,
    ) -> Result<(Vec<u8>, String), CoreError> {
        let context = self.prepare_signing(signer, Some(nonce), period, false)?;
        self.sign_prepared(call_data, signer, &context, 0)
    }

    fn prepare_signing(
        &self,
        signer: &Keypair,
        requested_nonce: Option<u64>,
        period: Option<u64>,
        shielded: bool,
    ) -> Result<SigningContext, CoreError> {
        let runtime = self.runtime()?;
        let mut registered = None;
        let mut authority_block = None;
        let stateless = crate::keys::is_fixed_pq_crypto(signer.crypto_type());
        let nonce = if stateless {
            if !runtime.supports_mldsa_profile(signer.mldsa_profile()?) {
                return Err(CoreError::Policy(
                    "runtime does not support registration-free post-quantum accounts".into(),
                ));
            }
            let state_hash = self.finalized_head()?;
            if self.query_with_runtime(
                &runtime,
                "AdminUtils",
                "HashedAccountsEnabled",
                &[],
                Some(&state_hash),
            )? != Value::Bool(true)
            {
                return Err(CoreError::Policy(
                    "post-quantum authorization is not active".into(),
                ));
            }
            authority_block = Some(state_hash);
            match requested_nonce {
                Some(nonce) => nonce,
                None => self.account_next_index(&signer.ss58_address())?,
            }
        } else if is_rotating_crypto(signer.crypto_type()) {
            let mldsa = is_pq_crypto(signer.crypto_type());
            if mldsa && !runtime.supports_mldsa_profile(signer.mldsa_descriptor()?[1]) {
                return Err(CoreError::Policy(
                    "runtime does not support post-quantum accounts".into(),
                ));
            }
            let state_hash = if shielded || mldsa {
                self.finalized_head()?
            } else {
                self.block_hash(None)?
            };
            let query = |pallet, storage, params: &[Value]| {
                self.query_with_runtime(&runtime, pallet, storage, params, Some(&state_hash))
            };
            if query("AdminUtils", "HashedAccountsEnabled", &[])? != Value::Bool(true) {
                return Err(CoreError::Policy(
                    "hashed account authorization is not active".into(),
                ));
            }
            let address = [Value::str(signer.ss58_address())];
            let record = query(
                "HashedAccounts",
                if mldsa { "MlDsaAccounts" } else { "Accounts" },
                &address,
            )?;
            if mldsa {
                let descriptor = field(&record, "descriptor")
                    .ok_or_else(|| CoreError::Codec("post-quantum descriptor missing".into()))?;
                let expected = signer.mldsa_descriptor()?;
                if field(descriptor, "version").and_then(as_u128) != Some(u128::from(expected[0]))
                    || field(descriptor, "algorithm").and_then(as_u128)
                        != Some(u128::from(expected[1]))
                    || field(descriptor, "initial_commitment")
                        .and_then(value_bytes)
                        .as_deref()
                        != Some(&expected[2..])
                {
                    return Err(CoreError::Policy(
                        "post-quantum descriptor does not match this wallet".into(),
                    ));
                }
            }
            let generation = field(&record, "generation")
                .and_then(as_u128)
                .and_then(|value| u64::try_from(value).ok())
                .ok_or_else(|| {
                    CoreError::Policy("hashed account must be registered before use".into())
                })?;
            let active = signer.at_generation(generation)?;
            let commitment = field(&record, "commitment")
                .and_then(value_bytes)
                .ok_or_else(|| CoreError::Codec("hashed account commitment missing".into()))?;
            if commitment != active.authorization_current_commitment()? {
                return Err(CoreError::Policy(
                    "hashed account commitment does not match this mnemonic".into(),
                ));
            }
            // Nonce and authority must come from the same state. A pool nonce
            // cannot be combined with an older finalized signing generation.
            let account = query("System", "Account", &address)?;
            let confirmed_nonce = field(&account, "nonce")
                .and_then(as_u128)
                .and_then(|value| u64::try_from(value).ok())
                .ok_or_else(|| CoreError::Codec("System.Account.nonce is missing".into()))?;
            self.ensure_registered_nonce(signer, confirmed_nonce)?;
            if requested_nonce.is_some_and(|nonce| nonce != confirmed_nonce) {
                return Err(CoreError::Policy(
                    "hashed accounts must use the current on-chain nonce; refresh and sign again"
                        .into(),
                ));
            }
            registered = Some(RegisteredSigningAuthority {
                key: active,
                generation,
            });
            authority_block = Some(state_hash);
            confirmed_nonce
        } else {
            match requested_nonce {
                Some(nonce) => nonce,
                None => self.account_next_index(&signer.ss58_address())?,
            }
        };
        let params = match authority_block {
            Some(hash) => {
                let current = self.header(Some(&hash))?.number;
                self.transaction_params_at(nonce, period, current)?
            }
            None => self.transaction_params(nonce, period)?,
        };
        Ok(SigningContext {
            runtime,
            params,
            registered,
        })
    }

    fn ensure_registered_nonce(
        &self,
        signer: &Keypair,
        confirmed_nonce: u64,
    ) -> Result<(), CoreError> {
        if self.account_next_index(&signer.ss58_address())? != confirmed_nonce {
            return Err(CoreError::Policy(
                "hashed account has a pending transaction; wait for finalization before signing again"
                    .into(),
            ));
        }
        Ok(())
    }

    /// Only the encrypted inner transaction may use offset one: its carrier
    /// consumes the selected generation and nonce before the inner executes.
    fn sign_prepared(
        &self,
        call_data: &[u8],
        signer: &Keypair,
        context: &SigningContext,
        generation_offset: u64,
    ) -> Result<(Vec<u8>, String), CoreError> {
        if generation_offset > 1 {
            return Err(CoreError::Policy("invalid Shield generation offset".into()));
        }
        let params = TxParams {
            era: context.params.era.clone(),
            nonce: context
                .params
                .nonce
                .checked_add(generation_offset)
                .ok_or_else(|| CoreError::Policy("account nonce exhausted".into()))?,
            tip: context.params.tip,
            tip_asset_id: context.params.tip_asset_id,
            genesis_hash: context.params.genesis_hash,
            era_block_hash: context.params.era_block_hash,
            metadata_hash: context.params.metadata_hash,
        };
        if crate::keys::is_fixed_pq_crypto(signer.crypto_type())
            && params.nonce >= u64::from(u32::MAX)
        {
            return Err(CoreError::Policy("account nonce exhausted".into()));
        }
        let runtime = &context.runtime;
        let signature = if let Some(authority) = &context.registered {
            // Also check immediately before each proof. A concurrent pending or
            // newly finalized transaction invalidates the whole prepared pair;
            // never refresh only the carrier to a newer generation.
            self.ensure_registered_nonce(signer, context.params.nonce)?;
            let generation = authority
                .generation
                .checked_add(generation_offset)
                .ok_or_else(|| CoreError::Policy("hashed signing generation exhausted".into()))?;
            let selected = authority.key.at_generation(generation)?;
            let (extra, additional) = runtime.signature_payload_parts(&params)?;
            let mut payload = call_data.to_vec();
            payload.extend_from_slice(&extra);
            payload.extend_from_slice(&additional);
            selected.sign_authorization(&payload)?
        } else if crate::keys::is_fixed_pq_crypto(signer.crypto_type()) {
            // All account/chain/call/nonce fields are authenticated in the full
            // transcript, including Shield's inner nonce offset. The fixed key
            // has no on-chain generation to reload between the two signatures.
            let (extra, additional) = runtime.signature_payload_parts(&params)?;
            let mut payload = call_data.to_vec();
            payload.extend_from_slice(&extra);
            payload.extend_from_slice(&additional);
            signer.sign_authorization(&payload)?
        } else {
            let payload = runtime.signature_payload(call_data, &params)?;
            signer.sign(&payload)?
        };
        let (extrinsic, hash) = runtime.encode_signed_extrinsic(
            call_data,
            signer.public_key_bytes(),
            &signature,
            signer.crypto_type(),
            &params,
        )?;
        Ok((extrinsic, hex_prefixed(&hash)))
    }

    fn transaction_params(&self, nonce: u64, period: Option<u64>) -> Result<TxParams, CoreError> {
        let current = self.block_number()?;
        self.transaction_params_at(nonce, period, current)
    }

    fn transaction_params_at(
        &self,
        nonce: u64,
        period: Option<u64>,
        current: u64,
    ) -> Result<TxParams, CoreError> {
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
        Ok(TxParams {
            era,
            nonce,
            tip: 0,
            tip_asset_id: None,
            genesis_hash: self.genesis_hash,
            era_block_hash,
            metadata_hash: None,
        })
    }

    /// Quotes only decode the envelope: never disclose an active signing key
    /// or produce a reusable authorization for an operation the user may cancel.
    fn payment_info(&self, call_data: &[u8], signer: &Keypair) -> Result<JsonValue, CoreError> {
        let runtime = self.runtime()?;
        let nonce = self.account_next_index(&signer.ss58_address())?;
        let params = self.transaction_params(nonce, Some(DEFAULT_ERA_PERIOD))?;
        let proof_len = if is_hashed_crypto(signer.crypto_type()) {
            subtensor_hashed::Proof::max_encoded_len()
        } else {
            64
        };
        let proof = if crate::keys::is_falcon_crypto(signer.crypto_type()) {
            let profile = crate::keys::pq_profile(signer.crypto_type())
                .ok_or_else(|| CoreError::Codec("invalid Falcon crypto type".into()))?;
            subtensor_hashed::falcon::SignerAddress {
                account: signer.public_key_bytes(),
                version: subtensor_hashed::falcon::VERSION,
                algorithm: profile,
                generation: 0,
                public_key: vec![
                    0;
                    subtensor_hashed::falcon::public_key_len(profile).ok_or_else(
                        || CoreError::Codec("invalid Falcon profile".into())
                    )?
                ],
                next_commitment: [1; 64],
                signature: vec![
                    0;
                    subtensor_hashed::falcon::signature_len(profile).ok_or_else(
                        || CoreError::Codec("invalid Falcon profile".into())
                    )?
                ],
            }
            .encode()
        } else if is_pq_crypto(signer.crypto_type()) {
            subtensor_hashed::mldsa::SignerAddress {
                account: signer.public_key_bytes(),
                version: subtensor_hashed::mldsa::VERSION,
                algorithm: if signer.crypto_type() == crate::keys::CRYPTO_MLDSA65 {
                    subtensor_hashed::mldsa::ALGORITHM
                } else {
                    subtensor_hashed::mldsa::FIXED_ALGORITHM
                },
                generation: 0,
                public_key: [0; subtensor_hashed::mldsa::PUBLIC_KEY_LEN],
                next_commitment: [1; 64],
                signature: [0; subtensor_hashed::mldsa::SIGNATURE_LEN],
            }
            .encode()
        } else {
            vec![0; proof_len]
        };
        let (extrinsic, _) = runtime.encode_signed_extrinsic(
            call_data,
            signer.public_key_bytes(),
            &proof,
            signer.crypto_type(),
            &params,
        )?;
        self.rpc_value("payment_queryInfo", json!([hex_prefixed(&extrinsic)]))
    }

    pub fn estimate_fee(&self, call_data: &[u8], signer: &Keypair) -> Result<u128, CoreError> {
        let value = self.payment_info(call_data, signer)?;
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
        let value = self.payment_info(call_data, signer)?;
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
        let context = self.prepare_signing(signer, nonce, period, false)?;
        let (extrinsic, hash) = self.sign_prepared(call_data, signer, &context, 0)?;
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
    /// the current ML-KEM-768 key, then submit the carrier at nonce. The result
    /// describes the decrypted call and sums both fees; `data` retains the
    /// carrier hash and each reported fee separately.
    pub fn submit_shielded(
        &self,
        call_data: &[u8],
        signer: &Keypair,
        wait_for_finalization: bool,
    ) -> Result<TxOutcome, CoreError> {
        let next_key = self.query("MevShield", "NextKey", &[], None)?;
        let public_key = value_bytes(&next_key)
            .ok_or_else(|| CoreError::Rpc("MevShield.NextKey is unavailable".into()))?;
        let context = self.prepare_signing(signer, None, Some(SHIELD_ERA_PERIOD), true)?;
        let (inner, inner_hash) = self.sign_prepared(call_data, signer, &context, 1)?;
        let ciphertext = mlkem::seal(&public_key, &inner, true)?;
        let outer = context.runtime.compose_call(
            "MevShield",
            "submit_encrypted",
            &Value::record(vec![("ciphertext".into(), Value::Bytes(ciphertext))]),
        )?;
        let (extrinsic, hash) = self.sign_prepared(&outer, signer, &context, 0)?;
        let carrier = self.submit_encoded(&extrinsic, hash, wait_for_finalization)?;
        self.resolve_shielded_inner(
            &inner,
            inner_hash,
            carrier,
            wait_for_finalization,
            DEFAULT_RECEIPT_TIMEOUT,
        )
    }

    fn resolve_shielded_inner(
        &self,
        inner: &[u8],
        inner_hash: String,
        carrier: TxOutcome,
        wait_for_finalization: bool,
        timeout: Duration,
    ) -> Result<TxOutcome, CoreError> {
        if !carrier.success {
            return Ok(carrier);
        }
        let start_block = carrier
            .block_number
            .ok_or_else(|| CoreError::Rpc("successful Shield carrier omitted its block".into()))?;
        // The carrier only accepts ciphertext. Report the requested call's
        // dispatch result, including nested failures and missing inner inclusion.
        let mut result = self.wait_for_inclusion(
            inner,
            inner_hash.clone(),
            start_block,
            wait_for_finalization,
            timeout,
        )?;
        result.data.insert("shielded".into(), Value::Bool(true));
        result
            .data
            .insert("inner_extrinsic_hash".into(), Value::str(inner_hash));
        result.data.insert(
            "carrier_extrinsic_hash".into(),
            Value::str(carrier.extrinsic_hash),
        );
        if let Some(fee) = result.fee_rao {
            result.data.insert("inner_fee_rao".into(), Value::Uint(fee));
        }
        if let Some(fee) = carrier.fee_rao {
            result
                .data
                .insert("carrier_fee_rao".into(), Value::Uint(fee));
            result.fee_rao = Some(
                fee.checked_add(result.fee_rao.unwrap_or_default())
                    .ok_or_else(|| CoreError::Codec("Shield total fee exceeds u128".into()))?,
            );
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

#[cfg(test)]
mod fee_quote_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    fn runtime() -> Runtime {
        crate::codec::extrinsic::hashed_tests::runtime_falcon()
    }

    #[test]
    fn fee_and_weight_rpc_use_sized_dummy_proofs_for_every_wallet_mode() {
        for crypto in [
            0,
            1,
            4,
            6,
            crate::keys::CRYPTO_MLDSA65,
            crate::keys::CRYPTO_MLDSA65_FIXED,
            crate::keys::CRYPTO_FALCON512,
            crate::keys::CRYPTO_FALCON512_HASHED,
            crate::keys::CRYPTO_FALCON1024,
            crate::keys::CRYPTO_FALCON1024_HASHED,
        ] {
            let runtime = runtime();
            let full = Keypair::from_seed(&[17; 32], crypto).unwrap();
            // Public-only keys cannot sign or derive a generation's public key.
            let public = Keypair::new(Some(&full.ss58_address()), None, crypto, 42).unwrap();
            let call = runtime
                .compose_call(
                    "System",
                    "remark",
                    &Value::record(vec![("remark".into(), Value::Bytes(vec![7; 300]))]),
                )
                .unwrap();
            let params = TxParams {
                era: Value::record(vec![
                    ("period".into(), Value::Uint(64)),
                    ("current".into(), Value::Uint(10)),
                ]),
                nonce: 64,
                tip: 0,
                tip_asset_id: None,
                genesis_hash: [8; 32],
                era_block_hash: [8; 32],
                metadata_hash: None,
            };
            let proof = if is_hashed_crypto(crypto) || is_pq_crypto(crypto) {
                full.sign_authorization(b"length control").unwrap()
            } else {
                full.sign(b"length control").unwrap()
            };
            let (control, _) = runtime
                .encode_signed_extrinsic(&call, full.public_key_bytes(), &proof, crypto, &params)
                .unwrap();
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            let server = thread::spawn(move || {
                let mut quotes = Vec::new();
                while quotes.len() < 2 {
                    let (mut stream, _) = listener.accept().unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .unwrap();
                    let mut reader = BufReader::new(&mut stream);
                    let mut length = 0;
                    let mut headers = String::new();
                    loop {
                        let mut line = String::new();
                        assert_ne!(reader.read_line(&mut line).unwrap(), 0);
                        headers.push_str(&line);
                        if line == "\r\n" {
                            break;
                        }
                        if let Some(value) = line.to_lowercase().strip_prefix("content-length:") {
                            length = value.trim().parse::<usize>().unwrap();
                        }
                    }
                    match headers.lines().next().unwrap() {
                        "HEAD / HTTP/1.1" => {
                            // Readiness probes are not JSON-RPC requests or fee quotes.
                            assert_eq!(length, 0, "HEAD probe must not carry a body");
                            write!(
                                stream,
                                "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            )
                            .unwrap();
                            continue;
                        }
                        "POST / HTTP/1.1" => {}
                        other => panic!("unexpected mock HTTP request: {other}"),
                    }
                    assert!(length > 0, "mock request has no body length: {headers:?}");
                    let mut body = vec![0; length];
                    reader.read_exact(&mut body).unwrap();
                    let request: JsonValue = serde_json::from_slice(&body).unwrap();
                    let result = match request["method"].as_str().unwrap() {
                        "system_accountNextIndex" => json!(64),
                        "chain_getHeader" => {
                            json!({"number":"0xa", "parentHash":hex_prefixed(&[8;32])})
                        }
                        "chain_getBlockHash" => json!(hex_prefixed(&[8; 32])),
                        "payment_queryInfo" => {
                            quotes
                                .push(decode_hex(request["params"][0].as_str().unwrap()).unwrap());
                            json!({"partialFee":"1234", "weight":{"ref_time":456, "proof_size":78}})
                        }
                        other => {
                            panic!("quote must not read authorization state or submit: {other}")
                        }
                    };
                    let response =
                        json!({"jsonrpc":"2.0", "id":request["id"], "result":result}).to_string();
                    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).unwrap();
                }
                quotes
            });
            let client = Client {
                endpoint,
                http: HttpClient::builder()
                    .no_proxy()
                    .timeout(Duration::from_secs(5))
                    .build()
                    .unwrap(),
                next_id: AtomicU64::new(1),
                runtime: RwLock::new(Arc::new(runtime)),
                genesis_hash: [8; 32],
                ss58_format: 42,
            };
            let probe = || {
                let response = client.http.head(&client.endpoint).send().unwrap();
                assert_eq!(response.status().as_u16(), 200);
                assert!(response.bytes().unwrap().is_empty());
            };
            // Reproduce a bodyless probe before each real quote. Both quotes must
            // still reach the server and satisfy the payload assertions below.
            probe();
            assert_eq!(client.estimate_fee(&call, &public).unwrap(), 1234);
            probe();
            assert_eq!(
                client.estimate_weight(&call, &public).unwrap(),
                json_to_value(&json!({"ref_time":456,"proof_size":78})).unwrap()
            );
            let quotes = server.join().unwrap();
            assert_eq!(quotes[0], quotes[1]);
            assert_eq!(quotes[0].len(), control.len());
            let mut body = quotes[0].as_slice();
            let _ = codec::Compact::<u32>::decode(&mut body).unwrap();
            let mut control_body = control.as_slice();
            let _ = codec::Compact::<u32>::decode(&mut control_body).unwrap();
            if crate::keys::is_falcon_crypto(crypto) {
                assert_eq!(&body[..2], &[0x84, 2]);
                let mut raw = &body[2..];
                let profile = full.mldsa_profile().unwrap();
                assert_eq!(
                    codec::Compact::<u32>::decode(&mut raw).unwrap().0 as usize,
                    subtensor_hashed::falcon::proof_len(profile).unwrap()
                );
                let address = subtensor_hashed::falcon::SignerAddress::decode(&mut raw).unwrap();
                assert_eq!(address.account, full.public_key_bytes());
                assert!(address.public_key.iter().all(|byte| *byte == 0));
                assert!(address.signature.iter().all(|byte| *byte == 0));
                assert_eq!(&raw[..65], &[vec![0], vec![0x4d; 64]].concat());
                let active = full.pq_public_key().unwrap();
                assert!(!quotes[0].windows(active.len()).any(|w| w == active));
            } else if is_pq_crypto(crypto) {
                assert_eq!(&body[..2], &[0x84, 2]);
                let mut raw = &body[2..];
                assert_eq!(codec::Compact::<u32>::decode(&mut raw).unwrap().0, 5367);
                let address = subtensor_hashed::mldsa::SignerAddress::decode(&mut raw).unwrap();
                assert_eq!(address.account, full.public_key_bytes());
                assert_eq!(address.public_key, [0; 1952]);
                assert_eq!(address.signature, [0; 3309]);
                assert_eq!(&raw[..65], &[vec![0], vec![0x4d; 64]].concat());
                let active = full.mldsa_public_key().unwrap();
                assert!(!quotes[0].windows(active.len()).any(|w| w == active));
            } else if is_hashed_crypto(crypto) {
                assert_eq!(&body[..2], &[0x84, 2]);
                let mut raw = &body[2..];
                assert_eq!(codec::Compact::<u32>::decode(&mut raw).unwrap().0, 106);
                let address = subtensor_hashed::SignerAddress::decode(&mut raw).unwrap();
                assert_eq!(address.account, full.public_key_bytes());
                assert_eq!(address.public_key, [0; 32]);
                assert_eq!(address.next_commitment, [0; 32]);
                assert_eq!(&raw[1..65], &[0; 64]);
                let active = full.hashed_public_key().unwrap();
                assert!(!quotes[0].windows(active.len()).any(|w| w == active));
            } else {
                assert_eq!(&body[35..99], &[0; 64]);
                assert_eq!(&body[..35], &control_body[..35]);
            }
            assert!(client
                .runtime()
                .unwrap()
                .decode_extrinsic(&quotes[0], true)
                .is_ok());
        }
    }
}

#[cfg(test)]
mod registered_signing_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use chacha20poly1305::{aead::Aead, KeyInit, XChaCha20Poly1305, XNonce};
    use ml_kem::{kem::Decapsulate, EncodedSizeUser, KemCore, MlKem768};
    use scale_info::{Field, Path, PortableType, Type, TypeDef, TypeDefArray, TypeDefComposite};
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::AtomicBool;
    use std::sync::Mutex;

    const GENERATION: u64 = 12;
    const NONCE: u64 = 63;
    const PINNED_HASH: [u8; 32] = [8; 32];
    const ADVANCED_HASH: [u8; 32] = [9; 32];

    fn add_type(runtime: &mut Runtime, type_def: TypeDef<scale_info::form::PortableForm>) -> u32 {
        let id = runtime.types.types.len() as u32;
        runtime.types.types.push(PortableType {
            id,
            ty: Type {
                path: Path::default(),
                type_params: vec![],
                type_def,
                docs: vec![],
            },
        });
        id
    }

    fn record_type(runtime: &mut Runtime, fields: &[(&str, u32)]) -> u32 {
        add_type(
            runtime,
            TypeDef::Composite(TypeDefComposite {
                fields: fields
                    .iter()
                    .map(|(name, ty)| Field {
                        name: Some((*name).into()),
                        ty: (*ty).into(),
                        type_name: None,
                        docs: vec![],
                    })
                    .collect(),
            }),
        )
    }

    /// Add the account records to the legacy codec fixture. Calls, extensions,
    /// signing envelopes, and System.Account keys retain the real metadata.
    fn runtime(crypto: u8) -> Runtime {
        let mut runtime = crate::codec::extrinsic::hashed_tests::runtime_falcon();
        let u8_ty = add_type(
            &mut runtime,
            TypeDef::Primitive(scale_info::TypeDefPrimitive::U8),
        );
        let u64_ty = add_type(
            &mut runtime,
            TypeDef::Primitive(scale_info::TypeDefPrimitive::U64),
        );
        let bool_ty = add_type(
            &mut runtime,
            TypeDef::Primitive(scale_info::TypeDefPrimitive::Bool),
        );
        let commitment_len = if is_pq_crypto(crypto) { 64 } else { 32 };
        let commitment_ty = add_type(
            &mut runtime,
            TypeDef::Array(TypeDefArray {
                len: commitment_len,
                type_param: u8_ty.into(),
            }),
        );
        let descriptor_ty = record_type(
            &mut runtime,
            &[
                ("version", u8_ty),
                ("algorithm", u8_ty),
                ("initial_commitment", commitment_ty),
            ],
        );
        let authority_ty = record_type(
            &mut runtime,
            &[
                ("descriptor", descriptor_ty),
                ("generation", u64_ty),
                ("commitment", commitment_ty),
            ],
        );
        let account_ty = record_type(&mut runtime, &[("nonce", u64_ty)]);
        let system = runtime
            .pallets
            .iter_mut()
            .find(|p| p.name == "System")
            .unwrap();
        let account = system
            .storage
            .iter_mut()
            .find(|s| s.name == "Account")
            .unwrap();
        account.value_type = account_ty;
        let mut authority = account.clone();
        authority.prefix = "HashedAccounts".into();
        authority.name = if is_pq_crypto(crypto) {
            "MlDsaAccounts"
        } else {
            "Accounts"
        }
        .into();
        authority.value_type = authority_ty;
        runtime
            .pallets
            .iter_mut()
            .find(|p| p.name == "HashedAccounts")
            .unwrap()
            .storage = vec![authority];
        runtime
            .pallets
            .iter_mut()
            .find(|p| p.name == "AdminUtils")
            .unwrap()
            .storage
            .push(StorageInfo {
                name: "HashedAccountsEnabled".into(),
                prefix: "AdminUtils".into(),
                modifier: "Default".into(),
                hashers: vec![],
                key_types: vec![],
                value_type: bool_ty,
                default_bytes: vec![0],
            });
        runtime
    }

    #[derive(Default)]
    struct NodeState {
        nonce_override: Option<u64>,
        advanced: bool,
        pending: bool,
        advance_after_inner: bool,
        consume_nonce_on_advance: bool,
        nonce_reads: usize,
        finalized_reads: usize,
        authority_reads: Vec<String>,
        account_reads: Vec<String>,
        submitted: Vec<Vec<u8>>,
        included: Vec<Vec<u8>>,
        events: Vec<u8>,
        block_reads: usize,
    }

    struct MockNode {
        client: Client,
        state: Arc<Mutex<NodeState>>,
        stop: Arc<AtomicBool>,
        server: Option<thread::JoinHandle<()>>,
        decrypt_key: <MlKem768 as KemCore>::DecapsulationKey,
    }

    impl Drop for MockNode {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(server) = self.server.take() {
                server.join().unwrap();
            }
        }
    }

    impl MockNode {
        fn new(signer: &Keypair) -> Self {
            let crypto = signer.crypto_type();
            let runtime = runtime(crypto);
            let address = [Value::str(signer.ss58_address())];
            let storage_key = |pallet, storage, params: &[Value]| {
                hex_prefixed(
                    &runtime
                        .storage_key(&storage_info(&runtime, pallet, storage).unwrap(), params)
                        .unwrap(),
                )
            };
            let enabled_key = storage_key("AdminUtils", "HashedAccountsEnabled", &[]);
            let authority_key = storage_key(
                "HashedAccounts",
                if is_pq_crypto(crypto) {
                    "MlDsaAccounts"
                } else {
                    "Accounts"
                },
                &address,
            );
            let account_key = storage_key("System", "Account", &address);
            let next_key = storage_key("MevShield", "NextKey", &[]);
            let events_key = storage_key("System", "Events", &[]);
            let descriptor = if is_pq_crypto(crypto) {
                signer.mldsa_descriptor().unwrap()
            } else {
                signer.hashed_descriptor().unwrap()
            };
            let authority = |generation: u64| {
                let mut encoded = descriptor.clone();
                encoded.extend(generation.encode());
                encoded.extend(
                    signer
                        .at_generation(generation)
                        .unwrap()
                        .authorization_current_commitment()
                        .unwrap(),
                );
                encoded
            };
            let (old_authority, new_authority) = if crate::keys::is_fixed_pq_crypto(crypto) {
                // A fixed signer must never read this deliberately absent record.
                (Vec::new(), Vec::new())
            } else {
                (authority(GENERATION), authority(GENERATION + 1))
            };
            let (decrypt_key, encrypt_key) = MlKem768::generate(&mut rand_core::OsRng);
            let key_info = storage_info(&runtime, "MevShield", "NextKey").unwrap();
            let mut encoded_key = Vec::new();
            runtime
                .encode_id(
                    key_info.value_type,
                    &Value::Bytes(encrypt_key.as_bytes().to_vec()),
                    &mut encoded_key,
                )
                .unwrap();
            let state = Arc::new(Mutex::new(NodeState::default()));
            let server_state = Arc::clone(&state);
            let stop = Arc::new(AtomicBool::new(false));
            let server_stop = Arc::clone(&stop);
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            let server = thread::spawn(move || {
                while !server_stop.load(Ordering::Relaxed) {
                    let (mut stream, _) = match listener.accept() {
                        Ok(value) => value,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(1));
                            continue;
                        }
                        Err(error) => panic!("mock accept failed: {error}"),
                    };
                    stream
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .unwrap();
                    let mut reader = BufReader::new(&mut stream);
                    let mut length = 0;
                    loop {
                        let mut line = String::new();
                        assert_ne!(reader.read_line(&mut line).unwrap(), 0);
                        if line == "\r\n" {
                            break;
                        }
                        if let Some(value) = line.to_lowercase().strip_prefix("content-length:") {
                            length = value.trim().parse::<usize>().unwrap();
                        }
                    }
                    let mut body = vec![0; length];
                    reader.read_exact(&mut body).unwrap();
                    let request: JsonValue = serde_json::from_slice(&body).unwrap();
                    let mut state = server_state.lock().unwrap();
                    let hash = if state.advanced {
                        ADVANCED_HASH
                    } else {
                        PINNED_HASH
                    };
                    let result = match request["method"].as_str().unwrap() {
                        "chain_getFinalizedHead" => {
                            state.finalized_reads += 1;
                            json!(hex_prefixed(&hash))
                        }
                        "chain_getBlockHash" => json!(hex_prefixed(&hash)),
                        "chain_getHeader" => {
                            json!({"number":"0xa", "parentHash":hex_prefixed(&PINNED_HASH)})
                        }
                        "chain_getBlock" => {
                            state.block_reads += 1;
                            json!({"block":{"extrinsics":state.included.iter().map(|xt| hex_prefixed(xt)).collect::<Vec<_>>()}})
                        }
                        "system_accountNextIndex" => {
                            state.nonce_reads += 1;
                            // Preparation and the inner proof already checked
                            // the nonce. Advance finality just before the
                            // carrier's check, after encryption has completed.
                            if state.advance_after_inner && state.nonce_reads == 3 {
                                state.advanced = true;
                            }
                            let nonce = NONCE
                                + u64::from(
                                    state.pending
                                        || (state.advanced && state.consume_nonce_on_advance),
                                );
                            json!(state.nonce_override.unwrap_or(nonce))
                        }
                        "state_getStorage" => {
                            let key = request["params"][0].as_str().unwrap();
                            let block = request["params"].get(1).and_then(JsonValue::as_str);
                            if key == next_key {
                                json!(hex_prefixed(&encoded_key))
                            } else if key == events_key {
                                json!(hex_prefixed(&state.events))
                            } else {
                                let block = block.expect("authority reads must specify a block");
                                let new = block == hex_prefixed(&ADVANCED_HASH);
                                assert!(new || block == hex_prefixed(&PINNED_HASH));
                                if key == enabled_key {
                                    json!("0x01")
                                } else if key == authority_key {
                                    state.authority_reads.push(block.into());
                                    json!(hex_prefixed(if new && state.consume_nonce_on_advance {
                                        &new_authority
                                    } else {
                                        &old_authority
                                    }))
                                } else if key == account_key {
                                    state.account_reads.push(block.into());
                                    json!(hex_prefixed(
                                        &(NONCE + u64::from(new && state.consume_nonce_on_advance))
                                            .encode()
                                    ))
                                } else {
                                    panic!("unexpected storage key {key}")
                                }
                            }
                        }
                        "author_submitExtrinsic" => {
                            state
                                .submitted
                                .push(decode_hex(request["params"][0].as_str().unwrap()).unwrap());
                            JsonValue::Null
                        }
                        method => panic!("unexpected RPC {method}"),
                    };
                    let response = if request["method"] == "author_submitExtrinsic" {
                        // Capture signing output without simulating block execution.
                        json!({"jsonrpc":"2.0", "id":request["id"], "error":{"code":1010,"message":"captured by signing test"}})
                    } else { json!({"jsonrpc":"2.0", "id":request["id"], "result":result}) }.to_string();
                    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).unwrap();
                }
            });
            Self {
                client: Client {
                    endpoint,
                    http: HttpClient::builder()
                        .no_proxy()
                        .timeout(Duration::from_secs(5))
                        .build()
                        .unwrap(),
                    next_id: AtomicU64::new(1),
                    runtime: RwLock::new(Arc::new(runtime)),
                    genesis_hash: PINNED_HASH,
                    ss58_format: 42,
                },
                state,
                stop,
                server: Some(server),
                decrypt_key,
            }
        }

        fn call(&self) -> Vec<u8> {
            self.client
                .compose_call(
                    "System",
                    "remark",
                    &Value::record(vec![("remark".into(), Value::Bytes(vec![42]))]),
                )
                .unwrap()
        }

        fn decrypt(&self, ciphertext: &[u8]) -> Vec<u8> {
            let blob = &ciphertext[16..];
            let kem_len = usize::from(u16::from_le_bytes([blob[0], blob[1]]));
            let kem = ml_kem::Ciphertext::<MlKem768>::try_from(&blob[2..2 + kem_len]).unwrap();
            let shared = self.decrypt_key.decapsulate(&kem).unwrap();
            let nonce = &blob[2 + kem_len..2 + kem_len + 24];
            XChaCha20Poly1305::new_from_slice(shared.as_ref())
                .unwrap()
                .decrypt(XNonce::from_slice(nonce), &blob[2 + kem_len + 24..])
                .unwrap()
        }
    }

    fn assert_proof(
        node: &MockNode,
        signer: &Keypair,
        extrinsic: &[u8],
        call: &[u8],
        generation: u64,
        nonce: u64,
    ) {
        use sp_core::Pair;
        let runtime = node.client.runtime().unwrap();
        let decoded = runtime.decode_extrinsic(extrinsic, true).unwrap();
        assert_eq!(
            field(&decoded, "nonce").and_then(as_u128),
            Some(u128::from(nonce))
        );
        let era = match field(&decoded, "era").unwrap() {
            Value::List(parts) | Value::Tuple(parts) => Value::record(vec![
                ("period".into(), parts[0].clone()),
                ("phase".into(), parts[1].clone()),
            ]),
            value => value.clone(),
        };
        let params = TxParams {
            era,
            nonce,
            tip: 0,
            tip_asset_id: None,
            genesis_hash: PINNED_HASH,
            era_block_hash: PINNED_HASH,
            metadata_hash: None,
        };
        let (extra, additional) = runtime.signature_payload_parts(&params).unwrap();
        let mut payload = call.to_vec();
        payload.extend(extra);
        payload.extend(additional);
        let mut bytes = extrinsic;
        let _ = codec::Compact::<u32>::decode(&mut bytes).unwrap();
        assert_eq!(u8::decode(&mut bytes).unwrap(), 0x84);
        assert_eq!(u8::decode(&mut bytes).unwrap(), 2);
        let address = Vec::<u8>::decode(&mut bytes).unwrap();
        if crate::keys::is_falcon_crypto(signer.crypto_type()) {
            let proof = subtensor_hashed::falcon::SignerAddress::from_bytes(&address).unwrap();
            assert_eq!(proof.generation, generation);
            assert_eq!(proof.account, signer.public_key_bytes());
            assert!(proof.verify(&payload));
        } else if is_pq_crypto(signer.crypto_type()) {
            let proof =
                subtensor_hashed::mldsa::SignerAddress::decode(&mut address.as_slice()).unwrap();
            assert_eq!(proof.generation, generation);
            assert_eq!(proof.account, signer.public_key_bytes());
            assert!(proof.verify(&payload));
        } else {
            let proof = subtensor_hashed::SignerAddress::decode(&mut address.as_slice()).unwrap();
            assert_eq!(proof.generation, generation);
            assert_eq!(proof.account, signer.public_key_bytes());
            let scheme = u8::decode(&mut bytes).unwrap();
            let signature = <[u8; 64]>::decode(&mut bytes).unwrap();
            let payload = proof.signing_hash(&payload);
            let valid = if scheme == 1 {
                sp_core::sr25519::Pair::verify(
                    &sp_core::sr25519::Signature::from_raw(signature),
                    payload,
                    &sp_core::sr25519::Public::from_raw(proof.public_key),
                )
            } else {
                assert_eq!(scheme, 0);
                sp_core::ed25519::Pair::verify(
                    &sp_core::ed25519::Signature::from_raw(signature),
                    payload,
                    &sp_core::ed25519::Public::from_raw(proof.public_key),
                )
            };
            assert!(valid);
        }
    }

    fn wallet_modes() -> [u8; 5] {
        [
            crate::keys::CRYPTO_HASHED,
            crate::keys::CRYPTO_HASHED_ED25519,
            crate::keys::CRYPTO_MLDSA65,
            crate::keys::CRYPTO_FALCON512_HASHED,
            crate::keys::CRYPTO_FALCON1024_HASHED,
        ]
    }

    fn shield_events(node: &MockNode, inner_success: bool) -> Vec<u8> {
        let dispatch_info = Value::record(vec![
            (
                "weight".into(),
                Value::record(vec![
                    ("ref_time".into(), Value::Uint(1)),
                    ("proof_size".into(), Value::Uint(0)),
                ]),
            ),
            ("class".into(), Value::str("Normal")),
            ("pays_fee".into(), Value::str("Yes")),
        ]);
        let event = |index, pallet: &str, name: &str, attributes| {
            Value::record(vec![
                (
                    "phase".into(),
                    Value::record(vec![("ApplyExtrinsic".into(), Value::Uint(index))]),
                ),
                (
                    "event".into(),
                    Value::record(vec![(
                        pallet.into(),
                        Value::record(vec![(name.into(), attributes)]),
                    )]),
                ),
                ("topics".into(), Value::List(vec![])),
            ])
        };
        let mut records = vec![event(
            0,
            "System",
            "ExtrinsicSuccess",
            Value::record(vec![("dispatch_info".into(), dispatch_info.clone())]),
        )];
        records.push(event(
            1,
            "System",
            if inner_success {
                "ExtrinsicSuccess"
            } else {
                "ExtrinsicFailed"
            },
            Value::record(if inner_success {
                vec![("dispatch_info".into(), dispatch_info)]
            } else {
                vec![
                    ("dispatch_error".into(), Value::str("BadOrigin")),
                    ("dispatch_info".into(), dispatch_info),
                ]
            }),
        ));
        records.push(event(
            1,
            "TransactionPayment",
            "TransactionFeePaid",
            Value::record(vec![
                ("who".into(), Value::Bytes(vec![42; 32])),
                ("actual_fee".into(), Value::Uint(3)),
                ("tip".into(), Value::Uint(0)),
            ]),
        ));
        let runtime = node.client.runtime().unwrap();
        let mut encoded = Vec::new();
        runtime
            .encode_id(
                storage_info(&runtime, "System", "Events")
                    .unwrap()
                    .value_type,
                &Value::List(records),
                &mut encoded,
            )
            .unwrap();
        encoded
    }

    fn successful_carrier() -> TxOutcome {
        let mut carrier = TxOutcome::pool_rejection("carrier".into(), "success".into());
        carrier.success = true;
        carrier.block_number = Some(10);
        carrier.fee_rao = Some(7);
        carrier
    }

    #[test]
    fn shield_reports_inner_dispatch_outcome_and_both_fees() {
        for success in [false, true] {
            let signer = Keypair::from_seed(&[34; 32], crate::keys::CRYPTO_MLDSA65_FIXED).unwrap();
            let node = MockNode::new(&signer);
            let inner = vec![42];
            let events = shield_events(&node, success);
            {
                let mut state = node.state.lock().unwrap();
                state.included = vec![vec![41], inner.clone()];
                state.events = events;
            }
            let result = node
                .client
                .resolve_shielded_inner(
                    &inner,
                    "inner".into(),
                    successful_carrier(),
                    true,
                    Duration::from_secs(5),
                )
                .unwrap();
            assert_eq!(result.success, success);
            assert_eq!(
                result.error.as_ref().map(|error| error.name.as_str()),
                if success { None } else { Some("BadOrigin") }
            );
            assert_eq!(result.extrinsic_hash, "inner");
            assert_eq!(result.extrinsic_index, Some(1));
            assert_eq!(result.fee_rao, Some(10));
            assert_eq!(result.data["carrier_extrinsic_hash"], Value::str("carrier"));
            assert_eq!(result.data["carrier_fee_rao"], Value::Uint(7));
            assert_eq!(result.data["inner_fee_rao"], Value::Uint(3));
        }
    }

    #[test]
    fn shield_carrier_success_does_not_hide_missing_inner_inclusion() {
        let signer = Keypair::from_seed(&[35; 32], crate::keys::CRYPTO_MLDSA65_FIXED).unwrap();
        let node = MockNode::new(&signer);
        node.state.lock().unwrap().included = vec![vec![41]];
        let result = node
            .client
            .resolve_shielded_inner(
                &[42],
                "inner".into(),
                successful_carrier(),
                false,
                Duration::from_millis(100),
            )
            .unwrap();
        assert!(!result.success);
        assert!(result.message.contains("not included"));
        assert_eq!(result.extrinsic_hash, "inner");
        assert_eq!(result.fee_rao, Some(7));
        assert_eq!(result.data["carrier_extrinsic_hash"], Value::str("carrier"));
        assert!(node.state.lock().unwrap().block_reads > 0);
    }

    #[test]
    fn fixed_ms_signs_queued_nonces_without_authority_records() {
        for crypto in [
            crate::keys::CRYPTO_MLDSA65_FIXED,
            crate::keys::CRYPTO_FALCON512,
            crate::keys::CRYPTO_FALCON1024,
        ] {
            let signer = Keypair::from_seed(&[31; 32], crypto).unwrap();
            let node = MockNode::new(&signer);
            let call = node.call();
            node.state.lock().unwrap().pending = true;
            for nonce in [NONCE, NONCE + 1, NONCE + 2] {
                let (encoded, _) = node
                    .client
                    .sign_extrinsic(&call, &signer, nonce, None)
                    .unwrap();
                assert_proof(&node, &signer, &encoded, &call, 0, nonce);
            }
            let state = node.state.lock().unwrap();
            assert!(state.authority_reads.is_empty());
            assert!(state.account_reads.is_empty());
        }
    }

    #[test]
    fn fixed_ms_shield_signs_both_nonces_with_the_same_key() {
        for crypto in [
            crate::keys::CRYPTO_MLDSA65_FIXED,
            crate::keys::CRYPTO_FALCON512,
            crate::keys::CRYPTO_FALCON1024,
        ] {
            let signer = Keypair::from_seed(&[32; 32], crypto).unwrap();
            let node = MockNode::new(&signer);
            let call = node.call();
            let outcome = node.client.submit_shielded(&call, &signer, false).unwrap();
            assert!(!outcome.success); // The server captures, then rejects the carrier.
            let state = node.state.lock().unwrap();
            assert!(state.authority_reads.is_empty());
            assert!(state.account_reads.is_empty());
            assert_eq!(state.finalized_reads, 1);
            let outer = &state.submitted[0];
            let runtime = node.client.runtime().unwrap();
            let decoded = runtime.decode_extrinsic(outer, true).unwrap();
            let Value::List(args) = field(field(&decoded, "call").unwrap(), "call_args").unwrap()
            else {
                panic!("decoded call arguments")
            };
            let ciphertext = args
                .iter()
                .find_map(|arg| {
                    (field(arg, "name") == Some(&Value::str("ciphertext")))
                        .then(|| value_bytes(field(arg, "value").unwrap()).unwrap())
                })
                .unwrap();
            let inner = node.decrypt(&ciphertext);
            let outer_call = runtime
                .compose_call(
                    "MevShield",
                    "submit_encrypted",
                    &Value::record(vec![("ciphertext".into(), Value::Bytes(ciphertext))]),
                )
                .unwrap();
            assert_proof(&node, &signer, outer, &outer_call, 0, NONCE);
            assert_proof(&node, &signer, &inner, &call, 0, NONCE + 1);
        }
    }

    #[test]
    fn shield_uses_short_mortal_eras_for_every_wallet_mode() {
        for crypto in [0, 1, 4, 6, 7, 8] {
            let signer = Keypair::from_seed(&[36; 32], crypto).unwrap();
            let fixture = Keypair::from_seed(&[36; 32], crate::keys::CRYPTO_HASHED).unwrap();
            let node = MockNode::new(if crypto <= 1 { &fixture } else { &signer });
            let call = node.call();
            let outcome = node.client.submit_shielded(&call, &signer, false).unwrap();
            assert!(!outcome.success); // The server captures, then rejects the carrier.
            let outer = node.state.lock().unwrap().submitted[0].clone();
            let runtime = node.client.runtime().unwrap();
            let decoded = runtime.decode_extrinsic(&outer, true).unwrap();
            let Value::List(args) = field(field(&decoded, "call").unwrap(), "call_args").unwrap()
            else {
                panic!("decoded call arguments")
            };
            let ciphertext = args
                .iter()
                .find_map(|arg| {
                    (field(arg, "name") == Some(&Value::str("ciphertext")))
                        .then(|| value_bytes(field(arg, "value").unwrap()).unwrap())
                })
                .unwrap();
            let inner = node.decrypt(&ciphertext);
            for extrinsic in [&outer, &inner] {
                let decoded = runtime.decode_extrinsic(extrinsic, true).unwrap();
                let era = field(&decoded, "era").unwrap();
                assert_eq!(tuple_item(era, 0).and_then(as_u128), Some(8));
            }
            let (ordinary, _) = node
                .client
                .sign_extrinsic(&call, &signer, NONCE, Some(DEFAULT_ERA_PERIOD))
                .unwrap();
            let decoded = runtime.decode_extrinsic(&ordinary, true).unwrap();
            assert_eq!(
                tuple_item(field(&decoded, "era").unwrap(), 0).and_then(as_u128),
                Some(64)
            );
        }
    }

    #[test]
    fn fixed_ms_shield_rejects_exhausted_inner_nonce_before_submitting() {
        for crypto in [
            crate::keys::CRYPTO_MLDSA65_FIXED,
            crate::keys::CRYPTO_FALCON512,
            crate::keys::CRYPTO_FALCON1024,
        ] {
            let signer = Keypair::from_seed(&[33; 32], crypto).unwrap();
            let node = MockNode::new(&signer);
            node.state.lock().unwrap().nonce_override = Some(u64::from(u32::MAX) - 1);
            let error = node
                .client
                .submit_shielded(&node.call(), &signer, false)
                .unwrap_err();
            assert!(error.to_string().contains("nonce exhausted"));
            assert!(node.state.lock().unwrap().submitted.is_empty());
        }
    }

    #[test]
    fn standard_keys_keep_explicit_nonce_signing_without_registered_authority() {
        for crypto in [crate::keys::CRYPTO_ED25519, crate::keys::CRYPTO_SR25519] {
            let signer = Keypair::from_seed(&[26; 32], crypto).unwrap();
            let fixture = Keypair::from_seed(&[26; 32], crate::keys::CRYPTO_HASHED).unwrap();
            let node = MockNode::new(&fixture);
            let (encoded, _) = node
                .client
                .sign_extrinsic(&node.call(), &signer, NONCE + 7, None)
                .unwrap();
            let decoded = node
                .client
                .runtime()
                .unwrap()
                .decode_extrinsic(&encoded, true)
                .unwrap();
            assert_eq!(
                field(&decoded, "nonce").and_then(as_u128),
                Some(u128::from(NONCE + 7))
            );
            let state = node.state.lock().unwrap();
            assert_eq!(state.finalized_reads, 0);
            assert_eq!(state.nonce_reads, 0);
            assert!(state.authority_reads.is_empty());
            assert!(state.account_reads.is_empty());
        }
    }

    #[test]
    fn shield_pair_pins_authority_and_nonce_when_finality_advances() {
        for crypto in wallet_modes() {
            let signer = Keypair::from_seed(&[27; 32], crypto).unwrap();
            let node = MockNode::new(&signer);
            node.state.lock().unwrap().advance_after_inner = true;
            let call = node.call();
            let outcome = node.client.submit_shielded(&call, &signer, false).unwrap();
            assert!(!outcome.success); // Mock node only captures the signed carrier.
            let state = node.state.lock().unwrap();
            assert!(state.advanced);
            assert_eq!(state.finalized_reads, 1);
            assert_eq!(state.authority_reads, [hex_prefixed(&PINNED_HASH)]);
            assert_eq!(state.account_reads, [hex_prefixed(&PINNED_HASH)]);
            assert_eq!(state.submitted.len(), 1);
            let outer = &state.submitted[0];
            let decoded = node
                .client
                .runtime()
                .unwrap()
                .decode_extrinsic(outer, true)
                .unwrap();
            let outer_call = field(&decoded, "call").unwrap();
            let args = field(outer_call, "call_args").unwrap();
            let Value::List(args) = args else {
                panic!("decoded call arguments")
            };
            let ciphertext = args
                .iter()
                .find_map(|arg| {
                    (field(arg, "name") == Some(&Value::str("ciphertext")))
                        .then(|| value_bytes(field(arg, "value").unwrap()).unwrap())
                })
                .unwrap();
            let inner = node.decrypt(&ciphertext);
            let encoded_outer = node
                .client
                .compose_call(
                    "MevShield",
                    "submit_encrypted",
                    &Value::record(vec![("ciphertext".into(), Value::Bytes(ciphertext))]),
                )
                .unwrap();
            assert_proof(&node, &signer, outer, &encoded_outer, GENERATION, NONCE);
            assert_proof(&node, &signer, &inner, &call, GENERATION + 1, NONCE + 1);
        }
    }

    #[test]
    fn shield_rejects_nonce_consumed_between_inner_and_carrier() {
        for crypto in wallet_modes() {
            let signer = Keypair::from_seed(&[28; 32], crypto).unwrap();
            let node = MockNode::new(&signer);
            {
                let mut state = node.state.lock().unwrap();
                state.advance_after_inner = true;
                state.consume_nonce_on_advance = true;
            }
            let error = node
                .client
                .submit_shielded(&node.call(), &signer, false)
                .unwrap_err();
            assert!(error.to_string().contains("pending transaction"));
            let state = node.state.lock().unwrap();
            assert!(state.advanced);
            assert_eq!(state.authority_reads, [hex_prefixed(&PINNED_HASH)]);
            assert!(state.submitted.is_empty());
        }
    }

    #[test]
    fn registered_submissions_reject_pending_and_explicit_future_nonces_then_refresh() {
        for crypto in wallet_modes() {
            let signer = Keypair::from_seed(&[29; 32], crypto).unwrap();
            let node = MockNode::new(&signer);
            let call = node.call();
            node.state.lock().unwrap().pending = true;
            let error = node
                .client
                .submit(&call, &signer, None, Some(DEFAULT_ERA_PERIOD), false)
                .unwrap_err();
            assert!(error.to_string().contains("pending transaction"));
            node.state.lock().unwrap().pending = false;
            let error = node
                .client
                .sign_extrinsic(&call, &signer, NONCE + 1, Some(DEFAULT_ERA_PERIOD))
                .unwrap_err();
            assert!(error.to_string().contains("current on-chain nonce"));
            let (first, _) = node
                .client
                .sign_extrinsic(&call, &signer, NONCE, Some(DEFAULT_ERA_PERIOD))
                .unwrap();
            assert_proof(&node, &signer, &first, &call, GENERATION, NONCE);
            {
                let mut state = node.state.lock().unwrap();
                state.advanced = true;
                state.consume_nonce_on_advance = true;
            }
            let (next, _) = node
                .client
                .sign_extrinsic(&call, &signer, NONCE + 1, None)
                .unwrap();
            assert_proof(&node, &signer, &next, &call, GENERATION + 1, NONCE + 1);
            assert!(node.state.lock().unwrap().submitted.is_empty());
        }
    }
}
