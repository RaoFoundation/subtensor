//! Version 2 of the limit-order payload.
//!
//! [`OrderV2`] keeps every field of [`crate::Order`] (v1) with identical meaning and
//! changes exactly one thing: the traded amount is an [`OrderAmount`] rather than a
//! bare `u64`. A user can therefore size an order either as
//!
//! * [`OrderAmount::Fixed`] — an absolute raw amount, exactly the v1 semantics, or
//! * [`OrderAmount::Percentage`] — a fraction of the signer's balance on the order's
//!   *input* side, resolved against on-chain state at execution time.
//!
//! Percentage sizing exists so a user can sign "sell all my alpha on subnet 7 if the
//! price falls below X" without knowing, at signing time, how much alpha they will
//! actually hold when the trigger fires.
//!
//! v1 is untouched: `VersionedOrder::V1` still decodes, renders, and verifies exactly
//! as before, so signatures produced before this upgrade remain valid.
//!
//! [`OrderView`] is the version-agnostic projection every execution path in the pallet
//! works on. v1 projects its `u64` amount to `OrderAmount::Fixed`, which makes v1 a
//! special case of v2 rather than a second, parallel code path.

use alloc::{format, string::String};

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use frame_support::{BoundedVec, traits::ConstU32};
use scale_info::TypeInfo;
use sp_runtime::Perbill;
use subtensor_macros::freeze_struct;
use subtensor_runtime_common::NetUid;

use crate::{Order, OrderType};

/// How much an order trades.
///
/// The distinction is part of the signed payload, so a signature over a fixed-amount
/// order can never be replayed against a percentage-amount order (the SCALE encoding
/// and the clear-signing rendering both differ — see [`OrderAmount::render`]).
#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    TypeInfo,
    MaxEncodedLen,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
)]
pub enum OrderAmount {
    /// An absolute raw amount: TAO for buys, alpha for sells. Identical in meaning to
    /// `Order::amount` in v1.
    Fixed(u64),
    /// A fraction of the signer's balance on the order's input side, read at
    /// execution time:
    ///
    /// - Buy:  fraction of the signer's transferable TAO balance.
    /// - Sell: fraction of the signer's alpha staked to the order's `hotkey` on the
    ///   order's `netuid` that is currently available to unstake.
    ///
    /// `Perbill`'s `Decode` rejects values above `1_000_000_000`, so a decoded
    /// percentage can never exceed 100% of that balance.
    Percentage(Perbill),
}

impl OrderAmount {
    /// `true` when this amount must be resolved against on-chain balance state.
    pub fn is_percentage(&self) -> bool {
        matches!(self, OrderAmount::Percentage(_))
    }

    /// The absolute amount for a [`OrderAmount::Fixed`]; `None` for a percentage.
    pub fn fixed(&self) -> Option<u64> {
        match self {
            OrderAmount::Fixed(amount) => Some(*amount),
            OrderAmount::Percentage(_) => None,
        }
    }

    /// Resolve to an absolute raw amount against the signer's `balance` on the order's
    /// input side. `Fixed` ignores `balance`; `Percentage` floors, so a fraction of a
    /// small balance can legitimately resolve to zero (rejected by the caller).
    pub fn resolve(&self, balance: u64) -> u64 {
        match self {
            OrderAmount::Fixed(amount) => *amount,
            OrderAmount::Percentage(pct) => pct.mul_floor(balance),
        }
    }

    /// Canonical single-line, printable-ASCII rendering for the clear-signing message.
    ///
    /// A pure function of the variant, and injective across variants: `Fixed` renders
    /// as bare digits while `Percentage` always carries the ` ppb of balance` suffix,
    /// so no fixed amount can ever render identically to a percentage.
    pub fn render(&self) -> String {
        match self {
            OrderAmount::Fixed(amount) => format!("{amount}"),
            OrderAmount::Percentage(pct) => format!("{} ppb of balance", pct.deconstruct()),
        }
    }
}

/// The v2 canonical order payload that users sign off-chain.
///
/// Field-for-field identical to [`crate::Order`] except for `amount`. Only its H256
/// hash is stored on-chain; the full struct is submitted by the relayer at execution
/// time (or by the user at cancellation time).
#[allow(clippy::multiple_bound_locations)] // bounds on AccountId required by FRAME derives
#[freeze_struct("efb643b01ddb053")]
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug,
)]
pub struct OrderV2<AccountId: Encode + Decode + TypeInfo + MaxEncodedLen + Clone> {
    /// The coldkey that authorised this order (pays TAO for buys; owns the
    /// staked alpha for sells).
    pub signer: AccountId,
    /// The hotkey to stake to (buy) or unstake from (sell).
    pub hotkey: AccountId,
    /// Target subnet.
    pub netuid: NetUid,
    /// Order type (LimitBuy, TakeProfit, or StopLoss).
    pub order_type: OrderType,
    /// Input amount, either fixed or a percentage of the signer's input-side
    /// balance. TAO for Buy, alpha for Sell.
    pub amount: OrderAmount,
    /// Price threshold in ×10⁹ scale (same as the `current_alpha_price` RPC endpoint).
    /// A value of `1_000_000_000` represents a price of 1.0 TAO/alpha.
    /// Sub-unity prices (e.g. 0.5 TAO/alpha) are expressed as `500_000_000`.
    /// Buy: maximum acceptable price.  Sell: minimum acceptable price.
    /// `u64::MAX` means no ceiling (buy at any price); `0` means no floor (sell at any price).
    pub limit_price: u64,
    /// Unix timestamp in milliseconds after which this order must not be executed.
    pub expiry: u64,
    /// Fee rate applied to this order's TAO amount (input for buys, output for sells).
    pub fee_rate: Perbill,
    /// Account that receives the fee collected from this order.
    pub fee_recipient: AccountId,
    /// Accounts authorized to relay this order. When set, only an account present
    /// in this list may submit the execution transaction. Supports up to 10 relayers.
    pub relayer: Option<BoundedVec<AccountId, ConstU32<10>>>,
    /// Maximum slippage tolerance in parts per billion applied to `limit_price`
    /// at execution time. `None` = no protection (execute at market).
    /// - Buy:  effective price ceiling = `limit_price + limit_price * max_slippage`
    /// - Sell: effective price floor   = `limit_price - limit_price * max_slippage`
    pub max_slippage: Option<Perbill>,
    /// EVM-compatible chain ID that this order is bound to.
    /// Prevents replay of testnet-signed orders on mainnet and vice versa.
    pub chain_id: u64,
    /// Wether partial fills are enabled.
    ///
    /// Ignored when `amount` is [`OrderAmount::Percentage`]: the resolved amount moves
    /// with the signer's balance, so there is no stable total to fill against and any
    /// submitted partial fill is rejected.
    pub partial_fills_enabled: bool,
}

/// Version-agnostic, owned projection of an order payload.
///
/// Every validation and execution path in the pallet reads this instead of a concrete
/// version, so supporting a new version means adding one projection here rather than
/// branching at each use site. `AccountId` fields are cloned; for `AccountId32` that
/// is a 32-byte copy, and a view is built at most once per order per dispatch.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct OrderView<AccountId> {
    pub signer: AccountId,
    pub hotkey: AccountId,
    pub netuid: NetUid,
    pub order_type: OrderType,
    pub amount: OrderAmount,
    pub limit_price: u64,
    pub expiry: u64,
    pub fee_rate: Perbill,
    pub fee_recipient: AccountId,
    pub relayer: Option<BoundedVec<AccountId, ConstU32<10>>>,
    pub max_slippage: Option<Perbill>,
    pub chain_id: u64,
    pub partial_fills_enabled: bool,
}

impl<AccountId: Encode + Decode + TypeInfo + MaxEncodedLen + Clone> OrderView<AccountId> {
    /// Project a v1 payload. Its `u64` amount becomes [`OrderAmount::Fixed`], which is
    /// precisely v1's semantics — v1 orders never consult a balance.
    pub fn from_v1(order: &Order<AccountId>) -> Self {
        Self {
            signer: order.signer.clone(),
            hotkey: order.hotkey.clone(),
            netuid: order.netuid,
            order_type: order.order_type.clone(),
            amount: OrderAmount::Fixed(order.amount),
            limit_price: order.limit_price,
            expiry: order.expiry,
            fee_rate: order.fee_rate,
            fee_recipient: order.fee_recipient.clone(),
            relayer: order.relayer.clone(),
            max_slippage: order.max_slippage,
            chain_id: order.chain_id,
            partial_fills_enabled: order.partial_fills_enabled,
        }
    }

    /// Project a v2 payload.
    pub fn from_v2(order: &OrderV2<AccountId>) -> Self {
        Self {
            signer: order.signer.clone(),
            hotkey: order.hotkey.clone(),
            netuid: order.netuid,
            order_type: order.order_type.clone(),
            amount: order.amount,
            limit_price: order.limit_price,
            expiry: order.expiry,
            fee_rate: order.fee_rate,
            fee_recipient: order.fee_recipient.clone(),
            relayer: order.relayer.clone(),
            max_slippage: order.max_slippage,
            chain_id: order.chain_id,
            partial_fills_enabled: order.partial_fills_enabled,
        }
    }
}
