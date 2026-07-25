//! Subnet lifecycle: registration, UIDs, serving, mechanisms, leasing, dissolve.
//!
//! Search anchors:
//! - [`subnet`] — create/init networks, subnet account IDs, owner-cut flags
//! - [`registration`] — neuron register / faucet / prune / POW helpers
//! - [`collateral`] — miner registration collateral lock and drain
//! - [`uids`] — append/replace/trim neurons and uid↔hotkey lookups
//! - [`serving`] — axon / prometheus endpoint publish + validation
//! - [`mechanism`] — sub-subnet storage index and multi-mechanism epoch
//! - [`leasing`] — crowdloan-backed leased subnet registration
//! - [`dissolution`] — dissolve queue + weight-metered cleanup phases
//! - [`symbols`] — default token symbol / name tables per netuid
//! - [`weights`] — commit/reveal/set weights (owned by a later shard; not edited here)

use super::*;
pub mod collateral;
pub mod dissolution;
pub mod leasing;
pub mod mechanism;
pub mod registration;
pub mod serving;
pub mod subnet;
pub mod symbols;
pub mod uids;
pub mod weights;
