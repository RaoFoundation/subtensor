//! The single error type crossing the core's boundary.
//!
//! Binding crates map each variant onto their language's existing exception
//! taxonomy exactly once (for Python: `KeyfileError`, `WrongPasswordError`,
//! `StorageFunctionNotFound`, `ValueError`), so no caller above the binding
//! ever changes an except clause.

use core::fmt;

#[derive(Debug)]
pub enum CoreError {
    /// Keyfile data is malformed or uses an unknown encryption envelope.
    Keyfile(String),
    /// Keyfile decryption failed because the password is wrong.
    WrongPassword(String),
    /// A named pallet / storage item / call / runtime API does not exist in
    /// the runtime this operation was given.
    NotInRuntime(String),
    /// Encode/decode rejected the value or bytes (maps to ValueError).
    Codec(String),
    /// Crypto operation failed (bad key length, invalid signature bytes...).
    Crypto(String),
    /// A hardware signing device could not be reached, rejected the request,
    /// or the user declined on-device.
    Device(String),
    /// JSON-RPC transport, protocol, or receipt processing failed.
    Rpc(String),
    /// A semantic transaction was rejected before signing by local policy.
    Policy(String),
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoreError::Keyfile(msg) => write!(f, "keyfile error: {msg}"),
            CoreError::WrongPassword(msg) => write!(f, "{msg}"),
            CoreError::NotInRuntime(what) => write!(f, "{what} not found in this runtime"),
            CoreError::Codec(msg) => write!(f, "codec error: {msg}"),
            CoreError::Crypto(msg) => write!(f, "crypto error: {msg}"),
            CoreError::Device(msg) => write!(f, "device error: {msg}"),
            CoreError::Rpc(msg) => write!(f, "rpc error: {msg}"),
            CoreError::Policy(msg) => write!(f, "policy error: {msg}"),
        }
    }
}

impl std::error::Error for CoreError {}
