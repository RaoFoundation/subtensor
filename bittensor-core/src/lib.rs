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

pub mod error;

pub use error::CoreError;
