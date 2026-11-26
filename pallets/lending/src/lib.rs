//! Synthetic-mint shorts (SPEC v1.2, shorts only).
//!
//! A short mints alpha into custody and sells it into the pool. The sale
//! proceeds stay locked in the position's custody account beside the owner's
//! TAO collateral and a small refundable deposit. A position may exist only
//! while its funds cover its pro-rata share of buying the whole subnet book
//! back after a hypothetical pump to `Stress_eff × max(EMA, spot, SMA30)`.
//! `Stress_eff` grows with the book relative to pool alpha, which replaces a
//! supply cap.
//!
//! Interest is charged in TAO on `minted × EMA` at a flat APR. Part burns;
//! part funds a per-subnet residual reserve. A position whose median-TWAP
//! equity turns negative, or whose collateral no longer covers a day of
//! interest, deleverages through paced buy-backs shared pro rata across the
//! subnet's deleveraging positions and paused while spot is pumped above the
//! TWAP. If its funds run out first it defaults: the unbought alpha becomes
//! [`ResidualAlpha`] (holder dilution), which the reserve retires over time and
//! which trips a per-subnet breaker on new opens. There is no term, no expiry
//! and no spot liquidation.
//!
//! On deregistration every short settles at the stakers' rate
//! `r = min(T_flag, T_ema) / all_alpha`, except alpha minted while the subnet
//! was in the prune zone, which settles at its own sale proceeds.
#![cfg_attr(not(feature = "std"), no_std)]
#![allow(clippy::too_many_arguments)]

extern crate alloc;
pub use pallet::*;
#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
pub mod dereg;
pub mod migrations;
#[cfg(test)]
mod tests;
pub mod weights;

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use frame_support::{
    PalletId,
    pallet_prelude::*,
    storage::{TransactionOutcome, with_transaction},
    weights::{Weight, WeightMeter},
};
use frame_system::pallet_prelude::*;
use scale_info::TypeInfo;
use sp_runtime::{
    Rounding,
    helpers_128bit::multiply_by_rational_with_rounding,
    traits::{AccountIdConversion, Hash, One, Saturating, Zero},
};
use substrate_fixed::types::U64F64;
use subtensor_macros::freeze_struct;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};
use subtensor_swap_interface::OrderSwapInterface;
use weights::WeightInfo;

type MigrationKeyMaxLen = ConstU32<128>;

/// Pool and custody adapter. Every asset-moving method must be transactional.
pub trait LendingPoolInterface<AccountId>: OrderSwapInterface<AccountId> {
    /// The subnet is live, dynamic and trading (start-call done).
    fn subnet_exists(netuid: NetUid) -> bool;
    /// `owner` may open or grow positions (e.g. no pending coldkey swap).
    fn owner_allowed(_owner: &AccountId) -> bool {
        true
    }
    /// `netuid` is immune from pruning (registration immunity), so it is
    /// never in the prune zone and does not set the zone floor (P16).
    fn prune_immune(_netuid: NetUid) -> bool {
        false
    }
    /// Alpha out for a real buy of `tao`, swap fees included.
    fn quote_buy(netuid: NetUid, tao: TaoBalance) -> Result<AlphaBalance, DispatchError>;
    /// TAO out for a real sale of `alpha`, swap fees included.
    fn quote_sell(netuid: NetUid, alpha: AlphaBalance) -> Result<TaoBalance, DispatchError>;
    /// Live pool TAO reserve (`SubnetTAO`). Short escrow is not in it.
    fn pool_tao(netuid: NetUid) -> TaoBalance;
    /// Live pool alpha reserve (`SubnetAlphaIn`).
    fn pool_alpha(netuid: NetUid) -> AlphaBalance;
    /// Balancer quote (TAO) weight in Perquintill parts.
    fn quote_weight_parts(netuid: NetUid) -> u64;
    /// Fee-free sale quote on hypothetical reserves.
    fn quote_sell_on(
        tao_reserve: TaoBalance,
        alpha_reserve: AlphaBalance,
        quote_weight_parts: u64,
        delta_alpha: AlphaBalance,
    ) -> TaoBalance;
    /// Fee-free buy quote on hypothetical reserves.
    fn quote_buy_on(
        tao_reserve: TaoBalance,
        alpha_reserve: AlphaBalance,
        quote_weight_parts: u64,
        delta_tao: TaoBalance,
    ) -> AlphaBalance;
    /// Raw subnet alpha issuance, synthetic and residual alpha included.
    fn alpha_issuance(netuid: NetUid) -> AlphaBalance;
    /// Slow emission moving price (the SPEC's EMA).
    fn moving_alpha_price(netuid: NetUid) -> U64F64;
    /// Slow EMA (or health TWAP) for notional, minimum size and interest.
    /// Never spot (SPEC P5). Coverage uses `max(EMA, spot, SMA30)` separately.
    fn notional_alpha_price(netuid: NetUid) -> U64F64 {
        Self::moving_alpha_price(netuid)
    }
    /// Non-root live subnets (TWAP sampling and the prune-zone floor).
    fn live_subnets() -> alloc::vec::Vec<NetUid>;
    /// Upper bound on live subnets for weight (matches registration
    /// `SubnetLimit`). Defaults to `u16::MAX` when the host has no limit map.
    fn subnet_limit() -> u64 {
        u64::from(u16::MAX)
    }
    /// Block when `netuid` was registered (tie-break for prune selection).
    fn network_registered_at(_netuid: NetUid) -> u64 {
        0
    }
    /// Registration block when `netuid` is prune-eligible at `now`; `None`
    /// when immune. `now` is the caller's shared block number so the floor
    /// refresh does not re-read System Number per candidate. Implementations
    /// should read `NetworkRegisteredAt` once and reuse it for the tie-break.
    fn prune_eligible_registered_at(netuid: NetUid, now: u64) -> Option<u64> {
        let _ = now;
        if Self::prune_immune(netuid) {
            None
        } else {
            Some(Self::network_registered_at(netuid))
        }
    }

    /// Mint `mint_alpha` onto custody stake and sell it into the pool. The sale
    /// records TAO flow like any other sale. Returns the TAO credited to
    /// `custody_cold`; fails below `min_tao_out`.
    fn mint_and_sell(
        netuid: NetUid,
        custody_cold: &AccountId,
        custody_hot: &AccountId,
        mint_alpha: AlphaBalance,
        min_tao_out: TaoBalance,
    ) -> Result<TaoBalance, DispatchError>;
    /// Buy at least `alpha` from the pool with `custody_cold`'s TAO, spending
    /// at most `max_tao_in`, and burn every bought unit. Returns
    /// `(tao_spent, alpha_burned)`.
    fn buy_and_burn(
        netuid: NetUid,
        custody_cold: &AccountId,
        custody_hot: &AccountId,
        alpha: AlphaBalance,
        max_tao_in: TaoBalance,
    ) -> Result<(TaoBalance, AlphaBalance), DispatchError>;

    /// Burn `amount` TAO from `account` (interest burn share, forfeited
    /// deposits).
    fn burn_interest_tao(account: &AccountId, amount: TaoBalance) -> DispatchResult;
    /// Move interest TAO into the residual reserve. A credit too small to
    /// create `to` may be recycled; returns the amount actually credited.
    fn collect_interest_tao(
        from: &AccountId,
        to: &AccountId,
        amount: TaoBalance,
    ) -> Result<TaoBalance, DispatchError> {
        Self::transfer_tao(from, to, amount)?;
        Ok(amount)
    }
    /// Refund custody TAO to an owner. Same dust rule as
    /// [`Self::collect_interest_tao`].
    fn refund_dissolution_tao(
        from: &AccountId,
        to: &AccountId,
        amount: TaoBalance,
    ) -> Result<TaoBalance, DispatchError> {
        Self::transfer_tao(from, to, amount)?;
        Ok(amount)
    }
    /// Move `recovery` to `vault` and `refund` to `recipient`. Overrides must
    /// withdraw both legs at once so a below-ED remainder cannot be reaped
    /// between them. Returns `(recovered, refunded)` after dust recycling.
    fn terminal_split_tao(
        from: &AccountId,
        vault: &AccountId,
        recipient: &AccountId,
        recovery: TaoBalance,
        refund: TaoBalance,
    ) -> Result<(TaoBalance, TaoBalance), DispatchError> {
        let recovered = if recovery.is_zero() {
            recovery
        } else {
            Self::refund_dissolution_tao(from, vault, recovery)?
        };
        let refunded = if refund.is_zero() {
            refund
        } else {
            Self::refund_dissolution_tao(from, recipient, refund)?
        };
        Ok((recovered, refunded))
    }
    /// [`Self::terminal_split_tao`] with an interest burn leg in the same
    /// withdraw. Returns `(recovered, burned, refunded)`.
    fn terminal_settle_short_tao(
        from: &AccountId,
        vault: &AccountId,
        recipient: &AccountId,
        recovery: TaoBalance,
        interest: TaoBalance,
        refund: TaoBalance,
    ) -> Result<(TaoBalance, TaoBalance, TaoBalance), DispatchError> {
        if !interest.is_zero() {
            Self::burn_interest_tao(from, interest)?;
        }
        let (recovered, refunded) =
            Self::terminal_split_tao(from, vault, recipient, recovery, refund)?;
        Ok((recovered, interest, refunded))
    }
    /// Account holding `netuid`'s dissolution pot. Must resolve after the
    /// subnet is flagged for dissolution.
    fn subnet_pot_account(netuid: NetUid) -> Option<AccountId>;
    /// Book `amount` TAO already on [`Self::subnet_pot_account`] into the pot
    /// (`SubnetTAO`).
    fn credit_pot_reserve(netuid: NetUid, amount: TaoBalance);
    /// Pay `amount` from `from` into `netuid`'s dissolution pot (`SubnetTAO`).
    /// Must work after the subnet is flagged for dissolution.
    fn pay_into_pot(
        from: &AccountId,
        netuid: NetUid,
        amount: TaoBalance,
    ) -> Result<TaoBalance, DispatchError>;

    /// Free (spendable) TAO balance of `who`. Used by `try_state` to verify
    /// that custody accounts hold at least the recorded funds + deposit.
    /// Production implementations should return the free balance; a default
    /// of zero makes the check a no-op for pallets that don't wire it up.
    fn free_tao(_who: &AccountId) -> TaoBalance {
        TaoBalance::ZERO
    }

    /// Benchmark-only: give `netuid` a deep, trading pool whose moving price
    /// equals spot.
    #[cfg(feature = "runtime-benchmarks")]
    fn benchmark_seed_pool(_netuid: NetUid, _tao: u64, _alpha: u64) {}
    /// Benchmark-only: fund free TAO on `who`.
    #[cfg(feature = "runtime-benchmarks")]
    fn benchmark_mint_tao(_who: &AccountId, _amount: u64) {}
}

/// Subtensor-facing hooks.
pub trait LendingInterface<AccountId> {
    /// `(residual reserve coldkey, custody hotkey)` for `netuid`.
    fn custody_accounts(netuid: NetUid) -> Option<(AccountId, AccountId)>;
    fn has_positions(owner: &AccountId) -> bool;
    /// Count `owner`'s positions, stopping at `limit`.
    fn count_positions_bounded(owner: &AccountId, limit: u32) -> u32 {
        let _ = (owner, limit);
        0
    }
    fn max_positions() -> u32;
    /// Re-key positions from `old` to `new` without moving custody funds.
    fn on_coldkey_swap(old: &AccountId, new: &AccountId) -> DispatchResult;
    /// Freeze the subnet's shorts and snapshot the settlement rate `r` (P12).
    /// Call before the pool is touched. Invalidates the prune floor in O(1);
    /// fail-closed zone checks cover opens until the next metered refresh.
    fn start_dissolution(netuid: NetUid) -> DispatchResult;
    /// Block-cached prune target from the metered floor refresh. `None` when
    /// lending is disabled or the cache is missing/stale. Lets signed
    /// registration read the victim in O(1) instead of rescanning ranks.
    fn cached_network_to_prune() -> Option<NetUid> {
        None
    }
    /// Lending ranks are live (`Enabled`). When true, capacity registration
    /// must use [`Self::cached_network_to_prune`] only — a cache miss must not
    /// fall back to emission moving price (that can pick a different victim
    /// after a mid-block cache kill).
    fn lending_ranks_active() -> bool {
        false
    }
    /// O(1) weight for [`Self::start_dissolution`] (Dissolutions insert + rate
    /// reads + floor/target kills). Callers that reach the hook from signed
    /// registration must saturate this onto their dispatch weight; changing
    /// those fees requires explicit fee-baseline approval.
    fn start_dissolution_weight() -> Weight {
        Weight::zero()
    }
    /// Settle every short at `r` (zone alpha at its proceeds) into the staker
    /// pot and add the residual reserve. Runs after the dissolution alpha
    /// count and before stakers are paid. Returns `true` when done.
    fn settle_positions_on_dissolution(netuid: NetUid, meter: &mut WeightMeter) -> bool;
    fn finish_dissolution(netuid: NetUid) -> DispatchResult;
    /// Frozen dissolution rate `r` while a subnet is dissolving, else `None`.
    fn frozen_dissolution_price(netuid: NetUid) -> Option<U64F64> {
        let _ = netuid;
        None
    }
    /// Recompute `dissolution_rate` now and update `Dissolutions.price` in place.
    /// Returns the new price. No-op (returns None) when the subnet is not dissolving.
    /// Call after basket conversion so the frozen rate reflects post-sale pool state.
    fn freeze_dissolution_price(netuid: NetUid) -> Option<U64F64> {
        let _ = netuid;
        None
    }
    /// Weight to reserve before [`Self::freeze_dissolution_price`].
    fn freeze_dissolution_weight() -> Weight {
        Weight::zero()
    }
    /// Weight to reserve before [`Self::finish_dissolution`].
    fn finish_dissolution_weight() -> Weight {
        Weight::zero()
    }
    /// `(hotkey, coldkey)` is lending custody that dissolution payout skips.
    fn is_lending_custody(hotkey: &AccountId, coldkey: &AccountId, netuid: NetUid) -> bool;
    /// `hotkey` is the lending custody hotkey (no third-party stake).
    fn is_custody_hotkey(hotkey: &AccountId) -> bool;
    fn on_subnet_registered(netuid: NetUid) {
        let _ = netuid;
    }
    fn on_subtoken_enabled(netuid: NetUid) {
        let _ = netuid;
    }
    /// Advance the dereg-ranking SMA and the pool-TAO EMA one block (P13, P12).
    fn update_dereg_moving_price(netuid: NetUid) {
        let _ = netuid;
    }
    /// 30-day SMA of raw spot that ranks subnets for pruning (P13). `None`
    /// before the first sample.
    fn dereg_moving_price(netuid: NetUid) -> Option<U64F64> {
        let _ = netuid;
        None
    }
    /// `(synthetic_outstanding, residual_alpha)` on `netuid`. Both are excluded
    /// from the issuance that drives the halving schedule and root proportion.
    fn synthetic_and_residual(netuid: NetUid) -> (u64, u64) {
        let _ = netuid;
        (0, 0)
    }
}

impl<AccountId> LendingInterface<AccountId> for () {
    fn custody_accounts(_: NetUid) -> Option<(AccountId, AccountId)> {
        None
    }
    fn has_positions(_: &AccountId) -> bool {
        false
    }
    fn max_positions() -> u32 {
        0
    }
    fn on_coldkey_swap(_: &AccountId, _: &AccountId) -> DispatchResult {
        Ok(())
    }
    fn start_dissolution(_: NetUid) -> DispatchResult {
        Ok(())
    }
    fn settle_positions_on_dissolution(_: NetUid, _: &mut WeightMeter) -> bool {
        true
    }
    fn finish_dissolution(_: NetUid) -> DispatchResult {
        Ok(())
    }
    fn is_lending_custody(_: &AccountId, _: &AccountId, _: NetUid) -> bool {
        false
    }
    fn is_custody_hotkey(_: &AccountId) -> bool {
        false
    }
}

const BPS: u128 = 10_000;
const WEIGHT_ACCURACY: u128 = 1_000_000_000_000_000_000;
const FIXED_ONE: u128 = 1 << 64;
/// Ring capacity for [`TwapState`]; bounds [`SynthParams::twap_samples`].
pub const MAX_TWAP_SAMPLES: u32 = 168;
/// Minimum health-TWAP samples before `open_short` / `increase_short` (P6).
pub const MIN_HEALTH_TWAP_SAMPLES: u32 = 37;
/// Ring capacity for [`dereg::RankState`]; bounds [`SynthParams::rank_days`].
pub const MAX_RANK_DAYS: u32 = 90;
/// Upper bound on [`SynthParams::apr_bps`] (100%).
pub const MAX_APR_BPS: u32 = 10_000;
/// Bounds on [`SynthParams::stress_bps`] (1.5× – 5×, SPEC P2).
pub const MIN_STRESS_BPS: u32 = 15_000;
pub const MAX_STRESS_BPS: u32 = 50_000;
/// Upper bound on [`SynthParams::stress_slope_bps`] (slope 10).
pub const MAX_STRESS_SLOPE_BPS: u32 = 100_000;
/// Deleverage steps attempted per block across all subnets.
pub const DELEVERAGE_STEPS_PER_BLOCK: u32 = 8;
/// Hard cap on paced buy-backs per block (≤ ~10% of a 4 s block at ~12 ms/step).
pub const MAX_DELEVERAGE_STEPS_PER_BLOCK: u32 = 32;
/// Interest charges attempted in `on_initialize` each block (MEDIUM 7).
pub const MAX_DUE_CHARGES_PER_BLOCK: u32 = 32;
/// `BlockNetMinted` rows cleared per block in `on_finalize` (bounded work).
pub const BLOCK_NET_MINT_CLEAR_PER_BLOCK: u32 = 64;
/// Subnets whose residual alpha the reserve may retire per block.
pub const RESIDUAL_RETIRES_PER_BLOCK: u32 = 4;
/// Empty `Due` buckets skipped per idle pass before yielding.
pub const MAX_EMPTY_DUE_BUMPS: u32 = 64;
/// Fallback upper bound on live subnets when the host pool does not expose
/// [`LendingPoolInterface::subnet_limit`]. Prefer the trait method so weight
/// tracks governance's actual `SubnetLimit` (`u16` max).
pub const MAX_LIVE_SUBNETS_FOR_WEIGHT: u64 = u16::MAX as u64;
/// Rules version stamped on new positions.
pub const TERMS_VERSION: u16 = 2;
/// Bisection depth for fee-free inversions over `u64`.
const SEARCH_STEPS: u32 = 64;

/// Root-settable synthetic-short parameters (SPEC v1.2 §2).
#[freeze_struct("338edfde226128bd")]
#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    TypeInfo,
    MaxEncodedLen,
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
)]
pub struct SynthParams {
    /// Base coverage pump multiple on the reference price (P2).
    pub stress_bps: u32,
    /// Book-aware stress slope: `Stress_eff = stress × (1 + slope × Q / A)`.
    pub stress_slope_bps: u32,
    /// Flat interest rate on `minted × EMA` (P3).
    pub apr_bps: u32,
    /// Interest share sent to the residual reserve; the rest burns (P4).
    pub reserve_share_bps: u16,
    /// Reserve cap as a share of pool TAO; overflow burns (P4).
    pub reserve_cap_bps: u16,
    /// TWAP ring length (P6, read as a median).
    pub twap_samples: u32,
    /// Blocks between TWAP samples (P6).
    pub twap_interval_blocks: u32,
    /// Max price lift per block from deleverage plus reserve retire (P7).
    pub pace_bps_per_block: u16,
    /// Max net alpha minted per block as a share of pool alpha (P8).
    pub open_net_mint_bps: u16,
    /// Minimum `minted × EMA` in rao (P10).
    pub min_notional: u64,
    /// Days in the dereg-ranking SMA (P13).
    pub rank_days: u32,
    /// Blocks per SMA day (P13).
    pub rank_day_blocks: u32,
    /// Time constant of the pool-TAO EMA used for `T_ema` (P12).
    pub tao_ema_blocks: u32,
    /// Prune zone: rank SMA within this multiple of the lowest non-immune
    /// rank SMA (P16).
    pub zone_bps: u32,
    /// Protocol buy-backs pause while spot exceeds this multiple of the
    /// median TWAP (P17).
    pub pause_bps: u32,
    /// Minimum pool TAO for opens and increases (P18).
    pub min_pool_depth: u64,
    /// Refundable per-position deposit, burned on default (P19).
    pub deposit: u64,
    /// Days of interest a withdrawal must leave on top of coverage (P20).
    pub withdraw_buffer_days: u16,
    /// Breaker trips when residual exceeds this share of `Issuance_ex` (P21).
    pub breaker_on_bps: u16,
    /// Breaker clears once residual falls below this share (P21).
    pub breaker_off_bps: u16,
}

impl Default for SynthParams {
    fn default() -> Self {
        Self {
            stress_bps: 30_000,
            stress_slope_bps: 20_000,
            apr_bps: 2_000,
            reserve_share_bps: 5_000,
            reserve_cap_bps: 200,
            twap_samples: 72,
            twap_interval_blocks: 300,
            pace_bps_per_block: 5,
            open_net_mint_bps: 100,
            min_notional: 10_000_000_000,
            rank_days: 30,
            rank_day_blocks: 7_200,
            tao_ema_blocks: 7_200,
            zone_bps: 12_500,
            pause_bps: 12_500,
            min_pool_depth: 0,
            deposit: 100_000_000,
            withdraw_buffer_days: 7,
            breaker_on_bps: 200,
            breaker_off_bps: 100,
        }
    }
}

impl SynthParams {
    pub fn is_valid(&self) -> bool {
        (MIN_STRESS_BPS..=MAX_STRESS_BPS).contains(&self.stress_bps)
            && self.stress_slope_bps <= MAX_STRESS_SLOPE_BPS
            && (1..=MAX_APR_BPS).contains(&self.apr_bps)
            && self.reserve_share_bps <= 10_000
            && self.reserve_cap_bps <= 10_000
            && (1..=MAX_TWAP_SAMPLES).contains(&self.twap_samples)
            && self.twap_interval_blocks >= 1
            && (1..=10_000).contains(&self.pace_bps_per_block)
            && (1..=10_000).contains(&self.open_net_mint_bps)
            && self.min_notional >= 1
            && (1..=MAX_RANK_DAYS).contains(&self.rank_days)
            && self.rank_day_blocks >= 1
            && self.tao_ema_blocks >= 1
            && (10_000..=100_000).contains(&self.zone_bps)
            && (10_000..=100_000).contains(&self.pause_bps)
            && self.withdraw_buffer_days <= 365
            && self.breaker_off_bps <= self.breaker_on_bps
            && self.breaker_on_bps <= 10_000
            && self.min_pool_depth <= 1_000_000_000_000_000
            && self.deposit <= 1_000_000_000_000_000
            && self.twap_interval_blocks <= 1_000_000
            && self.rank_day_blocks <= 1_000_000
            && self.tao_ema_blocks <= 1_000_000
    }
}

#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    TypeInfo,
    MaxEncodedLen,
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
)]
pub enum ShortState<BlockNumber> {
    Open,
    /// Forced close in progress (P5). Never returns to `Open`.
    Deleveraging {
        since: BlockNumber,
    },
}

#[freeze_struct("132ee04bf82d68d9")]
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, Debug, PartialEq, Eq,
)]
pub struct ShortPosition<AccountId, BlockNumber> {
    /// Custody coldkey holding `collateral + proceeds + deposit`. Survives
    /// coldkey swap.
    pub custody: AccountId,
    /// Minted alpha not yet bought back.
    pub minted: u64,
    /// Owner TAO in custody.
    pub collateral: u64,
    /// Locked sale proceeds in custody (P11).
    pub proceeds: u64,
    /// Refundable deposit in custody, outside the funds (P19).
    pub deposit: u64,
    /// Part of `minted` minted while the subnet was in the prune zone (P16).
    pub zone_minted: u64,
    /// Sale proceeds of `zone_minted`; its dereg settlement (P16).
    pub zone_proceeds: u64,
    pub opened_at: BlockNumber,
    pub last_accrued: BlockNumber,
    /// Interest numerator carried between charges (denominator
    /// `BPS × BlocksPerYear`).
    pub interest_remainder: u64,
    /// `Due` bucket holding this position's next interest charge.
    pub due: BlockNumber,
    pub state: ShortState<BlockNumber>,
    pub terms_version: u16,
}

impl<AccountId, BlockNumber> ShortPosition<AccountId, BlockNumber> {
    /// Collateral plus proceeds: what coverage, health and buy-backs use.
    pub fn funds(&self) -> u64 {
        self.collateral.saturating_add(self.proceeds)
    }

    /// Spend `amount` from proceeds first, then collateral.
    fn debit(&mut self, amount: u64) {
        let from_proceeds = amount.min(self.proceeds);
        self.proceeds = self.proceeds.saturating_sub(from_proceeds);
        self.collateral = self
            .collateral
            .saturating_sub(amount.saturating_sub(from_proceeds));
    }

    /// Drop `alpha` from `minted`, shrinking the zone tranche pro rata.
    fn reduce_minted(&mut self, alpha: u64) {
        let before = self.minted;
        let alpha = alpha.min(before);
        self.minted = before.saturating_sub(alpha);
        if before == 0 || self.zone_minted == 0 {
            return;
        }
        let keep = |v: u64| {
            multiply_by_rational_with_rounding(
                u128::from(v),
                u128::from(self.minted),
                u128::from(before),
                Rounding::Down,
            )
            .and_then(|v| u64::try_from(v).ok())
            .unwrap_or(0)
        };
        self.zone_minted = keep(self.zone_minted);
        self.zone_proceeds = keep(self.zone_proceeds);
    }
}

/// Spot samples for the health TWAP (P6).
#[freeze_struct("25904ca8bf3e6337")]
#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    TypeInfo,
    MaxEncodedLen,
    Clone,
    Debug,
    Default,
    PartialEq,
    Eq,
)]
pub struct TwapState<BlockNumber> {
    pub samples: BoundedVec<U64F64, ConstU32<MAX_TWAP_SAMPLES>>,
    /// Slot the next sample overwrites once the ring is full.
    pub cursor: u32,
    pub last_sample: BlockNumber,
    /// Spot prices summed over the open TWAP interval (P6).
    pub interval_sum: U64F64,
    /// Blocks counted into `interval_sum` for the open interval.
    pub interval_blocks: u32,
}

#[freeze_struct("8dab4693cc022346")]
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, Debug, PartialEq, Eq,
)]
pub struct Dissolution<BlockNumber> {
    pub frozen_at: BlockNumber,
    /// `r = min(T_flag, T_ema) / all_alpha` at flag (P12).
    pub price: U64F64,
}

#[freeze_struct("a717f90000059031")]
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, Debug, PartialEq, Eq,
)]
pub struct ShortOpenQuote {
    /// Sale proceeds, swap fees included.
    pub proceeds: u64,
    /// Smallest collateral that satisfies coverage and one day of interest.
    pub required_collateral: u64,
    /// Deposit charged on top of the collateral (P19).
    pub deposit: u64,
    /// `mint × EMA`.
    pub notional: u64,
    pub apr_bps: u32,
    pub annual_interest: u64,
    /// The mint would be tracked as prune-zone alpha (P16).
    pub in_prune_zone: bool,
}

#[freeze_struct("fec72bec798b2db3")]
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, Debug, PartialEq, Eq,
)]
pub struct ShortCloseQuote {
    /// Alpha bought back (clamped to the position).
    pub alpha: u64,
    /// TAO the buy-back costs, swap fees included.
    pub tao_in: u64,
    /// TAO returned to the owner on a full close, deposit included (zero on a
    /// partial close).
    pub refund: u64,
}

#[freeze_struct("32124cc913a5c62a")]
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, Debug, PartialEq, Eq,
)]
pub struct ShortHealth {
    pub minted: u64,
    pub collateral: u64,
    pub proceeds: u64,
    pub zone_minted: u64,
    /// Interest accrued since the last charge.
    pub accrued_interest: u64,
    /// `C + P − BuyCost(q) × TWAP / spot` with the median TWAP.
    pub equity_twap: i128,
    /// This position's share of the whole-book stressed buy-back; `None`
    /// when the pumped pool cannot supply the book.
    pub coverage_cost: Option<u64>,
    pub day_interest: u64,
    pub deleveraging: bool,
}

/// Closed custody that still holds TAO after a failed settle or default.
/// Sweep pays `due` into the pot and refunds `refund` to `owner`.
#[freeze_struct("cab344b74979e531")]
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, Debug, PartialEq, Eq,
)]
pub struct StrandedRecord<AccountId> {
    pub owner: AccountId,
    /// TAO owed to the subnet pot (dissolution due). Zero on a failed default.
    pub due: u64,
    /// TAO refundable to `owner` (`funds − due + deposit` at failure).
    pub refund: u64,
}

/// Reserves plus Balancer quote weight, for hypothetical pricing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PoolState {
    tao: u64,
    alpha: u64,
    weight: u64,
}

#[frame_support::pallet]
#[allow(clippy::expect_used)]
pub mod pallet {
    use super::*;

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    #[pallet::config]
    pub trait Config: frame_system::Config {
        type Pool: LendingPoolInterface<Self::AccountId>;
        #[pallet::constant]
        type PalletId: Get<PalletId>;
        /// Blocks between interest charges (P15).
        #[pallet::constant]
        type InterestPeriod: Get<BlockNumberFor<Self>>;
        #[pallet::constant]
        type BlocksPerYear: Get<u64>;
        #[pallet::constant]
        type MaxPositionsPerSubnet: Get<u32>;
        #[pallet::constant]
        type MaxTotalPositions: Get<u32>;
        /// Blocks before retrying an interest charge that rolled back.
        #[pallet::constant]
        type InterestRetryDelay: Get<BlockNumberFor<Self>>;
        type WeightInfo: WeightInfo;
    }

    #[pallet::storage]
    pub type Enabled<T> = StorageValue<_, bool, ValueQuery>;
    #[pallet::storage]
    pub type SynthParamsStore<T> = StorageValue<_, SynthParams, ValueQuery>;
    #[pallet::storage]
    pub type Positions<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        Identity,
        NetUid,
        ShortPosition<T::AccountId, BlockNumberFor<T>>,
        OptionQuery,
    >;
    #[pallet::storage]
    pub type OpenByNetuid<T: Config> =
        StorageDoubleMap<_, Identity, NetUid, Blake2_128Concat, T::AccountId, (), OptionQuery>;
    #[pallet::storage]
    pub type PositionCount<T> = StorageMap<_, Identity, NetUid, u32, ValueQuery>;
    #[pallet::storage]
    pub type TotalPositions<T> = StorageValue<_, u32, ValueQuery>;
    /// Custody coldkey → owner.
    #[pallet::storage]
    pub type EscrowOwner<T: Config> = StorageDoubleMap<
        _,
        Identity,
        NetUid,
        Blake2_128Concat,
        T::AccountId,
        T::AccountId,
        OptionQuery,
    >;
    /// Σ `minted` over the subnet's positions (I1).
    #[pallet::storage]
    pub type SyntheticOutstanding<T> = StorageMap<_, Identity, NetUid, u64, ValueQuery>;
    /// Alpha left in circulation by defaulted shorts.
    #[pallet::storage]
    pub type ResidualAlpha<T> = StorageMap<_, Identity, NetUid, u64, ValueQuery>;
    /// TAO held by [`Pallet::reserve_account`] for retiring residual alpha.
    #[pallet::storage]
    pub type ResidualReserve<T> = StorageMap<_, Identity, NetUid, u64, ValueQuery>;
    /// Dilution breaker latched on (P21).
    #[pallet::storage]
    pub type BreakerTripped<T> = StorageMap<_, Identity, NetUid, bool, ValueQuery>;
    /// Net alpha minted on the subnet in the tagged block (P8). Cleared in
    /// `on_finalize`; a stale tag reads as zero.
    #[pallet::storage]
    pub type BlockNetMinted<T: Config> =
        StorageMap<_, Identity, NetUid, (BlockNumberFor<T>, i128), OptionQuery>;
    #[pallet::storage]
    pub type TwapRing<T: Config> =
        StorageMap<_, Identity, NetUid, TwapState<BlockNumberFor<T>>, ValueQuery>;
    /// Daily means behind the dereg-ranking SMA (P13).
    #[pallet::storage]
    pub type RankRing<T: Config> = StorageMap<_, Identity, NetUid, dereg::RankState, OptionQuery>;
    /// Current dereg-ranking SMA; changes once per SMA day.
    #[pallet::storage]
    pub type RankPrice<T: Config> = StorageMap<_, Identity, NetUid, U64F64, OptionQuery>;
    /// Bumped on each re-enable so stale rings are ignored without an
    /// O(subnets) clear (weight stays inside the block ceiling).
    #[pallet::storage]
    pub type RankGeneration<T: Config> = StorageValue<_, u32, ValueQuery>;
    /// Generation under which this subnet's ring / price were last written.
    #[pallet::storage]
    pub type RankRingGeneration<T: Config> = StorageMap<_, Identity, NetUid, u32, ValueQuery>;
    /// EMA of pool TAO (`T_ema`, P12).
    #[pallet::storage]
    pub type PoolTaoEma<T: Config> = StorageMap<_, Identity, NetUid, u64, OptionQuery>;
    /// Uncapped slow EMA of spot for notional / interest / min size (P5).
    /// Separate from the emission [`SubnetMovingPrice`] which caps the feed at 1.0.
    #[pallet::storage]
    pub type NotionalEma<T: Config> = StorageMap<_, Identity, NetUid, U64F64, OptionQuery>;
    /// Lowest non-immune prune rank for the tagged block (P16). Tuple is
    /// `(block, RankGeneration, floor)`. Refreshed in `on_initialize` and on
    /// mid-block enable so signed opens/increases pay O(1) for the zone check.
    #[pallet::storage]
    pub type PruneFloor<T: Config> = StorageValue<_, (BlockNumberFor<T>, u32, U64F64), OptionQuery>;
    /// Winning prune candidate for the tagged block (same scan as
    /// [`PruneFloor`]). Tuple is `(block, RankGeneration, netuid)`. Signed
    /// registration reads this in O(1) instead of rescanning ranks.
    #[pallet::storage]
    pub type PruneTarget<T: Config> =
        StorageValue<_, (BlockNumberFor<T>, u32, NetUid), OptionQuery>;
    /// Closed custody coldkeys that still hold TAO after a failed settle or
    /// default. Root [`sweep_stranded_funds`] pays `due` to the pot and
    /// refunds the owner portion.
    #[pallet::storage]
    pub type StrandedCustody<T: Config> = StorageDoubleMap<
        _,
        Identity,
        NetUid,
        Blake2_128Concat,
        T::AccountId,
        StrandedRecord<T::AccountId>,
        OptionQuery,
    >;
    /// Positions in [`ShortState::Deleveraging`], by subnet. Value is the
    /// monotonic admission sequence used for O(1) cohort checks (I9).
    #[pallet::storage]
    pub type DeleverageQueue<T: Config> =
        StorageDoubleMap<_, Identity, NetUid, Blake2_128Concat, T::AccountId, u64, OptionQuery>;
    /// Next admission sequence for [`DeleverageQueue`] on each subnet.
    #[pallet::storage]
    pub type NextDeleverageSeq<T> = StorageMap<_, Identity, NetUid, u64, ValueQuery>;
    /// Σ `minted` over the subnet's deleveraging positions.
    #[pallet::storage]
    pub type DeleveragingMinted<T> = StorageMap<_, Identity, NetUid, u64, ValueQuery>;
    /// The block's pace budget, deleveraging book, and admission cutoff on the
    /// subnet, fixed at its first deleverage step so fills are pro rata and
    /// late joins wait until the next block (I9). Tuple is
    /// `(at, budget, book, cutoff_seq)`.
    #[pallet::storage]
    pub type DeleverageBudget<T: Config> =
        StorageMap<_, Identity, NetUid, (BlockNumberFor<T>, u64, u64, u64), OptionQuery>;
    /// Per-position deleverage fill this block: locked pro-rata allocation and
    /// TAO already spent against it. Stops repeated `poke`s from taking more
    /// than the position's share of the block's pace budget.
    #[pallet::storage]
    pub type DeleverageFilled<T: Config> = StorageDoubleMap<
        _,
        Identity,
        NetUid,
        Blake2_128Concat,
        T::AccountId,
        (BlockNumberFor<T>, u64, u64),
        OptionQuery,
    >;
    /// Last queue key served; the next pass resumes after it.
    #[pallet::storage]
    pub type DeleverageCursor<T: Config> = StorageValue<_, (NetUid, T::AccountId), OptionQuery>;
    /// Last subnet whose residual the reserve retired.
    #[pallet::storage]
    pub type RetireCursor<T> = StorageValue<_, NetUid, OptionQuery>;
    /// Positions due an interest charge at each block.
    #[pallet::storage]
    pub type Due<T: Config> = StorageDoubleMap<
        _,
        Twox64Concat,
        BlockNumberFor<T>,
        Blake2_128Concat,
        (T::AccountId, NetUid),
        (),
        OptionQuery,
    >;
    /// Earliest `Due` bucket not yet drained.
    #[pallet::storage]
    pub type NextDue<T: Config> = StorageValue<_, BlockNumberFor<T>, OptionQuery>;
    /// Interest that accrued but could not be collected from collateral (I5).
    #[pallet::storage]
    pub type UncollectedInterest<T: Config> =
        StorageDoubleMap<_, Blake2_128Concat, T::AccountId, Identity, NetUid, u64, ValueQuery>;
    /// Spot before the first protocol buy-back of the block (P7 budget).
    #[pallet::storage]
    pub type PaceAnchor<T: Config> =
        StorageMap<_, Identity, NetUid, (BlockNumberFor<T>, U64F64), OptionQuery>;
    #[pallet::storage]
    pub type Dissolutions<T: Config> =
        StorageMap<_, Identity, NetUid, Dissolution<BlockNumberFor<T>>, OptionQuery>;
    #[pallet::storage]
    pub type PalletHotkey<T: Config> = StorageValue<_, T::AccountId, OptionQuery>;
    /// One-shot migration markers.
    #[pallet::storage]
    pub type HasMigrationRun<T: Config> =
        StorageMap<_, Identity, BoundedVec<u8, MigrationKeyMaxLen>, bool, ValueQuery>;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        ShortOpened {
            who: T::AccountId,
            netuid: NetUid,
            minted: u64,
            proceeds: u64,
            collateral: u64,
        },
        /// Deltas added by `increase_short`.
        ShortIncreased {
            who: T::AccountId,
            netuid: NetUid,
            minted: u64,
            proceeds: u64,
            collateral: u64,
        },
        CollateralAdded {
            who: T::AccountId,
            netuid: NetUid,
            amount: u64,
        },
        CollateralWithdrawn {
            who: T::AccountId,
            netuid: NetUid,
            amount: u64,
        },
        /// `refund` is non-zero only when the position is fully closed.
        ShortClosed {
            who: T::AccountId,
            netuid: NetUid,
            burned: u64,
            tao_spent: u64,
            refund: u64,
        },
        InterestCharged {
            who: T::AccountId,
            netuid: NetUid,
            burned: u64,
            reserved: u64,
        },
        /// Accrued interest that collateral could not cover this charge.
        InterestUncollected {
            who: T::AccountId,
            netuid: NetUid,
            amount: u64,
            cumulative: u64,
        },
        DeleverageStarted {
            who: T::AccountId,
            netuid: NetUid,
            equity_twap: i128,
        },
        DeleverageStep {
            who: T::AccountId,
            netuid: NetUid,
            burned: u64,
            tao_spent: u64,
        },
        ShortDefaulted {
            who: T::AccountId,
            netuid: NetUid,
            residual_alpha: u64,
        },
        ResidualRetired {
            netuid: NetUid,
            alpha: u64,
            tao: u64,
        },
        ShortSettledOnDissolution {
            who: T::AccountId,
            netuid: NetUid,
            price: U64F64,
            paid: u64,
        },
        /// Dissolution settle failed for `who`; funds stay on custody, position removed.
        ShortSettleFailed {
            who: T::AccountId,
            netuid: NetUid,
            funds: u64,
            deposit: u64,
        },
        /// Reserve pay-into-pot failed; TAO stays on reserve account, book entry cleared.
        ReservePayFailed {
            netuid: NetUid,
            amount: u64,
        },
        /// Default best-effort burns failed; position still removed.
        ShortDefaultFailed {
            who: T::AccountId,
            netuid: NetUid,
        },
        DilutionBreaker {
            netuid: NetUid,
            tripped: bool,
        },
        SynthParamsSet {
            params: SynthParams,
        },
        EnabledSet {
            enabled: bool,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        Disabled,
        SubnetNotEligible,
        /// Health TWAP has fewer than [`MIN_HEALTH_TWAP_SAMPLES`] (P6).
        YoungSubnet,
        BelowMinNotional,
        /// Funds miss the position's whole-book share, or the pumped pool
        /// cannot supply the book.
        CoverageNotMet,
        /// Sale below `min_tao_out`, or `min_tao_out` of zero.
        SlippageExceeded,
        /// Net alpha minted this block would exceed `open_net_mint_bps` (P8).
        OpenImpactTooHigh,
        NoPosition,
        PositionDeleveraging,
        WithdrawBreaksCoverage,
        DissolutionInProgress,
        InvalidParams,
        CustodyUnavailable,
        Overflow,
        /// Residual alpha above the breaker threshold (P21).
        DilutionBreakerActive,
        /// Pool TAO below `min_pool_depth` (P18).
        PoolTooShallow,
        /// Withdrawal leaves less than the interest buffer (P20).
        WithdrawBufferBreached,
        /// `open_short` on an existing position; use `increase_short`.
        PositionExists,
        TooManyPositions,
        ZeroAmount,
        /// Custody funds cannot pay for the requested buy-back or withdrawal.
        InsufficientFunds,
        /// The owner is mid coldkey swap.
        OwnerNotAllowed,
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        fn integrity_test() {
            assert!(T::BlocksPerYear::get() >= 365);
            assert!(T::InterestPeriod::get() > Zero::zero());
            assert!(T::InterestRetryDelay::get() > Zero::zero());
            assert!(T::MaxTotalPositions::get() > 0);
            assert!(SynthParams::default().is_valid());
        }

        fn on_initialize(now: BlockNumberFor<T>) -> Weight {
            let params = SynthParamsStore::<T>::get();
            let db = T::DbWeight::get();
            // Params read plus bounded `on_finalize` `BlockNetMinted` clearing.
            let mut weight = db.reads_writes(2, 1).saturating_add(db.reads_writes(
                u64::from(BLOCK_NET_MINT_CLEAR_PER_BLOCK),
                u64::from(BLOCK_NET_MINT_CLEAR_PER_BLOCK),
            ));
            let interval = params.twap_interval_blocks.max(1);
            let close_interval = Self::block_u64(now)
                .checked_rem(u64::from(interval))
                .is_some_and(|r| r == 0);
            let subnets = T::Pool::live_subnets();
            // Enumeration scans `NetworksAdded`; each dynamic-pool tick reads
            // mechanism + both reserves + balancer + TwapRing and writes the
            // ring. Charge a fixed CPU/proof envelope for ring processing even
            // on non-rollover ticks (interval mean / sample push).
            weight = weight.saturating_add(db.reads(subnets.len().saturating_add(1) as u64));
            // Proof covers a full TWAP ring read/write (~168 samples).
            let twap_proof = TwapState::<BlockNumberFor<T>>::max_encoded_len() as u64;
            let twap_tick_w = db
                .reads_writes(5, 1)
                .saturating_add(Weight::from_parts(25_000_000, twap_proof));
            for netuid in subnets.iter() {
                Self::twap_tick(*netuid, now, &params, close_interval);
                weight = weight.saturating_add(twap_tick_w);
            }
            // Publish this block's prune-zone floor from the same live set so
            // open/increase stay O(1). Skip while disabled (opens cannot run).
            if Enabled::<T>::get() {
                Self::refresh_prune_floor(now, &subnets);
                weight =
                    weight.saturating_add(Self::prune_floor_refresh_weight(subnets.len() as u64));
            }
            if close_interval {
                weight = weight.saturating_add(
                    T::WeightInfo::twap_sample().saturating_mul(subnets.len() as u64),
                );
            }
            // Meter deleverage against ≤10% of max block weight with a hard
            // step cap. Skips no-progress entries and stops after a full pass
            // with no progress (H2 / MEDIUM 1). When `BlockWeights` is unset
            // (unit tests with `()`), fall back to the hard cap alone.
            let step_w = T::WeightInfo::deleverage_step();
            let max_block = <T as frame_system::Config>::BlockWeights::get().max_block;
            let ten_pct =
                Weight::from_parts(max_block.ref_time() / 10, max_block.proof_size() / 10);
            let hard_cap = step_w.saturating_mul(u64::from(MAX_DELEVERAGE_STEPS_PER_BLOCK));
            let deleverage_budget = if ten_pct.ref_time() == 0 {
                hard_cap
            } else {
                ten_pct.min(hard_cap)
            };
            let mut deleverage_meter = WeightMeter::with_limit(deleverage_budget);
            let _steps = Self::exhaust_deleverage_queue(&params, &mut deleverage_meter);
            let retires = Self::retire_residuals(RESIDUAL_RETIRES_PER_BLOCK, &params);
            // Empty residual scan still reads the cursor / map head and may
            // clear the cursor when the map ends.
            let residual_w = if retires == 0 {
                db.reads_writes(2, 1)
            } else {
                T::WeightInfo::retire_residual().saturating_mul(u64::from(retires))
            };
            let due_budget = T::WeightInfo::charge_interest()
                .saturating_mul(u64::from(MAX_DUE_CHARGES_PER_BLOCK))
                .saturating_add(T::DbWeight::get().reads_writes(u64::from(MAX_EMPTY_DUE_BUMPS), 0));
            let mut due_meter = WeightMeter::with_limit(due_budget);
            Self::collect_due(now, &mut due_meter);
            weight
                .saturating_add(deleverage_meter.consumed())
                .saturating_add(residual_w)
                .saturating_add(due_meter.consumed())
        }

        fn on_finalize(_now: BlockNumberFor<T>) {
            let limit = BLOCK_NET_MINT_CLEAR_PER_BLOCK;
            let mut cleared = 0u32;
            for (netuid, _) in BlockNetMinted::<T>::iter().take(limit as usize) {
                BlockNetMinted::<T>::remove(netuid);
                cleared = cleared.saturating_add(1);
            }
            let _ = cleared;
        }

        fn on_idle(now: BlockNumberFor<T>, remaining: Weight) -> Weight {
            let mut meter = WeightMeter::with_limit(remaining);
            Self::collect_due(now, &mut meter);
            meter.consumed()
        }

        #[cfg(feature = "try-runtime")]
        fn try_state(_n: BlockNumberFor<T>) -> Result<(), sp_runtime::TryRuntimeError> {
            Self::do_try_state()
        }
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Mint `mint_alpha`, sell it, and lock the proceeds beside
        /// `collateral` TAO and the deposit from the caller. `min_tao_out`
        /// must be non-zero.
        #[pallet::call_index(0)]
        #[pallet::weight({
            // O(1): this block's cached [`PruneFloor`] plus this subnet's
            // rank/moving/immune. Floor refresh is paid in `on_initialize`.
            let proof = (U64F64::max_encoded_len() as u64).saturating_mul(2);
            T::WeightInfo::open_short().saturating_add(
                T::DbWeight::get()
                    .reads(4)
                    .saturating_add(Weight::from_parts(0, proof)),
            )
        })]
        #[frame_support::transactional]
        pub fn open_short(
            origin: OriginFor<T>,
            netuid: NetUid,
            mint_alpha: u64,
            collateral: u64,
            min_tao_out: u64,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::do_open(who, netuid, mint_alpha, collateral, min_tao_out)
        }

        /// Mint and sell `extra_mint` more and/or add `extra_collateral`.
        /// `min_tao_out` must be non-zero when `extra_mint` is.
        #[pallet::call_index(1)]
        #[pallet::weight({
            let proof = (U64F64::max_encoded_len() as u64).saturating_mul(2);
            T::WeightInfo::increase_short().saturating_add(
                T::DbWeight::get()
                    .reads(4)
                    .saturating_add(Weight::from_parts(0, proof)),
            )
        })]
        #[frame_support::transactional]
        pub fn increase_short(
            origin: OriginFor<T>,
            netuid: NetUid,
            extra_mint: u64,
            extra_collateral: u64,
            min_tao_out: u64,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::do_increase(who, netuid, extra_mint, extra_collateral, min_tao_out)
        }

        /// Add TAO collateral. Works while disabled and while deleveraging.
        #[pallet::call_index(2)]
        #[pallet::weight(T::WeightInfo::add_collateral())]
        #[frame_support::transactional]
        pub fn add_collateral(origin: OriginFor<T>, netuid: NetUid, tao: u64) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::do_add_collateral(who, netuid, tao)
        }

        /// Withdraw collateral while coverage plus the interest buffer still
        /// hold afterwards (P20).
        #[pallet::call_index(3)]
        #[pallet::weight(T::WeightInfo::withdraw_collateral())]
        #[frame_support::transactional]
        pub fn withdraw_collateral(
            origin: OriginFor<T>,
            netuid: NetUid,
            tao: u64,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::do_withdraw(who, netuid, tao)
        }

        /// Buy back and burn `alpha` (clamped to the position), paying from
        /// proceeds then collateral. Closing the remainder refunds what is left
        /// and the deposit.
        #[pallet::call_index(4)]
        #[pallet::weight(T::WeightInfo::close_short())]
        #[frame_support::transactional]
        pub fn close_short(
            origin: OriginFor<T>,
            netuid: NetUid,
            alpha: u64,
            max_tao_in: u64,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::do_close(who, netuid, alpha, max_tao_in)
        }

        /// Permissionless health check: charge accrued interest, start
        /// deleveraging an unhealthy position, or run one deleverage step.
        #[pallet::call_index(5)]
        #[pallet::weight(T::WeightInfo::poke())]
        #[frame_support::transactional]
        pub fn poke(origin: OriginFor<T>, who: T::AccountId, netuid: NetUid) -> DispatchResult {
            ensure_signed(origin)?;
            Self::do_poke(who, netuid)
        }

        #[pallet::call_index(6)]
        #[pallet::weight({
            // Constant-time: never enumerate NetworksAdded during weight
            // validation (that can run before `ensure_root`). Floor republish
            // is deferred to the metered `on_initialize` refresh; this call
            // only flips the flag, bumps generation, and kills caches.
            T::WeightInfo::set_enabled()
                .saturating_add(T::DbWeight::get().reads_writes(2, 4))
        })]
        pub fn set_enabled(origin: OriginFor<T>, enabled: bool) -> DispatchResult {
            ensure_root(origin)?;
            let was = Enabled::<T>::get();
            Enabled::<T>::put(enabled);
            // Re-enable invalidates ranking by generation so rings warm from
            // real samples without an O(subnets) ring sweep. Kill the floor /
            // target so same-block opens fail closed until the next metered
            // `on_initialize` refresh (no uncharged live-set scan here).
            if enabled && !was {
                RankGeneration::<T>::mutate(|g| *g = g.saturating_add(1));
                PruneFloor::<T>::kill();
                PruneTarget::<T>::kill();
            }
            if !enabled {
                PruneFloor::<T>::kill();
                PruneTarget::<T>::kill();
            }
            Self::deposit_event(Event::EnabledSet { enabled });
            Ok(())
        }

        #[pallet::call_index(7)]
        #[pallet::weight(T::WeightInfo::set_synth_params())]
        pub fn set_synth_params(origin: OriginFor<T>, params: SynthParams) -> DispatchResult {
            ensure_root(origin)?;
            ensure!(params.is_valid(), Error::<T>::InvalidParams);
            SynthParamsStore::<T>::put(params);
            Self::deposit_event(Event::SynthParamsSet { params });
            Ok(())
        }

        /// Root-only recovery for TAO stranded on a custody coldkey or residual
        /// reserve after a failed settle/default transfer. Pays into the
        /// subnet pot when one exists; otherwise burns.
        #[pallet::call_index(8)]
        #[pallet::weight(T::WeightInfo::sweep_stranded())]
        pub fn sweep_stranded_funds(
            origin: OriginFor<T>,
            netuid: NetUid,
            who: T::AccountId,
        ) -> DispatchResult {
            ensure_root(origin)?;
            Self::do_sweep_stranded(netuid, &who)
        }
    }

    type PositionOf<T> = ShortPosition<<T as frame_system::Config>::AccountId, BlockNumberFor<T>>;

    impl<T: Config> Pallet<T> {
        // ---- accounts -------------------------------------------------------

        /// Per-subnet residual reserve coldkey.
        pub fn reserve_account(netuid: NetUid) -> T::AccountId {
            T::PalletId::get().into_sub_account_truncating((b"rsrv", netuid))
        }

        /// Custody coldkey of a position opened by `owner` at `opened_at`.
        /// The block keeps a re-opened position off a custody account that a
        /// coldkey swap moved to another owner.
        pub fn position_account(
            owner: &T::AccountId,
            netuid: NetUid,
            opened_at: BlockNumberFor<T>,
        ) -> T::AccountId {
            let key = T::Hashing::hash_of(&(b"short", owner, netuid, opened_at));
            T::PalletId::get().into_sub_account_truncating(key)
        }

        pub fn initialize_custody() -> DispatchResult {
            if PalletHotkey::<T>::exists() {
                return Ok(());
            }
            let coldkey: T::AccountId = T::PalletId::get().into_account_truncating();
            let key =
                T::Hashing::hash_of(&(b"lending/hotkey", frame_system::Pallet::<T>::parent_hash()));
            let hotkey = T::PalletId::get().into_sub_account_truncating(key);
            T::Pool::register_pallet_hotkey(&coldkey, &hotkey)?;
            ensure!(
                T::Pool::pallet_hotkey_registered(&coldkey, &hotkey),
                Error::<T>::CustodyUnavailable
            );
            PalletHotkey::<T>::put(hotkey);
            Ok(())
        }

        pub fn custody_hotkey() -> Result<T::AccountId, DispatchError> {
            PalletHotkey::<T>::get().ok_or(Error::<T>::CustodyUnavailable.into())
        }

        pub fn is_custody_hotkey(hotkey: &T::AccountId) -> bool {
            PalletHotkey::<T>::get().is_some_and(|h| h == *hotkey)
        }

        // ---- gates ----------------------------------------------------------

        fn ensure_not_dissolving(netuid: NetUid) -> DispatchResult {
            ensure!(
                !Dissolutions::<T>::contains_key(netuid),
                Error::<T>::DissolutionInProgress
            );
            Ok(())
        }

        fn ensure_tradeable(netuid: NetUid) -> DispatchResult {
            Self::ensure_not_dissolving(netuid)?;
            ensure!(
                T::Pool::subnet_exists(netuid),
                Error::<T>::SubnetNotEligible
            );
            Ok(())
        }

        /// Gates shared by every mint: enabled, live subnet, owner, pool depth
        /// (P18) and the dilution breaker (P21).
        fn ensure_can_grow(
            owner: &T::AccountId,
            netuid: NetUid,
            params: &SynthParams,
        ) -> DispatchResult {
            ensure!(Enabled::<T>::get(), Error::<T>::Disabled);
            Self::ensure_tradeable(netuid)?;
            ensure!(
                TwapRing::<T>::get(netuid).samples.len() as u32 >= MIN_HEALTH_TWAP_SAMPLES,
                Error::<T>::YoungSubnet
            );
            ensure!(T::Pool::owner_allowed(owner), Error::<T>::OwnerNotAllowed);
            ensure!(
                T::Pool::pool_tao(netuid).to_u64() >= params.min_pool_depth,
                Error::<T>::PoolTooShallow
            );
            Self::refresh_breaker(netuid, params);
            ensure!(
                !BreakerTripped::<T>::get(netuid),
                Error::<T>::DilutionBreakerActive
            );
            Ok(())
        }

        fn position(owner: &T::AccountId, netuid: NetUid) -> Result<PositionOf<T>, DispatchError> {
            Positions::<T>::get(owner, netuid).ok_or(Error::<T>::NoPosition.into())
        }

        fn ensure_open_state(pos: &PositionOf<T>) -> DispatchResult {
            ensure!(
                pos.state == ShortState::Open,
                Error::<T>::PositionDeleveraging
            );
            Ok(())
        }

        fn ensure_min_notional(
            netuid: NetUid,
            minted: u64,
            params: &SynthParams,
        ) -> DispatchResult {
            ensure!(
                Self::notional(netuid, minted) >= params.min_notional,
                Error::<T>::BelowMinNotional
            );
            Ok(())
        }

        // ---- P8 net-mint limit ---------------------------------------------

        pub fn block_net_minted(netuid: NetUid) -> i128 {
            let now = frame_system::Pallet::<T>::block_number();
            match BlockNetMinted::<T>::get(netuid) {
                Some((at, net)) if at == now => net,
                _ => 0,
            }
        }

        fn add_net_minted(netuid: NetUid, delta: i128) {
            let now = frame_system::Pallet::<T>::block_number();
            let net = Self::block_net_minted(netuid).saturating_add(delta);
            BlockNetMinted::<T>::insert(netuid, (now, net));
        }

        fn ensure_net_mint_room(netuid: NetUid, mint: u64, params: &SynthParams) -> DispatchResult {
            let limit = Self::mul_bps(
                T::Pool::pool_alpha(netuid).to_u64(),
                params.open_net_mint_bps,
            );
            let after = Self::block_net_minted(netuid).saturating_add(i128::from(mint));
            ensure!(after <= i128::from(limit), Error::<T>::OpenImpactTooHigh);
            Ok(())
        }

        // ---- P21 dilution breaker --------------------------------------------

        /// `ResidualAlpha / Issuance_ex` in bps.
        pub fn residual_share_bps(netuid: NetUid) -> u128 {
            let residual = u128::from(ResidualAlpha::<T>::get(netuid));
            if residual == 0 {
                return 0;
            }
            residual
                .saturating_mul(BPS)
                .checked_div(Self::issuance_ex(netuid))
                .unwrap_or(u128::MAX)
        }

        /// Latch the breaker on above `breaker_on_bps` and off below
        /// `breaker_off_bps`.
        pub(crate) fn refresh_breaker(netuid: NetUid, params: &SynthParams) {
            let share = Self::residual_share_bps(netuid);
            let tripped = BreakerTripped::<T>::get(netuid);
            let next = if tripped {
                share >= u128::from(params.breaker_off_bps)
            } else {
                share > u128::from(params.breaker_on_bps)
            };
            if next != tripped {
                if next {
                    BreakerTripped::<T>::insert(netuid, true);
                } else {
                    BreakerTripped::<T>::remove(netuid);
                }
                Self::deposit_event(Event::DilutionBreaker {
                    netuid,
                    tripped: next,
                });
            }
        }

        // ---- P16 prune zone -------------------------------------------------------

        /// Published ranking SMA for coverage (`max(EMA, spot, SMA)`). Zero
        /// when the ring has not published a full window so coverage does not
        /// treat an absent SMA as a floor of zero.
        pub fn rank_price(netuid: NetUid) -> U64F64 {
            Self::active_rank_price(netuid).unwrap_or_default()
        }

        /// Published [`RankPrice`] only when the ring's generation matches
        /// `epoch` (re-enable invalidates without an O(n) clear). Callers that
        /// already hold [`RankGeneration`] pass it so the floor refresh does
        /// not re-read the shared value per candidate.
        pub(crate) fn active_rank_price_at(netuid: NetUid, epoch: u32) -> Option<U64F64> {
            if RankRingGeneration::<T>::get(netuid) != epoch {
                return None;
            }
            RankPrice::<T>::get(netuid)
        }

        /// Published [`RankPrice`] only when the ring's generation matches
        /// [`RankGeneration`] (re-enable invalidates without an O(n) clear).
        pub(crate) fn active_rank_price(netuid: NetUid) -> Option<U64F64> {
            Self::active_rank_price_at(netuid, RankGeneration::<T>::get())
        }

        /// Same fallback as subtensor's [`dereg_rank_price`]: published
        /// [`RankPrice`], else the emission moving price. Used for prune-zone
        /// so unwarm rings cannot drag the floor to zero.
        pub fn prune_rank_price(netuid: NetUid) -> U64F64 {
            Self::active_rank_price(netuid).unwrap_or_else(|| T::Pool::moving_alpha_price(netuid))
        }

        /// Floor-refresh price using a shared `epoch` (avoids per-candidate
        /// [`RankGeneration`] re-reads).
        fn prune_rank_price_at(netuid: NetUid, epoch: u32) -> U64F64 {
            Self::active_rank_price_at(netuid, epoch)
                .unwrap_or_else(|| T::Pool::moving_alpha_price(netuid))
        }

        /// Test helper: republish [`PruneFloor`] for the current block.
        #[cfg(test)]
        pub(crate) fn refresh_prune_floor_for_test() {
            let now = frame_system::Pallet::<T>::block_number();
            Self::refresh_prune_floor(now, &T::Pool::live_subnets());
        }

        /// Worst-case weight to refresh the prune floor over `n` live subnets
        /// (warm-generation moving-price fallback + trie witness per key).
        pub fn prune_floor_refresh_weight(n: u64) -> Weight {
            let db = T::DbWeight::get();
            // Shared once per refresh: RankGeneration + System Number.
            // Per-candidate warm-miss path: NetworkRegisteredAt +
            // NetworkImmunityPeriod + Dissolutions + RankRingGeneration +
            // RankPrice-miss + SubnetMechanism + SubnetMovingPrice = 7.
            //
            // PoV: MaxEncodedLen(value) + one encoded trie path per key.
            // 3 KiB bounds a deep compact-encoded branch path (child hashes
            // + key nibbles), not a 64 B flat guess.
            //
            // Ref-time: DB reads/writes plus a conservative per-candidate
            // CPU envelope for eligibility, price compare, and winner
            // selection (same order as a TWAP tick body).
            const TRIE_PATH_PER_KEY: u64 = 3072;
            const KEYS_PER_SUBNET: u64 = 7;
            const SHARED_READS: u64 = 2; // RankGeneration + System Number
            const PER_SUBNET_CPU: u64 = 25_000_000;
            let per_subnet_value = (u64::max_encoded_len() as u64) // NetworkRegisteredAt
                .saturating_add(u64::max_encoded_len() as u64) // NetworkImmunityPeriod
                .saturating_add(Dissolution::<BlockNumberFor<T>>::max_encoded_len() as u64)
                .saturating_add(u32::max_encoded_len() as u64) // RankRingGeneration
                .saturating_add(U64F64::max_encoded_len() as u64) // RankPrice
                .saturating_add(u16::max_encoded_len() as u64) // SubnetMechanism
                .saturating_add(U64F64::max_encoded_len() as u64); // SubnetMovingPrice
            let per_subnet_proof =
                per_subnet_value.saturating_add(TRIE_PATH_PER_KEY.saturating_mul(KEYS_PER_SUBNET));
            let shared_value =
                (u32::max_encoded_len() as u64) // RankGeneration
                    .saturating_add(BlockNumberFor::<T>::max_encoded_len() as u64); // Number
            // Replace path: read existing floor/target values, then write new
            // (populated-cache worst case).
            let cache_value = (BlockNumberFor::<T>::max_encoded_len() as u64)
                .saturating_add(u32::max_encoded_len() as u64)
                .saturating_add(U64F64::max_encoded_len() as u64)
                .saturating_add(NetUid::max_encoded_len() as u64);
            let cache_proof = cache_value
                .saturating_mul(2) // old + new encoding
                .saturating_add(TRIE_PATH_PER_KEY.saturating_mul(2));
            let proof = n
                .saturating_mul(per_subnet_proof)
                .saturating_add(shared_value)
                .saturating_add(TRIE_PATH_PER_KEY.saturating_mul(SHARED_READS))
                .saturating_add(cache_proof);
            db.reads(
                n.saturating_mul(KEYS_PER_SUBNET)
                    .saturating_add(SHARED_READS)
                    .saturating_add(2), // existing floor/target for replace
            )
            .saturating_add(db.writes(2))
            .saturating_add(Weight::from_parts(PER_SUBNET_CPU.saturating_mul(n), proof))
        }

        /// Publish this block's lowest non-immune, non-dissolving prune rank
        /// and the winning netuid (earliest registration on price ties) for
        /// O(1) zone checks and O(1) registration prune selection. Dissolving
        /// nets are excluded so the cache stays coherent with dereg eligibility.
        fn refresh_prune_floor(now: BlockNumberFor<T>, subnets: &[NetUid]) {
            let epoch = RankGeneration::<T>::get();
            // Shared block number for immunity checks (one System Number read).
            let now_u64 = TryInto::<u64>::try_into(now).ok().unwrap_or(u64::MAX);
            let mut best: Option<(NetUid, U64F64, u64)> = None;
            for &netuid in subnets {
                let Some(registered_at) = T::Pool::prune_eligible_registered_at(netuid, now_u64)
                else {
                    continue;
                };
                if Dissolutions::<T>::contains_key(netuid) {
                    continue;
                }
                let price = Self::prune_rank_price_at(netuid, epoch);
                best = Some(match best {
                    Some((cur, lo, ts)) if price > lo || (price == lo && registered_at >= ts) => {
                        (cur, lo, ts)
                    }
                    _ => (netuid, price, registered_at),
                });
            }
            if let Some((netuid, floor, _)) = best {
                PruneFloor::<T>::put((now, epoch, floor));
                PruneTarget::<T>::put((now, epoch, netuid));
            } else {
                PruneFloor::<T>::kill();
                PruneTarget::<T>::kill();
            }
        }

        /// Fresh cached prune victim for this block and [`RankGeneration`].
        pub fn cached_network_to_prune() -> Option<NetUid> {
            if !Enabled::<T>::get() {
                return None;
            }
            let now = frame_system::Pallet::<T>::block_number();
            let epoch = RankGeneration::<T>::get();
            let (at, target_epoch, netuid) = PruneTarget::<T>::get()?;
            if at != now || target_epoch != epoch {
                return None;
            }
            // Dissolving nets must not be re-selected from a stale mid-block cache.
            if Dissolutions::<T>::contains_key(netuid) || T::Pool::prune_immune(netuid) {
                return None;
            }
            Some(netuid)
        }

        /// The subnet's prune rank is within `zone_bps` of the lowest
        /// non-immune prune rank, so it is a prune candidate (P16). Uses the
        /// block's cached floor from `on_initialize`. Missing or stale floor
        /// (wrong block, RankGeneration, or mid-block after enable/dissolve
        /// kill) fails closed: treat as in-zone so prune candidates cannot
        /// open unmarked.
        pub fn in_prune_zone(netuid: NetUid, params: &SynthParams) -> bool {
            if T::Pool::prune_immune(netuid) {
                return false;
            }
            let own = Self::prune_rank_price(netuid);
            let now = frame_system::Pallet::<T>::block_number();
            let epoch = RankGeneration::<T>::get();
            let Some((at, floor_epoch, floor)) = PruneFloor::<T>::get() else {
                return true;
            };
            if at != now || floor_epoch != epoch {
                return true;
            }
            own <= Self::scale_price(floor, u128::from(params.zone_bps))
        }

        // ---- extrinsic bodies -----------------------------------------------

        fn do_open(
            owner: T::AccountId,
            netuid: NetUid,
            mint: u64,
            collateral: u64,
            min_tao_out: u64,
        ) -> DispatchResult {
            let params = SynthParamsStore::<T>::get();
            Self::ensure_can_grow(&owner, netuid, &params)?;
            ensure!(
                !Positions::<T>::contains_key(&owner, netuid),
                Error::<T>::PositionExists
            );
            ensure!(mint > 0, Error::<T>::ZeroAmount);
            ensure!(
                TotalPositions::<T>::get() < T::MaxTotalPositions::get(),
                Error::<T>::TooManyPositions
            );
            Self::ensure_min_notional(netuid, mint, &params)?;
            let in_zone = Self::in_prune_zone(netuid, &params);
            Self::initialize_custody()?;
            let hot = Self::custody_hotkey()?;
            let now = frame_system::Pallet::<T>::block_number();
            let custody = Self::position_account(&owner, netuid, now);
            ensure!(
                !EscrowOwner::<T>::contains_key(netuid, &custody),
                Error::<T>::PositionExists
            );
            let escrow = collateral
                .checked_add(params.deposit)
                .ok_or(Error::<T>::Overflow)?;
            if escrow > 0 {
                T::Pool::transfer_tao(&owner, &custody, escrow.into())?;
            }
            let proceeds = Self::sell_minted(netuid, &custody, &hot, mint, min_tao_out, &params)?;
            let due = now.saturating_add(T::InterestPeriod::get());
            let pos = ShortPosition {
                custody: custody.clone(),
                minted: mint,
                collateral,
                proceeds,
                deposit: params.deposit,
                zone_minted: if in_zone { mint } else { 0 },
                zone_proceeds: if in_zone { proceeds } else { 0 },
                opened_at: now,
                last_accrued: now,
                interest_remainder: 0,
                due,
                state: ShortState::Open,
                terms_version: TERMS_VERSION,
            };
            Self::ensure_coverage(netuid, &pos, &params, Error::<T>::CoverageNotMet)?;
            Positions::<T>::insert(&owner, netuid, &pos);
            OpenByNetuid::<T>::insert(netuid, &owner, ());
            EscrowOwner::<T>::insert(netuid, &custody, &owner);
            PositionCount::<T>::mutate(netuid, |n| *n = n.saturating_add(1));
            TotalPositions::<T>::mutate(|n| *n = n.saturating_add(1));
            Self::schedule_due(&owner, netuid, due);
            Self::deposit_event(Event::ShortOpened {
                who: owner,
                netuid,
                minted: mint,
                proceeds,
                collateral,
            });
            Ok(())
        }

        fn do_increase(
            owner: T::AccountId,
            netuid: NetUid,
            extra_mint: u64,
            extra_collateral: u64,
            min_tao_out: u64,
        ) -> DispatchResult {
            let params = SynthParamsStore::<T>::get();
            Self::ensure_can_grow(&owner, netuid, &params)?;
            ensure!(
                extra_mint > 0 || extra_collateral > 0,
                Error::<T>::ZeroAmount
            );
            let mut pos = Self::position(&owner, netuid)?;
            Self::ensure_open_state(&pos)?;
            Self::charge_interest(&owner, netuid, &mut pos, &params)?;
            if extra_collateral > 0 {
                T::Pool::transfer_tao(&owner, &pos.custody, extra_collateral.into())?;
                pos.collateral = pos.collateral.saturating_add(extra_collateral);
            }
            let mut proceeds = 0;
            if extra_mint > 0 {
                let minted = pos
                    .minted
                    .checked_add(extra_mint)
                    .ok_or(Error::<T>::Overflow)?;
                Self::ensure_min_notional(netuid, minted, &params)?;
                let in_zone = Self::in_prune_zone(netuid, &params);
                let hot = Self::custody_hotkey()?;
                proceeds = Self::sell_minted(
                    netuid,
                    &pos.custody,
                    &hot,
                    extra_mint,
                    min_tao_out,
                    &params,
                )?;
                pos.minted = minted;
                pos.proceeds = pos.proceeds.saturating_add(proceeds);
                if in_zone {
                    pos.zone_minted = pos.zone_minted.saturating_add(extra_mint);
                    pos.zone_proceeds = pos.zone_proceeds.saturating_add(proceeds);
                }
            }
            Self::ensure_coverage(netuid, &pos, &params, Error::<T>::CoverageNotMet)?;
            Positions::<T>::insert(&owner, netuid, &pos);
            Self::deposit_event(Event::ShortIncreased {
                who: owner,
                netuid,
                minted: extra_mint,
                proceeds,
                collateral: extra_collateral,
            });
            Ok(())
        }

        fn do_add_collateral(owner: T::AccountId, netuid: NetUid, tao: u64) -> DispatchResult {
            Self::ensure_not_dissolving(netuid)?;
            ensure!(tao > 0, Error::<T>::ZeroAmount);
            let mut pos = Self::position(&owner, netuid)?;
            T::Pool::transfer_tao(&owner, &pos.custody, tao.into())?;
            pos.collateral = pos.collateral.saturating_add(tao);
            Positions::<T>::insert(&owner, netuid, &pos);
            Self::deposit_event(Event::CollateralAdded {
                who: owner,
                netuid,
                amount: tao,
            });
            Ok(())
        }

        fn do_withdraw(owner: T::AccountId, netuid: NetUid, tao: u64) -> DispatchResult {
            Self::ensure_tradeable(netuid)?;
            ensure!(tao > 0, Error::<T>::ZeroAmount);
            let mut pos = Self::position(&owner, netuid)?;
            Self::ensure_open_state(&pos)?;
            let params = SynthParamsStore::<T>::get();
            Self::charge_interest(&owner, netuid, &mut pos, &params)?;
            ensure!(tao <= pos.collateral, Error::<T>::InsufficientFunds);
            pos.collateral = pos.collateral.saturating_sub(tao);
            Self::ensure_coverage(netuid, &pos, &params, Error::<T>::WithdrawBreaksCoverage)?;
            let buffer = Self::days_interest(
                netuid,
                pos.minted,
                u64::from(params.withdraw_buffer_days),
                &params,
            );
            let need = Self::coverage_cost(netuid, pos.minted, &params).unwrap_or(u64::MAX);
            ensure!(
                pos.collateral >= buffer && pos.funds() >= need.saturating_add(buffer),
                Error::<T>::WithdrawBufferBreached
            );
            T::Pool::transfer_tao(&pos.custody, &owner, tao.into())?;
            Positions::<T>::insert(&owner, netuid, &pos);
            Self::deposit_event(Event::CollateralWithdrawn {
                who: owner,
                netuid,
                amount: tao,
            });
            Ok(())
        }

        fn do_close(
            owner: T::AccountId,
            netuid: NetUid,
            alpha: u64,
            max_tao_in: u64,
        ) -> DispatchResult {
            Self::ensure_tradeable(netuid)?;
            ensure!(alpha > 0, Error::<T>::ZeroAmount);
            let mut pos = Self::position(&owner, netuid)?;
            let params = SynthParamsStore::<T>::get();
            Self::charge_interest(&owner, netuid, &mut pos, &params)?;
            let alpha = alpha.min(pos.minted);
            let funds = pos.funds();
            let estimate =
                Self::buy_cost_fee_inclusive(netuid, alpha).ok_or(Error::<T>::SlippageExceeded)?;
            ensure!(estimate <= funds, Error::<T>::InsufficientFunds);
            ensure!(estimate <= max_tao_in, Error::<T>::SlippageExceeded);
            let (spent, burned) =
                Self::buy_back(netuid, &pos.custody, alpha, max_tao_in.min(funds))?;
            pos.debit(spent);
            Self::burn_from_position(netuid, &mut pos, burned, true);
            let refund = if pos.minted == 0 {
                Self::finish_position(&owner, netuid, &mut pos)?
            } else {
                if pos.state == ShortState::Open {
                    Self::ensure_min_notional(netuid, pos.minted, &params)?;
                    Self::ensure_coverage(netuid, &pos, &params, Error::<T>::CoverageNotMet)?;
                }
                Positions::<T>::insert(&owner, netuid, &pos);
                0
            };
            Self::deposit_event(Event::ShortClosed {
                who: owner,
                netuid,
                burned,
                tao_spent: spent,
                refund,
            });
            Ok(())
        }

        fn do_poke(owner: T::AccountId, netuid: NetUid) -> DispatchResult {
            Self::ensure_tradeable(netuid)?;
            let mut pos = Self::position(&owner, netuid)?;
            let params = SynthParamsStore::<T>::get();
            Self::charge_interest(&owner, netuid, &mut pos, &params)?;
            match pos.state {
                ShortState::Open => {
                    Self::start_deleverage_if_unhealthy(&owner, netuid, &mut pos, &params);
                    Positions::<T>::insert(&owner, netuid, &pos);
                }
                ShortState::Deleveraging { .. } => {
                    Positions::<T>::insert(&owner, netuid, &pos);
                    Self::deleverage_step(&owner, netuid, &params);
                }
            }
            Ok(())
        }

        // ---- pool operations ------------------------------------------------

        /// Mint and sell into the pool, enforcing mandatory slippage and the
        /// per-block net-mint limit (P8). Records the mint in
        /// `SyntheticOutstanding`.
        fn sell_minted(
            netuid: NetUid,
            custody: &T::AccountId,
            hot: &T::AccountId,
            mint: u64,
            min_tao_out: u64,
            params: &SynthParams,
        ) -> Result<u64, DispatchError> {
            ensure!(min_tao_out > 0, Error::<T>::SlippageExceeded);
            Self::ensure_net_mint_room(netuid, mint, params)?;
            let tao = T::Pool::mint_and_sell(netuid, custody, hot, mint.into(), TaoBalance::ZERO)?
                .to_u64();
            ensure!(tao >= min_tao_out, Error::<T>::SlippageExceeded);
            SyntheticOutstanding::<T>::mutate(netuid, |s| *s = s.saturating_add(mint));
            Self::add_net_minted(netuid, i128::from(mint));
            Ok(tao)
        }

        fn buy_back(
            netuid: NetUid,
            payer: &T::AccountId,
            alpha: u64,
            max_tao_in: u64,
        ) -> Result<(u64, u64), DispatchError> {
            let hot = Self::custody_hotkey()?;
            let (tao, burned) =
                T::Pool::buy_and_burn(netuid, payer, &hot, alpha.into(), max_tao_in.into())?;
            Ok((tao.to_u64(), burned.to_u64()))
        }

        /// Record `alpha` of `pos` bought back and burned.
        /// Caps the credited burn at `pos.minted` so stale callers cannot
        /// over-subtract `SyntheticOutstanding` or `DeleveragingMinted`.
        fn burn_from_position(
            netuid: NetUid,
            pos: &mut PositionOf<T>,
            alpha: u64,
            track_net_mint: bool,
        ) {
            let burned = alpha.min(pos.minted);
            pos.reduce_minted(burned);
            SyntheticOutstanding::<T>::mutate(netuid, |s| *s = s.saturating_sub(burned));
            if pos.state != ShortState::Open {
                DeleveragingMinted::<T>::mutate(netuid, |d| *d = d.saturating_sub(burned));
            }
            if track_net_mint {
                Self::add_net_minted(netuid, i128::from(burned).saturating_neg());
            }
        }

        /// TAO a protocol buy-back may still spend on `netuid` this block
        /// without lifting spot more than `pace` above the block's first
        /// buy-back (P7).
        fn pace_room(netuid: NetUid, params: &SynthParams) -> u64 {
            let now = frame_system::Pallet::<T>::block_number();
            let anchor = match PaceAnchor::<T>::get(netuid) {
                Some((at, price)) if at == now => price,
                _ => {
                    let spot = T::Pool::current_alpha_price(netuid);
                    PaceAnchor::<T>::insert(netuid, (now, spot));
                    spot
                }
            };
            let target = Self::scale_price(
                anchor,
                BPS.saturating_add(u128::from(params.pace_bps_per_block)),
            );
            let pool = Self::pool_state(netuid);
            Self::pump_to(pool, target)
                .map(|after| after.tao.saturating_sub(pool.tao))
                .unwrap_or(0)
        }

        /// Protocol buy-backs wait while spot is pumped above the median TWAP
        /// (P17).
        pub fn buybacks_paused(netuid: NetUid, params: &SynthParams) -> bool {
            let cap = Self::scale_price(Self::twap(netuid), u128::from(params.pause_bps));
            T::Pool::current_alpha_price(netuid) > cap
        }

        /// The block's pace budget and deleveraging book on `netuid`, fixed at
        /// the first call in the block. Snapshots the admission cutoff so
        /// later joins wait until the next block (I9) at constant cost.
        fn deleverage_budget(netuid: NetUid, params: &SynthParams) -> (u64, u64, u64) {
            let now = frame_system::Pallet::<T>::block_number();
            if let Some((at, budget, book, cutoff)) = DeleverageBudget::<T>::get(netuid)
                && at == now
            {
                return (budget, book, cutoff);
            }
            let budget = Self::pace_room(netuid, params);
            let book = DeleveragingMinted::<T>::get(netuid);
            let cutoff = NextDeleverageSeq::<T>::get(netuid);
            DeleverageBudget::<T>::insert(netuid, (now, budget, book, cutoff));
            (budget, book, cutoff)
        }

        /// `true` when `owner` was admitted at or before this block's cutoff.
        fn in_deleverage_cohort(netuid: NetUid, owner: &T::AccountId, cutoff: u64) -> bool {
            matches!(DeleverageQueue::<T>::get(netuid, owner), Some(seq) if seq <= cutoff)
        }

        // ---- position lifecycle --------------------------------------------

        /// Burn any accrued-but-uncollected interest from proceeds before the
        /// position closes. Best-effort: errors are silently ignored.
        #[cfg(test)]
        pub(crate) fn drain_uncollected_interest_for_test(
            owner: &T::AccountId,
            netuid: NetUid,
            pos: &mut PositionOf<T>,
        ) {
            Self::drain_uncollected_interest(owner, netuid, pos);
        }

        fn drain_uncollected_interest(
            owner: &T::AccountId,
            netuid: NetUid,
            pos: &mut PositionOf<T>,
        ) {
            let owed = UncollectedInterest::<T>::get(owner, netuid);
            if owed == 0 {
                return;
            }
            let available = pos.funds();
            let to_burn = owed.min(available);
            if to_burn == 0 {
                return;
            }
            // Only debit the position when the burn actually lands.
            if T::Pool::burn_interest_tao(&pos.custody, to_burn.into()).is_err() {
                return;
            }
            let from_proceeds = to_burn.min(pos.proceeds);
            pos.proceeds = pos.proceeds.saturating_sub(from_proceeds);
            pos.collateral = pos
                .collateral
                .saturating_sub(to_burn.saturating_sub(from_proceeds));
            UncollectedInterest::<T>::mutate(owner, netuid, |u| {
                *u = u.saturating_sub(to_burn);
            });
        }

        /// Refund remaining funds plus the deposit and drop every index.
        fn finish_position(
            owner: &T::AccountId,
            netuid: NetUid,
            pos: &mut PositionOf<T>,
        ) -> Result<u64, DispatchError> {
            Self::drain_uncollected_interest(owner, netuid, pos);
            let held = pos.funds().saturating_add(pos.deposit);
            let refund = if held > 0 {
                T::Pool::refund_dissolution_tao(&pos.custody, owner, held.into())?.to_u64()
            } else {
                0
            };
            Self::remove_position(owner, netuid, pos);
            Ok(refund)
        }

        fn remove_position(owner: &T::AccountId, netuid: NetUid, pos: &PositionOf<T>) {
            Positions::<T>::remove(owner, netuid);
            OpenByNetuid::<T>::remove(netuid, owner);
            EscrowOwner::<T>::remove(netuid, &pos.custody);
            Due::<T>::remove(pos.due, (owner.clone(), netuid));
            if pos.state != ShortState::Open {
                DeleverageQueue::<T>::remove(netuid, owner);
                DeleveragingMinted::<T>::mutate(netuid, |d| *d = d.saturating_sub(pos.minted));
            }
            DeleverageFilled::<T>::remove(netuid, owner);
            UncollectedInterest::<T>::remove(owner, netuid);
            PositionCount::<T>::mutate(netuid, |n| *n = n.saturating_sub(1));
            TotalPositions::<T>::mutate(|n| *n = n.saturating_sub(1));
        }

        pub(crate) fn start_deleverage_if_unhealthy(
            owner: &T::AccountId,
            netuid: NetUid,
            pos: &mut PositionOf<T>,
            params: &SynthParams,
        ) {
            if pos.state != ShortState::Open || !T::Pool::subnet_exists(netuid) {
                return;
            }
            let equity_twap = Self::equity_twap(netuid, pos);
            if equity_twap >= 0 && pos.collateral >= Self::day_interest(netuid, pos.minted, params)
            {
                return;
            }
            pos.state = ShortState::Deleveraging {
                since: frame_system::Pallet::<T>::block_number(),
            };
            let seq = NextDeleverageSeq::<T>::mutate(netuid, |s| {
                *s = s.saturating_add(1);
                *s
            });
            DeleverageQueue::<T>::insert(netuid, owner, seq);
            DeleveragingMinted::<T>::mutate(netuid, |d| *d = d.saturating_add(pos.minted));
            Self::deposit_event(Event::DeleverageStarted {
                who: owner.clone(),
                netuid,
                equity_twap,
            });
        }

        /// One paced buy-back for a deleveraging position: its pro-rata share
        /// of the block's pace budget, skipped while buy-backs are paused.
        /// Closes the position when the minted alpha is gone and defaults it
        /// when its funds are. Repeated `poke`s in the same block cannot spend
        /// more than that locked share.
        /// Returns whether the queue entry made state progress (buy-back,
        /// default, close, or cleanup).
        pub(crate) fn deleverage_step(
            owner: &T::AccountId,
            netuid: NetUid,
            params: &SynthParams,
        ) -> bool {
            let Some(mut pos) = Positions::<T>::get(owner, netuid) else {
                DeleverageQueue::<T>::remove(netuid, owner);
                return true;
            };
            if Dissolutions::<T>::contains_key(netuid)
                || !T::Pool::subnet_exists(netuid)
                || Self::buybacks_paused(netuid, params)
            {
                return false;
            }
            let funds = pos.funds();
            let (budget, book, cutoff) = Self::deleverage_budget(netuid, params);
            // Positions admitted after the cutoff wait until the next block.
            if !Self::in_deleverage_cohort(netuid, owner, cutoff) {
                return false;
            }
            let share = if book == 0 {
                budget
            } else {
                multiply_by_rational_with_rounding(
                    u128::from(budget),
                    u128::from(pos.minted),
                    u128::from(book),
                    Rounding::Down,
                )
                .and_then(|v| u64::try_from(v).ok())
                .unwrap_or(budget)
            };
            let now = frame_system::Pallet::<T>::block_number();
            let (allocation, filled) = match DeleverageFilled::<T>::get(netuid, owner) {
                Some((at, alloc, spent)) if at == now => (alloc, spent),
                _ => (share, 0),
            };
            let remaining = allocation.saturating_sub(filled);
            let spend = remaining.min(funds).min(Self::pace_room(netuid, params));
            let alpha = if spend == 0 {
                0
            } else {
                T::Pool::quote_buy(netuid, spend.into())
                    .map(|a| a.to_u64())
                    .unwrap_or(0)
                    .min(pos.minted)
            };
            if alpha == 0 {
                let dust = funds == 0
                    || T::Pool::quote_buy(netuid, funds.into()).is_ok_and(|a| a.to_u64() == 0);
                if dust {
                    Self::default_position(owner, netuid, &pos);
                    return true;
                }
                return false;
            }
            let bought =
                with_transaction(
                    || match Self::buy_back(netuid, &pos.custody, alpha, spend) {
                        Ok((spent, burned)) => TransactionOutcome::Commit(Ok((spent, burned))),
                        Err(e) => TransactionOutcome::Rollback(Err(e)),
                    },
                );
            let Ok((spent, burned)) = bought else {
                return false;
            };
            DeleverageFilled::<T>::insert(
                netuid,
                owner,
                (now, allocation, filled.saturating_add(spent)),
            );
            pos.debit(spent);
            Self::burn_from_position(netuid, &mut pos, burned, false);
            Self::deposit_event(Event::DeleverageStep {
                who: owner.clone(),
                netuid,
                burned,
                tao_spent: spent,
            });
            if pos.minted == 0 {
                let Ok(refund) = Self::finish_position(owner, netuid, &mut pos) else {
                    Positions::<T>::insert(owner, netuid, &pos);
                    return true;
                };
                Self::deposit_event(Event::ShortClosed {
                    who: owner.clone(),
                    netuid,
                    burned,
                    tao_spent: spent,
                    refund,
                });
            } else if pos.funds() == 0 {
                Self::default_position(owner, netuid, &pos);
            } else {
                Positions::<T>::insert(owner, netuid, &pos);
            }
            true
        }

        /// Unbought alpha becomes residual (holder dilution). Leftover dust
        /// funds go to the reserve that retires it; the deposit burns (P19).
        pub(crate) fn default_position(owner: &T::AccountId, netuid: NetUid, pos: &PositionOf<T>) {
            let params = SynthParamsStore::<T>::get();
            let residual = pos.minted;
            let dust = pos.funds();
            let mut burns_ok = true;
            // Dust that failed to reach the reserve becomes pot-due on strand.
            let mut pot_due = 0u64;
            if dust > 0 {
                let reserve = Self::reserve_account(netuid);
                let cap = Self::mul_bps(T::Pool::pool_tao(netuid).to_u64(), params.reserve_cap_bps);
                let room = cap.saturating_sub(ResidualReserve::<T>::get(netuid));
                let to_reserve = dust.min(room);
                let credited = if to_reserve > 0 {
                    match T::Pool::collect_interest_tao(&pos.custody, &reserve, to_reserve.into()) {
                        Ok(credited) => credited.to_u64(),
                        Err(_) => {
                            burns_ok = false;
                            pot_due = to_reserve;
                            0
                        }
                    }
                } else {
                    0
                };
                ResidualReserve::<T>::mutate(netuid, |r| *r = r.saturating_add(credited));
                let overflow = dust.saturating_sub(credited);
                if overflow > 0
                    && T::Pool::burn_interest_tao(&pos.custody, overflow.into()).is_err()
                {
                    // Overflow stays on the burn path (not owner refund / not pot).
                    burns_ok = false;
                }
            }
            if pos.deposit > 0
                && T::Pool::burn_interest_tao(&pos.custody, pos.deposit.into()).is_err()
            {
                burns_ok = false;
            }
            // Always remove the position and advance the queue regardless of burn outcomes.
            SyntheticOutstanding::<T>::mutate(netuid, |s| *s = s.saturating_sub(residual));
            ResidualAlpha::<T>::mutate(netuid, |r| *r = r.saturating_add(residual));
            if !burns_ok {
                // Deposit and overflow follow the burn path on sweep; only
                // failed reserve dust is pot-due. Owner refund stays 0 (P19).
                let left = T::Pool::free_tao(&pos.custody).to_u64();
                if left > 0 {
                    let due = pot_due.min(left);
                    StrandedCustody::<T>::insert(
                        netuid,
                        &pos.custody,
                        StrandedRecord {
                            owner: owner.clone(),
                            due,
                            refund: 0,
                        },
                    );
                }
            }
            Self::remove_position(owner, netuid, pos);
            Self::refresh_breaker(netuid, &SynthParamsStore::<T>::get());
            if burns_ok {
                Self::deposit_event(Event::ShortDefaulted {
                    who: owner.clone(),
                    netuid,
                    residual_alpha: residual,
                });
            } else {
                Self::deposit_event(Event::ShortDefaultFailed {
                    who: owner.clone(),
                    netuid,
                });
            }
        }

        /// Next queue key after the cursor, or the first key when wrapping /
        /// starting. `None` only when the queue is empty.
        fn peek_next_deleverage_key() -> Option<(NetUid, T::AccountId)> {
            let after_cursor = match DeleverageCursor::<T>::get() {
                Some((netuid, owner)) => DeleverageQueue::<T>::iter_keys_from(
                    DeleverageQueue::<T>::hashed_key_for(netuid, &owner),
                )
                .next(),
                None => None,
            };
            after_cursor.or_else(|| DeleverageQueue::<T>::iter_keys().next())
        }

        /// Walk up to [`MAX_DELEVERAGE_STEPS_PER_BLOCK`] keys of `from`'s
        /// subnet. Reserves the cursor write before the page and each
        /// iterator advance (including the boundary / exhaustion probe)
        /// before `next()`, with proof for visited keys. Returns `Ok(true)`
        /// when past the subnet, `Ok(false)` on page cap, `Err(())` when the
        /// meter is exhausted (cursor already parked on the last reserved
        /// write).
        fn advance_cursor_past_subnet(
            from: &(NetUid, T::AccountId),
            meter: &mut WeightMeter,
        ) -> Result<bool, ()> {
            let netuid = from.0;
            let key_proof = (NetUid::max_encoded_len() as u64)
                .saturating_add(T::AccountId::max_encoded_len() as u64);
            let key_w = T::DbWeight::get()
                .reads(1)
                .saturating_add(Weight::from_parts(0, key_proof));
            let cursor_w = T::DbWeight::get()
                .writes(1)
                .saturating_add(Weight::from_parts(0, key_proof));
            // Reserve the cursor write before any page work so a mid-page
            // meter miss never writes unbudgeted.
            meter.try_consume(cursor_w).map_err(|_| ())?;
            let mut walked = 1u32;
            let mut last = from.clone();
            let mut iter = DeleverageQueue::<T>::iter_keys_from(
                DeleverageQueue::<T>::hashed_key_for(from.0, &from.1),
            );
            loop {
                if meter.try_consume(key_w).is_err() {
                    DeleverageCursor::<T>::put(last);
                    return Err(());
                }
                match iter.next() {
                    Some((n, owner)) if n == netuid => {
                        walked = walked.saturating_add(1);
                        last = (n, owner);
                        if walked >= MAX_DELEVERAGE_STEPS_PER_BLOCK {
                            DeleverageCursor::<T>::put(last);
                            return Ok(false);
                        }
                    }
                    Some(_) | None => {
                        // Boundary probe or end of map — already charged.
                        DeleverageCursor::<T>::put(last);
                        return Ok(true);
                    }
                }
            }
        }

        /// Run paced buy-backs until a full queue pass makes no progress, the
        /// hard step cap is hit, or `meter` is exhausted. Paused subnets are
        /// skipped in metered pages so a flood of paused keys cannot spend
        /// the full step budget or overrun the hook weight.
        pub(crate) fn exhaust_deleverage_queue(
            params: &SynthParams,
            meter: &mut WeightMeter,
        ) -> u32 {
            let step_w = T::WeightInfo::deleverage_step();
            let db = T::DbWeight::get();
            // Worst-case lookahead: cursor read, suffix probe, wrap-to-start
            // probe, plus the pass-marker existence check — with key proof.
            const TRIE_PATH_OVERHEAD: u64 = 512;
            let key_proof = (NetUid::max_encoded_len() as u64)
                .saturating_add(T::AccountId::max_encoded_len() as u64)
                .saturating_add(TRIE_PATH_OVERHEAD);
            let peek_w = db
                .reads(4)
                .saturating_add(Weight::from_parts(0, key_proof.saturating_mul(2)));
            // Initial cursor read (reserved before `DeleverageCursor::get`).
            let cursor_read_w = db.reads(1).saturating_add(Weight::from_parts(0, key_proof));
            // Pause check worst case (empty TWAP ring): TwapRing, then spot
            // for the empty-ring fallback, then spot again for the pause
            // compare. Each spot reads mechanism + both reserves + balancer.
            let spot_value = (u16::max_encoded_len() as u64) // mechanism
                .saturating_add(u64::max_encoded_len() as u64) // alpha reserve
                .saturating_add(u64::max_encoded_len() as u64) // tao reserve
                .saturating_add(u64::max_encoded_len() as u64); // balancer
            let pause_proof = (TwapState::<BlockNumberFor<T>>::max_encoded_len() as u64)
                .saturating_add(TRIE_PATH_OVERHEAD)
                .saturating_add(
                    spot_value
                        .saturating_add(TRIE_PATH_OVERHEAD.saturating_mul(4))
                        .saturating_mul(2),
                );
            let pause_check_w = db
                .reads(9)
                .saturating_add(Weight::from_parts(0, pause_proof));
            let mut total = 0u32;
            let mut pass_progressed = false;
            let mut pass_start: Option<(NetUid, T::AccountId)> = None;
            // `true` = paused (keep paging), `false` = active.
            let mut pause_state: alloc::vec::Vec<(NetUid, bool)> = alloc::vec::Vec::new();
            // If the cursor already sits mid-subnet, resolve that subnet's
            // pause before the first active attempt so a paused mid-pass
            // cannot burn a full step.
            if meter.try_consume(cursor_read_w).is_err() {
                return 0;
            }
            if let Some((cursor_net, cursor_owner)) = DeleverageCursor::<T>::get() {
                if meter.try_consume(pause_check_w).is_err() {
                    return 0;
                }
                let paused = Self::buybacks_paused(cursor_net, params);
                pause_state.push((cursor_net, paused));
                if paused {
                    match Self::advance_cursor_past_subnet(&(cursor_net, cursor_owner), meter) {
                        Ok(true) => {}
                        Ok(false) => return 1,
                        Err(()) => return 0,
                    }
                }
            }
            while total < MAX_DELEVERAGE_STEPS_PER_BLOCK {
                if meter.try_consume(peek_w).is_err() {
                    break;
                }
                let Some(next) = Self::peek_next_deleverage_key() else {
                    break;
                };
                if let Some((netuid, owner)) = pass_start.as_ref()
                    && !DeleverageQueue::<T>::contains_key(netuid, owner)
                {
                    pass_start = None;
                }
                if pass_start.as_ref() == Some(&next) {
                    if !pass_progressed {
                        break;
                    }
                    pass_progressed = false;
                }
                if pass_start.is_none() {
                    pass_start = Some(next.clone());
                }
                let netuid = next.0;
                let paused = match pause_state
                    .iter()
                    .find(|(n, _)| *n == netuid)
                    .map(|(_, p)| *p)
                {
                    Some(p) => p,
                    None => {
                        if meter.try_consume(pause_check_w).is_err() {
                            break;
                        }
                        let p = Self::buybacks_paused(netuid, params);
                        pause_state.push((netuid, p));
                        p
                    }
                };
                if paused {
                    match Self::advance_cursor_past_subnet(&next, meter) {
                        Ok(true) => continue,
                        Ok(false) => {
                            // Count a skip page against the step cap so a
                            // huge paused prefix cannot spin forever.
                            total = total.saturating_add(1);
                            continue;
                        }
                        Err(()) => break,
                    }
                }
                if meter.try_consume(step_w.saturating_sub(peek_w)).is_err() {
                    break;
                }
                let (attempted, progressed) = Self::process_deleverage_queue(1, params);
                if attempted == 0 {
                    break;
                }
                total = total.saturating_add(attempted);
                if progressed > 0 {
                    pass_progressed = true;
                }
            }
            total
        }

        /// Serve up to `max` deleveraging positions, resuming after the cursor.
        /// Returns `(attempts, progressed)` for weight vs. pacing control.
        pub(crate) fn process_deleverage_queue(max: u32, params: &SynthParams) -> (u32, u32) {
            let mut keys: alloc::vec::Vec<(NetUid, T::AccountId)> =
                match DeleverageCursor::<T>::get() {
                    Some((netuid, owner)) => DeleverageQueue::<T>::iter_keys_from(
                        DeleverageQueue::<T>::hashed_key_for(netuid, &owner),
                    )
                    .take(max as usize)
                    .collect(),
                    None => DeleverageQueue::<T>::iter_keys()
                        .take(max as usize)
                        .collect(),
                };
            // A prior page may have parked the cursor on the last key; the next
            // `iter_keys_from` then yields nothing. Wrap so the queue is not
            // stranded when we page one entry at a time under the weight meter.
            if keys.is_empty() && DeleverageCursor::<T>::exists() {
                DeleverageCursor::<T>::kill();
                keys = DeleverageQueue::<T>::iter_keys()
                    .take(max as usize)
                    .collect();
            }
            if (keys.len() as u32) < max {
                DeleverageCursor::<T>::kill();
            } else if let Some(last) = keys.last() {
                DeleverageCursor::<T>::put(last.clone());
            }
            let mut progressed = 0u32;
            for (netuid, owner) in keys.iter() {
                if Self::deleverage_step(owner, *netuid, params) {
                    progressed = progressed.saturating_add(1);
                }
            }
            (keys.len() as u32, progressed)
        }

        /// The reserve buys back and burns residual alpha under the pace limit
        /// and the pump pause (SPEC §1.6). Returns the number of subnets
        /// served.
        pub(crate) fn retire_residuals(max: u32, params: &SynthParams) -> u32 {
            let entries: alloc::vec::Vec<(NetUid, u64)> = match RetireCursor::<T>::get() {
                Some(last) => {
                    ResidualAlpha::<T>::iter_from(ResidualAlpha::<T>::hashed_key_for(last))
                        .take(max as usize)
                        .collect()
                }
                None => ResidualAlpha::<T>::iter().take(max as usize).collect(),
            };
            if (entries.len() as u32) < max {
                RetireCursor::<T>::kill();
            } else if let Some((last, _)) = entries.last() {
                RetireCursor::<T>::put(*last);
            }
            for (netuid, residual) in entries.iter() {
                Self::retire_residual(*netuid, *residual, params);
            }
            entries.len() as u32
        }

        pub(crate) fn retire_residual(netuid: NetUid, residual: u64, params: &SynthParams) {
            let reserve_tao = ResidualReserve::<T>::get(netuid);
            if reserve_tao == 0
                || Dissolutions::<T>::contains_key(netuid)
                || !T::Pool::subnet_exists(netuid)
                || Self::buybacks_paused(netuid, params)
            {
                return;
            }
            let spend = Self::pace_room(netuid, params).min(reserve_tao);
            if spend == 0 {
                return;
            }
            let alpha = T::Pool::quote_buy(netuid, spend.into())
                .map(|a| a.to_u64())
                .unwrap_or(0)
                .min(residual);
            if alpha == 0 {
                return;
            }
            let reserve = Self::reserve_account(netuid);
            let bought =
                with_transaction(|| match Self::buy_back(netuid, &reserve, alpha, spend) {
                    Ok((spent, burned)) => TransactionOutcome::Commit(Ok((spent, burned))),
                    Err(e) => TransactionOutcome::Rollback(Err(e)),
                });
            let Ok((spent, burned)) = bought else {
                return;
            };
            ResidualReserve::<T>::mutate(netuid, |r| *r = r.saturating_sub(spent));
            let left = residual.saturating_sub(burned);
            if left == 0 {
                ResidualAlpha::<T>::remove(netuid);
            } else {
                ResidualAlpha::<T>::insert(netuid, left);
            }
            Self::refresh_breaker(netuid, params);
            Self::deposit_event(Event::ResidualRetired {
                netuid,
                alpha: burned,
                tao: spent,
            });
        }

        // ---- interest -------------------------------------------------------

        fn block_u64(n: BlockNumberFor<T>) -> u64 {
            sp_runtime::SaturatedConversion::saturated_into(n)
        }

        /// `Issuance_ex = issuance − SyntheticOutstanding − ResidualAlpha`.
        pub fn issuance_ex(netuid: NetUid) -> u128 {
            u128::from(T::Pool::alpha_issuance(netuid).to_u64())
                .saturating_sub(u128::from(SyntheticOutstanding::<T>::get(netuid)))
                .saturating_sub(u128::from(ResidualAlpha::<T>::get(netuid)))
        }

        /// Uncapped slow EMA of spot for interest / min notional (P5).
        /// Prefers the seeded [`NotionalEma`]; before the first tick falls
        /// back to the emission moving price — never raw spot.
        pub fn notional_price(netuid: NetUid) -> U64F64 {
            NotionalEma::<T>::get(netuid).unwrap_or_else(|| T::Pool::moving_alpha_price(netuid))
        }

        fn notional(netuid: NetUid, minted: u64) -> u64 {
            Self::mul_price(minted, Self::notional_price(netuid), false)
        }

        /// Root recovery of stranded free TAO on `who`.
        ///
        /// The subnet residual reserve (excess over the book) pays entirely
        /// into the pot. A custody in [`StrandedCustody`] pays `due` into the
        /// pot and refunds `refund` to the recorded owner — never an
        /// arbitrary EOA or another subnet's accounts.
        pub fn do_sweep_stranded(netuid: NetUid, who: &T::AccountId) -> DispatchResult {
            // Pot + refund must commit together so a failed refund cannot leave
            // a paid pot leg eligible for retry against the same record.
            with_transaction(|| {
                let reserve = Self::reserve_account(netuid);
                let free = T::Pool::free_tao(who).to_u64();
                if *who == reserve {
                    let amount = free.saturating_sub(ResidualReserve::<T>::get(netuid));
                    if amount == 0 {
                        return TransactionOutcome::Rollback(Err(Error::<T>::ZeroAmount.into()));
                    }
                    let paid = if T::Pool::subnet_pot_account(netuid).is_some() {
                        T::Pool::pay_into_pot(who, netuid, amount.into())
                    } else {
                        T::Pool::burn_interest_tao(who, amount.into()).map(|_| amount.into())
                    };
                    return match paid {
                        Ok(_) => TransactionOutcome::Commit(Ok(())),
                        Err(e) => TransactionOutcome::Rollback(Err(e)),
                    };
                }
                let Some(record) = StrandedCustody::<T>::get(netuid, who) else {
                    return TransactionOutcome::Rollback(Err(Error::<T>::OwnerNotAllowed.into()));
                };
                if EscrowOwner::<T>::contains_key(netuid, who) {
                    return TransactionOutcome::Rollback(Err(Error::<T>::PositionExists.into()));
                }
                let want = record.due.saturating_add(record.refund);
                if want == 0 || free == 0 {
                    return TransactionOutcome::Rollback(Err(Error::<T>::ZeroAmount.into()));
                }
                let (due_pay, refund_pay) = if free >= want {
                    (record.due, record.refund)
                } else {
                    let due_pay = record.due.min(free);
                    (due_pay, free.saturating_sub(due_pay).min(record.refund))
                };
                if due_pay > 0 {
                    let paid = if T::Pool::subnet_pot_account(netuid).is_some() {
                        T::Pool::pay_into_pot(who, netuid, due_pay.into())
                    } else {
                        T::Pool::burn_interest_tao(who, due_pay.into()).map(|_| due_pay.into())
                    };
                    if let Err(e) = paid {
                        return TransactionOutcome::Rollback(Err(e));
                    }
                }
                if refund_pay > 0
                    && let Err(e) =
                        T::Pool::refund_dissolution_tao(who, &record.owner, refund_pay.into())
                {
                    return TransactionOutcome::Rollback(Err(e));
                }
                // Finish the default burn path for any deposit / overflow that
                // remained after the pot and owner legs.
                let leftover = T::Pool::free_tao(who).to_u64();
                if leftover > 0
                    && let Err(e) = T::Pool::burn_interest_tao(who, leftover.into())
                {
                    return TransactionOutcome::Rollback(Err(e));
                }
                StrandedCustody::<T>::remove(netuid, who);
                TransactionOutcome::Commit(Ok(()))
            })
        }

        fn interest_denominator() -> u128 {
            BPS.saturating_mul(u128::from(T::BlocksPerYear::get().max(1)))
        }

        /// Interest on `minted` for `days` days at the flat APR, rounded up.
        pub fn days_interest(netuid: NetUid, minted: u64, days: u64, params: &SynthParams) -> u64 {
            let blocks = (T::BlocksPerYear::get() / 365).saturating_mul(days);
            let num = u128::from(Self::notional(netuid, minted))
                .saturating_mul(u128::from(params.apr_bps))
                .saturating_mul(u128::from(blocks));
            u64::try_from(num.div_ceil(Self::interest_denominator())).unwrap_or(u64::MAX)
        }

        pub fn day_interest(netuid: NetUid, minted: u64, params: &SynthParams) -> u64 {
            Self::days_interest(netuid, minted, 1, params)
        }

        /// Interest accrued from `last_accrued` to now, with the carried
        /// remainder, and the remainder left after it.
        pub(crate) fn accrued(
            netuid: NetUid,
            pos: &PositionOf<T>,
            params: &SynthParams,
        ) -> (u64, u64) {
            let now = frame_system::Pallet::<T>::block_number();
            let blocks = Self::block_u64(now.saturating_sub(pos.last_accrued));
            let num = u128::from(Self::notional(netuid, pos.minted))
                .saturating_mul(u128::from(params.apr_bps))
                .saturating_mul(u128::from(blocks))
                .saturating_add(u128::from(pos.interest_remainder));
            let den = Self::interest_denominator();
            let owed = u64::try_from(num.checked_div(den).unwrap_or(0)).unwrap_or(u64::MAX);
            let remainder = u64::try_from(num.checked_rem(den).unwrap_or(0)).unwrap_or(0);
            (owed, remainder)
        }

        /// Charge accrued interest from collateral (I5: never more than the
        /// collateral). The reserve share is capped at `reserve_cap_bps` of
        /// pool TAO; overflow and the burn share are burned (P4).
        pub(crate) fn charge_interest(
            owner: &T::AccountId,
            netuid: NetUid,
            pos: &mut PositionOf<T>,
            params: &SynthParams,
        ) -> DispatchResult {
            let (owed, remainder) = Self::accrued(netuid, pos, params);
            pos.last_accrued = frame_system::Pallet::<T>::block_number();
            pos.interest_remainder = remainder;
            let paid = owed.min(pos.collateral);
            let shortfall = owed.saturating_sub(paid);
            if shortfall > 0 {
                let cumulative = UncollectedInterest::<T>::mutate(owner, netuid, |u| {
                    *u = u.saturating_add(shortfall);
                    *u
                });
                Self::deposit_event(Event::InterestUncollected {
                    who: owner.clone(),
                    netuid,
                    amount: shortfall,
                    cumulative,
                });
            }
            if paid == 0 {
                return Ok(());
            }
            let share = Self::mul_bps(paid, params.reserve_share_bps);
            let cap = Self::mul_bps(T::Pool::pool_tao(netuid).to_u64(), params.reserve_cap_bps);
            let room = cap.saturating_sub(ResidualReserve::<T>::get(netuid));
            let to_reserve = share.min(room);
            let reserved = if to_reserve > 0 {
                T::Pool::collect_interest_tao(
                    &pos.custody,
                    &Self::reserve_account(netuid),
                    to_reserve.into(),
                )?
                .to_u64()
            } else {
                0
            };
            ResidualReserve::<T>::mutate(netuid, |r| *r = r.saturating_add(reserved));
            let burn_tao = paid.saturating_sub(reserved);
            if burn_tao > 0 {
                T::Pool::burn_interest_tao(&pos.custody, burn_tao.into())?;
            }
            pos.collateral = pos.collateral.saturating_sub(paid);
            Self::deposit_event(Event::InterestCharged {
                who: owner.clone(),
                netuid,
                burned: burn_tao,
                reserved,
            });
            Ok(())
        }

        fn schedule_due(owner: &T::AccountId, netuid: NetUid, at: BlockNumberFor<T>) {
            Due::<T>::insert(at, (owner.clone(), netuid), ());
            NextDue::<T>::mutate(|next| {
                if next.is_none_or(|n| at < n) {
                    *next = Some(at);
                }
            });
        }

        /// Drain due interest charges in block order within `meter`.
        pub fn collect_due(now: BlockNumberFor<T>, meter: &mut WeightMeter) {
            let db = T::DbWeight::get();
            let charge = T::WeightInfo::charge_interest();
            // Cursor read reserved before `NextDue::get`.
            let cursor_w = db.reads(1).saturating_add(Weight::from_parts(
                0,
                (BlockNumberFor::<T>::max_encoded_len() as u64).saturating_add(3072),
            ));
            // Empty bucket: Due prefix probe + TotalPositions + NextDue write,
            // with trie witness for the prefix / counter keys.
            let empty_w = db.reads_writes(3, 1).saturating_add(Weight::from_parts(
                0,
                3072u64
                    .saturating_mul(2)
                    .saturating_add(u32::max_encoded_len() as u64)
                    .saturating_add(BlockNumberFor::<T>::max_encoded_len() as u64),
            ));
            // Non-empty: Due prefix probe already paid for the next key.
            let probe_w = db.reads(1).saturating_add(Weight::from_parts(0, 3072));
            let mut bumps = 0u32;
            loop {
                if meter.try_consume(cursor_w).is_err() {
                    return;
                }
                let Some(at) = NextDue::<T>::get() else {
                    return;
                };
                if at > now {
                    return;
                }
                if meter.try_consume(probe_w).is_err() {
                    return;
                }
                match Due::<T>::iter_key_prefix(at).next() {
                    None => {
                        // Prefix miss already consumed; pay the remaining
                        // empty-bucket work (TotalPositions + NextDue write).
                        if meter.try_consume(empty_w.saturating_sub(probe_w)).is_err() {
                            return;
                        }
                        if TotalPositions::<T>::get() == 0 {
                            NextDue::<T>::kill();
                            return;
                        }
                        NextDue::<T>::put(at.saturating_add(One::one()));
                        bumps = bumps.saturating_add(1);
                        if bumps >= MAX_EMPTY_DUE_BUMPS {
                            return;
                        }
                    }
                    Some((owner, netuid)) => {
                        if meter.try_consume(charge).is_err() {
                            return;
                        }
                        Due::<T>::remove(at, (owner.clone(), netuid));
                        Self::process_due(&owner, netuid, at, now);
                    }
                }
            }
        }

        fn process_due(
            owner: &T::AccountId,
            netuid: NetUid,
            at: BlockNumberFor<T>,
            now: BlockNumberFor<T>,
        ) {
            let Some(mut pos) = Positions::<T>::get(owner, netuid) else {
                return;
            };
            if pos.due != at || Dissolutions::<T>::contains_key(netuid) {
                return;
            }
            let params = SynthParamsStore::<T>::get();
            let mut charged = pos.clone();
            let result = with_transaction(|| {
                match Self::charge_interest(owner, netuid, &mut charged, &params) {
                    Ok(()) => TransactionOutcome::Commit(Ok(())),
                    Err(e) => TransactionOutcome::Rollback(Err(e)),
                }
            });
            let next = if result.is_ok() {
                pos = charged;
                Self::start_deleverage_if_unhealthy(owner, netuid, &mut pos, &params);
                now.saturating_add(T::InterestPeriod::get())
            } else {
                now.saturating_add(T::InterestRetryDelay::get())
            };
            pos.due = next;
            Self::schedule_due(owner, netuid, next);
            Positions::<T>::insert(owner, netuid, &pos);
        }

        // ---- TWAP -------------------------------------------------------------

        /// Mean spot over the open interval; spot alone when no blocks accumulated.
        fn twap_interval_mean(ring: &TwapState<BlockNumberFor<T>>, spot: U64F64) -> U64F64 {
            if ring.interval_blocks == 0 {
                return spot;
            }
            ring.interval_sum
                .checked_div(U64F64::saturating_from_num(ring.interval_blocks))
                .unwrap_or(spot)
        }

        fn push_twap_sample(
            ring: &mut TwapState<BlockNumberFor<T>>,
            sample: U64F64,
            now: BlockNumberFor<T>,
            cap: usize,
        ) {
            if ring.samples.len() > cap {
                ring.samples.truncate(cap);
                ring.cursor = 0;
            }
            if ring.samples.len() < cap {
                let _ = ring.samples.try_push(sample);
            } else if let Some(slot) = ring.samples.get_mut(ring.cursor as usize) {
                *slot = sample;
                ring.cursor = ring
                    .cursor
                    .saturating_add(1)
                    .checked_rem(cap as u32)
                    .unwrap_or(0);
            }
            ring.last_sample = now;
        }

        /// Accumulate this block's spot; close the interval on boundary blocks.
        fn twap_tick(
            netuid: NetUid,
            now: BlockNumberFor<T>,
            params: &SynthParams,
            close_interval: bool,
        ) {
            let spot = T::Pool::current_alpha_price(netuid);
            let cap = params.twap_samples.clamp(1, MAX_TWAP_SAMPLES) as usize;
            TwapRing::<T>::mutate(netuid, |ring| {
                ring.interval_sum = ring.interval_sum.saturating_add(spot);
                ring.interval_blocks = ring.interval_blocks.saturating_add(1);
                if close_interval {
                    let sample = Self::twap_interval_mean(ring, spot);
                    ring.interval_sum = U64F64::from_num(0);
                    ring.interval_blocks = 0;
                    Self::push_twap_sample(ring, sample, now, cap);
                }
            });
        }

        /// Benchmark helper: push one interval mean as if every block had `spot`.
        pub fn sample_twap(netuid: NetUid, now: BlockNumberFor<T>, params: &SynthParams) {
            let spot = T::Pool::current_alpha_price(netuid);
            let cap = params.twap_samples.clamp(1, MAX_TWAP_SAMPLES) as usize;
            let interval = params.twap_interval_blocks.max(1);
            TwapRing::<T>::mutate(netuid, |ring| {
                ring.interval_sum = spot.saturating_mul(U64F64::saturating_from_num(interval));
                ring.interval_blocks = interval;
                let sample = Self::twap_interval_mean(ring, spot);
                ring.interval_sum = U64F64::from_num(0);
                ring.interval_blocks = 0;
                Self::push_twap_sample(ring, sample, now, cap);
            });
        }

        /// Median of the ring (P6); spot before the first sample. An even
        /// count averages the two middle samples.
        pub fn twap(netuid: NetUid) -> U64F64 {
            let mut samples = TwapRing::<T>::get(netuid).samples.into_inner();
            if samples.is_empty() {
                return T::Pool::current_alpha_price(netuid);
            }
            samples.sort_unstable();
            let mid = samples.len() / 2;
            let upper = samples.get(mid).copied().unwrap_or_default();
            if samples.len() % 2 == 1 {
                return upper;
            }
            let lower = samples.get(mid.saturating_sub(1)).copied().unwrap_or(upper);
            let half = upper
                .saturating_sub(lower)
                .checked_div(U64F64::from_num(2))
                .unwrap_or_default();
            lower.saturating_add(half)
        }

        // ---- health and coverage -------------------------------------------

        /// `C + P − BuyCost(q) × TWAP / spot` on the live pool (P5).
        pub fn equity_twap(netuid: NetUid, pos: &PositionOf<T>) -> i128 {
            let funds = i128::from(pos.funds());
            let Some(cost) = Self::buy_cost_on(Self::pool_state(netuid), pos.minted) else {
                return i128::MIN;
            };
            let spot = T::Pool::current_alpha_price(netuid).to_bits();
            let twap = Self::twap(netuid).to_bits();
            let scaled = if spot == 0 {
                u128::from(cost)
            } else {
                multiply_by_rational_with_rounding(u128::from(cost), twap, spot, Rounding::Up)
                    .unwrap_or(u128::MAX)
            };
            funds.saturating_sub(i128::try_from(scaled).unwrap_or(i128::MAX))
        }

        /// `Stress_eff = stress × (1 + slope × book / pool_alpha)` in bps.
        pub fn stress_eff_bps(book: u64, pool_alpha: u64, params: &SynthParams) -> u128 {
            let stress = u128::from(params.stress_bps);
            if pool_alpha == 0 {
                return u128::MAX;
            }
            let lift = multiply_by_rational_with_rounding(
                u128::from(params.stress_slope_bps),
                u128::from(book),
                u128::from(pool_alpha),
                Rounding::Up,
            )
            .unwrap_or(u128::MAX);
            multiply_by_rational_with_rounding(stress, BPS.saturating_add(lift), BPS, Rounding::Up)
                .unwrap_or(u128::MAX)
        }

        /// `q_pos`'s pro-rata share of buying back the whole `book` after a pump
        /// to `Stress_eff × ref` (P2). `None` when the pumped pool cannot
        /// supply the book.
        fn book_need(
            netuid: NetUid,
            pool: PoolState,
            reference: U64F64,
            book: u64,
            q_pos: u64,
            params: &SynthParams,
        ) -> Option<u64> {
            if q_pos == 0 {
                return Some(0);
            }
            let book = book.max(q_pos);
            let stress = Self::stress_eff_bps(book, pool.alpha, params);
            let target = Self::scale_price(reference, stress);
            let pumped = Self::pump_to(pool, target)?;
            let cost = Self::buy_cost_on(pumped, book)?;
            let scaled = Self::scale_fee_inclusive_book_cost(netuid, cost, book)?;
            multiply_by_rational_with_rounding(
                u128::from(scaled),
                u128::from(q_pos),
                u128::from(book),
                Rounding::Up,
            )
            .and_then(|v| u64::try_from(v).ok())
        }

        /// Fee-inclusive TAO to buy `alpha` on the live pool.
        pub(crate) fn buy_cost_fee_inclusive(netuid: NetUid, alpha: u64) -> Option<u64> {
            if alpha == 0 {
                return Some(0);
            }
            let pool = Self::pool_state(netuid);
            if alpha >= pool.alpha {
                return None;
            }
            let estimate = Self::buy_cost_on(pool, alpha)?;
            Self::search_min(estimate, |tao| {
                T::Pool::quote_buy(netuid, tao.into()).is_ok_and(|out| out.to_u64() >= alpha)
            })
        }

        /// Scale a fee-free whole-book cost to include swap fees from the live pool.
        /// Falls back to `2 × free_cost` when the fee ratio is unavailable, so
        /// coverage is conservative rather than propagating None to CoverageNotMet.
        fn scale_fee_inclusive_book_cost(netuid: NetUid, free_cost: u64, book: u64) -> Option<u64> {
            if free_cost == 0 || book == 0 {
                return Some(free_cost);
            }
            let sample = book.min(Self::pool_state(netuid).alpha.saturating_sub(1));
            if sample == 0 {
                return Some(free_cost);
            }
            let Some(fee) = Self::buy_cost_fee_inclusive(netuid, sample) else {
                return Some(free_cost.saturating_mul(2));
            };
            let Some(free) = Self::buy_cost_on(Self::pool_state(netuid), sample) else {
                return Some(free_cost.saturating_mul(2));
            };
            if free == 0 {
                return Some(free_cost);
            }
            multiply_by_rational_with_rounding(
                u128::from(free_cost),
                u128::from(fee),
                u128::from(free),
                Rounding::Up,
            )
            .and_then(|v| u64::try_from(v).ok())
            .or(Some(free_cost.saturating_mul(2)))
        }

        /// `max(uncapped_EMA, spot, SMA30_rank)` on `pool` (P2).
        fn reference_price(netuid: NetUid, pool: PoolState) -> U64F64 {
            Self::notional_price(netuid)
                .max(Self::price_on(pool))
                .max(Self::rank_price(netuid))
        }

        /// Whole-book coverage need of a position holding `minted` on the live
        /// pool and book.
        pub fn coverage_cost(netuid: NetUid, minted: u64, params: &SynthParams) -> Option<u64> {
            let pool = Self::pool_state(netuid);
            Self::book_need(
                netuid,
                pool,
                Self::reference_price(netuid, pool),
                SyntheticOutstanding::<T>::get(netuid),
                minted,
                params,
            )
        }

        /// Coverage (I4) plus one day of interest in collateral (P5).
        fn ensure_coverage(
            netuid: NetUid,
            pos: &PositionOf<T>,
            params: &SynthParams,
            err: Error<T>,
        ) -> DispatchResult {
            let err: DispatchError = err.into();
            let cost = Self::coverage_cost(netuid, pos.minted, params).ok_or(err)?;
            ensure!(pos.funds() >= cost, err);
            ensure!(
                pos.collateral >= Self::day_interest(netuid, pos.minted, params),
                err
            );
            Ok(())
        }

        // ---- pure pool math -------------------------------------------------

        pub(crate) fn pool_state(netuid: NetUid) -> PoolState {
            PoolState {
                tao: T::Pool::pool_tao(netuid).to_u64(),
                alpha: T::Pool::pool_alpha(netuid).to_u64(),
                weight: T::Pool::quote_weight_parts(netuid),
            }
        }

        /// Balancer spot `(w_base / w_quote) · (tao / alpha)`.
        pub(crate) fn price_on(pool: PoolState) -> U64F64 {
            if pool.alpha == 0 || pool.weight == 0 {
                return U64F64::from_num(0);
            }
            let w_quote = U64F64::saturating_from_num(pool.weight);
            let w_base = U64F64::saturating_from_num(
                WEIGHT_ACCURACY.saturating_sub(u128::from(pool.weight)),
            );
            let ratio = U64F64::saturating_from_num(pool.tao)
                .checked_div(U64F64::saturating_from_num(pool.alpha))
                .unwrap_or_default();
            w_base
                .checked_div(w_quote)
                .unwrap_or_default()
                .saturating_mul(ratio)
        }

        fn after_buy(pool: PoolState, tao: u64) -> PoolState {
            let out =
                T::Pool::quote_buy_on(pool.tao.into(), pool.alpha.into(), pool.weight, tao.into())
                    .to_u64();
            PoolState {
                tao: pool.tao.saturating_add(tao),
                alpha: pool.alpha.saturating_sub(out),
                weight: pool.weight,
            }
        }

        /// Smallest `x` in `(0, hi]` with `pred(x)`, given `pred` is monotone
        /// and `pred(hi)` holds. Doubles `hi` from `start` first.
        fn search_min(start: u64, pred: impl Fn(u64) -> bool) -> Option<u64> {
            let mut hi = start.max(1);
            let mut doublings = 0;
            while !pred(hi) {
                if hi == u64::MAX || doublings >= SEARCH_STEPS {
                    return None;
                }
                hi = hi.saturating_mul(2);
                doublings = doublings.saturating_add(1);
            }
            let mut lo = 0u64;
            while hi.saturating_sub(lo) > 1 {
                let mid = lo.saturating_add(hi.saturating_sub(lo) / 2);
                if pred(mid) {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            Some(hi)
        }

        /// Fee-free TAO that buys exactly `alpha` from `pool`; `None` when the
        /// pool cannot supply it.
        pub(crate) fn buy_cost_on(pool: PoolState, alpha: u64) -> Option<u64> {
            if alpha == 0 {
                return Some(0);
            }
            if alpha >= pool.alpha {
                return None;
            }
            let estimate = u128::from(alpha)
                .saturating_mul(u128::from(pool.tao))
                .checked_div(u128::from(pool.alpha.saturating_sub(alpha)))
                .unwrap_or(u128::MAX);
            Self::search_min(u64::try_from(estimate).unwrap_or(u64::MAX), |tao| {
                T::Pool::quote_buy_on(pool.tao.into(), pool.alpha.into(), pool.weight, tao.into())
                    .to_u64()
                    >= alpha
            })
        }

        /// Reserves after the smallest fee-free buy that lifts spot to
        /// `target`; `pool` itself when spot is already there.
        pub(crate) fn pump_to(pool: PoolState, target: U64F64) -> Option<PoolState> {
            if Self::price_on(pool) >= target {
                return Some(pool);
            }
            let tao = Self::search_min(pool.tao, |tao| {
                Self::price_on(Self::after_buy(pool, tao)) >= target
            })?;
            Some(Self::after_buy(pool, tao))
        }

        fn mul_price(amount: u64, price: U64F64, round_up: bool) -> u64 {
            let rounding = if round_up {
                Rounding::Up
            } else {
                Rounding::Down
            };
            multiply_by_rational_with_rounding(
                u128::from(amount),
                price.to_bits(),
                FIXED_ONE,
                rounding,
            )
            .and_then(|v| u64::try_from(v).ok())
            .unwrap_or(u64::MAX)
        }

        fn mul_bps(amount: u64, bps: u16) -> u64 {
            u64::try_from(u128::from(amount).saturating_mul(u128::from(bps)) / BPS)
                .unwrap_or(u64::MAX)
        }

        fn scale_price(price: U64F64, bps: u128) -> U64F64 {
            U64F64::from_bits(
                multiply_by_rational_with_rounding(price.to_bits(), bps, BPS, Rounding::Down)
                    .unwrap_or(u128::MAX),
            )
        }

        // ---- dissolution ------------------------------------------------------

        /// `r = min(T, T_ema) / all_alpha`, with `T` pool TAO (short escrow is
        /// not in the pool) and `all_alpha` raw issuance including synthetic
        /// and residual alpha (P12).
        pub fn dissolution_rate(netuid: NetUid) -> U64F64 {
            let tao = T::Pool::pool_tao(netuid).to_u64();
            let tao = PoolTaoEma::<T>::get(netuid).map_or(tao, |ema| ema.min(tao));
            let all = T::Pool::alpha_issuance(netuid).to_u64();
            if all == 0 {
                return U64F64::from_num(0);
            }
            U64F64::from_bits(
                multiply_by_rational_with_rounding(
                    u128::from(tao),
                    FIXED_ONE,
                    u128::from(all),
                    Rounding::Down,
                )
                .unwrap_or(0),
            )
        }

        /// O(1) weight for [`Self::start_dissolution`]: Enabled check, optional
        /// Dissolutions probe/insert, rate reads (pool tao, PoolTaoEma,
        /// three issuance keys), System Number, and floor/target kills —
        /// with trie proof.
        pub fn start_dissolution_weight() -> Weight {
            let db = T::DbWeight::get();
            const TRIE_PATH: u64 = 3072;
            // Enabled + Dissolutions + PoolTaoEma + pool_tao + SubnetAlphaIn +
            // SubnetAlphaOut + BalancerAlphaReservoir + Number + PruneFloor +
            // PruneTarget = 10 reads; Dissolutions + two kills = up to 3 writes.
            // `alpha_issuance` is three separate storage reads (not one aggregate).
            let value = (bool::max_encoded_len() as u64)
                .saturating_add(Dissolution::<BlockNumberFor<T>>::max_encoded_len() as u64)
                .saturating_add(U64F64::max_encoded_len() as u64) // PoolTaoEma
                .saturating_add(u64::max_encoded_len() as u64) // pool tao
                .saturating_add(u64::max_encoded_len() as u64) // SubnetAlphaIn
                .saturating_add(u64::max_encoded_len() as u64) // SubnetAlphaOut
                .saturating_add(u64::max_encoded_len() as u64) // BalancerAlphaReservoir
                .saturating_add(BlockNumberFor::<T>::max_encoded_len() as u64)
                .saturating_add(U64F64::max_encoded_len() as u64) // floor
                .saturating_add(NetUid::max_encoded_len() as u64); // target
            db.reads_writes(10, 3)
                .saturating_add(Weight::from_parts(0, value.saturating_add(TRIE_PATH * 10)))
        }

        pub fn start_dissolution(netuid: NetUid) -> DispatchResult {
            // Always freeze `r` (protocol dissolve accounting uses it even
            // with no shorts) and O(1)-kill the prune caches. Do not rescan
            // the floor here — signed registration reaches this hook without
            // a SubnetLimit surcharge (fee baselines protected). Saturating
            // `start_dissolution_weight` onto register_network / dissolve
            // dispatch weights needs explicit fee-baseline approval.
            Self::ensure_not_dissolving(netuid)?;
            let now = frame_system::Pallet::<T>::block_number();
            Dissolutions::<T>::insert(
                netuid,
                Dissolution {
                    frozen_at: now,
                    price: Self::dissolution_rate(netuid),
                },
            );
            PruneFloor::<T>::kill();
            PruneTarget::<T>::kill();
            Ok(())
        }

        /// What `pos` owes the pot at rate `r`: zone alpha at its own proceeds,
        /// the rest at `r`, capped at its funds.
        pub fn dissolution_owed(pos: &PositionOf<T>, rate: U64F64) -> u64 {
            let older = pos.minted.saturating_sub(pos.zone_minted);
            Self::mul_price(older, rate, true)
                .saturating_add(pos.zone_proceeds)
                .min(pos.funds())
        }

        /// Settle each short (P12, P16), then add the reserve to the pot and
        /// clear the subnet's synthetic state. A shortfall is not chased
        /// (holder dilution).
        pub fn settle_positions_on_dissolution(netuid: NetUid, meter: &mut WeightMeter) -> bool {
            let Some(dissolution) = Dissolutions::<T>::get(netuid) else {
                return true;
            };
            let settle = T::WeightInfo::settle_dissolution();
            while let Some(owner) = OpenByNetuid::<T>::iter_key_prefix(netuid).next() {
                if meter.try_consume(settle).is_err() {
                    return false;
                }
                Self::settle_one_on_dissolution(&owner, netuid, dissolution.price);
            }
            if meter.try_consume(settle).is_err() {
                return false;
            }
            let reserve_tao = ResidualReserve::<T>::get(netuid);
            if reserve_tao > 0 {
                let reserve = Self::reserve_account(netuid);
                if T::Pool::pay_into_pot(&reserve, netuid, reserve_tao.into()).is_err() {
                    // TAO stays on the reserve account; clear the book entry
                    // so we don't spin trying to re-pay. The on-chain balance
                    // is recovered by the reserve account owner later.
                    Self::deposit_event(Event::ReservePayFailed {
                        netuid,
                        amount: reserve_tao,
                    });
                }
                ResidualReserve::<T>::remove(netuid);
            }
            ResidualAlpha::<T>::remove(netuid);
            SyntheticOutstanding::<T>::remove(netuid);
            DeleveragingMinted::<T>::remove(netuid);
            DeleverageBudget::<T>::remove(netuid);
            NextDeleverageSeq::<T>::remove(netuid);
            let _ = DeleverageFilled::<T>::clear_prefix(netuid, u32::MAX, None);
            BreakerTripped::<T>::remove(netuid);
            TwapRing::<T>::remove(netuid);
            PaceAnchor::<T>::remove(netuid);
            BlockNetMinted::<T>::remove(netuid);
            true
        }

        fn settle_one_on_dissolution(owner: &T::AccountId, netuid: NetUid, rate: U64F64) {
            let Some(mut pos) = Positions::<T>::get(owner, netuid) else {
                OpenByNetuid::<T>::remove(netuid, owner);
                return;
            };
            let params = SynthParamsStore::<T>::get();
            // Same transactional + clone pattern as `process_due`: a mid-charge
            // failure must not keep the reserve share while `pos` still books
            // the transferred collateral for the terminal split.
            let mut charged = pos.clone();
            let interest_ok = with_transaction(|| {
                match Self::charge_interest(owner, netuid, &mut charged, &params) {
                    Ok(()) => TransactionOutcome::Commit(Ok(())),
                    Err(e) => TransactionOutcome::Rollback(Err(e)),
                }
            })
            .is_ok();
            if interest_ok {
                pos = charged;
                Positions::<T>::insert(owner, netuid, &pos);
            }
            // Best-effort: burn any accrued-but-uncollected interest from
            // proceeds before the final settlement split.
            Self::drain_uncollected_interest(owner, netuid, &mut pos);
            let (pot, due) = match T::Pool::subnet_pot_account(netuid) {
                Some(pot) => (pot, Self::dissolution_owed(&pos, rate)),
                None => (owner.clone(), 0),
            };
            let refund = pos.funds().saturating_sub(due).saturating_add(pos.deposit);
            match T::Pool::terminal_split_tao(&pos.custody, &pot, owner, due.into(), refund.into())
            {
                Ok((recovered, _)) => {
                    T::Pool::credit_pot_reserve(netuid, recovered);
                    let paid = recovered.to_u64();
                    SyntheticOutstanding::<T>::mutate(netuid, |s| {
                        *s = s.saturating_sub(pos.minted)
                    });
                    Self::remove_position(owner, netuid, &pos);
                    Self::deposit_event(Event::ShortSettledOnDissolution {
                        who: owner.clone(),
                        netuid,
                        price: rate,
                        paid,
                    });
                }
                Err(_) => {
                    // TAO stays on custody; record pot due + owner refund so
                    // root sweep cannot debit unrelated accounts.
                    if due > 0 || refund > 0 {
                        StrandedCustody::<T>::insert(
                            netuid,
                            &pos.custody,
                            StrandedRecord {
                                owner: owner.clone(),
                                due,
                                refund,
                            },
                        );
                    }
                    SyntheticOutstanding::<T>::mutate(netuid, |s| {
                        *s = s.saturating_sub(pos.minted)
                    });
                    Self::remove_position(owner, netuid, &pos);
                    Self::deposit_event(Event::ShortSettleFailed {
                        who: owner.clone(),
                        netuid,
                        funds: pos.funds(),
                        deposit: pos.deposit,
                    });
                }
            }
        }

        /// O(1) weight for [`Self::freeze_dissolution_price`]: Dissolutions
        /// mutate + pool tao / PoolTaoEma / three issuance keys + trie proof.
        ///
        /// `alpha_issuance` reads `SubnetAlphaIn`, `SubnetAlphaOut`, and
        /// `BalancerAlphaReservoir` separately — budget all three.
        pub fn freeze_dissolution_weight() -> Weight {
            let db = T::DbWeight::get();
            const TRIE_PATH: u64 = 3072;
            let value = (Dissolution::<BlockNumberFor<T>>::max_encoded_len() as u64)
                .saturating_add(u64::max_encoded_len() as u64) // pool tao
                .saturating_add(U64F64::max_encoded_len() as u64) // PoolTaoEma
                .saturating_add(u64::max_encoded_len() as u64) // SubnetAlphaIn
                .saturating_add(u64::max_encoded_len() as u64) // SubnetAlphaOut
                .saturating_add(u64::max_encoded_len() as u64); // BalancerAlphaReservoir
            db.reads_writes(6, 1)
                .saturating_add(Weight::from_parts(0, value.saturating_add(TRIE_PATH * 6)))
        }

        /// O(1) weight for [`Self::finish_dissolution`]: six storage removals
        /// with trie proof.
        pub fn finish_dissolution_weight() -> Weight {
            let db = T::DbWeight::get();
            const TRIE_PATH: u64 = 3072;
            db.writes(6)
                .saturating_add(Weight::from_parts(0, TRIE_PATH * 6))
        }

        pub fn finish_dissolution(netuid: NetUid) -> DispatchResult {
            Dissolutions::<T>::remove(netuid);
            PositionCount::<T>::remove(netuid);
            RankRing::<T>::remove(netuid);
            RankPrice::<T>::remove(netuid);
            PoolTaoEma::<T>::remove(netuid);
            NotionalEma::<T>::remove(netuid);
            Ok(())
        }

        pub fn is_lending_custody(
            hotkey: &T::AccountId,
            coldkey: &T::AccountId,
            netuid: NetUid,
        ) -> bool {
            Self::is_custody_hotkey(hotkey)
                && (*coldkey == Self::reserve_account(netuid)
                    || EscrowOwner::<T>::contains_key(netuid, coldkey))
        }

        /// Re-key `old`'s positions to `new`. Custody funds stay put. Refused
        /// while any of the subnets dissolves or when `new` already holds a
        /// position on the same subnet.
        #[frame_support::transactional]
        pub fn on_coldkey_swap(old: &T::AccountId, new: &T::AccountId) -> DispatchResult {
            if old == new {
                return Ok(());
            }
            let positions: alloc::vec::Vec<_> = Positions::<T>::iter_prefix(old).collect();
            for (netuid, pos) in positions {
                Self::ensure_not_dissolving(netuid)?;
                ensure!(
                    !Positions::<T>::contains_key(new, netuid),
                    Error::<T>::PositionExists
                );
                Positions::<T>::remove(old, netuid);
                Positions::<T>::insert(new, netuid, &pos);
                OpenByNetuid::<T>::remove(netuid, old);
                OpenByNetuid::<T>::insert(netuid, new, ());
                EscrowOwner::<T>::insert(netuid, &pos.custody, new);
                if Due::<T>::take(pos.due, (old.clone(), netuid)).is_some() {
                    Due::<T>::insert(pos.due, (new.clone(), netuid), ());
                }
                if let Some(seq) = DeleverageQueue::<T>::take(netuid, old) {
                    DeleverageQueue::<T>::insert(netuid, new, seq);
                    DeleverageCursor::<T>::kill();
                }
            }
            Ok(())
        }

        // ---- quotes -------------------------------------------------------------

        /// Proceeds, minimum collateral and deposit for opening a fresh `mint`
        /// short.
        pub fn quote_open_short(
            netuid: NetUid,
            mint: u64,
        ) -> Result<ShortOpenQuote, DispatchError> {
            Self::ensure_tradeable(netuid)?;
            ensure!(mint > 0, Error::<T>::ZeroAmount);
            let params = SynthParamsStore::<T>::get();
            ensure!(
                T::Pool::pool_tao(netuid).to_u64() >= params.min_pool_depth,
                Error::<T>::PoolTooShallow
            );
            ensure!(
                !BreakerTripped::<T>::get(netuid),
                Error::<T>::DilutionBreakerActive
            );
            Self::ensure_min_notional(netuid, mint, &params)?;
            Self::ensure_net_mint_room(netuid, mint, &params)?;
            let proceeds = T::Pool::quote_sell(netuid, mint.into())?.to_u64();
            let pool = Self::pool_state(netuid);
            let gross = T::Pool::quote_sell_on(
                pool.tao.into(),
                pool.alpha.into(),
                pool.weight,
                mint.into(),
            )
            .to_u64();
            let after_sale = PoolState {
                tao: pool.tao.saturating_sub(gross),
                alpha: pool.alpha.saturating_add(mint),
                weight: pool.weight,
            };
            let cost = Self::book_need(
                netuid,
                after_sale,
                Self::reference_price(netuid, after_sale),
                SyntheticOutstanding::<T>::get(netuid).saturating_add(mint),
                mint,
                &params,
            )
            .ok_or(Error::<T>::CoverageNotMet)?;
            let notional = Self::notional(netuid, mint);
            Ok(ShortOpenQuote {
                proceeds,
                required_collateral: cost
                    .saturating_sub(proceeds)
                    .max(Self::day_interest(netuid, mint, &params)),
                deposit: params.deposit,
                notional,
                apr_bps: params.apr_bps,
                annual_interest: Self::mul_bps_u32(notional, params.apr_bps),
                in_prune_zone: Self::in_prune_zone(netuid, &params),
            })
        }

        /// Buy-back cost and refund for closing `alpha` of `owner`'s short now.
        pub fn quote_close_short(
            owner: &T::AccountId,
            netuid: NetUid,
            alpha: u64,
        ) -> Result<ShortCloseQuote, DispatchError> {
            Self::ensure_tradeable(netuid)?;
            let pos = Self::position(owner, netuid)?;
            let params = SynthParamsStore::<T>::get();
            let (owed, _) = Self::accrued(netuid, &pos, &params);
            let alpha = alpha.min(pos.minted);
            ensure!(alpha > 0, Error::<T>::ZeroAmount);
            let tao_in =
                Self::buy_cost_fee_inclusive(netuid, alpha).ok_or(Error::<T>::SlippageExceeded)?;
            let funds = pos.funds().saturating_sub(owed.min(pos.collateral));
            ensure!(tao_in <= funds, Error::<T>::InsufficientFunds);
            let refund = if alpha == pos.minted {
                funds.saturating_sub(tao_in).saturating_add(pos.deposit)
            } else {
                0
            };
            Ok(ShortCloseQuote {
                alpha,
                tao_in,
                refund,
            })
        }

        pub fn short_health(owner: &T::AccountId, netuid: NetUid) -> Option<ShortHealth> {
            let pos = Positions::<T>::get(owner, netuid)?;
            let params = SynthParamsStore::<T>::get();
            Some(ShortHealth {
                minted: pos.minted,
                collateral: pos.collateral,
                proceeds: pos.proceeds,
                zone_minted: pos.zone_minted,
                accrued_interest: Self::accrued(netuid, &pos, &params).0,
                equity_twap: Self::equity_twap(netuid, &pos),
                coverage_cost: Self::coverage_cost(netuid, pos.minted, &params),
                day_interest: Self::day_interest(netuid, pos.minted, &params),
                deleveraging: pos.state != ShortState::Open,
            })
        }

        fn mul_bps_u32(amount: u64, bps: u32) -> u64 {
            u64::try_from(u128::from(amount).saturating_mul(u128::from(bps)) / BPS)
                .unwrap_or(u64::MAX)
        }

        // ---- invariants ---------------------------------------------------------

        /// I1, I11, index consistency, and the deleveraging book.
        #[cfg(any(test, feature = "try-runtime"))]
        pub fn do_try_state() -> Result<(), sp_runtime::TryRuntimeError> {
            use alloc::collections::BTreeMap;
            let mut minted: BTreeMap<NetUid, u64> = Default::default();
            let mut deleveraging: BTreeMap<NetUid, u64> = Default::default();
            let mut per_subnet: BTreeMap<NetUid, u32> = Default::default();
            let mut total = 0u32;
            for (owner, netuid, pos) in Positions::<T>::iter() {
                let m = minted.entry(netuid).or_default();
                *m = m.saturating_add(pos.minted);
                let c = per_subnet.entry(netuid).or_default();
                *c = c.saturating_add(1);
                total = total.saturating_add(1);
                ensure!(pos.minted > 0, "position with nothing minted");
                ensure!(
                    pos.zone_minted <= pos.minted,
                    "I11: zone_minted above minted"
                );
                ensure!(
                    pos.zone_proceeds <= pos.proceeds.saturating_add(pos.collateral),
                    "zone_proceeds above funds"
                );
                ensure!(
                    OpenByNetuid::<T>::contains_key(netuid, &owner),
                    "position missing from OpenByNetuid"
                );
                ensure!(
                    EscrowOwner::<T>::get(netuid, &pos.custody).as_ref() == Some(&owner),
                    "custody not mapped to owner"
                );
                ensure!(
                    Dissolutions::<T>::contains_key(netuid)
                        || Due::<T>::contains_key(pos.due, (owner.clone(), netuid)),
                    "position not scheduled for interest"
                );
                let queued = DeleverageQueue::<T>::contains_key(netuid, &owner);
                ensure!(
                    (pos.state != ShortState::Open) == queued,
                    "deleverage queue out of sync with position state"
                );
                if queued {
                    let d = deleveraging.entry(netuid).or_default();
                    *d = d.saturating_add(pos.minted);
                }
            }
            for (netuid, s) in SyntheticOutstanding::<T>::iter() {
                ensure!(
                    minted.get(&netuid).copied().unwrap_or(0) == s,
                    "I1: SyntheticOutstanding != sum of minted"
                );
            }
            for (netuid, m) in minted.iter() {
                ensure!(
                    SyntheticOutstanding::<T>::get(netuid) == *m,
                    "I1: minted without SyntheticOutstanding"
                );
                ensure!(
                    PositionCount::<T>::get(netuid) == per_subnet.get(netuid).copied().unwrap_or(0),
                    "PositionCount drift"
                );
            }
            for (netuid, d) in DeleveragingMinted::<T>::iter() {
                ensure!(
                    deleveraging.get(&netuid).copied().unwrap_or(0) == d,
                    "DeleveragingMinted drift"
                );
            }
            for (netuid, d) in deleveraging.iter() {
                ensure!(
                    DeleveragingMinted::<T>::get(netuid) == *d,
                    "deleveraging minted without DeleveragingMinted"
                );
            }
            ensure!(TotalPositions::<T>::get() == total, "TotalPositions drift");
            ensure!(
                OpenByNetuid::<T>::iter().count() as u32 == total,
                "OpenByNetuid has stale entries"
            );
            ensure!(
                EscrowOwner::<T>::iter().count() as u32 == total,
                "EscrowOwner has stale entries"
            );
            for (netuid, owner) in DeleverageQueue::<T>::iter_keys() {
                ensure!(
                    Positions::<T>::contains_key(&owner, netuid),
                    "deleverage queue entry without position"
                );
            }
            // I7: every BlockNetMinted tag must be for the current block.
            // Stale tags are cleared in on_finalize; try_state runs after that
            // (or mid-block with only fresh tags). Callers that advance the
            // block without finalize must clear or refresh first.
            let now = frame_system::Pallet::<T>::block_number();
            for (_, (at, _)) in BlockNetMinted::<T>::iter() {
                ensure!(at == now, "I7: BlockNetMinted stale tag");
            }

            // I2: alpha issuance must cover synthetic + residual.
            for (netuid, reserve) in ResidualReserve::<T>::iter() {
                let issued = T::Pool::alpha_issuance(netuid).to_u64();
                let synthetic = SyntheticOutstanding::<T>::get(netuid);
                let residual = ResidualAlpha::<T>::get(netuid);
                ensure!(
                    issued >= synthetic.saturating_add(residual),
                    "I2: issuance below synthetic + residual"
                );
                // I5/I9 (reserve cap): pool can shrink after the reserve was
                // written, making reserve > cap; that is a valid transient
                // state — do not enforce.
                let _ = reserve;
            }
            // I8: residual alpha with nothing synthetic still needs retire
            // funding on the reserve, else the residual is stranded.
            for (netuid, residual) in ResidualAlpha::<T>::iter() {
                if residual > 0 && SyntheticOutstanding::<T>::get(netuid) == 0 {
                    ensure!(
                        ResidualReserve::<T>::get(netuid) > 0,
                        "I8: residual alpha without retire funding"
                    );
                }
            }

            // I3: each custody account must hold at least funds + deposit.
            for (owner, netuid, pos) in Positions::<T>::iter() {
                let held = pos.funds().saturating_add(pos.deposit);
                if held > 0 {
                    let bal = T::Pool::free_tao(&pos.custody).to_u64();
                    ensure!(bal >= held, "I3: custody free balance below funds+deposit");
                }
                let _ = (owner, netuid);
            }
            // I3: reserve accounts hold at least their recorded ResidualReserve.
            for (netuid, reserve_stored) in ResidualReserve::<T>::iter() {
                if reserve_stored > 0 {
                    let reserve_acc = Self::reserve_account(netuid);
                    let reserve_bal = T::Pool::free_tao(&reserve_acc).to_u64();
                    ensure!(
                        reserve_bal >= reserve_stored,
                        "I3: reserve account balance below ResidualReserve"
                    );
                }
            }
            Ok(())
        }
    }

    impl<T: Config> LendingInterface<T::AccountId> for Pallet<T> {
        fn custody_accounts(netuid: NetUid) -> Option<(T::AccountId, T::AccountId)> {
            Some((Self::reserve_account(netuid), PalletHotkey::<T>::get()?))
        }
        fn has_positions(owner: &T::AccountId) -> bool {
            Positions::<T>::iter_prefix(owner).next().is_some()
        }
        fn count_positions_bounded(owner: &T::AccountId, limit: u32) -> u32 {
            Positions::<T>::iter_prefix(owner)
                .take(limit as usize)
                .count() as u32
        }
        fn max_positions() -> u32 {
            T::MaxTotalPositions::get()
        }
        fn on_coldkey_swap(old: &T::AccountId, new: &T::AccountId) -> DispatchResult {
            Self::on_coldkey_swap(old, new)
        }
        fn start_dissolution(netuid: NetUid) -> DispatchResult {
            Self::start_dissolution(netuid)
        }
        fn cached_network_to_prune() -> Option<NetUid> {
            Self::cached_network_to_prune()
        }
        fn lending_ranks_active() -> bool {
            Enabled::<T>::get()
        }
        fn start_dissolution_weight() -> Weight {
            Self::start_dissolution_weight()
        }
        fn settle_positions_on_dissolution(netuid: NetUid, meter: &mut WeightMeter) -> bool {
            Self::settle_positions_on_dissolution(netuid, meter)
        }
        fn finish_dissolution(netuid: NetUid) -> DispatchResult {
            Self::finish_dissolution(netuid)
        }
        fn frozen_dissolution_price(netuid: NetUid) -> Option<U64F64> {
            Dissolutions::<T>::get(netuid).map(|d| d.price)
        }
        fn freeze_dissolution_price(netuid: NetUid) -> Option<U64F64> {
            Dissolutions::<T>::mutate_exists(netuid, |opt| {
                let d = opt.as_mut()?;
                let price = Self::dissolution_rate(netuid);
                d.price = price;
                Some(price)
            })
        }
        fn freeze_dissolution_weight() -> Weight {
            Self::freeze_dissolution_weight()
        }
        fn finish_dissolution_weight() -> Weight {
            Self::finish_dissolution_weight()
        }
        fn is_lending_custody(
            hotkey: &T::AccountId,
            coldkey: &T::AccountId,
            netuid: NetUid,
        ) -> bool {
            Self::is_lending_custody(hotkey, coldkey, netuid)
        }
        fn is_custody_hotkey(hotkey: &T::AccountId) -> bool {
            Self::is_custody_hotkey(hotkey)
        }
        fn on_subnet_registered(netuid: NetUid) {
            // A reused netuid must not inherit the pruned subnet's prices.
            // Leave RankRing empty so it warms organically from real blocks.
            TwapRing::<T>::remove(netuid);
            RankRing::<T>::remove(netuid);
            RankPrice::<T>::remove(netuid);
            RankRingGeneration::<T>::remove(netuid);
            PoolTaoEma::<T>::remove(netuid);
            NotionalEma::<T>::remove(netuid);
        }
        fn update_dereg_moving_price(netuid: NetUid) {
            Self::update_dereg_moving_price(netuid)
        }
        fn dereg_moving_price(netuid: NetUid) -> Option<U64F64> {
            if !Enabled::<T>::get() {
                return None;
            }
            Self::active_rank_price(netuid)
        }
        fn synthetic_and_residual(netuid: NetUid) -> (u64, u64) {
            // Always subtract outstanding synthetic/residual from schedule
            // issuance — even while lending is disabled. A pause must not
            // reclassify leftover short/residual alpha as organic issuance.
            (
                SyntheticOutstanding::<T>::get(netuid),
                ResidualAlpha::<T>::get(netuid),
            )
        }
    }
}
