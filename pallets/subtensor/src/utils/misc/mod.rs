//! Grab-bag of pallet helpers shared across staking, epoch, admin-utils, and extrinsics.
//!
//! Prefer searching the concept modules below rather than this directory name:
//! - [`origin_and_admin`] — owner/root origins, admin freeze window, owner RL recording
//! - [`tempo_and_counters`] — tempo, registration counters, [`Pallet::get_current_block_as_u64`]
//! - [`consensus_params`] — emission/consensus/incentive vector accessors
//! - [`take_and_locks`] — take ownership checks, subnet locked TAO
//! - [`subnet_hyperparams`] — burn/difficulty/weights/owner-cut/… getters & setters
//! - [`q32_math`] — Q32 fixed-point multiply / pow / half-life decay

use super::*;

pub mod consensus_params;
pub mod origin_and_admin;
pub mod q32_math;
pub mod subnet_hyperparams;
pub mod take_and_locks;
pub mod tempo_and_counters;
