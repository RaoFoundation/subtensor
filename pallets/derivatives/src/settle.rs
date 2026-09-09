//! Adding to and settling positions. Every pool touch goes through `T::Pool`.
//!
//! A position is a sum of tranches, so there are only two primitives here: [`Pallet::lift_tranche`]
//! borrows one more slice of the pool and returns it as a [`Tranche`] to be folded in, and
//! [`Pallet::do_settle`] unwinds a fraction of the position at the current price. `add` is
//! built from those two: it settles when the deposit is on the other side, and grows an
//! existing or empty position otherwise. `close` is `do_settle` with a fraction of one.

use frame_support::storage::with_storage_layer;
use sp_runtime::{Rounding, helpers_128bit::multiply_by_rational_with_rounding};

use crate::*;

/// Take up to `want` from `pot`. Returns what was taken; less than `want` means the pot is
/// out.
fn take(pot: &mut TaoBalance, want: TaoBalance) -> TaoBalance {
    let taken = want.min(*pot);
    *pot = pot.saturating_sub(taken);
    taken
}

/// TAO and alpha the pallet holds for one share of a position and may spend while settling it:
/// that share of the cushion plus whatever its closing trade leaves.
struct Pot {
    tao: TaoBalance,
    alpha: AlphaBalance,
}

impl Pot {
    fn from_cushion<AccountId>(cushion: &Cushion<AccountId>) -> Self {
        Self {
            tao: cushion.tao,
            alpha: cushion.alpha,
        }
    }

    /// Take up to `want` alpha. Returns what was taken.
    fn draw_alpha(&mut self, want: AlphaBalance) -> AlphaBalance {
        let taken = want.min(self.alpha);
        self.alpha = self.alpha.saturating_sub(taken);
        taken
    }

    /// Cover `want` TAO: from the TAO held first, then by selling alpha. Any surplus from the
    /// sale stays in the pot. Returns what was covered; less than `want` means the pot is out.
    fn cover_tao<T: Config>(
        &mut self,
        want: TaoBalance,
        netuid: NetUid,
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
    ) -> Result<TaoBalance, DispatchError> {
        let from_tao = take(&mut self.tao, want);
        let gap = want.saturating_sub(from_tao);
        if gap.is_zero() || self.alpha.is_zero() {
            return Ok(from_tao);
        }
        let (sold, got) = T::Pool::sell_alpha_for(coldkey, hotkey, netuid, gap, self.alpha)?;
        self.alpha = self.alpha.saturating_sub(sold);
        let from_sale = got.min(gap);
        self.tao = self.tao.saturating_add(got.saturating_sub(from_sale));
        Ok(from_tao.saturating_add(from_sale))
    }

    /// Sell whatever alpha is left, so the whole pot is TAO.
    fn cash_out<T: Config>(
        &mut self,
        netuid: NetUid,
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
    ) -> DispatchResult {
        if self.alpha.is_zero() {
            return Ok(());
        }
        let got = T::Pool::sell_alpha_internal(coldkey, hotkey, netuid, self.alpha)?;
        self.alpha = AlphaBalance::ZERO;
        self.tao = self.tao.saturating_add(got);
        Ok(())
    }

    /// Empty the pot into `(tao, alpha)`.
    fn drain(&mut self) -> (TaoBalance, AlphaBalance) {
        let out = (self.tao, self.alpha);
        self.tao = TaoBalance::ZERO;
        self.alpha = AlphaBalance::ZERO;
        out
    }
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
        deposit: Deposit<T::AccountId>,
        leverage_percent: u16,
    ) -> DispatchResult {
        with_storage_layer(|| {
            let params = Params::<T>::get();
            ensure!(
                params.leverage_allowed(side, leverage_percent),
                Error::<T>::LeverageOutOfRange
            );
            let now = frame_system::Pallet::<T>::block_number();
            let fresh = || Position::empty(side, now);
            let (position, deposit) = match Positions::<T>::get(&owner, netuid) {
                None => (fresh(), deposit),
                Some(position) if position.side() == side => (position, deposit),
                Some(position) => {
                    match Self::reduce(
                        &owner,
                        netuid,
                        position,
                        deposit,
                        leverage_percent,
                        &params,
                    )? {
                        None => return Ok(()),
                        Some(rest) => (fresh(), rest),
                    }
                }
            };
            Self::grow(owner, netuid, position, deposit, leverage_percent, &params)
        })
    }

    /// Lift a tranche for `deposit` and fold it into `position`, which may be empty. Sums for
    /// everything but `opened_at`.
    fn grow(
        owner: T::AccountId,
        netuid: NetUid,
        mut position: Position<T::AccountId, BlockNumberFor<T>>,
        deposit: Deposit<T::AccountId>,
        leverage_percent: u16,
        params: &DerivativesParams,
    ) -> DispatchResult {
        let side = position.side();
        let tranche = Self::lift_tranche(&owner, netuid, side, deposit, leverage_percent, params)?;
        let added = Event::PositionAdded {
            owner: owner.clone(),
            netuid,
            side,
            cushion_added: tranche.cushion.clone(),
            leverage_percent,
            legs_added: tranche.legs,
            exposure_added: tranche.exposure_tao,
            fee_per_day_added: tranche.fee_per_day,
            exposure_tao: position.exposure_tao.saturating_add(tranche.exposure_tao),
        };
        position
            .fold(tranche, frame_system::Pallet::<T>::block_number())
            .ok_or(DispatchError::Corruption)?;
        Positions::<T>::insert(&owner, netuid, position);
        OpenByNetuid::<T>::insert(netuid, &owner, ());
        Self::deposit_event(added);
        Ok(())
    }

    /// Other side: `leverage` times the deposit's TAO value comes off the position. Less than
    /// it holds settles that share and returns `None`; more closes it and returns the part of
    /// the deposit past the flip point, to open the other side with, or `None` if that part is
    /// below `min_deposit_tao`.
    fn reduce(
        owner: &T::AccountId,
        netuid: NetUid,
        position: Position<T::AccountId, BlockNumberFor<T>>,
        deposit: Deposit<T::AccountId>,
        leverage_percent: u16,
        params: &DerivativesParams,
    ) -> Result<Option<Deposit<T::AccountId>>, DispatchError> {
        let (tao_reserve, alpha_reserve) = T::Pool::reserves(netuid);
        let value = match &deposit {
            Deposit::Tao(amount) => amount.to_u64(),
            Deposit::Alpha { amount, .. } => alpha_value_in_tao(
                amount.to_u64(),
                tao_reserve.to_u64(),
                alpha_reserve.to_u64(),
            ),
        };
        let asked = u128::from(value)
            .saturating_mul(u128::from(leverage_percent))
            .checked_div(100)
            .unwrap_or(0);
        ensure!(asked > 0, Error::<T>::ZeroExposure);
        let held = u128::from(position.exposure_tao.to_u64());

        if asked < held {
            // Both fit in u64: `asked < held <= u64::MAX`.
            let fraction = Perquintill::from_rational(asked as u64, held as u64);
            Self::do_settle(owner, netuid, fraction, Closer::Owner)?;
            return Ok(None);
        }

        Self::do_settle(owner, netuid, Perquintill::one(), Closer::Owner)?;
        let rest = asked.saturating_sub(held);
        let rest_value = rest
            .saturating_mul(100)
            .checked_div(u128::from(leverage_percent))
            .unwrap_or(0)
            .min(u128::from(u64::MAX)) as u64;
        if TaoBalance::from(rest_value) < params.min_deposit_tao {
            // Dust past the flip point is not worth a position.
            return Ok(None);
        }
        Ok(Some(deposit.part(Perquintill::from_rational(rest, asked))))
    }

    /// Take `deposit` from `owner`, lift `phi` of the pool and swap the borrowed half. The
    /// footprint is booked here; the caller folds the rest into a position.
    fn lift_tranche(
        owner: &T::AccountId,
        netuid: NetUid,
        side: Side,
        deposit: Deposit<T::AccountId>,
        leverage_percent: u16,
        params: &DerivativesParams,
    ) -> Result<Tranche<T::AccountId>, DispatchError> {
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

        // An alpha deposit is sized against the alpha reserve, which is the same share of the
        // pool as its spot value against the TAO reserve.
        let (deposit_amount, deposit_reserve, deposit_value_tao) = match &deposit {
            Deposit::Tao(amount) => (amount.to_u64(), t, amount.to_u64()),
            Deposit::Alpha { amount, .. } => {
                ensure!(
                    params.alpha_cushion_allowed(side),
                    Error::<T>::AlphaCushionDisabled
                );
                (
                    amount.to_u64(),
                    a,
                    alpha_value_in_tao(amount.to_u64(), t, a),
                )
            }
        };
        ensure!(deposit_amount > 0, Error::<T>::ZeroExposure);
        ensure!(
            TaoBalance::from(deposit_value_tao) >= params.min_deposit_tao,
            Error::<T>::DepositTooLow
        );

        let phi = pool_fraction(leverage_percent, deposit_amount, deposit_reserve)
            .ok_or(Error::<T>::ExposureTooLarge)?;

        let lent_reserve = match side {
            Side::Short => t,
            Side::Long => a,
        };
        let max_pool_share = override_
            .and_then(|o| o.max_pool_share)
            .unwrap_or(params.max_pool_share);
        let rate_per_year = override_
            .and_then(|o| o.rate_per_year)
            .unwrap_or(params.rate_per_year);
        let cap = max_pool_share.mul_floor(lent_reserve);
        let projected = projected_footprint(phi, lent_reserve);
        ensure!(
            Footprint::<T>::get(netuid, side).saturating_add(projected) <= cap,
            Error::<T>::PoolCapExceeded
        );

        match &deposit {
            Deposit::Tao(amount) => T::Pool::transfer_tao(owner, &pallet_account, *amount)?,
            Deposit::Alpha { hotkey, amount } => T::Pool::transfer_staked_alpha(
                owner,
                hotkey,
                &pallet_account,
                &pallet_hotkey,
                netuid,
                *amount,
                true,
                false,
            )?,
        }

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
            cushion: deposit,
            legs,
            exposure_tao: lifted_tao,
            fee_per_day: DerivativesParams::fee_per_day(rate_per_year, lifted_tao),
        })
    }

    /// Unwind `fraction` of the position at the current price: reverse that share of the open
    /// swap, repay the pool plus the whole rent owed so far, pay the owner what is left of that
    /// share of the cushion, in kind. A fraction of one closes the position. Atomic. Returns the
    /// TAO that reached the owner; everything paid is also reported in the event.
    ///
    /// A [`Closer::Liquidator`] is paid instead of the owner, in TAO: the rent owed, plus what
    /// is left after the pool is repaid, topped up from the pool to one day of rent when that is
    /// less. The liquidator only gets here once the position is unhealthy, so what the owner
    /// forgoes is at most one day of rent.
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
            let cushion_part = if full {
                position.cushion.clone()
            } else {
                position.cushion.part(fraction)
            };
            let cushion_rest = position.cushion.minus(&cushion_part);
            // This share's pot: its cushion plus whatever its closing trade leaves.
            let mut pot = Pot::from_cushion(&cushion_part);
            let mut rest = Pot::from_cushion(&cushion_rest);

            let (mut tao_to_pool, mut alpha_to_pool, shortfall) = match part {
                Legs::Short {
                    proceeds,
                    debt,
                    escrow,
                } => {
                    pot.tao = pot.tao.saturating_add(proceeds);
                    let (spent, bought) = T::Pool::buy_alpha_for(
                        &pallet_account,
                        &pallet_hotkey,
                        netuid,
                        debt,
                        pot.tao,
                    )?;
                    pot.tao = pot.tao.saturating_sub(spent);
                    // An alpha cushion tops up what the buyback missed; bought surplus is dust
                    // that goes back with the debt.
                    let alpha_back =
                        bought.saturating_add(pot.draw_alpha(debt.saturating_sub(bought)));
                    (
                        escrow,
                        alpha_back,
                        Lent::Alpha(debt.saturating_sub(alpha_back)),
                    )
                }
                Legs::Long {
                    proceeds,
                    debt,
                    escrow,
                } => {
                    pot.tao = pot.tao.saturating_add(T::Pool::sell_alpha_internal(
                        &pallet_account,
                        &pallet_hotkey,
                        netuid,
                        proceeds,
                    )?);
                    let repaid =
                        pot.cover_tao::<T>(debt, netuid, &pallet_account, &pallet_hotkey)?;
                    (repaid, escrow, Lent::Tao(debt.saturating_sub(repaid)))
                }
            };

            // The fee is owed by the whole position. This share pays it first; what it cannot
            // cover comes off the cushion that stays behind.
            let mut fee_paid =
                pot.cover_tao::<T>(fee_due, netuid, &pallet_account, &pallet_hotkey)?;
            fee_paid = fee_paid.saturating_add(rest.cover_tao::<T>(
                fee_due.saturating_sub(fee_paid),
                netuid,
                &pallet_account,
                &pallet_hotkey,
            )?);

            // A share that could not repay its debt is underwater: the owner gets nothing for it
            // and everything the pallet still holds for it goes to the pool. This does not depend
            // on the swap quotes being accurate; it is the rule that bounds the pool's loss.
            if !shortfall.is_zero() {
                let (tao, alpha) = pot.drain();
                tao_to_pool = tao_to_pool.saturating_add(tao);
                alpha_to_pool = alpha_to_pool.saturating_add(alpha);
            }

            // Pay people before the pool so the last TAO leaving the pallet account is the
            // pool's share; a payee that cannot be paid forfeits to the pool rather than failing
            // the settlement.
            let (tao_to_owner, alpha_to_owner, bounty) = match &closer {
                Closer::Liquidator(liquidator) => {
                    pot.cash_out::<T>(netuid, &pallet_account, &pallet_hotkey)?;
                    let due = fee_paid.saturating_add(pot.tao);
                    let paid = Self::pay_owner_tao(&pallet_account, liquidator, due);
                    tao_to_pool = tao_to_pool.saturating_add(due.saturating_sub(paid));
                    let floor = position.fee_per_day.saturating_sub(paid);
                    let topped_up = if floor.is_zero()
                        || T::Pool::draw_tao(netuid, liquidator, floor).is_err()
                    {
                        TaoBalance::ZERO
                    } else {
                        floor
                    };
                    (
                        TaoBalance::ZERO,
                        AlphaBalance::ZERO,
                        paid.saturating_add(topped_up),
                    )
                }
                Closer::Owner | Closer::Dissolution => {
                    tao_to_pool = tao_to_pool.saturating_add(fee_paid);
                    // Alpha goes back first; what cannot go back in kind is sold and joins the
                    // TAO. If it cannot be sold either, it stays with the pool.
                    let alpha_to_owner = Self::return_alpha(
                        &pallet_account,
                        &pallet_hotkey,
                        owner,
                        position.cushion.alpha_hotkey.as_ref(),
                        netuid,
                        pot.alpha,
                    );
                    pot.alpha = pot.alpha.saturating_sub(alpha_to_owner);
                    if pot
                        .cash_out::<T>(netuid, &pallet_account, &pallet_hotkey)
                        .is_err()
                    {
                        alpha_to_pool = alpha_to_pool.saturating_add(pot.alpha);
                        pot.alpha = AlphaBalance::ZERO;
                    }
                    let tao_to_owner = Self::pay_owner_tao(&pallet_account, owner, pot.tao);
                    tao_to_pool = tao_to_pool.saturating_add(pot.tao.saturating_sub(tao_to_owner));
                    (tao_to_owner, alpha_to_owner, TaoBalance::ZERO)
                }
            };

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
                OpenByNetuid::<T>::remove(netuid, owner);
                Self::deposit_event(Event::PositionClosed {
                    owner: owner.clone(),
                    netuid,
                    side,
                    closed_by: closer,
                    tao_to_owner,
                    alpha_to_owner,
                    fee_paid,
                    shortfall,
                    bounty,
                });
                return Ok(tao_to_owner);
            }

            position.cushion = Cushion {
                tao: rest.tao,
                alpha: rest.alpha,
                alpha_hotkey: cushion_rest.alpha_hotkey,
            };
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
                alpha_to_owner,
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
    /// settlement. Cushion alpha goes back to the owner's hotkey in kind, to be cashed out with
    /// every other stake; if that hotkey is gone it is valued at the price instead. Never fails;
    /// anything that cannot reach the owner stays with the pool.
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
        OpenByNetuid::<T>::remove(netuid, owner);
        Footprint::<T>::mutate(netuid, side, |f| {
            *f = f.saturating_sub(position.legs.footprint())
        });
        AlphaToSettle::<T>::mutate(netuid, side, |a| {
            *a = a.saturating_sub(position.legs.alpha_to_settle())
        });

        let pallet_account = Self::pallet_account();
        let now = frame_system::Pallet::<T>::block_number();
        let fee_due = position.fee_owed(now);
        let mut pot = position.cushion.tao;

        // Alpha the pallet holds is handed to the pool and credited at the price; alpha it owes
        // is charged at it. Both round in the pool's favour.
        let (mut credit, owed, mut tao_to_pool, mut alpha_to_pool) = match position.legs {
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

        // The cushion's alpha is the owner's own stake: back to their hotkey if it still exists,
        // otherwise to the pool at the price like the rest.
        let alpha_to_owner = Self::return_alpha(
            &pallet_account,
            &pallet_hotkey,
            owner,
            position.cushion.alpha_hotkey.as_ref(),
            netuid,
            position.cushion.alpha,
        );
        let alpha_stranded = position.cushion.alpha.saturating_sub(alpha_to_owner);
        if !alpha_stranded.is_zero() {
            credit = credit.saturating_add(tao_value(alpha_stranded, price, Rounding::Down));
            alpha_to_pool = alpha_to_pool.saturating_add(alpha_stranded);
        }

        if !credit.is_zero() {
            match T::Pool::draw_tao(netuid, &pallet_account, credit) {
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
            alpha_to_owner,
            fee_paid,
            shortfall: match side {
                Side::Short => Lent::Alpha(alpha_value(shortfall_tao, price, Rounding::Up)),
                Side::Long => Lent::Tao(shortfall_tao),
            },
            bounty: TaoBalance::ZERO,
        });
    }

    /// Returns how much reached `to` (all of it, or nothing).
    fn pay_owner_tao(from: &T::AccountId, to: &T::AccountId, amount: TaoBalance) -> TaoBalance {
        if amount.is_zero() {
            return TaoBalance::ZERO;
        }
        match with_storage_layer(|| T::Pool::transfer_tao(from, to, amount)) {
            Ok(()) => amount,
            Err(_) => TaoBalance::ZERO,
        }
    }

    /// Stake `amount` cushion alpha back to `(owner, hotkey)`. Returns how much went back: all
    /// of it, or nothing if there is no hotkey or it is gone (swapped or deregistered since the
    /// deposit). The caller decides what to do with the rest.
    fn return_alpha(
        from_coldkey: &T::AccountId,
        from_hotkey: &T::AccountId,
        owner: &T::AccountId,
        hotkey: Option<&T::AccountId>,
        netuid: NetUid,
        amount: AlphaBalance,
    ) -> AlphaBalance {
        let Some(hotkey) = hotkey else {
            return AlphaBalance::ZERO;
        };
        if amount.is_zero() {
            return AlphaBalance::ZERO;
        }
        match with_storage_layer(|| {
            T::Pool::transfer_stake_internal(
                from_coldkey,
                from_hotkey,
                owner,
                hotkey,
                netuid,
                amount,
            )
        }) {
            Ok(()) => amount,
            Err(_) => AlphaBalance::ZERO,
        }
    }
}
