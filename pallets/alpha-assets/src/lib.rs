//! # Alpha Assets
//!
//! Tracks per-subnet alpha issuance, burns, and recycles, and exposes mint/burn/recycle
//! through [`AlphaAssetsInterface`] so coinbase and staking can stay loosely coupled.
//!
//! This pallet has no extrinsics: all mutations go through the interface / `Pallet` helpers.
//! Issued alpha is represented as a [`PositiveAlphaImbalance`] that must be resolved by the
//! caller (it does not auto-apply on drop).

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;

mod alpha_imbalance;

pub use alpha_imbalance::{NegativeAlphaImbalance, PositiveAlphaImbalance};
pub use pallet::*;

use frame_support::pallet_prelude::*;
use sp_runtime::traits::Zero;
use subtensor_runtime_common::{AlphaBalance, NetUid, Token};

/// Loose-coupling interface for alpha issuance, burn, and recycle operations.
///
/// Runtime wiring typically binds this to [`Pallet`]; `()` is a no-op stub for tests that
/// do not need ledger side effects.
pub trait AlphaAssetsInterface {
    /// Current total alpha issued for `netuid` (rao), as tracked by this pallet.
    fn total_alpha_issuance(netuid: NetUid) -> AlphaBalance;

    /// Increases [`TotalAlphaIssuance`] and returns a mint imbalance for the caller to resolve.
    fn mint_alpha(netuid: NetUid, amount: AlphaBalance) -> PositiveAlphaImbalance;

    /// Records a burn against [`AlphaBurned`] without reducing [`TotalAlphaIssuance`].
    ///
    /// Returns `amount` unchanged. Destroying circulating stake is the caller's job; this
    /// only updates the burn counter.
    fn burn_alpha(netuid: NetUid, amount: AlphaBalance) -> AlphaBalance;

    /// Records a recycle against [`AlphaRecycled`] and saturating-subtracts from issuance.
    ///
    /// Returns `amount` unchanged. Unlike burn, recycle shrinks [`TotalAlphaIssuance`].
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

    /// Pallet that stores per-subnet alpha issuance / burn / recycle totals.
    #[pallet::pallet]
    #[pallet::without_storage_info]
    pub struct Pallet<T>(_);

    /// Runtime configuration for alpha-assets (no extra associated types today).
    #[pallet::config]
    pub trait Config: frame_system::Config {}

    /// Cumulative alpha minted per subnet (rao); increased by [`Pallet::mint_alpha`],
    /// decreased by [`Pallet::recycle_alpha`].
    #[pallet::storage]
    #[pallet::getter(fn total_alpha_issuance)]
    pub type TotalAlphaIssuance<T> = StorageMap<_, Twox64Concat, NetUid, AlphaBalance, ValueQuery>;

    /// Cumulative alpha burned per subnet (rao) via [`Pallet::burn_alpha`].
    ///
    /// Burn does not decrease [`TotalAlphaIssuance`]; it only accumulates this counter.
    #[pallet::storage]
    #[pallet::getter(fn alpha_burned)]
    pub type AlphaBurned<T> = StorageMap<_, Twox64Concat, NetUid, AlphaBalance, ValueQuery>;

    /// Cumulative alpha recycled per subnet (rao) via [`Pallet::recycle_alpha`].
    #[pallet::storage]
    #[pallet::getter(fn alpha_recycled)]
    pub type AlphaRecycled<T> = StorageMap<_, Twox64Concat, NetUid, AlphaBalance, ValueQuery>;
}

impl<T: pallet::Config> Pallet<T> {
    /// Mints `amount` of alpha for `netuid`, bumps issuance, and returns a resolveable imbalance.
    pub fn mint_alpha(netuid: NetUid, amount: AlphaBalance) -> PositiveAlphaImbalance {
        if !amount.is_zero() {
            TotalAlphaIssuance::<T>::mutate(netuid, |issuance| {
                *issuance = (*issuance).saturating_add(amount);
            });
        }

        PositiveAlphaImbalance::new(netuid, amount)
    }

    /// Records `amount` as burned for `netuid` without changing total issuance.
    pub fn burn_alpha(netuid: NetUid, amount: AlphaBalance) -> AlphaBalance {
        if !amount.is_zero() {
            AlphaBurned::<T>::mutate(netuid, |burned| {
                *burned = (*burned).saturating_add(amount);
            });
        }

        amount
    }

    /// Records `amount` as recycled and saturating-subtracts it from total issuance.
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
