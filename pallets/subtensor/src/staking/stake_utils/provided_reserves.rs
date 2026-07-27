//! Mutators for subnet-provided TAO / alpha reserve counters (`SubnetTAO`, `SubnetAlphaIn`).
use super::*;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};

impl<T: Config> Pallet<T> {
    pub fn increase_provided_tao_reserve(netuid: NetUid, tao: TaoBalance) {
        if !tao.is_zero() {
            SubnetTAO::<T>::mutate(netuid, |total| {
                *total = total.saturating_add(tao);
            });
        }
    }

    pub fn decrease_provided_tao_reserve(netuid: NetUid, tao: TaoBalance) {
        if !tao.is_zero() {
            SubnetTAO::<T>::mutate(netuid, |total| {
                *total = total.saturating_sub(tao);
            });
        }
    }

    pub fn increase_provided_alpha_reserve(netuid: NetUid, alpha: AlphaBalance) {
        if !alpha.is_zero() {
            SubnetAlphaIn::<T>::mutate(netuid, |total| {
                *total = total.saturating_add(alpha);
            });
        }
    }

    pub fn decrease_provided_alpha_reserve(netuid: NetUid, alpha: AlphaBalance) {
        if !alpha.is_zero() {
            SubnetAlphaIn::<T>::mutate(netuid, |total| {
                *total = total.saturating_sub(alpha);
            });
        }
    }
}
