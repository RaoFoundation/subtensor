//! Opening and settling positions. Every pool touch goes through `T::Pool`.

use frame_support::storage::with_storage_layer;

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

impl<T: Config> Pallet<T> {
    /// Open a position. Its own storage layer: the cushion transfer, the lift, and the opening
    /// swap all roll back if anything after them fails, whoever the caller is.
    pub(crate) fn do_open(
        owner: T::AccountId,
        netuid: NetUid,
        side: Side,
        cushion: TaoBalance,
    ) -> DispatchResult {
        with_storage_layer(|| Self::open_in_layer(owner, netuid, side, cushion))
    }

    fn open_in_layer(
        owner: T::AccountId,
        netuid: NetUid,
        side: Side,
        cushion: TaoBalance,
    ) -> DispatchResult {
        let params = Params::<T>::get();
        let override_ = SubnetOverrides::<T>::get(netuid);
        let enabled = override_
            .map(|o| o.side_enabled(side))
            .unwrap_or_else(|| params.side_enabled(side));
        ensure!(enabled, Error::<T>::SideDisabled);
        ensure!(T::Pool::is_dynamic(netuid), Error::<T>::SubnetNotDynamic);
        ensure!(
            !Positions::<T>::contains_key(&owner, (netuid, side)),
            Error::<T>::PositionExists
        );

        let pallet_account = Self::pallet_account();
        let pallet_hotkey = Self::pallet_hotkey()?;

        let (tao_reserve, alpha_reserve) = T::Pool::reserves(netuid);
        let (t, a) = (tao_reserve.to_u64(), alpha_reserve.to_u64());
        ensure!(t > 0 && a > 0, Error::<T>::SubnetNotDynamic);

        ensure!(!cushion.is_zero(), Error::<T>::ZeroExposure);
        ensure!(cushion >= params.min_deposit_tao, Error::<T>::DepositTooLow);

        let phi = pool_fraction(params.leverage_percent(side), cushion.to_u64(), t)
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

        T::Pool::transfer_tao(&owner, &pallet_account, cushion)?;

        let (lifted_tao, lifted_alpha) =
            T::Pool::lift_liquidity(netuid, phi, &pallet_account, &pallet_hotkey)?;
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

        let now = frame_system::Pallet::<T>::block_number();
        let expires_at = Self::schedule_expiry(
            &owner,
            netuid,
            side,
            now.saturating_add(params.lifetime_blocks),
        )?;
        let fee_per_day = params.fee_per_day(side, phi, lifted_tao);

        Positions::<T>::insert(
            &owner,
            (netuid, side),
            Position {
                cushion: Cushion::Tao(cushion),
                legs,
                exposure_tao: lifted_tao,
                fee_per_day,
                opened_at: now,
                expires_at,
                queued_at: expires_at,
                failed_sweeps: 0,
            },
        );
        OpenByNetuid::<T>::insert(netuid, (owner.clone(), side), ());
        Footprint::<T>::mutate(netuid, side, |f| *f = f.saturating_add(legs.footprint()));

        Self::deposit_event(Event::PositionOpened {
            owner,
            netuid,
            side,
            cushion: Cushion::Tao(cushion),
            legs,
            exposure_tao: lifted_tao,
            fee_per_day,
            expires_at,
        });
        Ok(())
    }

    /// Settle, then reopen the same side with the TAO that came back plus `top_up`. One storage
    /// layer around both: if the reopen fails, the settlement rolls back and the old position
    /// is still there.
    pub(crate) fn do_roll(
        owner: T::AccountId,
        netuid: NetUid,
        side: Side,
        top_up: TaoBalance,
    ) -> DispatchResult {
        with_storage_layer(|| {
            let back = Self::do_settle(&owner, netuid, side, Closer::Roll)?;
            Self::do_open(owner, netuid, side, back.saturating_add(top_up))
        })
    }

    /// Reverse the open swap, repay the pool plus fee, pay the owner what is left. Atomic.
    /// Returns the TAO that reached the owner; everything paid is also reported in
    /// `Event::PositionClosed`.
    pub(crate) fn do_settle(
        owner: &T::AccountId,
        netuid: NetUid,
        side: Side,
        closer: Closer<T::AccountId>,
    ) -> Result<TaoBalance, DispatchError> {
        with_storage_layer(|| {
            let position =
                Positions::<T>::take(owner, (netuid, side)).ok_or(Error::<T>::NoPosition)?;
            Self::drop_indexes(owner, netuid, side, &position);

            let pallet_account = Self::pallet_account();
            let pallet_hotkey = Self::pallet_hotkey()?;
            let now = frame_system::Pallet::<T>::block_number();
            let blocks_open: u64 = now
                .saturating_sub(position.opened_at)
                .unique_saturated_into();
            let fee_due = accrued_fee(position.fee_per_day, blocks_open);

            // Everything the pallet holds for the owner, in TAO: the cushion plus whatever the
            // closing trade leaves.
            let mut pot = position.cushion.tao();

            let (mut tao_to_pool, alpha_to_pool, shortfall) = match position.legs {
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

            let fee_paid = take(&mut pot, fee_due);
            tao_to_pool = tao_to_pool.saturating_add(fee_paid);

            // A position that could not repay its debt is underwater: the owner gets nothing and
            // everything the pallet still holds goes to the pool. This does not depend on the
            // swap quotes being accurate; it is the rule that bounds the pool's loss.
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

            Self::deposit_event(Event::PositionClosed {
                owner: owner.clone(),
                netuid,
                side,
                closed_by: closer,
                tao_to_owner,
                fee_paid,
                shortfall,
            });
            Ok(tao_to_owner)
        })
    }

    /// Dissolution path: hand the lifted slice back in kind, hand the cushion back, no swaps,
    /// no fee. Never fails; anything that cannot reach the owner stays with the pool.
    pub(crate) fn unwind(owner: &T::AccountId, netuid: NetUid, side: Side) {
        // A position can only exist once the hotkey is claimed.
        let Ok(pallet_hotkey) = Self::pallet_hotkey() else {
            return;
        };
        let Some(position) = Positions::<T>::take(owner, (netuid, side)) else {
            OpenByNetuid::<T>::remove(netuid, (owner.clone(), side));
            return;
        };
        Self::drop_indexes(owner, netuid, side, &position);

        let pallet_account = Self::pallet_account();
        let (mut tao_to_pool, alpha_to_pool) = match position.legs {
            Legs::Short {
                proceeds, escrow, ..
            } => (proceeds.saturating_add(escrow), AlphaBalance::ZERO),
            Legs::Long {
                proceeds, escrow, ..
            } => (TaoBalance::ZERO, proceeds.saturating_add(escrow)),
        };

        let cushion = position.cushion.tao();
        let tao_to_owner = Self::pay_owner_tao(&pallet_account, owner, cushion);
        tao_to_pool = tao_to_pool.saturating_add(cushion.saturating_sub(tao_to_owner));

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
            fee_paid: TaoBalance::ZERO,
            shortfall: match position.legs {
                Legs::Short { .. } => Lent::Alpha(AlphaBalance::ZERO),
                Legs::Long { .. } => Lent::Tao(TaoBalance::ZERO),
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
        side: Side,
        mut at: BlockNumberFor<T>,
    ) -> Result<BlockNumberFor<T>, DispatchError> {
        for _ in 0..MAX_EXPIRY_SHIFT {
            let pushed = Expiring::<T>::try_mutate(at, |queue| {
                queue
                    .try_push((owner.clone(), netuid, side))
                    .map_err(|_| ())
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
        side: Side,
        now: BlockNumberFor<T>,
    ) -> Option<BlockNumberFor<T>> {
        Positions::<T>::mutate_exists(owner, (netuid, side), |slot| {
            let position = slot.as_mut()?;
            if position.failed_sweeps >= MAX_SETTLE_RETRIES {
                return None;
            }
            let at =
                Self::schedule_expiry(owner, netuid, side, now.saturating_add(RETRY_DELAY.into()))
                    .ok()?;
            position.failed_sweeps.saturating_inc();
            position.queued_at = at;
            Some(at)
        })
    }

    fn drop_indexes(
        owner: &T::AccountId,
        netuid: NetUid,
        side: Side,
        position: &Position<BlockNumberFor<T>>,
    ) {
        OpenByNetuid::<T>::remove(netuid, (owner.clone(), side));
        Footprint::<T>::mutate(netuid, side, |f| {
            *f = f.saturating_sub(position.legs.footprint())
        });
        let mut queue = Expiring::<T>::take(position.queued_at);
        let entry = (owner.clone(), netuid, side);
        queue.retain(|queued| queued != &entry);
        if !queue.is_empty() {
            Expiring::<T>::insert(position.queued_at, queue);
        }
    }
}
