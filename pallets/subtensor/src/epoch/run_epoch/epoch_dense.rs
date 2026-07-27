//! Dense-matrix epoch path (test-only). Production uses [`super::epoch_mechanism`].

use super::*;
use crate::epoch::math::*;
use frame_support::IterableStorageDoubleMap;
use sp_runtime::PerU16;
use sp_std::vec;
use substrate_fixed::types::{I32F32, I64F64, I96F32};
use subtensor_runtime_common::{AlphaBalance, MechId, NetUid, NetUidStorageIndex};

impl<T: Config> Pallet<T> {
    /// Dense Yuma epoch (O(n^2) weights/bonds). Prefer [`Self::epoch_mechanism`] in production.
    #[allow(clippy::indexing_slicing)]
    pub fn epoch_dense_mechanism_for_tests(
        netuid: NetUid,
        mecid: MechId,
        rao_emission: AlphaBalance,
    ) -> Vec<(T::AccountId, AlphaBalance, AlphaBalance)> {
        // Calculate netuid storage index
        let netuid_index = Self::get_mechanism_storage_index(netuid, mecid);

        // Get subnetwork size.
        let n: u16 = Self::get_subnetwork_n(netuid);
        log::trace!("n: {n:?}");

        // ======================
        // == Active & updated ==
        // ======================

        // Get current block.
        let current_block: u64 = Self::get_current_block_as_u64();
        log::trace!("current_block: {current_block:?}");

        // Get tempo.
        let tempo: u64 = Self::get_tempo(netuid).into();
        log::trace!("tempo: {tempo:?}");

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
        log::trace!("Inactive: {:?}", inactive.clone());

        // Logical negation of inactive.
        let active: Vec<bool> = inactive.iter().map(|&b| !b).collect();

        // Block at registration vector (block when each neuron was most recently registered).
        let block_at_registration: Vec<u64> = Self::neuron_block_at_registration(netuid);
        log::trace!("Block at registration: {:?}", &block_at_registration);

        // Outdated matrix, outdated_ij=True if i has last updated (weights) after j has last registered.
        let outdated: Vec<Vec<bool>> = last_update
            .iter()
            .map(|updated| {
                block_at_registration
                    .iter()
                    .map(|registered| updated <= registered)
                    .collect()
            })
            .collect();
        log::trace!("Outdated: {:?}", &outdated);

        // Recently registered matrix, recently_ij=True if last_tempo was *before* j was last registered.
        // Mask if: the last tempo block happened *before* the registration block
        // ==> last_tempo <= registered
        // For dynamic tempo - we pick previous-successful-epoch block: `LastMechansimStepBlock + 1`
        let lms = LastMechansimStepBlock::<T>::get(netuid);
        let last_tempo: u64 = if lms == 0 {
            current_block.saturating_sub(tempo)
        } else {
            lms.saturating_add(1)
        };
        let recently_registered: Vec<bool> = block_at_registration
            .iter()
            .map(|registered| last_tempo <= *registered)
            .collect();
        log::trace!("Recently registered: {:?}", &recently_registered);

        // ===========
        // == Stake ==
        // ===========

        let hotkeys: Vec<(u16, T::AccountId)> =
            <Keys<T> as IterableStorageDoubleMap<NetUid, u16, T::AccountId>>::iter_prefix(netuid)
                .collect();
        log::trace!("hotkeys: {:?}", &hotkeys);

        // Access network stake as normalized vector.
        let (total_stake, _alpha_stake, _tao_stake): (Vec<I64F64>, Vec<I64F64>, Vec<I64F64>) =
            Self::get_stake_weights_for_network(netuid);

        // Get the minimum stake required.
        let min_stake = Self::get_stake_threshold();

        // Set stake of validators that doesn't meet the staking threshold to 0 as filter.
        let mut filtered_stake: Vec<I64F64> = total_stake
            .iter()
            .map(|&s| {
                if fixed64_to_u64(s) < min_stake {
                    return I64F64::from(0);
                }
                s
            })
            .collect();
        log::debug!("Filtered stake: {:?}", &filtered_stake);

        inplace_normalize_64(&mut filtered_stake);
        let stake: Vec<I32F32> = vec_fixed64_to_fixed32(filtered_stake);
        log::trace!("S: {:?}", &stake);

        // =======================
        // == Validator permits ==
        // =======================

        // Get validator permits.
        let validator_permits: Vec<bool> = Self::get_validator_permit(netuid);
        log::trace!("validator_permits: {validator_permits:?}");

        // Logical negation of validator_permits.
        let validator_forbids: Vec<bool> = validator_permits.iter().map(|&b| !b).collect();

        // Get max allowed validators.
        let max_allowed_validators: u16 = Self::get_max_allowed_validators(netuid);
        log::trace!("max_allowed_validators: {max_allowed_validators:?}");

        // Get new validator permits.
        let new_validator_permits: Vec<bool> =
            is_topk_nonzero_i32f32(&stake, max_allowed_validators as usize);
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
        log::trace!("S: {:?}", &active_stake);

        // =============
        // == Weights ==
        // =============

        // Get owner uid.
        let owner_uid: Option<u16> = Self::get_owner_uid(netuid);

        // Access network weights row unnormalized.
        let mut weights: Vec<Vec<I32F32>> = Self::get_weights(netuid_index);
        log::trace!("W: {:?}", &weights);

        // Mask weights that are not from permitted validators.
        inplace_mask_rows(&validator_forbids, &mut weights);
        log::trace!("W (permit): {:?}", &weights);

        // Remove self-weight by masking diagonal; keep owner_uid self-weight.
        if let Some(owner_uid) = owner_uid {
            inplace_mask_diag_except_index(&mut weights, owner_uid);
        } else {
            inplace_mask_diag(&mut weights);
        }

        inplace_mask_diag(&mut weights);
        log::trace!("W (permit+diag): {:?}", &weights);

        // Mask outdated weights: remove weights referring to deregistered neurons.
        inplace_mask_matrix(&outdated, &mut weights);
        log::trace!("W (permit+diag+outdate): {:?}", &weights);

        // Normalize remaining weights.
        inplace_row_normalize(&mut weights);
        log::trace!("W (mask+norm): {:?}", &weights);

        // ================================
        // == Consensus, Validator Trust ==
        // ================================

        // Consensus majority ratio, e.g. 51%.
        let kappa: I32F32 = Self::kappa_proportion_as_i32f32(netuid);
        // Calculate consensus as stake-weighted median of weights.
        let consensus: Vec<I32F32> = weighted_median_col(&active_stake, &weights, kappa);
        // Clip weights at majority consensus.
        let mut clipped_weights: Vec<Vec<I32F32>> = weights.clone();
        inplace_col_clip(&mut clipped_weights, &consensus);
        // Calculate validator trust as sum of clipped weights set by validator.
        let validator_trust: Vec<I32F32> = row_sum(&clipped_weights);

        // ====================================
        // == Ranks, Server Trust, Incentive ==
        // ====================================

        // Compute ranks: r_j = SUM(i) w_ij * s_i
        let mut ranks: Vec<I32F32> = matmul(&clipped_weights, &active_stake);

        inplace_normalize(&mut ranks);
        let incentive: Vec<I32F32> = ranks.clone();
        log::trace!("I: {:?}", &incentive);

        // =========================
        // == Bonds and Dividends ==
        // =========================

        // Get validator bonds penalty in [0, 1].
        let bonds_penalty: I32F32 = Self::bonds_penalty_proportion_as_i32f32(netuid);
        // Calculate weights for bonds, apply bonds penalty to weights.
        // bonds_penalty = 0: weights_for_bonds = weights.clone()
        // bonds_penalty = 1: weights_for_bonds = clipped_weights.clone()
        let weights_for_bonds: Vec<Vec<I32F32>> =
            interpolate(&weights, &clipped_weights, bonds_penalty);

        let mut dividends: Vec<I32F32>;
        let mut ema_bonds: Vec<Vec<I32F32>>;
        if Yuma3On::<T>::get(netuid) {
            // Access network bonds.
            let mut bonds: Vec<Vec<I32F32>> = Self::get_bonds_fixed_proportion(netuid_index);
            inplace_mask_cols(&recently_registered, &mut bonds); // mask outdated bonds
            log::trace!("B: {:?}", &bonds);

            // Compute the Exponential Moving Average (EMA) of bonds.
            ema_bonds = Self::compute_bonds(netuid, &weights_for_bonds, &bonds, &consensus);
            log::trace!("emaB: {:?}", &ema_bonds);

            // Normalize EMA bonds.
            let mut ema_bonds_norm = ema_bonds.clone();
            inplace_col_normalize(&mut ema_bonds_norm);
            log::trace!("emaB norm: {:?}", &ema_bonds_norm);

            // # === Dividend Calculation===
            let total_bonds_per_validator: Vec<I32F32> =
                row_sum(&mat_vec_mul(&ema_bonds_norm, &incentive));
            log::trace!(
                "total_bonds_per_validator: {:?}",
                &total_bonds_per_validator
            );

            dividends = vec_mul(&total_bonds_per_validator, &active_stake);
            inplace_normalize(&mut dividends);
            log::trace!("D: {:?}", &dividends);
        } else {
            // original Yuma - liquid alpha disabled
            // Access network bonds.
            let mut bonds: Vec<Vec<I32F32>> = Self::get_bonds(netuid_index);
            // Remove bonds referring to neurons that have registered since last tempo.
            inplace_mask_cols(&recently_registered, &mut bonds); // mask recently registered bonds
            inplace_col_normalize(&mut bonds); // sum_i b_ij = 1
            log::trace!("B: {:?}", &bonds);

            // Compute bonds delta column normalized.
            let mut bonds_delta: Vec<Vec<I32F32>> = row_hadamard(&weights_for_bonds, &active_stake); // ΔB = W◦S
            inplace_col_normalize(&mut bonds_delta); // sum_i b_ij = 1
            log::trace!("ΔB: {:?}", &bonds_delta);

            // Compute the Exponential Moving Average (EMA) of bonds.
            ema_bonds = Self::ema_bonds_normal_dense(&bonds_delta, &bonds, netuid);
            inplace_col_normalize(&mut ema_bonds); // sum_i b_ij = 1
            log::trace!("emaB: {:?}", &ema_bonds);

            // Compute dividends: d_i = SUM(j) b_ij * inc_j
            dividends = matmul_transpose(&ema_bonds, &incentive);
            inplace_normalize(&mut dividends);
            log::trace!("Dividends: {:?}", &dividends);

            // Column max-upscale EMA bonds for storage: max_i w_ij = 1.
            inplace_col_max_upscale(&mut ema_bonds);
        }

        // =================================
        // == Emission and Pruning scores ==
        // =================================

        // Compute emission scores.

        // Compute normalized emission scores. range: I32F32(0, 1)
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

        // Used only to track combined emission in the storage.
        let combined_emission: Vec<I96F32> = normalized_combined_emission
            .iter()
            .map(|ce: &I32F32| I96F32::saturating_from_num(*ce).saturating_mul(float_rao_emission))
            .collect();
        let combined_emission: Vec<AlphaBalance> = combined_emission
            .iter()
            .map(|e: &I96F32| AlphaBalance::from(e.saturating_to_num::<u64>()))
            .collect();

        log::trace!("nSE: {:?}", &normalized_server_emission);
        log::trace!("SE: {:?}", &server_emission);
        log::trace!("nVE: {:?}", &normalized_validator_emission);
        log::trace!("VE: {:?}", &validator_emission);
        log::trace!("nCE: {:?}", &normalized_combined_emission);
        log::trace!("CE: {:?}", &combined_emission);

        // ===================
        // == Value storage ==
        // ===================
        let cloned_emission = combined_emission.clone();
        let cloned_stake_weight: Vec<u16> = stake
            .iter()
            .map(|xi| fixed_proportion_to_u16(*xi))
            .collect::<Vec<u16>>();
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
        StakeWeight::<T>::insert(netuid, cloned_stake_weight.clone());
        Active::<T>::insert(netuid, active.clone());
        Emission::<T>::insert(netuid, cloned_emission);
        // Epoch math stays in raw u16; wrap into PerU16 only at the storage boundary.
        Consensus::<T>::insert(
            netuid,
            cloned_consensus
                .into_iter()
                .map(PerU16::from_parts)
                .collect::<Vec<PerU16>>(),
        );
        Incentive::<T>::insert(
            NetUidStorageIndex::from(netuid),
            cloned_incentive
                .into_iter()
                .map(PerU16::from_parts)
                .collect::<Vec<PerU16>>(),
        );
        Dividends::<T>::insert(
            netuid,
            cloned_dividends
                .into_iter()
                .map(PerU16::from_parts)
                .collect::<Vec<PerU16>>(),
        );
        ValidatorTrust::<T>::insert(
            netuid,
            cloned_validator_trust
                .into_iter()
                .map(PerU16::from_parts)
                .collect::<Vec<PerU16>>(),
        );
        ValidatorPermit::<T>::insert(netuid, new_validator_permits.clone());

        new_validator_permits
            .iter()
            .zip(validator_permits)
            .zip(ema_bonds)
            .enumerate()
            .for_each(|(i, ((new_permit, validator_permit), ema_bond))| {
                // Set bonds only if uid retains validator permit, otherwise clear bonds.
                if *new_permit {
                    let new_bonds_row: Vec<(u16, u16)> = (0..n)
                        .zip(vec_fixed_proportions_to_u16(ema_bond.clone()))
                        .collect();
                    Bonds::<T>::insert(netuid_index, i as u16, new_bonds_row);
                } else if validator_permit {
                    // Only overwrite the intersection.
                    let new_empty_bonds_row: Vec<(u16, u16)> = vec![];
                    Bonds::<T>::insert(netuid_index, i as u16, new_empty_bonds_row);
                }
            });

        hotkeys
            .into_iter()
            .map(|(uid_i, hotkey)| {
                (
                    hotkey,
                    server_emission[uid_i as usize],
                    validator_emission[uid_i as usize],
                )
            })
            .collect()
    }
}
