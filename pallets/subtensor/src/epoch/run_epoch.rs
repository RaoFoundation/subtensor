use super::*;
use crate::epoch::math::*;
use alloc::collections::{BTreeMap, BTreeSet};
use frame_support::IterableStorageDoubleMap;
use safe_math::*;
use sp_std::collections::btree_map::IntoIter;
use sp_std::vec;
use substrate_fixed::types::{I32F32, I64F64, I96F32};
use subtensor_runtime_common::{AlphaBalance, MechId, NetUid, NetUidStorageIndex};

#[derive(Debug, Default)]
pub struct EpochTerms {
    pub uid: usize,
    pub dividend: u16,
    pub incentive: u16,
    pub validator_emission: AlphaBalance,
    pub server_emission: AlphaBalance,
    pub stake_weight: u16,
    pub active: bool,
    pub emission: AlphaBalance,
    pub consensus: u16,
    pub validator_trust: u16,
    pub new_validator_permit: bool,
    pub bond: Vec<(u16, u16)>,
    pub stake: AlphaBalance,
}

pub struct EpochOutput<T: frame_system::Config>(pub BTreeMap<T::AccountId, EpochTerms>);

impl<T: frame_system::Config> EpochOutput<T> {
    pub fn as_map(&self) -> &BTreeMap<T::AccountId, EpochTerms> {
        &self.0
    }
}

impl<T> IntoIterator for EpochOutput<T>
where
    T: frame_system::Config,
    T::AccountId: Ord,
{
    type Item = (T::AccountId, EpochTerms);
    type IntoIter = IntoIter<T::AccountId, EpochTerms>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

#[macro_export]
macro_rules! extract_from_sorted_terms {
    ($sorted:expr, $field:ident) => {{
        ($sorted)
            .iter()
            .copied()
            .map(|t| t.$field)
            .collect::<sp_std::vec::Vec<_>>()
    }};
}

impl<T: Config> Pallet<T> {
    /// Legacy epoch function interface (TODO: Is only used for tests, remove)
    pub fn epoch(
        netuid: NetUid,
        rao_emission: AlphaBalance,
    ) -> Vec<(T::AccountId, AlphaBalance, AlphaBalance)> {
        // Run mechanism-style epoch
        let output = Self::epoch_mechanism(netuid, MechId::MAIN, rao_emission);

        // Persist values in legacy format
        Self::persist_mechanism_epoch_terms(netuid, MechId::MAIN, output.as_map());
        Self::persist_netuid_epoch_terms(netuid, output.as_map());

        // Remap and return
        output
            .into_iter()
            .map(|(hotkey, terms)| (hotkey, terms.server_emission, terms.validator_emission))
            .collect()
    }

    /// Legacy epoch_dense function interface (TODO: Is only used for tests, remove)
    pub fn epoch_dense(
        netuid: NetUid,
        rao_emission: AlphaBalance,
    ) -> Vec<(T::AccountId, AlphaBalance, AlphaBalance)> {
        Self::epoch_dense_mechanism(netuid, MechId::MAIN, rao_emission)
    }

    /// Persists per-mechanism epoch output in state
    pub fn persist_mechanism_epoch_terms(
        netuid: NetUid,
        mecid: MechId,
        output: &BTreeMap<T::AccountId, EpochTerms>,
    ) {
        let netuid_index = Self::get_mechanism_storage_index(netuid, mecid);
        let mut terms_sorted: sp_std::vec::Vec<&EpochTerms> = output.values().collect();
        terms_sorted.sort_unstable_by_key(|t| t.uid);

        let incentive = extract_from_sorted_terms!(terms_sorted, incentive);
        let bonds: Vec<Vec<(u16, u16)>> = terms_sorted
            .iter()
            .cloned()
            .map(|t| t.bond.clone())
            .collect::<sp_std::vec::Vec<_>>();

        Incentive::<T>::insert(netuid_index, incentive);

        let server_emission = extract_from_sorted_terms!(terms_sorted, server_emission);
        Self::deposit_event(Event::IncentiveAlphaEmittedToMiners {
            netuid: netuid_index,
            emissions: server_emission,
        });

        bonds
            .into_iter()
            .enumerate()
            .for_each(|(uid_usize, bond_vec)| {
                let uid: u16 = uid_usize.try_into().unwrap_or_default();
                Bonds::<T>::insert(netuid_index, uid, bond_vec);
            });
    }

    /// Persists per-netuid epoch output in state
    pub fn persist_netuid_epoch_terms(netuid: NetUid, output: &BTreeMap<T::AccountId, EpochTerms>) {
        let mut terms_sorted: sp_std::vec::Vec<&EpochTerms> = output.values().collect();
        terms_sorted.sort_unstable_by_key(|t| t.uid);

        let active = extract_from_sorted_terms!(terms_sorted, active);
        let emission = extract_from_sorted_terms!(terms_sorted, emission);
        let consensus = extract_from_sorted_terms!(terms_sorted, consensus);
        let dividend = extract_from_sorted_terms!(terms_sorted, dividend);
        let validator_trust = extract_from_sorted_terms!(terms_sorted, validator_trust);
        let new_validator_permit = extract_from_sorted_terms!(terms_sorted, new_validator_permit);
        let stake_weight = extract_from_sorted_terms!(terms_sorted, stake_weight);

        Active::<T>::insert(netuid, active.clone());
        Emission::<T>::insert(netuid, emission);
        Consensus::<T>::insert(netuid, consensus);
        Dividends::<T>::insert(netuid, dividend);
        ValidatorTrust::<T>::insert(netuid, validator_trust);
        ValidatorPermit::<T>::insert(netuid, new_validator_permit);
        StakeWeight::<T>::insert(netuid, stake_weight);
    }

    /// Calculates reward consensus and returns the emissions for uids/hotkeys in a given `netuid`.
    /// (Dense version used only for testing purposes.)
    #[allow(clippy::indexing_slicing)]
    pub fn epoch_dense_mechanism(
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
        let block_at_registration: Vec<u64> = Self::get_block_at_registration(netuid);
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
            is_topk_nonzero(&stake, max_allowed_validators as usize);
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
        let kappa: I32F32 = Self::get_float_kappa(netuid);
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
        let bonds_penalty: I32F32 = Self::get_float_bonds_penalty(netuid);
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
            ema_bonds = Self::compute_ema_bonds_normal(&bonds_delta, &bonds, netuid);
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
        inplace_normalize_using_sum(&mut normalized_server_emission, emission_sum);
        inplace_normalize_using_sum(&mut normalized_validator_emission, emission_sum);
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
        Consensus::<T>::insert(netuid, cloned_consensus);
        Incentive::<T>::insert(NetUidStorageIndex::from(netuid), cloned_incentive);
        Dividends::<T>::insert(netuid, cloned_dividends);
        ValidatorTrust::<T>::insert(netuid, cloned_validator_trust);
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

    /// Calculates reward consensus values, then updates rank, trust, consensus, incentive, dividend, pruning_score, emission and bonds, and
    /// returns the emissions for uids/hotkeys in a given `netuid`.
    ///
    /// # Args:
    ///  * 'netuid': ( u16 ):
    ///     - The network to distribute the emission onto.
    ///
    ///  * 'rao_emission': ( u64 ):
    ///     - The total emission for the epoch.
    ///
    ///  * 'debug' ( bool ):
    ///     - Print debugging outputs.
    ///
    pub fn epoch_mechanism(
        netuid: NetUid,
        mecid: MechId,
        rao_emission: AlphaBalance,
    ) -> EpochOutput<T> {
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
        let block_at_registration: Vec<u64> = Self::get_block_at_registration(netuid);
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
            is_topk_nonzero(&stake, max_allowed_validators as usize);
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
        let kappa: I32F32 = Self::get_float_kappa(netuid);
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
        let bonds_penalty: I32F32 = Self::get_float_bonds_penalty(netuid);
        // Calculate weights for bonds, apply bonds penalty to weights.
        // bonds_penalty = 0: weights_for_bonds = weights.clone()
        // bonds_penalty = 1: weights_for_bonds = clipped_weights.clone()
        let weights_for_bonds: Vec<Vec<(u16, I32F32)>> =
            interpolate_sparse(&weights, &clipped_weights, n, bonds_penalty);

        let mut dividends: Vec<I32F32>;
        let mut ema_bonds: Vec<Vec<(u16, I32F32)>>;
        if Yuma3On::<T>::get(netuid) {
            // Access network bonds.
            let mut bonds = Self::get_bonds_sparse_fixed_proportion(netuid_index);
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
            ema_bonds =
                Self::compute_bonds_sparse(netuid_index, &weights_for_bonds, &bonds, &consensus);
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
            let mut bonds: Vec<Vec<(u16, I32F32)>> = Self::get_bonds_sparse(netuid_index);
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
            ema_bonds = Self::compute_ema_bonds_normal_sparse(&bonds_delta, &bonds, netuid_index);
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
        inplace_normalize_using_sum(&mut normalized_server_emission, emission_sum);
        inplace_normalize_using_sum(&mut normalized_validator_emission, emission_sum);
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

        EpochOutput(terms_map)
    }

    pub fn get_float_rho(netuid: NetUid) -> I32F32 {
        I32F32::saturating_from_num(Self::get_rho(netuid))
    }
    pub fn get_float_kappa(netuid: NetUid) -> I32F32 {
        I32F32::saturating_from_num(Self::get_kappa(netuid))
            .safe_div(I32F32::saturating_from_num(u16::MAX))
    }
    pub fn get_float_bonds_penalty(netuid: NetUid) -> I32F32 {
        I32F32::saturating_from_num(Self::get_bonds_penalty(netuid))
            .safe_div(I32F32::saturating_from_num(u16::MAX))
    }

    pub fn get_block_at_registration(netuid: NetUid) -> Vec<u64> {
        let n = Self::get_subnetwork_n(netuid);
        let block_at_registration: Vec<u64> = (0..n)
            .map(|neuron_uid| {
                if Keys::<T>::contains_key(netuid, neuron_uid) {
                    Self::get_neuron_block_at_registration(netuid, neuron_uid)
                } else {
                    0
                }
            })
            .collect();
        block_at_registration
    }

    /// Output unnormalized sparse weights, input weights are assumed to be row max-upscaled in u16.
    pub fn get_weights_sparse(netuid_index: NetUidStorageIndex) -> Vec<Vec<(u16, I32F32)>> {
        let (netuid, _) = Self::get_netuid_and_subid(netuid_index).unwrap_or_default();
        let n = Self::get_subnetwork_n(netuid) as usize;
        let mut weights: Vec<Vec<(u16, I32F32)>> = vec![vec![]; n];
        for (uid_i, weights_i) in
            Weights::<T>::iter_prefix(netuid_index).filter(|(uid_i, _)| *uid_i < n as u16)
        {
            for (uid_j, weight_ij) in weights_i.iter().filter(|(uid_j, _)| *uid_j < n as u16) {
                if let Some(row) = weights.get_mut(uid_i as usize) {
                    row.push((*uid_j, I32F32::saturating_from_num(*weight_ij)));
                } else {
                    log::error!("math error: uid_i {uid_i:?} is filtered to be less than n");
                }
            }
        }
        weights
    }

    /// Output unnormalized weights in [n, n] matrix, input weights are assumed to be row max-upscaled in u16.
    pub fn get_weights(netuid_index: NetUidStorageIndex) -> Vec<Vec<I32F32>> {
        let (netuid, _) = Self::get_netuid_and_subid(netuid_index).unwrap_or_default();
        let n = Self::get_subnetwork_n(netuid) as usize;
        let mut weights: Vec<Vec<I32F32>> = vec![vec![I32F32::saturating_from_num(0.0); n]; n];
        for (uid_i, weights_vec) in
            Weights::<T>::iter_prefix(netuid_index).filter(|(uid_i, _)| *uid_i < n as u16)
        {
            for (uid_j, weight_ij) in weights_vec
                .into_iter()
                .filter(|(uid_j, _)| *uid_j < n as u16)
            {
                if let Some(cell) = weights
                    .get_mut(uid_i as usize)
                    .and_then(|row| row.get_mut(uid_j as usize))
                {
                    *cell = I32F32::saturating_from_num(weight_ij);
                }
            }
        }
        weights
    }

    /// Output unnormalized sparse bonds, input bonds are assumed to be column max-upscaled in u16.
    pub fn get_bonds_sparse(netuid_index: NetUidStorageIndex) -> Vec<Vec<(u16, I32F32)>> {
        let (netuid, _) = Self::get_netuid_and_subid(netuid_index).unwrap_or_default();
        let n = Self::get_subnetwork_n(netuid) as usize;
        let mut bonds: Vec<Vec<(u16, I32F32)>> = vec![vec![]; n];
        for (uid_i, bonds_vec) in
            Bonds::<T>::iter_prefix(netuid_index).filter(|(uid_i, _)| *uid_i < n as u16)
        {
            for (uid_j, bonds_ij) in bonds_vec {
                if let Some(row) = bonds.get_mut(uid_i as usize) {
                    row.push((uid_j, u16_to_fixed(bonds_ij)));
                } else {
                    // If the index is unexpectedly out of bounds, skip and log math error
                    log::error!(
                        "math error: bonds row index out of bounds (uid_i={uid_i}, n={n}, netuid_index={netuid_index})",
                    );
                }
            }
        }

        bonds
    }

    /// Output unnormalized bonds in [n, n] matrix, input bonds are assumed to be column max-upscaled in u16.
    pub fn get_bonds(netuid_index: NetUidStorageIndex) -> Vec<Vec<I32F32>> {
        let (netuid, _) = Self::get_netuid_and_subid(netuid_index).unwrap_or_default();
        let n: usize = Self::get_subnetwork_n(netuid) as usize;
        let mut bonds: Vec<Vec<I32F32>> = vec![vec![I32F32::saturating_from_num(0.0); n]; n];
        for (uid_i, bonds_vec) in
            Bonds::<T>::iter_prefix(netuid_index).filter(|(uid_i, _)| *uid_i < n as u16)
        {
            for (uid_j, bonds_ij) in bonds_vec.into_iter().filter(|(uid_j, _)| *uid_j < n as u16) {
                if let Some(row) = bonds.get_mut(uid_i as usize) {
                    if let Some(cell) = row.get_mut(uid_j as usize) {
                        *cell = u16_to_fixed(bonds_ij);
                    } else {
                        log::error!(
                            "math error: uid_j index out of bounds (uid_i={uid_i}, uid_j={uid_j}, n={n}, netuid_index={netuid_index})"
                        );
                    }
                } else {
                    log::error!(
                        "math error: uid_i row index out of bounds (uid_i={uid_i}, n={n}, netuid_index={netuid_index})"
                    );
                }
            }
        }

        bonds
    }

    pub fn get_bonds_fixed_proportion(netuid: NetUidStorageIndex) -> Vec<Vec<I32F32>> {
        let mut bonds = Self::get_bonds(netuid);
        bonds.iter_mut().for_each(|bonds_row| {
            bonds_row
                .iter_mut()
                .for_each(|bond| *bond = fixed_to_fixed_u16_proportion(*bond));
        });
        bonds
    }

    pub fn get_bonds_sparse_fixed_proportion(
        netuid: NetUidStorageIndex,
    ) -> Vec<Vec<(u16, I32F32)>> {
        let mut bonds = Self::get_bonds_sparse(netuid);
        bonds.iter_mut().for_each(|bonds_row| {
            bonds_row
                .iter_mut()
                .for_each(|(_, bond)| *bond = fixed_to_fixed_u16_proportion(*bond));
        });
        bonds
    }

    /// Compute the Exponential Moving Average (EMA) of bonds using a normal alpha value for a sparse matrix.
    ///
    /// # Args:
    /// * `bonds_delta` - A vector of bond deltas.
    /// * `bonds` - A vector of bonds.
    /// * `netuid` - The network ID.
    ///
    /// # Returns:
    /// A vector of EMA bonds.
    pub fn compute_ema_bonds_normal_sparse(
        bonds_delta: &[Vec<(u16, I32F32)>],
        bonds: &[Vec<(u16, I32F32)>],
        netuid_index: NetUidStorageIndex,
    ) -> Vec<Vec<(u16, I32F32)>> {
        let (netuid, _) = Self::get_netuid_and_subid(netuid_index).unwrap_or_default();

        // Retrieve the bonds moving average for the given network ID and scale it down.
        let bonds_moving_average: I64F64 =
            I64F64::saturating_from_num(Self::get_bonds_moving_average(netuid))
                .safe_div(I64F64::saturating_from_num(1_000_000));

        // Calculate the alpha value for the EMA calculation.
        // Alpha is derived by subtracting the scaled bonds moving average from 1.
        let alpha: I32F32 = I32F32::saturating_from_num(1)
            .saturating_sub(I32F32::saturating_from_num(bonds_moving_average));

        // Compute the Exponential Moving Average (EMA) of bonds using the calculated alpha value.
        let ema_bonds = mat_ema_sparse(bonds_delta, bonds, alpha);

        // Log the computed EMA bonds for debugging purposes.
        log::trace!("Exponential Moving Average Bonds Normal: {ema_bonds:?}");

        // Return the computed EMA bonds.
        ema_bonds
    }

    /// Compute the Exponential Moving Average (EMA) of bonds using a normal alpha value.
    ///
    /// # Args:
    /// * `bonds_delta` - A vector of bond deltas.
    /// * `bonds` - A vector of bonds.
    /// * `netuid` - The network ID.
    ///
    /// # Returns:
    /// A vector of EMA bonds.
    pub fn compute_ema_bonds_normal(
        bonds_delta: &[Vec<I32F32>],
        bonds: &[Vec<I32F32>],
        netuid: NetUid,
    ) -> Vec<Vec<I32F32>> {
        // Retrieve the bonds moving average for the given network ID and scale it down.
        let bonds_moving_average: I64F64 =
            I64F64::saturating_from_num(Self::get_bonds_moving_average(netuid))
                .safe_div(I64F64::saturating_from_num(1_000_000));

        // Calculate the alpha value for the EMA calculation.
        // Alpha is derived by subtracting the scaled bonds moving average from 1.
        let alpha: I32F32 = I32F32::saturating_from_num(1)
            .saturating_sub(I32F32::saturating_from_num(bonds_moving_average));

        // Compute the Exponential Moving Average (EMA) of bonds using the calculated alpha value.
        let ema_bonds = mat_ema(bonds_delta, bonds, alpha);

        // Log the computed EMA bonds for debugging purposes.
        log::trace!("Exponential Moving Average Bonds Normal: {ema_bonds:?}");

        // Return the computed EMA bonds.
        ema_bonds
    }

    /// Compute the Exponential Moving Average (EMA) of bonds based on the Liquid Alpha setting
    ///
    /// # Args:
    /// * `netuid` - The network ID.
    /// * `weights` - A vector of weights.
    /// * `bonds` - A vector of bonds.
    /// * `consensus` - A vector of consensus values.
    /// * `active_stake` - A vector of active stake values.
    ///
    /// # Returns:
    /// A vector of EMA bonds.
    pub fn compute_bonds(
        netuid: NetUid,
        weights: &[Vec<I32F32>], // weights_for_bonds
        bonds: &[Vec<I32F32>],
        consensus: &[I32F32],
    ) -> Vec<Vec<I32F32>> {
        // Check if Liquid Alpha is enabled, consensus is not empty, and contains non-zero values.
        if LiquidAlphaOn::<T>::get(netuid)
            && !consensus.is_empty()
            && consensus
                .iter()
                .any(|&c| c != I32F32::saturating_from_num(0))
        {
            // Liquid Alpha is enabled, compute the liquid alphas matrix.
            let alphas: Vec<Vec<I32F32>> =
                Self::compute_liquid_alpha_values(netuid, weights, bonds, consensus);
            log::trace!("alphas: {:?}", &alphas);

            // Compute the Exponential Moving Average (EMA) of bonds using the provided clamped alpha values.
            mat_ema_alpha(weights, bonds, &alphas)
        } else {
            // Liquid Alpha is disabled, compute the liquid alpha value.
            let alpha: I32F32 = Self::compute_disabled_liquid_alpha(netuid);

            // Compute the Exponential Moving Average (EMA) of bonds using the calculated alpha value.
            mat_ema(weights, bonds, alpha)
        }
    }

    /// Compute the Exponential Moving Average (EMA) of bonds based on the Liquid Alpha setting for a sparse matrix.
    ///
    /// # Args:
    /// * `netuid` - The network ID.
    /// * `weights` - A vector of weights.
    /// * `bonds` - A vector of bonds.
    /// * `consensus` - A vector of consensus values.
    /// * `active_stake` - A vector of active stake values.
    ///
    /// # Returns:
    /// A vector of EMA bonds.
    pub fn compute_bonds_sparse(
        netuid_index: NetUidStorageIndex,
        weights: &[Vec<(u16, I32F32)>],
        bonds: &[Vec<(u16, I32F32)>],
        consensus: &[I32F32],
    ) -> Vec<Vec<(u16, I32F32)>> {
        let (netuid, _) = Self::get_netuid_and_subid(netuid_index).unwrap_or_default();

        // Check if Liquid Alpha is enabled, consensus is not empty, and contains non-zero values.
        if LiquidAlphaOn::<T>::get(netuid)
            && !consensus.is_empty()
            && consensus
                .iter()
                .any(|&c| c != I32F32::saturating_from_num(0))
        {
            // Liquid Alpha is enabled, compute the liquid alphas matrix.
            let alphas: Vec<Vec<I32F32>> =
                Self::compute_liquid_alpha_values_sparse(netuid, weights, bonds, consensus);
            log::trace!("alphas: {:?}", &alphas);

            // Compute the Exponential Moving Average (EMA) of bonds using the provided clamped alpha values.
            mat_ema_alpha_sparse(weights, bonds, &alphas)
        } else {
            // Liquid Alpha is disabled, compute the liquid alpha value.
            let alpha: I32F32 = Self::compute_disabled_liquid_alpha(netuid);

            // Compute the Exponential Moving Average (EMA) of bonds using the calculated alpha value.
            mat_ema_sparse(weights, bonds, alpha)
        }
    }

    /// Compute liquid alphas matrix
    /// There is a separate alpha param for each validator-miner binding
    ///
    /// # Args:
    /// * `netuid` - The network ID.
    /// * `weights` - A vector of weights.
    /// * `bonds` - A vector of bonds.
    /// * `consensus` - A vector of consensus values.
    ///
    /// # Returns:
    /// A matrix of alphas
    pub fn compute_liquid_alpha_values(
        netuid: NetUid,
        weights: &[Vec<I32F32>], // current epoch weights
        bonds: &[Vec<I32F32>],   // previous epoch bonds
        consensus: &[I32F32],    // previous epoch consensus weights
    ) -> Vec<Vec<I32F32>> {
        let mut alphas = Vec::new();

        if weights.len() != bonds.len() {
            log::error!(
                "math error: compute_liquid_alpha_values: weights and bonds have different lengths: {:?} != {:?}",
                weights.len(),
                bonds.len()
            );
            return alphas;
        }

        // Get the high and low alpha values for the network.
        let alpha_sigmoid_steepness: I32F32 = Self::get_alpha_sigmoid_steepness(netuid);
        let (alpha_low, alpha_high): (I32F32, I32F32) = Self::get_alpha_values_32(netuid);

        for (w_row, b_row) in weights.iter().zip(bonds.iter()) {
            let mut row_alphas = Vec::new();

            for ((weight, bond), consensus_val) in
                w_row.iter().zip(b_row.iter()).zip(consensus.iter())
            {
                let alpha = Self::alpha_sigmoid(
                    *consensus_val,
                    *weight,
                    *bond,
                    alpha_low,
                    alpha_high,
                    alpha_sigmoid_steepness,
                );
                row_alphas.push(alpha);
            }
            alphas.push(row_alphas);
        }
        alphas
    }

    /// Compute liquid alphas sparse matrix
    /// There is a separate alpha param for each validator-miner binding
    ///
    /// # Args:
    /// * `netuid` - The network ID.
    /// * `weights` - A vector of weights.
    /// * `bonds` - A vector of bonds.
    /// * `consensus` - A vector of consensus values.
    ///
    /// # Returns:
    /// A dense matrix of alphas
    pub fn compute_liquid_alpha_values_sparse(
        netuid: NetUid,
        weights: &[Vec<(u16, I32F32)>], // current epoch weights
        bonds: &[Vec<(u16, I32F32)>],   // previous epoch bonds
        consensus: &[I32F32],           // previous epoch consensus weights
    ) -> Vec<Vec<I32F32>> {
        let mut alphas = Vec::with_capacity(consensus.len());

        if weights.len() != bonds.len() {
            log::error!(
                "math error: compute_liquid_alpha_values: weights and bonds have different lengths: {:?} != {:?}",
                weights.len(),
                bonds.len()
            );
            return alphas;
        }

        let alpha_sigmoid_steepness: I32F32 = Self::get_alpha_sigmoid_steepness(netuid);
        let (alpha_low, alpha_high): (I32F32, I32F32) = Self::get_alpha_values_32(netuid);

        let zero = I32F32::from_num(0.0);

        // iterate over rows
        for (w_row, b_row) in weights.iter().zip(bonds.iter()) {
            let mut row_alphas = Vec::with_capacity(w_row.len());
            let mut w_iter = w_row.iter().peekable();
            let mut b_iter = b_row.iter().peekable();
            for (j_pos, consensus_val) in consensus.iter().enumerate() {
                let j = j_pos as u16;

                let mut weight = zero;
                while let Some(&&(i, val)) = w_iter.peek() {
                    if i < j {
                        w_iter.next();
                    } else {
                        if i == j {
                            weight = val;
                        }
                        break;
                    }
                }

                let mut bond = zero;
                while let Some(&&(i, val)) = b_iter.peek() {
                    if i < j {
                        b_iter.next();
                    } else {
                        if i == j {
                            bond = val;
                        }
                        break;
                    }
                }

                let alpha = Self::alpha_sigmoid(
                    *consensus_val,
                    weight,
                    bond,
                    alpha_low,
                    alpha_high,
                    alpha_sigmoid_steepness,
                );
                row_alphas.push(alpha);
            }
            alphas.push(row_alphas);
        }
        alphas
    }

    /// Helper function to compute the alpha value using a sigmoid function.
    pub fn alpha_sigmoid(
        consensus: I32F32,
        weight: I32F32,
        bond: I32F32,
        alpha_low: I32F32,
        alpha_high: I32F32,
        alpha_sigmoid_steepness: I32F32,
    ) -> I32F32 {
        let zero = I32F32::from_num(0.0);
        let one = I32F32::from_num(1.0);

        let diff_buy = clamp_value(weight.saturating_sub(consensus), zero, one);
        let diff_sell = clamp_value(bond.saturating_sub(weight), zero, one);
        let combined_diff = if weight >= bond { diff_buy } else { diff_sell };

        // sigmoid = 1. / (1. + e^(-steepness * (combined_diff - 0.5)))
        let sigmoid = one.saturating_div(
            one.saturating_add(exp_safe(
                alpha_sigmoid_steepness
                    .saturating_div(I32F32::from_num(-100))
                    .saturating_mul(combined_diff.saturating_sub(I32F32::from_num(0.5))),
            )),
        );
        let alpha =
            alpha_low.saturating_add(sigmoid.saturating_mul(alpha_high.saturating_sub(alpha_low)));

        clamp_value(alpha, alpha_low, alpha_high)
    }

    pub fn compute_disabled_liquid_alpha(netuid: NetUid) -> I32F32 {
        // Retrieve the bonds moving average for the given network ID and scale it down.
        let bonds_moving_average: I64F64 = I64F64::from_num(Self::get_bonds_moving_average(netuid))
            .saturating_div(I64F64::from_num(1_000_000));

        // Calculate the alpha value for the EMA calculation.
        // Alpha is derived by subtracting the scaled bonds moving average from 1.
        let alpha: I32F32 =
            I32F32::from_num(1).saturating_sub(I32F32::from_num(bonds_moving_average));
        alpha
    }

    pub fn do_set_alpha_values(
        origin: OriginFor<T>,
        netuid: NetUid,
        alpha_low: u16,
        alpha_high: u16,
    ) -> Result<(), DispatchError> {
        Self::ensure_subnet_owner_or_root(origin, netuid)?;

        ensure!(
            Self::get_liquid_alpha_enabled(netuid),
            Error::<T>::LiquidAlphaDisabled
        );

        let max_u16: u32 = u16::MAX as u32; // 65535
        let min_alpha_low: u16 = (max_u16.safe_div(40)) as u16; // 1638
        let min_alpha_high: u16 = min_alpha_low;

        ensure!(alpha_high >= min_alpha_high, Error::<T>::AlphaHighTooLow);

        ensure!(
            alpha_low >= min_alpha_low && alpha_low <= alpha_high,
            Error::<T>::AlphaLowOutOfRange
        );

        AlphaValues::<T>::insert(netuid, (alpha_low, alpha_high));

        log::debug!(
            "AlphaValuesSet( netuid: {netuid:?}, AlphaLow: {alpha_low:?}, AlphaHigh: {alpha_high:?} ) ",
        );
        Ok(())
    }

    pub fn do_reset_bonds(
        netuid_index: NetUidStorageIndex,
        account_id: &T::AccountId,
    ) -> Result<(), DispatchError> {
        let (netuid, _) = Self::get_netuid_and_subid(netuid_index).unwrap_or_default();

        // check bonds reset enabled for this subnet
        let bonds_reset_enabled: bool = Self::get_bonds_reset(netuid);
        if !bonds_reset_enabled {
            return Ok(());
        }

        if let Ok(uid) = Self::get_uid_for_net_and_hotkey(netuid, account_id) {
            for (i, bonds_vec) in Bonds::<T>::iter_prefix(netuid_index) {
                Bonds::<T>::insert(
                    netuid_index,
                    i,
                    bonds_vec
                        .clone()
                        .iter()
                        .filter(|(j, _)| *j != uid)
                        .collect::<Vec<&(u16, u16)>>(),
                );
            }
            log::debug!("Reset bonds for {account_id:?}, netuid {netuid:?}");
        } else {
            log::warn!(
                "Uid not found for {account_id:?}, netuid {netuid:?} - skipping bonds reset"
            );
        }

        Ok(())
    }

    /// This function ensures major assumptions made by epoch function:
    ///   1. Keys map has no duplicate hotkeys
    ///
    pub fn is_epoch_input_state_consistent(netuid: NetUid) -> bool {
        // Check if Keys map has duplicate hotkeys or uids
        let mut hotkey_set: BTreeSet<T::AccountId> = BTreeSet::new();
        // `iter_prefix` over a double map yields (uid, value) for the given first key.
        for (_uid, hotkey) in Keys::<T>::iter_prefix(netuid) {
            if !hotkey_set.insert(hotkey) {
                log::error!("Duplicate hotkeys detected for netuid {netuid}");
                return false;
            }
        }
        true
    }
}
