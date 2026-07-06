use pallet_alpha_assets::{AlphaAssetsInterface, PositiveAlphaImbalance};
use subtensor_runtime_common::{AlphaBalance, NetUid, Token};

use super::*;

impl<T: Config> Pallet<T> {
    /// Create alpha and return the resulting imbalance for later resolution.
    pub fn mint_alpha(netuid: NetUid, amount: AlphaBalance) -> PositiveAlphaImbalance {
        T::AlphaAssets::mint_alpha(netuid, amount)
    }

    /// Resolve alpha imbalance into outstanding alpha on the subnet.
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

    /// Resolve alpha imbalance into alpha held in the subnet reserve.
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

    /// Recycle alpha (reduce total alpha issuance)
    pub fn recycle_subnet_alpha(netuid: NetUid, amount: AlphaBalance) {
        if amount.is_zero() {
            return;
        }

        SubnetAlphaOut::<T>::mutate(netuid, |total| {
            *total = total.saturating_sub(amount);
        });

        let _ = T::AlphaAssets::recycle_alpha(netuid, amount);
    }

    /// Burn alpha (no change to total alpha issuance)
    pub fn burn_subnet_alpha(netuid: NetUid, amount: AlphaBalance) {
        if amount.is_zero() {
            return;
        }

        let _ = T::AlphaAssets::burn_alpha(netuid, amount);
    }
}
