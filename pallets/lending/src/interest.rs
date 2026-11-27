//! Interest charging (DESIGN-v2 section 5; SPEC section 1 item 2, P3, P4, P15, I5, I19).
//!
//! Every open position has one entry in `Due`, at its `charge_at`. `on_initialize` drains the
//! buckets up to the current block in order, at most `MaxChargesPerBlock` charges per block;
//! `NextDue` is the first bucket not yet drained, so a backlog carries over. A charge accrues
//! the position to the current block, takes `min(due, C)` from its collateral and credits the
//! subnet pool's TAO reserve, then schedules the next charge `charge_interval_blocks` later.
//!
//! The operations of `positions` charge on touch with the same two steps as the queue:
//! [`Position::settle`] for the amounts and [`Pallet::pay_interest`] for the credit. A touch
//! does not move the position's `Due` entry (SPEC P15: queued entries are not moved).

use alloc::vec::Vec;
use frame_support::{dispatch::DispatchResult, storage::with_storage_layer, traits::Get};
use frame_system::pallet_prelude::BlockNumberFor;
use sp_runtime::{
    DispatchError,
    traits::{One, Saturating},
};
use subtensor_runtime_common::NetUid;

use crate::{
    Config, Due, Event, InterestRetry, InterestTotals, LendingParamsStore, LendingPoolInterface,
    NextDue, Pallet, Positions, TotalPositions,
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
    pub(crate) fn pay_interest(
        who: &T::AccountId,
        netuid: NetUid,
        side: Side,
        custody: &T::AccountId,
        amount: u64,
    ) -> DispatchResult {
        if amount == 0 {
            return Ok(());
        }
        let before = T::Pool::pool_tao(netuid);
        T::Pool::credit_pool_tao(netuid, custody, amount)?;
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
        });
        Ok(())
    }

    /// The block of a charge scheduled `delay` blocks after `now`.
    pub(crate) fn charge_block(now: BlockNumberFor<T>, delay: u32) -> BlockNumberFor<T> {
        now.saturating_add(delay.into())
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
    /// many it ran. It stops at `MaxChargesPerBlock` charges or [`MAX_DUE_BUCKETS_PER_BLOCK`]
    /// buckets; `NextDue` keeps the place for the next block (SPEC P15).
    pub(crate) fn collect_due(now: BlockNumberFor<T>) -> u32 {
        let Some(mut cursor) = NextDue::<T>::get() else {
            return 0;
        };
        if cursor > now {
            return 0;
        }
        let lending = LendingParamsStore::<T>::get();
        let budget = T::MaxChargesPerBlock::get();
        let mut charges = 0u32;
        for _ in 0..MAX_DUE_BUCKETS_PER_BLOCK {
            if cursor > now {
                break;
            }
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
        charges
    }

    /// Charges one position whose `Due` entry was just taken, and schedules its next charge:
    /// `charge_interval_blocks` after this one, read now, so a change of the interval applies
    /// from here (SPEC P15). A charge that fails leaves the position as it was, counts in
    /// `InterestRetry` and is tried again [`RETRY_DELAY_BLOCKS`] later.
    fn charge_due(id: &PositionId<T::AccountId>, now: BlockNumberFor<T>, lending: &LendingParams) {
        let Some(pos) = Positions::<T>::get(id) else {
            return;
        };
        let charged = with_storage_layer(|| Self::charge_position(id, pos, lending.apr_bps, now));
        let paid = charged.is_ok();
        let (pos, delay) = match charged {
            Ok(settled) => {
                InterestRetry::<T>::remove(id);
                (settled, lending.charge_interval_blocks)
            }
            Err(_) => {
                InterestRetry::<T>::mutate(id, |failed| *failed = failed.saturating_add(1));
                (pos, RETRY_DELAY_BLOCKS)
            }
        };
        let charge_at = Self::charge_block(now, delay);
        Positions::<T>::insert(id, Position { charge_at, ..pos });
        Self::schedule_charge(id, charge_at);
        if paid {
            Self::after_charge(id);
        }
    }

    /// Accrues `pos` to `now` at `apr_bps` on its notional (`q * TWAP_median` for a short, the
    /// debt for a long), takes `min(due, C)` from its collateral and pays it to the pool (SPEC
    /// section 1 item 2, I5). Returns the settled position; the caller stores it.
    fn charge_position(
        id: &PositionId<T::AccountId>,
        pos: Position<BlockNumberFor<T>>,
        apr_bps: u32,
        now: BlockNumberFor<T>,
    ) -> Result<Position<BlockNumberFor<T>>, DispatchError> {
        let (who, netuid, side) = id;
        let twap = Self::price_refs(*netuid)?.twap;
        let (settled, charge) = pos.settle(twap, apr_bps, now);
        let custody = Self::custody_of(*side, who, *netuid, pos.opened_at);
        Self::pay_interest(who, *netuid, *side, &custody, charge)?;
        Ok(settled)
    }

    /// Runs after every scheduled charge. Empty until the health check of SPEC section 1 item 4.
    pub(crate) fn after_charge(_id: &PositionId<T::AccountId>) {}
}
