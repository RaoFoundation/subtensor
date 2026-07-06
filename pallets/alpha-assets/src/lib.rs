#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;

use codec::{Decode, Encode, MaxEncodedLen};
use frame_support::pallet_prelude::*;
use frame_support::traits::{Imbalance, SameOrOther, TryDrop, tokens::imbalance::TryMerge};
use scale_info::TypeInfo;
use sp_runtime::traits::Zero;
use subtensor_macros::freeze_struct;
use subtensor_runtime_common::{AlphaBalance, NetUid, Token};

pub use pallet::*;

/// Lightweight mint record that can later be resolved to a subnet or user alpha balance.
#[freeze_struct("2da64a64e80a7880")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Encode, Decode, MaxEncodedLen, TypeInfo)]
pub struct PositiveAlphaImbalance {
    netuid: NetUid,
    amount: AlphaBalance,
}

#[freeze_struct("1f16c8937e05cf36")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Encode, Decode, MaxEncodedLen, TypeInfo)]
pub struct NegativeAlphaImbalance {
    netuid: NetUid,
    amount: AlphaBalance,
}

impl PositiveAlphaImbalance {
    pub fn new(netuid: NetUid, amount: AlphaBalance) -> Self {
        Self { netuid, amount }
    }

    pub fn netuid(&self) -> NetUid {
        self.netuid
    }

    pub fn amount(&self) -> AlphaBalance {
        self.amount
    }
}

impl NegativeAlphaImbalance {
    pub fn new(netuid: NetUid, amount: AlphaBalance) -> Self {
        Self { netuid, amount }
    }
}

fn log_netuid_mismatch(context: &'static str, left: NetUid, right: NetUid) {
    log::error!(
        target: "runtime::alpha-assets",
        "{context}: attempted to combine alpha imbalances from different netuids: left={left}, right={right}"
    );
}

impl TryDrop for PositiveAlphaImbalance {
    fn try_drop(self) -> Result<(), Self> {
        if self.amount.is_zero() {
            Ok(())
        } else {
            Err(self)
        }
    }
}

impl TryDrop for NegativeAlphaImbalance {
    fn try_drop(self) -> Result<(), Self> {
        if self.amount.is_zero() {
            Ok(())
        } else {
            Err(self)
        }
    }
}

impl TryMerge for PositiveAlphaImbalance {
    fn try_merge(self, other: Self) -> Result<Self, (Self, Self)> {
        if self.netuid == other.netuid {
            Ok(Self::new(
                self.netuid,
                self.amount.saturating_add(other.amount),
            ))
        } else {
            Err((self, other))
        }
    }
}

impl TryMerge for NegativeAlphaImbalance {
    fn try_merge(self, other: Self) -> Result<Self, (Self, Self)> {
        if self.netuid == other.netuid {
            Ok(Self::new(
                self.netuid,
                self.amount.saturating_add(other.amount),
            ))
        } else {
            Err((self, other))
        }
    }
}

impl Imbalance<AlphaBalance> for PositiveAlphaImbalance {
    type Opposite = NegativeAlphaImbalance;

    fn zero() -> Self {
        Self::default()
    }

    fn drop_zero(self) -> Result<(), Self> {
        self.try_drop()
    }

    fn split(self, amount: AlphaBalance) -> (Self, Self) {
        let first = self.amount.min(amount);
        let second = self.amount.saturating_sub(first);
        (
            Self::new(self.netuid, first),
            Self::new(self.netuid, second),
        )
    }

    fn extract(&mut self, amount: AlphaBalance) -> Self {
        let extracted = self.amount.min(amount);
        self.amount = self.amount.saturating_sub(extracted);
        Self::new(self.netuid, extracted)
    }

    fn merge(self, other: Self) -> Self {
        match self.try_merge(other) {
            Ok(merged) => merged,
            Err((left, right)) => {
                log_netuid_mismatch("merge(positive)", left.netuid, right.netuid);
                left
            }
        }
    }

    fn subsume(&mut self, other: Self) {
        if self.netuid != other.netuid {
            log_netuid_mismatch("subsume(positive)", self.netuid, other.netuid);
            return;
        }
        self.amount = self.amount.saturating_add(other.amount);
    }

    fn offset(self, other: Self::Opposite) -> SameOrOther<Self, Self::Opposite> {
        if self.netuid != other.netuid {
            log_netuid_mismatch("offset(positive)", self.netuid, other.netuid);
            return SameOrOther::Same(self);
        }
        if self.amount > other.amount {
            SameOrOther::Same(Self::new(
                self.netuid,
                self.amount.saturating_sub(other.amount),
            ))
        } else if other.amount > self.amount {
            SameOrOther::Other(NegativeAlphaImbalance::new(
                self.netuid,
                other.amount.saturating_sub(self.amount),
            ))
        } else {
            SameOrOther::None
        }
    }

    fn peek(&self) -> AlphaBalance {
        self.amount
    }
}

impl Imbalance<AlphaBalance> for NegativeAlphaImbalance {
    type Opposite = PositiveAlphaImbalance;

    fn zero() -> Self {
        Self::default()
    }

    fn drop_zero(self) -> Result<(), Self> {
        self.try_drop()
    }

    fn split(self, amount: AlphaBalance) -> (Self, Self) {
        let first = self.amount.min(amount);
        let second = self.amount.saturating_sub(first);
        (
            Self::new(self.netuid, first),
            Self::new(self.netuid, second),
        )
    }

    fn extract(&mut self, amount: AlphaBalance) -> Self {
        let extracted = self.amount.min(amount);
        self.amount = self.amount.saturating_sub(extracted);
        Self::new(self.netuid, extracted)
    }

    fn merge(self, other: Self) -> Self {
        match self.try_merge(other) {
            Ok(merged) => merged,
            Err((left, right)) => {
                log_netuid_mismatch("merge(negative)", left.netuid, right.netuid);
                left
            }
        }
    }

    fn subsume(&mut self, other: Self) {
        if self.netuid != other.netuid {
            log_netuid_mismatch("subsume(negative)", self.netuid, other.netuid);
            return;
        }
        self.amount = self.amount.saturating_add(other.amount);
    }

    fn offset(self, other: Self::Opposite) -> SameOrOther<Self, Self::Opposite> {
        if self.netuid != other.netuid {
            log_netuid_mismatch("offset(negative)", self.netuid, other.netuid);
            return SameOrOther::Same(self);
        }
        if self.amount > other.amount {
            SameOrOther::Same(Self::new(
                self.netuid,
                self.amount.saturating_sub(other.amount),
            ))
        } else if other.amount > self.amount {
            SameOrOther::Other(PositiveAlphaImbalance::new(
                self.netuid,
                other.amount.saturating_sub(self.amount),
            ))
        } else {
            SameOrOther::None
        }
    }

    fn peek(&self) -> AlphaBalance {
        self.amount
    }
}

/// Loose-coupling interface for alpha issuance operations.
pub trait AlphaAssetsInterface {
    fn total_alpha_issuance(netuid: NetUid) -> AlphaBalance;

    fn mint_alpha(netuid: NetUid, amount: AlphaBalance) -> PositiveAlphaImbalance;

    fn burn_alpha(netuid: NetUid, amount: AlphaBalance) -> AlphaBalance;

    fn recycle_alpha(netuid: NetUid, amount: AlphaBalance) -> AlphaBalance;
}

impl AlphaAssetsInterface for () {
    fn total_alpha_issuance(_netuid: NetUid) -> AlphaBalance {
        AlphaBalance::ZERO
    }

    fn mint_alpha(netuid: NetUid, amount: AlphaBalance) -> PositiveAlphaImbalance {
        PositiveAlphaImbalance::new(netuid, amount)
    }

    fn burn_alpha(_netuid: NetUid, amount: AlphaBalance) -> AlphaBalance {
        amount
    }

    fn recycle_alpha(_netuid: NetUid, amount: AlphaBalance) -> AlphaBalance {
        amount
    }
}

#[deny(missing_docs)]
#[frame_support::pallet]
#[allow(clippy::expect_used)]
pub mod pallet {
    use super::*;

    #[pallet::pallet]
    #[pallet::without_storage_info]
    pub struct Pallet<T>(_);

    #[pallet::config]
    pub trait Config: frame_system::Config {}

    /// Total alpha issuance tracked by the pallet.
    #[pallet::storage]
    #[pallet::getter(fn total_alpha_issuance)]
    pub type TotalAlphaIssuance<T> = StorageMap<_, Twox64Concat, NetUid, AlphaBalance, ValueQuery>;

    /// Total alpha burned per subnet through this pallet.
    #[pallet::storage]
    #[pallet::getter(fn alpha_burned)]
    pub type AlphaBurned<T> = StorageMap<_, Twox64Concat, NetUid, AlphaBalance, ValueQuery>;

    /// Total alpha recycled per subnet through this pallet.
    #[pallet::storage]
    #[pallet::getter(fn alpha_recycled)]
    pub type AlphaRecycled<T> = StorageMap<_, Twox64Concat, NetUid, AlphaBalance, ValueQuery>;
}

impl<T: pallet::Config> Pallet<T> {
    pub fn mint_alpha(netuid: NetUid, amount: AlphaBalance) -> PositiveAlphaImbalance {
        if !amount.is_zero() {
            TotalAlphaIssuance::<T>::mutate(netuid, |issuance| {
                *issuance = (*issuance).saturating_add(amount);
            });
        }

        PositiveAlphaImbalance::new(netuid, amount)
    }

    pub fn burn_alpha(netuid: NetUid, amount: AlphaBalance) -> AlphaBalance {
        if !amount.is_zero() {
            AlphaBurned::<T>::mutate(netuid, |burned| {
                *burned = (*burned).saturating_add(amount);
            });
        }

        amount
    }

    pub fn recycle_alpha(netuid: NetUid, amount: AlphaBalance) -> AlphaBalance {
        if !amount.is_zero() {
            AlphaRecycled::<T>::mutate(netuid, |recycled| {
                *recycled = (*recycled).saturating_add(amount);
            });
            TotalAlphaIssuance::<T>::mutate(netuid, |issuance| {
                *issuance = (*issuance).saturating_sub(amount);
            });
        }

        amount
    }
}

impl<T: pallet::Config> AlphaAssetsInterface for Pallet<T> {
    fn total_alpha_issuance(netuid: NetUid) -> AlphaBalance {
        TotalAlphaIssuance::<T>::get(netuid)
    }

    fn mint_alpha(netuid: NetUid, amount: AlphaBalance) -> PositiveAlphaImbalance {
        Self::mint_alpha(netuid, amount)
    }

    fn burn_alpha(netuid: NetUid, amount: AlphaBalance) -> AlphaBalance {
        Self::burn_alpha(netuid, amount)
    }

    fn recycle_alpha(netuid: NetUid, amount: AlphaBalance) -> AlphaBalance {
        Self::recycle_alpha(netuid, amount)
    }
}
