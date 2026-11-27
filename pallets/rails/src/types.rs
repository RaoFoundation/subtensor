//! The orders the hub settles and the messages it answers with.
//!
//! Every enum is append-only: a variant keeps its codec index for ever.

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use frame_support::{BoundedVec, pallet_prelude::*};
use scale_info::TypeInfo;
use sp_core::{ConstU32, H160};
use sp_runtime::DispatchError;
use subtensor_macros::freeze_struct;
use subtensor_runtime_common::{AlphaBalance, NetUid, rails::BridgeId};

/// The longest name of a token on a spoke, in bytes.
pub const MAX_TOKEN_NAME: u32 = 64;
/// The longest symbol of a token on a spoke, in bytes. Wallets show at most eleven characters.
pub const MAX_TOKEN_SYMBOL: u32 = 11;

/// The name of a token on a spoke.
pub type TokenName = BoundedVec<u8, ConstU32<MAX_TOKEN_NAME>>;
/// The symbol of a token on a spoke.
pub type TokenSymbol = BoundedVec<u8, ConstU32<MAX_TOKEN_SYMBOL>>;

/// An asset that a spoke holds for the hub: what an order pays with, and what a credit pays.
/// All amounts have 9 decimals on the hub, whatever the token has on its chain.
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
pub enum SpokeAsset {
    /// The stablecoin that the spoke's vault locks. The hub mints tUSD against it.
    #[codec(index = 0)]
    Stable,
    /// Canonical TAO: minted on the spoke one for one against TAO in the hub's reserve.
    #[codec(index = 1)]
    Tao,
    /// The alpha token of a subnet: shares of the hub's vault of that subnet's staked alpha.
    #[codec(index = 2)]
    Alpha(NetUid),
}

/// What an order asks for, and who receives it.
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
pub enum Destination<AccountId> {
    /// The stablecoin, released by the vault of the order's spoke.
    #[codec(index = 0)]
    Stable { to: H160 },
    /// Canonical TAO, minted on the order's spoke.
    #[codec(index = 1)]
    Tao { to: H160 },
    /// The alpha token of `netuid`, minted on the order's spoke.
    #[codec(index = 2)]
    Alpha { netuid: NetUid, to: H160 },
    /// Service credits: gamma of `netuid` burned at `service` for `beneficiary`. One way:
    /// the credit belongs to that address at that service and is never redeemed.
    #[codec(index = 3)]
    Credits {
        netuid: NetUid,
        service: AccountId,
        beneficiary: H160,
    },
    /// tUSD, at an account on Bittensor.
    #[codec(index = 4)]
    NativeTusd { to: AccountId },
    /// TAO, at an account on Bittensor.
    #[codec(index = 5)]
    NativeTao { to: AccountId },
}

/// An order from a spoke: its portal has locked or burned `amount` of `source` for `payer`.
#[freeze_struct("a3769629fe6df95f")]
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
pub struct Order<AccountId> {
    /// The portal's sequence number of the order. The hub settles them in sequence.
    pub nonce: u64,
    /// The account on the spoke that paid. A refund goes back to it.
    pub payer: H160,
    /// What the portal locked (the stablecoin) or burned (a canonical token).
    pub source: SpokeAsset,
    /// How much of it, with 9 decimals.
    pub amount: u64,
    /// What the payer wants for it.
    pub destination: Destination<AccountId>,
    /// The least of the destination asset the payer accepts. Less is refunded.
    pub min_out: u64,
    /// Unix time in seconds after which the order is refunded instead of settled.
    pub deadline: u64,
}

/// An asset on Bittensor that an account pays a bridge order with.
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
pub enum NativeAsset {
    #[codec(index = 0)]
    Tusd,
    #[codec(index = 1)]
    Tao,
}

/// How the hub disposed of an order.
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
pub enum Outcome {
    /// Settled: the payer received `amount_out` of the destination asset.
    #[codec(index = 0)]
    Settled { amount_out: u64 },
    /// Not settled, for the reason in `error`. The spoke returns to the payer what it took.
    #[codec(index = 1)]
    Refunded { error: DispatchError },
    /// Dropped: the spoke claims to have burned more than the hub ever let it mint. Nothing
    /// is returned, because nothing the hub issued was taken.
    #[codec(index = 2)]
    Rejected { error: DispatchError },
    /// Passed over by the admin origin without being read.
    #[codec(index = 3)]
    Skipped,
}

/// What became of an order, and when.
#[freeze_struct("671a31c2aed840bb")]
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
pub struct Receipt {
    /// The block the order was disposed of in.
    pub block: u32,
    pub outcome: Outcome,
}

/// What the hub tells a spoke to do.
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
pub enum Message {
    /// Pay `amount` of `asset` to `to`: release the stablecoin or mint the token.
    #[codec(index = 0)]
    Credit {
        asset: SpokeAsset,
        to: H160,
        /// With 9 decimals.
        amount: u64,
        /// The order of this spoke that the credit answers, if one does.
        order: Option<u64>,
        /// Whether the credit returns what `order` took, rather than paying what it bought.
        refund: bool,
        /// For an alpha token: the alpha one share is worth now, times 1e18. Zero otherwise.
        rate: u128,
    },
    /// Create the alpha token of `netuid` at address `token`, with the name and symbol the
    /// hub derived from the subnet's identity on chain.
    #[codec(index = 1)]
    RegisterToken {
        netuid: NetUid,
        token: H160,
        name: TokenName,
        symbol: TokenSymbol,
    },
}

/// A message waiting for the gateway to hand it to the transport.
#[freeze_struct("84b93997a780947d")]
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
pub struct Outbound {
    /// The bridge whose portal receives it.
    pub bridge: BridgeId,
    /// The hub's sequence number of the message for that bridge. A portal acts on each once.
    pub nonce: u64,
    pub message: Message,
}

/// A subnet's market: the vault whose shares its alpha token is.
#[freeze_struct("725910e380ee33d3")]
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
pub struct Market {
    /// Which registration of the netuid the market was opened for. A netuid that is
    /// registered again holds another subnet; this market then only winds down.
    pub generation: u64,
    /// Shares outstanding, on all spokes together.
    pub shares: u64,
    /// The most alpha the vault may hold after a purchase.
    pub cap: AlphaBalance,
}

/// A canonical token on a spoke, as wallets and integrators should know it.
#[freeze_struct("5e3ea321e3864377")]
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
pub struct TokenInfo {
    /// The token contract on the spoke's chain.
    pub address: H160,
    pub name: TokenName,
    pub symbol: TokenSymbol,
}
