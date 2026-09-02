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

/// TAO and alpha the pallet holds for one position and may spend while settling it.
struct Pot {
    tao: TaoBalance,
    alpha: AlphaBalance,
}

impl Pot {
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
        let from_tao = want.min(self.tao);
        self.tao = self.tao.saturating_sub(from_tao);
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

    /// Empty the pot. Returns `(tao, alpha)`.
    fn drain(&mut self) -> (TaoBalance, AlphaBalance) {
        let out = (self.tao, self.alpha);
        self.tao = TaoBalance::ZERO;
        self.alpha = AlphaBalance::ZERO;
        out
    }
}

/// What became of a cushion's alpha at close.
enum AlphaPayout {
    /// Staked back on the owner's hotkey.
    InKind(AlphaBalance),
    /// The owner's hotkey is gone; the alpha was sold and this TAO awaits the owner in the
    /// pallet account.
    Sold(TaoBalance),
    /// Neither worked; the alpha is still held and goes to the pool.
    Unpaid(AlphaBalance),
}

impl AlphaPayout {
    /// `(alpha reaching the owner, TAO joining the pot, alpha left for the pool)`.
    fn split(self) -> (AlphaBalance, TaoBalance, AlphaBalance) {
        match self {
            AlphaPayout::InKind(alpha) => (alpha, TaoBalance::ZERO, AlphaBalance::ZERO),
            AlphaPayout::Sold(tao) => (AlphaBalance::ZERO, tao, AlphaBalance::ZERO),
            AlphaPayout::Unpaid(alpha) => (AlphaBalance::ZERO, TaoBalance::ZERO, alpha),
        }
    }
}

impl<T: Config> Pallet<T> {
    pub(crate) fn do_open(
        owner: T::AccountId,
        netuid: NetUid,
        side: Side,
        deposit: Deposit<T::AccountId>,
    ) -> DispatchResult {
        let params = Params::<T>::get();
        let enabled = match side {
            Side::Short => params.shorts_enabled,
            Side::Long => params.longs_enabled,
        };
        ensure!(enabled, Error::<T>::SideDisabled);
        ensure!(T::Pool::is_dynamic(netuid), Error::<T>::SubnetNotDynamic);
        ensure!(
            !Positions::<T>::contains_key(&owner, (netuid, side)),
            Error::<T>::PositionExists
        );

        let pallet_account = Self::pallet_account();
        let pallet_hotkey = T::PalletHotkey::get();

        let (tao_reserve, alpha_reserve) = T::Pool::reserves(netuid);
        let (t, a) = (tao_reserve.to_u64(), alpha_reserve.to_u64());
        ensure!(t > 0 && a > 0, Error::<T>::SubnetNotDynamic);

        let (deposit_amount, deposit_reserve, deposit_value_tao) = match &deposit {
            Deposit::Tao(amount) => (amount.to_u64(), t, amount.to_u64()),
            Deposit::Alpha { amount, .. } => (
                amount.to_u64(),
                a,
                alpha_value_in_tao(amount.to_u64(), t, a),
            ),
        };
        ensure!(deposit_amount > 0, Error::<T>::ZeroExposure);
        ensure!(
            TaoBalance::from(deposit_value_tao) >= params.min_deposit_tao,
            Error::<T>::DepositTooLow
        );

        let phi = pool_fraction(params.leverage_percent, deposit_amount, deposit_reserve)
            .ok_or(Error::<T>::ExposureTooLarge)?;

        let lent_reserve = match side {
            Side::Short => t,
            Side::Long => a,
        };
        let cap = params.max_pool_share.mul_floor(lent_reserve);
        let projected = projected_footprint(phi, lent_reserve);
        ensure!(
            Footprint::<T>::get(netuid, side).saturating_add(projected) <= cap,
            Error::<T>::PoolCapExceeded
        );

        match &deposit {
            Deposit::Tao(amount) => T::Pool::transfer_tao(&owner, &pallet_account, *amount)?,
            Deposit::Alpha { hotkey, amount } => T::Pool::transfer_staked_alpha(
                &owner,
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
                cushion: deposit.clone(),
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
            cushion: deposit,
            legs,
            exposure_tao: lifted_tao,
            fee_per_day,
            expires_at,
        });
        Ok(())
    }

    /// Settle, then reopen the same side with what reached the owner. One transaction: if the
    /// reopen fails, the old position is still there.
    pub(crate) fn do_roll(
        owner: T::AccountId,
        netuid: NetUid,
        side: Side,
        top_up: Option<Deposit<T::AccountId>>,
    ) -> DispatchResult {
        let cushion_hotkey = Positions::<T>::get(&owner, (netuid, side))
            .ok_or(Error::<T>::NoPosition)?
            .cushion
            .alpha_hotkey()
            .cloned();
        let (tao_back, alpha_back) = Self::do_settle(&owner, netuid, side, Closer::Roll)?;

        // Reopen in the token the cushion came back in. An alpha cushion that had to be sold
        // (hotkey gone) comes back as TAO and rolls as TAO.
        let mut deposit = match cushion_hotkey {
            Some(hotkey) if !alpha_back.is_zero() => Deposit::Alpha {
                hotkey,
                amount: alpha_back,
            },
            _ => Deposit::Tao(tao_back),
        };
        if let Some(extra) = top_up {
            deposit = match (deposit, extra) {
                (Deposit::Tao(back), Deposit::Tao(more)) => Deposit::Tao(back.saturating_add(more)),
                (
                    Deposit::Alpha { hotkey, amount },
                    Deposit::Alpha {
                        hotkey: extra_hotkey,
                        amount: more,
                    },
                ) if hotkey == extra_hotkey => Deposit::Alpha {
                    hotkey,
                    amount: amount.saturating_add(more),
                },
                _ => return Err(Error::<T>::TopUpMismatch.into()),
            };
        }
        Self::do_open(owner, netuid, side, deposit)
    }

    /// Reverse the open swap, repay the pool plus fee, pay the owner what is left. Atomic.
    /// Returns `(tao, alpha)` that reached the owner; everything paid is also reported in
    /// `Event::PositionClosed`.
    pub(crate) fn do_settle(
        owner: &T::AccountId,
        netuid: NetUid,
        side: Side,
        closer: Closer<T::AccountId>,
    ) -> Result<(TaoBalance, AlphaBalance), DispatchError> {
        with_storage_layer(|| {
            let position =
                Positions::<T>::take(owner, (netuid, side)).ok_or(Error::<T>::NoPosition)?;
            Self::drop_indexes(owner, netuid, side, &position);

            let pallet_account = Self::pallet_account();
            let pallet_hotkey = T::PalletHotkey::get();
            let now = frame_system::Pallet::<T>::block_number();
            let blocks_open: u64 = now
                .saturating_sub(position.opened_at)
                .unique_saturated_into();
            let fee_due = accrued_fee(position.fee_per_day, blocks_open);

            let mut pot = Pot {
                tao: position.cushion.tao_part(),
                alpha: position.cushion.alpha_part(),
            };

            let (mut tao_to_pool, mut alpha_to_pool, shortfall) = match position.legs {
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

            let fee_paid = pot.cover_tao::<T>(fee_due, netuid, &pallet_account, &pallet_hotkey)?;
            tao_to_pool = tao_to_pool.saturating_add(fee_paid);

            // A position that could not repay its debt is underwater: the owner gets nothing and
            // everything the pallet still holds goes to the pool. This does not depend on the
            // swap quotes being accurate — it is the rule that bounds the pool's loss.
            if !shortfall.is_zero() {
                let (tao, alpha) = pot.drain();
                tao_to_pool = tao_to_pool.saturating_add(tao);
                alpha_to_pool = alpha_to_pool.saturating_add(alpha);
            }

            // Pay the owner before the pool so the last TAO leaving the pallet account is the
            // pool's share; an owner that cannot be paid forfeits to the pool rather than
            // failing the settlement. Alpha goes first: a cushion that cannot go back in kind
            // is sold, and that TAO joins the pot.
            let (alpha_to_owner, cushion_sold, alpha_unpaid) = Self::pay_owner_alpha(
                &pallet_account,
                &pallet_hotkey,
                owner,
                position.cushion.alpha_hotkey(),
                netuid,
                pot.alpha,
            )
            .split();
            alpha_to_pool = alpha_to_pool.saturating_add(alpha_unpaid);
            pot.tao = pot.tao.saturating_add(cushion_sold);
            let tao_to_owner = Self::pay_owner_tao(&pallet_account, owner, pot.tao);
            tao_to_pool = tao_to_pool.saturating_add(pot.tao.saturating_sub(tao_to_owner));

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
                alpha_to_owner,
                fee_paid,
                shortfall,
            });
            Ok((tao_to_owner, alpha_to_owner))
        })
    }

    /// Dissolution path: hand the lifted slice back in kind, hand the cushion back in kind, no
    /// swaps, no fee. Never fails; anything that cannot reach the owner stays with the pool.
    pub(crate) fn unwind(owner: &T::AccountId, netuid: NetUid, side: Side) {
        let Some(position) = Positions::<T>::take(owner, (netuid, side)) else {
            OpenByNetuid::<T>::remove(netuid, (owner.clone(), side));
            return;
        };
        Self::drop_indexes(owner, netuid, side, &position);

        let pallet_account = Self::pallet_account();
        let pallet_hotkey = T::PalletHotkey::get();
        let (mut tao_to_pool, mut alpha_to_pool) = match position.legs {
            Legs::Short {
                proceeds, escrow, ..
            } => (proceeds.saturating_add(escrow), AlphaBalance::ZERO),
            Legs::Long {
                proceeds, escrow, ..
            } => (TaoBalance::ZERO, proceeds.saturating_add(escrow)),
        };

        let alpha_cushion = position.cushion.alpha_part();
        let (alpha_to_owner, cushion_sold, alpha_unpaid) = Self::pay_owner_alpha(
            &pallet_account,
            &pallet_hotkey,
            owner,
            position.cushion.alpha_hotkey(),
            netuid,
            alpha_cushion,
        )
        .split();
        alpha_to_pool = alpha_to_pool.saturating_add(alpha_unpaid);

        let tao_cushion = position.cushion.tao_part().saturating_add(cushion_sold);
        let tao_to_owner = Self::pay_owner_tao(&pallet_account, owner, tao_cushion);
        tao_to_pool = tao_to_pool.saturating_add(tao_cushion.saturating_sub(tao_to_owner));

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

    /// Hand cushion alpha back in kind. If the owner's hotkey is gone (swapped or deregistered
    /// since open), sell the alpha instead so it can reach the owner as TAO.
    fn pay_owner_alpha(
        from_coldkey: &T::AccountId,
        from_hotkey: &T::AccountId,
        owner: &T::AccountId,
        owner_hotkey: Option<&T::AccountId>,
        netuid: NetUid,
        amount: AlphaBalance,
    ) -> AlphaPayout {
        if amount.is_zero() {
            return AlphaPayout::InKind(AlphaBalance::ZERO);
        }
        if let Some(hotkey) = owner_hotkey
            && with_storage_layer(|| {
                T::Pool::transfer_stake_internal(
                    from_coldkey,
                    from_hotkey,
                    owner,
                    hotkey,
                    netuid,
                    amount,
                )
            })
            .is_ok()
        {
            return AlphaPayout::InKind(amount);
        }
        match with_storage_layer(|| {
            T::Pool::sell_alpha_internal(from_coldkey, from_hotkey, netuid, amount)
        }) {
            Ok(tao) => AlphaPayout::Sold(tao),
            Err(_) => AlphaPayout::Unpaid(amount),
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
        position: &Position<T::AccountId, BlockNumberFor<T>>,
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
