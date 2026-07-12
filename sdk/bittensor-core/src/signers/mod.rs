//! Hardware and remote signing backends.
//!
//! Each backend is feature-gated so the default build (and the default
//! wheel) carries no device-transport native dependencies.

#[cfg(feature = "ledger")]
mod hid;
#[cfg(feature = "ledger")]
pub mod ledger;
