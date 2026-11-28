//! Interest charging (DESIGN-v2 section 5; SPEC section 1 item 2, P3, P4, P15, I5, I19).
//!
//! Every open position has one entry in `Due`, at its `charge_at`. `on_initialize` drains the
//! buckets up to the current block in order, at most `MaxChargesPerBlock` charges per block;
//! `NextDue` is the first bucket not yet drained, so a backlog carries over. A charge reads the
//! position's rate from the curve `min(cap, apr * max(1, u^k))`, accrues the position to the current
//! block at that rate, takes `min(due, C)` from its collateral and credits the subnet pool's
//! TAO reserve. The curve's parameters are read when the charge runs, so a root change applies
//! from each position's next accrual and no setter touches a position.
//!
//! The cadence is the model's (`model/v21/core.py`): a position is charged at every block
//! where `(block - opened_at) % charge_interval_blocks == 0`, with the interval live at that
//! block. After a successful charge at `now` the next one is the first grid block after `now`,
//! `opened_at + k * charge_interval_blocks` (see [`next_charge_block`]). A
//! charge that ran late therefore rejoins the grid at its next block instead of drifting by
//! its lateness, and a change of interval applies from the next charge. A failed charge is
//! tried again [`RETRY_DELAY_BLOCKS`] later, and the next successful charge realigns.
//!
//! The operations of `positions` charge on touch with the same two steps as the queue:
//! [`Position::settle`] for the amounts and [`Pallet::pay_interest`] for the credit. A touch
//! does not move the position's `Due` entry (SPEC P15: queued entries are not moved).
//!
//! Short default (SPEC section 1 item 4, I2, I5). The scheduled charge is the only place a
//! position defaults. Right after it paid what the charge policy takes (`math::default`), in
//! the same storage layer, it asks the policy whether the short defaults: at launch, when its
//! collateral is zero with interest still due (Jacob, Nov 27). A default credits what custody
//! still holds apart from the deposit (the proceeds `P`) to the pool's TAO reserve through the
//! pool-internal credit, moves the short's `q` from `SyntheticOutstanding` to `ResidualAlpha`
//! (no alpha is minted or burned), burns the deposit, forgives the unpaid interest and removes
//! the position with its custody record, counts and retry count. A touch never defaults: the
//! model's touches charge `min(due, C)` and leave the rest due (`model/v21/core.py`), so a short
//! a touch left with `C == 0` and interest due defaults at its next scheduled charge. There is
//! no default of a long.

use alloc::vec::Vec;
use frame_support::{dispatch::DispatchResult, storage::with_storage_layer, traits::Get};
use frame_system::pallet_prelude::BlockNumberFor;
use sp_runtime::{
    DispatchError, SaturatedConversion,
    traits::{One, Saturating},
};
use subtensor_runtime_common::NetUid;

use crate::{
    Config, Due, Error, Event, InterestRetry, InterestTotals, LendingParamsStore,
    LendingPoolInterface, NextDue, Pallet, Positions, ResidualAlpha, SyntheticOutstanding,
    TotalPositions,
    math::{
        default::{ChargePolicy, Launch},
        interest::next_charge_block,
        rate::RateCurve,
    },
    params::LendingParams,
    types::{Leg, Position, PositionId, Side},
};

/// What a scheduled charge left of a position.
enum Charged<BlockNumber> {
    /// The position stays open, settled to the charge's block.
    Open(Position<BlockNumber>),
    /// The short defaulted and is gone.
    Defaulted,
}

/// What one `collect_due` did, for the weight of `on_initialize`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Collected {
    /// Scheduled charges run, the defaulting ones included.
    pub charges: u32,
    /// `Due` buckets read.
    pub buckets: u32,
    /// Charges that ended in a default.
    pub defaults: u32,
}

/// Blocks a scheduled charge that failed waits before it is tried again.
pub const RETRY_DELAY_BLOCKS: u32 = 10;

/// `Due` buckets `on_initialize` visits per block. The cursor moves at most this far per block,
/// so catching up after a backlog costs a bounded number of bucket reads.
pub const MAX_DUE_BUCKETS_PER_BLOCK: u32 = 64;

impl<T: Config> Pallet<T> {
    /// Moves an interest charge from a position's custody into the subnet pool's TAO reserve
    /// (SPEC P4, P15) and adds it to the subnet's `InterestTotals`. The credit is pool-internal:
    /// it goes through `credit_pool_tao` alone, so it is neither flow nor volume (SPEC I19).
    /// `rate_bps` is the rate the accrual behind the charge read, for the event. A charge of
    /// nothing moves nothing and emits nothing.
    pub(crate) fn pay_interest(
        who: &T::AccountId,
        netuid: NetUid,
        side: Side,
        custody: &T::AccountId,
        amount: u64,
        rate_bps: u32,
    ) -> DispatchResult {
        if amount == 0 {
            return Ok(());
        }
        let before = T::Pool::pool_tao(netuid);
        Self::credit_pool(netuid, custody, amount)?;
        let risen = T::Pool::pool_tao(netuid).saturating_sub(before);
        InterestTotals::<T>::mutate(netuid, |total| {
            total.charged = total.charged.saturating_add(u128::from(amount));
            total.to_pool = total.to_pool.saturating_add(u128::from(risen));
        });
        Self::deposit_event(Event::InterestCharged {
            who: who.clone(),
            netuid,
            side,
            to_pool: amount,
            rate_bps,
        });
        Ok(())
    }

    /// The block of a charge scheduled `delay` blocks after `now`.
    pub(crate) fn charge_block(now: BlockNumberFor<T>, delay: u32) -> BlockNumberFor<T> {
        now.saturating_add(delay.into())
    }

    /// The first block after `now` on the charge grid of a position opened at `opened_at` with
    /// `interval` blocks (SPEC P15; the model's `(block - opened_at) % interval == 0`).
    pub(crate) fn grid_block(
        opened_at: BlockNumberFor<T>,
        now: BlockNumberFor<T>,
        interval: u32,
    ) -> BlockNumberFor<T> {
        let opened: u64 = opened_at.saturated_into();
        let now: u64 = now.saturated_into();
        next_charge_block(opened, now, interval).saturated_into()
    }

    /// Adds the `Due` entry of `id` at `at` and keeps `NextDue` at or before it.
    pub(crate) fn schedule_charge(id: &PositionId<T::AccountId>, at: BlockNumberFor<T>) {
        Due::<T>::insert(at, id, ());
        NextDue::<T>::mutate(|next| {
            if next.is_none_or(|block| at < block) {
                *next = Some(at);
            }
        });
    }

    /// Removes the `Due` entry of a position that closed, with its retry count (SPEC P15).
    pub(crate) fn unschedule_charge(id: &PositionId<T::AccountId>, at: BlockNumberFor<T>) {
        Due::<T>::remove(at, id);
        InterestRetry::<T>::remove(id);
    }

    /// The weight one default adds to a scheduled charge, counted by hand until the charge is
    /// measured with its default branch: the credit of the proceeds to the pool and the burn of
    /// the deposit (two transfers and the reserve), the two alpha books, the counts, the
    /// holder's list and the removals.
    pub(crate) fn default_weight() -> frame_support::weights::Weight {
        T::DbWeight::get().reads_writes(12, 14)
    }

    /// Runs the scheduled charges due at or before `now`, oldest bucket first, and returns how
    /// many it ran, how many of them defaulted a short and how many buckets it read. It stops
    /// at `MaxChargesPerBlock` charges or [`MAX_DUE_BUCKETS_PER_BLOCK`] buckets; `NextDue` keeps
    /// the place for the next block (SPEC P15). A default is part of its charge, so the same
    /// budget bounds the defaults of a block.
    pub(crate) fn collect_due(now: BlockNumberFor<T>) -> Collected {
        let Some(mut cursor) = NextDue::<T>::get() else {
            return Collected::default();
        };
        if cursor > now {
            return Collected::default();
        }
        let lending = LendingParamsStore::<T>::get();
        let budget = T::MaxChargesPerBlock::get();
        let mut charges = 0u32;
        let mut buckets = 0u32;
        let mut defaults = 0u32;
        for _ in 0..MAX_DUE_BUCKETS_PER_BLOCK {
            if cursor > now {
                break;
            }
            buckets = buckets.saturating_add(1);
            let room = usize::try_from(budget.saturating_sub(charges)).unwrap_or(usize::MAX);
            let ids: Vec<_> = Due::<T>::iter_key_prefix(cursor).take(room).collect();
            for id in &ids {
                Due::<T>::remove(cursor, id);
                if Self::charge_due::<Launch>(id, now, &lending) {
                    defaults = defaults.saturating_add(1);
                }
                charges = charges.saturating_add(1);
            }
            if charges >= budget {
                // The bucket may hold more: the next block resumes here.
                break;
            }
            cursor = cursor.saturating_add(One::one());
        }
        if TotalPositions::<T>::get() == 0 {
            NextDue::<T>::kill();
        } else {
            NextDue::<T>::put(cursor);
        }
        Collected {
            charges,
            buckets,
            defaults,
        }
    }

    /// Charges one position whose `Due` entry was just taken, under the charge policy `P`, and
    /// schedules its next charge or defaults it. Returns whether it defaulted.
    ///
    /// A charge that succeeds on a position that stays open schedules the first block after
    /// `now` on the position's grid, read with the interval live now, so a late charge rejoins
    /// the grid and an interval change applies from here (SPEC P15). A charge after which the
    /// policy defaults the short removes it in the same storage layer (SPEC section 1 item 4).
    /// A charge or default that fails leaves the position as it was, counts in `InterestRetry`
    /// and is tried again [`RETRY_DELAY_BLOCKS`] after `now`.
    pub(crate) fn charge_due<P: ChargePolicy>(
        id: &PositionId<T::AccountId>,
        now: BlockNumberFor<T>,
        lending: &LendingParams,
    ) -> bool {
        let Some(pos) = Positions::<T>::get(id) else {
            return false;
        };
        let charged =
            with_storage_layer(|| Self::charge_position::<P>(id, pos, &lending.rate_curve(), now));
        let (pos, charge_at) = match charged {
            Ok(Charged::Defaulted) => return true,
            Ok(Charged::Open(settled)) => {
                InterestRetry::<T>::remove(id);
                let next = Self::grid_block(settled.opened_at, now, lending.charge_interval_blocks);
                (settled, next)
            }
            Err(_) => {
                InterestRetry::<T>::mutate(id, |failed| *failed = failed.saturating_add(1));
                (pos, Self::charge_block(now, RETRY_DELAY_BLOCKS))
            }
        };
        Positions::<T>::insert(id, Position { charge_at, ..pos });
        Self::schedule_charge(id, charge_at);
        false
    }

    /// Accrues `pos` to `now` at its rate on `curve`, on its notional (`q * TWAP_median` for a
    /// short, the debt for a long), takes what the policy `P` names from it (`min(due, C)` at
    /// launch) and pays it to the pool (SPEC section 1 item 2, I5). Then, for a short the
    /// policy defaults, runs the default. Returns the settled position, which the caller
    /// stores, or that it defaulted.
    fn charge_position<P: ChargePolicy>(
        id: &PositionId<T::AccountId>,
        pos: Position<BlockNumberFor<T>>,
        curve: &RateCurve,
        now: BlockNumberFor<T>,
    ) -> Result<Charged<BlockNumberFor<T>>, DispatchError> {
        let (who, netuid, side) = id;
        let refs = Self::price_refs(*netuid)?;
        let (settled, charge) = pos.settle_with::<P>(&refs, curve, now);
        let custody = Self::position_custody(id, pos.opened_at);
        Self::pay_interest(
            who,
            *netuid,
            *side,
            &custody,
            charge.amount,
            charge.rate_bps,
        )?;
        if let Leg::Short { minted, proceeds } = settled.leg
            && P::should_default(settled.collateral, proceeds, settled.accrual.due)
        {
            Self::default_short(id, &settled, minted, proceeds, &custody)?;
            return Ok(Charged::Defaulted);
        }
        Ok(Charged::Open(settled))
    }

    /// Defaults a short whose scheduled charge was just paid (SPEC section 1 item 4):
    ///
    /// 1. what custody holds apart from the deposit goes to the pool's TAO reserve by the
    ///    pool-internal credit, which is neither flow nor volume (SPEC I18, I19). That is the
    ///    proceeds `P`; the collateral is zero at a default, and any left under another policy
    ///    goes with it, as the model sends `C + P` (`model/v201x/core.py`, `_default_short`).
    ///    It is not interest and is not counted in `InterestTotals`;
    /// 2. the deposit is burned;
    /// 3. `q` moves from `SyntheticOutstanding` to `ResidualAlpha`: the alpha was sold at the
    ///    open and stays in circulation, so none is minted or burned (SPEC I2);
    /// 4. the unpaid interest is forgiven with the position, which is removed with its custody
    ///    record, counts, holder entry, `Due` entry and retry count.
    fn default_short(
        id: &PositionId<T::AccountId>,
        pos: &Position<BlockNumberFor<T>>,
        minted: u64,
        proceeds: u64,
        custody: &T::AccountId,
    ) -> DispatchResult {
        let (who, netuid, _) = id;
        let proceeds_to_pool = pos
            .collateral
            .checked_add(proceeds)
            .ok_or(Error::<T>::Overflow)?;
        if proceeds_to_pool > 0 {
            Self::credit_pool(*netuid, custody, proceeds_to_pool)?;
        }
        if pos.deposit > 0 {
            let burned = T::Pool::burn_tao(custody, pos.deposit)?;
            frame_support::ensure!(burned == pos.deposit, Error::<T>::QuoteMismatch);
        }
        let book = SyntheticOutstanding::<T>::get(netuid)
            .checked_sub(minted)
            .ok_or(Error::<T>::Overflow)?;
        let residual = ResidualAlpha::<T>::get(netuid)
            .checked_add(minted)
            .ok_or(Error::<T>::Overflow)?;
        SyntheticOutstanding::<T>::insert(netuid, book);
        ResidualAlpha::<T>::insert(netuid, residual);
        Self::forget_position(id, pos.charge_at);
        Self::deposit_event(Event::ShortDefaulted {
            who: who.clone(),
            netuid: *netuid,
            residual_alpha: minted,
            proceeds_to_pool,
        });
        Ok(())
    }
}
