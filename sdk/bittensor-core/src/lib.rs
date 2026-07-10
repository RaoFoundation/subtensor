//! The chain-defined compute core for Bittensor clients.
//!
//! One rule decides what lives here: Rust owns everything whose right answer
//! is defined by the chain (crypto, SCALE, extrinsic payloads, metadata
//! digests); the client SDKs own everything whose right answer is a product
//! decision (intents, policy, CLI UX, transports). This crate contains no
//! binding code — `bittensor-core-py` (and future uniffi/napi siblings)
//! expose it to other languages.
//!
//! See `sdk/bittensor-core-spec.md` for the full design.

pub mod codec;
pub mod digest;
pub mod error;
pub mod keyfiles;
pub mod keys;
pub mod mlkem;
pub mod runtime;
pub mod signers;
pub mod timelock;

pub use error::CoreError;
