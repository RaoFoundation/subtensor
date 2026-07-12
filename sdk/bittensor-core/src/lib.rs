//! The chain-defined compute core and native Rust SDK for Bittensor clients.
//!
//! Rust owns the chain-defined pieces (crypto, SCALE, extrinsic payloads,
//! metadata digests) and now also exposes a thin native client and semantic
//! transaction executor over those primitives. Binding crates may reuse the
//! same client or expose only the compute surface to another language.
//!
//! See `sdk/bittensor-core-spec.md` for the full design.

pub mod client;
pub mod codec;
pub mod digest;
pub mod error;
#[cfg(feature = "host")]
pub mod keyfiles;
pub mod keys;
pub mod mlkem;
pub mod runtime;
pub mod signers;
pub mod timelock;
pub mod transaction;

pub use client::Client;
pub use error::CoreError;
pub use transaction::{Executor, IntentCall, Plan, Policy, SignerRole, Spend, Wallet};
