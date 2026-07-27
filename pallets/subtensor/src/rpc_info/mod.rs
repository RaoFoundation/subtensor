//! Runtime-API / RPC view builders for Subtensor state.
//!
//! Each submodule assembles a frozen SCALE DTO (`DelegateInfo`, `Metagraph`,
//! `StakeInfo`, …) from pallet storage for custom RPCs. Public method names here
//! are wired through `runtime-api` and must stay stable; private helpers in these
//! files are fair game for clearer naming.

use super::*;
pub mod delegate_info;
pub mod dynamic_info;
pub mod metagraph;
pub mod neuron_info;
pub mod show_subnet;
pub mod stake_info;
pub mod subnet_info;
