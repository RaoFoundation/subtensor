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
    WrongPassword,
    /// A named pallet / storage item / call / runtime API does not exist in
    /// the runtime this operation was given.
    NotInRuntime(String),
    /// Encode/decode rejected the value or bytes (maps to ValueError).
    Codec(String),
    /// Crypto operation failed (bad key length, invalid signature bytes...).
    Crypto(String),
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoreError::Keyfile(msg) => write!(f, "keyfile error: {msg}"),
            CoreError::WrongPassword => write!(f, "wrong password"),
            CoreError::NotInRuntime(what) => write!(f, "{what} not found in this runtime"),
            CoreError::Codec(msg) => write!(f, "codec error: {msg}"),
            CoreError::Crypto(msg) => write!(f, "crypto error: {msg}"),
        }
    }
}

impl std::error::Error for CoreError {}
