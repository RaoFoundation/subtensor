//! Position operations (DESIGN-v2 sections 4 and 5; SPEC 1.1-1.4, P2-P23, L2-L12): open,
//! increase, close and add collateral, on either side. Collateral leaves a position only on close.
//! A short opens and grows only on a subnet in the emission set (`ensure_in_emission_set`); a
//! partial close of a short passes on coverage or on de-risking (`partial_close_allowed`).
//!
//! Every operation charges interest on touch: `Position::settle` up front for the amounts and
//! `Pallet::pay_interest` at commit, the same pair the `Due` queue runs (see `interest`).
//!
//! One path serves each operation. The side enters through the position's [`Leg`]: the swap
//! leg, the close's proceeds, the custody flows and the coverage need are chosen by matching on
//! it. Every gate and quote runs before the first leg moves funds or reserves, so a refused
//! operation leaves nothing behind.

use frame_support::{dispatch::DispatchResult, ensure, traits::Get};
use frame_system::pallet_prelude::BlockNumberFor;
use sp_runtime::{
    DispatchError,
    traits::{AccountIdConversion, Hash},
};
use subtensor_runtime_common::NetUid;

use crate::{
    BlockNetLifted, BlockNetMinted, Config, Custody, DeregParamsStore, Enabled, Error, Event,
    HolderPositions, LendingParamsStore, LendingPoolInterface, LongLiftedTao, LongLockedAlpha,
    LongParamsStore, LongPositionCount, LongsEnabled, Pallet, PositionCount, Positions,
    ShortParamsStore, SwappedOutAt, SyntheticOutstanding, TotalPositions,
    math::{
        PriceRefs,
        coverage::{long_need, short_need},
        curve::{Curve, PoolState},
        fixed::BPS,
        rate::short_close_derisks,
    },
    params::{DeregParams, LendingParams, LongParams, RAO, ShortParams},
    types::{Leg, Market, Position, PositionId, Side},
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
pub(crate) struct Close {
    /// The pool after the swap, before the repayment and the interest charge are credited.
    pub(crate) after: PoolState,
    /// The TAO the swap moved: the buy-back cost (short) or the sale (long).
    pub(crate) value: u64,
    /// The leg's book retired: the proceeds used (short) or the debt repaid (long).
    pub(crate) consumed: u64,
    /// Collateral that pays the close: the buy-back beyond the proceeds, or a long's debt beyond
    /// its sale on a full close (SPEC 7.1.4).
    pub(crate) from_collateral: u64,
    /// TAO a long credits back to the pool from custody: the repaid debt.
    pub(crate) repaid: u64,
    /// TAO a long's sale leaves for the holder.
    pub(crate) surplus: u64,
}

/// `total * BPS <= bps * of`: `total` is within `bps` of `of` (SPEC P8, L5, L8).
fn within_share(total: u128, of: u64, bps: u32) -> bool {
    total.saturating_mul(BPS) <= u128::from(bps).saturating_mul(u128::from(of))
}

/// What a position holds against its coverage need: its collateral, plus the proceeds a short
/// holds (SPEC P2). A long holds collateral only (SPEC L2).
pub(crate) fn held_against(collateral: u64, leg: &Leg) -> u64 {
    collateral.saturating_add(leg.proceeds())
}

/// Whether `held` covers `need`. An infinite need (`None`) never does.
pub(crate) fn covers(need: Option<u64>, held: u64) -> bool {
    need.is_some_and(|need| held >= need)
}

/// Whether a partial close may leave `leg` with `collateral` (SPEC 1.4), given the coverage
/// `need` of what is left. `before` is the position with its interest settled and nothing of
/// the close applied. The close is allowed if what is left is covered. A short's is also
/// allowed if it de-risks: its `u` does not rise, `q2 * (C1 + P1) <= q1 * (C2 + P2)` at the same
/// slow price (`math::rate::short_close_derisks`), so an underwater short can always shrink
/// without a top-up. A long has the coverage rule only.
pub(crate) fn partial_close_allowed<B>(
    need: Option<u64>,
    before: &Position<B>,
    collateral: u64,
    leg: &Leg,
) -> bool {
    if covers(need, held_against(collateral, leg)) {
        return true;
    }
    match (before.leg, *leg) {
        (
            Leg::Short {
                minted: q_before,
                proceeds: p_before,
            },
            Leg::Short {
                minted: q_after,
                proceeds: p_after,
            },
        ) => short_close_derisks(
            q_before,
            u128::from(before.collateral).saturating_add(u128::from(p_before)),
            q_after,
            u128::from(collateral).saturating_add(u128::from(p_after)),
        ),
        _ => false,
    }
}

/// The pool a position's coverage is measured on: the pool after the operation, with the
/// interest charge the operation moves into it and any debt repaid to it (SPEC 1.1).
pub(crate) fn post_pool(pool: PoolState, credit: u64) -> PoolState {
    pool.with_reserves(pool.tao.saturating_add(credit), pool.alpha)
}

/// The reference prices with spot read from the pool after the operation (SPEC 1.1).
pub(crate) fn priced_at<C: Curve>(refs: PriceRefs, pool: &PoolState) -> PriceRefs {
    PriceRefs {
        spot: C::spot(pool),
        ..refs
    }
}

/// The coverage need of one position on `book` (SPEC 1.1, 7.1.3): a short's pro-rata buy-back
/// of the whole short book from the pumped pool, or a long's debt less its share of selling the
/// whole long book into the dumped pool. `None` is an infinite buy-back: the position is
/// uncovered.
pub(crate) fn coverage_need<C: Curve>(
    leg: &Leg,
    book: u64,
    pool: &PoolState,
    refs: &PriceRefs,
    params: &Params,
) -> Option<u64> {
    match *leg {
        Leg::Short { minted, .. } => {
            short_need::<C>(pool, refs, minted, book, params.short.stress_bps)
        }
        Leg::Long { locked_alpha, debt } => Some(long_need::<C>(
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
pub(crate) fn quote_grow<T: Config>(
    side: Side,
    pool: &PoolState,
    size: u64,
) -> Result<(PoolState, u64), DispatchError> {
    let quoted = match side {
        Side::Short => T::Curve::after_sell(pool, size),
        Side::Long => {
            let lifted = pool.with_reserves(
                pool.tao
                    .checked_sub(size)
                    .ok_or(Error::<T>::LiftCapExceeded)?,
                pool.alpha,
            );
            T::Curve::after_buy_tao(&lifted, size)
        }
    };
    quoted
        .ok_or(Error::<T>::Overflow)
        .map_err(DispatchError::from)
}

/// Quotes a close of `alpha` from a leg with `collateral` settled. `bound` is the caller's limit:
/// the most a short may pay for its buy-back, or the least a long may take for its sale.
pub(crate) fn quote_close<T: Config>(
    leg: &Leg,
    pool: &PoolState,
    alpha: u64,
    collateral: u64,
    bound: u64,
) -> Result<Close, DispatchError> {
    match *leg {
        Leg::Short { proceeds, .. } => {
            let (after, cost) =
                T::Curve::after_buy_alpha(pool, alpha).ok_or(Error::<T>::SlippageExceeded)?;
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
            let (after, sale) =
                T::Curve::after_sell(pool, alpha).ok_or(Error::<T>::SlippageExceeded)?;
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

    /// One subnet's state for an operation: the pool as the curve sees it, price references and
    /// books.
    pub(crate) fn market(netuid: NetUid) -> Result<Market, Error<T>> {
        let refs = Self::price_refs(netuid)?;
        Ok(Market {
            pool: PoolState {
                tao: T::Pool::pool_tao(netuid),
                alpha: T::Pool::subnet_alpha_in(netuid),
                quote_weight: T::Pool::quote_weight(netuid),
                fee: T::Pool::swap_fee(netuid),
            },
            refs,
            short_book: SyntheticOutstanding::<T>::get(netuid),
            long_book: LongLockedAlpha::<T>::get(netuid),
            long_debt: LongLiftedTao::<T>::get(netuid),
        })
    }

    /// The custody account of one position. It is seeded with the opening block, so a position
    /// opened later gets a fresh account; a position closed in full leaves its account empty.
    /// The seed is hashed first: the sub-account keeps only the leading bytes of its seed, and
    /// the hash puts the side, holder, subnet and block in all of them.
    pub(crate) fn custody_of(
        side: Side,
        who: &T::AccountId,
        netuid: NetUid,
        opened_at: BlockNumberFor<T>,
    ) -> T::AccountId {
        let seed = T::Hashing::hash_of(&(side, who, netuid, opened_at));
        T::PalletId::get().into_sub_account_truncating(seed)
    }

    /// Moves `amount` TAO between two accounts and refuses a pool that moved any other amount.
    pub(crate) fn move_tao(from: &T::AccountId, to: &T::AccountId, amount: u64) -> DispatchResult {
        let moved = T::Pool::transfer_tao(from, to, amount)?;
        ensure!(moved == amount, Error::<T>::QuoteMismatch);
        Ok(())
    }

    /// Credits `amount` TAO from `from` to the pool's reserve and refuses a pool that credited
    /// any other amount.
    pub(crate) fn credit_pool(netuid: NetUid, from: &T::AccountId, amount: u64) -> DispatchResult {
        let moved = T::Pool::credit_pool_tao(netuid, from, amount)?;
        ensure!(moved == amount, Error::<T>::QuoteMismatch);
        Ok(())
    }

    /// Removes every record of a position that is gone (a full close, a default): the
    /// position, its custody record, its `Due` entry at `charge_at` with the retry count, the
    /// counts and the holder's entry. Moves no funds and touches no book.
    pub(crate) fn forget_position(key: &PositionId<T::AccountId>, charge_at: BlockNumberFor<T>) {
        let (who, netuid, side) = key;
        Positions::<T>::remove(key);
        Custody::<T>::remove(key);
        Self::unschedule_charge(key, charge_at);
        PositionCount::<T>::mutate_exists(netuid, |count| {
            *count = count.unwrap_or(0).checked_sub(1).filter(|left| *left > 0);
        });
        TotalPositions::<T>::mutate(|count| *count = count.saturating_sub(1));
        HolderPositions::<T>::mutate_exists(who, |held| {
            if let Some(list) = held {
                list.retain(|entry| *entry != (*netuid, *side));
                if list.is_empty() {
                    *held = None;
                }
            }
        });
    }

    /// The emission-set gate (Jacob, Nov 27): a short opens and grows only on a subnet that is
    /// in the emission set now. This is the one check; `open_short`, `increase_short` and
    /// their quotes each run it once, and no other call does. The predicate is the runtime's
    /// (`LendingPoolInterface::is_in_emission_set`).
    pub(crate) fn ensure_in_emission_set(netuid: NetUid) -> DispatchResult {
        ensure!(
            T::Pool::is_in_emission_set(netuid),
            Error::<T>::SubnetNotInEmission
        );
        Ok(())
    }

    pub(crate) fn ensure_side_enabled(side: Side) -> DispatchResult {
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
    pub(crate) fn ensure_flow(
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
            // A coldkey holds a bounded number of positions, which bounds the re-key of a
            // coldkey swap (`hooks::rekey_positions`).
            ensure!(
                HolderPositions::<T>::decode_len(who).unwrap_or(0)
                    < crate::hooks::MAX_POSITIONS_PER_HOLDER as usize,
                Error::<T>::TooManyPositions
            );
            ensure!(
                SwappedOutAt::<T>::get(who) != Some(now),
                Error::<T>::SwappedOutThisBlock
            );
            let first_charge = Self::charge_block(now, params.lending.charge_interval_blocks);
            Position::fresh(side, now, first_charge)
        } else {
            Positions::<T>::get(&key).ok_or(Error::<T>::NoPosition)?
        };
        ensure!(size > 0, Error::<T>::InvalidAmount);
        let market = Self::market(netuid)?;
        ensure!(
            market.pool.tao >= params.lending.min_pool_tao,
            Error::<T>::PoolTooShallow
        );
        if side == Side::Short {
            Self::ensure_in_emission_set(netuid)?;
        }
        Self::ensure_flow(side, netuid, size, opening, &market, &params, now)?;

        let (pos, charge) = held.settle(&market.refs, &params.lending.rate_curve(), now);
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
        let post = post_pool(after, charge.amount);
        let need = coverage_need::<T::Curve>(
            &leg,
            book_after,
            &post,
            &priced_at::<T::Curve>(market.refs, &post),
            &params,
        );
        ensure!(
            covers(need, held_against(collateral_total, &leg)),
            coverage_error::<T>(&leg)
        );

        // Commit: custody funded, the leg moves the pool, then the charge is paid.
        let custody = Self::position_custody(&key, pos.opened_at);
        Self::move_tao(who, &custody, funded)?;
        let moved = match side {
            Side::Short => T::Pool::mint_and_sell(netuid, size, &custody)?,
            Side::Long => T::Pool::lift_and_buy(netuid, size, &custody)?,
        };
        ensure!(moved == value, Error::<T>::QuoteMismatch);
        Self::pay_interest(who, netuid, side, &custody, charge.amount, charge.rate_bps)?;
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
            // The first charge is one interval after the open (SPEC P15).
            Self::schedule_charge(&key, pos.charge_at);
            PositionCount::<T>::mutate(netuid, |count| *count = count.saturating_add(1));
            TotalPositions::<T>::mutate(|count| *count = count.saturating_add(1));
            HolderPositions::<T>::try_append(who, (netuid, side))
                .map_err(|()| Error::<T>::TooManyPositions)?;
            // A marker of an earlier block has done its work.
            if SwappedOutAt::<T>::contains_key(who) {
                SwappedOutAt::<T>::remove(who);
            }
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
        let (pos, charge) = held.settle(&market.refs, &params.lending.rate_curve(), now);
        let full = alpha == pos.leg.alpha();
        let quote = quote_close::<T>(&pos.leg, &market.pool, alpha, pos.collateral, bound)?;
        let leg = pos.leg.shrunk(alpha, quote.consumed);
        let collateral = pos.collateral.saturating_sub(quote.from_collateral);
        let book = market
            .book(side)
            .checked_sub(alpha)
            .ok_or(Error::<T>::Overflow)?;
        if !full {
            let post = post_pool(quote.after, charge.amount.saturating_add(quote.repaid));
            let need = coverage_need::<T::Curve>(
                &leg,
                book,
                &post,
                &priced_at::<T::Curve>(market.refs, &post),
                &params,
            );
            ensure!(
                partial_close_allowed(need, &pos, collateral, &leg),
                coverage_error::<T>(&leg)
            );
        }

        // Commit: the leg moves alpha and TAO, then the repayment, the charge and the books.
        let custody = Self::position_custody(&key, pos.opened_at);
        let paid = match side {
            Side::Short => T::Pool::buy_and_burn(netuid, &custody, alpha)?,
            Side::Long => T::Pool::sell_alpha(netuid, &custody, alpha)?,
        };
        ensure!(paid == quote.value, Error::<T>::QuoteMismatch);
        if quote.repaid > 0 {
            Self::credit_pool(netuid, &custody, quote.repaid)?;
        }
        Self::pay_interest(who, netuid, side, &custody, charge.amount, charge.rate_bps)?;
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
            Self::pay_interest(who, netuid, side, &custody, refunded, charge.rate_bps)?;
            payout = payout
                .saturating_add(collateral.saturating_sub(refund_c))
                .saturating_add(proceeds.saturating_sub(refund_p))
                .saturating_add(pos.deposit);
            Self::forget_position(&key, pos.charge_at);
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
            Self::move_tao(&custody, who, payout)?;
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
        let (pos, charge) = held.settle(&market.refs, &params.lending.rate_curve(), now);
        ensure!(
            T::Pool::tao_balance(who) >= amount,
            Error::<T>::InsufficientBalance
        );
        let collateral = pos
            .collateral
            .checked_add(amount)
            .ok_or(Error::<T>::Overflow)?;
        let custody = Self::position_custody(&key, pos.opened_at);
        Self::move_tao(who, &custody, amount)?;
        Self::pay_interest(who, netuid, side, &custody, charge.amount, charge.rate_bps)?;
        Positions::<T>::insert(&key, Position { collateral, ..pos });
        Self::deposit_event(Event::CollateralAdded {
            who: who.clone(),
            netuid,
            side,
            amount,
        });
        Ok(())
    }
}
