//! Validator-directed beta basket rebalancing (`swap_basket`): the money-moving path only.
//! Read-only views (`get_basket_trading_status`, budget arithmetic) live in `basket_views.rs`.

use super::basket_flush::MAX_BASKET_ROWS;
use super::*;
use crate::weights::WeightInfo;
use frame_support::storage::{TransactionOutcome, with_transaction};
use frame_support::weights::Weight;
use safe_math::*;
use sp_core::Get;
use sp_runtime::DispatchError;
use sp_runtime::traits::Zero;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance};
use subtensor_swap_interface::SwapHandler;

/// Basis-point denominator for [`crate::BASKET_TRADE_MAX_SLIPPAGE_BPS`].
const BPS_DENOMINATOR: u64 = 10_000;

/// Scale of the AMM `price_limit` argument (TAO per alpha × 10⁹), see `order_swap.rs`.
const PRICE_LIMIT_SCALE: u64 = 1_000_000_000;

/// Which side of the TAO middle a trade leg is on.
#[derive(Clone, Copy)]
enum Leg {
    /// Origin alpha -> TAO. Price bound is a floor.
    Sell,
    /// TAO -> destination alpha. Price bound is a ceiling.
    Buy,
}

/// Outcome of one executed basket trade, used for the event and the post-dispatch weight.
struct BasketTradeOutcome {
    /// TAO that passed through the middle of the swap.
    tao_mid: u64,
    /// Alpha (or TAO for a root destination) credited to the destination holding.
    alpha_bought: u64,
    /// Escrow holding rows valued: the pre-trade sweep, plus the destination row when the
    /// trade opens it.
    holdings: u64,
}

impl<T: Config> Pallet<T> {
    /// Validator-directed basket rebalance: sell `amount` of the fund's `origin_netuid`
    /// holding for TAO and buy `destination_netuid` with it. Either side may be root
    /// (netuid 0), the fund's TAO cash slot. Fund shares, rates, and watermarks are
    /// untouched: only the composition of the escrow holdings changes.
    ///
    /// Guardrails (see the storage docs on [`crate::BasketDailyTurnoverCap`]):
    /// * each AMM leg must fill fully within [`crate::BASKET_TRADE_MAX_SLIPPAGE_BPS`] of
    ///   the strictest of the subnet's slow moving (EMA) price, its fast moving price, and
    ///   its spot price ([`Self::basket_trade_price_limit`]);
    /// * the TAO through the middle is taken from the fund's turnover bucket, sized from
    ///   the fund's guarded NAV ([`Self::guarded_basket_holding_value`]);
    /// * the destination holding may not end above [`crate::BasketLiquidityCap`] of the
    ///   destination pool's alpha reserve, and destination *flow* in one refill
    ///   window is capped the same way (selling the holding does not restore
    ///   headroom);
    /// * the destination holding may not end above [`crate::BasketConcentrationCap`] of
    ///   the fund's guarded NAV;
    /// * both origin and destination (when not root) must have `SubtokenEnabled`.
    ///
    /// AMM fees are charged like any user swap; the block-author fee is settled through the
    /// same helpers `stake_into_subnet` / `unstake_from_subnet` use.
    ///
    /// `min_amount_out` is the caller's own floor on what the buy leg credits (destination
    /// alpha, or TAO when the destination is root); `0` means none. It sits on top of the
    /// protocol band, which is unchanged.
    pub fn do_swap_basket(
        coldkey: T::AccountId,
        hotkey: T::AccountId,
        origin_netuid: NetUid,
        destination_netuid: NetUid,
        amount: u64,
        min_amount_out: u64,
    ) -> Result<Weight, DispatchError> {
        ensure!(
            BasketTradingEnabled::<T>::get(),
            Error::<T>::BasketTradingDisabled
        );
        ensure!(
            !BasketTradingFrozen::<T>::contains_key(&hotkey),
            Error::<T>::BasketTradingFrozen
        );
        Self::ensure_beta_basket_seed_idle()?;
        ensure!(
            origin_netuid != destination_netuid,
            Error::<T>::BasketSameSubnet
        );
        ensure!(
            Self::coldkey_owns_hotkey(&coldkey, &hotkey),
            Error::<T>::NonAssociatedColdKey
        );
        ensure!(
            Self::is_hotkey_registered_on_network(NetUid::ROOT, &hotkey),
            Error::<T>::HotKeyNotRegisteredInSubNet
        );
        ensure!(
            origin_netuid.is_root() || Self::if_subnet_exist(origin_netuid),
            Error::<T>::SubnetNotExists
        );
        ensure!(
            destination_netuid.is_root() || Self::if_subnet_exist(destination_netuid),
            Error::<T>::SubnetNotExists
        );
        if !origin_netuid.is_root() {
            Self::ensure_subtoken_enabled(origin_netuid)?;
        }
        if !destination_netuid.is_root() {
            Self::ensure_subtoken_enabled(destination_netuid)?;
        }
        ensure!(amount > 0, Error::<T>::AmountTooLow);

        // Settle queued dividend credits first so the budget and the cap are measured
        // against the fund's full, current NAV. The flush work is priced into the
        // post-dispatch weight; the declared weight carries its flat allowance.
        let (flush_work, _, _) = Self::flush_basket_deposits_for_hotkey(&hotkey);

        let escrow = Self::get_beta_escrow_account_id();
        let held =
            Self::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &escrow, origin_netuid)
                .to_u64();
        ensure!(amount <= held, Error::<T>::NotEnoughStakeToWithdraw);

        let outcome = with_transaction(|| {
            match Self::try_swap_basket(
                &hotkey,
                &escrow,
                origin_netuid,
                destination_netuid,
                amount,
                min_amount_out,
            ) {
                Ok(outcome) => TransactionOutcome::Commit(Ok(outcome)),
                Err(err) => TransactionOutcome::Rollback(Err(err)),
            }
        })?;

        Self::deposit_event(Event::BasketSwapped {
            hotkey,
            origin_netuid,
            destination_netuid,
            alpha_sold: amount.into(),
            tao_mid: outcome.tao_mid.into(),
            alpha_bought: outcome.alpha_bought.into(),
        });

        Ok(Self::swap_basket_weight(outcome.holdings)
            .saturating_add(Self::basket_flush_weight(flush_work)))
    }

    /// Transactional body of [`Self::do_swap_basket`]; any error rolls the whole trade back.
    fn try_swap_basket(
        hotkey: &T::AccountId,
        escrow: &T::AccountId,
        origin_netuid: NetUid,
        destination_netuid: NetUid,
        amount: u64,
        min_amount_out: u64,
    ) -> Result<BasketTradeOutcome, DispatchError> {
        // Every holding is valued twice: at its realizable quote (the NAV the fund could
        // pay out, used for bookkeeping) and at its guarded mark (the same, capped at the
        // slow-EMA value of the alpha — the figure the turnover budget and the
        // concentration cap are measured against, see [`Self::guarded_basket_holding_value`]).
        let before = Self::try_valued_basket_holdings(hotkey)?;
        let guarded_before: Vec<(NetUid, u64)> = before
            .iter()
            .map(|(netuid, alpha, value)| {
                (
                    *netuid,
                    Self::guarded_basket_holding_value(*netuid, alpha.to_u64(), *value),
                )
            })
            .collect();
        let guarded_nav_before: u64 = guarded_before
            .iter()
            .fold(0u64, |nav, (_, value)| nav.saturating_add(*value));
        let guarded_value_before = |netuid: NetUid| -> u64 {
            guarded_before
                .iter()
                .find(|(row, _)| *row == netuid)
                .map(|(_, value)| *value)
                .unwrap_or(0)
        };
        let destination_is_new = !before.iter().any(|(row, _, _)| *row == destination_netuid);

        // --- 1. Sell leg: origin holding -> free TAO on the origin pot.
        let tao_mid: u64 = Self::sell_basket_leg(hotkey, escrow, origin_netuid, amount.into())?;
        ensure!(
            TaoBalance::from(tao_mid) >= DefaultMinStake::<T>::get(),
            Error::<T>::AmountTooLow
        );

        // --- 2. Turnover budget, charged on the TAO through the middle and sized from the
        // guarded (un-pumpable) NAV.
        Self::consume_basket_trade_budget(hotkey, guarded_nav_before, tao_mid)?;

        // --- 3. Move the cash from the origin pot to the destination pot.
        let destination_account =
            Self::get_subnet_account_id(destination_netuid).ok_or(Error::<T>::SubnetNotExists)?;
        Self::transfer_tao_from_subnet(origin_netuid, &destination_account, tao_mid.into())?;

        // --- 4. Buy leg: TAO -> destination holding.
        let alpha_bought =
            Self::buy_basket_leg(hotkey, escrow, destination_netuid, tao_mid.into())?;

        // --- 4a. Caller's floor on the credited amount (fees already settled in the leg).
        // The unit follows the destination: alpha, or rao of TAO for the root cash slot.
        ensure!(
            alpha_bought.to_u64() >= min_amount_out,
            Error::<T>::BasketMinOutNotMet
        );

        // --- 4b. Liquidity rule: the fund may not hold more of the destination than
        // `BasketLiquidityCap` of the pool's alpha reserve, and may not *buy*
        // more than that share in one refill window (the standing holding resets
        // on unwind; the flow counter does not).
        Self::ensure_within_liquidity_cap(hotkey, escrow, destination_netuid)?;
        Self::consume_basket_liquidity_flow(hotkey, destination_netuid, alpha_bought.to_u64())?;

        // --- 5. Shape rule on the post-trade fund. The trade moved only the origin and
        // destination pools (root is 1:1), so every other row's mark is unchanged and the
        // full re-valuation collapses to re-quoting those two holdings. The destination is
        // measured at its realizable value (a pump there only makes the check stricter);
        // the fund it is measured against is the guarded NAV, which a same-block pump of
        // any held pool cannot inflate.
        let (_, origin_after) = Self::basket_holding_marks(hotkey, escrow, origin_netuid)?;
        let (destination_value, destination_after) =
            Self::basket_holding_marks(hotkey, escrow, destination_netuid)?;
        let guarded_nav_after: u64 = guarded_nav_before
            .saturating_sub(guarded_value_before(origin_netuid))
            .saturating_sub(guarded_value_before(destination_netuid))
            .saturating_add(origin_after)
            .saturating_add(destination_after);
        Self::ensure_within_concentration_cap(destination_value, guarded_nav_after)?;

        Ok(BasketTradeOutcome {
            tao_mid,
            alpha_bought: alpha_bought.to_u64(),
            holdings: (before.len() as u64).saturating_add(u64::from(destination_is_new)),
        })
    }

    /// The fund's current holding on `netuid` at both marks: `(realizable, guarded)`, priced
    /// like a row of [`Self::try_valued_basket_holdings`] (terminal garbage is zero) and
    /// then capped by [`Self::guarded_basket_holding_value`].
    fn basket_holding_marks(
        hotkey: &T::AccountId,
        escrow: &T::AccountId,
        netuid: NetUid,
    ) -> Result<(u64, u64), DispatchError> {
        let held =
            Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, escrow, netuid).to_u64();
        let realizable = Self::try_realizable_tao_for_alpha(netuid, held)?.unwrap_or(0);
        Ok((
            realizable,
            Self::guarded_basket_holding_value(netuid, held, realizable),
        ))
    }

    /// The guarded mark of `alpha` on `netuid` given its `realizable` quote:
    /// `min(realizable, alpha × slow EMA)`. Root cash is TAO 1:1 and passes through.
    ///
    /// The realizable quote is bounded only by the pool's TAO reserve, and a pumper grows
    /// that reserve by whatever TAO they deposit — so for one block a held thin pool can be
    /// marked at roughly the pump size, inflating the NAV the turnover budget and the
    /// concentration cap are measured against (a refused oversize buy becomes admitted).
    /// The slow EMA cannot be moved inside a block, so capping the mark at the EMA value
    /// of the alpha makes both guards un-pumpable. The cap is one-sided on purpose: a pool
    /// trading above its monthly EMA — or above parity, where the slow EMA clamps at 1.0 —
    /// is under-marked, which only tightens the guards (smaller budget, stricter cap).
    pub(crate) fn guarded_basket_holding_value(netuid: NetUid, alpha: u64, realizable: u64) -> u64 {
        if netuid.is_root() {
            return realizable;
        }
        let ema_value: u64 = Self::get_moving_alpha_price(netuid)
            .saturating_mul(U64F64::saturating_from_num(alpha))
            .saturating_to_num::<u64>();
        realizable.min(ema_value)
    }

    /// Sell `alpha` of the fund's `netuid` holding for TAO, leaving the TAO on the subnet's
    /// pot for the caller to move on. Root is the fund's cash slot: TAO 1:1, no pool. A
    /// dynamic subnet must fill fully at or above the price floor.
    fn sell_basket_leg(
        hotkey: &T::AccountId,
        escrow: &T::AccountId,
        netuid: NetUid,
        alpha: AlphaBalance,
    ) -> Result<u64, DispatchError> {
        // Only alpha that really left the holding may be sold (or, on root, moved on): a
        // short debit would otherwise still be swapped for TAO in full, letting the fund sell
        // alpha it never held.
        if netuid.is_root() {
            let removed = Self::debit_root_slot(hotkey, escrow, alpha.to_u64().into());
            ensure!(
                removed.to_u64() == alpha.to_u64(),
                Error::<T>::NotEnoughStakeToWithdraw
            );
            return Ok(alpha.to_u64());
        }
        let alpha_removed =
            Self::decrease_stake_for_hotkey_and_coldkey_on_subnet(hotkey, escrow, netuid, alpha);
        ensure!(alpha_removed == alpha, Error::<T>::NotEnoughStakeToWithdraw);
        let floor = Self::basket_trade_price_limit(netuid, Leg::Sell)?;
        let out = Self::swap_alpha_for_tao(netuid, alpha, floor, false)?;
        let consumed = out.amount_paid_in.saturating_add(out.fee_paid);
        ensure!(consumed == alpha, Error::<T>::SlippageTooHigh);
        ensure!(!out.amount_paid_out.is_zero(), Error::<T>::AmountTooLow);

        let fee_outflow = Self::settle_alpha_fee_to_author(netuid, out.fee_to_block_author)?;
        Self::record_protocol_outflow(netuid, out.amount_paid_out.saturating_add(fee_outflow));
        Ok(out.amount_paid_out.to_u64())
    }

    /// Buy alpha on `netuid` with `tao` already sitting on the subnet's pot and credit it to
    /// the fund's holding. Root is the fund's cash slot: TAO 1:1, no pool. A dynamic subnet
    /// must fill fully at or below the price ceiling.
    fn buy_basket_leg(
        hotkey: &T::AccountId,
        escrow: &T::AccountId,
        netuid: NetUid,
        tao: TaoBalance,
    ) -> Result<AlphaBalance, DispatchError> {
        if netuid.is_root() {
            Self::credit_root_slot(hotkey, escrow, tao);
            return Ok(tao.to_u64().into());
        }
        let ceiling = Self::basket_trade_price_limit(netuid, Leg::Buy)?;
        let out = Self::swap_tao_for_alpha(netuid, tao, ceiling, false)?;
        let consumed = out.amount_paid_in.saturating_add(out.fee_paid);
        ensure!(consumed == tao, Error::<T>::SlippageTooHigh);
        ensure!(!out.amount_paid_out.is_zero(), Error::<T>::AmountTooLow);

        Self::settle_tao_fee_to_author(netuid, out.fee_to_block_author)?;
        // Same basis as `stake_into_subnet`: what entered the pool, fee excluded.
        Self::record_protocol_inflow(netuid, out.amount_paid_in);
        Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
            hotkey,
            escrow,
            netuid,
            out.amount_paid_out,
        );
        Ok(out.amount_paid_out)
    }

    /// AMM `price_limit` (TAO per alpha × 10⁹) for one trade leg: the strictest of the
    /// subnet's slow moving (emission EMA) price, its fast moving price
    /// ([`crate::SubnetFastMovingPrice`]), and its spot price, shifted by
    /// [`crate::BASKET_TRADE_MAX_SLIPPAGE_BPS`] against the trade (up for a buy ceiling,
    /// down for a sell floor).
    ///
    /// Each anchor closes a different hole. Spot caps the trade's own price impact. The fast
    /// EMA (two-hour half-life, updated from the previous block's closing spot) is a price
    /// nobody can move inside a block: a key holder who lifts spot and has the fund buy, or
    /// dumps spot and has the fund sell, finds the fund bound to within 2% of where the pool
    /// traded before the move, so the fund never fills at the manipulated price. Pulling
    /// the anchor along means holding the pump — and the capital behind it — against
    /// arbitrage for several half-lives. The slow EMA (monthly) stays in the min/max as the
    /// level cap on how far even a held pump can carry the fund. A subnet missing either
    /// EMA (never emitted, or not yet updated since the fast series was introduced) is
    /// refused.
    ///
    /// When spot has already crossed the anchored bound the engine would reject the order
    /// with its own `PriceLimitExceeded`; refusing here keeps the caller-facing error
    /// `SlippageTooHigh` for every way a leg can miss the band.
    fn basket_trade_price_limit(netuid: NetUid, leg: Leg) -> Result<TaoBalance, DispatchError> {
        let slow: U64F64 = Self::get_moving_alpha_price(netuid);
        ensure!(
            slow > U64F64::saturating_from_num(0),
            Error::<T>::SlippageTooHigh
        );
        let fast: U64F64 =
            SubnetFastMovingPrice::<T>::get(netuid).ok_or(Error::<T>::SlippageTooHigh)?;
        ensure!(
            fast > U64F64::saturating_from_num(0),
            Error::<T>::SlippageTooHigh
        );
        let spot: U64F64 = T::SwapInterface::current_alpha_price(netuid.into());
        let (reference, bps) = match leg {
            Leg::Buy => (
                slow.min(fast).min(spot),
                BPS_DENOMINATOR.saturating_add(crate::BASKET_TRADE_MAX_SLIPPAGE_BPS),
            ),
            Leg::Sell => (
                slow.max(fast).max(spot),
                BPS_DENOMINATOR.saturating_sub(crate::BASKET_TRADE_MAX_SLIPPAGE_BPS),
            ),
        };
        let bound: U64F64 = reference
            .saturating_mul(U64F64::saturating_from_num(bps))
            .safe_div(U64F64::saturating_from_num(BPS_DENOMINATOR));
        let already_past = match leg {
            Leg::Buy => spot > bound,
            Leg::Sell => spot < bound,
        };
        ensure!(!already_past, Error::<T>::SlippageTooHigh);

        let limit = bound.saturating_mul(U64F64::saturating_from_num(PRICE_LIMIT_SCALE));
        Ok(limit.saturating_to_num::<u64>().into())
    }

    /// Take `tao_mid` out of the fund's turnover bucket after refilling it for the blocks
    /// elapsed (capacity `nav_before × BasketDailyTurnoverCap / u16::MAX` with `nav_before`
    /// the guarded NAV, full refill over [`crate::BASKET_TRADE_REFILL_BLOCKS`]).
    fn consume_basket_trade_budget(
        hotkey: &T::AccountId,
        nav_before: u64,
        tao_mid: u64,
    ) -> DispatchResult {
        let now = Self::get_current_block_as_u64();
        let budget = Self::basket_trade_budget_tao(nav_before);
        let available = Self::basket_trade_bucket_at(hotkey, now, budget);
        let remaining = available
            .checked_sub(tao_mid)
            .ok_or(Error::<T>::BasketTurnoverBudgetExceeded)?;
        BasketTradeBucket::<T>::insert(hotkey, (remaining, now));
        Ok(())
    }

    /// Cumulative destination flow in the current refill window. Decays linearly
    /// to zero over [`crate::BASKET_TRADE_REFILL_BLOCKS`]. Missing row is zero.
    fn basket_liquidity_used_at(hotkey: &T::AccountId, netuid: NetUid, now: u64) -> u64 {
        match BasketLiquidityUsed::<T>::get(hotkey, netuid) {
            None => 0,
            Some((used, last_block)) => {
                let elapsed = now.saturating_sub(last_block);
                if elapsed >= crate::BASKET_TRADE_REFILL_BLOCKS {
                    0
                } else {
                    let remaining = crate::BASKET_TRADE_REFILL_BLOCKS.saturating_sub(elapsed);
                    Self::mul_div_u64(used, remaining, crate::BASKET_TRADE_REFILL_BLOCKS)
                }
            }
        }
    }

    /// Charge `alpha_bought` against the destination's refill-window flow cap
    /// (same share of `SubnetAlphaIn` as the standing liquidity cap). Root is
    /// exempt. This is what stops accumulate/unwind wash: selling the holding
    /// does not restore flow headroom.
    fn consume_basket_liquidity_flow(
        hotkey: &T::AccountId,
        netuid: NetUid,
        alpha_bought: u64,
    ) -> DispatchResult {
        if netuid.is_root() {
            return Ok(());
        }
        let now = Self::get_current_block_as_u64();
        let used = Self::basket_liquidity_used_at(hotkey, netuid, now);
        let new_used = used.saturating_add(alpha_bought);
        let reserve = SubnetAlphaIn::<T>::get(netuid).to_u64();
        let cap = BasketLiquidityCap::<T>::get() as u64;
        ensure!(
            Self::share_within_cap(new_used, reserve, cap),
            Error::<T>::BasketLiquidityCapExceeded
        );
        BasketLiquidityUsed::<T>::insert(hotkey, netuid, (new_used, now));
        Ok(())
    }

    /// Post-buy liquidity check: the fund's holding on `netuid` may not exceed
    /// [`crate::BasketLiquidityCap`] of the pool's alpha reserve. Root is the fund's cash
    /// slot with no pool and is exempt. Realizable value (the concentration cap's measure)
    /// is bounded by the pool's TAO reserve, so it cannot see a fund accumulating a thin
    /// pool's supply while counterparties sell into its price support; this rule can.
    fn ensure_within_liquidity_cap(
        hotkey: &T::AccountId,
        escrow: &T::AccountId,
        netuid: NetUid,
    ) -> DispatchResult {
        if netuid.is_root() {
            return Ok(());
        }
        let held =
            Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, escrow, netuid).to_u64();
        let reserve = SubnetAlphaIn::<T>::get(netuid).to_u64();
        let cap = BasketLiquidityCap::<T>::get() as u64;
        ensure!(
            Self::share_within_cap(held, reserve, cap),
            Error::<T>::BasketLiquidityCapExceeded
        );
        Ok(())
    }

    /// The basket concentration cap ([`crate::BasketConcentrationCap`], u16-normalized) when
    /// it is enforceable with `available` subnets, else `None`. A cap of 1/16 needs at least
    /// 16 destinations to be satisfiable, so the rule is skipped while the chain has fewer
    /// (young chains, tests).
    pub(crate) fn binding_basket_concentration_cap(available: u64) -> Option<u64> {
        let cap = BasketConcentrationCap::<T>::get() as u64;
        let min_dests_for_cap = (u16::MAX as u64).div_ceil(cap.max(1));
        (available >= min_dests_for_cap).then_some(cap)
    }

    /// `part / whole <= cap / u16::MAX`, computed in u128 so chain-scale TAO values cannot
    /// overflow. `whole == 0` (empty fund) trivially passes.
    pub(crate) fn share_within_cap(part: u64, whole: u64, cap: u64) -> bool {
        u128::from(part).saturating_mul(u128::from(u16::MAX))
            <= u128::from(cap).saturating_mul(u128::from(whole))
    }

    /// Post-trade concentration check: the destination holding's realizable value may not
    /// exceed [`crate::BasketConcentrationCap`] of the fund's guarded NAV.
    fn ensure_within_concentration_cap(destination_value: u64, nav: u64) -> DispatchResult {
        let available = Self::get_all_subnet_netuids().len() as u64;
        if let Some(cap) = Self::binding_basket_concentration_cap(available) {
            ensure!(
                Self::share_within_cap(destination_value, nav, cap),
                Error::<T>::BasketConcentrationCapExceeded
            );
        }
        Ok(())
    }

    /// Weight of one basket trade over `num_holdings` escrow rows: two AMM legs with fee
    /// settlement plus the pre-trade realizable-NAV sweep and the two post-trade re-quotes
    /// (origin and destination), as benchmarked.
    ///
    /// Plus one `BasketLiquidityUsed` get/insert on a non-root destination. Do not invent
    /// CPU time here — CI's reference `bench-patch` updates
    /// [`WeightInfo::swap_basket`](crate::weights::WeightInfo::swap_basket).
    pub(crate) fn swap_basket_weight(num_holdings: u64) -> Weight {
        <T as crate::pallet::Config>::WeightInfo::swap_basket(
            u32::try_from(num_holdings).unwrap_or(u32::MAX),
        )
        .saturating_add(T::DbWeight::get().reads_writes(1, 1))
    }

    /// Pre-dispatch weight of `swap_basket`: the trade over the row cap plus the flat
    /// pending-deposit flush allowance ([`Self::basket_flush_weight_bound`]) shared by every
    /// extrinsic that flushes. Refunded to actual post-dispatch.
    pub(crate) fn swap_basket_declared_weight() -> Weight {
        Self::swap_basket_weight(MAX_BASKET_ROWS).saturating_add(Self::basket_flush_weight_bound())
    }
}
