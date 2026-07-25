//! `pallet_section` modules composing the SubtensorModule pallet.
//!
//! Each child is imported into `pallet` via `import_section`. Dispatchables, events, and errors
//! live in sibling modules and are frozen for metadata; this module also owns `config`,
//! `genesis`, and `hooks`.

pub mod config;
pub mod dispatches;
pub mod errors;
pub mod events;
pub mod genesis;
pub mod hooks;
