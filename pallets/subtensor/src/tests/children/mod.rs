#![allow(
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::indexing_slicing
)]
//! Integration tests for parent/child hotkeys ([`crate::staking::set_children`]).
//!
//! Layout mirrors `staking/set_children/` plus inherited-stake / emission concepts
//! exercised through childkey edges.
//!
//! ## Search anchors
//!
//! | Module | Owns |
//! |--------|------|
//! | [`helpers`] | `close` numeric assert helper |
//! | [`schedule_singular`] | singular `do_schedule_children` / revoke |
//! | [`schedule_multiple`] | multi-child schedule, revoke, storage clear |
//! | [`pending_children`] | cooldown, pending apply, min-stake / rate-limit gates |
//! | [`childkey_take`] | `do_set_childkey_take` / take drain |
//! | [`inherited_stake`] | inherited stake via parent/child proportions |
//! | [`child_weights`] | set_weights with parent/child edges |
//! | [`child_emission`] | emission / epoch through parent-child chains |
//! | [`child_dividends`] | dividend distribution with children |
//! | [`root_validators`] | root-validator auto child scheduling |

mod child_dividends;
mod child_emission;
mod child_weights;
mod childkey_take;
mod helpers;
mod inherited_stake;
mod pending_children;
mod root_validators;
mod schedule_multiple;
mod schedule_singular;
