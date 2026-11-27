//! Position operations (DESIGN-v2 sections 4 and 5; SPEC 1.1-1.4, P2-P23, L2-L12): open,
//! increase, close, add collateral and withdraw collateral, on either side.
//!
//! One path serves each operation. The side enters through the position's [`Leg`]: the swap
//! leg, the close's proceeds, the custody flows and the coverage need are chosen by matching on
//! it. Every gate and quote runs before the first leg moves funds or reserves, so a refused
//! operation leaves nothing behind.

use frame_support::{dispatch::DispatchResult, ensure, traits::Get};
use frame_system::pallet_prelude::BlockNumberFor;
use sp_runtime::{DispatchError, traits::AccountIdConversion};
use subtensor_runtime_common::NetUid;

use crate::{
    BlockNetLifted, BlockNetMinted, Config, DeregParamsStore, Enabled, Error, Event,
    LendingParamsStore, LendingPoolInterface, LongLiftedTao, LongLockedAlpha, LongParamsStore,
    LongPositionCount, LongsEnabled, Pallet, Positions, ShortParamsStore, SyntheticOutstanding,
    math::{
        PriceRefs,
        coverage::{long_need, short_need},
        curve::{ConstantProduct as Cp, Curve, PoolState},
        fixed::BPS,
        interest::interest_for_blocks,
    },
    params::{DeregParams, LendingParams, LongParams, RAO, ShortParams},
    types::{Leg, Market, Position, Side},
};

/// The alpha supply cap (SPEC P23, I21): 21,000,000 alpha.
pub const ALPHA_SUPPLY_CAP: u64 = 21_000_000 * RAO;

/// The live parameter groups an operation reads, read once.
pub struct Params {
    pub lending: LendingParams,
    pub short: ShortParams,
    pub long: LongParams,
    pub dereg: DeregParams,
}

/// A request to open or increase one side of a position. `size` is `q` for a short and the
/// lifted TAO `L` for a long; `min_out` bounds the value the pool gives (`P` or `A`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grow {
    pub netuid: NetUid,
    pub side: Side,
    pub size: u64,
    pub collateral: u64,
    pub min_out: u64,
    pub opening: bool,
}

/// What closing `alpha` of one side does, quoted before any leg moves.
struct Close {
    /// The pool after the swap, before the repayment and the interest charge are credited.
    after: PoolState,
    /// The TAO the swap moved: the buy-back cost (short) or the sale (long).
    value: u64,
    /// The leg's book retired: the proceeds used (short) or the debt repaid (long).
    consumed: u64,
    /// Collateral that pays the close: the buy-back beyond the proceeds, or a long's debt beyond
    /// its sale on a full close (SPEC 7.1.4).
    from_collateral: u64,
    /// TAO a long credits back to the pool from custody: the repaid debt.
    repaid: u64,
    /// TAO a long's sale leaves for the holder.
    surplus: u64,
}

/// `total * BPS <= bps * of`: `total` is within `bps` of `of` (SPEC P8, L5, L8).
fn within_share(total: u128, of: u64, bps: u32) -> bool {
    total.saturating_mul(BPS) <= u128::from(bps).saturating_mul(u128::from(of))
}

/// What a position holds against its coverage need: its collateral, plus the proceeds a short
/// holds (SPEC P2). A long holds collateral only (SPEC L2).
fn held_against(collateral: u64, leg: &Leg) -> u64 {
    collateral.saturating_add(leg.proceeds())
}

/// Whether `held` covers `need` plus `buffer`. An infinite need (`None`) never does.
fn covers(need: Option<u64>, held: u64, buffer: u64) -> bool {
    need.is_some_and(|need| held >= need.saturating_add(buffer))
}

/// The pool a position's coverage is measured on: the pool after the operation, with the
/// interest charge the operation moves into it and any debt repaid to it (SPEC 1.1).
fn post_pool(pool: PoolState, credit: u64) -> PoolState {
    PoolState {
        tao: pool.tao.saturating_add(credit),
        alpha: pool.alpha,
    }
}

/// The reference prices with spot read from the pool after the operation (SPEC 1.1).
fn priced_at(refs: PriceRefs, pool: &PoolState) -> PriceRefs {
    PriceRefs {
        spot: Cp::spot(pool),
        ..refs
    }
}

/// The coverage need of one position on `book` (SPEC 1.1, 7.1.3): a short's pro-rata buy-back
/// of the whole short book from the pumped pool, or a long's debt less its share of selling the
/// whole long book into the dumped pool. `None` is an infinite buy-back: the position is
/// uncovered.
pub(crate) fn coverage_need(
    leg: &Leg,
    book: u64,
    pool: &PoolState,
    refs: &PriceRefs,
    params: &Params,
) -> Option<u64> {
    match *leg {
        Leg::Short { minted, .. } => short_need::<Cp>(
            pool,
            refs,
            minted,
            book,
            params.short.stress_bps,
            params.short.stress_slope_bps,
        ),
        Leg::Long { locked_alpha, debt } => Some(long_need::<Cp>(
            pool,
            refs,
            debt,
            locked_alpha,
            book,
            params.long.stress_bps,
        )),
    }
}

/// The error a coverage failure reports on this side.
fn coverage_error<T: Config>(leg: &Leg) -> Error<T> {
    match leg {
        Leg::Short { .. } => Error::<T>::CoverageNotMet,
        Leg::Long { .. } => Error::<T>::LongCoverageNotMet,
    }
}

/// The net of a per-block counter (SPEC P8, L8), read as zero in any block but its own.
fn block_net<B: PartialEq + Copy>(stored: Option<(B, i128)>, now: B) -> i128 {
    stored
        .filter(|(block, _)| *block == now)
        .map_or(0, |(_, net)| net)
}

/// The per-block counter after adding `delta` in block `now`.
fn bump_block_net<B: PartialEq + Copy>(
    stored: Option<(B, i128)>,
    now: B,
    delta: i128,
) -> (B, i128) {
    (now, block_net(stored, now).saturating_add(delta))
}

/// Quotes a grow: the pool after it and the value the pool gives. A short sells `size` alpha
/// into the pool for `P`; a long lifts `size` TAO out of the pool and buys `A` with it (SPEC 7.1.1).
fn quote_grow<T: Config>(
    side: Side,
    pool: &PoolState,
    size: u64,
) -> Result<(PoolState, u64), DispatchError> {
    let quoted = match side {
        Side::Short => Cp::after_sell(pool, size),
        Side::Long => {
            let lifted = PoolState {
                tao: pool
                    .tao
                    .checked_sub(size)
                    .ok_or(Error::<T>::LiftCapExceeded)?,
                alpha: pool.alpha,
            };
            Cp::after_buy_tao(&lifted, size)
        }
    };
    quoted
        .ok_or(Error::<T>::Overflow)
        .map_err(DispatchError::from)
}

/// Quotes a close of `alpha` from a leg with `collateral` settled. `bound` is the caller's limit:
/// the most a short may pay for its buy-back, or the least a long may take for its sale.
fn quote_close<T: Config>(
    leg: &Leg,
    pool: &PoolState,
    alpha: u64,
    collateral: u64,
    bound: u64,
) -> Result<Close, DispatchError> {
    match *leg {
        Leg::Short { proceeds, .. } => {
            let (after, cost) =
                Cp::after_buy_alpha(pool, alpha).ok_or(Error::<T>::SlippageExceeded)?;
            ensure!(cost <= bound, Error::<T>::SlippageExceeded);
            let consumed = cost.min(proceeds);
            let from_collateral = cost.saturating_sub(consumed);
            ensure!(from_collateral <= collateral, Error::<T>::InsufficientFunds);
            Ok(Close {
                after,
                value: cost,
                consumed,
                from_collateral,
                repaid: 0,
                surplus: 0,
            })
        }
        Leg::Long { locked_alpha, debt } => {
            let (after, sale) = Cp::after_sell(pool, alpha).ok_or(Error::<T>::SlippageExceeded)?;
            ensure!(sale >= bound, Error::<T>::SlippageExceeded);
            let repaid = sale.min(debt);
            let from_collateral = if alpha == locked_alpha {
                debt.saturating_sub(repaid)
            } else {
                0
            };
            ensure!(from_collateral <= collateral, Error::<T>::CloseInsolvent);
            let consumed = repaid.saturating_add(from_collateral);
            Ok(Close {
                after,
                value: sale,
                consumed,
                from_collateral,
                repaid: consumed,
                surplus: sale.saturating_sub(repaid),
            })
        }
    }
}

impl<T: Config> Pallet<T> {
    /// The live parameter groups, read once per operation.
    pub(crate) fn params() -> Params {
        Params {
            lending: LendingParamsStore::<T>::get(),
            short: ShortParamsStore::<T>::get(),
            long: LongParamsStore::<T>::get(),
            dereg: DeregParamsStore::<T>::get(),
        }
    }

    /// One subnet's state for an operation: pool reserves, price references and books.
    pub(crate) fn market(netuid: NetUid) -> Result<Market, Error<T>> {
        let refs = Self::price_refs(netuid)?;
        Ok(Market {
            pool: PoolState {
                tao: T::Pool::pool_tao(netuid),
                alpha: T::Pool::subnet_alpha_in(netuid),
            },
            refs,
            short_book: SyntheticOutstanding::<T>::get(netuid),
            long_book: LongLockedAlpha::<T>::get(netuid),
            long_debt: LongLiftedTao::<T>::get(netuid),
        })
    }

    /// The custody account of one position. It is seeded with the opening block, so a position
    /// opened later gets a fresh account; a position closed in full leaves its account empty.
    pub(crate) fn custody_of(
        side: Side,
        who: &T::AccountId,
        netuid: NetUid,
        opened_at: BlockNumberFor<T>,
    ) -> T::AccountId {
        T::PalletId::get().into_sub_account_truncating((side, who, netuid, opened_at))
    }

    fn ensure_side_enabled(side: Side) -> DispatchResult {
        match side {
            Side::Short => {
                ensure!(Enabled::<T>::get(), Error::<T>::Disabled);
            }
            Side::Long => {
                ensure!(LongsEnabled::<T>::get(), Error::<T>::LongsDisabled);
            }
        }
        Ok(())
    }

    /// Flow gates, checked before any quote: the supply cap and the net-minted limit (SPEC P23,
    /// P8) on shorts; the longs cap, the lift cap and the net-lifted limit (SPEC 7.4, L5, L8) on
    /// longs.
    fn ensure_flow(
        side: Side,
        netuid: NetUid,
        size: u64,
        opening: bool,
        market: &Market,
        params: &Params,
        now: BlockNumberFor<T>,
    ) -> DispatchResult {
        match side {
            Side::Short => {
                let issued = T::Pool::alpha_issuance(netuid);
                ensure!(
                    issued
                        .checked_add(size)
                        .is_some_and(|total| total <= ALPHA_SUPPLY_CAP),
                    Error::<T>::AlphaSupplyCap
                );
                let net = block_net(BlockNetMinted::<T>::get(netuid), now)
                    .saturating_add(i128::from(size));
                ensure!(
                    within_share(
                        u128::try_from(net).unwrap_or(0),
                        market.pool.alpha,
                        params.short.open_limit_bps
                    ),
                    Error::<T>::OpenImpactTooHigh
                );
            }
            Side::Long => {
                if opening {
                    ensure!(
                        LongPositionCount::<T>::get(netuid)
                            < params.dereg.max_long_positions_per_subnet,
                        Error::<T>::TooManyLongPositions
                    );
                }
                let lifted = u128::from(market.long_debt).saturating_add(u128::from(size));
                ensure!(
                    within_share(lifted, market.pool.tao, params.long.lift_cap_bps),
                    Error::<T>::LiftCapExceeded
                );
                let net = block_net(BlockNetLifted::<T>::get(netuid), now)
                    .saturating_add(i128::from(size));
                ensure!(
                    within_share(
                        u128::try_from(net).unwrap_or(0),
                        market.pool.tao,
                        params.long.open_limit_bps
                    ),
                    Error::<T>::LongOpenLimit
                );
            }
        }
        Ok(())
    }

    /// Moves an interest charge from a position's custody into the subnet pool (SPEC P15, I19).
    /// Pool-internal: it is not flow or volume.
    fn pay_interest(
        who: &T::AccountId,
        netuid: NetUid,
        side: Side,
        custody: &T::AccountId,
        amount: u64,
    ) -> DispatchResult {
        if amount > 0 {
            T::Pool::credit_pool_tao(netuid, custody, amount)?;
            Self::deposit_event(Event::InterestCharged {
                who: who.clone(),
                netuid,
                side,
                amount,
            });
        }
        Ok(())
    }

    /// Opens or increases one side of a position (SPEC P1-P23, L1-L12). The collateral and
    /// deposit move to custody, the pool gives the leg's value, and the books move with it.
    pub(crate) fn do_grow(who: &T::AccountId, grow: Grow) -> DispatchResult {
        let Grow {
            netuid,
            side,
            size,
            collateral,
            min_out,
            opening,
        } = grow;
        Self::ensure_side_enabled(side)?;
        let key = (who.clone(), netuid, side);
        let now = frame_system::Pallet::<T>::block_number();
        let params = Self::params();
        let held = if opening {
            ensure!(
                !Positions::<T>::contains_key(&key),
                Error::<T>::PositionExists
            );
            ensure!(
                !Positions::<T>::contains_key((who.clone(), netuid, side.other())),
                Error::<T>::OppositeSideOpen
            );
            Position::fresh(side, now)
        } else {
            Positions::<T>::get(&key).ok_or(Error::<T>::NoPosition)?
        };
        ensure!(size > 0, Error::<T>::InvalidAmount);
        let market = Self::market(netuid)?;
        ensure!(
            market.pool.tao >= params.lending.min_pool_tao,
            Error::<T>::PoolTooShallow
        );
        Self::ensure_flow(side, netuid, size, opening, &market, &params, now)?;

        let (pos, charge) = held.settle(market.refs.twap, params.lending.apr_bps, now);
        let (after, value) = quote_grow::<T>(side, &market.pool, size)?;
        ensure!(value > 0, Error::<T>::InvalidAmount);
        ensure!(value >= min_out, Error::<T>::SlippageExceeded);
        ensure!(
            after.tao >= params.lending.min_pool_tao,
            Error::<T>::PoolTooShallow
        );
        let leg = pos.leg.grown(size, value);
        ensure!(
            leg.notional(market.refs.twap) >= params.lending.min_notional,
            Error::<T>::BelowMinNotional
        );

        let deposit = if opening { params.lending.deposit } else { 0 };
        let funded = collateral
            .checked_add(deposit)
            .ok_or(Error::<T>::Overflow)?;
        ensure!(
            T::Pool::tao_balance(who) >= funded,
            Error::<T>::InsufficientBalance
        );
        let collateral_total = pos
            .collateral
            .checked_add(collateral)
            .ok_or(Error::<T>::Overflow)?;
        let deposit_total = pos
            .deposit
            .checked_add(deposit)
            .ok_or(Error::<T>::Overflow)?;

        // The alpha this grow adds to the book: `q` for a short, `A` for a long.
        let added = leg.alpha().saturating_sub(pos.leg.alpha());
        let book_after = market
            .book(side)
            .checked_add(added)
            .ok_or(Error::<T>::Overflow)?;
        let post = post_pool(after, charge);
        let need = coverage_need(
            &leg,
            book_after,
            &post,
            &priced_at(market.refs, &post),
            &params,
        );
        ensure!(
            covers(need, held_against(collateral_total, &leg), 0),
            coverage_error::<T>(&leg)
        );

        // Commit: custody funded, the leg moves the pool, then the charge is paid.
        let custody = Self::custody_of(side, who, netuid, pos.opened_at);
        T::Pool::transfer_tao(who, &custody, funded)?;
        let moved = match side {
            Side::Short => T::Pool::mint_and_sell(netuid, size, &custody)?,
            Side::Long => T::Pool::lift_and_buy(netuid, size, &custody)?,
        };
        ensure!(moved == value, Error::<T>::QuoteMismatch);
        Self::pay_interest(who, netuid, side, &custody, charge)?;
        match side {
            Side::Short => {
                SyntheticOutstanding::<T>::insert(netuid, book_after);
                BlockNetMinted::<T>::insert(
                    netuid,
                    bump_block_net(BlockNetMinted::<T>::get(netuid), now, i128::from(size)),
                );
            }
            Side::Long => {
                LongLockedAlpha::<T>::insert(netuid, book_after);
                LongLiftedTao::<T>::insert(netuid, market.long_debt.saturating_add(size));
                BlockNetLifted::<T>::insert(
                    netuid,
                    bump_block_net(BlockNetLifted::<T>::get(netuid), now, i128::from(size)),
                );
                if opening {
                    LongPositionCount::<T>::insert(
                        netuid,
                        LongPositionCount::<T>::get(netuid).saturating_add(1),
                    );
                }
            }
        }
        Positions::<T>::insert(
            &key,
            Position {
                collateral: collateral_total,
                deposit: deposit_total,
                leg,
                ..pos
            },
        );
        if opening {
            Self::deposit_event(Event::PositionOpened {
                who: who.clone(),
                netuid,
                side,
                size,
                value,
                collateral,
            });
        } else {
            Self::deposit_event(Event::PositionIncreased {
                who: who.clone(),
                netuid,
                side,
                size,
                value,
                collateral,
            });
        }
        Ok(())
    }

    /// Closes up to `amount` of one side (SPEC 1.4, 7.1.4). A full close refunds the custody's
    /// remainder, including the deposit, and removes the position.
    pub(crate) fn do_close(
        who: &T::AccountId,
        netuid: NetUid,
        side: Side,
        amount: u64,
        bound: u64,
    ) -> DispatchResult {
        let key = (who.clone(), netuid, side);
        let held = Positions::<T>::get(&key).ok_or(Error::<T>::NoPosition)?;
        let alpha = amount.min(held.leg.alpha());
        ensure!(alpha > 0, Error::<T>::InvalidAmount);
        let now = frame_system::Pallet::<T>::block_number();
        let params = Self::params();
        let market = Self::market(netuid)?;
        let (pos, charge) = held.settle(market.refs.twap, params.lending.apr_bps, now);
        let full = alpha == pos.leg.alpha();
        let quote = quote_close::<T>(&pos.leg, &market.pool, alpha, pos.collateral, bound)?;
        let leg = pos.leg.shrunk(alpha, quote.consumed);
        let collateral = pos.collateral.saturating_sub(quote.from_collateral);
        let book = market
            .book(side)
            .checked_sub(alpha)
            .ok_or(Error::<T>::Overflow)?;
        if !full {
            let post = post_pool(quote.after, charge.saturating_add(quote.repaid));
            let need = coverage_need(&leg, book, &post, &priced_at(market.refs, &post), &params);
            ensure!(
                covers(need, held_against(collateral, &leg), 0),
                coverage_error::<T>(&leg)
            );
        }

        // Commit: the leg moves alpha and TAO, then the repayment, the charge and the books.
        let custody = Self::custody_of(side, who, netuid, pos.opened_at);
        let paid = match side {
            Side::Short => T::Pool::buy_and_burn(netuid, &custody, alpha)?,
            Side::Long => T::Pool::sell_alpha(netuid, &custody, alpha)?,
        };
        ensure!(paid == quote.value, Error::<T>::QuoteMismatch);
        if quote.repaid > 0 {
            T::Pool::credit_pool_tao(netuid, &custody, quote.repaid)?;
        }
        Self::pay_interest(who, netuid, side, &custody, charge)?;
        match side {
            Side::Short => {
                SyntheticOutstanding::<T>::insert(netuid, book);
                BlockNetMinted::<T>::insert(
                    netuid,
                    bump_block_net(
                        BlockNetMinted::<T>::get(netuid),
                        now,
                        i128::from(alpha).saturating_neg(),
                    ),
                );
            }
            Side::Long => {
                LongLockedAlpha::<T>::insert(netuid, book);
                LongLiftedTao::<T>::insert(netuid, market.long_debt.saturating_sub(quote.consumed));
                BlockNetLifted::<T>::insert(
                    netuid,
                    bump_block_net(
                        BlockNetLifted::<T>::get(netuid),
                        now,
                        i128::from(quote.consumed).saturating_neg(),
                    ),
                );
                if full {
                    LongPositionCount::<T>::insert(
                        netuid,
                        LongPositionCount::<T>::get(netuid).saturating_sub(1),
                    );
                }
            }
        }

        // The holder receives a long's surplus; a full close also refunds the remainder.
        let mut payout = quote.surplus;
        if full {
            let proceeds = leg.proceeds();
            let refund_c = pos.accrual.due.min(collateral);
            let refund_p = pos.accrual.due.saturating_sub(refund_c).min(proceeds);
            let refunded = refund_c.saturating_add(refund_p);
            Self::pay_interest(who, netuid, side, &custody, refunded)?;
            payout = payout
                .saturating_add(collateral.saturating_sub(refund_c))
                .saturating_add(proceeds.saturating_sub(refund_p))
                .saturating_add(pos.deposit);
            Positions::<T>::remove(&key);
        } else {
            Positions::<T>::insert(
                &key,
                Position {
                    collateral,
                    leg,
                    ..pos
                },
            );
        }
        if payout > 0 {
            T::Pool::transfer_tao(&custody, who, payout)?;
        }
        Self::deposit_event(Event::PositionClosed {
            who: who.clone(),
            netuid,
            side,
            alpha,
            value: quote.value,
            full,
        });
        Ok(())
    }

    /// Adds collateral to one side of a position (SPEC P19, L11: the collateral is the holder's
    /// to add at any time).
    pub(crate) fn do_add_collateral(
        who: &T::AccountId,
        netuid: NetUid,
        side: Side,
        amount: u64,
    ) -> DispatchResult {
        let key = (who.clone(), netuid, side);
        let held = Positions::<T>::get(&key).ok_or(Error::<T>::NoPosition)?;
        ensure!(amount > 0, Error::<T>::InvalidAmount);
        let now = frame_system::Pallet::<T>::block_number();
        let params = Self::params();
        let market = Self::market(netuid)?;
        let (pos, charge) = held.settle(market.refs.twap, params.lending.apr_bps, now);
        ensure!(
            T::Pool::tao_balance(who) >= amount,
            Error::<T>::InsufficientBalance
        );
        let collateral = pos
            .collateral
            .checked_add(amount)
            .ok_or(Error::<T>::Overflow)?;
        let custody = Self::custody_of(side, who, netuid, pos.opened_at);
        T::Pool::transfer_tao(who, &custody, amount)?;
        Self::pay_interest(who, netuid, side, &custody, charge)?;
        Positions::<T>::insert(&key, Position { collateral, ..pos });
        Self::deposit_event(Event::CollateralAdded {
            who: who.clone(),
            netuid,
            side,
            amount,
        });
        Ok(())
    }

    /// Withdraws collateral from one side of a position. The collateral left must still cover
    /// the need (SPEC P2, L2) and leave the interest buffer on top of it (SPEC P20, L12).
    pub(crate) fn do_withdraw_collateral(
        who: &T::AccountId,
        netuid: NetUid,
        side: Side,
        amount: u64,
    ) -> DispatchResult {
        let key = (who.clone(), netuid, side);
        let held = Positions::<T>::get(&key).ok_or(Error::<T>::NoPosition)?;
        ensure!(amount > 0, Error::<T>::InvalidAmount);
        let now = frame_system::Pallet::<T>::block_number();
        let params = Self::params();
        let market = Self::market(netuid)?;
        let (pos, charge) = held.settle(market.refs.twap, params.lending.apr_bps, now);
        ensure!(amount <= pos.collateral, Error::<T>::WithdrawBreaksCoverage);
        let collateral = pos.collateral.saturating_sub(amount);
        let post = post_pool(market.pool, charge);
        let need = coverage_need(
            &pos.leg,
            market.book(side),
            &post,
            &priced_at(market.refs, &post),
            &params,
        );
        let held_after = held_against(collateral, &pos.leg);
        ensure!(
            covers(need, held_after, 0),
            Error::<T>::WithdrawBreaksCoverage
        );
        let buffer = interest_for_blocks(
            pos.leg.notional(market.refs.twap),
            params.lending.apr_bps,
            params.lending.withdraw_buffer_blocks,
        );
        ensure!(
            covers(need, held_after, buffer),
            Error::<T>::WithdrawBufferBreached
        );
        let custody = Self::custody_of(side, who, netuid, pos.opened_at);
        Self::pay_interest(who, netuid, side, &custody, charge)?;
        T::Pool::transfer_tao(&custody, who, amount)?;
        Positions::<T>::insert(&key, Position { collateral, ..pos });
        Self::deposit_event(Event::CollateralWithdrawn {
            who: who.clone(),
            netuid,
            side,
            amount,
        });
        Ok(())
    }
}
