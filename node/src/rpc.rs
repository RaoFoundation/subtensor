//! Assemble Subtensor full-node JSON-RPC: system, payment, custom, swap, manual-seal, Eth.
//!
//! Substrate `sc-rpc` supplies the core layer; this module merges runtime-specific
//! APIs and Frontier eth deps. Client-facing RPC method name strings must stay stable.

#![warn(missing_docs)]

use std::sync::Arc;

use futures::channel::mpsc;

use crate::{
    client::{FullBackend, FullClient},
    ethereum::create_eth,
};
use fc_rpc::EthBlockDataCacheTask;
pub use fc_rpc_core::types::{FeeHistoryCache, FeeHistoryCacheLimit, FilterPool};
/// Frontier DB backend type.
pub use fc_storage::StorageOverride;
use jsonrpsee::{Methods, RpcModule};
use node_subtensor_runtime::opaque::Block;
use sc_consensus_manual_seal::EngineCommand;
use sc_network::service::traits::NetworkService;
use sc_network_sync::SyncingService;
use sc_rpc::SubscriptionTaskExecutor;
use sc_transaction_pool_api::TransactionPool;
use sp_core::H256;
use sp_inherents::CreateInherentDataProviders;
use sp_runtime::{OpaqueExtrinsic, traits::BlakeTwo256, traits::Block as BlockT};
use std::collections::BTreeMap;
use subtensor_runtime_common::Hash;

/// Extra dependencies for Ethereum compatibility.
pub struct EthDeps<P, CT, CIDP> {
    /// The client instance to use.
    pub client: Arc<FullClient>,
    /// Transaction pool instance.
    pub pool: Arc<P>,
    /// Graph pool instance.
    pub graph: Arc<P>,
    /// Ethereum transaction converter.
    pub converter: Option<CT>,
    /// The Node authority flag
    pub is_authority: bool,
    /// Whether to enable dev signer
    pub enable_dev_signer: bool,
    /// Network service
    pub network: Arc<dyn NetworkService>,
    /// Chain syncing service
    pub sync: Arc<SyncingService<Block>>,
    /// Frontier Backend.
    pub frontier_backend: Arc<dyn fc_api::Backend<Block>>,
    /// Ethereum data access overrides.
    pub storage_override: Arc<dyn StorageOverride<Block>>,
    /// Cache for Ethereum block data.
    pub block_data_cache: Arc<EthBlockDataCacheTask<Block>>,
    /// EthFilterApi pool.
    pub filter_pool: Option<FilterPool>,
    /// Maximum number of logs in a query.
    pub max_past_logs: u32,
    /// Fee history cache.
    pub fee_history_cache: FeeHistoryCache,
    /// Maximum fee history cache size.
    pub fee_history_cache_limit: FeeHistoryCacheLimit,
    /// Maximum allowed gas limit will be ` block.gas_limit * execute_gas_limit_multiplier` when
    /// using eth_call/eth_estimateGas.
    pub execute_gas_limit_multiplier: u64,
    /// Mandated parent hashes for a given block hash.
    pub forced_parent_hashes: Option<BTreeMap<H256, H256>>,
    /// Something that can create the inherent data providers for pending state
    pub pending_create_inherent_data_providers: CIDP,
}

/// Default Eth RPC configuration
pub struct DefaultEthConfig;

impl fc_rpc::EthConfig<Block, FullClient> for DefaultEthConfig {
    type EstimateGasAdapter = ();
    type RuntimeStorageOverride = fc_rpc::frontier_backend_client::SystemAccountId20StorageOverride<
        Block,
        FullClient,
        FullBackend,
    >;
}

/// Full client dependencies.
pub struct FullDeps<P, CT, CIDP> {
    /// The client instance to use.
    pub client: Arc<FullClient>,
    /// Transaction pool instance.
    pub pool: Arc<P>,
    /// Manual seal command sink
    pub command_sink: Option<mpsc::Sender<EngineCommand<Hash>>>,
    /// Ethereum-compatibility specific dependencies.
    pub eth: EthDeps<P, CT, CIDP>,
}

/// Instantiate all full RPC extensions (Subtensor custom + Frontier eth + optional manual seal).
///
/// Does not alter registered RPC method name strings.
pub fn create_full<P, CT, CIDP>(
    deps: FullDeps<P, CT, CIDP>,
    subscription_task_executor: SubscriptionTaskExecutor,
    pubsub_notification_sinks: Arc<
        fc_mapping_sync::EthereumBlockNotificationSinks<
            fc_mapping_sync::EthereumBlockNotification<Block>,
        >,
    >,
    frontier_pending_consensus_data_provider: Box<
        dyn fc_rpc::pending::ConsensusDataProvider<Block>,
    >,
    other_methods: &[Methods],
) -> Result<RpcModule<()>, Box<dyn std::error::Error + Send + Sync>>
where
    P: TransactionPool<
            Block = Block,
            Hash = <sp_runtime::generic::Block<
                sp_runtime::generic::Header<u32, BlakeTwo256>,
                OpaqueExtrinsic,
            > as BlockT>::Hash,
        > + 'static,
    CIDP: CreateInherentDataProviders<Block, ()> + Send + Clone + 'static,
    CT: fp_rpc::ConvertTransaction<<Block as BlockT>::Extrinsic> + Send + Sync + Clone + 'static,
{
    use pallet_subtensor_swap_rpc::{Swap, SwapRpcApiServer};
    use pallet_transaction_payment_rpc::{TransactionPayment, TransactionPaymentApiServer};
    use sc_consensus_manual_seal::rpc::{ManualSeal, ManualSealApiServer};
    use substrate_frame_rpc_system::{System, SystemApiServer};
    use subtensor_custom_rpc::{SubtensorCustom, SubtensorCustomApiServer};

    let mut module = RpcModule::new(());
    let FullDeps {
        client,
        pool,
        command_sink,
        eth,
    } = deps;

    // Custom RPC methods for Paratensor
    module.merge(SubtensorCustom::new(client.clone()).into_rpc())?;

    // Swap RPC
    module.merge(Swap::new(client.clone()).into_rpc())?;

    module.merge(System::new(client.clone(), pool.clone()).into_rpc())?;
    module.merge(TransactionPayment::new(client.clone()).into_rpc())?;

    // Extend this RPC with a custom API by using the following syntax.
    // `YourRpcStruct` should have a reference to a client, which is needed
    // to call into the runtime.
    // `module.merge(YourRpcTrait::into_rpc(YourRpcStruct::new(ReferenceToClient, ...)))?;`

    if let Some(command_sink) = command_sink {
        module.merge(
            // We provide the rpc handler with the sending end of the channel to allow the rpc
            // send EngineCommands to the background block authorship task.
            ManualSeal::new(command_sink).into_rpc(),
        )?;
    }

    // Other methods provided by the caller
    for m in other_methods {
        module.merge(m.clone())?;
    }

    // Ethereum compatibility RPCs
    let module = create_eth::<_, _, _, DefaultEthConfig>(
        module,
        eth,
        subscription_task_executor,
        pubsub_notification_sinks,
        Some(frontier_pending_consensus_data_provider),
    )?;

    Ok(module)
}
