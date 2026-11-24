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
    weights::WeightMeter,
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
    /// Non-root live subnets (TWAP sampling and the prune-zone floor).
    fn live_subnets() -> alloc::vec::Vec<NetUid>;

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
    /// at most `max_tao_in`, and burn every bought unit. Returns the TAO spent.
    fn buy_and_burn(
        netuid: NetUid,
        custody_cold: &AccountId,
        custody_hot: &AccountId,
        alpha: AlphaBalance,
        max_tao_in: TaoBalance,
    ) -> Result<TaoBalance, DispatchError>;

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
    /// Call before the pool is touched.
    fn start_dissolution(netuid: NetUid) -> DispatchResult;
    /// Retired with lifts; kept so dissolution callers stay unchanged.
    fn readd_lifts_for_dissolution(netuid: NetUid) -> DispatchResult {
        let _ = netuid;
        Ok(())
    }
    /// Settle every short at `r` (zone alpha at its proceeds) into the staker
    /// pot and add the residual reserve. Runs after the dissolution alpha
    /// count and before stakers are paid. Returns `true` when done.
    fn settle_positions_on_dissolution(netuid: NetUid, meter: &mut WeightMeter) -> bool;
    fn finish_dissolution(netuid: NetUid) -> DispatchResult;
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
    /// Retired long add-back. Shorts never add back, so emission uses spot.
    fn emission_reserve_offsets(netuid: NetUid) -> (u64, u64, u64, u64, u64) {
        let _ = netuid;
        (0, 0, 0, 0, 0)
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
/// Subnets whose residual alpha the reserve may retire per block.
pub const RESIDUAL_RETIRES_PER_BLOCK: u32 = 4;
/// Empty `Due` buckets skipped per idle pass before yielding.
pub const MAX_EMPTY_DUE_BUMPS: u32 = 64;
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
#[freeze_struct("2bca3316f5ce8016")]
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
    /// EMA of pool TAO (`T_ema`, P12).
    #[pallet::storage]
    pub type PoolTaoEma<T: Config> = StorageMap<_, Identity, NetUid, u64, OptionQuery>;
    /// Positions in [`ShortState::Deleveraging`], by subnet.
    #[pallet::storage]
    pub type DeleverageQueue<T: Config> =
        StorageDoubleMap<_, Identity, NetUid, Blake2_128Concat, T::AccountId, (), OptionQuery>;
    /// Σ `minted` over the subnet's deleveraging positions.
    #[pallet::storage]
    pub type DeleveragingMinted<T> = StorageMap<_, Identity, NetUid, u64, ValueQuery>;
    /// The block's pace budget and deleveraging book on the subnet, fixed at
    /// its first deleverage step so fills are pro rata.
    #[pallet::storage]
    pub type DeleverageBudget<T: Config> =
        StorageMap<_, Identity, NetUid, (BlockNumberFor<T>, u64, u64), OptionQuery>;
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
            assert!(T::MaxPositionsPerSubnet::get() > 0);
            assert!(T::MaxPositionsPerSubnet::get() <= T::MaxTotalPositions::get());
            assert!(SynthParams::default().is_valid());
        }

        fn on_initialize(now: BlockNumberFor<T>) -> Weight {
            let params = SynthParamsStore::<T>::get();
            let db = T::DbWeight::get();
            // The params read plus `on_finalize`'s clear of `BlockNetMinted`.
            let mut weight = db.reads_writes(2, 1);
            if Self::block_u64(now).checked_rem(u64::from(params.twap_interval_blocks.max(1)))
                == Some(0)
            {
                let subnets = T::Pool::live_subnets();
                for netuid in subnets.iter() {
                    Self::sample_twap(*netuid, now, &params);
                }
                weight = weight.saturating_add(
                    T::WeightInfo::twap_sample().saturating_mul(subnets.len() as u64),
                );
            }
            let steps = Self::process_deleverage_queue(DELEVERAGE_STEPS_PER_BLOCK, &params);
            let retires = Self::retire_residuals(RESIDUAL_RETIRES_PER_BLOCK, &params);
            weight
                .saturating_add(T::WeightInfo::deleverage_step().saturating_mul(u64::from(steps)))
                .saturating_add(T::WeightInfo::retire_residual().saturating_mul(u64::from(retires)))
        }

        fn on_finalize(_now: BlockNumberFor<T>) {
            let _ = BlockNetMinted::<T>::clear(u32::MAX, None);
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
        #[pallet::weight(T::WeightInfo::open_short())]
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
        #[pallet::weight(T::WeightInfo::increase_short())]
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
        #[pallet::weight(T::WeightInfo::set_enabled())]
        pub fn set_enabled(origin: OriginFor<T>, enabled: bool) -> DispatchResult {
            ensure_root(origin)?;
            Enabled::<T>::put(enabled);
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

        /// Dereg-ranking SMA, falling back to the emission EMA before the first
        /// sample (the same fallback subtensor's ranking uses).
        pub fn rank_price(netuid: NetUid) -> U64F64 {
            RankPrice::<T>::get(netuid).unwrap_or_else(|| T::Pool::moving_alpha_price(netuid))
        }

        /// The subnet's rank SMA is within `zone_bps` of the lowest non-immune
        /// rank SMA, so it is a prune candidate (P16).
        pub fn in_prune_zone(netuid: NetUid, params: &SynthParams) -> bool {
            if T::Pool::prune_immune(netuid) {
                return false;
            }
            let own = Self::rank_price(netuid);
            let floor = T::Pool::live_subnets()
                .into_iter()
                .filter(|n| *n == netuid || !T::Pool::prune_immune(*n))
                .map(Self::rank_price)
                .fold(own, |lo, p| lo.min(p));
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
                PositionCount::<T>::get(netuid) < T::MaxPositionsPerSubnet::get()
                    && TotalPositions::<T>::get() < T::MaxTotalPositions::get(),
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
            let estimate = Self::buy_cost_on(Self::pool_state(netuid), alpha)
                .ok_or(Error::<T>::SlippageExceeded)?;
            ensure!(estimate <= funds, Error::<T>::InsufficientFunds);
            ensure!(estimate <= max_tao_in, Error::<T>::SlippageExceeded);
            let spent = Self::buy_back(netuid, &pos.custody, alpha, max_tao_in.min(funds))?;
            pos.debit(spent);
            Self::burn_from_position(netuid, &mut pos, alpha);
            let refund = if pos.minted == 0 {
                Self::finish_position(&owner, netuid, &pos)?
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
                burned: alpha,
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
        ) -> Result<u64, DispatchError> {
            let hot = Self::custody_hotkey()?;
            Ok(
                T::Pool::buy_and_burn(netuid, payer, &hot, alpha.into(), max_tao_in.into())?
                    .to_u64(),
            )
        }

        /// Record `alpha` of `pos` bought back and burned.
        fn burn_from_position(netuid: NetUid, pos: &mut PositionOf<T>, alpha: u64) {
            pos.reduce_minted(alpha);
            SyntheticOutstanding::<T>::mutate(netuid, |s| *s = s.saturating_sub(alpha));
            if pos.state != ShortState::Open {
                DeleveragingMinted::<T>::mutate(netuid, |d| *d = d.saturating_sub(alpha));
            }
            Self::add_net_minted(netuid, i128::from(alpha).saturating_neg());
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
        /// the first call in the block.
        fn deleverage_budget(netuid: NetUid, params: &SynthParams) -> (u64, u64) {
            let now = frame_system::Pallet::<T>::block_number();
            if let Some((at, budget, book)) = DeleverageBudget::<T>::get(netuid)
                && at == now
            {
                return (budget, book);
            }
            let budget = Self::pace_room(netuid, params);
            let book = DeleveragingMinted::<T>::get(netuid);
            DeleverageBudget::<T>::insert(netuid, (now, budget, book));
            (budget, book)
        }

        // ---- position lifecycle --------------------------------------------

        /// Refund remaining funds plus the deposit and drop every index.
        fn finish_position(
            owner: &T::AccountId,
            netuid: NetUid,
            pos: &PositionOf<T>,
        ) -> Result<u64, DispatchError> {
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
            DeleverageQueue::<T>::insert(netuid, owner, ());
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
        /// when its funds are.
        pub(crate) fn deleverage_step(owner: &T::AccountId, netuid: NetUid, params: &SynthParams) {
            let Some(mut pos) = Positions::<T>::get(owner, netuid) else {
                DeleverageQueue::<T>::remove(netuid, owner);
                return;
            };
            if Dissolutions::<T>::contains_key(netuid)
                || !T::Pool::subnet_exists(netuid)
                || Self::buybacks_paused(netuid, params)
            {
                return;
            }
            let funds = pos.funds();
            let (budget, book) = Self::deleverage_budget(netuid, params);
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
            let spend = share.min(funds).min(Self::pace_room(netuid, params));
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
                }
                return;
            }
            let bought =
                with_transaction(
                    || match Self::buy_back(netuid, &pos.custody, alpha, spend) {
                        Ok(spent) => TransactionOutcome::Commit(Ok(spent)),
                        Err(e) => TransactionOutcome::Rollback(Err(e)),
                    },
                );
            let Ok(spent) = bought else {
                return;
            };
            pos.debit(spent);
            Self::burn_from_position(netuid, &mut pos, alpha);
            Self::deposit_event(Event::DeleverageStep {
                who: owner.clone(),
                netuid,
                burned: alpha,
                tao_spent: spent,
            });
            if pos.minted == 0 {
                let refund = Self::finish_position(owner, netuid, &pos).unwrap_or_else(|_| {
                    Self::remove_position(owner, netuid, &pos);
                    0
                });
                Self::deposit_event(Event::ShortClosed {
                    who: owner.clone(),
                    netuid,
                    burned: alpha,
                    tao_spent: spent,
                    refund,
                });
            } else if pos.funds() == 0 {
                Self::default_position(owner, netuid, &pos);
            } else {
                Positions::<T>::insert(owner, netuid, &pos);
            }
        }

        /// Unbought alpha becomes residual (holder dilution). Leftover dust
        /// funds go to the reserve that retires it; the deposit burns (P19).
        pub(crate) fn default_position(owner: &T::AccountId, netuid: NetUid, pos: &PositionOf<T>) {
            let residual = pos.minted;
            SyntheticOutstanding::<T>::mutate(netuid, |s| *s = s.saturating_sub(residual));
            ResidualAlpha::<T>::mutate(netuid, |r| *r = r.saturating_add(residual));
            let dust = pos.funds();
            if dust > 0 {
                let reserve = Self::reserve_account(netuid);
                if let Ok(credited) =
                    T::Pool::collect_interest_tao(&pos.custody, &reserve, dust.into())
                {
                    ResidualReserve::<T>::mutate(netuid, |r| {
                        *r = r.saturating_add(credited.to_u64())
                    });
                }
            }
            if pos.deposit > 0 {
                let _ = T::Pool::burn_interest_tao(&pos.custody, pos.deposit.into());
            }
            Self::remove_position(owner, netuid, pos);
            Self::refresh_breaker(netuid, &SynthParamsStore::<T>::get());
            Self::deposit_event(Event::ShortDefaulted {
                who: owner.clone(),
                netuid,
                residual_alpha: residual,
            });
        }

        /// Serve up to `max` deleveraging positions, resuming after the cursor.
        /// Returns the number of steps taken.
        pub(crate) fn process_deleverage_queue(max: u32, params: &SynthParams) -> u32 {
            let keys: alloc::vec::Vec<(NetUid, T::AccountId)> = match DeleverageCursor::<T>::get() {
                Some((netuid, owner)) => DeleverageQueue::<T>::iter_keys_from(
                    DeleverageQueue::<T>::hashed_key_for(netuid, &owner),
                )
                .take(max as usize)
                .collect(),
                None => DeleverageQueue::<T>::iter_keys()
                    .take(max as usize)
                    .collect(),
            };
            if (keys.len() as u32) < max {
                DeleverageCursor::<T>::kill();
            } else if let Some(last) = keys.last() {
                DeleverageCursor::<T>::put(last.clone());
            }
            for (netuid, owner) in keys.iter() {
                Self::deleverage_step(owner, *netuid, params);
            }
            keys.len() as u32
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
                    Ok(spent) => TransactionOutcome::Commit(Ok(spent)),
                    Err(e) => TransactionOutcome::Rollback(Err(e)),
                });
            let Ok(spent) = bought else {
                return;
            };
            ResidualReserve::<T>::mutate(netuid, |r| *r = r.saturating_sub(spent));
            let left = residual.saturating_sub(alpha);
            if left == 0 {
                ResidualAlpha::<T>::remove(netuid);
            } else {
                ResidualAlpha::<T>::insert(netuid, left);
            }
            Self::add_net_minted(netuid, i128::from(alpha).saturating_neg());
            Self::refresh_breaker(netuid, params);
            Self::deposit_event(Event::ResidualRetired {
                netuid,
                alpha,
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

        fn notional(netuid: NetUid, minted: u64) -> u64 {
            Self::mul_price(minted, T::Pool::moving_alpha_price(netuid), false)
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
            let burned = paid.saturating_sub(to_reserve);
            T::Pool::burn_interest_tao(&pos.custody, burned.into())?;
            pos.collateral = pos.collateral.saturating_sub(paid);
            Self::deposit_event(Event::InterestCharged {
                who: owner.clone(),
                netuid,
                burned: paid.saturating_sub(reserved),
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
            let charge = T::WeightInfo::charge_interest();
            let probe = T::DbWeight::get().reads_writes(2, 1);
            let mut bumps = 0u32;
            while let Some(at) = NextDue::<T>::get() {
                if at > now || meter.try_consume(probe).is_err() {
                    return;
                }
                match Due::<T>::iter_key_prefix(at).next() {
                    None => {
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

        pub fn sample_twap(netuid: NetUid, now: BlockNumberFor<T>, params: &SynthParams) {
            let spot = T::Pool::current_alpha_price(netuid);
            let cap = params.twap_samples.clamp(1, MAX_TWAP_SAMPLES) as usize;
            TwapRing::<T>::mutate(netuid, |ring| {
                if ring.samples.len() > cap {
                    ring.samples.truncate(cap);
                    ring.cursor = 0;
                }
                if ring.samples.len() < cap {
                    let _ = ring.samples.try_push(spot);
                } else {
                    if let Some(slot) = ring.samples.get_mut(ring.cursor as usize) {
                        *slot = spot;
                    }
                    ring.cursor = ring
                        .cursor
                        .saturating_add(1)
                        .checked_rem(cap as u32)
                        .unwrap_or(0);
                }
                ring.last_sample = now;
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
            multiply_by_rational_with_rounding(
                u128::from(cost),
                u128::from(q_pos),
                u128::from(book),
                Rounding::Up,
            )
            .and_then(|v| u64::try_from(v).ok())
        }

        /// `max(EMA, spot, SMA30_rank)` on `pool` (P2).
        fn reference_price(netuid: NetUid, pool: PoolState) -> U64F64 {
            T::Pool::moving_alpha_price(netuid)
                .max(Self::price_on(pool))
                .max(Self::rank_price(netuid))
        }

        /// Whole-book coverage need of a position holding `minted` on the live
        /// pool and book.
        pub fn coverage_cost(netuid: NetUid, minted: u64, params: &SynthParams) -> Option<u64> {
            let pool = Self::pool_state(netuid);
            Self::book_need(
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

        pub fn start_dissolution(netuid: NetUid) -> DispatchResult {
            Self::ensure_not_dissolving(netuid)?;
            Dissolutions::<T>::insert(
                netuid,
                Dissolution {
                    frozen_at: frame_system::Pallet::<T>::block_number(),
                    price: Self::dissolution_rate(netuid),
                },
            );
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
            let reserve_tao = ResidualReserve::<T>::take(netuid);
            if reserve_tao > 0 {
                let _ = T::Pool::pay_into_pot(
                    &Self::reserve_account(netuid),
                    netuid,
                    reserve_tao.into(),
                );
            }
            ResidualAlpha::<T>::remove(netuid);
            SyntheticOutstanding::<T>::remove(netuid);
            DeleveragingMinted::<T>::remove(netuid);
            DeleverageBudget::<T>::remove(netuid);
            BreakerTripped::<T>::remove(netuid);
            TwapRing::<T>::remove(netuid);
            PaceAnchor::<T>::remove(netuid);
            BlockNetMinted::<T>::remove(netuid);
            true
        }

        fn settle_one_on_dissolution(owner: &T::AccountId, netuid: NetUid, rate: U64F64) {
            let Some(pos) = Positions::<T>::get(owner, netuid) else {
                OpenByNetuid::<T>::remove(netuid, owner);
                return;
            };
            let (pot, due) = match T::Pool::subnet_pot_account(netuid) {
                Some(pot) => (pot, Self::dissolution_owed(&pos, rate)),
                None => (owner.clone(), 0),
            };
            let refund = pos.funds().saturating_sub(due).saturating_add(pos.deposit);
            let paid = match T::Pool::terminal_split_tao(
                &pos.custody,
                &pot,
                owner,
                due.into(),
                refund.into(),
            ) {
                Ok((recovered, _)) => {
                    T::Pool::credit_pot_reserve(netuid, recovered);
                    recovered.to_u64()
                }
                Err(_) => 0,
            };
            SyntheticOutstanding::<T>::mutate(netuid, |s| *s = s.saturating_sub(pos.minted));
            Self::remove_position(owner, netuid, &pos);
            Self::deposit_event(Event::ShortSettledOnDissolution {
                who: owner.clone(),
                netuid,
                price: rate,
                paid,
            });
        }

        pub fn finish_dissolution(netuid: NetUid) -> DispatchResult {
            Dissolutions::<T>::remove(netuid);
            PositionCount::<T>::remove(netuid);
            RankRing::<T>::remove(netuid);
            RankPrice::<T>::remove(netuid);
            PoolTaoEma::<T>::remove(netuid);
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
                if DeleverageQueue::<T>::take(netuid, old).is_some() {
                    DeleverageQueue::<T>::insert(netuid, new, ());
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
            let estimate = Self::buy_cost_on(Self::pool_state(netuid), alpha)
                .ok_or(Error::<T>::SlippageExceeded)?;
            let tao_in = Self::search_min(estimate, |tao| {
                T::Pool::quote_buy(netuid, tao.into()).is_ok_and(|out| out.to_u64() >= alpha)
            })
            .ok_or(Error::<T>::SlippageExceeded)?;
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
        fn settle_positions_on_dissolution(netuid: NetUid, meter: &mut WeightMeter) -> bool {
            Self::settle_positions_on_dissolution(netuid, meter)
        }
        fn finish_dissolution(netuid: NetUid) -> DispatchResult {
            Self::finish_dissolution(netuid)
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
            TwapRing::<T>::remove(netuid);
            RankRing::<T>::remove(netuid);
            RankPrice::<T>::remove(netuid);
            PoolTaoEma::<T>::remove(netuid);
        }
        fn update_dereg_moving_price(netuid: NetUid) {
            Self::update_dereg_moving_price(netuid)
        }
        fn dereg_moving_price(netuid: NetUid) -> Option<U64F64> {
            Self::dereg_moving_price(netuid)
        }
        fn synthetic_and_residual(netuid: NetUid) -> (u64, u64) {
            (
                SyntheticOutstanding::<T>::get(netuid),
                ResidualAlpha::<T>::get(netuid),
            )
        }
    }
}
