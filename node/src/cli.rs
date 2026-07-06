use crate::{
    client::{FullBackend, FullClient},
    consensus::{AuraConsensus, BabeConsensus},
    ethereum::{EthConfiguration, FrontierBackend},
    service::new_chain_ops,
};
use node_subtensor_runtime::opaque::Block;
use sc_cli::RunCmd;
use sc_consensus::BasicQueue;
use sc_service::{Configuration, TaskManager};
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, clap::Parser)]
pub struct Cli {
    #[command(subcommand)]
    pub subcommand: Option<Subcommand>,

    #[clap(flatten)]
    pub run: RunCmd,

    /// Choose sealing method: manual, instant, or interval=<ms>.
    #[arg(long)]
    pub sealing: Option<Sealing>,

    /// Whether to try Aura or Babe consensus on first start.
    ///
    /// After starting, the consensus used by the node will automatically
    /// switch to whatever is required to continue validating / syncing.
    ///
    /// TODO: Remove this after the Babe transition has settled.
    #[arg(long, value_enum, ignore_case = true, default_value_t=SupportedConsensusMechanism::default())]
    pub initial_consensus: SupportedConsensusMechanism,

    #[command(flatten)]
    pub eth: EthConfiguration,

    /// Control historical gap-backfill during initial/catch-up sync.
    ///
    /// `keep` preserves complete history (default for normal node runs).
    /// `skip` is faster/lighter but historical block data may be incomplete.
    /// For `build-patched-spec`, the implicit default is `skip` unless this flag is explicitly set.
    #[arg(long, value_enum, default_value_t = HistoryBackfill::Keep)]
    pub history_backfill: HistoryBackfill,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, clap::Subcommand)]
pub enum Subcommand {
    // Key management cli utilities
    #[command(subcommand)]
    Key(sc_cli::KeySubcommand),

    // Build a chain specification.
    BuildSpec(sc_cli::BuildSpecCmd),

    // Validate blocks.
    CheckBlock(sc_cli::CheckBlockCmd),

    // Export blocks.
    ExportBlocks(sc_cli::ExportBlocksCmd),

    // Export the state of a given block into a chain spec.
    ExportState(sc_cli::ExportStateCmd),

    // Import blocks.
    ImportBlocks(sc_cli::ImportBlocksCmd),

    // Remove the whole chain.
    PurgeChain(sc_cli::PurgeChainCmd),

    // Revert the chain to a previous state.
    Revert(sc_cli::RevertCmd),

    // Sub-commands concerned with benchmarking.
    #[cfg(feature = "runtime-benchmarks")]
    #[command(subcommand)]
    Benchmark(frame_benchmarking_cli::BenchmarkCmd),

    // Db meta columns information.
    ChainInfo(sc_cli::ChainInfoCmd),

    // Build a patched test chainspec from synced network state.
    #[command(name = "build-patched-spec")]
    CloneState(CloneStateCmd),
}

/// Build a patched clone chainspec by syncing state, exporting raw state, and applying test patch.
#[derive(Debug, Clone, clap::Args)]
pub struct CloneStateCmd {
    /// Chain spec identifier or path (same semantics as `--chain`).
    #[arg(long, value_name = "CHAIN")]
    pub chain: String,

    /// Base path used for syncing and state export.
    #[arg(long, value_name = "PATH")]
    pub base_path: PathBuf,

    /// Output file path for the final patched chainspec JSON.
    #[arg(long, value_name = "FILE")]
    pub output: PathBuf,

    /// Sync mode for the temporary sync node.
    #[arg(long, value_enum, default_value_t = sc_cli::SyncMode::Warp)]
    pub sync: sc_cli::SyncMode,

    /// Database backend for the temporary sync/export node.
    #[arg(long, value_enum, default_value_t = sc_cli::Database::ParityDb)]
    pub database: sc_cli::Database,

    /// RPC port used by the temporary sync node.
    #[arg(long, default_value_t = 9966)]
    pub rpc_port: u16,

    /// P2P port used by the temporary sync node.
    #[arg(long, default_value_t = 30466)]
    pub port: u16,

    /// Maximum time to wait for sync completion.
    #[arg(long, default_value_t = 7200)]
    pub sync_timeout_sec: u64,

    /// Accept sync completion when current is within this many blocks of highest.
    #[arg(long, default_value_t = 8)]
    pub sync_lag_blocks: u64,

    /// Optional bootnodes for the sync step. Repeatable.
    #[arg(long, value_name = "BOOTNODE")]
    pub bootnodes: Vec<String>,

    /// Include Alice in patched validator authorities (default if no validator flags are passed;
    /// Sudo is assigned to the first selected validator in Alice->Bob->Charlie order).
    #[arg(long, default_value_t = false)]
    pub alice: bool,

    /// Include Bob in patched validator authorities (if any validator flag is set, only selected
    /// validators are used; Sudo is assigned to the first selected validator in Alice->Bob->Charlie
    /// order).
    #[arg(long, default_value_t = false)]
    pub bob: bool,

    /// Include Charlie in patched validator authorities (if any validator flag is set, only
    /// selected validators are used; Sudo is assigned to the first selected validator in
    /// Alice->Bob->Charlie order).
    #[arg(long, default_value_t = false)]
    pub charlie: bool,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum, Default)]
pub enum HistoryBackfill {
    #[default]
    Keep,
    Skip,
}

impl AsRef<str> for HistoryBackfill {
    fn as_ref(&self) -> &str {
        match self {
            HistoryBackfill::Keep => "keep",
            HistoryBackfill::Skip => "skip",
        }
    }
}

impl fmt::Display for HistoryBackfill {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_ref())
    }
}

/// Available Sealing methods.
#[derive(Copy, Clone, Debug, Default)]
pub enum Sealing {
    /// Seal using rpc method.
    #[default]
    Manual,
    /// Seal when transaction is executed.
    Instant,
    /// Seal on a fixed timer interval.  Value is the period in milliseconds.
    Interval(u64),
}

impl std::str::FromStr for Sealing {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "manual" => Ok(Sealing::Manual),
            "instant" => Ok(Sealing::Instant),
            s => s
                .parse::<u64>()
                .map(Sealing::Interval)
                .map_err(|_| format!(
                    "unknown sealing mode '{s}': expected 'manual', 'instant', or a number of milliseconds"
                )),
        }
    }
}

/// Supported consensus mechanisms.
#[derive(Copy, Clone, Debug, Default, clap::ValueEnum)]
pub enum SupportedConsensusMechanism {
    // Babe
    Babe,
    /// Aura
    #[default]
    Aura,
}

// Convinience methods for static dispatch of different service methods with
// different consensus mechanisms.
impl SupportedConsensusMechanism {
    pub fn new_chain_ops(
        &self,
        config: &mut Configuration,
        eth_config: &EthConfiguration,
        skip_history_backfill: bool,
    ) -> Result<
        (
            Arc<FullClient>,
            Arc<FullBackend>,
            BasicQueue<Block>,
            TaskManager,
            FrontierBackend,
        ),
        sc_service::Error,
    > {
        match self {
            SupportedConsensusMechanism::Aura => {
                new_chain_ops::<AuraConsensus>(config, eth_config, skip_history_backfill)
            }
            SupportedConsensusMechanism::Babe => {
                new_chain_ops::<BabeConsensus>(config, eth_config, skip_history_backfill)
            }
        }
    }
}
