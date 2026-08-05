//! Version 2 of the limit-order payload: **linked orders**.
//!
//! A linked order is sized as a fraction of the output another order *already
//! produced*, rather than as an absolute amount the user has to know at signing
//! time. The driving case is a rotation — *"sell my subnet-7 alpha, then put the
//! TAO that produced into subnet 12"* — and its mirror, *"buy into subnet 7 now,
//! and take profit on exactly the alpha that buy produced"*.
//!
//! Two additions to the v1 payload express the whole mechanism:
//!
//! * [`OrderV2::amount`] is an [`OrderAmount`], which is either
//!   [`OrderAmount::Fixed`] (v1 semantics, an absolute raw amount) or
//!   [`OrderAmount::LinkedPercentage`] (a fraction of a named earlier order's
//!   output).
//! * [`OrderV2::has_linked_order`] declares that this order's output should be
//!   *recorded* so that later linked orders can draw against it. Without the
//!   flag no record is written and nothing can link to the order — so a user
//!   opts in to being a provider, in the payload they sign.
//!
//! The two halves are deliberately independent. An order can be a provider
//! (`has_linked_order = true`) with a `Fixed` amount, a consumer
//! (`LinkedPercentage`) whose own output is not recorded, both at once — which
//! is how chains longer than two legs are built — or neither, which is exactly
//! a v1 order.
//!
//! ## Why the provider is named by `order_id`
//!
//! A consumer names its provider by `blake2_256` over the SCALE-encoded
//! `VersionedOrder`, i.e. what `Pallet::derive_order_id` returns and what the
//! user can compute off-chain before signing. Anything weaker leaves a
//! substitution gap: if the link were positional, or pinned only the provider's
//! subnet, a relayer holding several of the user's signed sells could fund a
//! consumer out of the *wrong* one — a forced portfolio rotation that every
//! signature and price bound would still accept. Naming the id makes the
//! reference a singleton.
//!
//! ## A record is drawn exactly once
//!
//! The first linked order to draw against a provider record takes `pct` of the
//! recorded output and the record is **removed**, whatever `pct` was. So a
//! provider funds one linked order, not a basket: `pct` is "spend this much of
//! the proceeds", and `1 - pct` simply stays with the signer as ordinary
//! balance.
//!
//! That is what makes the conservation invariant free. There is no `consumed`
//! counter to keep, no ordering between competing consumers to reason about,
//! and no way for two linked orders naming one provider to collectively draw
//! more than it produced — the second finds no record and fails.
//!
//! Fan-out, if it is ever wanted, is a strictly additive change: reintroduce a
//! drawn-so-far counter and delete the record only when it reaches `total`.
//! Nothing in the payload would have to change.
//!
//! v1 is untouched: `VersionedOrder::V1` still decodes, renders, and verifies
//! exactly as before, so signatures produced before this upgrade remain valid.
//! [`OrderView`] is the version-agnostic projection every execution path works
//! on; v1 projects its `u64` amount to [`OrderAmount::Fixed`] and its
//! `has_linked_order` to `false`, which makes v1 a special case of v2 rather
//! than a second, parallel code path.

use alloc::{format, string::String};

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use frame_support::{BoundedVec, traits::ConstU32};
use scale_info::TypeInfo;
use sp_core::{H256, hexdisplay::HexDisplay};
use sp_runtime::Perbill;
use subtensor_macros::freeze_struct;
use subtensor_runtime_common::NetUid;

use crate::{Order, OrderType};

/// The asset an order produced, and therefore the asset a linked order may
/// consume.
///
/// Alpha is only fungible within a single `(netuid, hotkey)` stake position, so
/// the identity of alpha output carries both. TAO needs no qualifier.
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug,
)]
pub enum LinkedAsset<AccountId> {
    /// Free TAO, produced by a sell (`TakeProfit` / `StopLoss`).
    Tao,
    /// Alpha staked to `hotkey` on `netuid`, produced by a `LimitBuy`.
    Alpha { netuid: NetUid, hotkey: AccountId },
}

/// How much an order trades.
///
/// The distinction is part of the signed payload, so a signature over a fixed-amount
/// order can never be replayed against a linked order (the SCALE encoding and the
/// clear-signing rendering both differ — see [`OrderAmount::render`]).
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
    /// A fraction of the output recorded for the order identified by `provider`.
    ///
    /// The provider must have executed already, in this dispatch or an earlier one,
    /// with `has_linked_order = true` — that flag is what causes its output to be
    /// recorded at all.
    ///
    /// Drawing consumes the whole record: this order takes `pct` of the recorded
    /// output and the record is removed, so a provider funds exactly one linked
    /// order and the unspent `1 - pct` stays with the signer as ordinary balance.
    ///
    /// The provider's output asset must match this order's *input* side: a sell
    /// provider yields TAO, which only a `LimitBuy` can spend; a buy provider yields
    /// alpha on a specific `(netuid, hotkey)`, which only a sell from that same
    /// position can spend.
    ///
    /// `Perbill`'s `Decode` rejects values above `1_000_000_000`, so a decoded
    /// fraction can never exceed 100% of the recorded output.
    LinkedPercentage {
        /// `order_id` of the provider: `blake2_256` over its SCALE-encoded
        /// `VersionedOrder`, i.e. exactly what `Pallet::derive_order_id` returns.
        /// Computable off-chain by the user before signing.
        provider: H256,
        /// Fraction of the provider's recorded output to consume.
        pct: Perbill,
    },
}

impl OrderAmount {
    /// `true` when this amount must be resolved against a provider's recorded output.
    pub fn is_linked(&self) -> bool {
        matches!(self, OrderAmount::LinkedPercentage { .. })
    }

    /// The absolute amount for an [`OrderAmount::Fixed`]; `None` for a linked amount.
    pub fn fixed(&self) -> Option<u64> {
        match self {
            OrderAmount::Fixed(amount) => Some(*amount),
            OrderAmount::LinkedPercentage { .. } => None,
        }
    }

    /// The `(provider, pct)` pair for an [`OrderAmount::LinkedPercentage`]; `None` for
    /// a fixed amount.
    pub fn linked(&self) -> Option<(H256, Perbill)> {
        match self {
            OrderAmount::LinkedPercentage { provider, pct } => Some((*provider, *pct)),
            OrderAmount::Fixed(_) => None,
        }
    }

    /// Canonical single-line, printable-ASCII rendering for the clear-signing message.
    ///
    /// A pure function of the variant, and injective across variants: `Fixed` renders
    /// as bare digits while `LinkedPercentage` always carries the
    /// ` ppb of order 0x… output` suffix, so no fixed amount can ever render
    /// identically to a linked one.
    ///
    /// The provider id is rendered in full — 64 lowercase hex characters — because a
    /// truncated or descriptive reference would reintroduce exactly the substitution
    /// gap the id exists to close. It is tool-verifiable rather than human-verifiable,
    /// and it costs nothing in the signing path: the readable payload is already past
    /// [`crate::LEDGER_MAX_SIGN_SIZE`] and therefore hashed on-device regardless.
    pub fn render(&self) -> String {
        match self {
            OrderAmount::Fixed(amount) => format!("{amount}"),
            OrderAmount::LinkedPercentage { provider, pct } => format!(
                "{} ppb of order 0x{} output",
                pct.deconstruct(),
                HexDisplay::from(&provider.0),
            ),
        }
    }
}

/// The v2 canonical order payload that users sign off-chain.
///
/// Field-for-field identical to [`crate::Order`] except that `amount` is an
/// [`OrderAmount`] and `has_linked_order` is new. Only its H256 hash is stored
/// on-chain; the full struct is submitted by the relayer at execution time (or by the
/// user at cancellation time).
#[allow(clippy::multiple_bound_locations)] // bounds on AccountId required by FRAME derives
#[freeze_struct("ece70ed49d6d8357")]
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
    /// Input amount, either fixed or a fraction of an earlier order's recorded
    /// output. TAO for Buy, alpha for Sell.
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
    /// Ignored, and any submitted partial fill rejected, whenever the order takes
    /// part in a link at all — either because `amount` is
    /// [`OrderAmount::LinkedPercentage`] or because `has_linked_order` is set. A
    /// linked order's total is derived rather than signed, and a provider filled in
    /// instalments would record an output that depends on how the relayer sliced it.
    pub partial_fills_enabled: bool,
    /// Whether this order's output should be recorded so that later linked orders
    /// can draw against it.
    ///
    /// `true` writes a provider record keyed by this order's `order_id`, holding
    /// the output the execution produced and a prune deadline. `false` records
    /// nothing, and no order can ever link to this one.
    ///
    /// This is a *signed authorisation*, not a hint: it is the user declaring that
    /// the proceeds of this order may be spent by the linked order they signed
    /// alongside it. v1 orders project to `false` and can never be providers.
    ///
    /// Mutually exclusive with partial fills — see `partial_fills_enabled`.
    pub has_linked_order: bool,
}

/// A provider order's recorded output — the denominator the one linked order sized
/// against it resolves through.
///
/// A record is single-use: the linked order that draws against it takes `pct` of
/// `total` and the record is deleted. There is deliberately no drawn-so-far
/// counter, because there is never a second draw to check one against.
///
/// This is **accounting, not custody**. The output was credited to `signer` by the
/// provider's own execution, exactly as an unlinked order would have credited it;
/// the record only caps how much of it a linked order is authorised to spend. A
/// consumer that draws against the record still pays out of the signer's own
/// balance, so spending the proceeds elsewhere first makes the consumer fail on
/// funds rather than on this cap.
#[allow(clippy::multiple_bound_locations)] // bounds on AccountId required by FRAME derives
#[freeze_struct("f77c740885be5f1d")]
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug,
)]
pub struct LinkedOutput<AccountId: Encode + Decode + TypeInfo + MaxEncodedLen + Clone> {
    /// The provider's signer. The linked order must be signed by this same coldkey —
    /// the proceeds sit in this account, so a differently-signed consumer would be
    /// spending its own funds against someone else's authorisation.
    pub signer: AccountId,
    /// What the provider produced, and therefore what a consumer's input side must be.
    pub asset: LinkedAsset<AccountId>,
    /// Output the provider produced, post-fee — i.e. the amount that actually landed
    /// with `signer`. A fixed quantity: providers are barred from partial fills, so
    /// the order executes once and this is written once.
    pub total: u64,
    /// Unix timestamp in milliseconds after which anyone may prune this record, and
    /// past which no consumer may draw against it.
    pub expires_at: u64,
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
    pub has_linked_order: bool,
}

impl<AccountId: Encode + Decode + TypeInfo + MaxEncodedLen + Clone> OrderView<AccountId> {
    /// Project a v1 payload. Its `u64` amount becomes [`OrderAmount::Fixed`] and it is
    /// never a provider — precisely v1's semantics, which knew nothing of linking.
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
            has_linked_order: false,
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
            has_linked_order: order.has_linked_order,
        }
    }

    /// The asset this order *spends*. A buy spends TAO; a sell spends alpha from the
    /// `(netuid, hotkey)` position it unstakes from.
    ///
    /// A linked order may only draw against a provider record whose asset equals this.
    pub fn input_asset(&self) -> LinkedAsset<AccountId> {
        if self.order_type.is_buy() {
            LinkedAsset::Tao
        } else {
            LinkedAsset::Alpha {
                netuid: self.netuid,
                hotkey: self.hotkey.clone(),
            }
        }
    }

    /// The asset this order *produces* — the mirror of [`Self::input_asset`]. A buy
    /// produces alpha on its `(netuid, hotkey)`; a sell produces TAO.
    ///
    /// This is what a provider record is stamped with.
    pub fn output_asset(&self) -> LinkedAsset<AccountId> {
        if self.order_type.is_buy() {
            LinkedAsset::Alpha {
                netuid: self.netuid,
                hotkey: self.hotkey.clone(),
            }
        } else {
            LinkedAsset::Tao
        }
    }
}
