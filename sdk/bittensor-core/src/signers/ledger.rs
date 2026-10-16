//! The Ledger Polkadot generic app over an APDU transport.
//!
//! Speaks the Zondax APDU protocol (CLA `0xF9`): `GET_VERSION`, `GET_ADDR`,
//! and the chunked `SIGN` whose message is the signature payload followed by
//! the RFC-0078 metadata proof (see [`crate::digest`]). The generic app
//! clear-signs: it decodes the transaction on-device against the proof and
//! refuses to sign what it cannot display, so there is no blind-signing path
//! here.
//!
//! The app signs with ed25519 (scheme 0) on the Polkadot derivation path
//! `m/44'/354'/account'/0'/index'` — the same keys Ledger Live and every
//! generic-app wallet derive, so one device shows one set of addresses
//! everywhere.
//!
//! [`LedgerDevice::open`] reaches the device over USB HID;
//! [`LedgerDevice::with_transport`] sends the same requests over another
//! [`ApduTransport`], such as Ledger's Speculos emulator.

// Client-side device I/O, not runtime code: bounds are checked with explicit
// length guards before slicing.
#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

use ledger_apdu::{APDUAnswer, APDUCommand};

use crate::error::CoreError;
use crate::signers::hid::Transport;
use crate::signers::ApduTransport;

const CLA: u8 = 0xF9;
const INS_GET_VERSION: u8 = 0x00;
const INS_GET_ADDR: u8 = 0x01;
const INS_SIGN: u8 = 0x02;

const P1_ADDR_SHOW: u8 = 1;
const P1_ADDR_SILENT: u8 = 0;
const P1_SIGN_INIT: u8 = 0;
const P1_SIGN_ADD: u8 = 1;
const P1_SIGN_LAST: u8 = 2;

/// Scheme byte (P2): the generic app supports ed25519 (0) and ecdsa (2).
const SCHEME_ED25519: u8 = 0;

const HARDENED: u32 = 0x8000_0000;
const SLIP44_POLKADOT: u32 = 354;
const CHUNK_SIZE: usize = 250;
const RETCODE_OK: u16 = 0x9000;
const RETCODE_USER_REFUSED: u16 = 0x6986;

/// An address the device derived (and optionally displayed).
#[derive(Debug, Clone)]
pub struct LedgerAddress {
    pub public_key: [u8; 32],
    pub ss58_address: String,
}

/// One connected Ledger running the Polkadot generic app.
pub struct LedgerDevice {
    transport: Box<dyn ApduTransport>,
}

impl LedgerDevice {
    /// Connect to the first Ledger reachable over HID.
    ///
    /// Fails with a device error when no Ledger is plugged in / unlocked or
    /// when the OS denies HID access.
    pub fn open() -> Result<Self, CoreError> {
        Ok(Self {
            transport: Box::new(Transport::open()?),
        })
    }

    /// Drive the generic app over `transport` instead of USB HID.
    ///
    /// Requests are built and answers parsed exactly as for a device from
    /// [`LedgerDevice::open`]; only the channel differs.
    pub fn with_transport(transport: impl ApduTransport + 'static) -> Self {
        Self {
            transport: Box::new(transport),
        }
    }

    /// The generic app's version, as `(major, minor, patch)`.
    ///
    /// Also serves as the "is the right app open?" probe: any other app
    /// rejects CLA `0xF9`.
    pub fn app_version(&self) -> Result<(u16, u16, u16), CoreError> {
        let answer = self.exchange(INS_GET_VERSION, 0, 0, Vec::new())?;
        let data = answer.as_slice();
        if data.len() < 7 {
            return Err(CoreError::Device(
                "unexpected GET_VERSION response length".into(),
            ));
        }
        let word = |i: usize| u16::from_be_bytes([data[i], data[i + 1]]);
        Ok((word(1), word(3), word(5)))
    }

    /// Derive `m/44'/354'/account'/0'/index'` on-device.
    ///
    /// With `confirm`, the device displays the address and waits for the
    /// user to approve before returning it.
    pub fn address(
        &self,
        account: u32,
        index: u32,
        ss58_prefix: u16,
        confirm: bool,
    ) -> Result<LedgerAddress, CoreError> {
        let mut payload = bip44_path(account, index).to_vec();
        payload.extend_from_slice(&ss58_prefix.to_le_bytes());
        let p1 = if confirm {
            P1_ADDR_SHOW
        } else {
            P1_ADDR_SILENT
        };
        let answer = self.exchange(INS_GET_ADDR, p1, SCHEME_ED25519, payload)?;
        let data = answer.as_slice();
        if data.len() < 32 {
            return Err(CoreError::Device(
                "unexpected GET_ADDR response length".into(),
            ));
        }
        let mut public_key = [0u8; 32];
        public_key.copy_from_slice(&data[..32]);
        let ss58_address = String::from_utf8(data[32..].to_vec())
            .map_err(|_| CoreError::Device("device returned a non-UTF-8 address".into()))?;
        Ok(LedgerAddress {
            public_key,
            ss58_address,
        })
    }

    /// Clear-sign `payload` (the exact signature payload bytes, unhashed)
    /// using the RFC-0078 `proof` for on-device decoding.
    ///
    /// Blocks until the user approves or rejects on the device. Returns the
    /// 65-byte MultiSignature (version prefix + ed25519 signature) — the
    /// shape the SDK's `attach_signature` already understands.
    pub fn sign(
        &self,
        account: u32,
        index: u32,
        payload: &[u8],
        proof: &[u8],
    ) -> Result<Vec<u8>, CoreError> {
        let payload_len = u16::try_from(payload.len()).map_err(|_| {
            CoreError::Device("signature payload exceeds the device's 64 KiB limit".into())
        })?;
        let mut first = bip44_path(account, index).to_vec();
        first.extend_from_slice(&payload_len.to_le_bytes());
        self.exchange(INS_SIGN, P1_SIGN_INIT, SCHEME_ED25519, first)?;

        let message: Vec<u8> = [payload, proof].concat();
        let chunks: Vec<&[u8]> = message.chunks(CHUNK_SIZE).collect();
        let last = chunks.len().saturating_sub(1);
        let mut signature = Vec::new();
        for (i, chunk) in chunks.iter().enumerate() {
            let p1 = if i == last { P1_SIGN_LAST } else { P1_SIGN_ADD };
            let answer = self.exchange(INS_SIGN, p1, SCHEME_ED25519, chunk.to_vec())?;
            if i == last {
                signature = answer;
            }
        }
        if signature.len() != 65 {
            return Err(CoreError::Device(format!(
                "unexpected signature length {} (want 65)",
                signature.len()
            )));
        }
        Ok(signature)
    }

    fn exchange(&self, ins: u8, p1: u8, p2: u8, data: Vec<u8>) -> Result<Vec<u8>, CoreError> {
        let command = APDUCommand {
            cla: CLA,
            ins,
            p1,
            p2,
            data,
        };
        let raw = self.transport.exchange(&command.serialize())?;
        let answer = APDUAnswer::from_answer(raw)
            .map_err(|_| CoreError::Device("APDU response was too short".into()))?;
        match answer.retcode() {
            RETCODE_OK => Ok(answer.data().to_vec()),
            RETCODE_USER_REFUSED => Err(CoreError::Device(
                "the request was rejected on the device".into(),
            )),
            code => Err(CoreError::Device(device_error(code, answer.data()))),
        }
    }
}

/// The 5-element hardened BIP44 path the generic app expects, serialized as
/// 20 little-endian bytes: `m/44'/354'/account'/0'/index'`.
fn bip44_path(account: u32, index: u32) -> [u8; 20] {
    let elements = [
        HARDENED | 44,
        HARDENED | SLIP44_POLKADOT,
        HARDENED | account,
        HARDENED, // change = 0'
        HARDENED | index,
    ];
    let mut out = [0u8; 20];
    for (slot, element) in out.chunks_exact_mut(4).zip(elements) {
        slot.copy_from_slice(&element.to_le_bytes());
    }
    out
}

/// A readable message for a non-success APDU return code. The app often
/// appends an ASCII reason to the error payload; surface it when present.
fn device_error(code: u16, data: &[u8]) -> String {
    let known = match code {
        0x6400 => Some("execution error"),
        0x6700 => Some("wrong buffer length"),
        0x6982 => Some("empty buffer"),
        0x6983 => Some("output buffer too small"),
        0x6984 => Some("data is invalid"),
        0x6987 => Some("transaction is not initialized"),
        0x6B00 => Some("invalid P1/P2"),
        0x6D00 => Some("instruction not supported (is the Polkadot app open?)"),
        0x6E00 => Some("app not recognized (is the Polkadot app open?)"),
        0x6F01 => Some("sign/verify error"),
        _ => None,
    };
    let reason = core::str::from_utf8(data).ok().filter(|s| !s.is_empty());
    match (known, reason) {
        (Some(k), Some(r)) => format!("device returned {code:#06x} ({k}): {r}"),
        (Some(k), None) => format!("device returned {code:#06x} ({k})"),
        (None, Some(r)) => format!("device returned {code:#06x}: {r}"),
        (None, None) => format!("device returned {code:#06x}"),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    struct ScriptedTransport {
        exchanges: Arc<Mutex<VecDeque<(Vec<u8>, Vec<u8>)>>>,
    }

    impl ApduTransport for ScriptedTransport {
        fn exchange(&self, apdu: &[u8]) -> Result<Vec<u8>, CoreError> {
            let (expected, response) = self
                .exchanges
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected APDU");
            assert_eq!(apdu, expected);
            Ok(response)
        }
    }

    #[test]
    fn ledger_device_is_send_and_sync() {
        fn assert_send_and_sync<T: Send + Sync>() {}
        assert_send_and_sync::<LedgerDevice>();
    }

    #[test]
    fn with_transport_carries_the_chunked_sign() {
        let payload = vec![0xab; 300];
        let proof = vec![0xcd; 200];
        let mut init = vec![0xf9, 0x02, 0x00, 0x00, 22];
        init.extend_from_slice(&[
            44, 0, 0, 0x80, 0x62, 0x01, 0, 0x80, 2, 0, 0, 0x80, 0, 0, 0, 0x80, 7, 0, 0, 0x80, 0x2c,
            0x01,
        ]);
        let mut add = vec![0xf9, 0x02, 0x01, 0x00, 250];
        add.extend_from_slice(&payload[..250]);
        let mut last = vec![0xf9, 0x02, 0x02, 0x00, 250];
        last.extend_from_slice(&payload[250..]);
        last.extend_from_slice(&proof);
        let mut signature = vec![0];
        signature.extend_from_slice(&[0xef; 64]);
        let mut reply = signature.clone();
        reply.extend_from_slice(&[0x90, 0x00]);
        let exchanges = Arc::new(Mutex::new(VecDeque::from([
            (init, vec![0x90, 0x00]),
            (add, vec![0x90, 0x00]),
            (last, reply),
        ])));
        let device = LedgerDevice::with_transport(ScriptedTransport {
            exchanges: Arc::clone(&exchanges),
        });
        assert_eq!(device.sign(2, 7, &payload, &proof).unwrap(), signature);
        assert!(exchanges.lock().unwrap().is_empty());
    }

    #[test]
    fn with_transport_maps_user_refusal() {
        let exchanges = Arc::new(Mutex::new(VecDeque::from([(
            vec![0xf9, 0x00, 0x00, 0x00, 0x00],
            vec![0x69, 0x86],
        )])));
        let device = LedgerDevice::with_transport(ScriptedTransport { exchanges });
        assert_eq!(
            device.app_version().unwrap_err().to_string(),
            "device error: the request was rejected on the device"
        );
    }

    #[test]
    fn bip44_path_matches_generic_app_layout() {
        let path = bip44_path(0, 0);
        // 44' | 354' | 0' | 0' | 0', little-endian words.
        assert_eq!(&path[0..4], &[44, 0, 0, 0x80]);
        assert_eq!(&path[4..8], &[0x62, 0x01, 0, 0x80]);
        assert_eq!(&path[8..12], &[0, 0, 0, 0x80]);
        assert_eq!(&path[12..16], &[0, 0, 0, 0x80]);
        assert_eq!(&path[16..20], &[0, 0, 0, 0x80]);

        let path = bip44_path(2, 7);
        assert_eq!(&path[8..12], &[2, 0, 0, 0x80]);
        assert_eq!(&path[16..20], &[7, 0, 0, 0x80]);
    }

    #[test]
    fn device_error_formats() {
        assert_eq!(
            device_error(0x6D00, b""),
            "device returned 0x6d00 (instruction not supported (is the Polkadot app open?))"
        );
        assert!(device_error(0x1234, b"boom").contains("boom"));
    }
}
