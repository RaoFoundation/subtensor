//! Beta basket valuation and read-only views (for RPC / dashboards).
//!
//! Everything here is side-effect free. The valuation primitives
//! (`realizable_tao_for_alpha`, `get_validator_basket_nav_tao`, `basket_payout_from`) are
//! also the single source of truth used by the money-moving paths in `claim_root.rs`, so
//! deposit share pricing, redemption sizing, and what dashboards report can never diverge.

use super::*;
use subtensor_runtime_common::NetUidStorageIndex;
use subtensor_swap_interface::{Order, SwapHandler};

impl<T: Config> Pallet<T> {
    /// Realizable TAO value of `alpha` on `netuid`: the slippage-aware quote a full redemption
    /// would fetch right now (net of fees), not the marked spot value `price * amount`. On a
    /// thin pool a tiny buy can push spot arbitrarily high, letting a marked NAV grow without
    /// bound (and saturate to `u64::MAX`); the realizable quote is bounded by the pool's TAO
    /// reserve, so NAV computed from it matches what the fund could actually pay out.
    pub fn realizable_tao_for_alpha(netuid: NetUid, alpha: u64) -> u64 {
        if alpha == 0 {
            return 0;
        }
        // Root holdings are TAO held 1:1; nothing to swap.
        if netuid.is_root() {
            return alpha;
        }
        // Stable-mechanism subnets redeem 1:1 (mirrors `swap_alpha_for_tao`).
        if SubnetMechanism::<T>::get(netuid) != 1 {
            return alpha;
        }
        let order = GetTaoForAlpha::<T>::with_amount(alpha);
        T::SwapInterface::sim_swap(netuid.into(), order)
            .map(|res| res.amount_paid_out.to_u64())
            .unwrap_or(0)
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

    /// A validator's beta basket weight vector `w`: the `(subnet, weight)` pairs it deploys its
    /// root dividends into (its curation strategy), exactly as stored.
    pub fn get_validator_root_weights(hotkey: &T::AccountId) -> Vec<(NetUid, u16)> {
        Uids::<T>::try_get(NetUid::ROOT, hotkey)
            .ok()
            .map(|uid| Weights::<T>::get(NetUidStorageIndex::ROOT, uid))
            .unwrap_or_default()
            .into_iter()
            .map(|(dest, weight)| (NetUid::from(dest), weight))
            .collect()
    }
}
