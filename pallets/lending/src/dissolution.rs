//! Shorts at subnet dissolution (SPEC section 1 items 7-8, section 7.4 phase E, P12, P16, I8;
//! Jacob, Nov 27).
//!
//! `pallet_subtensor` flags a subnet for dissolution once ([`crate::hooks::LendingInterface::on_dissolve_flag`],
//! with the pool's TAO at that moment). From the flag until the subnet's record is dropped
//! (`on_dissolve_done`) no short of the subnet opens, grows, closes or takes collateral
//! (`positions::ensure_not_dissolving`), and its scheduled charges do not run (`interest`): the
//! dissolution alone settles the shorts.
//!
//! The cleanup then calls [`crate::hooks::LendingInterface::on_dissolve`] before the pot is
//! distributed to stakers, once per cleanup step until it reports that it is done. The steps
//! together settle every short of the subnet:
//!
//! 1. each short's interest is accrued to the block of the first step at the curve rate and
//!    credited to the subnet's pool TAO (`SubnetTAO`, not `TotalStake`: the flag already took the
//!    pool out of `TotalStake`);
//! 2. the dereg rate `r = min(T_flag, T_ema, T_A) / all_alpha` is read once, after every interest
//!    credit, so `T_A` includes them (P12);
//! 3. each short owes `q * r` (zone part per [`P16_ZONE_SETTLEMENT`]). It pays `min(owed, C + P)`
//!    from its collateral and proceeds into the subnet pot, and the rest of its custody, the
//!    deposit included, goes back to the holder. The minted alpha `q` leaves `SyntheticOutstanding`
//!    and stays in the pool, so no alpha is minted or burned (SPEC I2, I7).
//!
//! The walk. The shorts are read off the per-subnet index `PositionsByNetuid`, prefix
//! `(netuid, Short)`, so a step reads that subnet's shorts and no other position. A step visits
//! at most [`MAX_SETTLEMENTS_PER_BLOCK`] shorts and at most what its `WeightMeter` affords at the
//! measured weight of a visit (`WeightInfo::on_dissolve`); the stage, the block of the accrual,
//! the rate and the cursor are kept in `DissolutionSettlements` ([`Settlement`]) for the next
//! step. The stages run in order: accrue every short (1), read `r` (2), settle every short (3),
//! then try again the shorts that failed.
//!
//! Failures. Every visited short ends settled (`ShortSettledOnDissolution`, the position gone) or
//! failed: `ShortSettlementFailed` with the error, the short as it was (the visit runs in a
//! storage layer) and an entry in `SettlementRetry`. Once the walk has ended, each later step
//! tries the queued shorts again, one attempt per short per step, and reports each that fails
//! again. The settlement is done, and the cleanup goes on to pay the pot, only when the queue is
//! empty. Terminal case: a short whose settlement can never succeed (its custody no longer holds
//! what the position records, or the pool or the holder cannot be credited) is reported on every
//! step and holds the subnet's cleanup at the lending phase, with its custody untouched, until
//! the cause is removed. It is never dropped and never settled twice; the pot of the subnet is
//! not paid without it (SPEC I8).
//!
//! The settled TAO is recorded in [`Dissolving::settled_tao`]. `pallet_subtensor` pays it to the
//! stakers and takes it out of the pool reserve before the owner refund reads the pot, so the
//! refund is computed as it is for the same subnet with no lending (see the subtensor side).
//!
//! Longs are not settled here; see the TODO on `on_dissolve` in `hooks`.

use alloc::vec::Vec;
use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use frame_support::{
    dispatch::DispatchResult,
    ensure,
    storage::with_storage_layer,
    weights::{Weight, WeightMeter},
};
use frame_system::pallet_prelude::BlockNumberFor;
use scale_info::TypeInfo;
use sp_runtime::DispatchError;
use subtensor_macros::freeze_struct;
use subtensor_runtime_common::{NetUid, TaoBalance};

use crate::{
    Config, DissolutionSettlements, DissolvingSubnets, Error, Event, LendingParamsStore,
    LendingPoolInterface, Pallet, PoolTaoEma, Positions, PositionsByNetuid, ResidualAlpha,
    SettlementRetry, SyntheticOutstanding, WeightInfo,
    math::{
        PriceRefs,
        default::Launch,
        dereg::{rate, short_owed},
        fixed::{Price, mul_price_floor},
        rate::RateCurve,
    },
    types::{Leg, Position, PositionId, Side},
};

/// The most shorts one step of a subnet's dissolution settlement visits (one call of
/// `on_dissolve`, so one block of the cleanup). A visit accrues a short, settles it, or tries a
/// failed one again. `WeightInfo::on_dissolve` is measured up to this many visits, each of them
/// the costliest kind (an accrual and a settlement), and a step takes fewer when its meter
/// affords fewer. A larger book is settled over several steps.
pub const MAX_SETTLEMENTS_PER_BLOCK: u32 = 64;

/// P16 zone settlement: a short minted in the dereg zone settles at its own proceeds, the older
/// alpha at `r` (`math::dereg::short_owed`). Pending Jacob. The positions carry no zone fields
/// yet, so the zone part is zero and this switch changes no settlement until they do. Removing P16
/// is this one line plus the test that runs both settings ([`settle_short_amount_with`]).
pub const P16_ZONE_SETTLEMENT: bool = true; // pending Jacob

/// What a short owes the pot at dereg, with the P16 switch of the build ([`P16_ZONE_SETTLEMENT`]).
#[must_use]
pub fn settle_short_amount(q: u64, zone_minted: u64, zone_proceeds: u64, r: Price) -> u64 {
    settle_short_amount_with(P16_ZONE_SETTLEMENT, q, zone_minted, zone_proceeds, r)
}

/// [`settle_short_amount`] under an explicit P16 setting. With the zone on, the zone part settles
/// at its proceeds and the rest at `r`; with it off, every short settles at `r`.
#[must_use]
pub fn settle_short_amount_with(
    zone_settlement: bool,
    q: u64,
    zone_minted: u64,
    zone_proceeds: u64,
    r: Price,
) -> u64 {
    if zone_settlement {
        short_owed(q, zone_minted, zone_proceeds, r)
    } else {
        mul_price_floor(q, r)
    }
}

/// The lending record of a subnet being dissolved, kept until the owner refund has read it.
#[freeze_struct("5a1c5445e7a5ae40")]
#[derive(
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Debug,
    Encode,
    Decode,
    DecodeWithMemTracking,
    MaxEncodedLen,
    TypeInfo,
)]
pub struct Dissolving {
    /// The pool's TAO when the subnet was flagged, in rao: `T_flag`.
    pub t_flag: u64,
    /// The lending alpha of the subnet at the flag (`SyntheticOutstanding` plus
    /// `ResidualAlpha`). Frozen until the refund, so the legacy owner-cut deduction reads the
    /// same value as it does for the subnet with no lending, whose issuance it was taken from.
    pub lending_alpha: u64,
    /// TAO the shorts settled into the pot, in rao: `S`. Paid to the stakers, not the refund.
    pub settled_tao: u64,
}

/// Where the settlement of a dissolving subnet's shorts stands (see the module doc).
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
    Encode,
    Decode,
    DecodeWithMemTracking,
    MaxEncodedLen,
    TypeInfo,
)]
pub enum SettlementStage {
    /// Accruing each short's interest to [`Settlement::at`]; the rate is not read yet.
    Accruing,
    /// The rate is read; settling each short at it.
    Settling,
    /// The walk has ended; trying again the shorts queued in `SettlementRetry`.
    Retrying,
    /// Every short of the subnet is settled.
    Done,
}

/// The progress of the settlement of one dissolving subnet's shorts, kept between cleanup steps.
#[freeze_struct("afc88a3e56781c70")]
#[derive(
    Clone, PartialEq, Eq, Debug, Encode, Decode, DecodeWithMemTracking, MaxEncodedLen, TypeInfo,
)]
pub struct Settlement<AccountId, BlockNumber> {
    /// The stage the next step resumes in.
    pub stage: SettlementStage,
    /// The block of the first step: every short's interest is accrued to it, whichever step
    /// reaches the short.
    pub at: BlockNumber,
    /// The dereg rate `r` (P12), read once when the accrual walk ended. Zero until then.
    pub rate: Price,
    /// The last holder the walk of the current stage visited; the next step resumes after its
    /// key. `None` at the start of a stage.
    pub cursor: Option<AccountId>,
}

/// What one settlement step may still do and what it has done.
struct Step {
    /// Visits the meter and [`MAX_SETTLEMENTS_PER_BLOCK`] allow in this step.
    budget: u32,
    /// Visits made.
    visits: u32,
    /// TAO the shorts settled in this step paid into the pot, in rao.
    paid: u64,
}

impl Step {
    /// Visits left in this step.
    fn room(&self) -> usize {
        usize::try_from(self.budget.saturating_sub(self.visits)).unwrap_or(usize::MAX)
    }
}

/// What a settlement step reads once for every short it visits.
struct Terms<BlockNumber> {
    /// The block the shorts are accrued to.
    at: BlockNumber,
    /// The price references of the subnet. Without the price rings a short settles with no
    /// accrual; the rings of a live subnet always exist (SPEC I25), so `None` only guards a
    /// subnet that never had them.
    refs: Option<PriceRefs>,
    /// The live rate curve.
    curve: RateCurve,
}

impl<T: Config> Pallet<T> {
    /// The lending alpha of `netuid` as it stands: the open shorts' `q` and the defaulted shorts'
    /// residual alpha.
    #[must_use]
    pub(crate) fn live_lending_alpha(netuid: NetUid) -> u64 {
        SyntheticOutstanding::<T>::get(netuid).saturating_add(ResidualAlpha::<T>::get(netuid))
    }

    /// Credits `amount` TAO from `from` to the subnet's pool reserve while the subnet is being
    /// dissolved: the TAO joins `SubnetTAO` and not `TotalStake`, which the flag already took the
    /// pool out of. Refuses a pool that credited any other amount.
    pub(crate) fn credit_dissolving(
        netuid: NetUid,
        from: &T::AccountId,
        amount: u64,
    ) -> DispatchResult {
        let moved = T::Pool::credit_dissolving_tao(netuid, from, amount)?;
        ensure!(moved == amount, Error::<T>::QuoteMismatch);
        Ok(())
    }

    /// The flag of `do_dissolve_network` (see the module doc). Records the subnet's lending
    /// values, for a subnet with no lending too: the record is what closes the subnet to the
    /// short calls until `on_dissolve_done` drops it.
    pub fn record_dissolve_flag(netuid: NetUid, pool_tao: TaoBalance) {
        DissolvingSubnets::<T>::insert(
            netuid,
            Dissolving {
                t_flag: pool_tao.into(),
                lending_alpha: Self::live_lending_alpha(netuid),
                settled_tao: 0,
            },
        );
    }

    /// The most visits a settlement step may make on `meter`: the largest count up to
    /// [`MAX_SETTLEMENTS_PER_BLOCK`] whose measured weight the meter can take, or `None` when it
    /// cannot take one visit.
    fn settlement_budget(meter: &WeightMeter) -> Option<u32> {
        (1..=MAX_SETTLEMENTS_PER_BLOCK)
            .rev()
            .find(|visits| meter.can_consume(T::WeightInfo::on_dissolve(*visits)))
    }

    /// One step of the settlement of the shorts of `netuid` (see the module doc). Returns whether
    /// the settlement is done: `false` while shorts are left to walk or to try again, and also
    /// when `meter` cannot take one visit, in which case nothing is read or written.
    ///
    /// The step takes the measured weight of the visits it made from `meter`
    /// (`WeightInfo::on_dissolve`, at least one visit: the step's own reads and writes are part
    /// of that measurement).
    pub fn settle_shorts_at_dissolution(netuid: NetUid, meter: &mut WeightMeter) -> bool {
        let Some(budget) = Self::settlement_budget(meter) else {
            return false;
        };
        let now = frame_system::Pallet::<T>::block_number();
        let mut progress = DissolutionSettlements::<T>::get(netuid).unwrap_or(Settlement {
            stage: SettlementStage::Accruing,
            at: now,
            rate: Price::from_bits(0),
            cursor: None,
        });
        if progress.stage == SettlementStage::Done {
            meter.consume(T::WeightInfo::on_dissolve(1));
            return true;
        }
        // The frozen values: the flag's, or the subnet's as it stands if the flag never ran.
        let mut record = DissolvingSubnets::<T>::get(netuid).unwrap_or_else(|| Dissolving {
            t_flag: T::Pool::pool_tao(netuid),
            lending_alpha: Self::live_lending_alpha(netuid),
            settled_tao: 0,
        });
        let terms = Terms {
            at: progress.at,
            refs: Self::price_refs(netuid).ok(),
            curve: LendingParamsStore::<T>::get().rate_curve(),
        };
        let mut step = Step {
            budget,
            visits: 0,
            paid: 0,
        };

        if progress.stage == SettlementStage::Accruing {
            Self::accrual_stage(netuid, &mut progress, &terms, &mut step);
            if progress.stage == SettlementStage::Settling {
                // The dereg rate (P12), read once after the interest credits. No EMA means no
                // constraint from it: the rate is then `min(T_flag, T_A)`.
                let t_a = T::Pool::pool_tao(netuid);
                let all_alpha = u128::from(T::Pool::alpha_issuance(netuid));
                let t_ema = PoolTaoEma::<T>::get(netuid).unwrap_or(u64::MAX);
                progress.rate = rate(record.t_flag, t_ema, t_a, all_alpha);
            }
        }
        if progress.stage == SettlementStage::Settling {
            Self::settling_stage(netuid, &mut progress, &terms, &mut step);
        } else if progress.stage == SettlementStage::Retrying {
            Self::retry_stage(netuid, &mut progress, &terms, &mut step);
        }

        let done = progress.stage == SettlementStage::Done;
        if done {
            // The residual alpha of the defaulted shorts stays in the pool's issuance.
            ResidualAlpha::<T>::remove(netuid);
        }
        record.settled_tao = record.settled_tao.saturating_add(step.paid);
        DissolvingSubnets::<T>::insert(netuid, record);
        DissolutionSettlements::<T>::insert(netuid, progress);
        meter.consume(T::WeightInfo::on_dissolve(step.visits.max(1)));
        done
    }

    /// The holders of the next shorts of `netuid` in the per-subnet index, after `cursor`: at
    /// most `room` of them, and whether they are the last of the walk.
    fn next_shorts(
        netuid: NetUid,
        cursor: Option<&T::AccountId>,
        room: usize,
    ) -> (Vec<T::AccountId>, bool) {
        let prefix = (netuid, Side::Short);
        let keys = match cursor {
            Some(last) => PositionsByNetuid::<T>::iter_key_prefix_from(
                prefix,
                PositionsByNetuid::<T>::hashed_key_for((netuid, Side::Short, last.clone())),
            ),
            None => PositionsByNetuid::<T>::iter_key_prefix(prefix),
        };
        Self::take_with_end(keys, room)
    }

    /// The first `room` items of `keys` and whether `keys` has no more after them. Reads one
    /// key past `room`, so a walk that ends exactly on a step's last visit ends in that step.
    fn take_with_end<I: Iterator<Item = T::AccountId>>(
        keys: I,
        room: usize,
    ) -> (Vec<T::AccountId>, bool) {
        let mut taken: Vec<T::AccountId> = keys.take(room.saturating_add(1)).collect();
        let ended = taken.len() <= room;
        taken.truncate(room);
        (taken, ended)
    }

    /// Stage 1: accrues the next shorts of the walk to `terms.at`. When the walk ends the stage
    /// becomes `Settling`, and the caller reads the rate.
    fn accrual_stage(
        netuid: NetUid,
        progress: &mut Settlement<T::AccountId, BlockNumberFor<T>>,
        terms: &Terms<BlockNumberFor<T>>,
        step: &mut Step,
    ) {
        let (holders, ended) = Self::next_shorts(netuid, progress.cursor.as_ref(), step.room());
        for who in holders {
            step.visits = step.visits.saturating_add(1);
            let id = (who.clone(), netuid, Side::Short);
            if let Err(error) = Self::accrue_for_settlement(&id, terms) {
                Self::record_settlement_failure(netuid, &who, error);
            }
            progress.cursor = Some(who);
        }
        if ended {
            progress.stage = SettlementStage::Settling;
            progress.cursor = None;
        }
    }

    /// Stage 3: settles the next shorts of the walk at the rate. When the walk ends the
    /// settlement is done if no short is queued to be tried again; otherwise the next step
    /// starts the retry pass.
    fn settling_stage(
        netuid: NetUid,
        progress: &mut Settlement<T::AccountId, BlockNumberFor<T>>,
        terms: &Terms<BlockNumberFor<T>>,
        step: &mut Step,
    ) {
        let (holders, ended) = Self::next_shorts(netuid, progress.cursor.as_ref(), step.room());
        for who in holders {
            Self::settlement_visit(netuid, &who, progress.rate, terms, step);
            progress.cursor = Some(who);
        }
        if ended {
            progress.cursor = None;
            progress.stage = if SettlementRetry::<T>::iter_key_prefix(netuid)
                .next()
                .is_none()
            {
                SettlementStage::Done
            } else {
                SettlementStage::Retrying
            };
        }
    }

    /// The retry pass: tries the next queued shorts again. A pass that reaches the end of the
    /// queue starts over in the next step, so a short is tried once per pass; the settlement is
    /// done when a pass ends with the queue empty.
    fn retry_stage(
        netuid: NetUid,
        progress: &mut Settlement<T::AccountId, BlockNumberFor<T>>,
        terms: &Terms<BlockNumberFor<T>>,
        step: &mut Step,
    ) {
        let (holders, ended) = match progress.cursor.as_ref() {
            Some(last) => Self::take_with_end(
                SettlementRetry::<T>::iter_key_prefix_from(
                    netuid,
                    SettlementRetry::<T>::hashed_key_for(netuid, last),
                ),
                step.room(),
            ),
            None => Self::take_with_end(SettlementRetry::<T>::iter_key_prefix(netuid), step.room()),
        };
        for who in holders {
            Self::settlement_visit(netuid, &who, progress.rate, terms, step);
            progress.cursor = Some(who);
        }
        if ended {
            progress.cursor = None;
            if SettlementRetry::<T>::iter_key_prefix(netuid)
                .next()
                .is_none()
            {
                progress.stage = SettlementStage::Done;
            }
        }
    }

    /// One visit of the settling walk or the retry pass: settles the short of `who` at `r`, or
    /// records that it failed.
    fn settlement_visit(
        netuid: NetUid,
        who: &T::AccountId,
        r: Price,
        terms: &Terms<BlockNumberFor<T>>,
        step: &mut Step,
    ) {
        step.visits = step.visits.saturating_add(1);
        let id = (who.clone(), netuid, Side::Short);
        match with_storage_layer(|| Self::settle_short(&id, r, terms)) {
            Ok(paid) => step.paid = step.paid.saturating_add(paid),
            Err(error) => Self::record_settlement_failure(netuid, who, error),
        }
    }

    /// Queues the short of `who` to be tried again and reports the failure. The caller ran the
    /// failed attempt in a storage layer, so the short is as it was.
    fn record_settlement_failure(netuid: NetUid, who: &T::AccountId, error: DispatchError) {
        // An index key or a retry entry with no position behind it has nothing to settle. The
        // index and the queue are kept exact (`try_state`), so this only clears a stale key.
        if !Positions::<T>::contains_key((who.clone(), netuid, Side::Short)) {
            PositionsByNetuid::<T>::remove((netuid, Side::Short, who.clone()));
            SettlementRetry::<T>::remove(netuid, who);
            return;
        }
        SettlementRetry::<T>::mutate(netuid, who, |attempts| {
            *attempts = Some(attempts.unwrap_or(0).saturating_add(1));
        });
        Self::deposit_event(Event::ShortSettlementFailed {
            who: who.clone(),
            netuid,
            error,
        });
    }

    /// Accrues the short `id` to `terms.at` and credits the interest to the dissolving pool.
    /// Returns the short as stored after it. A short already accrued to that block, and any
    /// short of a subnet without price rings, is returned as it is.
    fn accrue_for_settlement(
        id: &PositionId<T::AccountId>,
        terms: &Terms<BlockNumberFor<T>>,
    ) -> Result<Position<BlockNumberFor<T>>, DispatchError> {
        let pos = Positions::<T>::get(id).ok_or(Error::<T>::NoPosition)?;
        let Some(refs) = terms.refs else {
            return Ok(pos);
        };
        if pos.accrual.last_accrued >= terms.at {
            return Ok(pos);
        }
        let (settled, charge) = pos.settle_with::<Launch>(&refs, &terms.curve, terms.at);
        let custody = Self::position_custody(id, pos.opened_at);
        with_storage_layer(|| {
            Self::pay_interest_via(
                &id.0,
                id.1,
                Side::Short,
                &custody,
                charge.amount,
                charge.rate_bps,
                true,
            )
        })?;
        Positions::<T>::insert(id, settled);
        Ok(settled)
    }

    /// Settles the short `id` at the rate `r` and removes it. Returns what it paid into the pot.
    /// The short is accrued first if the accrual walk did not accrue it (its accrual failed
    /// there, or a coldkey swap moved it behind the walk).
    fn settle_short(
        id: &PositionId<T::AccountId>,
        r: Price,
        terms: &Terms<BlockNumberFor<T>>,
    ) -> Result<u64, DispatchError> {
        let pos = Self::accrue_for_settlement(id, terms)?;
        let Leg::Short { minted, proceeds } = pos.leg else {
            return Err(Error::<T>::NoPosition.into());
        };
        let custody = Self::position_custody(id, pos.opened_at);
        let holdings = pos.collateral.saturating_add(proceeds);
        let owed = settle_short_amount(minted, 0, 0, r);
        // The payment is drawn from the collateral and then the proceeds (the total is what
        // moves; the order only names the parts of it).
        let paid = owed.min(holdings);
        let refund = holdings.saturating_sub(paid).saturating_add(pos.deposit);
        Self::settle_one_short(id, &pos, &custody, minted, paid, refund, r)?;
        Ok(paid)
    }

    /// Pays one short's settlement and removes it: the paid amount from custody into the pot,
    /// the rest of custody to the holder, `q` off the short book and the position with its
    /// records.
    fn settle_one_short(
        id: &PositionId<T::AccountId>,
        pos: &Position<BlockNumberFor<T>>,
        custody: &T::AccountId,
        minted: u64,
        paid: u64,
        refund: u64,
        r: Price,
    ) -> DispatchResult {
        let (who, netuid, _) = id;
        if paid > 0 {
            Self::credit_dissolving(*netuid, custody, paid)?;
        }
        if refund > 0 {
            Self::move_tao(custody, who, refund)?;
        }
        let book = SyntheticOutstanding::<T>::get(netuid)
            .checked_sub(minted)
            .ok_or(Error::<T>::Overflow)?;
        SyntheticOutstanding::<T>::insert(netuid, book);
        Self::forget_position(id, pos.charge_at);
        Self::deposit_event(Event::ShortSettledOnDissolution {
            who: who.clone(),
            netuid: *netuid,
            price: r,
            paid,
        });
        Ok(())
    }

    /// The measured weight of a settlement step that makes `visits` visits
    /// (`WeightInfo::on_dissolve`); a step is charged for one visit at least.
    #[must_use]
    pub fn dissolution_weight(visits: u32) -> Weight {
        T::WeightInfo::on_dissolve(visits.max(1))
    }
}
