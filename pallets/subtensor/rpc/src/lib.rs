//! RPC interface for the custom Subtensor rpc methods

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

#[rpc(client, server)]
pub trait SubtensorCustomApi<BlockHash> {
    #[method(name = "delegateInfo_getDelegates")]
    fn get_delegates(&self, at: Option<BlockHash>) -> RpcResult<Vec<u8>>;
    #[method(name = "delegateInfo_getDelegate")]
    fn get_delegate(
        &self,
        delegate_account_vec: Vec<u8>,
        at: Option<BlockHash>,
    ) -> RpcResult<Vec<u8>>;
    #[method(name = "delegateInfo_getDelegated")]
    fn get_delegated(
        &self,
        delegatee_account_vec: Vec<u8>,
        at: Option<BlockHash>,
    ) -> RpcResult<Vec<u8>>;

    #[method(name = "neuronInfo_getNeuronsLite")]
    fn get_neurons_lite(&self, netuid: NetUid, at: Option<BlockHash>) -> RpcResult<Vec<u8>>;
    #[method(name = "neuronInfo_getNeuronLite")]
    fn get_neuron_lite(
        &self,
        netuid: NetUid,
        uid: u16,
        at: Option<BlockHash>,
    ) -> RpcResult<Vec<u8>>;
    #[method(name = "neuronInfo_getNeurons")]
    fn get_neurons(&self, netuid: NetUid, at: Option<BlockHash>) -> RpcResult<Vec<u8>>;
    #[method(name = "neuronInfo_getNeuron")]
    fn get_neuron(&self, netuid: NetUid, uid: u16, at: Option<BlockHash>) -> RpcResult<Vec<u8>>;
    #[method(name = "subnetInfo_getSubnetInfo")]
    fn get_subnet_info(&self, netuid: NetUid, at: Option<BlockHash>) -> RpcResult<Vec<u8>>;
    #[method(name = "subnetInfo_getSubnetsInfo")]
    fn get_subnets_info(&self, at: Option<BlockHash>) -> RpcResult<Vec<u8>>;
    #[method(name = "subnetInfo_getSubnetInfo_v2")]
    fn get_subnet_info_v2(&self, netuid: NetUid, at: Option<BlockHash>) -> RpcResult<Vec<u8>>;
    #[method(name = "subnetInfo_getSubnetsInfo_v2")]
    fn get_subnets_info_v2(&self, at: Option<BlockHash>) -> RpcResult<Vec<u8>>;
    #[method(name = "subnetInfo_getSubnetHyperparams")]
    fn get_subnet_hyperparams(&self, netuid: NetUid, at: Option<BlockHash>) -> RpcResult<Vec<u8>>;
    #[method(name = "subnetInfo_getSubnetHyperparamsV2")]
    fn get_subnet_hyperparams_v2(
        &self,
        netuid: NetUid,
        at: Option<BlockHash>,
    ) -> RpcResult<Vec<u8>>;
    #[method(name = "subnetInfo_getAllDynamicInfo")]
    fn get_all_dynamic_info(&self, at: Option<BlockHash>) -> RpcResult<Vec<u8>>;
    #[method(name = "subnetInfo_getDynamicInfo")]
    fn get_dynamic_info(&self, netuid: NetUid, at: Option<BlockHash>) -> RpcResult<Vec<u8>>;
    #[method(name = "subnetInfo_getAllMetagraphs")]
    fn get_all_metagraphs(&self, at: Option<BlockHash>) -> RpcResult<Vec<u8>>;
    #[method(name = "subnetInfo_getMetagraph")]
    fn get_metagraph(&self, netuid: NetUid, at: Option<BlockHash>) -> RpcResult<Vec<u8>>;
    #[method(name = "subnetInfo_getAllMechagraphs")]
    fn get_all_mechagraphs(&self, at: Option<BlockHash>) -> RpcResult<Vec<u8>>;
    #[method(name = "subnetInfo_getMechagraph")]
    fn get_mechagraph(
        &self,
        netuid: NetUid,
        mecid: MechId,
        at: Option<BlockHash>,
    ) -> RpcResult<Vec<u8>>;
    #[method(name = "subnetInfo_getSubnetState")]
    fn get_subnet_state(&self, netuid: NetUid, at: Option<BlockHash>) -> RpcResult<Vec<u8>>;

    #[method(name = "subnetInfo_getBlockEmission")]
    fn get_block_emission(&self, at: Option<BlockHash>) -> RpcResult<TaoBalance>;
    #[method(name = "subnetInfo_getLockCost")]
    fn get_network_lock_cost(&self, at: Option<BlockHash>) -> RpcResult<TaoBalance>;
    #[method(name = "subnetInfo_getSelectiveMetagraph")]
    fn get_selective_metagraph(
        &self,
        netuid: NetUid,
        metagraph_index: Vec<u16>,
        at: Option<BlockHash>,
    ) -> RpcResult<Vec<u8>>;
    #[method(name = "subnetInfo_getColdkeyAutoStakeHotkey")]
    fn get_coldkey_auto_stake_hotkey(
        &self,
        coldkey: AccountId32,
        netuid: NetUid,
        at: Option<BlockHash>,
    ) -> RpcResult<Vec<u8>>;
    #[method(name = "subnetInfo_getSelectiveMechagraph")]
    fn get_selective_mechagraph(
        &self,
        netuid: NetUid,
        mecid: MechId,
        metagraph_index: Vec<u16>,
        at: Option<BlockHash>,
    ) -> RpcResult<Vec<u8>>;
    #[method(name = "subnetInfo_getSubnetToPrune")]
    fn get_subnet_to_prune(&self, at: Option<BlockHash>) -> RpcResult<Option<NetUid>>;
    #[method(name = "subnetInfo_getSubnetAccountId")]
    fn get_subnet_account_id(&self, netuid: NetUid, at: Option<BlockHash>) -> RpcResult<Vec<u8>>;
    #[method(name = "stakeInfo_getColdkeyLock")]
    fn get_coldkey_lock(
        &self,
        coldkey: AccountId32,
        netuid: NetUid,
        at: Option<BlockHash>,
    ) -> RpcResult<Vec<u8>>;
}

pub struct SubtensorCustom<C, P> {
    /// Shared reference to the client.
    client: Arc<C>,
    _marker: std::marker::PhantomData<P>,
}

impl<C, P> SubtensorCustom<C, P> {
    /// Creates a new instance of the TransactionPayment Rpc helper.
    pub fn new(client: Arc<C>) -> Self {
        Self {
            client,
            _marker: Default::default(),
        }
    }
}

/// Error type of this RPC api.
pub enum Error {
    /// The call to runtime failed.
    RuntimeError(String),
}

impl From<Error> for ErrorObjectOwned {
    fn from(e: Error) -> Self {
        match e {
            Error::RuntimeError(e) => ErrorObject::owned(1, e, None::<()>),
        }
    }
}

impl From<Error> for i32 {
    fn from(e: Error) -> i32 {
        match e {
            Error::RuntimeError(_) => 1,
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
        let at = at.unwrap_or_else(|| self.client.info().best_hash);

        match api.get_delegates(at) {
            Ok(result) => Ok(result.encode()),
            Err(e) => {
                Err(Error::RuntimeError(format!("Unable to get delegates info: {e:?}")).into())
            }
        }
    }

    fn get_delegate(
        &self,
        delegate_account_vec: Vec<u8>,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = at.unwrap_or_else(|| self.client.info().best_hash);

        let delegate_account = match AccountId32::decode(&mut &delegate_account_vec[..]) {
            Ok(delegate_account) => delegate_account,
            Err(e) => {
                return Err(
                    Error::RuntimeError(format!("Unable to get delegates info: {e:?}")).into(),
                );
            }
        };
        match api.get_delegate(at, delegate_account) {
            Ok(result) => Ok(result.encode()),
            Err(e) => {
                Err(Error::RuntimeError(format!("Unable to get delegates info: {e:?}")).into())
            }
        }
    }

    fn get_delegated(
        &self,
        delegatee_account_vec: Vec<u8>,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = at.unwrap_or_else(|| self.client.info().best_hash);

        let delegatee_account = match AccountId32::decode(&mut &delegatee_account_vec[..]) {
            Ok(delegatee_account) => delegatee_account,
            Err(e) => {
                return Err(
                    Error::RuntimeError(format!("Unable to get delegates info: {e:?}")).into(),
                );
            }
        };
        match api.get_delegated(at, delegatee_account) {
            Ok(result) => Ok(result.encode()),
            Err(e) => {
                Err(Error::RuntimeError(format!("Unable to get delegates info: {e:?}")).into())
            }
        }
    }

    fn get_neurons_lite(
        &self,
        netuid: NetUid,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = at.unwrap_or_else(|| self.client.info().best_hash);

        match api.get_neurons_lite(at, netuid) {
            Ok(result) => Ok(result.encode()),
            Err(e) => {
                Err(Error::RuntimeError(format!("Unable to get neurons lite info: {e:?}")).into())
            }
        }
    }

    fn get_neuron_lite(
        &self,
        netuid: NetUid,
        uid: u16,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = at.unwrap_or_else(|| self.client.info().best_hash);

        match api.get_neuron_lite(at, netuid, uid) {
            Ok(result) => Ok(result.encode()),
            Err(e) => {
                Err(Error::RuntimeError(format!("Unable to get neurons lite info: {e:?}")).into())
            }
        }
    }

    fn get_neurons(
        &self,
        netuid: NetUid,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = at.unwrap_or_else(|| self.client.info().best_hash);

        match api.get_neurons(at, netuid) {
            Ok(result) => Ok(result.encode()),
            Err(e) => Err(Error::RuntimeError(format!("Unable to get neurons info: {e:?}")).into()),
        }
    }

    fn get_neuron(
        &self,
        netuid: NetUid,
        uid: u16,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = at.unwrap_or_else(|| self.client.info().best_hash);

        match api.get_neuron(at, netuid, uid) {
            Ok(result) => Ok(result.encode()),
            Err(e) => Err(Error::RuntimeError(format!("Unable to get neuron info: {e:?}")).into()),
        }
    }

    fn get_subnet_info(
        &self,
        netuid: NetUid,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = at.unwrap_or_else(|| self.client.info().best_hash);

        match api.get_subnet_info(at, netuid) {
            Ok(result) => Ok(result.encode()),
            Err(e) => Err(Error::RuntimeError(format!("Unable to get subnet info: {e:?}")).into()),
        }
    }

    #[allow(deprecated)]
    fn get_subnet_hyperparams(
        &self,
        netuid: NetUid,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = at.unwrap_or_else(|| self.client.info().best_hash);

        match api.get_subnet_hyperparams(at, netuid) {
            Ok(result) => Ok(result.encode()),
            Err(e) => Err(Error::RuntimeError(format!("Unable to get subnet info: {e:?}")).into()),
        }
    }

    #[allow(deprecated)]
    fn get_subnet_hyperparams_v2(
        &self,
        netuid: NetUid,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = at.unwrap_or_else(|| self.client.info().best_hash);

        match api.get_subnet_hyperparams_v2(at, netuid) {
            Ok(result) => Ok(result.encode()),
            Err(e) => Err(Error::RuntimeError(format!("Unable to get subnet info: {e:?}")).into()),
        }
    }

    fn get_all_dynamic_info(&self, at: Option<<Block as BlockT>::Hash>) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = at.unwrap_or_else(|| self.client.info().best_hash);

        match api.get_all_dynamic_info(at) {
            Ok(result) => Ok(result.encode()),
            Err(e) => Err(Error::RuntimeError(format!(
                "Unable to get dynamic subnets info: {e:?}"
            ))
            .into()),
        }
    }

    fn get_all_metagraphs(&self, at: Option<<Block as BlockT>::Hash>) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = at.unwrap_or_else(|| self.client.info().best_hash);

        match api.get_all_metagraphs(at) {
            Ok(result) => Ok(result.encode()),
            Err(e) => Err(Error::RuntimeError(format!("Unable to get metagraps: {e:?}")).into()),
        }
    }

    fn get_all_mechagraphs(&self, at: Option<<Block as BlockT>::Hash>) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = at.unwrap_or_else(|| self.client.info().best_hash);

        match api.get_all_mechagraphs(at) {
            Ok(result) => Ok(result.encode()),
            Err(e) => Err(Error::RuntimeError(format!("Unable to get metagraps: {e:?}")).into()),
        }
    }

    fn get_dynamic_info(
        &self,
        netuid: NetUid,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = at.unwrap_or_else(|| self.client.info().best_hash);

        match api.get_dynamic_info(at, netuid) {
            Ok(result) => Ok(result.encode()),
            Err(e) => Err(Error::RuntimeError(format!(
                "Unable to get dynamic subnets info: {e:?}"
            ))
            .into()),
        }
    }

    fn get_metagraph(
        &self,
        netuid: NetUid,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = at.unwrap_or_else(|| self.client.info().best_hash);
        match api.get_metagraph(at, netuid) {
            Ok(result) => Ok(result.encode()),
            Err(e) => Err(Error::RuntimeError(format!(
                "Unable to get dynamic subnets info: {e:?}"
            ))
            .into()),
        }
    }

    fn get_mechagraph(
        &self,
        netuid: NetUid,
        mecid: MechId,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = at.unwrap_or_else(|| self.client.info().best_hash);
        match api.get_mechagraph(at, netuid, mecid) {
            Ok(result) => Ok(result.encode()),
            Err(e) => Err(Error::RuntimeError(format!(
                "Unable to get dynamic subnets info: {e:?}"
            ))
            .into()),
        }
    }

    fn get_subnet_state(
        &self,
        netuid: NetUid,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = at.unwrap_or_else(|| self.client.info().best_hash);

        match api.get_subnet_state(at, netuid) {
            Ok(result) => Ok(result.encode()),
            Err(e) => {
                Err(Error::RuntimeError(format!("Unable to get subnet state info: {e:?}")).into())
            }
        }
    }

    fn get_subnets_info(&self, at: Option<<Block as BlockT>::Hash>) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = at.unwrap_or_else(|| self.client.info().best_hash);

        match api.get_subnets_info(at) {
            Ok(result) => Ok(result.encode()),
            Err(e) => Err(Error::RuntimeError(format!("Unable to get subnets info: {e:?}")).into()),
        }
    }

    fn get_subnet_info_v2(
        &self,
        netuid: NetUid,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = at.unwrap_or_else(|| self.client.info().best_hash);

        match api.get_subnet_info_v2(at, netuid) {
            Ok(result) => Ok(result.encode()),
            Err(e) => Err(Error::RuntimeError(format!("Unable to get subnet info: {e:?}")).into()),
        }
    }

    fn get_subnets_info_v2(&self, at: Option<<Block as BlockT>::Hash>) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = at.unwrap_or_else(|| self.client.info().best_hash);

        match api.get_subnets_info_v2(at) {
            Ok(result) => Ok(result.encode()),
            Err(e) => Err(Error::RuntimeError(format!("Unable to get subnets info: {e:?}")).into()),
        }
    }

    fn get_block_emission(&self, at: Option<<Block as BlockT>::Hash>) -> RpcResult<TaoBalance> {
        let api = self.client.runtime_api();
        let at = at.unwrap_or_else(|| self.client.info().best_hash);

        api.get_block_emission(at)
            .map_err(|e| Error::RuntimeError(format!("Unable to get block emission: {e:?}")).into())
    }

    fn get_network_lock_cost(&self, at: Option<<Block as BlockT>::Hash>) -> RpcResult<TaoBalance> {
        let api = self.client.runtime_api();
        let at = at.unwrap_or_else(|| self.client.info().best_hash);

        api.get_network_registration_cost(at).map_err(|e| {
            Error::RuntimeError(format!("Unable to get subnet lock cost: {e:?}")).into()
        })
    }

    fn get_selective_metagraph(
        &self,
        netuid: NetUid,
        metagraph_index: Vec<u16>,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = at.unwrap_or_else(|| self.client.info().best_hash);

        match api.get_selective_metagraph(at, netuid, metagraph_index) {
            Ok(result) => Ok(result.encode()),
            Err(e) => {
                Err(Error::RuntimeError(format!("Unable to get selective metagraph: {e:?}")).into())
            }
        }
    }

    fn get_coldkey_auto_stake_hotkey(
        &self,
        coldkey: AccountId32,
        netuid: NetUid,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = at.unwrap_or_else(|| self.client.info().best_hash);

        match api.get_coldkey_auto_stake_hotkey(at, coldkey, netuid) {
            Ok(result) => Ok(result.encode()),
            Err(e) => Err(Error::RuntimeError(format!(
                "Unable to get coldkey auto stake hotkey: {e:?}"
            ))
            .into()),
        }
    }

    fn get_selective_mechagraph(
        &self,
        netuid: NetUid,
        mecid: MechId,
        metagraph_index: Vec<u16>,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = at.unwrap_or_else(|| self.client.info().best_hash);

        match api.get_selective_mechagraph(at, netuid, mecid, metagraph_index) {
            Ok(result) => Ok(result.encode()),
            Err(e) => {
                Err(Error::RuntimeError(format!("Unable to get selective metagraph: {e:?}")).into())
            }
        }
    }

    fn get_subnet_to_prune(
        &self,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Option<NetUid>> {
        let api = self.client.runtime_api();
        let at = at.unwrap_or_else(|| self.client.info().best_hash);

        match api.get_subnet_to_prune(at) {
            Ok(result) => Ok(result),
            Err(e) => {
                Err(Error::RuntimeError(format!("Unable to get subnet to prune: {e:?}")).into())
            }
        }
    }

    fn get_subnet_account_id(
        &self,
        netuid: NetUid,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = at.unwrap_or_else(|| self.client.info().best_hash);

        match api.get_subnet_account_id(at, netuid) {
            Ok(result) => Ok(result.encode()),
            Err(_) => Err(Error::RuntimeError("Subnet does not exist".to_string()).into()),
        }
    }

    fn get_coldkey_lock(
        &self,
        coldkey: AccountId32,
        netuid: NetUid,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<u8>> {
        let api = self.client.runtime_api();
        let at = at.unwrap_or_else(|| self.client.info().best_hash);

        match api.get_coldkey_lock(at, coldkey, netuid) {
            Ok(result) => Ok(result.encode()),
            Err(e) => Err(Error::RuntimeError(format!("Unable to get coldkey lock: {e:?}")).into()),
        }
    }
}
