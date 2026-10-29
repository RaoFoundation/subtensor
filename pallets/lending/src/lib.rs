//! Lift-based escrowed lending on Balancer pools (spec v5.1 / Jacob §13.2).
//!
//! Liquidity is pulled from the pool only when someone borrows (`lift`). Shorts
//! force-sell borrowed alpha into escrowed TAO; longs force-buy with borrowed TAO
//! into escrowed alpha. No free borrowed assets are delivered. Interest burns.
//! Dissolution settlement rate uses a TAO EMA hardened against one-block pumps
//! (§13.2): `r = min(T_flag, t_ema_flag) / D`, then capped by moving alpha price.
#![cfg_attr(not(feature = "std"), no_std)]
#![allow(
    clippy::too_many_arguments,
    clippy::arithmetic_side_effects,
    clippy::collapsible_if,
    clippy::let_and_return
)]

extern crate alloc;
pub use pallet::*;
#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
pub mod migrations;
#[cfg(test)]
mod tests;
pub mod weights;

/// Bound on `ActiveNets` walked per block. Matches `GLOBAL_MAX_SUBNET_COUNT` so
/// every seeded subnet can age its reference / `G_ema` without exceeding the
/// production block weight budget (`update_reference × 4096` ≪ block limit).
pub const MAX_ACTIVE_NET_UPDATES: u64 = 4_096;

type MigrationKeyMaxLen = frame_support::traits::ConstU32<128>;

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use frame_support::{PalletId, pallet_prelude::*, transactional, weights::WeightMeter};
use frame_system::pallet_prelude::*;
use scale_info::TypeInfo;
use sp_runtime::traits::{AccountIdConversion, Hash, SaturatedConversion, Saturating};
use substrate_fixed::{
    transcendental::{exp, ln},
    types::{I64F64, U64F64},
};
use subtensor_macros::freeze_struct;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};
use subtensor_swap_interface::OrderSwapInterface;
use weights::WeightInfo;

/// Pool + custody adapter used by lending. Implementations must be transactional
/// for every asset-moving method.
pub trait LendingPoolInterface<AccountId>: OrderSwapInterface<AccountId> {
    fn subnet_exists(netuid: NetUid) -> bool;
    fn owner_allowed(_owner: &AccountId) -> bool {
        true
    }
    fn quote_sell(netuid: NetUid, alpha: AlphaBalance) -> Result<TaoBalance, DispatchError>;
    fn quote_buy(netuid: NetUid, tao: TaoBalance) -> Result<AlphaBalance, DispatchError>;
    fn fast_alpha_price(_netuid: NetUid) -> Option<U64F64> {
        None
    }
    fn max_buy_input(_netuid: NetUid) -> TaoBalance {
        u64::MAX.into()
    }
    fn max_sell_input(_netuid: NetUid) -> AlphaBalance {
        u64::MAX.into()
    }
    fn quote_may_round_to_zero(_error: &DispatchError) -> bool {
        false
    }
    fn buy_spendable_tao(_account: &AccountId) -> TaoBalance {
        u64::MAX.into()
    }
    fn refund_dissolution_tao(
        from: &AccountId,
        to: &AccountId,
        amount: TaoBalance,
    ) -> Result<TaoBalance, DispatchError> {
        Self::transfer_tao(from, to, amount)?;
        Ok(amount)
    }
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
    fn collect_interest_tao(
        from: &AccountId,
        to: &AccountId,
        amount: TaoBalance,
    ) -> Result<TaoBalance, DispatchError> {
        Self::transfer_tao(from, to, amount)?;
        Ok(amount)
    }
    fn burn_interest_tao(account: &AccountId, amount: TaoBalance) -> DispatchResult;
    fn lock_alpha_collateral(
        from: &AccountId,
        source_hotkey: &AccountId,
        escrow: &AccountId,
        custody_hotkey: &AccountId,
        netuid: NetUid,
        amount: AlphaBalance,
    ) -> DispatchResult {
        Self::transfer_staked_alpha(
            from,
            source_hotkey,
            escrow,
            custody_hotkey,
            netuid,
            amount,
            true,
            false,
        )
    }
    fn repay_alpha(
        from: &AccountId,
        source_hotkey: &AccountId,
        vault: &AccountId,
        custody_hotkey: &AccountId,
        netuid: NetUid,
        amount: AlphaBalance,
    ) -> DispatchResult {
        Self::transfer_staked_alpha(
            from,
            source_hotkey,
            vault,
            custody_hotkey,
            netuid,
            amount,
            true,
            false,
        )
    }
    fn transfer_dissolution_alpha(
        from: &AccountId,
        to: &AccountId,
        hotkey: &AccountId,
        netuid: NetUid,
        amount: AlphaBalance,
    ) -> DispatchResult {
        Self::transfer_staked_alpha(from, hotkey, to, hotkey, netuid, amount, false, false)
    }

    /// Live pool TAO reserve (`y` / SubnetTAO).
    fn pool_tao(netuid: NetUid) -> TaoBalance;
    /// Live pool alpha reserve (`x` / SubnetAlphaIn).
    fn pool_alpha(netuid: NetUid) -> AlphaBalance;
    /// Balancer quote-weight parts for hypothetical quotes.
    fn quote_weight_parts(netuid: NetUid) -> u64;
    /// `y*` such that a fee-free buy of `ref_tao` moves price by `move_bps`.
    fn buy_depth_threshold(netuid: NetUid, ref_tao: TaoBalance, move_bps: u16) -> TaoBalance;
    fn quote_sell_on(
        tao_reserve: TaoBalance,
        alpha_reserve: AlphaBalance,
        quote_weight_parts: u64,
        delta_alpha: AlphaBalance,
    ) -> TaoBalance;
    fn quote_buy_on(
        tao_reserve: TaoBalance,
        alpha_reserve: AlphaBalance,
        quote_weight_parts: u64,
        delta_tao: TaoBalance,
    ) -> AlphaBalance;

    /// Lift `g = numer/denom` from the pool, debit SubnetTAO/AlphaIn/TotalStake,
    /// and credit the returned legs into lending custody.
    fn lift_and_debit(
        netuid: NetUid,
        g_numer: u64,
        g_denom: u64,
        custody_cold: &AccountId,
        custody_hot: &AccountId,
    ) -> Result<(AlphaBalance, TaoBalance), DispatchError>;

    /// Re-add lift legs and credit SubnetTAO/AlphaIn/TotalStake as appropriate.
    fn readd_and_credit(
        netuid: NetUid,
        custody_cold: &AccountId,
        custody_hot: &AccountId,
        tao: TaoBalance,
        alpha: AlphaBalance,
    ) -> DispatchResult;

    /// Forced buy: spend custody TAO into the pool, credit alpha to custody stake.
    fn forced_buy_into_custody(
        custody_cold: &AccountId,
        custody_hot: &AccountId,
        netuid: NetUid,
        tao: TaoBalance,
    ) -> Result<AlphaBalance, DispatchError>;

    /// Forced sell of custody alpha into escrow TAO on `escrow_cold`.
    /// Fails if the realised TAO is below `min_tao_out` (pass zero to disable).
    fn forced_sell_into_escrow(
        custody_cold: &AccountId,
        custody_hot: &AccountId,
        escrow_cold: &AccountId,
        netuid: NetUid,
        alpha: AlphaBalance,
        min_tao_out: TaoBalance,
    ) -> Result<TaoBalance, DispatchError>;

    /// Unlocked stake available for collateral / raise sale (excludes locks).
    fn unlocked_stake(hotkey: &AccountId, coldkey: &AccountId, netuid: NetUid) -> AlphaBalance;

    /// Raise sale leg: unstake `alpha` to `coldkey` free TAO (`allow_partial = false`).
    fn unstake_for_raise(
        hotkey: &AccountId,
        coldkey: &AccountId,
        netuid: NetUid,
        alpha: AlphaBalance,
        min_tao_out: TaoBalance,
    ) -> Result<TaoBalance, DispatchError>;

    /// Eligible alpha for dissolution pot (ordinary claims). Lending adds escrow/collateral.
    fn eligible_alpha_for_dissolution(netuid: NetUid) -> Result<u128, DispatchError>;

    /// §13 lending settlement divisor: **all** subnet alpha **including pool alpha**,
    /// on every subnet (legacy and new). Must not omit `SubnetAlphaIn`. Typically
    /// `SubnetAlphaIn + SubnetAlphaOut` (u128). Used only for `r = pot / all_alpha`.
    fn all_alpha_for_lending_settlement(netuid: NetUid) -> u128;

    /// Slow moving alpha price used to cap dissolution settlement rate (§13.2).
    fn moving_alpha_price(netuid: NetUid) -> U64F64;

    /// Non-root live subnet netuids (for per-block TAO EMA updates).
    fn live_subnets() -> alloc::vec::Vec<NetUid>;

    /// Pay `amount` TAO from the dissolution pot (`SubnetTAO` + subnet account) to `to`.
    fn pay_from_pot(
        netuid: NetUid,
        to: &AccountId,
        amount: TaoBalance,
    ) -> Result<TaoBalance, DispatchError>;

    /// Burn `amount` TAO from the dissolution pot (pending long-interest conversion).
    fn burn_from_pot(netuid: NetUid, amount: TaoBalance) -> DispatchResult;

    /// Destroy custody alpha on dissolution without returning it to the pool
    /// (claims are settled via `r`, not staker payout).
    fn destroy_custody_alpha(
        coldkey: &AccountId,
        hotkey: &AccountId,
        netuid: NetUid,
        amount: AlphaBalance,
    ) -> DispatchResult;

    /// Block when the subnet was registered (`NetworkRegisteredAt`).
    fn network_registered_at(netuid: NetUid) -> u64;
    /// Owner start-call / subtoken trading enabled (`SubtokenEnabled`).
    fn subtoken_enabled(netuid: NetUid) -> bool;

    /// Subnet is present and uses the dynamic (lendable) mechanism.
    ///
    /// Unlike [`Self::subnet_exists`], this does **not** require subtoken /
    /// start-call. Used to seed references at registration so EMAs age during
    /// the new-subnet wait.
    fn network_alive(netuid: NetUid) -> bool {
        Self::subnet_exists(netuid)
    }

    /// Benchmark-only: deep-pool `netuid` so lift / depth checks pass.
    #[cfg(feature = "runtime-benchmarks")]
    fn benchmark_seed_pool(_netuid: NetUid, _tao: u64, _alpha: u64) {}

    /// Benchmark-only: fund free TAO on `who`.
    #[cfg(feature = "runtime-benchmarks")]
    fn benchmark_mint_tao(_who: &AccountId, _amount: u64) {}

    /// Benchmark-only: fund staked alpha for `(who, hotkey, netuid)`.
    #[cfg(feature = "runtime-benchmarks")]
    fn benchmark_mint_alpha(_who: &AccountId, _hotkey: &AccountId, _netuid: NetUid, _amount: u64) {}
}

/// Subtensor dissolution / hotkey integration.
pub trait LendingInterface<AccountId> {
    fn custody_accounts(netuid: NetUid) -> Option<(AccountId, AccountId)>;
    fn has_positions(owner: &AccountId) -> bool;
    /// Count open loans for `owner`, stopping at `limit` (admission sentinel).
    ///
    /// Coldkey-swap work folds this into the combined position bound so TAO-only
    /// shorts (no stake rows) cannot bypass swap weight / admission.
    fn count_positions_bounded(owner: &AccountId, limit: u32) -> u32 {
        let _ = (owner, limit);
        0
    }
    fn max_positions() -> u32;
    /// Re-key positions from `old` to `new` without moving custody funds (§14).
    fn on_coldkey_swap(old: &AccountId, new: &AccountId) -> DispatchResult;
    fn start_dissolution(netuid: NetUid) -> DispatchResult;
    /// Re-add held lift legs into reserves. Called from `do_dissolve_network` (§13).
    fn readd_lifts_for_dissolution(netuid: NetUid) -> DispatchResult;
    /// Settle all lending positions at fixed §13.2.2 rate
    /// `r = (min(T_flag, t_ema_flag) − Δ0) / ALL_alpha` (capped by moving price).
    /// Runs after main counts alpha and before stakers are paid. Returns `true` when done.
    fn settle_lending(netuid: NetUid, meter: &mut WeightMeter) -> bool;
    fn finish_dissolution(netuid: NetUid) -> DispatchResult;
    /// Whether `(hotkey, coldkey)` holds lending custody alpha on `netuid` that
    /// main's alpha count/payout must skip (collateral or long escrow).
    fn is_lending_custody(hotkey: &AccountId, coldkey: &AccountId, netuid: NetUid) -> bool;
    /// Whether `hotkey` is the lending custody hotkey (reject third-party stake, §14).
    fn is_custody_hotkey(hotkey: &AccountId) -> bool;
    /// Best-effort seed after a subnet is created (pool reserves exist).
    fn on_subnet_registered(netuid: NetUid) {
        let _ = netuid;
    }
    /// Best-effort seed when start-call enables the subtoken (fallback if
    /// registration-time price was zero).
    fn on_subtoken_enabled(netuid: NetUid) {
        let _ = netuid;
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
    fn readd_lifts_for_dissolution(_: NetUid) -> DispatchResult {
        Ok(())
    }
    fn settle_lending(_: NetUid, _: &mut WeightMeter) -> bool {
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
    fn on_subnet_registered(_: NetUid) {}
    fn on_subtoken_enabled(_: NetUid) {}
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
pub enum Side {
    Short,
    Long,
}

/// Root-settable capacity / LTV / interest / pacing parameters (bps where noted).
#[freeze_struct("9a49f40a29bbd017")]
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
pub struct LiftParams {
    /// Floor capacity as fraction of G_ref (default 3300 = 33%).
    pub floor_bps: u16,
    /// Thin-pool floor when G_ref < thin_tao (default 1500 = 15%).
    pub thin_floor_bps: u16,
    /// Reference buy size for impact rule (default 1000 τ).
    pub ref_tao: u64,
    /// Target price move for impact rule (default 1000 = 10%).
    pub move_bps: u16,
    /// Ceiling as fraction of G_ref (default 5000 = 50%).
    pub ceiling_bps: u16,
    /// Short side share of Cap (default 5000 = 50%).
    pub split_short_bps: u16,
    /// Optional rebalance target for split_short (0 = off).
    pub rebalance_target_bps: u16,
    /// Max rebalance of Cap per day in bps (unused while target is 0).
    pub rebalance_rate_bps: u16,
    /// Thin-pool threshold on G_ref (default 3000 τ).
    pub thin_tao: u64,
    /// Short LTV (default 2500 = 25%).
    pub ltv_short_bps: u16,
    /// Long LTV (default 2500 = 25%).
    pub ltv_long_bps: u16,
    /// Thin long LTV when G_ref < thin_tao (default 1500 = 15%).
    pub thin_ltv_long_bps: u16,
    /// APR in bps (default 10000 = 100%).
    pub apr_bps: u16,
    /// Per-block long-interest sell cap as spot move bps (default 1).
    pub slice_bps: u16,
    /// Skip paced sells while spot < crash_guard_bps of EMA (default 9500).
    pub crash_guard_bps: u16,
    /// Blocks for settlement TAO EMA half-life divisor (default 7200 ≈ 24h).
    pub settle_ema_blocks: u64,
    /// Minimum subnet age before lending opens (§2.3; default 50_400 ≈ 7 days).
    pub new_subnet_min_age: u64,
    /// Blocks after the gate over which Cap ramps 0→1 (§2.3; default 216_000 ≈ 30 days).
    pub new_subnet_ramp: u64,
    /// Max post-trade spot deviation from the lending EMA on the exploitable
    /// side (§5.4; default 1500 = 15%). Longs reject if spot > EMA·(1+band);
    /// shorts reject if spot < EMA·(1−band). Closes / repays / settles ignore it.
    pub ema_band_bps: u16,
}

impl Default for LiftParams {
    fn default() -> Self {
        Self {
            floor_bps: 3_300,
            thin_floor_bps: 1_500,
            ref_tao: 1_000_000_000_000,
            move_bps: 1_000,
            ceiling_bps: 5_000,
            split_short_bps: 5_000,
            rebalance_target_bps: 0,
            rebalance_rate_bps: 0,
            thin_tao: 3_000_000_000_000,
            ltv_short_bps: 2_500,
            ltv_long_bps: 2_500,
            thin_ltv_long_bps: 1_500,
            apr_bps: 10_000,
            slice_bps: 1,
            crash_guard_bps: 9_500,
            settle_ema_blocks: 7_200,
            new_subnet_min_age: 50_400,
            new_subnet_ramp: 216_000,
            ema_band_bps: 1_500,
        }
    }
}

/// Pre-`ema_band_bps` layout for the one-shot LiftParams migration.
#[derive(Encode, Decode, MaxEncodedLen, TypeInfo, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LiftParamsBeforeEmaBand {
    floor_bps: u16,
    thin_floor_bps: u16,
    ref_tao: u64,
    move_bps: u16,
    ceiling_bps: u16,
    split_short_bps: u16,
    rebalance_target_bps: u16,
    rebalance_rate_bps: u16,
    thin_tao: u64,
    ltv_short_bps: u16,
    ltv_long_bps: u16,
    thin_ltv_long_bps: u16,
    apr_bps: u16,
    slice_bps: u16,
    crash_guard_bps: u16,
    settle_ema_blocks: u64,
    new_subnet_min_age: u64,
    new_subnet_ramp: u64,
}

#[freeze_struct("d323ee362fafacc9")]
#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    TypeInfo,
    MaxEncodedLen,
    Clone,
    Default,
    Debug,
    PartialEq,
    Eq,
)]
pub struct LiftBook {
    /// TAO legs of outstanding short lifts (`L_S`).
    pub l_s: u64,
    /// TAO principal of outstanding long lifts (`L_L`).
    pub l_l: u64,
    /// TAO spent on forced long buys (`B_L`, equals `L_L` under escrowed longs).
    pub b_l: u64,
    pub held_tao: u64,
    pub held_alpha: u64,
    /// Aggregate short escrow TAO (`E_S`).
    pub escrow_tao: u64,
    /// Aggregate long escrow alpha (`A`).
    pub escrow_alpha: u64,
    pub long_collateral_alpha: u64,
    /// Geometric EMA of un-lifted depth `G`.
    pub g_ema: u64,
    pub last: u64,
}

#[freeze_struct("dc8f38da430bf563")]
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, Debug, PartialEq, Eq,
)]
pub struct Position<AccountId, BlockNumber> {
    pub side: Side,
    /// Per-position custody coldkey (escrow account). Stored so coldkey swap
    /// re-keys ownership without moving funds (§14).
    pub custody: AccountId,
    /// Alpha for Short, TAO for Long.
    pub principal: u64,
    /// TAO for Short, alpha for Long.
    pub collateral: u64,
    /// Short escrow TAO (`E_S`). Zero for longs.
    pub proceeds: u64,
    /// Long escrow alpha (`A`). Zero for shorts.
    pub escrow_alpha: u64,
    pub lift_tao: u64,
    pub lift_alpha: u64,
    pub annual_interest: u64,
    pub last_accrued: BlockNumber,
    pub interest_remainder: u64,
    pub due: BlockNumber,
    /// Fixed one-year term end (§15). Set at open; increases restart it;
    /// partial repay keeps it.
    pub expires_at: BlockNumber,
    /// Current `Expiry` queue bucket for this position. Equals `expires_at` at
    /// open; advances when a failed settle is deferred onto a retry slot so
    /// coldkey re-key / remove always find the live claim.
    pub expiry_due: BlockNumber,
}

#[freeze_struct("78138cda5aaf128")]
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, Debug, PartialEq, Eq,
)]
pub struct LendingReference<BlockNumber> {
    pub price: U64F64,
    pub last_updated: BlockNumber,
    pub valid_after: BlockNumber,
}

#[freeze_struct("aa9abd5a02a75fa9")]
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, Debug, PartialEq, Eq,
)]
pub struct Dissolution<BlockNumber> {
    pub frozen_at: BlockNumber,
    /// Lift legs have been re-added into reserves.
    pub lifts_readded: bool,
    /// Settlement rate `r` as U64F64 (TAO per alpha), set in settle phase 0.
    pub rate: U64F64,
    /// Snapshot of `SubnetTAO + Lifts.held_tao` at `start_dissolution` (§13.2).
    pub t_flag: u64,
    /// Snapshot of settlement TAO EMA at `start_dissolution` (§13.2).
    pub t_ema_flag: u64,
}

#[freeze_struct("5da4836dd626280e")]
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, Debug, PartialEq, Eq,
)]
pub struct OpeningQuote {
    pub principal: u64,
    pub annual_interest: u64,
    pub opening_value: u64,
    pub collateral: u64,
    pub v_dyn: u64,
    pub escrow_estimate: u64,
}

#[freeze_struct("432492aae8580c0d")]
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, Debug, PartialEq, Eq,
)]
pub struct ClosingQuote {
    pub payment: u64,
    pub refund: u64,
    pub escrow_return: u64,
}

#[freeze_struct("16dfaee81372751a")]
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, Debug, PartialEq, Eq,
)]
pub struct RaiseQuote {
    pub principal_tao: u64,
    pub collateral_alpha: u64,
    pub escrow_alpha: u64,
    pub sell_alpha: u64,
    pub free_tao_out: u64,
    pub price_before: U64F64,
    pub price_after_buy: U64F64,
    pub price_after: U64F64,
    pub v_dyn: u64,
    pub ltv_tier_bps: u16,
    pub long_cap_headroom: u64,
    pub annual_interest_alpha: u64,
}

/// Geometric EMA update weight: 1 - 2^(-1 / 7200), 24h half-life at 12s blocks.
const EMA_WEIGHT_BITS: u128 = 1_775_790_721_272_497;
pub const MAX_BUYBACK_STEPS: u32 = 6;
const BPS_DENOM: u128 = 10_000;
const BISECTION_STEPS: u32 = 16;

/// Why a position is being settled at a smoothed rate (no AMM liquidation).
#[derive(Clone, Copy, PartialEq, Eq)]
enum SettleKind {
    /// Subnet dissolution (§13): rate is pot `r`.
    Dissolution,
    /// Fixed one-year term end (§15): rate is the lending reference EMA.
    Expiry,
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
        #[pallet::constant]
        type MinimumLoanValue: Get<u64>;
        #[pallet::constant]
        type InterestPeriod: Get<BlockNumberFor<Self>>;
        #[pallet::constant]
        type BlocksPerYear: Get<u64>;
        #[pallet::constant]
        type ReferenceWarmup: Get<BlockNumberFor<Self>>;
        #[pallet::constant]
        type MaxPositionsPerSubnet: Get<u32>;
        #[pallet::constant]
        type MaxTotalPositions: Get<u32>;
        /// Blocks to wait before retrying an expiry settle that rolled back.
        /// Failed claims stay covered but must not pin `NextExpiry` forever.
        #[pallet::constant]
        type ExpiryRetryDelay: Get<BlockNumberFor<Self>>;
        type WeightInfo: WeightInfo;
    }

    #[pallet::storage]
    pub type Enabled<T> = StorageValue<_, bool, ValueQuery>;
    #[pallet::storage]
    pub type LiftParamsStore<T> = StorageValue<_, LiftParams, ValueQuery>;
    #[pallet::storage]
    pub type Lifts<T: Config> = StorageMap<_, Identity, NetUid, LiftBook, ValueQuery>;
    #[pallet::storage]
    pub type LongInterestPending<T> = StorageMap<_, Identity, NetUid, u64, ValueQuery>;
    #[pallet::storage]
    pub type Positions<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        Identity,
        NetUid,
        Position<T::AccountId, BlockNumberFor<T>>,
        OptionQuery,
    >;
    #[pallet::storage]
    pub type OpenByNetuid<T: Config> =
        StorageDoubleMap<_, Identity, NetUid, Blake2_128Concat, T::AccountId, (), OptionQuery>;
    #[pallet::storage]
    pub type PositionCount<T> = StorageMap<_, Identity, NetUid, u32, ValueQuery>;
    #[pallet::storage]
    pub type TotalPositions<T> = StorageValue<_, u32, ValueQuery>;
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
    #[pallet::storage]
    pub type References<T: Config> =
        StorageMap<_, Identity, NetUid, LendingReference<BlockNumberFor<T>>, OptionQuery>;
    #[pallet::storage]
    pub type Dissolutions<T: Config> =
        StorageMap<_, Identity, NetUid, Dissolution<BlockNumberFor<T>>, OptionQuery>;
    #[pallet::storage]
    pub type DissolutionCursor<T: Config> =
        StorageMap<_, Identity, NetUid, T::AccountId, OptionQuery>;
    #[pallet::storage]
    pub type DissolutionPhase<T> = StorageMap<_, Identity, NetUid, u8, ValueQuery>;
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
    /// Count of positions due at each block. Avoids unbounded Due key scans.
    #[pallet::storage]
    pub type DueIndex<T: Config> = StorageMap<_, Twox64Concat, BlockNumberFor<T>, u32, ValueQuery>;
    /// Sorted due-bucket successor (earliest → later). Enables O(1) head advance.
    #[pallet::storage]
    pub type DueNext<T: Config> =
        StorageMap<_, Twox64Concat, BlockNumberFor<T>, BlockNumberFor<T>, OptionQuery>;
    /// Sorted due-bucket predecessor.
    #[pallet::storage]
    pub type DuePrev<T: Config> =
        StorageMap<_, Twox64Concat, BlockNumberFor<T>, BlockNumberFor<T>, OptionQuery>;
    #[pallet::storage]
    pub type NextDue<T: Config> = StorageValue<_, BlockNumberFor<T>, OptionQuery>;
    /// Latest due-bucket in the sorted schedule (for O(1) append).
    #[pallet::storage]
    pub type DueTail<T: Config> = StorageValue<_, BlockNumberFor<T>, OptionQuery>;
    /// Resume point inside the current due bucket: (owner, netuid).
    #[pallet::storage]
    pub type DueBucketCursor<T: Config> = StorageValue<_, (T::AccountId, NetUid), OptionQuery>;
    /// Positions indexed by fixed-term expiry block (§15). Parallel to [`Due`].
    #[pallet::storage]
    pub type Expiry<T: Config> = StorageDoubleMap<
        _,
        Twox64Concat,
        BlockNumberFor<T>,
        Blake2_128Concat,
        (T::AccountId, NetUid),
        (),
        OptionQuery,
    >;
    /// Count of positions expiring at each block.
    #[pallet::storage]
    pub type ExpiryIndex<T: Config> =
        StorageMap<_, Twox64Concat, BlockNumberFor<T>, u32, ValueQuery>;
    #[pallet::storage]
    pub type ExpiryNext<T: Config> =
        StorageMap<_, Twox64Concat, BlockNumberFor<T>, BlockNumberFor<T>, OptionQuery>;
    #[pallet::storage]
    pub type ExpiryPrev<T: Config> =
        StorageMap<_, Twox64Concat, BlockNumberFor<T>, BlockNumberFor<T>, OptionQuery>;
    #[pallet::storage]
    pub type NextExpiry<T: Config> = StorageValue<_, BlockNumberFor<T>, OptionQuery>;
    #[pallet::storage]
    pub type ExpiryTail<T: Config> = StorageValue<_, BlockNumberFor<T>, OptionQuery>;
    /// Resume point inside the current expiry bucket: (owner, netuid).
    #[pallet::storage]
    pub type ExpiryBucketCursor<T: Config> = StorageValue<_, (T::AccountId, NetUid), OptionQuery>;
    #[pallet::storage]
    pub type ConversionCursor<T> = StorageValue<_, NetUid, OptionQuery>;
    #[pallet::storage]
    pub type PalletHotkey<T: Config> = StorageValue<_, T::AccountId, OptionQuery>;
    /// Subnets with a seeded lending reference (EMA finalize walk).
    #[pallet::storage]
    pub type ActiveNets<T> = StorageMap<_, Identity, NetUid, (), OptionQuery>;
    /// Count of `ActiveNets` entries — drives on_initialize weight.
    #[pallet::storage]
    pub type ActiveNetCount<T> = StorageValue<_, u32, ValueQuery>;
    /// Settlement TAO EMA per subnet (§13.2). Basis is `pool_tao + held_tao`.
    #[pallet::storage]
    pub type TaoEma<T> = StorageMap<_, Identity, NetUid, u64, OptionQuery>;
    /// One-shot migration markers (name → ran).
    #[pallet::storage]
    pub type HasMigrationRun<T: Config> =
        StorageMap<_, Identity, BoundedVec<u8, super::MigrationKeyMaxLen>, bool, ValueQuery>;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        Opened {
            owner: T::AccountId,
            netuid: NetUid,
            side: Side,
            principal: u64,
            collateral: u64,
            proceeds: u64,
            escrow_alpha: u64,
            lift_tao: u64,
            lift_alpha: u64,
            annual_interest: u64,
        },
        Increased {
            owner: T::AccountId,
            netuid: NetUid,
            side: Side,
            principal: u64,
            collateral: u64,
            proceeds: u64,
            escrow_alpha: u64,
            lift_tao: u64,
            lift_alpha: u64,
            annual_interest: u64,
        },
        Closed {
            owner: T::AccountId,
            netuid: NetUid,
            side: Side,
            refund: u64,
            escrow_returned: u64,
        },
        Forfeited {
            owner: T::AccountId,
            netuid: NetUid,
            side: Side,
            principal_lost: u64,
        },
        Lifted {
            netuid: NetUid,
            tao: u64,
            alpha: u64,
        },
        Readded {
            netuid: NetUid,
            tao: u64,
            alpha: u64,
        },
        LongEscrowed {
            owner: T::AccountId,
            netuid: NetUid,
            tao_in: u64,
            alpha_escrowed: u64,
        },
        Raised {
            owner: T::AccountId,
            hotkey: T::AccountId,
            netuid: NetUid,
            collateral_alpha: u64,
            principal_tao: u64,
            escrow_alpha: u64,
            sell_alpha: u64,
            free_tao_out: u64,
            price_before: U64F64,
            price_after: U64F64,
        },
        InterestCollected {
            owner: T::AccountId,
            netuid: NetUid,
            collateral_paid: u64,
        },
        InterestBurned {
            netuid: NetUid,
            side: Side,
            tao: u64,
        },
        LongInterestSold {
            netuid: NetUid,
            alpha: u64,
            tao: u64,
        },
        LiftParamsSet {
            params: LiftParams,
        },
        EnabledSet {
            enabled: bool,
        },
        DissolutionFrozen {
            netuid: NetUid,
            frozen_at: BlockNumberFor<T>,
        },
        /// §13.2.2 phase-0 rate fix after basket conversions.
        SettleRateFixed {
            netuid: NetUid,
            t_flag: u64,
            t_ema_flag: u64,
            t_pre: u64,
            delta0: u64,
            d: u128,
            r: U64F64,
        },
        Repaid {
            owner: T::AccountId,
            netuid: NetUid,
            side: Side,
            principal_repaid: u64,
            refund: u64,
            escrow_returned: u64,
        },
        DissolutionSettled {
            owner: T::AccountId,
            netuid: NetUid,
            side: Side,
            principal_tao_equiv: u64,
            tao_to_pot: u64,
            tao_refund: u64,
        },
        /// Alias of [`Event::DissolutionSettled`] for Jacob §13 naming.
        Settled {
            owner: T::AccountId,
            netuid: NetUid,
            side: Side,
            principal_tao_equiv: u64,
            tao_to_pot: u64,
            tao_refund: u64,
        },
        DustForfeited {
            netuid: NetUid,
            recipient: T::AccountId,
            tao: u64,
        },
        /// Permissionless seed of the lending reference and G_ema for a subnet.
        ReferenceWarmed {
            netuid: NetUid,
            valid_after: BlockNumberFor<T>,
            g_ema: u64,
        },
        /// Fixed-term expiry auto-settled at the smoothed reference price (§15).
        Expired {
            owner: T::AccountId,
            netuid: NetUid,
            side: Side,
            principal_tao_equiv: u64,
            tao_to_pool: u64,
            tao_refund: u64,
            rate: U64F64,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        Disabled,
        SubnetUnavailable,
        ReferenceUnavailable,
        ReferenceWarmingUp,
        PositionExists,
        PositionMissing,
        /// Opening value or accrued interest is below the runtime minimum
        /// (non-zero principal that still fails economic floors). Prefer
        /// [`SideCapacity`] / [`SideCapacityRamping`] when the side cap is what
        /// blocks a minimum loan.
        AmountTooSmall,
        /// Side-cap headroom cannot fit this (or any MinimumLoanValue) borrow
        /// even at full (post-ramp) Cap. Prefer [`SideCapacityRamping`] when only
        /// the new-subnet ramp is what binds.
        SideCapacity,
        /// Side capacity is binding only because the subnet is still inside the
        /// §2.3 Cap ramp (`ramp(age) < 1`). Full Cap would allow this borrow —
        /// try again later as the ramp rises. Quotes use the same check.
        SideCapacityRamping,
        /// Legacy depth refusal; prefer [`EmptyPool`] /
        /// [`PrincipalExceedsAlphaIn`] / [`PrincipalExceedsTaoIn`].
        DepthBelowThreshold,
        DynamicCollateral,
        TooManyPositions,
        BelowMinimumBorrow,
        AboveMaximumPayment,
        BelowMinimumRefund,
        InsufficientEscrow,
        InvalidQuote,
        Arithmetic,
        CustodyUnavailable,
        AlreadyDissolving,
        /// Requested principal needs more collateral under V_dyn / LTV.
        InsufficientCollateral,
        InsufficientSellStake,
        /// Forced trade missed the user limit (§5.2). Replaces RaiseSlippage /
        /// BelowMinimumProceeds for trading floors and ceilings.
        SlippageExceeded,
        /// Extrinsic `valid_until` has passed (§5.2). Replaces RaiseDeadlinePassed.
        DeadlinePassed,
        ConflictingSizing,
        /// Long open/increase requires `source_hotkey`; short open must omit it
        /// (TAO-only — no stake moves on open).
        InvalidSourceHotkey,
        InvalidLiftParams,
        PositionSideMismatch,
        /// Subnet age is below `new_subnet_min_age` (§2.3).
        SubnetTooNew,
        /// Third-party stake onto the lending custody hotkey is rejected (§14).
        CustodyHotkey,
        /// Derived position escrow is still assigned (e.g. after a coldkey swap
        /// that kept the old custody account). Refuse a fresh open so two loans
        /// cannot share one escrow (§14).
        EscrowInUse,
        /// Post-trade spot is outside the directional EMA band on the
        /// exploitable side (§5.4). Longs: spot > EMA·(1+band); shorts:
        /// spot < EMA·(1−band). Closes and repays are never blocked by this.
        PriceTooFarFromEma,
        /// Principal, principal_part, or borrow amount is zero.
        PrincipalZero,
        /// Collateral amount is zero.
        CollateralZero,
        /// Subtoken trading has not been started on this subnet (§2.3).
        SubnetNotStarted,
        /// `G_ema` is missing or has not aged one full EMA window (§2.3).
        GemaWarmingUp,
        /// Pool TAO or alpha reserve is zero; cannot size a lift.
        EmptyPool,
        /// Short principal is not strictly less than pool alpha in.
        PrincipalExceedsAlphaIn,
        /// Long principal is not strictly less than pool TAO in.
        PrincipalExceedsTaoIn,
        /// Raise / open collateral exceeds the caller’s max collateral cap.
        CollateralExceedsMax,
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        fn integrity_test() {
            assert!(T::BlocksPerYear::get() > 0);
            assert!(T::InterestPeriod::get() > BlockNumberFor::<T>::default());
            assert!(T::MinimumLoanValue::get() > 0);
            assert!(T::MaxPositionsPerSubnet::get() > 0);
            assert!(T::MaxPositionsPerSubnet::get() <= T::MaxTotalPositions::get());
        }

        fn on_initialize(_: BlockNumberFor<T>) -> Weight {
            // Reserve for ActiveNets that on_finalize walks. When lending is
            // unused the count is 0 and we charge a single reference unit so the
            // block remains usable. Cap matches `MAX_ACTIVE_NET_UPDATES`.
            let n = u64::from(ActiveNetCount::<T>::get()).clamp(1, MAX_ACTIVE_NET_UPDATES);
            T::WeightInfo::update_reference()
                .saturating_mul(n)
                .saturating_add(T::DbWeight::get().reads(2))
        }

        fn on_finalize(now: BlockNumberFor<T>) {
            let params = LiftParamsStore::<T>::get();
            // Seeded nets only — registration / migration / warm_reference enroll.
            let mut updated = 0u64;
            for (netuid, _) in ActiveNets::<T>::iter() {
                if updated >= MAX_ACTIVE_NET_UPDATES {
                    break;
                }
                if Dissolutions::<T>::contains_key(netuid) {
                    continue;
                }
                Self::update_tao_ema(netuid, &params);
                Self::update_g_ema(netuid, now, &params);
                References::<T>::mutate(netuid, |reference| {
                    if let Some(reference) = reference {
                        if reference.last_updated >= now {
                            return;
                        }
                        let spot = T::Pool::current_alpha_price(netuid);
                        if let Some(next) = Self::ema_update(reference.price, spot) {
                            reference.price = next;
                            reference.last_updated = now;
                        }
                    }
                });
                updated = updated.saturating_add(1);
            }
        }

        fn on_runtime_upgrade() -> Weight {
            // Warm-reference and LiftParams ema-band migrations live in the
            // runtime `Migrations` tuple so try-runtime runs their pre/post
            // checks. This hook only repairs in-place derived indexes.
            let weight = Weight::zero();

            // Initialize missing TaoEma entries to the current settlement basis.
            let mut writes = 0u64;
            let mut reads = 0u64;
            for netuid in T::Pool::live_subnets() {
                reads = reads.saturating_add(1);
                if TaoEma::<T>::contains_key(netuid) {
                    continue;
                }
                let basis = Self::settlement_tao_basis(netuid);
                // settlement_tao_basis: pool_tao + Lifts read
                reads = reads.saturating_add(2);
                TaoEma::<T>::insert(netuid, basis);
                writes = writes.saturating_add(1);
            }
            // Sync ActiveNetCount after storage item introduction / migration.
            let active = ActiveNets::<T>::iter().count() as u32;
            reads = reads.saturating_add(u64::from(active).saturating_add(1));
            if ActiveNetCount::<T>::get() != active {
                ActiveNetCount::<T>::put(active);
                writes = writes.saturating_add(1);
            }
            // Rebuild DueTail from the schedule head when missing.
            if DueTail::<T>::get().is_none() {
                reads = reads.saturating_add(1);
                if let Some(mut cur) = NextDue::<T>::get() {
                    reads = reads.saturating_add(1);
                    while let Some(n) = DueNext::<T>::get(cur) {
                        reads = reads.saturating_add(1);
                        cur = n;
                    }
                    DueTail::<T>::put(cur);
                    writes = writes.saturating_add(1);
                }
            }
            weight.saturating_add(T::DbWeight::get().reads_writes(reads, writes))
        }

        fn on_idle(now: BlockNumberFor<T>, remaining: Weight) -> Weight {
            let mut meter = WeightMeter::with_limit(remaining);
            Self::collect_due(now, &mut meter);
            Self::collect_expiry(now, &mut meter);
            Self::convert_pending(now, &mut meter);
            meter.consumed()
        }
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Open or increase a loan. Shorts force-sell; longs force-buy into escrow.
        ///
        /// Longs require `source_hotkey` (collateral alpha is locked from that
        /// stake). Shorts are TAO-only and must pass `None` — no hotkey is
        /// stored on the position either way.
        #[pallet::call_index(0)]
        #[pallet::weight(T::WeightInfo::open())]
        pub fn open(
            origin: OriginFor<T>,
            netuid: NetUid,
            side: Side,
            collateral: u64,
            source_hotkey: Option<T::AccountId>,
            min_borrow: u64,
            min_proceeds: u64,
            min_escrow_alpha: u64,
            valid_until: BlockNumberFor<T>,
        ) -> DispatchResult {
            let owner = ensure_signed(origin)?;
            Self::do_open(
                owner,
                netuid,
                side,
                collateral,
                source_hotkey,
                None,
                min_borrow,
                min_proceeds,
                min_escrow_alpha,
                valid_until,
            )
        }

        /// Open or increase with an exact additional principal.
        #[pallet::call_index(1)]
        #[pallet::weight(T::WeightInfo::open_exact())]
        pub fn open_exact(
            origin: OriginFor<T>,
            netuid: NetUid,
            side: Side,
            collateral: u64,
            source_hotkey: Option<T::AccountId>,
            principal: u64,
            min_proceeds: u64,
            min_escrow_alpha: u64,
            valid_until: BlockNumberFor<T>,
        ) -> DispatchResult {
            let owner = ensure_signed(origin)?;
            // PrincipalZero is checked inside `do_open` after lending/subnet
            // gates so a young subnet reports SubnetTooNew / SubnetNotStarted
            // rather than a zero-amount error.
            Self::do_open(
                owner,
                netuid,
                side,
                collateral,
                source_hotkey,
                Some(principal),
                principal,
                min_proceeds,
                min_escrow_alpha,
                valid_until,
            )
        }

        /// Close a position. Shorts buy back (or repay alpha); longs repay TAO (or sell escrow).
        ///
        /// Wallet alpha repayment is drawn from stake on `hotkey`, and returned
        /// alpha is staked to `hotkey`.
        #[pallet::call_index(2)]
        #[pallet::weight(T::WeightInfo::close())]
        #[transactional]
        pub fn close(
            origin: OriginFor<T>,
            netuid: NetUid,
            hotkey: T::AccountId,
            repay_from_wallet: bool,
            max_payment: u64,
            min_refund: u64,
            valid_until: BlockNumberFor<T>,
        ) -> DispatchResult {
            let owner = ensure_signed(origin)?;
            Self::do_close(
                owner,
                netuid,
                hotkey,
                repay_from_wallet,
                max_payment,
                min_refund,
                valid_until,
            )
        }

        /// Atomic raise: escrowed long + spot sale of own unlocked alpha.
        #[pallet::call_index(3)]
        #[pallet::weight(T::WeightInfo::open_long_and_sell())]
        #[transactional]
        pub fn open_long_and_sell(
            origin: OriginFor<T>,
            netuid: NetUid,
            hotkey: T::AccountId,
            collateral_alpha: Option<u64>,
            borrow_tao: Option<u64>,
            max_collateral_alpha: u64,
            min_borrow_tao: u64,
            sell_alpha: Option<u64>,
            min_tao_out: u64,
            min_escrow_alpha: u64,
            valid_until: BlockNumberFor<T>,
        ) -> DispatchResultWithPostInfo {
            let owner = ensure_signed(origin)?;
            Self::do_open_long_and_sell(
                owner,
                netuid,
                hotkey,
                collateral_alpha,
                borrow_tao,
                max_collateral_alpha,
                min_borrow_tao,
                sell_alpha,
                min_tao_out,
                min_escrow_alpha,
                valid_until,
            )?;
            Ok(().into())
        }

        #[pallet::call_index(4)]
        #[pallet::weight(T::WeightInfo::set_enabled())]
        pub fn set_enabled(origin: OriginFor<T>, enabled: bool) -> DispatchResult {
            ensure_root(origin)?;
            Enabled::<T>::put(enabled);
            Self::deposit_event(Event::EnabledSet { enabled });
            Ok(())
        }

        #[pallet::call_index(5)]
        #[pallet::weight(T::WeightInfo::set_lift_params())]
        pub fn set_lift_params(origin: OriginFor<T>, params: LiftParams) -> DispatchResult {
            ensure_root(origin)?;
            Self::ensure_valid_lift_params(&params)?;
            LiftParamsStore::<T>::put(params);
            Self::deposit_event(Event::LiftParamsSet { params });
            Ok(())
        }

        /// Partial repay of `principal_part` (§4.4). `principal_part == principal` closes.
        ///
        /// `hotkey` is the alpha source and destination, as in [`Pallet::close`].
        #[pallet::call_index(6)]
        #[pallet::weight(T::WeightInfo::repay())]
        #[transactional]
        pub fn repay(
            origin: OriginFor<T>,
            netuid: NetUid,
            hotkey: T::AccountId,
            principal_part: u64,
            repay_from_wallet: bool,
            max_payment: u64,
            min_refund: u64,
            valid_until: BlockNumberFor<T>,
        ) -> DispatchResult {
            let owner = ensure_signed(origin)?;
            Self::do_repay(
                owner,
                netuid,
                hotkey,
                principal_part,
                repay_from_wallet,
                max_payment,
                min_refund,
                valid_until,
            )
        }

        /// Permissionless fallback seed of the lending reference and `G_ema`.
        ///
        /// Prefer automatic seeding: registration (`on_subnet_registered`),
        /// start-call (`on_subtoken_enabled`), and the upgrade migration.
        /// This extrinsic remains for ops recovery. Idempotent.
        #[pallet::call_index(7)]
        #[pallet::weight(T::WeightInfo::warm_reference())]
        pub fn warm_reference(origin: OriginFor<T>, netuid: NetUid) -> DispatchResult {
            ensure_signed(origin)?;
            Self::do_warm_reference(netuid)
        }
    }

    impl<T: Config> Pallet<T> {
        pub fn lift_params() -> LiftParams {
            LiftParamsStore::<T>::get()
        }

        fn ensure_valid_lift_params(p: &LiftParams) -> DispatchResult {
            ensure!(
                p.floor_bps <= p.ceiling_bps && p.thin_floor_bps <= p.ceiling_bps,
                Error::<T>::InvalidLiftParams
            );
            ensure!(
                p.ceiling_bps <= 10_000
                    && p.split_short_bps <= 10_000
                    && p.ltv_short_bps <= 10_000
                    && p.ltv_long_bps <= 10_000
                    && p.thin_ltv_long_bps <= 10_000
                    && p.apr_bps <= 10_000
                    && p.crash_guard_bps <= 10_000
                    && p.ema_band_bps <= 10_000
                    && p.move_bps > 0
                    && p.ref_tao > 0
                    && p.settle_ema_blocks > 0
                    && p.new_subnet_ramp > 0,
                Error::<T>::InvalidLiftParams
            );
            Ok(())
        }

        /// §5.4: directional EMA band on opens. Checked against **post-trade**
        /// spot so one large trade cannot jump past the band; splitting cannot
        /// either, because the limit is on price state, not per-trade size.
        /// Opposite-side opens are always allowed. Closes / repays / settles
        /// never call this.
        fn ensure_open_ema_band(
            side: Side,
            spot_after: U64F64,
            ema: U64F64,
            params: &LiftParams,
        ) -> DispatchResult {
            let band = u32::from(params.ema_band_bps);
            let denom = U64F64::from_num(10_000u32);
            match side {
                Side::Long => {
                    // Reject if spot_after > EMA * (1 + band).
                    let ceiling = ema
                        .saturating_mul(U64F64::from_num(10_000u32.saturating_add(band)))
                        / denom;
                    ensure!(spot_after <= ceiling, Error::<T>::PriceTooFarFromEma);
                }
                Side::Short => {
                    // Reject if spot_after < EMA * (1 − band).
                    let floor = ema
                        .saturating_mul(U64F64::from_num(10_000u32.saturating_sub(band)))
                        / denom;
                    ensure!(spot_after >= floor, Error::<T>::PriceTooFarFromEma);
                }
            }
            Ok(())
        }

        /// §2.3: subtoken started, age ≥ min_age, reference warm, G_ema aged one EMA window.
        ///
        /// Each failed condition has its own error so callers can tell *why*
        /// the gate refused (not a single umbrella `SubnetTooNew`).
        fn ensure_new_subnet_gate(netuid: NetUid) -> DispatchResult {
            ensure!(
                T::Pool::subtoken_enabled(netuid),
                Error::<T>::SubnetNotStarted
            );
            let params = Self::lift_params();
            let now: u64 = frame_system::Pallet::<T>::block_number().saturated_into();
            let registered = T::Pool::network_registered_at(netuid);
            let age = now.saturating_sub(registered);
            ensure!(age >= params.new_subnet_min_age, Error::<T>::SubnetTooNew);
            // Reference must be warm (valid_after reached).
            let reference = References::<T>::get(netuid).ok_or(Error::<T>::ReferenceUnavailable)?;
            ensure!(
                frame_system::Pallet::<T>::block_number() >= reference.valid_after,
                Error::<T>::ReferenceWarmingUp
            );
            // G_ema initialised and updated for at least one EMA window.
            let book = Lifts::<T>::get(netuid);
            ensure!(
                book.g_ema > 0 && book.last.saturating_sub(registered) >= params.settle_ema_blocks,
                Error::<T>::GemaWarmingUp
            );
            Ok(())
        }

        /// Linear Cap multiplier after the new-subnet gate (§2.3).
        fn capacity_ramp(netuid: NetUid, params: &LiftParams) -> u64 {
            let now: u64 = frame_system::Pallet::<T>::block_number().saturated_into();
            let registered = T::Pool::network_registered_at(netuid);
            let age = now.saturating_sub(registered);
            if age <= params.new_subnet_min_age {
                return 0;
            }
            let elapsed = age.saturating_sub(params.new_subnet_min_age);
            let ramp = params.new_subnet_ramp.max(1);
            if elapsed >= ramp {
                BPS_DENOM as u64
            } else {
                (u128::from(elapsed) * BPS_DENOM / u128::from(ramp)) as u64
            }
        }

        /// Settlement TAO basis `T = SubnetTAO + Lifts.held_tao` (§13.2).
        pub fn settlement_tao_basis(netuid: NetUid) -> u64 {
            T::Pool::pool_tao(netuid)
                .to_u64()
                .saturating_add(Lifts::<T>::get(netuid).held_tao)
        }

        /// Return stored TAO EMA, initializing to the current settlement basis if missing.
        pub fn ensure_tao_ema(netuid: NetUid) -> u64 {
            if let Some(ema) = TaoEma::<T>::get(netuid) {
                return ema;
            }
            let basis = Self::settlement_tao_basis(netuid);
            TaoEma::<T>::insert(netuid, basis);
            basis
        }

        /// One-block integer EMA update: `t_ema += (T - t_ema) / settle_ema_blocks`.
        pub fn update_tao_ema(netuid: NetUid, params: &LiftParams) {
            let t = Self::settlement_tao_basis(netuid);
            let ema = Self::ensure_tao_ema(netuid);
            let blocks = params.settle_ema_blocks.max(1);
            let next = if t >= ema {
                ema.saturating_add(t.saturating_sub(ema) / blocks)
            } else {
                ema.saturating_sub(ema.saturating_sub(t) / blocks)
            };
            TaoEma::<T>::insert(netuid, next);
        }

        pub fn recovery_account() -> T::AccountId {
            T::PalletId::get().into_sub_account_truncating(b"recovery")
        }

        /// Lending custody coldkey for held lift legs on a subnet.
        pub fn reserve_account(netuid: NetUid) -> T::AccountId {
            T::PalletId::get().into_sub_account_truncating((b"lift", netuid))
        }

        pub fn position_account(owner: &T::AccountId, netuid: NetUid) -> T::AccountId {
            let key = T::Hashing::hash_of(&(b"position", owner, netuid));
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

        fn ensure_lending_live(netuid: NetUid) -> DispatchResult {
            ensure!(Enabled::<T>::get(), Error::<T>::Disabled);
            ensure!(
                T::Pool::subnet_exists(netuid) && !Dissolutions::<T>::contains_key(netuid),
                Error::<T>::SubnetUnavailable
            );
            Ok(())
        }

        fn ensure_reference_warm(
            netuid: NetUid,
        ) -> Result<LendingReference<BlockNumberFor<T>>, DispatchError> {
            let reference = References::<T>::get(netuid).ok_or(Error::<T>::ReferenceUnavailable)?;
            let now = frame_system::Pallet::<T>::block_number();
            ensure!(now >= reference.valid_after, Error::<T>::ReferenceWarmingUp);
            Ok(reference)
        }

        /// Require a warm reference previously committed by [`Self::do_warm_reference`].
        ///
        /// Does **not** insert: open/raise paths are transactional, so a
        /// seed-then-`Err(ReferenceWarmingUp)` pattern would roll the insert
        /// back and leave the subnet permanently unable to create a reference.
        fn ensure_or_init_reference(
            netuid: NetUid,
        ) -> Result<LendingReference<BlockNumberFor<T>>, DispatchError> {
            Self::ensure_reference_warm(netuid)
        }

        /// Best-effort seed of `References`, `G_ema`, and `ActiveNets`.
        ///
        /// Does **not** require lending `Enabled` or subtoken (start call): used
        /// at registration, start-call, and the upgrade migration so EMAs age
        /// during the new-subnet wait. Returns `false` when the subnet cannot
        /// be seeded (missing / dissolving / zero price / custody unavailable).
        /// Idempotent when already seeded.
        pub fn try_seed_reference(netuid: NetUid) -> bool {
            if Dissolutions::<T>::contains_key(netuid) {
                return false;
            }
            if !T::Pool::network_alive(netuid) {
                return false;
            }
            let spot = T::Pool::current_alpha_price(netuid);
            if spot.to_bits() == 0 {
                // Keep an already-seeded reference; otherwise wait for a price.
                return References::<T>::contains_key(netuid);
            }
            if Self::initialize_custody().is_err() {
                return false;
            }

            let now = frame_system::Pallet::<T>::block_number();
            let (valid_after, created) = if let Some(reference) = References::<T>::get(netuid) {
                (reference.valid_after, false)
            } else {
                let valid_after = now.saturating_add(T::ReferenceWarmup::get());
                References::<T>::insert(
                    netuid,
                    LendingReference {
                        price: spot,
                        last_updated: now,
                        valid_after,
                    },
                );
                (valid_after, true)
            };

            let g_ema = Lifts::<T>::mutate(netuid, |book| {
                if book.g_ema == 0 {
                    book.g_ema = Self::g_spot(netuid, book).max(1);
                    book.last = now.saturated_into();
                }
                book.g_ema
            });
            Self::note_active_net(netuid);
            let _ = Self::ensure_tao_ema(netuid);

            if created {
                Self::deposit_event(Event::ReferenceWarmed {
                    netuid,
                    valid_after,
                    g_ema,
                });
            }
            true
        }

        /// Permissionless fallback: require lending live + subtoken, then seed.
        ///
        /// Prefer automatic seeding via registration / start-call / migration.
        pub fn do_warm_reference(netuid: NetUid) -> DispatchResult {
            Self::ensure_lending_live(netuid)?;
            ensure!(
                T::Pool::subtoken_enabled(netuid),
                Error::<T>::SubnetNotStarted
            );
            ensure!(
                Self::try_seed_reference(netuid),
                Error::<T>::ReferenceUnavailable
            );
            Ok(())
        }

        fn g_spot(netuid: NetUid, book: &LiftBook) -> u64 {
            // G = y + L_S + E_S (long term cancels)
            T::Pool::pool_tao(netuid)
                .to_u64()
                .saturating_add(book.l_s)
                .saturating_add(book.escrow_tao)
        }

        fn g_ref(netuid: NetUid, book: &LiftBook) -> u64 {
            let spot = Self::g_spot(netuid, book);
            if book.g_ema == 0 {
                spot
            } else {
                spot.min(book.g_ema)
            }
        }

        fn update_g_ema(netuid: NetUid, now: BlockNumberFor<T>, _params: &LiftParams) {
            Lifts::<T>::mutate(netuid, |book| {
                let now_u: u64 = now.saturated_into();
                if book.last >= now_u && book.g_ema > 0 {
                    return;
                }
                let spot = Self::g_spot(netuid, book);
                if book.g_ema == 0 {
                    book.g_ema = spot;
                } else if let (Some(prev), Some(obs)) = (
                    U64F64::checked_from_num(book.g_ema),
                    U64F64::checked_from_num(spot.max(1)),
                ) {
                    if let Some(next) = Self::ema_update(prev, obs) {
                        book.g_ema = next.saturating_to_num::<u64>().max(1);
                    }
                }
                book.last = now_u;
            });
        }

        /// `(Cap, Cap_short, Cap_long)` at `ramp_bps` (≤ 10_000). `None` uses the
        /// live §2.3 ramp. Pass `Some(10_000)` for the post-ramp full Cap.
        fn side_caps_at(
            netuid: NetUid,
            params: &LiftParams,
            ramp_bps: Option<u64>,
        ) -> Result<(u64, u64, u64), DispatchError> {
            let book = Lifts::<T>::get(netuid);
            let g_ref = Self::g_ref(netuid, &book);
            let y_star =
                T::Pool::buy_depth_threshold(netuid, params.ref_tao.into(), params.move_bps)
                    .to_u64();
            let f_bps = if g_ref < params.thin_tao {
                params.thin_floor_bps
            } else {
                params.floor_bps
            };
            let floor_cap = Self::mul_bps(g_ref, f_bps)?;
            let ceiling_cap = Self::mul_bps(g_ref, params.ceiling_bps)?;
            let impact_cap = g_ref.saturating_sub(y_star);
            let raw_cap = floor_cap.max(impact_cap.min(ceiling_cap));
            // §2.3: Cap *= ramp(age) in bps of full Cap.
            let ramp = ramp_bps
                .unwrap_or_else(|| Self::capacity_ramp(netuid, params))
                .min(10_000);
            let cap = Self::mul_bps(raw_cap, ramp as u16)?;
            let cap_short = Self::mul_bps(cap, params.split_short_bps)?;
            let cap_long = cap.saturating_sub(cap_short);
            Ok((cap, cap_short, cap_long))
        }

        fn side_caps(
            netuid: NetUid,
            params: &LiftParams,
        ) -> Result<(u64, u64, u64), DispatchError> {
            Self::side_caps_at(netuid, params, None)
        }

        /// Cap_side for `side` from a `(Cap, Cap_short, Cap_long)` triple.
        fn cap_for_side(side: Side, caps: (u64, u64, u64)) -> u64 {
            match side {
                Side::Short => caps.1,
                Side::Long => caps.2,
            }
        }

        /// L_side currently booked on `side`.
        fn side_lift_tao(netuid: NetUid, side: Side) -> u64 {
            let book = Lifts::<T>::get(netuid);
            match side {
                Side::Short => book.l_s,
                Side::Long => book.l_l,
            }
        }

        /// When live (ramped) Cap_side rejects `used + delta_l`, prefer
        /// [`SideCapacityRamping`] if full Cap would still fit — else
        /// [`SideCapacity`].
        fn side_capacity_error(
            netuid: NetUid,
            side: Side,
            delta_l: u64,
            params: &LiftParams,
        ) -> Result<Error<T>, DispatchError> {
            let used = Self::side_lift_tao(netuid, side);
            let need = used.saturating_add(delta_l);
            let full = Self::cap_for_side(side, Self::side_caps_at(netuid, params, Some(10_000))?);
            Ok(if need <= full {
                Error::<T>::SideCapacityRamping
            } else {
                Error::<T>::SideCapacity
            })
        }

        /// Zero / unusable headroom under the live ramp: Ramping if full Cap
        /// still has usable headroom for a MinimumLoanValue borrow, else
        /// SideCapacity.
        fn refuse_capacity(
            netuid: NetUid,
            side: Side,
            price: U64F64,
            params: &LiftParams,
        ) -> DispatchError {
            if Self::capacity_ramp(netuid, params) >= 10_000 {
                return Error::<T>::SideCapacity.into();
            }
            match Self::capacity_blocks_minimum_loan_at(netuid, side, price, params, Some(10_000)) {
                Ok(false) => Error::<T>::SideCapacityRamping.into(),
                _ => Error::<T>::SideCapacity.into(),
            }
        }

        fn ensure_side_capacity(netuid: NetUid, side: Side, delta_l: u64) -> DispatchResult {
            let params = Self::lift_params();
            let cap_side = Self::cap_for_side(side, Self::side_caps(netuid, &params)?);
            let used = Self::side_lift_tao(netuid, side);
            if used.saturating_add(delta_l) > cap_side {
                return Err(Self::side_capacity_error(netuid, side, delta_l, &params)?.into());
            }
            Ok(())
        }

        fn ltv_tier_bps(netuid: NetUid, side: Side, params: &LiftParams) -> u16 {
            match side {
                Side::Short => params.ltv_short_bps,
                Side::Long => {
                    let book = Lifts::<T>::get(netuid);
                    let g_ref = Self::g_ref(netuid, &book);
                    if g_ref < params.thin_tao {
                        params.thin_ltv_long_bps
                    } else {
                        params.ltv_long_bps
                    }
                }
            }
        }

        /// Karl dynamic valuation (spec §5). Evaluated on pre-extrinsic pool / after lift.
        fn v_dyn(
            netuid: NetUid,
            collateral: u64,
            principal_tao: u64,
            reference: &LendingReference<BlockNumberFor<T>>,
            growing_owner: Option<&T::AccountId>,
        ) -> Result<u64, DispatchError> {
            let p_spot = T::Pool::current_alpha_price(netuid);
            let p_ref = reference.price;
            let y = T::Pool::pool_tao(netuid).to_u64();
            let x = T::Pool::pool_alpha(netuid).to_u64();
            ensure!(principal_tao > 0, Error::<T>::PrincipalZero);
            ensure!(y > 0, Error::<T>::EmptyPool);
            ensure!(y > principal_tao, Error::<T>::PrincipalExceedsTaoIn);
            // pool' after lift: (y - P, x * (1 - P/y))
            let y_prime = y.saturating_sub(principal_tao);
            let x_prime = {
                let lift_alpha = (u128::from(x).saturating_mul(u128::from(principal_tao))
                    / u128::from(y).max(1)) as u64;
                x.saturating_sub(lift_alpha)
            };
            let w = T::Pool::quote_weight_parts(netuid);
            let a_ahead = Self::a_ahead(netuid, growing_owner)?;
            // Sell A_ahead into pool': alpha in, TAO out → y decreases, x increases.
            let tao_out_ahead = if a_ahead > 0 {
                T::Pool::quote_sell_on(y_prime.into(), x_prime.into(), w, a_ahead.into()).to_u64()
            } else {
                0
            };
            let y_after = y_prime.saturating_sub(tao_out_ahead);
            let x_after = x_prime.saturating_add(a_ahead);
            let q = if collateral > 0 {
                // Fee-inclusive sale of C uses live quote path on hypothetical state via quote_sell_on (fee-free).
                // Conservative: use fee-free as upper bound then take min with spots.
                T::Pool::quote_sell_on(y_after.into(), x_after.into(), w, collateral.into())
                    .to_u64()
            } else {
                0
            };
            let at_ref = Self::tao_for_alpha(collateral, p_ref, false)?;
            let at_spot = Self::tao_for_alpha(collateral, p_spot, false)?;
            Ok(q.min(at_ref).min(at_spot))
        }

        fn a_ahead(
            netuid: NetUid,
            growing_owner: Option<&T::AccountId>,
        ) -> Result<u64, DispatchError> {
            // Aggregate book + pending interest; exclude the growing owner's current
            // collateral because `v_dyn` sells that quantity separately as `C`.
            let pending = LongInterestPending::<T>::get(netuid);
            let booked = Lifts::<T>::get(netuid).long_collateral_alpha;
            let mut total = Self::add(pending, booked)?;
            if let Some(owner) = growing_owner {
                if let Some(pos) = Positions::<T>::get(owner, netuid) {
                    if pos.side == Side::Long {
                        total = total.saturating_sub(pos.collateral);
                    }
                }
            }
            Ok(total)
        }

        fn mul_bps(value: u64, bps: u16) -> Result<u64, DispatchError> {
            let product = u128::from(value)
                .checked_mul(u128::from(bps))
                .ok_or(Error::<T>::Arithmetic)?;
            Ok((product / BPS_DENOM) as u64)
        }

        fn add(a: u64, b: u64) -> Result<u64, DispatchError> {
            a.checked_add(b).ok_or(Error::<T>::Arithmetic.into())
        }
        fn sub(a: u64, b: u64) -> Result<u64, DispatchError> {
            a.checked_sub(b).ok_or(Error::<T>::Arithmetic.into())
        }

        fn tao_for_alpha(alpha: u64, price: U64F64, round_up: bool) -> Result<u64, DispatchError> {
            let value = price
                .checked_mul(U64F64::from_num(alpha))
                .ok_or(Error::<T>::Arithmetic)?;
            let rounded = if round_up {
                value.checked_ceil().ok_or(Error::<T>::Arithmetic)?
            } else {
                value.floor()
            };
            Ok(rounded.saturating_to_num())
        }

        fn alpha_for_tao(tao: u64, price: U64F64, round_up: bool) -> Result<u64, DispatchError> {
            ensure!(price.to_bits() > 0, Error::<T>::Arithmetic);
            let value = U64F64::from_num(tao)
                .checked_div(price)
                .ok_or(Error::<T>::Arithmetic)?;
            let rounded = if round_up {
                value.checked_ceil().ok_or(Error::<T>::Arithmetic)?
            } else {
                value.floor()
            };
            Ok(rounded.saturating_to_num())
        }

        pub fn ema_update(previous: U64F64, observed: U64F64) -> Option<U64F64> {
            if previous.to_bits() == 0 || observed.to_bits() == 0 {
                return None;
            }
            let half = previous.checked_div(U64F64::from_num(2))?;
            let twice = previous.saturating_mul(U64F64::from_num(2));
            let bounded = observed.clamp(half, twice);
            let ratio = bounded.checked_div(previous)?;
            let log_ratio: I64F64 = ln(I64F64::checked_from_num(ratio)?).ok()?;
            let exponent =
                log_ratio.checked_mul(I64F64::from_num(U64F64::from_bits(EMA_WEIGHT_BITS)))?;
            let multiplier: I64F64 = exp(exponent).ok()?;
            previous.checked_mul(U64F64::checked_from_num(multiplier)?)
        }

        fn ensure_matching_side(
            position: &Position<T::AccountId, BlockNumberFor<T>>,
            side: Side,
        ) -> DispatchResult {
            ensure!(position.side == side, Error::<T>::PositionSideMismatch);
            Ok(())
        }

        fn opening_quote(
            side: Side,
            principal: u64,
            price: U64F64,
            collateral: u64,
            v_dyn: u64,
            escrow_estimate: u64,
            params: &LiftParams,
        ) -> Result<OpeningQuote, DispatchError> {
            let opening_value = match side {
                Side::Short => Self::tao_for_alpha(principal, price, true)?,
                Side::Long => principal,
            };
            let annual_interest = match side {
                Side::Short => Self::mul_bps(opening_value, params.apr_bps)?,
                Side::Long => {
                    let alpha = Self::alpha_for_tao(principal, price, true)?;
                    Self::mul_bps(alpha, params.apr_bps)?
                }
            };
            ensure!(principal > 0, Error::<T>::PrincipalZero);
            ensure!(
                annual_interest > 0 && opening_value >= T::MinimumLoanValue::get(),
                Error::<T>::AmountTooSmall
            );
            Ok(OpeningQuote {
                principal,
                annual_interest,
                opening_value,
                collateral,
                v_dyn,
                escrow_estimate,
            })
        }

        /// Side-cap headroom in TAO lift legs (`Cap_side − L_side`).
        fn side_headroom_tao(
            netuid: NetUid,
            side: Side,
            params: &LiftParams,
        ) -> Result<u64, DispatchError> {
            Self::side_headroom_tao_at(netuid, side, params, None)
        }

        fn side_headroom_tao_at(
            netuid: NetUid,
            side: Side,
            params: &LiftParams,
            ramp_bps: Option<u64>,
        ) -> Result<u64, DispatchError> {
            let caps = Self::side_caps_at(netuid, params, ramp_bps)?;
            Ok(Self::cap_for_side(side, caps).saturating_sub(Self::side_lift_tao(netuid, side)))
        }

        /// Accrue pending interest on an existing position the same way
        /// `do_open` / raise do before sizing. May forfeit (re-add lift legs,
        /// free side capacity, mutate pool reserves). Callers that must not
        /// persist those effects wrap this in a rolled-back storage transaction.
        fn accrue_existing_before_size(
            owner: &T::AccountId,
            netuid: NetUid,
            side: Side,
            now: BlockNumberFor<T>,
        ) -> Result<Option<Position<T::AccountId, BlockNumberFor<T>>>, DispatchError> {
            let mut existing = Positions::<T>::get(owner, netuid);
            if let Some(position) = &existing {
                Self::ensure_matching_side(position, side)?;
            }
            if let Some(mut position) = existing.take() {
                Self::charge_interest(owner, netuid, &mut position, now, false)?;
                if Positions::<T>::contains_key(owner, netuid) {
                    Positions::<T>::insert(owner, netuid, &position);
                    existing = Some(position);
                } else {
                    // Exhausted and forfeited — treat as a fresh open.
                    existing = None;
                }
            }
            Ok(existing)
        }

        /// True when side capacity cannot fit a MinimumLoanValue borrow that
        /// also produces a non-zero lift (`ΔL ≥ 1`).
        ///
        /// On cheap pools `floor(H·x/y)` can sit below `ceil(x/y)` (the smallest
        /// executable short principal) while `H > 0`. That leftover rao of
        /// headroom is not a usable borrow — report it as capacity-exhausted
        /// rather than `AmountTooSmall`.
        fn capacity_blocks_minimum_loan(
            netuid: NetUid,
            side: Side,
            price: U64F64,
            params: &LiftParams,
        ) -> Result<bool, DispatchError> {
            Self::capacity_blocks_minimum_loan_at(netuid, side, price, params, None)
        }

        fn capacity_blocks_minimum_loan_at(
            netuid: NetUid,
            side: Side,
            price: U64F64,
            params: &LiftParams,
            ramp_bps: Option<u64>,
        ) -> Result<bool, DispatchError> {
            let headroom = Self::side_headroom_tao_at(netuid, side, params, ramp_bps)?;
            if headroom == 0 {
                return Ok(true);
            }
            let min_value = T::MinimumLoanValue::get();
            let y = T::Pool::pool_tao(netuid).to_u64();
            let x = T::Pool::pool_alpha(netuid).to_u64();
            if y == 0 || x == 0 {
                return Ok(true);
            }
            // Smallest principal that meets the economic floor *and* lifts ≥1 rao.
            let min_principal = match side {
                Side::Long => min_value.max(1),
                Side::Short => {
                    let for_value = Self::alpha_for_tao(min_value, price, true)?.max(1);
                    // floor(y·P/x) ≥ 1  ⇒  P ≥ ceil(x/y).
                    let for_lift = ((u128::from(x).saturating_add(u128::from(y).saturating_sub(1)))
                        / u128::from(y))
                    .min(u128::from(u64::MAX)) as u64;
                    for_value.max(for_lift).max(1)
                }
            };
            let delta_l = match Self::lift_tao_for_principal(netuid, side, min_principal) {
                Ok(d) => d,
                Err(_) => return Ok(true),
            };
            if delta_l == 0 || delta_l > headroom {
                return Ok(true);
            }
            // Honest search bound (no ceil padding). If it cannot reach the
            // minimum executable principal, the leftover headroom is unusable.
            let floor_add_cap = match side {
                Side::Short => ((u128::from(headroom) * u128::from(x)) / u128::from(y)) as u64,
                Side::Long => headroom,
            };
            Ok(floor_add_cap < min_principal)
        }

        /// Map a zero / dust additional principal to SideCapacity /
        /// SideCapacityRamping when the side cap is what prevents a
        /// MinimumLoanValue borrow; otherwise AmountTooSmall.
        fn refuse_dust_or_capacity(
            netuid: NetUid,
            side: Side,
            price: U64F64,
            params: &LiftParams,
        ) -> DispatchError {
            match Self::capacity_blocks_minimum_loan(netuid, side, price, params) {
                Ok(true) => Self::refuse_capacity(netuid, side, price, params),
                _ => Error::<T>::AmountTooSmall.into(),
            }
        }

        /// Shared open-size gate for quotes and extrinsics.
        ///
        /// Zero principal, zero-lift principal, or an `opening_quote` economic
        /// floor miss becomes `SideCapacity` when the side cannot fit a
        /// MinimumLoanValue borrow with `ΔL ≥ 1`, else `AmountTooSmall`.
        /// Keeps quote Ok ⇒ dispatch Ok for the same sized principal.
        fn validate_sized_principal(
            netuid: NetUid,
            side: Side,
            principal: u64,
            price: U64F64,
            collateral: u64,
            v_dyn: u64,
            escrow_estimate: u64,
            params: &LiftParams,
        ) -> Result<OpeningQuote, DispatchError> {
            if principal == 0 {
                return Err(Self::refuse_dust_or_capacity(netuid, side, price, params));
            }
            // `lift_and_debit` requires both legs > 0; a capacity-clamped dust
            // principal with ΔL = 0 must not quote Ok.
            match Self::lift_tao_for_principal(netuid, side, principal) {
                Ok(0) => {
                    return Err(Self::refuse_dust_or_capacity(netuid, side, price, params));
                }
                Ok(_) => {}
                Err(e) => return Err(e),
            }
            match Self::opening_quote(
                side,
                principal,
                price,
                collateral,
                v_dyn,
                escrow_estimate,
                params,
            ) {
                Ok(q) => Ok(q),
                Err(e) if e == Error::<T>::AmountTooSmall.into() => {
                    Err(Self::refuse_dust_or_capacity(netuid, side, price, params))
                }
                Err(e) => Err(e),
            }
        }

        /// Max additional principal for collateral under LTV·V_dyn and side caps.
        ///
        /// Side-cap headroom (`Cap_side − L_side`) is always in **TAO lift legs**.
        /// Long principal is already TAO, so that headroom is the principal bound.
        /// Short principal is **alpha**; invert `ΔL = floor(y·P/x) ≤ headroom_tao`
        /// before searching so a cheap subnet is not capped at `headroom_tao` alpha.
        ///
        /// Returns `Err(SideCapacity)` / `Err(SideCapacityRamping)` when the side
        /// has no TAO headroom (or the headroom cannot size any principal).
        /// Returns `Ok(0)` only when collateral/LTV cannot support a positive
        /// borrow despite capacity.
        fn max_principal_for_collateral(
            netuid: NetUid,
            side: Side,
            collateral: u64,
            existing: Option<&Position<T::AccountId, BlockNumberFor<T>>>,
            owner: Option<&T::AccountId>,
            reference: &LendingReference<BlockNumberFor<T>>,
            params: &LiftParams,
        ) -> Result<u64, DispatchError> {
            let old_principal = existing.map(|p| p.principal).unwrap_or(0);
            let old_collateral = existing.map(|p| p.collateral).unwrap_or(0);
            let total_collateral = Self::add(old_collateral, collateral)?;
            let ltv = Self::ltv_tier_bps(netuid, side, params);
            let headroom_tao = Self::side_headroom_tao(netuid, side, params)?;
            let price = T::Pool::current_alpha_price(netuid);
            if headroom_tao == 0 {
                return Err(Self::refuse_capacity(netuid, side, price, params));
            }
            if total_collateral == 0 {
                return Ok(0);
            }
            let y = T::Pool::pool_tao(netuid).to_u64();
            let x = T::Pool::pool_alpha(netuid).to_u64();
            if y == 0 || x == 0 {
                return Err(Error::<T>::EmptyPool.into());
            }
            // Additional principal upper bound in the side's native units.
            let add_cap = match side {
                Side::Short => {
                    // floor(y·P/x) ≤ headroom_tao  ⇒  P ≤ floor(headroom_tao · x / y).
                    let mut p = ((u128::from(headroom_tao) * u128::from(x)) / u128::from(y)) as u64;
                    // On cheap pools floor(H·x/y) can sit below the smallest P
                    // with ΔL ≥ 1 (ceil(x/y)). Include that P in the search when
                    // headroom ≥ 1; principal_feasible still enforces ΔL ≤ H so
                    // the bisection clamps to remaining capacity instead of
                    // returning a zero-lift dust principal.
                    if headroom_tao > 0 {
                        let min_lift_p = (u128::from(x)
                            .saturating_add(u128::from(y).saturating_sub(1)))
                            / u128::from(y);
                        p = p.max(min_lift_p.min(u128::from(u64::MAX)) as u64);
                    }
                    p.min(x.saturating_sub(1))
                }
                Side::Long => headroom_tao.min(y.saturating_sub(1)),
            };
            if add_cap == 0 {
                return Err(Self::refuse_capacity(netuid, side, price, params));
            }
            // Bisection over *total* principal in [old, old+add_cap].
            let mut lo = old_principal;
            let mut hi = Self::add(old_principal, add_cap)?;
            for _ in 0..BISECTION_STEPS {
                if lo + 1 >= hi {
                    break;
                }
                let mid = lo.saturating_add(hi.saturating_sub(lo) / 2);
                let ok = Self::principal_feasible(
                    netuid,
                    side,
                    mid,
                    total_collateral,
                    old_principal,
                    owner,
                    reference,
                    ltv,
                    params,
                )?;
                if ok {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let candidate = if Self::principal_feasible(
                netuid,
                side,
                hi,
                total_collateral,
                old_principal,
                owner,
                reference,
                ltv,
                params,
            )? {
                hi
            } else {
                lo
            };
            let additional = candidate.saturating_sub(old_principal).min(add_cap);
            if additional == 0 {
                // Capacity leftover that cannot express a minimum executable
                // borrow must not look like an LTV / dust refusal.
                if Self::capacity_blocks_minimum_loan(netuid, side, price, params)? {
                    return Err(Self::refuse_capacity(netuid, side, price, params));
                }
            }
            Ok(additional)
        }

        fn principal_feasible(
            netuid: NetUid,
            side: Side,
            total_principal: u64,
            total_collateral: u64,
            old_principal: u64,
            owner: Option<&T::AccountId>,
            reference: &LendingReference<BlockNumberFor<T>>,
            ltv_bps: u16,
            _params: &LiftParams,
        ) -> Result<bool, DispatchError> {
            if total_principal == 0 {
                return Ok(false);
            }
            let incremental = total_principal.saturating_sub(old_principal);
            if incremental == 0 {
                return Ok(true);
            }
            // Principal past the pool reserve is infeasible for a search; an
            // empty pool is still a hard EmptyPool from lift_tao_for_principal.
            let y = T::Pool::pool_tao(netuid).to_u64();
            let x = T::Pool::pool_alpha(netuid).to_u64();
            match side {
                Side::Short if x > 0 && incremental >= x => return Ok(false),
                Side::Long if y > 0 && incremental >= y => return Ok(false),
                _ => {}
            }
            let delta_l = Self::lift_tao_for_principal(netuid, side, incremental)?;
            // Zero-lift increments are not executable and must not win bisection.
            if delta_l == 0 {
                return Ok(false);
            }
            if Self::ensure_side_capacity(netuid, side, delta_l).is_err() {
                return Ok(false);
            }
            match side {
                // Short collateral is TAO. Apply LTV to that TAO amount directly;
                // convert only the alpha principal to a TAO debt value.
                Side::Short => {
                    let max_debt = Self::mul_bps(total_collateral, ltv_bps)?;
                    let debt_value = Self::tao_for_alpha(
                        total_principal,
                        T::Pool::current_alpha_price(netuid),
                        true,
                    )?;
                    Ok(debt_value <= max_debt)
                }
                Side::Long => {
                    let v =
                        Self::v_dyn(netuid, total_collateral, total_principal, reference, owner)?;
                    let max_p = Self::mul_bps(v, ltv_bps)?;
                    Ok(total_principal <= max_p)
                }
            }
        }

        fn lift_tao_for_principal(
            netuid: NetUid,
            side: Side,
            principal: u64,
        ) -> Result<u64, DispatchError> {
            let y = T::Pool::pool_tao(netuid).to_u64();
            let x = T::Pool::pool_alpha(netuid).to_u64();
            ensure!(y > 0 && x > 0, Error::<T>::EmptyPool);
            match side {
                // g = P/x → τ_L = floor(y * P / x)
                Side::Short => {
                    ensure!(principal < x, Error::<T>::PrincipalExceedsAlphaIn);
                    Ok(((u128::from(y) * u128::from(principal)) / u128::from(x)) as u64)
                }
                // g = P/y → τ_L = P
                Side::Long => {
                    ensure!(principal < y, Error::<T>::PrincipalExceedsTaoIn);
                    Ok(principal)
                }
            }
        }

        fn lift_fraction(
            netuid: NetUid,
            side: Side,
            principal: u64,
        ) -> Result<(u64, u64), DispatchError> {
            let y = T::Pool::pool_tao(netuid).to_u64();
            let x = T::Pool::pool_alpha(netuid).to_u64();
            ensure!(principal > 0, Error::<T>::PrincipalZero);
            ensure!(y > 0 && x > 0, Error::<T>::EmptyPool);
            match side {
                Side::Short => {
                    ensure!(principal < x, Error::<T>::PrincipalExceedsAlphaIn);
                    Ok((principal, x))
                }
                Side::Long => {
                    ensure!(principal < y, Error::<T>::PrincipalExceedsTaoIn);
                    Ok((principal, y))
                }
            }
        }

        #[transactional]
        fn do_open(
            owner: T::AccountId,
            netuid: NetUid,
            side: Side,
            collateral: u64,
            source_hotkey: Option<T::AccountId>,
            requested_principal: Option<u64>,
            min_borrow: u64,
            min_proceeds: u64,
            min_escrow_alpha: u64,
            valid_until: BlockNumberFor<T>,
        ) -> DispatchResult {
            let now = frame_system::Pallet::<T>::block_number();
            ensure!(now <= valid_until, Error::<T>::DeadlinePassed);
            // Subnet / lending gates before any amount or depth check.
            Self::ensure_lending_live(netuid)?;
            Self::ensure_new_subnet_gate(netuid)?;
            ensure!(
                T::Pool::owner_allowed(&owner),
                Error::<T>::SubnetUnavailable
            );
            ensure!(collateral > 0, Error::<T>::CollateralZero);
            if let Some(p) = requested_principal {
                ensure!(p > 0, Error::<T>::PrincipalZero);
            }
            // Longs lock stake on source_hotkey; shorts are TAO-only (§14).
            match side {
                Side::Long => ensure!(source_hotkey.is_some(), Error::<T>::InvalidSourceHotkey),
                Side::Short => ensure!(source_hotkey.is_none(), Error::<T>::InvalidSourceHotkey),
            }
            // Gate already requires a warm reference; re-check for a clear error.
            let reference = Self::ensure_or_init_reference(netuid)?;

            // Accrue interest before sizing so LTV/capacity use remaining collateral.
            let mut existing = Positions::<T>::get(&owner, netuid);
            if let Some(ref position) = existing {
                Self::ensure_matching_side(position, side)?;
            }
            if let Some(mut position) = existing.take() {
                Self::charge_interest(&owner, netuid, &mut position, now, false)?;
                if Positions::<T>::contains_key(&owner, netuid) {
                    // Persist accrued collateral so aggregate `a_ahead` exclusion
                    // matches pending interest already moved off the book.
                    Positions::<T>::insert(&owner, netuid, &position);
                    existing = Some(position);
                } else {
                    // Exhausted and forfeited — treat as a fresh open.
                    existing = None;
                }
            }
            if existing.is_none() {
                ensure!(
                    PositionCount::<T>::get(netuid) < T::MaxPositionsPerSubnet::get(),
                    Error::<T>::TooManyPositions
                );
                ensure!(
                    TotalPositions::<T>::get() < T::MaxTotalPositions::get(),
                    Error::<T>::TooManyPositions
                );
            }
            let params = Self::lift_params();
            let principal = if let Some(p) = requested_principal {
                let old = existing.as_ref().map(|x| x.principal).unwrap_or(0);
                let total = Self::add(old, p)?;
                let total_coll = Self::add(
                    existing.as_ref().map(|x| x.collateral).unwrap_or(0),
                    collateral,
                )?;
                let ltv = Self::ltv_tier_bps(netuid, side, &params);
                ensure!(
                    Self::principal_feasible(
                        netuid,
                        side,
                        total,
                        total_coll,
                        old,
                        Some(&owner),
                        &reference,
                        ltv,
                        &params,
                    )?,
                    Error::<T>::DynamicCollateral
                );
                let delta_l = Self::lift_tao_for_principal(netuid, side, p)?;
                Self::ensure_side_capacity(netuid, side, delta_l)?;
                p
            } else {
                Self::max_principal_for_collateral(
                    netuid,
                    side,
                    collateral,
                    existing.as_ref(),
                    Some(&owner),
                    &reference,
                    &params,
                )?
            };

            let price = reference.price.min(T::Pool::current_alpha_price(netuid));
            // Dust / side-capacity before V_dyn and min_borrow so a capacity-bound
            // zero sizes as SideCapacity (not PrincipalZero / BelowMinimumBorrow).
            if principal == 0 {
                return Err(Self::refuse_dust_or_capacity(netuid, side, price, &params));
            }
            let v = match side {
                // Short collateral is already TAO; do not price it as alpha.
                Side::Short => Self::add(
                    existing.as_ref().map(|x| x.collateral).unwrap_or(0),
                    collateral,
                )?,
                Side::Long => {
                    let old_p = existing.as_ref().map(|x| x.principal).unwrap_or(0);
                    let total_p = Self::add(old_p, principal)?;
                    let total_c = Self::add(
                        existing.as_ref().map(|x| x.collateral).unwrap_or(0),
                        collateral,
                    )?;
                    Self::v_dyn(netuid, total_c, total_p, &reference, Some(&owner))?
                }
            };
            let quote = Self::validate_sized_principal(
                netuid, side, principal, price, collateral, v, 0, &params,
            )?;
            ensure!(principal >= min_borrow, Error::<T>::BelowMinimumBorrow);

            let increasing = existing.is_some();
            let mut position = if let Some(position) = existing {
                position
            } else {
                let escrow = Self::position_account(&owner, netuid);
                // After a coldkey swap the old owner's derived escrow remains the
                // migrated loan's custody. Refuse a fresh open that would reuse it.
                ensure!(
                    !EscrowOwner::<T>::contains_key(netuid, &escrow),
                    Error::<T>::EscrowInUse
                );
                // One-year fixed term from open (§15). Increases restart it.
                let term: BlockNumberFor<T> = T::BlocksPerYear::get().saturated_into();
                Position {
                    side,
                    custody: escrow,
                    principal: 0,
                    collateral: 0,
                    proceeds: 0,
                    escrow_alpha: 0,
                    lift_tao: 0,
                    lift_alpha: 0,
                    annual_interest: 0,
                    last_accrued: now,
                    interest_remainder: 0,
                    due: now.saturating_add(T::InterestPeriod::get()),
                    expires_at: now.saturating_add(term),
                    expiry_due: now.saturating_add(term),
                }
            };

            let custody = Self::custody_hotkey()?;
            let reserve = Self::reserve_account(netuid);
            let escrow = position.custody.clone();
            let (g_numer, g_denom) = Self::lift_fraction(netuid, side, principal)?;
            let (lift_alpha, lift_tao) =
                T::Pool::lift_and_debit(netuid, g_numer, g_denom, &reserve, &custody)?;
            Self::deposit_event(Event::Lifted {
                netuid,
                tao: lift_tao.to_u64(),
                alpha: lift_alpha.to_u64(),
            });

            let (proceeds, escrow_alpha) = match side {
                Side::Short => {
                    // Collateral is TAO.
                    T::Pool::transfer_tao(&owner, &escrow, collateral.into())?;
                    // Force-sell principal alpha (must match lift alpha principal leg).
                    let sell = principal.min(lift_alpha.to_u64());
                    ensure!(sell == principal, Error::<T>::InvalidQuote);
                    let e_s = T::Pool::forced_sell_into_escrow(
                        &reserve,
                        &custody,
                        &escrow,
                        netuid,
                        sell.into(),
                        0u64.into(),
                    )?;
                    ensure!(e_s.to_u64() >= min_proceeds, Error::<T>::SlippageExceeded);
                    // Hold remaining lift TAO; alpha principal was sold (lift_alpha may exceed
                    // principal by rounding — hold dust alpha).
                    let held_alpha = lift_alpha.to_u64().saturating_sub(sell);
                    Lifts::<T>::mutate(netuid, |book| {
                        book.l_s = book.l_s.saturating_add(lift_tao.to_u64());
                        book.held_tao = book.held_tao.saturating_add(lift_tao.to_u64());
                        book.held_alpha = book.held_alpha.saturating_add(held_alpha);
                        book.escrow_tao = book.escrow_tao.saturating_add(e_s.to_u64());
                    });
                    (e_s.to_u64(), 0u64)
                }
                Side::Long => {
                    let source_hotkey = source_hotkey
                        .as_ref()
                        .ok_or(Error::<T>::InvalidSourceHotkey)?;
                    T::Pool::lock_alpha_collateral(
                        &owner,
                        source_hotkey,
                        &escrow,
                        &custody,
                        netuid,
                        collateral.into(),
                    )?;
                    // Force-buy with principal TAO into escrow alpha.
                    let buy_tao = principal.min(lift_tao.to_u64());
                    ensure!(buy_tao == principal, Error::<T>::InvalidQuote);
                    // Move buy TAO from reserve custody free balance into escrow, then buy.
                    T::Pool::transfer_tao(&reserve, &escrow, buy_tao.into())?;
                    let a = T::Pool::forced_buy_into_custody(
                        &escrow,
                        &custody,
                        netuid,
                        buy_tao.into(),
                    )?;
                    ensure!(a.to_u64() >= min_escrow_alpha, Error::<T>::SlippageExceeded);
                    Self::deposit_event(Event::LongEscrowed {
                        owner: owner.clone(),
                        netuid,
                        tao_in: buy_tao,
                        alpha_escrowed: a.to_u64(),
                    });
                    let held_tao = lift_tao.to_u64().saturating_sub(buy_tao);
                    Lifts::<T>::mutate(netuid, |book| {
                        book.l_l = book.l_l.saturating_add(principal);
                        book.b_l = book.b_l.saturating_add(principal);
                        book.held_tao = book.held_tao.saturating_add(held_tao);
                        book.held_alpha = book.held_alpha.saturating_add(lift_alpha.to_u64());
                        book.escrow_alpha = book.escrow_alpha.saturating_add(a.to_u64());
                        book.long_collateral_alpha =
                            book.long_collateral_alpha.saturating_add(collateral);
                    });
                    (0u64, a.to_u64())
                }
            };

            // §5.4: post-trade spot vs lending EMA (transactional → rolls back on Err).
            Self::ensure_open_ema_band(
                side,
                T::Pool::current_alpha_price(netuid),
                reference.price,
                &params,
            )?;

            position.principal = Self::add(position.principal, principal)?;
            position.collateral = Self::add(position.collateral, collateral)?;
            position.proceeds = Self::add(position.proceeds, proceeds)?;
            position.escrow_alpha = Self::add(position.escrow_alpha, escrow_alpha)?;
            position.lift_tao = Self::add(position.lift_tao, lift_tao.to_u64())?;
            position.lift_alpha = Self::add(position.lift_alpha, lift_alpha.to_u64())?;
            position.annual_interest = Self::add(position.annual_interest, quote.annual_interest)?;

            if !increasing {
                let due = position.due;
                let expires_at = position.expires_at;
                OpenByNetuid::<T>::insert(netuid, &owner, ());
                EscrowOwner::<T>::insert(netuid, &escrow, &owner);
                PositionCount::<T>::mutate(netuid, |n| *n = n.saturating_add(1));
                TotalPositions::<T>::mutate(|n| *n = n.saturating_add(1));
                Due::<T>::insert(due, (&owner, netuid), ());
                Self::note_due_inserted(due);
                Expiry::<T>::insert(expires_at, (&owner, netuid), ());
                Self::note_expiry_inserted(expires_at);
                Self::note_active_net(netuid);
            } else if principal > 0 || collateral > 0 {
                // Restart the one-year term on increases (Jacob allowed
                // keep-or-restart). Keep-original enabled near-expiry drains:
                // long collateral redeemed from the pot at EMA above AMM, and
                // short principal force-sold at live spot then settled at a
                // lower EMA (§15 / Skeptic).
                let term: BlockNumberFor<T> = T::BlocksPerYear::get().saturated_into();
                let new_expiry = now.saturating_add(term);
                if new_expiry != position.expiry_due {
                    if Expiry::<T>::take(position.expiry_due, (&owner, netuid)).is_some() {
                        Self::note_expiry_removed(position.expiry_due);
                    }
                    Expiry::<T>::insert(new_expiry, (&owner, netuid), ());
                    Self::note_expiry_inserted(new_expiry);
                    position.expires_at = new_expiry;
                    position.expiry_due = new_expiry;
                }
            }
            Positions::<T>::insert(&owner, netuid, position);
            Self::deposit_event(if increasing {
                Event::Increased {
                    owner,
                    netuid,
                    side,
                    principal,
                    collateral,
                    proceeds,
                    escrow_alpha,
                    lift_tao: lift_tao.to_u64(),
                    lift_alpha: lift_alpha.to_u64(),
                    annual_interest: quote.annual_interest,
                }
            } else {
                Event::Opened {
                    owner,
                    netuid,
                    side,
                    principal,
                    collateral,
                    proceeds,
                    escrow_alpha,
                    lift_tao: lift_tao.to_u64(),
                    lift_alpha: lift_alpha.to_u64(),
                    annual_interest: quote.annual_interest,
                }
            });
            Ok(())
        }

        #[transactional]
        fn do_close(
            owner: T::AccountId,
            netuid: NetUid,
            hotkey: T::AccountId,
            repay_from_wallet: bool,
            max_payment: u64,
            min_refund: u64,
            valid_until: BlockNumberFor<T>,
        ) -> DispatchResult {
            let now = frame_system::Pallet::<T>::block_number();
            ensure!(now <= valid_until, Error::<T>::DeadlinePassed);
            ensure!(
                !Dissolutions::<T>::contains_key(netuid),
                Error::<T>::SubnetUnavailable
            );
            let mut position =
                Positions::<T>::get(&owner, netuid).ok_or(Error::<T>::PositionMissing)?;
            Self::charge_interest(&owner, netuid, &mut position, now, true)?;
            // Interest can exhaust collateral and `forfeit`, which already restored
            // lifts, moved escrow, and removed indexes. Do not continue close on a
            // forfeited loan (would double-readd / undercount PositionCount).
            if !Positions::<T>::contains_key(&owner, netuid) {
                return Ok(());
            }
            ensure!(
                position.collateral > 0 || position.side == Side::Short,
                Error::<T>::InsufficientEscrow
            );

            let custody = Self::custody_hotkey()?;
            let reserve = Self::reserve_account(netuid);
            let escrow = position.custody.clone();
            let side = position.side;
            let (refund, escrow_returned) = match side {
                Side::Short => {
                    let payment = if repay_from_wallet {
                        // §5.2: max_payment caps alpha repaid (post-interest P).
                        ensure!(
                            position.principal <= max_payment,
                            Error::<T>::SlippageExceeded
                        );
                        T::Pool::repay_alpha(
                            &owner,
                            &hotkey,
                            &reserve,
                            &custody,
                            netuid,
                            position.principal.into(),
                        )?;
                        // Return escrow proceeds + remaining collateral to owner.
                        let pot = Self::add(position.proceeds, position.collateral)?;
                        ensure!(pot >= min_refund, Error::<T>::BelowMinimumRefund);
                        T::Pool::transfer_tao(&escrow, &owner, pot.into())?;
                        // Readd lift legs: (τ_L, P)
                        T::Pool::readd_and_credit(
                            netuid,
                            &reserve,
                            &custody,
                            position.lift_tao.into(),
                            position.principal.into(),
                        )?;
                        Self::deposit_event(Event::Readded {
                            netuid,
                            tao: position.lift_tao,
                            alpha: position.principal,
                        });
                        Lifts::<T>::mutate(netuid, |book| {
                            book.l_s = book.l_s.saturating_sub(position.lift_tao);
                            book.held_tao = book.held_tao.saturating_sub(position.lift_tao);
                            book.escrow_tao = book.escrow_tao.saturating_sub(position.proceeds);
                        });
                        (pot, position.proceeds)
                    } else {
                        // Buy back P alpha with E_S + collateral.
                        let budget = Self::add(position.proceeds, position.collateral)?;
                        let (paid, bought) = Self::execute_buyback(
                            &escrow,
                            &custody,
                            netuid,
                            position.principal,
                            budget,
                        )?;
                        ensure!(paid <= max_payment, Error::<T>::SlippageExceeded);
                        let refund = budget.saturating_sub(paid);
                        ensure!(refund >= min_refund, Error::<T>::BelowMinimumRefund);
                        if refund > 0 {
                            T::Pool::transfer_tao(&escrow, &owner, refund.into())?;
                        }
                        // Return any surplus alpha from overshoot before readd.
                        let excess = bought.saturating_sub(position.principal);
                        if excess > 0 {
                            T::Pool::transfer_staked_alpha(
                                &escrow,
                                &custody,
                                &owner,
                                &hotkey,
                                netuid,
                                excess.into(),
                                false,
                                true,
                            )?;
                        }
                        // Move bought principal alpha to reserve and readd.
                        T::Pool::transfer_staked_alpha(
                            &escrow,
                            &custody,
                            &reserve,
                            &custody,
                            netuid,
                            position.principal.into(),
                            false,
                            false,
                        )?;
                        T::Pool::readd_and_credit(
                            netuid,
                            &reserve,
                            &custody,
                            position.lift_tao.into(),
                            position.principal.into(),
                        )?;
                        Lifts::<T>::mutate(netuid, |book| {
                            book.l_s = book.l_s.saturating_sub(position.lift_tao);
                            book.held_tao = book.held_tao.saturating_sub(position.lift_tao);
                            book.escrow_tao = book.escrow_tao.saturating_sub(position.proceeds);
                        });
                        (refund, 0)
                    };
                    payment
                }
                Side::Long => {
                    if repay_from_wallet {
                        ensure!(
                            position.principal <= max_payment,
                            Error::<T>::SlippageExceeded
                        );
                        T::Pool::transfer_tao(&owner, &reserve, position.principal.into())?;
                        // Return A + remaining C to owner.
                        let alpha_out = Self::add(position.escrow_alpha, position.collateral)?;
                        if alpha_out > 0 {
                            T::Pool::transfer_staked_alpha(
                                &escrow,
                                &custody,
                                &owner,
                                &hotkey,
                                netuid,
                                alpha_out.into(),
                                false,
                                true,
                            )?;
                        }
                        T::Pool::readd_and_credit(
                            netuid,
                            &reserve,
                            &custody,
                            position.principal.into(),
                            position.lift_alpha.into(),
                        )?;
                        Lifts::<T>::mutate(netuid, |book| {
                            book.l_l = book.l_l.saturating_sub(position.principal);
                            book.b_l = book.b_l.saturating_sub(position.principal);
                            book.held_alpha = book.held_alpha.saturating_sub(position.lift_alpha);
                            book.escrow_alpha =
                                book.escrow_alpha.saturating_sub(position.escrow_alpha);
                            book.long_collateral_alpha = book
                                .long_collateral_alpha
                                .saturating_sub(position.collateral);
                        });
                        ensure!(
                            alpha_out >= min_refund || min_refund == 0,
                            Error::<T>::BelowMinimumRefund
                        );
                        (alpha_out, position.escrow_alpha)
                    } else {
                        // Sell A, then the minimal collateral needed to cover P;
                        // leftover collateral returns as alpha (min_refund is alpha).
                        let mut raised = 0u64;
                        if position.escrow_alpha > 0 {
                            let out = T::Pool::forced_sell_into_escrow(
                                &escrow,
                                &custody,
                                &escrow,
                                netuid,
                                position.escrow_alpha.into(),
                                0u64.into(),
                            )?;
                            raised = Self::add(raised, out.to_u64())?;
                        }
                        let mut coll_left = position.collateral;
                        if raised < position.principal && coll_left > 0 {
                            let need = position.principal.saturating_sub(raised);
                            // Never fall back to dumping all C on a quote error —
                            // propagate so the call reverts instead of liquidating.
                            let needed_alpha = Self::alpha_to_raise_tao(netuid, need)?;
                            let sell = needed_alpha.min(coll_left);
                            ensure!(sell > 0, Error::<T>::InsufficientEscrow);
                            let floor = if sell >= needed_alpha { need } else { 0u64 };
                            let out = T::Pool::forced_sell_into_escrow(
                                &escrow,
                                &custody,
                                &escrow,
                                netuid,
                                sell.into(),
                                floor.into(),
                            )?;
                            raised = Self::add(raised, out.to_u64())?;
                            coll_left = coll_left.saturating_sub(sell);
                        }
                        ensure!(raised >= position.principal, Error::<T>::InsufficientEscrow);
                        ensure!(
                            position.principal <= max_payment,
                            Error::<T>::SlippageExceeded
                        );
                        T::Pool::transfer_tao(&escrow, &reserve, position.principal.into())?;
                        // Dust TAO above P (rounding) returns with the alpha refund path
                        // via free balance; primary refund is remaining collateral alpha.
                        let surplus = raised.saturating_sub(position.principal);
                        if surplus > 0 {
                            T::Pool::transfer_tao(&escrow, &owner, surplus.into())?;
                        }
                        if coll_left > 0 {
                            T::Pool::transfer_staked_alpha(
                                &escrow,
                                &custody,
                                &owner,
                                &hotkey,
                                netuid,
                                coll_left.into(),
                                false,
                                true,
                            )?;
                        }
                        T::Pool::readd_and_credit(
                            netuid,
                            &reserve,
                            &custody,
                            position.principal.into(),
                            position.lift_alpha.into(),
                        )?;
                        Lifts::<T>::mutate(netuid, |book| {
                            book.l_l = book.l_l.saturating_sub(position.principal);
                            book.b_l = book.b_l.saturating_sub(position.principal);
                            book.held_alpha = book.held_alpha.saturating_sub(position.lift_alpha);
                            book.escrow_alpha =
                                book.escrow_alpha.saturating_sub(position.escrow_alpha);
                            book.long_collateral_alpha = book
                                .long_collateral_alpha
                                .saturating_sub(position.collateral);
                        });
                        ensure!(
                            coll_left >= min_refund || min_refund == 0,
                            Error::<T>::BelowMinimumRefund
                        );
                        (coll_left, 0)
                    }
                }
            };

            Self::remove_position(&owner, netuid, &position);
            Self::deposit_event(Event::Closed {
                owner,
                netuid,
                side,
                refund,
                escrow_returned,
            });
            Ok(())
        }

        /// Pro-rata field slice: `field * principal_part / principal` (floor).
        fn scale_part(field: u64, principal_part: u64, principal: u64) -> u64 {
            if principal == 0 || principal_part == 0 {
                return 0;
            }
            ((u128::from(field) * u128::from(principal_part)) / u128::from(principal)) as u64
        }

        /// §4.4 partial repay: accrue+charge interest on the whole position, then
        /// settle a `k = principal_part / P` slice and scale the remainder by `(1 − k)`.
        #[transactional]
        fn do_repay(
            owner: T::AccountId,
            netuid: NetUid,
            hotkey: T::AccountId,
            principal_part: u64,
            repay_from_wallet: bool,
            max_payment: u64,
            min_refund: u64,
            valid_until: BlockNumberFor<T>,
        ) -> DispatchResult {
            let now = frame_system::Pallet::<T>::block_number();
            ensure!(now <= valid_until, Error::<T>::DeadlinePassed);
            ensure!(
                !Dissolutions::<T>::contains_key(netuid),
                Error::<T>::SubnetUnavailable
            );
            ensure!(principal_part > 0, Error::<T>::PrincipalZero);
            let existing =
                Positions::<T>::get(&owner, netuid).ok_or(Error::<T>::PositionMissing)?;
            ensure!(
                principal_part <= existing.principal,
                Error::<T>::AmountTooSmall
            );
            // Full repay is close (interest charged once inside do_close).
            if principal_part == existing.principal {
                return Self::do_close(
                    owner,
                    netuid,
                    hotkey,
                    repay_from_wallet,
                    max_payment,
                    min_refund,
                    valid_until,
                );
            }
            let mut position = existing;
            // Interest first on the whole position (§4.4).
            Self::charge_interest(&owner, netuid, &mut position, now, true)?;
            // Same as close / settle: forfeit already cleaned up — stop.
            if !Positions::<T>::contains_key(&owner, netuid) {
                return Ok(());
            }
            ensure!(
                position.collateral > 0 || position.side == Side::Short,
                Error::<T>::InsufficientEscrow
            );

            let custody = Self::custody_hotkey()?;
            let reserve = Self::reserve_account(netuid);
            let escrow = position.custody.clone();
            let side = position.side;
            let p = position.principal;
            let slice_p = principal_part;
            let slice_c = Self::scale_part(position.collateral, slice_p, p);
            let slice_e_s = Self::scale_part(position.proceeds, slice_p, p);
            let slice_a = Self::scale_part(position.escrow_alpha, slice_p, p);
            let slice_lift_tao = Self::scale_part(position.lift_tao, slice_p, p);
            let slice_lift_alpha = Self::scale_part(position.lift_alpha, slice_p, p);

            let (refund, escrow_returned) = match side {
                Side::Short => {
                    if repay_from_wallet {
                        // §5.2: max_payment caps alpha repaid on the slice.
                        ensure!(slice_p <= max_payment, Error::<T>::SlippageExceeded);
                        T::Pool::repay_alpha(
                            &owner,
                            &hotkey,
                            &reserve,
                            &custody,
                            netuid,
                            slice_p.into(),
                        )?;
                        let pot = slice_e_s;
                        ensure!(pot >= min_refund, Error::<T>::BelowMinimumRefund);
                        if pot > 0 {
                            T::Pool::transfer_tao(&escrow, &owner, pot.into())?;
                        }
                        // Release slice collateral TAO.
                        if slice_c > 0 {
                            T::Pool::transfer_tao(&escrow, &owner, slice_c.into())?;
                        }
                        T::Pool::readd_and_credit(
                            netuid,
                            &reserve,
                            &custody,
                            slice_lift_tao.into(),
                            slice_p.into(),
                        )?;
                        Self::deposit_event(Event::Readded {
                            netuid,
                            tao: slice_lift_tao,
                            alpha: slice_p,
                        });
                        Lifts::<T>::mutate(netuid, |book| {
                            book.l_s = book.l_s.saturating_sub(slice_lift_tao);
                            book.held_tao = book.held_tao.saturating_sub(slice_lift_tao);
                            book.escrow_tao = book.escrow_tao.saturating_sub(slice_e_s);
                        });
                        (pot.saturating_add(slice_c), slice_e_s)
                    } else {
                        let budget = Self::add(slice_e_s, slice_c)?;
                        let (paid, bought) =
                            Self::execute_buyback(&escrow, &custody, netuid, slice_p, budget)?;
                        ensure!(paid <= max_payment, Error::<T>::SlippageExceeded);
                        let refund = budget.saturating_sub(paid);
                        ensure!(refund >= min_refund, Error::<T>::BelowMinimumRefund);
                        if refund > 0 {
                            T::Pool::transfer_tao(&escrow, &owner, refund.into())?;
                        }
                        let excess = bought.saturating_sub(slice_p);
                        if excess > 0 {
                            T::Pool::transfer_staked_alpha(
                                &escrow,
                                &custody,
                                &owner,
                                &hotkey,
                                netuid,
                                excess.into(),
                                false,
                                true,
                            )?;
                        }
                        T::Pool::transfer_staked_alpha(
                            &escrow,
                            &custody,
                            &reserve,
                            &custody,
                            netuid,
                            slice_p.into(),
                            false,
                            false,
                        )?;
                        T::Pool::readd_and_credit(
                            netuid,
                            &reserve,
                            &custody,
                            slice_lift_tao.into(),
                            slice_p.into(),
                        )?;
                        Lifts::<T>::mutate(netuid, |book| {
                            book.l_s = book.l_s.saturating_sub(slice_lift_tao);
                            book.held_tao = book.held_tao.saturating_sub(slice_lift_tao);
                            book.escrow_tao = book.escrow_tao.saturating_sub(slice_e_s);
                        });
                        (refund, 0)
                    }
                }
                Side::Long => {
                    if repay_from_wallet {
                        ensure!(slice_p <= max_payment, Error::<T>::SlippageExceeded);
                        T::Pool::transfer_tao(&owner, &reserve, slice_p.into())?;
                        // Return k·A + k·C to owner stake.
                        let alpha_out = Self::add(slice_a, slice_c)?;
                        if alpha_out > 0 {
                            T::Pool::transfer_staked_alpha(
                                &escrow,
                                &custody,
                                &owner,
                                &hotkey,
                                netuid,
                                alpha_out.into(),
                                false,
                                true,
                            )?;
                        }
                        T::Pool::readd_and_credit(
                            netuid,
                            &reserve,
                            &custody,
                            slice_p.into(),
                            slice_lift_alpha.into(),
                        )?;
                        Lifts::<T>::mutate(netuid, |book| {
                            book.l_l = book.l_l.saturating_sub(slice_p);
                            book.b_l = book.b_l.saturating_sub(slice_p);
                            book.held_alpha = book.held_alpha.saturating_sub(slice_lift_alpha);
                            book.escrow_alpha = book.escrow_alpha.saturating_sub(slice_a);
                            book.long_collateral_alpha =
                                book.long_collateral_alpha.saturating_sub(slice_c);
                        });
                        ensure!(
                            alpha_out >= min_refund || min_refund == 0,
                            Error::<T>::BelowMinimumRefund
                        );
                        (alpha_out, slice_a)
                    } else {
                        let mut raised = 0u64;
                        if slice_a > 0 {
                            let out = T::Pool::forced_sell_into_escrow(
                                &escrow,
                                &custody,
                                &escrow,
                                netuid,
                                slice_a.into(),
                                0u64.into(),
                            )?;
                            raised = Self::add(raised, out.to_u64())?;
                        }
                        let mut coll_left = slice_c;
                        if raised < slice_p && coll_left > 0 {
                            let need = slice_p.saturating_sub(raised);
                            let needed_alpha = Self::alpha_to_raise_tao(netuid, need)?;
                            let sell = needed_alpha.min(coll_left);
                            ensure!(sell > 0, Error::<T>::InsufficientEscrow);
                            let floor = if sell >= needed_alpha { need } else { 0u64 };
                            let out = T::Pool::forced_sell_into_escrow(
                                &escrow,
                                &custody,
                                &escrow,
                                netuid,
                                sell.into(),
                                floor.into(),
                            )?;
                            raised = Self::add(raised, out.to_u64())?;
                            coll_left = coll_left.saturating_sub(sell);
                        }
                        ensure!(raised >= slice_p, Error::<T>::InsufficientEscrow);
                        ensure!(slice_p <= max_payment, Error::<T>::SlippageExceeded);
                        T::Pool::transfer_tao(&escrow, &reserve, slice_p.into())?;
                        let surplus = raised.saturating_sub(slice_p);
                        if surplus > 0 {
                            T::Pool::transfer_tao(&escrow, &owner, surplus.into())?;
                        }
                        if coll_left > 0 {
                            T::Pool::transfer_staked_alpha(
                                &escrow,
                                &custody,
                                &owner,
                                &hotkey,
                                netuid,
                                coll_left.into(),
                                false,
                                true,
                            )?;
                        }
                        T::Pool::readd_and_credit(
                            netuid,
                            &reserve,
                            &custody,
                            slice_p.into(),
                            slice_lift_alpha.into(),
                        )?;
                        Lifts::<T>::mutate(netuid, |book| {
                            book.l_l = book.l_l.saturating_sub(slice_p);
                            book.b_l = book.b_l.saturating_sub(slice_p);
                            book.held_alpha = book.held_alpha.saturating_sub(slice_lift_alpha);
                            book.escrow_alpha = book.escrow_alpha.saturating_sub(slice_a);
                            book.long_collateral_alpha =
                                book.long_collateral_alpha.saturating_sub(slice_c);
                        });
                        ensure!(
                            coll_left >= min_refund || min_refund == 0,
                            Error::<T>::BelowMinimumRefund
                        );
                        (coll_left, 0)
                    }
                }
            };

            // Scale remainder by (1 − k); annual_interest scales with principal.
            position.principal = position.principal.saturating_sub(slice_p);
            position.collateral = position.collateral.saturating_sub(slice_c);
            position.proceeds = position.proceeds.saturating_sub(slice_e_s);
            position.escrow_alpha = position.escrow_alpha.saturating_sub(slice_a);
            position.lift_tao = position.lift_tao.saturating_sub(slice_lift_tao);
            position.lift_alpha = position.lift_alpha.saturating_sub(slice_lift_alpha);
            position.annual_interest =
                Self::scale_part(position.annual_interest, position.principal, p);

            // Dust: remainder 0 or opening value ≥ MinimumLoanValue.
            if position.principal > 0 {
                let price = T::Pool::current_alpha_price(netuid);
                let opening_value = match side {
                    Side::Short => Self::tao_for_alpha(position.principal, price, true)?,
                    Side::Long => position.principal,
                };
                ensure!(
                    opening_value >= T::MinimumLoanValue::get(),
                    Error::<T>::AmountTooSmall
                );
            }

            Positions::<T>::insert(&owner, netuid, position);
            Self::deposit_event(Event::Repaid {
                owner,
                netuid,
                side,
                principal_repaid: slice_p,
                refund,
                escrow_returned,
            });
            Ok(())
        }

        /// Largest buy input in `1..=cap` for which `quote_buy` succeeds.
        fn max_fillable_buy(netuid: NetUid, cap: u64) -> Result<u64, DispatchError> {
            if cap == 0 {
                return Err(Error::<T>::InsufficientEscrow.into());
            }
            if T::Pool::quote_buy(netuid, cap.into()).is_ok() {
                return Ok(cap);
            }
            let mut lo = 1u64;
            let mut hi = cap.saturating_sub(1);
            let mut best = 0u64;
            for _ in 0..64 {
                if lo > hi {
                    break;
                }
                let mid = lo.saturating_add(hi.saturating_sub(lo) / 2);
                match T::Pool::quote_buy(netuid, mid.into()) {
                    Ok(_) => {
                        best = mid;
                        lo = mid.saturating_add(1);
                    }
                    Err(_) => {
                        hi = mid.saturating_sub(1);
                    }
                }
            }
            ensure!(best > 0, Error::<T>::InsufficientEscrow);
            Ok(best)
        }

        /// Conservative TAO input that quotes to at least `alpha_wanted`.
        /// Seeds from a CP invert rather than probing `max_buy_input` (which
        /// fails with `SlippageTooHigh` on real Balancer pools at reserve×1000).
        fn tao_to_buy_alpha(netuid: NetUid, alpha_wanted: u64) -> Result<u64, DispatchError> {
            if alpha_wanted == 0 {
                return Ok(0);
            }
            let y = T::Pool::pool_tao(netuid).to_u64();
            let x = T::Pool::pool_alpha(netuid).to_u64();
            ensure!(y > 0 && x > 0, Error::<T>::InsufficientEscrow);
            let max_in = T::Pool::max_buy_input(netuid).to_u64().max(1);
            // CP invert: t ≥ want·y / (x − want), then ×2 headroom.
            let mut hi = if alpha_wanted >= x {
                max_in
            } else {
                let numer = u128::from(alpha_wanted).saturating_mul(u128::from(y));
                let denom = u128::from(x.saturating_sub(alpha_wanted)).max(1);
                ((numer / denom).saturating_add(1) as u64)
                    .saturating_mul(2)
                    .max(1)
                    .min(max_in)
            };
            let mut fillable_cap = max_in;
            for _ in 0..64 {
                match T::Pool::quote_buy(netuid, hi.into()) {
                    Ok(got) if got.to_u64() >= alpha_wanted => break,
                    Ok(_) => {
                        let next = hi.saturating_mul(2).min(fillable_cap);
                        if next == hi {
                            return Err(Error::<T>::InsufficientEscrow.into());
                        }
                        hi = next;
                    }
                    Err(_) => {
                        fillable_cap = Self::max_fillable_buy(netuid, hi)?;
                        hi = fillable_cap;
                        let got = T::Pool::quote_buy(netuid, hi.into())?.to_u64();
                        ensure!(got >= alpha_wanted, Error::<T>::InsufficientEscrow);
                        break;
                    }
                }
            }
            ensure!(
                T::Pool::quote_buy(netuid, hi.into())?.to_u64() >= alpha_wanted,
                Error::<T>::InsufficientEscrow
            );
            let mut lo = 1u64;
            for _ in 0..64 {
                if lo >= hi {
                    break;
                }
                let mid = lo.saturating_add(hi.saturating_sub(lo) / 2);
                match T::Pool::quote_buy(netuid, mid.into()) {
                    Ok(got) if got.to_u64() >= alpha_wanted => hi = mid,
                    Ok(_) => lo = mid.saturating_add(1),
                    Err(_) => {
                        hi = mid.saturating_sub(1).max(lo);
                    }
                }
            }
            ensure!(
                T::Pool::quote_buy(netuid, hi.into())?.to_u64() >= alpha_wanted,
                Error::<T>::InsufficientEscrow
            );
            Ok(hi)
        }

        /// Largest sell input in `1..=cap` for which `quote_sell` succeeds.
        /// Production `quote_sell` returns `SlippageTooHigh` when `sim_swap`
        /// cannot consume the full input (e.g. probing `max_sell_input` =
        /// reserve×1000), so callers must not assume the cap is fillable.
        fn max_fillable_sell(netuid: NetUid, cap: u64) -> Result<u64, DispatchError> {
            if cap == 0 {
                return Err(Error::<T>::InsufficientEscrow.into());
            }
            if T::Pool::quote_sell(netuid, cap.into()).is_ok() {
                return Ok(cap);
            }
            let mut lo = 1u64;
            let mut hi = cap.saturating_sub(1);
            let mut best = 0u64;
            for _ in 0..64 {
                if lo > hi {
                    break;
                }
                let mid = lo.saturating_add(hi.saturating_sub(lo) / 2);
                match T::Pool::quote_sell(netuid, mid.into()) {
                    Ok(_) => {
                        best = mid;
                        lo = mid.saturating_add(1);
                    }
                    Err(_) => {
                        hi = mid.saturating_sub(1);
                    }
                }
            }
            ensure!(best > 0, Error::<T>::InsufficientEscrow);
            Ok(best)
        }

        /// Minimal alpha that quotes to at least `tao_wanted` on the live pool.
        ///
        /// Does **not** start by quoting `max_sell_input` (reserve×1000), which
        /// fails with `SlippageTooHigh` on real Balancer pools. Seeds `hi` from
        /// a constant-product invert, grows only while quotes succeed, and
        /// shrinks on quote errors to the fillable bound.
        fn alpha_to_raise_tao(netuid: NetUid, tao_wanted: u64) -> Result<u64, DispatchError> {
            if tao_wanted == 0 {
                return Ok(0);
            }
            let y = T::Pool::pool_tao(netuid).to_u64();
            let x = T::Pool::pool_alpha(netuid).to_u64();
            ensure!(y > 0 && x > 0, Error::<T>::InsufficientEscrow);
            let max_in = T::Pool::max_sell_input(netuid).to_u64().max(1);
            // CP invert: a ≥ need·x / (y − need), then ×2 headroom; cap at max_in.
            let mut hi = if tao_wanted >= y {
                max_in
            } else {
                let numer = u128::from(tao_wanted).saturating_mul(u128::from(x));
                let denom = u128::from(y.saturating_sub(tao_wanted)).max(1);
                ((numer / denom).saturating_add(1) as u64)
                    .saturating_mul(2)
                    .max(1)
                    .min(max_in)
            };
            let mut fillable_cap = max_in;
            for _ in 0..64 {
                match T::Pool::quote_sell(netuid, hi.into()) {
                    Ok(got) if got.to_u64() >= tao_wanted => break,
                    Ok(_) => {
                        let next = hi.saturating_mul(2).min(fillable_cap);
                        if next == hi {
                            return Err(Error::<T>::InsufficientEscrow.into());
                        }
                        hi = next;
                    }
                    Err(_) => {
                        fillable_cap = Self::max_fillable_sell(netuid, hi)?;
                        hi = fillable_cap;
                        let got = T::Pool::quote_sell(netuid, hi.into())?.to_u64();
                        ensure!(got >= tao_wanted, Error::<T>::InsufficientEscrow);
                        break;
                    }
                }
            }
            ensure!(
                T::Pool::quote_sell(netuid, hi.into())?.to_u64() >= tao_wanted,
                Error::<T>::InsufficientEscrow
            );
            let mut lo = 1u64;
            for _ in 0..64 {
                if lo >= hi {
                    break;
                }
                let mid = lo.saturating_add(hi.saturating_sub(lo) / 2);
                match T::Pool::quote_sell(netuid, mid.into()) {
                    Ok(got) if got.to_u64() >= tao_wanted => hi = mid,
                    Ok(_) => lo = mid.saturating_add(1),
                    Err(_) => {
                        hi = mid.saturating_sub(1).max(lo);
                    }
                }
            }
            ensure!(
                T::Pool::quote_sell(netuid, hi.into())?.to_u64() >= tao_wanted,
                Error::<T>::InsufficientEscrow
            );
            Ok(hi)
        }

        /// Minimal alpha that raises `tao_wanted` on hypothetical reserves
        /// (fee-free `quote_sell_on`; used by `quote_close` after a simulated A sale).
        fn alpha_to_raise_tao_on(
            tao_reserve: u64,
            alpha_reserve: u64,
            quote_weight_parts: u64,
            tao_wanted: u64,
        ) -> Result<u64, DispatchError> {
            if tao_wanted == 0 {
                return Ok(0);
            }
            ensure!(
                tao_reserve > 0 && alpha_reserve > 0,
                Error::<T>::InsufficientEscrow
            );
            let mut lo = 1u64;
            // Upper bound: sell enough that CP out ≈ y * a / (x+a) ≥ need → a ≥ need·x / (y−need).
            let mut hi = if tao_wanted >= tao_reserve {
                u64::MAX / 4
            } else {
                let numer = u128::from(tao_wanted).saturating_mul(u128::from(alpha_reserve));
                let denom = u128::from(tao_reserve.saturating_sub(tao_wanted)).max(1);
                ((numer / denom).saturating_add(1) as u64)
                    .saturating_mul(2)
                    .max(1)
            };
            for _ in 0..64 {
                let got = T::Pool::quote_sell_on(
                    tao_reserve.into(),
                    alpha_reserve.into(),
                    quote_weight_parts,
                    hi.into(),
                )
                .to_u64();
                if got >= tao_wanted {
                    break;
                }
                let next = hi.saturating_mul(2);
                if next == hi {
                    break;
                }
                hi = next;
            }
            ensure!(
                T::Pool::quote_sell_on(
                    tao_reserve.into(),
                    alpha_reserve.into(),
                    quote_weight_parts,
                    hi.into(),
                )
                .to_u64()
                    >= tao_wanted,
                Error::<T>::InsufficientEscrow
            );
            for _ in 0..64 {
                if lo >= hi {
                    break;
                }
                let mid = lo.saturating_add(hi.saturating_sub(lo) / 2);
                if T::Pool::quote_sell_on(
                    tao_reserve.into(),
                    alpha_reserve.into(),
                    quote_weight_parts,
                    mid.into(),
                )
                .to_u64()
                    >= tao_wanted
                {
                    hi = mid;
                } else {
                    lo = mid.saturating_add(1);
                }
            }
            Ok(hi)
        }

        fn execute_buyback(
            escrow: &T::AccountId,
            custody: &T::AccountId,
            netuid: NetUid,
            principal: u64,
            budget: u64,
        ) -> Result<(u64, u64), DispatchError> {
            let mut payment = 0u64;
            let mut alpha = 0u64;
            for _ in 0..MAX_BUYBACK_STEPS {
                let remaining = principal.saturating_sub(alpha);
                if remaining == 0 {
                    return Ok((payment, alpha));
                }
                // Bound each buy by the TAO needed for the remaining principal,
                // not the entire surplus collateral budget.
                let needed = Self::tao_to_buy_alpha(netuid, remaining)?;
                let capacity = budget
                    .saturating_sub(payment)
                    .min(T::Pool::buy_spendable_tao(escrow).to_u64())
                    .min(T::Pool::max_buy_input(netuid).to_u64())
                    .min(needed);
                ensure!(capacity > 0, Error::<T>::InsufficientEscrow);
                let received = T::Pool::buy_alpha(
                    escrow,
                    custody,
                    netuid,
                    capacity.into(),
                    u64::MAX.into(),
                    false,
                )?
                .to_u64();
                payment = Self::add(payment, capacity)?;
                alpha = Self::add(alpha, received)?;
                if received == 0 {
                    break;
                }
            }
            ensure!(alpha >= principal, Error::<T>::InsufficientEscrow);
            Ok((payment, alpha))
        }

        fn do_open_long_and_sell(
            owner: T::AccountId,
            netuid: NetUid,
            hotkey: T::AccountId,
            collateral_alpha: Option<u64>,
            borrow_tao: Option<u64>,
            max_collateral_alpha: u64,
            min_borrow_tao: u64,
            sell_alpha: Option<u64>,
            min_tao_out: u64,
            min_escrow_alpha: u64,
            valid_until: BlockNumberFor<T>,
        ) -> DispatchResult {
            let now = frame_system::Pallet::<T>::block_number();
            ensure!(now <= valid_until, Error::<T>::DeadlinePassed);
            ensure!(
                collateral_alpha.is_some() ^ borrow_tao.is_some(),
                Error::<T>::ConflictingSizing
            );
            // Lending/subnet gates before amount or depth checks.
            Self::ensure_lending_live(netuid)?;
            Self::ensure_new_subnet_gate(netuid)?;
            let reference = Self::ensure_or_init_reference(netuid)?;
            let params = Self::lift_params();
            let price_before = T::Pool::current_alpha_price(netuid);

            // Accrue (and forfeit if exhausted) *before* sizing and escrow
            // snapshot so an omitted `sell_alpha` never subtracts a vanished
            // prior book, and LTV sizing sees the post-interest position.
            if let Some(mut position) = Positions::<T>::get(&owner, netuid) {
                Self::ensure_matching_side(&position, Side::Long)?;
                Self::charge_interest(&owner, netuid, &mut position, now, false)?;
                if Positions::<T>::contains_key(&owner, netuid) {
                    Positions::<T>::insert(&owner, netuid, &position);
                }
            }

            let (collateral, principal) = if let Some(c) = collateral_alpha {
                ensure!(c > 0, Error::<T>::CollateralZero);
                ensure!(c <= max_collateral_alpha, Error::<T>::CollateralExceedsMax);
                let p = Self::max_principal_for_collateral(
                    netuid,
                    Side::Long,
                    c,
                    Positions::<T>::get(&owner, netuid).as_ref(),
                    Some(&owner),
                    &reference,
                    &params,
                )?;
                // Same dust/capacity map as `do_open` / quotes before min_borrow.
                if p == 0 {
                    let price = reference.price.min(price_before);
                    return Err(Self::refuse_dust_or_capacity(
                        netuid,
                        Side::Long,
                        price,
                        &params,
                    ));
                }
                ensure!(p >= min_borrow_tao, Error::<T>::BelowMinimumBorrow);
                (c, p)
            } else {
                let p = borrow_tao.ok_or(Error::<T>::ConflictingSizing)?;
                ensure!(p > 0, Error::<T>::PrincipalZero);
                // Solve minimal collateral via bisection under max_collateral_alpha.
                let mut lo = 1u64;
                let mut hi = max_collateral_alpha;
                ensure!(hi > 0, Error::<T>::CollateralExceedsMax);
                let mut found = None;
                for _ in 0..BISECTION_STEPS {
                    if lo > hi {
                        break;
                    }
                    let mid = lo.saturating_add(hi.saturating_sub(lo) / 2);
                    let old = Positions::<T>::get(&owner, netuid);
                    let old_p = old.as_ref().map(|x| x.principal).unwrap_or(0);
                    let total_p = Self::add(old_p, p)?;
                    let total_c = Self::add(old.as_ref().map(|x| x.collateral).unwrap_or(0), mid)?;
                    let ltv = Self::ltv_tier_bps(netuid, Side::Long, &params);
                    let ok = Self::principal_feasible(
                        netuid,
                        Side::Long,
                        total_p,
                        total_c,
                        old_p,
                        Some(&owner),
                        &reference,
                        ltv,
                        &params,
                    )?;
                    if ok {
                        found = Some(mid);
                        hi = mid.saturating_sub(1);
                    } else {
                        lo = mid.saturating_add(1);
                    }
                }
                let c = found.ok_or(Error::<T>::DynamicCollateral)?;
                (c, p)
            };

            // Quote A for sell default and stake check.
            let y = T::Pool::pool_tao(netuid).to_u64();
            let x = T::Pool::pool_alpha(netuid).to_u64();
            let w = T::Pool::quote_weight_parts(netuid);
            let y_lift = y.saturating_sub(principal);
            let x_lift = x.saturating_sub(
                ((u128::from(x) * u128::from(principal)) / u128::from(y).max(1)) as u64,
            );
            let a_quote =
                T::Pool::quote_buy_on(y_lift.into(), x_lift.into(), w, principal.into()).to_u64();
            let s_needed = sell_alpha.unwrap_or(a_quote);
            let unlocked = T::Pool::unlocked_stake(&hotkey, &owner, netuid).to_u64();
            ensure!(
                unlocked >= collateral.saturating_add(s_needed),
                Error::<T>::InsufficientSellStake
            );

            // Post-interest escrow snapshot: increase → sale defaults to delta;
            // forfeited / fresh → entire new escrow (escrow_before is 0).
            let escrow_before = Positions::<T>::get(&owner, netuid)
                .map(|p| p.escrow_alpha)
                .unwrap_or(0);

            // Open escrowed long (locks C, lifts, forced buy).
            // `do_open` re-charges interest; elapsed is 0 after the pre-charge.
            Self::do_open(
                owner.clone(),
                netuid,
                Side::Long,
                collateral,
                Some(hotkey.clone()),
                Some(principal),
                principal,
                0,
                min_escrow_alpha,
                valid_until,
            )?;
            let position =
                Positions::<T>::get(&owner, netuid).ok_or(Error::<T>::PositionMissing)?;
            let realised_a = position.escrow_alpha.saturating_sub(escrow_before);
            ensure!(realised_a >= min_escrow_alpha, Error::<T>::SlippageExceeded);
            let s = sell_alpha.unwrap_or(realised_a);

            let out =
                T::Pool::unstake_for_raise(&hotkey, &owner, netuid, s.into(), min_tao_out.into())?;
            ensure!(out.to_u64() >= min_tao_out, Error::<T>::SlippageExceeded);
            let price_after = T::Pool::current_alpha_price(netuid);
            Self::deposit_event(Event::Raised {
                owner,
                hotkey,
                netuid,
                collateral_alpha: collateral,
                principal_tao: principal,
                escrow_alpha: realised_a,
                sell_alpha: s,
                free_tao_out: out.to_u64(),
                price_before,
                price_after,
            });
            let _ = a_quote;
            Ok(())
        }

        fn interest_due(
            position: &Position<T::AccountId, BlockNumberFor<T>>,
            until: BlockNumberFor<T>,
            final_charge: bool,
        ) -> Result<(u64, u64), DispatchError> {
            let elapsed: u64 = until.saturating_sub(position.last_accrued).saturated_into();
            let denominator = u128::from(T::BlocksPerYear::get());
            ensure!(denominator > 0, Error::<T>::Arithmetic);
            let numerator = u128::from(position.annual_interest)
                .saturating_mul(u128::from(elapsed))
                .saturating_add(u128::from(position.interest_remainder));
            let whole = numerator
                .checked_div(denominator)
                .ok_or(Error::<T>::Arithmetic)?;
            let remainder = numerator
                .checked_rem(denominator)
                .ok_or(Error::<T>::Arithmetic)?;
            let paid = if final_charge && remainder > 0 {
                whole.saturating_add(1)
            } else {
                whole
            };
            Ok((
                paid.min(u128::from(u64::MAX)) as u64,
                if final_charge { 0 } else { remainder as u64 },
            ))
        }

        fn charge_interest(
            owner: &T::AccountId,
            netuid: NetUid,
            position: &mut Position<T::AccountId, BlockNumberFor<T>>,
            until: BlockNumberFor<T>,
            final_charge: bool,
        ) -> DispatchResult {
            if Dissolutions::<T>::contains_key(netuid) {
                // Freeze at dissolution cutoff: do not accrue beyond frozen_at.
                if let Some(d) = Dissolutions::<T>::get(netuid) {
                    if until > d.frozen_at {
                        return Self::charge_interest(
                            owner,
                            netuid,
                            position,
                            d.frozen_at,
                            final_charge,
                        );
                    }
                }
            }
            let (due, remainder) = Self::interest_due(position, until, final_charge)?;
            let paid = due.min(position.collateral);
            if paid > 0 {
                let escrow = position.custody.clone();
                match position.side {
                    Side::Short => {
                        let burn_from = escrow.clone();
                        let credited =
                            T::Pool::collect_interest_tao(&escrow, &burn_from, paid.into())?
                                .to_u64();
                        // credited stays on escrow then burn
                        T::Pool::burn_interest_tao(&escrow, credited.into())?;
                        Self::deposit_event(Event::InterestBurned {
                            netuid,
                            side: Side::Short,
                            tao: credited,
                        });
                    }
                    Side::Long => {
                        let custody = Self::custody_hotkey()?;
                        // Live path: move interest alpha to the reserve for the
                        // paced sell schedule. Dissolution clears NetworksAdded
                        // before metered settle, so transfer_staked_alpha would
                        // fail with SubnetNotExists and roll the whole settle
                        // back — destroy interest alpha in place instead (§13).
                        if Dissolutions::<T>::contains_key(netuid) {
                            T::Pool::destroy_custody_alpha(&escrow, &custody, netuid, paid.into())?;
                        } else {
                            let reserve = Self::reserve_account(netuid);
                            T::Pool::transfer_staked_alpha(
                                &escrow,
                                &custody,
                                &reserve,
                                &custody,
                                netuid,
                                paid.into(),
                                false,
                                false,
                            )?;
                            LongInterestPending::<T>::mutate(netuid, |pending| {
                                *pending = pending.saturating_add(paid);
                            });
                        }
                        Lifts::<T>::mutate(netuid, |book| {
                            book.long_collateral_alpha =
                                book.long_collateral_alpha.saturating_sub(paid);
                        });
                    }
                }
                position.collateral = Self::sub(position.collateral, paid)?;
                Self::deposit_event(Event::InterestCollected {
                    owner: owner.clone(),
                    netuid,
                    collateral_paid: paid,
                });
            }
            position.last_accrued = until;
            position.interest_remainder = remainder;
            if position.collateral == 0 && due > 0 {
                // Dissolution already re-added lift legs; never forfeit there.
                // Live-market path still defaults when collateral is exhausted.
                if !Dissolutions::<T>::contains_key(netuid) {
                    Self::forfeit(owner, netuid, position)?;
                }
            }
            Ok(())
        }

        fn forfeit(
            owner: &T::AccountId,
            netuid: NetUid,
            position: &Position<T::AccountId, BlockNumberFor<T>>,
        ) -> DispatchResult {
            let custody = Self::custody_hotkey()?;
            let reserve = Self::reserve_account(netuid);
            let escrow = position.custody.clone();
            match position.side {
                Side::Short => {
                    // Default: readd(τ_L + E_S, 0)
                    if position.proceeds > 0 {
                        T::Pool::transfer_tao(&escrow, &reserve, position.proceeds.into())?;
                    }
                    T::Pool::readd_and_credit(
                        netuid,
                        &reserve,
                        &custody,
                        position.lift_tao.saturating_add(position.proceeds).into(),
                        0u64.into(),
                    )?;
                    Lifts::<T>::mutate(netuid, |book| {
                        book.l_s = book.l_s.saturating_sub(position.lift_tao);
                        book.held_tao = book.held_tao.saturating_sub(position.lift_tao);
                        book.escrow_tao = book.escrow_tao.saturating_sub(position.proceeds);
                    });
                }
                Side::Long => {
                    // Default: readd(0, α_L + A)
                    let alpha = position.lift_alpha.saturating_add(position.escrow_alpha);
                    if position.escrow_alpha > 0 {
                        T::Pool::transfer_staked_alpha(
                            &escrow,
                            &custody,
                            &reserve,
                            &custody,
                            netuid,
                            position.escrow_alpha.into(),
                            false,
                            false,
                        )?;
                    }
                    T::Pool::readd_and_credit(
                        netuid,
                        &reserve,
                        &custody,
                        0u64.into(),
                        alpha.into(),
                    )?;
                    Lifts::<T>::mutate(netuid, |book| {
                        book.l_l = book.l_l.saturating_sub(position.principal);
                        book.b_l = book.b_l.saturating_sub(position.principal);
                        book.held_alpha = book.held_alpha.saturating_sub(position.lift_alpha);
                        book.escrow_alpha = book.escrow_alpha.saturating_sub(position.escrow_alpha);
                        book.long_collateral_alpha = book
                            .long_collateral_alpha
                            .saturating_sub(position.collateral);
                    });
                }
            }
            let principal_lost = position.principal;
            let side = position.side;
            Self::remove_position(owner, netuid, position);
            Self::deposit_event(Event::Forfeited {
                owner: owner.clone(),
                netuid,
                side,
                principal_lost,
            });
            Ok(())
        }

        fn remove_position(
            owner: &T::AccountId,
            netuid: NetUid,
            position: &Position<T::AccountId, BlockNumberFor<T>>,
        ) {
            Positions::<T>::remove(owner, netuid);
            OpenByNetuid::<T>::remove(netuid, owner);
            EscrowOwner::<T>::remove(netuid, &position.custody);
            if Due::<T>::take(position.due, (owner, netuid)).is_some() {
                Self::note_due_removed(position.due);
            }
            if Expiry::<T>::take(position.expiry_due, (owner, netuid)).is_some() {
                Self::note_expiry_removed(position.expiry_due);
            }
            PositionCount::<T>::mutate(netuid, |count| *count = count.saturating_sub(1));
            let remaining = TotalPositions::<T>::mutate(|count| {
                *count = count.saturating_sub(1);
                *count
            });
            if remaining == 0 {
                NextDue::<T>::kill();
                DueBucketCursor::<T>::kill();
                NextExpiry::<T>::kill();
                ExpiryBucketCursor::<T>::kill();
            }
        }

        fn unlink_due_bucket(due: BlockNumberFor<T>) {
            let prev = DuePrev::<T>::take(due);
            let next = DueNext::<T>::take(due);
            match (prev, next) {
                (None, None) => {
                    if NextDue::<T>::get() == Some(due) {
                        NextDue::<T>::kill();
                    }
                    if DueTail::<T>::get() == Some(due) {
                        DueTail::<T>::kill();
                    }
                }
                (None, Some(n)) => {
                    DuePrev::<T>::remove(n);
                    if NextDue::<T>::get() == Some(due) {
                        NextDue::<T>::put(n);
                    }
                }
                (Some(p), None) => {
                    DueNext::<T>::remove(p);
                    if DueTail::<T>::get() == Some(due) {
                        DueTail::<T>::put(p);
                    }
                }
                (Some(p), Some(n)) => {
                    DueNext::<T>::insert(p, n);
                    DuePrev::<T>::insert(n, p);
                }
            }
        }

        fn note_due_removed(due: BlockNumberFor<T>) {
            let remaining = DueIndex::<T>::mutate(due, |count| {
                *count = count.saturating_sub(1);
                *count
            });
            if remaining == 0 {
                DueIndex::<T>::remove(due);
                Self::unlink_due_bucket(due);
            }
        }

        fn note_due_inserted(due: BlockNumberFor<T>) {
            let was_empty = DueIndex::<T>::get(due) == 0;
            DueIndex::<T>::mutate(due, |count| *count = count.saturating_add(1));
            if !was_empty {
                return;
            }
            // Fast paths: empty / prepend / append. Interest reschedule and new
            // opens almost always append (`now + InterestPeriod`), so avoid the
            // linear mid-list walk on the hot path.
            match (NextDue::<T>::get(), DueTail::<T>::get()) {
                (None, _) => {
                    NextDue::<T>::put(due);
                    DueTail::<T>::put(due);
                }
                (Some(head), _) if due < head => {
                    DueNext::<T>::insert(due, head);
                    DuePrev::<T>::insert(head, due);
                    NextDue::<T>::put(due);
                }
                (_, Some(tail)) if due > tail => {
                    DueNext::<T>::insert(tail, due);
                    DuePrev::<T>::insert(due, tail);
                    DueTail::<T>::put(due);
                }
                (Some(head), _) => {
                    // Rare out-of-order insert (e.g. clock skew / period change).
                    let mut prev = head;
                    loop {
                        match DueNext::<T>::get(prev) {
                            Some(n) if n < due => prev = n,
                            Some(n) => {
                                DueNext::<T>::insert(prev, due);
                                DuePrev::<T>::insert(due, prev);
                                DueNext::<T>::insert(due, n);
                                DuePrev::<T>::insert(n, due);
                                break;
                            }
                            None => {
                                DueNext::<T>::insert(prev, due);
                                DuePrev::<T>::insert(due, prev);
                                DueTail::<T>::put(due);
                                break;
                            }
                        }
                    }
                }
            }
        }

        fn unlink_expiry_bucket(at: BlockNumberFor<T>) {
            let prev = ExpiryPrev::<T>::take(at);
            let next = ExpiryNext::<T>::take(at);
            match (prev, next) {
                (None, None) => {
                    if NextExpiry::<T>::get() == Some(at) {
                        NextExpiry::<T>::kill();
                    }
                    if ExpiryTail::<T>::get() == Some(at) {
                        ExpiryTail::<T>::kill();
                    }
                }
                (None, Some(n)) => {
                    ExpiryPrev::<T>::remove(n);
                    if NextExpiry::<T>::get() == Some(at) {
                        NextExpiry::<T>::put(n);
                    }
                }
                (Some(p), None) => {
                    ExpiryNext::<T>::remove(p);
                    if ExpiryTail::<T>::get() == Some(at) {
                        ExpiryTail::<T>::put(p);
                    }
                }
                (Some(p), Some(n)) => {
                    ExpiryNext::<T>::insert(p, n);
                    ExpiryPrev::<T>::insert(n, p);
                }
            }
        }

        fn note_expiry_removed(at: BlockNumberFor<T>) {
            let remaining = ExpiryIndex::<T>::mutate(at, |count| {
                *count = count.saturating_sub(1);
                *count
            });
            if remaining == 0 {
                ExpiryIndex::<T>::remove(at);
                Self::unlink_expiry_bucket(at);
            }
        }

        fn note_expiry_inserted(at: BlockNumberFor<T>) {
            let was_empty = ExpiryIndex::<T>::get(at) == 0;
            ExpiryIndex::<T>::mutate(at, |count| *count = count.saturating_add(1));
            if !was_empty {
                return;
            }
            match (NextExpiry::<T>::get(), ExpiryTail::<T>::get()) {
                (None, _) => {
                    NextExpiry::<T>::put(at);
                    ExpiryTail::<T>::put(at);
                }
                (Some(head), _) if at < head => {
                    ExpiryNext::<T>::insert(at, head);
                    ExpiryPrev::<T>::insert(head, at);
                    NextExpiry::<T>::put(at);
                }
                (_, Some(tail)) if at > tail => {
                    ExpiryNext::<T>::insert(tail, at);
                    ExpiryPrev::<T>::insert(at, tail);
                    ExpiryTail::<T>::put(at);
                }
                (Some(head), _) => {
                    let mut prev = head;
                    loop {
                        match ExpiryNext::<T>::get(prev) {
                            Some(n) if n < at => prev = n,
                            Some(n) => {
                                ExpiryNext::<T>::insert(prev, at);
                                ExpiryPrev::<T>::insert(at, prev);
                                ExpiryNext::<T>::insert(at, n);
                                ExpiryPrev::<T>::insert(n, at);
                                break;
                            }
                            None => {
                                ExpiryNext::<T>::insert(prev, at);
                                ExpiryPrev::<T>::insert(at, prev);
                                ExpiryTail::<T>::put(at);
                                break;
                            }
                        }
                    }
                }
            }
        }

        fn note_active_net(netuid: NetUid) {
            if !ActiveNets::<T>::contains_key(netuid) {
                ActiveNets::<T>::insert(netuid, ());
                ActiveNetCount::<T>::mutate(|n| *n = n.saturating_add(1));
            }
        }

        fn forget_active_net(netuid: NetUid) {
            if ActiveNets::<T>::take(netuid).is_some() {
                ActiveNetCount::<T>::mutate(|n| *n = n.saturating_sub(1));
            }
        }

        fn collect_due(now: BlockNumberFor<T>, meter: &mut WeightMeter) {
            use frame_support::storage::{TransactionOutcome, with_transaction};
            if meter
                .try_consume(T::DbWeight::get().reads_writes(2, 1))
                .is_err()
            {
                return;
            }
            if TotalPositions::<T>::get() == 0 {
                NextDue::<T>::kill();
                DueBucketCursor::<T>::kill();
                NextExpiry::<T>::kill();
                ExpiryBucketCursor::<T>::kill();
                return;
            }
            let Some(mut due) = NextDue::<T>::get() else {
                return;
            };
            while due <= now {
                // Charge before touching the bucket. Resume via DueBucketCursor.
                if meter.try_consume(T::DbWeight::get().reads(1)).is_err() {
                    NextDue::<T>::put(due);
                    return;
                }
                let iter = if let Some(cursor) = DueBucketCursor::<T>::get() {
                    Due::<T>::iter_prefix_from(due, Due::<T>::hashed_key_for(due, cursor))
                } else {
                    Due::<T>::iter_prefix(due)
                };
                let mut saw_entry = false;
                // Cursor stores the last successfully processed key so
                // `iter_prefix_from` resumes at the next unprocessed entry.
                let mut last_done: Option<(T::AccountId, NetUid)> = None;
                for ((owner, netuid), ()) in iter {
                    saw_entry = true;
                    // Meter each position before charging interest.
                    if meter.try_consume(T::WeightInfo::collect()).is_err() {
                        if let Some(done) = last_done {
                            DueBucketCursor::<T>::put(done);
                        }
                        // else: keep the prior exclusive cursor (or none) so the
                        // unprocessed entry is retried, not skipped.
                        NextDue::<T>::put(due);
                        return;
                    }
                    let _ = with_transaction(|| {
                        let result = (|| {
                            let mut position = Positions::<T>::get(&owner, netuid)
                                .ok_or(Error::<T>::PositionMissing)?;
                            if Dissolutions::<T>::contains_key(netuid) {
                                return Ok(());
                            }
                            Self::charge_interest(&owner, netuid, &mut position, now, false)?;
                            if Positions::<T>::contains_key(&owner, netuid) {
                                let next = now.saturating_add(T::InterestPeriod::get());
                                if Due::<T>::take(position.due, (&owner, netuid)).is_some() {
                                    Self::note_due_removed(position.due);
                                }
                                position.due = next;
                                Due::<T>::insert(next, (&owner, netuid), ());
                                Self::note_due_inserted(next);
                                Positions::<T>::insert(&owner, netuid, position);
                            }
                            Ok(())
                        })();
                        match result {
                            Ok(()) => TransactionOutcome::Commit(Ok::<(), DispatchError>(())),
                            Err(e) => TransactionOutcome::Rollback(Err(e)),
                        }
                    });
                    // Drop the processed due entry if charge_interest forfeited
                    // (remove_position already cleared it) or left it for reschedule.
                    if Due::<T>::contains_key(due, (&owner, netuid)) {
                        Due::<T>::remove(due, (&owner, netuid));
                        Self::note_due_removed(due);
                    }
                    last_done = Some((owner, netuid));
                }
                DueBucketCursor::<T>::kill();
                if !saw_entry {
                    // Stale NextDue pointing at an empty bucket.
                    DueIndex::<T>::remove(due);
                    Self::unlink_due_bucket(due);
                }
                // `note_due_removed` / unlink already advanced NextDue off this bucket.
                let Some(n) = NextDue::<T>::get() else {
                    break;
                };
                if n == due {
                    // Defensive: still parked on a drained bucket.
                    Self::unlink_due_bucket(due);
                    let Some(n) = NextDue::<T>::get() else {
                        break;
                    };
                    due = n;
                } else {
                    due = n;
                }
            }
        }

        /// Auto-settle positions whose fixed one-year term has ended (§15).
        /// Uses the smoothed reference price (same math as dissolution settle).
        /// Dissolution wins if the subnet is already dissolving — skip those.
        pub(crate) fn collect_expiry(now: BlockNumberFor<T>, meter: &mut WeightMeter) {
            use frame_support::storage::{TransactionOutcome, with_transaction};
            if meter
                .try_consume(T::DbWeight::get().reads_writes(2, 1))
                .is_err()
            {
                return;
            }
            if TotalPositions::<T>::get() == 0 {
                NextExpiry::<T>::kill();
                ExpiryBucketCursor::<T>::kill();
                return;
            }
            let Some(mut at) = NextExpiry::<T>::get() else {
                return;
            };
            while at <= now {
                if meter.try_consume(T::DbWeight::get().reads(1)).is_err() {
                    NextExpiry::<T>::put(at);
                    return;
                }
                let iter = if let Some(cursor) = ExpiryBucketCursor::<T>::get() {
                    Expiry::<T>::iter_prefix_from(at, Expiry::<T>::hashed_key_for(at, cursor))
                } else {
                    Expiry::<T>::iter_prefix(at)
                };
                let mut saw_entry = false;
                let mut last_done: Option<(T::AccountId, NetUid)> = None;
                for ((owner, netuid), ()) in iter {
                    saw_entry = true;
                    if meter.try_consume(T::WeightInfo::settle()).is_err() {
                        if let Some(done) = last_done {
                            ExpiryBucketCursor::<T>::put(done);
                        }
                        NextExpiry::<T>::put(at);
                        return;
                    }
                    let outcome = with_transaction(|| {
                        let result = (|| {
                            // Dissolution settle is authoritative if it started first.
                            if Dissolutions::<T>::contains_key(netuid) {
                                return Ok(());
                            }
                            let position = Positions::<T>::get(&owner, netuid)
                                .ok_or(Error::<T>::PositionMissing)?;
                            // Stale queue entry (rekeyed / already closed).
                            if position.expires_at > now {
                                return Ok(());
                            }
                            let rate = References::<T>::get(netuid)
                                .map(|r| r.price)
                                .unwrap_or_else(|| T::Pool::current_alpha_price(netuid));
                            Self::settle_position_at_rate(
                                netuid,
                                &owner,
                                position,
                                rate,
                                now,
                                SettleKind::Expiry,
                            )
                        })();
                        match result {
                            Ok(()) => TransactionOutcome::Commit(Ok::<(), DispatchError>(())),
                            Err(e) => TransactionOutcome::Rollback(Err(e)),
                        }
                    });
                    match outcome {
                        Ok(()) => {
                            // Stale / dissolving skips leave the queue row; drop it.
                            // Successful settle already cleared it via remove_position.
                            if Expiry::<T>::contains_key(at, (&owner, netuid)) {
                                Expiry::<T>::remove(at, (&owner, netuid));
                                Self::note_expiry_removed(at);
                            }
                        }
                        Err(_) => {
                            // Preserve coverage, but do not pin NextExpiry on this
                            // bucket: a persistently failing loan would otherwise
                            // starve later expiries on every subnet. Drop orphan
                            // claims; defer live ones onto a bounded retry slot and
                            // record `expiry_due` so coldkey re-key finds the claim.
                            if Expiry::<T>::contains_key(at, (&owner, netuid)) {
                                Expiry::<T>::remove(at, (&owner, netuid));
                                Self::note_expiry_removed(at);
                            }
                            if let Some(mut position) = Positions::<T>::get(&owner, netuid) {
                                let retry_at = now.saturating_add(T::ExpiryRetryDelay::get());
                                if !Expiry::<T>::contains_key(retry_at, (&owner, netuid)) {
                                    Expiry::<T>::insert(retry_at, (&owner, netuid), ());
                                    Self::note_expiry_inserted(retry_at);
                                }
                                position.expiry_due = retry_at;
                                Positions::<T>::insert(&owner, netuid, position);
                            }
                        }
                    }
                    last_done = Some((owner, netuid));
                }
                ExpiryBucketCursor::<T>::kill();
                if !saw_entry {
                    ExpiryIndex::<T>::remove(at);
                    Self::unlink_expiry_bucket(at);
                }
                let Some(n) = NextExpiry::<T>::get() else {
                    break;
                };
                if n == at {
                    Self::unlink_expiry_bucket(at);
                    let Some(n) = NextExpiry::<T>::get() else {
                        break;
                    };
                    at = n;
                } else {
                    at = n;
                }
            }
        }

        fn convert_pending(now: BlockNumberFor<T>, meter: &mut WeightMeter) {
            let params = Self::lift_params();
            // Bounded, resumable walk — never materialise the full map first.
            // Cursor is exclusive (`iter_from`); store the last fully handled key.
            let iter = if let Some(cursor) = ConversionCursor::<T>::get() {
                LongInterestPending::<T>::iter_from(LongInterestPending::<T>::hashed_key_for(
                    cursor,
                ))
            } else {
                LongInterestPending::<T>::iter()
            };
            let mut last_done: Option<NetUid> = None;
            for (netuid, pending) in iter {
                // Charge the storage read before any conversion work.
                if meter.try_consume(T::DbWeight::get().reads(1)).is_err() {
                    if let Some(done) = last_done {
                        ConversionCursor::<T>::put(done);
                    }
                    // else: keep the prior exclusive cursor so this key is retried.
                    return;
                }
                if pending == 0 || Dissolutions::<T>::contains_key(netuid) {
                    last_done = Some(netuid);
                    continue;
                }
                if meter.try_consume(T::WeightInfo::collect()).is_err() {
                    if let Some(done) = last_done {
                        ConversionCursor::<T>::put(done);
                    }
                    return;
                }
                let spot = T::Pool::current_alpha_price(netuid);
                let reference = References::<T>::get(netuid);
                // Crash guard
                if let Some(reference) = reference {
                    let floor = reference
                        .price
                        .saturating_mul(U64F64::from_num(params.crash_guard_bps))
                        / U64F64::from_num(10_000u16);
                    let backlog_weeks =
                        4u64.saturating_mul(T::InterestPeriod::get().saturated_into::<u64>());
                    let force = pending
                        > (u128::from(pending) // placeholder: force if pending aged — use book timestamp
                            .min(u128::from(u64::MAX))) as u64
                        && Lifts::<T>::get(netuid).last + backlog_weeks
                            < now.saturated_into::<u64>();
                    if spot < floor && !force {
                        last_done = Some(netuid);
                        continue;
                    }
                }
                let x = T::Pool::pool_alpha(netuid).to_u64();
                // Cap slice by slice_bps spot move: Δx_max ≈ x * ((1/(1-ε))^wq - 1); approximate ε*x for small.
                // Take the full pending backlog up to that cap — do not divide by
                // InterestPeriod (that stranded coupons worth ≫ 0.01 τ forever).
                let max_slice = Self::mul_bps(x, params.slice_bps).unwrap_or(0).max(1);
                let slice = pending.min(max_slice).max(1);
                // Reference-derived floor: EMA value × crash_guard_bps (default 95%).
                // Defer (do not revert idle) when the live quote misses the floor —
                // crash guard alone does not protect against an adverse fill.
                let min_out = if let Some(reference) = References::<T>::get(netuid) {
                    match Self::tao_for_alpha(slice, reference.price, false)
                        .and_then(|fair| Self::mul_bps(fair, params.crash_guard_bps))
                    {
                        Ok(v) => v,
                        Err(_) => {
                            last_done = Some(netuid);
                            continue;
                        }
                    }
                } else {
                    last_done = Some(netuid);
                    continue;
                };
                // Skip dust below ~0.01 τ or when quote misses the EMA floor.
                match T::Pool::quote_sell(netuid, slice.into()) {
                    Ok(tao_est)
                        if tao_est.to_u64() >= 10_000_000 && tao_est.to_u64() >= min_out => {}
                    _ => {
                        last_done = Some(netuid);
                        continue;
                    }
                }
                let custody = match Self::custody_hotkey() {
                    Ok(c) => c,
                    Err(_) => {
                        last_done = Some(netuid);
                        continue;
                    }
                };
                let reserve = Self::reserve_account(netuid);
                let sold = match T::Pool::forced_sell_into_escrow(
                    &reserve,
                    &custody,
                    &reserve,
                    netuid,
                    slice.into(),
                    min_out.into(),
                ) {
                    Ok(t) => t,
                    Err(_) => {
                        last_done = Some(netuid);
                        continue;
                    }
                };
                let _ = T::Pool::burn_interest_tao(&reserve, sold);
                LongInterestPending::<T>::mutate(netuid, |p| *p = p.saturating_sub(slice));
                Self::deposit_event(Event::LongInterestSold {
                    netuid,
                    alpha: slice,
                    tao: sold.to_u64(),
                });
                Self::deposit_event(Event::InterestBurned {
                    netuid,
                    side: Side::Long,
                    tao: sold.to_u64(),
                });
                ConversionCursor::<T>::put(netuid);
                return;
            }
            ConversionCursor::<T>::kill();
        }

        pub fn start_dissolution(netuid: NetUid) -> DispatchResult {
            ensure!(
                !Dissolutions::<T>::contains_key(netuid),
                Error::<T>::AlreadyDissolving
            );
            // Snapshot settlement basis and EMA at flag (do not update EMA here).
            let t_flag = Self::settlement_tao_basis(netuid);
            let t_ema_flag = Self::ensure_tao_ema(netuid);
            // No-op if lending never touched this subnet.
            if !ActiveNets::<T>::contains_key(netuid)
                && PositionCount::<T>::get(netuid) == 0
                && LongInterestPending::<T>::get(netuid) == 0
            {
                Dissolutions::<T>::insert(
                    netuid,
                    Dissolution {
                        frozen_at: frame_system::Pallet::<T>::block_number(),
                        lifts_readded: true,
                        rate: U64F64::from_num(0),
                        t_flag: 0,
                        t_ema_flag: 0,
                    },
                );
                return Ok(());
            }
            let frozen_at = frame_system::Pallet::<T>::block_number();
            Dissolutions::<T>::insert(
                netuid,
                Dissolution {
                    frozen_at,
                    lifts_readded: false,
                    rate: U64F64::from_num(0),
                    t_flag,
                    t_ema_flag,
                },
            );
            DissolutionPhase::<T>::insert(netuid, 0u8);
            Self::deposit_event(Event::DissolutionFrozen { netuid, frozen_at });
            Ok(())
        }

        /// §13: re-add held lift legs. Called from `do_dissolve_network` before cleanup.
        pub fn readd_lifts_for_dissolution(netuid: NetUid) -> DispatchResult {
            let book = Lifts::<T>::get(netuid);
            if book.held_tao == 0 && book.held_alpha == 0 {
                if let Some(mut d) = Dissolutions::<T>::get(netuid) {
                    d.lifts_readded = true;
                    Dissolutions::<T>::insert(netuid, d);
                }
                return Ok(());
            }
            let custody = Self::custody_hotkey()?;
            let reserve = Self::reserve_account(netuid);
            T::Pool::readd_and_credit(
                netuid,
                &reserve,
                &custody,
                book.held_tao.into(),
                book.held_alpha.into(),
            )?;
            Self::deposit_event(Event::Readded {
                netuid,
                tao: book.held_tao,
                alpha: book.held_alpha,
            });
            Lifts::<T>::mutate(netuid, |b| {
                b.held_tao = 0;
                b.held_alpha = 0;
            });
            if let Some(mut d) = Dissolutions::<T>::get(netuid) {
                d.lifts_readded = true;
                Dissolutions::<T>::insert(netuid, d);
            }
            Ok(())
        }

        /// Whether this (hotkey, coldkey) is lending custody that main must skip.
        pub fn is_lending_custody(
            hotkey: &T::AccountId,
            coldkey: &T::AccountId,
            netuid: NetUid,
        ) -> bool {
            let Ok(custody_hot) = Self::custody_hotkey() else {
                return false;
            };
            if hotkey != &custody_hot {
                return false;
            }
            // Reserve account holds held legs (cleared on readd); position escrows hold C/A.
            if *coldkey == Self::reserve_account(netuid) {
                return true;
            }
            EscrowOwner::<T>::contains_key(netuid, coldkey)
        }

        /// Jacob §13.2.2 settlement (weight-metered, multi-block OK).
        /// Assumes lift legs were already re-added in the dissolve call.
        /// Phase 0: freeze rate after phase-0 basket conversions
        ///   `Δ0 = max(0, T_flag − T_pre)`,
        ///   `T_settle = min(T_flag, t_ema_flag) − Δ0`,
        ///   `r = T_settle / ALL_alpha`, then `r = min(r, moving_alpha_price)`.
        /// Phase 1: settle positions.
        /// Phase 2: convert pending long interest at r and burn.
        pub fn settle_lending(netuid: NetUid, meter: &mut WeightMeter) -> bool {
            let Some(mut dissolution) = Dissolutions::<T>::get(netuid) else {
                return true;
            };
            let phase = DissolutionPhase::<T>::get(netuid);
            if phase == 0 {
                if meter.try_consume(T::WeightInfo::settle()).is_err() {
                    return false;
                }
                // §13.2.2: read live pot after phase-0 basket → root sales.
                let t_pre = Self::settlement_tao_basis(netuid);
                let delta0 = dissolution.t_flag.saturating_sub(t_pre);
                let t_settle = dissolution
                    .t_flag
                    .min(dissolution.t_ema_flag)
                    .saturating_sub(delta0);
                let all_alpha = T::Pool::all_alpha_for_lending_settlement(netuid);
                let mut r = if all_alpha == 0 {
                    U64F64::from_num(0)
                } else {
                    // r = T_settle / all_alpha in U64F64; use u128 division on Q64.64 bits.
                    let bits = (u128::from(t_settle) << 64) / all_alpha;
                    U64F64::from_bits(bits)
                };
                let moving_price = T::Pool::moving_alpha_price(netuid);
                if moving_price > U64F64::from_num(0) {
                    r = r.min(moving_price);
                }
                dissolution.rate = r;
                dissolution.lifts_readded = true;
                Dissolutions::<T>::insert(netuid, dissolution.clone());
                DissolutionPhase::<T>::insert(netuid, 1u8);
                DissolutionCursor::<T>::remove(netuid);
                Self::deposit_event(Event::SettleRateFixed {
                    netuid,
                    t_flag: dissolution.t_flag,
                    t_ema_flag: dissolution.t_ema_flag,
                    t_pre,
                    delta0,
                    d: all_alpha,
                    r,
                });
            }

            // Phase 1: settle positions
            let dissolution = match Dissolutions::<T>::get(netuid) {
                Some(d) => d,
                None => return true,
            };
            let positions = if let Some(cursor) = DissolutionCursor::<T>::get(netuid) {
                OpenByNetuid::<T>::iter_prefix_from(
                    netuid,
                    OpenByNetuid::<T>::hashed_key_for(netuid, cursor),
                )
            } else {
                OpenByNetuid::<T>::iter_prefix(netuid)
            };
            for (owner, ()) in positions {
                if meter.try_consume(T::WeightInfo::settle()).is_err() {
                    return false;
                }
                let Some(position) = Positions::<T>::get(&owner, netuid) else {
                    continue;
                };
                if Self::settle_position_at_rate(
                    netuid,
                    &owner,
                    position,
                    dissolution.rate,
                    dissolution.frozen_at,
                    SettleKind::Dissolution,
                )
                .is_err()
                {
                    return false;
                }
                DissolutionCursor::<T>::insert(netuid, owner);
            }
            DissolutionCursor::<T>::remove(netuid);

            // Phase 2: pending long interest at r → burn TAO
            if meter.try_consume(T::WeightInfo::settle()).is_err() {
                DissolutionPhase::<T>::insert(netuid, 2u8);
                return false;
            }
            let pending = LongInterestPending::<T>::get(netuid);
            if pending > 0 {
                let tao = Self::tao_for_alpha(pending, dissolution.rate, false).unwrap_or(0);
                LongInterestPending::<T>::remove(netuid);
                if tao > 0 {
                    // §13: pending long-interest alpha converts at r; burn the TAO from the pot.
                    let _ = T::Pool::burn_from_pot(netuid, tao.into());
                    Self::deposit_event(Event::InterestBurned {
                        netuid,
                        side: Side::Long,
                        tao,
                    });
                }
            }
            DissolutionPhase::<T>::insert(netuid, 3u8);
            true
        }

        /// Atomic per-position settlement at a smoothed rate. Must not leave a
        /// half-paid escrow: short recovery+refund are withdrawn together via
        /// `terminal_split_tao` (Expendable transfers would otherwise reap a
        /// below-ED refund remainder). Used by dissolution (§13) and fixed-term
        /// expiry (§15); no AMM liquidations.
        #[transactional]
        fn settle_position_at_rate(
            netuid: NetUid,
            owner: &T::AccountId,
            mut position: Position<T::AccountId, BlockNumberFor<T>>,
            r: U64F64,
            interest_until: BlockNumberFor<T>,
            kind: SettleKind,
        ) -> DispatchResult {
            // Accrue and burn interest through the cutoff before settlement.
            Self::charge_interest(owner, netuid, &mut position, interest_until, true)?;
            // Live-path interest can exhaust collateral and `forfeit`, which already
            // restored lifts, moved escrow, and removed the claim. Do not reinsert
            // or continue settlement on a forfeited loan (would double-recover and
            // fail, then defer forever). Dissolution skips forfeit above.
            if !Positions::<T>::contains_key(owner, netuid) {
                return Ok(());
            }
            Positions::<T>::insert(owner, netuid, position.clone());

            let escrow = position.custody.clone();
            let reserve = Self::reserve_account(netuid);
            let custody = Self::custody_hotkey()?;

            match position.side {
                Side::Short => {
                    // Pay min(P*r, E+C) into the pot/pool; borrower gets E+C−P·r
                    // (interest already burned from collateral above).
                    let debt = Self::tao_for_alpha(position.principal, r, true)?;
                    let available = Self::add(position.proceeds, position.collateral)?;
                    let to_pot = debt.min(available);
                    let refund = available.saturating_sub(to_pot);
                    // Expiry (unlike dissolution) has not restored held lift legs yet.
                    if matches!(kind, SettleKind::Expiry) && position.lift_tao > 0 {
                        T::Pool::readd_and_credit(
                            netuid,
                            &reserve,
                            &custody,
                            position.lift_tao.into(),
                            0u64.into(),
                        )?;
                        Lifts::<T>::mutate(netuid, |book| {
                            book.held_tao = book.held_tao.saturating_sub(position.lift_tao);
                        });
                    }
                    // Withdraw recovery + refund in one credit split so a below-ED
                    // refund cannot be reaped by an intervening Expendable transfer.
                    let (recovered, refunded) = T::Pool::terminal_split_tao(
                        &escrow,
                        &reserve,
                        owner,
                        to_pot.into(),
                        refund.into(),
                    )?;
                    let recovered = recovered.to_u64();
                    let refunded = refunded.to_u64();
                    if recovered > 0 {
                        T::Pool::readd_and_credit(
                            netuid,
                            &reserve,
                            &custody,
                            recovered.into(),
                            0u64.into(),
                        )?;
                    }
                    if refunded < refund {
                        Self::deposit_event(Event::DustForfeited {
                            netuid,
                            recipient: owner.clone(),
                            tao: refund.saturating_sub(refunded),
                        });
                    }
                    Lifts::<T>::mutate(netuid, |book| {
                        book.l_s = book.l_s.saturating_sub(position.lift_tao);
                        book.escrow_tao = book.escrow_tao.saturating_sub(position.proceeds);
                    });
                    Self::remove_position(owner, netuid, &position);
                    match kind {
                        SettleKind::Dissolution => {
                            Self::deposit_event(Event::DissolutionSettled {
                                owner: owner.clone(),
                                netuid,
                                side: Side::Short,
                                principal_tao_equiv: debt,
                                tao_to_pot: to_pot,
                                tao_refund: refund,
                            });
                            Self::deposit_event(Event::Settled {
                                owner: owner.clone(),
                                netuid,
                                side: Side::Short,
                                principal_tao_equiv: debt,
                                tao_to_pot: to_pot,
                                tao_refund: refund,
                            });
                        }
                        SettleKind::Expiry => {
                            Self::deposit_event(Event::Expired {
                                owner: owner.clone(),
                                netuid,
                                side: Side::Short,
                                principal_tao_equiv: debt,
                                tao_to_pool: to_pot,
                                tao_refund: refund,
                                rate: r,
                            });
                        }
                    }
                }
                Side::Long => {
                    // Escrow A settles at EMA (quantity-backed). Collateral C is
                    // capped at an impact-aware AMM sale so near-expiry top-ups
                    // cannot redeem above dumping C into the live pool.
                    let a_val = Self::tao_for_alpha(position.escrow_alpha, r, false)?;
                    let c = position.collateral;
                    let c_ema = Self::tao_for_alpha(c, r, false)?;
                    let c_amm = if c > 0 {
                        match T::Pool::quote_sell(netuid, c.into()) {
                            Ok(t) => t.to_u64(),
                            Err(_) => {
                                let y = T::Pool::pool_tao(netuid).to_u64();
                                let x = T::Pool::pool_alpha(netuid).to_u64();
                                let w = T::Pool::quote_weight_parts(netuid);
                                T::Pool::quote_sell_on(y.into(), x.into(), w, c.into()).to_u64()
                            }
                        }
                    } else {
                        0
                    };
                    let c_val = c_ema.min(c_amm);
                    let alpha = Self::add(position.escrow_alpha, c)?;
                    let payout = Self::add(a_val, c_val)?;
                    let repay = payout.min(position.principal);
                    let remainder = payout.saturating_sub(repay);
                    let shortfall = position.principal.saturating_sub(repay);
                    // Expiry must restore held lift alpha; dissolution already did.
                    if matches!(kind, SettleKind::Expiry) && position.lift_alpha > 0 {
                        T::Pool::readd_and_credit(
                            netuid,
                            &reserve,
                            &custody,
                            0u64.into(),
                            position.lift_alpha.into(),
                        )?;
                        Lifts::<T>::mutate(netuid, |book| {
                            book.held_alpha = book.held_alpha.saturating_sub(position.lift_alpha);
                        });
                    }
                    // Pay remainder before destroying custody alpha. A pot miss
                    // must revert the whole transactional settle so collateral is
                    // not destroyed; collect_expiry then defers the claim onto a
                    // retry slot instead of pinning NextExpiry. Below-ED dust only.
                    let mut paid_remainder = 0u64;
                    if remainder > 0 {
                        let paid = T::Pool::pay_from_pot(netuid, owner, remainder.into())?;
                        paid_remainder = paid.to_u64();
                        if paid_remainder < remainder {
                            Self::deposit_event(Event::DustForfeited {
                                netuid,
                                recipient: owner.clone(),
                                tao: remainder.saturating_sub(paid_remainder),
                            });
                        }
                    }
                    if alpha > 0 {
                        T::Pool::destroy_custody_alpha(&escrow, &custody, netuid, alpha.into())?;
                    }
                    Lifts::<T>::mutate(netuid, |book| {
                        book.l_l = book.l_l.saturating_sub(position.principal);
                        book.b_l = book.b_l.saturating_sub(position.principal);
                        book.escrow_alpha = book.escrow_alpha.saturating_sub(position.escrow_alpha);
                        book.long_collateral_alpha = book
                            .long_collateral_alpha
                            .saturating_sub(position.collateral);
                    });
                    Self::remove_position(owner, netuid, &position);
                    match kind {
                        SettleKind::Dissolution => {
                            Self::deposit_event(Event::DissolutionSettled {
                                owner: owner.clone(),
                                netuid,
                                side: Side::Long,
                                principal_tao_equiv: position.principal,
                                tao_to_pot: shortfall,
                                tao_refund: paid_remainder,
                            });
                            Self::deposit_event(Event::Settled {
                                owner: owner.clone(),
                                netuid,
                                side: Side::Long,
                                principal_tao_equiv: position.principal,
                                tao_to_pot: shortfall,
                                tao_refund: paid_remainder,
                            });
                        }
                        SettleKind::Expiry => {
                            Self::deposit_event(Event::Expired {
                                owner: owner.clone(),
                                netuid,
                                side: Side::Long,
                                principal_tao_equiv: position.principal,
                                tao_to_pool: shortfall,
                                tao_refund: paid_remainder,
                                rate: r,
                            });
                        }
                    }
                }
            }
            Ok(())
        }

        pub fn finish_dissolution(netuid: NetUid) -> DispatchResult {
            Dissolutions::<T>::remove(netuid);
            DissolutionCursor::<T>::remove(netuid);
            DissolutionPhase::<T>::remove(netuid);
            References::<T>::remove(netuid);
            Lifts::<T>::remove(netuid);
            LongInterestPending::<T>::remove(netuid);
            Self::forget_active_net(netuid);
            TaoEma::<T>::remove(netuid);
            Ok(())
        }

        pub fn dissolution_eligible_alpha(netuid: NetUid) -> u128 {
            let book = Lifts::<T>::get(netuid);
            u128::from(book.escrow_alpha)
                .saturating_add(u128::from(book.long_collateral_alpha))
                .saturating_add(u128::from(LongInterestPending::<T>::get(netuid)))
        }

        pub fn quote_open(
            netuid: NetUid,
            side: Side,
            collateral: u64,
        ) -> Result<OpeningQuote, DispatchError> {
            Self::quote_open_for_inner(None, netuid, side, collateral, None)
        }

        pub fn quote_open_for(
            owner: &T::AccountId,
            netuid: NetUid,
            side: Side,
            collateral: u64,
            source_hotkey: Option<&T::AccountId>,
        ) -> Result<OpeningQuote, DispatchError> {
            // Match `do_open`: Long requires a source hotkey; Short must omit it.
            match (side, source_hotkey) {
                (Side::Long, Some(_)) | (Side::Short, None) => {}
                _ => return Err(Error::<T>::InvalidSourceHotkey.into()),
            }
            Self::quote_open_for_inner(Some(owner), netuid, side, collateral, None)
        }

        pub fn quote_open_exact_for(
            owner: &T::AccountId,
            netuid: NetUid,
            side: Side,
            collateral: u64,
            source_hotkey: Option<&T::AccountId>,
            principal: u64,
        ) -> Result<OpeningQuote, DispatchError> {
            match (side, source_hotkey) {
                (Side::Long, Some(_)) | (Side::Short, None) => {}
                _ => return Err(Error::<T>::InvalidSourceHotkey.into()),
            }
            Self::quote_open_for_inner(Some(owner), netuid, side, collateral, Some(principal))
        }

        fn quote_open_for_inner(
            owner: Option<&T::AccountId>,
            netuid: NetUid,
            side: Side,
            collateral: u64,
            requested: Option<u64>,
        ) -> Result<OpeningQuote, DispatchError> {
            // Accrual / forfeit mutates pool + capacity; always roll back so a
            // quote cannot persist those effects while still matching dispatch.
            use frame_support::storage::{TransactionOutcome, with_transaction};
            with_transaction(|| {
                TransactionOutcome::Rollback(Self::quote_open_for_inner_after_accrual(
                    owner, netuid, side, collateral, requested,
                ))
            })
        }

        fn quote_open_for_inner_after_accrual(
            owner: Option<&T::AccountId>,
            netuid: NetUid,
            side: Side,
            collateral: u64,
            requested: Option<u64>,
        ) -> Result<OpeningQuote, DispatchError> {
            // Same gate order as `do_open`: live + §2.3 before amount/depth.
            Self::ensure_lending_live(netuid)?;
            Self::ensure_new_subnet_gate(netuid)?;
            if let Some(o) = owner {
                ensure!(T::Pool::owner_allowed(o), Error::<T>::SubnetUnavailable);
            }
            ensure!(collateral > 0, Error::<T>::CollateralZero);
            if let Some(p) = requested {
                ensure!(p > 0, Error::<T>::PrincipalZero);
            }
            let reference = Self::ensure_reference_warm(netuid)?;
            let now = frame_system::Pallet::<T>::block_number();
            // Match `do_open`: charge interest (may forfeit → re-add lift /
            // free Cap_side) before sizing so quote Ok ⇒ dispatch Ok.
            let existing = if let Some(o) = owner {
                Self::accrue_existing_before_size(o, netuid, side, now)?
            } else {
                None
            };
            if existing.is_none() && owner.is_some() {
                ensure!(
                    PositionCount::<T>::get(netuid) < T::MaxPositionsPerSubnet::get(),
                    Error::<T>::TooManyPositions
                );
                ensure!(
                    TotalPositions::<T>::get() < T::MaxTotalPositions::get(),
                    Error::<T>::TooManyPositions
                );
            }
            let params = Self::lift_params();
            let principal = if let Some(p) = requested {
                // Exact-principal path: same collateral / LTV / capacity feasibility
                // as `do_open`. Return `InsufficientCollateral` (not
                // `DynamicCollateral`) so `lending_required_collateral` can
                // search higher without treating capacity/EMA refusals as soft.
                let old_p = existing.as_ref().map(|x| x.principal).unwrap_or(0);
                let total_p = Self::add(old_p, p)?;
                let total_c = Self::add(
                    existing.as_ref().map(|x| x.collateral).unwrap_or(0),
                    collateral,
                )?;
                let ltv = Self::ltv_tier_bps(netuid, side, &params);
                ensure!(
                    Self::principal_feasible(
                        netuid, side, total_p, total_c, old_p, owner, &reference, ltv, &params,
                    )?,
                    Error::<T>::InsufficientCollateral
                );
                let delta_l = Self::lift_tao_for_principal(netuid, side, p)?;
                Self::ensure_side_capacity(netuid, side, delta_l)?;
                p
            } else {
                Self::max_principal_for_collateral(
                    netuid,
                    side,
                    collateral,
                    existing.as_ref(),
                    owner,
                    &reference,
                    &params,
                )?
            };
            let price = reference.price.min(T::Pool::current_alpha_price(netuid));
            // Zero additional principal: map via shared dust/capacity gate before
            // V_dyn / EMA (those require a positive principal).
            if principal == 0 {
                return Err(Self::refuse_dust_or_capacity(netuid, side, price, &params));
            }
            // Post-trade EMA band for both exact and collateral-only sizing
            // (same gate as `do_open`; catches OpenLoan / quote_open without amount).
            let spot_after = Self::simulate_open_spot_after(netuid, side, principal)?;
            Self::ensure_open_ema_band(side, spot_after, reference.price, &params)?;
            let total_p = Self::add(
                existing.as_ref().map(|p| p.principal).unwrap_or(0),
                principal,
            )?;
            let total_c = Self::add(
                existing.as_ref().map(|p| p.collateral).unwrap_or(0),
                collateral,
            )?;
            let v = match side {
                // Short collateral is TAO; long collateral uses dynamic valuation.
                Side::Short => total_c,
                Side::Long => Self::v_dyn(netuid, total_c, total_p, &reference, owner)?,
            };
            Self::validate_sized_principal(
                netuid, side, principal, price, collateral, v, 0, &params,
            )
        }

        /// Approximate post-forced-trade spot after a proportional lift + buy/sell.
        /// Price is assumed ∝ reserve ratio (equal-weight Balancer / MockPool).
        fn simulate_open_spot_after(
            netuid: NetUid,
            side: Side,
            principal: u64,
        ) -> Result<U64F64, DispatchError> {
            let spot_before = T::Pool::current_alpha_price(netuid);
            let y = T::Pool::pool_tao(netuid).to_u64();
            let x = T::Pool::pool_alpha(netuid).to_u64();
            ensure!(y > 0 && x > 0, Error::<T>::EmptyPool);
            let w = T::Pool::quote_weight_parts(netuid);
            let (g_numer, g_denom) = Self::lift_fraction(netuid, side, principal)?;
            let lift_tao =
                ((u128::from(y) * u128::from(g_numer)) / u128::from(g_denom.max(1))) as u64;
            let lift_alpha =
                ((u128::from(x) * u128::from(g_numer)) / u128::from(g_denom.max(1))) as u64;
            let y1 = y.saturating_sub(lift_tao);
            let x1 = x.saturating_sub(lift_alpha);
            ensure!(y1 > 0 && x1 > 0, Error::<T>::EmptyPool);
            let (y2, x2) = match side {
                Side::Long => {
                    let a_out =
                        T::Pool::quote_buy_on(y1.into(), x1.into(), w, principal.into()).to_u64();
                    (y1.saturating_add(principal), x1.saturating_sub(a_out))
                }
                Side::Short => {
                    let t_out =
                        T::Pool::quote_sell_on(y1.into(), x1.into(), w, principal.into()).to_u64();
                    (y1.saturating_sub(t_out), x1.saturating_add(principal))
                }
            };
            ensure!(y2 > 0 && x2 > 0, Error::<T>::Arithmetic);
            let before_ratio = U64F64::from_num(y1) / U64F64::from_num(x1);
            let after_ratio = U64F64::from_num(y2) / U64F64::from_num(x2);
            let factor = after_ratio
                .checked_div(before_ratio)
                .ok_or(Error::<T>::Arithmetic)?;
            spot_before
                .checked_mul(factor)
                .ok_or_else(|| Error::<T>::Arithmetic.into())
        }

        pub fn quote_close(
            owner: &T::AccountId,
            netuid: NetUid,
            _hotkey: &T::AccountId,
            repay_from_wallet: bool,
        ) -> Result<ClosingQuote, DispatchError> {
            // Match `do_close`: disabled / dissolving before position work.
            Self::ensure_lending_live(netuid)?;
            let position = Positions::<T>::get(owner, netuid).ok_or(Error::<T>::PositionMissing)?;
            let (interest, _) =
                Self::interest_due(&position, frame_system::Pallet::<T>::block_number(), true)?;
            let remaining = position.collateral.saturating_sub(interest);
            match position.side {
                Side::Short => {
                    let pot = Self::add(remaining, position.proceeds)?;
                    if repay_from_wallet {
                        Ok(ClosingQuote {
                            payment: position.principal,
                            refund: pot,
                            escrow_return: position.proceeds,
                        })
                    } else {
                        // Escrow buyback: payment ≈ TAO to buy P alpha; refund is leftover.
                        let payment = Self::tao_to_buy_alpha(netuid, position.principal)?;
                        ensure!(payment <= pot, Error::<T>::InsufficientEscrow);
                        Ok(ClosingQuote {
                            payment,
                            refund: pot.saturating_sub(payment),
                            escrow_return: 0,
                        })
                    }
                }
                Side::Long => {
                    if repay_from_wallet {
                        Ok(ClosingQuote {
                            payment: position.principal,
                            refund: remaining.saturating_add(position.escrow_alpha),
                            escrow_return: position.escrow_alpha,
                        })
                    } else {
                        // Sell A, then minimal C for the shortfall; refund leftover C as alpha.
                        let w = T::Pool::quote_weight_parts(netuid);
                        let mut y = T::Pool::pool_tao(netuid).to_u64();
                        let mut x = T::Pool::pool_alpha(netuid).to_u64();
                        let mut raised = 0u64;
                        if position.escrow_alpha > 0 {
                            let out = T::Pool::quote_sell_on(
                                y.into(),
                                x.into(),
                                w,
                                position.escrow_alpha.into(),
                            )
                            .to_u64();
                            raised = Self::add(raised, out)?;
                            y = y.saturating_sub(out);
                            x = x.saturating_add(position.escrow_alpha);
                        }
                        let mut coll_left = remaining;
                        if raised < position.principal && coll_left > 0 {
                            let need = position.principal.saturating_sub(raised);
                            let needed_alpha = Self::alpha_to_raise_tao_on(y, x, w, need)?;
                            let sell = needed_alpha.min(coll_left);
                            ensure!(sell > 0, Error::<T>::InsufficientEscrow);
                            let out =
                                T::Pool::quote_sell_on(y.into(), x.into(), w, sell.into()).to_u64();
                            raised = Self::add(raised, out)?;
                            coll_left = coll_left.saturating_sub(sell);
                        }
                        ensure!(raised >= position.principal, Error::<T>::InsufficientEscrow);
                        Ok(ClosingQuote {
                            payment: position.principal,
                            refund: coll_left,
                            escrow_return: 0,
                        })
                    }
                }
            }
        }

        pub fn quote_raise(
            owner: &T::AccountId,
            netuid: NetUid,
            _source_hotkey: &T::AccountId,
            amount: Option<u64>,
            collateral: Option<u64>,
            sell: Option<u64>,
        ) -> Result<RaiseQuote, DispatchError> {
            // Accrual / forfeit mutates pool + capacity; roll back after sizing.
            use frame_support::storage::{TransactionOutcome, with_transaction};
            with_transaction(|| {
                TransactionOutcome::Rollback(Self::quote_raise_after_accrual(
                    owner, netuid, amount, collateral, sell,
                ))
            })
        }

        fn quote_raise_after_accrual(
            owner: &T::AccountId,
            netuid: NetUid,
            amount: Option<u64>,
            collateral: Option<u64>,
            sell: Option<u64>,
        ) -> Result<RaiseQuote, DispatchError> {
            ensure!(
                amount.is_some() ^ collateral.is_some(),
                Error::<T>::ConflictingSizing
            );
            // Same gate order as `do_open_long_and_sell`.
            Self::ensure_lending_live(netuid)?;
            Self::ensure_new_subnet_gate(netuid)?;
            ensure!(T::Pool::owner_allowed(owner), Error::<T>::SubnetUnavailable);
            let reference = Self::ensure_reference_warm(netuid)?;
            let params = Self::lift_params();
            let now = frame_system::Pallet::<T>::block_number();
            // Match raise extrinsic: charge interest (may forfeit → re-add α_L+A
            // and free Cap_long) before sizing / sale simulation.
            let existing = Self::accrue_existing_before_size(owner, netuid, Side::Long, now)?;
            let price_before = T::Pool::current_alpha_price(netuid);
            let (c, p) = if let Some(col) = collateral {
                ensure!(col > 0, Error::<T>::CollateralZero);
                let p = Self::max_principal_for_collateral(
                    netuid,
                    Side::Long,
                    col,
                    existing.as_ref(),
                    Some(owner),
                    &reference,
                    &params,
                )?;
                (col, p)
            } else {
                let p = amount.ok_or(Error::<T>::ConflictingSizing)?;
                ensure!(p > 0, Error::<T>::PrincipalZero);
                // Same feasibility (LTV + side capacity) as `do_open_long_and_sell`.
                let old_p = existing.as_ref().map(|x| x.principal).unwrap_or(0);
                let total_p = Self::add(old_p, p)?;
                let ltv = Self::ltv_tier_bps(netuid, Side::Long, &params);
                let delta_l = Self::lift_tao_for_principal(netuid, Side::Long, p)?;
                Self::ensure_side_capacity(netuid, Side::Long, delta_l)?;
                let mut lo = 1u64;
                let mut hi = u64::MAX / 4;
                let mut best = None;
                for _ in 0..BISECTION_STEPS {
                    if lo > hi {
                        break;
                    }
                    let mid = lo.saturating_add(hi.saturating_sub(lo) / 2);
                    let total_c =
                        Self::add(existing.as_ref().map(|x| x.collateral).unwrap_or(0), mid)?;
                    if Self::principal_feasible(
                        netuid,
                        Side::Long,
                        total_p,
                        total_c,
                        old_p,
                        Some(owner),
                        &reference,
                        ltv,
                        &params,
                    )? {
                        best = Some(mid);
                        hi = mid.saturating_sub(1);
                    } else {
                        lo = mid.saturating_add(1);
                    }
                }
                let best = best.ok_or(Error::<T>::InsufficientCollateral)?;
                (best, p)
            };
            let price = reference.price.min(price_before);
            // Same dust / capacity / MinimumLoanValue gate as `do_open`.
            if p == 0 {
                return Err(Self::refuse_dust_or_capacity(
                    netuid,
                    Side::Long,
                    price,
                    &params,
                ));
            }
            let interest_alpha = Self::alpha_for_tao(p, price, true)?;
            let annual_interest = Self::mul_bps(interest_alpha, params.apr_bps)?;
            if annual_interest == 0 || p < T::MinimumLoanValue::get() {
                return Err(Self::refuse_dust_or_capacity(
                    netuid,
                    Side::Long,
                    price,
                    &params,
                ));
            }
            // Simulate the raise's reserve path: lift → forced buy → wallet sale.
            // Sale must be quoted on the *post-buy* pool (not live pre-trade).
            let y = T::Pool::pool_tao(netuid).to_u64();
            let x = T::Pool::pool_alpha(netuid).to_u64();
            ensure!(y > p && x > 0, Error::<T>::EmptyPool);
            let w = T::Pool::quote_weight_parts(netuid);
            let y_lift = y.saturating_sub(p);
            let x_lift =
                x.saturating_sub(((u128::from(x) * u128::from(p)) / u128::from(y).max(1)) as u64);
            ensure!(y_lift > 0 && x_lift > 0, Error::<T>::EmptyPool);
            let a = T::Pool::quote_buy_on(y_lift.into(), x_lift.into(), w, p.into()).to_u64();
            let y_buy = y_lift.saturating_add(p);
            let x_buy = x_lift.saturating_sub(a);
            ensure!(y_buy > 0 && x_buy > 0, Error::<T>::Arithmetic);
            let price_after_buy =
                Self::approx_spot_after(price_before, y_lift, x_lift, y_buy, x_buy)?;
            let s = sell.unwrap_or(a);
            let free = T::Pool::quote_sell_on(y_buy.into(), x_buy.into(), w, s.into()).to_u64();
            let y_after = y_buy.saturating_sub(free);
            let x_after = x_buy.saturating_add(s);
            ensure!(y_after > 0 && x_after > 0, Error::<T>::Arithmetic);
            let price_after =
                Self::approx_spot_after(price_before, y_lift, x_lift, y_after, x_after)?;
            let (_cap, _cs, cl) = Self::side_caps(netuid, &params)?;
            let book = Lifts::<T>::get(netuid);
            let headroom = cl.saturating_sub(book.l_l);
            Ok(RaiseQuote {
                principal_tao: p,
                collateral_alpha: c,
                escrow_alpha: a,
                sell_alpha: s,
                free_tao_out: free,
                price_before,
                price_after_buy,
                price_after,
                v_dyn: Self::v_dyn(netuid, c, p, &reference, Some(owner))?,
                ltv_tier_bps: Self::ltv_tier_bps(netuid, Side::Long, &params),
                long_cap_headroom: headroom,
                annual_interest_alpha: annual_interest,
            })
        }

        /// Scale `spot_before` by the reserve-ratio change from `(y0,x0)` → `(y1,x1)`.
        fn approx_spot_after(
            spot_before: U64F64,
            y0: u64,
            x0: u64,
            y1: u64,
            x1: u64,
        ) -> Result<U64F64, DispatchError> {
            ensure!(y0 > 0 && x0 > 0 && y1 > 0 && x1 > 0, Error::<T>::Arithmetic);
            let before_ratio = U64F64::from_num(y0) / U64F64::from_num(x0);
            let after_ratio = U64F64::from_num(y1) / U64F64::from_num(x1);
            let factor = after_ratio
                .checked_div(before_ratio)
                .ok_or(Error::<T>::Arithmetic)?;
            spot_before
                .checked_mul(factor)
                .ok_or_else(|| Error::<T>::Arithmetic.into())
        }

        /// §14: re-key Positions / OpenByNetuid / EscrowOwner / Due / Expiry from
        /// `old` to `new` without moving custody funds (`Position.custody` stays
        /// put). Rejects when `new` already has a loan on the same netuid —
        /// overwriting would orphan the destination's custody funds. Also
        /// rejects while a subnet is dissolving: re-keying `OpenByNetuid` past
        /// `DissolutionCursor` would skip settlement and strand custody.
        /// Re-keying Due / Expiry rewinds their bucket cursors so a destination
        /// key sorting before the cursor cannot skip mandatory collection.
        #[transactional]
        pub fn on_coldkey_swap(old: &T::AccountId, new: &T::AccountId) -> DispatchResult {
            if old == new {
                return Ok(());
            }
            let positions: alloc::vec::Vec<_> = Positions::<T>::iter_prefix(old).collect();
            let mut rekeyed_due = false;
            let mut rekeyed_expiry = false;
            for (netuid, position) in positions {
                ensure!(
                    !Dissolutions::<T>::contains_key(netuid),
                    Error::<T>::SubnetUnavailable
                );
                ensure!(
                    !Positions::<T>::contains_key(new, netuid),
                    Error::<T>::PositionExists
                );
                Positions::<T>::remove(old, netuid);
                Positions::<T>::insert(new, netuid, &position);
                OpenByNetuid::<T>::remove(netuid, old);
                OpenByNetuid::<T>::insert(netuid, new, ());
                EscrowOwner::<T>::insert(netuid, &position.custody, new);
                if Due::<T>::take(position.due, (old, netuid)).is_some() {
                    Due::<T>::insert(position.due, (new, netuid), ());
                    rekeyed_due = true;
                }
                if Expiry::<T>::take(position.expiry_due, (old, netuid)).is_some() {
                    Expiry::<T>::insert(position.expiry_due, (new, netuid), ());
                    rekeyed_expiry = true;
                }
            }
            if rekeyed_due {
                DueBucketCursor::<T>::kill();
            }
            if rekeyed_expiry {
                ExpiryBucketCursor::<T>::kill();
            }
            Ok(())
        }

        pub fn is_custody_hotkey(hotkey: &T::AccountId) -> bool {
            PalletHotkey::<T>::get().is_some_and(|h| h == *hotkey)
        }
    }

    impl<T: Config> LendingInterface<T::AccountId> for Pallet<T> {
        fn custody_accounts(netuid: NetUid) -> Option<(T::AccountId, T::AccountId)> {
            let hotkey = PalletHotkey::<T>::get()?;
            Some((Self::reserve_account(netuid), hotkey))
        }
        fn has_positions(owner: &T::AccountId) -> bool {
            Positions::<T>::iter_prefix(owner).next().is_some()
        }
        fn count_positions_bounded(owner: &T::AccountId, limit: u32) -> u32 {
            if limit == 0 {
                return 0;
            }
            let mut count = 0u32;
            for _ in Positions::<T>::iter_prefix(owner) {
                count = count.saturating_add(1);
                if count >= limit {
                    break;
                }
            }
            count
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
        fn readd_lifts_for_dissolution(netuid: NetUid) -> DispatchResult {
            Self::readd_lifts_for_dissolution(netuid)
        }
        fn settle_lending(netuid: NetUid, meter: &mut WeightMeter) -> bool {
            Self::settle_lending(netuid, meter)
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
            let _ = Self::try_seed_reference(netuid);
        }
        fn on_subtoken_enabled(netuid: NetUid) {
            let _ = Self::try_seed_reference(netuid);
        }
    }
}
