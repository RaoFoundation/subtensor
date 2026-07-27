#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
//! Unit tests for stake locks, conviction, and lock invariants.
//!
//! Split from the former monolithic `tests/locks.rs` into concept modules.
//!
//! ## Search anchors
//!
//! | Module | Owns |
//! |--------|------|
//! | [`helpers`] | setup/roll-forward fixtures |
//! | [`account_flags_reject_locked_alpha`] | AccountFlags reject-locked-alpha |
//! | [`lock_stake_creation`] | Green-path — basic lock creation |
//! | [`lock_queries`] | Green-path — lock queries |
//! | [`lock_topup`] | Incremental locks (top-up) |
//! | [`lock_rejection`] | Lock rejection cases |
//! | [`conviction_roll_forward`] | ConvictionModel roll-forward math |
//! | [`unstake_lock_invariant`] | Unstake invariant enforcement |
//! | [`move_transfer_lock`] | Move/transfer invariant enforcement |
//! | [`multi_subnet_locks`] | Multi-subnet locks |
//! | [`hotkey_conviction_subnet_king`] | Hotkey conviction and subnet king |
//! | [`force_reduce_lock`] | Lock force-reduction |
//! | [`coldkey_swap_lock`] | Coldkey swap interaction |
//! | [`hotkey_swap_lock`] | Hotkey swap interaction |
//! | [`lock_stake_extrinsic`] | Lock extrinsic via dispatch |
//! | [`recycle_burn_lock`] | Recycle/burn alpha checks against lock |
//! | [`subnet_dissolution_lock`] | Subnet dissolution |
//! | [`clear_small_nomination_lock`] | Clear small nomination checks lock |
//! | [`emission_lock`] | Emission interaction |
//! | [`neuron_replacement_lock`] | Neuron replacement |
//! | [`moving_lock`] | Moving lock |

mod account_flags_reject_locked_alpha;
mod clear_small_nomination_lock;
mod coldkey_swap_lock;
mod conviction_roll_forward;
mod emission_lock;
mod force_reduce_lock;
mod helpers;
mod hotkey_conviction_subnet_king;
mod hotkey_swap_lock;
mod lock_queries;
mod lock_rejection;
mod lock_stake_creation;
mod lock_stake_extrinsic;
mod lock_topup;
mod move_transfer_lock;
mod moving_lock;
mod multi_subnet_locks;
mod neuron_replacement_lock;
mod prelude;
mod recycle_burn_lock;
mod subnet_dissolution_lock;
mod unstake_lock_invariant;
