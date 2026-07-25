#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
//! Integration tests for [`crate::epoch`] (`run_epoch`, bonds EMA / liquid alpha, weight loaders).
//!
//! Split from the former monolithic `tests/epoch.rs` into concept-named modules.
//!
//! ## Search anchors
//!
//! | Module | Owns |
//! |--------|------|
//! | [`helpers`] | `init_run_epochs`, node distribution, normalize helpers |
//! | [`graph_epochs`] | 1/10/512-node graph epoch runs |
//! | [`bonds`] | bond accumulation and deregistered-miner bonds |
//! | [`liquid_alpha`] | liquid alpha get/set and equal-alpha checks |
//! | [`active_stake`] | active-stake filtering |
//! | [`weight_activity`] | outdated / zero weights |
//! | [`validator_permits`] | validator permit issuance |
//! | [`epoch_timing`] | blocks since last step |
//! | [`self_weight`] | subnet-owner self-weight |
//! | [`epoch_outputs`] | minimal topology epoch outputs |
//! | [`yuma_3`] | Yuma3 kappa / bonds / liquid-alpha scenarios |
//! | [`snipe_weight_mask`] | sniped-UID weight masking |
//! | [`epoch_input_state`] | input consistency + LastUpdate mismatch |

mod active_stake;
mod bonds;
mod epoch_input_state;
mod epoch_outputs;
mod epoch_timing;
mod graph_epochs;
mod helpers;
mod liquid_alpha;
mod self_weight;
mod snipe_weight_mask;
mod validator_permits;
mod weight_activity;
mod yuma_3;
