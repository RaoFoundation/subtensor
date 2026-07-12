use std::sync::Arc;

use bittensor_core::client::{BlockHeader, DispatchError, SubnetInfo, SwapQuote, TxOutcome};
use bittensor_core::transaction::{Executor, IntentCall, Plan, Policy, SignerRole, Spend, Wallet};
use bittensor_core::Client;
use napi::bindgen_prelude::{BigInt, Buffer};
use napi_derive::napi;
use serde_json::Value as JsonValue;

use crate::errors::{invalid_arg, CoreResultExt, NapiResult};
use crate::keys::NativeKeypair;
use crate::runtime::NativeMapPair;
use crate::values::{from_wire, to_wire};

#[napi(object)]
pub struct NativePolicyOptions {
    pub max_fee_rao: Option<BigInt>,
    pub max_spend_rao: Option<BigInt>,
    pub allowed_netuids: Option<Vec<u16>>,
    pub allow_raw_calls: Option<bool>,
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
            },
        })
    }

    #[napi(getter)]
    pub fn allow_raw_calls(&self) -> bool {
        self.inner.allow_raw_calls
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
        Ok(Self {
            inner: IntentCall::serve_axon(
                netuid,
                version,
                bigint_u128("ip", &ip)?,
                port,
                ip_type,
                protocol,
            ),
        })
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
pub struct NativeClient {
    pub(crate) inner: Arc<Client>,
}

#[napi]
impl NativeClient {
    #[napi(factory)]
    pub fn connect(endpoint: String) -> napi::Result<Self> {
        Client::connect(endpoint).napi().map(|inner| Self {
            inner: Arc::new(inner),
        })
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
    pub fn block_hash(&self, block: Option<BigInt>) -> NapiResult<String> {
        let block = block
            .as_ref()
            .map(|value| bigint_u64("block", value))
            .transpose()?;
        self.inner.block_hash(block).napi()
    }

    #[napi(js_name = "finalizedHead")]
    pub fn finalized_head(&self) -> NapiResult<String> {
        self.inner.finalized_head().napi()
    }

    #[napi(js_name = "blockNumber")]
    pub fn block_number(&self, block_hash: Option<String>) -> NapiResult<BigInt> {
        let number = match block_hash {
            Some(hash) => self.inner.header(Some(&hash)).napi()?.number,
            None => self.inner.block_number().napi()?,
        };
        Ok(BigInt::from(number))
    }

    #[napi(js_name = "header")]
    pub fn header(&self, block_hash: Option<String>) -> NapiResult<NativeBlockHeader> {
        self.inner
            .header(block_hash.as_deref())
            .napi()
            .map(header_to_native)
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
    pub fn refresh_runtime(&self) -> NapiResult<bool> {
        self.inner.refresh_runtime().napi()
    }

    #[napi(js_name = "composeCall")]
    pub fn compose_call(
        &self,
        pallet: String,
        call_function: String,
        params: JsonValue,
    ) -> NapiResult<Buffer> {
        self.inner
            .compose_call(&pallet, &call_function, &from_wire(params)?)
            .napi()
            .map(Buffer::from)
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
    ) -> NapiResult<JsonValue> {
        let params = wire_value_list("params", params)?;
        self.inner
            .query(&pallet, &storage, &params, block_hash.as_deref())
            .napi()
            .and_then(|value| to_wire(&value))
    }

    #[napi(js_name = "queryBatch")]
    pub fn query_batch(
        &self,
        pallet: String,
        storage: String,
        param_sets: JsonValue,
        block_hash: Option<String>,
    ) -> NapiResult<Vec<JsonValue>> {
        let param_sets = wire_value_list_list("paramSets", param_sets)?;
        self.inner
            .query_batch(&pallet, &storage, &param_sets, block_hash.as_deref())
            .napi()?
            .iter()
            .map(to_wire)
            .collect()
    }

    #[napi(js_name = "queryMap")]
    pub fn query_map(
        &self,
        pallet: String,
        storage: String,
        fixed_params: JsonValue,
        block_hash: Option<String>,
    ) -> NapiResult<Vec<NativeMapPair>> {
        let fixed_params = wire_value_list("fixedParams", fixed_params)?;
        self.inner
            .query_map(&pallet, &storage, &fixed_params, block_hash.as_deref())
            .napi()?
            .iter()
            .map(|(key, value)| {
                Ok(NativeMapPair {
                    key: to_wire(key)?,
                    value: to_wire(value)?,
                })
            })
            .collect()
    }

    #[napi(js_name = "runtimeCall")]
    pub fn runtime_call(
        &self,
        api: String,
        method: String,
        params: JsonValue,
        block_hash: Option<String>,
    ) -> NapiResult<JsonValue> {
        let params = wire_value_list("params", params)?;
        self.inner
            .runtime_call(&api, &method, &params, block_hash.as_deref())
            .napi()
            .and_then(|value| to_wire(&value))
    }

    #[napi(js_name = "accountNextIndex")]
    pub fn account_next_index(&self, address: String) -> NapiResult<BigInt> {
        self.inner.account_next_index(&address).napi().map(BigInt::from)
    }

    #[napi(js_name = "signExtrinsic")]
    pub fn sign_extrinsic(
        &self,
        call_data: Buffer,
        signer: &NativeKeypair,
        nonce: BigInt,
        period: Option<BigInt>,
    ) -> NapiResult<NativeSignedExtrinsic> {
        let nonce = bigint_u64("nonce", &nonce)?;
        let period = period
            .as_ref()
            .map(|value| bigint_u64("period", value))
            .transpose()?;
        self.inner
            .sign_extrinsic(&call_data, &signer.inner, nonce, period)
            .napi()
            .map(|(bytes, hash)| NativeSignedExtrinsic {
                bytes: bytes.into(),
                hash,
            })
    }

    #[napi(js_name = "estimateFee")]
    pub fn estimate_fee(&self, call_data: Buffer, signer: &NativeKeypair) -> NapiResult<String> {
        self.inner
            .estimate_fee(&call_data, &signer.inner)
            .napi()
            .map(|value| value.to_string())
    }

    #[napi(js_name = "submit")]
    pub fn submit(
        &self,
        call_data: Buffer,
        signer: &NativeKeypair,
        nonce: Option<BigInt>,
        period: Option<BigInt>,
        wait_for_finalization: Option<bool>,
    ) -> NapiResult<NativeTxOutcome> {
        let nonce = nonce
            .as_ref()
            .map(|value| bigint_u64("nonce", value))
            .transpose()?;
        let period = period
            .as_ref()
            .map(|value| bigint_u64("period", value))
            .transpose()?;
        self.inner
            .submit(
                &call_data,
                &signer.inner,
                nonce,
                period,
                wait_for_finalization.unwrap_or(false),
            )
            .napi()
            .and_then(outcome_to_native)
    }

    #[napi(js_name = "submitEncoded")]
    pub fn submit_encoded(
        &self,
        extrinsic: Buffer,
        expected_hash: String,
        wait_for_finalization: Option<bool>,
    ) -> NapiResult<NativeTxOutcome> {
        self.inner
            .submit_encoded(
                &extrinsic,
                expected_hash,
                wait_for_finalization.unwrap_or(false),
            )
            .napi()
            .and_then(outcome_to_native)
    }

    #[napi(js_name = "balanceRao")]
    pub fn balance_rao(&self, address: String) -> NapiResult<String> {
        self.inner
            .balance_rao(&address)
            .napi()
            .map(|value| value.to_string())
    }

    #[napi(js_name = "existentialDepositRao")]
    pub fn existential_deposit_rao(&self) -> NapiResult<String> {
        self.inner
            .existential_deposit_rao()
            .napi()
            .map(|value| value.to_string())
    }

    #[napi(js_name = "subnets")]
    pub fn subnets(&self, block_hash: Option<String>) -> NapiResult<Vec<NativeSubnetInfo>> {
        self.inner
            .subnets(block_hash.as_deref())
            .napi()
            .map(|items| items.into_iter().map(subnet_to_native).collect())
    }

    #[napi(js_name = "metagraph")]
    pub fn metagraph(&self, netuid: u16, block_hash: Option<String>) -> NapiResult<JsonValue> {
        self.inner
            .metagraph(netuid, block_hash.as_deref())
            .napi()
            .and_then(|value| to_wire(&value))
    }

    #[napi(js_name = "neurons")]
    pub fn neurons(&self, netuid: u16, block_hash: Option<String>) -> NapiResult<Vec<JsonValue>> {
        self.inner
            .neurons(netuid, block_hash.as_deref())
            .napi()?
            .iter()
            .map(to_wire)
            .collect()
    }

    #[napi(js_name = "subnetHyperparameters")]
    pub fn subnet_hyperparameters(
        &self,
        netuid: u16,
        block_hash: Option<String>,
    ) -> NapiResult<JsonValue> {
        self.inner
            .subnet_hyperparameters(netuid, block_hash.as_deref())
            .napi()
            .and_then(|value| to_wire(&value))
    }

    #[napi(js_name = "stakeRao")]
    pub fn stake_rao(
        &self,
        coldkey: String,
        hotkey: String,
        netuid: u16,
        block_hash: Option<String>,
    ) -> NapiResult<String> {
        self.inner
            .stake_rao(&coldkey, &hotkey, netuid, block_hash.as_deref())
            .napi()
            .map(|value| value.to_string())
    }

    #[napi(js_name = "quoteStake")]
    pub fn quote_stake(
        &self,
        netuid: u16,
        amount_rao: BigInt,
        block_hash: Option<String>,
    ) -> NapiResult<NativeSwapQuote> {
        self.inner
            .quote_stake(
                netuid,
                bigint_u128("amountRao", &amount_rao)?,
                block_hash.as_deref(),
            )
            .napi()
            .map(quote_to_native)
    }

    #[napi(js_name = "composeIntent")]
    pub fn compose_intent(&self, intent: &NativeIntentCall) -> NapiResult<Buffer> {
        intent.inner.encode(&self.inner).napi().map(Buffer::from)
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
    pub fn plan(&self, intent: &NativeIntentCall, wallet: &NativeWallet) -> NapiResult<NativePlan> {
        let executor = self.executor();
        executor
            .plan(&intent.inner, &wallet.inner)
            .napi()
            .and_then(plan_to_native)
    }

    #[napi(js_name = "planWithPolicy")]
    pub fn plan_with_policy(
        &self,
        intent: &NativeIntentCall,
        wallet: &NativeWallet,
        policy: &NativePolicy,
    ) -> NapiResult<NativePlan> {
        let executor = self.executor();
        executor
            .plan_with_policy(&intent.inner, &wallet.inner, &policy.inner)
            .napi()
            .and_then(plan_to_native)
    }

    #[napi(js_name = "execute")]
    pub fn execute(
        &self,
        intent: &NativeIntentCall,
        wallet: &NativeWallet,
        wait_for_finalization: Option<bool>,
    ) -> NapiResult<NativeTxOutcome> {
        let executor = self.executor();
        executor
            .execute_with(
                &intent.inner,
                &wallet.inner,
                None,
                None,
                None,
                wait_for_finalization.unwrap_or(true),
            )
            .napi()
            .and_then(outcome_to_native)
    }

    #[napi(js_name = "submitShielded")]
    pub fn submit_shielded(
        &self,
        intent: &NativeIntentCall,
        wallet: &NativeWallet,
    ) -> NapiResult<NativeTxOutcome> {
        let executor = self.executor();
        executor
            .submit_shielded(&intent.inner, &wallet.inner, None)
            .napi()
            .and_then(outcome_to_native)
    }
}

impl NativeExecutor {
    fn executor(&self) -> Executor<'_> {
        match &self.policy {
            Some(policy) => Executor::with_policy(&self.client, policy.clone()),
            None => Executor::new(&self.client),
        }
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

fn wire_value_list(name: &str, value: JsonValue) -> NapiResult<Vec<bittensor_core::codec::Value>> {
    match from_wire(value)? {
        bittensor_core::codec::Value::List(values) | bittensor_core::codec::Value::Tuple(values) => {
            Ok(values)
        }
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
