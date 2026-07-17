use crate::client::FullClient;
use crate::conditional_evm_block_import::ConditionalEVMBlockImport;
use crate::service::GrandpaBlockImport;
use fc_consensus::FrontierBlockImport;
use node_subtensor_runtime::opaque::Block;
use sc_client_api::AuxStore;
use sc_client_api::BlockOf;
use sc_client_api::UsageProvider;
use sc_consensus::BlockCheckParams;
use sc_consensus::BlockImport;
use sc_consensus::BlockImportParams;
use sc_consensus::BoxJustificationImport;
use sc_consensus::ImportResult;
use sc_consensus::Verifier;
use sc_consensus::{BasicQueue, DefaultImportQueue};
use sc_consensus_aura::AuraVerifier;
use sc_consensus_aura::CheckForEquivocation;
use sc_consensus_aura::CompatibilityMode;
use sc_consensus_babe::BabeLink;
use sc_consensus_babe::BabeVerifier;
use sc_consensus_babe::Epoch;
use sc_consensus_epochs::SharedEpochChanges;
use sc_consensus_slots::InherentDataProviderExt;
use sc_telemetry::TelemetryHandle;
use sp_api::ApiExt;
use sp_api::ProvideRuntimeApi;
use sp_block_builder::BlockBuilder as BlockBuilderApi;
use sp_blockchain::HeaderBackend;
use sp_blockchain::HeaderMetadata;
use sp_consensus::error::Error as ConsensusError;
use sp_consensus_aura::AuraApi;
use sp_consensus_aura::sr25519::AuthorityId as AuraAuthorityId;
use sp_consensus_aura::sr25519::AuthorityPair as AuraAuthorityPair;
use sp_consensus_babe::BABE_ENGINE_ID;
use sp_consensus_babe::BabeApi;
use sp_consensus_babe::BabeConfiguration;
use sp_inherents::CreateInherentDataProviders;
use sp_runtime::Digest;
use sp_runtime::DigestItem;
use sp_runtime::traits::NumberFor;
use sp_runtime::traits::{Block as BlockT, Header as HeaderT};
use std::sync::Arc;
use substrate_prometheus_endpoint::Registry;

/// `BlockImport` implementations that supports importing both Aura and Babe blocks.
#[derive(Clone)]
pub struct HybridBlockImport<BI>
where
    BI: BlockImport<Block, Error = ConsensusError> + Send + Sync,
{
    inner_aura: ConditionalEVMBlockImport<
        Block,
        GrandpaBlockImport,
        FrontierBlockImport<Block, GrandpaBlockImport, FullClient>,
    >,
    inner_babe: ConditionalEVMBlockImport<Block, BI, FrontierBlockImport<Block, BI, FullClient>>,
    babe_link: BabeLink<Block>,
    client: Arc<FullClient>,
}

impl<BI> HybridBlockImport<BI>
where
    BI: BlockImport<Block, Error = ConsensusError> + Clone + Send + Sync,
{
    pub fn new(
        client: Arc<FullClient>,
        grandpa_block_import: GrandpaBlockImport,
        babe_import: BI,
        babe_link: BabeLink<Block>,
        skip_history_backfill: bool,
    ) -> Self {
        let inner_aura = ConditionalEVMBlockImport::new(
            grandpa_block_import.clone(),
            FrontierBlockImport::new(grandpa_block_import.clone(), client.clone()),
            skip_history_backfill,
        );

        let inner_babe = ConditionalEVMBlockImport::new(
            babe_import.clone(),
            FrontierBlockImport::new(babe_import.clone(), client.clone()),
            skip_history_backfill,
        );

        HybridBlockImport {
            inner_aura,
            inner_babe,
            babe_link,
            client,
        }
    }

    pub fn babe_link(&self) -> &BabeLink<Block> {
        &self.babe_link
    }
}

#[async_trait::async_trait]
impl<BI> BlockImport<Block> for HybridBlockImport<BI>
where
    BI: BlockImport<Block, Error = ConsensusError> + Clone + Send + Sync,
{
    type Error = ConsensusError;

    async fn check_block(
        &self,
        block: BlockCheckParams<Block>,
    ) -> Result<ImportResult, Self::Error> {
        // The Babe and Aura `BlockImport` implementations both defer to the inner
        // client's `check_block` implementation defined here:
        // https://github.com/RaoFoundation/polkadot-sdk/blob/d13f915d8a1f55af53fd51fdb4544c47badddc7e/substrate/client/service/src/client/client.rs#L1748.
        self.client.check_block(block).await.map_err(Into::into)
    }

    async fn import_block(
        &self,
        block: BlockImportParams<Block>,
    ) -> Result<ImportResult, Self::Error> {
        if is_babe_digest(block.header.digest()) {
            self.inner_babe
                .import_block(block)
                .await
                .map_err(Into::into)
        } else {
            self.inner_aura
                .import_block(block)
                .await
                .map_err(Into::into)
        }
    }
}

/// `Verifier` implementation that supports verifying both Aura and Babe blocks.
struct HybridVerifier<B: BlockT, C, CIDP> {
    inner_aura: AuraVerifier<C, AuraAuthorityPair, CIDP, B>,
    inner_babe: BabeVerifier<B, C>,
}

impl<B: BlockT, C, CIDP> HybridVerifier<B, C, CIDP>
where
    CIDP: CreateInherentDataProviders<B, ()> + Send + Sync + Clone,
    CIDP::InherentDataProviders: InherentDataProviderExt + Send + Sync,
    C: ProvideRuntimeApi<B> + Send + Sync + sc_client_api::backend::AuxStore,
    C::Api: BlockBuilderApi<B> + BabeApi<B> + AuraApi<B, AuraAuthorityId> + ApiExt<B>,
    C: HeaderBackend<B> + HeaderMetadata<B, Error = sp_blockchain::Error>,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        client: Arc<C>,
        create_inherent_data_providers: CIDP,
        telemetry: Option<TelemetryHandle>,
        check_for_equivocation: CheckForEquivocation,
        compatibility_mode: sc_consensus_aura::CompatibilityMode<NumberFor<B>>,
        babe_config: BabeConfiguration,
        epoch_changes: SharedEpochChanges<B, Epoch>,
    ) -> Self {
        let aura_params = sc_consensus_aura::BuildVerifierParams::<C, CIDP, _> {
            client: client.clone(),
            create_inherent_data_providers: create_inherent_data_providers.clone(),
            telemetry: telemetry.clone(),
            check_for_equivocation,
            compatibility_mode,
        };
        let inner_aura =
            sc_consensus_aura::build_verifier::<AuraAuthorityPair, C, CIDP, B>(aura_params);

        let inner_babe = BabeVerifier::new(
            client,
            babe_config.slot_duration(),
            babe_config,
            epoch_changes,
            telemetry,
        );

        HybridVerifier {
            inner_aura,
            inner_babe,
        }
    }
}

#[async_trait::async_trait]
impl<B: BlockT, C, CIDP> Verifier<B> for HybridVerifier<B, C, CIDP>
where
    C: ProvideRuntimeApi<B> + Send + Sync + sc_client_api::backend::AuxStore,
    C::Api: BlockBuilderApi<B> + BabeApi<B> + AuraApi<B, AuraAuthorityId> + ApiExt<B>,
    C: HeaderBackend<B> + HeaderMetadata<B, Error = sp_blockchain::Error>,
    CIDP: CreateInherentDataProviders<B, ()> + Send + Sync + Clone,
    CIDP::InherentDataProviders: InherentDataProviderExt + Send + Sync,
{
    async fn verify(&self, block: BlockImportParams<B>) -> Result<BlockImportParams<B>, String> {
        let number: NumberFor<B> = *block.post_header().number();
        log::debug!("Verifying block: {number:?}");
        if is_babe_digest(block.header.digest()) {
            self.inner_babe.verify(block).await
        } else {
            self.inner_aura.verify(block).await
        }
    }
}

/// Parameters for our [`import_queue`].
pub struct HybridImportQueueParams<'a, Block: BlockT, I, C, S, CIDP> {
    /// The block import to use.
    pub block_import: I,
    /// The justification import.
    pub justification_import: Option<BoxJustificationImport<Block>>,
    /// The client to interact with the chain.
    pub client: Arc<C>,
    /// Something that can create the inherent data providers.
    pub create_inherent_data_providers: CIDP,
    /// The spawner to spawn background tasks.
    pub spawner: &'a S,
    /// The prometheus registry.
    pub registry: Option<&'a Registry>,
    /// Should we check for equivocation?
    pub check_for_equivocation: CheckForEquivocation,
    /// Telemetry instance used to report telemetry metrics.
    pub telemetry: Option<TelemetryHandle>,
    /// Compatibility mode that should be used.
    ///
    /// If in doubt, use `Default::default()`.
    pub compatibility_mode: CompatibilityMode<NumberFor<Block>>,
    /// The configuration for the BABE consensus algorithm.
    pub babe_config: BabeConfiguration,
    /// The epoch changes for the BABE consensus algorithm.
    pub epoch_changes: SharedEpochChanges<Block, Epoch>,
}

/// Start a hybrid import queue that supports importing both Aura and Babe blocks.
pub fn import_queue<B, I, C, S, CIDP>(
    params: HybridImportQueueParams<B, I, C, S, CIDP>,
) -> Result<DefaultImportQueue<B>, sp_consensus::Error>
where
    B: BlockT,
    C::Api: BlockBuilderApi<B> + BabeApi<B> + AuraApi<B, AuraAuthorityId> + ApiExt<B>,
    C: 'static
        + ProvideRuntimeApi<B>
        + BlockOf
        + Send
        + Sync
        + AuxStore
        + UsageProvider<B>
        + HeaderBackend<B>
        + HeaderMetadata<B, Error = sp_blockchain::Error>,
    I: BlockImport<B, Error = ConsensusError> + Send + Sync + 'static,
    S: sp_core::traits::SpawnEssentialNamed,
    CIDP: CreateInherentDataProviders<B, ()> + Sync + Send + Clone + 'static,
    CIDP::InherentDataProviders: InherentDataProviderExt + Send + Sync,
{
    let verifier = HybridVerifier::<B, C, CIDP>::new(
        params.client,
        params.create_inherent_data_providers,
        params.telemetry,
        params.check_for_equivocation,
        params.compatibility_mode,
        params.babe_config,
        params.epoch_changes,
    );

    Ok(BasicQueue::new(
        verifier,
        Box::new(params.block_import),
        params.justification_import,
        params.spawner,
        params.registry,
    ))
}

fn is_babe_digest(digest: &Digest) -> bool {
    digest
        .logs()
        .iter()
        .any(|d| matches!(d, DigestItem::PreRuntime(engine_id, _) if engine_id == &BABE_ENGINE_ID))
}
