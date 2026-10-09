//! Read-only loan quotes use exactly the arithmetic of the dispatchables.

#![cfg_attr(not(feature = "std"), no_std)]

use codec::Codec;
use pallet_lending::{ClosingQuote, OpeningQuote, Side};
use sp_runtime::DispatchError;

sp_api::decl_runtime_apis! {
    #[api_version(2)]
    pub trait LendingRuntimeApi<AccountId> where AccountId: Codec {
        /// Full opening principal and fixed collateral interest, including swap fees.
        fn quote_open(netuid: u16, side: Side, collateral: u64) -> Result<OpeningQuote, DispatchError>;

        /// Additional principal and coupon after checking the owner's combined position.
        fn quote_open_for(owner: AccountId, netuid: u16, side: Side, collateral: u64, hotkey: AccountId) -> Result<OpeningQuote, DispatchError>;

        /// Full close payment and collateral refund after interest accrued to this block.
        fn quote_close(owner: AccountId, netuid: u16, repay_from_wallet: bool) -> Result<ClosingQuote, DispatchError>;
    }
}
