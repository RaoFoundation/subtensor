//! Read-only loan quotes use exactly the arithmetic of the dispatchables.

#![cfg_attr(not(feature = "std"), no_std)]

use codec::Codec;
use pallet_lending::{ClosingQuote, OpeningQuote, RaiseQuote, Side};
use sp_runtime::DispatchError;

sp_api::decl_runtime_apis! {
    #[api_version(6)]
    pub trait LendingRuntimeApi<AccountId> where AccountId: Codec {
        /// Full opening principal and fixed collateral interest, including swap fees.
        fn quote_open(netuid: u16, side: Side, collateral: u64) -> Result<OpeningQuote, DispatchError>;

        /// Additional principal and coupon after checking the owner's combined position.
        /// `source_hotkey` is required for Long and must be `None` for Short.
        fn quote_open_for(owner: AccountId, netuid: u16, side: Side, collateral: u64, source_hotkey: Option<AccountId>) -> Result<OpeningQuote, DispatchError>;

        /// Exactly the requested additional debt, with its actual fixed coupon.
        /// `source_hotkey` is required for Long and must be `None` for Short.
        #[allow(clippy::too_many_arguments)]
        fn quote_open_exact_for(owner: AccountId, netuid: u16, side: Side, collateral: u64, source_hotkey: Option<AccountId>, principal: u64) -> Result<OpeningQuote, DispatchError>;

        /// Full close payment and collateral refund after interest accrued to this block.
        /// `hotkey` may be `None` to skip the hotkey-existence check (escrow TAO
        /// paths never need it; alpha repay/refund paths still should pass one).
        fn quote_close(owner: AccountId, netuid: u16, hotkey: Option<AccountId>, repay_from_wallet: bool) -> Result<ClosingQuote, DispatchError>;

        /// Atomic raise quote: escrowed long sizing plus optional unlocked-alpha sale.
        #[allow(clippy::too_many_arguments)]
        fn quote_raise(
            owner: AccountId,
            netuid: u16,
            hotkey: AccountId,
            amount: Option<u64>,
            collateral: Option<u64>,
            sell: Option<u64>,
        ) -> Result<RaiseQuote, DispatchError>;
    }
}
