//! v2 order payload: an order may be sized as a fraction of another order's
//! already-produced output, and may opt in to recording its own output so a
//! later order can draw against it.

use alloc::{format, string::String};

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use frame_support::{BoundedVec, traits::ConstU32};
use scale_info::TypeInfo;
use sp_core::{H256, hexdisplay::HexDisplay};
use sp_runtime::Perbill;
use subtensor_macros::freeze_struct;
use subtensor_runtime_common::NetUid;

use crate::{Order, OrderType};

/// What a provider produced — and therefore what a linked order may spend.
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug,
)]
pub enum LinkedAsset<AccountId> {
    /// Free TAO, produced by a sell.
    Tao,
    /// Alpha staked to `hotkey` on `netuid`, produced by a buy.
    Alpha { netuid: NetUid, hotkey: AccountId },
}

/// How much an order trades. Part of the signed payload.
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
    /// Absolute raw amount: TAO for buys, alpha for sells. v1 semantics.
    Fixed(u64),
    /// `pct` of the output recorded for `provider`. Drawing removes the record.
    LinkedPercentage { provider: H256, pct: Perbill },
}

impl OrderAmount {
    pub fn is_linked(&self) -> bool {
        matches!(self, OrderAmount::LinkedPercentage { .. })
    }

    pub fn fixed(&self) -> Option<u64> {
        match self {
            OrderAmount::Fixed(amount) => Some(*amount),
            OrderAmount::LinkedPercentage { .. } => None,
        }
    }

    pub fn linked(&self) -> Option<(H256, Perbill)> {
        match self {
            OrderAmount::LinkedPercentage { provider, pct } => Some((*provider, *pct)),
            OrderAmount::Fixed(_) => None,
        }
    }

    /// Clear-signing rendering. `Fixed(n)` is bare digits (v1-identical);
    /// linked amounts always carry the ` ppb of order 0x… output` suffix.
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

/// v2 signed payload. Field-for-field v1 plus [`OrderAmount`] and `has_linked_order`.
#[allow(clippy::multiple_bound_locations)]
#[freeze_struct("71aea78239e92aa7")]
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug,
)]
pub struct OrderV2<AccountId: Encode + Decode + TypeInfo + MaxEncodedLen + Clone> {
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
    /// When true, this order's post-fee output is recorded so a later linked
    /// order can draw against it. Mutually exclusive with partial fills.
    pub has_linked_order: bool,
}

/// Single-use record of a provider's post-fee output.
#[allow(clippy::multiple_bound_locations)]
#[freeze_struct("8e774441a7370a3f")]
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug,
)]
pub struct LinkedOutput<AccountId: Encode + Decode + TypeInfo + MaxEncodedLen + Clone> {
    pub signer: AccountId,
    pub asset: LinkedAsset<AccountId>,
    pub total: u64,
    pub expires_at: u64,
}

/// Version-agnostic projection used by every validation/execution path.
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
