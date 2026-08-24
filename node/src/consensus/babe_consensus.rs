use crate::consensus::ConsensusMechanism;
use crate::consensus::StartAuthoringParams;
use crate::{
    client::{FullBackend, FullClient},
    conditional_evm_block_import::ConditionalEVMBlockImport,
    ethereum::EthConfiguration,
    service::{BIQ, FullSelectChain, GrandpaBlockImport},
};
use fc_consensus::FrontierBlockImport;
use jsonrpsee::Methods;
use node_subtensor_runtime::opaque::Block;
use num_traits::Zero as _;
use sc_client_api::{AuxStore, BlockOf};
use sc_consensus::{BlockImport, BoxBlockImport};
use sc_consensus_babe::{BabeLink, BabeWorkerHandle};
use sc_consensus_babe_rpc::{Babe, BabeApiServer};
use sc_consensus_grandpa::BlockNumberOps;
use sc_consensus_slots::{BackoffAuthoringBlocksStrategy, InherentDataProviderExt};
use sc_network_sync::SyncingService;
use sc_service::{Configuration, TaskManager};
use sc_telemetry::TelemetryHandle;
use sc_transaction_pool::TransactionPoolHandle;
use sc_transaction_pool_api::OffchainTransactionPoolFactory;
use sp_api::ProvideRuntimeApi;
use sp_blockchain::{HeaderBackend, HeaderMetadata};
use sp_consensus::{Environment, Proposer, SelectChain, SyncOracle};
use sp_consensus_aura::AuraApi;
use sp_consensus_aura::sr25519::AuthorityId;
use sp_consensus_babe::BabeApi;
use sp_consensus_slots::SlotDuration;
use sp_inherents::CreateInherentDataProviders;
use sp_keystore::KeystorePtr;
use sp_runtime::traits::NumberFor;
use stc_shield::InherentDataProvider as ShieldInherentDataProvider;
use std::{error::Error, sync::Arc};
use stp_shield::ShieldKeystorePtr;

pub struct BabeConsensus {
    babe_link: Option<BabeLink<Block>>,
    babe_worker_handle: Option<BabeWorkerHandle<Block>>,
}

impl ConsensusMechanism for BabeConsensus {
    type InherentDataProviders = (
        sp_consensus_babe::inherents::InherentDataProvider,
        sp_timestamp::InherentDataProvider,
        stc_shield::InherentDataProvider,
    );

    #[allow(clippy::expect_used)]
    fn start_authoring<C, SC, I, PF, SO, L, CIDP, BS, Error>(
        self,
        task_manager: &mut TaskManager,
        params: StartAuthoringParams<C, SC, I, PF, SO, L, CIDP, BS>,
    ) -> Result<(), sp_consensus::Error>
    where
        C: ProvideRuntimeApi<Block>
            + BlockOf
            + AuxStore
            + HeaderBackend<Block>
            + HeaderMetadata<Block, Error = sp_blockchain::Error>
            + Send
            + Sync
            + 'static,
        C::Api: AuraApi<Block, AuthorityId> + BabeApi<Block>,
        SC: SelectChain<Block> + 'static,
        I: BlockImport<Block, Error = sp_consensus::Error> + Send + Sync + 'static,
        PF: Environment<Block, Error = Error> + Send + Sync + 'static,
        PF::Proposer: Proposer<Block, Error = Error>,
        SO: SyncOracle + Send + Sync + Clone + 'static,
        L: sc_consensus::JustificationSyncLink<Block> + 'static,
        CIDP: CreateInherentDataProviders<Block, ()> + Send + Sync + 'static,
        CIDP::InherentDataProviders: InherentDataProviderExt + Send,
        BS: BackoffAuthoringBlocksStrategy<NumberFor<Block>> + Send + Sync + 'static,
        Error: std::error::Error + Send + From<sp_consensus::Error> + From<I::Error> + 'static,
    {
        let babe = sc_consensus_babe::start_babe::<Block, C, SC, PF, I, SO, CIDP, BS, L, Error>(
            sc_consensus_babe::BabeParams {
                keystore: params.keystore,
                client: params.client,
                select_chain: params.select_chain,
                env: params.proposer_factory,
                block_import: params.block_import,
                sync_oracle: params.sync_oracle,
                justification_sync_link: params.justification_sync_link,
                create_inherent_data_providers: params.create_inherent_data_providers,
                force_authoring: params.force_authoring,
                backoff_authoring_blocks: params.backoff_authoring_blocks,
                babe_link: self
                    .babe_link
                    .expect("Must build the import queue before starting authoring."),
                block_proposal_slot_portion: params.block_proposal_slot_portion,
                max_block_proposal_slot_portion: params.max_block_proposal_slot_portion,
                telemetry: params.telemetry,
            },
        )?;

        // the BABE authoring task is considered essential, i.e. if it
        // fails we take down the service with it.
        task_manager.spawn_essential_handle().spawn_blocking(
            "babe-proposer",
            Some("block-authoring"),
            babe,
        );

        Ok(())
    }

    fn frontier_consensus_data_provider(
        client: Arc<FullClient>,
    ) -> Result<Box<dyn fc_rpc::pending::ConsensusDataProvider<Block>>, sp_blockchain::Error> {
        Ok(Box::new(fc_babe::BabeConsensusDataProvider::new(client)?))
    }

    fn create_inherent_data_providers(
        slot_duration: SlotDuration,
        shield_keystore: ShieldKeystorePtr,
    ) -> Result<Self::InherentDataProviders, Box<dyn Error + Send + Sync>> {
        let timestamp = sp_timestamp::InherentDataProvider::from_system_time();
        let slot =
            sp_consensus_babe::inherents::InherentDataProvider::from_timestamp_and_slot_duration(
                *timestamp,
                slot_duration,
            );
        let shield = stc_shield::InherentDataProvider::new(shield_keystore);
        Ok((slot, timestamp, shield))
    }

    fn pending_create_inherent_data_providers(
        slot_duration: SlotDuration,
        shield_keystore: ShieldKeystorePtr,
    ) -> Result<Self::InherentDataProviders, Box<dyn Error + Send + Sync>> {
        let current = sp_timestamp::InherentDataProvider::from_system_time();
        let next_slot = current
            .timestamp()
            .as_millis()
            .saturating_add(slot_duration.as_millis());
        let timestamp = sp_timestamp::InherentDataProvider::new(next_slot.into());
        let slot =
            sp_consensus_babe::inherents::InherentDataProvider::from_timestamp_and_slot_duration(
                *timestamp,
                slot_duration,
            );
        let shield = ShieldInherentDataProvider::new(shield_keystore);
        Ok((slot, timestamp, shield))
    }

    fn new() -> Self {
        Self {
            babe_link: None,
            babe_worker_handle: None,
        }
    }

    fn build_biq(&mut self, skip_history_backfill: bool) -> Result<BIQ<'_>, sc_service::Error>
    where
        NumberFor<Block>: BlockNumberOps,
    {
        let build_import_queue = Box::new(
            move |client: Arc<FullClient>,
                  backend: Arc<FullBackend>,
                  config: &Configuration,
                  _eth_config: &EthConfiguration,
                  task_manager: &TaskManager,
                  telemetry: Option<TelemetryHandle>,
                  grandpa_block_import: GrandpaBlockImport,
                  transaction_pool: Arc<TransactionPoolHandle<Block, FullClient>>| {
                let configuration = sc_consensus_babe::configuration(&*client)?;
                // When Babe slot duration is zero, it means we are running an Aura runtime with a
                // placeholder BabeApi, therefore the BabeApi is invalid.
                //
                // In this case, we return the same error if there was no BabeApi at all,
                // signalling to the node that it needs an Aura service.
                if configuration.slot_duration.is_zero() {
                    return Err(sc_service::Error::Client(
                        sp_blockchain::Error::VersionInvalid(
                            "Unsupported or invalid BabeApi version".to_string(),
                        ),
                    ));
                }

                let (babe_import, babe_link) = sc_consensus_babe::block_import(
                    configuration,
                    grandpa_block_import.clone(),
                    client.clone(),
                )?;

                let conditional_block_import = ConditionalEVMBlockImport::new(
                    babe_import.clone(),
                    FrontierBlockImport::new(babe_import.clone(), client.clone()),
                    skip_history_backfill,
                );

                let slot_duration = babe_link.config().slot_duration();
                let create_inherent_data_providers = move |_, ()| async move {
                    let timestamp = sp_timestamp::InherentDataProvider::from_system_time();
                    let slot =
						sp_consensus_babe::inherents::InherentDataProvider::from_timestamp_and_slot_duration(
							*timestamp,
							slot_duration,
						);
                    Ok((slot, timestamp))
                };

                let (import_queue, babe_worker_handle) =
                    sc_consensus_babe::import_queue(sc_consensus_babe::ImportQueueParams {
                        link: babe_link.clone(),
                        block_import: conditional_block_import.clone(),
                        justification_import: Some(Box::new(grandpa_block_import)),
                        client,
                        select_chain: sc_consensus::LongestChain::new(backend.clone()),
                        create_inherent_data_providers,
                        spawner: &task_manager.spawn_essential_handle(),
                        registry: config.prometheus_registry(),
                        telemetry,
                        offchain_tx_pool_factory: OffchainTransactionPoolFactory::new(
                            transaction_pool,
                        ),
                    })?;

                self.babe_link = Some(babe_link);
                self.babe_worker_handle = Some(babe_worker_handle);
                Ok((
                    import_queue,
                    Box::new(conditional_block_import) as BoxBlockImport<Block>,
                ))
            },
        );

        Ok(build_import_queue)
    }

    fn slot_duration(&self, _client: &FullClient) -> Result<SlotDuration, sc_service::Error> {
        if let Some(ref babe_link) = self.babe_link {
            Ok(babe_link.config().slot_duration())
        } else {
            Err(sc_service::Error::Other(
				"Babe link not initialized. Ensure that the import queue has been built before calling slot_duration.".to_string()
			))
        }
    }

    fn spawn_essential_handles(
        &self,
        _task_manager: &mut TaskManager,
        _client: Arc<FullClient>,
        _custom_service_signal: Option<Arc<std::sync::atomic::AtomicBool>>,
        _sync_service: Arc<SyncingService<Block>>,
    ) -> Result<(), sc_service::Error> {
        // No additional Babe handles required.
        Ok(())
    }

    fn rpc_methods(
        &self,
        client: Arc<FullClient>,
        keystore: KeystorePtr,
        select_chain: FullSelectChain,
    ) -> Result<Vec<Methods>, sc_service::Error> {
        if let Some(ref babe_worker_handle) = self.babe_worker_handle {
            Ok(vec![
                Babe::new(
                    client.clone(),
                    babe_worker_handle.clone(),
                    keystore,
                    select_chain,
                )
                .into_rpc()
                .into(),
            ])
        } else {
            Err(sc_service::Error::Other(
				"Babe link not initialized. Ensure that the import queue has been built before calling slot_duration.".to_string()
			))
        }
    }
}
