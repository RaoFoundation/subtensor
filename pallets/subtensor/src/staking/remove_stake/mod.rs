//! Remove / unstake alpha and dissolve-time alpha destruction.
//!
//! ## Search anchors
//!
//! | Module | Owns |
//! |--------|------|
//! | [`remove_stake_ops`] | `do_remove_stake`, `do_unstake_all`, limit helpers |
//! | [`destroy_alpha`] | `destroy_alpha_in_out_stakes` dissolve pipeline |

use super::*;

pub mod destroy_alpha;
pub mod remove_stake_ops;
