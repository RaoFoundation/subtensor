//! Hardware and remote signing backends.
//!
//! Each backend is feature-gated so the default build (and the default
//! wheel) carries no device-transport native dependencies.

use crate::error::CoreError;

#[cfg(feature = "ledger")]
mod hid;
#[cfg(feature = "ledger")]
pub mod ledger;

/// A blocking channel that carries one APDU to a device and returns its answer.
///
/// `ledger::LedgerDevice::open` uses the built-in USB HID transport;
/// `ledger::LedgerDevice::with_transport` accepts another implementation,
/// such as a TCP client for Ledger's Speculos emulator or a replay of
/// recorded exchanges, so the APDU logic can run without a device.
pub trait ApduTransport: Send + Sync {
    /// Send the serialized APDU, block until the device answers, and return
    /// the answer's data followed by its two-byte status word.
    fn exchange(&self, apdu: &[u8]) -> Result<Vec<u8>, CoreError>;
}
