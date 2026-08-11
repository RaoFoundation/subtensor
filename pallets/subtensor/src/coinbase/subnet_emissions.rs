use super::*;
use alloc::collections::BTreeMap;
use safe_math::FixedExt;
use substrate_fixed::transcendental::{exp, ln};
use substrate_fixed::types::{I32F32, I64F64, U64F64, U96F32};

impl<T: Config> Pallet<T> {
    /// Returns the subnets that are eligible to receive emissions.
    ///
    /// # Arguments
    /// * `subnets`: Candidate subnet IDs to evaluate in order.
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
        // They stay in the map so the normal alpha_out path still runs.
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

    // Price-based emission shares: normalize EMA prices, reweight each share by
    // (1 - MinerBurned), renormalize, then pass the result through the emission
    // gate. Emit-disabled subnets are zeroed and their share redistributed to
    // enabled subnets in `get_subnet_block_emissions`.
    pub(crate) fn get_shares(subnets_to_emit_to: &[NetUid]) -> BTreeMap<NetUid, U64F64> {
        let price_shares = Self::get_shares_price_ema(subnets_to_emit_to);

        // Reallocate emission away from subnets that withhold miner emission.
        // The effective pre-gate weight is price_i * (1 - miner_burned_i).
        let zero = U64F64::saturating_from_num(0);
        let one = U64F64::saturating_from_num(1);
        let weighted: BTreeMap<NetUid, U64F64> = price_shares
            .iter()
            .map(|(netuid, share)| {
                let burned = U64F64::saturating_from_num(MinerBurned::<T>::get(netuid)).min(one);
                let factor = one.saturating_sub(burned);

                (*netuid, share.saturating_mul(factor))
            })
            .collect();

        let total_weight = weighted
            .values()
            .copied()
            .fold(zero, |acc, weight| acc.saturating_add(weight));

        let mut shares = if total_weight > zero {
            weighted
                .into_iter()
                .map(|(netuid, weight)| (netuid, weight.safe_div(total_weight)))
                .collect()
        } else {
            // If every weight is zero (for example, every eligible subnet has
            // MinerBurned == 1), restore the price shares so emission is not stranded.
            price_shares
        };

        Self::maybe_update_emission_gate_bar(&shares);
        Self::apply_emission_gate(&mut shares);
        shares
    }

    /// Blocks between emission gate bar recomputations. Matching the standard
    /// tempo keeps the bar steady between epochs so per-block price wiggles
    /// cannot flap emission at the boundary.
    pub const EMISSION_BAR_UPDATE_INTERVAL: u64 = 360;

    /// Recomputes the emission gate bar (theta) when due.
    ///
    /// Two selection modes, both properties of the demand distribution over
    /// *positive* shares, so registering empty subnets does not move the bar:
    ///
    /// * Rank mode (`EmissionBarRank` N > 0): theta is the Nth-largest demand
    ///   share, so exactly the top N subnets sit at or above the gate midpoint
    ///   regardless of how the distribution shifts. If fewer than N subnets
    ///   have demand, theta is the smallest positive share (everyone passes).
    ///
    /// * q-mass mode (N == 0): sort demand shares descending and accumulate
    ///   until the running total crosses `EmissionBarQuantile` (q); the share
    ///   at the crossing is the bar. Subnets above the bar collectively carry
    ///   q of demand.
    fn maybe_update_emission_gate_bar(shares: &BTreeMap<NetUid, U64F64>) {
        let zero = U64F64::saturating_from_num(0);
        let current_bar = EmissionGateBar::<T>::get();
        let block = Self::get_current_block_as_u64();
        let update_due = block
            .checked_rem(Self::EMISSION_BAR_UPDATE_INTERVAL)
            .unwrap_or_default()
            == 0;
        if current_bar > zero && !update_due {
            return;
        }

        let mut sorted: Vec<U64F64> = shares.values().copied().collect();
        sorted.sort_unstable_by(|a, b| b.cmp(a));

        let rank = EmissionBarRank::<T>::get();
        let theta = if rank > 0 {
            sorted
                .iter()
                .filter(|share| **share > zero)
                .nth(usize::from(rank).saturating_sub(1))
                .or_else(|| sorted.iter().filter(|share| **share > zero).next_back())
                .copied()
                .unwrap_or(zero)
        } else {
            let q = EmissionBarQuantile::<T>::get();
            let mut cumulative = zero;
            let mut crossing = zero;
            for share in sorted {
                cumulative = cumulative.saturating_add(share);
                crossing = share;
                if cumulative >= q {
                    break;
                }
            }
            crossing
        };

        if theta > zero {
            EmissionGateBar::<T>::put(theta);
            log::debug!("Emission gate bar updated: theta = {theta:?} (rank = {rank:?})");
        }
    }

    /// Applies the Hill emission gate in place and renormalizes.
    ///
    /// gate(s) = s^h / (s^h + theta^h), computed as 1 / (1 + (theta/s)^h) so
    /// that `safe_pow` operates on the well-conditioned ratio theta/s instead
    /// of s^h itself, which underflows I32F32 precision for deep-tail shares.
    /// The gate passes exactly 1/2 at the bar, ~1 well above it, and ~0 well
    /// below it. A zero bar (never computed) disables the gate.
    ///
    /// When every gated share underflows to zero (e.g. a stale `theta` far above
    /// all shares with a steep `h`), the ungated shares are restored so the
    /// block's emission cannot be stranded.
    pub(crate) fn apply_emission_gate(shares: &mut BTreeMap<NetUid, U64F64>) {
        let zero = U64F64::saturating_from_num(0);
        let one = U64F64::saturating_from_num(1);

        let theta = EmissionGateBar::<T>::get();
        if theta <= zero {
            return;
        }
        let h = EmissionGateExponent::<T>::get();

        let ungated = shares.clone();

        for share in shares.values_mut() {
            if *share <= zero {
                continue;
            }
            let ratio = theta.safe_div(*share);
            // For s << theta, safe_pow saturates and the gate goes to ~0;
            // for s >> theta, the ratio underflows ln and safe_pow returns 0,
            // so the gate goes to 1. Both limits are the intended behavior.
            let gate = one.safe_div(one.saturating_add(Self::safe_pow(ratio, h)));
            *share = share.saturating_mul(gate);
        }

        let total = shares
            .values()
            .copied()
            .fold(zero, |acc, s| acc.saturating_add(s));
        if total > zero {
            for share in shares.values_mut() {
                *share = share.safe_div(total);
            }
        } else {
            // Fixed-point underflow can zero every gated product (stale
            // theta ≫ share with large h). Restore the pre-gate distribution
            // rather than emit zero everywhere.
            *shares = ungated;
            log::warn!("Emission gate underflowed to zero total; restoring ungated shares");
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
