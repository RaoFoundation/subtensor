#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    unused_imports
)]
//! Unit tests for auxiliary helpers in `pallet-limit-orders`, split by concept.

pub(crate) use codec::Encode;
pub(crate) use frame_support::{BoundedVec, assert_noop, assert_ok, traits::ConstU32};
pub(crate) use sp_core::H256;
pub(crate) use sp_core::Pair;
pub(crate) use sp_keyring::Sr25519Keyring as AccountKeyring;
pub(crate) use sp_runtime::Perbill;
pub(crate) use substrate_fixed::types::U64F64;
pub(crate) use subtensor_runtime_common::NetUid;

pub(crate) use crate::pallet::Pallet as LimitOrders;
pub(crate) use crate::{Error, OrderEntry, OrderSide, OrderStatus, OrderType, Orders};

pub(crate) use super::mock::*;

// Shared OrderEntry builders used by distribute_* / collect_fees tests.
pub(crate) fn make_buy_entry(
    order_id: H256,
    signer: AccountId,
    hotkey: AccountId,
    gross: u64,
    net: u64,
    fee_rate: Perbill,
    fee_recipient: AccountId,
) -> OrderEntry<AccountId> {
    OrderEntry {
        order_id,
        signer,
        hotkey,
        side: OrderType::LimitBuy,
        gross,
        order_amount: gross,
        net,
        fee_rate,
        fee_recipient,
        effective_swap_limit: u64::MAX, // no slippage constraint
        partial_fill: None,
    }
}

pub(crate) fn bounded_buy_entries(
    v: Vec<OrderEntry<AccountId>>,
) -> BoundedVec<OrderEntry<AccountId>, ConstU32<64>> {
    BoundedVec::try_from(v).unwrap()
}

pub(crate) fn bounded_sell_entries(
    v: Vec<OrderEntry<AccountId>>,
) -> BoundedVec<OrderEntry<AccountId>, ConstU32<64>> {
    BoundedVec::try_from(v).unwrap()
}

mod collect_fees;
mod compute_effective_swap_limit;
mod compute_order_status;
mod distribute_alpha_pro_rata;
mod distribute_tao_pro_rata;
mod is_order_valid;
mod net_amount_for_event;
mod validate_and_classify;
mod validate_and_classify_slippage_relayer;
