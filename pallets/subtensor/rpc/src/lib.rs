//! JSON-RPC surface for Subtensor custom queries (`delegateInfo_*`, `neuronInfo_*`,
//! `subnetInfo_*`, `stakeInfo_*`).
//!
//! Each method resolves an optional block hash (default: best), calls the matching
//! [`subtensor_custom_rpc_runtime_api`] trait, and returns **SCALE-encoded** bytes
//! (except emission / lock-cost / prune helpers that return typed values).
//!
//! # Frozen names
//!
//! `#[method(name = "…")]` strings are Tier C — never rename. Rust trait method names
//! on [`SubtensorCustomApi`] match those strings for searchability; keep them aligned.
//!
//! # Related crate
//!
//! Runtime API declarations: `subtensor-custom-rpc-runtime-api`
//! (`pallets/subtensor/runtime-api`).

use codec::{Decode, Encode};
use jsonrpsee::{
    core::RpcResult,
    proc_macros::rpc,
    types::{ErrorObjectOwned, error::ErrorObject},
};
use sp_blockchain::HeaderBackend;
use sp_runtime::{AccountId32, traits::Block as BlockT};
use std::sync::Arc;
use subtensor_runtime_common::{MechId, NetUid, TaoBalance};

use sp_api::ProvideRuntimeApi;

pub use subtensor_custom_rpc_runtime_api::{
    DelegateInfoRuntimeApi, NeuronInfoRuntimeApi, StakeInfoRuntimeApi, SubnetInfoRuntimeApi,
    SubnetRegistrationRuntimeApi,
};

/// Custom Subtensor JSON-RPC methods (jsonrpsee client + server).
///
/// Most getters return SCALE-encoded pallet `rpc_info` structs as `Vec<u8>` for
/// substrate-facing clients; decode with the matching type from `pallet_subtensor::rpc_info`.
#[rpc(client, server)]
pub trait SubtensorCustomApi<BlockHash> {
    /// All delegates (`DelegateInfo`), SCALE-encoded.
    #[method(name = "delegateInfo_getDelegates")]
    fn get_delegates(&self, at: Option<BlockHash>) -> RpcResult<Vec<u8>>;
    /// One delegate by SCALE-encoded [`AccountId32`] hotkey bytes.
    #[method(name = "delegateInfo_getDelegate")]
    fn get_delegate(
        &self,
        delegate_account_vec: Vec<u8>,
        at: Option<BlockHash>,
    ) -> RpcResult<Vec<u8>>;
    /// Delegates that a coldkey has stake on (`get_delegated`), SCALE-encoded.
    #[method(name = "delegateInfo_getDelegated")]
    fn get_delegated(
        &self,
        delegatee_account_vec: Vec<u8>,
        at: Option<BlockHash>,
    ) -> RpcResult<Vec<u8>>;

    /// Lite neurons for a subnet, SCALE-encoded.
    #[method(name = "neuronInfo_getNeuronsLite")]
    fn get_neurons_lite(&self, netuid: NetUid, at: Option<BlockHash>) -> RpcResult<Vec<u8>>;
    /// One lite neuron by uid, SCALE-encoded.
    #[method(name = "neuronInfo_getNeuronLite")]
    fn get_neuron_lite(
        &self,
        netuid: NetUid,
        uid: u16,
        at: Option<BlockHash>,
    ) -> RpcResult<Vec<u8>>;
    /// Full neurons for a subnet, SCALE-encoded.
    #[method(name = "neuronInfo_getNeurons")]
    fn get_neurons(&self, netuid: NetUid, at: Option<BlockHash>) -> RpcResult<Vec<u8>>;
    /// One full neuron by uid, SCALE-encoded.
    #[method(name = "neuronInfo_getNeuron")]
    fn get_neuron(&self, netuid: NetUid, uid: u16, at: Option<BlockHash>) -> RpcResult<Vec<u8>>;
    /// Legacy subnet info for one netuid, SCALE-encoded.
    #[method(name = "subnetInfo_getSubnetInfo")]
    fn get_subnet_info(&self, netuid: NetUid, at: Option<BlockHash>) -> RpcResult<Vec<u8>>;
    /// Legacy subnet info for all netuids, SCALE-encoded.
    #[method(name = "subnetInfo_getSubnetsInfo")]
    fn get_subnets_info(&self, at: Option<BlockHash>) -> RpcResult<Vec<u8>>;
    /// `SubnetInfov2` for one netuid, SCALE-encoded.
    #[method(name = "subnetInfo_getSubnetInfo_v2")]
    fn get_subnet_info_v2(&self, netuid: NetUid, at: Option<BlockHash>) -> RpcResult<Vec<u8>>;
    /// `SubnetInfov2` for all netuids, SCALE-encoded.
    #[method(name = "subnetInfo_getSubnetsInfo_v2")]
    fn get_subnets_info_v2(&self, at: Option<BlockHash>) -> RpcResult<Vec<u8>>;
    /// Deprecated hyperparams v1; prefer on-chain `get_subnet_hyperparams_v3` via runtime API.
    #[method(name = "subnetInfo_getSubnetHyperparams")]
    fn get_subnet_hyperparams(&self, netuid: NetUid, at: Option<BlockHash>) -> RpcResult<Vec<u8>>;
    /// Deprecated hyperparams v2; prefer on-chain `get_subnet_hyperparams_v3` via runtime API.
    #[method(name = "subnetInfo_getSubnetHyperparamsV2")]
    fn get_subnet_hyperparams_v2(
        &self,
        netuid: NetUid,
        at: Option<BlockHash>,
    ) -> RpcResult<Vec<u8>>;
    /// Dynamic pool info for all subnets, SCALE-encoded.
    #[method(name = "subnetInfo_getAllDynamicInfo")]
    fn get_all_dynamic_info(&self, at: Option<BlockHash>) -> RpcResult<Vec<u8>>;
    /// Dynamic pool info for one subnet, SCALE-encoded.
    #[method(name = "subnetInfo_getDynamicInfo")]
    fn get_dynamic_info(&self, netuid: NetUid, at: Option<BlockHash>) -> RpcResult<Vec<u8>>;
    /// Root metagraphs for all subnets, SCALE-encoded.
    #[method(name = "subnetInfo_getAllMetagraphs")]
    fn get_all_metagraphs(&self, at: Option<BlockHash>) -> RpcResult<Vec<u8>>;
    /// Root metagraph for one subnet, SCALE-encoded.
    #[method(name = "subnetInfo_getMetagraph")]
    fn get_metagraph(&self, netuid: NetUid, at: Option<BlockHash>) -> RpcResult<Vec<u8>>;
    /// All mechanism metagraphs, SCALE-encoded.
    #[method(name = "subnetInfo_getAllMechagraphs")]
    fn get_all_mechagraphs(&self, at: Option<BlockHash>) -> RpcResult<Vec<u8>>;
    /// Mechanism metagraph for `(netuid, mecid)`, SCALE-encoded.
    #[method(name = "subnetInfo_getMechagraph")]
    fn get_mechagraph(
        &self,
        netuid: NetUid,
        mecid: MechId,
        at: Option<BlockHash>,
    ) -> RpcResult<Vec<u8>>;
    /// Show-subnet state snapshot, SCALE-encoded.
    #[method(name = "subnetInfo_getSubnetState")]
    fn get_subnet_state(&self, netuid: NetUid, at: Option<BlockHash>) -> RpcResult<Vec<u8>>;

    /// Network-wide TAO block emission (rao), not SCALE-wrapped.
    #[method(name = "subnetInfo_getBlockEmission")]
    fn get_block_emission(&self, at: Option<BlockHash>) -> RpcResult<TaoBalance>;
    /// TAO lock cost to register a new subnet (rao); calls `get_network_registration_cost`.
    #[method(name = "subnetInfo_getLockCost")]
    fn get_network_lock_cost(&self, at: Option<BlockHash>) -> RpcResult<TaoBalance>;
    /// Partial root metagraph columns, SCALE-encoded.
    #[method(name = "subnetInfo_getSelectiveMetagraph")]
    fn get_selective_metagraph(
        &self,
        netuid: NetUid,
        metagraph_index: Vec<u16>,
        at: Option<BlockHash>,
    ) -> RpcResult<Vec<u8>>;
    /// Coldkey auto-stake hotkey for a subnet, SCALE-encoded `Option<AccountId32>`.
    #[method(name = "subnetInfo_getColdkeyAutoStakeHotkey")]
    fn get_coldkey_auto_stake_hotkey(
        &self,
        coldkey: AccountId32,
        netuid: NetUid,
        at: Option<BlockHash>,
    ) -> RpcResult<Vec<u8>>;
    /// Partial mechanism metagraph columns, SCALE-encoded.
    #[method(name = "subnetInfo_getSelectiveMechagraph")]
    fn get_selective_mechagraph(
        &self,
        netuid: NetUid,
        mecid: MechId,
        metagraph_index: Vec<u16>,
        at: Option<BlockHash>,
    ) -> RpcResult<Vec<u8>>;
    /// Netuid next up for pruning, if any.
    #[method(name = "subnetInfo_getSubnetToPrune")]
    fn get_subnet_to_prune(&self, at: Option<BlockHash>) -> RpcResult<Option<NetUid>>;
    /// Subnet account id, SCALE-encoded; errors if the subnet does not exist.
    #[method(name = "subnetInfo_getSubnetAccountId")]
    fn get_subnet_account_id(&self, netuid: NetUid, at: Option<BlockHash>) -> RpcResult<Vec<u8>>;
    /// Coldkey lock state on a subnet, SCALE-encoded.
    #[method(name = "stakeInfo_getColdkeyLock")]
    fn get_coldkey_lock(
        &self,
        coldkey: AccountId32,
        netuid: NetUid,
        at: Option<BlockHash>,
    ) -> RpcResult<Vec<u8>>;
}

/// Node-side RPC handler that forwards to Subtensor custom runtime APIs.
pub struct SubtensorCustom<C, P> {
    /// Shared reference to the client.
    client: Arc<C>,
    _marker: std::marker::PhantomData<P>,
}

impl<C, P> SubtensorCustom<C, P> {
    /// Creates a new Subtensor custom RPC handler around `client`.
    pub fn new(client: Arc<C>) -> Self {
        Self {
            client,
            _marker: Default::default(),
        }
    }
}

impl<C, Block> SubtensorCustom<C, Block>
where
    Block: BlockT,
    C: HeaderBackend<Block>,
{
    /// `at` if provided, otherwise the client's best block hash.
    fn block_hash_or_best(&self, at: Option<Block::Hash>) -> Block::Hash {
        at.unwrap_or_else(|| self.client.info().best_hash)
    }
}

/// Maps a runtime-API `Result` to SCALE-encoded `Vec<u8>`, or a JSON-RPC runtime error.
fn scale_encode_runtime_api_result<T, E>(result: Result<T, E>, context: &str) -> RpcResult<Vec<u8>>
where
    T: Encode,
    E: core::fmt::Debug,
{
    match result {
        Ok(value) => Ok(value.encode()),
        Err(e) => Err(SubtensorRpcError::RuntimeError(format!("{context}: {e:?}")).into()),
    }
}

/// Decodes an [`AccountId32`] from raw bytes for delegate RPC account arguments.
fn decode_account_id32_arg(account_bytes: &[u8], context: &str) -> RpcResult<AccountId32> {
    AccountId32::decode(&mut &account_bytes[..])
        .map_err(|e| SubtensorRpcError::RuntimeError(format!("{context}: {e:?}")).into())
}

/// Error type of this RPC api.
pub enum SubtensorRpcError {
    /// The call to runtime failed.
    RuntimeError(String),
}

impl From<SubtensorRpcError> for ErrorObjectOwned {
    fn from(e: SubtensorRpcError) -> Self {
        match e {
            SubtensorRpcError::RuntimeError(e) => ErrorObject::owned(1, e, None::<()>),
        }
    }
}

impl From<SubtensorRpcError> for i32 {
    fn from(e: SubtensorRpcError) -> i32 {
        match e {
            SubtensorRpcError::RuntimeError(_) => 1,
        }
    }
}

impl<C, Block> SubtensorCustomApiServer<<Block as BlockT>::Hash> for SubtensorCustom<C, Block>
where
    Block: BlockT,
    C: ProvideRuntimeApi<Block> + HeaderBackend<Block> + Send + Sync + 'static,
    C::Api: DelegateInfoRuntimeApi<Block>,
    C::Api: NeuronInfoRuntimeApi<Block>,
    C::Api: SubnetInfoRuntimeApi<Block>,
    C::Api: StakeInfoRuntimeApi<Block>,
    C::Api: SubnetRegistrationRuntimeApi<Block>,
{
    fn get_delegates(&self, at: Option<<Block as BlockT>::Hash>) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = self.block_hash_or_best(at);
        scale_encode_runtime_api_result(api.get_delegates(at), "Unable to get delegates info")
    }

    fn get_delegate(
        &self,
        delegate_account_vec: Vec<u8>,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = self.block_hash_or_best(at);
        let delegate_account =
            decode_account_id32_arg(&delegate_account_vec, "Unable to get delegates info")?;
        scale_encode_runtime_api_result(
            api.get_delegate(at, delegate_account),
            "Unable to get delegates info",
        )
    }

    fn get_delegated(
        &self,
        delegatee_account_vec: Vec<u8>,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = self.block_hash_or_best(at);
        let delegatee_account =
            decode_account_id32_arg(&delegatee_account_vec, "Unable to get delegates info")?;
        scale_encode_runtime_api_result(
            api.get_delegated(at, delegatee_account),
            "Unable to get delegates info",
        )
    }

    fn get_neurons_lite(
        &self,
        netuid: NetUid,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = self.block_hash_or_best(at);
        scale_encode_runtime_api_result(
            api.get_neurons_lite(at, netuid),
            "Unable to get neurons lite info",
        )
    }

    fn get_neuron_lite(
        &self,
        netuid: NetUid,
        uid: u16,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = self.block_hash_or_best(at);
        scale_encode_runtime_api_result(
            api.get_neuron_lite(at, netuid, uid),
            "Unable to get neurons lite info",
        )
    }

    fn get_neurons(
        &self,
        netuid: NetUid,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = self.block_hash_or_best(at);
        scale_encode_runtime_api_result(api.get_neurons(at, netuid), "Unable to get neurons info")
    }

    fn get_neuron(
        &self,
        netuid: NetUid,
        uid: u16,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = self.block_hash_or_best(at);
        scale_encode_runtime_api_result(
            api.get_neuron(at, netuid, uid),
            "Unable to get neuron info",
        )
    }

    fn get_subnet_info(
        &self,
        netuid: NetUid,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = self.block_hash_or_best(at);
        scale_encode_runtime_api_result(
            api.get_subnet_info(at, netuid),
            "Unable to get subnet info",
        )
    }

    #[allow(deprecated)]
    fn get_subnet_hyperparams(
        &self,
        netuid: NetUid,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = self.block_hash_or_best(at);
        scale_encode_runtime_api_result(
            api.get_subnet_hyperparams(at, netuid),
            "Unable to get subnet hyperparams",
        )
    }

    #[allow(deprecated)]
    fn get_subnet_hyperparams_v2(
        &self,
        netuid: NetUid,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = self.block_hash_or_best(at);
        scale_encode_runtime_api_result(
            api.get_subnet_hyperparams_v2(at, netuid),
            "Unable to get subnet hyperparams v2",
        )
    }

    fn get_all_dynamic_info(&self, at: Option<<Block as BlockT>::Hash>) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = self.block_hash_or_best(at);
        scale_encode_runtime_api_result(
            api.get_all_dynamic_info(at),
            "Unable to get dynamic subnets info",
        )
    }

    fn get_all_metagraphs(&self, at: Option<<Block as BlockT>::Hash>) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = self.block_hash_or_best(at);
        scale_encode_runtime_api_result(api.get_all_metagraphs(at), "Unable to get metagraphs")
    }

    fn get_all_mechagraphs(&self, at: Option<<Block as BlockT>::Hash>) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = self.block_hash_or_best(at);
        scale_encode_runtime_api_result(api.get_all_mechagraphs(at), "Unable to get mechagraphs")
    }

    fn get_dynamic_info(
        &self,
        netuid: NetUid,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = self.block_hash_or_best(at);
        scale_encode_runtime_api_result(
            api.get_dynamic_info(at, netuid),
            "Unable to get dynamic subnet info",
        )
    }

    fn get_metagraph(
        &self,
        netuid: NetUid,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = self.block_hash_or_best(at);
        scale_encode_runtime_api_result(api.get_metagraph(at, netuid), "Unable to get metagraph")
    }

    fn get_mechagraph(
        &self,
        netuid: NetUid,
        mecid: MechId,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = self.block_hash_or_best(at);
        scale_encode_runtime_api_result(
            api.get_mechagraph(at, netuid, mecid),
            "Unable to get mechagraph",
        )
    }

    fn get_subnet_state(
        &self,
        netuid: NetUid,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = self.block_hash_or_best(at);
        scale_encode_runtime_api_result(
            api.get_subnet_state(at, netuid),
            "Unable to get subnet state info",
        )
    }

    fn get_subnets_info(&self, at: Option<<Block as BlockT>::Hash>) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = self.block_hash_or_best(at);
        scale_encode_runtime_api_result(api.get_subnets_info(at), "Unable to get subnets info")
    }

    fn get_subnet_info_v2(
        &self,
        netuid: NetUid,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = self.block_hash_or_best(at);
        scale_encode_runtime_api_result(
            api.get_subnet_info_v2(at, netuid),
            "Unable to get subnet info",
        )
    }

    fn get_subnets_info_v2(&self, at: Option<<Block as BlockT>::Hash>) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = self.block_hash_or_best(at);
        scale_encode_runtime_api_result(api.get_subnets_info_v2(at), "Unable to get subnets info")
    }

    fn get_block_emission(&self, at: Option<<Block as BlockT>::Hash>) -> RpcResult<TaoBalance> {
        let api = self.client.runtime_api();
        let at = self.block_hash_or_best(at);

        api.get_block_emission(at).map_err(|e| {
            SubtensorRpcError::RuntimeError(format!("Unable to get block emission: {e:?}")).into()
        })
    }

    fn get_network_lock_cost(&self, at: Option<<Block as BlockT>::Hash>) -> RpcResult<TaoBalance> {
        let api = self.client.runtime_api();
        let at = self.block_hash_or_best(at);

        api.get_network_registration_cost(at).map_err(|e| {
            SubtensorRpcError::RuntimeError(format!("Unable to get subnet lock cost: {e:?}")).into()
        })
    }

    fn get_selective_metagraph(
        &self,
        netuid: NetUid,
        metagraph_index: Vec<u16>,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = self.block_hash_or_best(at);
        scale_encode_runtime_api_result(
            api.get_selective_metagraph(at, netuid, metagraph_index),
            "Unable to get selective metagraph",
        )
    }

    fn get_coldkey_auto_stake_hotkey(
        &self,
        coldkey: AccountId32,
        netuid: NetUid,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = self.block_hash_or_best(at);
        scale_encode_runtime_api_result(
            api.get_coldkey_auto_stake_hotkey(at, coldkey, netuid),
            "Unable to get coldkey auto stake hotkey",
        )
    }

    fn get_selective_mechagraph(
        &self,
        netuid: NetUid,
        mecid: MechId,
        metagraph_index: Vec<u16>,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = self.block_hash_or_best(at);
        scale_encode_runtime_api_result(
            api.get_selective_mechagraph(at, netuid, mecid, metagraph_index),
            "Unable to get selective mechagraph",
        )
    }

    fn get_subnet_to_prune(
        &self,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Option<NetUid>> {
        let api = self.client.runtime_api();
        let at = self.block_hash_or_best(at);

        api.get_subnet_to_prune(at).map_err(|e| {
            SubtensorRpcError::RuntimeError(format!("Unable to get subnet to prune: {e:?}")).into()
        })
    }

    fn get_subnet_account_id(
        &self,
        netuid: NetUid,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = self.block_hash_or_best(at);

        match api.get_subnet_account_id(at, netuid) {
            Ok(result) => Ok(result.encode()),
            Err(_) => {
                Err(SubtensorRpcError::RuntimeError("Subnet does not exist".to_string()).into())
            }
        }
    }

    fn get_coldkey_lock(
        &self,
        coldkey: AccountId32,
        netuid: NetUid,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = self.block_hash_or_best(at);
        scale_encode_runtime_api_result(
            api.get_coldkey_lock(at, coldkey, netuid),
            "Unable to get coldkey lock",
        )
    }
}
