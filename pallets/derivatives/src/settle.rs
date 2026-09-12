//! Adding to and settling positions. Every pool touch goes through `T::Pool`.
//!
//! A position is a sum of tranches, so there are only two primitives here: [`Pallet::lift_tranche`]
//! borrows one more slice of the pool and returns it as a [`Tranche`] to be folded in, and
//! [`Pallet::do_settle`] unwinds a fraction of the position at the current price. `add` is
//! built from those two: it settles when the deposit is on the other side, and grows an
//! existing or empty position otherwise. `close` is `do_settle` with a fraction of one.
//!
//! The chain's own work is [`Pallet::collect_due`]: the positions due this block, each one's
//! interest moved from its cushion into its pool, and any that cannot pay handed back whole
//! by [`Pallet::forfeit`].

use frame_support::storage::with_storage_layer;
use sp_runtime::{Rounding, helpers_128bit::multiply_by_rational_with_rounding};

use crate::{weights::WeightInfo, *};

/// Take up to `want` from `pot`. Returns what was taken; less than `want` means the pot is
/// out.
fn take(pot: &mut TaoBalance, want: TaoBalance) -> TaoBalance {
    let taken = want.min(*pot);
    *pot = pot.saturating_sub(taken);
    taken
}

/// `alpha` in TAO at the price `tao / alpha` given by `price`. Alpha is worth nothing when the
/// swap moved no TAO; it is worth everything when it moved TAO for no alpha.
fn tao_value(
    alpha: AlphaBalance,
    (price_tao, price_alpha): (TaoBalance, AlphaBalance),
    rounding: Rounding,
) -> TaoBalance {
    let value = multiply_by_rational_with_rounding(
        u128::from(alpha.to_u64()),
        u128::from(price_tao.to_u64()),
        u128::from(price_alpha.to_u64()),
        rounding,
    )
    .unwrap_or(if alpha.is_zero() { 0 } else { u128::MAX });
    TaoBalance::from(u64::try_from(value).unwrap_or(u64::MAX))
}

/// The inverse of [`tao_value`]: `tao` in alpha at the same price.
fn alpha_value(
    tao: TaoBalance,
    (price_tao, price_alpha): (TaoBalance, AlphaBalance),
    rounding: Rounding,
) -> AlphaBalance {
    let value = multiply_by_rational_with_rounding(
        u128::from(tao.to_u64()),
        u128::from(price_alpha.to_u64()),
        u128::from(price_tao.to_u64()),
        rounding,
    )
    .unwrap_or(if tao.is_zero() { 0 } else { u128::MAX });
    AlphaBalance::from(u64::try_from(value).unwrap_or(u64::MAX))
}

impl<T: Config> Pallet<T> {
    /// Open, add to, reduce, or flip the caller's position on `netuid`. One storage layer around
    /// the whole call: whichever branch runs, all of it lands or none of it does.
    pub(crate) fn do_add(
        owner: T::AccountId,
        netuid: NetUid,
        side: Side,
        deposit: TaoBalance,
        leverage_percent: u16,
    ) -> DispatchResult {
        with_storage_layer(|| {
            ensure!(
                Self::leverage_allowed(side, leverage_percent),
                Error::<T>::LeverageOutOfRange
            );
            let now = frame_system::Pallet::<T>::block_number();
            let (position, deposit) = match Positions::<T>::get(&owner, netuid) {
                None => (Position::empty(side, now), deposit),
                Some(position) if position.side() == side => (position, deposit),
                Some(position) => {
                    match Self::reduce(&owner, netuid, position, deposit, leverage_percent)? {
                        None => return Ok(()),
                        Some(rest) => (Position::empty(side, now), rest),
                    }
                }
            };
            Self::grow(owner, netuid, position, deposit, leverage_percent)
        })
    }

    /// Lift a tranche for `deposit` and fold it into `position`, which may be empty.
    fn grow(
        owner: T::AccountId,
        netuid: NetUid,
        mut position: Position<BlockNumberFor<T>>,
        deposit: TaoBalance,
        leverage_percent: u16,
    ) -> DispatchResult {
        let side = position.side();
        let tranche = Self::lift_tranche(&owner, netuid, side, deposit, leverage_percent)?;
        let added = Event::PositionAdded {
            owner: owner.clone(),
            netuid,
            side,
            deposit,
            leverage_percent,
            legs: tranche.legs,
            exposure_added: tranche.exposure_tao,
            exposure_tao: position.exposure_tao.saturating_add(tranche.exposure_tao),
        };
        position
            .fold(tranche, frame_system::Pallet::<T>::block_number())
            .ok_or(DispatchError::Corruption)?;
        // A new position joins the interest queue at its first due block; an existing one is
        // already listed and keeps its slot.
        Due::<T>::insert(position.due, (&owner, netuid), ());
        Positions::<T>::insert(&owner, netuid, position);
        OpenByNetuid::<T>::insert(netuid, &owner, ());
        Self::deposit_event(added);
        Ok(())
    }

    /// Take a position out of every index: the book, the subnet index, and the interest queue.
    fn remove(owner: &T::AccountId, netuid: NetUid, position: &Position<BlockNumberFor<T>>) {
        Positions::<T>::remove(owner, netuid);
        OpenByNetuid::<T>::remove(netuid, owner);
        Due::<T>::remove(position.due, (owner, netuid));
        let side = position.side();
        Footprint::<T>::mutate(netuid, side, |f| {
            *f = f.saturating_sub(position.legs.footprint())
        });
    }

    /// Other side: `leverage` times the deposit comes off the position. Less than it holds
    /// settles that share and returns `None`; more closes it and returns the part of the deposit
    /// past the flip point, to open the other side with, or `None` if that part is below
    /// `MinDeposit`.
    fn reduce(
        owner: &T::AccountId,
        netuid: NetUid,
        position: Position<BlockNumberFor<T>>,
        deposit: TaoBalance,
        leverage_percent: u16,
    ) -> Result<Option<TaoBalance>, DispatchError> {
        let asked = u128::from(deposit.to_u64())
            .saturating_mul(u128::from(leverage_percent))
            .checked_div(100)
            .unwrap_or(0);
        ensure!(asked > 0, Error::<T>::ZeroExposure);
        let held = u128::from(position.exposure_tao.to_u64());

        if asked < held {
            // Both fit in u64: `asked < held <= u64::MAX`.
            let fraction = Perquintill::from_rational(asked as u64, held as u64);
            Self::do_settle(owner, netuid, fraction)?;
            return Ok(None);
        }

        Self::do_settle(owner, netuid, Perquintill::one())?;
        let rest = asked.saturating_sub(held);
        let rest_deposit = rest
            .saturating_mul(100)
            .checked_div(u128::from(leverage_percent))
            .unwrap_or(0)
            .min(u128::from(u64::MAX)) as u64;
        let rest_deposit = TaoBalance::from(rest_deposit);
        if rest_deposit < T::MinDeposit::get() {
            // Dust past the flip point is not worth a position.
            return Ok(None);
        }
        Ok(Some(rest_deposit))
    }

    /// Take `deposit` from `owner`, lift `phi` of the pool and swap the borrowed half. The
    /// footprint is booked here; the caller folds the rest into a position.
    fn lift_tranche(
        owner: &T::AccountId,
        netuid: NetUid,
        side: Side,
        deposit: TaoBalance,
        leverage_percent: u16,
    ) -> Result<Tranche, DispatchError> {
        ensure!(T::Pool::is_dynamic(netuid), Error::<T>::SubnetNotDynamic);
        ensure!(deposit >= T::MinDeposit::get(), Error::<T>::DepositTooLow);

        let pallet_account = Self::pallet_account();
        let pallet_hotkey = Self::pallet_hotkey()?;
        let params = Params::<T>::get();

        let (tao_reserve, alpha_reserve) = T::Pool::reserves(netuid);
        let (t, a) = (tao_reserve.to_u64(), alpha_reserve.to_u64());
        ensure!(t > 0 && a > 0, Error::<T>::SubnetNotDynamic);
        let (smoothed_tao, smoothed_alpha) = T::Pool::smoothed_reserves(netuid);

        // The slice a spot swap in the same block cannot improve on: no more TAO than the
        // deposit times the leverage, and no more alpha than that TAO is worth at the smoothed
        // price. Whichever bound the live reserves hit first sets `phi`; on an untouched pool
        // both give the same share. The cap is taken at the smaller of the live and smoothed
        // lent reserve, and the footprint is projected on the live one, which is what the lift
        // takes.
        let phi = pool_fraction(
            leverage_percent,
            deposit.to_u64(),
            (t, a),
            (smoothed_tao.to_u64(), smoothed_alpha.to_u64()),
        );
        ensure!(!phi.is_zero(), Error::<T>::ZeroExposure);
        let (live_lent, smoothed_lent) = match side {
            Side::Short => (t, smoothed_tao.to_u64()),
            Side::Long => (a, smoothed_alpha.to_u64()),
        };
        let cap = params.pool_share.mul_floor(live_lent.min(smoothed_lent));
        let projected = projected_footprint(phi, live_lent);
        ensure!(
            !phi.is_one() && Footprint::<T>::get(netuid, side).saturating_add(projected) <= cap,
            Error::<T>::PoolCapExceeded
        );

        T::Pool::transfer_tao(owner, &pallet_account, deposit)?;

        let (lifted_tao, lifted_alpha) =
            T::Pool::lift_liquidity(netuid, phi, &pallet_account, &pallet_hotkey)?;
        ensure!(!lifted_tao.is_zero(), Error::<T>::ZeroExposure);
        let legs = match side {
            Side::Short => {
                let proceeds = T::Pool::sell_alpha_internal(
                    &pallet_account,
                    &pallet_hotkey,
                    netuid,
                    lifted_alpha,
                )?;
                ensure!(!proceeds.is_zero(), Error::<T>::ZeroExposure);
                Legs::Short {
                    proceeds,
                    debt: lifted_alpha,
                    escrow: lifted_tao,
                }
            }
            Side::Long => {
                let proceeds = T::Pool::buy_alpha_internal(
                    &pallet_account,
                    &pallet_hotkey,
                    netuid,
                    lifted_tao,
                )?;
                ensure!(!proceeds.is_zero(), Error::<T>::ZeroExposure);
                Legs::Long {
                    proceeds,
                    debt: lifted_tao,
                    escrow: lifted_alpha,
                }
            }
        };
        Footprint::<T>::mutate(netuid, side, |f| *f = f.saturating_add(legs.footprint()));

        Ok(Tranche {
            deposit,
            legs,
            exposure_tao: lifted_tao,
            interest_per_year: params.interest_for(lifted_tao),
        })
    }

    /// Unwind `fraction` of the position at the current price: reverse that share of the open
    /// swap, repay the pool plus the whole interest owed so far, pay the owner what is left of
    /// that share of the cushion. A fraction of one closes the position. Atomic. Everything paid
    /// is reported in the event.
    pub(crate) fn do_settle(
        owner: &T::AccountId,
        netuid: NetUid,
        fraction: Perquintill,
    ) -> DispatchResult {
        with_storage_layer(|| {
            let mut position = Positions::<T>::take(owner, netuid).ok_or(Error::<T>::NoPosition)?;
            let side = position.side();
            let full = fraction.is_one();

            let pallet_account = Self::pallet_account();
            let pallet_hotkey = Self::pallet_hotkey()?;
            let now = frame_system::Pallet::<T>::block_number();
            let interest_due = position.interest_due(now);

            let part = if full {
                position.legs
            } else {
                position.legs.part(fraction)
            };
            // This share's pot: its cushion plus whatever its closing trade leaves. The rest of
            // the cushion stays behind, except to cover interest this share cannot.
            let mut pot = if full {
                position.cushion
            } else {
                TaoBalance::from(fraction.mul_floor(position.cushion.to_u64()))
            };
            let mut rest = position.cushion.saturating_sub(pot);

            let (mut tao_to_pool, alpha_to_pool, shortfall) = match part {
                Legs::Short {
                    proceeds,
                    debt,
                    escrow,
                } => {
                    pot = pot.saturating_add(proceeds);
                    let (spent, bought) =
                        T::Pool::buy_alpha_for(&pallet_account, &pallet_hotkey, netuid, debt, pot)?;
                    pot = pot.saturating_sub(spent);
                    // Bought surplus is dust that goes back with the debt.
                    (escrow, bought, Lent::Alpha(debt.saturating_sub(bought)))
                }
                Legs::Long {
                    proceeds,
                    debt,
                    escrow,
                } => {
                    pot = pot.saturating_add(T::Pool::sell_alpha_internal(
                        &pallet_account,
                        &pallet_hotkey,
                        netuid,
                        proceeds,
                    )?);
                    let repaid = take(&mut pot, debt);
                    (repaid, escrow, Lent::Tao(debt.saturating_sub(repaid)))
                }
            };

            // The interest is owed by the whole position. This share pays it first; what it cannot
            // cover comes off the cushion that stays behind.
            let mut interest_paid = take(&mut pot, interest_due);
            interest_paid = interest_paid
                .saturating_add(take(&mut rest, interest_due.saturating_sub(interest_paid)));

            // A share that could not repay its debt is underwater: the owner gets nothing for it
            // and everything the pallet still holds for it goes to the pool. This does not depend
            // on the swap quotes being accurate; it is the rule that bounds the pool's loss.
            if !shortfall.is_zero() {
                tao_to_pool = tao_to_pool.saturating_add(pot);
                pot = TaoBalance::ZERO;
            }

            // The interest buys alpha that is recycled; what the pool will not swap goes back
            // to it as TAO.
            tao_to_pool = tao_to_pool.saturating_add(Self::burn_interest(netuid, interest_paid));

            // Pay the owner before the pool so the last TAO leaving the pallet account is the
            // pool's share; an owner that cannot be paid forfeits to the pool rather than failing
            // the settlement.
            let payout = Self::pay_tao(&pallet_account, owner, pot);
            tao_to_pool = tao_to_pool.saturating_add(pot.saturating_sub(payout));

            T::Pool::return_liquidity(
                netuid,
                tao_to_pool,
                alpha_to_pool,
                &pallet_account,
                &pallet_hotkey,
            )?;

            if full {
                Self::remove(owner, netuid, &position);
                Self::deposit_event(Event::PositionClosed {
                    owner: owner.clone(),
                    netuid,
                    side,
                    closed_by: Closer::Owner,
                    payout,
                    interest_paid,
                    shortfall,
                });
                return Ok(());
            }

            Footprint::<T>::mutate(netuid, side, |f| *f = f.saturating_sub(part.footprint()));
            position.cushion = rest;
            position.legs = position.legs.minus(&part);
            position.exposure_tao = position.exposure_tao.saturating_sub(TaoBalance::from(
                fraction.mul_floor(position.exposure_tao.to_u64()),
            ));
            position.interest_per_year = position.interest_per_year.saturating_sub(
                TaoBalance::from(fraction.mul_floor(position.interest_per_year.to_u64())),
            );
            position.interest_owed = interest_due.saturating_sub(interest_paid);
            position.since = now;
            let exposure_tao = position.exposure_tao;
            Positions::<T>::insert(owner, netuid, position);

            Self::deposit_event(Event::PositionReduced {
                owner: owner.clone(),
                netuid,
                side,
                fraction,
                payout,
                interest_paid,
                shortfall,
                exposure_tao,
            });
            Ok(())
        })
    }

    /// Collect the interest of every position due by `now`. Walks the [`Due`] queue slot by
    /// slot from [`NextDue`] within a budget of [`COLLECTIONS_PER_BLOCK`] collections: an empty
    /// slot costs one read and is passed, so the pointer catches up quickly after any stall; a
    /// slot with more positions than the budget allows is finished over the following blocks.
    /// Returns the weight used.
    pub(crate) fn collect_due(now: BlockNumberFor<T>) -> Weight {
        let per_collection = T::WeightInfo::collect_interest();
        let per_slot = T::DbWeight::get().reads(1);
        let mut meter =
            WeightMeter::with_limit(per_collection.saturating_mul(COLLECTIONS_PER_BLOCK.into()));
        let mut slot = NextDue::<T>::get();
        while slot <= now && meter.try_consume(per_slot).is_ok() {
            let mut keys = Due::<T>::iter_key_prefix(slot);
            loop {
                if !meter.can_consume(per_collection) {
                    // Out of room with this slot unfinished: resume here next block.
                    NextDue::<T>::put(slot);
                    return meter
                        .consumed()
                        .saturating_add(T::DbWeight::get().writes(1));
                }
                let Some(key) = keys.next() else {
                    break;
                };
                meter.consume(per_collection);
                Due::<T>::remove(slot, &key);
                let (owner, netuid) = key;
                // A collection that cannot go through is tried again next block; the queue
                // never drops a position.
                if with_storage_layer(|| Self::collect_interest(&owner, netuid, now)).is_err() {
                    Self::defer(&owner, netuid, slot.saturating_add(1u32.into()));
                }
            }
            slot = slot.saturating_add(1u32.into());
        }
        NextDue::<T>::put(slot);
        meter
            .consumed()
            .saturating_add(T::DbWeight::get().writes(1))
    }

    /// Collect one position's interest at `now`: out of its cushion, into its pool, and on to
    /// its next slot in the queue. A cushion that cannot cover it forfeits the position. The
    /// caller has already taken the position out of its current slot.
    pub(crate) fn collect_interest(
        owner: &T::AccountId,
        netuid: NetUid,
        now: BlockNumberFor<T>,
    ) -> DispatchResult {
        let Some(mut position) = Positions::<T>::get(owner, netuid) else {
            // A stale queue entry; nothing to collect.
            return Ok(());
        };
        let Some(paid) = position.collect(now) else {
            return Self::forfeit(owner, netuid, position);
        };
        Due::<T>::insert(position.due, (owner, netuid), ());
        Positions::<T>::insert(owner, netuid, position);
        let unburned = Self::burn_interest(netuid, paid);
        T::Pool::return_liquidity(
            netuid,
            unburned,
            AlphaBalance::ZERO,
            &Self::pallet_account(),
            &Self::pallet_hotkey()?,
        )
    }

    /// Pay `interest` to the pool as buy pressure: spend it on alpha and recycle the alpha, so
    /// the pool keeps the TAO and the alpha leaves circulation. Returns the TAO the pool would
    /// not swap (dust, or a paused pool), for the caller to hand back as plain TAO instead.
    fn burn_interest(netuid: NetUid, interest: TaoBalance) -> TaoBalance {
        if interest.is_zero() {
            return TaoBalance::ZERO;
        }
        let burned = with_storage_layer(|| -> DispatchResult {
            let pallet_account = Self::pallet_account();
            let pallet_hotkey = Self::pallet_hotkey()?;
            let alpha =
                T::Pool::buy_alpha_internal(&pallet_account, &pallet_hotkey, netuid, interest)?;
            T::Pool::recycle_alpha(&pallet_account, &pallet_hotkey, netuid, alpha)
        });
        match burned {
            Ok(()) => TaoBalance::ZERO,
            Err(_) => interest,
        }
    }

    /// Put a position back in the queue at `slot`, to be collected then instead.
    fn defer(owner: &T::AccountId, netuid: NetUid, slot: BlockNumberFor<T>) {
        Positions::<T>::mutate_exists(owner, netuid, |position| {
            if let Some(position) = position {
                position.due = slot;
            }
        });
        Due::<T>::insert(slot, (owner, netuid), ());
    }

    /// Hand a starved position back to the pool whole: every TAO and every alpha the pallet
    /// holds for it, with no swap. The pool gets its slice back in kind plus the cushion; the
    /// owner gets nothing. Because nothing is traded, a forfeit moves no price.
    fn forfeit(
        owner: &T::AccountId,
        netuid: NetUid,
        position: Position<BlockNumberFor<T>>,
    ) -> DispatchResult {
        let side = position.side();
        let (tao, alpha) = match position.legs {
            Legs::Short {
                proceeds, escrow, ..
            } => (
                position
                    .cushion
                    .saturating_add(proceeds)
                    .saturating_add(escrow),
                AlphaBalance::ZERO,
            ),
            Legs::Long {
                proceeds, escrow, ..
            } => (position.cushion, proceeds.saturating_add(escrow)),
        };
        T::Pool::return_liquidity(
            netuid,
            tao,
            alpha,
            &Self::pallet_account(),
            &Self::pallet_hotkey()?,
        )?;
        Self::remove(owner, netuid, &position);
        Self::deposit_event(Event::PositionClosed {
            owner: owner.clone(),
            netuid,
            side,
            closed_by: Closer::Starved,
            payout: TaoBalance::ZERO,
            interest_paid: position.cushion,
            shortfall: match side {
                Side::Short => Lent::Alpha(AlphaBalance::ZERO),
                Side::Long => Lent::Tao(TaoBalance::ZERO),
            },
        });
        Ok(())
    }

    /// Dissolution path: cash settlement at `price`, the pool's spot at dissolution, with no
    /// swap. The pool takes back what it lent, alpha valued at that price, plus the interest
    /// owed; the owner is paid the rest in TAO, or nothing if the position is underwater at
    /// that price, as at any other settlement. Everything returned lands in the reserves the
    /// stakers are paid from in the later phases.
    ///
    /// **Rounding favours the pool.** A short's alpha debt is converted to TAO rounding up; a
    /// long's alpha is credited in TAO rounding down. The pool is the party being wound up and
    /// cannot come back for a missing rao; the owner's loss is at most one.
    ///
    /// **Never fails, never blocks.** The position is removed from every index first, so a
    /// failure further down cannot leave it to be visited again and paid twice, and the
    /// caller's loop always terminates. The pool paying out a long's credit and the final
    /// return of liquidity can only fail in a pool that is already broken; a failure there is
    /// logged and the settlement carries on, because one position's mishap must not stop the
    /// subnet from being cleaned up or the stakers from being paid. Whatever cannot reach the
    /// owner stays with the pool.
    pub(crate) fn settle_at_dissolution(
        owner: &T::AccountId,
        netuid: NetUid,
        price: (TaoBalance, AlphaBalance),
    ) {
        // A position can only exist once the hotkey is claimed.
        let Ok(pallet_hotkey) = Self::pallet_hotkey() else {
            OpenByNetuid::<T>::remove(netuid, owner);
            return;
        };
        let Some(position) = Positions::<T>::get(owner, netuid) else {
            OpenByNetuid::<T>::remove(netuid, owner);
            return;
        };
        let side = position.side();
        // Out of the book before any transfer: the caller iterates `OpenByNetuid` until it is
        // empty, so a position must leave it whether or not what follows succeeds.
        Self::remove(owner, netuid, &position);

        let pallet_account = Self::pallet_account();
        let now = frame_system::Pallet::<T>::block_number();
        let interest_due = position.interest_due(now);
        let mut pot = position.cushion;

        // Alpha the pallet holds is handed to the pool and credited at the price; alpha it owes
        // is charged at it. Both round in the pool's favour. A long's alpha is not sold: it
        // goes back in kind and the pool pays its value in TAO from its own reserve, so no
        // position's settlement moves the price another one is settled at.
        let (credit, owed, mut tao_to_pool, alpha_to_pool) = match position.legs {
            Legs::Short {
                proceeds,
                debt,
                escrow,
            } => {
                pot = pot.saturating_add(proceeds);
                (
                    TaoBalance::ZERO,
                    tao_value(debt, price, Rounding::Up),
                    escrow,
                    AlphaBalance::ZERO,
                )
            }
            Legs::Long {
                proceeds,
                debt,
                escrow,
            } => (
                tao_value(proceeds, price, Rounding::Down),
                debt,
                TaoBalance::ZERO,
                proceeds.saturating_add(escrow),
            ),
        };

        if !credit.is_zero() {
            // A reserve that cannot pay the credit is a pool already short of what its own
            // price says it holds; the owner loses the credit, the cleanup goes on.
            match T::Pool::draw_tao(netuid, &pallet_account, credit) {
                Ok(()) => pot = pot.saturating_add(credit),
                Err(error) => log::error!(
                    "derivatives: pool could not pay {credit:?} for {owner:?} on {netuid:?}: {error:?}"
                ),
            }
        }

        // Debt first, then interest, then the underwater rule, as at any settlement. The
        // interest is plain TAO here rather than buy pressure: there is no pool left to buy
        // from, and the reserves it joins are what the stakers are paid.
        let repaid = take(&mut pot, owed);
        let interest_paid = take(&mut pot, interest_due);
        tao_to_pool = tao_to_pool
            .saturating_add(repaid)
            .saturating_add(interest_paid);
        let shortfall_tao = owed.saturating_sub(repaid);
        if !shortfall_tao.is_zero() {
            tao_to_pool = tao_to_pool.saturating_add(pot);
            pot = TaoBalance::ZERO;
        }

        let payout = Self::pay_tao(&pallet_account, owner, pot);
        tao_to_pool = tao_to_pool.saturating_add(pot.saturating_sub(payout));

        if let Err(error) = T::Pool::return_liquidity(
            netuid,
            tao_to_pool,
            alpha_to_pool,
            &pallet_account,
            &pallet_hotkey,
        ) {
            log::error!(
                "derivatives: could not return liquidity for {owner:?} on {netuid:?}: {error:?}"
            );
        }

        Self::deposit_event(Event::PositionClosed {
            owner: owner.clone(),
            netuid,
            side,
            closed_by: Closer::Dissolution,
            payout,
            interest_paid,
            shortfall: match side {
                Side::Short => Lent::Alpha(alpha_value(shortfall_tao, price, Rounding::Up)),
                Side::Long => Lent::Tao(shortfall_tao),
            },
        });
    }

    /// Returns how much reached `to` (all of it, or nothing).
    fn pay_tao(from: &T::AccountId, to: &T::AccountId, amount: TaoBalance) -> TaoBalance {
        if amount.is_zero() {
            return TaoBalance::ZERO;
        }
        match with_storage_layer(|| T::Pool::transfer_tao(from, to, amount)) {
            Ok(()) => amount,
            Err(_) => TaoBalance::ZERO,
        }
    }
}
