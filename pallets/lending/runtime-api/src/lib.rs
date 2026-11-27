//! Read-only runtime API of the lending pallet (`pallet_lending` DESIGN-v2 section 8). The view
//! types are the pallet's own (`pallet_lending::views`); nothing here writes state.

#![cfg_attr(not(feature = "std"), no_std)]

use codec::Codec;
use pallet_lending::{
    types::{Position, Side},
    views::{HeldPosition, MarketView, ParamsView, ShortCloseQuote, ShortGrowQuote},
};
use sp_runtime::DispatchError;
use sp_std::vec::Vec;
use subtensor_runtime_common::NetUid;

sp_api::decl_runtime_apis! {
    pub trait LendingRuntimeApi<AccountId, BlockNumber>
    where
        AccountId: Codec,
        BlockNumber: Codec,
    {
        /// The `Enabled` and `LongsEnabled` switches and the four live parameter groups.
        fn params() -> ParamsView;
        /// The position `coldkey` holds on `side` of `netuid`, as stored.
        fn position(coldkey: AccountId, netuid: NetUid, side: Side) -> Option<Position<BlockNumber>>;
        /// Every position `coldkey` holds.
        fn positions_of(coldkey: AccountId) -> Vec<HeldPosition<BlockNumber>>;
        /// The pool, the books (`SyntheticOutstanding` and the long books) and the price
        /// references of `netuid`; `None` for a subnet without price rings.
        fn market(netuid: NetUid) -> Option<MarketView>;
        /// What `open_short(netuid, q, ..)` would do now, or the error it would fail with.
        fn quote_open_short(netuid: NetUid, q: u64) -> Result<ShortGrowQuote, DispatchError>;
        /// What `increase_short(netuid, q, ..)` on the short of `coldkey` would do now.
        fn quote_increase_short(
            coldkey: AccountId,
            netuid: NetUid,
            q: u64,
        ) -> Result<ShortGrowQuote, DispatchError>;
        /// What `close_short(netuid, alpha, ..)` on the short of `coldkey` would do now.
        fn quote_close_short(
            coldkey: AccountId,
            netuid: NetUid,
            alpha: u64,
        ) -> Result<ShortCloseQuote, DispatchError>;
    }
}
