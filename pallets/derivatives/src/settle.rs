//! Adding to and settling positions. Every pool touch goes through `T::Pool`.
//!
//! A position is a sum of tranches, so there are only two primitives here: [`Pallet::lift_tranche`]
//! borrows one more slice of the pool and returns it as a [`Tranche`] to be added in, and
//! [`Pallet::do_settle`] unwinds a fraction of the position at the current price. `add` is
//! built from those two; `close` is `do_settle` with a fraction of one.

use frame_support::storage::with_storage_layer;
use sp_runtime::{Rounding, helpers_128bit::multiply_by_rational_with_rounding};

use crate::*;

/// How many blocks past the nominal expiry a position may be pushed when the expiry queue for
/// a block is full.
pub(crate) const MAX_EXPIRY_SHIFT: u32 = 64;

/// Blocks to wait before sweeping a position again after its settlement failed (~1 hour).
const RETRY_DELAY: u32 = 300;

/// Failed sweeps after which a position is left to permissionless `close`.
const MAX_SETTLE_RETRIES: u8 = 3;

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
        amount: TaoBalance,
        leverage_percent: u16,
    ) -> DispatchResult {
        with_storage_layer(|| {
            let params = Params::<T>::get();
            ensure!(
                params.leverage_allowed(side, leverage_percent),
                Error::<T>::LeverageOutOfRange
            );
            match Positions::<T>::get(&owner, netuid) {
                None => Self::open_position(owner, netuid, side, amount, leverage_percent, &params),
                Some(position) if position.side() == side => {
                    Self::add_tranche(owner, netuid, position, amount, leverage_percent, &params)
                }
                Some(position) => Self::reduce_or_flip(
                    owner,
                    netuid,
                    side,
                    position,
                    amount,
                    leverage_percent,
                    &params,
                ),
            }
        })
    }

    /// First tranche: the position gets its clocks here. Adds never move `expires_at`.
    fn open_position(
        owner: T::AccountId,
        netuid: NetUid,
        side: Side,
        amount: TaoBalance,
        leverage_percent: u16,
        params: &DerivativesParams<BlockNumberFor<T>>,
    ) -> DispatchResult {
        let tranche = Self::lift_tranche(&owner, netuid, side, amount, leverage_percent, params)?;
        let now = frame_system::Pallet::<T>::block_number();
        let expires_at =
            Self::schedule_expiry(&owner, netuid, now.saturating_add(params.lifetime_blocks))?;

        Positions::<T>::insert(
            &owner,
            netuid,
            Position {
                cushion: Cushion::Tao(tranche.cushion),
                legs: tranche.legs,
                exposure_tao: tranche.exposure_tao,
                fee_per_day: tranche.fee_per_day,
                // The first day is booked up front.
                fee_accrued: tranche.fee_per_day,
                last_touch: now,
                opened_at: now,
                expires_at,
                queued_at: expires_at,
                failed_sweeps: 0,
            },
        );
        OpenByNetuid::<T>::insert(netuid, &owner, ());

        Self::deposit_event(Event::PositionAdded {
            owner,
            netuid,
            side,
            cushion_added: tranche.cushion,
            leverage_percent,
            legs_added: tranche.legs,
            exposure_added: tranche.exposure_tao,
            fee_per_day_added: tranche.fee_per_day,
            exposure_tao: tranche.exposure_tao,
            expires_at,
        });
        Ok(())
    }

    /// Same side: lift another slice and fold it in. Sums for everything but the clocks; the
    /// fee is brought up to date first so the new rate only runs from now.
    fn add_tranche(
        owner: T::AccountId,
        netuid: NetUid,
        mut position: Position<BlockNumberFor<T>>,
        amount: TaoBalance,
        leverage_percent: u16,
        params: &DerivativesParams<BlockNumberFor<T>>,
    ) -> DispatchResult {
        let now = frame_system::Pallet::<T>::block_number();
        ensure!(now < position.expires_at, Error::<T>::Expired);
        let side = position.side();

        let tranche = Self::lift_tranche(&owner, netuid, side, amount, leverage_percent, params)?;
        let legs = position
            .legs
            .plus(&tranche.legs)
            .ok_or(DispatchError::Corruption)?;

        position.fee_accrued = position.fee_owed(now).saturating_add(tranche.fee_per_day);
        position.last_touch = now;
        position.cushion = Cushion::Tao(position.cushion.tao().saturating_add(tranche.cushion));
        position.legs = legs;
        position.exposure_tao = position.exposure_tao.saturating_add(tranche.exposure_tao);
        position.fee_per_day = position.fee_per_day.saturating_add(tranche.fee_per_day);
        let exposure_tao = position.exposure_tao;
        let expires_at = position.expires_at;
        Positions::<T>::insert(&owner, netuid, position);

        Self::deposit_event(Event::PositionAdded {
            owner,
            netuid,
            side,
            cushion_added: tranche.cushion,
            leverage_percent,
            legs_added: tranche.legs,
            exposure_added: tranche.exposure_tao,
            fee_per_day_added: tranche.fee_per_day,
            exposure_tao,
            expires_at,
        });
        Ok(())
    }

    /// Other side: `amount * leverage` of exposure comes off the position. Less than it holds
    /// settles that share; more closes it and opens the rest on `side`, taking only the cushion
    /// for that rest from the caller.
    fn reduce_or_flip(
        owner: T::AccountId,
        netuid: NetUid,
        side: Side,
        position: Position<BlockNumberFor<T>>,
        amount: TaoBalance,
        leverage_percent: u16,
        params: &DerivativesParams<BlockNumberFor<T>>,
    ) -> DispatchResult {
        let asked = (amount.to_u64() as u128)
            .saturating_mul(u128::from(leverage_percent))
            .checked_div(100)
            .unwrap_or(0);
        ensure!(asked > 0, Error::<T>::ZeroExposure);
        let held = u128::from(position.exposure_tao.to_u64());

        if asked < held {
            // Both fit in u64: `asked < held <= u64::MAX`.
            let fraction = Perquintill::from_rational(asked as u64, held as u64);
            Self::do_settle(&owner, netuid, fraction, Closer::Account(owner.clone()))?;
            return Ok(());
        }

        Self::do_settle(
            &owner,
            netuid,
            Perquintill::one(),
            Closer::Account(owner.clone()),
        )?;
        let rest_cushion = asked
            .saturating_sub(held)
            .saturating_mul(100)
            .checked_div(u128::from(leverage_percent))
            .unwrap_or(0)
            .min(u128::from(u64::MAX)) as u64;
        let rest_cushion = TaoBalance::from(rest_cushion);
        if rest_cushion < params.min_deposit_tao {
            // Dust past the flip point is not worth a position.
            return Ok(());
        }
        Self::open_position(owner, netuid, side, rest_cushion, leverage_percent, params)
    }

    /// Take `amount` TAO from `owner`, lift `phi` of the pool and swap the borrowed half. The
    /// footprint is booked here; the caller folds the rest into a position.
    fn lift_tranche(
        owner: &T::AccountId,
        netuid: NetUid,
        side: Side,
        amount: TaoBalance,
        leverage_percent: u16,
        params: &DerivativesParams<BlockNumberFor<T>>,
    ) -> Result<Tranche, DispatchError> {
        let override_ = SubnetOverrides::<T>::get(netuid);
        let enabled = override_
            .map(|o| o.side_enabled(side))
            .unwrap_or_else(|| params.side_enabled(side));
        ensure!(enabled, Error::<T>::SideDisabled);
        ensure!(T::Pool::is_dynamic(netuid), Error::<T>::SubnetNotDynamic);

        let pallet_account = Self::pallet_account();
        let pallet_hotkey = Self::pallet_hotkey()?;

        let (tao_reserve, alpha_reserve) = T::Pool::reserves(netuid);
        let (t, a) = (tao_reserve.to_u64(), alpha_reserve.to_u64());
        ensure!(t > 0 && a > 0, Error::<T>::SubnetNotDynamic);

        ensure!(!amount.is_zero(), Error::<T>::ZeroExposure);
        ensure!(amount >= params.min_deposit_tao, Error::<T>::DepositTooLow);

        let phi = pool_fraction(leverage_percent, amount.to_u64(), t)
            .ok_or(Error::<T>::ExposureTooLarge)?;

        let lent_reserve = match side {
            Side::Short => t,
            Side::Long => a,
        };
        let max_pool_share = override_
            .and_then(|o| o.max_pool_share)
            .unwrap_or(params.max_pool_share);
        let cap = max_pool_share.mul_floor(lent_reserve);
        let projected = projected_footprint(phi, lent_reserve);
        ensure!(
            Footprint::<T>::get(netuid, side).saturating_add(projected) <= cap,
            Error::<T>::PoolCapExceeded
        );

        T::Pool::transfer_tao(owner, &pallet_account, amount)?;

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
                ensure!(!proceeds.is_zero(), Error::<T>::SwapReturnedZero);
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
                ensure!(!proceeds.is_zero(), Error::<T>::SwapReturnedZero);
                Legs::Long {
                    proceeds,
                    debt: lifted_tao,
                    escrow: lifted_alpha,
                }
            }
        };
        Footprint::<T>::mutate(netuid, side, |f| *f = f.saturating_add(legs.footprint()));
        AlphaToSettle::<T>::mutate(netuid, side, |a| {
            *a = a.saturating_add(legs.alpha_to_settle())
        });

        Ok(Tranche {
            cushion: amount,
            legs,
            exposure_tao: lifted_tao,
            fee_per_day: params.fee_per_day(side, phi, lifted_tao),
        })
    }

    /// Unwind `fraction` of the position at the current price: reverse that share of the open
    /// swap, repay the pool plus the whole fee owed so far, pay the owner what is left of that
    /// share of the cushion. A fraction of one closes the position. Atomic. Returns the TAO that
    /// reached the owner; everything paid is also reported in the event.
    pub(crate) fn do_settle(
        owner: &T::AccountId,
        netuid: NetUid,
        fraction: Perquintill,
        closer: Closer<T::AccountId>,
    ) -> Result<TaoBalance, DispatchError> {
        with_storage_layer(|| {
            let mut position = Positions::<T>::take(owner, netuid).ok_or(Error::<T>::NoPosition)?;
            let side = position.side();
            let full = fraction.is_one();

            let pallet_account = Self::pallet_account();
            let pallet_hotkey = Self::pallet_hotkey()?;
            let now = frame_system::Pallet::<T>::block_number();
            let fee_due = position.fee_owed(now);

            let part = if full {
                position.legs
            } else {
                position.legs.part(fraction)
            };
            let cushion = position.cushion.tao();
            // This share's pot: its cushion plus whatever its closing trade leaves.
            let mut pot = if full {
                cushion
            } else {
                TaoBalance::from(fraction.mul_floor(cushion.to_u64()))
            };
            let mut cushion_rest = cushion.saturating_sub(pot);

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

            // The fee is owed by the whole position. This share pays it first; what it cannot
            // cover comes off the cushion that stays behind.
            let mut fee_paid = take(&mut pot, fee_due);
            fee_paid =
                fee_paid.saturating_add(take(&mut cushion_rest, fee_due.saturating_sub(fee_paid)));
            tao_to_pool = tao_to_pool.saturating_add(fee_paid);

            // A share that could not repay its debt is underwater: the owner gets nothing for it
            // and everything the pallet still holds for it goes to the pool. This does not depend
            // on the swap quotes being accurate; it is the rule that bounds the pool's loss.
            if !shortfall.is_zero() {
                tao_to_pool = tao_to_pool.saturating_add(pot);
                pot = TaoBalance::ZERO;
            }

            // Pay the owner before the pool so the last TAO leaving the pallet account is the
            // pool's share; an owner that cannot be paid forfeits to the pool rather than
            // failing the settlement.
            let tao_to_owner = Self::pay_owner_tao(&pallet_account, owner, pot);
            tao_to_pool = tao_to_pool.saturating_add(pot.saturating_sub(tao_to_owner));

            T::Pool::return_liquidity(
                netuid,
                tao_to_pool,
                alpha_to_pool,
                &pallet_account,
                &pallet_hotkey,
            )?;
            Footprint::<T>::mutate(netuid, side, |f| *f = f.saturating_sub(part.footprint()));
            AlphaToSettle::<T>::mutate(netuid, side, |a| {
                *a = a.saturating_sub(part.alpha_to_settle())
            });

            if full {
                Self::drop_indexes(owner, netuid, &position);
                Self::deposit_event(Event::PositionClosed {
                    owner: owner.clone(),
                    netuid,
                    side,
                    closed_by: closer,
                    tao_to_owner,
                    fee_paid,
                    shortfall,
                });
                return Ok(tao_to_owner);
            }

            position.cushion = Cushion::Tao(cushion_rest);
            position.legs = position.legs.minus(&part);
            position.exposure_tao = position.exposure_tao.saturating_sub(TaoBalance::from(
                fraction.mul_floor(position.exposure_tao.to_u64()),
            ));
            position.fee_per_day = position.fee_per_day.saturating_sub(TaoBalance::from(
                fraction.mul_floor(position.fee_per_day.to_u64()),
            ));
            position.fee_accrued = fee_due.saturating_sub(fee_paid);
            position.last_touch = now;
            let exposure_tao = position.exposure_tao;
            Positions::<T>::insert(owner, netuid, position);

            Self::deposit_event(Event::PositionReduced {
                owner: owner.clone(),
                netuid,
                side,
                fraction,
                tao_to_owner,
                fee_paid,
                shortfall,
                exposure_tao,
            });
            Ok(tao_to_owner)
        })
    }

    /// Dissolution path: this position's share of the one net swap, at its `price`. The pool
    /// takes back what it lent, alpha valued at that price, plus the fee owed; the owner is paid
    /// the rest in TAO, or nothing if the position is underwater at that price, as at any other
    /// settlement. Never fails; anything that cannot reach the owner stays with the pool.
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
        let Some(position) = Positions::<T>::take(owner, netuid) else {
            OpenByNetuid::<T>::remove(netuid, owner);
            return;
        };
        let side = position.side();
        Self::drop_indexes(owner, netuid, &position);
        Footprint::<T>::mutate(netuid, side, |f| {
            *f = f.saturating_sub(position.legs.footprint())
        });
        AlphaToSettle::<T>::mutate(netuid, side, |a| {
            *a = a.saturating_sub(position.legs.alpha_to_settle())
        });

        let pallet_account = Self::pallet_account();
        let now = frame_system::Pallet::<T>::block_number();
        let fee_due = position.fee_owed(now);
        let mut pot = position.cushion.tao();

        // Alpha the pallet holds is handed to the pool and credited at the price; alpha it owes
        // is charged at it. Both round in the pool's favour.
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
            match T::Pool::draw_tao_at_dissolution(netuid, &pallet_account, credit) {
                Ok(()) => pot = pot.saturating_add(credit),
                Err(error) => log::error!(
                    "derivatives: pool could not pay {credit:?} for {owner:?} on {netuid:?}: {error:?}"
                ),
            }
        }

        let repaid = take(&mut pot, owed);
        let fee_paid = take(&mut pot, fee_due);
        tao_to_pool = tao_to_pool.saturating_add(repaid).saturating_add(fee_paid);
        let shortfall_tao = owed.saturating_sub(repaid);
        if !shortfall_tao.is_zero() {
            tao_to_pool = tao_to_pool.saturating_add(pot);
            pot = TaoBalance::ZERO;
        }

        let tao_to_owner = Self::pay_owner_tao(&pallet_account, owner, pot);
        tao_to_pool = tao_to_pool.saturating_add(pot.saturating_sub(tao_to_owner));

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
            tao_to_owner,
            fee_paid,
            shortfall: match side {
                Side::Short => Lent::Alpha(alpha_value(shortfall_tao, price, Rounding::Up)),
                Side::Long => Lent::Tao(shortfall_tao),
            },
        });
    }

    /// Returns how much reached the owner (all of it, or nothing).
    fn pay_owner_tao(from: &T::AccountId, owner: &T::AccountId, amount: TaoBalance) -> TaoBalance {
        if amount.is_zero() {
            return TaoBalance::ZERO;
        }
        match with_storage_layer(|| T::Pool::transfer_tao(from, owner, amount)) {
            Ok(()) => amount,
            Err(_) => TaoBalance::ZERO,
        }
    }

    fn schedule_expiry(
        owner: &T::AccountId,
        netuid: NetUid,
        mut at: BlockNumberFor<T>,
    ) -> Result<BlockNumberFor<T>, DispatchError> {
        for _ in 0..MAX_EXPIRY_SHIFT {
            let pushed = Expiring::<T>::try_mutate(at, |queue| {
                queue.try_push((owner.clone(), netuid)).map_err(|_| ())
            });
            if pushed.is_ok() {
                return Ok(at);
            }
            at.saturating_inc();
        }
        Err(Error::<T>::ExpiryQueueFull.into())
    }

    /// After a failed sweep: queue the position again `RETRY_DELAY` blocks out, at most
    /// `MAX_SETTLE_RETRIES` times. Returns the retry block, or `None` once the position is left
    /// to permissionless `close`.
    pub(crate) fn reschedule_failed(
        owner: &T::AccountId,
        netuid: NetUid,
        now: BlockNumberFor<T>,
    ) -> Option<BlockNumberFor<T>> {
        Positions::<T>::mutate_exists(owner, netuid, |slot| {
            let position = slot.as_mut()?;
            if position.failed_sweeps >= MAX_SETTLE_RETRIES {
                return None;
            }
            let at = Self::schedule_expiry(owner, netuid, now.saturating_add(RETRY_DELAY.into()))
                .ok()?;
            position.failed_sweeps.saturating_inc();
            position.queued_at = at;
            Some(at)
        })
    }

    /// Remove a closed position from the subnet index and the expiry queue. The footprint is
    /// the caller's: a partial settlement releases only its share.
    fn drop_indexes(owner: &T::AccountId, netuid: NetUid, position: &Position<BlockNumberFor<T>>) {
        OpenByNetuid::<T>::remove(netuid, owner);
        let mut queue = Expiring::<T>::take(position.queued_at);
        let entry = (owner.clone(), netuid);
        queue.retain(|queued| queued != &entry);
        if !queue.is_empty() {
            Expiring::<T>::insert(position.queued_at, queue);
        }
    }
}
