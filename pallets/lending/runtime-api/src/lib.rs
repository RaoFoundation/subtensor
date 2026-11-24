//! Read-only short quotes use exactly the arithmetic of the dispatchables.

#![cfg_attr(not(feature = "std"), no_std)]

use codec::Codec;
use pallet_lending::{ShortCloseQuote, ShortHealth, ShortOpenQuote};
use sp_runtime::DispatchError;

sp_api::decl_runtime_apis! {
    #[api_version(9)]
    pub trait LendingRuntimeApi<AccountId> where AccountId: Codec {
        /// Proceeds, required collateral, notional, and interest for minting
        /// and selling `mint` alpha on `netuid`.
        fn quote_open_short(netuid: u16, mint: u64) -> Result<ShortOpenQuote, DispatchError>;

        /// Buy-back cost and refund for closing `alpha` of `owner`'s short.
        fn quote_close_short(owner: AccountId, netuid: u16, alpha: u64) -> Result<ShortCloseQuote, DispatchError>;

        /// Coverage, equity, and accrued interest of `owner`'s short on `netuid`.
        fn short_health(owner: AccountId, netuid: u16) -> Option<ShortHealth>;

        /// Terms version assigned to newly opened positions.
        fn current_terms_version() -> u16;
    }
}
