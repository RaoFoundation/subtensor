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
    pub trait SwapRuntimeApi {
        fn current_alpha_price(netuid: NetUid) -> u64;
        fn current_alpha_price_all() -> Vec<SubnetPrice>;
        fn sim_swap_tao_for_alpha(netuid: NetUid, tao: TaoBalance) -> SimSwapResult;
        fn sim_swap_alpha_for_tao(netuid: NetUid, alpha: AlphaBalance) -> SimSwapResult;
    }
}
