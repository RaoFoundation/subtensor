//! Sparse Yuma-consensus epoch: activity masks, stake filter, weights→bonds→emission per mechanism.

use super::*;
use crate::epoch::math::*;
use alloc::collections::BTreeMap;
use sp_std::vec;
use substrate_fixed::types::{I32F32, I64F64, I96F32};
use subtensor_runtime_common::{AlphaBalance, MechId, NetUid};

impl<T: Config> Pallet<T> {
    /// Calculates reward consensus values, then updates rank, trust, consensus, incentive, dividend, pruning_score, emission and bonds, and
    /// returns the emissions for uids/hotkeys in a given `netuid`.
    ///
    /// # Arguments
    /// * `netuid`: The network to distribute the emission onto.
    ///
    /// * `rao_emission`: The total emission for the epoch.
    ///
    /// * `debug`: Print debugging outputs.
    ///
    pub fn epoch_mechanism(
        netuid: NetUid,
        mecid: MechId,
        rao_emission: AlphaBalance,
    ) -> HotkeyEpochTerms<T> {
        // Calculate netuid storage index
        let netuid_index = Self::get_mechanism_storage_index(netuid, mecid);

        // Initialize output keys (neuron hotkeys) and UIDs
        let mut terms_map: BTreeMap<T::AccountId, EpochTerms> = Keys::<T>::iter_prefix(netuid)
            .map(|(uid, hotkey)| {
                (
                    hotkey,
                    EpochTerms {
                        uid: uid as usize,
                        ..Default::default()
                    },
                )
            })
            .collect();

        // Get subnetwork size.
        let n = Self::get_subnetwork_n(netuid);
        log::trace!("Number of Neurons in Network: {n:?}");

        // ======================
        // == Active & updated ==
        // ======================

        // Get current block.
        let current_block: u64 = Self::get_current_block_as_u64();
        log::trace!("current_block: {current_block:?}");

        // Get tempo.
        let tempo: u64 = Self::get_tempo(netuid).into();
        log::trace!("tempo:\n{tempo:?}\n");

        // Get activity cutoff.
        let activity_cutoff: u64 = Self::get_activity_cutoff_blocks(netuid);
        log::trace!("activity_cutoff: {activity_cutoff:?}");

        // Last update vector.
        let last_update: Vec<u64> = Self::get_last_update(netuid_index);
        log::trace!("Last update: {:?}", &last_update);

        // Inactive mask.
        let inactive: Vec<bool> = last_update
            .iter()
            .map(|updated| updated.saturating_add(activity_cutoff) < current_block)
            .collect();
        log::debug!("Inactive: {:?}", inactive.clone());

        // Logical negation of inactive.
        let active: Vec<bool> = inactive.iter().map(|&b| !b).collect();

        // Block at registration vector (block when each neuron was most recently registered).
        let block_at_registration: Vec<u64> = Self::neuron_block_at_registration(netuid);
        log::trace!("Block at registration: {:?}", &block_at_registration);

        // ===========
        // == Stake ==
        // ===========

        // Access network stake as normalized vector.
        let (total_stake, _alpha_stake, _tao_stake): (Vec<I64F64>, Vec<I64F64>, Vec<I64F64>) =
            Self::get_stake_weights_for_network(netuid);

        // Get the minimum stake required.
        let min_stake = Self::get_stake_threshold();

        // Get owner uid.
        let owner_uid: Option<u16> = Self::get_owner_uid(netuid);

        // Set stake of validators that doesn't meet the staking threshold to 0 as filter.
        let mut filtered_stake: Vec<I64F64> = total_stake
            .iter()
            .enumerate()
            .map(|(uid, &s)| {
                if owner_uid != Some(uid as u16) && fixed64_to_u64(s) < min_stake {
                    return I64F64::from(0);
                }
                s
            })
            .collect();
        log::debug!("Filtered stake: {:?}", &filtered_stake);

        inplace_normalize_64(&mut filtered_stake);
        let stake: Vec<I32F32> = vec_fixed64_to_fixed32(filtered_stake);
        log::debug!("Normalised Stake: {:?}", &stake);

        // =======================
        // == Validator permits ==
        // =======================

        // Get current validator permits.
        let mut validator_permits: Vec<bool> = Self::get_validator_permit(netuid);
        if let Some(owner_uid) = owner_uid
            && let Some(owner_permit) = validator_permits.get_mut(owner_uid as usize)
        {
            *owner_permit = true;
        }
        log::trace!("validator_permits: {validator_permits:?}");

        // Logical negation of validator_permits.
        let validator_forbids: Vec<bool> = validator_permits.iter().map(|&b| !b).collect();

        // Get max allowed validators.
        let max_allowed_validators: u16 = Self::get_max_allowed_validators(netuid);
        log::trace!("max_allowed_validators: {max_allowed_validators:?}");

        // Get new validator permits.
        let mut new_validator_permits: Vec<bool> =
            is_topk_nonzero_i32f32(&stake, max_allowed_validators as usize);
        if let Some(owner_uid) = owner_uid
            && let Some(owner_permit) = new_validator_permits.get_mut(owner_uid as usize)
        {
            *owner_permit = true;
        }
        log::trace!("new_validator_permits: {new_validator_permits:?}");

        // ==================
        // == Active Stake ==
        // ==================

        let mut active_stake: Vec<I32F32> = stake.clone();

        // Remove inactive stake.
        inplace_mask_vector(&inactive, &mut active_stake);

        // Remove non-validator stake.
        inplace_mask_vector(&validator_forbids, &mut active_stake);

        // Normalize active stake.
        inplace_normalize(&mut active_stake);
        log::trace!("Active Stake: {:?}", &active_stake);

        // =============
        // == Weights ==
        // =============

        // Access network weights row unnormalized.
        let mut weights: Vec<Vec<(u16, I32F32)>> = Self::get_weights_sparse(netuid_index);
        log::trace!("Weights: {:?}", &weights);

        // Mask weights that are not from permitted validators.
        weights = mask_rows_sparse(&validator_forbids, &weights);
        log::trace!("Weights (permit): {:?}", &weights);

        // Remove self-weight by masking diagonal; keep owner_uid self-weight.
        if let Some(owner_uid) = owner_uid {
            weights = mask_diag_sparse_except_index(&weights, owner_uid);
        } else {
            weights = mask_diag_sparse(&weights);
        }
        log::trace!("Weights (permit+diag): {:?}", &weights);

        // Remove weights referring to deregistered neurons.
        weights = vec_mask_sparse_matrix(
            &weights,
            &last_update,
            &block_at_registration,
            &|updated, registered| updated <= registered,
        );
        log::trace!("Weights (permit+diag+outdate): {:?}", &weights);

        if Self::get_commit_reveal_weights_enabled(netuid) {
            let mut commit_blocks: Vec<u64> = vec![u64::MAX; n as usize]; // MAX ⇒ “no active commit”

            // helper: hotkey → uid
            let uid_of = |acct: &T::AccountId| terms_map.get(acct).map(|t| t.uid);

            // ---------- v2 ------------------------------------------------------
            // `WeightCommits` tuple: (hash, commit_epoch, commit_block, _).
            // Expiry keys off `commit_epoch`; the column mask compares the absolute
            // `commit_block` against `block_at_registration` (both block numbers).
            for (who, q) in WeightCommits::<T>::iter_prefix(netuid_index) {
                for (_, commit_epoch, commit_block, _) in q.iter() {
                    if !Self::is_commit_expired(netuid, *commit_epoch) {
                        if let Some(cell) = uid_of(&who).and_then(|i| commit_blocks.get_mut(i)) {
                            *cell = (*cell).min(*commit_block);
                        }
                        break; // earliest active found
                    }
                }
            }

            // ---------- v4 ------------------------------------------------------
            // `TimelockedWeightCommits` is keyed by `commit_epoch`; the value tuple
            // carries the absolute `commit_block` in field 1.
            for (commit_epoch, q) in TimelockedWeightCommits::<T>::iter_prefix(netuid_index) {
                if Self::is_commit_expired(netuid, commit_epoch) {
                    continue;
                }
                for (who, commit_block, ..) in q.iter() {
                    if let Some(cell) = uid_of(who).and_then(|i| commit_blocks.get_mut(i)) {
                        *cell = (*cell).min(*commit_block);
                    }
                }
            }

            weights = vec_mask_sparse_matrix(
                &weights,
                &commit_blocks,
                &block_at_registration,
                &|cb, reg| cb < reg,
            );

            log::trace!(
                "Commit-reveal column mask applied ({} masked rows)",
                commit_blocks.iter().filter(|&&cb| cb != u64::MAX).count()
            );
        }

        // Normalize remaining weights.
        inplace_row_normalize_sparse(&mut weights);
        log::trace!("Weights (mask+norm): {:?}", &weights);

        // ================================
        // == Consensus, Validator Trust ==
        // ================================

        // Consensus majority ratio, e.g. 51%.
        let kappa: I32F32 = Self::kappa_proportion_as_i32f32(netuid);
        // Calculate consensus as stake-weighted median of weights.
        let consensus: Vec<I32F32> = weighted_median_col_sparse(&active_stake, &weights, n, kappa);
        log::trace!("Consensus: {:?}", &consensus);

        // Clip weights at majority consensus.
        let clipped_weights: Vec<Vec<(u16, I32F32)>> = col_clip_sparse(&weights, &consensus);
        log::trace!("Clipped Weights: {:?}", &clipped_weights);

        // Calculate validator trust as sum of clipped weights set by validator.
        let validator_trust: Vec<I32F32> = row_sum_sparse(&clipped_weights);
        log::trace!("Validator Trust: {:?}", &validator_trust);

        // =============================
        // == Ranks, Trust, Incentive ==
        // =============================

        // Compute ranks: r_j = SUM(i) w_ij * s_i.
        let mut ranks: Vec<I32F32> = matmul_sparse(&clipped_weights, &active_stake, n);

        inplace_normalize(&mut ranks); // range: I32F32(0, 1)
        let incentive: Vec<I32F32> = ranks.clone();
        log::trace!("Incentive (=Rank): {:?}", &incentive);

        // =========================
        // == Bonds and Dividends ==
        // =========================

        // Get validator bonds penalty in [0, 1].
        let bonds_penalty: I32F32 = Self::bonds_penalty_proportion_as_i32f32(netuid);
        // Calculate weights for bonds, apply bonds penalty to weights.
        // bonds_penalty = 0: weights_for_bonds = weights.clone()
        // bonds_penalty = 1: weights_for_bonds = clipped_weights.clone()
        let weights_for_bonds: Vec<Vec<(u16, I32F32)>> =
            interpolate_sparse(&weights, &clipped_weights, n, bonds_penalty);

        let mut dividends: Vec<I32F32>;
        let mut ema_bonds: Vec<Vec<(u16, I32F32)>>;
        if Yuma3On::<T>::get(netuid) {
            // Access network bonds.
            let mut bonds = Self::bonds_sparse_as_u16_proportion(netuid_index);
            log::trace!("Bonds: {:?}", &bonds);

            // Remove bonds referring to neurons that have registered since last tempo.
            // Mask if: the last tempo block happened *before* the registration block
            // ==> last_tempo <= registered
            // For dynamic tempo - we pick previous-successful-epoch block: `LastMechansimStepBlock + 1`
            let lms = LastMechansimStepBlock::<T>::get(netuid);
            let last_tempo: u64 = if lms == 0 {
                current_block.saturating_sub(tempo)
            } else {
                lms.saturating_add(1)
            };
            bonds = scalar_vec_mask_sparse_matrix(
                &bonds,
                last_tempo,
                &block_at_registration,
                &|last_tempo, registered| last_tempo <= registered,
            );
            log::trace!("Bonds: (mask) {:?}", &bonds);

            // Compute the Exponential Moving Average (EMA) of bonds.
            log::trace!("weights_for_bonds: {:?}", &weights_for_bonds);
            ema_bonds = Self::ema_bonds_liquid_or_normal_sparse(
                netuid_index,
                &weights_for_bonds,
                &bonds,
                &consensus,
            );
            log::trace!("emaB: {:?}", &ema_bonds);

            // Normalize EMA bonds.
            let mut ema_bonds_norm = ema_bonds.clone();
            inplace_col_normalize_sparse(&mut ema_bonds_norm, n); // sum_i b_ij = 1
            log::trace!("emaB norm: {:?}", &ema_bonds_norm);

            // # === Dividend Calculation===
            let total_bonds_per_validator: Vec<I32F32> =
                row_sum_sparse(&mat_vec_mul_sparse(&ema_bonds_norm, &incentive));
            log::trace!(
                "total_bonds_per_validator: {:?}",
                &total_bonds_per_validator
            );

            dividends = vec_mul(&total_bonds_per_validator, &active_stake);
            inplace_normalize(&mut dividends);
            log::trace!("Dividends: {:?}", &dividends);
        } else {
            // original Yuma - liquid alpha disabled
            // Access network bonds.
            let mut bonds: Vec<Vec<(u16, I32F32)>> = Self::unnormalized_bonds_sparse(netuid_index);
            log::trace!("B: {:?}", &bonds);

            // Remove bonds referring to neurons that have registered since last tempo.
            // Mask if: the last tempo block happened *before* the registration block
            // ==> last_tempo <= registered
            // For dynamic tempo - we pick previous-successful-epoch block: `LastMechansimStepBlock + 1`
            let lms = LastMechansimStepBlock::<T>::get(netuid);
            let last_tempo: u64 = if lms == 0 {
                current_block.saturating_sub(tempo)
            } else {
                lms.saturating_add(1)
            };
            bonds = scalar_vec_mask_sparse_matrix(
                &bonds,
                last_tempo,
                &block_at_registration,
                &|last_tempo, registered| last_tempo <= registered,
            );
            log::trace!("B (outdatedmask): {:?}", &bonds);

            // Normalize remaining bonds: sum_i b_ij = 1.
            inplace_col_normalize_sparse(&mut bonds, n);
            log::trace!("B (mask+norm): {:?}", &bonds);

            // Compute bonds delta column normalized.
            let mut bonds_delta: Vec<Vec<(u16, I32F32)>> =
                row_hadamard_sparse(&weights_for_bonds, &active_stake); // ΔB = W◦S (outdated W masked)
            log::trace!("ΔB: {:?}", &bonds_delta);

            // Normalize bonds delta.
            inplace_col_normalize_sparse(&mut bonds_delta, n); // sum_i b_ij = 1
            log::trace!("ΔB (norm): {:?}", &bonds_delta);

            // Compute the Exponential Moving Average (EMA) of bonds.
            ema_bonds = Self::ema_bonds_normal_sparse(&bonds_delta, &bonds, netuid_index);
            // Normalize EMA bonds.
            inplace_col_normalize_sparse(&mut ema_bonds, n); // sum_i b_ij = 1
            log::trace!("Exponential Moving Average Bonds: {:?}", &ema_bonds);

            // Compute dividends: d_i = SUM(j) b_ij * inc_j.
            // range: I32F32(0, 1)
            dividends = matmul_transpose_sparse(&ema_bonds, &incentive);
            inplace_normalize(&mut dividends);
            log::trace!("Dividends: {:?}", &dividends);

            // Column max-upscale EMA bonds for storage: max_i w_ij = 1.
            inplace_col_max_upscale_sparse(&mut ema_bonds, n);
        }

        // =================================
        // == Emission and Pruning scores ==
        // =================================

        // Compute normalized emission scores. range: I32F32(0, 1)
        let combined_emission: Vec<I32F32> = incentive
            .iter()
            .zip(dividends.clone())
            .map(|(ii, di)| ii.saturating_add(di))
            .collect();
        let emission_sum: I32F32 = combined_emission.iter().sum();

        let mut normalized_server_emission: Vec<I32F32> = incentive.clone(); // Servers get incentive.
        let mut normalized_validator_emission: Vec<I32F32> = dividends.clone(); // Validators get dividends.
        let mut normalized_combined_emission: Vec<I32F32> = combined_emission.clone();
        // Normalize on the sum of incentive + dividends.
        inplace_normalize_i32f32_with_sum(&mut normalized_server_emission, emission_sum);
        inplace_normalize_i32f32_with_sum(&mut normalized_validator_emission, emission_sum);
        inplace_normalize(&mut normalized_combined_emission);

        // If emission is zero, replace emission with normalized stake.
        if emission_sum == I32F32::from(0) {
            // no weights set | outdated weights | self_weights
            if is_zero(&active_stake) {
                // no active stake
                normalized_validator_emission.clone_from(&stake); // do not mask inactive, assumes stake is normalized
                normalized_combined_emission.clone_from(&stake);
            } else {
                normalized_validator_emission.clone_from(&active_stake); // emission proportional to inactive-masked normalized stake
                normalized_combined_emission.clone_from(&active_stake);
            }
        }

        // Compute rao based emission scores. range: I96F32(0, rao_emission)
        let float_rao_emission: I96F32 = I96F32::saturating_from_num(rao_emission);

        let server_emission: Vec<I96F32> = normalized_server_emission
            .iter()
            .map(|se: &I32F32| I96F32::saturating_from_num(*se).saturating_mul(float_rao_emission))
            .collect();
        let server_emission: Vec<AlphaBalance> = server_emission
            .iter()
            .map(|e: &I96F32| e.saturating_to_num::<u64>().into())
            .collect();

        let validator_emission: Vec<I96F32> = normalized_validator_emission
            .iter()
            .map(|ve: &I32F32| I96F32::saturating_from_num(*ve).saturating_mul(float_rao_emission))
            .collect();
        let validator_emission: Vec<AlphaBalance> = validator_emission
            .iter()
            .map(|e: &I96F32| e.saturating_to_num::<u64>().into())
            .collect();

        // Only used to track emission in storage.
        let combined_emission: Vec<I96F32> = normalized_combined_emission
            .iter()
            .map(|ce: &I32F32| I96F32::saturating_from_num(*ce).saturating_mul(float_rao_emission))
            .collect();
        let combined_emission: Vec<AlphaBalance> = combined_emission
            .iter()
            .map(|e: &I96F32| AlphaBalance::from(e.saturating_to_num::<u64>()))
            .collect();

        log::trace!(
            "Normalized Server Emission: {:?}",
            &normalized_server_emission
        );
        log::trace!("Server Emission: {:?}", &server_emission);
        log::trace!(
            "Normalized Validator Emission: {:?}",
            &normalized_validator_emission
        );
        log::trace!("Validator Emission: {:?}", &validator_emission);
        log::trace!(
            "Normalized Combined Emission: {:?}",
            &normalized_combined_emission
        );
        log::trace!("Combined Emission: {:?}", &combined_emission);

        // ===========================
        // == Populate epoch output ==
        // ===========================
        let cloned_stake_weight: Vec<u16> = stake
            .iter()
            .map(|xi| fixed_proportion_to_u16(*xi))
            .collect::<Vec<u16>>();
        let cloned_emission = combined_emission.clone();
        let cloned_consensus: Vec<u16> = consensus
            .iter()
            .map(|xi| fixed_proportion_to_u16(*xi))
            .collect::<Vec<u16>>();
        let cloned_incentive: Vec<u16> = incentive
            .iter()
            .map(|xi| fixed_proportion_to_u16(*xi))
            .collect::<Vec<u16>>();
        let cloned_dividends: Vec<u16> = dividends
            .iter()
            .map(|xi| fixed_proportion_to_u16(*xi))
            .collect::<Vec<u16>>();
        let cloned_validator_trust: Vec<u16> = validator_trust
            .iter()
            .map(|xi| fixed_proportion_to_u16(*xi))
            .collect::<Vec<u16>>();
        let raw_stake: Vec<u64> = total_stake
            .iter()
            .map(|s| s.saturating_to_num::<u64>())
            .collect::<Vec<u64>>();

        for (_hotkey, terms) in terms_map.iter_mut() {
            terms.dividend = cloned_dividends.get(terms.uid).copied().unwrap_or_default();
            terms.incentive = cloned_incentive.get(terms.uid).copied().unwrap_or_default();
            terms.validator_emission = validator_emission
                .get(terms.uid)
                .copied()
                .unwrap_or_default();
            terms.server_emission = server_emission.get(terms.uid).copied().unwrap_or_default();
            terms.stake_weight = cloned_stake_weight
                .get(terms.uid)
                .copied()
                .unwrap_or_default();
            terms.active = active.get(terms.uid).copied().unwrap_or_default();
            terms.emission = cloned_emission.get(terms.uid).copied().unwrap_or_default();
            terms.consensus = cloned_consensus.get(terms.uid).copied().unwrap_or_default();
            terms.validator_trust = cloned_validator_trust
                .get(terms.uid)
                .copied()
                .unwrap_or_default();
            terms.new_validator_permit = new_validator_permits
                .get(terms.uid)
                .copied()
                .unwrap_or_default();
            terms.stake = raw_stake.get(terms.uid).copied().unwrap_or_default().into();
            let old_validator_permit = validator_permits
                .get(terms.uid)
                .copied()
                .unwrap_or_default();

            // Bonds
            if terms.new_validator_permit {
                let ema_bond = ema_bonds.get(terms.uid).cloned().unwrap_or_default();
                terms.bond = ema_bond
                    .iter()
                    .map(|(j, value)| (*j, fixed_proportion_to_u16(*value)))
                    .collect();
            } else if old_validator_permit {
                // Only overwrite the intersection.
                terms.bond = vec![];
            }
        }

        HotkeyEpochTerms(terms_map)
    }
}
