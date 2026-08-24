use jsonrpsee::Methods;
use node_subtensor_runtime::opaque::Block;
use sc_client_api::AuxStore;
use sc_client_api::BlockOf;
use sc_consensus::BlockImport;
use sc_consensus_aura::AuraApi;
use sc_consensus_slots::BackoffAuthoringBlocksStrategy;
use sc_consensus_slots::InherentDataProviderExt;
use sc_consensus_slots::SlotProportion;
use sc_network_sync::SyncingService;
use sc_service::{TaskManager, error::Error as ServiceError};
use sc_telemetry::TelemetryHandle;
use sp_api::ProvideRuntimeApi;
use sp_blockchain::HeaderBackend;
use sp_blockchain::HeaderMetadata;
use sp_consensus::Proposer;
use sp_consensus::SyncOracle;
use sp_consensus::{Environment, SelectChain};
use sp_consensus_aura::sr25519::AuthorityId as AuraAuthorityId;
use sp_consensus_babe::BabeApi;
use sp_consensus_slots::SlotDuration;
use sp_inherents::CreateInherentDataProviders;
use sp_keystore::KeystorePtr;
use sp_runtime::traits::NumberFor;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use stp_shield::ShieldKeystorePtr;

use crate::client::FullClient;
use crate::service::BIQ;
use crate::service::FullSelectChain;

pub struct StartAuthoringParams<C, SC, I, PF, SO, L, CIDP, BS> {
    /// The duration of a slot.
    pub slot_duration: SlotDuration,
    /// The client to interact with the chain.
    pub client: Arc<C>,
    /// A select chain implementation to select the best block.
    pub select_chain: SC,
    /// The block import.
    pub block_import: I,
    /// The proposer factory to build proposer instances.
    pub proposer_factory: PF,
    /// The sync oracle that can give us the current sync status.
    pub sync_oracle: SO,
    /// Hook into the sync module to control the justification sync process.
    pub justification_sync_link: L,
    /// Something that can create the inherent data providers.
    pub create_inherent_data_providers: CIDP,
    /// Should we force the authoring of blocks?
    pub force_authoring: bool,
    /// The backoff strategy when we miss slots.
    pub backoff_authoring_blocks: Option<BS>,
    /// The keystore used by the node.
    pub keystore: KeystorePtr,
    /// The proportion of the slot dedicated to proposing.
    ///
    /// The block proposing will be limited to this proportion of the slot from the starting of the
    /// slot. However, the proposing can still take longer when there is some lenience factor
    /// applied, because there were no blocks produced for some slots.
    pub block_proposal_slot_portion: SlotProportion,
    /// The maximum proportion of the slot dedicated to proposing with any lenience factor applied
    /// due to no blocks being produced.
    pub max_block_proposal_slot_portion: Option<SlotProportion>,
    /// Telemetry instance used to report telemetry metrics.
    pub telemetry: Option<TelemetryHandle>,
}

/// All consensus mechanism specific node logic should be covered by this trait,
/// so files like service.rs and rpc.rs can be generic over consensus mechanisms.
pub trait ConsensusMechanism {
    /// IDPs inserted into the block by the ConsensusMechanism.
    type InherentDataProviders: sp_inherents::InherentDataProvider
        + sc_consensus_slots::InherentDataProviderExt
        + 'static;

    /// Creates a new instance of the ConsensusMechanism.
    fn new() -> Self;

    /// Builds a `BIQ` that uses the ConsensusMechanism.
    fn build_biq(&mut self, skip_history_backfill: bool) -> Result<BIQ<'_>, sc_service::Error>;

    /// Returns the slot duration.
    fn slot_duration(&self, client: &FullClient) -> Result<SlotDuration, ServiceError>;

    /// Creates IDPs for the consensus mechanism.
    fn create_inherent_data_providers(
        slot_duration: SlotDuration,
        shield_keystore: ShieldKeystorePtr,
    ) -> Result<Self::InherentDataProviders, Box<dyn std::error::Error + Send + Sync>>;

    /// Creates IDPs for the consensus mechanism for pending blocks.
    fn pending_create_inherent_data_providers(
        slot_duration: SlotDuration,
        shield_keystore: ShieldKeystorePtr,
    ) -> Result<Self::InherentDataProviders, Box<dyn std::error::Error + Send + Sync>>;

    /// Creates the frontier consensus data provider with this mechanism.
    fn frontier_consensus_data_provider(
        client: Arc<FullClient>,
    ) -> Result<Box<dyn fc_rpc::pending::ConsensusDataProvider<Block>>, sp_blockchain::Error>;

    /// Starts authoring process for the consensus mechanism.
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
        C::Api: AuraApi<Block, AuraAuthorityId> + BabeApi<Block>,
        SC: SelectChain<Block> + 'static,
        I: BlockImport<Block, Error = sp_consensus::Error> + Send + Sync + 'static,
        PF: Environment<Block, Error = Error> + Send + Sync + 'static,
        PF::Proposer: Proposer<Block, Error = Error>,
        SO: SyncOracle + Send + Sync + Clone + 'static,
        L: sc_consensus::JustificationSyncLink<Block> + 'static,
        CIDP: CreateInherentDataProviders<Block, ()> + Send + Sync + 'static,
        CIDP::InherentDataProviders: InherentDataProviderExt + Send,
        BS: BackoffAuthoringBlocksStrategy<NumberFor<Block>> + Send + Sync + 'static,
        Error: std::error::Error + Send + From<sp_consensus::Error> + From<I::Error> + 'static;

    /// Spawns any consensus mechanism specific essential handles.
    fn spawn_essential_handles(
        &self,
        task_manager: &mut TaskManager,
        client: Arc<FullClient>,
        custom_service_signal: Option<Arc<AtomicBool>>,
        sync_service: Arc<SyncingService<Block>>,
    ) -> Result<(), ServiceError>;

    /// Returns any consensus mechanism specific rpc methods to register.
    fn rpc_methods(
        &self,
        client: Arc<FullClient>,
        keystore: KeystorePtr,
        select_chain: FullSelectChain,
    ) -> Result<Vec<Methods>, sc_service::Error>;
}
