//! Read-only views of the hub for clients.

use sp_runtime::DispatchError;
use subtensor_runtime_common::{NetUid, rails::BridgeId};

use crate::{Destination, NativeAsset, SpokeAsset};

sp_api::decl_runtime_apis! {
    pub trait RailsApi<AccountId> where AccountId: codec::Codec {
        /// What an order of `amount` of `source` from the spoke of `bridge` would deliver
        /// to `destination` now, or why the hub would refund or drop it.
        fn quote_order(
            bridge: BridgeId,
            source: SpokeAsset,
            amount: u64,
            destination: Destination<AccountId>,
        ) -> Result<u64, DispatchError>;

        /// What `who` paying `amount` of its `source` through `Rails.bridge_out` would
        /// deliver to `destination` on the spoke of `bridge` now, or why the call would fail.
        fn quote_bridge_out(
            who: AccountId,
            bridge: BridgeId,
            source: NativeAsset,
            amount: u64,
            destination: Destination<AccountId>,
        ) -> Result<u64, DispatchError>;

        /// The alpha that one share of the vault of `netuid` is worth now, times 1e18.
        /// `None` if the subnet has no market or its market only winds down.
        fn alpha_rate(netuid: NetUid) -> Option<u128>;
    }
}
