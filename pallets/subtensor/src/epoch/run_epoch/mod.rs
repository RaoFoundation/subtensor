//! Subnet epoch: Yuma consensus scoring and per-hotkey emission terms.
//!
//! Production path: [`epoch_mechanism`] → [`persist_mechanism_epoch_terms`] /
//! [`persist_netuid_epoch_terms`] (usually via the coinbase / mechanism runners).
//!
//! ## Search anchors
//!
//! | Module | Owns |
//! |--------|------|
//! | [`epoch_terms`] | [`EpochTerms`], [`HotkeyEpochTerms`], [`collect_sorted_epoch_field`] |
//! | [`persist_epoch_terms`] | legacy `epoch` wrappers + storage writes |
//! | [`epoch_mechanism`] | sparse production epoch |
//! | [`epoch_dense`] | dense epoch (tests only) |
//! | [`weights_bonds_loaders`] | read weights/bonds; kappa/rho fixed casts |
//! | [`bonds_ema_liquid_alpha`] | bonds EMA, liquid alpha, `do_set_alpha_values`, bonds reset |

use super::*;

mod bonds_ema_liquid_alpha;
mod epoch_dense;
mod epoch_mechanism;
mod epoch_terms;
mod persist_epoch_terms;
mod weights_bonds_loaders;

pub use epoch_terms::{EpochTerms, HotkeyEpochTerms};
