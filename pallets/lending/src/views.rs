//! Read-only views for the runtime API (DESIGN-v2 section 8): the parameters, a subnet's
//! market, positions, and quotes of the short operations.
//!
//! A quote runs the gates and the arithmetic of the call it quotes (`positions::do_grow`,
//! `positions::do_close`) up to the point where the call would move funds, on the state it is
//! asked on, and writes nothing. It reports the error the call would fail with for every gate
//! that does not depend on the caller's balance. Only shorts are quoted: longs are disabled
//! and their pool legs are pending spec.

use alloc::vec::Vec;
use codec::{Decode, Encode};
use frame_support::ensure;
use frame_system::pallet_prelude::BlockNumberFor;
use scale_info::TypeInfo;
use sp_runtime::{DispatchError, Perquintill};
use subtensor_macros::freeze_struct;
use subtensor_runtime_common::NetUid;

use crate::{
    Config, Enabled, Error, HolderPositions, LongsEnabled, Pallet, Positions,
    math::{
        curve::{Curve, FeeRate},
        fixed::Price,
    },
    params::{DeregParams, LendingParams, LongParams, ShortParams},
    positions::{
        coverage_need, covers, held_against, post_pool, priced_at, quote_close, quote_grow,
    },
    types::{Position, Side},
};

/// The two switches and the four live parameter groups (SPEC 2.1).
#[freeze_struct("56d71713a46dbe6")]
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode, TypeInfo)]
pub struct ParamsView {
    /// Whether shorts may be opened or increased (SPEC P1).
    pub enabled: bool,
    /// Whether longs may be opened or increased (SPEC L1).
    pub longs_enabled: bool,
    pub lending: LendingParams,
    pub short: ShortParams,
    pub long: LongParams,
    pub dereg: DeregParams,
}

/// One subnet's lending market: the pool as the curve prices it, the books and the three
/// price references. Amounts are in rao; prices are rao TAO per rao alpha.
#[freeze_struct("a2ae438f1cdf8eb2")]
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode, TypeInfo)]
pub struct MarketView {
    /// The pool's TAO reserve (`SubnetTAO`).
    pub pool_tao: u64,
    /// The pool's alpha reserve (`SubnetAlphaIn`).
    pub pool_alpha: u64,
    /// The Balancer weight of the TAO reserve.
    pub quote_weight: Perquintill,
    /// The swap fee rate, in parts of `u16::MAX`.
    pub swap_fee: FeeRate,
    /// `SyntheticOutstanding`: alpha minted by shorts and not yet bought back.
    pub synthetic_outstanding: u64,
    /// `LongLockedAlpha`: alpha held by all longs.
    pub long_locked_alpha: u64,
    /// `LongLiftedTao`: TAO lifted from the pool by longs and not yet repaid.
    pub long_lifted_tao: u64,
    /// Live pool spot.
    pub spot: Price,
    /// Lower median of the TWAP ring (SPEC P6).
    pub twap: Price,
    /// SMA of the rank ring (SPEC P13).
    pub sma: Price,
}

/// One open position of a holder.
#[freeze_struct("d958c52dbb93caa4")]
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode, TypeInfo)]
pub struct HeldPosition<BlockNumber> {
    pub netuid: NetUid,
    pub side: Side,
    pub position: Position<BlockNumber>,
}

/// What `open_short` or `increase_short` of some `q` would do.
#[freeze_struct("f4f3da6d5aeea0d1")]
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode, TypeInfo)]
pub struct ShortGrowQuote {
    /// TAO the sale of `q` pays into custody (`P`): the call's `min_tao_out` bounds it.
    pub proceeds: u64,
    /// Interest the call settles from the position's collateral first; zero for an open.
    pub interest: u64,
    /// The refundable open deposit the call takes on top of the collateral; zero for an
    /// increase.
    pub deposit: u64,
    /// The coverage need of the grown position (SPEC 1.1), which its collateral and proceeds
    /// together must reach.
    pub coverage_need: u64,
    /// The least collateral the call must bring for the grown position to be covered.
    pub min_collateral: u64,
    /// Pool spot after the sale.
    pub spot_after: Price,
}

/// What `close_short` of some `alpha` would do.
#[freeze_struct("b1c65628ad20abaa")]
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode, TypeInfo)]
pub struct ShortCloseQuote {
    /// The alpha bought back: the request, capped at the position's size.
    pub alpha: u64,
    /// TAO the buy-back costs: the call's `max_tao_in` bounds it.
    pub cost: u64,
    /// The part of the cost paid from the position's proceeds.
    pub from_proceeds: u64,
    /// The part of the cost paid from the position's collateral.
    pub from_collateral: u64,
    /// Interest the call settles from the position's collateral first.
    pub interest: u64,
    /// Whether the close retires the whole position, which also refunds its custody.
    pub full: bool,
}

impl<T: Config> Pallet<T> {
    /// The switches and the live parameter groups.
    #[must_use]
    pub fn params_view() -> ParamsView {
        let params = Self::params();
        ParamsView {
            enabled: Enabled::<T>::get(),
            longs_enabled: LongsEnabled::<T>::get(),
            lending: params.lending,
            short: params.short,
            long: params.long,
            dereg: params.dereg,
        }
    }

    /// The market of `netuid`, or `None` for a subnet without price rings (not a live subnet).
    #[must_use]
    pub fn market_view(netuid: NetUid) -> Option<MarketView> {
        let market = Self::market(netuid).ok()?;
        Some(MarketView {
            pool_tao: market.pool.tao,
            pool_alpha: market.pool.alpha,
            quote_weight: market.pool.quote_weight,
            swap_fee: market.pool.fee,
            synthetic_outstanding: market.short_book,
            long_locked_alpha: market.long_book,
            long_lifted_tao: market.long_debt,
            spot: market.refs.spot,
            twap: market.refs.twap,
            sma: market.refs.sma,
        })
    }

    /// The position `who` holds on `side` of `netuid`, as stored: interest accrued since its
    /// last touch is not settled in it.
    #[must_use]
    pub fn position_view(
        who: &T::AccountId,
        netuid: NetUid,
        side: Side,
    ) -> Option<Position<BlockNumberFor<T>>> {
        Positions::<T>::get((who.clone(), netuid, side))
    }

    /// Every position `who` holds, at most `hooks::MAX_POSITIONS_PER_HOLDER`.
    #[must_use]
    pub fn positions_of(who: &T::AccountId) -> Vec<HeldPosition<BlockNumberFor<T>>> {
        HolderPositions::<T>::get(who)
            .into_iter()
            .filter_map(|(netuid, side)| {
                Positions::<T>::get((who.clone(), netuid, side)).map(|position| HeldPosition {
                    netuid,
                    side,
                    position,
                })
            })
            .collect()
    }

    /// Quotes `open_short(netuid, q, ..)`. The gates on the caller (an existing position, the
    /// position limit, the balance) are not part of it.
    pub fn quote_open_short(netuid: NetUid, q: u64) -> Result<ShortGrowQuote, DispatchError> {
        let now = frame_system::Pallet::<T>::block_number();
        let interval = Self::params().lending.charge_interval_blocks;
        let fresh = Position::fresh(Side::Short, now, Self::charge_block(now, interval));
        Self::quote_short_grow(netuid, q, fresh, true)
    }

    /// Quotes `increase_short(netuid, q, ..)` on the short `who` holds.
    pub fn quote_increase_short(
        who: &T::AccountId,
        netuid: NetUid,
        q: u64,
    ) -> Result<ShortGrowQuote, DispatchError> {
        let held = Positions::<T>::get((who.clone(), netuid, Side::Short))
            .ok_or(Error::<T>::NoPosition)?;
        Self::quote_short_grow(netuid, q, held, false)
    }

    /// The gates and quote of `do_grow` for a short, in its order, without the caller's
    /// balance and without a collateral: the least collateral that passes is part of the quote.
    fn quote_short_grow(
        netuid: NetUid,
        q: u64,
        held: Position<BlockNumberFor<T>>,
        opening: bool,
    ) -> Result<ShortGrowQuote, DispatchError> {
        let side = Side::Short;
        Self::ensure_side_enabled(side)?;
        let now = frame_system::Pallet::<T>::block_number();
        let params = Self::params();
        ensure!(q > 0, Error::<T>::InvalidAmount);
        let market = Self::market(netuid)?;
        ensure!(
            market.pool.tao >= params.lending.min_pool_tao,
            Error::<T>::PoolTooShallow
        );
        Self::ensure_flow(side, netuid, q, opening, &market, &params, now)?;

        let (pos, charge) = held.settle(market.refs.twap, &params.lending.rate_curve(), now);
        let (after, proceeds) = quote_grow::<T>(side, &market.pool, q)?;
        ensure!(proceeds > 0, Error::<T>::InvalidAmount);
        ensure!(
            after.tao >= params.lending.min_pool_tao,
            Error::<T>::PoolTooShallow
        );
        let leg = pos.leg.grown(q, proceeds);
        ensure!(
            leg.notional(market.refs.twap) >= params.lending.min_notional,
            Error::<T>::BelowMinNotional
        );
        let book_after = market
            .book(side)
            .checked_add(q)
            .ok_or(Error::<T>::Overflow)?;
        let post = post_pool(after, charge.amount);
        let need = coverage_need::<T::Curve>(
            &leg,
            book_after,
            &post,
            &priced_at::<T::Curve>(market.refs, &post),
            &params,
        )
        .ok_or(Error::<T>::CoverageNotMet)?;
        Ok(ShortGrowQuote {
            proceeds,
            interest: charge.amount,
            deposit: if opening { params.lending.deposit } else { 0 },
            coverage_need: need,
            min_collateral: need.saturating_sub(held_against(pos.collateral, &leg)),
            spot_after: T::Curve::spot(&post),
        })
    }

    /// Quotes `close_short(netuid, alpha, ..)` on the short `who` holds, with no price bound.
    pub fn quote_close_short(
        who: &T::AccountId,
        netuid: NetUid,
        alpha: u64,
    ) -> Result<ShortCloseQuote, DispatchError> {
        let side = Side::Short;
        let held =
            Positions::<T>::get((who.clone(), netuid, side)).ok_or(Error::<T>::NoPosition)?;
        let alpha = alpha.min(held.leg.alpha());
        ensure!(alpha > 0, Error::<T>::InvalidAmount);
        let now = frame_system::Pallet::<T>::block_number();
        let params = Self::params();
        let market = Self::market(netuid)?;
        let (pos, charge) = held.settle(market.refs.twap, &params.lending.rate_curve(), now);
        let full = alpha == pos.leg.alpha();
        let quote = quote_close::<T>(&pos.leg, &market.pool, alpha, pos.collateral, u64::MAX)?;
        if !full {
            let leg = pos.leg.shrunk(alpha, quote.consumed);
            let collateral = pos.collateral.saturating_sub(quote.from_collateral);
            let book = market
                .book(side)
                .checked_sub(alpha)
                .ok_or(Error::<T>::Overflow)?;
            let post = post_pool(quote.after, charge.amount.saturating_add(quote.repaid));
            let need = coverage_need::<T::Curve>(
                &leg,
                book,
                &post,
                &priced_at::<T::Curve>(market.refs, &post),
                &params,
            );
            ensure!(
                covers(need, held_against(collateral, &leg)),
                Error::<T>::CoverageNotMet
            );
        }
        Ok(ShortCloseQuote {
            alpha,
            cost: quote.value,
            from_proceeds: quote.consumed,
            from_collateral: quote.from_collateral,
            interest: charge.amount,
            full,
        })
    }
}
