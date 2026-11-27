//! Shared types of the canonical rails: the bridges that mint tUSD, the identities that pay
//! from other chains, and the one rate-limit algorithm every rail uses.
//!
//! Every enum here is append-only: a variant keeps its codec index for ever, and consumers
//! must tolerate variants they do not know.

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use frame_support::{pallet_prelude::*, weights::Weight};
use scale_info::TypeInfo;
use sp_core::H160;
use sp_runtime::{DispatchError, Vec};
use subtensor_macros::freeze_struct;

use crate::{AlphaBalance, GammaBalance, NetUid, TaoBalance};

/// Identifies an approved bridge: one portal contract on one origin chain.
pub type BridgeId = u32;

/// The chain a payment was authorized on.
#[non_exhaustive]
#[derive(
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
pub enum OriginChain {
    /// A native Bittensor extrinsic.
    #[codec(index = 0)]
    Bittensor,
    /// Bittensor's own EVM.
    #[codec(index = 1)]
    BittensorEvm,
    /// An external EVM chain, by EIP-155 chain id (Base is 8453, Ethereum is 1).
    #[codec(index = 2)]
    Evm(u64),
    /// An external SVM chain, by transport domain id.
    #[codec(index = 3)]
    Svm(u32),
}

/// Who paid: the credential a service credits and later authenticates.
#[non_exhaustive]
#[derive(
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
pub enum Payer<AccountId> {
    /// A Bittensor hotkey.
    #[codec(index = 0)]
    Ss58(AccountId),
    /// A 20-byte Ethereum-style address. One key controls it on every EVM chain.
    #[codec(index = 1)]
    Evm(H160),
    /// A 32-byte ed25519 public key (Solana and other ed25519 chains).
    #[codec(index = 2)]
    Ed25519([u8; 32]),
    /// A 33-byte compressed secp256k1 public key (Cosmos-style accounts).
    #[codec(index = 3)]
    Cosmos([u8; 33]),
}

/// Who submitted a spend on Bittensor.
#[non_exhaustive]
#[derive(
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
pub enum SpenderKind {
    /// The payer's own hotkey signed the spend.
    #[codec(index = 0)]
    Hotkey,
    /// A protocol bridge relayed a payment authorized on `chain`.
    #[codec(index = 1)]
    Bridge { chain: OriginChain },
    /// A contract on Bittensor's own EVM.
    #[codec(index = 2)]
    EvmContract,
    /// A multisig or proxy held the deposit.
    #[codec(index = 3)]
    Multisig,
}

/// A payment that a bridge relays: who paid, on which chain, and the bridge's sequence number
/// of the payment there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgedPayment<AccountId> {
    pub payer: Payer<AccountId>,
    pub origin_chain: OriginChain,
    pub origin_nonce: u64,
}

/// Whether a netuid holds a subnet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubnetStatus {
    /// A subnet lives there. `generation` changes when the netuid is registered again.
    Live { generation: u64 },
    /// A subnet was dissolved and its stake is still being paid out.
    Dissolving,
    /// No subnet, and nothing left to pay out.
    Absent,
}

/// What the hub of the rails needs from staking. The subtensor pallet implements it; the hub
/// does not depend on that pallet.
pub trait RailsStaking<AccountId> {
    /// Whether `netuid` holds a subnet.
    fn subnet_status(netuid: NetUid) -> SubnetStatus;

    /// The name in the identity the subnet published on chain, if it published one.
    fn subnet_name(netuid: NetUid) -> Option<Vec<u8>>;

    /// Opens the index hotkey of `netuid`, owned by `vault`.
    fn open_index(netuid: NetUid, vault: &AccountId) -> DispatchResult;

    /// The alpha that `vault` has staked at the index hotkey of `netuid`.
    fn staked(vault: &AccountId, netuid: NetUid) -> AlphaBalance;

    /// Stakes `tao` of `vault`'s balance at the index hotkey of `netuid`. Returns the alpha
    /// it bought.
    fn stake(
        vault: &AccountId,
        netuid: NetUid,
        tao: TaoBalance,
    ) -> Result<AlphaBalance, DispatchError>;

    /// Unstakes `alpha` of `vault`'s from the index hotkey of `netuid`. Returns the TAO that
    /// reached `vault`'s balance.
    fn unstake(
        vault: &AccountId,
        netuid: NetUid,
        alpha: AlphaBalance,
    ) -> Result<TaoBalance, DispatchError>;

    /// Spends `tao` of `funder`'s balance as gamma of `netuid` at `service` for the payer of
    /// `payment`, valuing a TAO at no more than `tao_usd_price_cap`. Returns the alpha burned
    /// and the gamma spent.
    fn spend_at_service(
        funder: &AccountId,
        netuid: NetUid,
        service: &AccountId,
        tao: TaoBalance,
        tao_usd_price_cap: u64,
        payment: BridgedPayment<AccountId>,
    ) -> Result<(AlphaBalance, GammaBalance), DispatchError>;

    /// At least the weight of the staking that any one route does: an unstake and then a
    /// stake or a spend.
    fn weight() -> Weight;

    /// Benchmark setup: makes sure a subnet lives and returns its netuid.
    #[cfg(feature = "runtime-benchmarks")]
    fn benchmark_subnet() -> NetUid;
}

/// The stablecoin a bridge locks on its origin chain.
#[non_exhaustive]
#[derive(
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
pub enum StableAsset {
    #[codec(index = 0)]
    Usdc,
    #[codec(index = 1)]
    Usdt,
}

/// A linearly refilling allowance.
///
/// At most `limit` can be consumed at once, and what was consumed is given back evenly over
/// `period` blocks: an empty window is full again after `period` blocks. The contracts on
/// the other side of a bridge implement the same algorithm over seconds.
#[freeze_struct("7774687af4f0bd66")]
#[derive(
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
    /// The most that can be outstanding in the window.
    pub limit: u64,
    /// Blocks an exhausted window takes to refill. Zero behaves as one.
    pub period: u32,
    /// What was consumed, as of `updated_at`.
    pub used: u64,
    /// The block `used` was last brought up to date.
    pub updated_at: u32,
}

impl RateWindow {
    /// A full window.
    pub const fn new(limit: u64, period: u32) -> Self {
        Self {
            limit,
            period,
            used: 0,
            updated_at: 0,
        }
    }

    /// What is still consumed at `now`, after the refill since `updated_at`.
    pub fn used_at(&self, now: u32) -> u64 {
        let elapsed = u128::from(now.saturating_sub(self.updated_at));
        let refill = u128::from(self.limit)
            .saturating_mul(elapsed)
            .checked_div(u128::from(self.period.max(1)))
            .unwrap_or(u128::MAX);
        self.used
            .saturating_sub(u64::try_from(refill).unwrap_or(u64::MAX))
    }

    /// What can be consumed at `now`.
    pub fn available_at(&self, now: u32) -> u64 {
        self.limit.saturating_sub(self.used_at(now))
    }

    /// Consumes `amount` at `now`. Returns `false`, changing nothing, if it does not fit.
    pub fn try_consume(&mut self, now: u32, amount: u64) -> bool {
        let used = self.used_at(now);
        match used.checked_add(amount) {
            Some(next) if next <= self.limit => {
                self.used = next;
                self.updated_at = now;
                true
            }
            _ => false,
        }
    }

    /// The same consumption under new terms, as an administrator sets them at `now`.
    pub fn retargeted(&self, now: u32, limit: u64, period: u32) -> Self {
        Self {
            limit,
            period,
            used: self.used_at(now),
            updated_at: now,
        }
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn window_consumes_up_to_its_limit() {
        let mut window = RateWindow::new(100, 10);
        assert!(window.try_consume(5, 60));
        assert!(window.try_consume(5, 40));
        assert!(!window.try_consume(5, 1));
        assert_eq!(window.available_at(5), 0);
    }

    #[test]
    fn window_refills_linearly_and_never_past_its_limit() {
        let mut window = RateWindow::new(100, 10);
        assert!(window.try_consume(0, 100));
        assert_eq!(window.available_at(1), 10);
        assert_eq!(window.available_at(5), 50);
        assert_eq!(window.available_at(10), 100);
        assert_eq!(window.available_at(u32::MAX), 100);
    }

    #[test]
    fn failed_consume_changes_nothing() {
        let mut window = RateWindow::new(100, 10);
        assert!(window.try_consume(0, 100));
        let before = window;
        assert!(!window.try_consume(3, 31));
        assert_eq!(window, before);
        assert!(window.try_consume(3, 30));
    }

    #[test]
    fn window_does_not_overflow() {
        let mut window = RateWindow::new(u64::MAX, 1);
        assert!(window.try_consume(0, u64::MAX));
        assert!(!window.try_consume(0, 1));
        assert_eq!(window.available_at(u32::MAX), u64::MAX);
    }

    #[test]
    fn zero_period_refills_every_block() {
        let mut window = RateWindow::new(100, 0);
        assert!(window.try_consume(7, 100));
        assert!(!window.try_consume(7, 1));
        assert!(window.try_consume(8, 100));
    }

    #[test]
    fn zero_limit_admits_nothing() {
        let mut window = RateWindow::new(0, 10);
        assert!(!window.try_consume(1, 1));
        assert!(window.try_consume(1, 0));
    }

    #[test]
    fn retargeting_keeps_consumption() {
        let mut window = RateWindow::new(100, 10);
        assert!(window.try_consume(0, 100));
        // Half refilled, then the limit drops below what is still consumed.
        let lowered = window.retargeted(5, 20, 10);
        assert_eq!(lowered.used, 50);
        assert_eq!(lowered.available_at(5), 0);
        // 20 per 10 blocks: the excess clears at the new rate.
        assert_eq!(lowered.available_at(20), 0);
        assert_eq!(lowered.available_at(30), 20);
    }

    #[test]
    fn codec_indices_are_pinned() {
        assert_eq!(OriginChain::Bittensor.encode(), vec![0]);
        assert_eq!(OriginChain::BittensorEvm.encode(), vec![1]);
        assert_eq!(OriginChain::Evm(8453).encode()[0], 2);
        assert_eq!(OriginChain::Svm(1).encode()[0], 3);
        assert_eq!(Payer::<u64>::Ss58(1).encode()[0], 0);
        assert_eq!(Payer::<u64>::Evm(H160::zero()).encode()[0], 1);
        assert_eq!(Payer::<u64>::Ed25519([0; 32]).encode()[0], 2);
        assert_eq!(Payer::<u64>::Cosmos([0; 33]).encode()[0], 3);
        assert_eq!(SpenderKind::Hotkey.encode(), vec![0]);
        assert_eq!(
            SpenderKind::Bridge {
                chain: OriginChain::Evm(1)
            }
            .encode()[0],
            1
        );
        assert_eq!(SpenderKind::EvmContract.encode(), vec![2]);
        assert_eq!(SpenderKind::Multisig.encode(), vec![3]);
        assert_eq!(StableAsset::Usdc.encode(), vec![0]);
        assert_eq!(StableAsset::Usdt.encode(), vec![1]);
    }
}
