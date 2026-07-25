//! Signed-transaction extensions for Subtensor (`TransactionExtension`).
//!
//! Unlike [`crate::guards`] (which run on every `dispatch`, including nested
//! proxy calls), types here participate in the outer signed extrinsic pipeline:
//! they validate and charge weight before the call is included, and map pallet
//! [`Error`](crate::Error) values onto
//! [`CustomTransactionError`](subtensor_runtime_common::CustomTransactionError)
//! for the pool.
//!
//! ## Search anchors
//!
//! | Type | Role |
//! |------|------|
//! | [`SubtensorTransactionExtension`] | Runs coldkey-swap + weight/rate/delegate/serve/EVM guard checks at validate time |
//!
//! Guard implementations live under [`crate::guards`]; this module only wires them
//! into `TransactionExtension::{weight, validate}`.

mod subtensor;

pub use subtensor::*;
