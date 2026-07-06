use super::*;
use alloc::collections::BTreeMap;
use safe_math::FixedExt;
use substrate_fixed::transcendental::{exp, ln};
use substrate_fixed::types::{I32F32, I64F64, U64F64, U96F32};

impl<T: Config> Pallet<T> {
    /// Returns the subnets that are eligible to receive emissions.
    ///
    /// # Parameters
    /// - `subnets`: Candidate subnet IDs to evaluate in order.
    ///
    /// # Returns
    /// A vector containing the candidate subnet IDs that are non-root, have
    /// started emissions, have subtokens enabled, and currently allow network
    /// registration.
    ///
    /// AI-readable: This output is passed to `get_shares_flow`, so changing these
    /// eligibility rules also changes which subnet user TAO flow EMAs and protocol
    /// flow EMAs are advanced during emission sharing.
    pub fn get_subnets_to_emit_to(subnets: &[NetUid]) -> Vec<NetUid> {
        // Filter out root subnet.
        // Filter out subnets with no first emission block number.
        subnets
            .iter()
            .filter(|netuid| !netuid.is_root())
            .filter(|netuid| FirstEmissionBlockNumber::<T>::get(*netuid).is_some())
            .filter(|netuid| SubtokenEnabled::<T>::get(*netuid))
            .filter(|&netuid| Self::get_network_registration_allowed(*netuid))
            .copied()
            .collect()
    }

    pub fn get_subnet_block_emissions(
        subnets_to_emit_to: &[NetUid],
        block_emission: U96F32,
    ) -> BTreeMap<NetUid, U96F32> {
        // Disabled subnets get zero TAO-side emission, redistributed to enabled subnets.
        // They stay in the map so the normal alpha_out/root-prop path still runs.
        let shares = Self::get_shares(subnets_to_emit_to);
        log::debug!("Subnet emission shares = {shares:?}");

        let zero = U64F64::saturating_from_num(0.0);
        let mut shares_with_emission_enabled = Vec::with_capacity(shares.len());
        let mut has_disabled_subnets = false;
        let mut enabled_share_sum = zero;

        for (netuid, share) in shares {
            let emission_enabled = SubnetEmissionEnabled::<T>::get(netuid);

            if emission_enabled {
                enabled_share_sum = enabled_share_sum.saturating_add(share);
            } else {
                has_disabled_subnets = true;
            }

            shares_with_emission_enabled.push((netuid, share, emission_enabled));
        }

        shares_with_emission_enabled
            .into_iter()
            .map(|(netuid, share, emission_enabled)| {
                let share = if has_disabled_subnets {
                    if emission_enabled && enabled_share_sum > zero {
                        share.safe_div(enabled_share_sum)
                    } else {
                        zero
                    }
                } else {
                    share
                };
                let emission = U64F64::saturating_from_num(block_emission).saturating_mul(share);
                (netuid, U96F32::saturating_from_num(emission))
            })
            .collect::<BTreeMap<NetUid, U96F32>>()
    }
    pub fn record_tao_inflow(netuid: NetUid, tao: TaoBalance) {
        SubnetTaoFlow::<T>::mutate(netuid, |flow| {
            *flow = flow.saturating_add(u64::from(tao) as i64);
        });
    }

    pub fn record_tao_outflow(netuid: NetUid, tao: TaoBalance) {
        SubnetTaoFlow::<T>::mutate(netuid, |flow| {
            *flow = flow.saturating_sub(u64::from(tao) as i64)
        });
    }

    pub fn reset_tao_outflow(netuid: NetUid) {
        SubnetTaoFlow::<T>::remove(netuid);
    }

    pub fn record_protocol_inflow(netuid: NetUid, tao: TaoBalance) {
        SubnetProtocolFlow::<T>::mutate(netuid, |flow| {
            *flow = flow.saturating_add(u64::from(tao) as i64);
        });
    }

    pub fn record_protocol_outflow(netuid: NetUid, tao: TaoBalance) {
        SubnetProtocolFlow::<T>::mutate(netuid, |flow| {
            *flow = flow.saturating_sub(u64::from(tao) as i64);
        });
    }

    pub fn reset_protocol_flow(netuid: NetUid) {
        SubnetProtocolFlow::<T>::remove(netuid);
    }

    fn update_ema_protocol_flow(netuid: NetUid) -> I64F64 {
        let current_block: u64 = Self::get_current_block_as_u64();

        let block_flow = I64F64::saturating_from_num(SubnetProtocolFlow::<T>::get(netuid));
        let (last_block, last_block_ema) =
            SubnetEmaProtocolFlow::<T>::get(netuid).unwrap_or((0, I64F64::saturating_from_num(0)));

        if last_block != current_block {
            let flow_alpha = I64F64::saturating_from_num(FlowEmaSmoothingFactor::<T>::get())
                .safe_div(I64F64::saturating_from_num(i64::MAX));
            let one = I64F64::saturating_from_num(1);
            let ema_flow = (one.saturating_sub(flow_alpha))
                .saturating_mul(last_block_ema)
                .saturating_add(flow_alpha.saturating_mul(block_flow));
            SubnetEmaProtocolFlow::<T>::insert(netuid, (current_block, ema_flow));

            Self::reset_protocol_flow(netuid);
            ema_flow
        } else {
            last_block_ema
        }
    }

    // Update SubnetEmaTaoFlow if needed and return its value for
    // the current block
    #[allow(dead_code)]
    fn get_ema_flow(netuid: NetUid) -> I64F64 {
        let current_block: u64 = Self::get_current_block_as_u64();

        // Calculate net ema flow for the next block
        let block_flow = I64F64::saturating_from_num(SubnetTaoFlow::<T>::get(netuid));
        let (last_block, last_block_ema) =
            SubnetEmaTaoFlow::<T>::get(netuid).unwrap_or((0, I64F64::saturating_from_num(0)));

        // EMA flow already initialized
        if last_block != current_block {
            let flow_alpha = I64F64::saturating_from_num(FlowEmaSmoothingFactor::<T>::get())
                .safe_div(I64F64::saturating_from_num(i64::MAX));
            let one = I64F64::saturating_from_num(1);
            let ema_flow = (one.saturating_sub(flow_alpha))
                .saturating_mul(last_block_ema)
                .saturating_add(flow_alpha.saturating_mul(block_flow));
            SubnetEmaTaoFlow::<T>::insert(netuid, (current_block, ema_flow));

            // Drop the accumulated flow in the last block
            Self::reset_tao_outflow(netuid);
            ema_flow
        } else {
            last_block_ema
        }
    }

    // Either the minimal EMA flow L = min{Si}, or an artificial
    // cut off at some higher value A (TaoFlowCutoff)
    // L = max {A, min{min{S[i], 0}}}
    #[allow(dead_code)]
    fn get_lower_limit(ema_flows: &BTreeMap<NetUid, I64F64>) -> I64F64 {
        let zero = I64F64::saturating_from_num(0);
        let min_flow = ema_flows
            .values()
            .map(|flow| flow.min(&zero))
            .min()
            .unwrap_or(&zero);
        let flow_cutoff = TaoFlowCutoff::<T>::get();
        flow_cutoff.max(*min_flow)
    }

    // Estimate the upper value of pow with hardcoded p = 2
    fn pow_estimate(val: U64F64) -> U64F64 {
        val.saturating_mul(val)
    }

    fn safe_pow(val: U64F64, p: U64F64) -> U64F64 {
        // If val is too low so that ln(val) doesn't fit I32F32::MIN,
        // return 0 from the function
        let zero = U64F64::saturating_from_num(0);
        let i32f32_max = I32F32::saturating_from_num(i32::MAX);
        if let Ok(val_ln) = ln(I32F32::saturating_from_num(val)) {
            // If exp doesn't fit, do the best we can - max out on I32F32::MAX
            U64F64::saturating_from_num(I32F32::saturating_from_num(
                exp(I32F32::saturating_from_num(p).saturating_mul(val_ln)).unwrap_or(i32f32_max),
            ))
        } else {
            zero
        }
    }

    fn inplace_scale(offset_flows: &mut BTreeMap<NetUid, U64F64>) {
        let zero = U64F64::saturating_from_num(0);
        let flow_max = offset_flows.values().copied().max().unwrap_or(zero);

        // Calculate scale factor so that max becomes 1.0
        let flow_factor = U64F64::saturating_from_num(1).safe_div(flow_max);

        // Upscale/downscale in-place
        for flow in offset_flows.values_mut() {
            *flow = flow_factor.saturating_mul(*flow);
        }
    }

    pub(crate) fn inplace_pow_normalize(offset_flows: &mut BTreeMap<NetUid, U64F64>, p: U64F64) {
        // Scale offset flows so that that are no overflows and underflows when we use safe_pow:
        //  flow_factor * subnet_count * (flow_max ^ p) <= I32F32::MAX
        let zero = U64F64::saturating_from_num(0);
        let subnet_count = offset_flows.len();

        // Pre-scale to max 1.0
        Self::inplace_scale(offset_flows);

        // Scale to maximize precision
        let flow_max = offset_flows.values().copied().max().unwrap_or(zero);
        log::debug!("Offset flow max: {flow_max:?}");
        let flow_max_pow_est = Self::pow_estimate(flow_max);
        log::debug!("flow_max_pow_est: {flow_max_pow_est:?}");

        let max_times_count =
            U64F64::saturating_from_num(subnet_count).saturating_mul(flow_max_pow_est);
        let i32f32_max = U64F64::saturating_from_num(i32::MAX);
        let precision_min = i32f32_max.safe_div(U64F64::saturating_from_num(u64::MAX));

        // If max_times_count < precision_min, all flow values are too low to fit I32F32.
        if max_times_count >= precision_min {
            let epsilon =
                U64F64::saturating_from_num(1).safe_div(U64F64::saturating_from_num(1_000));
            let flow_factor = i32f32_max
                .safe_div(max_times_count)
                .checked_sqrt(epsilon)
                .unwrap_or(zero);

            // Calculate sum
            let sum = offset_flows
                .clone()
                .into_values()
                .map(|flow| flow_factor.saturating_mul(flow))
                .map(|scaled_flow| Self::safe_pow(scaled_flow, p))
                .sum();
            log::debug!("Scaled offset flow sum: {sum:?}");

            // Normalize in-place
            for flow in offset_flows.values_mut() {
                let scaled_flow = flow_factor.saturating_mul(*flow);
                *flow = Self::safe_pow(scaled_flow, p).safe_div(sum);
            }
        }
    }

    // Implementation of shares that uses TAO flow
    #[allow(dead_code)]
    fn get_shares_flow(subnets_to_emit_to: &[NetUid]) -> BTreeMap<NetUid, U64F64> {
        let net_flow_enabled = NetTaoFlowEnabled::<T>::get();
        let zero = I64F64::saturating_from_num(0);

        // Always update both EMAs (keeps protocol EMA warm for when toggled on).
        // Note:
        // User TAO EMAs are updated every time this method runs because get_ema_flow()
        // is called before the NetTaoFlowEnabled branch. Protocol EMAs are different:
        // update_ema_protocol_flow() is only called while NetTaoFlowEnabled is true.
        // If net flow is disabled, protocol flow keeps accumulating in SubnetProtocolFlow
        // and SubnetEmaProtocolFlow is not advanced/reset, so toggling net flow back on
        // applies stale accumulated protocol flow in the next EMA update.
        let subnet_emas: Vec<(NetUid, I64F64, I64F64)> = subnets_to_emit_to
            .iter()
            .map(|netuid| {
                let user_ema = Self::get_ema_flow(*netuid);
                let protocol_ema = Self::update_ema_protocol_flow(*netuid);
                (*netuid, user_ema, protocol_ema)
            })
            .collect();

        // When net flow is enabled, normalize protocol EMA so that its
        // positive total matches the user EMA positive total. This prevents
        // subsidy concentration: as emissions concentrate on fewer subnets,
        // their protocol EMA grows, but the normalization factor shrinks to
        // compensate, keeping the deduction proportional to user demand.
        let norm_factor = if net_flow_enabled {
            let (user_positive_ema_sum, protocol_positive_ema_sum) =
                subnet_emas
                    .iter()
                    .fold((zero, zero), |(su, sp), (_, u, p)| {
                        (
                            su.saturating_add((*u).max(zero)),
                            sp.saturating_add((*p).max(zero)),
                        )
                    });
            let one = I64F64::saturating_from_num(1);
            if protocol_positive_ema_sum > zero {
                user_positive_ema_sum
                    .safe_div(protocol_positive_ema_sum)
                    .min(one)
            } else {
                zero
            }
        } else {
            zero
        };
        log::debug!("Protocol normalization factor: {norm_factor:?}");

        let ema_flows: BTreeMap<NetUid, I64F64> = subnet_emas
            .into_iter()
            .map(|(netuid, user_ema, protocol_ema)| {
                let net = if net_flow_enabled {
                    // Only scale positive protocol cost by norm_factor. Negative
                    // protocol cost (root drain > emissions) is a benefit, kept as-is.
                    let scaled_protocol = if protocol_ema > zero {
                        norm_factor.saturating_mul(protocol_ema)
                    } else {
                        protocol_ema
                    };
                    user_ema.saturating_sub(scaled_protocol)
                } else {
                    user_ema
                };
                (netuid, net)
            })
            .collect();
        log::debug!("EMA flows (net_flow_enabled={net_flow_enabled}): {ema_flows:?}");

        // Clip the EMA flow with lower limit L
        // z[i] = max{S[i] − L, 0}
        let lower_limit = Self::get_lower_limit(&ema_flows);
        log::debug!("Lower flow limit: {lower_limit:?}");
        let mut offset_flows = ema_flows
            .iter()
            .map(|(netuid, flow)| {
                (
                    *netuid,
                    if *flow > lower_limit {
                        U64F64::saturating_from_num(flow.saturating_sub(lower_limit))
                    } else {
                        U64F64::saturating_from_num(0)
                    },
                )
            })
            .collect::<BTreeMap<NetUid, U64F64>>();

        // Normalize the set {z[i]}, using an exponent parameter (p ≥ 1)
        let p = FlowNormExponent::<T>::get();
        Self::inplace_pow_normalize(&mut offset_flows, p);
        offset_flows
    }

    // Price-based emission shares: each subnet's share is its EMA price normalized
    // by the sum of EMA prices. Emit-disabled subnets are zeroed and their share
    // redistributed to enabled subnets in `get_subnet_block_emissions`, so the
    // effective emission is e_i = p_i / sum(p_j) over emit-enabled subnets.
    pub(crate) fn get_shares(subnets_to_emit_to: &[NetUid]) -> BTreeMap<NetUid, U64F64> {
        let price_shares = Self::get_shares_price_ema(subnets_to_emit_to);

        // Weight each subnet's price share by root_proportion * (1 - miner_burned), then
        // renormalize. The effective emission is therefore proportional to
        //   root_proportion_i * price_i * (1 - miner_burned_i).
        // - root_proportion shrinks as a subnet's alpha issuance grows, so emission is
        //   reallocated away from older subnets toward newer ones (easier entrance).
        // - (1 - miner_burned) reallocates away from subnets that withhold miner emission.
        let zero = U64F64::saturating_from_num(0);
        let one = U64F64::saturating_from_num(1);
        let weighted: BTreeMap<NetUid, U64F64> = price_shares
            .iter()
            .map(|(netuid, share)| {
                let burned = U64F64::saturating_from_num(MinerBurned::<T>::get(netuid)).min(one);
                let root_prop = U64F64::saturating_from_num(Self::root_proportion(*netuid));
                let factor = root_prop.saturating_mul(one.saturating_sub(burned));
                (*netuid, share.saturating_mul(factor))
            })
            .collect();

        let total_weight = weighted
            .values()
            .copied()
            .fold(zero, |acc, w| acc.saturating_add(w));

        if total_weight > zero {
            weighted
                .into_iter()
                .map(|(netuid, w)| (netuid, w.safe_div(total_weight)))
                .collect()
        } else {
            // The combined weight zeroes out for every subnet (e.g. no root stake, or
            // every subnet burning all of its miner emission); fall back to the
            // unweighted price shares so the block's emission is not stranded.
            price_shares
        }
    }

    // Implementation of shares that uses subnet EMA prices (SubnetMovingPrice),
    // not the active/spot alpha price.
    fn get_shares_price_ema(subnets_to_emit_to: &[NetUid]) -> BTreeMap<NetUid, U64F64> {
        // Get sum of alpha moving prices
        let total_moving_prices = subnets_to_emit_to
            .iter()
            .map(|netuid| U64F64::saturating_from_num(Self::get_moving_alpha_price(*netuid)))
            .fold(U64F64::saturating_from_num(0.0), |acc, ema| {
                acc.saturating_add(ema)
            });
        log::debug!("total_moving_prices: {total_moving_prices:?}");

        // Calculate shares.
        subnets_to_emit_to
            .iter()
            .map(|netuid| {
                let moving_price =
                    U64F64::saturating_from_num(Self::get_moving_alpha_price(*netuid));
                log::debug!("moving_price_i: {moving_price:?}");

                let share = moving_price
                    .checked_div(total_moving_prices)
                    .unwrap_or(U64F64::saturating_from_num(0));

                (*netuid, share)
            })
            .collect::<BTreeMap<NetUid, U64F64>>()
    }
}
