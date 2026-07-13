use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use bittensor_core::client::{
    BlockHeader, DispatchError, ExternalSigner, ExternalSigningOptions, ExternalSigningPlan,
    MetadataHashMode, SubnetHyperparameter, SubnetHyperparameterValue, SubnetInfo, SwapQuote,
    TxOutcome, TxWait, DEFAULT_RECEIPT_TIMEOUT,
};
use bittensor_core::codec::Value;
use bittensor_core::digest::ChainInfo;
use bittensor_core::transaction::{Executor, IntentCall, Plan, Policy, SignerRole, Spend, Wallet};
use bittensor_core::{Client, CoreError};
use napi::bindgen_prelude::{AsyncTask, BigInt, Buffer, PromiseRaw, Unknown};
use napi::{Env, JsValue, ScopedTask, Task};
use napi_derive::napi;
use serde_json::Value as JsonValue;

use crate::errors::{into_napi, invalid_arg, CoreResultExt, NapiResult};
use crate::keys::NativeKeypair;
use crate::runtime::{NativeRuntime, NativeSignerPayload, NativeTxParams};
use crate::values::{from_wire, to_wire};

#[napi(object)]
pub struct NativePolicyOptions {
    pub max_fee_rao: Option<BigInt>,
    pub max_spend_rao: Option<BigInt>,
    pub allowed_netuids: Option<Vec<u16>>,
    pub allow_raw_calls: Option<bool>,
    pub allow_global: Option<bool>,
}

#[napi(object)]
pub struct NativeSubmitOptions {
    pub wait_for_inclusion: Option<bool>,
    pub wait_for_finalization: Option<bool>,
    pub timeout_ms: Option<BigInt>,
}

#[napi(object)]
pub struct NativePlan {
    pub op: String,
    pub summary: String,
    pub signer_role: String,
    pub signer_address: String,
    pub fee_rao: Option<String>,
    pub warnings: Vec<String>,
    pub violations: Vec<String>,
    pub ok: bool,
    pub call_data: Buffer,
}

#[napi(object)]
pub struct NativeDispatchError {
    pub pallet: Option<String>,
    pub name: String,
    pub docs: Vec<String>,
    pub semantic_code: String,
}

#[napi(object)]
pub struct NativeTxOutcome {
    pub success: bool,
    pub extrinsic_hash: String,
    pub block_hash: Option<String>,
    pub block_number: Option<BigInt>,
    pub extrinsic_index: Option<u32>,
    pub fee_rao: Option<String>,
    pub events: Vec<JsonValue>,
    pub error: Option<NativeDispatchError>,
    pub message: String,
    pub data: JsonValue,
}

#[napi(object)]
pub struct NativeBlockHeader {
    pub hash: String,
    pub parent_hash: String,
    pub number: BigInt,
}

#[napi(object)]
pub struct NativeSubnetInfo {
    pub netuid: u16,
    pub tempo: u16,
    pub burn_rao: String,
    pub neuron_count: u16,
}

#[napi(object)]
pub struct NativeSubnetHyperparameter {
    pub name: String,
    pub value_type: String,
    pub value: JsonValue,
}

#[napi(object)]
pub struct NativeSwapQuote {
    pub tao_amount: String,
    pub alpha_amount: String,
    pub tao_fee: String,
    pub alpha_fee: String,
    pub tao_slippage: String,
    pub alpha_slippage: String,
}

#[napi(object)]
pub struct NativeSignedExtrinsic {
    pub bytes: Buffer,
    pub hash: String,
}

#[napi(object)]
pub struct NativeExternalSigningOptions {
    pub nonce: Option<BigInt>,
    pub period: Option<BigInt>,
    pub immortal: Option<bool>,
    pub tip: Option<BigInt>,
    pub tip_asset_id: Option<BigInt>,
    pub metadata_hash_mode: Option<String>,
    pub metadata_hash: Option<Buffer>,
}

#[napi(object)]
pub struct NativeExternalSigner {
    pub signer_address: String,
    pub public_key: Buffer,
    pub crypto_type: u32,
    pub requires_metadata_proof: bool,
}

#[napi(object)]
pub struct NativeChainInfo {
    pub spec_version: u32,
    pub spec_name: String,
    pub base58_prefix: u16,
    pub decimals: u8,
    pub token_symbol: String,
}

#[napi]
pub struct NativeExternalSigningPlan {
    pub(crate) inner: Arc<ExternalSigningPlan>,
}

#[napi]
pub struct NativeCancellationToken {
    cancelled: Arc<AtomicBool>,
}

#[napi]
impl NativeCancellationToken {
    #[napi(constructor)]
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    #[napi]
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    #[napi(getter)]
    pub fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

type ClientJob<T> = Box<dyn FnOnce(&Client) -> Result<T, CoreError> + Send + 'static>;

fn task_already_completed() -> napi::Error {
    napi::Error::from_reason("native async task was already completed")
}

macro_rules! client_task {
    ($name:ident, $output:ty, $js:ty, $resolve:expr) => {
        pub struct $name {
            client: Arc<Client>,
            job: Option<ClientJob<$output>>,
        }

        impl $name {
            fn new<F>(client: Arc<Client>, job: F) -> Self
            where
                F: FnOnce(&Client) -> Result<$output, CoreError> + Send + 'static,
            {
                Self {
                    client,
                    job: Some(Box::new(job)),
                }
            }
        }

        impl Task for $name {
            type Output = $output;
            type JsValue = $js;

            fn compute(&mut self) -> napi::Result<Self::Output> {
                let job = self.job.take().ok_or_else(task_already_completed)?;
                job(&self.client).napi()
            }

            fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
                ($resolve)(output)
            }
        }
    };
}

pub struct ClientConnectTask {
    endpoints: Vec<String>,
}

impl Task for ClientConnectTask {
    type Output = Client;
    type JsValue = NativeClient;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        Client::connect_many(self.endpoints.clone()).napi()
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(NativeClient {
            inner: Arc::new(output),
        })
    }
}

client_task!(ClientStringTask, String, String, Ok);
client_task!(ClientBoolTask, bool, bool, Ok);
client_task!(ClientU64Task, u64, BigInt, |value| Ok(BigInt::from(value)));
client_task!(ClientHeaderTask, BlockHeader, NativeBlockHeader, |value| {
    Ok(header_to_native(value))
});
client_task!(ClientBytesTask, Vec<u8>, Buffer, |value: Vec<u8>| {
    Ok(value.into())
});
client_task!(
    ClientSignedExtrinsicTask,
    (Vec<u8>, String),
    NativeSignedExtrinsic,
    |(bytes, hash): (Vec<u8>, String)| Ok(NativeSignedExtrinsic {
        bytes: bytes.into(),
        hash,
    })
);
client_task!(
    ClientExternalSigningPlanTask,
    ExternalSigningPlan,
    NativeExternalSigningPlan,
    |plan: ExternalSigningPlan| Ok(NativeExternalSigningPlan {
        inner: Arc::new(plan),
    })
);
client_task!(
    ClientSubnetsTask,
    Vec<SubnetInfo>,
    Vec<NativeSubnetInfo>,
    |items: Vec<SubnetInfo>| Ok(items.into_iter().map(subnet_to_native).collect())
);
client_task!(
    ClientSubnetHyperparametersTask,
    Option<Vec<SubnetHyperparameter>>,
    Option<Vec<NativeSubnetHyperparameter>>,
    |items: Option<Vec<SubnetHyperparameter>>| items
        .map(|items| {
            items
                .into_iter()
                .map(subnet_hyperparameter_to_native)
                .collect::<NapiResult<Vec<_>>>()
        })
        .transpose()
);
client_task!(ClientSwapQuoteTask, SwapQuote, NativeSwapQuote, |value| {
    Ok(quote_to_native(value))
});
client_task!(ClientChainInfoTask, ChainInfo, NativeChainInfo, |value| {
    Ok(chain_info_to_native(value))
});

fn spawn_tx_outcome<F>(
    env: &Env,
    client: Arc<Client>,
    cancellation: Option<&NativeCancellationToken>,
    job: F,
) -> NapiResult<PromiseRaw<'static, NativeTxOutcome>>
where
    F: FnOnce(&Client, Option<&AtomicBool>) -> Result<TxOutcome, CoreError> + Send + 'static,
{
    let cancelled = cancellation.map(|token| Arc::clone(&token.cancelled));
    let (deferred, promise) = env.create_deferred::<NativeTxOutcome, _>()?;
    let promise = PromiseRaw::new(env.raw(), promise.raw());
    thread::spawn(move || {
        let result = job(&client, cancelled.as_deref());
        match result {
            Ok(outcome) => deferred.resolve(move |_env| outcome_to_native(outcome)),
            Err(error) => deferred.reject(into_napi(error)),
        }
    });
    Ok(promise)
}

pub struct ClientJsonTask {
    client: Arc<Client>,
    job: Option<ClientJob<JsonValue>>,
}

impl ClientJsonTask {
    fn new<F>(client: Arc<Client>, job: F) -> Self
    where
        F: FnOnce(&Client) -> Result<JsonValue, CoreError> + Send + 'static,
    {
        Self {
            client,
            job: Some(Box::new(job)),
        }
    }
}

impl<'task> ScopedTask<'task> for ClientJsonTask {
    type Output = JsonValue;
    type JsValue = Unknown<'task>;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        let job = self.job.take().ok_or_else(task_already_completed)?;
        job(&self.client).napi()
    }

    fn resolve(&mut self, env: &'task Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        env.to_js_value(&output)
    }
}

pub struct ClientWireValueTask {
    client: Arc<Client>,
    job: Option<ClientJob<Value>>,
}

impl ClientWireValueTask {
    fn new<F>(client: Arc<Client>, job: F) -> Self
    where
        F: FnOnce(&Client) -> Result<Value, CoreError> + Send + 'static,
    {
        Self {
            client,
            job: Some(Box::new(job)),
        }
    }
}

impl<'task> ScopedTask<'task> for ClientWireValueTask {
    type Output = Value;
    type JsValue = Unknown<'task>;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        let job = self.job.take().ok_or_else(task_already_completed)?;
        job(&self.client).napi()
    }

    fn resolve(&mut self, env: &'task Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        env.to_js_value(&to_wire(&output)?)
    }
}

pub struct ClientWireValuesTask {
    client: Arc<Client>,
    job: Option<ClientJob<Vec<Value>>>,
}

impl ClientWireValuesTask {
    fn new<F>(client: Arc<Client>, job: F) -> Self
    where
        F: FnOnce(&Client) -> Result<Vec<Value>, CoreError> + Send + 'static,
    {
        Self {
            client,
            job: Some(Box::new(job)),
        }
    }
}

impl<'task> ScopedTask<'task> for ClientWireValuesTask {
    type Output = Vec<Value>;
    type JsValue = Unknown<'task>;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        let job = self.job.take().ok_or_else(task_already_completed)?;
        job(&self.client).napi()
    }

    fn resolve(&mut self, env: &'task Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        let wire = output
            .iter()
            .map(to_wire)
            .collect::<napi::Result<Vec<_>>>()?;
        env.to_js_value(&wire)
    }
}

pub struct ClientMapTask {
    client: Arc<Client>,
    job: Option<ClientJob<Vec<(Value, Value)>>>,
}

impl ClientMapTask {
    fn new<F>(client: Arc<Client>, job: F) -> Self
    where
        F: FnOnce(&Client) -> Result<Vec<(Value, Value)>, CoreError> + Send + 'static,
    {
        Self {
            client,
            job: Some(Box::new(job)),
        }
    }
}

impl<'task> ScopedTask<'task> for ClientMapTask {
    type Output = Vec<(Value, Value)>;
    type JsValue = Unknown<'task>;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        let job = self.job.take().ok_or_else(task_already_completed)?;
        job(&self.client).napi()
    }

    fn resolve(&mut self, env: &'task Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        let wire = output
            .iter()
            .map(|(key, value)| {
                let mut item = serde_json::Map::new();
                item.insert("key".to_owned(), to_wire(key)?);
                item.insert("value".to_owned(), to_wire(value)?);
                Ok(JsonValue::Object(item))
            })
            .collect::<napi::Result<Vec<_>>>()?;
        env.to_js_value(&wire)
    }
}

#[napi]
pub enum NativeSignerRole {
    Coldkey,
    Hotkey,
}

#[napi]
pub enum NativeSpendKind {
    None,
    Bounded,
    Unbounded,
}

#[napi]
pub struct NativePolicy {
    pub(crate) inner: Policy,
}

#[napi]
impl NativePolicy {
    #[napi(factory, js_name = "fromOptions")]
    pub fn from_options(options: Option<NativePolicyOptions>) -> napi::Result<Self> {
        let Some(options) = options else {
            return Ok(Self {
                inner: Policy::default(),
            });
        };
        Ok(Self {
            inner: Policy {
                max_fee_rao: options
                    .max_fee_rao
                    .as_ref()
                    .map(|value| bigint_u128("maxFeeRao", value))
                    .transpose()?,
                max_spend_rao: options
                    .max_spend_rao
                    .as_ref()
                    .map(|value| bigint_u128("maxSpendRao", value))
                    .transpose()?,
                allowed_netuids: options
                    .allowed_netuids
                    .map(|items| items.into_iter().collect()),
                allow_raw_calls: options.allow_raw_calls.unwrap_or(false),
                allow_global: options.allow_global.unwrap_or(false),
            },
        })
    }

    #[napi(getter)]
    pub fn allow_raw_calls(&self) -> bool {
        self.inner.allow_raw_calls
    }

    #[napi(getter, js_name = "allowGlobal")]
    pub fn allow_global(&self) -> bool {
        self.inner.allow_global
    }

    #[napi(js_name = "check")]
    pub fn check(
        &self,
        intent: &NativeIntentCall,
        fee_rao: Option<BigInt>,
    ) -> NapiResult<Vec<String>> {
        let fee = fee_rao
            .as_ref()
            .map(|value| bigint_u128("feeRao", value))
            .transpose()?;
        Ok(self.inner.check(&intent.inner, fee))
    }
}

#[napi]
pub struct NativeIntentCall {
    pub(crate) inner: IntentCall,
}

#[napi]
impl NativeIntentCall {
    #[napi(factory, js_name = "rawCall")]
    pub fn raw_call(
        op: String,
        signer_role: NativeSignerRole,
        pallet: String,
        call_function: String,
        params: JsonValue,
    ) -> napi::Result<Self> {
        Ok(Self {
            inner: IntentCall::raw_call(
                op,
                signer_role.into(),
                pallet,
                call_function,
                from_wire(params)?,
            ),
        })
    }

    #[napi(factory)]
    pub fn transfer(dest: String, amount_rao: BigInt) -> napi::Result<Self> {
        Ok(Self {
            inner: IntentCall::transfer(dest, bigint_u128("amountRao", &amount_rao)?),
        })
    }

    #[napi(factory, js_name = "fundEvmKey")]
    pub fn fund_evm_key(mirror: String, amount_rao: BigInt) -> napi::Result<Self> {
        Ok(Self {
            inner: IntentCall::fund_evm_key(mirror, bigint_u128("amountRao", &amount_rao)?),
        })
    }

    #[napi(factory, js_name = "transferAllowDeath")]
    pub fn transfer_allow_death(dest: String, amount_rao: BigInt) -> napi::Result<Self> {
        Ok(Self {
            inner: IntentCall::transfer_allow_death(dest, bigint_u128("amountRao", &amount_rao)?),
        })
    }

    #[napi(factory, js_name = "transferAll")]
    pub fn transfer_all(dest: String, keep_alive: bool) -> Self {
        Self {
            inner: IntentCall::transfer_all(dest, keep_alive),
        }
    }

    #[napi(factory, js_name = "addStake")]
    pub fn add_stake(hotkey: String, netuid: u16, amount_rao: BigInt) -> napi::Result<Self> {
        Ok(Self {
            inner: IntentCall::add_stake(hotkey, netuid, bigint_u128("amountRao", &amount_rao)?),
        })
    }

    #[napi(factory, js_name = "addStakeLimit")]
    pub fn add_stake_limit(
        hotkey: String,
        netuid: u16,
        amount_rao: BigInt,
        limit_price_rao: BigInt,
        allow_partial: bool,
    ) -> napi::Result<Self> {
        Ok(Self {
            inner: IntentCall::add_stake_limit(
                hotkey,
                netuid,
                bigint_u128("amountRao", &amount_rao)?,
                bigint_u128("limitPriceRao", &limit_price_rao)?,
                allow_partial,
            ),
        })
    }

    #[napi(factory, js_name = "removeStake")]
    pub fn remove_stake(
        hotkey: String,
        netuid: u16,
        amount_alpha_rao: BigInt,
    ) -> napi::Result<Self> {
        Ok(Self {
            inner: IntentCall::remove_stake(
                hotkey,
                netuid,
                bigint_u128("amountAlphaRao", &amount_alpha_rao)?,
            ),
        })
    }

    #[napi(factory, js_name = "removeStakeLimit")]
    pub fn remove_stake_limit(
        hotkey: String,
        netuid: u16,
        amount_alpha_rao: BigInt,
        limit_price_rao: BigInt,
        allow_partial: bool,
    ) -> napi::Result<Self> {
        Ok(Self {
            inner: IntentCall::remove_stake_limit(
                hotkey,
                netuid,
                bigint_u128("amountAlphaRao", &amount_alpha_rao)?,
                bigint_u128("limitPriceRao", &limit_price_rao)?,
                allow_partial,
            ),
        })
    }

    #[napi(factory, js_name = "registerSubnet")]
    pub fn register_subnet(hotkey: String) -> Self {
        Self {
            inner: IntentCall::register_subnet(hotkey),
        }
    }

    #[napi(factory, js_name = "startCall")]
    pub fn start_call(netuid: u16) -> Self {
        Self {
            inner: IntentCall::start_call(netuid),
        }
    }

    #[napi(factory, js_name = "setWeights")]
    pub fn set_weights(
        netuid: u16,
        dests: Vec<u16>,
        weights: Vec<u16>,
        version_key: BigInt,
    ) -> napi::Result<Self> {
        Ok(Self {
            inner: IntentCall::set_weights(
                netuid,
                dests,
                weights,
                bigint_u64("versionKey", &version_key)?,
            ),
        })
    }

    #[napi(factory, js_name = "serveAxon")]
    pub fn serve_axon(
        netuid: u16,
        version: u32,
        ip: BigInt,
        port: u16,
        ip_type: u8,
        protocol: u8,
    ) -> napi::Result<Self> {
        IntentCall::serve_axon(
            netuid,
            version,
            bigint_u128("ip", &ip)?,
            port,
            ip_type,
            protocol,
        )
        .napi()
        .map(|inner| Self { inner })
    }

    #[napi(factory, js_name = "burnedRegister")]
    pub fn burned_register(netuid: u16, hotkey: String) -> Self {
        Self {
            inner: IntentCall::burned_register(netuid, hotkey),
        }
    }

    #[napi(factory, js_name = "rootRegister")]
    pub fn root_register(hotkey: String) -> Self {
        Self {
            inner: IntentCall::root_register(hotkey),
        }
    }

    #[napi(factory, js_name = "moveStake")]
    pub fn move_stake(
        origin_hotkey: String,
        origin_netuid: u16,
        destination_hotkey: String,
        destination_netuid: u16,
        amount_alpha_rao: BigInt,
    ) -> napi::Result<Self> {
        Ok(Self {
            inner: IntentCall::move_stake(
                origin_hotkey,
                origin_netuid,
                destination_hotkey,
                destination_netuid,
                bigint_u128("amountAlphaRao", &amount_alpha_rao)?,
            ),
        })
    }

    #[napi(factory, js_name = "swapStake")]
    pub fn swap_stake(
        hotkey: String,
        origin_netuid: u16,
        destination_netuid: u16,
        amount_alpha_rao: BigInt,
    ) -> napi::Result<Self> {
        Ok(Self {
            inner: IntentCall::swap_stake(
                hotkey,
                origin_netuid,
                destination_netuid,
                bigint_u128("amountAlphaRao", &amount_alpha_rao)?,
            ),
        })
    }

    #[napi(factory, js_name = "transferStake")]
    pub fn transfer_stake(
        destination_coldkey: String,
        hotkey: String,
        origin_netuid: u16,
        destination_netuid: u16,
        amount_alpha_rao: BigInt,
    ) -> napi::Result<Self> {
        Ok(Self {
            inner: IntentCall::transfer_stake(
                destination_coldkey,
                hotkey,
                origin_netuid,
                destination_netuid,
                bigint_u128("amountAlphaRao", &amount_alpha_rao)?,
            ),
        })
    }

    #[napi(factory, js_name = "unstakeAll")]
    pub fn unstake_all(hotkey: String) -> Self {
        Self {
            inner: IntentCall::unstake_all(hotkey),
        }
    }

    #[napi(factory, js_name = "unstakeAllAlpha")]
    pub fn unstake_all_alpha(hotkey: String) -> Self {
        Self {
            inner: IntentCall::unstake_all_alpha(hotkey),
        }
    }

    #[napi(factory, js_name = "setHyperparameter")]
    pub fn set_hyperparameter(netuid: u16, name: String, value: JsonValue) -> napi::Result<Self> {
        IntentCall::set_hyperparameter(netuid, &name, from_wire(value)?)
            .napi()
            .map(|inner| Self { inner })
    }

    #[napi(factory, js_name = "setRootClaimType")]
    pub fn set_root_claim_type(
        claim_type: String,
        subnets: Option<Vec<u16>>,
    ) -> napi::Result<Self> {
        IntentCall::set_root_claim_type(&claim_type, subnets)
            .napi()
            .map(|inner| Self { inner })
    }

    #[napi(getter)]
    pub fn op(&self) -> String {
        self.inner.op().to_owned()
    }

    #[napi(getter)]
    pub fn summary(&self) -> String {
        self.inner.summary.clone()
    }

    #[napi(getter, js_name = "signerRole")]
    pub fn signer_role(&self) -> String {
        signer_role_name(self.inner.signer_role()).to_owned()
    }

    #[napi(getter)]
    pub fn pallet(&self) -> String {
        self.inner.pallet().to_owned()
    }

    #[napi(getter, js_name = "callFunction")]
    pub fn call_function(&self) -> String {
        self.inner.function().to_owned()
    }

    #[napi(getter)]
    pub fn params(&self) -> NapiResult<JsonValue> {
        to_wire(self.inner.params())
    }

    #[napi(js_name = "withSummary")]
    pub fn with_summary(&self, summary: String) -> Self {
        Self {
            inner: self.inner.clone().summary(summary),
        }
    }

    #[napi(js_name = "forceRaw")]
    pub fn force_raw(&self) -> Self {
        Self {
            inner: self.inner.clone().raw(),
        }
    }

    #[napi(js_name = "asCallTuple")]
    pub fn as_call_tuple(&self) -> NapiResult<Vec<JsonValue>> {
        Ok(vec![
            JsonValue::String(self.inner.pallet().to_owned()),
            JsonValue::String(self.inner.function().to_owned()),
            to_wire(self.inner.params())?,
        ])
    }
}

#[napi]
impl NativeExternalSigningPlan {
    #[napi(getter, js_name = "callData")]
    pub fn call_data(&self) -> Buffer {
        self.inner.call_data.clone().into()
    }

    #[napi(getter, js_name = "signerAddress")]
    pub fn signer_address(&self) -> String {
        self.inner.signer_address.clone()
    }

    #[napi(getter, js_name = "publicKey")]
    pub fn public_key(&self) -> Buffer {
        self.inner.public_key.to_vec().into()
    }

    #[napi(getter, js_name = "cryptoType")]
    pub fn crypto_type(&self) -> u32 {
        u32::from(self.inner.crypto_type)
    }

    #[napi(getter)]
    pub fn nonce(&self) -> BigInt {
        BigInt::from(self.inner.params.nonce)
    }

    #[napi(getter)]
    pub fn payload(&self) -> Buffer {
        self.inner.payload.clone().into()
    }

    #[napi(getter, js_name = "includedInExtrinsic")]
    pub fn included_in_extrinsic(&self) -> Buffer {
        self.inner.included_in_extrinsic.clone().into()
    }

    #[napi(getter, js_name = "includedInSignedData")]
    pub fn included_in_signed_data(&self) -> Buffer {
        self.inner.included_in_signed_data.clone().into()
    }

    #[napi(getter, js_name = "metadataHash")]
    pub fn metadata_hash(&self) -> Option<Buffer> {
        self.inner
            .params
            .metadata_hash
            .map(|hash| hash.to_vec().into())
    }

    #[napi(getter, js_name = "metadataProof")]
    pub fn metadata_proof(&self) -> Option<Buffer> {
        self.inner.metadata_proof.clone().map(Into::into)
    }

    #[napi(getter, js_name = "txParams")]
    pub fn tx_params(&self) -> NapiResult<NativeTxParams> {
        tx_params_to_native(&self.inner.params)
    }

    #[napi(getter, js_name = "signerPayload")]
    pub fn signer_payload(&self) -> NativeSignerPayload {
        self.inner.signer_payload.clone().into()
    }

    #[napi(getter)]
    pub fn runtime(&self) -> NativeRuntime {
        NativeRuntime::from_arc(Arc::clone(&self.inner.runtime))
    }

    #[napi(getter, js_name = "chainInfo")]
    pub fn chain_info(&self) -> Option<NativeChainInfo> {
        self.inner.chain_info.clone().map(chain_info_to_native)
    }

    #[napi(getter, js_name = "feeRao")]
    pub fn fee_rao(&self) -> Option<String> {
        self.inner.fee_rao.map(|value| value.to_string())
    }

    #[napi(getter, js_name = "runtimeSpecVersion")]
    pub fn runtime_spec_version(&self) -> u32 {
        self.inner.runtime_spec_version
    }

    #[napi(getter, js_name = "runtimeTransactionVersion")]
    pub fn runtime_transaction_version(&self) -> u32 {
        self.inner.runtime_transaction_version
    }

    #[napi(getter)]
    pub fn warnings(&self) -> Vec<String> {
        self.inner.warnings.clone()
    }
}

#[napi]
pub struct NativeClient {
    pub(crate) inner: Arc<Client>,
}

#[napi]
impl NativeClient {
    #[napi]
    pub fn connect(endpoint: String) -> AsyncTask<ClientConnectTask> {
        AsyncTask::new(ClientConnectTask {
            endpoints: vec![endpoint],
        })
    }

    #[napi(js_name = "connectEndpoints")]
    pub fn connect_endpoints(endpoints: Vec<String>) -> AsyncTask<ClientConnectTask> {
        AsyncTask::new(ClientConnectTask { endpoints })
    }

    #[napi(getter)]
    pub fn endpoint(&self) -> String {
        self.inner.endpoint().to_owned()
    }

    #[napi(getter, js_name = "ss58Format")]
    pub fn ss58_format(&self) -> u16 {
        self.inner.ss58_format()
    }

    #[napi(getter, js_name = "genesisHash")]
    pub fn genesis_hash(&self) -> Buffer {
        self.inner.genesis_hash().to_vec().into()
    }

    #[napi(js_name = "blockHash")]
    pub fn block_hash(&self, block: Option<BigInt>) -> NapiResult<AsyncTask<ClientStringTask>> {
        let block = block
            .as_ref()
            .map(|value| bigint_u64("block", value))
            .transpose()?;
        Ok(AsyncTask::new(ClientStringTask::new(
            Arc::clone(&self.inner),
            move |client| client.block_hash(block),
        )))
    }

    #[napi(js_name = "finalizedHead")]
    pub fn finalized_head(&self) -> AsyncTask<ClientStringTask> {
        AsyncTask::new(ClientStringTask::new(Arc::clone(&self.inner), |client| {
            client.finalized_head()
        }))
    }

    #[napi(js_name = "blockNumber")]
    pub fn block_number(&self, block_hash: Option<String>) -> AsyncTask<ClientU64Task> {
        AsyncTask::new(ClientU64Task::new(
            Arc::clone(&self.inner),
            move |client| match block_hash {
                Some(hash) => client.header(Some(&hash)).map(|header| header.number),
                None => client.block_number(),
            },
        ))
    }

    #[napi(js_name = "header")]
    pub fn header(&self, block_hash: Option<String>) -> AsyncTask<ClientHeaderTask> {
        AsyncTask::new(ClientHeaderTask::new(
            Arc::clone(&self.inner),
            move |client| client.header(block_hash.as_deref()),
        ))
    }

    #[napi(js_name = "readCatalog")]
    pub fn read_catalog(&self) -> Vec<String> {
        self.inner
            .read_catalog()
            .iter()
            .map(|item| (*item).to_owned())
            .collect()
    }

    #[napi(js_name = "refreshRuntime")]
    pub fn refresh_runtime(&self) -> AsyncTask<ClientBoolTask> {
        AsyncTask::new(ClientBoolTask::new(Arc::clone(&self.inner), |client| {
            client.refresh_runtime()
        }))
    }

    #[napi(js_name = "runtime")]
    pub fn runtime(&self) -> NapiResult<NativeRuntime> {
        self.inner.runtime().napi().map(NativeRuntime::from_arc)
    }

    #[napi(js_name = "rpcValue")]
    pub fn rpc_value(
        &self,
        method: String,
        params: JsonValue,
    ) -> NapiResult<AsyncTask<ClientJsonTask>> {
        Ok(AsyncTask::new(ClientJsonTask::new(
            Arc::clone(&self.inner),
            move |client| client.rpc_value(&method, params),
        )))
    }

    #[napi(js_name = "chainInfo")]
    pub fn chain_info(&self) -> NapiResult<AsyncTask<ClientChainInfoTask>> {
        Ok(AsyncTask::new(ClientChainInfoTask::new(
            Arc::clone(&self.inner),
            |client| client.chain_info(),
        )))
    }

    #[napi(js_name = "composeCall")]
    pub fn compose_call(
        &self,
        pallet: String,
        call_function: String,
        params: JsonValue,
    ) -> NapiResult<AsyncTask<ClientBytesTask>> {
        let params = from_wire(params)?;
        Ok(AsyncTask::new(ClientBytesTask::new(
            Arc::clone(&self.inner),
            move |client| client.compose_call(&pallet, &call_function, &params),
        )))
    }

    #[napi(js_name = "decodeScale")]
    pub fn decode_scale(&self, type_name: String, data: Buffer) -> NapiResult<JsonValue> {
        self.inner
            .decode_scale(&type_name, &data)
            .napi()
            .and_then(|value| to_wire(&value))
    }

    #[napi(js_name = "constant")]
    pub fn constant(&self, pallet: String, name: String) -> NapiResult<JsonValue> {
        self.inner
            .constant(&pallet, &name)
            .napi()
            .and_then(|value| to_wire(&value))
    }

    #[napi(js_name = "query")]
    pub fn query(
        &self,
        pallet: String,
        storage: String,
        params: JsonValue,
        block_hash: Option<String>,
    ) -> NapiResult<AsyncTask<ClientWireValueTask>> {
        let params = wire_value_list("params", params)?;
        Ok(AsyncTask::new(ClientWireValueTask::new(
            Arc::clone(&self.inner),
            move |client| client.query(&pallet, &storage, &params, block_hash.as_deref()),
        )))
    }

    #[napi(js_name = "queryBatch")]
    pub fn query_batch(
        &self,
        pallet: String,
        storage: String,
        param_sets: JsonValue,
        block_hash: Option<String>,
    ) -> NapiResult<AsyncTask<ClientWireValuesTask>> {
        let param_sets = wire_value_list_list("paramSets", param_sets)?;
        Ok(AsyncTask::new(ClientWireValuesTask::new(
            Arc::clone(&self.inner),
            move |client| client.query_batch(&pallet, &storage, &param_sets, block_hash.as_deref()),
        )))
    }

    #[napi(js_name = "queryMap")]
    pub fn query_map(
        &self,
        pallet: String,
        storage: String,
        fixed_params: JsonValue,
        block_hash: Option<String>,
    ) -> NapiResult<AsyncTask<ClientMapTask>> {
        let fixed_params = wire_value_list("fixedParams", fixed_params)?;
        Ok(AsyncTask::new(ClientMapTask::new(
            Arc::clone(&self.inner),
            move |client| client.query_map(&pallet, &storage, &fixed_params, block_hash.as_deref()),
        )))
    }

    #[napi(js_name = "runtimeCall")]
    pub fn runtime_call(
        &self,
        api: String,
        method: String,
        params: JsonValue,
        block_hash: Option<String>,
    ) -> NapiResult<AsyncTask<ClientWireValueTask>> {
        let params = wire_value_list("params", params)?;
        Ok(AsyncTask::new(ClientWireValueTask::new(
            Arc::clone(&self.inner),
            move |client| client.runtime_call(&api, &method, &params, block_hash.as_deref()),
        )))
    }

    #[napi(js_name = "accountNextIndex")]
    pub fn account_next_index(&self, address: String) -> AsyncTask<ClientU64Task> {
        AsyncTask::new(ClientU64Task::new(Arc::clone(&self.inner), move |client| {
            client.account_next_index(&address)
        }))
    }

    #[napi(js_name = "signExtrinsic")]
    pub fn sign_extrinsic(
        &self,
        call_data: Buffer,
        signer: &NativeKeypair,
        nonce: BigInt,
        period: Option<BigInt>,
    ) -> NapiResult<AsyncTask<ClientSignedExtrinsicTask>> {
        let nonce = bigint_u64("nonce", &nonce)?;
        let period = period
            .as_ref()
            .map(|value| bigint_u64("period", value))
            .transpose()?;
        let call_data = call_data.to_vec();
        let signer = signer.inner.clone();
        Ok(AsyncTask::new(ClientSignedExtrinsicTask::new(
            Arc::clone(&self.inner),
            move |client| client.sign_extrinsic(&call_data, &signer, nonce, period),
        )))
    }

    #[napi(js_name = "estimateFee")]
    pub fn estimate_fee(
        &self,
        call_data: Buffer,
        signer: &NativeKeypair,
    ) -> AsyncTask<ClientStringTask> {
        let call_data = call_data.to_vec();
        let signer = signer.inner.clone();
        AsyncTask::new(ClientStringTask::new(
            Arc::clone(&self.inner),
            move |client| {
                client
                    .estimate_fee(&call_data, &signer)
                    .map(|value| value.to_string())
            },
        ))
    }

    #[napi(js_name = "submit")]
    pub fn submit(
        &self,
        env: Env,
        call_data: Buffer,
        signer: &NativeKeypair,
        nonce: Option<BigInt>,
        period: Option<BigInt>,
        options: Option<NativeSubmitOptions>,
        cancellation: Option<&NativeCancellationToken>,
    ) -> NapiResult<PromiseRaw<'static, NativeTxOutcome>> {
        let nonce = nonce
            .as_ref()
            .map(|value| bigint_u64("nonce", value))
            .transpose()?;
        let period = period
            .as_ref()
            .map(|value| bigint_u64("period", value))
            .transpose()?;
        let call_data = call_data.to_vec();
        let signer = signer.inner.clone();
        let (wait, timeout) = submit_options(options)?;
        spawn_tx_outcome(
            &env,
            Arc::clone(&self.inner),
            cancellation,
            move |client, cancelled| {
                client.submit_with_cancel(
                    &call_data, &signer, nonce, period, wait, timeout, cancelled,
                )
            },
        )
    }

    #[napi(js_name = "submitEncoded")]
    pub fn submit_encoded(
        &self,
        env: Env,
        extrinsic: Buffer,
        expected_hash: String,
        options: Option<NativeSubmitOptions>,
        cancellation: Option<&NativeCancellationToken>,
    ) -> NapiResult<PromiseRaw<'static, NativeTxOutcome>> {
        let extrinsic = extrinsic.to_vec();
        let (wait, timeout) = submit_options(options)?;
        spawn_tx_outcome(
            &env,
            Arc::clone(&self.inner),
            cancellation,
            move |client, cancelled| {
                client.submit_encoded_with_wait_cancelled(
                    &extrinsic,
                    expected_hash,
                    wait,
                    timeout,
                    cancelled,
                )
            },
        )
    }

    #[napi(js_name = "externalSigningPlan")]
    pub fn external_signing_plan(
        &self,
        call_data: Buffer,
        signer: NativeExternalSigner,
        options: Option<NativeExternalSigningOptions>,
    ) -> NapiResult<AsyncTask<ClientExternalSigningPlanTask>> {
        let call_data = call_data.to_vec();
        let signer = external_signer(signer)?;
        let options = external_signing_options(options)?;
        Ok(AsyncTask::new(ClientExternalSigningPlanTask::new(
            Arc::clone(&self.inner),
            move |client| client.external_signing_plan(&call_data, signer, options),
        )))
    }

    #[napi(js_name = "externalSigningPlanForIntent")]
    pub fn external_signing_plan_for_intent(
        &self,
        intent: &NativeIntentCall,
        signer: NativeExternalSigner,
        policy: &NativePolicy,
        options: Option<NativeExternalSigningOptions>,
    ) -> NapiResult<AsyncTask<ClientExternalSigningPlanTask>> {
        let intent = intent.inner.clone();
        let signer = external_signer(signer)?;
        let policy = policy.inner.clone();
        let options = external_signing_options(options)?;
        Ok(AsyncTask::new(ClientExternalSigningPlanTask::new(
            Arc::clone(&self.inner),
            move |client| {
                Executor::new(client).external_signing_plan(&intent, signer, options, Some(&policy))
            },
        )))
    }

    #[napi(js_name = "estimateFeeExternal")]
    pub fn estimate_fee_external(
        &self,
        plan: &NativeExternalSigningPlan,
    ) -> AsyncTask<ClientStringTask> {
        let plan = Arc::clone(&plan.inner);
        AsyncTask::new(ClientStringTask::new(
            Arc::clone(&self.inner),
            move |client| {
                client
                    .estimate_fee_external_plan(&plan)
                    .map(|value| value.to_string())
            },
        ))
    }

    #[napi(js_name = "assembleExternal")]
    pub fn assemble_external(
        &self,
        plan: &NativeExternalSigningPlan,
        signature: Buffer,
        crypto_type: Option<u32>,
    ) -> NapiResult<AsyncTask<ClientSignedExtrinsicTask>> {
        let plan = Arc::clone(&plan.inner);
        let signature = signature.to_vec();
        let crypto_type = crypto_type
            .map(|value| u8::try_from(value).map_err(|_| invalid_arg("cryptoType must fit u8")))
            .transpose()?;
        Ok(AsyncTask::new(ClientSignedExtrinsicTask::new(
            Arc::clone(&self.inner),
            move |client| client.assemble_external_extrinsic(&plan, &signature, crypto_type),
        )))
    }

    #[napi(js_name = "submitExternal")]
    pub fn submit_external(
        &self,
        env: Env,
        plan: &NativeExternalSigningPlan,
        signature: Buffer,
        options: Option<NativeSubmitOptions>,
        crypto_type: Option<u32>,
        cancellation: Option<&NativeCancellationToken>,
    ) -> NapiResult<PromiseRaw<'static, NativeTxOutcome>> {
        let plan = Arc::clone(&plan.inner);
        let signature = signature.to_vec();
        let crypto_type = crypto_type
            .map(|value| u8::try_from(value).map_err(|_| invalid_arg("cryptoType must fit u8")))
            .transpose()?;
        let (wait, timeout) = submit_options(options)?;
        spawn_tx_outcome(
            &env,
            Arc::clone(&self.inner),
            cancellation,
            move |client, cancelled| {
                client.submit_external_with_cancel(
                    &plan,
                    &signature,
                    crypto_type,
                    wait,
                    timeout,
                    cancelled,
                )
            },
        )
    }

    #[napi(js_name = "balanceRao")]
    pub fn balance_rao(&self, address: String) -> AsyncTask<ClientStringTask> {
        AsyncTask::new(ClientStringTask::new(
            Arc::clone(&self.inner),
            move |client| client.balance_rao(&address).map(|value| value.to_string()),
        ))
    }

    #[napi(js_name = "existentialDepositRao")]
    pub fn existential_deposit_rao(&self) -> AsyncTask<ClientStringTask> {
        AsyncTask::new(ClientStringTask::new(Arc::clone(&self.inner), |client| {
            client
                .existential_deposit_rao()
                .map(|value| value.to_string())
        }))
    }

    #[napi(js_name = "subnets")]
    pub fn subnets(&self, block_hash: Option<String>) -> AsyncTask<ClientSubnetsTask> {
        AsyncTask::new(ClientSubnetsTask::new(
            Arc::clone(&self.inner),
            move |client| client.subnets(block_hash.as_deref()),
        ))
    }

    #[napi(js_name = "metagraph")]
    pub fn metagraph(
        &self,
        netuid: u16,
        block_hash: Option<String>,
    ) -> AsyncTask<ClientWireValueTask> {
        AsyncTask::new(ClientWireValueTask::new(
            Arc::clone(&self.inner),
            move |client| client.metagraph(netuid, block_hash.as_deref()),
        ))
    }

    #[napi(js_name = "neurons")]
    pub fn neurons(
        &self,
        netuid: u16,
        block_hash: Option<String>,
    ) -> AsyncTask<ClientWireValuesTask> {
        AsyncTask::new(ClientWireValuesTask::new(
            Arc::clone(&self.inner),
            move |client| client.neurons(netuid, block_hash.as_deref()),
        ))
    }

    #[napi(js_name = "subnetHyperparameters")]
    pub fn subnet_hyperparameters(
        &self,
        netuid: u16,
        block_hash: Option<String>,
    ) -> AsyncTask<ClientSubnetHyperparametersTask> {
        AsyncTask::new(ClientSubnetHyperparametersTask::new(
            Arc::clone(&self.inner),
            move |client| client.subnet_hyperparameters(netuid, block_hash.as_deref()),
        ))
    }

    #[napi(js_name = "stakeRao")]
    pub fn stake_rao(
        &self,
        coldkey: String,
        hotkey: String,
        netuid: u16,
        block_hash: Option<String>,
    ) -> AsyncTask<ClientStringTask> {
        AsyncTask::new(ClientStringTask::new(
            Arc::clone(&self.inner),
            move |client| {
                client
                    .stake_rao(&coldkey, &hotkey, netuid, block_hash.as_deref())
                    .map(|value| value.to_string())
            },
        ))
    }

    #[napi(js_name = "quoteStake")]
    pub fn quote_stake(
        &self,
        netuid: u16,
        amount_rao: BigInt,
        block_hash: Option<String>,
    ) -> NapiResult<AsyncTask<ClientSwapQuoteTask>> {
        let amount_rao = bigint_u128("amountRao", &amount_rao)?;
        Ok(AsyncTask::new(ClientSwapQuoteTask::new(
            Arc::clone(&self.inner),
            move |client| client.quote_stake(netuid, amount_rao, block_hash.as_deref()),
        )))
    }

    #[napi(js_name = "composeIntent")]
    pub fn compose_intent(&self, intent: &NativeIntentCall) -> AsyncTask<ClientBytesTask> {
        let intent = intent.inner.clone();
        AsyncTask::new(ClientBytesTask::new(
            Arc::clone(&self.inner),
            move |client| intent.encode(client),
        ))
    }
}

#[napi]
pub struct NativeWallet {
    pub(crate) inner: Wallet,
}

#[napi]
impl NativeWallet {
    #[napi(factory, js_name = "fromKeypairs")]
    pub fn from_keypairs(coldkey: &NativeKeypair, hotkey: &NativeKeypair) -> Self {
        Self {
            inner: Wallet {
                coldkey: coldkey.inner.clone(),
                hotkey: hotkey.inner.clone(),
            },
        }
    }

    #[napi(factory, js_name = "fromUris")]
    pub fn from_uris(coldkey_uri: String, hotkey_uri: String) -> napi::Result<Self> {
        Wallet::from_uris(&coldkey_uri, &hotkey_uri)
            .napi()
            .map(|inner| Self { inner })
    }
}

#[napi]
pub struct NativeExecutor {
    client: Arc<Client>,
    policy: Option<Policy>,
}

pub struct ExecutorPlanTask {
    client: Arc<Client>,
    policy: Option<Policy>,
    intent: IntentCall,
    wallet: Wallet,
    check_policy: Option<Policy>,
}

impl Task for ExecutorPlanTask {
    type Output = Plan;
    type JsValue = NativePlan;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        let executor = executor_from_parts(&self.client, &self.policy);
        match &self.check_policy {
            Some(policy) => executor
                .plan_with_policy(&self.intent, &self.wallet, policy)
                .napi(),
            None => executor.plan(&self.intent, &self.wallet).napi(),
        }
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        plan_to_native(output)
    }
}

pub struct ExecutorExecuteTask {
    client: Arc<Client>,
    policy: Option<Policy>,
    intent: IntentCall,
    wallet: Wallet,
    wait_for_finalization: bool,
}

impl Task for ExecutorExecuteTask {
    type Output = TxOutcome;
    type JsValue = NativeTxOutcome;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        let executor = executor_from_parts(&self.client, &self.policy);
        executor
            .execute_with(
                &self.intent,
                &self.wallet,
                None,
                None,
                None,
                self.wait_for_finalization,
            )
            .napi()
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        outcome_to_native(output)
    }
}

pub struct ExecutorSubmitShieldedTask {
    client: Arc<Client>,
    policy: Option<Policy>,
    intent: IntentCall,
    wallet: Wallet,
}

impl Task for ExecutorSubmitShieldedTask {
    type Output = TxOutcome;
    type JsValue = NativeTxOutcome;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        let executor = executor_from_parts(&self.client, &self.policy);
        executor
            .submit_shielded(&self.intent, &self.wallet, None)
            .napi()
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        outcome_to_native(output)
    }
}

#[napi]
impl NativeExecutor {
    #[napi(factory, js_name = "fromClient")]
    pub fn from_client(client: &NativeClient) -> Self {
        Self {
            client: Arc::clone(&client.inner),
            policy: None,
        }
    }

    #[napi(factory, js_name = "withPolicy")]
    pub fn with_policy(client: &NativeClient, policy: &NativePolicy) -> Self {
        Self {
            client: Arc::clone(&client.inner),
            policy: Some(policy.inner.clone()),
        }
    }

    #[napi(js_name = "plan")]
    pub fn plan(
        &self,
        intent: &NativeIntentCall,
        wallet: &NativeWallet,
    ) -> AsyncTask<ExecutorPlanTask> {
        AsyncTask::new(ExecutorPlanTask {
            client: Arc::clone(&self.client),
            policy: self.policy.clone(),
            intent: intent.inner.clone(),
            wallet: wallet.inner.clone(),
            check_policy: None,
        })
    }

    #[napi(js_name = "planWithPolicy")]
    pub fn plan_with_policy(
        &self,
        intent: &NativeIntentCall,
        wallet: &NativeWallet,
        policy: &NativePolicy,
    ) -> AsyncTask<ExecutorPlanTask> {
        AsyncTask::new(ExecutorPlanTask {
            client: Arc::clone(&self.client),
            policy: self.policy.clone(),
            intent: intent.inner.clone(),
            wallet: wallet.inner.clone(),
            check_policy: Some(policy.inner.clone()),
        })
    }

    #[napi(js_name = "execute")]
    pub fn execute(
        &self,
        intent: &NativeIntentCall,
        wallet: &NativeWallet,
        wait_for_finalization: Option<bool>,
    ) -> AsyncTask<ExecutorExecuteTask> {
        AsyncTask::new(ExecutorExecuteTask {
            client: Arc::clone(&self.client),
            policy: self.policy.clone(),
            intent: intent.inner.clone(),
            wallet: wallet.inner.clone(),
            wait_for_finalization: wait_for_finalization.unwrap_or(true),
        })
    }

    #[napi(js_name = "submitShielded")]
    pub fn submit_shielded(
        &self,
        intent: &NativeIntentCall,
        wallet: &NativeWallet,
    ) -> AsyncTask<ExecutorSubmitShieldedTask> {
        AsyncTask::new(ExecutorSubmitShieldedTask {
            client: Arc::clone(&self.client),
            policy: self.policy.clone(),
            intent: intent.inner.clone(),
            wallet: wallet.inner.clone(),
        })
    }
}

fn executor_from_parts<'a>(client: &'a Arc<Client>, policy: &Option<Policy>) -> Executor<'a> {
    match policy {
        Some(policy) => Executor::with_policy(client, policy.clone()),
        None => Executor::new(client),
    }
}

impl From<NativeSignerRole> for SignerRole {
    fn from(value: NativeSignerRole) -> Self {
        match value {
            NativeSignerRole::Coldkey => SignerRole::Coldkey,
            NativeSignerRole::Hotkey => SignerRole::Hotkey,
        }
    }
}

fn signer_role_name(role: SignerRole) -> &'static str {
    match role {
        SignerRole::Coldkey => "coldkey",
        SignerRole::Hotkey => "hotkey",
    }
}

fn plan_to_native(plan: Plan) -> NapiResult<NativePlan> {
    let ok = plan.ok();
    Ok(NativePlan {
        op: plan.op,
        summary: plan.summary,
        signer_role: signer_role_name(plan.signer).to_owned(),
        signer_address: plan.signer_address,
        fee_rao: plan.fee_rao.map(|value| value.to_string()),
        warnings: plan.warnings,
        ok,
        violations: plan.violations,
        call_data: plan.call_data.into(),
    })
}

fn outcome_to_native(outcome: TxOutcome) -> NapiResult<NativeTxOutcome> {
    let data = outcome
        .data
        .iter()
        .map(|(key, value)| Ok((key.clone(), to_wire(value)?)))
        .collect::<NapiResult<serde_json::Map<String, JsonValue>>>()?;
    let events = outcome
        .events
        .iter()
        .map(to_wire)
        .collect::<NapiResult<Vec<_>>>()?;
    let error = outcome.error.map(dispatch_error_to_native);
    Ok(NativeTxOutcome {
        success: outcome.success,
        extrinsic_hash: outcome.extrinsic_hash,
        block_hash: outcome.block_hash,
        block_number: outcome.block_number.map(BigInt::from),
        extrinsic_index: outcome.extrinsic_index,
        fee_rao: outcome.fee_rao.map(|value| value.to_string()),
        events,
        error,
        message: outcome.message,
        data: JsonValue::Object(data),
    })
}

fn dispatch_error_to_native(error: DispatchError) -> NativeDispatchError {
    NativeDispatchError {
        pallet: error.pallet,
        name: error.name,
        docs: error.docs,
        semantic_code: error.semantic_code,
    }
}

fn header_to_native(header: BlockHeader) -> NativeBlockHeader {
    NativeBlockHeader {
        hash: header.hash,
        parent_hash: header.parent_hash,
        number: BigInt::from(header.number),
    }
}

fn subnet_to_native(subnet: SubnetInfo) -> NativeSubnetInfo {
    NativeSubnetInfo {
        netuid: subnet.netuid,
        tempo: subnet.tempo,
        burn_rao: subnet.burn_rao.to_string(),
        neuron_count: subnet.neuron_count,
    }
}

fn subnet_hyperparameter_to_native(
    entry: SubnetHyperparameter,
) -> NapiResult<NativeSubnetHyperparameter> {
    let (value_type, value) = match entry.value {
        SubnetHyperparameterValue::Bool(value) => {
            ("Bool", bittensor_core::codec::Value::Bool(value))
        }
        SubnetHyperparameterValue::U16(value) => {
            ("U16", bittensor_core::codec::Value::Uint(u128::from(value)))
        }
        SubnetHyperparameterValue::U32(value) => {
            ("U32", bittensor_core::codec::Value::Uint(u128::from(value)))
        }
        SubnetHyperparameterValue::U64(value) => {
            ("U64", bittensor_core::codec::Value::Uint(u128::from(value)))
        }
        SubnetHyperparameterValue::U128(value) => {
            ("U128", bittensor_core::codec::Value::Uint(value))
        }
        SubnetHyperparameterValue::TaoBalance(value) => {
            ("TaoBalance", bittensor_core::codec::Value::Uint(value))
        }
        SubnetHyperparameterValue::I32F32Bits(value) => (
            "I32F32",
            bittensor_core::codec::Value::Int(i128::from(value)),
        ),
        SubnetHyperparameterValue::U64F64Bits(value) => {
            ("U64F64", bittensor_core::codec::Value::Uint(value))
        }
    };
    Ok(NativeSubnetHyperparameter {
        name: entry.name,
        value_type: value_type.to_owned(),
        value: to_wire(&value)?,
    })
}

fn quote_to_native(quote: SwapQuote) -> NativeSwapQuote {
    NativeSwapQuote {
        tao_amount: quote.tao_amount.to_string(),
        alpha_amount: quote.alpha_amount.to_string(),
        tao_fee: quote.tao_fee.to_string(),
        alpha_fee: quote.alpha_fee.to_string(),
        tao_slippage: quote.tao_slippage.to_string(),
        alpha_slippage: quote.alpha_slippage.to_string(),
    }
}

fn external_signer(signer: NativeExternalSigner) -> NapiResult<ExternalSigner> {
    let public_key = buffer_32("publicKey", &signer.public_key)?;
    let crypto_type =
        u8::try_from(signer.crypto_type).map_err(|_| invalid_arg("cryptoType must fit u8"))?;
    Ok(ExternalSigner {
        ss58_address: signer.signer_address,
        public_key,
        crypto_type,
        requires_metadata_proof: signer.requires_metadata_proof,
    })
}

fn submit_options(options: Option<NativeSubmitOptions>) -> NapiResult<(TxWait, Duration)> {
    let Some(options) = options else {
        return Ok((TxWait::Submitted, DEFAULT_RECEIPT_TIMEOUT));
    };
    let wait = if options.wait_for_finalization.unwrap_or(false) {
        TxWait::Finalized
    } else if options.wait_for_inclusion.unwrap_or(false) {
        TxWait::Included
    } else {
        TxWait::Submitted
    };
    let timeout = options
        .timeout_ms
        .as_ref()
        .map(|value| bigint_u64("timeoutMs", value).map(Duration::from_millis))
        .transpose()?
        .unwrap_or(DEFAULT_RECEIPT_TIMEOUT);
    Ok((wait, timeout))
}

fn external_signing_options(
    options: Option<NativeExternalSigningOptions>,
) -> NapiResult<ExternalSigningOptions> {
    let Some(options) = options else {
        return Ok(ExternalSigningOptions::default());
    };
    let nonce = options
        .nonce
        .as_ref()
        .map(|value| bigint_u64("nonce", value))
        .transpose()?;
    let period = if options.immortal.unwrap_or(false) {
        None
    } else if let Some(period) = &options.period {
        Some(bigint_u64("period", period)?)
    } else {
        ExternalSigningOptions::default().period
    };
    let tip = options
        .tip
        .as_ref()
        .map(|value| bigint_u128("tip", value))
        .transpose()?
        .unwrap_or(0);
    let tip_asset_id = options
        .tip_asset_id
        .as_ref()
        .map(|value| bigint_u128("tipAssetId", value))
        .transpose()?;
    let mode = options
        .metadata_hash_mode
        .as_deref()
        .unwrap_or("auto")
        .to_ascii_lowercase();
    let metadata_hash = match mode.as_str() {
        "auto" => MetadataHashMode::Auto,
        "disabled" => MetadataHashMode::Disabled,
        "explicit" => MetadataHashMode::Explicit(buffer_32(
            "metadataHash",
            options.metadata_hash.as_ref().ok_or_else(|| {
                invalid_arg("metadataHash is required when metadataHashMode is explicit")
            })?,
        )?),
        other => {
            return Err(invalid_arg(format!(
                "unsupported metadataHashMode {other:?}; expected auto, disabled, or explicit"
            )))
        }
    };
    Ok(ExternalSigningOptions {
        nonce,
        period,
        tip,
        tip_asset_id,
        metadata_hash,
    })
}

fn tx_params_to_native(
    params: &bittensor_core::codec::extrinsic::TxParams,
) -> NapiResult<NativeTxParams> {
    Ok(NativeTxParams {
        era: to_wire(&params.era)?,
        nonce: BigInt::from(params.nonce),
        tip: BigInt::from(params.tip),
        tip_asset_id: params.tip_asset_id.map(BigInt::from),
        genesis_hash: params.genesis_hash.to_vec().into(),
        era_block_hash: params.era_block_hash.to_vec().into(),
        metadata_hash: params.metadata_hash.map(|hash| hash.to_vec().into()),
    })
}

fn chain_info_to_native(info: ChainInfo) -> NativeChainInfo {
    NativeChainInfo {
        spec_version: info.spec_version,
        spec_name: info.spec_name,
        base58_prefix: info.base58_prefix,
        decimals: info.decimals,
        token_symbol: info.token_symbol,
    }
}

fn buffer_32(name: &str, value: &Buffer) -> NapiResult<[u8; 32]> {
    <[u8; 32]>::try_from(value.as_ref())
        .map_err(|_| invalid_arg(format!("{name} must be exactly 32 bytes")))
}

fn wire_value_list(name: &str, value: JsonValue) -> NapiResult<Vec<bittensor_core::codec::Value>> {
    match from_wire(value)? {
        bittensor_core::codec::Value::List(values)
        | bittensor_core::codec::Value::Tuple(values) => Ok(values),
        _ => Err(invalid_arg(format!("{name} must be an array"))),
    }
}

fn wire_value_list_list(
    name: &str,
    value: JsonValue,
) -> NapiResult<Vec<Vec<bittensor_core::codec::Value>>> {
    wire_value_list(name, value)?
        .into_iter()
        .enumerate()
        .map(|(index, value)| match value {
            bittensor_core::codec::Value::List(values)
            | bittensor_core::codec::Value::Tuple(values) => Ok(values),
            _ => Err(invalid_arg(format!("{name}[{index}] must be an array"))),
        })
        .collect()
}

fn bigint_u128(name: &str, value: &BigInt) -> NapiResult<u128> {
    let (negative, value, lossless) = value.get_u128();
    if negative || !lossless {
        return Err(invalid_arg(format!("{name} must fit the Rust u128 range")));
    }
    Ok(value)
}

fn bigint_u64(name: &str, value: &BigInt) -> NapiResult<u64> {
    let value = bigint_u128(name, value)?;
    u64::try_from(value).map_err(|_| invalid_arg(format!("{name} must fit the Rust u64 range")))
}

#[allow(dead_code)]
fn spend_kind(spend: Spend) -> NativeSpendKind {
    match spend {
        Spend::None => NativeSpendKind::None,
        Spend::Bounded(_) => NativeSpendKind::Bounded,
        Spend::Unbounded => NativeSpendKind::Unbounded,
    }
}
