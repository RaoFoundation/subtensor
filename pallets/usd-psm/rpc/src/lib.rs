//! RPC interface for the canonical rails (USD PSM pallet).

use std::sync::Arc;

use jsonrpsee::{
    core::RpcResult,
    proc_macros::rpc,
    types::{ErrorObjectOwned, error::ErrorObject},
};
use sp_api::ProvideRuntimeApi;
use sp_blockchain::HeaderBackend;
use sp_core::H160;
use sp_core::crypto::AccountId32;
use sp_runtime::traits::Block as BlockT;

pub use pallet_usd_psm_runtime_api::{
    RailsAlphaAttestation, RailsAssetInfo, RailsHubInfo, RailsPoolState, RailsRuntimeApi,
};

#[rpc(client, server)]
pub trait RailsRpcApi<BlockHash> {
    /// Quote tUSD -> TAO through the canonical pool.
    #[method(name = "rails_quoteUsdToTao")]
    fn quote_usd_to_tao(&self, amount: u64, at: Option<BlockHash>) -> RpcResult<Option<u64>>;
    /// Quote TAO -> tUSD through the canonical pool.
    #[method(name = "rails_quoteTaoToUsd")]
    fn quote_tao_to_usd(&self, amount: u64, at: Option<BlockHash>) -> RpcResult<Option<u64>>;
    /// Canonical pool reserves and fee.
    #[method(name = "rails_poolState")]
    fn pool_state(&self, at: Option<BlockHash>) -> RpcResult<RailsPoolState>;
    /// Registered PSM assets.
    #[method(name = "rails_assets")]
    fn assets(&self, at: Option<BlockHash>) -> RpcResult<Vec<RailsAssetInfo>>;
    /// The registered Gateway contract.
    #[method(name = "rails_gateway")]
    fn gateway(&self, at: Option<BlockHash>) -> RpcResult<Option<H160>>;
    /// tUSD balance of an account.
    #[method(name = "rails_tusdBalance")]
    fn tusd_balance(&self, account: AccountId32, at: Option<BlockHash>) -> RpcResult<u64>;
    /// Supply attestation for a subnet's wrapped alpha.
    #[method(name = "rails_alphaAttestation")]
    fn alpha_attestation(
        &self,
        netuid: u16,
        at: Option<BlockHash>,
    ) -> RpcResult<RailsAlphaAttestation>;
    /// Outbound hub configuration.
    #[method(name = "rails_hubInfo")]
    fn hub_info(&self, at: Option<BlockHash>) -> RpcResult<RailsHubInfo>;
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

/// RPC handler.
pub struct Rails<C, P> {
    client: Arc<C>,
    _marker: std::marker::PhantomData<P>,
}

impl<C, P> Rails<C, P> {
    /// Create a new handler backed by `client`.
    pub fn new(client: Arc<C>) -> Self {
        Self {
            client,
            _marker: Default::default(),
        }
    }
}

impl<C, Block> RailsRpcApiServer<<Block as BlockT>::Hash> for Rails<C, Block>
where
    Block: BlockT,
    C: ProvideRuntimeApi<Block> + HeaderBackend<Block> + Send + Sync + 'static,
    C::Api: RailsRuntimeApi<Block>,
{
    fn quote_usd_to_tao(
        &self,
        amount: u64,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Option<u64>> {
        let api = self.client.runtime_api();
        let at = at.unwrap_or_else(|| self.client.info().best_hash);
        api.rails_quote_usd_to_tao(at, amount)
            .map_err(|e| Error::RuntimeError(format!("unable to quote: {e:?}")).into())
    }

    fn quote_tao_to_usd(
        &self,
        amount: u64,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Option<u64>> {
        let api = self.client.runtime_api();
        let at = at.unwrap_or_else(|| self.client.info().best_hash);
        api.rails_quote_tao_to_usd(at, amount)
            .map_err(|e| Error::RuntimeError(format!("unable to quote: {e:?}")).into())
    }

    fn pool_state(&self, at: Option<<Block as BlockT>::Hash>) -> RpcResult<RailsPoolState> {
        let api = self.client.runtime_api();
        let at = at.unwrap_or_else(|| self.client.info().best_hash);
        api.rails_pool_state(at)
            .map_err(|e| Error::RuntimeError(format!("unable to read pool: {e:?}")).into())
    }

    fn assets(&self, at: Option<<Block as BlockT>::Hash>) -> RpcResult<Vec<RailsAssetInfo>> {
        let api = self.client.runtime_api();
        let at = at.unwrap_or_else(|| self.client.info().best_hash);
        api.rails_assets(at)
            .map_err(|e| Error::RuntimeError(format!("unable to read assets: {e:?}")).into())
    }

    fn gateway(&self, at: Option<<Block as BlockT>::Hash>) -> RpcResult<Option<H160>> {
        let api = self.client.runtime_api();
        let at = at.unwrap_or_else(|| self.client.info().best_hash);
        api.rails_gateway(at)
            .map_err(|e| Error::RuntimeError(format!("unable to read gateway: {e:?}")).into())
    }

    fn tusd_balance(
        &self,
        account: AccountId32,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<u64> {
        let api = self.client.runtime_api();
        let at = at.unwrap_or_else(|| self.client.info().best_hash);
        api.rails_tusd_balance(at, account)
            .map_err(|e| Error::RuntimeError(format!("unable to read balance: {e:?}")).into())
    }

    fn alpha_attestation(
        &self,
        netuid: u16,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<RailsAlphaAttestation> {
        let api = self.client.runtime_api();
        let at = at.unwrap_or_else(|| self.client.info().best_hash);
        api.rails_alpha_attestation(at, netuid)
            .map_err(|e| Error::RuntimeError(format!("unable to read attestation: {e:?}")).into())
    }

    fn hub_info(&self, at: Option<<Block as BlockT>::Hash>) -> RpcResult<RailsHubInfo> {
        let api = self.client.runtime_api();
        let at = at.unwrap_or_else(|| self.client.info().best_hash);
        api.rails_hub_info(at)
            .map_err(|e| Error::RuntimeError(format!("unable to read hub info: {e:?}")).into())
    }
}
