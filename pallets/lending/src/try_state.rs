//! Invariant checks over storage, one function per SPEC section 5 invariant.

use frame_support::{ensure, traits::Get};
use sp_runtime::TryRuntimeError;

use crate::{
    Config, DeregParamsStore, LendingParamsStore, LendingPoolInterface, LongParamsStore, Pallet,
    PendingDeregParams, PendingLendingParams, PendingLongParams, PendingShortParams,
    RankGeneration, RankPrice, RankRing, RankRingGeneration, ShortParamsStore, TwapRing,
};

impl<T: Config> Pallet<T> {
    /// Runs every invariant check.
    pub fn do_try_state() -> Result<(), TryRuntimeError> {
        Self::check_i25_price_rings()?;
        Self::check_i26_params()
    }

    /// SPEC I25: every live non-root subnet has a full rank ring (`rank_window_days` daily
    /// means) of the current generation and a published `RankPrice` equal to the ring's mean,
    /// whether lending is enabled or not. The TWAP ring is held to the same standard (full at
    /// `twap_samples`), since no read may fall back when a ring is missing.
    fn check_i25_price_rings() -> Result<(), TryRuntimeError> {
        let generation = RankGeneration::<T>::get();
        let rank_days = DeregParamsStore::<T>::get().rank_window_days;
        let twap_samples = LendingParamsStore::<T>::get().twap_samples;
        for netuid in T::Pool::live_subnets() {
            let rank = RankRing::<T>::get(netuid).ok_or("I25: live subnet has no rank ring")?;
            ensure!(
                rank.days.len() == rank_days as usize,
                "I25: rank ring is not a full window"
            );
            ensure!(
                RankRingGeneration::<T>::get(netuid) == Some(generation),
                "I25: rank ring generation is stale"
            );
            let published =
                RankPrice::<T>::get(netuid).ok_or("I25: live subnet has no published RankPrice")?;
            ensure!(
                rank.sma() == Some(published),
                "I25: RankPrice is not the mean of the ring"
            );
            let twap = TwapRing::<T>::get(netuid).ok_or("I25: live subnet has no TWAP ring")?;
            ensure!(
                twap.samples.len() == twap_samples as usize,
                "I25: TWAP ring is not a full window"
            );
        }
        Ok(())
    }

    /// SPEC I26: every live and pending parameter value satisfies the section 2.1 bounds.
    /// No long positions exist yet, so the longs cap is checked against a count of zero.
    fn check_i26_params() -> Result<(), TryRuntimeError> {
        let ceiling = T::MaxLongPositionsCeiling::get();
        let lending = PendingLendingParams::<T>::get()
            .into_iter()
            .chain([LendingParamsStore::<T>::get()]);
        for params in lending {
            ensure!(
                params.validate().is_ok(),
                "I26: LendingParams out of bounds"
            );
        }
        let short = PendingShortParams::<T>::get()
            .into_iter()
            .chain([ShortParamsStore::<T>::get()]);
        for params in short {
            ensure!(params.validate().is_ok(), "I26: ShortParams out of bounds");
        }
        let long = PendingLongParams::<T>::get()
            .into_iter()
            .chain([LongParamsStore::<T>::get()]);
        for params in long {
            ensure!(params.validate().is_ok(), "I26: LongParams out of bounds");
        }
        let dereg = PendingDeregParams::<T>::get()
            .into_iter()
            .chain([DeregParamsStore::<T>::get()]);
        for params in dereg {
            ensure!(
                params.validate(0, ceiling).is_ok(),
                "I26: DeregParams out of bounds"
            );
        }
        Ok(())
    }
}
