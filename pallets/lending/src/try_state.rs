//! Invariant checks over storage, one function per SPEC section 5 invariant.

use alloc::collections::BTreeMap;
use frame_support::{ensure, traits::Get};
use frame_system::pallet_prelude::BlockNumberFor;
use sp_runtime::TryRuntimeError;
use subtensor_runtime_common::NetUid;

use crate::{
    Config, DeregParamsStore, LendingParamsStore, LendingPoolInterface, LongLiftedTao,
    LongLockedAlpha, LongParamsStore, LongPositionCount, Pallet, PendingDeregParams,
    PendingLendingParams, PendingLongParams, PendingShortParams, Positions, RankGeneration,
    RankPrice, RankRing, RankRingGeneration, ShortParamsStore, SyntheticOutstanding, TwapRing,
    positions::ALPHA_SUPPLY_CAP,
    types::{Leg, Position},
};

/// One subnet's sums over its open positions, to compare with the stored books.
#[derive(Default)]
struct BookTotals {
    /// Σ minted over shorts (SPEC I1).
    minted: u128,
    /// Σ locked alpha over longs (SPEC I12).
    locked_alpha: u128,
    /// Σ debt over longs (SPEC I12).
    debt: u128,
    /// Open longs (SPEC 7.4).
    longs: u128,
}

/// Checks that each subnet's stored book equals its total over the open positions, for every
/// subnet with positions and every subnet with a stored entry.
fn ensure_book(
    message: &'static str,
    totals: &BTreeMap<NetUid, BookTotals>,
    total: fn(&BookTotals) -> u128,
    stored: BTreeMap<NetUid, u128>,
) -> Result<(), TryRuntimeError> {
    for netuid in totals.keys().chain(stored.keys()) {
        let expected = totals.get(netuid).map_or(0, total);
        let actual = stored.get(netuid).copied().unwrap_or(0);
        ensure!(actual == expected, message);
    }
    Ok(())
}

/// A stored per-subnet book as a map, for [`ensure_book`].
fn as_book(entries: impl Iterator<Item = (NetUid, impl Into<u128>)>) -> BTreeMap<NetUid, u128> {
    entries
        .map(|(netuid, value)| (netuid, value.into()))
        .collect()
}

impl<T: Config> Pallet<T> {
    /// Runs every invariant check.
    pub fn do_try_state() -> Result<(), TryRuntimeError> {
        Self::check_i25_price_rings()?;
        Self::check_i26_params()?;
        Self::check_i1_synthetic_book()?;
        Self::check_i3_custody_tao()?;
        Self::check_i12_long_books()?;
        Self::check_i21_supply_cap()
    }

    /// Every open position with its subnet and custody account.
    fn open_positions() -> impl Iterator<Item = (NetUid, T::AccountId, Position<BlockNumberFor<T>>)>
    {
        Positions::<T>::iter().map(|((who, netuid, side), pos)| {
            let custody = Self::custody_of(side, &who, netuid, pos.opened_at);
            (netuid, custody, pos)
        })
    }

    /// The sums of every subnet's open positions.
    fn book_totals() -> BTreeMap<NetUid, BookTotals> {
        let mut totals = BTreeMap::<NetUid, BookTotals>::new();
        for (netuid, _, pos) in Self::open_positions() {
            let total = totals.entry(netuid).or_default();
            match pos.leg {
                Leg::Short { minted, .. } => {
                    total.minted = total.minted.saturating_add(u128::from(minted));
                }
                Leg::Long { locked_alpha, debt } => {
                    total.locked_alpha =
                        total.locked_alpha.saturating_add(u128::from(locked_alpha));
                    total.debt = total.debt.saturating_add(u128::from(debt));
                    total.longs = total.longs.saturating_add(1);
                }
            }
        }
        totals
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

    /// SPEC I26: every live and pending parameter value satisfies the section 2.1 bounds. The
    /// longs cap is checked against the largest current long count on any subnet, as
    /// `DeregParams::validate` requires.
    fn check_i26_params() -> Result<(), TryRuntimeError> {
        let ceiling = T::MaxLongPositionsCeiling::get();
        let max_longs = LongPositionCount::<T>::iter_values().max().unwrap_or(0);
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
                params.validate(max_longs, ceiling).is_ok(),
                "I26: DeregParams out of bounds"
            );
        }
        Ok(())
    }

    /// SPEC I1: Σ minted over the open shorts equals `SyntheticOutstanding`, per subnet.
    fn check_i1_synthetic_book() -> Result<(), TryRuntimeError> {
        ensure_book(
            "I1: SyntheticOutstanding is not the sum of the shorts' minted alpha",
            &Self::book_totals(),
            |total| total.minted,
            as_book(SyntheticOutstanding::<T>::iter()),
        )
    }

    /// SPEC I3: each open position's custody holds exactly its collateral, its short proceeds
    /// and its deposit. Custody is per position, so the sum over positions is the protocol's
    /// custody TAO.
    fn check_i3_custody_tao() -> Result<(), TryRuntimeError> {
        for (_, custody, pos) in Self::open_positions() {
            let held = u128::from(T::Pool::tao_balance(&custody));
            let owed = u128::from(pos.collateral)
                .saturating_add(u128::from(pos.leg.proceeds()))
                .saturating_add(u128::from(pos.deposit));
            ensure!(
                held == owed,
                "I3: custody TAO differs from collateral + proceeds + deposit"
            );
        }
        Ok(())
    }

    /// SPEC I12: Σ debt equals `LongLiftedTao`, Σ locked alpha equals `LongLockedAlpha`, and each
    /// long's custody holds exactly its locked alpha. The open-long count must equal
    /// `LongPositionCount` (SPEC 7.4).
    fn check_i12_long_books() -> Result<(), TryRuntimeError> {
        let totals = Self::book_totals();
        ensure_book(
            "I12: LongLockedAlpha is not the sum of the longs' locked alpha",
            &totals,
            |total| total.locked_alpha,
            as_book(LongLockedAlpha::<T>::iter()),
        )?;
        ensure_book(
            "I12: LongLiftedTao is not the sum of the longs' debt",
            &totals,
            |total| total.debt,
            as_book(LongLiftedTao::<T>::iter()),
        )?;
        ensure_book(
            "I12: LongPositionCount is not the number of open longs",
            &totals,
            |total| total.longs,
            as_book(LongPositionCount::<T>::iter()),
        )?;
        for (netuid, custody, pos) in Self::open_positions() {
            if let Leg::Long { locked_alpha, .. } = pos.leg {
                ensure!(
                    T::Pool::held_alpha(&custody, netuid) == locked_alpha,
                    "I12: long custody alpha differs from its locked alpha"
                );
            }
        }
        Ok(())
    }

    /// SPEC I21: alpha issuance is at most `ALPHA_SUPPLY_CAP` on every live subnet.
    fn check_i21_supply_cap() -> Result<(), TryRuntimeError> {
        for netuid in T::Pool::live_subnets() {
            ensure!(
                T::Pool::alpha_issuance(netuid) <= ALPHA_SUPPLY_CAP,
                "I21: alpha issuance exceeds the 21,000,000 cap"
            );
        }
        Ok(())
    }
}
