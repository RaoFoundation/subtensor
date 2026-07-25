//! Runtime API for off-chain TAO↔alpha price and swap simulation queries.
//!
//! Implemented by the node runtime; consumed by the swap RPC crate.
//! Method names and [`SimSwapResult`] / [`SubnetPrice`] layouts are wire-stable.

#![cfg_attr(not(feature = "std"), no_std)]

use frame_support::pallet_prelude::*;
use serde::{Deserialize, Serialize};
use sp_std::vec::Vec;
use subtensor_macros::freeze_struct;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance};

#[freeze_struct("8e70f7cc0b118c6")]
#[derive(Decode, Encode, PartialEq, Eq, Clone, Debug, TypeInfo)]
pub struct SimSwapResult {
    pub tao_amount: TaoBalance,
    pub alpha_amount: AlphaBalance,
    pub tao_fee: TaoBalance,
    pub alpha_fee: AlphaBalance,
    pub tao_slippage: TaoBalance,
    pub alpha_slippage: AlphaBalance,
}

#[freeze_struct("d7bbb761fc2b2eac")]
#[derive(Decode, Deserialize, Encode, PartialEq, Eq, Clone, Debug, Serialize, TypeInfo)]
pub struct SubnetPrice {
    pub netuid: NetUid,
    pub price: u64,
}

sp_api::decl_runtime_apis! {
    /// Runtime API for swap price quotes and dry-run swaps.
    ///
    /// RPC method strings (`swap_currentAlphaPrice`, etc.) must stay in sync with
    /// `pallets/swap/rpc` — do not rename these trait methods.
    pub trait SwapRuntimeApi {
        /// Alpha price for `netuid`, scaled by `1e9`.
        fn current_alpha_price(netuid: NetUid) -> u64;
        /// Alpha prices for every subnet that has a dynamic mechanism.
        fn current_alpha_price_all() -> Vec<SubnetPrice>;
        /// Dry-run buy: pay `tao` rao, receive alpha (fees included in result).
        fn sim_swap_tao_for_alpha(netuid: NetUid, tao: TaoBalance) -> SimSwapResult;
        /// Dry-run sell: pay `alpha`, receive TAO rao (fees included in result).
        fn sim_swap_alpha_for_tao(netuid: NetUid, alpha: AlphaBalance) -> SimSwapResult;
    }
}
