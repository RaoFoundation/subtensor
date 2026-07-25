//! Alpha mint / resolve / recycle helpers used by the coinbase and staking paths.
//!
//! Mint returns a [`PositiveAlphaImbalance`]; callers must resolve it into
//! [`SubnetAlphaOut`] (outstanding) or [`SubnetAlphaIn`] (pool reserve), or recycle/burn
//! via the alpha-assets pallet.

use pallet_alpha_assets::{AlphaAssetsInterface, PositiveAlphaImbalance};
use subtensor_runtime_common::{AlphaBalance, NetUid, Token};

use super::*;

impl<T: Config> Pallet<T> {
    /// Mint `amount` alpha on `netuid` and return the imbalance for later resolution.
    pub fn mint_alpha(netuid: NetUid, amount: AlphaBalance) -> PositiveAlphaImbalance {
        T::AlphaAssets::mint_alpha(netuid, amount)
    }

    /// Resolve alpha imbalance into outstanding alpha ([`SubnetAlphaOut`]) on the subnet.
    pub fn resolve_to_alpha_out(imbalance: PositiveAlphaImbalance) {
        let netuid = imbalance.netuid();
        let amount = imbalance.amount();
        if amount.is_zero() {
            return;
        }

        SubnetAlphaOut::<T>::mutate(netuid, |total| {
            *total = total.saturating_add(amount);
        });
    }

    /// Resolve alpha imbalance into alpha held in the subnet pool reserve ([`SubnetAlphaIn`]).
    pub fn resolve_to_alpha_in(imbalance: PositiveAlphaImbalance) {
        let netuid = imbalance.netuid();
        let amount = imbalance.amount();
        if amount.is_zero() {
            return;
        }

        SubnetAlphaIn::<T>::mutate(netuid, |total| {
            *total = total.saturating_add(amount);
        });
    }

    /// Recycle alpha: decrease [`SubnetAlphaOut`] and call alpha-assets recycle (reduces
    /// total alpha issuance).
    pub fn recycle_subnet_alpha(netuid: NetUid, amount: AlphaBalance) {
        if amount.is_zero() {
            return;
        }

        SubnetAlphaOut::<T>::mutate(netuid, |total| {
            *total = total.saturating_sub(amount);
        });

        let _ = T::AlphaAssets::recycle_alpha(netuid, amount);
    }

    /// Burn alpha via alpha-assets without changing [`SubnetAlphaOut`] (issuance unchanged).
    pub fn burn_subnet_alpha(netuid: NetUid, amount: AlphaBalance) {
        if amount.is_zero() {
            return;
        }

        let _ = T::AlphaAssets::burn_alpha(netuid, amount);
    }
}
