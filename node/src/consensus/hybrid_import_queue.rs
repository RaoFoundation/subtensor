//! Import queue that verifies both Aura and Babe digests during consensus migration.
//!
//! Digest inspection (`is_babe_digest`) chooses Babe vs Aura verification so the
//! node can sync across the Aura→Babe boundary without restarting mid-import.

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
use sc_consensus_babe::BabeBlockImport;
use sc_consensus_babe::BabeLink;
use sc_consensus_babe::BabeVerifier;
use sc_consensus_babe::Epoch;
use sc_consensus_epochs::SharedEpochChanges;
use sc_consensus_slots::InherentDataProviderExt;
use sc_telemetry::TelemetryHandle;
use sc_transaction_pool_api::OffchainTransactionPoolFactory;
use sp_api::ApiExt;
use sp_api::ProvideRuntimeApi;
use sp_block_builder::BlockBuilder as BlockBuilderApi;
use sp_blockchain::HeaderBackend;
use sp_blockchain::HeaderMetadata;
use sp_consensus::SelectChain;
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
pub struct HybridBlockImport {
    inner_aura: ConditionalEVMBlockImport<
        Block,
        GrandpaBlockImport,
        FrontierBlockImport<Block, GrandpaBlockImport, FullClient>,
    >,
    inner_babe: ConditionalEVMBlockImport<
        Block,
        BabeBlockImport<Block, FullClient, GrandpaBlockImport>,
        FrontierBlockImport<
            Block,
            BabeBlockImport<Block, FullClient, GrandpaBlockImport>,
            FullClient,
        >,
    >,
    babe_link: BabeLink<Block>,
    client: Arc<FullClient>,
}

impl HybridBlockImport {
    pub fn new(
        client: Arc<FullClient>,
        grandpa_block_import: GrandpaBlockImport,
        babe_config: BabeConfiguration,
        skip_history_backfill: bool,
    ) -> Self {
        let inner_aura = ConditionalEVMBlockImport::new(
            grandpa_block_import.clone(),
            FrontierBlockImport::new(grandpa_block_import.clone(), client.clone()),
            skip_history_backfill,
        );

        #[allow(clippy::expect_used)]
        let (babe_import, babe_link) = sc_consensus_babe::block_import(
            babe_config,
            grandpa_block_import.clone(),
            client.clone(),
        )
        .expect("Failed to create Babe block_import");

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
impl BlockImport<Block> for HybridBlockImport {
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
struct HybridVerifier<B: BlockT, C, CIDP, N, SC> {
    inner_aura: AuraVerifier<C, AuraAuthorityPair, CIDP, N>,
    inner_babe: BabeVerifier<B, C, SC, CIDP>,
}

impl<B: BlockT, C, CIDP, N, SC> HybridVerifier<B, C, CIDP, N, SC>
where
    CIDP: CreateInherentDataProviders<B, ()> + Send + Sync + Clone,
    CIDP::InherentDataProviders: InherentDataProviderExt + Send + Sync,
    C: ProvideRuntimeApi<B> + Send + Sync + sc_client_api::backend::AuxStore,
    C::Api: BlockBuilderApi<B> + BabeApi<B> + AuraApi<B, AuraAuthorityId> + ApiExt<B>,
    C: HeaderBackend<B> + HeaderMetadata<B, Error = sp_blockchain::Error>,
    SC: SelectChain<B> + 'static,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        client: Arc<C>,
        create_inherent_data_providers: CIDP,
        telemetry: Option<TelemetryHandle>,
        check_for_equivocation: CheckForEquivocation,
        compatibility_mode: sc_consensus_aura::CompatibilityMode<N>,
        select_chain: SC,
        babe_config: BabeConfiguration,
        epoch_changes: SharedEpochChanges<B, Epoch>,
        offchain_tx_pool_factory: OffchainTransactionPoolFactory<B>,
    ) -> Self {
        let aura_params = sc_consensus_aura::BuildVerifierParams::<C, CIDP, _> {
            client: client.clone(),
            create_inherent_data_providers: create_inherent_data_providers.clone(),
            telemetry: telemetry.clone(),
            check_for_equivocation,
            compatibility_mode,
        };
        let inner_aura =
            sc_consensus_aura::build_verifier::<AuraAuthorityPair, C, CIDP, N>(aura_params);

        let inner_babe = BabeVerifier::new(
            client.clone(),
            select_chain,
            create_inherent_data_providers,
            babe_config,
            epoch_changes,
            telemetry,
            offchain_tx_pool_factory,
        );

        HybridVerifier {
            inner_aura,
            inner_babe,
        }
    }
}

#[async_trait::async_trait]
impl<B: BlockT, C, CIDP, SC> Verifier<B> for HybridVerifier<B, C, CIDP, NumberFor<B>, SC>
where
    C: ProvideRuntimeApi<B> + Send + Sync + sc_client_api::backend::AuxStore,
    C::Api: BlockBuilderApi<B> + BabeApi<B> + AuraApi<B, AuraAuthorityId> + ApiExt<B>,
    C: HeaderBackend<B> + HeaderMetadata<B, Error = sp_blockchain::Error>,
    CIDP: CreateInherentDataProviders<B, ()> + Send + Sync + Clone,
    CIDP::InherentDataProviders: InherentDataProviderExt + Send + Sync,
    SC: SelectChain<B> + 'static,
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
pub struct HybridImportQueueParams<'a, Block: BlockT, I, C, S, CIDP, SC> {
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
    /// SelectChain strategy to use.
    pub select_chain: SC,
    /// The configuration for the BABE consensus algorithm.
    pub babe_config: BabeConfiguration,
    /// The epoch changes for the BABE consensus algorithm.
    pub epoch_changes: SharedEpochChanges<Block, Epoch>,
    /// The offchain transaction pool factory.
    pub offchain_tx_pool_factory: OffchainTransactionPoolFactory<Block>,
}

/// Start a hybrid import queue that supports importing both Aura and Babe blocks.
pub fn import_queue<B, I, C, S, CIDP, SC>(
    params: HybridImportQueueParams<B, I, C, S, CIDP, SC>,
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
    SC: SelectChain<B> + 'static,
{
    let verifier = HybridVerifier::<B, C, CIDP, NumberFor<B>, SC>::new(
        params.client,
        params.create_inherent_data_providers,
        params.telemetry,
        params.check_for_equivocation,
        params.compatibility_mode,
        params.select_chain,
        params.babe_config,
        params.epoch_changes,
        params.offchain_tx_pool_factory,
    );

    Ok(BasicQueue::new(
        verifier,
        Box::new(params.block_import),
        params.justification_import,
        params.spawner,
        params.registry,
    ))
}

/// True when the block digest carries a Babe engine id (vs Aura-only).
fn is_babe_digest(digest: &Digest) -> bool {
    digest
        .logs()
        .iter()
        .any(|d| matches!(d, DigestItem::PreRuntime(engine_id, _) if engine_id == &BABE_ENGINE_ID))
}
