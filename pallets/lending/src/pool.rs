//! The pool adapter the pallet reads through and moves funds through (DESIGN-v2 section 2).
//!
//! `pallet_lending` does not depend on `pallet_subtensor`; subtensor implements this trait. The
//! reads feed the gates and quotes. The legs move TAO, alpha, reserves and issuance; the pallet
//! checks every gate and computes every quote before it calls a leg, so a leg never decides a
//! position's outcome. Each swap leg moves the reserves as `Config::Curve` quotes it from the
//! pool's reserves, weight and fee, and returns the amount the pool actually gave; the pallet
//! refuses a leg whose result differs from its quote.

use alloc::vec::Vec;
use frame_support::dispatch::DispatchResult;
use sp_arithmetic::Perquintill;
use sp_runtime::DispatchError;
use subtensor_runtime_common::NetUid;

use crate::math::{curve::FeeRate, fixed::Price};

/// Read access to subnet pools and the write legs of the positions. `AccountId` is the holder
/// of TAO balances and of the alpha that custody keeps.
pub trait LendingPoolInterface<AccountId> {
    /// Every live non-root subnet: the set that must hold price rings (SPEC I25).
    fn live_subnets() -> Vec<NetUid>;
    /// Upper bound on `live_subnets().len()` (the registration `SubnetLimit`), used to charge
    /// `set_enabled` for its re-seed (SPEC section 9 item 3).
    fn subnet_limit() -> u32;
    /// Live pool spot, rao TAO per rao alpha, with no add-back of minted alpha (SPEC P14).
    fn spot_price(netuid: NetUid) -> Price;
    /// Live pool TAO reserve (`SubnetTAO`) in rao; lending custody is not in it.
    fn pool_tao(netuid: NetUid) -> u64;
    /// The emission EMA (`SubnetMovingPrice`). The only permitted reads are the rank-ring
    /// seeds at the runtime upgrade and on enable (SPEC 8.7, section 9 items 1 and 3).
    fn moving_alpha_price(netuid: NetUid) -> Price;
    /// Live pool alpha reserve (`SubnetAlphaIn`) in rao.
    fn subnet_alpha_in(netuid: NetUid) -> u64;
    /// The pool's Balancer quote (TAO) weight; 1/2 is constant product.
    fn quote_weight(netuid: NetUid) -> Perquintill;
    /// The fee rate the pool's swaps take from their input, in units of 1/65535.
    fn swap_fee(netuid: NetUid) -> FeeRate;
    /// Total alpha issued on the subnet (`SubnetAlphaOut` plus the pool reserve), in rao. The
    /// synthetic mint counts against the supply cap (SPEC P23).
    fn alpha_issuance(netuid: NetUid) -> u64;
    /// Free TAO balance of `who` in rao.
    fn tao_balance(who: &AccountId) -> u64;
    /// Alpha of `netuid` held by `who` (the custody's locked alpha for a long).
    fn held_alpha(who: &AccountId, netuid: NetUid) -> u64;

    /// Registers `hotkey`, the hotkey custody alpha is staked on, as owned by `coldkey`. A
    /// no-op if it is already registered to `coldkey`.
    fn register_pallet_hotkey(coldkey: &AccountId, hotkey: &AccountId) -> DispatchResult;

    /// Moves `amount` TAO from `from` to `to`. Returns the TAO moved. Like every leg below, it
    /// records no TAO flow, protocol flow or subnet volume (SPEC I18, I19).
    fn transfer_tao(from: &AccountId, to: &AccountId, amount: u64) -> Result<u64, DispatchError>;
    /// Moves `amount` TAO from `from` into the subnet pool's TAO reserve: interest, repayment
    /// of a long's debt, and the fee-net part of a buy-back. Pool-internal; not flow (SPEC I19).
    /// Returns the TAO credited.
    fn credit_pool_tao(netuid: NetUid, from: &AccountId, amount: u64)
    -> Result<u64, DispatchError>;
    /// Mints `q` alpha, sells it into the pool and pays the TAO out to `to`. Returns the TAO
    /// paid (`P`).
    fn mint_and_sell(netuid: NetUid, q: u64, to: &AccountId) -> Result<u64, DispatchError>;
    /// Buys `alpha` out of the pool with TAO from `from` and burns it. Returns the TAO cost.
    fn buy_and_burn(netuid: NetUid, from: &AccountId, alpha: u64) -> Result<u64, DispatchError>;
    /// Lifts `lift` TAO out of the pool's reserve, swaps it for alpha and credits that alpha to
    /// `to`'s custody. Returns the alpha bought (`A`).
    fn lift_and_buy(netuid: NetUid, lift: u64, to: &AccountId) -> Result<u64, DispatchError>;
    /// Sells `alpha` from `from`'s custody into the pool and pays the TAO out to `from`.
    /// Returns the TAO paid.
    fn sell_alpha(netuid: NetUid, from: &AccountId, alpha: u64) -> Result<u64, DispatchError>;
}

/// What the benchmarks need from the runtime to build the pools the calls run against. The
/// runtime implements it over the real subnet pools, so the benchmarks measure the adapter's
/// legs; `()` is for the mocks of pallets that only carry this pallet's `Config`.
#[cfg(feature = "runtime-benchmarks")]
pub trait BenchmarkHelper<AccountId> {
    /// Removes every live non-root subnet, so that a benchmark sets the number of them.
    fn clear_subnets();
    /// Makes `netuid` a live subnet whose pool holds `tao` and `alpha` in reserve, backed as
    /// the chain backs a pool, with the swap fee on. Does not seed the lending price rings.
    fn setup_pool(netuid: NetUid, tao: u64, alpha: u64);
    /// Adds `tao` to the free balance of `who`.
    fn fund(who: &AccountId, tao: u64);
    /// Gives the current block a known author, so that a swap leg settles its fee to an
    /// account, the costlier of its two paths.
    fn set_block_author();
}

#[cfg(feature = "runtime-benchmarks")]
impl<AccountId> BenchmarkHelper<AccountId> for () {
    fn clear_subnets() {}
    fn setup_pool(_netuid: NetUid, _tao: u64, _alpha: u64) {}
    fn fund(_who: &AccountId, _tao: u64) {}
    fn set_block_author() {}
}
