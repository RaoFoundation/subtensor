//! Invariant checks over storage, one function per SPEC section 5 invariant.

use alloc::collections::BTreeMap;
use frame_support::{ensure, traits::Get};
use frame_system::pallet_prelude::BlockNumberFor;
use sp_runtime::TryRuntimeError;
use subtensor_runtime_common::NetUid;

use crate::{
    Config, DeregParamsStore, DissolutionSettlements, DissolvingSubnets, Due, HolderPositions,
    InterestRetry, InterestTotals, LendingParamsStore, LendingPoolInterface, LongLiftedTao,
    LongLockedAlpha, LongParamsStore, LongPositionCount, NextDue, Pallet, PendingDeregParams,
    PendingLendingParams, PendingLongParams, PendingShortParams, PositionCount, Positions,
    PositionsByNetuid, RankGeneration, RankPrice, RankRing, RankRingGeneration, ResidualAlpha,
    SettlementRetry, ShortParamsStore, SyntheticOutstanding, TotalPositions, TwapRing,
    dissolution::SettlementStage,
    math::interest::ACCRUAL_DEN,
    positions::ALPHA_SUPPLY_CAP,
    types::{Leg, Position, Side},
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
    /// Open positions of both sides (SPEC 7.5).
    positions: u128,
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
        Self::check_i2_residual_alpha()?;
        Self::check_i3_custody_tao()?;
        Self::check_i5_interest_schedule()?;
        Self::check_i23_rate()?;
        Self::check_i12_long_books()?;
        Self::check_i19_interest_to_pool()?;
        Self::check_position_counts()?;
        Self::check_holder_positions()?;
        Self::check_position_index()?;
        Self::check_i8_dissolution_settlement()?;
        Self::check_i21_supply_cap()
    }

    /// Every open position with its subnet and custody account.
    fn open_positions() -> impl Iterator<Item = (NetUid, T::AccountId, Position<BlockNumberFor<T>>)>
    {
        Positions::<T>::iter().map(|((who, netuid, side), pos)| {
            let custody = Self::position_custody(&(who, netuid, side), pos.opened_at);
            (netuid, custody, pos)
        })
    }

    /// The sums of every subnet's open positions.
    fn book_totals() -> BTreeMap<NetUid, BookTotals> {
        let mut totals = BTreeMap::<NetUid, BookTotals>::new();
        for (netuid, _, pos) in Self::open_positions() {
            let total = totals.entry(netuid).or_default();
            total.positions = total.positions.saturating_add(1);
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
        let live = LendingParamsStore::<T>::get();
        ensure!(live.validate().is_ok(), "I26: LendingParams out of bounds");
        // A pending group is within the bounds, with the cap cross-check against the live group,
        // and not yet due: `on_initialize` applies it in its block.
        if let Some((pending, apply_at)) = PendingLendingParams::<T>::get() {
            ensure!(
                pending.validate_against_live(&live).is_ok(),
                "I26: pending LendingParams out of bounds"
            );
            ensure!(
                apply_at > frame_system::Pallet::<T>::block_number(),
                "I26: pending LendingParams is overdue"
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

    /// SPEC I2, the part storage can show: a default only moves alpha from
    /// `SyntheticOutstanding` to `ResidualAlpha`, so an entry is never stored at zero and the
    /// two books of a subnet add up without overflow (`lending_alpha`). The conservation
    /// itself (`Δ(SyntheticOutstanding + ResidualAlpha) == minted - bought back`, and no change
    /// of alpha issuance at a default) has no cumulative record in storage to check against;
    /// the tests assert it across every operation ([`Self::lending_alpha_books`]).
    fn check_i2_residual_alpha() -> Result<(), TryRuntimeError> {
        for (netuid, residual) in ResidualAlpha::<T>::iter() {
            ensure!(residual > 0, "I2: ResidualAlpha stored at zero");
            ensure!(
                SyntheticOutstanding::<T>::get(netuid)
                    .checked_add(residual)
                    .is_some(),
                "I2: SyntheticOutstanding + ResidualAlpha overflows"
            );
        }
        Ok(())
    }

    /// The I2 accounting of one subnet: `(SyntheticOutstanding, ResidualAlpha)`. Their sum
    /// rises by exactly the alpha a short mints and falls by exactly the alpha a close buys
    /// back and burns; a default leaves it unchanged.
    #[must_use]
    pub fn lending_alpha_books(netuid: NetUid) -> (u64, u64) {
        (
            SyntheticOutstanding::<T>::get(netuid),
            ResidualAlpha::<T>::get(netuid),
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

    /// SPEC I5: charged interest never exceeds collateral, and every open position has exactly
    /// one scheduled charge. A charge is `min(due, C)` taken from a `u64` collateral, so the
    /// first half holds while custody still holds the collateral left (I3). The second half is
    /// checked here: `Due` has one entry per position, at the position's `charge_at`, no entry
    /// lies before `NextDue`, and a retry count belongs to an open position.
    ///
    /// The default half of I5 ("no short is stored with `C == 0` and interest due after the
    /// block's charges") is not true of storage as a whole, so only what is true is asserted:
    /// such a short may be stored when a touch left it so (a touch charges `min(due, C)` and
    /// never defaults, as in the model), when its charge was deferred by `MaxChargesPerBlock`
    /// or when its charge failed and waits in `InterestRetry`. In each case it still has its
    /// `Due` entry, checked below for every position, and that scheduled charge defaults it.
    /// That a scheduled charge never stores such a short is a property of the charge itself
    /// (`interest::charge_position`) and is asserted by the tests. A long has no default.
    fn check_i5_interest_schedule() -> Result<(), TryRuntimeError> {
        let next = NextDue::<T>::get();
        let mut scheduled = 0u64;
        for (at, id, ()) in Due::<T>::iter() {
            let pos = Positions::<T>::get(&id).ok_or("I5: Due entry without a position")?;
            ensure!(
                pos.charge_at == at,
                "I5: Due entry is not at the position's charge block"
            );
            ensure!(
                next.is_some_and(|next| next <= at),
                "I5: Due entry before NextDue"
            );
            scheduled = scheduled.saturating_add(1);
        }
        let mut open = 0u64;
        for (id, pos) in Positions::<T>::iter() {
            ensure!(
                Due::<T>::contains_key(pos.charge_at, &id),
                "I5: position without a scheduled charge"
            );
            open = open.saturating_add(1);
        }
        ensure!(
            scheduled == open,
            "I5: position with more than one scheduled charge"
        );
        for id in InterestRetry::<T>::iter_keys() {
            ensure!(
                Positions::<T>::contains_key(&id),
                "I5: retry count without a position"
            );
        }
        Ok(())
    }

    /// SPEC I23: the rate of every open position, recomputed from its stored `q`, `C`, `P` (or
    /// `D`, `A`, `C`) and the subnet's slow price on the live curve, lies in `apr_bps..=
    /// rate_cap_bps`. A stored position keeps no record of the rate it was last charged at (the
    /// charge reports it in `InterestCharged`), so the equality half of I23 is checked by the
    /// tests. The carried remainder of every accrual is below one rao (`math::interest::accrue`).
    fn check_i23_rate() -> Result<(), TryRuntimeError> {
        let curve = LendingParamsStore::<T>::get().rate_curve();
        ensure!(
            curve.apr_bps <= curve.cap_bps,
            "I23: rate_cap_bps below apr_bps"
        );
        for ((_, netuid, _), pos) in Positions::<T>::iter() {
            ensure!(
                pos.accrual.remainder < ACCRUAL_DEN,
                "I23: accrual remainder is a rao or more"
            );
            // A subnet without price rings fails I25; its positions have no rate to check.
            if let Ok(refs) = Self::price_refs(netuid) {
                let rate = pos.rate_bps(&refs, &curve);
                ensure!(
                    rate >= curve.apr_bps && rate <= curve.cap_bps,
                    "I23: rate outside apr_bps..=rate_cap_bps"
                );
            }
        }
        Ok(())
    }

    /// SPEC I19: on every subnet the pool's TAO reserve rose, across the interest credits, by
    /// exactly the interest charged from collateral.
    fn check_i19_interest_to_pool() -> Result<(), TryRuntimeError> {
        for total in InterestTotals::<T>::iter_values() {
            ensure!(
                total.to_pool == total.charged,
                "I19: pool TAO did not rise by the interest charged"
            );
        }
        Ok(())
    }

    /// SPEC 7.5: `PositionCount` is the number of open positions of both sides per subnet and
    /// `TotalPositions` their number on all subnets.
    fn check_position_counts() -> Result<(), TryRuntimeError> {
        let totals = Self::book_totals();
        ensure_book(
            "PositionCount is not the number of open positions",
            &totals,
            |total| total.positions,
            as_book(PositionCount::<T>::iter()),
        )?;
        let open = totals
            .values()
            .fold(0u128, |sum, total| sum.saturating_add(total.positions));
        ensure!(
            u128::from(TotalPositions::<T>::get()) == open,
            "TotalPositions is not the number of open positions"
        );
        Ok(())
    }

    /// `HolderPositions` lists exactly the open positions of each coldkey: every entry is an
    /// open position, every open position is listed once, and no list is stored empty. The
    /// list's type bounds it at `hooks::MAX_POSITIONS_PER_HOLDER`.
    fn check_holder_positions() -> Result<(), TryRuntimeError> {
        let mut listed = 0u32;
        for (who, held) in HolderPositions::<T>::iter() {
            ensure!(!held.is_empty(), "HolderPositions stores an empty list");
            for (index, (netuid, side)) in held.iter().enumerate() {
                ensure!(
                    Positions::<T>::contains_key((who.clone(), *netuid, *side)),
                    "HolderPositions lists a position that is not open"
                );
                ensure!(
                    !held
                        .iter()
                        .skip(index.saturating_add(1))
                        .any(|other| other.0 == *netuid),
                    "HolderPositions lists two positions on one subnet"
                );
                listed = listed.saturating_add(1);
            }
        }
        ensure!(
            listed == TotalPositions::<T>::get(),
            "HolderPositions does not list every open position"
        );
        Ok(())
    }

    /// `PositionsByNetuid` holds exactly the keys of `Positions`: every open position has its
    /// key in the index and the index has no other key, on every subnet and side. So the walk
    /// of a subnet's shorts over the index sees each of them once and nothing stale.
    fn check_position_index() -> Result<(), TryRuntimeError> {
        let mut open = 0u32;
        for (who, netuid, side) in Positions::<T>::iter_keys() {
            ensure!(
                PositionsByNetuid::<T>::contains_key((netuid, side, who)),
                "PositionsByNetuid misses an open position"
            );
            open = open.saturating_add(1);
        }
        let mut indexed = 0u32;
        for (netuid, side, who) in PositionsByNetuid::<T>::iter_keys() {
            ensure!(
                Positions::<T>::contains_key((who, netuid, side)),
                "PositionsByNetuid has a key with no open position"
            );
            indexed = indexed.saturating_add(1);
        }
        ensure!(
            indexed == open,
            "PositionsByNetuid does not have one key per open position"
        );
        Ok(())
    }

    /// SPEC I8, the part storage can show: on a subnet being dissolved, every short still open
    /// is still to be settled by the path the settlement is on, and nothing else is recorded.
    ///
    /// - Before the first settlement step and while the accrual walk runs, every short is ahead
    ///   of the settling walk, which starts at the head of the index.
    /// - While the settling walk runs, a short lies after the walk's cursor in the index, or is
    ///   queued in `SettlementRetry`.
    /// - In the retry pass every short left is queued in `SettlementRetry`.
    /// - Once the settlement is done the subnet has no short, no queued retry and no
    ///   `ResidualAlpha`.
    ///
    /// A queued retry is a short that is open, on a subnet whose settlement is running; a
    /// settlement progress belongs to a subnet with a dissolution record.
    fn check_i8_dissolution_settlement() -> Result<(), TryRuntimeError> {
        for (netuid, progress) in DissolutionSettlements::<T>::iter() {
            ensure!(
                DissolvingSubnets::<T>::contains_key(netuid),
                "I8: settlement progress without a dissolution record"
            );
            let passed = progress.cursor.as_ref().map(|last| {
                PositionsByNetuid::<T>::hashed_key_for((netuid, Side::Short, last.clone()))
            });
            for who in PositionsByNetuid::<T>::iter_key_prefix((netuid, Side::Short)) {
                let queued = SettlementRetry::<T>::contains_key(netuid, &who);
                let pending = match progress.stage {
                    SettlementStage::Accruing => true,
                    SettlementStage::Settling => {
                        let key = PositionsByNetuid::<T>::hashed_key_for((
                            netuid,
                            Side::Short,
                            who.clone(),
                        ));
                        queued || passed.as_ref().is_none_or(|passed| key > *passed)
                    }
                    SettlementStage::Retrying => queued,
                    SettlementStage::Done => false,
                };
                ensure!(
                    pending,
                    "I8: a short of a dissolving subnet is neither settled nor pending"
                );
            }
            if progress.stage == SettlementStage::Done {
                ensure!(
                    !ResidualAlpha::<T>::contains_key(netuid),
                    "I8: ResidualAlpha left after the settlement"
                );
            }
        }
        for (netuid, who, _) in SettlementRetry::<T>::iter() {
            ensure!(
                Positions::<T>::contains_key((who, netuid, Side::Short)),
                "I8: settlement retry without an open short"
            );
            ensure!(
                DissolutionSettlements::<T>::get(netuid)
                    .is_some_and(|progress| progress.stage != SettlementStage::Done),
                "I8: settlement retry on a subnet whose settlement is not running"
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
