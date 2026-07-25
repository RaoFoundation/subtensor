//! Block import that routes pre-/post-Frontier eras and optional history-gap skip.
//!
//! Blocks before the mainnet Frontier upgrade (#4345557) use the inner import;
//! later blocks go through Frontier. When history backfill is skipped, initial-sync
//! imports clear `create_gap` so Substrate does not schedule gap reconstruction.

use sc_consensus::{BlockCheckParams, BlockImport, BlockImportParams, ImportResult};
use sp_consensus::{BlockOrigin, Error as ConsensusError};
use sp_runtime::traits::{Block as BlockT, Header};
use std::marker::PhantomData;

/// Dual-path block import: native before Frontier activation, Frontier afterward.
pub struct ConditionalEVMBlockImport<B: BlockT, I, F> {
    inner: I,
    frontier_block_import: F,
    skip_history_backfill: bool,
    _marker: PhantomData<B>,
}

impl<B, I, F> Clone for ConditionalEVMBlockImport<B, I, F>
where
    B: BlockT,
    I: Clone + BlockImport<B>,
    F: Clone + BlockImport<B>,
{
    fn clone(&self) -> Self {
        ConditionalEVMBlockImport {
            inner: self.inner.clone(),
            frontier_block_import: self.frontier_block_import.clone(),
            skip_history_backfill: self.skip_history_backfill,
            _marker: PhantomData,
        }
    }
}

impl<B, I, F> ConditionalEVMBlockImport<B, I, F>
where
    B: BlockT,
    I: BlockImport<B>,
    I::Error: Into<ConsensusError>,
    F: BlockImport<B>,
    F::Error: Into<ConsensusError>,
{
    /// Wrap an inner + Frontier import with optional history-gap suppression.
    pub fn new(inner: I, frontier_block_import: F, skip_history_backfill: bool) -> Self {
        Self {
            inner,
            frontier_block_import,
            skip_history_backfill,
            _marker: PhantomData,
        }
    }
}

#[async_trait::async_trait]
impl<B, I, F> BlockImport<B> for ConditionalEVMBlockImport<B, I, F>
where
    B: BlockT,
    I: BlockImport<B> + Send + Sync,
    I::Error: Into<ConsensusError>,
    F: BlockImport<B> + Send + Sync,
    F::Error: Into<ConsensusError>,
{
    type Error = ConsensusError;

    async fn check_block(&self, block: BlockCheckParams<B>) -> Result<ImportResult, Self::Error> {
        self.inner.check_block(block).await.map_err(Into::into)
    }

    async fn import_block(
        &self,
        mut block: BlockImportParams<B>,
    ) -> Result<ImportResult, Self::Error> {
        if self.skip_history_backfill && matches!(block.origin, BlockOrigin::NetworkInitialSync) {
            // During initial network sync, Substrate can mark missing historical ranges as "gaps"
            // (`create_gap = true`) and then backfill them later. When history backfill is set to
            // `skip`, we disable gap creation so no history reconstruction work is scheduled.
            // `build-patched-spec` just defaults this setting to `skip`.
            block.create_gap = false;
        }
        // 4345556 - mainnet runtime upgrade block with Frontier
        if *block.header.number() < 4345557u32.into() {
            self.inner.import_block(block).await.map_err(Into::into)
        } else {
            self.frontier_block_import
                .import_block(block)
                .await
                .map_err(Into::into)
        }
    }
}
