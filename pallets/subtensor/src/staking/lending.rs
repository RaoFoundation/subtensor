//! The lending pallet's pricing function on subnet pools (pallet_lending DESIGN-v2 section 4 and
//! section 10 item 3).
//!
//! Subnet pools are Balancer weighted pools. [`BalancerCurve`] implements `pallet_lending`'s
//! `Curve` with the swap pallet's own pure functions (`pallet_subtensor_swap::balancer`): the
//! fee is [`fee_amount`], a buy converts with `Balancer::alpha_out_for_tao_in` and a sell with
//! `Balancer::tao_out_for_alpha_in`. Those are the functions every swap executes, so a quote
//! equals what the swap returns on the same reserves, weight and fee rate.
//!
//! At quote weight 1/2 the pool is constant product and the quotes agree with the lending
//! model's `ConstantProduct` up to rounding; `tests::lending_curve` measures the difference.

//!
//! The second half of the file is the pool adapter: `pallet_lending`'s `LendingPoolInterface`
//! on this pallet. Its legs move TAO, alpha, reserves and issuance with the same swap and the
//! same fee settlement as staking, and record no TAO flow, protocol flow or subnet volume
//! (pallet_lending SPEC I18, I19).

use frame_support::{dispatch::DispatchResult, ensure};
use pallet_lending::{
    LendingInterface, LendingPoolInterface,
    math::{
        curve::{Curve, FeeRate, PoolState},
        fixed::Price,
    },
};
use pallet_subtensor_swap::balancer::{Balancer, fee_amount};
use sp_runtime::traits::Zero;
use sp_runtime::{DispatchError, Perquintill};
use sp_std::vec::Vec;
use subtensor_runtime_common::{AlphaBalance, AuthorshipInfo, NetUid, TaoBalance, Token};
use subtensor_swap_interface::SwapHandler;

use crate::{
    Config, Error, NetworksAdded, OwnedHotkeys, Owner, Pallet, StakingHotkeys, SubnetAlphaIn,
    SubnetAlphaOut, SubnetLimit, SubnetTAO, TotalStake,
};

/// Fee rates are parts of `u16::MAX`.
const FEE_DEN: u128 = 65_535;

/// Balancer weighted-pool pricing, identical to the swap pallet's.
pub struct BalancerCurve;

/// The pool's balancer. `None` if the weight is outside the range the swap pallet accepts; no
/// live pool has such a weight.
fn balancer(pool: &PoolState) -> Option<Balancer> {
    Balancer::new(pool.quote_weight).ok()
}

/// The part of a swap input that reaches the curve: the input less the swap pallet's fee.
fn net_of_fee(pool: &PoolState, amount: u64) -> u64 {
    amount.saturating_sub(fee_amount(pool.fee, amount))
}

/// `ceil(net * 65535 / (65535 - fee))`: an input whose fee'd part is about `net`. Only a
/// starting point for [`smallest_input`].
fn gross_up(pool: &PoolState, net: u64) -> u64 {
    let keep = FEE_DEN.saturating_sub(u128::from(pool.fee));
    u128::from(net)
        .saturating_mul(FEE_DEN)
        .saturating_add(keep.saturating_sub(1))
        .checked_div(keep)
        .and_then(|gross| u64::try_from(gross).ok())
        .unwrap_or(u64::MAX)
}

/// The smallest input for which `reaches` holds, for a predicate that is false below some
/// input and true from it on. Starts at `estimate`, brackets the threshold by doubling steps
/// and bisects; each probe is one swap conversion, and an exact estimate costs two. `None` if
/// no `u64` input reaches.
fn smallest_input(estimate: u64, reaches: impl Fn(u64) -> bool) -> Option<u64> {
    let mut step = 1u64;
    // Invariant after bracketing: `reaches(high)` and not `reaches(low)`.
    let (mut low, mut high) = if reaches(estimate) {
        let mut high = estimate;
        loop {
            if high == 0 {
                return Some(0);
            }
            let probe = high.saturating_sub(step);
            if reaches(probe) {
                high = probe;
                step = step.saturating_mul(2);
            } else {
                break (probe, high);
            }
        }
    } else {
        let mut low = estimate;
        loop {
            let probe = low.checked_add(step)?;
            if reaches(probe) {
                break (low, probe);
            }
            low = probe;
            step = step.saturating_mul(2);
        }
    };
    while high.saturating_sub(low) > 1 {
        let mid = low.saturating_add(high.saturating_sub(low) >> 1);
        if reaches(mid) {
            high = mid;
        } else {
            low = mid;
        }
    }
    Some(high)
}

/// The smallest TAO input whose buy returns at least `alpha_out`, with the fee taken from the
/// input or fee-free. `None` if `alpha_out >= pool.alpha` or no input reaches it.
fn buy_input(pool: &PoolState, alpha_out: u64, with_fee: bool) -> Option<u64> {
    if alpha_out == 0 {
        return Some(0);
    }
    let left = pool.alpha.checked_sub(alpha_out).filter(|left| *left > 0)?;
    let balancer = balancer(pool)?;
    // The invariant's own answer, rounded up: y * ((x / (x - a))^(w1/w2) - 1).
    let net = balancer
        .tao_reserve_on_invariant(pool.tao, pool.alpha, left, true)?
        .saturating_sub(pool.tao);
    let estimate = if with_fee { gross_up(pool, net) } else { net };
    smallest_input(estimate, |tao_in| {
        let reaching = if with_fee {
            net_of_fee(pool, tao_in)
        } else {
            tao_in
        };
        balancer.alpha_out_for_tao_in(pool.tao, pool.alpha, reaching) >= alpha_out
    })
}

impl BalancerCurve {
    /// The hypothetical pool at spot `price` on the same weighted invariant (no fee):
    /// `X' = X * (p0 / p)^w_quote` rounded down, then `T' = T * (X / X')^(w_base / w_quote)`,
    /// which equals `T * (p / p0)^w_base` and keeps the integer state on the invariant. `T'` is
    /// rounded up when `tao_up`, else down. `None` if the state is not representable.
    #[must_use]
    pub fn at_price(pool: &PoolState, price: Price, tao_up: bool) -> Option<PoolState> {
        let balancer = balancer(pool)?;
        let alpha = balancer.alpha_reserve_at_price(pool.tao, pool.alpha, price)?;
        // A pump never adds alpha and a dump never removes it, whatever the rounding.
        let alpha = if tao_up {
            alpha.min(pool.alpha).max(1)
        } else {
            alpha.max(pool.alpha)
        };
        let tao = balancer.tao_reserve_on_invariant(pool.tao, pool.alpha, alpha, tao_up)?;
        Some(pool.with_reserves(tao, alpha))
    }
}

impl Curve for BalancerCurve {
    /// `(w_base / w_quote) * (tao / alpha)`, the swap pallet's `current_price`.
    fn spot(pool: &PoolState) -> Price {
        if pool.alpha == 0 {
            return Price::from_bits(0);
        }
        balancer(pool).map_or(Price::from_bits(0), |balancer| {
            balancer.calculate_price(pool.alpha, pool.tao)
        })
    }

    fn out_alpha_for_tao(pool: &PoolState, tao_in: u64) -> u64 {
        balancer(pool).map_or(0, |balancer| {
            balancer.alpha_out_for_tao_in(pool.tao, pool.alpha, net_of_fee(pool, tao_in))
        })
    }

    fn out_tao_for_alpha(pool: &PoolState, alpha_in: u64) -> u64 {
        balancer(pool).map_or(0, |balancer| {
            balancer.tao_out_for_alpha_in(pool.tao, pool.alpha, net_of_fee(pool, alpha_in))
        })
    }

    fn tao_in_for_alpha(pool: &PoolState, alpha_out: u64) -> Option<u128> {
        buy_input(pool, alpha_out, true).map(u128::from)
    }

    fn buy_cost_nofee(pool: &PoolState, alpha_out: u64) -> Option<u128> {
        buy_input(pool, alpha_out, false).map(u128::from)
    }

    fn sell_proceeds_nofee(pool: &PoolState, alpha_in: u64) -> u64 {
        balancer(pool).map_or(0, |balancer| {
            balancer.tao_out_for_alpha_in(pool.tao, pool.alpha, alpha_in)
        })
    }

    /// Rounds toward the pool as `ConstantProduct` does: alpha down, TAO up.
    fn pumped(pool: &PoolState, target: Price) -> Option<PoolState> {
        if Self::spot(pool) >= target {
            return Some(*pool);
        }
        Self::at_price(pool, target, true)
    }

    /// Rounds as `ConstantProduct` does: alpha down, TAO down.
    fn dumped(pool: &PoolState, target: Price) -> Option<PoolState> {
        if target == Price::from_bits(0) || Self::spot(pool) <= target {
            return Some(*pool);
        }
        Self::at_price(pool, target, false)
    }

    fn spot_after_buy(pool: &PoolState, alpha_out: u64) -> Option<Price> {
        let (after, _) = Self::after_buy_alpha(pool, alpha_out)?;
        (after.alpha > 0).then(|| Self::spot(&after))
    }

    fn after_sell(pool: &PoolState, alpha_in: u64) -> Option<(PoolState, u64)> {
        let tao_out = Self::out_tao_for_alpha(pool, alpha_in);
        let tao = pool.tao.checked_sub(tao_out)?;
        let alpha = pool.alpha.checked_add(net_of_fee(pool, alpha_in))?;
        Some((pool.with_reserves(tao, alpha), tao_out))
    }

    /// The pool gives exactly `alpha_out`; alpha the minimal input would buy beyond it stays in
    /// the reserve.
    fn after_buy_alpha(pool: &PoolState, alpha_out: u64) -> Option<(PoolState, u64)> {
        let cost = buy_input(pool, alpha_out, true)?;
        let tao = pool.tao.checked_add(net_of_fee(pool, cost))?;
        let alpha = pool.alpha.checked_sub(alpha_out)?;
        Some((pool.with_reserves(tao, alpha), cost))
    }

    fn after_buy_tao(pool: &PoolState, tao_in: u64) -> Option<(PoolState, u64)> {
        let alpha_out = Self::out_alpha_for_tao(pool, tao_in);
        let tao = pool.tao.checked_add(net_of_fee(pool, tao_in))?;
        let alpha = pool.alpha.checked_sub(alpha_out)?;
        Some((pool.with_reserves(tao, alpha), alpha_out))
    }
}

impl<T: Config + pallet_subtensor_swap::Config> Pallet<T> {
    /// The live pool of `netuid` as [`BalancerCurve`] prices it.
    #[must_use]
    pub fn lending_pool_state(netuid: NetUid) -> PoolState {
        PoolState {
            tao: SubnetTAO::<T>::get(netuid).to_u64(),
            alpha: SubnetAlphaIn::<T>::get(netuid).to_u64(),
            quote_weight: pallet_subtensor_swap::SwapBalancer::<T>::get(netuid).get_quote_weight(),
            fee: pallet_subtensor_swap::FeeRate::<T>::get(netuid),
        }
    }
}

impl<T: Config + pallet_subtensor_swap::Config> LendingPoolInterface<T::AccountId> for Pallet<T> {
    fn live_subnets() -> Vec<NetUid> {
        NetworksAdded::<T>::iter()
            .filter(|(netuid, added)| *added && !netuid.is_root())
            .map(|(netuid, _)| netuid)
            .collect()
    }

    fn subnet_limit() -> u32 {
        u32::from(SubnetLimit::<T>::get())
    }

    fn spot_price(netuid: NetUid) -> Price {
        T::SwapInterface::current_alpha_price(netuid)
    }

    fn pool_tao(netuid: NetUid) -> u64 {
        SubnetTAO::<T>::get(netuid).to_u64()
    }

    fn moving_alpha_price(netuid: NetUid) -> Price {
        Self::get_moving_alpha_price(netuid)
    }

    fn subnet_alpha_in(netuid: NetUid) -> u64 {
        SubnetAlphaIn::<T>::get(netuid).to_u64()
    }

    fn quote_weight(netuid: NetUid) -> Perquintill {
        pallet_subtensor_swap::SwapBalancer::<T>::get(netuid).get_quote_weight()
    }

    fn swap_fee(netuid: NetUid) -> FeeRate {
        pallet_subtensor_swap::FeeRate::<T>::get(netuid)
    }

    /// `get_alpha_issuance`: the reserve, the outstanding alpha and the swap pallet's
    /// reservoir. A short's minted alpha lands in the first two (see `mint_and_sell`), so it
    /// is counted once (SPEC I7, P23). A default burns none of it: the alpha it moves from
    /// `SyntheticOutstanding` to `ResidualAlpha` stays where the sale put it and stays counted
    /// here, so in the halving schedule, `root_proportion` and the 21M cap as well (SPEC I2).
    fn alpha_issuance(netuid: NetUid) -> u64 {
        Self::get_alpha_issuance(netuid).to_u64()
    }

    /// The coinbase's own answer (`Pallet::is_in_emission_set`), read-only.
    fn is_in_emission_set(netuid: NetUid) -> bool {
        Self::is_in_emission_set(netuid)
    }

    fn tao_balance(who: &T::AccountId) -> u64 {
        Self::get_coldkey_balance(who).to_u64()
    }

    /// The stake of `who` on the pallet hotkey; zero in a runtime without lending.
    fn held_alpha(who: &T::AccountId, netuid: NetUid) -> u64 {
        T::Lending::pallet_hotkey().map_or(0, |hotkey| {
            Self::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, who, netuid).to_u64()
        })
    }

    /// Writes the ownership records of `create_account_if_non_existent` directly: that
    /// function refuses a lending account as a hotkey for every caller. Idempotent; fails if
    /// the hotkey is already owned by another coldkey.
    fn register_pallet_hotkey(coldkey: &T::AccountId, hotkey: &T::AccountId) -> DispatchResult {
        if let Ok(owner) = Owner::<T>::try_get(hotkey) {
            ensure!(owner == *coldkey, Error::<T>::NonAssociatedColdKey);
        } else {
            Owner::<T>::insert(hotkey, coldkey);
        }
        OwnedHotkeys::<T>::mutate(coldkey, |hotkeys| {
            if !hotkeys.contains(hotkey) {
                hotkeys.push(hotkey.clone());
            }
        });
        StakingHotkeys::<T>::mutate(coldkey, |hotkeys| {
            if !hotkeys.contains(hotkey) {
                hotkeys.push(hotkey.clone());
            }
        });
        Ok(())
    }

    fn transfer_tao(
        from: &T::AccountId,
        to: &T::AccountId,
        amount: u64,
    ) -> Result<u64, DispatchError> {
        Self::transfer_tao(from, to, TaoBalance::from(amount))?;
        Ok(amount)
    }

    /// The TAO joins the reserve as `swap_tao_for_alpha` adds it: `SubnetTAO` and `TotalStake`
    /// rise together, backed by the subnet account. No alpha is created, and nothing is
    /// recorded as flow or volume.
    fn credit_pool_tao(
        netuid: NetUid,
        from: &T::AccountId,
        amount: u64,
    ) -> Result<u64, DispatchError> {
        if amount == 0 {
            return Ok(0);
        }
        let subnet_account =
            Self::get_subnet_account_id(netuid).ok_or(Error::<T>::SubnetNotExists)?;
        let tao = TaoBalance::from(amount);
        Self::transfer_tao(from, &subnet_account, tao)?;
        Self::increase_provided_tao_reserve(netuid, tao);
        TotalStake::<T>::mutate(|total| *total = total.saturating_add(tao));
        Ok(amount)
    }

    /// `burn_tao`: the TAO moves to the burn account, as every other burn of this pallet.
    fn burn_tao(from: &T::AccountId, amount: u64) -> Result<u64, DispatchError> {
        Self::burn_tao(from, TaoBalance::from(amount))?;
        Ok(amount)
    }

    /// Mints `q` into the outstanding alpha and sells it with the swap an unstake runs, so
    /// the reserves move as `BalancerCurve::after_sell` quotes: the alpha reserve gains `q`
    /// less the fee and the TAO reserve loses the proceeds. The fee alpha is settled as for
    /// any sale (`settle_lending_alpha_fee`). Issuance rises by exactly `q` (SPEC I7).
    fn mint_and_sell(netuid: NetUid, q: u64, to: &T::AccountId) -> Result<u64, DispatchError> {
        let alpha = AlphaBalance::from(q);
        Self::resolve_to_alpha_out(Self::mint_alpha(netuid, alpha));
        let swap = Self::swap_alpha_for_tao_no_volume(
            netuid,
            alpha,
            T::SwapInterface::min_price::<TaoBalance>(),
            false,
        )?;
        ensure!(
            swap.amount_paid_in.saturating_add(swap.fee_paid) == alpha,
            Error::<T>::LendingLegMismatch
        );
        Self::transfer_tao_from_subnet(netuid, to, swap.amount_paid_out)?;
        Self::settle_lending_alpha_fee(netuid, swap.fee_to_block_author)?;
        Ok(swap.amount_paid_out.to_u64())
    }

    /// Buys with the smallest TAO input that returns `alpha` (`BalancerCurve::tao_in_for_alpha`)
    /// through the swap a stake runs, and recycles exactly `alpha`. Alpha the input buys beyond
    /// `alpha` (rounding) goes back to the reserve, so the reserves move as
    /// `BalancerCurve::after_buy_alpha` quotes and issuance falls by exactly `alpha` (SPEC I7).
    fn buy_and_burn(netuid: NetUid, from: &T::AccountId, alpha: u64) -> Result<u64, DispatchError> {
        if alpha == 0 {
            return Ok(0);
        }
        let pool = Self::lending_pool_state(netuid);
        let cost = BalancerCurve::tao_in_for_alpha(&pool, alpha)
            .and_then(|cost| u64::try_from(cost).ok())
            .ok_or(Error::<T>::InsufficientLiquidity)?;
        let subnet_account =
            Self::get_subnet_account_id(netuid).ok_or(Error::<T>::SubnetNotExists)?;
        let tao = TaoBalance::from(cost);
        Self::transfer_tao(from, &subnet_account, tao)?;
        let swap = Self::swap_tao_for_alpha_no_volume(
            netuid,
            tao,
            T::SwapInterface::max_price::<TaoBalance>(),
            false,
        )?;
        ensure!(
            swap.amount_paid_in.saturating_add(swap.fee_paid) == tao,
            Error::<T>::LendingLegMismatch
        );
        let wanted = AlphaBalance::from(alpha);
        let excess = swap
            .amount_paid_out
            .to_u64()
            .checked_sub(alpha)
            .map(AlphaBalance::from)
            .ok_or(Error::<T>::LendingLegMismatch)?;
        if !excess.is_zero() {
            SubnetAlphaOut::<T>::mutate(netuid, |total| *total = total.saturating_sub(excess));
            Self::increase_provided_alpha_reserve(netuid, excess);
        }
        Self::recycle_subnet_alpha(netuid, wanted);
        Self::settle_tao_fee_to_author(netuid, swap.fee_to_block_author)?;
        Ok(cost)
    }

    // HOLD(longs): spec under review
    fn lift_and_buy(_netuid: NetUid, _lift: u64, _to: &T::AccountId) -> Result<u64, DispatchError> {
        Err(Error::<T>::LongsPendingSpec.into())
    }

    // HOLD(longs): spec under review
    fn sell_alpha(
        _netuid: NetUid,
        _from: &T::AccountId,
        _alpha: u64,
    ) -> Result<u64, DispatchError> {
        Err(Error::<T>::LongsPendingSpec.into())
    }
}

impl<T: Config> Pallet<T> {
    /// Refuses a lending account (a position's custody, the pallet account, the pallet hotkey)
    /// as the coldkey of a stake, lock or key-swap call.
    pub fn ensure_not_lending_custody(who: &T::AccountId) -> Result<(), Error<T>> {
        ensure!(
            !T::Lending::is_lending_custody(who),
            Error::<T>::LendingCustodyAccount
        );
        Ok(())
    }

    /// Refuses a lending account as a hotkey: nobody stakes or locks to the pallet hotkey,
    /// registers it, names it as a parent or child or makes it an owner hotkey.
    pub fn ensure_not_lending_hotkey(hotkey: &T::AccountId) -> Result<(), Error<T>> {
        ensure!(
            !T::Lending::is_lending_custody(hotkey),
            Error::<T>::LendingPalletHotkey
        );
        Ok(())
    }

    /// Refuses to take `amount` alpha of `netuid` from `coldkey` if that reaches into the
    /// alpha lending keeps locked. Free for an account with no locked alpha.
    pub fn ensure_alpha_not_lending_locked(
        coldkey: &T::AccountId,
        netuid: NetUid,
        amount: AlphaBalance,
    ) -> Result<(), Error<T>> {
        let locked = AlphaBalance::from(T::Lending::locked_alpha(coldkey, netuid));
        if !locked.is_zero() {
            let free = Self::total_coldkey_alpha_on_subnet(coldkey, netuid).saturating_sub(locked);
            ensure!(amount <= free, Error::<T>::LendingLockedAlpha);
        }
        Ok(())
    }

    /// `settle_alpha_fee_to_author` without the `SubnetVolume` record: the fee alpha of a
    /// lending sale is swapped fee-free for TAO that goes to the block author, or burned when
    /// no author is known. Either way the alpha stays in issuance.
    fn settle_lending_alpha_fee(netuid: NetUid, fee: AlphaBalance) -> DispatchResult {
        if fee.is_zero() {
            return Ok(());
        }
        match <T as Config>::AuthorshipProvider::author() {
            Some(author) => {
                let swap = Self::swap_alpha_for_tao_no_volume(
                    netuid,
                    fee,
                    T::SwapInterface::min_price::<TaoBalance>(),
                    true,
                )?;
                Self::transfer_tao_from_subnet(netuid, &author, swap.amount_paid_out)
            }
            None => {
                Self::burn_subnet_alpha(netuid, fee);
                Ok(())
            }
        }
    }
}
