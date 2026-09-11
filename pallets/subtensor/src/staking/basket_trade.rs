//! Validator-directed beta basket rebalancing (`swap_basket`): the money-moving path only.
//! Read-only views (`get_basket_trading_status`, budget arithmetic) live in `basket_views.rs`.

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
    /// Escrow holding rows valued by the two NAV sweeps (the larger of before / after).
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
    ///   both the subnet's moving (EMA) price and its spot price;
    /// * the TAO through the middle is taken from the fund's turnover bucket;
    /// * the destination holding may not end above [`crate::BasketLiquidityCap`] of the
    ///   destination pool's alpha reserve;
    /// * the destination holding may not end above [`crate::RootWeightsCap`] of fund NAV.
    ///
    /// AMM fees are charged like any user swap; the block-author fee is settled through the
    /// same helpers `stake_into_subnet` / `unstake_from_subnet` use.
    pub fn do_swap_basket(
        coldkey: T::AccountId,
        hotkey: T::AccountId,
        origin_netuid: NetUid,
        destination_netuid: NetUid,
        amount: u64,
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
        if !destination_netuid.is_root() {
            Self::ensure_subtoken_enabled(destination_netuid)?;
        }
        ensure!(amount > 0, Error::<T>::AmountTooLow);

        // Settle queued dividend credits first so the budget and the cap are measured
        // against the fund's full, current NAV. The flush work is priced into the
        // post-dispatch weight.
        let (flush_work, _, _) = Self::flush_basket_deposits_for_hotkey(&hotkey);

        let escrow = Self::get_beta_escrow_account_id();
        let held =
            Self::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &escrow, origin_netuid)
                .to_u64();
        ensure!(amount <= held, Error::<T>::NotEnoughStakeToWithdraw);

        let outcome = with_transaction(|| {
            match Self::try_swap_basket(&hotkey, &escrow, origin_netuid, destination_netuid, amount)
            {
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
    ) -> Result<BasketTradeOutcome, DispatchError> {
        let before = Self::try_valued_basket_holdings(hotkey)?;
        let nav_before: u64 = before
            .iter()
            .fold(0u64, |nav, (_, _, value)| nav.saturating_add(*value));

        // --- 1. Sell leg: origin holding -> free TAO on the origin pot.
        Self::decrease_stake_for_hotkey_and_coldkey_on_subnet(
            hotkey,
            escrow,
            origin_netuid,
            amount.into(),
        );
        let tao_mid: u64 = Self::sell_basket_leg(origin_netuid, amount.into())?;
        ensure!(
            TaoBalance::from(tao_mid) >= DefaultMinStake::<T>::get(),
            Error::<T>::AmountTooLow
        );

        // --- 2. Turnover budget, charged on the TAO through the middle.
        Self::consume_basket_trade_budget(hotkey, nav_before, tao_mid)?;

        // --- 3. Move the cash from the origin pot to the destination pot.
        let destination_account =
            Self::get_subnet_account_id(destination_netuid).ok_or(Error::<T>::SubnetNotExists)?;
        Self::transfer_tao_from_subnet(origin_netuid, &destination_account, tao_mid.into())?;

        // --- 4. Buy leg: TAO -> destination holding.
        let alpha_bought = Self::buy_basket_leg(destination_netuid, tao_mid.into())?;
        Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
            hotkey,
            escrow,
            destination_netuid,
            alpha_bought,
        );

        // --- 4b. Liquidity rule: the fund may not hold more of the destination than
        // `BasketLiquidityCap` of the pool's alpha reserve.
        Self::ensure_within_liquidity_cap(hotkey, escrow, destination_netuid)?;

        // --- 5. Shape rule on the post-trade fund: one valuation sweep gives the NAV, the
        // destination's value, and the row count.
        let after = Self::try_valued_basket_holdings(hotkey)?;
        let nav_after: u64 = after
            .iter()
            .fold(0u64, |nav, (_, _, value)| nav.saturating_add(*value));
        let destination_value: u64 = after
            .iter()
            .find(|(netuid, _, _)| *netuid == destination_netuid)
            .map_or(0, |(_, _, value)| *value);
        Self::ensure_within_root_cap(destination_value, nav_after)?;

        Ok(BasketTradeOutcome {
            tao_mid,
            alpha_bought: alpha_bought.to_u64(),
            holdings: (before.len() as u64).max(after.len() as u64),
        })
    }

    /// Sell `alpha` on `netuid` for TAO, leaving the TAO on the subnet's pot for the caller
    /// to move on. Root is the fund's cash slot: TAO 1:1, no pool, reserves unwound by
    /// hand. A dynamic subnet must fill fully at or above the price floor.
    fn sell_basket_leg(netuid: NetUid, alpha: AlphaBalance) -> Result<u64, DispatchError> {
        if netuid.is_root() {
            Self::debit_root_reserves(alpha.to_u64().into());
            return Ok(alpha.to_u64());
        }
        let floor = Self::basket_trade_price_limit(netuid, Leg::Sell)?;
        let out = Self::swap_alpha_for_tao(netuid, alpha, floor, false)?;
        let consumed = out.amount_paid_in.saturating_add(out.fee_paid);
        ensure!(consumed == alpha, Error::<T>::SlippageTooHigh);
        ensure!(!out.amount_paid_out.is_zero(), Error::<T>::AmountTooLow);

        let fee_outflow = Self::settle_alpha_fee_to_author(netuid, out.fee_to_block_author)?;
        Self::record_protocol_outflow(netuid, out.amount_paid_out.saturating_add(fee_outflow));
        Ok(out.amount_paid_out.to_u64())
    }

    /// Buy alpha on `netuid` with `tao` already sitting on the subnet's pot. Root is the
    /// fund's cash slot: TAO 1:1, no pool, reserves credited by hand. A dynamic subnet must
    /// fill fully at or below the price ceiling.
    fn buy_basket_leg(netuid: NetUid, tao: TaoBalance) -> Result<AlphaBalance, DispatchError> {
        if netuid.is_root() {
            Self::credit_root_reserves(tao);
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
        Ok(out.amount_paid_out)
    }

    /// AMM `price_limit` (TAO per alpha × 10⁹) for one trade leg: the stricter of the
    /// subnet's moving (EMA) price and its spot price, shifted by
    /// [`crate::BASKET_TRADE_MAX_SLIPPAGE_BPS`] against the trade (up for a buy ceiling,
    /// down for a sell floor). The EMA anchor defeats a pre-trade pump; the spot anchor caps
    /// the trade's own price impact. A subnet with no moving price yet is refused.
    ///
    /// When spot has already crossed the EMA-anchored bound the engine would reject the
    /// order with its own `PriceLimitExceeded`; refusing here keeps the caller-facing error
    /// `SlippageTooHigh` for every way a leg can miss the band.
    fn basket_trade_price_limit(netuid: NetUid, leg: Leg) -> Result<TaoBalance, DispatchError> {
        let ema: U64F64 = Self::get_moving_alpha_price(netuid);
        ensure!(
            ema > U64F64::saturating_from_num(0),
            Error::<T>::SlippageTooHigh
        );
        let spot: U64F64 = T::SwapInterface::current_alpha_price(netuid.into());
        let (reference, bps) = match leg {
            Leg::Buy => (
                ema.min(spot),
                BPS_DENOMINATOR.saturating_add(crate::BASKET_TRADE_MAX_SLIPPAGE_BPS),
            ),
            Leg::Sell => (
                ema.max(spot),
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
    /// elapsed (capacity `nav_before × BasketDailyTurnoverCap / u16::MAX`, full refill over
    /// [`crate::BASKET_TRADE_REFILL_BLOCKS`]).
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
            Self::share_within_root_cap(held, reserve, cap),
            Error::<T>::BasketLiquidityCapExceeded
        );
        Ok(())
    }

    /// Post-trade concentration check: the destination holding's realizable value may not
    /// exceed `RootWeightsCap[ROOT]` of fund NAV. Same rule and same young-chain softening
    /// as `do_set_root_weights`.
    fn ensure_within_root_cap(destination_value: u64, nav: u64) -> DispatchResult {
        let available = Self::get_all_subnet_netuids().len() as u64;
        if let Some(cap) = Self::binding_root_weights_cap(available) {
            ensure!(
                Self::share_within_root_cap(destination_value, nav, cap),
                Error::<T>::RootWeightCapExceeded
            );
        }
        Ok(())
    }

    /// Weight of one basket trade over `num_holdings` escrow rows: two AMM legs with fee
    /// settlement plus two realizable-NAV sweeps (before and after), as benchmarked.
    pub(crate) fn swap_basket_weight(num_holdings: u64) -> Weight {
        <T as crate::pallet::Config>::WeightInfo::swap_basket(
            u32::try_from(num_holdings).unwrap_or(u32::MAX),
        )
    }

    /// Weight of settling `flush_work` units of queued dividend credits ahead of a trade.
    /// `flush_basket_deposits_for_hotkey` reports its work in quote units (one sim-swap
    /// valuation each), so they are priced like NAV-sweep rows, as `claim_root` does. Zero
    /// when nothing was queued.
    pub(crate) fn basket_flush_weight(flush_work: u64) -> Weight {
        if flush_work == 0 {
            Weight::zero()
        } else {
            Self::basket_nav_sweep_weight(flush_work)
        }
    }

    /// Pre-dispatch weight of `swap_basket` for `hotkey`: the 256-holding trade cap plus the
    /// flush work its pending-deposit queue implies. Refunded to actual post-dispatch.
    pub(crate) fn swap_basket_declared_weight(hotkey: &T::AccountId) -> Weight {
        Self::swap_basket_weight(256).saturating_add(Self::basket_flush_weight(
            Self::swap_basket_flush_estimate(hotkey),
        ))
    }

    /// Upper estimate of the flush work a trade will do: nothing when the queue is empty,
    /// else one unit per queued credit plus the deposit's own sweeps (`holdings × 3 +
    /// destinations`, both bounded by the 256-row cap the trade weight already assumes).
    fn swap_basket_flush_estimate(hotkey: &T::AccountId) -> u64 {
        let credits = PendingBasketDeposits::<T>::iter_prefix(hotkey).count() as u64;
        if credits == 0 {
            0
        } else {
            credits.saturating_add(4 * 256)
        }
    }
}
