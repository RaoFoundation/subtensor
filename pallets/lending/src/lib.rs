//! Lift-based escrowed lending on Balancer pools (spec v5.1 / Jacob §13.2).
//!
//! Liquidity is pulled from the pool only when someone borrows (`lift`). Shorts
//! force-sell borrowed alpha into escrowed TAO; longs force-buy with borrowed TAO
//! into escrowed alpha. No free borrowed assets are delivered. Interest burns.
//! Dissolution price uses a TAO EMA hardened against one-block pumps
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

use core::sync::atomic::{AtomicU32, Ordering};

/// Sentinel: `on_initialize` has not yet published this block's leftover.
const EXPIRY_ATTEMPTS_UNSET: u32 = u32::MAX;

/// Block-local leftover expiry settle attempts after `on_initialize`.
/// Handed to `on_idle` so idle cannot exceed the advertised per-block cap
/// (skeptic fid:da997768). Not consensus storage — scratch within one block.
static EXPIRY_ATTEMPTS_LEFT: AtomicU32 = AtomicU32::new(EXPIRY_ATTEMPTS_UNSET);

/// Bound on `ActiveNets` walked per block. Matches `GLOBAL_MAX_SUBNET_COUNT` so
/// every seeded subnet can age its reference / `G_ema` without exceeding the
/// production block weight budget (`update_reference × 4096` ≪ block limit).
pub const MAX_ACTIVE_NET_UPDATES: u64 = 4_096;
/// Hard bound on the linear mid-list walk when inserting a new expiry bucket
/// ahead of many later deadlines (skeptic fid:600d74dc). Append / prepend /
/// empty paths stay O(1). Charged as worst-case reads on open / roll weight.
pub const MAX_EXPIRY_BUCKET_WALK: u32 = 256;

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
    /// Free TAO the account can spend while preserving its existential deposit.
    /// Used to preflight underwater-short close top-ups (E2E F4).
    fn free_tao(_account: &AccountId) -> TaoBalance {
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
    /// Short dissolution settle: withdraw `recovery + interest + refund` once,
    /// then credit pot / burn / owner from that credit so a below-ED remainder
    /// cannot be reaped mid-path (skeptic fid:0fadfc1c / 83813c42).
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

    /// Re-add lift legs and credit SubnetTAO/AlphaIn/TotalStake.
    /// **Weight-preserving** (exact Balancer inverse of lift): never rewrites
    /// `w1`/`w2` from the current reserve ratio. Routing through a weight
    /// update after intervening swaps was the SN29 drain.
    fn readd_and_credit(
        netuid: NetUid,
        custody_cold: &AccountId,
        custody_hot: &AccountId,
        tao: TaoBalance,
        alpha: AlphaBalance,
    ) -> DispatchResult;

    /// Alias of [`Self::readd_and_credit`] (always weight-preserving). Kept so
    /// call sites can document wallet-repay / asymmetric-residual intent.
    fn readd_and_credit_preserve_weights(
        netuid: NetUid,
        custody_cold: &AccountId,
        custody_hot: &AccountId,
        tao: TaoBalance,
        alpha: AlphaBalance,
    ) -> DispatchResult;

    /// `(burn_tao, burn_alpha)` that restore Balancer `L` to a pre-lending
    /// baseline at the current spot. Never burns below `floor_*` (pre-credit
    /// reserves). Default delegates to
    /// [`subtensor_swap_interface::burn_amounts_to_invariant_baseline`].
    fn burn_amounts_to_k_baseline(
        tao: TaoBalance,
        alpha: AlphaBalance,
        quote_weight_parts: u64,
        baseline_tao: TaoBalance,
        baseline_alpha: AlphaBalance,
        floor_tao: TaoBalance,
        floor_alpha: AlphaBalance,
    ) -> (TaoBalance, AlphaBalance) {
        let (bt, ba) = subtensor_swap_interface::burn_amounts_to_invariant_baseline(
            tao.to_u64(),
            alpha.to_u64(),
            quote_weight_parts,
            baseline_tao.to_u64(),
            baseline_alpha.to_u64(),
            floor_tao.to_u64(),
            floor_alpha.to_u64(),
        );
        (bt.into(), ba.into())
    }

    /// Remove `tao` / `alpha` from pool reserves and destroy them (TAO burn /
    /// alpha discard). Used after lending readd to enforce k-neutrality.
    fn burn_protocol_reserves(
        netuid: NetUid,
        tao: TaoBalance,
        alpha: AlphaBalance,
    ) -> DispatchResult;

    /// Forced buy: spend custody TAO into the pool, credit alpha to custody stake.
    ///
    /// `exclude_emission_flow`: when true (lone-long books: `short_sold_alpha
    /// == 0`), skip dormant `SubnetTaoFlow`; when false (shorts already offset
    /// on the book), record so long buys can cancel short dumps in the flow
    /// accumulator. Short buybacks use [`Self::buy_alpha`] and always record.
    fn forced_buy_into_custody(
        custody_cold: &AccountId,
        custody_hot: &AccountId,
        netuid: NetUid,
        tao: TaoBalance,
        exclude_emission_flow: bool,
    ) -> Result<AlphaBalance, DispatchError>;

    /// Forced sell of custody alpha into escrow TAO on `escrow_cold`.
    /// Fails if the realised TAO is below `min_tao_out` (pass zero to disable).
    ///
    /// `exclude_emission_flow`: long open **and** long exit paths (close /
    /// repay / interest / forfeit / expiry) use the same gate —
    /// `short_sold_alpha == 0` skips, otherwise record — so a hedged long
    /// round trip nets ~0 dormant flow. Short opens always record (`false`).
    fn forced_sell_into_escrow(
        custody_cold: &AccountId,
        custody_hot: &AccountId,
        escrow_cold: &AccountId,
        netuid: NetUid,
        alpha: AlphaBalance,
        min_tao_out: TaoBalance,
        exclude_emission_flow: bool,
    ) -> Result<TaoBalance, DispatchError>;

    /// Unlocked stake available for collateral / raise sale (excludes locks).
    fn unlocked_stake(hotkey: &AccountId, coldkey: &AccountId, netuid: NetUid) -> AlphaBalance;

    /// Require `hotkey` to be a registered on-chain hotkey account.
    ///
    /// Used by close/open quotes so quote Ok ⇒ extrinsic Ok when alpha moves
    /// to or from the caller's stake. Production maps to
    /// `HotKeyAccountNotExists`.
    fn ensure_hotkey_account(hotkey: &AccountId) -> DispatchResult;

    /// Require unlocked stake on `(hotkey, coldkey, netuid)` to cover `amount`.
    ///
    /// Mirrors the stake gates inside [`Self::lock_alpha_collateral`] so a long
    /// open quote cannot say Ok when the extrinsic would fail with
    /// `StakeUnavailable`.
    fn ensure_collateral_stake(
        hotkey: &AccountId,
        coldkey: &AccountId,
        netuid: NetUid,
        amount: AlphaBalance,
    ) -> DispatchResult;

    /// Raise sale leg: unstake `alpha` to `coldkey` free TAO (`allow_partial = false`).
    fn unstake_for_raise(
        hotkey: &AccountId,
        coldkey: &AccountId,
        netuid: NetUid,
        alpha: AlphaBalance,
        min_tao_out: TaoBalance,
    ) -> Result<TaoBalance, DispatchError>;

    /// §13 lending settlement divisor: **all** subnet alpha **including pool alpha**,
    /// on every subnet (legacy and new). Must not omit `SubnetAlphaIn`. Typically
    /// `SubnetAlphaIn + SubnetAlphaOut` (u128). Used only for `r = pot / all_alpha`.
    fn all_alpha_for_lending_settlement(netuid: NetUid) -> u128;

    /// Slow moving alpha price used to cap dissolution_price (§13.2).
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
    /// Subnet-dissolution only: settle all lending positions at fixed §13.2.2
    /// rate `r = (min(T_flag, t_ema_flag) − Δ0) / ALL_alpha` (capped by moving
    /// price). Not used for ordinary expiry / close. Runs after main counts
    /// alpha and before stakers are paid. Returns `true` when done.
    fn settle_positions_on_dissolution(netuid: NetUid, meter: &mut WeightMeter) -> bool;
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
    /// O(1) `Lifts` book legs for emission's **net-long** counterfactual price.
    ///
    /// Returns `(held_tao, held_alpha, escrow_tao, escrow_alpha, short_sold_alpha)`.
    /// Callers add back only `max(0, held_alpha + escrow_alpha − short_sold_alpha)`
    /// to `SubnetAlphaIn` and leave TAO at the live reserve so lone shorts still
    /// move emission, lone longs cannot raise it, and an equal hedge ≈ spot
    /// (Jacob v5.8). When the net add-back is zero the emission input is
    /// byte-identical to live spot.
    fn emission_reserve_offsets(netuid: NetUid) -> (u64, u64, u64, u64, u64) {
        let _ = netuid;
        (0, 0, 0, 0, 0)
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

/// Inclusive upper bound on [`LiftParams::thin_blend_tao`] (1e6 τ in rao).
pub const MAX_THIN_BLEND_TAO: u64 = 1_000_000_000_000_000;
/// Inclusive lower bound on [`LiftParams::short_book_max_pool_alpha_bps`] (1%).
pub const MIN_SHORT_BOOK_MAX_POOL_ALPHA_BPS: u16 = 100;
/// Inclusive upper bound on [`LiftParams::short_book_max_pool_alpha_bps`] (50%).
pub const MAX_SHORT_BOOK_MAX_POOL_ALPHA_BPS: u16 = 5_000;
/// Inclusive lower bound on [`LiftParams::long_book_max_pool_tao_bps`] (1%).
pub const MIN_LONG_BOOK_MAX_POOL_TAO_BPS: u16 = 100;
/// Inclusive upper bound on [`LiftParams::long_book_max_pool_tao_bps`] (50%).
pub const MAX_LONG_BOOK_MAX_POOL_TAO_BPS: u16 = 5_000;
/// Inclusive upper bound on short utilization APR curve peaks (500%).
pub const MAX_APR_SHORT_CURVE_BPS: u16 = 50_000;

/// Root-settable capacity / LTV / interest / pacing parameters (bps where noted).
#[freeze_struct("4357ac49c5139b54")]
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
    /// Thin-pool floor when G_ref ≤ thin_tao (default 1500 = 15%).
    pub thin_floor_bps: u16,
    /// Reference buy size for impact rule (default 1000 τ).
    pub ref_tao: u64,
    /// Target price move for impact rule (default 1000 = 10%).
    pub move_bps: u16,
    /// Ceiling as fraction of G_ref (default 5000 = 50%).
    pub ceiling_bps: u16,
    /// Short side share of Cap (default 5000 = 50% so Cap_short = Cap_long).
    /// Additional short bound: [`Self::short_book_max_pool_alpha_bps`].
    pub split_short_bps: u16,
    /// Optional rebalance target for split_short (0 = off).
    pub rebalance_target_bps: u16,
    /// Max rebalance of Cap per day in bps (unused while target is 0).
    pub rebalance_rate_bps: u16,
    /// Thin-pool threshold on G_ref (default 3000 τ). Below/at this depth the
    /// thin floor and thin long LTV apply; above it they blend toward the full
    /// values over [`Self::thin_blend_tao`].
    pub thin_tao: u64,
    /// Short LTV (default 3300 = 33%).
    pub ltv_short_bps: u16,
    /// Long LTV (default 3300 = 33%).
    pub ltv_long_bps: u16,
    /// Thin long LTV when G_ref ≤ thin_tao (default 1500 = 15%).
    pub thin_ltv_long_bps: u16,
    /// Long APR in bps (default 10000 = 100%). Split from legacy `apr_bps`.
    pub apr_long_bps: u16,
    /// Short APR in bps (default 2000 = 20%). Split from legacy `apr_bps`.
    pub apr_short_bps: u16,
    /// Per-block long-interest sell cap as spot move bps (default 1).
    pub slice_bps: u16,
    /// Skip paced sells while spot < crash_guard_bps of EMA (default 9500).
    pub crash_guard_bps: u16,
    /// EMA window (blocks) for the subnet's **total TAO**
    /// (`SubnetTAO` / LP TAO + `Lifts.held_tao`) that feeds the dissolution
    /// price `t_ema` (default 7200 ≈ 24h). Also used as the G_ema warm-up age.
    pub total_tao_ema_blocks: u64,
    /// Minimum subnet age before lending opens (§2.3; default 50_400 ≈ 7 days).
    pub new_subnet_min_age: u64,
    /// Blocks after the gate over which Cap ramps 0→1 (§2.3; default 216_000 ≈ 30 days).
    pub new_subnet_ramp: u64,
    /// Max post-trade spot deviation from the lending EMA on the exploitable
    /// side (§5.4; default 1500 = 15%). Longs reject if spot > EMA·(1+band);
    /// shorts reject if spot < EMA·(1−band). Closes / repays / settles ignore it.
    pub ema_band_bps: u16,
    /// Hard maximum loan lifetime in blocks for **new opens and rolls only**
    /// (default 216_000 ≈ 30 days). Changing this never rewrites live
    /// `expires_at`. Bounds: [`MIN_MAX_LOAN_DURATION`]..=[`MAX_MAX_LOAN_DURATION`].
    pub max_loan_duration: u64,
    /// Ops window for settlement age (default 50_400 ≈ 7 days), measured from
    /// the position's `expires_at` (stamped into [`BandDeferralSince`] on the
    /// first band deferral — not from the first attempt block). After this
    /// window the next expiry/retry pass **force-settles at market**, bypassing
    /// [`LiftParams::expiry_max_slippage_bps`], but still subject to the
    /// per-subnet per-block [`ProtocolForcedImpact`] cap (surplus claims wait
    /// for later blocks). Increases / rolls stay frozen until settled or the
    /// owner closes. With the per-block expiry/retry budget split, a deferred
    /// claim is attempted at least once by
    /// `expires_at + max_settlement_deferral + drain`, where drain is the
    /// worst-case queue catch-up under the retry share of
    /// [`LiftParams::max_expiry_settlements_per_block`].
    pub max_settlement_deferral: u64,
    /// Age window under which a short at dissolution settles at `max(r, P_open)`
    /// (default 25_200 ≈ 3.5 days).
    pub young_short_settlement_window: u64,
    /// Minimum opening value in TAO rao (default 1_000_000_000 = 1 τ).
    pub minimum_loan_value: u64,
    /// Cap on expiry settles per `on_initialize` (default 16). Runtime value
    /// must be ≤ the compile-time [`Config::MaxExpirySettlementsPerBlock`] hard
    /// cap used for weight safety.
    pub max_expiry_settlements_per_block: u32,
    /// Max **realized** execution deviation from the lending EMA on
    /// protocol-forced AMM trades (default 200 = 2%).
    ///
    /// Shorts: cumulative TAO spent to recover alpha must be
    /// `≤ EMA·α·(1+band)`. Longs / interest sells: cumulative TAO received
    /// must be `≥ EMA·α·(1−band)`. Checked after every internal chunk; a
    /// breach rolls the whole settle back and defers. Same bound gates
    /// further expiry settles once per-subnet per-block cumulative impact
    /// exceeds the band. After [`Self::max_settlement_deferral`] the next
    /// expiry pass force-settles at market (this per-trade band is skipped)
    /// but the cumulative impact cap still paces how much can clear per
    /// block. Increases / rolls stay frozen until settled; interest keeps
    /// accruing past `expires_at` up to remaining collateral while deferred
    /// so freezing is never a free option. Owner wallet repay is
    /// unconstrained; AMM close under deferral uses the same realized bound
    /// until the force window. Only trades needed to make the pool whole are
    /// band-gated; leftover long collateral is returned as staked alpha.
    pub expiry_max_slippage_bps: u16,
    /// Width of the continuous thin→full blend on G_ref (default 3000 τ).
    ///
    /// Capacity floor and long LTV use thin values at `G_ref ≤ thin_tao`, full
    /// values at `G_ref ≥ thin_tao + thin_blend_tao`, and linear interpolation
    /// in between (Greg Shorting 102513 / Karl). `0` collapses to a step at
    /// `thin_tao` (≤ thin, > full). Bounded by [`MAX_THIN_BLEND_TAO`].
    pub thin_blend_tao: u64,
    /// Minimum forced-buyback progress per block as bps of remaining principal
    /// (default 100 = 1%). When force-market unwind cannot size any α under the
    /// spot-relative [`ProtocolForcedImpact`] band, this floor still executes
    /// (even if it exceeds the remaining impact budget) so a stuck EMA gap
    /// cannot freeze the book. First forced trade of the block only; later
    /// claims wait for the next block. `0` disables the floor (impact-only).
    pub forced_min_buyback_bps: u16,
    /// Max total short-book borrowed alpha as a share of live pool alpha
    /// (`SubnetAlphaIn` / `pool_alpha`). Default 1500 = 15%. Bounds
    /// [`MIN_SHORT_BOOK_MAX_POOL_ALPHA_BPS`]..=[`MAX_SHORT_BOOK_MAX_POOL_ALPHA_BPS`].
    /// Checked on open/increase only (never force-closes live loans; roll does
    /// not increase debt so it skips this gate). TAO Cap_short
    /// (`split_short_bps`) remains an additional bound.
    pub short_book_max_pool_alpha_bps: u16,
    /// Short APR at utilization kink
    /// ([`Self::apr_short_kink_utilization_bps`]). Default 2000 = 20% (flat
    /// from base to kink under launch defaults).
    pub apr_short_kink_bps: u16,
    /// Utilization (bps of short-book alpha cap) where the short APR curve
    /// reaches [`Self::apr_short_kink_bps`]. Default 5000 = 50%.
    pub apr_short_kink_utilization_bps: u16,
    /// Short APR at full utilization (U = 1). Default 10_000 = 100%.
    /// May exceed 100% (bounded by [`MAX_APR_SHORT_CURVE_BPS`]).
    pub apr_short_max_bps: u16,
    /// Max total long-book borrowed TAO as a share of live pool TAO
    /// (`SubnetTAO` / `pool_tao`). Default 1500 = 15%. Bounds
    /// [`MIN_LONG_BOOK_MAX_POOL_TAO_BPS`]..=[`MAX_LONG_BOOK_MAX_POOL_TAO_BPS`].
    /// Checked on open/increase only (never recalls live loans; roll with
    /// ΔL = 0 skips). Cap_long (`split_short_bps`) remains an additional bound.
    pub long_book_max_pool_tao_bps: u16,
}

impl LiftParams {
    /// Short APR (bps) at utilization `u_bps` (0..=10_000 = 0%..=100% of the
    /// short-book alpha cap). Kinked linear: base → kink_bps on
    /// `[0, kink_util]`, then kink_bps → max_bps on `(kink_util, 10_000]`.
    pub fn short_apr_bps_at_utilization(&self, u_bps: u16) -> u16 {
        let u = u_bps.min(10_000);
        let kink_u = self.apr_short_kink_utilization_bps.min(10_000);
        let base = self.apr_short_bps;
        let at_kink = self.apr_short_kink_bps;
        let at_max = self.apr_short_max_bps;
        if u <= kink_u {
            if kink_u == 0 {
                return at_kink;
            }
            let base_i = i128::from(base);
            let kink_i = i128::from(at_kink);
            let delta = (kink_i - base_i)
                .saturating_mul(i128::from(u))
                .checked_div(i128::from(kink_u))
                .unwrap_or(0);
            return base_i.saturating_add(delta).clamp(0, i128::from(u16::MAX)) as u16;
        }
        let span = 10_000u16.saturating_sub(kink_u);
        if span == 0 {
            return at_max;
        }
        let t = u.saturating_sub(kink_u);
        let kink_i = i128::from(at_kink);
        let max_i = i128::from(at_max);
        let delta = (max_i - kink_i)
            .saturating_mul(i128::from(t))
            .checked_div(i128::from(span))
            .unwrap_or(0);
        kink_i.saturating_add(delta).clamp(0, i128::from(u16::MAX)) as u16
    }

    /// Linear blend of `thin_bps` → `full_bps` across
    /// `[thin_tao, thin_tao + thin_blend_tao]` on `g_ref`.
    pub fn thin_depth_blend_bps(&self, g_ref: u64, thin_bps: u16, full_bps: u16) -> u16 {
        if g_ref <= self.thin_tao {
            return thin_bps;
        }
        let blend = self.thin_blend_tao;
        if blend == 0 {
            return full_bps;
        }
        let end = self.thin_tao.saturating_add(blend);
        if g_ref >= end {
            return full_bps;
        }
        let span = g_ref.saturating_sub(self.thin_tao);
        // thin + (full − thin) · span / blend, rounded toward thin (truncation).
        let thin_i = i128::from(thin_bps);
        let full_i = i128::from(full_bps);
        let delta = (full_i - thin_i)
            .saturating_mul(i128::from(span))
            .checked_div(i128::from(blend))
            .unwrap_or(0);
        let out = thin_i.saturating_add(delta);
        out.clamp(0, 10_000) as u16
    }
}

/// Inclusive lower bound on [`LiftParams::max_loan_duration`] (≈ 1 day at 12 s).
pub const MIN_MAX_LOAN_DURATION: u64 = 7_200;
/// Inclusive upper bound on [`LiftParams::max_loan_duration`] (≈ 1 year at 12 s).
pub const MAX_MAX_LOAN_DURATION: u64 = 2_628_000;
/// Inclusive upper bound on [`LiftParams::new_subnet_min_age`] (≈ 30 days at 12 s).
pub const MAX_NEW_SUBNET_MIN_AGE: u64 = 216_000;

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
            ltv_short_bps: 3_300,
            ltv_long_bps: 3_300,
            thin_ltv_long_bps: 1_500,
            apr_long_bps: 10_000,
            apr_short_bps: 2_000,
            slice_bps: 1,
            crash_guard_bps: 9_500,
            total_tao_ema_blocks: 7_200,
            new_subnet_min_age: 50_400,
            new_subnet_ramp: 216_000,
            ema_band_bps: 1_500,
            max_loan_duration: 216_000,
            max_settlement_deferral: 50_400,
            young_short_settlement_window: 25_200,
            minimum_loan_value: 1_000_000_000,
            max_expiry_settlements_per_block: 16,
            expiry_max_slippage_bps: 200,
            thin_blend_tao: 3_000_000_000_000,
            forced_min_buyback_bps: 100,
            short_book_max_pool_alpha_bps: 1_500,
            apr_short_kink_bps: 2_000,
            apr_short_kink_utilization_bps: 5_000,
            apr_short_max_bps: 10_000,
            long_book_max_pool_tao_bps: 1_500,
        }
    }
}

/// Pre-`ema_band_bps` layout for the one-shot LiftParams migration.
///
/// `total_tao_ema_blocks` is the same SCALE slot as the former
/// `settle_ema_blocks` (layout-safe rename; no storage rewrite).
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
    total_tao_ema_blocks: u64,
    new_subnet_min_age: u64,
    new_subnet_ramp: u64,
}

/// Pre-economic-expand layout (`apr_bps` + `ema_band_bps`, no duration knobs).
///
/// Decoded by [`migrations::migrate_lift_params_economic`] and produced by the
/// ema-band migration before that expand lands.
///
/// `total_tao_ema_blocks` is the same SCALE slot as the former
/// `settle_ema_blocks` (layout-safe rename; no storage rewrite).
#[derive(Encode, Decode, MaxEncodedLen, TypeInfo, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LiftParamsBeforeEconomic {
    pub floor_bps: u16,
    pub thin_floor_bps: u16,
    pub ref_tao: u64,
    pub move_bps: u16,
    pub ceiling_bps: u16,
    pub split_short_bps: u16,
    pub rebalance_target_bps: u16,
    pub rebalance_rate_bps: u16,
    pub thin_tao: u64,
    pub ltv_short_bps: u16,
    pub ltv_long_bps: u16,
    pub thin_ltv_long_bps: u16,
    pub apr_bps: u16,
    pub slice_bps: u16,
    pub crash_guard_bps: u16,
    pub total_tao_ema_blocks: u64,
    pub new_subnet_min_age: u64,
    pub new_subnet_ramp: u64,
    pub ema_band_bps: u16,
}

impl LiftParamsBeforeEconomic {
    pub(crate) fn into_current(self) -> LiftParams {
        LiftParams {
            floor_bps: self.floor_bps,
            thin_floor_bps: self.thin_floor_bps,
            ref_tao: self.ref_tao,
            move_bps: self.move_bps,
            ceiling_bps: self.ceiling_bps,
            split_short_bps: self.split_short_bps,
            rebalance_target_bps: self.rebalance_target_bps,
            rebalance_rate_bps: self.rebalance_rate_bps,
            thin_tao: self.thin_tao,
            ltv_short_bps: self.ltv_short_bps,
            ltv_long_bps: self.ltv_long_bps,
            thin_ltv_long_bps: self.thin_ltv_long_bps,
            apr_long_bps: self.apr_bps,
            apr_short_bps: self.apr_bps,
            slice_bps: self.slice_bps,
            crash_guard_bps: self.crash_guard_bps,
            total_tao_ema_blocks: self.total_tao_ema_blocks,
            new_subnet_min_age: self.new_subnet_min_age,
            new_subnet_ramp: self.new_subnet_ramp,
            ema_band_bps: self.ema_band_bps,
            max_loan_duration: LiftParams::default().max_loan_duration,
            max_settlement_deferral: LiftParams::default().max_settlement_deferral,
            young_short_settlement_window: LiftParams::default().young_short_settlement_window,
            minimum_loan_value: LiftParams::default().minimum_loan_value,
            max_expiry_settlements_per_block: LiftParams::default()
                .max_expiry_settlements_per_block,
            expiry_max_slippage_bps: LiftParams::default().expiry_max_slippage_bps,
            thin_blend_tao: LiftParams::default().thin_blend_tao,
            forced_min_buyback_bps: LiftParams::default().forced_min_buyback_bps,
            short_book_max_pool_alpha_bps: LiftParams::default().short_book_max_pool_alpha_bps,
            apr_short_kink_bps: LiftParams::default().apr_short_kink_bps,
            apr_short_kink_utilization_bps: LiftParams::default().apr_short_kink_utilization_bps,
            apr_short_max_bps: LiftParams::default().apr_short_max_bps,
            long_book_max_pool_tao_bps: LiftParams::default().long_book_max_pool_tao_bps,
        }
    }
}

/// Post-economic, pre-`expiry_max_slippage_bps` LiftParams layout.
///
/// Decoded by [`migrations::migrate_lift_params_expiry_slippage`].
#[derive(Encode, Decode, MaxEncodedLen, TypeInfo, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LiftParamsBeforeExpirySlippage {
    pub floor_bps: u16,
    pub thin_floor_bps: u16,
    pub ref_tao: u64,
    pub move_bps: u16,
    pub ceiling_bps: u16,
    pub split_short_bps: u16,
    pub rebalance_target_bps: u16,
    pub rebalance_rate_bps: u16,
    pub thin_tao: u64,
    pub ltv_short_bps: u16,
    pub ltv_long_bps: u16,
    pub thin_ltv_long_bps: u16,
    pub apr_long_bps: u16,
    pub apr_short_bps: u16,
    pub slice_bps: u16,
    pub crash_guard_bps: u16,
    pub total_tao_ema_blocks: u64,
    pub new_subnet_min_age: u64,
    pub new_subnet_ramp: u64,
    pub ema_band_bps: u16,
    pub max_loan_duration: u64,
    pub max_settlement_deferral: u64,
    pub young_short_settlement_window: u64,
    pub minimum_loan_value: u64,
    pub max_expiry_settlements_per_block: u32,
}

impl LiftParamsBeforeExpirySlippage {
    pub(crate) fn into_current(self) -> LiftParams {
        LiftParams {
            floor_bps: self.floor_bps,
            thin_floor_bps: self.thin_floor_bps,
            ref_tao: self.ref_tao,
            move_bps: self.move_bps,
            ceiling_bps: self.ceiling_bps,
            split_short_bps: self.split_short_bps,
            rebalance_target_bps: self.rebalance_target_bps,
            rebalance_rate_bps: self.rebalance_rate_bps,
            thin_tao: self.thin_tao,
            ltv_short_bps: self.ltv_short_bps,
            ltv_long_bps: self.ltv_long_bps,
            thin_ltv_long_bps: self.thin_ltv_long_bps,
            apr_long_bps: self.apr_long_bps,
            apr_short_bps: self.apr_short_bps,
            slice_bps: self.slice_bps,
            crash_guard_bps: self.crash_guard_bps,
            total_tao_ema_blocks: self.total_tao_ema_blocks,
            new_subnet_min_age: self.new_subnet_min_age,
            new_subnet_ramp: self.new_subnet_ramp,
            ema_band_bps: self.ema_band_bps,
            max_loan_duration: self.max_loan_duration,
            max_settlement_deferral: self.max_settlement_deferral,
            young_short_settlement_window: self.young_short_settlement_window,
            minimum_loan_value: self.minimum_loan_value,
            max_expiry_settlements_per_block: self.max_expiry_settlements_per_block,
            expiry_max_slippage_bps: LiftParams::default().expiry_max_slippage_bps,
            thin_blend_tao: LiftParams::default().thin_blend_tao,
            forced_min_buyback_bps: LiftParams::default().forced_min_buyback_bps,
            short_book_max_pool_alpha_bps: LiftParams::default().short_book_max_pool_alpha_bps,
            apr_short_kink_bps: LiftParams::default().apr_short_kink_bps,
            apr_short_kink_utilization_bps: LiftParams::default().apr_short_kink_utilization_bps,
            apr_short_max_bps: LiftParams::default().apr_short_max_bps,
            long_book_max_pool_tao_bps: LiftParams::default().long_book_max_pool_tao_bps,
        }
    }
}

/// Post-expiry-slippage, pre-`thin_blend_tao` LiftParams layout.
///
/// Decoded by [`migrations::migrate_lift_params_thin_blend`].
#[derive(Encode, Decode, MaxEncodedLen, TypeInfo, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LiftParamsBeforeThinBlend {
    pub floor_bps: u16,
    pub thin_floor_bps: u16,
    pub ref_tao: u64,
    pub move_bps: u16,
    pub ceiling_bps: u16,
    pub split_short_bps: u16,
    pub rebalance_target_bps: u16,
    pub rebalance_rate_bps: u16,
    pub thin_tao: u64,
    pub ltv_short_bps: u16,
    pub ltv_long_bps: u16,
    pub thin_ltv_long_bps: u16,
    pub apr_long_bps: u16,
    pub apr_short_bps: u16,
    pub slice_bps: u16,
    pub crash_guard_bps: u16,
    pub total_tao_ema_blocks: u64,
    pub new_subnet_min_age: u64,
    pub new_subnet_ramp: u64,
    pub ema_band_bps: u16,
    pub max_loan_duration: u64,
    pub max_settlement_deferral: u64,
    pub young_short_settlement_window: u64,
    pub minimum_loan_value: u64,
    pub max_expiry_settlements_per_block: u32,
    pub expiry_max_slippage_bps: u16,
}

impl LiftParamsBeforeThinBlend {
    pub(crate) fn into_current(self) -> LiftParams {
        LiftParams {
            floor_bps: self.floor_bps,
            thin_floor_bps: self.thin_floor_bps,
            ref_tao: self.ref_tao,
            move_bps: self.move_bps,
            ceiling_bps: self.ceiling_bps,
            split_short_bps: self.split_short_bps,
            rebalance_target_bps: self.rebalance_target_bps,
            rebalance_rate_bps: self.rebalance_rate_bps,
            thin_tao: self.thin_tao,
            ltv_short_bps: self.ltv_short_bps,
            ltv_long_bps: self.ltv_long_bps,
            thin_ltv_long_bps: self.thin_ltv_long_bps,
            apr_long_bps: self.apr_long_bps,
            apr_short_bps: self.apr_short_bps,
            slice_bps: self.slice_bps,
            crash_guard_bps: self.crash_guard_bps,
            total_tao_ema_blocks: self.total_tao_ema_blocks,
            new_subnet_min_age: self.new_subnet_min_age,
            new_subnet_ramp: self.new_subnet_ramp,
            ema_band_bps: self.ema_band_bps,
            max_loan_duration: self.max_loan_duration,
            max_settlement_deferral: self.max_settlement_deferral,
            young_short_settlement_window: self.young_short_settlement_window,
            minimum_loan_value: self.minimum_loan_value,
            max_expiry_settlements_per_block: self.max_expiry_settlements_per_block,
            expiry_max_slippage_bps: self.expiry_max_slippage_bps,
            thin_blend_tao: LiftParams::default().thin_blend_tao,
            forced_min_buyback_bps: LiftParams::default().forced_min_buyback_bps,
            short_book_max_pool_alpha_bps: LiftParams::default().short_book_max_pool_alpha_bps,
            apr_short_kink_bps: LiftParams::default().apr_short_kink_bps,
            apr_short_kink_utilization_bps: LiftParams::default().apr_short_kink_utilization_bps,
            apr_short_max_bps: LiftParams::default().apr_short_max_bps,
            long_book_max_pool_tao_bps: LiftParams::default().long_book_max_pool_tao_bps,
        }
    }
}

/// Post-`thin_blend_tao`, pre-`forced_min_buyback_bps` LiftParams layout.
///
/// Decoded by [`migrations::migrate_lift_params_forced_min_buyback`].
#[derive(Encode, Decode, MaxEncodedLen, TypeInfo, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LiftParamsBeforeForcedMinBuyback {
    pub floor_bps: u16,
    pub thin_floor_bps: u16,
    pub ref_tao: u64,
    pub move_bps: u16,
    pub ceiling_bps: u16,
    pub split_short_bps: u16,
    pub rebalance_target_bps: u16,
    pub rebalance_rate_bps: u16,
    pub thin_tao: u64,
    pub ltv_short_bps: u16,
    pub ltv_long_bps: u16,
    pub thin_ltv_long_bps: u16,
    pub apr_long_bps: u16,
    pub apr_short_bps: u16,
    pub slice_bps: u16,
    pub crash_guard_bps: u16,
    pub total_tao_ema_blocks: u64,
    pub new_subnet_min_age: u64,
    pub new_subnet_ramp: u64,
    pub ema_band_bps: u16,
    pub max_loan_duration: u64,
    pub max_settlement_deferral: u64,
    pub young_short_settlement_window: u64,
    pub minimum_loan_value: u64,
    pub max_expiry_settlements_per_block: u32,
    pub expiry_max_slippage_bps: u16,
    pub thin_blend_tao: u64,
}

impl LiftParamsBeforeForcedMinBuyback {
    pub(crate) fn into_current(self) -> LiftParams {
        LiftParams {
            floor_bps: self.floor_bps,
            thin_floor_bps: self.thin_floor_bps,
            ref_tao: self.ref_tao,
            move_bps: self.move_bps,
            ceiling_bps: self.ceiling_bps,
            split_short_bps: self.split_short_bps,
            rebalance_target_bps: self.rebalance_target_bps,
            rebalance_rate_bps: self.rebalance_rate_bps,
            thin_tao: self.thin_tao,
            ltv_short_bps: self.ltv_short_bps,
            ltv_long_bps: self.ltv_long_bps,
            thin_ltv_long_bps: self.thin_ltv_long_bps,
            apr_long_bps: self.apr_long_bps,
            apr_short_bps: self.apr_short_bps,
            slice_bps: self.slice_bps,
            crash_guard_bps: self.crash_guard_bps,
            total_tao_ema_blocks: self.total_tao_ema_blocks,
            new_subnet_min_age: self.new_subnet_min_age,
            new_subnet_ramp: self.new_subnet_ramp,
            ema_band_bps: self.ema_band_bps,
            max_loan_duration: self.max_loan_duration,
            max_settlement_deferral: self.max_settlement_deferral,
            young_short_settlement_window: self.young_short_settlement_window,
            minimum_loan_value: self.minimum_loan_value,
            max_expiry_settlements_per_block: self.max_expiry_settlements_per_block,
            expiry_max_slippage_bps: self.expiry_max_slippage_bps,
            thin_blend_tao: self.thin_blend_tao,
            forced_min_buyback_bps: LiftParams::default().forced_min_buyback_bps,
            short_book_max_pool_alpha_bps: LiftParams::default().short_book_max_pool_alpha_bps,
            apr_short_kink_bps: LiftParams::default().apr_short_kink_bps,
            apr_short_kink_utilization_bps: LiftParams::default().apr_short_kink_utilization_bps,
            apr_short_max_bps: LiftParams::default().apr_short_max_bps,
            long_book_max_pool_tao_bps: LiftParams::default().long_book_max_pool_tao_bps,
        }
    }
}

/// Post-`forced_min_buyback_bps`, pre-`short_book_max_pool_alpha_bps` layout.
///
/// Decoded by [`migrations::migrate_lift_params_short_book_alpha`].
#[derive(Encode, Decode, MaxEncodedLen, TypeInfo, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LiftParamsBeforeShortBookAlpha {
    pub floor_bps: u16,
    pub thin_floor_bps: u16,
    pub ref_tao: u64,
    pub move_bps: u16,
    pub ceiling_bps: u16,
    pub split_short_bps: u16,
    pub rebalance_target_bps: u16,
    pub rebalance_rate_bps: u16,
    pub thin_tao: u64,
    pub ltv_short_bps: u16,
    pub ltv_long_bps: u16,
    pub thin_ltv_long_bps: u16,
    pub apr_long_bps: u16,
    pub apr_short_bps: u16,
    pub slice_bps: u16,
    pub crash_guard_bps: u16,
    pub total_tao_ema_blocks: u64,
    pub new_subnet_min_age: u64,
    pub new_subnet_ramp: u64,
    pub ema_band_bps: u16,
    pub max_loan_duration: u64,
    pub max_settlement_deferral: u64,
    pub young_short_settlement_window: u64,
    pub minimum_loan_value: u64,
    pub max_expiry_settlements_per_block: u32,
    pub expiry_max_slippage_bps: u16,
    pub thin_blend_tao: u64,
    pub forced_min_buyback_bps: u16,
}

impl LiftParamsBeforeShortBookAlpha {
    pub(crate) fn into_current(self) -> LiftParams {
        LiftParams {
            floor_bps: self.floor_bps,
            thin_floor_bps: self.thin_floor_bps,
            ref_tao: self.ref_tao,
            move_bps: self.move_bps,
            ceiling_bps: self.ceiling_bps,
            // Revert accidental 2× short TAO split if still at the prior launch
            // default; otherwise preserve a root-set value.
            split_short_bps: if self.split_short_bps == 6_667 {
                LiftParams::default().split_short_bps
            } else {
                self.split_short_bps
            },
            rebalance_target_bps: self.rebalance_target_bps,
            rebalance_rate_bps: self.rebalance_rate_bps,
            thin_tao: self.thin_tao,
            ltv_short_bps: self.ltv_short_bps,
            ltv_long_bps: self.ltv_long_bps,
            thin_ltv_long_bps: self.thin_ltv_long_bps,
            apr_long_bps: self.apr_long_bps,
            apr_short_bps: self.apr_short_bps,
            slice_bps: self.slice_bps,
            crash_guard_bps: self.crash_guard_bps,
            total_tao_ema_blocks: self.total_tao_ema_blocks,
            new_subnet_min_age: self.new_subnet_min_age,
            new_subnet_ramp: self.new_subnet_ramp,
            ema_band_bps: self.ema_band_bps,
            max_loan_duration: self.max_loan_duration,
            max_settlement_deferral: self.max_settlement_deferral,
            young_short_settlement_window: self.young_short_settlement_window,
            minimum_loan_value: self.minimum_loan_value,
            max_expiry_settlements_per_block: self.max_expiry_settlements_per_block,
            expiry_max_slippage_bps: self.expiry_max_slippage_bps,
            thin_blend_tao: self.thin_blend_tao,
            forced_min_buyback_bps: self.forced_min_buyback_bps,
            short_book_max_pool_alpha_bps: LiftParams::default().short_book_max_pool_alpha_bps,
            apr_short_kink_bps: LiftParams::default().apr_short_kink_bps,
            apr_short_kink_utilization_bps: LiftParams::default().apr_short_kink_utilization_bps,
            apr_short_max_bps: LiftParams::default().apr_short_max_bps,
            long_book_max_pool_tao_bps: LiftParams::default().long_book_max_pool_tao_bps,
        }
    }
}

/// Post-`short_book_max_pool_alpha_bps`, pre-utilization-APR / long-book-share
/// layout. Decoded by [`migrations::migrate_lift_params_util_apr_long_book`].
#[derive(Encode, Decode, MaxEncodedLen, TypeInfo, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LiftParamsBeforeUtilAprLongBook {
    pub floor_bps: u16,
    pub thin_floor_bps: u16,
    pub ref_tao: u64,
    pub move_bps: u16,
    pub ceiling_bps: u16,
    pub split_short_bps: u16,
    pub rebalance_target_bps: u16,
    pub rebalance_rate_bps: u16,
    pub thin_tao: u64,
    pub ltv_short_bps: u16,
    pub ltv_long_bps: u16,
    pub thin_ltv_long_bps: u16,
    pub apr_long_bps: u16,
    pub apr_short_bps: u16,
    pub slice_bps: u16,
    pub crash_guard_bps: u16,
    pub total_tao_ema_blocks: u64,
    pub new_subnet_min_age: u64,
    pub new_subnet_ramp: u64,
    pub ema_band_bps: u16,
    pub max_loan_duration: u64,
    pub max_settlement_deferral: u64,
    pub young_short_settlement_window: u64,
    pub minimum_loan_value: u64,
    pub max_expiry_settlements_per_block: u32,
    pub expiry_max_slippage_bps: u16,
    pub thin_blend_tao: u64,
    pub forced_min_buyback_bps: u16,
    pub short_book_max_pool_alpha_bps: u16,
}

impl LiftParamsBeforeUtilAprLongBook {
    pub(crate) fn into_current(self) -> LiftParams {
        LiftParams {
            floor_bps: self.floor_bps,
            thin_floor_bps: self.thin_floor_bps,
            ref_tao: self.ref_tao,
            move_bps: self.move_bps,
            ceiling_bps: self.ceiling_bps,
            // Same 2× Cap_short revert as the short-book-alpha expand path.
            split_short_bps: if self.split_short_bps == 6_667 {
                LiftParams::default().split_short_bps
            } else {
                self.split_short_bps
            },
            rebalance_target_bps: self.rebalance_target_bps,
            rebalance_rate_bps: self.rebalance_rate_bps,
            thin_tao: self.thin_tao,
            ltv_short_bps: self.ltv_short_bps,
            ltv_long_bps: self.ltv_long_bps,
            thin_ltv_long_bps: self.thin_ltv_long_bps,
            apr_long_bps: self.apr_long_bps,
            apr_short_bps: self.apr_short_bps,
            slice_bps: self.slice_bps,
            crash_guard_bps: self.crash_guard_bps,
            total_tao_ema_blocks: self.total_tao_ema_blocks,
            new_subnet_min_age: self.new_subnet_min_age,
            new_subnet_ramp: self.new_subnet_ramp,
            ema_band_bps: self.ema_band_bps,
            max_loan_duration: self.max_loan_duration,
            max_settlement_deferral: self.max_settlement_deferral,
            young_short_settlement_window: self.young_short_settlement_window,
            minimum_loan_value: self.minimum_loan_value,
            max_expiry_settlements_per_block: self.max_expiry_settlements_per_block,
            expiry_max_slippage_bps: self.expiry_max_slippage_bps,
            thin_blend_tao: self.thin_blend_tao,
            forced_min_buyback_bps: self.forced_min_buyback_bps,
            short_book_max_pool_alpha_bps: self.short_book_max_pool_alpha_bps,
            apr_short_kink_bps: LiftParams::default().apr_short_kink_bps,
            apr_short_kink_utilization_bps: LiftParams::default().apr_short_kink_utilization_bps,
            apr_short_max_bps: LiftParams::default().apr_short_max_bps,
            long_book_max_pool_tao_bps: LiftParams::default().long_book_max_pool_tao_bps,
        }
    }
}

#[freeze_struct("7bf4887a563d4874")]
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
    /// Aggregate short principal alpha force-sold into the pool (v5.8 net-long
    /// emission). Subtracted from long alpha when building `emission_alpha_price`
    /// so a matched long+short leaves emission ≈ spot.
    pub short_sold_alpha: u64,
}

/// First terms version: close / interest / forfeit / expiry / dissolution rules
/// under the legacy one-year fixed term. Superseded for new opens by
/// [`TERMS_VERSION_V2`].
pub const TERMS_VERSION_V1: u16 = 1;
/// Terms version for the hard max loan duration (Jacob 2026-11-19): new opens
/// expire after [`LiftParams::max_loan_duration`], increases never extend the clock,
/// and young-short age uses `expires_at − max_loan_duration`.
pub const TERMS_VERSION_V2: u16 = 2;

#[freeze_struct("4219c1e8f15c7714")]
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
    /// Fixed-term end (§15). Set at open to `now + max_loan_duration` (terms v2);
    /// increases and partial repay preserve it (no renewal).
    pub expires_at: BlockNumber,
    /// Current `Expiry` queue bucket for this position. Equals `expires_at` at
    /// open; advances when a failed settle is deferred onto a retry slot so
    /// coldkey re-key / remove always find the live claim.
    pub expiry_due: BlockNumber,
    /// Economic rules version snapshotted at open (see [`TERMS_VERSION_V2`]).
    /// Raise / top-up keep this value and the original `expires_at`.
    pub terms_version: u16,
    /// Interest charge cadence in blocks, snapshotted at open from
    /// [`Config::InterestPeriod`].
    pub interest_period: BlockNumber,
    /// Accrual year length in blocks, snapshotted at open from
    /// [`Config::BlocksPerYear`]. Used by interest math only (not the term).
    pub blocks_per_year: u64,
    /// Block of open, or the last increase that added principal. Young-short
    /// settlement ages from this clock (not from `expires_at`), so an increase
    /// that adds exposure restarts the 3.5-day young window (Greg Zaitsev P1-b).
    pub last_exposure_added_at: BlockNumber,
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

#[freeze_struct("276cb2f05faec7bb")]
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, Debug, PartialEq, Eq,
)]
pub struct Dissolution<BlockNumber> {
    pub frozen_at: BlockNumber,
    /// Lift legs have been re-added into reserves.
    pub lifts_readded: bool,
    /// Dissolution price (TAO per alpha), set in settle phase 0 (§13.2).
    /// Equals `min(T_flag, t_ema_flag)/all_alpha`, then capped by moving price.
    /// SCALE layout unchanged vs the former `rate` field (rename only).
    pub dissolution_price: U64F64,
    /// Snapshot of `SubnetTAO + Lifts.held_tao` at `start_dissolution` (§13.2).
    pub t_flag: u64,
    /// Snapshot of settlement TAO EMA at `start_dissolution` (§13.2).
    pub t_ema_flag: u64,
}

/// Retired: was the pool-wide idle→busy k-neutrality snapshot.
///
/// Per-loan readd burn (2026-11-03) no longer uses a pool-wide baseline.
/// Storage entries are cleared by
/// [`migrations::migrate_clear_invariant_baseline`]; the type remains so the
/// migration can decode and drain legacy rows.
#[freeze_struct("d5ab536ed45fbf01")]
#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    TypeInfo,
    MaxEncodedLen,
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
)]
pub struct InvariantBaselineReserves {
    pub tao: u64,
    pub alpha: u64,
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

#[freeze_struct("ff18c1df3354af35")]
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, Debug, PartialEq, Eq,
)]
pub struct ClosingQuote {
    pub payment: u64,
    pub refund: u64,
    pub escrow_return: u64,
    /// TAO the owner must supply from free balance for an underwater short
    /// escrow buyback (`payment.saturating_sub(pot)`). Zero when the pot
    /// covers the buy or the path is wallet-repay / long.
    pub shortfall: u64,
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

/// Cumulative lending health counters (per-netuid and global).
///
/// Monotonic saturating accumulators for what the chain earns (interest /
/// k-neutral surplus burns) versus what pools lose (forfeited collateral,
/// shortfalls below the per-loan readd target). Never reset except that
/// per-netuid values may be removed on dissolution while globals stay.
#[freeze_struct("202664312187e3bb")]
#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    TypeInfo,
    MaxEncodedLen,
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
)]
pub struct LendingStats {
    /// TAO burned from interest charges (`InterestBurned.tao`).
    pub interest_burned_tao: u128,
    /// Alpha sold then burned as long interest (`LongInterestSold.alpha`), plus
    /// alpha destroyed in place during dissolution interest charges.
    pub interest_burned_alpha: u128,
    /// TAO burned by the k-neutral surplus path (`InvariantSurplusBurned.tao`).
    pub k_neutral_surplus_burned_tao: u128,
    /// Alpha burned by the k-neutral surplus path (`InvariantSurplusBurned.alpha`).
    pub k_neutral_surplus_burned_alpha: u128,
    /// Leftover short collateral TAO burned on forfeit (after buyback).
    pub forfeited_collateral_tao: u128,
    /// Leftover long collateral alpha destroyed on forfeit (after buyback).
    pub forfeited_collateral_alpha: u128,
    /// TAO spent on forfeit buyback (short) or repaid to the pool (long).
    pub forfeit_buyback_tao: u128,
    /// Alpha bought on short forfeit buyback, or alpha sold toward long repay.
    pub forfeit_buyback_alpha: u128,
    /// Number of [`Event::Forfeited`] emissions.
    pub forfeit_count: u64,
    /// Number of [`Event::Expired`] emissions (solvent expiry settlements).
    pub expiry_settlement_count: u64,
    /// Number of [`Event::Closed`] on the long side.
    pub closes_long: u64,
    /// Number of [`Event::Closed`] on the short side.
    pub closes_short: u64,
    /// Number of [`Event::Opened`] on the long side.
    pub opens_long: u64,
    /// Number of [`Event::Opened`] on the short side.
    pub opens_short: u64,
    /// Free-balance TAO paid into escrow on underwater short closes
    /// (`Closed.wallet_top_up`).
    pub wallet_top_ups_tao: u128,
    /// TAO-valued Balancer-`L` shortfall vs the per-loan readd target after a
    /// last-loan readd (N5 / N24). Only increments when `L' < L*`; per-reserve
    /// dips with `k` restored do not count. Valued at the live spot.
    pub pool_value_given_below_baseline_tao: u128,
    /// Retained for ABI / storage layout. New shortfalls are recorded only in
    /// `pool_value_given_below_baseline_tao` (N24); this stays 0.
    pub pool_value_given_below_baseline_alpha: u128,
    /// Cumulative long principal opened (`Opened` / `Increased` / raise).
    pub principal_opened_long: u128,
    /// Cumulative short principal opened (`Opened` / `Increased`).
    pub principal_opened_short: u128,
}

/// Geometric EMA update weight: 1 - 2^(-1 / 7200), 24h half-life at 12s blocks.
const EMA_WEIGHT_BITS: u128 = 1_775_790_721_272_497;
pub const MAX_BUYBACK_STEPS: u32 = 6;
const BPS_DENOM: u128 = 10_000;
/// Steps for principal/collateral bisections once a tight `[lo, hi]` is known.
const BISECTION_STEPS: u32 = 64;

/// Which queue a collect pass drains for one interest or expiry entry.
///
/// Outer loops stay separate (linked-list unlink vs sparse empty-bump) so
/// weight / proof-size charges stay identical to the pre-dedupe collectors.
/// Shared settle / charge / deferral bodies take this so queue differences
/// are explicit parameters, not copy-paste drift.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CollectSource {
    /// Primary Due / Expiry linked schedule (sorted buckets + Next/Prev).
    Scheduled,
    /// Sparse side queue after a failed charge/settle. O(1) enqueue; empty
    /// buckets bump with a hard cap so a sparse Next cannot spin the block.
    Retry,
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
        /// Interest charge cadence (compile-time; snapshotted onto positions).
        #[pallet::constant]
        type InterestPeriod: Get<BlockNumberFor<Self>>;
        /// Accrual year length in blocks (compile-time; snapshotted onto positions).
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
        /// Blocks to wait before retrying an interest collection that rolled back.
        /// Surviving loans must not lose `Due` coverage (Auditor fid:2aff7381).
        #[pallet::constant]
        type InterestRetryDelay: Get<BlockNumberFor<Self>>;
        /// Retry delay after a partial forced unwind or when the per-block
        /// [`ProtocolForcedImpact`] budget is exhausted. Default 1 so unwind
        /// progresses every block (not [`Self::ExpiryRetryDelay`]).
        #[pallet::constant]
        type ForcedSettlementRetryDelay: Get<BlockNumberFor<Self>>;
        /// Hard compile-time cap on expiry settles per block (weight safety).
        /// Live [`LiftParams::max_expiry_settlements_per_block`] must be ≤ this.
        #[pallet::constant]
        type MaxExpirySettlementsPerBlock: Get<u32>;
        type WeightInfo: WeightInfo;
    }

    #[pallet::storage]
    pub type Enabled<T> = StorageValue<_, bool, ValueQuery>;
    #[pallet::storage]
    pub type LiftParamsStore<T> = StorageValue<_, LiftParams, ValueQuery>;
    #[pallet::storage]
    pub type Lifts<T: Config> = StorageMap<_, Identity, NetUid, LiftBook, ValueQuery>;
    /// Retired pool-wide k-baseline (idle→busy snapshot). Cleared by
    /// [`migrations::migrate_clear_invariant_baseline`]; per-loan readd burn
    /// no longer reads or writes this map. Kept so the migration can drain it.
    #[pallet::storage]
    pub type InvariantBaseline<T: Config> =
        StorageMap<_, Identity, NetUid, InvariantBaselineReserves, OptionQuery>;
    #[pallet::storage]
    pub type LongInterestPending<T> = StorageMap<_, Identity, NetUid, u64, ValueQuery>;
    /// Block when [`LongInterestPending`] first became non-zero on this subnet.
    ///
    /// Used by the §6 crash-guard backlog override: once pending has aged past
    /// four [`Config::InterestPeriod`]s, paced sales resume even while spot is
    /// below the EMA floor. Cleared when pending returns to zero.
    #[pallet::storage]
    pub type LongInterestSince<T: Config> =
        StorageMap<_, Identity, NetUid, BlockNumberFor<T>, OptionQuery>;
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
    /// Multi-block resume cursor for [`migrations::migrate_rebuild_due_tail`].
    /// Present only while the hop-capped walk is incomplete (audit M2).
    #[pallet::storage]
    pub type DueTailRebuildCursor<T: Config> = StorageValue<_, BlockNumberFor<T>, OptionQuery>;
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
    /// Failed interest charges awaiting retry. Kept off the main [`Due`] schedule
    /// so recovery never mid-inserts into a long overdue linked list
    /// (Skeptic fid:0776a829). Insert is O(1); drain uses a lazy `Next` bump.
    #[pallet::storage]
    pub type InterestRetry<T: Config> = StorageDoubleMap<
        _,
        Twox64Concat,
        BlockNumberFor<T>,
        Blake2_128Concat,
        (T::AccountId, NetUid),
        (),
        OptionQuery,
    >;
    #[pallet::storage]
    pub type NextInterestRetry<T: Config> = StorageValue<_, BlockNumberFor<T>, OptionQuery>;
    /// Live [`InterestRetry`] entry count — `NextInterestRetry` clears only at zero
    /// (Skeptic fid:1c6bb1dc).
    #[pallet::storage]
    pub type InterestRetryPending<T: Config> = StorageValue<_, u32, ValueQuery>;
    #[pallet::storage]
    pub type InterestRetryCursor<T: Config> = StorageValue<_, (T::AccountId, NetUid), OptionQuery>;
    /// Failed expiry settles awaiting retry — same shape as [`InterestRetry`].
    #[pallet::storage]
    pub type ExpiryRetry<T: Config> = StorageDoubleMap<
        _,
        Twox64Concat,
        BlockNumberFor<T>,
        Blake2_128Concat,
        (T::AccountId, NetUid),
        (),
        OptionQuery,
    >;
    #[pallet::storage]
    pub type NextExpiryRetry<T: Config> = StorageValue<_, BlockNumberFor<T>, OptionQuery>;
    /// Live [`ExpiryRetry`] entry count — `NextExpiryRetry` clears only at zero.
    #[pallet::storage]
    pub type ExpiryRetryPending<T: Config> = StorageValue<_, u32, ValueQuery>;
    #[pallet::storage]
    pub type ExpiryRetryCursor<T: Config> = StorageValue<_, (T::AccountId, NetUid), OptionQuery>;
    /// Last deferral for a position: `(kind, retry_at, reason)` where kind is
    /// `0` = interest charge, `1` = expiry settlement (E2E N13). Cleared when
    /// the retry succeeds or the position closes.
    #[pallet::storage]
    pub type LastDeferral<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        Identity,
        NetUid,
        (u8, BlockNumberFor<T>, BoundedVec<u8, ConstU32<64>>),
        OptionQuery,
    >;
    /// Age anchor for a band / realized-slippage deferral: the position's
    /// `expires_at` (not the first-attempt wall-clock). Cleared with the
    /// position. While set: increases and rolls are refused
    /// ([`Error::SettlementFrozen`]). After
    /// [`LiftParams::max_settlement_deferral`] the next expiry pass
    /// force-settles at market (still paced by [`ProtocolForcedImpact`]).
    #[pallet::storage]
    pub type BandDeferralSince<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        Identity,
        NetUid,
        BlockNumberFor<T>,
        OptionQuery,
    >;
    /// Per-subnet cumulative protocol-forced AMM impact for the current block:
    /// `(block, Σ|realized_tao − fair_spot_tao|, Σ fair_spot_tao)`.
    /// Fair notional is **pre-trade spot** × α (not EMA) so a held spot≫EMA
    /// gap cannot freeze unwind. Further expiry settles defer once
    /// `Σ|diff| / Σfair > expiry_max_slippage_bps`, except the first
    /// force-market trade may still run `forced_min_buyback_bps` of remaining
    /// principal even when that floor exceeds the remaining budget.
    #[pallet::storage]
    pub type ProtocolForcedImpact<T: Config> =
        StorageMap<_, Identity, NetUid, (BlockNumberFor<T>, u128, u128), OptionQuery>;
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
    /// Cumulative lending health counters per subnet. Cleared on dissolution;
    /// globals in [`StatsGlobal`] are kept.
    #[pallet::storage]
    pub type StatsByNetuid<T: Config> = StorageMap<_, Identity, NetUid, LendingStats, ValueQuery>;
    /// Chain-wide cumulative lending health counters. Never cleared.
    #[pallet::storage]
    pub type StatsGlobal<T: Config> = StorageValue<_, LendingStats, ValueQuery>;
    /// Terms version assigned to newly opened positions. Live loans keep the
    /// version snapshotted into [`Position::terms_version`] at open.
    #[pallet::type_value]
    pub fn DefaultCurrentTermsVersion<T: Config>() -> u16 {
        TERMS_VERSION_V2
    }
    #[pallet::storage]
    pub type CurrentTermsVersion<T: Config> =
        StorageValue<_, u16, ValueQuery, DefaultCurrentTermsVersion<T>>;
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
            terms_version: u16,
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
            terms_version: u16,
        },
        Closed {
            owner: T::AccountId,
            netuid: NetUid,
            side: Side,
            refund: u64,
            escrow_returned: u64,
            /// Free-balance TAO the owner paid into escrow on an underwater
            /// short close (0 when the pot covered buyback / wallet-repay /
            /// long paths). E2E N10.
            wallet_top_up: u64,
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
        /// Surplus burned after readd so Balancer `L` returns to the
        /// pre-lending baseline (k-neutrality). Never exceeds the just-credited
        /// legs; never touches user funds or escrow.
        InvariantSurplusBurned {
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
        /// Atomic raise completed: escrowed long + spot sale. Emitted for both
        /// fresh opens and increases (see `increased`). E2E N11.
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
            /// True when this raise topped up an existing long (not a fresh open).
            increased: bool,
            /// Economic rules version on the resulting position.
            terms_version: u16,
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
        /// §13.2.2 phase-0 dissolution price fix after basket conversions.
        SettleDissolutionPriceFixed {
            netuid: NetUid,
            t_flag: u64,
            t_ema_flag: u64,
            t_pre: u64,
            delta0: u64,
            d: u128,
            dissolution_price: U64F64,
        },
        Repaid {
            owner: T::AccountId,
            netuid: NetUid,
            side: Side,
            principal_repaid: u64,
            refund: u64,
            escrow_returned: u64,
        },
        /// Interest charge failed this block; retried at `retry_at` (E2E U5 / N14).
        InterestDeferred {
            owner: T::AccountId,
            netuid: NetUid,
            retry_at: BlockNumberFor<T>,
            /// Decoded failure label (pallet error name or DispatchError variant).
            reason: BoundedVec<u8, ConstU32<64>>,
        },
        /// Expiry settle failed this block; retried at `retry_at` (E2E U5 / N14).
        SettlementDeferred {
            owner: T::AccountId,
            netuid: NetUid,
            retry_at: BlockNumberFor<T>,
            /// Decoded failure label (pallet error name or DispatchError variant).
            reason: BoundedVec<u8, ConstU32<64>>,
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
        /// Fixed-term expiry closed through the pool at spot, exactly like a
        /// default `close` with no wallet top-up and no slippage limits (§15).
        ///
        /// Emitted only for solvent positions. Underwater shorts and insolvent
        /// longs reuse the collateral-exhaustion [`Event::Forfeited`] path
        /// instead of recording a shortfall.
        ///
        /// Short: `payment` is TAO spent buying alpha back and
        /// `principal_tao_equiv` equals it. Long: `payment` is the TAO returned
        /// to the pool (`principal`), and `tao_refund` is AMM sale surplus
        /// only. Leftover long collateral returns as staked alpha on the
        /// custody hotkey (not sold through the realized band). `shortfall_alpha` /
        /// `shortfall_tao` are always zero (ABI retained; insolvency forfeits).
        Expired {
            owner: T::AccountId,
            netuid: NetUid,
            side: Side,
            principal_tao_equiv: u64,
            /// Native principal: alpha for shorts, TAO for longs (E2E F7).
            principal: u64,
            /// TAO paid toward the debt.
            payment: u64,
            /// TAO credited to the pool (equals `payment` on a solvent close).
            tao_to_pool: u64,
            tao_refund: u64,
            /// Spot (TAO per alpha) sampled at emit time for observability only.
            /// Does not size debt, payment, or refund — those come from the AMM
            /// close / forfeit path. Not the dissolution price.
            spot_price: U64F64,
            /// Always zero. Kept for event ABI compatibility.
            shortfall_alpha: u64,
            /// Always zero. Kept for event ABI compatibility.
            shortfall_tao: u64,
        },
        /// Lifecycle signal after every fixed-term expiry close or forfeit
        /// (including deferred forced settles). Emitted alongside the
        /// accounting [`Event::Expired`] / [`Event::Forfeited`] event.
        /// `position_id` is a stable twox-64 of `(owner, netuid)` for indexers.
        LoanExpired {
            position_id: u64,
            owner: T::AccountId,
            netuid: NetUid,
            side: Side,
            expires_at: BlockNumberFor<T>,
            settled_at: BlockNumberFor<T>,
        },
        /// Owner rolled a live loan onto a fresh max_loan_duration clock.
        Rolled {
            owner: T::AccountId,
            netuid: NetUid,
            side: Side,
            old_expires_at: BlockNumberFor<T>,
            new_expires_at: BlockNumberFor<T>,
        },
        /// Owner added collateral to a live position (no new principal).
        /// Short: `amount` is TAO moved into escrow. Long: `amount` is alpha
        /// locked from `source_hotkey` into escrow custody.
        CollateralAdded {
            owner: T::AccountId,
            netuid: NetUid,
            side: Side,
            amount: u64,
            /// New total collateral after the top-up.
            collateral: u64,
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
        /// Opening value or accrued interest is below
        /// [`LiftParams::minimum_loan_value`] (non-zero principal that still
        /// fails economic floors). Prefer [`SideCapacity`] /
        /// [`SideCapacityRamping`] when the side cap is what blocks a minimum
        /// loan.
        AmountTooSmall,
        /// Side-cap headroom cannot fit this (or any minimum_loan_value) borrow
        /// even at full (post-ramp) Cap. Prefer [`SideCapacityRamping`] when only
        /// the new-subnet ramp is what binds.
        SideCapacity,
        /// Side capacity is binding only because the subnet is still inside the
        /// §2.3 Cap ramp (`ramp(age) < 1`). Full Cap would allow this borrow —
        /// try again later as the ramp rises. Quotes use the same check.
        SideCapacityRamping,
        DynamicCollateral,
        TooManyPositions,
        BelowMinimumBorrow,
        BelowMinimumRefund,
        InsufficientEscrow,
        InvalidQuote,
        Arithmetic,
        CustodyUnavailable,
        AlreadyDissolving,
        /// Requested principal needs more collateral under V_dyn / LTV.
        InsufficientCollateral,
        InsufficientSellStake,
        /// Forced trade missed the user limit (§5.2), including `max_payment`
        /// ceilings on close/repay. Replaces RaiseSlippage / BelowMinimumProceeds
        /// for trading floors and ceilings.
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
        /// Derived position escrow is still assigned (e.g. after a coldkey swap
        /// that kept the old custody account). Refuse a fresh open so two loans
        /// cannot share one escrow (§14).
        EscrowInUse,
        /// Post-trade spot is outside the directional EMA band on the
        /// exploitable side (§5.4). Longs: spot > EMA·(1+band); shorts:
        /// spot < EMA·(1−band). Manual closes and repays are never blocked by
        /// this; permissionless settlement uses
        /// [`Self::SettlementPriceOutsideEmaBand`] instead.
        PriceTooFarFromEma,
        /// Permissionless / protocol-forced settle saw realized execution (or
        /// per-block cumulative impact) outside
        /// [`LiftParams::expiry_max_slippage_bps`] of the lending EMA, or the
        /// wider §5.4 spot band on interest-exhaustion forfeit pre-checks.
        /// The claim is deferred and retried. After
        /// [`LiftParams::max_settlement_deferral`] the next pass force-settles
        /// at market (skips the per-trade band); this error then means only
        /// the cumulative impact cap blocked the settle for this block.
        SettlementPriceOutsideEmaBand,
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
        /// Position carries a `terms_version` this runtime does not implement.
        UnsupportedTermsVersion,
        /// Expiry-bucket linked list is too deep for a fixed-weight open/roll
        /// insert (mid-list walk exceeded [`MAX_EXPIRY_BUCKET_WALK`]). Retry
        /// after attrition drains earlier expiries, or open with a later term.
        ExpiryScheduleCongested,
        /// Increase or roll refused while this position has an active
        /// settlement deferral ([`BandDeferralSince`]). Owner may still close
        /// or repay; permissionless settle resumes when execution is inside
        /// the realized EMA band.
        SettlementFrozen,
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        fn integrity_test() {
            assert!(T::BlocksPerYear::get() > 0);
            assert!(T::InterestPeriod::get() > BlockNumberFor::<T>::default());
            assert!(T::ExpiryRetryDelay::get() > BlockNumberFor::<T>::default());
            assert!(T::ForcedSettlementRetryDelay::get() > BlockNumberFor::<T>::default());
            assert!(T::InterestRetryDelay::get() > BlockNumberFor::<T>::default());
            assert!(T::MaxExpirySettlementsPerBlock::get() > 0);
            assert!(T::MaxPositionsPerSubnet::get() > 0);
            assert!(T::MaxPositionsPerSubnet::get() <= T::MaxTotalPositions::get());
            // Economic floors live in [`LiftParams::default`] / [`LiftParamsStore`].
            let defaults = LiftParams::default();
            assert!(defaults.max_loan_duration > 0);
            assert!(defaults.max_settlement_deferral > 0);
            assert!(defaults.young_short_settlement_window > 0);
            assert!(defaults.minimum_loan_value > 0);
        }

        fn on_initialize(now: BlockNumberFor<T>) -> Weight {
            // Mandatory bounded expiry drain so the 30-day unwind cannot stall
            // on empty `on_idle` weight. Cap attempts; remainder resumes via
            // ExpiryBucketCursor / ExpiryRetry. Dissolution settle still wins
            // inside the collectors. Keep the ActiveNets reserve for finalize.
            // Weight budget uses the compile-time hard cap; the live drain
            // count is the root-settable value clamped to that cap.
            let hard_cap = T::MaxExpirySettlementsPerBlock::get();
            let max = Self::max_expiry_settlements_per_block();
            // settle() × hard_cap plus proof-aware queue probes (header, bucket
            // open, peek per attempt + trailing end-of-bucket peek, empty-bucket
            // cleanup). Without these, RocksDbWeight-only padding under-funds
            // proof size and the u32 drain cap cannot be reached (skeptic
            // fid:a74169e6).
            let settle_budget = T::WeightInfo::settle()
                .saturating_mul(u64::from(hard_cap))
                .saturating_add(Self::expiry_collect_header_weight())
                .saturating_add(Self::expiry_bucket_open_weight())
                .saturating_add(
                    Self::expiry_queue_peek_weight()
                        .saturating_mul(u64::from(hard_cap.saturating_add(1))),
                )
                .saturating_add(Self::expiry_empty_bucket_weight())
                .saturating_add(T::DbWeight::get().reads_writes(8, 8));
            let mut meter = WeightMeter::with_limit(settle_budget);
            // Fair split so a sustained stream of new Expiry rows cannot starve
            // ExpiryRetry past the advertised deferral window (Greg Shorting
            // 102478). ceil(max/2) reserved for retries; leftover from either
            // pass fills the other so an idle side does not waste budget.
            let retry_share = max.saturating_add(1) / 2;
            let sched_share = max.saturating_sub(retry_share);
            let after_sched = Self::collect_expiry(now, &mut meter, sched_share);
            let after_retry = Self::collect_expiry_retries(
                now,
                &mut meter,
                retry_share.saturating_add(after_sched),
            );
            let leftover = Self::collect_expiry(now, &mut meter, after_retry);
            // Hand unused attempt budget to on_idle (never u32::MAX there).
            EXPIRY_ATTEMPTS_LEFT.store(leftover, Ordering::Relaxed);
            let n = u64::from(ActiveNetCount::<T>::get()).clamp(1, MAX_ACTIVE_NET_UPDATES);
            meter
                .consumed()
                .saturating_add(T::WeightInfo::update_reference().saturating_mul(n))
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
            // DueTail rebuild moved to `RebuildDueTailMigration` (audit M2).
            weight.saturating_add(T::DbWeight::get().reads_writes(reads, writes))
        }

        fn on_idle(now: BlockNumberFor<T>, remaining: Weight) -> Weight {
            let mut meter = WeightMeter::with_limit(remaining);
            // Resume hop-capped DueTail rebuild before queue collectors (audit M2).
            // Charge the pending probe first, then reserve hop budget before
            // work so post-hoc consume cannot fail after execution
            // (skeptic fid:849ace2f).
            let hop = T::DbWeight::get().reads(1);
            let overhead = T::DbWeight::get().reads_writes(4, 3);
            let pending_probe = T::DbWeight::get().reads(4);
            if meter.try_consume(pending_probe).is_ok()
                && migrations::rebuild_due_tail_pending::<T>()
                && meter.remaining().all_gte(overhead.saturating_add(hop))
            {
                let room = meter.remaining().saturating_sub(overhead);
                let hops = room
                    .ref_time()
                    .checked_div(hop.ref_time().max(1))
                    .unwrap_or(0)
                    .min(u64::from(migrations::MAX_DUE_TAIL_WALK));
                if let Ok(hops) = u32::try_from(hops) {
                    if hops > 0 {
                        let reserved = overhead.saturating_add(hop.saturating_mul(u64::from(hops)));
                        if meter.try_consume(reserved).is_ok() {
                            let _used = migrations::migrate_rebuild_due_tail_limited::<T>(hops);
                        }
                    }
                }
            }
            Self::collect_due(now, &mut meter);
            Self::collect_interest_retries(now, &mut meter);
            // Metered fallback after the bounded on_initialize drain. Attempt
            // budget is the leftover from on_initialize — never a raw u32::MAX
            // drain (skeptic fid:da997768). If on_initialize did not publish
            // (unit tests that call on_idle alone), fall back to the live cap.
            let published = EXPIRY_ATTEMPTS_LEFT.swap(EXPIRY_ATTEMPTS_UNSET, Ordering::Relaxed);
            let left = if published == EXPIRY_ATTEMPTS_UNSET {
                Self::max_expiry_settlements_per_block()
            } else {
                published
            };
            if left > 0 {
                let retry_share = left.saturating_add(1) / 2;
                let sched_share = left.saturating_sub(retry_share);
                let after_sched = Self::collect_expiry(now, &mut meter, sched_share);
                let after_retry = Self::collect_expiry_retries(
                    now,
                    &mut meter,
                    retry_share.saturating_add(after_sched),
                );
                let _ = Self::collect_expiry(now, &mut meter, after_retry);
            }
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
        // Short and long auto-size paths differ (long: up to 64× `v_dyn` + stake
        // locks). Charge the componentwise max of both measured benchmarks
        // (Skeptic fid:75297abf). Plus worst-case expiry mid-list walk
        // (skeptic fid:98821ef3 / 600d74dc) — DbWeight only, no invented time.
        #[pallet::weight(
            T::WeightInfo::open()
                .max(T::WeightInfo::open_long())
                .saturating_add(Pallet::<T>::expiry_insert_weight())
        )]
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
        #[pallet::weight(
            T::WeightInfo::open_exact().saturating_add(Pallet::<T>::expiry_insert_weight())
        )]
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
        /// alpha is staked to `hotkey`. Short escrow buyback may pull extra free
        /// TAO from the owner when `E_S + C` is underwater (≤ `max_payment`).
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
        // Fixed-collateral and amount-mode bisection paths differ (amount: up to
        // 64× `principal_feasible` / `v_dyn`). Charge the componentwise max of
        // both measured benchmarks (Skeptic fid:98e6778c).
        #[pallet::weight(
            T::WeightInfo::open_long_and_sell()
                .max(T::WeightInfo::open_long_and_sell_by_amount())
                .saturating_add(Pallet::<T>::expiry_insert_weight())
        )]
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
        // Full repay delegates to `do_close`; charge the heavier of the two measured
        // paths so the close-path R/W budget is never undercharged (Auditor fid:424daaa1).
        #[pallet::weight(T::WeightInfo::close().max(T::WeightInfo::repay()))]
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

        /// Atomically extend a live loan's fixed term without changing exposure.
        ///
        /// Same open gates (lending live, new-subnet, warm reference, EMA band).
        /// Charges interest to now, re-checks LTV at the current open price, and
        /// rejects when the side book already exceeds the current side cap so
        /// caps can shrink by attrition. Sets `expires_at = now + max_loan_duration`
        /// and clears band deferral; does not touch principal, collateral,
        /// proceeds / escrow_alpha, lift legs, or `last_exposure_added_at`.
        /// Opens-off (`Enabled=false`) disables rolls.
        #[pallet::call_index(8)]
        // Alias close until a reference-hardware re-measure (H2). Roll charges
        // interest and rewrites the expiry queue but does no AMM trade; close is
        // a safe upper bound. Plus worst-case expiry mid-list walk DbWeight
        // (skeptic fid:98821ef3). Do not invent local execution-time numerics.
        #[pallet::weight(
            T::WeightInfo::roll().saturating_add(Pallet::<T>::expiry_insert_weight())
        )]
        #[transactional]
        pub fn roll(origin: OriginFor<T>, netuid: NetUid) -> DispatchResult {
            let owner = ensure_signed(origin)?;
            Self::do_roll(owner, netuid)
        }

        /// Add collateral to an open position without borrowing more principal.
        ///
        /// Short: moves `amount` TAO from the owner into the position escrow
        /// (raises loss headroom). Long: locks `amount` alpha from
        /// `source_hotkey` into escrow custody (symmetric to open). Refused
        /// while the position is settlement-frozen / in forced deferral, or
        /// while the subnet is dissolving. Allowed under `ProxyType::Lending`.
        /// Positions are keyed by `(owner, netuid)` — there is no separate
        /// `position_id` argument.
        #[pallet::call_index(9)]
        // Transfer / lock only (no AMM). Alias repay until reference re-measure.
        #[pallet::weight(T::WeightInfo::add_collateral())]
        #[transactional]
        pub fn add_collateral(
            origin: OriginFor<T>,
            netuid: NetUid,
            amount: u64,
            source_hotkey: Option<T::AccountId>,
        ) -> DispatchResult {
            let owner = ensure_signed(origin)?;
            Self::do_add_collateral(owner, netuid, amount, source_hotkey)
        }
    }

    impl<T: Config> Pallet<T> {
        pub fn lift_params() -> LiftParams {
            LiftParamsStore::<T>::get()
        }

        /// Worst-case weight for a mid-list expiry-bucket insert (or the
        /// congestion reject that walks the same bound). Added on open / roll
        /// dispatch weights so the charged envelope covers
        /// [`MAX_EXPIRY_BUCKET_WALK`] (skeptic fid:98821ef3 / b6129f8a).
        ///
        /// Includes **proof size**: `RocksDbWeight::reads` charges ref_time
        /// only (proof_size 0), so we add MaxEncodedLen "added" bytes for each
        /// possible `ExpiryNext` key (2491 from weight annotations) plus a few
        /// Index/Prev/Tail/Head touches. Measured extrinsic times stay from
        /// reference benches (H2).
        pub(crate) fn expiry_insert_weight() -> Weight {
            // MaxEncodedLen "added" for `ExpiryNext` / `ExpiryPrev` /
            // `ExpiryIndex` map keys (see `weights.rs` Proof lines).
            const EXPIRY_MAP_PROOF: u64 = 2_491;
            let hops = u64::from(MAX_EXPIRY_BUCKET_WALK);
            let reads = hops.saturating_add(4);
            Weight::from_parts(0, reads.saturating_mul(EXPIRY_MAP_PROOF))
                .saturating_add(T::DbWeight::get().reads_writes(reads, 4))
        }

        // MaxEncodedLen "added" proof bytes from `weights.rs` Proof lines.
        // `RocksDbWeight` charges ref_time only (proof_size 0), so collectors
        // that touch these keys must add proof explicitly (skeptic fid:a74169e6).
        const EXPIRY_INDEX_PROOF: u64 = 2_491;
        const EXPIRY_ENTRY_PROOF: u64 = 2_537;
        const EXPIRY_CURSOR_PROOF: u64 = 529;
        const EXPIRY_SCALAR_PROOF: u64 = 499; // NextExpiry / Tail / TotalPositions

        /// Header for [`Self::collect_expiry`]: `TotalPositions` + `NextExpiry`
        /// (and the empty-book kill writes). Proof-aware.
        pub(crate) fn expiry_collect_header_weight() -> Weight {
            Weight::from_parts(0, Self::EXPIRY_SCALAR_PROOF.saturating_mul(2))
                .saturating_add(T::DbWeight::get().reads_writes(2, 1))
        }

        /// Open one due expiry bucket: cursor read + resume writes for
        /// `NextExpiry` / `ExpiryBucketCursor`. Charged **before** touching
        /// those keys (skeptic fid:a74169e6).
        pub(crate) fn expiry_bucket_open_weight() -> Weight {
            Weight::from_parts(
                0,
                Self::EXPIRY_CURSOR_PROOF.saturating_add(Self::EXPIRY_SCALAR_PROOF),
            )
            .saturating_add(T::DbWeight::get().reads_writes(1, 2))
        }

        /// One `Expiry` map probe (proof + DbWeight read) before `iter.next()`.
        pub(crate) fn expiry_queue_peek_weight() -> Weight {
            Weight::from_parts(0, Self::EXPIRY_ENTRY_PROOF)
                .saturating_add(T::DbWeight::get().reads(1))
        }

        /// One `Due` / `InterestRetry` map probe before `iter.next()`. Same
        /// key shape as expiry entries (`AccountId` + `NetUid`).
        pub(crate) fn due_queue_peek_weight() -> Weight {
            Self::expiry_queue_peek_weight()
        }

        /// Empty-bucket cleanup: `ExpiryIndex` remove + linked-list unlink
        /// (`ExpiryPrev` / `ExpiryNext` / head / tail). Charged before those
        /// writes (skeptic fid:a74169e6).
        pub(crate) fn expiry_empty_bucket_weight() -> Weight {
            Weight::from_parts(
                0,
                Self::EXPIRY_INDEX_PROOF
                    .saturating_add(Self::EXPIRY_INDEX_PROOF.saturating_mul(2))
                    .saturating_add(Self::EXPIRY_SCALAR_PROOF.saturating_mul(2)),
            )
            .saturating_add(T::DbWeight::get().reads_writes(5, 5))
        }

        /// Live max loan duration from [`LiftParamsStore`] (opens + rolls only).
        pub fn max_loan_duration() -> BlockNumberFor<T> {
            Self::lift_params().max_loan_duration.saturated_into()
        }

        /// Live max settlement band-deferral window from [`LiftParamsStore`].
        pub fn max_settlement_deferral() -> BlockNumberFor<T> {
            Self::lift_params().max_settlement_deferral.saturated_into()
        }

        /// Live young-short dissolution window from [`LiftParamsStore`].
        pub fn young_short_settlement_window() -> BlockNumberFor<T> {
            Self::lift_params()
                .young_short_settlement_window
                .saturated_into()
        }

        /// Live minimum loan opening value (TAO rao) from [`LiftParamsStore`].
        pub fn minimum_loan_value() -> u64 {
            Self::lift_params().minimum_loan_value
        }

        /// Live expiry-settle budget per block, capped by the compile-time hard
        /// limit used for weight safety.
        pub fn max_expiry_settlements_per_block() -> u32 {
            let live = Self::lift_params().max_expiry_settlements_per_block;
            live.min(T::MaxExpirySettlementsPerBlock::get())
        }

        fn apr_bps_for_side(side: Side, params: &LiftParams) -> u16 {
            match side {
                Side::Long => params.apr_long_bps,
                Side::Short => params.apr_short_bps,
            }
        }

        fn ensure_valid_lift_params(p: &LiftParams) -> DispatchResult {
            ensure!(
                p.floor_bps <= p.ceiling_bps && p.thin_floor_bps <= p.ceiling_bps,
                Error::<T>::InvalidLiftParams
            );
            ensure!(
                p.ceiling_bps <= 10_000
                    && p.split_short_bps <= 10_000
                    && p.ltv_short_bps <= 5_000
                    && p.ltv_long_bps <= 5_000
                    && p.thin_ltv_long_bps <= 5_000
                    && p.apr_long_bps <= 10_000
                    && p.apr_short_bps <= p.apr_short_max_bps
                    && p.apr_short_kink_bps <= p.apr_short_max_bps
                    && p.apr_short_max_bps <= MAX_APR_SHORT_CURVE_BPS
                    && p.apr_short_kink_utilization_bps <= 10_000
                    && p.crash_guard_bps <= 10_000
                    && p.ema_band_bps <= 10_000
                    && p.slice_bps <= 10_000
                    && p.rebalance_target_bps <= 10_000
                    && p.rebalance_rate_bps <= 10_000
                    && p.move_bps > 0
                    && p.ref_tao > 0
                    && p.total_tao_ema_blocks > 0
                    && p.new_subnet_ramp > 0
                    && p.new_subnet_min_age <= MAX_NEW_SUBNET_MIN_AGE
                    && p.max_loan_duration >= MIN_MAX_LOAN_DURATION
                    && p.max_loan_duration <= MAX_MAX_LOAN_DURATION
                    && p.max_settlement_deferral > 0
                    && p.young_short_settlement_window > 0
                    && p.minimum_loan_value > 0
                    && p.max_expiry_settlements_per_block > 0
                    && p.max_expiry_settlements_per_block <= T::MaxExpirySettlementsPerBlock::get()
                    && p.expiry_max_slippage_bps > 0
                    && p.expiry_max_slippage_bps <= 10_000
                    && p.thin_blend_tao <= MAX_THIN_BLEND_TAO
                    && p.forced_min_buyback_bps <= 10_000
                    && p.short_book_max_pool_alpha_bps >= MIN_SHORT_BOOK_MAX_POOL_ALPHA_BPS
                    && p.short_book_max_pool_alpha_bps <= MAX_SHORT_BOOK_MAX_POOL_ALPHA_BPS
                    && p.long_book_max_pool_tao_bps >= MIN_LONG_BOOK_MAX_POOL_TAO_BPS
                    && p.long_book_max_pool_tao_bps <= MAX_LONG_BOOK_MAX_POOL_TAO_BPS,
                Error::<T>::InvalidLiftParams
            );
            Ok(())
        }

        /// §5.4: directional EMA band on opens. Checked against **post-trade**
        /// spot so one large trade cannot jump past the band; splitting cannot
        /// either, because the limit is on price state, not per-trade size.
        /// Opposite-side opens are always allowed. Manual closes / repays never
        /// call this; permissionless settlement uses
        /// [`Self::ensure_settlement_ema_band`] (symmetric inside-band check).
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

        /// True when spot is inside the symmetric §5.4 EMA band
        /// `[EMA·(1−band), EMA·(1+band)]`. Missing reference → treat as inside
        /// (settlement must not stall for lack of an EMA seed).
        fn spot_within_settlement_band(netuid: NetUid) -> bool {
            let Some(reference) = References::<T>::get(netuid) else {
                return true;
            };
            let band = u32::from(Self::lift_params().ema_band_bps);
            let denom = U64F64::from_num(10_000u32);
            let ema = reference.price;
            let ceiling =
                ema.saturating_mul(U64F64::from_num(10_000u32.saturating_add(band))) / denom;
            let floor =
                ema.saturating_mul(U64F64::from_num(10_000u32.saturating_sub(band))) / denom;
            let spot = T::Pool::current_alpha_price(netuid);
            spot >= floor && spot <= ceiling
        }

        /// True once [`BandDeferralSince`] + [`LiftParams::max_settlement_deferral`]
        /// has elapsed. The next protocol-forced settle then runs at market
        /// (skips [`LiftParams::expiry_max_slippage_bps`] and the EMA spot
        /// band) but still respects per-block [`ProtocolForcedImpact`].
        fn past_max_settlement_deferral(owner: &T::AccountId, netuid: NetUid) -> bool {
            let Some(since) = BandDeferralSince::<T>::get(owner, netuid) else {
                return false;
            };
            let now = frame_system::Pallet::<T>::block_number();
            now >= since.saturating_add(Self::max_settlement_deferral())
        }

        /// Gate permissionless interest-exhaustion forfeit on the §5.4 EMA
        /// spot band. After max deferral the gate lifts so collectors can
        /// force-settle at market (realized chunk checks are also skipped on
        /// that path; [`ProtocolForcedImpact`] still paces the block).
        fn ensure_settlement_ema_band(
            owner: &T::AccountId,
            netuid: NetUid,
            _now: BlockNumberFor<T>,
        ) -> DispatchResult {
            if Self::past_max_settlement_deferral(owner, netuid) {
                return Ok(());
            }
            ensure!(
                Self::spot_within_settlement_band(netuid),
                Error::<T>::SettlementPriceOutsideEmaBand
            );
            Ok(())
        }

        /// Refuse increases / rolls while a settlement deferral is active.
        fn ensure_not_settlement_frozen(owner: &T::AccountId, netuid: NetUid) -> DispatchResult {
            ensure!(
                !BandDeferralSince::<T>::contains_key(owner, netuid),
                Error::<T>::SettlementFrozen
            );
            Ok(())
        }

        /// Fair EMA TAO for `alpha` (round up = buy / short cost; round down =
        /// sell / long proceeds). Missing reference → `None` (treat as ok).
        /// Used by the per-trade realized settlement band — not by
        /// [`ProtocolForcedImpact`] (that uses pre-trade spot).
        fn fair_ema_tao(
            netuid: NetUid,
            alpha: u64,
            round_up: bool,
        ) -> Result<Option<u64>, DispatchError> {
            let Some(reference) = References::<T>::get(netuid) else {
                return Ok(None);
            };
            Ok(Some(Self::tao_for_alpha(alpha, reference.price, round_up)?))
        }

        /// Pre-trade spot TAO for `alpha` (ProtocolForcedImpact fair notional).
        /// Caller must sample before the AMM mutates the pool.
        fn fair_spot_tao(netuid: NetUid, alpha: u64, round_up: bool) -> Result<u64, DispatchError> {
            Self::tao_for_alpha(alpha, T::Pool::current_alpha_price(netuid), round_up)
        }

        /// Floor α for one force-market progress slice of remaining `principal`.
        fn forced_min_buyback_alpha(principal: u64) -> u64 {
            if principal == 0 {
                return 0;
            }
            let bps = Self::lift_params().forced_min_buyback_bps;
            if bps == 0 {
                return 0;
            }
            let from_bps = (u128::from(principal) * u128::from(bps) / 10_000u128) as u64;
            from_bps.max(1).min(principal)
        }

        /// Short buyback: cumulative TAO spent to recover `alpha_bought` must
        /// stay `≤ fair_ema·(1 + expiry_max_slippage_bps)`.
        fn ensure_realized_buy_within_slippage(
            netuid: NetUid,
            tao_spent: u64,
            alpha_bought: u64,
        ) -> DispatchResult {
            if alpha_bought == 0 {
                return Ok(());
            }
            let Some(fair) = Self::fair_ema_tao(netuid, alpha_bought, true)? else {
                return Ok(());
            };
            let band = Self::lift_params().expiry_max_slippage_bps;
            let max_allowed = u128::from(fair)
                .saturating_mul(10_000u128.saturating_add(u128::from(band)))
                / 10_000u128;
            ensure!(
                u128::from(tao_spent) <= max_allowed,
                Error::<T>::SettlementPriceOutsideEmaBand
            );
            Ok(())
        }

        /// Long / interest sell: cumulative TAO received for `alpha_sold` must
        /// stay `≥ fair_ema·(1 − expiry_max_slippage_bps)`.
        fn ensure_realized_sell_within_slippage(
            netuid: NetUid,
            tao_received: u64,
            alpha_sold: u64,
        ) -> DispatchResult {
            if alpha_sold == 0 {
                return Ok(());
            }
            let Some(fair) = Self::fair_ema_tao(netuid, alpha_sold, false)? else {
                return Ok(());
            };
            let band = Self::lift_params().expiry_max_slippage_bps;
            let min_allowed = u128::from(fair)
                .saturating_mul(10_000u128.saturating_sub(u128::from(band)))
                / 10_000u128;
            ensure!(
                u128::from(tao_received) >= min_allowed,
                Error::<T>::SettlementPriceOutsideEmaBand
            );
            Ok(())
        }

        /// Live per-block forced-impact accumulator for `netuid` (or zeros on
        /// a fresh block / missing slot). First trade of a block starts at
        /// `(0, 0)` so it is sized against the band — no uncapped bypass.
        fn forced_impact_state(netuid: NetUid) -> (u128, u128) {
            let now = frame_system::Pallet::<T>::block_number();
            match ProtocolForcedImpact::<T>::get(netuid) {
                Some((at, cum_diff, cum_fair)) if at == now => (cum_diff, cum_fair),
                _ => (0, 0),
            }
        }

        /// Max additional `|realized − fair|` that still keeps the cumulative
        /// impact ratio ≤ `band` after adding a trade with fair notional `fair`.
        fn max_diff_for_fair(cum_diff: u128, cum_fair: u128, band: u128, fair: u128) -> u128 {
            let allowed = band
                .saturating_mul(cum_fair.saturating_add(fair))
                .checked_div(10_000u128)
                .unwrap_or(0);
            allowed.saturating_sub(cum_diff)
        }

        /// Whether `(fair, realized)` fits under the remaining impact budget.
        fn fits_forced_impact(
            cum_diff: u128,
            cum_fair: u128,
            band: u128,
            fair: u64,
            realized: u64,
        ) -> bool {
            let fair_u = u128::from(fair);
            let diff = fair_u.abs_diff(u128::from(realized));
            diff <= Self::max_diff_for_fair(cum_diff, cum_fair, band, fair_u)
        }

        /// Before starting another protocol-forced expiry settle this block:
        /// prior cumulative impact must still be inside the tolerance. Empty /
        /// stale slots are OK (caller sizes the first trade against the band).
        fn ensure_block_forced_impact_ok(netuid: NetUid) -> DispatchResult {
            let (cum_diff, cum_fair) = Self::forced_impact_state(netuid);
            if cum_fair == 0 {
                return Ok(());
            }
            let band = u128::from(Self::lift_params().expiry_max_slippage_bps);
            let impact_bps = cum_diff.saturating_mul(10_000u128) / cum_fair;
            ensure!(
                impact_bps <= band,
                Error::<T>::SettlementPriceOutsideEmaBand
            );
            Ok(())
        }

        /// Largest buyback α ≤ `principal` that fits the remaining
        /// [`ProtocolForcedImpact`] room (quoted vs **pre-trade spot** fair).
        ///
        /// Spot-relative (not EMA): a held gap of spot ≫ EMA must not make
        /// every slice exceed the cap. When `force_market` and this is the
        /// block's first forced trade, also guarantees at least
        /// [`LiftParams::forced_min_buyback_bps`] of remaining principal even
        /// if that floor exceeds the impact budget — bounded unwind progress.
        /// Later claims in the same block still wait when the budget is spent.
        /// Returns 0 when nothing can run (caller defers).
        fn max_forced_buyback_alpha(
            netuid: NetUid,
            principal: u64,
            force_market: bool,
        ) -> Result<u64, DispatchError> {
            if principal == 0 {
                return Ok(0);
            }
            let (cum_diff, cum_fair) = Self::forced_impact_state(netuid);
            let band = u128::from(Self::lift_params().expiry_max_slippage_bps);
            let first_forced = cum_fair == 0 && cum_diff == 0;
            // Common case: the whole principal fits — one quote, not log2(P).
            if let Ok(needed) = Self::tao_to_buy_alpha(netuid, principal) {
                let fair = Self::fair_spot_tao(netuid, principal, true)?;
                if Self::fits_forced_impact(cum_diff, cum_fair, band, fair, needed) {
                    return Ok(principal);
                }
            }
            let mut lo = 0u64;
            let mut hi = principal.saturating_sub(1);
            let mut best = 0u64;
            // Binary search: largest α that quotes and fits the impact budget.
            while lo <= hi {
                let mid = lo.saturating_add(hi.saturating_sub(lo) / 2).max(1);
                if mid == 0 {
                    break;
                }
                let Ok(needed) = Self::tao_to_buy_alpha(netuid, mid) else {
                    if mid <= 1 {
                        break;
                    }
                    hi = mid.saturating_sub(1);
                    continue;
                };
                let fair = Self::fair_spot_tao(netuid, mid, true)?;
                if Self::fits_forced_impact(cum_diff, cum_fair, band, fair, needed) {
                    best = mid;
                    if mid == principal {
                        break;
                    }
                    lo = mid.saturating_add(1);
                } else {
                    if mid <= 1 {
                        break;
                    }
                    hi = mid.saturating_sub(1);
                }
            }
            if best > 0 {
                // First force-market trade: never go below the progress floor.
                if force_market && first_forced {
                    let min = Self::forced_min_buyback_alpha(principal);
                    return Ok(best.max(min).min(principal));
                }
                return Ok(best);
            }
            // No impact-fit size. Force-market first trade still advances by
            // the sudo min fraction (may exceed the remaining impact budget).
            if force_market && first_forced {
                let min = Self::forced_min_buyback_alpha(principal);
                if min > 0 {
                    return Ok(min);
                }
            }
            Ok(0)
        }

        /// Record one protocol-forced trade's realized vs **pre-trade spot**
        /// fair notional into the per-subnet per-block impact accumulator.
        fn note_protocol_forced_impact(netuid: NetUid, fair_tao: u64, realized_tao: u64) {
            if fair_tao == 0 && realized_tao == 0 {
                return;
            }
            let now = frame_system::Pallet::<T>::block_number();
            let diff = u128::from(fair_tao).abs_diff(u128::from(realized_tao));
            ProtocolForcedImpact::<T>::mutate(netuid, |slot| {
                let (at, cum_diff, cum_fair) = slot.unwrap_or((now, 0u128, 0u128));
                if at != now {
                    *slot = Some((now, diff, u128::from(fair_tao)));
                } else {
                    *slot = Some((
                        at,
                        cum_diff.saturating_add(diff),
                        cum_fair.saturating_add(u128::from(fair_tao)),
                    ));
                }
            });
        }

        /// Stamp [`BandDeferralSince`] on the first band deferral (outside the
        /// rolled-back settle transaction). `since` is the age anchor: for
        /// expiry settles pass `position.expires_at`; for interest-exhaustion
        /// forfeit pass wall-clock `now`.
        fn note_band_deferral(
            owner: &T::AccountId,
            netuid: NetUid,
            since: BlockNumberFor<T>,
            error: DispatchError,
        ) {
            let is_band = match error {
                DispatchError::Module(module)
                    if module.index
                        == <Pallet<T> as frame_support::traits::PalletInfoAccess>::index()
                            as u8 =>
                {
                    matches!(
                        Error::<T>::decode(&mut &module.error[..]),
                        Ok(Error::<T>::SettlementPriceOutsideEmaBand)
                    )
                }
                _ => false,
            };
            if is_band && !BandDeferralSince::<T>::contains_key(owner, netuid) {
                BandDeferralSince::<T>::insert(owner, netuid, since);
            }
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
                book.g_ema > 0
                    && book.last.saturating_sub(registered) >= params.total_tao_ema_blocks,
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

        /// One-block integer EMA of total TAO (LP + lifted):
        /// `t_ema += (T - t_ema) / total_tao_ema_blocks`. Feeds dissolution price.
        pub fn update_tao_ema(netuid: NetUid, params: &LiftParams) {
            let t = Self::settlement_tao_basis(netuid);
            let ema = Self::ensure_tao_ema(netuid);
            let blocks = params.total_tao_ema_blocks.max(1);
            let next = if t >= ema {
                ema.saturating_add(t.saturating_sub(ema) / blocks)
            } else {
                ema.saturating_sub(ema.saturating_sub(t) / blocks)
            };
            TaoEma::<T>::insert(netuid, next);
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
            Self::ensure_subnet_repayable(netuid)
        }

        /// Lifecycle gate shared by close / repay and their quotes: subnet must
        /// be alive and not dissolving. Does **not** require [`Enabled`] —
        /// disabling new borrowing must not block repayment (Auditor fid:160e365a).
        fn ensure_subnet_repayable(netuid: NetUid) -> DispatchResult {
            // Use `network_alive` (registration + dynamic mechanism) — not
            // `subnet_exists`, which also requires SubtokenEnabled. Otherwise a
            // registered-but-unstarted subnet fails here as SubnetUnavailable
            // and `ensure_new_subnet_gate`'s SubnetNotStarted never fires.
            ensure!(
                T::Pool::network_alive(netuid) && !Dissolutions::<T>::contains_key(netuid),
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
        ///
        /// Cap_short = `split_short_bps · Cap` (TAO bound, default 50/50).
        /// Shorts also face [`Self::ensure_short_book_pool_alpha`] (share of
        /// pool alpha). Keep Cap_short derivation here only.
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
            let f_bps = params.thin_depth_blend_bps(g_ref, params.thin_floor_bps, params.floor_bps);
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

        /// Skip dormant `SubnetTaoFlow` for long forced trades iff the book has
        /// no offsetting short sold-alpha. Shared by long opens and every long
        /// exit (close / repay / interest / forfeit / expiry) so a round trip
        /// while shorts are open nets ~0 flow (Jacob v5.8 clone fix).
        fn skip_long_tao_flow(netuid: NetUid) -> bool {
            Lifts::<T>::get(netuid).short_sold_alpha == 0
        }

        /// Opening-value (TAO) of a principal amount for dust / min-loan gates.
        fn opening_value_of(
            side: Side,
            principal: u64,
            price: U64F64,
        ) -> Result<u64, DispatchError> {
            match side {
                Side::Long => Ok(principal),
                Side::Short => Self::tao_for_alpha(principal, price, true),
            }
        }

        /// Partial-repay dust gates: both the repaid slice and any nonzero
        /// remainder must have opening value ≥ [`LiftParams::minimum_loan_value`],
        /// else [`Error::AmountTooSmall`] (never surface swap `ReservesTooLow`
        /// from a 1-rao / sub-min slice).
        fn ensure_partial_repay_sizes(
            side: Side,
            principal: u64,
            principal_part: u64,
            price: U64F64,
        ) -> DispatchResult {
            let min = Self::minimum_loan_value();
            let part_value = Self::opening_value_of(side, principal_part, price)?;
            ensure!(part_value >= min, Error::<T>::AmountTooSmall);
            let remainder = principal.saturating_sub(principal_part);
            if remainder > 0 {
                let rem_value = Self::opening_value_of(side, remainder, price)?;
                ensure!(rem_value >= min, Error::<T>::AmountTooSmall);
            }
            Ok(())
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
        /// still has usable headroom for a minimum_loan_value borrow, else
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

        /// Max short-book borrowed alpha = `short_book_max_pool_alpha_bps` of
        /// live pool alpha.
        fn short_book_alpha_cap(netuid: NetUid, params: &LiftParams) -> Result<u64, DispatchError> {
            Self::mul_bps(
                T::Pool::pool_alpha(netuid).to_u64(),
                params.short_book_max_pool_alpha_bps,
            )
        }

        /// Outstanding short borrowed alpha (`Lifts.short_sold_alpha`).
        fn short_book_alpha_used(netuid: NetUid) -> u64 {
            Lifts::<T>::get(netuid).short_sold_alpha
        }

        /// Open/increase: `short_sold_alpha + delta_alpha ≤ pool_alpha · share`.
        ///
        /// `delta_alpha == 0` (roll / no new borrow) always passes — this cap
        /// never force-closes live loans and does not block term extension.
        fn ensure_short_book_pool_alpha(netuid: NetUid, delta_alpha: u64) -> DispatchResult {
            if delta_alpha == 0 {
                return Ok(());
            }
            let params = Self::lift_params();
            let max = Self::short_book_alpha_cap(netuid, &params)?;
            let used = Self::short_book_alpha_used(netuid);
            ensure!(
                used.saturating_add(delta_alpha) <= max,
                Error::<T>::SideCapacity
            );
            Ok(())
        }

        /// Max long-book borrowed TAO = `long_book_max_pool_tao_bps` of pool TAO.
        fn long_book_tao_cap(netuid: NetUid, params: &LiftParams) -> Result<u64, DispatchError> {
            Self::mul_bps(
                T::Pool::pool_tao(netuid).to_u64(),
                params.long_book_max_pool_tao_bps,
            )
        }

        /// Outstanding long borrowed TAO (`Lifts.l_l`).
        fn long_book_tao_used(netuid: NetUid) -> u64 {
            Lifts::<T>::get(netuid).l_l
        }

        /// Open/increase: `l_l + delta_tao ≤ pool_tao · share`.
        ///
        /// `delta_tao == 0` (roll / no new borrow) always passes — never recalls
        /// live longs.
        fn ensure_long_book_pool_tao(netuid: NetUid, delta_tao: u64) -> DispatchResult {
            if delta_tao == 0 {
                return Ok(());
            }
            let params = Self::lift_params();
            let max = Self::long_book_tao_cap(netuid, &params)?;
            let used = Self::long_book_tao_used(netuid);
            ensure!(
                used.saturating_add(delta_tao) <= max,
                Error::<T>::SideCapacity
            );
            Ok(())
        }

        /// Short-book utilization in bps of the alpha share cap (0..=10_000).
        /// O(1): `short_sold_alpha` and `pool_alpha` only — no position scan.
        fn short_book_utilization_bps(
            netuid: NetUid,
            params: &LiftParams,
        ) -> Result<u16, DispatchError> {
            let cap = Self::short_book_alpha_cap(netuid, params)?;
            if cap == 0 {
                return Ok(10_000);
            }
            let used = u128::from(Self::short_book_alpha_used(netuid));
            let u = used
                .saturating_mul(10_000)
                .checked_div(u128::from(cap))
                .unwrap_or(10_000)
                .min(10_000);
            Ok(u as u16)
        }

        /// Live short APR from current book utilization (O(1)).
        fn short_apr_bps_now(netuid: NetUid, params: &LiftParams) -> Result<u16, DispatchError> {
            let u = Self::short_book_utilization_bps(netuid, params)?;
            Ok(params.short_apr_bps_at_utilization(u))
        }

        /// Effective annual coupon for accrual. Shorts scale the stored
        /// base-APR coupon by live utilization APR / base so the rate tracks
        /// U without rewriting `annual_interest` or scanning the book.
        fn effective_annual_interest(
            netuid: NetUid,
            position: &Position<T::AccountId, BlockNumberFor<T>>,
        ) -> Result<u64, DispatchError> {
            match position.side {
                Side::Long => Ok(position.annual_interest),
                Side::Short => {
                    let params = Self::lift_params();
                    let base = params.apr_short_bps.max(1);
                    let live = Self::short_apr_bps_now(netuid, &params)?;
                    if live == base {
                        return Ok(position.annual_interest);
                    }
                    let scaled = u128::from(position.annual_interest)
                        .saturating_mul(u128::from(live))
                        .checked_div(u128::from(base))
                        .ok_or(Error::<T>::Arithmetic)?;
                    Ok(scaled.min(u128::from(u64::MAX)) as u64)
                }
            }
        }

        fn ltv_tier_bps(netuid: NetUid, side: Side, params: &LiftParams) -> u16 {
            match side {
                Side::Short => params.ltv_short_bps,
                Side::Long => {
                    let book = Lifts::<T>::get(netuid);
                    let g_ref = Self::g_ref(netuid, &book);
                    params.thin_depth_blend_bps(
                        g_ref,
                        params.thin_ltv_long_bps,
                        params.ltv_long_bps,
                    )
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
            let w = T::Pool::quote_weight_parts(netuid);
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

        /// F16 (locked, terms v1): coupon / opening-value price is
        /// `min(lending EMA, spot)`.
        ///
        /// For shorts this is the TAO value of borrowed alpha, so a pumped spot
        /// cannot inflate the annual TAO interest and a pumped reference cannot
        /// either. Kept on terms v1 (no mainnet positions; this has been the
        /// open-path price since the pallet landed).
        fn coupon_price(netuid: NetUid, reference: &LendingReference<BlockNumberFor<T>>) -> U64F64 {
            reference.price.min(T::Pool::current_alpha_price(netuid))
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
                Side::Short => Self::mul_bps(opening_value, Self::apr_bps_for_side(side, params))?,
                Side::Long => {
                    let alpha = Self::alpha_for_tao(principal, price, true)?;
                    Self::mul_bps(alpha, Self::apr_bps_for_side(side, params))?
                }
            };
            ensure!(principal > 0, Error::<T>::PrincipalZero);
            ensure!(
                annual_interest > 0 && opening_value >= Self::minimum_loan_value(),
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
                Self::charge_interest(owner, netuid, &mut position, now, false, false, false)?;
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

        /// True when side capacity cannot fit a minimum_loan_value borrow that
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
            let min_value = Self::minimum_loan_value();
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
        /// minimum_loan_value borrow; otherwise AmountTooSmall.
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
        /// minimum_loan_value borrow with `ΔL ≥ 1`, else `AmountTooSmall`.
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
                    // Also bound by remaining short-book pool-alpha share.
                    let alpha_cap = Self::short_book_alpha_cap(netuid, params)?;
                    let alpha_used = Self::short_book_alpha_used(netuid);
                    let alpha_headroom = alpha_cap.saturating_sub(alpha_used);
                    if alpha_headroom == 0 {
                        return Err(Self::refuse_capacity(netuid, side, price, params));
                    }
                    p.min(x.saturating_sub(1)).min(alpha_headroom)
                }
                Side::Long => {
                    let tao_cap = Self::long_book_tao_cap(netuid, params)?;
                    let tao_used = Self::long_book_tao_used(netuid);
                    let tao_headroom = tao_cap.saturating_sub(tao_used);
                    if tao_headroom == 0 {
                        return Err(Self::refuse_capacity(netuid, side, price, params));
                    }
                    headroom_tao.min(y.saturating_sub(1)).min(tao_headroom)
                }
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

        /// Extend `expires_at` under open gates without changing booked exposure.
        ///
        /// Collateral adequacy (Jacob 2026-11-23): escrow + collateral must
        /// cover buy-back / repay of principal at the opening valuation with
        /// the configured LTV margin. Replaces the prior "spot vs open EMA
        /// band" and collateral-alone LTV gates so a short can roll when
        /// price is above open price if the pot still covers the debt under
        /// LTV. Owner may `batch_all(add_collateral, roll)` to qualify.
        #[transactional]
        fn do_roll(owner: T::AccountId, netuid: NetUid) -> DispatchResult {
            let now = frame_system::Pallet::<T>::block_number();
            // Same gates as open (opens-off disables rolls). Dissolution blocks.
            Self::ensure_lending_live(netuid)?;
            Self::ensure_new_subnet_gate(netuid)?;
            ensure!(
                T::Pool::owner_allowed(&owner),
                Error::<T>::SubnetUnavailable
            );
            // Frozen while permissionless settle is deferred (no roll escape).
            Self::ensure_not_settlement_frozen(&owner, netuid)?;
            let reference = Self::ensure_or_init_reference(netuid)?;
            let mut position =
                Positions::<T>::get(&owner, netuid).ok_or(Error::<T>::PositionMissing)?;
            ensure!(now < position.expires_at, Error::<T>::DeadlinePassed);

            let params = Self::lift_params();
            let side = position.side;

            // Accrue through now before adequacy / cap checks.
            Self::charge_interest(&owner, netuid, &mut position, now, false, false, false)?;
            if !Positions::<T>::contains_key(&owner, netuid) {
                // Exhausted and forfeited during charge — nothing left to roll.
                return Err(Error::<T>::PositionMissing.into());
            }

            Self::ensure_roll_collateral_adequate(netuid, &owner, &position, &reference, &params)?;

            // Cap attrition: reject roll when the live book already exceeds the
            // current side cap (ΔL = 0).
            Self::ensure_side_capacity(netuid, side, 0)?;

            let old_expires_at = position.expires_at;
            let old_expiry_due = position.expiry_due;
            let term = Self::max_loan_duration();
            let new_expires_at = now.saturating_add(term);

            // Move expiry queue: remove old claim, insert at the new term end.
            Expiry::<T>::remove(old_expiry_due, (&owner, netuid));
            Self::note_expiry_removed(old_expiry_due);
            Expiry::<T>::insert(new_expires_at, (&owner, netuid), ());
            Self::note_expiry_inserted(new_expires_at)?;

            position.expires_at = new_expires_at;
            position.expiry_due = new_expires_at;
            // Cleared so a prior band deferral does not force-settle the new term.
            BandDeferralSince::<T>::remove(&owner, netuid);
            Positions::<T>::insert(&owner, netuid, &position);

            Self::deposit_event(Event::Rolled {
                owner,
                netuid,
                side,
                old_expires_at,
                new_expires_at,
            });
            Ok(())
        }

        /// Roll gate: escrow + collateral covers principal under LTV at the
        /// same valuation open uses (short: coupon `min(EMA, spot)`; long:
        /// `V_dyn` on collateral, plus escrow alpha in the pot).
        ///
        /// Short: `debt_value(P) ≤ (proceeds + collateral) · LTV` and the live
        /// AMM buyback of `P` must fit in the pot (solvent close headroom).
        /// Long: `P ≤ (V_dyn(C) + fair(escrow_α)) · LTV` and escrow+C must
        /// raise at least `P` through the pool (mirror of short coverage).
        fn ensure_roll_collateral_adequate(
            netuid: NetUid,
            owner: &T::AccountId,
            position: &Position<T::AccountId, BlockNumberFor<T>>,
            reference: &LendingReference<BlockNumberFor<T>>,
            params: &LiftParams,
        ) -> DispatchResult {
            let ltv = Self::ltv_tier_bps(netuid, position.side, params);
            match position.side {
                Side::Short => {
                    let pot = Self::add(position.proceeds, position.collateral)?;
                    let debt_value = Self::tao_for_alpha(
                        position.principal,
                        Self::coupon_price(netuid, reference),
                        true,
                    )?;
                    let max_debt = Self::mul_bps(pot, ltv)?;
                    ensure!(debt_value <= max_debt, Error::<T>::DynamicCollateral);
                    // Live AMM buyback must also fit (fees / impact vs coupon).
                    match Self::tao_to_buy_alpha(netuid, position.principal) {
                        Ok(cost) => ensure!(cost <= pot, Error::<T>::DynamicCollateral),
                        Err(_) => return Err(Error::<T>::DynamicCollateral.into()),
                    }
                }
                Side::Long => {
                    let v_coll = Self::v_dyn(
                        netuid,
                        position.collateral,
                        position.principal,
                        reference,
                        Some(owner),
                    )?;
                    let v_escrow = Self::fair_ema_tao(netuid, position.escrow_alpha, false)?
                        .unwrap_or(Self::tao_for_alpha(
                            position.escrow_alpha,
                            T::Pool::current_alpha_price(netuid),
                            false,
                        )?);
                    let pot_value = Self::add(v_coll, v_escrow)?;
                    let max_p = Self::mul_bps(pot_value, ltv)?;
                    ensure!(position.principal <= max_p, Error::<T>::DynamicCollateral);
                    let (raised, _) = Self::simulate_long_escrow_raise(
                        netuid,
                        &position.custody,
                        position.escrow_alpha,
                        position.collateral,
                        position.principal,
                    )?;
                    ensure!(raised >= position.principal, Error::<T>::DynamicCollateral);
                }
            }
            Ok(())
        }

        /// Top up collateral on a live position (no principal change).
        #[transactional]
        fn do_add_collateral(
            owner: T::AccountId,
            netuid: NetUid,
            amount: u64,
            source_hotkey: Option<T::AccountId>,
        ) -> DispatchResult {
            ensure!(amount > 0, Error::<T>::CollateralZero);
            ensure!(
                T::Pool::network_alive(netuid) && !Dissolutions::<T>::contains_key(netuid),
                Error::<T>::AlreadyDissolving
            );
            ensure!(
                T::Pool::owner_allowed(&owner),
                Error::<T>::SubnetUnavailable
            );
            Self::ensure_not_settlement_frozen(&owner, netuid)?;
            let mut position =
                Positions::<T>::get(&owner, netuid).ok_or(Error::<T>::PositionMissing)?;
            let side = position.side;
            let escrow = position.custody.clone();
            match side {
                Side::Short => {
                    ensure!(source_hotkey.is_none(), Error::<T>::InvalidSourceHotkey);
                    T::Pool::transfer_tao(&owner, &escrow, amount.into())?;
                }
                Side::Long => {
                    let hotkey = source_hotkey.ok_or(Error::<T>::InvalidSourceHotkey)?;
                    let custody = Self::custody_hotkey()?;
                    T::Pool::lock_alpha_collateral(
                        &owner,
                        &hotkey,
                        &escrow,
                        &custody,
                        netuid,
                        amount.into(),
                    )?;
                    Lifts::<T>::mutate(netuid, |book| {
                        book.long_collateral_alpha =
                            book.long_collateral_alpha.saturating_add(amount);
                    });
                }
            }
            position.collateral = Self::add(position.collateral, amount)?;
            let collateral = position.collateral;
            Positions::<T>::insert(&owner, netuid, &position);
            Self::deposit_event(Event::CollateralAdded {
                owner,
                netuid,
                side,
                amount,
                collateral,
            });
            Ok(())
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
                // Terms v2: no renewal. An increase at or after expiry is
                // rejected; the owner must let the position settle, then open
                // a fresh loan under a new max_loan_duration clock.
                ensure!(now < position.expires_at, Error::<T>::DeadlinePassed);
                // Frozen while permissionless settle is deferred.
                Self::ensure_not_settlement_frozen(&owner, netuid)?;
            }
            if let Some(mut position) = existing.take() {
                Self::charge_interest(&owner, netuid, &mut position, now, false, false, false)?;
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
                // Capacity before LTV/collateral sizing so an oversize principal
                // reports SideCapacity / SideCapacityRamping (not DynamicCollateral).
                let delta_l = Self::lift_tao_for_principal(netuid, side, p)?;
                Self::ensure_side_capacity(netuid, side, delta_l)?;
                if side == Side::Short {
                    Self::ensure_short_book_pool_alpha(netuid, p)?;
                }
                if side == Side::Long {
                    Self::ensure_long_book_pool_tao(netuid, p)?;
                }
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
            if side == Side::Short && requested_principal.is_none() {
                // Max-borrow path already clamped add_cap by alpha headroom;
                // re-check the chosen principal.
                Self::ensure_short_book_pool_alpha(netuid, principal)?;
            }
            if side == Side::Long && requested_principal.is_none() {
                Self::ensure_long_book_pool_tao(netuid, principal)?;
            }

            let price = Self::coupon_price(netuid, &reference);
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
                // Hard max_loan_duration term from open (§15 / terms v2).
                // Increases preserve expires_at (no renewal).
                let term = Self::max_loan_duration();
                let terms_version = {
                    let v = CurrentTermsVersion::<T>::get();
                    if v == 0 { TERMS_VERSION_V2 } else { v }
                };
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
                    terms_version,
                    interest_period: T::InterestPeriod::get(),
                    blocks_per_year: T::BlocksPerYear::get(),
                    last_exposure_added_at: now,
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
                        false,
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
                        book.short_sold_alpha = book.short_sold_alpha.saturating_add(sell);
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
                    // Same short_sold_alpha gate as long exits (net-long flow).
                    let skip_flow = Self::skip_long_tao_flow(netuid);
                    let a = T::Pool::forced_buy_into_custody(
                        &escrow,
                        &custody,
                        netuid,
                        buy_tao.into(),
                        skip_flow,
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
            // Any increase that adds principal restarts the young-short clock
            // (Greg Zaitsev P1-b). Collateral-only top-ups keep the prior stamp.
            if principal > 0 {
                position.last_exposure_added_at = now;
            }

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
                Self::note_expiry_inserted(expires_at)?;
                Self::note_active_net(netuid);
            }
            // Terms v2: increases and collateral top-ups keep the original
            // expires_at / expiry queue entry (no renewal, no extension).
            let terms_version = position.terms_version;
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
                    terms_version,
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
                    terms_version,
                }
            });
            // Accumulators mirror Opened / Increased principal (and open count
            // for fresh opens only). Raise also lands here via do_open.
            Self::bump_stats(netuid, |s| match side {
                Side::Long => {
                    if !increasing {
                        s.opens_long = s.opens_long.saturating_add(1);
                    }
                    s.principal_opened_long = s
                        .principal_opened_long
                        .saturating_add(u128::from(principal));
                }
                Side::Short => {
                    if !increasing {
                        s.opens_short = s.opens_short.saturating_add(1);
                    }
                    s.principal_opened_short = s
                        .principal_opened_short
                        .saturating_add(u128::from(principal));
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
            // Versioned close path (wallet top-up, weight-preserving readd,
            // k-neutral burn); dispatches on position.terms_version (v1 legacy
            // one-year; v2 max_loan_duration — CurrentTermsVersion defaults to v2).
            Self::ensure_terms_version(position.terms_version)?;
            Self::charge_interest(&owner, netuid, &mut position, now, true, false, false)?;
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
            let mut wallet_top_up = 0u64;
            let (refund, escrow_returned) = match side {
                Side::Short => {
                    let payment = if repay_from_wallet {
                        // §5.2: max_payment caps alpha repaid (post-interest P).
                        ensure!(
                            position.principal <= max_payment,
                            Error::<T>::SlippageExceeded
                        );
                        T::Pool::ensure_hotkey_account(&hotkey)?;
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
                        // Wallet repay leaves an asymmetric residual — preserve
                        // weights so the post-sale spot is not crystallized into w1.
                        T::Pool::readd_and_credit_preserve_weights(
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
                            // Short lift dust (lift_alpha − principal) leaves held_alpha (audit L3).
                            book.held_alpha = book.held_alpha.saturating_sub(
                                position.lift_alpha.saturating_sub(position.principal),
                            );
                            book.escrow_tao = book.escrow_tao.saturating_sub(position.proceeds);
                            book.short_sold_alpha =
                                book.short_sold_alpha.saturating_sub(position.principal);
                        });
                        (pot, position.proceeds)
                    } else {
                        // Buy back P alpha with E_S + collateral. When the price
                        // rose so pot < cost, the owner tops up free-balance TAO
                        // (no liquidations — close must stay possible). Size the
                        // top-up from *spendable* escrow balance (Preserve ED),
                        // not the accounting pot, so buyback cannot stall on a
                        // zero-capacity step (E2E F4).
                        let pot = Self::add(position.proceeds, position.collateral)?;
                        let needed = Self::tao_to_buy_alpha(netuid, position.principal)?;
                        // Usable escrow is the lesser of the accounting pot and
                        // Preserve-spendable free balance (ED). Cap MAX mock
                        // spendable by pot so tests still size top_up correctly.
                        let usable = T::Pool::buy_spendable_tao(&escrow).to_u64().min(pot);
                        let top_up = needed.saturating_sub(usable);
                        if top_up > 0 {
                            ensure!(needed <= max_payment, Error::<T>::SlippageExceeded);
                            let free = T::Pool::free_tao(&owner).to_u64();
                            // Same InsufficientEscrow surface as buyback failure;
                            // quote.shortfall carries the exact TAO needed (F4).
                            ensure!(free >= top_up, Error::<T>::InsufficientEscrow);
                            T::Pool::transfer_tao(&owner, &escrow, top_up.into())?;
                        }
                        wallet_top_up = top_up;
                        let budget = Self::add(pot, top_up)?;
                        // While settlement is deferred, AMM close uses the same
                        // realized EMA band as protocol-forced settles.
                        let realized_guard = BandDeferralSince::<T>::contains_key(&owner, netuid);
                        let (paid, bought) = Self::execute_buyback(
                            &escrow,
                            &custody,
                            netuid,
                            position.principal,
                            budget,
                            realized_guard,
                        )?;
                        ensure!(paid <= max_payment, Error::<T>::SlippageExceeded);
                        let mut refund = budget.saturating_sub(paid);
                        ensure!(refund >= min_refund, Error::<T>::BelowMinimumRefund);
                        // Fold buyback overshoot into the TAO refund — never stake
                        // dust to an unregistered wallet hotkey (E2E F5).
                        let excess = bought.saturating_sub(position.principal);
                        if excess > 0 {
                            match T::Pool::ensure_hotkey_account(&hotkey) {
                                Ok(()) => {
                                    T::Pool::transfer_staked_alpha(
                                        &escrow,
                                        &custody,
                                        &owner,
                                        &hotkey,
                                        netuid,
                                        excess.into(),
                                        false,
                                        false,
                                    )?;
                                }
                                Err(_) => {
                                    if let Ok(dust) = T::Pool::sell_alpha(
                                        &escrow,
                                        &custody,
                                        netuid,
                                        excess.into(),
                                        0u64.into(),
                                        false,
                                    ) {
                                        refund = refund.saturating_add(dust.to_u64());
                                    }
                                }
                            }
                        }
                        if refund > 0 {
                            T::Pool::transfer_tao(&escrow, &owner, refund.into())?;
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
                        Self::deposit_event(Event::Readded {
                            netuid,
                            tao: position.lift_tao,
                            alpha: position.principal,
                        });
                        Lifts::<T>::mutate(netuid, |book| {
                            book.l_s = book.l_s.saturating_sub(position.lift_tao);
                            book.held_tao = book.held_tao.saturating_sub(position.lift_tao);
                            // Short lift dust (lift_alpha − principal) leaves held_alpha (audit L3).
                            book.held_alpha = book.held_alpha.saturating_sub(
                                position.lift_alpha.saturating_sub(position.principal),
                            );
                            book.escrow_tao = book.escrow_tao.saturating_sub(position.proceeds);
                            book.short_sold_alpha =
                                book.short_sold_alpha.saturating_sub(position.principal);
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
                            T::Pool::ensure_hotkey_account(&hotkey)?;
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
                        // Wallet repay: preserve weights (see short wallet path).
                        T::Pool::readd_and_credit_preserve_weights(
                            netuid,
                            &reserve,
                            &custody,
                            position.principal.into(),
                            position.lift_alpha.into(),
                        )?;
                        Self::deposit_event(Event::Readded {
                            netuid,
                            tao: position.principal,
                            alpha: position.lift_alpha,
                        });
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
                        // While settlement is deferred, AMM close uses the same
                        // realized EMA band as protocol-forced settles (parity
                        // with the short buyback path above).
                        let realized_guard = BandDeferralSince::<T>::contains_key(&owner, netuid);
                        let mut raised = 0u64;
                        let mut sold_alpha = 0u64;
                        let skip_flow = Self::skip_long_tao_flow(netuid);
                        if position.escrow_alpha > 0 {
                            let out = T::Pool::forced_sell_into_escrow(
                                &escrow,
                                &custody,
                                &escrow,
                                netuid,
                                position.escrow_alpha.into(),
                                0u64.into(),
                                skip_flow,
                            )?;
                            raised = Self::add(raised, out.to_u64())?;
                            sold_alpha = Self::add(sold_alpha, position.escrow_alpha)?;
                            if realized_guard {
                                Self::ensure_realized_sell_within_slippage(
                                    netuid, raised, sold_alpha,
                                )?;
                            }
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
                                skip_flow,
                            )?;
                            raised = Self::add(raised, out.to_u64())?;
                            sold_alpha = Self::add(sold_alpha, sell)?;
                            coll_left = coll_left.saturating_sub(sell);
                            if realized_guard {
                                Self::ensure_realized_sell_within_slippage(
                                    netuid, raised, sold_alpha,
                                )?;
                            }
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
                            T::Pool::ensure_hotkey_account(&hotkey)?;
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
                        Self::deposit_event(Event::Readded {
                            netuid,
                            tao: position.principal,
                            alpha: position.lift_alpha,
                        });
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

            let (credited_tao, credited_alpha) = match side {
                Side::Short => (position.lift_tao, position.principal),
                Side::Long => (position.principal, position.lift_alpha),
            };
            // N31: L(lift) is the position's lifted amounts — not the credited
            // legs. Credited alpha on a short is the bought-back principal;
            // using it as L(lift) under-sizes the target when buyback recovers
            // less than was lent (forfeit) and over-burns below the no-loan level.
            Self::maybe_burn_invariant_surplus(
                netuid,
                credited_tao,
                credited_alpha,
                position.lift_tao,
                position.lift_alpha,
                Dissolutions::<T>::contains_key(netuid),
            )?;

            Self::remove_position(&owner, netuid, &position);
            Self::deposit_event(Event::Closed {
                owner,
                netuid,
                side,
                refund,
                escrow_returned,
                wallet_top_up,
            });
            Self::bump_stats(netuid, |s| {
                match side {
                    Side::Long => s.closes_long = s.closes_long.saturating_add(1),
                    Side::Short => s.closes_short = s.closes_short.saturating_add(1),
                }
                s.wallet_top_ups_tao = s
                    .wallet_top_ups_tao
                    .saturating_add(u128::from(wallet_top_up));
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
            Self::ensure_terms_version(existing.terms_version)?;
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
            // Reject dust-sized slices / remainders before any pool trade so a
            // 1-rao repay surfaces AmountTooSmall instead of swap ReservesTooLow.
            Self::ensure_partial_repay_sizes(
                existing.side,
                existing.principal,
                principal_part,
                T::Pool::current_alpha_price(netuid),
            )?;
            let mut position = existing;
            // Interest first on the whole position (§4.4).
            Self::charge_interest(&owner, netuid, &mut position, now, true, false, false)?;
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
                        T::Pool::ensure_hotkey_account(&hotkey)?;
                        T::Pool::repay_alpha(
                            &owner,
                            &hotkey,
                            &reserve,
                            &custody,
                            netuid,
                            slice_p.into(),
                        )?;
                        let pot = slice_e_s;
                        // Refund is proceeds + collateral; check the floor
                        // against the total (Auditor fid:837984df).
                        let total_refund = pot.saturating_add(slice_c);
                        ensure!(total_refund >= min_refund, Error::<T>::BelowMinimumRefund);
                        if pot > 0 {
                            T::Pool::transfer_tao(&escrow, &owner, pot.into())?;
                        }
                        // Release slice collateral TAO.
                        if slice_c > 0 {
                            T::Pool::transfer_tao(&escrow, &owner, slice_c.into())?;
                        }
                        T::Pool::readd_and_credit_preserve_weights(
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
                            book.held_alpha = book
                                .held_alpha
                                .saturating_sub(slice_lift_alpha.saturating_sub(slice_p));
                            book.escrow_tao = book.escrow_tao.saturating_sub(slice_e_s);
                            book.short_sold_alpha = book.short_sold_alpha.saturating_sub(slice_p);
                        });
                        (total_refund, slice_e_s)
                    } else {
                        // Mirror full close: top up free TAO when the slice pot
                        // cannot cover the buyback (Auditor fid:70e48032).
                        let pot = Self::add(slice_e_s, slice_c)?;
                        let needed = Self::tao_to_buy_alpha(netuid, slice_p)?;
                        let usable = T::Pool::buy_spendable_tao(&escrow).to_u64().min(pot);
                        let top_up = needed.saturating_sub(usable);
                        if top_up > 0 {
                            ensure!(needed <= max_payment, Error::<T>::SlippageExceeded);
                            let free = T::Pool::free_tao(&owner).to_u64();
                            ensure!(free >= top_up, Error::<T>::InsufficientEscrow);
                            T::Pool::transfer_tao(&owner, &escrow, top_up.into())?;
                        }
                        let budget = Self::add(pot, top_up)?;
                        let realized_guard = BandDeferralSince::<T>::contains_key(&owner, netuid);
                        let (paid, bought) = Self::execute_buyback(
                            &escrow,
                            &custody,
                            netuid,
                            slice_p,
                            budget,
                            realized_guard,
                        )?;
                        ensure!(paid <= max_payment, Error::<T>::SlippageExceeded);
                        let mut refund = budget.saturating_sub(paid);
                        ensure!(refund >= min_refund, Error::<T>::BelowMinimumRefund);
                        let excess = bought.saturating_sub(slice_p);
                        if excess > 0 {
                            match T::Pool::ensure_hotkey_account(&hotkey) {
                                Ok(()) => {
                                    T::Pool::transfer_staked_alpha(
                                        &escrow,
                                        &custody,
                                        &owner,
                                        &hotkey,
                                        netuid,
                                        excess.into(),
                                        false,
                                        false,
                                    )?;
                                }
                                Err(_) => {
                                    if let Ok(dust) = T::Pool::sell_alpha(
                                        &escrow,
                                        &custody,
                                        netuid,
                                        excess.into(),
                                        0u64.into(),
                                        false,
                                    ) {
                                        refund = refund.saturating_add(dust.to_u64());
                                    }
                                }
                            }
                        }
                        if refund > 0 {
                            T::Pool::transfer_tao(&escrow, &owner, refund.into())?;
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
                        Self::deposit_event(Event::Readded {
                            netuid,
                            tao: slice_lift_tao,
                            alpha: slice_p,
                        });
                        Lifts::<T>::mutate(netuid, |book| {
                            book.l_s = book.l_s.saturating_sub(slice_lift_tao);
                            book.held_tao = book.held_tao.saturating_sub(slice_lift_tao);
                            book.held_alpha = book
                                .held_alpha
                                .saturating_sub(slice_lift_alpha.saturating_sub(slice_p));
                            book.escrow_tao = book.escrow_tao.saturating_sub(slice_e_s);
                            book.short_sold_alpha = book.short_sold_alpha.saturating_sub(slice_p);
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
                            T::Pool::ensure_hotkey_account(&hotkey)?;
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
                        T::Pool::readd_and_credit_preserve_weights(
                            netuid,
                            &reserve,
                            &custody,
                            slice_p.into(),
                            slice_lift_alpha.into(),
                        )?;
                        Self::deposit_event(Event::Readded {
                            netuid,
                            tao: slice_p,
                            alpha: slice_lift_alpha,
                        });
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
                        // Deferred AMM repay: same realized EMA band as close /
                        // protocol-forced long sells.
                        let realized_guard = BandDeferralSince::<T>::contains_key(&owner, netuid);
                        let mut raised = 0u64;
                        let mut sold_alpha = 0u64;
                        let skip_flow = Self::skip_long_tao_flow(netuid);
                        if slice_a > 0 {
                            let out = T::Pool::forced_sell_into_escrow(
                                &escrow,
                                &custody,
                                &escrow,
                                netuid,
                                slice_a.into(),
                                0u64.into(),
                                skip_flow,
                            )?;
                            raised = Self::add(raised, out.to_u64())?;
                            sold_alpha = Self::add(sold_alpha, slice_a)?;
                            if realized_guard {
                                Self::ensure_realized_sell_within_slippage(
                                    netuid, raised, sold_alpha,
                                )?;
                            }
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
                                skip_flow,
                            )?;
                            raised = Self::add(raised, out.to_u64())?;
                            sold_alpha = Self::add(sold_alpha, sell)?;
                            coll_left = coll_left.saturating_sub(sell);
                            if realized_guard {
                                Self::ensure_realized_sell_within_slippage(
                                    netuid, raised, sold_alpha,
                                )?;
                            }
                        }
                        ensure!(raised >= slice_p, Error::<T>::InsufficientEscrow);
                        ensure!(slice_p <= max_payment, Error::<T>::SlippageExceeded);
                        T::Pool::transfer_tao(&escrow, &reserve, slice_p.into())?;
                        let surplus = raised.saturating_sub(slice_p);
                        if surplus > 0 {
                            T::Pool::transfer_tao(&escrow, &owner, surplus.into())?;
                        }
                        if coll_left > 0 {
                            T::Pool::ensure_hotkey_account(&hotkey)?;
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
                        Self::deposit_event(Event::Readded {
                            netuid,
                            tao: slice_p,
                            alpha: slice_lift_alpha,
                        });
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

            // Dust remainder already gated pre-trade via ensure_partial_repay_sizes;
            // keep a post-trade assert for defence in depth after interest/scale.
            if position.principal > 0 {
                let opening_value = Self::opening_value_of(
                    side,
                    position.principal,
                    T::Pool::current_alpha_price(netuid),
                )?;
                ensure!(
                    opening_value >= Self::minimum_loan_value(),
                    Error::<T>::AmountTooSmall
                );
            }

            let (credited_tao, credited_alpha) = match side {
                Side::Short => (slice_lift_tao, slice_p),
                Side::Long => (slice_p, slice_lift_alpha),
            };
            // N31: burn target uses the slice's lifted amounts, not credited
            // principal (same as full close / forfeit).
            Self::maybe_burn_invariant_surplus(
                netuid,
                credited_tao,
                credited_alpha,
                slice_lift_tao,
                slice_lift_alpha,
                Dissolutions::<T>::contains_key(netuid),
            )?;

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

        fn execute_buyback(
            escrow: &T::AccountId,
            custody: &T::AccountId,
            netuid: NetUid,
            principal: u64,
            budget: u64,
            realized_guard: bool,
        ) -> Result<(u64, u64), DispatchError> {
            let (payment, alpha) = Self::execute_buyback_best_effort(
                escrow,
                custody,
                netuid,
                principal,
                budget,
                realized_guard,
            )?;
            ensure!(alpha >= principal, Error::<T>::InsufficientEscrow);
            Ok((payment, alpha))
        }

        /// Like [`Self::execute_buyback`] but returns a partial fill when the
        /// budget or pool depth cannot cover `principal` (forfeit path).
        ///
        /// When `realized_guard` is set, every chunk checks cumulative TAO
        /// spent vs EMA·α·(1+band); a breach returns
        /// [`Error::SettlementPriceOutsideEmaBand`] so the outer
        /// `with_transaction` rolls the whole settle back.
        fn execute_buyback_best_effort(
            escrow: &T::AccountId,
            custody: &T::AccountId,
            netuid: NetUid,
            principal: u64,
            budget: u64,
            realized_guard: bool,
        ) -> Result<(u64, u64), DispatchError> {
            let mut payment = 0u64;
            let mut alpha = 0u64;
            for _ in 0..MAX_BUYBACK_STEPS {
                let remaining = principal.saturating_sub(alpha);
                if remaining == 0 {
                    return Ok((payment, alpha));
                }
                let needed = match Self::tao_to_buy_alpha(netuid, remaining) {
                    Ok(n) => n,
                    Err(_) => break,
                };
                let capacity = budget
                    .saturating_sub(payment)
                    .min(T::Pool::buy_spendable_tao(escrow).to_u64())
                    .min(T::Pool::max_buy_input(netuid).to_u64())
                    .min(needed);
                if capacity == 0 {
                    break;
                }
                let received = match T::Pool::buy_alpha(
                    escrow,
                    custody,
                    netuid,
                    capacity.into(),
                    u64::MAX.into(),
                    false,
                ) {
                    Ok(out) => out.to_u64(),
                    Err(_) => break,
                };
                payment = Self::add(payment, capacity)?;
                alpha = Self::add(alpha, received)?;
                if realized_guard {
                    Self::ensure_realized_buy_within_slippage(netuid, payment, alpha)?;
                }
                if received == 0 {
                    break;
                }
            }
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
                collateral_alpha.is_some() || borrow_tao.is_some(),
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
                Self::ensure_not_settlement_frozen(&owner, netuid)?;
                Self::charge_interest(&owner, netuid, &mut position, now, false, false, false)?;
                if Positions::<T>::contains_key(&owner, netuid) {
                    Positions::<T>::insert(&owner, netuid, &position);
                }
            }

            let (collateral, principal) = if let (Some(c), Some(p)) = (collateral_alpha, borrow_tao)
            {
                // Exact collateral + exact principal (lower-LTV / CLI
                // --collateral --ltv). Feasibility still gates LTV + capacity.
                ensure!(c > 0, Error::<T>::CollateralZero);
                ensure!(p > 0, Error::<T>::PrincipalZero);
                ensure!(c <= max_collateral_alpha, Error::<T>::CollateralExceedsMax);
                ensure!(p >= min_borrow_tao, Error::<T>::BelowMinimumBorrow);
                let old = Positions::<T>::get(&owner, netuid);
                let old_p = old.as_ref().map(|x| x.principal).unwrap_or(0);
                let total_p = Self::add(old_p, p)?;
                let total_c = Self::add(old.as_ref().map(|x| x.collateral).unwrap_or(0), c)?;
                let ltv = Self::ltv_tier_bps(netuid, Side::Long, &params);
                let delta_l = Self::lift_tao_for_principal(netuid, Side::Long, p)?;
                Self::ensure_side_capacity(netuid, Side::Long, delta_l)?;
                Self::ensure_long_book_pool_tao(netuid, p)?;
                ensure!(
                    Self::principal_feasible(
                        netuid,
                        Side::Long,
                        total_p,
                        total_c,
                        old_p,
                        Some(&owner),
                        &reference,
                        ltv,
                        &params,
                    )?,
                    Error::<T>::DynamicCollateral
                );
                (c, p)
            } else if let Some(c) = collateral_alpha {
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
                    return Err(Self::refuse_dust_or_capacity(
                        netuid,
                        Side::Long,
                        Self::coupon_price(netuid, &reference),
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
            let increasing = Positions::<T>::contains_key(&owner, netuid);
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
            let terms_version = Positions::<T>::get(&owner, netuid)
                .map(|p| p.terms_version)
                .unwrap_or(TERMS_VERSION_V1);
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
                increased: increasing,
                terms_version,
            });
            let _ = a_quote;
            Ok(())
        }

        /// Select the economic rules table for a live loan. v1 is the legacy
        /// one-year term; v2 is the hard max_loan_duration term (Jacob 2026-11-19).
        fn ensure_terms_version(version: u16) -> DispatchResult {
            match version {
                TERMS_VERSION_V1 | TERMS_VERSION_V2 => Ok(()),
                _ => Err(Error::<T>::UnsupportedTermsVersion.into()),
            }
        }

        /// Stable indexer id for the unique `(owner, netuid)` loan slot.
        fn loan_position_id(owner: &T::AccountId, netuid: NetUid) -> u64 {
            u64::from_le_bytes((owner, netuid).using_encoded(sp_io::hashing::twox_64))
        }

        /// Young-short age clock: open block, or the last increase that added
        /// principal (`last_exposure_added_at`). Do not derive from `expires_at`
        /// — increases keep the original term end, which would otherwise leave
        /// newly added exposure looking mature (Greg Zaitsev P1-b).
        fn position_opened_at(
            position: &Position<T::AccountId, BlockNumberFor<T>>,
        ) -> BlockNumberFor<T> {
            position.last_exposure_added_at
        }

        fn emit_loan_expired(
            owner: &T::AccountId,
            netuid: NetUid,
            side: Side,
            expires_at: BlockNumberFor<T>,
            settled_at: BlockNumberFor<T>,
        ) {
            Self::deposit_event(Event::LoanExpired {
                position_id: Self::loan_position_id(owner, netuid),
                owner: owner.clone(),
                netuid,
                side,
                expires_at,
                settled_at,
            });
        }

        fn interest_due(
            netuid: NetUid,
            position: &Position<T::AccountId, BlockNumberFor<T>>,
            until: BlockNumberFor<T>,
            final_charge: bool,
        ) -> Result<(u64, u64), DispatchError> {
            Self::ensure_terms_version(position.terms_version)?;
            // Accrue through `until` (including past `expires_at` while a
            // settlement-band deferral holds the loan open). Charge paths still
            // clamp the debit to remaining collateral, so a freeze cannot outrun
            // the coupon.
            let elapsed: u64 = until.saturating_sub(position.last_accrued).saturated_into();
            // Accrual year length is snapshotted on the position at open so a
            // later Config / terms bump cannot rewrite remaining interest.
            let denominator = u128::from(position.blocks_per_year);
            ensure!(denominator > 0, Error::<T>::Arithmetic);
            // Shorts: live utilization APR (O(1) from short_sold_alpha /
            // pool_alpha share). Longs: frozen annual_interest from open.
            let annual = Self::effective_annual_interest(netuid, position)?;
            let numerator = u128::from(annual)
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

        /// `permissionless`: when true (collectors), interest-exhaustion
        /// forfeit is gated by [`Self::ensure_settlement_ema_band`] and the
        /// forfeit AMM uses realized EMA slippage.
        /// `enforce_realized_slippage`: force realized chunk checks on any
        /// forfeit AMM (expiry path passes true even when not permissionless).
        fn charge_interest(
            owner: &T::AccountId,
            netuid: NetUid,
            position: &mut Position<T::AccountId, BlockNumberFor<T>>,
            until: BlockNumberFor<T>,
            final_charge: bool,
            permissionless: bool,
            enforce_realized_slippage: bool,
        ) -> DispatchResult {
            // Versioned interest path: v1 is the current burn / pending-sell
            // behaviour. Future versions branch here without touching v1.
            Self::ensure_terms_version(position.terms_version)?;
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
                            permissionless,
                            enforce_realized_slippage,
                        );
                    }
                }
            }
            // Past `expires_at`, interest still accrues while the position is
            // open (including band-deferred freezes), bounded by collateral.
            let (due, remainder) = Self::interest_due(netuid, position, until, final_charge)?;
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
                        Self::bump_stats(netuid, |s| {
                            s.interest_burned_tao =
                                s.interest_burned_tao.saturating_add(u128::from(credited));
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
                            Self::bump_stats(netuid, |s| {
                                s.interest_burned_alpha =
                                    s.interest_burned_alpha.saturating_add(u128::from(paid));
                            });
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
                                if *pending == 0 {
                                    LongInterestSince::<T>::insert(
                                        netuid,
                                        frame_system::Pallet::<T>::block_number(),
                                    );
                                }
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
            // Interest exhaustion (this tick drained C) or a prior
            // impact-capped forfeit left C at 0 with principal still open —
            // continue the forced unwind. Dissolution already re-added lift
            // legs and must never forfeit.
            if position.collateral == 0
                && position.principal > 0
                && !Dissolutions::<T>::contains_key(netuid)
            {
                // Permissionless / expiry forfeit uses realized slippage
                // unless past max deferral (force at market).
                let force_market = Self::past_max_settlement_deferral(owner, netuid);
                if permissionless && !force_market {
                    // Wall-clock now for deferral stamping; accrual `until`
                    // stays for interest math only (audit L4).
                    Self::ensure_settlement_ema_band(
                        owner,
                        netuid,
                        frame_system::Pallet::<T>::block_number(),
                    )?;
                }
                let fully_closed = Self::forfeit(
                    owner,
                    netuid,
                    position,
                    !force_market && (enforce_realized_slippage || permissionless),
                )?;
                // Partial unwind: refresh the caller's view so outer
                // collectors do not overwrite the shrunk storage.
                if !fully_closed {
                    if let Some(p) = Positions::<T>::get(owner, netuid) {
                        *position = p;
                    }
                }
            }
            Ok(())
        }

        /// v1 forfeit: buy back principal through the pool with escrow +
        /// collateral, weight-preserving two-sided readd, k-neutral burn.
        /// Leftover collateral after restoring the pool is burned (borrower
        /// still gets nothing). Uncovered principal records below-baseline
        /// shortfall via [`Self::maybe_burn_invariant_surplus`]. No one-sided
        /// readd.
        ///
        /// Returns `Ok(true)` when the position is fully removed, `Ok(false)`
        /// when only a [`ProtocolForcedImpact`]-capped slice cleared (remainder
        /// stays open for the next-block retry).
        ///
        /// `enforce_realized_slippage`: when true, every AMM chunk is gated by
        /// cumulative realized TAO vs EMA·α within
        /// [`LiftParams::expiry_max_slippage_bps`]. A breach rolls the settle
        /// back. Force-at-market after max deferral passes `false` so the
        /// per-trade band is skipped. Protocol-forced impact is **always**
        /// paced via [`Self::ensure_block_forced_impact_ok`] and recorded via
        /// [`Self::note_protocol_forced_impact`] (independent of the realized
        /// guard) so force-market forfeits cannot skip the per-block cap.
        fn forfeit(
            owner: &T::AccountId,
            netuid: NetUid,
            position: &Position<T::AccountId, BlockNumberFor<T>>,
            enforce_realized_slippage: bool,
        ) -> Result<bool, DispatchError> {
            Self::ensure_terms_version(position.terms_version)?;
            // Pace with prior protocol-forced trades this block (force-market
            // forfeits included). Cap already hit → Err → outer transactional
            // rollback + ExpiryRetry / InterestRetry requeue.
            Self::ensure_block_forced_impact_ok(netuid)?;
            let custody = Self::custody_hotkey()?;
            let reserve = Self::reserve_account(netuid);
            let escrow = position.custody.clone();
            let side = position.side;
            let principal = position.principal;
            // Force-market when the per-trade band is off (past max deferral).
            let force_market = !enforce_realized_slippage;

            // Pre-trade spot fair stamped for ProtocolForcedImpact on the
            // full-clear fallthrough (partials note and return above).
            let mut impact_fair_spot: Option<u64> = None;
            let (credited_tao, credited_alpha, leftover_collateral, buyback_tao, buyback_alpha) =
                match side {
                    Side::Short => {
                        // Cap this block's buyback to remaining ProtocolForcedImpact
                        // room (spot-relative; first trade included). Force-market
                        // guarantees forced_min_buyback_bps when impact admits 0.
                        let target =
                            Self::max_forced_buyback_alpha(netuid, principal, force_market)?;
                        ensure!(target > 0, Error::<T>::SettlementPriceOutsideEmaBand);
                        // Pro-rata debt / escrow / collateral for this slice.
                        // Best-effort fill may buy less than target when
                        // underwater; we still shrink by `target` and let
                        // invariant shortfall absorb the gap (same as a full
                        // underwater forfeit on this slice).
                        let slice_p = target;
                        let slice_c = Self::scale_part(position.collateral, slice_p, principal);
                        let slice_e = Self::scale_part(position.proceeds, slice_p, principal);
                        let slice_lift_tao =
                            Self::scale_part(position.lift_tao, slice_p, principal);
                        let slice_pot = slice_e.saturating_add(slice_c);
                        // Final slice stays terminal even when it cannot buy
                        // anything (record shortfall + remove); otherwise a
                        // dust / unfillable remainder retries every block.
                        let terminal = slice_p == principal;
                        ensure!(slice_pot > 0 || terminal, Error::<T>::InsufficientEscrow);
                        // Pre-trade spot fair for ProtocolForcedImpact (post-trade
                        // spot has moved).
                        let pre_spot = T::Pool::current_alpha_price(netuid);
                        let (paid, bought) = Self::execute_buyback_best_effort(
                            &escrow,
                            &custody,
                            netuid,
                            slice_p,
                            slice_pot,
                            enforce_realized_slippage,
                        )?;
                        let repaid_alpha = bought.min(slice_p);
                        ensure!(
                            repaid_alpha > 0 || terminal,
                            Error::<T>::SettlementPriceOutsideEmaBand
                        );
                        let mut burn_tao = slice_pot.saturating_sub(paid);
                        let excess = bought.saturating_sub(slice_p);
                        if excess > 0 {
                            if let Ok(dust) = T::Pool::sell_alpha(
                                &escrow,
                                &custody,
                                netuid,
                                excess.into(),
                                0u64.into(),
                                false,
                            ) {
                                burn_tao = burn_tao.saturating_add(dust.to_u64());
                            }
                        }
                        if burn_tao > 0 {
                            T::Pool::burn_interest_tao(&escrow, burn_tao.into())?;
                        }
                        if repaid_alpha > 0 {
                            T::Pool::transfer_staked_alpha(
                                &escrow,
                                &custody,
                                &reserve,
                                &custody,
                                netuid,
                                repaid_alpha.into(),
                                false,
                                false,
                            )?;
                        }
                        T::Pool::readd_and_credit(
                            netuid,
                            &reserve,
                            &custody,
                            slice_lift_tao.into(),
                            repaid_alpha.into(),
                        )?;
                        Self::deposit_event(Event::Readded {
                            netuid,
                            tao: slice_lift_tao,
                            alpha: repaid_alpha,
                        });
                        Lifts::<T>::mutate(netuid, |book| {
                            book.l_s = book.l_s.saturating_sub(slice_lift_tao);
                            book.held_tao = book.held_tao.saturating_sub(slice_lift_tao);
                            book.held_alpha = book.held_alpha.saturating_sub(Self::scale_part(
                                position.lift_alpha.saturating_sub(position.principal),
                                slice_p,
                                principal,
                            ));
                            book.escrow_tao = book.escrow_tao.saturating_sub(slice_e);
                            book.short_sold_alpha = book.short_sold_alpha.saturating_sub(slice_p);
                        });
                        // Partial: shrink the live position and stop (caller
                        // requeues at ForcedSettlementRetryDelay). Full: fall
                        // through to remove_position below.
                        if slice_p < principal {
                            let slice_lift_alpha =
                                Self::scale_part(position.lift_alpha, slice_p, principal);
                            Self::maybe_burn_invariant_surplus(
                                netuid,
                                slice_lift_tao,
                                repaid_alpha,
                                slice_lift_tao,
                                slice_lift_alpha,
                                Dissolutions::<T>::contains_key(netuid),
                            )?;
                            let fair = if repaid_alpha > 0 {
                                Self::tao_for_alpha(repaid_alpha, pre_spot, true)?
                            } else {
                                0
                            };
                            Self::note_protocol_forced_impact(netuid, fair, paid);
                            // Shrink the caller's `position`, not storage:
                            // callers charge interest on their local copy
                            // before forfeiting, and storage has not seen
                            // that debit (collateral, last_accrued) yet.
                            let mut shrunk = position.clone();
                            shrunk.principal = shrunk.principal.saturating_sub(slice_p);
                            shrunk.collateral = shrunk.collateral.saturating_sub(slice_c);
                            shrunk.proceeds = shrunk.proceeds.saturating_sub(slice_e);
                            shrunk.lift_tao = shrunk.lift_tao.saturating_sub(slice_lift_tao);
                            shrunk.lift_alpha = shrunk.lift_alpha.saturating_sub(slice_lift_alpha);
                            shrunk.annual_interest = Self::scale_part(
                                shrunk.annual_interest,
                                shrunk.principal,
                                principal,
                            );
                            Positions::<T>::insert(owner, netuid, shrunk);
                            Self::bump_stats(netuid, |s| {
                                s.forfeit_buyback_tao =
                                    s.forfeit_buyback_tao.saturating_add(u128::from(paid));
                                s.forfeit_buyback_alpha = s
                                    .forfeit_buyback_alpha
                                    .saturating_add(u128::from(repaid_alpha));
                            });
                            return Ok(false);
                        }
                        impact_fair_spot = Some(if repaid_alpha > 0 {
                            Self::tao_for_alpha(repaid_alpha, pre_spot, true)?
                        } else {
                            0
                        });
                        (slice_lift_tao, repaid_alpha, burn_tao, paid, repaid_alpha)
                    }
                    Side::Long => {
                        // Sell A, then as much of C as needed (or all of C), repay
                        // min(raised, P); burn leftover TAO and leftover alpha C.
                        let mut raised = 0u64;
                        let mut sold_alpha = 0u64;
                        let skip_flow = Self::skip_long_tao_flow(netuid);
                        if position.escrow_alpha > 0 {
                            let out = T::Pool::forced_sell_into_escrow(
                                &escrow,
                                &custody,
                                &escrow,
                                netuid,
                                position.escrow_alpha.into(),
                                0u64.into(),
                                skip_flow,
                            )?;
                            raised = Self::add(raised, out.to_u64())?;
                            sold_alpha = Self::add(sold_alpha, position.escrow_alpha)?;
                            if enforce_realized_slippage {
                                Self::ensure_realized_sell_within_slippage(
                                    netuid, raised, sold_alpha,
                                )?;
                            }
                        }
                        let mut coll_left = position.collateral;
                        if raised < principal && coll_left > 0 {
                            let need = principal.saturating_sub(raised);
                            let sell = match Self::alpha_to_raise_tao(netuid, need) {
                                Ok(needed_alpha) => needed_alpha.min(coll_left),
                                Err(_) => coll_left,
                            };
                            if sell > 0 {
                                match T::Pool::forced_sell_into_escrow(
                                    &escrow,
                                    &custody,
                                    &escrow,
                                    netuid,
                                    sell.into(),
                                    0u64.into(),
                                    skip_flow,
                                ) {
                                    Ok(out) => {
                                        raised = Self::add(raised, out.to_u64())?;
                                        sold_alpha = Self::add(sold_alpha, sell)?;
                                        coll_left = coll_left.saturating_sub(sell);
                                        if enforce_realized_slippage {
                                            Self::ensure_realized_sell_within_slippage(
                                                netuid, raised, sold_alpha,
                                            )?;
                                        }
                                    }
                                    Err(e) if T::Pool::quote_may_round_to_zero(&e) => {
                                        // Dust C cannot raise TAO — burn it below.
                                    }
                                    Err(e) => return Err(e),
                                }
                            }
                        }
                        let repay = raised.min(principal);
                        let surplus_tao = raised.saturating_sub(repay);
                        if repay > 0 {
                            T::Pool::transfer_tao(&escrow, &reserve, repay.into())?;
                        }
                        if surplus_tao > 0 {
                            T::Pool::burn_interest_tao(&escrow, surplus_tao.into())?;
                        }
                        if coll_left > 0 {
                            T::Pool::destroy_custody_alpha(
                                &escrow,
                                &custody,
                                netuid,
                                coll_left.into(),
                            )?;
                        }
                        T::Pool::readd_and_credit(
                            netuid,
                            &reserve,
                            &custody,
                            repay.into(),
                            position.lift_alpha.into(),
                        )?;
                        Self::deposit_event(Event::Readded {
                            netuid,
                            tao: repay,
                            alpha: position.lift_alpha,
                        });
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
                        (repay, position.lift_alpha, coll_left, repay, sold_alpha)
                    }
                };

            // N31: target = L(pre) + L(position.lift_*). Credited may be a
            // partial buyback (repaid_alpha < principal); burning against that
            // as L(lift) deepens k below the no-loan level.
            Self::maybe_burn_invariant_surplus(
                netuid,
                credited_tao,
                credited_alpha,
                position.lift_tao,
                position.lift_alpha,
                Dissolutions::<T>::contains_key(netuid),
            )?;
            // Always record forfeit AMM impact (force-market path included).
            // Prefer pre-trade spot fair stamped above; fall back to current
            // spot × α for long sells (impact of sells is secondary).
            let fair = match impact_fair_spot {
                Some(f) => f,
                None if buyback_alpha > 0 => {
                    Self::fair_spot_tao(netuid, buyback_alpha, side == Side::Short)?
                }
                None => 0,
            };
            Self::note_protocol_forced_impact(netuid, fair, buyback_tao);
            let principal_lost = principal;
            Self::remove_position(owner, netuid, position);
            Self::deposit_event(Event::Forfeited {
                owner: owner.clone(),
                netuid,
                side,
                principal_lost,
            });
            Self::bump_stats(netuid, |s| {
                s.forfeit_count = s.forfeit_count.saturating_add(1);
                s.forfeit_buyback_tao = s
                    .forfeit_buyback_tao
                    .saturating_add(u128::from(buyback_tao));
                s.forfeit_buyback_alpha = s
                    .forfeit_buyback_alpha
                    .saturating_add(u128::from(buyback_alpha));
                match side {
                    Side::Short => {
                        s.forfeited_collateral_tao = s
                            .forfeited_collateral_tao
                            .saturating_add(u128::from(leftover_collateral));
                    }
                    Side::Long => {
                        s.forfeited_collateral_alpha = s
                            .forfeited_collateral_alpha
                            .saturating_add(u128::from(leftover_collateral));
                    }
                }
            });
            Ok(true)
        }

        /// Saturating bump of per-netuid and global lending health counters.
        fn bump_stats(netuid: NetUid, update: impl Fn(&mut LendingStats)) {
            StatsByNetuid::<T>::mutate(netuid, |stats| update(stats));
            StatsGlobal::<T>::mutate(|stats| update(stats));
        }

        /// Read per-netuid cumulative lending health counters.
        pub fn lending_stats(netuid: NetUid) -> LendingStats {
            StatsByNetuid::<T>::get(netuid)
        }

        /// Read chain-wide cumulative lending health counters.
        pub fn lending_stats_global() -> LendingStats {
            StatsGlobal::<T>::get()
        }

        /// After every position readd: burn surplus so Balancer `L` returns to
        /// the per-loan target `L(pre) + L(lift)` (Jacob 2026-11-03).
        ///
        /// - `credited_*`: reserves just credited by this readd (burn floor /
        ///   cap — never burn past the credit).
        /// - `lift_*`: this position's (or repay slice's) **lifted** amounts —
        ///   never the credited buyback legs (N31). Using credited alpha when
        ///   a forfeit under-recovers under-sizes the target and burns the
        ///   pool below the no-loan level. Summed lifts across increases at
        ///   different prices slightly under-burn versus per-increase `L`
        ///   (safe; see spec).
        /// - `skip_burn`: dissolution paths skip (subnet winding down).
        ///
        /// No pool-wide idle snapshot: every close / repay / expiry / forfeit
        /// burns against its own target, so a busy book cannot accumulate
        /// surplus and emission growth between open and close is not burned.
        /// A forfeit must never end below baseline because of the burn itself
        /// — under-recovery records shortfall instead. N30: `|L−L*| / L < 1e-8`
        /// is treated as zero (no dust burn / no dust shortfall) inside the
        /// k-neutral helpers.
        fn maybe_burn_invariant_surplus(
            netuid: NetUid,
            credited_tao: u64,
            credited_alpha: u64,
            lift_tao: u64,
            lift_alpha: u64,
            skip_burn: bool,
        ) -> DispatchResult {
            if skip_burn || (credited_tao == 0 && credited_alpha == 0) {
                return Ok(());
            }
            let y = T::Pool::pool_tao(netuid).to_u64();
            let x = T::Pool::pool_alpha(netuid).to_u64();
            let floor_y = y.saturating_sub(credited_tao);
            let floor_x = x.saturating_sub(credited_alpha);
            let w = T::Pool::quote_weight_parts(netuid);
            let Some((base_tao, base_alpha)) =
                subtensor_swap_interface::loan_target_baseline_reserves(
                    floor_y, floor_x, lift_tao, lift_alpha, w,
                )
            else {
                return Ok(());
            };
            let (burn_t, burn_a) = T::Pool::burn_amounts_to_k_baseline(
                y.into(),
                x.into(),
                w,
                base_tao.into(),
                base_alpha.into(),
                floor_y.into(),
                floor_x.into(),
            );
            let burn_t = burn_t.to_u64().min(credited_tao);
            let burn_a = burn_a.to_u64().min(credited_alpha);
            if burn_t > 0 || burn_a > 0 {
                T::Pool::burn_protocol_reserves(netuid, burn_t.into(), burn_a.into())?;
                Self::deposit_event(Event::InvariantSurplusBurned {
                    netuid,
                    tao: burn_t,
                    alpha: burn_a,
                });

                Self::bump_stats(netuid, |s| {
                    s.k_neutral_surplus_burned_tao = s
                        .k_neutral_surplus_burned_tao
                        .saturating_add(u128::from(burn_t));
                    s.k_neutral_surplus_burned_alpha = s
                        .k_neutral_surplus_burned_alpha
                        .saturating_add(u128::from(burn_a));
                });
            }

            // N24: real invariant shortfall vs the per-loan target, valued in
            // TAO. Measure after the burn so a successful restore records 0.
            let y_after = y.saturating_sub(burn_t);
            let x_after = x.saturating_sub(burn_a);
            let shortfall_tao = subtensor_swap_interface::invariant_shortfall_tao(
                y_after, x_after, w, base_tao, base_alpha,
            );
            if shortfall_tao > 0 {
                Self::bump_stats(netuid, |s| {
                    s.pool_value_given_below_baseline_tao = s
                        .pool_value_given_below_baseline_tao
                        .saturating_add(u128::from(shortfall_tao));
                });
            }
            Ok(())
        }

        fn remove_position(
            owner: &T::AccountId,
            netuid: NetUid,
            position: &Position<T::AccountId, BlockNumberFor<T>>,
        ) {
            Positions::<T>::remove(owner, netuid);
            LastDeferral::<T>::remove(owner, netuid);
            BandDeferralSince::<T>::remove(owner, netuid);
            OpenByNetuid::<T>::remove(netuid, owner);
            EscrowOwner::<T>::remove(netuid, &position.custody);
            if Due::<T>::take(position.due, (owner, netuid)).is_some() {
                Self::note_due_removed(position.due);
            }
            let _ = Self::take_interest_retry(position.due, owner, netuid);
            if Expiry::<T>::take(position.expiry_due, (owner, netuid)).is_some() {
                Self::note_expiry_removed(position.expiry_due);
            }
            let _ = Self::take_expiry_retry(position.expiry_due, owner, netuid);
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
                NextInterestRetry::<T>::kill();
                InterestRetryPending::<T>::kill();
                InterestRetryCursor::<T>::kill();
                NextExpiryRetry::<T>::kill();
                ExpiryRetryPending::<T>::kill();
                ExpiryRetryCursor::<T>::kill();
            }
        }

        /// O(1) enqueue onto the interest-retry side queue (Skeptic fid:0776a829).
        /// Human-readable deferral reason for events (E2E N14 / N17): prefer
        /// `Pallet::ErrorName` for Module errors. This pallet decodes its own
        /// Error enum; foreign modules use a compact `Pallet::{idx|name}` form
        /// (production indices) so clients can expand via metadata. Avoid the
        /// Debug dump of `Module { index, error: [...] }`, which truncates past
        /// 64 bytes before the error index is readable.
        fn deferral_reason(error: DispatchError) -> BoundedVec<u8, ConstU32<64>> {
            use frame_support::traits::PalletInfoAccess;
            let label = match error {
                DispatchError::Module(module) => {
                    let self_index = <Pallet<T> as PalletInfoAccess>::index() as u8;
                    if module.index == self_index {
                        match Error::<T>::decode(&mut &module.error[..]) {
                            Ok(e) => alloc::format!("{e:?}"),
                            Err(_) => alloc::format!("Lending::{}", module.error[0]),
                        }
                    } else {
                        let pallet = Self::module_pallet_label(module.index);
                        if let Some(msg) = module.message {
                            alloc::format!("{pallet}::{msg}")
                        } else {
                            // Compact; SDK expands `Pallet::{code}` via ERRORS.
                            alloc::format!("{pallet}::{}", module.error[0])
                        }
                    }
                }
                DispatchError::Token(t) => alloc::format!("Token.{t:?}"),
                DispatchError::Arithmetic(a) => alloc::format!("Arithmetic.{a:?}"),
                DispatchError::Transactional(t) => alloc::format!("Transactional.{t:?}"),
                DispatchError::BadOrigin => alloc::string::String::from("BadOrigin"),
                DispatchError::CannotLookup => alloc::string::String::from("CannotLookup"),
                other => alloc::format!("{other:?}"),
            };
            BoundedVec::<u8, ConstU32<64>>::truncate_from(label.into_bytes())
        }

        /// Production `construct_runtime!` pallet names for foreign Module
        /// errors (N17). Unknown indices stay numeric so the SDK can still
        /// resolve `(index, err)` via generated metadata.
        fn module_pallet_label(index: u8) -> alloc::string::String {
            match index {
                5 => alloc::string::String::from("Balances"),
                7 => alloc::string::String::from("SubtensorModule"),
                28 => alloc::string::String::from("Swap"),
                33 => alloc::string::String::from("Lending"),
                other => alloc::format!("{other}"),
            }
        }

        fn enqueue_interest_retry(
            owner: &T::AccountId,
            netuid: NetUid,
            retry_at: BlockNumberFor<T>,
        ) {
            if !InterestRetry::<T>::contains_key(retry_at, (owner, netuid)) {
                InterestRetryPending::<T>::mutate(|c| *c = c.saturating_add(1));
            }
            InterestRetry::<T>::insert(retry_at, (owner, netuid), ());
            match NextInterestRetry::<T>::get() {
                Some(n) if n <= retry_at => {}
                _ => NextInterestRetry::<T>::put(retry_at),
            }
        }

        fn take_interest_retry(
            at: BlockNumberFor<T>,
            owner: &T::AccountId,
            netuid: NetUid,
        ) -> bool {
            if InterestRetry::<T>::take(at, (owner, netuid)).is_some() {
                let left = InterestRetryPending::<T>::mutate(|c| {
                    *c = c.saturating_sub(1);
                    *c
                });
                if left == 0 {
                    NextInterestRetry::<T>::kill();
                    InterestRetryCursor::<T>::kill();
                }
                true
            } else {
                false
            }
        }

        /// O(1) enqueue onto the expiry-retry side queue (Skeptic fid:0776a829).
        fn enqueue_expiry_retry(owner: &T::AccountId, netuid: NetUid, retry_at: BlockNumberFor<T>) {
            if !ExpiryRetry::<T>::contains_key(retry_at, (owner, netuid)) {
                ExpiryRetryPending::<T>::mutate(|c| *c = c.saturating_add(1));
            }
            ExpiryRetry::<T>::insert(retry_at, (owner, netuid), ());
            match NextExpiryRetry::<T>::get() {
                Some(n) if n <= retry_at => {}
                _ => NextExpiryRetry::<T>::put(retry_at),
            }
        }

        /// One-shot max_loan_duration rewrite used by
        /// [`migrations::migrate_position_max_duration`].
        pub(crate) fn migrate_position_max_duration(
            migration_name: BoundedVec<u8, super::MigrationKeyMaxLen>,
        ) -> Weight {
            use codec::DecodeAll;
            use frame_support::storage::unhashed;
            use migrations::PositionBeforeExposure;
            use sp_runtime::traits::Zero;

            let mut weight = T::DbWeight::get().reads(1);
            if HasMigrationRun::<T>::get(&migration_name) {
                return weight;
            }

            let now = frame_system::Pallet::<T>::block_number();
            let term = Self::max_loan_duration();
            let new_expires = now.saturating_add(term);
            weight = weight.saturating_add(T::DbWeight::get().reads(1));

            let keys: alloc::vec::Vec<_> = Positions::<T>::iter_keys().collect();
            weight = weight.saturating_add(T::DbWeight::get().reads(keys.len() as u64));

            // Drain the legacy expiry linked list first, then rewrite every
            // position onto `new_expires` and rebuild a single-bucket index.
            // Bounded mid-list insert must never run during upgrade (skeptic
            // fid:a2a459cf — `.expect` on a congested staging book panics).
            weight = weight.saturating_add(Self::drain_expiry_schedule());

            let mut migrated = 0u32;
            for (owner, netuid) in keys {
                let key = Positions::<T>::hashed_key_for(&owner, netuid);
                let Some(bytes) = unhashed::get_raw(&key) else {
                    continue;
                };
                // Prefer the pre-exposure layout when present; otherwise decode
                // the current Position (already has last_exposure_added_at).
                let mut position = if let Ok(old) = PositionBeforeExposure::<
                    T::AccountId,
                    BlockNumberFor<T>,
                >::decode_all(&mut &bytes[..])
                {
                    old.into_position::<T>()
                } else if let Ok(cur) =
                    Position::<T::AccountId, BlockNumberFor<T>>::decode_all(&mut &bytes[..])
                {
                    cur
                } else {
                    continue;
                };
                weight = weight.saturating_add(T::DbWeight::get().reads(1));

                let _ = Expiry::<T>::take(position.expiry_due, (&owner, netuid));
                let _ = Self::take_expiry_retry(position.expiry_due, &owner, netuid);

                // Preserve the young-short exposure clock across the expires_at
                // rewrite (Greg Zaitsev P1-b). Only seed from the old term end
                // when the field is still zero (pre-exposure rows).
                if position.last_exposure_added_at.is_zero() {
                    let prior_term = match position.terms_version {
                        TERMS_VERSION_V2 => Self::max_loan_duration(),
                        _ => position.blocks_per_year.saturated_into(),
                    };
                    position.last_exposure_added_at =
                        position.expires_at.saturating_sub(prior_term);
                }

                position.terms_version = TERMS_VERSION_V2;
                position.expires_at = new_expires;
                position.expiry_due = new_expires;
                Positions::<T>::insert(&owner, netuid, &position);
                Expiry::<T>::insert(new_expires, (&owner, netuid), ());
                migrated = migrated.saturating_add(1);
                BandDeferralSince::<T>::remove(&owner, netuid);
                LastDeferral::<T>::remove(&owner, netuid);
                weight = weight.saturating_add(T::DbWeight::get().reads_writes(3, 4));
            }

            if migrated > 0 {
                ExpiryIndex::<T>::insert(new_expires, migrated);
                Self::set_next_expiry_head(Some(new_expires));
                ExpiryTail::<T>::put(new_expires);
                weight = weight.saturating_add(T::DbWeight::get().writes(3));
            }

            CurrentTermsVersion::<T>::put(TERMS_VERSION_V2);
            HasMigrationRun::<T>::insert(&migration_name, true);
            weight.saturating_add(T::DbWeight::get().writes(2))
        }

        /// Drop the expiry linked-list metadata (head/tail/next/prev/index).
        /// Position `Expiry` map entries are left for the caller to rewrite.
        fn drain_expiry_schedule() -> Weight {
            let mut weight = T::DbWeight::get().reads(1);
            let Some(at) = NextExpiry::<T>::take() else {
                ExpiryTail::<T>::kill();
                return weight.saturating_add(T::DbWeight::get().writes(1));
            };
            ExpiryTail::<T>::kill();
            weight = weight.saturating_add(T::DbWeight::get().writes(2));
            let mut next = Some(at);
            while let Some(cur) = next {
                ExpiryIndex::<T>::remove(cur);
                ExpiryPrev::<T>::remove(cur);
                next = ExpiryNext::<T>::take(cur);
                weight = weight.saturating_add(T::DbWeight::get().reads_writes(1, 3));
            }
            weight
        }

        fn take_expiry_retry(at: BlockNumberFor<T>, owner: &T::AccountId, netuid: NetUid) -> bool {
            if ExpiryRetry::<T>::take(at, (owner, netuid)).is_some() {
                let left = ExpiryRetryPending::<T>::mutate(|c| {
                    *c = c.saturating_sub(1);
                    *c
                });
                if left == 0 {
                    NextExpiryRetry::<T>::kill();
                    ExpiryRetryCursor::<T>::kill();
                }
                true
            } else {
                false
            }
        }

        /// Advance or clear the interest due-queue head. Always kills
        /// [`DueBucketCursor`]: the cursor is exclusive to the previous head
        /// bucket, and applying it to a successor skips earlier keys then
        /// orphans them when an empty-looking drain unlinks the bucket
        /// (Skeptic fid:2d4a1d05).
        fn set_next_due_head(new_head: Option<BlockNumberFor<T>>) {
            match new_head {
                Some(n) => NextDue::<T>::put(n),
                None => NextDue::<T>::kill(),
            }
            DueBucketCursor::<T>::kill();
        }

        fn unlink_due_bucket(due: BlockNumberFor<T>) {
            let prev = DuePrev::<T>::take(due);
            let next = DueNext::<T>::take(due);
            // Keep a mid-rebuild cursor on the live list (skeptic fid:9c383f98).
            if DueTailRebuildCursor::<T>::get() == Some(due) {
                match next {
                    Some(n) => DueTailRebuildCursor::<T>::put(n),
                    None => DueTailRebuildCursor::<T>::kill(),
                }
            }
            match (prev, next) {
                (None, None) => {
                    if NextDue::<T>::get() == Some(due) {
                        Self::set_next_due_head(None);
                    }
                    if DueTail::<T>::get() == Some(due) {
                        DueTail::<T>::kill();
                    }
                }
                (None, Some(n)) => {
                    DuePrev::<T>::remove(n);
                    if NextDue::<T>::get() == Some(due) {
                        Self::set_next_due_head(Some(n));
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
                    Self::set_next_due_head(Some(due));
                    DueTail::<T>::put(due);
                }
                (Some(head), _) if due < head => {
                    DueNext::<T>::insert(due, head);
                    DuePrev::<T>::insert(head, due);
                    // New head — drop any cursor belonging to the old head.
                    Self::set_next_due_head(Some(due));
                }
                (_, Some(tail)) if due > tail => {
                    DueNext::<T>::insert(tail, due);
                    DuePrev::<T>::insert(due, tail);
                    DueTail::<T>::put(due);
                }
                (Some(head), _) => {
                    // Rare out-of-order insert (e.g. clock skew / period change).
                    let mut prev = head;
                    'insert: {
                        while let Some(n) = DueNext::<T>::get(prev) {
                            if n < due {
                                prev = n;
                                continue;
                            }
                            DueNext::<T>::insert(prev, due);
                            DuePrev::<T>::insert(due, prev);
                            DueNext::<T>::insert(due, n);
                            DuePrev::<T>::insert(n, due);
                            break 'insert;
                        }
                        DueNext::<T>::insert(prev, due);
                        DuePrev::<T>::insert(due, prev);
                        DueTail::<T>::put(due);
                    }
                }
            }
        }

        /// Advance or clear the expiry-queue head. Always kills
        /// [`ExpiryBucketCursor`] for the same reason as [`Self::set_next_due_head`]
        /// (Skeptic fid:2d4a1d05).
        fn set_next_expiry_head(new_head: Option<BlockNumberFor<T>>) {
            match new_head {
                Some(n) => NextExpiry::<T>::put(n),
                None => NextExpiry::<T>::kill(),
            }
            ExpiryBucketCursor::<T>::kill();
        }

        /// Persist resume state after a mid-bucket stop in [`Self::collect_expiry`].
        ///
        /// Settling the last row of bucket `at` unlinks it and advances
        /// [`NextExpiry`] (via [`Self::note_expiry_removed`]). If the meter then
        /// fails the trailing peek, a naive `NextExpiry::put(at)` would resurrect
        /// the detached bucket, drop the successor, and strand later expiries
        /// (skeptic fid:eb66f29d). When the live head is no longer `at`, keep it.
        fn resume_expiry_collect(at: BlockNumberFor<T>, last_done: Option<(T::AccountId, NetUid)>) {
            if NextExpiry::<T>::get() != Some(at) {
                // Unlinked / advanced (or cleared). Cursor already killed by
                // `set_next_expiry_head`.
                return;
            }
            if let Some(done) = last_done {
                ExpiryBucketCursor::<T>::put(done);
            }
        }

        fn unlink_expiry_bucket(at: BlockNumberFor<T>) {
            let prev = ExpiryPrev::<T>::take(at);
            let next = ExpiryNext::<T>::take(at);
            match (prev, next) {
                (None, None) => {
                    if NextExpiry::<T>::get() == Some(at) {
                        Self::set_next_expiry_head(None);
                    }
                    if ExpiryTail::<T>::get() == Some(at) {
                        ExpiryTail::<T>::kill();
                    }
                }
                (None, Some(n)) => {
                    ExpiryPrev::<T>::remove(n);
                    if NextExpiry::<T>::get() == Some(at) {
                        Self::set_next_expiry_head(Some(n));
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

        fn note_expiry_inserted(at: BlockNumberFor<T>) -> DispatchResult {
            let was_empty = ExpiryIndex::<T>::get(at) == 0;
            ExpiryIndex::<T>::mutate(at, |count| *count = count.saturating_add(1));
            if !was_empty {
                return Ok(());
            }
            match (NextExpiry::<T>::get(), ExpiryTail::<T>::get()) {
                (None, _) => {
                    Self::set_next_expiry_head(Some(at));
                    ExpiryTail::<T>::put(at);
                }
                (Some(head), _) if at < head => {
                    ExpiryNext::<T>::insert(at, head);
                    ExpiryPrev::<T>::insert(head, at);
                    Self::set_next_expiry_head(Some(at));
                }
                (_, Some(tail)) if at > tail => {
                    ExpiryNext::<T>::insert(tail, at);
                    ExpiryPrev::<T>::insert(at, tail);
                    ExpiryTail::<T>::put(at);
                }
                (Some(head), _) => {
                    // Mid-list insert (shorter term ahead of many later
                    // deadlines). Bound the walk so open/roll stay inside a
                    // fixed weight envelope (skeptic fid:600d74dc).
                    let mut prev = head;
                    let mut walked = 0u32;
                    // Bare loop: each step must charge the walk budget before
                    // reading Next (including the terminal None that appends).
                    loop {
                        ensure!(
                            walked < MAX_EXPIRY_BUCKET_WALK,
                            Error::<T>::ExpiryScheduleCongested
                        );
                        walked = walked.saturating_add(1);
                        let Some(n) = ExpiryNext::<T>::get(prev) else {
                            ExpiryNext::<T>::insert(prev, at);
                            ExpiryPrev::<T>::insert(at, prev);
                            ExpiryTail::<T>::put(at);
                            break;
                        };
                        if n < at {
                            prev = n;
                            continue;
                        }
                        ExpiryNext::<T>::insert(prev, at);
                        ExpiryPrev::<T>::insert(at, prev);
                        ExpiryNext::<T>::insert(at, n);
                        ExpiryPrev::<T>::insert(n, at);
                        break;
                    }
                }
            }
            Ok(())
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

        /// Charge one interest-due entry and dequeue / requeue by `source`.
        ///
        /// Intentional Source differences (not drift):
        /// - Scheduled success unlinks `Due` via `note_due_removed`; Retry
        ///   drops the side-queue row via `take_interest_retry` then rejoins
        ///   `Due` through `note_due_inserted`.
        /// - On failure both paths enqueue InterestRetry, but Scheduled also
        ///   removes the Due row so NextDue cannot pin an empty bucket.
        fn process_one_interest(
            source: CollectSource,
            now: BlockNumberFor<T>,
            at: BlockNumberFor<T>,
            owner: &T::AccountId,
            netuid: NetUid,
        ) {
            use frame_support::storage::{TransactionOutcome, with_transaction};
            let outcome = with_transaction(|| {
                let result = (|| {
                    let mut position =
                        Positions::<T>::get(owner, netuid).ok_or(Error::<T>::PositionMissing)?;
                    if Dissolutions::<T>::contains_key(netuid) {
                        return Ok(());
                    }
                    Self::charge_interest(owner, netuid, &mut position, now, false, true, false)?;
                    // `charge_interest` mutates `position` (collateral,
                    // last_accrued) and refreshes it after a partial forfeit,
                    // so the local is authoritative while the loan is open.
                    if Positions::<T>::contains_key(owner, netuid) {
                        // Zero collateral + open principal = interest-exhaustion
                        // unwind still in progress (impact-capped slice).
                        if position.collateral == 0 {
                            match source {
                                CollectSource::Scheduled => {
                                    if Due::<T>::take(at, (owner, netuid)).is_some() {
                                        Self::note_due_removed(at);
                                    }
                                }
                                CollectSource::Retry => {
                                    let _ = Self::take_interest_retry(at, owner, netuid);
                                }
                            }
                            let retry_at = now.saturating_add(T::ForcedSettlementRetryDelay::get());
                            Self::enqueue_interest_retry(owner, netuid, retry_at);
                            position.due = retry_at;
                            Positions::<T>::insert(owner, netuid, position);
                            return Ok(());
                        }
                        let next = now.saturating_add(position.interest_period);
                        match source {
                            CollectSource::Scheduled => {
                                if Due::<T>::take(position.due, (owner, netuid)).is_some() {
                                    Self::note_due_removed(position.due);
                                }
                            }
                            CollectSource::Retry => {
                                let _ = Self::take_interest_retry(position.due, owner, netuid);
                            }
                        }
                        position.due = next;
                        Due::<T>::insert(next, (owner, netuid), ());
                        Self::note_due_inserted(next);
                        Positions::<T>::insert(owner, netuid, position);
                    }
                    Ok(())
                })();
                match result {
                    Ok(()) => TransactionOutcome::Commit(Ok::<(), DispatchError>(())),
                    Err(e) => TransactionOutcome::Rollback(Err(e)),
                }
            });
            match outcome {
                Ok(()) => {
                    match source {
                        CollectSource::Scheduled => {
                            // Forfeit / dissolve / partial-unwind skips leave
                            // the queue row; drop it. Successful charge already
                            // moved a surviving loan onto the next period.
                            if Due::<T>::contains_key(at, (owner, netuid)) {
                                Due::<T>::remove(at, (owner, netuid));
                                Self::note_due_removed(at);
                            }
                        }
                        CollectSource::Retry => {
                            let _ = Self::take_interest_retry(at, owner, netuid);
                        }
                    }
                    // Partial forced unwind: keep deferral stamp and emit so
                    // operators see next-block progress (not a clean accrual).
                    if let Some(position) = Positions::<T>::get(owner, netuid) {
                        if position.collateral == 0 {
                            let reason =
                                BoundedVec::truncate_from(b"forced_impact_partial".to_vec());
                            LastDeferral::<T>::insert(
                                owner,
                                netuid,
                                (0u8, position.due, reason.clone()),
                            );
                            Self::deposit_event(Event::InterestDeferred {
                                owner: owner.clone(),
                                netuid,
                                retry_at: position.due,
                                reason,
                            });
                            return;
                        }
                    }
                    LastDeferral::<T>::remove(owner, netuid);
                }
                Err(error) => {
                    match source {
                        CollectSource::Scheduled => {
                            // Preserve coverage without pinning NextDue /
                            // mid-inserting into the Due schedule (Auditor
                            // fid:2aff7381, Skeptic fid:0776a829). Side-queue
                            // insert is O(1).
                            if Due::<T>::contains_key(at, (owner, netuid)) {
                                Due::<T>::remove(at, (owner, netuid));
                                Self::note_due_removed(at);
                            }
                        }
                        CollectSource::Retry => {
                            let _ = Self::take_interest_retry(at, owner, netuid);
                        }
                    }
                    Self::note_band_deferral(owner, netuid, now, error);
                    if let Some(mut position) = Positions::<T>::get(owner, netuid) {
                        // Impact-budget hit after max deferral → next block;
                        // EMA band deferral keeps the longer InterestRetryDelay.
                        let impact_err: DispatchError =
                            Error::<T>::SettlementPriceOutsideEmaBand.into();
                        let delay = if error == impact_err
                            && Self::past_max_settlement_deferral(owner, netuid)
                        {
                            T::ForcedSettlementRetryDelay::get()
                        } else {
                            T::InterestRetryDelay::get()
                        };
                        let retry_at = now.saturating_add(delay);
                        Self::enqueue_interest_retry(owner, netuid, retry_at);
                        position.due = retry_at;
                        Positions::<T>::insert(owner, netuid, position);
                        let reason = Self::deferral_reason(error);
                        LastDeferral::<T>::insert(owner, netuid, (0u8, retry_at, reason.clone()));
                        Self::deposit_event(Event::InterestDeferred {
                            owner: owner.clone(),
                            netuid,
                            retry_at,
                            reason,
                        });
                    }
                }
            }
        }

        /// Settle one expiry entry and dequeue / requeue by `source`.
        ///
        /// Intentional Source differences (not drift):
        /// - Scheduled removes the Expiry linked-list row (+ `note_expiry_removed`);
        ///   Retry drops the side-queue row via `take_expiry_retry`.
        /// - On failure both enqueue ExpiryRetry; Scheduled also clears Expiry
        ///   so NextExpiry cannot pin an empty bucket.
        fn process_one_expiry(
            source: CollectSource,
            now: BlockNumberFor<T>,
            at: BlockNumberFor<T>,
            owner: &T::AccountId,
            netuid: NetUid,
        ) {
            use frame_support::storage::{TransactionOutcome, with_transaction};
            let outcome = with_transaction(|| {
                let result = (|| {
                    // Dissolution settle is authoritative if it started first.
                    if Dissolutions::<T>::contains_key(netuid) {
                        return Ok(());
                    }
                    let position =
                        Positions::<T>::get(owner, netuid).ok_or(Error::<T>::PositionMissing)?;
                    // Stale queue entry (rekeyed / already closed).
                    if position.expires_at > now {
                        return Ok(());
                    }
                    // Expiry closes through the pool (AMM path); spot is event-only.
                    Self::settle_expiry_as_close(netuid, owner, position, now)
                })();
                match result {
                    Ok(()) => TransactionOutcome::Commit(Ok::<(), DispatchError>(())),
                    Err(e) => TransactionOutcome::Rollback(Err(e)),
                }
            });
            match outcome {
                Ok(()) => {
                    match source {
                        CollectSource::Scheduled => {
                            // Stale / dissolving skips leave the queue row; drop it.
                            // Successful settle already cleared it via remove_position.
                            if Expiry::<T>::contains_key(at, (owner, netuid)) {
                                Expiry::<T>::remove(at, (owner, netuid));
                                Self::note_expiry_removed(at);
                            }
                        }
                        CollectSource::Retry => {
                            let _ = Self::take_expiry_retry(at, owner, netuid);
                        }
                    }
                    // Partial forced unwind leaves the position open past
                    // expires_at — requeue for the next block so unwind
                    // progresses under the per-block impact budget.
                    // Dissolution settles its own positions; do not spin the
                    // per-block expiry retry on the early `Ok` skip.
                    if let Some(mut position) = Positions::<T>::get(owner, netuid)
                        .filter(|_| !Dissolutions::<T>::contains_key(netuid))
                    {
                        if position.expires_at <= now {
                            let retry_at = now.saturating_add(T::ForcedSettlementRetryDelay::get());
                            Self::enqueue_expiry_retry(owner, netuid, retry_at);
                            position.expiry_due = retry_at;
                            Positions::<T>::insert(owner, netuid, position);
                            let reason =
                                BoundedVec::truncate_from(b"forced_impact_partial".to_vec());
                            LastDeferral::<T>::insert(
                                owner,
                                netuid,
                                (1u8, retry_at, reason.clone()),
                            );
                            Self::deposit_event(Event::SettlementDeferred {
                                owner: owner.clone(),
                                netuid,
                                retry_at,
                                reason,
                            });
                            return;
                        }
                    }
                    LastDeferral::<T>::remove(owner, netuid);
                }
                Err(error) => {
                    match source {
                        CollectSource::Scheduled => {
                            // Preserve coverage without pinning NextExpiry /
                            // mid-inserting into the Expiry schedule (Skeptic
                            // fid:0776a829). Side-queue insert is O(1).
                            if Expiry::<T>::contains_key(at, (owner, netuid)) {
                                Expiry::<T>::remove(at, (owner, netuid));
                                Self::note_expiry_removed(at);
                            }
                        }
                        CollectSource::Retry => {
                            let _ = Self::take_expiry_retry(at, owner, netuid);
                        }
                    }
                    // Age from expires_at (not first-attempt wall clock).
                    let since = Positions::<T>::get(owner, netuid)
                        .map(|p| p.expires_at)
                        .unwrap_or(now);
                    Self::note_band_deferral(owner, netuid, since, error);
                    if let Some(mut position) = Positions::<T>::get(owner, netuid) {
                        // Past max deferral: always next-block retry so force
                        // unwind keeps progressing (impact budget, AMM dust,
                        // or transient pool errors). Otherwise keep the longer
                        // ExpiryRetryDelay for EMA-band deferrals.
                        let delay = if Self::past_max_settlement_deferral(owner, netuid) {
                            T::ForcedSettlementRetryDelay::get()
                        } else {
                            T::ExpiryRetryDelay::get()
                        };
                        let retry_at = now.saturating_add(delay);
                        Self::enqueue_expiry_retry(owner, netuid, retry_at);
                        position.expiry_due = retry_at;
                        Positions::<T>::insert(owner, netuid, position);
                        let reason = Self::deferral_reason(error);
                        LastDeferral::<T>::insert(owner, netuid, (1u8, retry_at, reason.clone()));
                        Self::deposit_event(Event::SettlementDeferred {
                            owner: owner.clone(),
                            netuid,
                            retry_at,
                            reason,
                        });
                    }
                }
            }
        }

        /// Drain the primary Due linked schedule. Outer loop stays separate from
        /// [`Self::collect_interest_retries`]: linked-list unlink + TotalPositions
        /// wipe of all queue heads (intentional; not a shared Source param).
        pub(crate) fn collect_due(now: BlockNumberFor<T>, meter: &mut WeightMeter) {
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
                NextInterestRetry::<T>::kill();
                InterestRetryPending::<T>::kill();
                InterestRetryCursor::<T>::kill();
                NextExpiryRetry::<T>::kill();
                ExpiryRetryPending::<T>::kill();
                ExpiryRetryCursor::<T>::kill();
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
                let mut iter = if let Some(cursor) = DueBucketCursor::<T>::get() {
                    Due::<T>::iter_prefix_from(due, Due::<T>::hashed_key_for(due, cursor))
                } else {
                    Due::<T>::iter_prefix(due)
                };
                let mut saw_entry = false;
                // Cursor stores the last successfully processed key so
                // `iter_prefix_from` resumes at the next unprocessed entry.
                let mut last_done: Option<(T::AccountId, NetUid)> = None;
                // Bare loop: peek weight before each `iter.next()` (skeptic
                // fid:5012ec2d).
                loop {
                    if meter.try_consume(Self::due_queue_peek_weight()).is_err() {
                        if let Some(done) = last_done {
                            DueBucketCursor::<T>::put(done);
                        }
                        NextDue::<T>::put(due);
                        return;
                    }
                    let Some(((owner, netuid), ())) = iter.next() else {
                        break;
                    };
                    saw_entry = true;
                    if meter.try_consume(T::WeightInfo::collect()).is_err() {
                        if let Some(done) = last_done {
                            DueBucketCursor::<T>::put(done);
                        }
                        // else: keep the prior exclusive cursor (or none) so the
                        // unprocessed entry is retried, not skipped.
                        NextDue::<T>::put(due);
                        return;
                    }
                    Self::process_one_interest(CollectSource::Scheduled, now, due, &owner, netuid);
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

        /// Auto-close positions whose fixed term has ended (§15).
        /// Runs the default close path at spot (no wallet top-up, no slippage
        /// limits, best-effort on underwater loans).
        /// Dissolution wins if the subnet is already dissolving — skip those.
        ///
        /// `budget` caps settle attempts (shared with expiry retries in
        /// `on_initialize`). Returns the remaining budget. Pass `u32::MAX` from
        /// metered `on_idle`.
        ///
        /// Outer loop stays separate from [`Self::collect_expiry_retries`]:
        /// linked-list unlink, proof-aware header, and `resume_expiry_collect`
        /// (intentional; settle body is shared via `process_one_expiry`).
        pub(crate) fn collect_expiry(
            now: BlockNumberFor<T>,
            meter: &mut WeightMeter,
            mut budget: u32,
        ) -> u32 {
            if budget == 0 {
                return 0;
            }
            // Proof-aware header before TotalPositions / NextExpiry (skeptic
            // fid:a74169e6). Insufficient budget: touch nothing.
            if meter
                .try_consume(Self::expiry_collect_header_weight())
                .is_err()
            {
                return budget;
            }
            if TotalPositions::<T>::get() == 0 {
                NextExpiry::<T>::kill();
                ExpiryBucketCursor::<T>::kill();
                NextExpiryRetry::<T>::kill();
                ExpiryRetryPending::<T>::kill();
                ExpiryRetryCursor::<T>::kill();
                return budget;
            }
            let Some(mut at) = NextExpiry::<T>::get() else {
                return budget;
            };
            while at <= now {
                if budget == 0 {
                    // Resume write reserved in the bucket-open charge below on
                    // prior iterations; when we never opened, charge a scalar
                    // write before putting.
                    if meter
                        .try_consume(
                            Weight::from_parts(0, Self::EXPIRY_SCALAR_PROOF)
                                .saturating_add(T::DbWeight::get().writes(1)),
                        )
                        .is_err()
                    {
                        return 0;
                    }
                    NextExpiry::<T>::put(at);
                    return 0;
                }
                // Cursor + resume writes reserved before any queue touch.
                if meter
                    .try_consume(Self::expiry_bucket_open_weight())
                    .is_err()
                {
                    return budget;
                }
                let mut iter = if let Some(cursor) = ExpiryBucketCursor::<T>::get() {
                    Expiry::<T>::iter_prefix_from(at, Expiry::<T>::hashed_key_for(at, cursor))
                } else {
                    Expiry::<T>::iter_prefix(at)
                };
                let mut saw_entry = false;
                let mut last_done: Option<(T::AccountId, NetUid)> = None;
                // Bare loop: peek weight must be charged before each
                // `iter.next()` so a mid-bucket stop can resume via last_done
                // without skipping keys (skeptic fid:eb66f29d).
                loop {
                    if meter.try_consume(Self::expiry_queue_peek_weight()).is_err() {
                        Self::resume_expiry_collect(at, last_done);
                        return budget;
                    }
                    let Some(((owner, netuid), ())) = iter.next() else {
                        break;
                    };
                    if budget == 0 {
                        Self::resume_expiry_collect(at, last_done);
                        return 0;
                    }
                    if meter.try_consume(T::WeightInfo::settle()).is_err() {
                        // Row was fetched under peek weight; resume from
                        // last_done so this key is retried (iter_prefix_from
                        // is exclusive of the cursor) — unless the prior
                        // settlement already advanced the head.
                        Self::resume_expiry_collect(at, last_done);
                        return budget;
                    }
                    saw_entry = true;
                    budget = budget.saturating_sub(1);
                    Self::process_one_expiry(CollectSource::Scheduled, now, at, &owner, netuid);
                    last_done = Some((owner, netuid));
                }
                ExpiryBucketCursor::<T>::kill();
                if !saw_entry {
                    if meter
                        .try_consume(Self::expiry_empty_bucket_weight())
                        .is_err()
                    {
                        NextExpiry::<T>::put(at);
                        return budget;
                    }
                    ExpiryIndex::<T>::remove(at);
                    Self::unlink_expiry_bucket(at);
                }
                let Some(n) = NextExpiry::<T>::get() else {
                    break;
                };
                if n == at {
                    // Stale head: unlink needs the same proof envelope as empty
                    // cleanup when the index path did not already unlink.
                    if saw_entry {
                        if meter
                            .try_consume(Self::expiry_empty_bucket_weight())
                            .is_err()
                        {
                            NextExpiry::<T>::put(at);
                            return budget;
                        }
                    }
                    Self::unlink_expiry_bucket(at);
                    let Some(n) = NextExpiry::<T>::get() else {
                        break;
                    };
                    at = n;
                } else {
                    at = n;
                }
            }
            budget
        }

        /// Drain [`InterestRetry`] with metered empty-bucket bumps (no linked-list
        /// mid-insert). Successful charges rejoin the normal [`Due`] schedule via
        /// the append-heavy `note_due_inserted` path.
        ///
        /// Outer loop stays separate from [`Self::collect_due`]: sparse empty-bump
        /// + `MAX_EMPTY_BUMPS` (intentional). Charge/deferral body is shared via
        /// `process_one_interest(CollectSource::Retry, ...)`.
        fn collect_interest_retries(now: BlockNumberFor<T>, meter: &mut WeightMeter) {
            if meter
                .try_consume(T::DbWeight::get().reads_writes(1, 1))
                .is_err()
            {
                return;
            }
            let Some(mut at) = NextInterestRetry::<T>::get() else {
                return;
            };
            // Cap empty-gap advances so a sparse Next cannot spin the block.
            let mut empty_bumps = 0u32;
            const MAX_EMPTY_BUMPS: u32 = 64;
            while at <= now {
                if meter.try_consume(T::DbWeight::get().reads(1)).is_err() {
                    NextInterestRetry::<T>::put(at);
                    return;
                }
                let mut iter = if let Some(cursor) = InterestRetryCursor::<T>::get() {
                    InterestRetry::<T>::iter_prefix_from(
                        at,
                        InterestRetry::<T>::hashed_key_for(at, cursor),
                    )
                } else {
                    InterestRetry::<T>::iter_prefix(at)
                };
                let mut saw_entry = false;
                let mut last_done: Option<(T::AccountId, NetUid)> = None;
                // Bare loop: peek weight before each `iter.next()` (skeptic
                // fid:5012ec2d).
                loop {
                    if meter.try_consume(Self::due_queue_peek_weight()).is_err() {
                        if let Some(done) = last_done {
                            InterestRetryCursor::<T>::put(done);
                        }
                        NextInterestRetry::<T>::put(at);
                        return;
                    }
                    let Some(((owner, netuid), ())) = iter.next() else {
                        break;
                    };
                    saw_entry = true;
                    empty_bumps = 0;
                    if meter.try_consume(T::WeightInfo::collect()).is_err() {
                        if let Some(done) = last_done {
                            InterestRetryCursor::<T>::put(done);
                        }
                        NextInterestRetry::<T>::put(at);
                        return;
                    }
                    Self::process_one_interest(CollectSource::Retry, now, at, &owner, netuid);
                    last_done = Some((owner, netuid));
                }
                InterestRetryCursor::<T>::kill();
                if InterestRetryPending::<T>::get() == 0 {
                    return;
                }
                if saw_entry {
                    // More entries may remain under `at` if we stopped early; if
                    // the prefix is drained, advance past this bucket. Meter the
                    // empty-prefix probe (skeptic fid:203d3004 class).
                    if meter.try_consume(Self::due_queue_peek_weight()).is_err() {
                        NextInterestRetry::<T>::put(at);
                        return;
                    }
                    if InterestRetry::<T>::iter_prefix(at).next().is_none() {
                        at = at.saturating_add(1u32.into());
                        NextInterestRetry::<T>::put(at);
                    } else {
                        NextInterestRetry::<T>::put(at);
                        return;
                    }
                } else {
                    empty_bumps = empty_bumps.saturating_add(1);
                    if empty_bumps >= MAX_EMPTY_BUMPS {
                        // Resumable pointer — never kill while pending > 0
                        // (Skeptic fid:1c6bb1dc; production delay is 100 > 64).
                        NextInterestRetry::<T>::put(at.saturating_add(1u32.into()));
                        return;
                    }
                    at = at.saturating_add(1u32.into());
                    NextInterestRetry::<T>::put(at);
                }
            }
            // Caught up past `now` with live pending entries still ahead of the
            // production retry delay: keep the resumable pointer at `at`.
            if InterestRetryPending::<T>::get() > 0 {
                NextInterestRetry::<T>::put(at);
            }
        }

        /// Drain [`ExpiryRetry`] with the same O(1)-enqueue / metered-bump shape
        /// as [`Self::collect_interest_retries`]. `budget` is shared with
        /// [`Self::collect_expiry`] in `on_initialize`.
        ///
        /// Outer loop stays separate from [`Self::collect_expiry`]: sparse
        /// empty-bump + `MAX_EMPTY_BUMPS` (intentional). Settle body is shared
        /// via `process_one_expiry(CollectSource::Retry, ...)`.
        fn collect_expiry_retries(
            now: BlockNumberFor<T>,
            meter: &mut WeightMeter,
            mut budget: u32,
        ) -> u32 {
            if budget == 0 {
                return 0;
            }
            // Proof-aware read of the retry head before touching storage.
            if meter
                .try_consume(
                    Weight::from_parts(0, Self::EXPIRY_SCALAR_PROOF)
                        .saturating_add(T::DbWeight::get().reads_writes(1, 1)),
                )
                .is_err()
            {
                return budget;
            }
            let Some(mut at) = NextExpiryRetry::<T>::get() else {
                return budget;
            };
            let mut empty_bumps = 0u32;
            const MAX_EMPTY_BUMPS: u32 = 64;
            while at <= now {
                if budget == 0 {
                    NextExpiryRetry::<T>::put(at);
                    return 0;
                }
                if meter
                    .try_consume(Self::expiry_bucket_open_weight())
                    .is_err()
                {
                    return budget;
                }
                let mut iter = if let Some(cursor) = ExpiryRetryCursor::<T>::get() {
                    ExpiryRetry::<T>::iter_prefix_from(
                        at,
                        ExpiryRetry::<T>::hashed_key_for(at, cursor),
                    )
                } else {
                    ExpiryRetry::<T>::iter_prefix(at)
                };
                let mut saw_entry = false;
                let mut last_done: Option<(T::AccountId, NetUid)> = None;
                // Bare loop: peek weight must be charged before each
                // `iter.next()` so a mid-bucket stop can resume via cursor
                // without skipping keys.
                loop {
                    if meter.try_consume(Self::expiry_queue_peek_weight()).is_err() {
                        if let Some(done) = last_done {
                            ExpiryRetryCursor::<T>::put(done);
                        }
                        NextExpiryRetry::<T>::put(at);
                        return budget;
                    }
                    let Some(((owner, netuid), ())) = iter.next() else {
                        break;
                    };
                    if budget == 0 {
                        if let Some(done) = last_done {
                            ExpiryRetryCursor::<T>::put(done);
                        }
                        NextExpiryRetry::<T>::put(at);
                        return 0;
                    }
                    if meter.try_consume(T::WeightInfo::settle()).is_err() {
                        if let Some(done) = last_done {
                            ExpiryRetryCursor::<T>::put(done);
                        }
                        NextExpiryRetry::<T>::put(at);
                        return budget;
                    }
                    saw_entry = true;
                    empty_bumps = 0;
                    budget = budget.saturating_sub(1);
                    Self::process_one_expiry(CollectSource::Retry, now, at, &owner, netuid);
                    last_done = Some((owner, netuid));
                }
                ExpiryRetryCursor::<T>::kill();
                if ExpiryRetryPending::<T>::get() == 0 {
                    return budget;
                }
                if saw_entry {
                    // Meter the empty-prefix probe before a fresh iterator
                    // (skeptic fid:203d3004).
                    if meter.try_consume(Self::expiry_queue_peek_weight()).is_err() {
                        NextExpiryRetry::<T>::put(at);
                        return budget;
                    }
                    if ExpiryRetry::<T>::iter_prefix(at).next().is_none() {
                        at = at.saturating_add(1u32.into());
                        NextExpiryRetry::<T>::put(at);
                    } else {
                        NextExpiryRetry::<T>::put(at);
                        return budget;
                    }
                } else {
                    empty_bumps = empty_bumps.saturating_add(1);
                    if empty_bumps >= MAX_EMPTY_BUMPS {
                        NextExpiryRetry::<T>::put(at.saturating_add(1u32.into()));
                        return budget;
                    }
                    at = at.saturating_add(1u32.into());
                    NextExpiryRetry::<T>::put(at);
                }
            }
            if ExpiryRetryPending::<T>::get() > 0 {
                NextExpiryRetry::<T>::put(at);
            }
            budget
        }

        fn convert_pending(now: BlockNumberFor<T>, meter: &mut WeightMeter) {
            // Reserve params + cursor (+ optional cursor write on exit) before
            // touching storage (skeptic fid:e2f153d7).
            if meter
                .try_consume(
                    Weight::from_parts(0, Self::EXPIRY_SCALAR_PROOF.saturating_mul(2))
                        .saturating_add(T::DbWeight::get().reads_writes(2, 1)),
                )
                .is_err()
            {
                return;
            }
            let params = Self::lift_params();
            // Bounded, resumable walk — never materialise the full map first.
            // Cursor is exclusive (`iter_from`); store the last fully handled key.
            let mut iter = if let Some(cursor) = ConversionCursor::<T>::get() {
                LongInterestPending::<T>::iter_from(LongInterestPending::<T>::hashed_key_for(
                    cursor,
                ))
            } else {
                LongInterestPending::<T>::iter()
            };
            let mut last_done: Option<NetUid> = None;
            loop {
                // Peek before each map advance (same ordering as expiry).
                if meter.try_consume(Self::due_queue_peek_weight()).is_err() {
                    if let Some(done) = last_done {
                        ConversionCursor::<T>::put(done);
                    }
                    return;
                }
                let Some((netuid, pending)) = iter.next() else {
                    break;
                };
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
                // §6 crash guard with 4-week backlog override. Seed `Since` for
                // pre-existing pending so the clock starts at first observation.
                let since = LongInterestSince::<T>::get(netuid).unwrap_or_else(|| {
                    LongInterestSince::<T>::insert(netuid, now);
                    now
                });
                // Four interest periods ≈ 4 weeks at the default 7-day period.
                let backlog_age = T::InterestPeriod::get()
                    .saturating_mul(4u32.into())
                    .max(1u32.into());
                let force = now.saturating_sub(since) >= backlog_age;
                if let Some(reference) = reference.as_ref() {
                    let floor = reference
                        .price
                        .saturating_mul(U64F64::from_num(params.crash_guard_bps))
                        / U64F64::from_num(10_000u16);
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
                // Under the backlog override, drop the floor so the forced sale
                // can clear (still subject to the 0.01 τ dust gate).
                let min_out = if force {
                    0u64
                } else if let Some(reference) = reference.as_ref() {
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
                // Sell + burn + pending debit are atomic: a failed burn must not
                // clear pending interest or emit success (Greg Zaitsev P2-b).
                use frame_support::storage::{TransactionOutcome, with_transaction};
                let sold = match with_transaction(|| {
                    let sold = match T::Pool::forced_sell_into_escrow(
                        &reserve,
                        &custody,
                        &reserve,
                        netuid,
                        slice.into(),
                        min_out.into(),
                        Self::skip_long_tao_flow(netuid),
                    ) {
                        Ok(t) => t,
                        Err(e) => return TransactionOutcome::Rollback(Err(e)),
                    };
                    // Same realized EMA band as expiry / forfeit forced sells.
                    if let Err(e) =
                        Self::ensure_realized_sell_within_slippage(netuid, sold.to_u64(), slice)
                    {
                        return TransactionOutcome::Rollback(Err(e));
                    }
                    if let Err(e) = T::Pool::burn_interest_tao(&reserve, sold) {
                        return TransactionOutcome::Rollback(Err(e));
                    }
                    TransactionOutcome::Commit(Ok::<TaoBalance, DispatchError>(sold))
                }) {
                    Ok(t) => t,
                    Err(_) => {
                        last_done = Some(netuid);
                        continue;
                    }
                };
                // Record impact so a same-block expiry drain can defer.
                if let Ok(Some(fair)) = Self::fair_ema_tao(netuid, slice, false) {
                    Self::note_protocol_forced_impact(netuid, fair, sold.to_u64());
                }
                LongInterestPending::<T>::mutate(netuid, |p| {
                    *p = p.saturating_sub(slice);
                    if *p == 0 {
                        LongInterestSince::<T>::remove(netuid);
                    }
                });
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
                let sold_tao = sold.to_u64();
                Self::bump_stats(netuid, |s| {
                    s.interest_burned_alpha =
                        s.interest_burned_alpha.saturating_add(u128::from(slice));
                    s.interest_burned_tao =
                        s.interest_burned_tao.saturating_add(u128::from(sold_tao));
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
                        dissolution_price: U64F64::from_num(0),
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
                    dissolution_price: U64F64::from_num(0),
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
                b.l_s = 0;
                b.l_l = 0;
            });
            // Dissolution winds the subnet down — skip k-burn.
            let _ = Self::maybe_burn_invariant_surplus(
                netuid,
                book.held_tao,
                book.held_alpha,
                book.held_tao,
                book.held_alpha,
                true,
            );
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

        /// Subnet-dissolution step only (Jacob §13.2.2; weight-metered,
        /// multi-block OK). Not ordinary expiry / close.
        /// Assumes lift legs were already re-added in the dissolve call.
        /// Phase 0: freeze dissolution price after phase-0 basket conversions
        ///   `Δ0 = max(0, T_flag − T_pre)`,
        ///   `T_settle = min(T_flag, t_ema_flag) − Δ0`,
        ///   `dissolution_price = T_settle / ALL_alpha`, then
        ///   `dissolution_price = min(dissolution_price, moving_alpha_price)`.
        /// Phase 1: settle positions.
        /// Phase 2: convert pending long interest at dissolution_price and burn.
        pub fn settle_positions_on_dissolution(netuid: NetUid, meter: &mut WeightMeter) -> bool {
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
                let mut dissolution_price = if all_alpha == 0 {
                    U64F64::from_num(0)
                } else {
                    // dissolution_price = T_settle / all_alpha in U64F64.
                    let bits = (u128::from(t_settle) << 64) / all_alpha;
                    U64F64::from_bits(bits)
                };
                let moving_price = T::Pool::moving_alpha_price(netuid);
                if moving_price > U64F64::from_num(0) {
                    dissolution_price = dissolution_price.min(moving_price);
                }
                dissolution.dissolution_price = dissolution_price;
                dissolution.lifts_readded = true;
                Dissolutions::<T>::insert(netuid, dissolution.clone());
                DissolutionPhase::<T>::insert(netuid, 1u8);
                DissolutionCursor::<T>::remove(netuid);
                Self::deposit_event(Event::SettleDissolutionPriceFixed {
                    netuid,
                    t_flag: dissolution.t_flag,
                    t_ema_flag: dissolution.t_ema_flag,
                    t_pre,
                    delta0,
                    d: all_alpha,
                    dissolution_price,
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
                if Self::settle_position_on_dissolution(
                    netuid,
                    &owner,
                    position,
                    dissolution.dissolution_price,
                    dissolution.frozen_at,
                )
                .is_err()
                {
                    return false;
                }
                DissolutionCursor::<T>::insert(netuid, owner);
            }
            DissolutionCursor::<T>::remove(netuid);

            // Phase 2: pending long interest at dissolution_price → burn TAO
            if meter.try_consume(T::WeightInfo::settle()).is_err() {
                DissolutionPhase::<T>::insert(netuid, 2u8);
                return false;
            }
            let pending = LongInterestPending::<T>::get(netuid);
            if pending > 0 {
                let tao =
                    Self::tao_for_alpha(pending, dissolution.dissolution_price, false).unwrap_or(0);
                if tao > 0 {
                    // §13: burn first; only then clear pending. A failed burn
                    // must retain pending interest and not advance the phase
                    // (Greg Zaitsev P2-b).
                    if T::Pool::burn_from_pot(netuid, tao.into()).is_err() {
                        DissolutionPhase::<T>::insert(netuid, 2u8);
                        return false;
                    }
                    Self::deposit_event(Event::InterestBurned {
                        netuid,
                        side: Side::Long,
                        tao,
                    });
                    Self::bump_stats(netuid, |s| {
                        s.interest_burned_tao =
                            s.interest_burned_tao.saturating_add(u128::from(tao));
                    });
                }
                LongInterestPending::<T>::remove(netuid);
                LongInterestSince::<T>::remove(netuid);
            }
            DissolutionPhase::<T>::insert(netuid, 3u8);
            true
        }

        /// Subnet-dissolution only (§13): settle one position at
        /// `dissolution_price` with **no AMM trades**.
        ///
        /// Money path: short debt / long A+C valuation use `dissolution_price`
        /// (young shorts may take `max(dissolution_price, P_open)`). This is
        /// not expiry — fixed-term expiry calls [`Self::settle_expiry_as_close`]
        /// and never takes a dissolution price.
        ///
        /// Short path: withdraw recovery + interest + refund in one credit
        /// settle so no Expendable mid-path debit can reap a below-ED remainder
        /// (skeptic fid:0fadfc1c / 83813c42).
        #[transactional]
        fn settle_position_on_dissolution(
            netuid: NetUid,
            owner: &T::AccountId,
            mut position: Position<T::AccountId, BlockNumberFor<T>>,
            dissolution_price: U64F64,
            interest_until: BlockNumberFor<T>,
        ) -> DispatchResult {
            Self::ensure_terms_version(position.terms_version)?;

            let escrow = position.custody.clone();
            let reserve = Self::reserve_account(netuid);
            let custody = Self::custody_hotkey()?;

            match position.side {
                Side::Short => {
                    // §13 / Greg Zaitsev P2-a: pot (staker recovery) has first
                    // claim on E_S + C; interest burns from the remainder only.
                    // Accrue the due amount without debiting collateral yet.
                    let mut until = interest_until;
                    if let Some(d) = Dissolutions::<T>::get(netuid) {
                        if until > d.frozen_at {
                            until = d.frozen_at;
                        }
                    }
                    let (due, rem) = Self::interest_due(netuid, &position, until, true)?;
                    position.last_accrued = until;
                    position.interest_remainder = rem;

                    // Young shorts (age < young_short_settlement_window):
                    // max(dissolution_price, P_open) where P_open is the
                    // principal-weighted average open price
                    // `proceeds/principal` (VWAP across open + increases).
                    // Age uses last_exposure_added_at (Greg Zaitsev P1-b).
                    // Boundary age == window keeps dissolution_price.
                    let mut short_price = dissolution_price;
                    if position.principal > 0 {
                        let opened_at = Self::position_opened_at(&position);
                        let age = until.saturating_sub(opened_at);
                        if age < Self::young_short_settlement_window() {
                            if let Some(p_open) = U64F64::from_num(position.proceeds)
                                .checked_div(U64F64::from_num(position.principal))
                            {
                                if p_open > short_price {
                                    short_price = p_open;
                                }
                            }
                        }
                    }
                    let debt = Self::tao_for_alpha(position.principal, short_price, true)?;
                    let available = Self::add(position.proceeds, position.collateral)?;
                    let to_pot = debt.min(available);
                    let after_pot = available.saturating_sub(to_pot);
                    let interest = due.min(after_pot);
                    let refund = after_pot.saturating_sub(interest);
                    // Single credit withdraw for pot + interest + refund
                    // (fid:0fadfc1c). Separate Expendable burns can reap a
                    // below-ED remainder and stall dissolution.
                    let (recovered, burned, refunded) = T::Pool::terminal_settle_short_tao(
                        &escrow,
                        &reserve,
                        owner,
                        to_pot.into(),
                        interest.into(),
                        refund.into(),
                    )?;
                    let recovered = recovered.to_u64();
                    let _burned = burned.to_u64();
                    let refunded = refunded.to_u64();
                    if interest > 0 {
                        // Credit consumed as burn transfer or recycled dust.
                        Self::deposit_event(Event::InterestBurned {
                            netuid,
                            side: Side::Short,
                            tao: interest,
                        });
                        Self::bump_stats(netuid, |s| {
                            s.interest_burned_tao =
                                s.interest_burned_tao.saturating_add(u128::from(interest));
                        });
                    }
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
                        book.held_alpha = book
                            .held_alpha
                            .saturating_sub(position.lift_alpha.saturating_sub(position.principal));
                        book.escrow_tao = book.escrow_tao.saturating_sub(position.proceeds);
                        book.short_sold_alpha =
                            book.short_sold_alpha.saturating_sub(position.principal);
                    });
                    // Dissolution already re-added lift legs; skip k-burn.
                    Self::maybe_burn_invariant_surplus(
                        netuid,
                        recovered,
                        0,
                        position.lift_tao,
                        position.lift_alpha,
                        true,
                    )?;
                    Self::remove_position(owner, netuid, &position);
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
                Side::Long => {
                    // Accrue / destroy due interest alpha before valuing A+C.
                    Self::charge_interest(
                        owner,
                        netuid,
                        &mut position,
                        interest_until,
                        true,
                        false,
                        false,
                    )?;
                    // Dissolution skips forfeit inside `charge_interest`.
                    if !Positions::<T>::contains_key(owner, netuid) {
                        return Ok(());
                    }
                    Positions::<T>::insert(owner, netuid, position.clone());
                    // Escrow A settles at EMA (quantity-backed). Collateral C is
                    // capped at an impact-aware AMM sale so near-expiry top-ups
                    // cannot redeem above dumping C into the live pool.
                    let a_val =
                        Self::tao_for_alpha(position.escrow_alpha, dissolution_price, false)?;
                    let c = position.collateral;
                    let c_ema = Self::tao_for_alpha(c, dissolution_price, false)?;
                    let c_amm = if c > 0 {
                        match T::Pool::quote_sell(netuid, c.into()) {
                            Ok(t) => t.to_u64(),
                            Err(_) => {
                                let w = T::Pool::quote_weight_parts(netuid);
                                let y = T::Pool::pool_tao(netuid).to_u64();
                                let x = T::Pool::pool_alpha(netuid).to_u64();
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
                    // Pay remainder before destroying custody alpha. A pot miss
                    // must revert the whole transactional settle so collateral is
                    // not destroyed. Below-ED dust only.
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
                    Self::maybe_burn_invariant_surplus(
                        netuid,
                        0,
                        0,
                        position.lift_tao,
                        position.lift_alpha,
                        true,
                    )?;
                    Self::remove_position(owner, netuid, &position);
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
            }
            Ok(())
        }

        /// Rolled-back fee-bearing simulation of the long escrow close sells
        /// (`forced_sell_into_escrow` of A, then minimal C) matching `do_close`.
        /// Returns `(tao_raised, collateral_alpha_left)`.
        ///
        /// Auditor fid:178e1fb3 / fid:8a7b4914 — quotes and solvency probes must
        /// not use fee-free `quote_sell_on`, which overstates proceeds.
        fn simulate_long_escrow_raise(
            netuid: NetUid,
            escrow: &T::AccountId,
            escrow_alpha: u64,
            collateral: u64,
            principal: u64,
        ) -> Result<(u64, u64), DispatchError> {
            use frame_support::storage::{TransactionOutcome, with_transaction};
            with_transaction(|| {
                let probe = (|| -> Result<(u64, u64), DispatchError> {
                    let custody = Self::custody_hotkey()?;
                    let mut raised = 0u64;
                    let skip_flow = Self::skip_long_tao_flow(netuid);
                    if escrow_alpha > 0 {
                        let out = T::Pool::forced_sell_into_escrow(
                            escrow,
                            &custody,
                            escrow,
                            netuid,
                            escrow_alpha.into(),
                            0u64.into(),
                            skip_flow,
                        )?;
                        raised = Self::add(raised, out.to_u64())?;
                    }
                    let mut coll_left = collateral;
                    if raised < principal && coll_left > 0 {
                        let need = principal.saturating_sub(raised);
                        let sell = match Self::alpha_to_raise_tao(netuid, need) {
                            Ok(needed_alpha) => needed_alpha.min(coll_left),
                            Err(_) => coll_left,
                        };
                        if sell > 0 {
                            let out = T::Pool::forced_sell_into_escrow(
                                escrow,
                                &custody,
                                escrow,
                                netuid,
                                sell.into(),
                                0u64.into(),
                                skip_flow,
                            )?;
                            raised = Self::add(raised, out.to_u64())?;
                            coll_left = coll_left.saturating_sub(sell);
                        }
                    }
                    Ok((raised, coll_left))
                })();
                match probe {
                    Ok(v) => TransactionOutcome::Rollback(Ok::<(u64, u64), DispatchError>(v)),
                    Err(e) => TransactionOutcome::Rollback(Err(e)),
                }
            })
        }

        /// Whether escrow + collateral can fully repay principal through the
        /// pool at the current spot (no wallet top-up). Used by expiry to choose
        /// between a solvent close and the collateral-exhaustion forfeit path.
        ///
        /// Long legs dry-run the same fee-bearing `forced_sell_into_escrow`
        /// path as settlement (rolled back), so fee-free `quote_sell_on` cannot
        /// misclassify a marginally solvent long (Auditor fid:8a7b4914).
        fn can_fully_close_from_escrow(
            netuid: NetUid,
            position: &Position<T::AccountId, BlockNumberFor<T>>,
        ) -> Result<bool, DispatchError> {
            match position.side {
                Side::Short => {
                    let pot = Self::add(position.proceeds, position.collateral)?;
                    match Self::tao_to_buy_alpha(netuid, position.principal) {
                        Ok(cost) => Ok(cost <= pot),
                        // Pool cannot supply `P` (or quote fails) → insolvent.
                        Err(_) => Ok(false),
                    }
                }
                Side::Long => {
                    let (raised, _) = Self::simulate_long_escrow_raise(
                        netuid,
                        &position.custody,
                        position.escrow_alpha,
                        position.collateral,
                        position.principal,
                    )?;
                    Ok(raised >= position.principal)
                }
            }
        }

        /// Fixed-term expiry (§15): default `close` path through the **pool AMM**
        /// (`repay_from_wallet = false`). Money moves via buyback / forced sells
        /// at live pool prices — there is no dissolution-price argument.
        ///
        /// Intentional differences from manual `do_close`:
        /// * Realized cumulative execution vs lending EMA within
        ///   [`LiftParams::expiry_max_slippage_bps`] on every AMM chunk
        ///   needed to make the pool whole (shorts: TAO spent buying P;
        ///   longs: TAO received from selling A and minimal C). Breach →
        ///   transactional rollback + defer. After
        ///   [`LiftParams::max_settlement_deferral`] the next pass
        ///   force-settles at market (this per-trade band is skipped).
        /// * Per-block cumulative protocol-forced impact still paces force
        ///   settles once the same tolerance is exceeded — excess claims
        ///   wait for later blocks (transactional rollback + retry).
        /// * Owner may wallet-repay unconstrained while deferred; AMM close
        ///   uses the same realized bound until the force window. Increases
        ///   / rolls are frozen.
        /// * Underwater short / insolvent long: [`Self::forfeit`] (best-effort
        ///   buyback under the same realized / force-market guard).
        /// * No call-supplied hotkey: leftover long collateral returns as
        ///   staked alpha on the custody hotkey (not force-sold through the
        ///   band). Short buyback overshoot still sells to TAO for the owner
        ///   (dust only; not needed to make the pool whole beyond P).
        ///
        /// `Expired.spot_price` is reported for observability only (read at emit
        /// time); it does not size debt, payment, or refund.
        fn settle_expiry_as_close(
            netuid: NetUid,
            owner: &T::AccountId,
            mut position: Position<T::AccountId, BlockNumberFor<T>>,
            interest_until: BlockNumberFor<T>,
        ) -> DispatchResult {
            Self::ensure_terms_version(position.terms_version)?;
            let expires_at = position.expires_at;
            let side = position.side;
            let spot_price_for_event = T::Pool::current_alpha_price(netuid);
            // Past max deferral: force at market (skip per-trade band). The
            // cumulative impact cap still gates how much clears this block.
            let force_market = Self::past_max_settlement_deferral(owner, netuid);
            let realized_guard = !force_market;
            Self::ensure_block_forced_impact_ok(netuid)?;

            // Interest forfeit (if any) shares the same realized / force guard.
            Self::charge_interest(
                owner,
                netuid,
                &mut position,
                interest_until,
                true,
                false,
                realized_guard,
            )?;
            if !Positions::<T>::contains_key(owner, netuid) {
                Self::emit_loan_expired(owner, netuid, side, expires_at, interest_until);
                return Ok(());
            }

            if !Self::can_fully_close_from_escrow(netuid, &position)? {
                // Shorts that cannot buy P back: best-effort forfeit (burn
                // leftover; record shortfall). Force-market skips the band.
                // Partial (impact-capped) leaves the position open; outer
                // process_one_expiry requeues at ForcedSettlementRetryDelay.
                let fully_closed = Self::forfeit(owner, netuid, &position, realized_guard)?;
                if fully_closed {
                    Self::emit_loan_expired(owner, netuid, side, expires_at, interest_until);
                }
                return Ok(());
            }

            let custody = Self::custody_hotkey()?;
            let reserve = Self::reserve_account(netuid);
            let escrow = position.custody.clone();

            let mut leftover_collateral = 0u64;
            let (credited_tao, credited_alpha, paid, to_pool, refund, impact_fair, impact_realized) =
                match side {
                    Side::Short => {
                        let principal = position.principal;
                        // Size to remaining ProtocolForcedImpact room
                        // (spot-relative; first trade included). Force-market
                        // guarantees forced_min_buyback_bps when impact admits 0.
                        let slice_p =
                            Self::max_forced_buyback_alpha(netuid, principal, force_market)?;
                        ensure!(slice_p > 0, Error::<T>::SettlementPriceOutsideEmaBand);
                        let slice_c = Self::scale_part(position.collateral, slice_p, principal);
                        let slice_e = Self::scale_part(position.proceeds, slice_p, principal);
                        let slice_lift_tao =
                            Self::scale_part(position.lift_tao, slice_p, principal);
                        let slice_pot = slice_e.saturating_add(slice_c);
                        // Solvent path: prefer a full fill. Under force-market,
                        // fall back to forfeit (best-effort + shortfall) when
                        // the strict buyback cannot spend the pot (AMM drift
                        // vs the can_fully_close quote).
                        // Storage layer: a failed strict buyback has already
                        // traded; roll those chunks back before the forfeit
                        // fallback re-sizes from the untouched escrow.
                        let pre_spot = T::Pool::current_alpha_price(netuid);
                        let (paid, bought) =
                            match frame_support::storage::with_storage_layer(|| {
                                Self::execute_buyback(
                                    &escrow,
                                    &custody,
                                    netuid,
                                    slice_p,
                                    slice_pot,
                                    realized_guard,
                                )
                            }) {
                                Ok(pair) => pair,
                                Err(_) if force_market => {
                                    let fully_closed =
                                        Self::forfeit(owner, netuid, &position, false)?;
                                    if fully_closed {
                                        Self::emit_loan_expired(
                                            owner,
                                            netuid,
                                            side,
                                            expires_at,
                                            interest_until,
                                        );
                                    }
                                    return Ok(());
                                }
                                Err(e) => return Err(e),
                            };
                        let impact_fair = if bought > 0 {
                            Self::tao_for_alpha(bought, pre_spot, true)?
                        } else {
                            0
                        };
                        let impact_realized = paid;
                        let repaid_alpha = bought.min(slice_p);
                        let mut refund = slice_pot.saturating_sub(paid);
                        let excess = bought.saturating_sub(slice_p);
                        if excess > 0 {
                            if let Ok(dust) = T::Pool::sell_alpha(
                                &escrow,
                                &custody,
                                netuid,
                                excess.into(),
                                0u64.into(),
                                false,
                            ) {
                                refund = refund.saturating_add(dust.to_u64());
                            }
                        }
                        if refund > 0 {
                            T::Pool::transfer_tao(&escrow, owner, refund.into())?;
                        }
                        if repaid_alpha > 0 {
                            T::Pool::transfer_staked_alpha(
                                &escrow,
                                &custody,
                                &reserve,
                                &custody,
                                netuid,
                                repaid_alpha.into(),
                                false,
                                false,
                            )?;
                        }
                        T::Pool::readd_and_credit(
                            netuid,
                            &reserve,
                            &custody,
                            slice_lift_tao.into(),
                            repaid_alpha.into(),
                        )?;
                        Self::deposit_event(Event::Readded {
                            netuid,
                            tao: slice_lift_tao,
                            alpha: repaid_alpha,
                        });
                        Lifts::<T>::mutate(netuid, |book| {
                            book.l_s = book.l_s.saturating_sub(slice_lift_tao);
                            book.held_tao = book.held_tao.saturating_sub(slice_lift_tao);
                            book.held_alpha = book.held_alpha.saturating_sub(Self::scale_part(
                                position.lift_alpha.saturating_sub(position.principal),
                                slice_p,
                                principal,
                            ));
                            book.escrow_tao = book.escrow_tao.saturating_sub(slice_e);
                            book.short_sold_alpha = book.short_sold_alpha.saturating_sub(slice_p);
                        });
                        if slice_p < principal {
                            // Partial forced settle: note impact, shrink, leave
                            // the remainder for ForcedSettlementRetryDelay.
                            Self::note_protocol_forced_impact(netuid, impact_fair, impact_realized);
                            let slice_lift_alpha =
                                Self::scale_part(position.lift_alpha, slice_p, principal);
                            Self::maybe_burn_invariant_surplus(
                                netuid,
                                slice_lift_tao,
                                repaid_alpha,
                                slice_lift_tao,
                                slice_lift_alpha,
                                false,
                            )?;
                            let rem_principal = position.principal.saturating_sub(slice_p);
                            position.principal = rem_principal;
                            position.collateral = position.collateral.saturating_sub(slice_c);
                            position.proceeds = position.proceeds.saturating_sub(slice_e);
                            position.lift_tao = position.lift_tao.saturating_sub(slice_lift_tao);
                            position.lift_alpha =
                                position.lift_alpha.saturating_sub(slice_lift_alpha);
                            position.annual_interest = Self::scale_part(
                                position.annual_interest,
                                rem_principal,
                                principal,
                            );
                            Positions::<T>::insert(owner, netuid, &position);
                            return Ok(());
                        }
                        (
                            slice_lift_tao,
                            repaid_alpha,
                            paid,
                            paid,
                            refund,
                            impact_fair,
                            impact_realized,
                        )
                    }
                    Side::Long => {
                        let mut raised = 0u64;
                        let mut sold_alpha = 0u64;
                        let skip_flow = Self::skip_long_tao_flow(netuid);
                        if position.escrow_alpha > 0 {
                            let out = T::Pool::forced_sell_into_escrow(
                                &escrow,
                                &custody,
                                &escrow,
                                netuid,
                                position.escrow_alpha.into(),
                                0u64.into(),
                                skip_flow,
                            )?;
                            raised = Self::add(raised, out.to_u64())?;
                            sold_alpha = Self::add(sold_alpha, position.escrow_alpha)?;
                            if realized_guard {
                                Self::ensure_realized_sell_within_slippage(
                                    netuid, raised, sold_alpha,
                                )?;
                            }
                        }
                        let mut coll_left = position.collateral;
                        if raised < position.principal && coll_left > 0 {
                            let need = position.principal.saturating_sub(raised);
                            let needed_alpha = Self::alpha_to_raise_tao(netuid, need)?;
                            let sell = needed_alpha.min(coll_left);
                            ensure!(sell > 0, Error::<T>::InsufficientEscrow);
                            let out = T::Pool::forced_sell_into_escrow(
                                &escrow,
                                &custody,
                                &escrow,
                                netuid,
                                sell.into(),
                                0u64.into(),
                                skip_flow,
                            )?;
                            raised = Self::add(raised, out.to_u64())?;
                            sold_alpha = Self::add(sold_alpha, sell)?;
                            coll_left = coll_left.saturating_sub(sell);
                            if realized_guard {
                                Self::ensure_realized_sell_within_slippage(
                                    netuid, raised, sold_alpha,
                                )?;
                            }
                        }
                        ensure!(raised >= position.principal, Error::<T>::InsufficientEscrow);
                        // Spot-relative ProtocolForcedImpact (sells); EMA still
                        // gates the per-trade realized band above.
                        let impact_fair = Self::fair_spot_tao(netuid, sold_alpha, false)?;
                        let impact_realized = raised;
                        let repay = position.principal;
                        let surplus = raised.saturating_sub(repay);
                        T::Pool::transfer_tao(&escrow, &reserve, repay.into())?;
                        if surplus > 0 {
                            T::Pool::transfer_tao(&escrow, owner, surplus.into())?;
                        }
                        T::Pool::readd_and_credit(
                            netuid,
                            &reserve,
                            &custody,
                            repay.into(),
                            position.lift_alpha.into(),
                        )?;
                        Self::deposit_event(Event::Readded {
                            netuid,
                            tao: repay,
                            alpha: position.lift_alpha,
                        });
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
                        leftover_collateral = coll_left;
                        (
                            repay,
                            position.lift_alpha,
                            repay,
                            repay,
                            surplus,
                            impact_fair,
                            impact_realized,
                        )
                    }
                };

            Self::maybe_burn_invariant_surplus(
                netuid,
                credited_tao,
                credited_alpha,
                position.lift_tao,
                position.lift_alpha,
                false,
            )?;
            // Leftover long C is not needed to make the pool whole: return it
            // as staked alpha (same economic outcome as `do_close`) instead of
            // force-selling through the realized band / ProtocolForcedImpact.
            // That sell was freezing thin-pool longs with no manipulation
            // (clone retest F1). Shorts have no analogous leftover-C sale.
            if leftover_collateral > 0 {
                T::Pool::transfer_staked_alpha(
                    &escrow,
                    &custody,
                    owner,
                    &custody,
                    netuid,
                    leftover_collateral.into(),
                    false,
                    false,
                )?;
            }
            Self::note_protocol_forced_impact(netuid, impact_fair, impact_realized);
            Self::remove_position(owner, netuid, &position);
            Self::deposit_event(Event::Expired {
                owner: owner.clone(),
                netuid,
                side,
                principal_tao_equiv: match side {
                    Side::Short => paid,
                    Side::Long => position.principal,
                },
                principal: position.principal,
                payment: paid,
                tao_to_pool: to_pool,
                tao_refund: refund,
                spot_price: spot_price_for_event,
                shortfall_alpha: 0,
                shortfall_tao: 0,
            });
            Self::emit_loan_expired(owner, netuid, side, expires_at, interest_until);
            Self::bump_stats(netuid, |s| {
                s.expiry_settlement_count = s.expiry_settlement_count.saturating_add(1);
            });
            Ok(())
        }

        pub fn finish_dissolution(netuid: NetUid) -> DispatchResult {
            Dissolutions::<T>::remove(netuid);
            DissolutionCursor::<T>::remove(netuid);
            DissolutionPhase::<T>::remove(netuid);
            References::<T>::remove(netuid);
            Lifts::<T>::remove(netuid);
            let _ = InvariantBaseline::<T>::take(netuid);
            LongInterestPending::<T>::remove(netuid);
            LongInterestSince::<T>::remove(netuid);
            // Per-netuid accumulators may be cleaned up; globals stay.
            StatsByNetuid::<T>::remove(netuid);
            Self::forget_active_net(netuid);
            TaoEma::<T>::remove(netuid);
            Ok(())
        }

        pub fn quote_open(
            netuid: NetUid,
            side: Side,
            collateral: u64,
        ) -> Result<OpeningQuote, DispatchError> {
            Self::quote_open_for_inner(None, netuid, side, collateral, None, None)
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
            Self::quote_open_for_inner(Some(owner), netuid, side, collateral, source_hotkey, None)
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
            Self::quote_open_for_inner(
                Some(owner),
                netuid,
                side,
                collateral,
                source_hotkey,
                Some(principal),
            )
        }

        fn quote_open_for_inner(
            owner: Option<&T::AccountId>,
            netuid: NetUid,
            side: Side,
            collateral: u64,
            source_hotkey: Option<&T::AccountId>,
            requested: Option<u64>,
        ) -> Result<OpeningQuote, DispatchError> {
            // Accrual / forfeit mutates pool + capacity; always roll back so a
            // quote cannot persist those effects while still matching dispatch.
            use frame_support::storage::{TransactionOutcome, with_transaction};
            with_transaction(|| {
                TransactionOutcome::Rollback(Self::quote_open_for_inner_after_accrual(
                    owner,
                    netuid,
                    side,
                    collateral,
                    source_hotkey,
                    requested,
                ))
            })
        }

        fn quote_open_for_inner_after_accrual(
            owner: Option<&T::AccountId>,
            netuid: NetUid,
            side: Side,
            collateral: u64,
            source_hotkey: Option<&T::AccountId>,
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
            // Long owner-aware quotes check source stake like `lock_alpha_collateral`.
            if let (Side::Long, Some(o), Some(hk)) = (side, owner, source_hotkey) {
                T::Pool::ensure_hotkey_account(hk)?;
                T::Pool::ensure_collateral_stake(hk, o, netuid, collateral.into())?;
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
                // Exact-principal path: same order as `do_open` — side capacity
                // first (hard), then LTV/collateral. Return
                // `InsufficientCollateral` (not `DynamicCollateral`) so
                // `lending_required_collateral` can search higher without
                // treating capacity/EMA refusals as soft.
                let old_p = existing.as_ref().map(|x| x.principal).unwrap_or(0);
                let total_p = Self::add(old_p, p)?;
                let total_c = Self::add(
                    existing.as_ref().map(|x| x.collateral).unwrap_or(0),
                    collateral,
                )?;
                let ltv = Self::ltv_tier_bps(netuid, side, &params);
                let delta_l = Self::lift_tao_for_principal(netuid, side, p)?;
                Self::ensure_side_capacity(netuid, side, delta_l)?;
                if side == Side::Short {
                    Self::ensure_short_book_pool_alpha(netuid, p)?;
                }
                if side == Side::Long {
                    Self::ensure_long_book_pool_tao(netuid, p)?;
                }
                ensure!(
                    Self::principal_feasible(
                        netuid, side, total_p, total_c, old_p, owner, &reference, ltv, &params,
                    )?,
                    Error::<T>::InsufficientCollateral
                );
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
            if side == Side::Short && requested.is_none() {
                Self::ensure_short_book_pool_alpha(netuid, principal)?;
            }
            if side == Side::Long && requested.is_none() {
                Self::ensure_long_book_pool_tao(netuid, principal)?;
            }
            let price = Self::coupon_price(netuid, &reference);
            // Zero additional principal: map via shared dust/capacity gate before
            // V_dyn / EMA (those require a positive principal).
            if principal == 0 {
                return Err(Self::refuse_dust_or_capacity(netuid, side, price, &params));
            }
            // Post-trade EMA band for both exact and collateral-only sizing
            // (same gate as `do_open`; catches OpenLoan / quote_open without amount).
            let (spot_after, escrow_estimate) = Self::simulate_open_trade(netuid, side, principal)?;
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
            // `escrow_estimate`: short = simulated TAO proceeds; long = simulated
            // escrowed alpha. CLI floors min_proceeds / min_escrow on this (N3 / F10).
            Self::validate_sized_principal(
                netuid,
                side,
                principal,
                price,
                collateral,
                v,
                escrow_estimate,
                &params,
            )
        }

        /// Approximate post-forced-trade spot and escrow fill after a
        /// proportional lift + buy/sell. Price ∝ reserve ratio.
        ///
        /// Returns `(spot_after, escrow_out)` where `escrow_out` is TAO proceeds
        /// for a short sale or alpha escrowed for a long buy — the same fill the
        /// open path realises (fee-free quote_sell_on / quote_buy_on).
        fn simulate_open_trade(
            netuid: NetUid,
            side: Side,
            principal: u64,
        ) -> Result<(U64F64, u64), DispatchError> {
            let spot_before = T::Pool::current_alpha_price(netuid);
            let y = T::Pool::pool_tao(netuid).to_u64();
            let x = T::Pool::pool_alpha(netuid).to_u64();
            ensure!(y > 0 && x > 0, Error::<T>::EmptyPool);
            let (g_numer, g_denom) = Self::lift_fraction(netuid, side, principal)?;
            let lift_tao =
                ((u128::from(y) * u128::from(g_numer)) / u128::from(g_denom.max(1))) as u64;
            let lift_alpha =
                ((u128::from(x) * u128::from(g_numer)) / u128::from(g_denom.max(1))) as u64;
            let y1 = y.saturating_sub(lift_tao);
            let x1 = x.saturating_sub(lift_alpha);
            ensure!(y1 > 0 && x1 > 0, Error::<T>::EmptyPool);
            let w = T::Pool::quote_weight_parts(netuid);
            let (y2, x2, escrow_out) = match side {
                Side::Long => {
                    let a_out =
                        T::Pool::quote_buy_on(y1.into(), x1.into(), w, principal.into()).to_u64();
                    (
                        y1.saturating_add(principal),
                        x1.saturating_sub(a_out),
                        a_out,
                    )
                }
                Side::Short => {
                    let t_out =
                        T::Pool::quote_sell_on(y1.into(), x1.into(), w, principal.into()).to_u64();
                    (
                        y1.saturating_sub(t_out),
                        x1.saturating_add(principal),
                        t_out,
                    )
                }
            };
            ensure!(y2 > 0 && x2 > 0, Error::<T>::Arithmetic);
            let before_ratio = U64F64::from_num(y1) / U64F64::from_num(x1);
            let after_ratio = U64F64::from_num(y2) / U64F64::from_num(x2);
            let factor = after_ratio
                .checked_div(before_ratio)
                .ok_or(Error::<T>::Arithmetic)?;
            let spot_after = spot_before
                .checked_mul(factor)
                .ok_or(Error::<T>::Arithmetic)?;
            Ok((spot_after, escrow_out))
        }

        pub fn quote_close(
            owner: &T::AccountId,
            netuid: NetUid,
            hotkey: Option<&T::AccountId>,
            repay_from_wallet: bool,
        ) -> Result<ClosingQuote, DispatchError> {
            // Match `do_close` / `do_repay`: repayments stay available when new
            // borrowing is disabled (`Enabled = false`).
            Self::ensure_subnet_repayable(netuid)?;
            let position = Positions::<T>::get(owner, netuid).ok_or(Error::<T>::PositionMissing)?;
            let (interest, _) = Self::interest_due(
                netuid,
                &position,
                frame_system::Pallet::<T>::block_number(),
                true,
            )?;
            let remaining = position.collateral.saturating_sub(interest);
            let quoted = match position.side {
                Side::Short => {
                    let pot = Self::add(remaining, position.proceeds)?;
                    if repay_from_wallet {
                        ClosingQuote {
                            payment: position.principal,
                            refund: pot,
                            escrow_return: position.proceeds,
                            shortfall: 0,
                        }
                    } else {
                        // Escrow buyback: payment ≈ TAO to buy P alpha. When
                        // payment > pot the short is underwater — report the
                        // exact shortfall so CLI/SDK can size max_payment and
                        // surface a clear free-balance requirement (E2E F4).
                        let payment = Self::tao_to_buy_alpha(netuid, position.principal)?;
                        let shortfall = payment.saturating_sub(pot);
                        ClosingQuote {
                            payment,
                            refund: pot.saturating_sub(payment),
                            escrow_return: 0,
                            shortfall,
                        }
                    }
                }
                Side::Long => {
                    if repay_from_wallet {
                        ClosingQuote {
                            payment: position.principal,
                            refund: remaining.saturating_add(position.escrow_alpha),
                            escrow_return: position.escrow_alpha,
                            shortfall: 0,
                        }
                    } else {
                        // Fee-bearing sell of A, then minimal C (same path as
                        // `do_close` / `can_fully_close_from_escrow`). Fee-free
                        // `quote_sell_on` overstates refunds (Auditor fid:178e1fb3).
                        // When A+C cannot cover P, report shortfall (do not Err)
                        // so CLI/SDK can flag insolvent longs (E2E U2).
                        let (raised, coll_left) = Self::simulate_long_escrow_raise(
                            netuid,
                            &position.custody,
                            position.escrow_alpha,
                            remaining,
                            position.principal,
                        )?;
                        ClosingQuote {
                            payment: position.principal,
                            refund: coll_left,
                            escrow_return: 0,
                            shortfall: position.principal.saturating_sub(raised),
                        }
                    }
                }
            };
            // Hotkey is required when alpha is repaid from or refunded to stake.
            // Short escrow buyback refunds TAO only (overshoot dust does not
            // require a registered destination — see `do_close`).
            // `None` skips the check (must not panic); callers that omit it
            // accept that the extrinsic may still refuse an unregistered hotkey.
            if Self::close_requires_hotkey(position.side, repay_from_wallet, &quoted) {
                if let Some(hk) = hotkey {
                    T::Pool::ensure_hotkey_account(hk)?;
                }
            }
            Ok(quoted)
        }

        /// Partial-repay quote: same `ClosingQuote` shape as [`Self::quote_close`],
        /// sized for `principal_part` via the same pro-rata slice as `do_repay`.
        /// `principal_part == principal` delegates to `quote_close`.
        pub fn quote_repay(
            owner: &T::AccountId,
            netuid: NetUid,
            hotkey: Option<&T::AccountId>,
            principal_part: u64,
            repay_from_wallet: bool,
        ) -> Result<ClosingQuote, DispatchError> {
            Self::ensure_subnet_repayable(netuid)?;
            ensure!(principal_part > 0, Error::<T>::PrincipalZero);
            let position = Positions::<T>::get(owner, netuid).ok_or(Error::<T>::PositionMissing)?;
            ensure!(
                principal_part <= position.principal,
                Error::<T>::AmountTooSmall
            );
            if principal_part == position.principal {
                return Self::quote_close(owner, netuid, hotkey, repay_from_wallet);
            }
            Self::ensure_partial_repay_sizes(
                position.side,
                position.principal,
                principal_part,
                T::Pool::current_alpha_price(netuid),
            )?;
            let (interest, _) = Self::interest_due(
                netuid,
                &position,
                frame_system::Pallet::<T>::block_number(),
                true,
            )?;
            let remaining = position.collateral.saturating_sub(interest);
            let p = position.principal;
            let slice_p = principal_part;
            let slice_c = Self::scale_part(remaining, slice_p, p);
            let slice_e_s = Self::scale_part(position.proceeds, slice_p, p);
            let slice_a = Self::scale_part(position.escrow_alpha, slice_p, p);
            let quoted = match position.side {
                Side::Short => {
                    let pot = Self::add(slice_c, slice_e_s)?;
                    if repay_from_wallet {
                        ClosingQuote {
                            payment: slice_p,
                            refund: pot,
                            escrow_return: slice_e_s,
                            shortfall: 0,
                        }
                    } else {
                        let payment = Self::tao_to_buy_alpha(netuid, slice_p)?;
                        let shortfall = payment.saturating_sub(pot);
                        ClosingQuote {
                            payment,
                            refund: pot.saturating_sub(payment),
                            escrow_return: 0,
                            shortfall,
                        }
                    }
                }
                Side::Long => {
                    if repay_from_wallet {
                        ClosingQuote {
                            payment: slice_p,
                            refund: slice_c.saturating_add(slice_a),
                            escrow_return: slice_a,
                            shortfall: 0,
                        }
                    } else {
                        // Mirror quote_close with fee-bearing sells (fid:178e1fb3).
                        let (raised, coll_left) = Self::simulate_long_escrow_raise(
                            netuid,
                            &position.custody,
                            slice_a,
                            slice_c,
                            slice_p,
                        )?;
                        ClosingQuote {
                            payment: slice_p,
                            refund: coll_left,
                            escrow_return: 0,
                            shortfall: slice_p.saturating_sub(raised),
                        }
                    }
                }
            };
            if Self::close_requires_hotkey(position.side, repay_from_wallet, &quoted) {
                if let Some(hk) = hotkey {
                    T::Pool::ensure_hotkey_account(hk)?;
                }
            }
            Ok(quoted)
        }

        /// Whether close moves alpha via the call-supplied hotkey.
        fn close_requires_hotkey(
            side: Side,
            repay_from_wallet: bool,
            quoted: &ClosingQuote,
        ) -> bool {
            match side {
                // Wallet alpha repay always draws from the hotkey.
                Side::Short => repay_from_wallet,
                // Long refund is alpha; only required when some alpha returns.
                Side::Long => quoted.refund > 0 || quoted.escrow_return > 0,
            }
        }

        pub fn quote_raise(
            owner: &T::AccountId,
            netuid: NetUid,
            source_hotkey: &T::AccountId,
            amount: Option<u64>,
            collateral: Option<u64>,
            sell: Option<u64>,
        ) -> Result<RaiseQuote, DispatchError> {
            // Accrual / forfeit mutates pool + capacity; roll back after sizing.
            use frame_support::storage::{TransactionOutcome, with_transaction};
            with_transaction(|| {
                TransactionOutcome::Rollback(Self::quote_raise_after_accrual(
                    owner,
                    netuid,
                    source_hotkey,
                    amount,
                    collateral,
                    sell,
                ))
            })
        }

        fn quote_raise_after_accrual(
            owner: &T::AccountId,
            netuid: NetUid,
            source_hotkey: &T::AccountId,
            amount: Option<u64>,
            collateral: Option<u64>,
            sell: Option<u64>,
        ) -> Result<RaiseQuote, DispatchError> {
            ensure!(
                amount.is_some() || collateral.is_some(),
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
            let (c, p) = if let (Some(col), Some(p)) = (collateral, amount) {
                ensure!(col > 0, Error::<T>::CollateralZero);
                ensure!(p > 0, Error::<T>::PrincipalZero);
                let old_p = existing.as_ref().map(|x| x.principal).unwrap_or(0);
                let total_p = Self::add(old_p, p)?;
                let total_c = Self::add(existing.as_ref().map(|x| x.collateral).unwrap_or(0), col)?;
                let ltv = Self::ltv_tier_bps(netuid, Side::Long, &params);
                let delta_l = Self::lift_tao_for_principal(netuid, Side::Long, p)?;
                Self::ensure_side_capacity(netuid, Side::Long, delta_l)?;
                Self::ensure_long_book_pool_tao(netuid, p)?;
                ensure!(
                    Self::principal_feasible(
                        netuid,
                        Side::Long,
                        total_p,
                        total_c,
                        old_p,
                        Some(owner),
                        &reference,
                        ltv,
                        &params,
                    )?,
                    Error::<T>::DynamicCollateral
                );
                (col, p)
            } else if let Some(col) = collateral {
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
                Self::ensure_long_book_pool_tao(netuid, p)?;
                // Exponential search for a feasible upper bound, then bisect.
                // A fixed hi=u64::MAX/4 with only 16 steps left a 2^46 artifact
                // (Skeptic/E2E F2); doubling finds a tight window in ≤63 steps.
                let old_c = existing.as_ref().map(|x| x.collateral).unwrap_or(0);
                let feasible = |mid: u64| -> Result<bool, DispatchError> {
                    let total_c = Self::add(old_c, mid)?;
                    Self::principal_feasible(
                        netuid,
                        Side::Long,
                        total_p,
                        total_c,
                        old_p,
                        Some(owner),
                        &reference,
                        ltv,
                        &params,
                    )
                };
                let mut hi = 1u64;
                let mut found_hi = false;
                while hi < u64::MAX / 4 {
                    if feasible(hi)? {
                        found_hi = true;
                        break;
                    }
                    let next = hi.saturating_mul(2);
                    if next <= hi {
                        break;
                    }
                    hi = next;
                }
                ensure!(
                    found_hi || feasible(hi)?,
                    Error::<T>::InsufficientCollateral
                );
                let mut lo = hi / 2;
                if lo == 0 {
                    lo = 1;
                }
                let mut best = Some(hi);
                for _ in 0..BISECTION_STEPS {
                    if lo > hi {
                        break;
                    }
                    let mid = lo.saturating_add(hi.saturating_sub(lo) / 2);
                    if feasible(mid)? {
                        best = Some(mid);
                        hi = mid.saturating_sub(1);
                    } else {
                        lo = mid.saturating_add(1);
                    }
                }
                let best = best.ok_or(Error::<T>::InsufficientCollateral)?;
                (best, p)
            };
            let price = Self::coupon_price(netuid, &reference);
            // Same dust / capacity / minimum_loan_value gate as `do_open`.
            if p == 0 {
                return Err(Self::refuse_dust_or_capacity(
                    netuid,
                    Side::Long,
                    price,
                    &params,
                ));
            }
            let interest_alpha = Self::alpha_for_tao(p, price, true)?;
            let annual_interest =
                Self::mul_bps(interest_alpha, Self::apr_bps_for_side(Side::Long, &params))?;
            if annual_interest == 0 || p < Self::minimum_loan_value() {
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
            // Same EMA gate as `do_open` for the long leg (E2E N1).
            Self::ensure_open_ema_band(Side::Long, price_after_buy, reference.price, &params)?;
            let s = sell.unwrap_or(a);
            // Same stake gates as `do_open_long_and_sell` (collateral + sale).
            T::Pool::ensure_collateral_stake(source_hotkey, owner, netuid, c.into())?;
            let unlocked = T::Pool::unlocked_stake(source_hotkey, owner, netuid).to_u64();
            ensure!(
                unlocked >= c.saturating_add(s),
                Error::<T>::InsufficientSellStake
            );
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
                if Self::take_interest_retry(position.due, old, netuid) {
                    Self::enqueue_interest_retry(new, netuid, position.due);
                    rekeyed_due = true;
                }
                if Expiry::<T>::take(position.expiry_due, (old, netuid)).is_some() {
                    Expiry::<T>::insert(position.expiry_due, (new, netuid), ());
                    rekeyed_expiry = true;
                }
                if Self::take_expiry_retry(position.expiry_due, old, netuid) {
                    Self::enqueue_expiry_retry(new, netuid, position.expiry_due);
                    rekeyed_expiry = true;
                }
                // Move LastDeferral with the position so the old coldkey cannot
                // inherit a stale deferral when it opens again (Auditor fid:0566f709).
                if let Some(deferred) = LastDeferral::<T>::take(old, netuid) {
                    LastDeferral::<T>::insert(new, netuid, deferred);
                }
                if let Some(since) = BandDeferralSince::<T>::take(old, netuid) {
                    BandDeferralSince::<T>::insert(new, netuid, since);
                }
            }
            if rekeyed_due {
                DueBucketCursor::<T>::kill();
                InterestRetryCursor::<T>::kill();
            }
            if rekeyed_expiry {
                ExpiryBucketCursor::<T>::kill();
                ExpiryRetryCursor::<T>::kill();
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
            let _ = Self::try_seed_reference(netuid);
        }
        fn on_subtoken_enabled(netuid: NetUid) {
            let _ = Self::try_seed_reference(netuid);
        }
        fn emission_reserve_offsets(netuid: NetUid) -> (u64, u64, u64, u64, u64) {
            let book = Lifts::<T>::get(netuid);
            (
                book.held_tao,
                book.held_alpha,
                book.escrow_tao,
                book.escrow_alpha,
                book.short_sold_alpha,
            )
        }
    }
}
