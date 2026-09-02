use super::*;
use frame_support::transactional;
#[cfg(feature = "runtime-benchmarks")]
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};
use subtensor_swap_interface::{DerivativesPoolInterface, Perquintill, SwapHandler};

/// Quoted swap passes before whatever budget is left is spent outright.
const QUOTED_PASSES: u8 = 2;

/// Spend from `budget` until `want` of the other token has been obtained or the budget is gone.
/// Returns `(spent, obtained)`.
///
/// The Balancer quote is exact up to fixed-point rounding, and it rounds down. Asking it for
/// one more unit of output and adding one unit of input makes a quoted pass land at or above
/// the gap (and never at zero output, which the swap rejects); a second quoted pass absorbs
/// any residual disagreement between quote and swap arithmetic. If a gap still remains, the
/// last pass spends everything left: a budget that cannot cover the gap at the current price
/// must end up fully spent, so the caller's shortfall is exact.
fn swap_until<In: Token, Out: Token>(
    want: Out,
    budget: In,
    quote: impl Fn(Out) -> In,
    mut swap: impl FnMut(In) -> Result<Out, DispatchError>,
) -> Result<(In, Out), DispatchError> {
    let mut spent = In::ZERO;
    let mut got = Out::ZERO;
    for pass in 0..=QUOTED_PASSES {
        let gap = want.saturating_sub(got);
        let left = budget.saturating_sub(spent);
        if gap.is_zero() || left.is_zero() {
            break;
        }
        let spend = if pass < QUOTED_PASSES {
            quote(gap.saturating_add(Out::one()))
                .saturating_add(In::one())
                .min(left)
        } else {
            left
        };
        got = got.saturating_add(swap(spend)?);
        spent = spent.saturating_add(spend);
    }
    Ok((spent, got))
}

impl<T: Config> Pallet<T> {
    /// A subnet whose pool still physically exists: live, or queued for dissolution cleanup
    /// (its sub-account and reserves survive until the `ProtocolLiquidity` phase).
    fn derivatives_pool_present(netuid: NetUid) -> bool {
        !netuid.is_root() && Self::get_subnet_account_id(netuid).is_some()
    }

    /// `TotalStake` mirrors `SubnetTAO` only for live subnets; `do_dissolve_network` already
    /// subtracted the whole reserve for a dissolving one.
    fn derivatives_adjust_total_stake(netuid: NetUid, delta: TaoBalance, add: bool) {
        if delta.is_zero() || !Self::if_subnet_exist(netuid) {
            return;
        }
        TotalStake::<T>::mutate(|total| {
            *total = if add {
                total.saturating_add(delta)
            } else {
                total.saturating_sub(delta)
            }
        });
    }
}

/// Pool access for `pallet-derivatives`.
///
/// A derivative position borrows a slice of the pool and hands it back later. None of these
/// operations are user stake or unstake events, so none of them touch `SubnetTaoFlow`, charge
/// swap fees, or pay the block author. They do keep `SubnetTAO`, `SubnetAlphaIn`,
/// `SubnetAlphaOut` and `TotalStake` consistent so the rest of the chain sees a normal pool.
impl<T: Config> DerivativesPoolInterface<T::AccountId> for Pallet<T> {
    fn is_dynamic(netuid: NetUid) -> bool {
        Self::if_subnet_exist(netuid)
            && !netuid.is_root()
            && SubnetMechanism::<T>::get(netuid) == 1
            && SubtokenEnabled::<T>::get(netuid)
    }

    fn reserves(netuid: NetUid) -> (TaoBalance, AlphaBalance) {
        (SubnetTAO::<T>::get(netuid), SubnetAlphaIn::<T>::get(netuid))
    }

    #[transactional]
    fn lift_liquidity(
        netuid: NetUid,
        phi: Perquintill,
        to_coldkey: &T::AccountId,
        to_hotkey: &T::AccountId,
    ) -> Result<(TaoBalance, AlphaBalance), DispatchError> {
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        let (tao_reserve, alpha_reserve) =
            <Self as DerivativesPoolInterface<T::AccountId>>::reserves(netuid);
        let tao = TaoBalance::from(phi.mul_floor(tao_reserve.to_u64()));
        let alpha = AlphaBalance::from(phi.mul_floor(alpha_reserve.to_u64()));
        ensure!(!tao.is_zero() && !alpha.is_zero(), Error::<T>::AmountTooLow);
        ensure!(
            tao < tao_reserve && alpha < alpha_reserve,
            Error::<T>::InsufficientLiquidity
        );

        // Both reserves shrink by the same fraction, so price and balancer weights are unchanged.
        Self::decrease_provided_tao_reserve(netuid, tao);
        Self::derivatives_adjust_total_stake(netuid, tao, false);
        Self::transfer_tao_from_subnet(netuid, to_coldkey, tao)?;

        Self::decrease_provided_alpha_reserve(netuid, alpha);
        SubnetAlphaOut::<T>::mutate(netuid, |total| *total = total.saturating_add(alpha));
        Self::increase_stake_for_hotkey_and_coldkey_on_subnet(to_hotkey, to_coldkey, netuid, alpha);

        Ok((tao, alpha))
    }

    #[transactional]
    fn return_liquidity(
        netuid: NetUid,
        tao: TaoBalance,
        alpha: AlphaBalance,
        from_coldkey: &T::AccountId,
        from_hotkey: &T::AccountId,
    ) -> DispatchResult {
        ensure!(
            Self::derivatives_pool_present(netuid),
            Error::<T>::SubnetNotExists
        );
        if tao.is_zero() && alpha.is_zero() {
            return Ok(());
        }

        if !tao.is_zero() {
            let subnet_account =
                Self::get_subnet_account_id(netuid).ok_or(Error::<T>::SubnetNotExists)?;
            Self::transfer_tao(from_coldkey, &subnet_account, tao)?;
        }

        if !alpha.is_zero() {
            let held =
                Self::get_stake_for_hotkey_and_coldkey_on_subnet(from_hotkey, from_coldkey, netuid);
            ensure!(held >= alpha, Error::<T>::NotEnoughStakeToWithdraw);
            Self::decrease_stake_for_hotkey_and_coldkey_on_subnet(
                from_hotkey,
                from_coldkey,
                netuid,
                alpha,
            );
            SubnetAlphaOut::<T>::mutate(netuid, |total| *total = total.saturating_sub(alpha));
        }

        // A dissolving subnet has already had its reservoirs purged into the reserves, and
        // nothing drains them again, so the pair goes straight to the reserves that the
        // later cleanup phases pay out from. `TotalStake` no longer counts this subnet.
        if !Self::if_subnet_exist(netuid) {
            Self::increase_provided_tao_reserve(netuid, tao);
            Self::increase_provided_alpha_reserve(netuid, alpha);
            return Ok(());
        }

        // Same bookkeeping as coinbase emission: the swap layer decides how much of the pair
        // can become price-active without pushing weights out of range; the rest waits in the
        // balancer reservoirs. TAO is already on the subnet account either way.
        let (tao_active, alpha_active) =
            T::SwapInterface::adjust_protocol_liquidity(netuid, tao, alpha);
        Self::increase_provided_alpha_reserve(netuid, alpha_active);
        Self::increase_provided_tao_reserve(netuid, tao_active);
        Self::derivatives_adjust_total_stake(netuid, tao_active, true);

        Ok(())
    }

    #[transactional]
    fn sell_alpha_internal(
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        netuid: NetUid,
        alpha: AlphaBalance,
    ) -> Result<TaoBalance, DispatchError> {
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        if alpha.is_zero() {
            return Ok(TaoBalance::ZERO);
        }
        let held = Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, netuid);
        ensure!(held >= alpha, Error::<T>::NotEnoughStakeToWithdraw);

        Self::decrease_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, netuid, alpha);
        let swap = Self::swap_alpha_for_tao(
            netuid,
            alpha,
            T::SwapInterface::min_price::<TaoBalance>(),
            true,
        )?;
        let unused = alpha.saturating_sub(swap.amount_paid_in.saturating_add(swap.fee_paid));
        if !unused.is_zero() {
            Self::increase_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, netuid, unused);
        }
        Self::transfer_tao_from_subnet(netuid, coldkey, swap.amount_paid_out)?;
        Ok(swap.amount_paid_out)
    }

    #[transactional]
    fn buy_alpha_internal(
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        netuid: NetUid,
        tao: TaoBalance,
    ) -> Result<AlphaBalance, DispatchError> {
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        if tao.is_zero() {
            return Ok(AlphaBalance::ZERO);
        }
        let subnet_account =
            Self::get_subnet_account_id(netuid).ok_or(Error::<T>::SubnetNotExists)?;
        // Plain transfer: the derivatives pallet account may legitimately be emptied here.
        Self::transfer_tao(coldkey, &subnet_account, tao)?;

        let swap = Self::swap_tao_for_alpha(
            netuid,
            tao,
            T::SwapInterface::max_price::<TaoBalance>(),
            true,
        )?;
        let bought = swap.amount_paid_out;
        ensure!(
            Self::try_increase_stake_for_hotkey_and_coldkey_on_subnet(hotkey, netuid, bought),
            Error::<T>::InsufficientLiquidity
        );
        Self::increase_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, netuid, bought);

        let unused = tao.saturating_sub(swap.amount_paid_in.saturating_add(swap.fee_paid));
        if !unused.is_zero() {
            Self::transfer_tao_from_subnet(netuid, coldkey, unused)?;
        }
        Ok(bought)
    }

    #[transactional]
    fn buy_alpha_for(
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        netuid: NetUid,
        want: AlphaBalance,
        budget: TaoBalance,
    ) -> Result<(TaoBalance, AlphaBalance), DispatchError> {
        swap_until(
            want,
            budget,
            |gap| T::SwapInterface::tao_needed_for_alpha(netuid, gap),
            |tao| {
                <Self as DerivativesPoolInterface<T::AccountId>>::buy_alpha_internal(
                    coldkey, hotkey, netuid, tao,
                )
            },
        )
    }

    #[transactional]
    fn sell_alpha_for(
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        netuid: NetUid,
        want: TaoBalance,
        budget: AlphaBalance,
    ) -> Result<(AlphaBalance, TaoBalance), DispatchError> {
        swap_until(
            want,
            budget,
            |gap| T::SwapInterface::alpha_needed_for_tao(netuid, gap),
            |alpha| {
                <Self as DerivativesPoolInterface<T::AccountId>>::sell_alpha_internal(
                    coldkey, hotkey, netuid, alpha,
                )
            },
        )
    }

    fn transfer_stake_internal(
        from_coldkey: &T::AccountId,
        from_hotkey: &T::AccountId,
        to_coldkey: &T::AccountId,
        to_hotkey: &T::AccountId,
        netuid: NetUid,
        amount: AlphaBalance,
    ) -> DispatchResult {
        ensure!(
            Self::derivatives_pool_present(netuid),
            Error::<T>::SubnetNotExists
        );
        // Stake on a hotkey with no owner cannot be moved out again by anyone.
        ensure!(
            Self::hotkey_account_exists(to_hotkey),
            Error::<T>::HotKeyAccountNotExists
        );
        if amount.is_zero() {
            return Ok(());
        }
        let held =
            Self::get_stake_for_hotkey_and_coldkey_on_subnet(from_hotkey, from_coldkey, netuid);
        ensure!(held >= amount, Error::<T>::NotEnoughStakeToWithdraw);
        Self::decrease_stake_for_hotkey_and_coldkey_on_subnet(
            from_hotkey,
            from_coldkey,
            netuid,
            amount,
        );
        Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
            to_hotkey, to_coldkey, netuid, amount,
        );
        Ok(())
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn set_up_pool_for_benchmark(netuid: NetUid) {
        let tao = TaoBalance::from(1_000_000_000_000_u64);
        let alpha = AlphaBalance::from(1_000_000_000_000_u64);
        if !Self::if_subnet_exist(netuid) {
            Self::init_new_network(netuid, 100);
        }
        SubtokenEnabled::<T>::insert(netuid, true);
        SubnetMechanism::<T>::insert(netuid, 1);
        FirstEmissionBlockNumber::<T>::insert(netuid, 1);
        SubnetTAO::<T>::insert(netuid, tao);
        SubnetAlphaIn::<T>::insert(netuid, alpha);
        SubnetAlphaOut::<T>::insert(netuid, alpha);
        TotalStake::<T>::mutate(|total| *total = total.saturating_add(tao));
        // The subnet account must physically hold the TAO reserve.
        if let Some(subnet_account) = Self::get_subnet_account_id(netuid) {
            let credit = Self::mint_tao(tao);
            let _ = Self::spend_tao(&subnet_account, credit, tao);
        }
        let price = U64F64::from_num(tao.to_u64())
            .checked_div(U64F64::from_num(alpha.to_u64()))
            .unwrap_or_default();
        T::SwapInterface::init_swap(netuid, Some(price));
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn forget_hotkey_for_benchmark(hotkey: &T::AccountId) {
        Owner::<T>::remove(hotkey);
    }
}
