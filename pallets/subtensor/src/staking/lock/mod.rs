//! Conviction / exponential stake locks.
//!
//! Locked alpha decays over time; matured conviction is the integral of locked
//! mass and feeds subnet-king selection and unstake availability.
//!
//! ## Search anchors
//!
//! | Module | Owns |
//! |--------|------|
//! | [`conviction_model`] | [`LockState`], [`RollDelta`], [`ConvictionModel`] math |
//! | [`lock_storage`] | Persist / load models, locking-coldkey index, perpetual flag |
//! | [`lock_availability`] | Locked / conviction getters, `available_to_unstake` |
//! | [`lock_operations`] | `do_lock_stake`, aggregate upserts / reductions |
//! | [`subnet_conviction`] | Hotkey totals, `subnet_king`, owner rotation |
//! | [`lock_key_swaps`] | Coldkey / hotkey swap lock migration |
//! | [`lock_transfer`] | `do_move_lock`, `transfer_lock`, network lock wipe |

use super::*;

pub mod conviction_model;
pub mod lock_availability;
pub mod lock_key_swaps;
pub mod lock_operations;
pub mod lock_storage;
pub mod lock_transfer;
pub mod subnet_conviction;

pub use conviction_model::{
    ConvictionModel, LOCK_STATE_ZERO_THRESHOLD, LockState, ONE_YEAR, RollDelta,
};
