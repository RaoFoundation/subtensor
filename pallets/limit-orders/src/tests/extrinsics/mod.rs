#![allow(clippy::indexing_slicing, unused_imports)]
//! Integration tests for `pallet-limit-orders` extrinsics, split by concept.

pub(crate) use codec::Encode;
pub(crate) use frame_support::{BoundedVec, assert_noop, assert_ok};
pub(crate) use sp_core::Pair;
pub(crate) use sp_keyring::Sr25519Keyring as AccountKeyring;
pub(crate) use sp_runtime::{DispatchError, Perbill};
pub(crate) use subtensor_runtime_common::NetUid;

pub(crate) use crate::{
    Error, Order, OrderSide, OrderStatus, OrderType, Orders, VersionedOrder, pallet::Event,
};

pub(crate) type LimitOrders = crate::pallet::Pallet<Test>;

pub(crate) use super::mock::*;

/// Check that a specific pallet event was emitted.
pub(crate) fn assert_event(event: Event<Test>) {
    assert!(
        System::events()
            .iter()
            .any(|r| r.event == RuntimeEvent::LimitOrders(event.clone())),
        "expected event not found: {event:?}",
    );
}

/// Build a signed order with a specific `max_slippage` value.
#[allow(clippy::too_many_arguments)]
pub(crate) fn make_signed_order_with_slippage(
    keyring: AccountKeyring,
    hotkey: AccountId,
    netuid: subtensor_runtime_common::NetUid,
    order_type: OrderType,
    amount: u64,
    limit_price: u64,
    expiry: u64,
    fee_rate: sp_runtime::Perbill,
    fee_recipient: AccountId,
    max_slippage: Option<sp_runtime::Perbill>,
) -> crate::SignedOrder<AccountId> {
    let order = crate::VersionedOrder::V1(crate::Order {
        signer: keyring.to_account_id(),
        hotkey,
        netuid,
        order_type,
        amount,
        limit_price,
        expiry,
        fee_rate,
        fee_recipient,
        relayer: None,
        max_slippage,
        chain_id: 945,
        partial_fills_enabled: false,
    });
    let sig = keyring.pair().sign(&order.encode());
    crate::SignedOrder {
        order,
        signature: sp_runtime::MultiSignature::Sr25519(sig),
        partial_fill: None,
    }
}

mod cancel_order;
mod execute_batched_orders;
mod execute_batched_orders_fee_routing;
mod execute_batched_orders_swap_errors;
mod execute_orders;
mod execute_orders_skip_invalid;
mod max_slippage;
mod pallet_status;
mod partial_fill;
mod relayer;
mod simulate_partial_fill;
