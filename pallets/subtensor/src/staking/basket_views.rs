//! Beta basket valuation and read-only views (for RPC / dashboards).
//!
//! Everything here is side-effect free. The valuation primitives
//! (`realizable_tao_for_alpha`, `get_validator_basket_nav_tao`, `basket_payout_from`) are
//! also the single source of truth used by the money-moving paths in `claim_root.rs`, so
//! deposit share pricing, redemption sizing, and what dashboards report can never diverge.

use super::*;
use crate::rpc_info::basket_info::BasketTradingStatus;
use frame_support::storage::{TransactionOutcome, with_transaction};
use sp_runtime::DispatchError;
use subtensor_swap_interface::{Order, SwapHandler};

impl<T: Config> Pallet<T> {
    /// Realizable TAO value of `alpha` on `netuid`: the slippage-aware quote a full basket
    /// redemption would fetch right now, using the same fee-free protocol swap as the
    /// money-moving claim path, not the marked spot value `price * amount`. On a thin pool a
    /// tiny buy can push spot arbitrarily high, letting a marked NAV grow without bound (and
    /// saturate to `u64::MAX`); the realizable quote is bounded by the pool's TAO reserve, so
    /// NAV computed from it matches what the fund could actually pay out.
    pub fn realizable_tao_for_alpha(netuid: NetUid, alpha: u64) -> u64 {
        Self::try_realizable_tao_for_alpha(netuid, alpha)
            .ok()
            .flatten()
            .unwrap_or(0)
    }

    /// Fallible money-moving valuation. `Ok(None)` means the pool is terminally too shallow to
    /// realize even one atomic unit and the holding may be explicitly written off. Unknown or
    /// accounting failures remain `Err`; callers must not silently mark them to zero.
    pub(crate) fn try_realizable_tao_for_alpha(
        netuid: NetUid,
        alpha: u64,
    ) -> Result<Option<u64>, DispatchError> {
        if alpha == 0 {
            return Ok(Some(0));
        }
        if netuid.is_root() || SubnetMechanism::<T>::get(netuid) != 1 {
            return Ok(Some(alpha));
        }
        #[cfg(test)]
        crate::tests::mock::inc_basket_quote_ops();

        let maximum = T::SwapInterface::max_swap_input::<GetTaoForAlpha<T>>(netuid).to_u64();
        let quote = if alpha <= maximum {
            // Preserve the cheap single-swap simulation for ordinary holdings. Only an
            // oversized position needs the sequential reserve updates of the rollback overlay.
            let order = GetTaoForAlpha::<T>::with_amount(alpha);
            T::SwapInterface::swap(
                netuid.into(),
                order,
                T::SwapInterface::min_price::<TaoBalance>(),
                true,
                true,
            )
            .map(|result| result.amount_paid_out)
        } else {
            with_transaction(|| {
                TransactionOutcome::Rollback(Self::swap_basket_alpha_for_tao_chunks(
                    netuid,
                    alpha.into(),
                ))
            })
        };
        match quote {
            Ok(tao) => Ok(Some(tao.to_u64())),
            Err(err)
                if T::SwapInterface::classify_failure(&err)
                    == subtensor_swap_interface::SwapFailureKind::TerminalLiquidity =>
            {
                Ok(None)
            }
            Err(err) => Err(err),
        }
    }

    /// Every escrow holding with its realizable TAO value: `(netuid, alpha, value)`. One
    /// sim-swap per row; terminal garbage values at zero, every unknown valuation error
    /// propagates. NAV is the sum of the values; the row count sizes weight.
    pub(crate) fn try_valued_basket_holdings(
        hotkey: &T::AccountId,
    ) -> Result<Vec<(NetUid, AlphaBalance, u64)>, DispatchError> {
        Self::get_basket_holdings(hotkey)
            .into_iter()
            .map(|(netuid, alpha)| {
                let value =
                    Self::try_realizable_tao_for_alpha(netuid, alpha.to_u64())?.unwrap_or(0);
                Ok((netuid, alpha, value))
            })
            .collect()
    }

    /// Fallible NAV for share pricing and claims. Terminal garbage contributes zero; every
    /// unknown valuation error aborts the money-moving operation.
    pub(crate) fn try_get_validator_basket_nav_tao(
        hotkey: &T::AccountId,
    ) -> Result<u64, DispatchError> {
        Ok(Self::try_valued_basket_holdings(hotkey)?
            .into_iter()
            .fold(0u64, |nav, (_, _, value)| nav.saturating_add(value)))
    }

    /// Single source of truth for redemption sizing: a staker's owed shares are worth
    /// `owed * N / P` TAO (fund NAV over outstanding shares), capped at the NAV so a claim can
    /// never be marked above what the fund holds.
    pub fn basket_payout_from(owed_shares: u64, nav: u64, shares_total: u64) -> u64 {
        Self::mul_div_u64(owed_shares, nav, shares_total).min(nav)
    }

    /// A validator's fund NAV in TAO at realizable (slippage-aware) quotes. This is the single
    /// valuation used for both deposit share pricing and redemption sizing, so the two can
    /// never diverge under a manipulated spot price.
    pub fn get_validator_basket_nav_tao(hotkey: &T::AccountId) -> TaoBalance {
        let mut nav: u64 = 0;
        for (netuid, alpha) in Self::get_basket_holdings(hotkey) {
            nav = nav.saturating_add(Self::realizable_tao_for_alpha(netuid, alpha.to_u64()));
        }
        nav.into()
    }

    /// Current TAO payout a staker would realize (mark-to-market) by redeeming their owed
    /// shares on a validator.
    pub fn get_basket_payout_tao(hotkey: &T::AccountId, coldkey: &T::AccountId) -> u64 {
        let owed_shares = Self::get_basket_owed_shares(hotkey, coldkey);
        let shares_total = BasketShares::<T>::get(hotkey);
        let nav: u64 = Self::get_validator_basket_nav_tao(hotkey).to_u64();
        Self::basket_payout_from(owed_shares.min(shares_total), nav, shares_total)
    }

    /// Current TAO entitlement of a staker's unclaimed share of one subnet holding in a
    /// validator's basket. Claims sell the corresponding pro-rata alpha amount, but pay this
    /// full-liquidation-NAV fraction and retain any larger concavity surplus in the fund.
    pub fn get_basket_subnet_payout_tao(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        netuid: NetUid,
    ) -> u64 {
        let shares_total = BasketShares::<T>::get(hotkey);
        if shares_total == 0 {
            return 0;
        }

        let owed_shares = Self::get_basket_owed_shares(hotkey, coldkey).min(shares_total);
        if owed_shares == 0 {
            return 0;
        }

        let escrow = Self::get_beta_escrow_account_id();
        let holding =
            Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, &escrow, netuid).to_u64();
        let slot_nav = Self::realizable_tao_for_alpha(netuid, holding);
        Self::basket_payout_from(owed_shares, slot_nav, shares_total)
    }

    /// Total TAO a coldkey would realize by redeeming every beta basket it holds across all of
    /// its validators (mark-to-market). This is the "pending TAO owed" figure for a staker.
    pub fn get_root_basket_owed_tao(coldkey: &T::AccountId) -> TaoBalance {
        let mut total: u64 = 0;
        for hotkey in StakingHotkeys::<T>::get(coldkey) {
            total = total.saturating_add(Self::get_basket_payout_tao(&hotkey, coldkey));
        }
        total.into()
    }

    /// A validator's full basket breakdown: per subnet, the alpha held and its realizable
    /// TAO value.
    pub fn get_validator_basket(hotkey: &T::AccountId) -> Vec<(NetUid, AlphaBalance, TaoBalance)> {
        Self::get_basket_holdings(hotkey)
            .into_iter()
            .map(|(netuid, alpha)| {
                let tao = Self::realizable_tao_for_alpha(netuid, alpha.to_u64());
                (netuid, alpha, tao.into())
            })
            .collect()
    }

    /// TAO the fund's `swap_basket` turnover bucket holds at block `now` given a bucket
    /// capacity of `budget`: the stored level plus `budget / BASKET_TRADE_REFILL_BLOCKS` per
    /// block elapsed since the last refill, clamped to `budget`. A fund with no stored
    /// bucket (never traded) is full. Clamping also absorbs a NAV drop: the level can never
    /// exceed one current budget.
    pub fn basket_trade_bucket_at(hotkey: &T::AccountId, now: u64, budget: u64) -> u64 {
        Self::basket_bucket_level_at(BasketTradeBucket::<T>::get(hotkey), now, budget)
    }

    /// Level of a refilling bucket stored as `(level, last_refill_block)` at block `now`
    /// with capacity `budget`: the stored level plus `budget / BASKET_TRADE_REFILL_BLOCKS`
    /// per elapsed block, clamped to `budget`; a missing row is a full bucket. Shared by
    /// the `swap_basket` turnover bucket and the cash-first claim bucket. Monotone in
    /// `budget` and in `now`.
    pub fn basket_bucket_level_at(stored: Option<(u64, u64)>, now: u64, budget: u64) -> u64 {
        match stored {
            None => budget,
            Some((level, last_refill_block)) => {
                let elapsed = now.saturating_sub(last_refill_block);
                let refill = Self::mul_div_u64(budget, elapsed, crate::BASKET_TRADE_REFILL_BLOCKS);
                level.saturating_add(refill).min(budget)
            }
        }
    }

    /// The fund's NAV at the cash-claim mark ([`Self::basket_cash_nav_tao`]): every holding
    /// at its anchored liquidation value, root cash 1:1, rows that realize nothing live at
    /// zero, less the cost-basis correction. What a cash-first claim prices the claimant's
    /// shares against. A view; valuation failures read as zero.
    pub fn get_validator_basket_cash_mark_nav_tao(hotkey: &T::AccountId) -> TaoBalance {
        Self::basket_cash_nav_tao(hotkey).unwrap_or(0).into()
    }

    /// Capacity of a fund's `swap_basket` turnover bucket at `nav`
    /// (`nav × BasketDailyTurnoverCap / u16::MAX`).
    pub fn basket_trade_budget_tao(nav: u64) -> u64 {
        Self::mul_div_u64(
            nav,
            BasketDailyTurnoverCap::<T>::get() as u64,
            u16::MAX as u64,
        )
    }

    /// The fund's guarded NAV: every holding at
    /// [`Self::guarded_basket_holding_value`] — its realizable quote capped at the slow-EMA
    /// value of the alpha — summed. This is the NAV the `swap_basket` turnover budget and
    /// concentration cap are measured against; unlike the realizable NAV it cannot be
    /// inflated by pumping a held pool inside a block. Valuation failures mark the row at
    /// zero (a view, not a money path).
    pub fn get_validator_basket_guarded_nav_tao(hotkey: &T::AccountId) -> TaoBalance {
        let mut nav: u64 = 0;
        for (netuid, alpha) in Self::get_basket_holdings(hotkey) {
            let realizable = Self::realizable_tao_for_alpha(netuid, alpha.to_u64());
            nav = nav.saturating_add(Self::guarded_basket_holding_value(
                netuid,
                alpha.to_u64(),
                realizable,
            ));
        }
        nav.into()
    }

    /// Explorer / CLI view of one fund's `swap_basket` status as a trade at the current
    /// block would see it (the budget is sized from the guarded NAV, exactly as a trade
    /// sizes it).
    pub fn get_basket_trading_status(hotkey: &T::AccountId) -> BasketTradingStatus {
        let now = Self::get_current_block_as_u64();
        let nav = Self::get_validator_basket_guarded_nav_tao(hotkey).to_u64();
        let budget = Self::basket_trade_budget_tao(nav);
        BasketTradingStatus {
            enabled: BasketTradingEnabled::<T>::get(),
            frozen: BasketTradingFrozen::<T>::contains_key(hotkey),
            refill_blocks: crate::BASKET_TRADE_REFILL_BLOCKS,
            tao_available: Self::basket_trade_bucket_at(hotkey, now, budget).into(),
            budget_tao: budget.into(),
        }
    }

    /// Network-wide total beta basket NAV across all validators, in TAO (mark-to-market).
    /// Sampling this over time yields the TAO/day flowing to root stakers.
    pub fn get_root_basket_total_nav_tao() -> TaoBalance {
        // Accumulate in u128 so per-validator values near u64::MAX cannot silently pin the
        // network-wide aggregate at the saturation ceiling.
        let mut nav: u128 = 0;
        for hotkey in BasketShares::<T>::iter_keys() {
            nav = nav.saturating_add(u128::from(
                Self::get_validator_basket_nav_tao(&hotkey).to_u64(),
            ));
        }
        u64::try_from(nav).unwrap_or(u64::MAX).into()
    }
}
