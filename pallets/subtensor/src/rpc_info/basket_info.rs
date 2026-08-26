extern crate alloc;

use frame_support::pallet_prelude::{Decode, Encode};
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance};
use subtensor_swap_interface::SwapHandler;

use super::*;

/// One holding inside a validator's beta basket, valued two ways: `spot_tao` marks at the
/// pool's spot price (`price * alpha`, comparable across explorers), `realizable_tao` is the
/// slippage-aware quote a full redemption would fetch right now (what NAV is built from).
#[freeze_struct("eb63d95e3b7a7bc7")]
#[derive(Decode, Encode, PartialEq, Eq, Clone, Debug, TypeInfo)]
pub struct BasketHolding {
    pub netuid: NetUid,
    pub alpha: AlphaBalance,
    pub spot_tao: TaoBalance,
    pub realizable_tao: TaoBalance,
}

/// Everything an explorer needs to render one validator's beta basket in a single call:
/// valuation (realizable NAV and spot NAV), share supply (share price = `nav_tao / shares`),
/// lifetime flow counters (performance = `(nav + redeemed) / deposited`), the validator's
/// weight vector (strategy), and the per-subnet holdings breakdown.
#[freeze_struct("f467309dda61a20e")]
#[derive(Decode, Encode, PartialEq, Eq, Clone, Debug, TypeInfo)]
pub struct BasketSummary<AccountId: TypeInfo + Encode + Decode> {
    pub hotkey: AccountId,
    /// Realizable (slippage-aware) NAV in TAO — the fund's redemption value.
    pub nav_tao: TaoBalance,
    /// Spot-marked NAV in TAO (`Σ price * alpha`) — the fund's headline mark.
    pub spot_nav_tao: TaoBalance,
    /// Outstanding fund shares `P`.
    pub shares: u64,
    /// Lifetime TAO value deposited into the fund.
    pub deposited_tao: TaoBalance,
    /// Lifetime TAO redeemed (claimed) out of the fund.
    pub redeemed_tao: TaoBalance,
    /// The validator's root weight vector `w` (its curation strategy), exactly as stored.
    pub weights: Vec<(NetUid, u16)>,
    /// Per-subnet holdings, each valued at spot and realizable.
    pub holdings: Vec<BasketHolding>,
}

/// One staker's beta-denominated position in one validator's beta basket. `beta` is the
/// staker's beta token balance — the product-facing number: it never moves with market
/// prices, only grows as dividends accrue and shrinks when the staker claims. Everything
/// else is valuation context: `beta_total` and `nav_tao` give the beta price
/// (`nav_tao / beta_total`, par 1.0 at inception), `value_tao` is what a claim would pay
/// right now (realizable, slippage-aware), and `spot_value_tao` is the same slice marked
/// at spot prices (display only).
#[freeze_struct("b42252207e6e2d4b")]
#[derive(Decode, Encode, PartialEq, Eq, Clone, Debug, TypeInfo)]
pub struct BasketPosition<AccountId: TypeInfo + Encode + Decode> {
    pub hotkey: AccountId,
    /// The staker's owed beta tokens on this validator (capped at `beta_total`).
    pub beta: u64,
    /// Outstanding beta tokens `P` across all stakers of this validator.
    pub beta_total: u64,
    /// Fund NAV `N` in TAO at realizable (slippage-aware) quotes.
    pub nav_tao: TaoBalance,
    /// Realizable TAO a claim would pay now: `min(beta * N / P, N)`.
    pub value_tao: TaoBalance,
    /// The same pro-rata slice marked at spot prices — never used for redemption sizing.
    pub spot_value_tao: TaoBalance,
}

/// One fund's standardized pricing snapshot: the index-spliced prices every consumer
/// (SDK, explorers, EVM) should display, computed on-chain from the fund's frozen
/// [`crate::BetaBaselineOf`] so the whole ecosystem shows the same numbers.
///
/// All prices are **spot** marks (zero-size, comparable across fund sizes) as `U64F64`
/// ratios; see `staking/beta_pricing.rs` for the math. Redemption sizing still uses
/// realizable NAV — none of these fields price a claim.
#[freeze_struct("c61ec6cf57dbb4d3")]
#[derive(Decode, Encode, PartialEq, Eq, Clone, Debug, TypeInfo)]
pub struct BetaPricing<AccountId: TypeInfo + Encode + Decode> {
    pub hotkey: AccountId,
    /// Raw spot beta price: spot NAV over outstanding shares (τ per β).
    pub spot_price: U64F64,
    /// Index-spliced bag price (`spot_price / price_divisor`): mix performance,
    /// comparable across funds of any age. On the bag-index line = market average.
    pub display_price: U64F64,
    /// Total-return stake price: what τ1 staked here at the fund's first sighting is
    /// worth now (principal plus accrued β at today's spot rate), in stake-index units.
    pub stake_price: U64F64,
    /// Period staker yield since first sighting: `(BasketRate - rate0) * spot_price`.
    pub staker_yield: U64F64,
    /// The fund's staker total-return accumulator (see `BasketTwr` storage): the
    /// canonical period-yield series. Staker return over any window is
    /// `twr(t1) / twr(t0) - 1`; sample historically via archive state.
    pub staker_twr: U64F64,
    /// The live bag index level this snapshot was marked against.
    pub bag_index: U64F64,
    /// The live stake (total-return) index level this snapshot was marked against.
    pub stake_index: U64F64,
    /// Block of the fund's baseline stamp; 0 while provisional.
    pub first_block: u64,
    /// True when the fund has no frozen baseline yet: it prices pinned to the current
    /// index levels until its next share mint stamps one.
    pub provisional: bool,
    /// Spot-marked NAV in rao (the fund's index weight).
    pub spot_nav_tao: TaoBalance,
    /// Outstanding fund shares in raw (storage) units.
    pub shares: u64,
    /// Outstanding supply in display units: `shares * price_divisor`. The consistent
    /// public pair: `display_shares * display_price = spot NAV`, exactly as
    /// `shares * spot_price` does in raw units.
    pub display_shares: U64F64,
}

/// One staker's β position in one fund, denominated in the same display units as
/// [`BetaPricing`] — the numbers a wallet should show. `display_beta * display_price`
/// is the position's spot value; `value_tao` is what a claim would actually pay
/// (realizable, slippage-aware).
#[freeze_struct("d936145a80362ce7")]
#[derive(Decode, Encode, PartialEq, Eq, Clone, Debug, TypeInfo)]
pub struct BetaPosition<AccountId: TypeInfo + Encode + Decode> {
    pub hotkey: AccountId,
    /// The staker's owed β in raw (storage) units, capped at the outstanding supply.
    pub beta: u64,
    /// The same position in display units: `beta * price_divisor`.
    pub display_beta: U64F64,
    /// The fund's index-spliced display price (τ per display-β).
    pub display_price: U64F64,
    /// Realizable TAO a claim would pay right now: `min(beta * NAV / supply, NAV)`.
    pub value_tao: TaoBalance,
    /// The same pro-rata slice marked at spot (`display_beta * display_price`) —
    /// display only, never used for redemption sizing.
    pub spot_value_tao: TaoBalance,
    /// True when the fund has no frozen baseline yet (provisional divisor pinned to
    /// the current index level).
    pub provisional: bool,
}

impl<T: Config> Pallet<T> {
    /// Spot-marked TAO value of `alpha` on `netuid`: `current_price * alpha`. Unlike
    /// [`Self::realizable_tao_for_alpha`] this ignores depth/slippage, so it can exceed what a
    /// redemption would fetch — it is exposed for display/analytics only and is never used for
    /// share pricing or redemption sizing.
    pub fn spot_tao_for_alpha(netuid: NetUid, alpha: u64) -> u64 {
        if alpha == 0 {
            return 0;
        }
        // Root holdings are TAO held 1:1; stable-mechanism subnets redeem 1:1.
        if netuid.is_root() || SubnetMechanism::<T>::get(netuid) != 1 {
            return alpha;
        }
        T::SwapInterface::current_alpha_price(netuid.into())
            .saturating_mul(U64F64::saturating_from_num(alpha))
            .saturating_to_num::<u64>()
    }

    /// Full explorer-facing summary of one validator's beta basket. Always returns a summary
    /// (all-zero for a hotkey with no basket).
    pub fn get_validator_basket_summary(hotkey: &T::AccountId) -> BasketSummary<T::AccountId> {
        let mut nav: u64 = 0;
        let mut spot_nav: u64 = 0;
        let holdings: Vec<BasketHolding> = Self::get_basket_holdings(hotkey)
            .into_iter()
            .map(|(netuid, alpha)| {
                let realizable = Self::realizable_tao_for_alpha(netuid, alpha.to_u64());
                let spot = Self::spot_tao_for_alpha(netuid, alpha.to_u64());
                nav = nav.saturating_add(realizable);
                spot_nav = spot_nav.saturating_add(spot);
                BasketHolding {
                    netuid,
                    alpha,
                    spot_tao: spot.into(),
                    realizable_tao: realizable.into(),
                }
            })
            .collect();

        BasketSummary {
            hotkey: hotkey.clone(),
            nav_tao: nav.into(),
            spot_nav_tao: spot_nav.into(),
            shares: BasketShares::<T>::get(hotkey),
            deposited_tao: BasketDepositedTao::<T>::get(hotkey),
            redeemed_tao: BasketRedeemedTao::<T>::get(hotkey),
            weights: Self::get_validator_root_weights(hotkey),
            holdings,
        }
    }

    /// Summaries for every validator with an active basket (outstanding shares), the
    /// network-wide leaderboard in one call.
    pub fn get_all_validator_baskets() -> Vec<BasketSummary<T::AccountId>> {
        BasketShares::<T>::iter_keys()
            .map(|hotkey| Self::get_validator_basket_summary(&hotkey))
            .collect()
    }

    /// One staker's beta-denominated position on one validator, or `None` when the staker
    /// has no owed beta there. Valuation reuses the same primitives as claims
    /// (`get_validator_basket_nav_tao` / `basket_payout_from`), so `value_tao` is exactly
    /// what `claim_root_with_hotkey` would pay right now.
    pub fn get_basket_position(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
    ) -> Option<BasketPosition<T::AccountId>> {
        let beta_total = BasketShares::<T>::get(hotkey);
        let beta = Self::get_basket_owed_shares(hotkey, coldkey).min(beta_total);
        if beta == 0 {
            return None;
        }

        let mut nav: u64 = 0;
        let mut spot_nav: u64 = 0;
        for (netuid, alpha) in Self::get_basket_holdings(hotkey) {
            nav = nav.saturating_add(Self::realizable_tao_for_alpha(netuid, alpha.to_u64()));
            spot_nav = spot_nav.saturating_add(Self::spot_tao_for_alpha(netuid, alpha.to_u64()));
        }

        Some(BasketPosition {
            hotkey: hotkey.clone(),
            beta,
            beta_total,
            nav_tao: nav.into(),
            value_tao: Self::basket_payout_from(beta, nav, beta_total).into(),
            spot_value_tao: Self::mul_div_u64(beta, spot_nav, beta_total)
                .min(spot_nav)
                .into(),
        })
    }

    /// A coldkey's full basket portfolio: one beta-denominated position per validator on
    /// which it has owed beta.
    pub fn get_root_basket_portfolio(coldkey: &T::AccountId) -> Vec<BasketPosition<T::AccountId>> {
        StakingHotkeys::<T>::get(coldkey)
            .into_iter()
            .filter_map(|hotkey| Self::get_basket_position(&hotkey, coldkey))
            .collect()
    }

    /// A staker's basket positions across all its validators: `(hotkey, owed_shares,
    /// payout_tao)` per validator with a non-zero position. `payout_tao` is the realizable
    /// (mark-to-market) TAO a claim would fetch right now.
    pub fn get_root_basket_positions(
        coldkey: &T::AccountId,
    ) -> Vec<(T::AccountId, u64, TaoBalance)> {
        StakingHotkeys::<T>::get(coldkey)
            .into_iter()
            .filter_map(|hotkey| {
                let owed_shares = Self::get_basket_owed_shares(&hotkey, coldkey)
                    .min(BasketShares::<T>::get(&hotkey));
                if owed_shares == 0 {
                    return None;
                }
                let payout = Self::get_basket_payout_tao(&hotkey, coldkey);
                Some((hotkey, owed_shares, payout.into()))
            })
            .collect()
    }
}
