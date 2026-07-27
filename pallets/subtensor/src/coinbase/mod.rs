//! Coinbase / emission pipeline for Subtensor.
//!
//! Each block, [`Pallet::block_step`] (via the runtime hook):
//! 1. Updates registration prices
//! 2. Mints this block's TAO ([`block_emission`])
//! 3. Reveals matured CRv3 weight commits ([`reveal_commits`])
//! 4. Runs the coinbase ([`run_coinbase`]) — inject liquidity, accumulate pending alpha,
//!    drain on epoch, pay dividends
//! 5. Updates moving prices / root proportions
//! 6. Applies pending children, auto-claims root divs, refreshes root coldkey maps
//!
//! ## Search anchors
//!
//! | Module | Owns |
//! |--------|------|
//! | [`tao`] | TAO mint/burn/recycle/transfer and registration locks |
//! | [`alpha`] | Alpha mint/resolve/recycle/burn into subnet reserves |
//! | [`block_emission`] | Logarithmic TAO emission schedule vs total issuance |
//! | [`subnet_emissions`] | Which subnets emit and how TAO is shared among them |
//! | [`run_coinbase`] | Injection, pending drain, dividend payout |
//! | [`tempo_control`] | Owner/root tempo, activity cutoff, epoch trigger |
//! | [`root`] | Root registration, network lock cost, prune candidate |
//! | [`reveal_commits`] | Timelock (drand) weight reveal |
//! | [`block_step`] | Ordered per-block orchestration of the above |

use super::*;

pub mod alpha;
pub mod block_emission;
pub mod block_step;
pub mod reveal_commits;
pub mod root;
pub mod run_coinbase;
pub mod subnet_emissions;
pub mod tao;
pub mod tempo_control;
