//! Ledger's USB HID transport framing, over `hidapi`.
//!
//! A reimplementation of Zondax's `ledger-transport-hid` (Apache-2.0)
//! narrowed to the one blocking exchange `ledger.rs` needs. It exists so the
//! Linux build can select hidapi's pure-Rust `linux-native-basic-udev`
//! backend: the published transport crate hard-pins `linux-static-hidraw`,
//! which links `libudev` and would disqualify the wheel from the manylinux
//! tag (see the crate's Cargo.toml and `build-core-wheels.yml`).
//!
//! The wire format is Ledger's HID framing: APDUs travel in 64-byte reports,
//! each prefixed with a 5-byte header (channel, tag `0x05`, big-endian
//! sequence number); the first report additionally carries the big-endian
//! total length.

// Client-side device I/O, not runtime code: bounds are checked with explicit
// length guards before slicing.
#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

use std::sync::Mutex;

use hidapi::{DeviceInfo, HidApi, HidDevice};

use crate::error::CoreError;

const LEDGER_VID: u16 = 0x2c97;
const LEDGER_USAGE_PAGE: u16 = 0xFFA0;
const CHANNEL: u16 = 0x0101;
const TAG: u8 = 0x05;
/// HID reports are 64 bytes; writes carry one extra leading report-ID byte
/// (required on Windows, tolerated everywhere else).
const READ_SIZE: usize = 64;
const WRITE_SIZE: usize = READ_SIZE + 1;
/// Effectively "wait for the user": reads block on on-device approval.
const READ_TIMEOUT_MS: i32 = 10_000_000;

/// A blocking APDU channel to one Ledger device.
///
/// The mutex makes the channel `Sync` (bindings hold it in shared host
/// objects and release their locks around exchanges) and serializes whole
/// write+read round-trips so concurrent callers cannot interleave frames.
pub struct Transport {
    device: Mutex<HidDevice>,
}

/// The APDU interface advertises usage page `0xFFA0`; interface 0 is the
/// documented fallback for platforms/kernels that don't expose usage pages.
fn is_ledger(info: &DeviceInfo) -> bool {
    info.vendor_id() == LEDGER_VID
        && (info.usage_page() == LEDGER_USAGE_PAGE || info.interface_number() == 0)
}

impl Transport {
    /// Open the first Ledger reachable over HID.
    pub fn open() -> Result<Self, CoreError> {
        let api =
            HidApi::new().map_err(|e| CoreError::Device(format!("cannot initialize HID: {e}")))?;
        let info = api
            .device_list()
            .find(|info| is_ledger(info))
            .ok_or_else(|| {
                CoreError::Device("no Ledger device found (is it connected and unlocked?)".into())
            })?;
        let device = info.open_device(&api).map_err(|e| {
            CoreError::Device(format!(
                "no Ledger device found (is it connected and unlocked?): {e}"
            ))
        })?;
        let _ = device.set_blocking_mode(true);
        Ok(Self {
            device: Mutex::new(device),
        })
    }

    /// One APDU round-trip. Returns the raw answer including the trailing
    /// two-byte return code; blocks until the device responds.
    pub fn exchange(&self, apdu: &[u8]) -> Result<Vec<u8>, CoreError> {
        let device = self
            .device
            .lock()
            .map_err(|_| CoreError::Device("HID transport poisoned by an earlier panic".into()))?;
        Self::write_apdu(&device, apdu)?;
        Self::read_apdu(&device)
    }

    fn write_apdu(device: &HidDevice, apdu: &[u8]) -> Result<(), CoreError> {
        let total = u16::try_from(apdu.len())
            .map_err(|_| CoreError::Device("APDU exceeds the 64 KiB framing limit".into()))?;
        let mut message = Vec::with_capacity(apdu.len() + 2);
        message.extend_from_slice(&total.to_be_bytes());
        message.extend_from_slice(apdu);

        // [report id, channel, channel, tag, seq, seq, payload...]
        for (sequence, chunk) in message.chunks(WRITE_SIZE - 6).enumerate() {
            let sequence = u16::try_from(sequence)
                .map_err(|_| CoreError::Device("APDU framing sequence overflow".into()))?;
            let mut report = [0u8; WRITE_SIZE];
            report[1..3].copy_from_slice(&CHANNEL.to_be_bytes());
            report[3] = TAG;
            report[4..6].copy_from_slice(&sequence.to_be_bytes());
            report[6..6 + chunk.len()].copy_from_slice(chunk);
            let written = device
                .write(&report)
                .map_err(|e| CoreError::Device(format!("HID write failed: {e}")))?;
            if written < report.len() {
                return Err(CoreError::Device("HID write was truncated".into()));
            }
        }
        Ok(())
    }

    fn read_apdu(device: &HidDevice) -> Result<Vec<u8>, CoreError> {
        let mut answer: Vec<u8> = Vec::new();
        let mut expected = 0usize;
        let mut report = [0u8; READ_SIZE];
        for sequence in 0u16.. {
            let read = device
                .read_timeout(&mut report, READ_TIMEOUT_MS)
                .map_err(|e| CoreError::Device(format!("HID read failed: {e}")))?;
            // The first report's header also carries the 2-byte total length.
            let header = if sequence == 0 { 7 } else { 5 };
            if read < header {
                return Err(CoreError::Device("HID read returned a short report".into()));
            }
            if u16::from_be_bytes([report[0], report[1]]) != CHANNEL {
                return Err(CoreError::Device("HID response on wrong channel".into()));
            }
            if report[2] != TAG {
                return Err(CoreError::Device("HID response with wrong tag".into()));
            }
            if u16::from_be_bytes([report[3], report[4]]) != sequence {
                return Err(CoreError::Device(
                    "HID response out of sequence (is another process using the device?)".into(),
                ));
            }
            if sequence == 0 {
                expected = usize::from(u16::from_be_bytes([report[5], report[6]]));
            }
            let payload = &report[header..read];
            let missing = expected.saturating_sub(answer.len());
            answer.extend_from_slice(&payload[..payload.len().min(missing)]);
            if answer.len() >= expected {
                break;
            }
        }
        Ok(answer)
    }
}
