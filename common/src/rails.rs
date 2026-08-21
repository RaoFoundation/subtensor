//! Shared types for the canonical rails: the USD -> TAO -> alpha settlement
//! pipeline (tUSD ledger, PSM, canonical pool, gateway envelope wire format).
//!
//! The [`GatewayEnvelope`] byte format is the only cross-chain wire contract:
//! clients (SDK / btcli) SCALE-encode an envelope, bridge message bodies carry
//! it opaquely through Solidity, and the runtime decodes it in
//! `gateway_execute`. Solidity never parses envelope internals, so this module
//! is the single source of truth for the format.
//!
//! Versioning: the first byte of the wire format is a version tag. New
//! envelope layouts get a new version byte and a new decoder arm; existing
//! versions are never reinterpreted.

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use frame_support::pallet_prelude::*;
use scale_info::TypeInfo;
use serde::{Deserialize, Serialize};
use sp_runtime::AccountId32;
use sp_runtime::Vec;
use subtensor_macros::freeze_struct;

use crate::currency::AlphaBalance;
use crate::NetUid;

/// Version tag for [`GatewayEnvelope`] wire encoding.
pub const ENVELOPE_VERSION_V1: u8 = 1;

/// Number of decimals used by every rails asset on every chain (matches rao).
pub const RAILS_DECIMALS: u8 = 9;

/// Identifier of an external USD-like asset registered in the PSM
/// (peg stability module) registry.
pub type UsdAssetId = u32;

/// Canonical asset identifier used across runtime, precompiles, SDK and CLI.
///
/// Non-exhaustive with explicit codec indices: new variants can be appended
/// without a storage migration and without disturbing existing encodings.
#[non_exhaustive]
#[derive(
    Deserialize,
    Serialize,
    Clone,
    Copy,
    Decode,
    DecodeWithMemTracking,
    Encode,
    Eq,
    MaxEncodedLen,
    PartialEq,
    RuntimeDebug,
    TypeInfo,
)]
pub enum AssetId {
    /// Native TAO.
    #[codec(index = 0)]
    Tao,
    /// Internal USD accounting unit. Never leaves the runtime.
    #[codec(index = 1)]
    TUsd,
    /// Subnet alpha for a given netuid.
    #[codec(index = 2)]
    Alpha(NetUid),
    /// An external USD asset registered in the PSM.
    #[codec(index = 3)]
    Usd(UsdAssetId),
}

/// Action requested by an inbound gateway envelope, executed atomically by
/// the runtime after the deposited funds are secured.
///
/// Action failures never revert the delivery: the fallback credits tUSD to
/// the destination account instead (see [`FallbackReason`]).
#[non_exhaustive]
#[derive(
    Deserialize,
    Serialize,
    Clone,
    Decode,
    DecodeWithMemTracking,
    Encode,
    Eq,
    MaxEncodedLen,
    PartialEq,
    RuntimeDebug,
    TypeInfo,
)]
pub enum GatewayAction {
    /// Credit tUSD to the destination account and stop. Also the fallback
    /// outcome of every failed deposit-side action.
    #[codec(index = 0)]
    CreditTUsd,
    // index 1 retired: SwapToTao (hub-side product, cut).
    /// Swap for TAO, then stake into `netuid` under `hotkey`.
    #[codec(index = 2)]
    Stake {
        netuid: NetUid,
        hotkey: AccountId32,
        min_alpha: AlphaBalance,
    },
    // index 3 retired: ReleaseAlpha (unwrap-to-hub, cut).
    /// Buy canonical shares: USD deposit -> pool swap -> stake into the hub
    /// escrow -> mint shares (at the current index) to `recipient` on
    /// `domain`. The envelope `amount` is the USD amount.
    #[codec(index = 4)]
    BuyShares {
        netuid: NetUid,
        /// EVM address receiving the share mint on the spoke chain.
        recipient: [u8; 20],
        min_alpha: AlphaBalance,
        /// Hyperlane domain of the spoke chain (where the shares mint).
        domain: u32,
    },
    /// Sell canonical shares that were already burned on the spoke: unstake
    /// escrowed alpha -> pool swap -> release `usd_asset` from PSM reserves
    /// to `recipient` on `domain`. The envelope `amount` is the share count.
    #[codec(index = 5)]
    SellShares {
        netuid: NetUid,
        /// EVM address receiving the released USD on the spoke chain.
        recipient: [u8; 20],
        /// PSM asset to release reserves from.
        usd_asset: UsdAssetId,
        min_usd: u64,
        /// Hyperlane domain of the spoke chain (where the USD releases).
        domain: u32,
    },
}

/// The v1 gateway envelope: the payload carried in bridge message bodies.
#[freeze_struct("73fb42f814f9019d")]
#[derive(
    Deserialize,
    Serialize,
    Clone,
    Decode,
    DecodeWithMemTracking,
    Encode,
    Eq,
    MaxEncodedLen,
    PartialEq,
    RuntimeDebug,
    TypeInfo,
)]
pub struct GatewayEnvelope {
    /// Asset being deposited (v1: always `AssetId::Usd(_)`).
    pub asset: AssetId,
    /// Amount in rails units (9 decimals).
    pub amount: u64,
    /// Destination account on the runtime.
    pub dest: AccountId32,
    /// What to do with the deposit once secured.
    pub action: GatewayAction,
    /// Origin-assigned sequential nonce. The runtime keeps a `NextNonce`
    /// counter: an envelope executes only when its nonce equals the counter,
    /// so deliveries are strictly ordered and replays are rejected.
    pub nonce: u64,
}

/// Why an envelope's requested action could not be executed. In every case
/// the deposit was still secured and tUSD was credited to `dest`.
#[non_exhaustive]
#[derive(
    Deserialize,
    Serialize,
    Clone,
    Copy,
    Decode,
    DecodeWithMemTracking,
    Encode,
    Eq,
    MaxEncodedLen,
    PartialEq,
    RuntimeDebug,
    TypeInfo,
)]
pub enum FallbackReason {
    /// The requested action variant is unknown to this runtime version.
    #[codec(index = 0)]
    UnknownAction,
    /// The canonical pool swap failed (e.g. slippage guard).
    #[codec(index = 1)]
    SwapFailed,
    /// The staking leg failed (e.g. bad netuid, staking limits).
    #[codec(index = 2)]
    StakeFailed,
    // index 3 retired: ReleaseFailed (unwrap-to-hub, cut).
    /// The buy-shares pipeline failed after the deposit was secured; tUSD
    /// stays credited to the buyer's mirror account.
    #[codec(index = 4)]
    BuyFailed,
    /// The sell-shares pipeline failed. Shares were already burned on the
    /// spoke; the receipt records the failure for manual follow-up (no tUSD
    /// is credited).
    #[codec(index = 5)]
    SellFailed,
}

/// Terminal record of an executed inbound envelope, stored per nonce.
#[freeze_struct("a2c07a90fc2eddc8")]
#[derive(
    Deserialize,
    Serialize,
    Clone,
    Copy,
    Decode,
    DecodeWithMemTracking,
    Encode,
    Eq,
    MaxEncodedLen,
    PartialEq,
    RuntimeDebug,
    TypeInfo,
)]
pub struct InboundReceipt {
    /// Block at which the envelope was executed.
    pub block: u32,
    /// `None` if the requested action executed; `Some(reason)` if the
    /// fallback path credited tUSD instead.
    pub fallback: Option<FallbackReason>,
}

/// Errors decoding an envelope from wire bytes. These are the only inbound
/// failures that revert delivery (the bridge will retry / funds stay locked
/// at origin).
#[derive(Clone, Copy, Eq, PartialEq, RuntimeDebug)]
pub enum EnvelopeError {
    /// Empty payload.
    Empty,
    /// Unknown version byte.
    UnsupportedVersion(u8),
    /// SCALE decoding of the payload body failed.
    Malformed,
}

impl GatewayEnvelope {
    /// Encode to wire bytes: `[version] ++ SCALE(body)`.
    pub fn to_wire(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.encoded_size().saturating_add(1));
        out.push(ENVELOPE_VERSION_V1);
        self.encode_to(&mut out);
        out
    }

    /// Decode from wire bytes, dispatching on the version byte.
    pub fn from_wire(bytes: &[u8]) -> Result<Self, EnvelopeError> {
        let (version, mut body) = match bytes.split_first() {
            Some((v, rest)) => (*v, rest),
            None => return Err(EnvelopeError::Empty),
        };
        match version {
            ENVELOPE_VERSION_V1 => {
                Self::decode(&mut body).map_err(|_| EnvelopeError::Malformed)
            }
            other => Err(EnvelopeError::UnsupportedVersion(other)),
        }
    }
}

/// A linearly-refilling rate limit window.
///
/// This is the single rate-limit algorithm used on both sides of the bridge:
/// runtime PSM caps here, and the equivalent implementation in the
/// canonical xERC-20 contracts. `used` decays by `refill_per_block` for every
/// elapsed block; reservations fail once `used` would exceed `limit`.
#[freeze_struct("a957b157945fb6d4")]
#[derive(
    Deserialize,
    Serialize,
    Clone,
    Copy,
    Decode,
    DecodeWithMemTracking,
    Default,
    Encode,
    Eq,
    MaxEncodedLen,
    PartialEq,
    RuntimeDebug,
    TypeInfo,
)]
pub struct RateWindow {
    /// Maximum outstanding amount inside the window.
    pub limit: u64,
    /// Amount currently consumed (after decay).
    pub used: u64,
    /// Linear decay of `used` per block.
    pub refill_per_block: u64,
    /// Block at which `used` was last updated.
    pub last_update_block: u32,
}

impl RateWindow {
    /// A window with a fixed limit and refill rate and nothing consumed.
    pub fn new(limit: u64, refill_per_block: u64) -> Self {
        Self {
            limit,
            used: 0,
            refill_per_block,
            last_update_block: 0,
        }
    }

    /// Decay `used` according to blocks elapsed since the last update.
    pub fn refresh(&mut self, now: u32) {
        let elapsed = u64::from(now.saturating_sub(self.last_update_block));
        let refill = elapsed.saturating_mul(self.refill_per_block);
        self.used = self.used.saturating_sub(refill);
        self.last_update_block = now;
    }

    /// Headroom available at block `now` without mutating the window.
    pub fn available(&self, now: u32) -> u64 {
        let mut probe = *self;
        probe.refresh(now);
        probe.limit.saturating_sub(probe.used)
    }

    /// Atomically consume `amount` of headroom. Returns `false` (and leaves
    /// `used` untouched apart from decay) if the window lacks capacity.
    pub fn try_reserve(&mut self, now: u32, amount: u64) -> bool {
        self.refresh(now);
        match self.used.checked_add(amount) {
            Some(next) if next <= self.limit => {
                self.used = next;
                true
            }
            _ => false,
        }
    }

    /// Release previously reserved headroom (e.g. reserves leaving the PSM).
    pub fn release(&mut self, now: u32, amount: u64) {
        self.refresh(now);
        self.used = self.used.saturating_sub(amount);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_envelope() -> GatewayEnvelope {
        GatewayEnvelope {
            asset: AssetId::Usd(0),
            amount: 1_000_000_000_000,
            dest: AccountId32::new([7u8; 32]),
            action: GatewayAction::Stake {
                netuid: NetUid::from(64),
                hotkey: AccountId32::new([9u8; 32]),
                min_alpha: AlphaBalance::from(1),
            },
            nonce: 42,
        }
    }

    #[test]
    fn envelope_wire_roundtrip() {
        let env = sample_envelope();
        let wire = env.to_wire();
        assert_eq!(wire.first(), Some(&ENVELOPE_VERSION_V1));
        let decoded = GatewayEnvelope::from_wire(&wire).expect("roundtrip");
        assert_eq!(decoded, env);
    }

    /// Golden wire vectors, mirrored in
    /// `ts-tests/suites/rails/fixtures/golden-envelopes.json` and replayed by
    /// the rails e2e suite: both encoders must produce these exact bytes.
    #[test]
    fn envelope_golden_vectors() {
        let alice = AccountId32::new(
            hex_literal::hex!("d43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d"),
        );
        let hot = AccountId32::new([0x90u8; 32]);
        let evm_recipient: [u8; 20] =
            hex_literal::hex!("f39fd6e51aad88f6f4ce6ab8827279cfffb92266");
        let zero_dest = AccountId32::new([0u8; 32]);

        let cases: [(GatewayEnvelope, &str); 4] = [
            (
                GatewayEnvelope {
                    asset: AssetId::Usd(0),
                    amount: 250_000_000_000,
                    dest: alice.clone(),
                    action: GatewayAction::CreditTUsd,
                    nonce: 7,
                },
                "010300000000004429353a000000d43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d000700000000000000",
            ),
            (
                GatewayEnvelope {
                    asset: AssetId::Usd(0),
                    amount: 1_000_000_000_000,
                    dest: alice,
                    action: GatewayAction::Stake {
                        netuid: NetUid::from(64),
                        hotkey: hot,
                        min_alpha: AlphaBalance::from(1u64),
                    },
                    nonce: 42,
                },
                "0103000000000010a5d4e8000000d43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d024000909090909090909090909090909090909090909090909090909090909090909001000000000000002a00000000000000",
            ),
            (
                GatewayEnvelope {
                    asset: AssetId::Usd(0),
                    amount: 500_000_000_000,
                    dest: zero_dest.clone(),
                    action: GatewayAction::BuyShares {
                        netuid: NetUid::from(64),
                        recipient: evm_recipient,
                        min_alpha: AlphaBalance::from(1u64),
                        domain: 8453,
                    },
                    nonce: 0,
                },
                "0103000000000088526a740000000000000000000000000000000000000000000000000000000000000000000000044000f39fd6e51aad88f6f4ce6ab8827279cfffb922660100000000000000052100000000000000000000",
            ),
            (
                GatewayEnvelope {
                    asset: AssetId::Alpha(NetUid::from(64)),
                    amount: 2_000_000_000,
                    dest: zero_dest,
                    action: GatewayAction::SellShares {
                        netuid: NetUid::from(64),
                        recipient: evm_recipient,
                        usd_asset: 0,
                        min_usd: 1_000_000_000,
                        domain: 8453,
                    },
                    nonce: 1,
                },
                "0102400000943577000000000000000000000000000000000000000000000000000000000000000000000000054000f39fd6e51aad88f6f4ce6ab8827279cfffb922660000000000ca9a3b00000000052100000100000000000000",
            ),
        ];

        for (envelope, expected_hex) in cases {
            assert_eq!(
                hex::encode(envelope.to_wire()),
                expected_hex,
                "wire mismatch for {envelope:?}"
            );
        }
    }

    #[test]
    fn envelope_rejects_unknown_version() {
        let mut wire = sample_envelope().to_wire();
        if let Some(first) = wire.first_mut() {
            *first = 99;
        }
        assert_eq!(
            GatewayEnvelope::from_wire(&wire),
            Err(EnvelopeError::UnsupportedVersion(99))
        );
    }

    #[test]
    fn envelope_rejects_empty_and_malformed() {
        assert_eq!(GatewayEnvelope::from_wire(&[]), Err(EnvelopeError::Empty));
        assert_eq!(
            GatewayEnvelope::from_wire(&[ENVELOPE_VERSION_V1, 0xFF]),
            Err(EnvelopeError::Malformed)
        );
    }

    #[test]
    fn rate_window_reserve_and_refill() {
        let mut w = RateWindow::new(100, 10);
        assert!(w.try_reserve(0, 100));
        assert!(!w.try_reserve(0, 1));
        // 5 blocks refill 50 units.
        assert_eq!(w.available(5), 50);
        assert!(w.try_reserve(5, 50));
        assert!(!w.try_reserve(5, 1));
        // Full refill after long idle; never exceeds limit.
        assert_eq!(w.available(1_000), 100);
    }

    #[test]
    fn rate_window_release() {
        let mut w = RateWindow::new(100, 0);
        assert!(w.try_reserve(0, 80));
        w.release(0, 30);
        assert_eq!(w.available(0), 50);
        // Releasing more than used saturates to zero used.
        w.release(0, 1_000);
        assert_eq!(w.available(0), 100);
    }
}
