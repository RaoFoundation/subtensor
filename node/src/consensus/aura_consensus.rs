use crate::consensus::hybrid_import_queue::HybridBlockImport;
use crate::consensus::{ConsensusMechanism, StartAuthoringParams};
use crate::{
    client::{FullBackend, FullClient},
    ethereum::EthConfiguration,
    service::{BIQ, FullSelectChain, GrandpaBlockImport},
};
use jsonrpsee::tokio;
use node_subtensor_runtime::opaque::Block;
use sc_client_api::{AuxStore, BlockOf, UsageProvider};
use sc_consensus::{BlockImport, BoxBlockImport};
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
use sp_consensus_aura::sr25519::AuthorityId as AuraAuthorityId;
use sp_consensus_aura::{AuraApi, sr25519::AuthorityPair as AuraPair};
use sp_consensus_babe::AuthorityId as BabeAuthorityId;
use sp_consensus_babe::BabeConfiguration;
use sp_consensus_slots::SlotDuration;
use sp_inherents::CreateInherentDataProviders;
use sp_keystore::KeystorePtr;
use sp_runtime::traits::Block as BlockT;
use sp_runtime::traits::NumberFor;
use stc_shield::InherentDataProvider as ShieldInherentDataProvider;
use std::{error::Error, sync::Arc};
use stp_shield::ShieldKeystorePtr;

pub struct AuraConsensus;

impl ConsensusMechanism for AuraConsensus {
    type InherentDataProviders = (
        sp_consensus_aura::inherents::InherentDataProvider,
        sp_timestamp::InherentDataProvider,
        stc_shield::InherentDataProvider,
    );

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
        C::Api: AuraApi<Block, AuraAuthorityId>,
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
        let aura = sc_consensus_aura::start_aura::<AuraPair, Block, _, _, _, _, _, _, _, _, _>(
            sc_consensus_aura::StartAuraParams {
                slot_duration: params.slot_duration,
                client: params.client,
                select_chain: params.select_chain,
                block_import: params.block_import,
                proposer_factory: params.proposer_factory,
                sync_oracle: params.sync_oracle,
                justification_sync_link: params.justification_sync_link,
                create_inherent_data_providers: params.create_inherent_data_providers,
                force_authoring: params.force_authoring,
                backoff_authoring_blocks: params.backoff_authoring_blocks,
                keystore: params.keystore,
                block_proposal_slot_portion: params.block_proposal_slot_portion,
                max_block_proposal_slot_portion: params.max_block_proposal_slot_portion,
                telemetry: params.telemetry,
                compatibility_mode: Default::default(),
            },
        )?;

        // the AURA authoring task is considered essential, i.e. if it
        // fails we take down the service with it.
        task_manager
            .spawn_essential_handle()
            .spawn_blocking("aura", Some("block-authoring"), aura);

        Ok(())
    }

    fn frontier_consensus_data_provider(
        client: Arc<FullClient>,
    ) -> Result<Box<dyn fc_rpc::pending::ConsensusDataProvider<Block>>, sp_blockchain::Error> {
        Ok(Box::new(fc_aura::AuraConsensusDataProvider::new(client)))
    }

    fn create_inherent_data_providers(
        slot_duration: SlotDuration,
        shield_keystore: ShieldKeystorePtr,
    ) -> Result<Self::InherentDataProviders, Box<dyn Error + Send + Sync>> {
        let timestamp = sp_timestamp::InherentDataProvider::from_system_time();
        let slot =
            sp_consensus_aura::inherents::InherentDataProvider::from_timestamp_and_slot_duration(
                *timestamp,
                slot_duration,
            );
        let shield = ShieldInherentDataProvider::new(shield_keystore);
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
            sp_consensus_aura::inherents::InherentDataProvider::from_timestamp_and_slot_duration(
                *timestamp,
                slot_duration,
            );
        let shield = stc_shield::InherentDataProvider::new(shield_keystore);
        Ok((slot, timestamp, shield))
    }

    fn new() -> Self {
        Self {}
    }

    fn build_biq(&mut self, skip_history_backfill: bool) -> Result<BIQ<'_>, sc_service::Error>
    where
        NumberFor<Block>: BlockNumberOps,
    {
        let build_import_queue = Box::new(
            move |client: Arc<FullClient>,
                  backend: Arc<FullBackend>,
                  service_config: &Configuration,
                  _eth_config: &EthConfiguration,
                  task_manager: &TaskManager,
                  telemetry: Option<TelemetryHandle>,
                  grandpa_block_import: GrandpaBlockImport,
                  transaction_pool: Arc<TransactionPoolHandle<Block, FullClient>>| {
                let expected_babe_config = get_expected_babe_configuration(&*client)?;
                let conditional_block_import = HybridBlockImport::new(
                    client.clone(),
                    grandpa_block_import.clone(),
                    expected_babe_config.clone(),
                    skip_history_backfill,
                );

                let slot_duration = sc_consensus_aura::slot_duration(&*client)?;
                let create_inherent_data_providers = move |_, ()| async move {
                    let timestamp = sp_timestamp::InherentDataProvider::from_system_time();
                    let slot =
						sp_consensus_aura::inherents::InherentDataProvider::from_timestamp_and_slot_duration(
							*timestamp,
							slot_duration,
						);
                    Ok((slot, timestamp))
                };

                // Aura needs the hybrid import queue, because it needs to
                // 1. Validate the first Babe block it encounters before switching into Babe
                //    consensus mode
                // 2. Import the entire blockchain without restarting during warp sync, because
                //    warp sync does not allow restarting sync midway.
                let import_queue = super::hybrid_import_queue::import_queue(
                    crate::consensus::hybrid_import_queue::HybridImportQueueParams {
                        block_import: conditional_block_import.clone(),
                        justification_import: Some(Box::new(grandpa_block_import.clone())),
                        client,
                        create_inherent_data_providers,
                        spawner: &task_manager.spawn_essential_handle(),
                        registry: service_config.prometheus_registry(),
                        check_for_equivocation: Default::default(),
                        telemetry,
                        compatibility_mode: sc_consensus_aura::CompatibilityMode::None,
                        select_chain: sc_consensus::LongestChain::new(backend.clone()),
                        babe_config: expected_babe_config,
                        epoch_changes: conditional_block_import.babe_link().epoch_changes().clone(),
                        offchain_tx_pool_factory: OffchainTransactionPoolFactory::new(
                            transaction_pool,
                        ),
                    },
                )
                .map_err::<sc_service::Error, _>(Into::into)?;

                Ok((
                    import_queue,
                    Box::new(conditional_block_import) as BoxBlockImport<Block>,
                ))
            },
        );

        Ok(build_import_queue)
    }

    fn slot_duration(&self, client: &FullClient) -> Result<SlotDuration, sc_service::Error> {
        sc_consensus_aura::slot_duration(client).map_err(Into::into)
    }

    fn spawn_essential_handles(
        &self,
        task_manager: &mut TaskManager,
        client: Arc<FullClient>,
        custom_service_signal: Option<Arc<std::sync::atomic::AtomicBool>>,
        sync_service: Arc<SyncingService<Block>>,
    ) -> Result<(), sc_service::Error> {
        let client_clone = client.clone();
        let custom_service_signal_clone = custom_service_signal.clone();
        let slot_duration = self.slot_duration(&client)?;
        task_manager.spawn_essential_handle().spawn(
            "babe-switch",
            None,
            Box::pin(async move {
                let client = client_clone;
                let custom_service_signal = custom_service_signal_clone;
                loop {
                    // Check if the runtime is Babe once per block.
                    if let Ok(c) = sc_consensus_babe::configuration(&*client) {
                        // Aura Consensus uses the hybrid import queue which is able to import both
                        // Aura and Babe blocks. Wait until sync finishes before switching to the
                        // Babe service to not break warp sync.
						//
						// Note that although unintuitive, it is required that we wait until BOTH
						// warp sync and state sync are finished before we can safely switch to the
						// Babe service. If we only wait for the "warp sync" to finish while state
						// sync is still in progress prior to switching, the warp sync will not
						// complete successfully.
                        let syncing = sync_service.status().await.is_ok_and(|status| status.warp_sync.is_some() || status.state_sync.is_some());
                        if !c.authorities.is_empty() && !syncing {
                            log::info!("Babe runtime detected! Intentionally failing the essential handle `babe-switch` to trigger switch to Babe service.");
							// Signal that the node stopped due to the custom service exiting.
                            if let Some(custom_service_signal) = custom_service_signal {
                                custom_service_signal.store(true, std::sync::atomic::Ordering::SeqCst);
                            };
                            break;
                        }
                    };
                    tokio::time::sleep(slot_duration.as_duration()).await;
                }
            }),
        );
        Ok(())
    }

    fn rpc_methods(
        &self,
        _client: Arc<FullClient>,
        _keystore: KeystorePtr,
        _select_chain: FullSelectChain,
    ) -> Result<Vec<jsonrpsee::Methods>, sc_service::Error> {
        // Aura requires no special RPC methods.
        Ok(Default::default())
    }
}

/// Returns what the Babe configuration is expected to be at the first Babe block.
///
/// This is required for the hybrid import queue, so it is ready to validate the first encountered
/// babe block(s) before switching to Babe consensus.
fn get_expected_babe_configuration<B: BlockT, C>(
    client: &C,
) -> sp_blockchain::Result<BabeConfiguration>
where
    C: AuxStore + ProvideRuntimeApi<B> + UsageProvider<B>,
    C::Api: AuraApi<B, AuraAuthorityId>,
{
    let at_hash = if client.usage_info().chain.finalized_state.is_some() {
        client.usage_info().chain.best_hash
    } else {
        client.usage_info().chain.genesis_hash
    };

    let runtime_api = client.runtime_api();
    let authorities = runtime_api
        .authorities(at_hash)?
        .into_iter()
        .map(|a| (BabeAuthorityId::from(a.into_inner()), 1))
        .collect();

    let slot_duration = runtime_api.slot_duration(at_hash)?.as_millis();
    let epoch_config = node_subtensor_runtime::BABE_GENESIS_EPOCH_CONFIG;
    let config = sp_consensus_babe::BabeConfiguration {
        slot_duration,
        epoch_length: node_subtensor_runtime::EPOCH_DURATION_IN_SLOTS,
        c: epoch_config.c,
        authorities,
        randomness: Default::default(),
        allowed_slots: epoch_config.allowed_slots,
    };

    Ok(config)
}
