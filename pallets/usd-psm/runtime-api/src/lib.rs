//! Runtime API for the canonical rails: the single quote and state source
//! consumed by the RPC layer, the USD precompile, and btcli.

#![cfg_attr(not(feature = "std"), no_std)]

use codec::{Decode, Encode};
use scale_info::TypeInfo;
use serde::{Deserialize, Serialize};
use sp_core::H160;
use sp_std::vec::Vec;
use subtensor_macros::freeze_struct;

/// State of the canonical tUSD/TAO pool.
#[freeze_struct("7545405eb292b50c")]
#[derive(Decode, Deserialize, Encode, PartialEq, Eq, Clone, Debug, Serialize, TypeInfo)]
pub struct RailsPoolState {
    /// TAO reserve (rao).
    pub tao_reserve: u64,
    /// tUSD reserve (9 decimals).
    pub tusd_reserve: u64,
    /// Swap fee in basis points.
    pub fee_bps: u16,
}

/// Supply attestation for one subnet's canonical shares.
#[freeze_struct("5546774173f27e8e")]
#[derive(Decode, Deserialize, Encode, PartialEq, Eq, Clone, Debug, Serialize, TypeInfo)]
pub struct RailsAlphaAttestation {
    /// Live alpha in the hub escrow (rao-scale, 9 decimals). Grows with
    /// emissions.
    pub escrowed_alpha: u64,
    /// Canonical shares outstanding across spokes.
    pub shares_outstanding: u64,
    /// Share index in 1e9 fixed point (escrowed alpha per share).
    pub index_e9: u64,
}

/// Outbound hub configuration.
#[freeze_struct("9a789215c814f751")]
#[derive(Decode, Deserialize, Encode, PartialEq, Eq, Clone, Debug, Serialize, TypeInfo)]
pub struct RailsHubInfo {
    /// The keyless EVM identity dispatching outbound messages (the trusted
    /// `hubSender` on remote canonical share tokens).
    pub hub_sender: H160,
    /// The Bittensor-EVM Mailbox used for outbound messages, if configured.
    pub mailbox: Option<H160>,
    /// Next inbound envelope nonce expected by the sequential guard.
    pub next_nonce: u64,
}

/// Public view of a PSM-registered USD asset.
#[freeze_struct("ecc97f3d44576ea2")]
#[derive(Decode, Deserialize, Encode, PartialEq, Eq, Clone, Debug, Serialize, TypeInfo)]
pub struct RailsAssetInfo {
    /// PSM asset id.
    pub asset_id: u32,
    /// ERC-20 contract on the Bittensor EVM.
    pub erc20: H160,
    /// Reserves currently backing tUSD.
    pub reserves: u64,
    /// Rate window headroom right now.
    pub available: u64,
    /// Haircut in basis points.
    pub haircut_bps: u16,
    /// Deposits enabled.
    pub enabled: bool,
}

sp_api::decl_runtime_apis! {
    /// Quotes and registry views for the canonical rails.
    pub trait RailsRuntimeApi {
        /// Quote tUSD -> TAO through the canonical pool.
        fn rails_quote_usd_to_tao(amount: u64) -> Option<u64>;
        /// Quote TAO -> tUSD through the canonical pool.
        fn rails_quote_tao_to_usd(amount: u64) -> Option<u64>;
        /// Canonical pool state.
        fn rails_pool_state() -> RailsPoolState;
        /// All registered PSM assets.
        fn rails_assets() -> Vec<RailsAssetInfo>;
        /// The registered Gateway contract (H160), if set.
        fn rails_gateway() -> Option<H160>;
        /// tUSD ledger balance of an account.
        fn rails_tusd_balance(account: sp_core::crypto::AccountId32) -> u64;
        /// Supply attestation for a subnet's wrapped alpha.
        fn rails_alpha_attestation(netuid: u16) -> RailsAlphaAttestation;
        /// Outbound hub configuration (hub sender identity + mailbox).
        fn rails_hub_info() -> RailsHubInfo;
    }
}
