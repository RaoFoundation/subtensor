//! Interest charging (DESIGN-v2 section 5; SPEC section 1 item 2, P3, P4, P15, I5, I19).
//!
//! Every open position has one entry in `Due`, at its `charge_at`. `on_initialize` drains the
//! buckets up to the current block in order, at most `MaxChargesPerBlock` charges per block;
//! `NextDue` is the first bucket not yet drained, so a backlog carries over. A charge reads the
//! position's rate from the curve `min(cap, apr * u^k)`, accrues the position to the current
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

use alloc::vec::Vec;
use frame_support::{dispatch::DispatchResult, storage::with_storage_layer, traits::Get};
use frame_system::pallet_prelude::BlockNumberFor;
use sp_runtime::{
    DispatchError, SaturatedConversion,
    traits::{One, Saturating},
};
use subtensor_runtime_common::NetUid;

use crate::{
    Config, Due, Event, InterestRetry, InterestTotals, LendingParamsStore, LendingPoolInterface,
    NextDue, Pallet, Positions, TotalPositions,
    math::{interest::next_charge_block, rate::RateCurve},
    params::LendingParams,
    types::{Position, PositionId, Side},
};

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

    /// Runs the scheduled charges due at or before `now`, oldest bucket first, and returns how
    /// many it ran and how many buckets it read. It stops at `MaxChargesPerBlock` charges or
    /// [`MAX_DUE_BUCKETS_PER_BLOCK`] buckets; `NextDue` keeps the place for the next block
    /// (SPEC P15).
    pub(crate) fn collect_due(now: BlockNumberFor<T>) -> (u32, u32) {
        let Some(mut cursor) = NextDue::<T>::get() else {
            return (0, 0);
        };
        if cursor > now {
            return (0, 0);
        }
        let lending = LendingParamsStore::<T>::get();
        let budget = T::MaxChargesPerBlock::get();
        let mut charges = 0u32;
        let mut buckets = 0u32;
        for _ in 0..MAX_DUE_BUCKETS_PER_BLOCK {
            if cursor > now {
                break;
            }
            buckets = buckets.saturating_add(1);
            let room = usize::try_from(budget.saturating_sub(charges)).unwrap_or(usize::MAX);
            let ids: Vec<_> = Due::<T>::iter_key_prefix(cursor).take(room).collect();
            for id in &ids {
                Due::<T>::remove(cursor, id);
                Self::charge_due(id, now, &lending);
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
        (charges, buckets)
    }

    /// Charges one position whose `Due` entry was just taken, and schedules its next charge. A
    /// charge that succeeds schedules the first block after `now` on the position's grid, read
    /// with the interval live now, so a late charge rejoins the grid and an interval change
    /// applies from here (SPEC P15). A charge that fails leaves the position as it was, counts
    /// in `InterestRetry` and is tried again [`RETRY_DELAY_BLOCKS`] after `now`.
    fn charge_due(id: &PositionId<T::AccountId>, now: BlockNumberFor<T>, lending: &LendingParams) {
        let Some(pos) = Positions::<T>::get(id) else {
            return;
        };
        let charged =
            with_storage_layer(|| Self::charge_position(id, pos, &lending.rate_curve(), now));
        let paid = charged.is_ok();
        let (pos, charge_at) = match charged {
            Ok(settled) => {
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
        if paid {
            Self::after_charge(id);
        }
    }

    /// Accrues `pos` to `now` at its rate on `curve`, on its notional (`q * TWAP_median` for a
    /// short, the debt for a long), takes `min(due, C)` from its collateral and pays it to the
    /// pool (SPEC section 1 item 2, I5). Returns the settled position; the caller stores it.
    fn charge_position(
        id: &PositionId<T::AccountId>,
        pos: Position<BlockNumberFor<T>>,
        curve: &RateCurve,
        now: BlockNumberFor<T>,
    ) -> Result<Position<BlockNumberFor<T>>, DispatchError> {
        let (who, netuid, side) = id;
        let twap = Self::price_refs(*netuid)?.twap;
        let (settled, charge) = pos.settle(twap, curve, now);
        let custody = Self::position_custody(id, pos.opened_at);
        Self::pay_interest(
            who,
            *netuid,
            *side,
            &custody,
            charge.amount,
            charge.rate_bps,
        )?;
        Ok(settled)
    }

    /// Runs after every scheduled charge. Empty until the default of SPEC section 1 item 4.
    pub(crate) fn after_charge(_id: &PositionId<T::AccountId>) {}
}
