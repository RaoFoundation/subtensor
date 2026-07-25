//! Parent/child hotkey delegation (childkeys) and childkey take.
//!
//! ## Search anchors
//!
//! | Module | Owns |
//! |--------|------|
//! | [`parent_child_relations`] | [`ParentChildRelations`] in-memory graph + invariants |
//! | [`parent_child_storage`] | Load/persist edges, `parent_child_swap_hotkey` |
//! | [`schedule_children`] | `do_schedule_children`, pending apply |
//! | [`childkey_take`] | `do_set_childkey_take` / getters |
//! | [`root_validators`] | `do_set_root_validators_for_subnet` |
//! | [`repair_children`] | Empty-vector / self-loop / consistency repair |

use super::*;

pub mod childkey_take;
pub mod parent_child_relations;
pub mod parent_child_storage;
pub mod repair_children;
pub mod root_validators;
pub mod schedule_children;

pub use parent_child_relations::ParentChildRelations;
