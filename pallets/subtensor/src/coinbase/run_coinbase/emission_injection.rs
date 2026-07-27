//! Pool liquidity injection and per-block pending alpha accumulation.

use super::*;
use super::{as_u96f32, to_u64};
use crate::coinbase::tao::TaoCreditOf;
use alloc::collections::BTreeMap;
use frame_support::traits::Imbalance;
use safe_math::*;
use substrate_fixed::types::{U64F64, U96F32};
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};
use subtensor_swap_interface::SwapHandler;

impl<T: Config> Pallet<T> {
    /// Spend minted TAO credit into each subnet account: swap `excess_tao` for protocol
    /// alpha, then materialize `tao_in`/`alpha_in` into price-active pool reserves via the
    /// swap pallet's balancer reservoir.
    pub fn inject_pool_liquidity_and_swap_excess(
        subnets_to_emit_to: &[NetUid],
        tao_in: &BTreeMap<NetUid, U96F32>,
        alpha_in: &BTreeMap<NetUid, U96F32>,
        excess_tao: &BTreeMap<NetUid, U96F32>,
        credit: TaoCreditOf<T>,
    ) {
        let mut remaining_credit = credit;
        for netuid_i in subnets_to_emit_to.iter() {
            let maybe_subnet_account_id = Self::get_subnet_account_id(*netuid_i);
            if let Some(subnet_account_id) = maybe_subnet_account_id {
                let tao_in_i: TaoBalance =
                    to_u64!(*tao_in.get(netuid_i).unwrap_or(&as_u96f32!(0))).into();
                let alpha_in_i: AlphaBalance =
                    to_u64!(*alpha_in.get(netuid_i).unwrap_or(&as_u96f32!(0))).into();
                let tao_to_swap_with: TaoBalance =
                    to_u64!(excess_tao.get(netuid_i).unwrap_or(&as_u96f32!(0))).into();

                // Clear per-block pool-side emission counters up front so a subnet
                // disabled this block does not display stale values from an earlier block.
                SubnetExcessTao::<T>::insert(*netuid_i, TaoBalance::ZERO);
                SubnetTaoInEmission::<T>::insert(*netuid_i, TaoBalance::ZERO);

                if tao_to_swap_with > TaoBalance::ZERO {
                    // Turn excess_tao portion of credit into TaoBalance on subnet account
                    match Self::spend_tao(&subnet_account_id, remaining_credit, tao_to_swap_with) {
                        Ok(remainder) => {
                            remaining_credit = remainder;

                            let buy_swap_result = Self::swap_tao_for_alpha(
                                *netuid_i,
                                tao_to_swap_with,
                                T::SwapInterface::max_price(),
                                true,
                            );
                            match buy_swap_result {
                                Ok(buy_swap_result_ok) => {
                                    let bought_alpha: AlphaBalance =
                                        buy_swap_result_ok.amount_paid_out.into();
                                    SubnetProtocolAlpha::<T>::mutate(*netuid_i, |total| {
                                        *total = total.saturating_add(bought_alpha);
                                    });

                                    // Record actual excess TAO that entered pool.
                                    let actual_excess: TaoBalance =
                                        buy_swap_result_ok.amount_paid_in;
                                    SubnetExcessTao::<T>::insert(*netuid_i, actual_excess);
                                    Self::record_protocol_inflow(*netuid_i, actual_excess);
                                }
                                Err(error) => {
                                    match Self::withdraw_tao_as_credit(
                                        &subnet_account_id,
                                        tao_to_swap_with,
                                    ) {
                                        Ok(refund_credit) => {
                                            remaining_credit =
                                                remaining_credit.merge(refund_credit);
                                        }
                                        Err(withdraw_error) => {
                                            log::error!(
                                                "Failed to revert excess TAO deposit after swap failure: netuid_i = {netuid_i:?}, tao_to_swap_with = {tao_to_swap_with:?}, swap_error = {error:?}, withdraw_error = {withdraw_error:?}"
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        Err(remainder) => {
                            remaining_credit = remainder;
                            let remaining_balance = remaining_credit.peek();
                            log::error!(
                                "Failed to spend credit: tao_to_swap_with = {tao_to_swap_with:?}, netuid_i = {netuid_i:?}, remaining_balance = {remaining_balance:?}"
                            );
                        }
                    }
                }

                // Materialize this block's TAO before updating balancer reservoir
                // state. If spending fails, do not let the swap pallet consume
                // reservoir state as if this block's TAO arrived.
                let materialized_tao_delta = if tao_in_i.is_zero() {
                    TaoBalance::ZERO
                } else {
                    match Self::spend_tao(&subnet_account_id, remaining_credit, tao_in_i) {
                        Ok(remainder) => {
                            remaining_credit = remainder;
                            tao_in_i
                        }
                        Err(remainder) => {
                            remaining_credit = remainder;
                            let remaining_balance = remaining_credit.peek();
                            log::error!(
                                "Failed to spend credit: tao_delta = {tao_in_i:?}, netuid_i = {netuid_i:?}, remaining_balance = {remaining_balance:?}"
                            );
                            TaoBalance::ZERO
                        }
                    }
                };

                // Decide which current/reservoir liquidity can become price-active
                // without pushing balancer weights out of range. Only already
                // materialized current TAO is offered to the swap pallet.
                let (price_active_tao, price_active_alpha) =
                    T::SwapInterface::adjust_protocol_liquidity(
                        *netuid_i,
                        materialized_tao_delta,
                        alpha_in_i,
                    );

                // Materialize this block's alpha emission, then add only the
                // price-active portion to the pool reserve. The price-active
                // portion may include alpha that was materialized in an earlier
                // block and held in the reservoir.
                let _ = Self::mint_alpha(*netuid_i, alpha_in_i);
                SubnetAlphaInEmission::<T>::insert(*netuid_i, price_active_alpha);
                Self::increase_provided_alpha_reserve(*netuid_i, price_active_alpha);

                // Add only the price-active TAO to the pool reserve. This may
                // include TAO materialized in an earlier block and held in the
                // reservoir.
                if !price_active_tao.is_zero() {
                    SubnetTaoInEmission::<T>::insert(*netuid_i, price_active_tao);
                    Self::increase_provided_tao_reserve(*netuid_i, price_active_tao);
                    TotalStake::<T>::mutate(|total| {
                        *total = total.saturating_add(price_active_tao);
                    });
                    Self::record_protocol_inflow(*netuid_i, price_active_tao);
                }
            }
        }

        // Remaining imbalance should be zero at this point. If not, log error and burn.
        let remaining_balance = remaining_credit.peek();
        if !remaining_balance.is_zero() {
            // log::error!("Unspent imbalance remains: remaining_balance = {remaining_balance:?}");
            Self::recycle_credit(remaining_credit);
        }
    }

    /// Split each subnet's TAO emission into `(tao_in, alpha_in, alpha_out, excess_tao)`
    /// using spot price and the root-proportion injection cap (dTAO whitepaper).
    pub fn compute_subnet_emission_terms(
        subnet_emissions: &BTreeMap<NetUid, U96F32>,
    ) -> (
        BTreeMap<NetUid, U96F32>,
        BTreeMap<NetUid, U96F32>,
        BTreeMap<NetUid, U96F32>,
        BTreeMap<NetUid, U96F32>,
    ) {
        // Computation is described in detail in the dtao whitepaper.
        let mut tao_in: BTreeMap<NetUid, U96F32> = BTreeMap::new();
        let mut alpha_in: BTreeMap<NetUid, U96F32> = BTreeMap::new();
        let mut alpha_out: BTreeMap<NetUid, U96F32> = BTreeMap::new();
        let mut excess_tao: BTreeMap<NetUid, U96F32> = BTreeMap::new();

        // Only calculate for subnets that we are emitting to.
        for (&netuid_i, &tao_emission_i) in subnet_emissions.iter() {
            // Get alpha_emission this block.
            let alpha_emission_i: U96F32 = as_u96f32!(
                Self::get_block_emission_for_issuance(Self::get_alpha_issuance(netuid_i).into())
                    .unwrap_or(0)
            );
            log::debug!("alpha_emission_i: {alpha_emission_i:?}");

            // Get subnet price.
            let price_i: U96F32 =
                U96F32::saturating_from_num(T::SwapInterface::current_alpha_price(netuid_i.into()));
            log::debug!("price_i: {price_i:?}");

            let mut tao_in_i: U96F32 = tao_emission_i;
            let alpha_out_i: U96F32 = alpha_emission_i;
            let mut alpha_in_i: U96F32 = tao_emission_i.safe_div_or(price_i, U96F32::from_num(0.0));

            // Cap alpha injection by the subnet's root proportion of its alpha emission.
            // root_proportion = tao_weight / (tao_weight + alpha_issuance), so as a subnet
            // ages its alpha issuance grows, root_proportion shrinks, and the injection cap
            // falls. The TAO emission that can no longer be injected as liquidity becomes
            // excess TAO and is routed into chain buys instead. This is what transitions
            // older subnets from liquidity injection to chain buys over time.
            let root_proportion_i: U96F32 = Self::root_proportion(netuid_i);
            let alpha_injection_cap: U96F32 = root_proportion_i.saturating_mul(alpha_emission_i);
            if alpha_in_i > alpha_injection_cap {
                alpha_in_i = alpha_injection_cap;
                tao_in_i = alpha_in_i.saturating_mul(price_i);
            }

            let excess_amount: U96F32 = tao_emission_i.saturating_sub(tao_in_i);
            excess_tao.insert(netuid_i, excess_amount);

            // Insert values into maps
            tao_in.insert(netuid_i, tao_in_i);
            alpha_in.insert(netuid_i, alpha_in_i);
            alpha_out.insert(netuid_i, alpha_out_i);
        }
        (tao_in, alpha_in, alpha_out, excess_tao)
    }

    /// Inject pool liquidity for this block, mint outstanding `alpha_out`, take the owner
    /// cut, and accumulate pending server/validator/root alpha for later epoch drain.
    pub fn emit_to_subnets(
        subnets_to_emit_to: &[NetUid],
        subnet_emissions: &BTreeMap<NetUid, U96F32>,
        credit: TaoCreditOf<T>,
        root_sell_flag: bool,
    ) {
        // --- 1. Get subnet terms (tao_in, alpha_in, and alpha_out)
        // and excess_tao amounts.
        let (tao_in, alpha_in, alpha_out, excess_amount) = Self::compute_subnet_emission_terms(subnet_emissions);

        log::debug!("tao_in: {tao_in:?}");
        log::debug!("alpha_in: {alpha_in:?}");
        log::debug!("alpha_out: {alpha_out:?}");
        log::debug!("excess_amount: {excess_amount:?}");

        // --- 2. Inject TAO and ALPHA to pool and swap with excess TAO.
        Self::inject_pool_liquidity_and_swap_excess(
            subnets_to_emit_to,
            &tao_in,
            &alpha_in,
            &excess_amount,
            credit,
        );

        // --- 3. Inject ALPHA for participants.
        let cut_percent: U96F32 = Self::get_float_subnet_owner_cut();

        for netuid_i in subnets_to_emit_to.iter() {
            // Get alpha_out for this block.
            let mut alpha_out_i: U96F32 = *alpha_out.get(netuid_i).unwrap_or(&as_u96f32!(0));

            let alpha_created: AlphaBalance = AlphaBalance::from(to_u64!(alpha_out_i));
            SubnetAlphaOutEmission::<T>::insert(*netuid_i, alpha_created);

            // Mint and resolve outstanding alpha
            Self::resolve_to_alpha_out(Self::mint_alpha(*netuid_i, alpha_created));

            // Calculate the owner cut.
            if Self::get_owner_cut_enabled(*netuid_i) {
                let owner_cut_i: U96F32 = alpha_out_i.saturating_mul(cut_percent);
                log::debug!("owner_cut_i: {owner_cut_i:?}");
                // Deduct owner cut from alpha_out.
                alpha_out_i = alpha_out_i.saturating_sub(owner_cut_i);
                // Accumulate the owner cut in pending.
                PendingOwnerCut::<T>::mutate(*netuid_i, |total| {
                    *total = total.saturating_add(to_u64!(owner_cut_i).into());
                });
            }

            // Get root proportional dividends.
            let root_proportion = Self::root_proportion(*netuid_i);
            log::debug!("root_proportion: {root_proportion:?}");

            // Get root alpha from root prop.
            let root_alpha: U96F32 = root_proportion
                .saturating_mul(alpha_out_i) // Total alpha emission per block remaining.
                .saturating_mul(as_u96f32!(0.5)); // 50% to validators.
            log::debug!("root_alpha: {root_alpha:?}");

            // Get pending server alpha, which is the miner cut of the alpha out.
            // Currently miner cut is 50% of the alpha out.
            let pending_server_alpha = alpha_out_i.saturating_mul(as_u96f32!(0.5));
            log::debug!("pending_server_alpha: {pending_server_alpha:?}");
            // The total validator alpha is the remaining alpha out minus the server alpha.
            let total_validator_alpha = alpha_out_i.saturating_sub(pending_server_alpha);
            log::debug!("total_validator_alpha: {total_validator_alpha:?}");
            // The alpha validators don't get the root alpha.
            let pending_validator_alpha = total_validator_alpha.saturating_sub(root_alpha);
            log::debug!("pending_validator_alpha: {pending_validator_alpha:?}");

            // Accumulate the server alpha emission.
            PendingServerEmission::<T>::mutate(*netuid_i, |total| {
                *total = total.saturating_add(to_u64!(pending_server_alpha).into());
            });
            // Accumulate the validator alpha emission.
            PendingValidatorEmission::<T>::mutate(*netuid_i, |total| {
                *total = total.saturating_add(to_u64!(pending_validator_alpha).into());
            });

            if root_sell_flag {
                // Only accumulate root alpha divs if root sell is allowed.
                PendingRootAlphaDivs::<T>::mutate(*netuid_i, |total| {
                    *total = total.saturating_add(to_u64!(root_alpha).into());
                });
            } else {
                // If we are not selling the root alpha, we should recycle it.
                Self::recycle_subnet_alpha(*netuid_i, AlphaBalance::from(to_u64!(root_alpha)));
            }
        }
    }

    /// `true` when the sum of emit-eligible subnet EMA alpha prices exceeds 1 — only then
    /// are root alpha dividends accumulated (otherwise recycled).
    pub fn get_network_root_sell_flag(subnets_to_emit_to: &[NetUid]) -> bool {
        let total_ema_price: U64F64 = subnets_to_emit_to
            .iter()
            .map(|netuid| Self::get_moving_alpha_price(*netuid))
            .sum();

        // If the total EMA price is less than or equal to 1
        // then we WILL NOT root sell.
        total_ema_price > U64F64::saturating_from_num(1)
    }
}
