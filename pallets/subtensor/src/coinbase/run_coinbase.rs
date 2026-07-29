use super::*;
use crate::coinbase::tao::CreditOf;
use alloc::collections::{BTreeMap, BTreeSet};
use frame_support::traits::Imbalance;
use safe_math::*;
use substrate_fixed::types::{U64F64, U96F32};
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};
use subtensor_swap_interface::SwapHandler;

// Distribute dividends to each hotkey
macro_rules! asfloat {
    ($val:expr) => {
        U96F32::saturating_from_num($val)
    };
}

macro_rules! tou64 {
    ($val:expr) => {
        $val.saturating_to_num::<u64>()
    };
}

impl<T: Config> Pallet<T> {
    pub fn run_coinbase(block_emission_credit: CreditOf<T>) {
        // --- 0. Get current block.
        let current_block: u64 = Self::get_current_block_as_u64();
        let block_emission = U96F32::saturating_from_num(block_emission_credit.peek());
        log::debug!(
            "Running coinbase for block {current_block:?} with block emission: {block_emission:?}"
        );

        // Reset per-block root sell counters from the previous block.
        // Root sells happen after coinbase, so their accumulated values
        // are consumed here at the start of the next block.
        let _ = SubnetRootSellTao::<T>::clear(u32::MAX, None);

        // --- 1. Get all subnets (excluding root).
        let subnets: Vec<NetUid> = Self::get_all_subnet_netuids()
            .into_iter()
            .filter(|netuid| *netuid != NetUid::ROOT)
            .collect();
        log::debug!("All subnets: {subnets:?}");

        // --- 2. Get subnets to emit to
        let subnets_to_emit_to: Vec<NetUid> = Self::get_subnets_to_emit_to(&subnets);
        log::debug!("Subnets to emit to: {subnets_to_emit_to:?}");

        // --- 3. Get emissions for subnets to emit to
        let subnet_emissions =
            Self::get_subnet_block_emissions(&subnets_to_emit_to, block_emission);
        log::debug!("Raw subnet emission allocations: {subnet_emissions:?}");
        let root_sell_flag = Self::get_network_root_sell_flag(&subnets_to_emit_to);
        log::debug!("Root sell flag: {root_sell_flag:?}");

        // --- 4. Emit to subnets for this block.
        Self::emit_to_subnets(
            &subnets_to_emit_to,
            &subnet_emissions,
            block_emission_credit,
            root_sell_flag,
        );

        // --- 5. Drain pending emissions.
        let emissions_to_distribute = Self::drain_pending(&subnets, current_block);

        // --- 6. Distribute the emissions to the subnets.
        Self::distribute_emissions_to_subnets(&emissions_to_distribute);
    }

    pub fn inject_and_maybe_swap(
        subnets_to_emit_to: &[NetUid],
        subnet_emissions: &BTreeMap<NetUid, U96F32>,
        tao_in: &BTreeMap<NetUid, U96F32>,
        alpha_in: &BTreeMap<NetUid, U96F32>,
        excess_tao: &BTreeMap<NetUid, U96F32>,
        credit: CreditOf<T>,
    ) {
        // This map reports only the current block's final materialized allocation.
        // Clearing the complete prefix also prevents an ineligible subnet from
        // retaining a stale value from the previous block.
        let _ = SubnetTaoEmission::<T>::clear(u32::MAX, None);

        let mut remaining_credit = credit;
        for netuid_i in subnets_to_emit_to.iter() {
            let maybe_subnet_account_id = Self::get_subnet_account_id(*netuid_i);
            if let Some(subnet_account_id) = maybe_subnet_account_id {
                let tao_in_i: TaoBalance =
                    tou64!(*tao_in.get(netuid_i).unwrap_or(&asfloat!(0))).into();
                let alpha_in_i: AlphaBalance =
                    tou64!(*alpha_in.get(netuid_i).unwrap_or(&asfloat!(0))).into();
                let tao_to_swap_with: TaoBalance =
                    tou64!(excess_tao.get(netuid_i).unwrap_or(&asfloat!(0))).into();

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
                                    Self::record_subnet_tao_emission(*netuid_i, tao_to_swap_with);
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
                                            // The credit was already materialized on the subnet
                                            // account and could not be recovered, so it remains
                                            // part of this subnet's final TAO allocation.
                                            Self::record_subnet_tao_emission(
                                                *netuid_i,
                                                tao_to_swap_with,
                                            );
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
                            Self::record_subnet_tao_emission(*netuid_i, tao_in_i);
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

        // After spillover has filled every available chain-buy cap, the block
        // emission can still contain unspent TAO. Allocate that terminal remainder
        // across all emitting subnets in proportion to their emission weights. It
        // is materialized into each subnet's protocol TAO reservoir rather than
        // sent through another chain buy, which preserves the caps and avoids an
        // unpaired liquidity injection changing the spot price.
        Self::allocate_remaining_tao(subnets_to_emit_to, subnet_emissions, remaining_credit);
    }

    fn record_subnet_tao_emission(netuid: NetUid, amount: TaoBalance) {
        if amount.is_zero() {
            return;
        }

        SubnetTaoEmission::<T>::mutate(netuid, |total| {
            *total = total.saturating_add(amount);
        });
    }

    fn allocate_remaining_tao(
        subnets_to_emit_to: &[NetUid],
        subnet_emissions: &BTreeMap<NetUid, U96F32>,
        credit: CreditOf<T>,
    ) {
        let mut remaining_credit = credit;
        if remaining_credit.peek().is_zero() {
            return;
        }

        let zero = asfloat!(0);
        let recipients: Vec<(NetUid, T::AccountId, U96F32)> = subnets_to_emit_to
            .iter()
            .filter_map(|netuid| {
                let weight = *subnet_emissions.get(netuid).unwrap_or(&zero);
                if weight == zero {
                    return None;
                }
                Self::get_subnet_account_id(*netuid).map(|account| (*netuid, account, weight))
            })
            .collect();
        let total_weight = recipients
            .iter()
            .fold(zero, |total, (_, _, weight)| total.saturating_add(*weight));

        if total_weight == zero {
            log::debug!("No weighted emitting subnet is available for remaining TAO");
            Self::recycle_credit(remaining_credit);
            return;
        }

        let distributable = asfloat!(remaining_credit.peek());
        let last_index = recipients.len().saturating_sub(1);
        for (index, (netuid, account, weight)) in recipients.into_iter().enumerate() {
            let amount: TaoBalance = if index == last_index {
                remaining_credit.peek()
            } else {
                tou64!(distributable.saturating_mul(weight.safe_div(total_weight))).into()
            };
            if amount.is_zero() {
                continue;
            }

            match Self::spend_tao(&account, remaining_credit, amount) {
                Ok(remainder) => {
                    remaining_credit = remainder;
                    T::SwapInterface::reserve_protocol_tao(netuid, amount);
                    Self::record_subnet_tao_emission(netuid, amount);
                }
                Err(remainder) => {
                    remaining_credit = remainder;
                    log::error!(
                        "Failed to allocate remaining TAO: netuid = {netuid:?}, amount = {amount:?}"
                    );
                }
            }
        }

        if !remaining_credit.peek().is_zero() {
            log::error!(
                "Unable to allocate all remaining TAO: remaining = {:?}",
                remaining_credit.peek()
            );
            Self::recycle_credit(remaining_credit);
        }
    }

    pub fn get_subnet_terms(
        subnet_emissions: &BTreeMap<NetUid, U96F32>,
    ) -> (
        BTreeMap<NetUid, U96F32>,
        BTreeMap<NetUid, U96F32>,
        BTreeMap<NetUid, U96F32>,
        BTreeMap<NetUid, U96F32>,
    ) {
        // Build the complete pre-execution emission plan. `subnet_emissions`
        // contains raw allocations and remains the authoritative priority signal
        // for spillover; the returned TAO maps contain the post-cap plan.
        let mut tao_in: BTreeMap<NetUid, U96F32> = BTreeMap::new();
        let mut alpha_in: BTreeMap<NetUid, U96F32> = BTreeMap::new();
        let mut alpha_out: BTreeMap<NetUid, U96F32> = BTreeMap::new();
        let mut excess_tao: BTreeMap<NetUid, U96F32> = BTreeMap::new();
        let mut chain_buy_caps: BTreeMap<NetUid, U96F32> = BTreeMap::new();
        let mut alpha_injection_caps: BTreeMap<NetUid, U96F32> = BTreeMap::new();
        let mut subnet_prices: BTreeMap<NetUid, U96F32> = BTreeMap::new();
        let mut spillover_tao = asfloat!(0);
        let owner_cut_percent = Self::get_float_subnet_owner_cut();
        let one = asfloat!(1);
        let miner_share = asfloat!(0.5);

        // Only calculate for subnets that we are emitting to.
        for (&netuid_i, &tao_emission_i) in subnet_emissions.iter() {
            // Get alpha_emission this block.
            let alpha_emission_i: U96F32 = asfloat!(
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

            // Chain buys may consume at most the TAO value of unburned miner alpha.
            // Start from the miner half of alpha_out after owner cut, then exclude
            // the proportion withheld from miners by burn/recycle UIDs in the last
            // settled tempo. If owner cut is disabled, miners receive half of the
            // full alpha_out before the burn adjustment.
            let participant_alpha_i = if Self::get_owner_cut_enabled(netuid_i) {
                alpha_emission_i.saturating_sub(alpha_emission_i.saturating_mul(owner_cut_percent))
            } else {
                alpha_emission_i
            };
            let miner_burned_i = MinerBurned::<T>::get(netuid_i).min(one);
            let unburned_miner_alpha_i = participant_alpha_i
                .saturating_mul(miner_share)
                .saturating_mul(one.saturating_sub(miner_burned_i));
            let chain_buy_cap_i = unburned_miner_alpha_i.saturating_mul(price_i);
            let available_chain_buy_i = tao_emission_i.saturating_sub(tao_in_i);
            let chain_buy_i = available_chain_buy_i.min(chain_buy_cap_i);
            spillover_tao =
                spillover_tao.saturating_add(available_chain_buy_i.saturating_sub(chain_buy_i));
            excess_tao.insert(netuid_i, chain_buy_i);
            chain_buy_caps.insert(netuid_i, chain_buy_cap_i);
            alpha_injection_caps.insert(netuid_i, alpha_injection_cap);
            subnet_prices.insert(netuid_i, price_i);

            // Insert values into maps
            tao_in.insert(netuid_i, tao_in_i);
            alpha_in.insert(netuid_i, alpha_in_i);
            alpha_out.insert(netuid_i, alpha_out_i);
        }

        // Reassign TAO rejected by a subnet's local cap to the highest-emission
        // subnet that still has uncovered unburned-miner-emission capacity.
        // Preserve the normal emission ordering for each recipient: first fill
        // its remaining liquidity-injection capacity, then fill its chain buy.
        // Move to the next subnet only after both are full or spillover is
        // exhausted. Netuid ascending is the deterministic tie-breaker for equal
        // emission weights.
        let mut spillover_priority: Vec<(NetUid, U96F32)> = subnet_emissions
            .iter()
            .filter(|(_, emission)| **emission > asfloat!(0))
            .map(|(netuid, emission)| (*netuid, *emission))
            .collect();
        spillover_priority.sort_by(|(netuid_a, emission_a), (netuid_b, emission_b)| {
            emission_b
                .cmp(emission_a)
                .then_with(|| netuid_a.cmp(netuid_b))
        });

        for (netuid, _) in spillover_priority.iter().copied() {
            if spillover_tao == asfloat!(0) {
                break;
            }

            let current_chain_buy = *excess_tao.get(&netuid).unwrap_or(&asfloat!(0));
            let chain_buy_cap = *chain_buy_caps.get(&netuid).unwrap_or(&asfloat!(0));
            if current_chain_buy >= chain_buy_cap {
                continue;
            }

            let current_tao_in = *tao_in.get(&netuid).unwrap_or(&asfloat!(0));
            let alpha_injection_cap = *alpha_injection_caps.get(&netuid).unwrap_or(&asfloat!(0));
            let price = *subnet_prices.get(&netuid).unwrap_or(&asfloat!(0));
            let tao_injection_cap = alpha_injection_cap.saturating_mul(price);
            let uncovered_liquidity = tao_injection_cap.saturating_sub(current_tao_in);
            let spillover_liquidity = spillover_tao.min(uncovered_liquidity);
            if spillover_liquidity > asfloat!(0) {
                let updated_tao_in = current_tao_in.saturating_add(spillover_liquidity);
                let updated_alpha_in = if spillover_liquidity == uncovered_liquidity {
                    alpha_injection_cap
                } else {
                    updated_tao_in.safe_div_or(price, asfloat!(0))
                };
                tao_in.insert(netuid, updated_tao_in);
                alpha_in.insert(netuid, updated_alpha_in);
                spillover_tao = spillover_tao.saturating_sub(spillover_liquidity);
            }

            if spillover_tao == asfloat!(0) {
                break;
            }

            let uncovered_chain_buy = chain_buy_cap.saturating_sub(current_chain_buy);
            let spillover_chain_buy = spillover_tao.min(uncovered_chain_buy);
            if spillover_chain_buy == asfloat!(0) {
                continue;
            }

            excess_tao.insert(
                netuid,
                current_chain_buy.saturating_add(spillover_chain_buy),
            );
            spillover_tao = spillover_tao.saturating_sub(spillover_chain_buy);
        }

        // Converting each fixed-point plan entry to an integer balance can leave
        // a small number of rao unassigned even when the fixed-point spillover
        // was exhausted. Reconcile that integer remainder through the same
        // liquidity-first priority before allowing it to reach the terminal
        // reservoir.
        let total_emission_rao = subnet_emissions
            .values()
            .fold(asfloat!(0), |total, emission| {
                total.saturating_add(*emission)
            })
            .saturating_to_num::<u64>();
        let planned_tao_rao = tao_in
            .values()
            .chain(excess_tao.values())
            .fold(0_u64, |total, amount| {
                total.saturating_add(amount.saturating_to_num::<u64>())
            });
        let mut integer_spillover_rao = total_emission_rao.saturating_sub(planned_tao_rao);

        for (netuid, _) in spillover_priority {
            if integer_spillover_rao == 0 {
                break;
            }

            let current_chain_buy = *excess_tao.get(&netuid).unwrap_or(&asfloat!(0));
            let chain_buy_cap = *chain_buy_caps.get(&netuid).unwrap_or(&asfloat!(0));
            let current_chain_buy_rao = current_chain_buy.saturating_to_num::<u64>();
            let chain_buy_cap_rao = chain_buy_cap.saturating_to_num::<u64>();
            if current_chain_buy_rao >= chain_buy_cap_rao {
                continue;
            }

            let current_tao_in = *tao_in.get(&netuid).unwrap_or(&asfloat!(0));
            let alpha_injection_cap = *alpha_injection_caps.get(&netuid).unwrap_or(&asfloat!(0));
            let price = *subnet_prices.get(&netuid).unwrap_or(&asfloat!(0));
            let tao_injection_cap = alpha_injection_cap.saturating_mul(price);
            let current_tao_in_rao = current_tao_in.saturating_to_num::<u64>();
            let tao_injection_cap_rao = tao_injection_cap.saturating_to_num::<u64>();
            let uncovered_liquidity_rao = tao_injection_cap_rao.saturating_sub(current_tao_in_rao);
            let spillover_liquidity_rao = integer_spillover_rao.min(uncovered_liquidity_rao);
            if spillover_liquidity_rao > 0 {
                let updated_tao_in_rao = current_tao_in_rao.saturating_add(spillover_liquidity_rao);
                let updated_tao_in = asfloat!(updated_tao_in_rao);
                tao_in.insert(netuid, updated_tao_in);
                alpha_in.insert(
                    netuid,
                    updated_tao_in
                        .safe_div_or(price, asfloat!(0))
                        .min(alpha_injection_cap),
                );
                integer_spillover_rao =
                    integer_spillover_rao.saturating_sub(spillover_liquidity_rao);
            }

            if integer_spillover_rao == 0 {
                break;
            }

            let uncovered_chain_buy_rao = chain_buy_cap_rao.saturating_sub(current_chain_buy_rao);
            let spillover_chain_buy_rao = integer_spillover_rao.min(uncovered_chain_buy_rao);
            if spillover_chain_buy_rao == 0 {
                continue;
            }

            excess_tao.insert(
                netuid,
                asfloat!(current_chain_buy_rao.saturating_add(spillover_chain_buy_rao)),
            );
            integer_spillover_rao = integer_spillover_rao.saturating_sub(spillover_chain_buy_rao);
        }

        (tao_in, alpha_in, alpha_out, excess_tao)
    }

    pub fn emit_to_subnets(
        subnets_to_emit_to: &[NetUid],
        subnet_emissions: &BTreeMap<NetUid, U96F32>,
        credit: CreditOf<T>,
        root_sell_flag: bool,
    ) {
        // --- 1. Get subnet terms (tao_in, alpha_in, and alpha_out)
        // and excess_tao amounts.
        let (tao_in, alpha_in, alpha_out, excess_amount) = Self::get_subnet_terms(subnet_emissions);

        log::debug!("tao_in: {tao_in:?}");
        log::debug!("alpha_in: {alpha_in:?}");
        log::debug!("alpha_out: {alpha_out:?}");
        log::debug!("excess_amount: {excess_amount:?}");

        // --- 2. Inject TAO and ALPHA to pool and swap with excess TAO.
        Self::inject_and_maybe_swap(
            subnets_to_emit_to,
            subnet_emissions,
            &tao_in,
            &alpha_in,
            &excess_amount,
            credit,
        );

        // --- 3. Inject ALPHA for participants.
        let cut_percent: U96F32 = Self::get_float_subnet_owner_cut();

        for netuid_i in subnets_to_emit_to.iter() {
            // Get alpha_out for this block.
            let mut alpha_out_i: U96F32 = *alpha_out.get(netuid_i).unwrap_or(&asfloat!(0));

            let alpha_created: AlphaBalance = AlphaBalance::from(tou64!(alpha_out_i));
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
                    *total = total.saturating_add(tou64!(owner_cut_i).into());
                });
            }

            // Get root proportional dividends.
            let root_proportion = Self::root_proportion(*netuid_i);
            log::debug!("root_proportion: {root_proportion:?}");

            // Get root alpha from root prop.
            let root_alpha: U96F32 = root_proportion
                .saturating_mul(alpha_out_i) // Total alpha emission per block remaining.
                .saturating_mul(asfloat!(0.5)); // 50% to validators.
            log::debug!("root_alpha: {root_alpha:?}");

            // Get pending server alpha, which is the miner cut of the alpha out.
            // Currently miner cut is 50% of the alpha out.
            let pending_server_alpha = alpha_out_i.saturating_mul(asfloat!(0.5));
            log::debug!("pending_server_alpha: {pending_server_alpha:?}");
            // The total validator alpha is the remaining alpha out minus the server alpha.
            let total_validator_alpha = alpha_out_i.saturating_sub(pending_server_alpha);
            log::debug!("total_validator_alpha: {total_validator_alpha:?}");
            // The alpha validators don't get the root alpha.
            let pending_validator_alpha = total_validator_alpha.saturating_sub(root_alpha);
            log::debug!("pending_validator_alpha: {pending_validator_alpha:?}");

            // Accumulate the server alpha emission.
            PendingServerEmission::<T>::mutate(*netuid_i, |total| {
                *total = total.saturating_add(tou64!(pending_server_alpha).into());
            });
            // Accumulate the validator alpha emission.
            PendingValidatorEmission::<T>::mutate(*netuid_i, |total| {
                *total = total.saturating_add(tou64!(pending_validator_alpha).into());
            });

            if root_sell_flag {
                // Only accumulate root alpha divs if root sell is allowed.
                PendingRootAlphaDivs::<T>::mutate(*netuid_i, |total| {
                    *total = total.saturating_add(tou64!(root_alpha).into());
                });
            } else {
                // If we are not selling the root alpha, we should recycle it.
                Self::recycle_subnet_alpha(*netuid_i, AlphaBalance::from(tou64!(root_alpha)));
            }
        }
    }

    /// Subnets whose epoch slot is due *this* block but is deferred by the per-block
    /// cap (`MaxEpochsPerBlock`).
    pub fn epochs_deferred_this_block(subnets: &[NetUid], current_block: u64) -> BTreeSet<NetUid> {
        let cap = Self::get_max_epochs_per_block() as u32;
        let mut deferred: BTreeSet<NetUid> = BTreeSet::new();
        let mut epochs_run_this_block: u32 = 0;

        for &netuid in subnets.iter() {
            if !Self::should_run_epoch(netuid, current_block) {
                continue;
            }
            // Per-block cap — due subnets beyond the limit are deferred.
            if epochs_run_this_block >= cap {
                deferred.insert(netuid);
                continue;
            }
            if Self::is_epoch_input_state_consistent(netuid) {
                epochs_run_this_block = epochs_run_this_block.saturating_add(1);
            }
        }
        deferred
    }

    pub fn drain_pending(
        subnets: &[NetUid],
        current_block: u64,
    ) -> BTreeMap<NetUid, (AlphaBalance, AlphaBalance, AlphaBalance, AlphaBalance)> {
        // Map of netuid to (pending_server_alpha, pending_validator_alpha, pending_root_alpha, pending_owner_cut).
        let mut emissions_to_distribute: BTreeMap<
            NetUid,
            (AlphaBalance, AlphaBalance, AlphaBalance, AlphaBalance),
        > = BTreeMap::new();
        // Per-block cap on number of epochs that may run; the rest are deferred 1 block forward
        // by setting `PendingEpochAt`.
        let max_epochs_per_block = Self::get_max_epochs_per_block() as u32;
        let mut epochs_run_this_block: u32 = 0;

        for &netuid in subnets.iter() {
            // Keep the scheduler age bounded per subnet. `tempo + 1` is enough to
            // record that a due epoch missed its slot while avoiding an unbounded
            // public counter when the epoch is repeatedly deferred or its input
            // state remains inconsistent.
            let tempo = Self::get_tempo(netuid);
            let max_blocks_since_last_step = u64::from(tempo).saturating_add(1);
            BlocksSinceLastStep::<T>::mutate(netuid, |total| {
                *total = total.saturating_add(1).min(max_blocks_since_last_step)
            });

            if !Self::should_run_epoch_with_tempo(netuid, current_block, tempo) {
                continue;
            }

            // Per-block cap — defer if already at limit.
            if epochs_run_this_block >= max_epochs_per_block {
                let next_block = current_block.saturating_add(1);
                PendingEpochAt::<T>::insert(netuid, next_block);
                Self::deposit_event(Event::EpochDeferred {
                    netuid,
                    from_block: current_block,
                    to_block: next_block,
                });
                continue;
            }

            if Self::is_epoch_input_state_consistent(netuid) {
                // Reset blocks-since counter; LastMechansimStepBlock is written
                // post-distribute (see the caller), so bonds masking can read the
                // previous successful run.
                BlocksSinceLastStep::<T>::insert(netuid, 0);

                // Get and drain the subnet pending emission.
                let pending_server_alpha = PendingServerEmission::<T>::get(netuid);
                PendingServerEmission::<T>::insert(netuid, AlphaBalance::ZERO);

                let pending_validator_alpha = PendingValidatorEmission::<T>::get(netuid);
                PendingValidatorEmission::<T>::insert(netuid, AlphaBalance::ZERO);

                // Get and drain the pending Alpha for root divs.
                let pending_root_alpha = PendingRootAlphaDivs::<T>::get(netuid);
                PendingRootAlphaDivs::<T>::insert(netuid, AlphaBalance::ZERO);

                // Get and drain the pending owner cut.
                let owner_cut = PendingOwnerCut::<T>::get(netuid);
                PendingOwnerCut::<T>::insert(netuid, AlphaBalance::ZERO);

                // Save the emissions to distribute.
                emissions_to_distribute.insert(
                    netuid,
                    (
                        pending_server_alpha,
                        pending_validator_alpha,
                        pending_root_alpha,
                        owner_cut,
                    ),
                );
                epochs_run_this_block = epochs_run_this_block.saturating_add(1);

                // Change subnet owner based on conviction.
                Self::change_subnet_owner_if_needed(netuid);
            } else {
                // Schedule advances below; execution skipped. Pending emissions accumulate
                // and will be drained by the next successful epoch.
                Self::deposit_event(Event::EpochSkipped {
                    netuid,
                    block: current_block,
                });
            }

            // Advance the schedule unconditionally — the slot is consumed.
            LastEpochBlock::<T>::insert(netuid, current_block);
            PendingEpochAt::<T>::insert(netuid, 0);
            SubnetEpochIndex::<T>::mutate(netuid, |idx| *idx = idx.saturating_add(1));
        }
        emissions_to_distribute
    }

    pub fn distribute_emissions_to_subnets(
        emissions_to_distribute: &BTreeMap<
            NetUid,
            (AlphaBalance, AlphaBalance, AlphaBalance, AlphaBalance),
        >,
    ) {
        let current_block = Self::get_current_block_as_u64();
        for (
            &netuid,
            &(pending_server_alpha, pending_validator_alpha, pending_root_alpha, pending_owner_cut),
        ) in emissions_to_distribute.iter()
        {
            // Distribute the emission to the subnet.
            Self::distribute_emission(
                netuid,
                pending_server_alpha,
                pending_validator_alpha,
                pending_root_alpha,
                pending_owner_cut,
            );
            LastMechansimStepBlock::<T>::insert(netuid, current_block);
        }
    }

    pub fn get_network_root_sell_flag(subnets_to_emit_to: &[NetUid]) -> bool {
        let total_ema_price: U64F64 = subnets_to_emit_to
            .iter()
            .map(|netuid| Self::get_moving_alpha_price(*netuid))
            .sum();

        // If the total EMA price is less than or equal to 1
        // then we WILL NOT root sell.
        total_ema_price > U64F64::saturating_from_num(1)
    }

    pub fn calculate_dividends_and_incentives(
        netuid: NetUid,
        hotkey_emission: Vec<(T::AccountId, AlphaBalance, AlphaBalance)>,
    ) -> (
        BTreeMap<T::AccountId, AlphaBalance>,
        BTreeMap<T::AccountId, U96F32>,
    ) {
        // Accumulate emission of dividends and incentive per hotkey.
        let mut incentives: BTreeMap<T::AccountId, AlphaBalance> = BTreeMap::new();
        let mut dividends: BTreeMap<T::AccountId, U96F32> = BTreeMap::new();
        for (hotkey, incentive, dividend) in hotkey_emission {
            // Accumulate incentives to miners.
            incentives
                .entry(hotkey.clone())
                .and_modify(|e| *e = e.saturating_add(incentive))
                .or_insert(incentive);
            // Accumulate dividends to parents.
            let div_tuples: Vec<(T::AccountId, AlphaBalance)> =
                Self::get_parent_child_dividends_distribution(&hotkey, netuid, dividend);
            // Accumulate dividends per hotkey.
            for (parent, parent_div) in div_tuples {
                dividends
                    .entry(parent)
                    .and_modify(|e| *e = e.saturating_add(asfloat!(parent_div)))
                    .or_insert(asfloat!(parent_div));
            }
        }
        log::debug!("incentives: {incentives:?}");
        log::debug!("dividends: {dividends:?}");

        (incentives, dividends)
    }

    pub fn calculate_dividend_distribution(
        pending_alpha: AlphaBalance,
        pending_root_alpha: AlphaBalance,
        tao_weight: U96F32,
        stake_map: BTreeMap<T::AccountId, (AlphaBalance, AlphaBalance)>,
        dividends: BTreeMap<T::AccountId, U96F32>,
    ) -> (
        BTreeMap<T::AccountId, U96F32>,
        BTreeMap<T::AccountId, U96F32>,
    ) {
        log::debug!("dividends: {dividends:?}");
        log::debug!("stake_map: {stake_map:?}");
        log::debug!("pending_alpha: {pending_alpha:?}");
        log::debug!("pending_root_alpha: {pending_root_alpha:?}");
        log::debug!("tao_weight: {tao_weight:?}");

        // Setup.
        let zero: U96F32 = asfloat!(0.0);

        // Accumulate root alpha divs and alpha_divs. For each hotkey we compute their
        // local and root dividend proportion based on their alpha_stake/root_stake
        let mut total_root_divs: U96F32 = asfloat!(0);
        let mut total_alpha_divs: U96F32 = asfloat!(0);
        let mut root_dividends: BTreeMap<T::AccountId, U96F32> = BTreeMap::new();
        let mut alpha_dividends: BTreeMap<T::AccountId, U96F32> = BTreeMap::new();
        for (hotkey, dividend) in dividends {
            if let Some((alpha_stake, root_stake)) = stake_map.get(&hotkey) {
                let alpha_stake = alpha_stake.to_u64();
                let root_stake = root_stake.to_u64();
                // Get hotkey ALPHA on subnet.
                let alpha_stake = asfloat!(alpha_stake);
                // Get hotkey TAO on root.
                let root_stake = asfloat!(root_stake);

                // Convert TAO to alpha with weight.
                let root_alpha = root_stake.saturating_mul(tao_weight);
                // Get total from root and local
                let total_alpha = alpha_stake.saturating_add(root_alpha);
                // Compute root prop.
                let root_prop = root_alpha.checked_div(total_alpha).unwrap_or(zero);
                // Compute root dividends
                let root_divs = dividend.saturating_mul(root_prop);
                // Compute alpha dividends
                let alpha_divs = dividend.saturating_sub(root_divs);
                // Record the alpha dividends.
                alpha_dividends
                    .entry(hotkey.clone())
                    .and_modify(|e| *e = e.saturating_add(alpha_divs))
                    .or_insert(alpha_divs);
                // Accumulate total alpha divs.
                total_alpha_divs = total_alpha_divs.saturating_add(alpha_divs);
                // Record the root dividends.
                root_dividends
                    .entry(hotkey.clone())
                    .and_modify(|e| *e = e.saturating_add(root_divs))
                    .or_insert(root_divs);
                // Accumulate total root divs.
                total_root_divs = total_root_divs.saturating_add(root_divs);
            }
        }
        log::debug!("alpha_dividends: {alpha_dividends:?}");
        log::debug!("root_dividends: {root_dividends:?}");
        log::debug!("total_root_divs: {total_root_divs:?}");
        log::debug!("total_alpha_divs: {total_alpha_divs:?}");

        // Compute root alpha divs. Here we take
        let mut root_alpha_dividends: BTreeMap<T::AccountId, U96F32> = BTreeMap::new();
        for (hotkey, root_divs) in root_dividends {
            // Root proportion.
            let root_share: U96F32 = root_divs.checked_div(total_root_divs).unwrap_or(zero);
            log::debug!("hotkey: {hotkey:?}, root_share: {root_share:?}");
            // Root proportion in alpha
            let root_alpha: U96F32 = asfloat!(pending_root_alpha).saturating_mul(root_share);
            log::debug!("hotkey: {hotkey:?}, root_alpha: {root_alpha:?}");
            // Record root dividends as TAO.
            root_alpha_dividends
                .entry(hotkey)
                .and_modify(|e| *e = root_alpha)
                .or_insert(root_alpha);
        }
        log::debug!("root_alpha_dividends: {root_alpha_dividends:?}");

        // Compute proportional alpha divs using the pending alpha and total alpha divs from the epoch.
        let mut prop_alpha_dividends: BTreeMap<T::AccountId, U96F32> = BTreeMap::new();
        for (hotkey, alpha_divs) in alpha_dividends {
            // Alpha proportion.
            let alpha_share: U96F32 = alpha_divs.checked_div(total_alpha_divs).unwrap_or(zero);
            log::debug!("hotkey: {hotkey:?}, alpha_share: {alpha_share:?}");

            // Compute the proportional pending_alpha to this hotkey.
            let prop_alpha = asfloat!(pending_alpha).saturating_mul(alpha_share);
            log::debug!("hotkey: {hotkey:?}, prop_alpha: {prop_alpha:?}");
            // Record the proportional alpha dividends.
            prop_alpha_dividends
                .entry(hotkey.clone())
                .and_modify(|e| *e = prop_alpha)
                .or_insert(prop_alpha);
        }
        log::debug!("prop_alpha_dividends: {prop_alpha_dividends:?}");

        (prop_alpha_dividends, root_alpha_dividends)
    }

    fn get_owner_hotkeys(netuid: NetUid, coldkey: &T::AccountId) -> Vec<T::AccountId> {
        // Gather (block, uid, hotkey) only for hotkeys that have a UID and a registration block.
        let mut triples: Vec<(u64, u16, T::AccountId)> = OwnedHotkeys::<T>::get(coldkey)
            .into_iter()
            .filter_map(|hotkey| {
                // Uids must exist, filter_map ignores hotkeys without UID
                Uids::<T>::get(netuid, &hotkey).map(|uid| {
                    let block = BlockAtRegistration::<T>::get(netuid, uid);
                    (block, uid, hotkey)
                })
            })
            .collect();

        // Sort by BlockAtRegistration (descending), then by uid (ascending)
        // Recent registration is priority so that we can let older keys expire (get non-immune)
        triples.sort_by(|(b1, u1, _), (b2, u2, _)| b2.cmp(b1).then(u1.cmp(u2)));

        // Project to just hotkeys
        let mut owner_hotkeys: Vec<T::AccountId> =
            triples.into_iter().map(|(_, _, hk)| hk).collect();

        // Insert subnet owner hotkey in the beginning of the list if valid and not
        // already present
        if let Ok(owner_hk) = SubnetOwnerHotkey::<T>::try_get(netuid)
            && Uids::<T>::get(netuid, &owner_hk).is_some()
            && !owner_hotkeys.contains(&owner_hk)
        {
            owner_hotkeys.insert(0, owner_hk);
        }

        owner_hotkeys
    }

    pub fn distribute_dividends_and_incentives(
        netuid: NetUid,
        owner_cut: AlphaBalance,
        incentives: BTreeMap<T::AccountId, AlphaBalance>,
        alpha_dividends: BTreeMap<T::AccountId, U96F32>,
        root_alpha_dividends: BTreeMap<T::AccountId, U96F32>,
    ) {
        // Distribute the owner cut.
        if let Ok(owner_coldkey) = SubnetOwner::<T>::try_get(netuid)
            && let Ok(owner_hotkey) = SubnetOwnerHotkey::<T>::try_get(netuid)
        {
            // Increase stake for owner hotkey and coldkey.
            log::debug!(
                "owner_hotkey: {owner_hotkey:?} owner_coldkey: {owner_coldkey:?}, owner_cut: {owner_cut:?}"
            );
            Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &owner_hotkey,
                &owner_coldkey,
                netuid,
                owner_cut,
            );
            // If the subnet is leased, notify the lease logic that owner cut has been distributed.
            if let Some(lease_id) = SubnetUidToLeaseId::<T>::get(netuid) {
                Self::distribute_leased_network_dividends(lease_id, owner_cut);
            }

            // Auto-lock owner's cut
            Self::auto_lock_owner_cut(netuid, owner_cut);
        }

        // Distribute mining incentives.
        let subnet_owner_coldkey = SubnetOwner::<T>::get(netuid);
        let owner_hotkeys = Self::get_owner_hotkeys(netuid, &subnet_owner_coldkey);
        log::debug!("incentives: owner hotkeys: {owner_hotkeys:?}");
        // Track total miner emission vs the portion withheld from miners this tempo
        // (directed to an owner/immune hotkey) to record the withheld proportion.
        let mut total_incentive: AlphaBalance = AlphaBalance::ZERO;
        let mut withheld_incentive: AlphaBalance = AlphaBalance::ZERO;
        for (hotkey, incentive) in incentives {
            log::debug!("incentives: hotkey: {incentive:?}");
            total_incentive = total_incentive.saturating_add(incentive);

            // Skip/burn miner-emission for immune keys
            if owner_hotkeys.contains(&hotkey) {
                log::debug!(
                    "incentives: hotkey: {hotkey:?} is SN owner hotkey or associated hotkey, skipping {incentive:?}"
                );
                // Miner emission directed to an owner (immune) hotkey is withheld from
                // miners whether it is recycled or burned. Count both toward the withheld
                // proportion so the emission penalty cannot be dodged by choosing Recycle
                // and an unset RecycleOrBurn config is not uniquely penalized.
                withheld_incentive = withheld_incentive.saturating_add(incentive);
                // Check if we should recycle or burn the incentive
                match RecycleOrBurn::<T>::try_get(netuid) {
                    Ok(RecycleOrBurnEnum::Recycle) => {
                        log::debug!("recycling {incentive:?}");
                        Self::recycle_subnet_alpha(netuid, incentive);
                    }
                    Ok(RecycleOrBurnEnum::Burn) | Err(_) => {
                        log::debug!("burning {incentive:?}");
                        Self::burn_subnet_alpha(netuid, incentive);
                    }
                }
                continue;
            }

            let owner: T::AccountId = Owner::<T>::get(&hotkey);

            // Settle collateral first: below a miner-set floor, part of the
            // emission is captured into the lock (staked to the registered
            // hotkey itself, never the auto-stake destination, so it lands on
            // the guarded position); above the floor, earned emission
            // releases locked collateral. Miner incentive is fully
            // capturable. Only the uncaptured remainder is credited below.
            let captured =
                Self::settle_miner_collateral(netuid, &hotkey, &owner, incentive, incentive);
            let liquid = incentive.saturating_sub(captured);
            if liquid.is_zero() {
                continue;
            }

            let maybe_dest = AutoStakeDestination::<T>::get(&owner, netuid);

            // Always stake but only emit event if autostake is set.
            let destination = maybe_dest.clone().unwrap_or(hotkey.clone());

            if let Some(dest) = maybe_dest {
                log::debug!("incentives: auto staking {liquid:?} to {dest:?}");
                Self::deposit_event(Event::<T>::AutoStakeAdded {
                    netuid,
                    destination: dest,
                    hotkey: hotkey.clone(),
                    owner: owner.clone(),
                    incentive: liquid,
                });
            }

            Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &destination,
                &owner,
                netuid,
                liquid,
            );
        }

        // Record the proportion of this tempo's miner emission that was withheld from
        // miners (directed to owner/immune hotkeys, whether recycled or burned).
        let withheld_proportion: U96F32 = U96F32::saturating_from_num(withheld_incentive.to_u64())
            .checked_div(U96F32::saturating_from_num(total_incentive.to_u64()))
            .unwrap_or_else(|| U96F32::saturating_from_num(0));
        MinerBurned::<T>::insert(netuid, withheld_proportion);

        // Distribute alpha divs. Split take vs nominators first so nominator
        // shares can never be floor-captured into owner collateral. Full
        // dividend emission still drives release rate / earned; only the
        // validator take is capturable.
        let _ = AlphaDividendsPerSubnet::<T>::clear_prefix(netuid, u32::MAX, None);
        for (hotkey, alpha_divs) in alpha_dividends {
            let owner: T::AccountId = Owner::<T>::get(&hotkey);
            let total: AlphaBalance = tou64!(alpha_divs).into();
            let alpha_take: U96F32 =
                Self::get_hotkey_take_float(&hotkey).saturating_mul(alpha_divs);
            let nominator_divs: U96F32 = alpha_divs.saturating_sub(alpha_take);
            let take: AlphaBalance = tou64!(alpha_take).into();
            let captured = Self::settle_miner_collateral(netuid, &hotkey, &owner, total, take);
            let liquid_take = take.saturating_sub(captured);
            if !liquid_take.is_zero() {
                log::debug!("hotkey: {hotkey:?} alpha_take: {liquid_take:?}");
                Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey,
                    &owner,
                    netuid,
                    liquid_take,
                );
            }
            let nominator_alpha: AlphaBalance = tou64!(nominator_divs).into();
            if !nominator_alpha.is_zero() {
                log::debug!("hotkey: {hotkey:?} alpha_divs: {nominator_divs:?}");
                Self::increase_stake_for_hotkey_on_subnet(&hotkey, netuid, nominator_alpha);
                AlphaDividendsPerSubnet::<T>::mutate(netuid, &hotkey, |divs| {
                    *divs = divs.saturating_add(nominator_alpha);
                });
            }
            let total_hotkey_alpha = TotalHotkeyAlpha::<T>::get(&hotkey, netuid);
            TotalHotkeyAlphaLastEpoch::<T>::insert(hotkey, netuid, total_hotkey_alpha);
        }

        // Distribute root alpha divs. Same ownership rule: full root emission
        // for release/earned; only validator take is capturable.
        let _ = RootAlphaDividendsPerSubnet::<T>::clear_prefix(netuid, u32::MAX, None);
        for (hotkey, root_alpha) in root_alpha_dividends {
            let owner: T::AccountId = Owner::<T>::get(&hotkey);
            let total: AlphaBalance = tou64!(root_alpha).into();
            let alpha_take: U96F32 =
                Self::get_hotkey_take_float(&hotkey).saturating_mul(root_alpha);
            let root_claimable: U96F32 = root_alpha.saturating_sub(alpha_take);
            let take: AlphaBalance = tou64!(alpha_take).into();
            let captured = Self::settle_miner_collateral(netuid, &hotkey, &owner, total, take);
            let liquid_take = take.saturating_sub(captured);
            if !liquid_take.is_zero() {
                log::debug!("hotkey: {hotkey:?} alpha_take: {liquid_take:?}");
                Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey,
                    &owner,
                    netuid,
                    liquid_take,
                );
            }

            let root_claimable_alpha: AlphaBalance = tou64!(root_claimable).into();
            if !root_claimable_alpha.is_zero() {
                Self::increase_root_claimable_for_hotkey_and_subnet(
                    &hotkey,
                    netuid,
                    root_claimable_alpha,
                );

                RootAlphaDividendsPerSubnet::<T>::mutate(netuid, &hotkey, |divs| {
                    *divs = divs.saturating_add(root_claimable_alpha);
                });
            }
        }
    }

    pub fn get_stake_map(
        netuid: NetUid,
        hotkeys: Vec<&T::AccountId>,
    ) -> BTreeMap<T::AccountId, (AlphaBalance, AlphaBalance)> {
        let mut stake_map: BTreeMap<T::AccountId, (AlphaBalance, AlphaBalance)> = BTreeMap::new();
        for hotkey in hotkeys {
            // Get hotkey ALPHA on subnet.
            let alpha_stake = Self::get_stake_for_hotkey_on_subnet(hotkey, netuid);
            // Get hotkey TAO on root.
            let root_stake = Self::get_stake_for_hotkey_on_subnet(hotkey, NetUid::ROOT);
            stake_map.insert(hotkey.clone(), (alpha_stake, root_stake));
        }
        stake_map
    }

    pub fn calculate_dividend_and_incentive_distribution(
        netuid: NetUid,
        pending_root_alpha: AlphaBalance,
        pending_validator_alpha: AlphaBalance,
        hotkey_emission: Vec<(T::AccountId, AlphaBalance, AlphaBalance)>,
        tao_weight: U96F32,
    ) -> (
        BTreeMap<T::AccountId, AlphaBalance>,
        (
            BTreeMap<T::AccountId, U96F32>,
            BTreeMap<T::AccountId, U96F32>,
        ),
    ) {
        let (incentives, dividends) =
            Self::calculate_dividends_and_incentives(netuid, hotkey_emission);

        let stake_map = Self::get_stake_map(netuid, dividends.keys().collect::<Vec<_>>());

        let (alpha_dividends, root_alpha_dividends) = Self::calculate_dividend_distribution(
            pending_validator_alpha,
            pending_root_alpha,
            tao_weight,
            stake_map,
            dividends,
        );

        (incentives, (alpha_dividends, root_alpha_dividends))
    }

    pub fn distribute_emission(
        netuid: NetUid,
        pending_server_alpha: AlphaBalance,
        pending_validator_alpha: AlphaBalance,
        pending_root_alpha: AlphaBalance,
        pending_owner_cut: AlphaBalance,
    ) {
        log::debug!(
            "Draining pending alpha emission for netuid {netuid:?}, pending_server_alpha: {pending_server_alpha:?}, pending_validator_alpha: {pending_validator_alpha:?}, pending_root_alpha: {pending_root_alpha:?}, pending_owner_cut: {pending_owner_cut:?}"
        );

        let tao_weight = Self::get_tao_weight();
        let total_alpha_minus_owner_cut = pending_server_alpha
            .saturating_add(pending_validator_alpha)
            .saturating_add(pending_root_alpha);

        // Run the epoch, using the alpha going to both the servers and the validators.
        let hotkey_emission: Vec<(T::AccountId, AlphaBalance, AlphaBalance)> =
            Self::epoch_with_mechanisms(netuid, total_alpha_minus_owner_cut);
        log::debug!("hotkey_emission: {hotkey_emission:?}");

        // Compute the pending validator alpha.
        // This is the total alpha being injected,
        // minus the the alpha for the miners, (50%)
        // and minus the alpha swapped for TAO (pending_swapped).
        // Important! If the incentives are 0, then Validators get 100% of the alpha.
        let incentive_sum = hotkey_emission
            .iter()
            .fold(AlphaBalance::default(), |acc, (_, incentive, _)| {
                acc.saturating_add(*incentive)
            });
        log::debug!("incentive_sum: {incentive_sum:?}");

        let validator_alpha = if !incentive_sum.is_zero() {
            pending_validator_alpha
        } else {
            // If the incentive is 0, then Alpha Validators get both the server and validator alpha.
            pending_validator_alpha.saturating_add(pending_server_alpha)
        };
        let root_alpha = pending_root_alpha;
        let owner_cut = pending_owner_cut;

        let (incentives, (alpha_dividends, root_alpha_dividends)) =
            Self::calculate_dividend_and_incentive_distribution(
                netuid,
                root_alpha,
                validator_alpha,
                hotkey_emission,
                tao_weight,
            );

        Self::distribute_dividends_and_incentives(
            netuid,
            owner_cut,
            incentives,
            alpha_dividends,
            root_alpha_dividends,
        );
    }

    /// Returns the self contribution of a hotkey on a subnet.
    /// This is the portion of the hotkey's stake that is provided by itself, and not delegated to other hotkeys.
    pub fn get_self_contribution(hotkey: &T::AccountId, netuid: NetUid) -> u64 {
        // Get all childkeys for this hotkey.
        let childkeys = Self::get_children(hotkey, netuid);
        let mut remaining_proportion: U96F32 = U96F32::saturating_from_num(1.0);
        for (proportion, _) in childkeys {
            remaining_proportion = remaining_proportion.saturating_sub(
                U96F32::saturating_from_num(proportion) // Normalize
                    .safe_div(U96F32::saturating_from_num(u64::MAX)),
            );
        }

        // Get TAO weight
        let tao_weight: U96F32 = Self::get_tao_weight();

        // Get the hotkey's stake including weight
        let root_stake: U96F32 =
            U96F32::saturating_from_num(Self::get_stake_for_hotkey_on_subnet(hotkey, NetUid::ROOT));
        let alpha_stake: U96F32 =
            U96F32::saturating_from_num(Self::get_stake_for_hotkey_on_subnet(hotkey, netuid));

        // Calculate the
        let alpha_contribution: U96F32 = alpha_stake.saturating_mul(remaining_proportion);
        let root_contribution: U96F32 = root_stake
            .saturating_mul(remaining_proportion)
            .saturating_mul(tao_weight);
        let combined_contribution: U96F32 = alpha_contribution.saturating_add(root_contribution);

        // Return the combined contribution as a u64
        combined_contribution.saturating_to_num::<u64>()
    }

    /// Returns a list of tuples for each parent associated with this hotkey including self
    /// Each tuples contains the dividends owed to that hotkey given their parent proportion
    /// The hotkey child take proportion is removed from this and added to the tuples for self.
    /// The hotkey also gets a portion based on its own stake contribution, this is added to the childkey take.
    ///
    /// # Arguments
    /// * `hotkye`: The hotkey to distribute out from.
    /// * `netuid`: The netuid we are computing on.
    /// * `dividends`: the dividends to distribute.
    ///
    /// # Returns
    /// * dividend_tuples: `Vec<(T::AccountId, u64)>` - Vector of (hotkey, divs) for each parent including self.
    ///
    pub fn get_parent_child_dividends_distribution(
        hotkey: &T::AccountId,
        netuid: NetUid,
        dividends: AlphaBalance,
    ) -> Vec<(T::AccountId, AlphaBalance)> {
        // hotkey dividends.
        let mut dividend_tuples: Vec<(T::AccountId, AlphaBalance)> = vec![];

        // Calculate the hotkey's share of the validator emission based on its childkey take
        let validating_emission: U96F32 = U96F32::saturating_from_num(dividends);
        let mut remaining_emission: U96F32 = validating_emission;
        let burn_take_proportion: U96F32 = Self::get_ck_burn();
        let child_take_proportion: U96F32 =
            U96F32::saturating_from_num(Self::get_childkey_take(hotkey, netuid))
                .safe_div(U96F32::saturating_from_num(u16::MAX));
        log::debug!("Childkey take proportion: {child_take_proportion:?} for hotkey {hotkey:?}");
        // NOTE: Only the validation emission should be split amongst parents.

        // Grab the owner of the childkey.
        let childkey_owner = Self::get_owning_coldkey_for_hotkey(hotkey);

        // Initialize variables to track emission distribution
        let mut to_parents: u64 = 0;
        let mut total_child_take: U96F32 = U96F32::saturating_from_num(0);

        // Initialize variables to calculate total stakes from parents
        let mut total_contribution: U96F32 = U96F32::saturating_from_num(0);
        let mut parent_contributions: Vec<(T::AccountId, U96F32)> = Vec::new();

        // Get the weights for root and alpha stakes in emission distribution
        let tao_weight: U96F32 = Self::get_tao_weight();

        // Get self contribution, removing any childkey proportions.
        let self_contribution = Self::get_self_contribution(hotkey, netuid);
        log::debug!(
            "Self contribution for hotkey {hotkey:?} on netuid {netuid:?}: {self_contribution:?}"
        );
        // Add self contribution to total contribution but not to the parent contributions.
        total_contribution =
            total_contribution.saturating_add(U96F32::saturating_from_num(self_contribution));

        // Calculate total root and alpha (subnet-specific) stakes from all parents
        for (proportion, parent) in Self::get_parents(hotkey, netuid) {
            // Convert the parent's stake proportion to a fractional value
            let parent_proportion: U96F32 = U96F32::saturating_from_num(proportion)
                .safe_div(U96F32::saturating_from_num(u64::MAX));

            // Get the parent's root and subnet-specific (alpha) stakes
            let parent_root: U96F32 = U96F32::saturating_from_num(
                Self::get_stake_for_hotkey_on_subnet(&parent, NetUid::ROOT),
            );
            let parent_alpha: U96F32 =
                U96F32::saturating_from_num(Self::get_stake_for_hotkey_on_subnet(&parent, netuid));

            // Calculate the parent's contribution to the hotkey's stakes
            let parent_alpha_contribution: U96F32 = parent_alpha.saturating_mul(parent_proportion);
            let parent_root_contribution: U96F32 = parent_root
                .saturating_mul(parent_proportion)
                .saturating_mul(tao_weight);
            let combined_contribution: U96F32 =
                parent_alpha_contribution.saturating_add(parent_root_contribution);

            // Add to the total stakes
            total_contribution = total_contribution.saturating_add(combined_contribution);
            // Store the parent's contributions for later use
            parent_contributions.push((parent.clone(), combined_contribution));
            log::debug!(
                "Parent contribution for hotkey {hotkey:?} from parent {parent:?}: {combined_contribution:?}"
            );
        }

        // Distribute emission to parents based on their contributions.
        // Deduct childkey take from parent contribution.
        for (parent, contribution) in parent_contributions {
            let parent_owner = Self::get_owning_coldkey_for_hotkey(&parent);

            // Get the stake contribution of this parent key of the total stake.
            let emission_factor: U96F32 = contribution
                .checked_div(total_contribution)
                .unwrap_or(U96F32::saturating_from_num(0));

            // Get the parent's portion of the validating emission based on their contribution.
            let mut parent_emission: U96F32 = validating_emission.saturating_mul(emission_factor);
            // Remove this emission from the remaining emission.
            remaining_emission = remaining_emission.saturating_sub(parent_emission);

            // Get the childkey take for this parent.
            let mut burn_take: U96F32 = U96F32::saturating_from_num(0);
            let mut child_take: U96F32 = U96F32::saturating_from_num(0);
            if parent_owner != childkey_owner {
                // The parent is from a different coldkey, we burn some proportion
                burn_take = burn_take_proportion.saturating_mul(parent_emission);
                child_take = child_take_proportion.saturating_mul(parent_emission);
                parent_emission = parent_emission.saturating_sub(burn_take);
                parent_emission = parent_emission.saturating_sub(child_take);
                total_child_take = total_child_take.saturating_add(child_take);

                Self::recycle_subnet_alpha(
                    netuid,
                    AlphaBalance::from(burn_take.saturating_to_num::<u64>()),
                );
            };
            log::debug!("burn_takee: {burn_take:?} for hotkey {hotkey:?}");
            log::debug!("child_take: {child_take:?} for hotkey {hotkey:?}");
            log::debug!("parent_emission: {parent_emission:?} for hotkey {hotkey:?}");
            log::debug!("total_child_take: {total_child_take:?} for hotkey {hotkey:?}");

            log::debug!("remaining emission: {remaining_emission:?}");

            // Add the parent's emission to the distribution list
            dividend_tuples.push((
                parent.clone(),
                parent_emission.saturating_to_num::<u64>().into(),
            ));

            // Keep track of total emission distributed to parents
            to_parents = to_parents.saturating_add(parent_emission.saturating_to_num::<u64>());
            log::debug!(
                "Parent contribution for parent {parent:?} with contribution: {contribution:?}, of total: {total_contribution:?} ({emission_factor:?}), of emission: {validating_emission:?} gets: {parent_emission:?}",
            );
        }
        // Calculate the final emission for the hotkey itself.
        // This includes the take left from the parents and the self contribution.
        let child_emission = remaining_emission
            .saturating_add(total_child_take)
            .saturating_to_num::<u64>()
            .into();

        // Add the hotkey's own emission to the distribution list
        dividend_tuples.push((hotkey.clone(), child_emission));

        dividend_tuples
    }

    /// Checks if the epoch should run for a given subnet based on the current block.
    ///
    /// # Arguments
    /// * `netuid`: The unique identifier of the subnet.
    ///
    /// # Returns
    /// * `bool`: True if the epoch should run, false otherwise.
    pub fn should_run_epoch(netuid: NetUid, current_block: u64) -> bool {
        let tempo = Self::get_tempo(netuid);
        Self::should_run_epoch_with_tempo(netuid, current_block, tempo)
    }

    /// Same predicate as `should_run_epoch`, using an already-loaded tempo so
    /// callers that also need the tempo do not charge a duplicate storage read.
    fn should_run_epoch_with_tempo(netuid: NetUid, current_block: u64, tempo: u16) -> bool {
        if tempo == 0 {
            return false;
        }
        let pending = PendingEpochAt::<T>::get(netuid);
        if pending > 0 && current_block >= pending {
            return true;
        }
        if BlocksSinceLastStep::<T>::get(netuid) > u64::from(tempo) {
            return true;
        }
        let last = LastEpochBlock::<T>::get(netuid);
        let blocks_since = current_block.saturating_sub(last);
        blocks_since >= tempo as u64
    }

    /// Returns the number of blocks remaining before the next automatic epoch under the
    /// stateful scheduler (period `tempo`, anchored on `LastEpochBlock`). Does NOT account for:
    ///     - `PendingEpochAt` (owner-triggered manual fire — could happen sooner),
    ///     - `BlocksSinceLastStep > tempo` safety-net,
    ///     - per-block-cap defer (could push the actual fire one or more blocks later)
    /// Used by the admin-freeze-window predicate and external tooling. Returns `u64::MAX` when
    /// `tempo == 0` (legacy defensive short-circuit).
    pub fn blocks_until_next_auto_epoch(netuid: NetUid, tempo: u16, block_number: u64) -> u64 {
        if tempo == 0 {
            return u64::MAX;
        }
        let last = LastEpochBlock::<T>::get(netuid);
        // Period is `tempo`: next firing at `last + tempo`.
        let next_auto = last.saturating_add(tempo as u64);
        next_auto.saturating_sub(block_number)
    }

    /// Returns the absolute block number at which the next epoch is expected to fire for the
    /// given subnet, considering both the automatic schedule (`LastEpochBlock + tempo`) and
    /// any owner-triggered `PendingEpochAt`. Returns `None` if `tempo == 0` (subnet does not run).
    /// Does NOT account for the per-block cap deferral or the `BlocksSinceLastStep > tempo`
    /// safety-net (which can fire earlier under extreme drift).
    pub fn get_next_epoch_start_block(netuid: NetUid) -> Option<u64> {
        let tempo = Self::get_tempo(netuid);
        if tempo == 0 {
            return None;
        }
        let last = LastEpochBlock::<T>::get(netuid);
        let auto_next = last.saturating_add(tempo as u64);

        let pending = PendingEpochAt::<T>::get(netuid);
        if pending > 0 {
            Some(auto_next.min(pending))
        } else {
            Some(auto_next)
        }
    }
}
