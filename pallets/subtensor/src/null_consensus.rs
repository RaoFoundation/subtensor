//! Opt-in arithmetic-mean scoring without Yuma, bonds, or validator dividends.
//!
//! UIDs remain u16. Only scores widen; existing subnets and their SCALE layout are
//! unchanged. Rows are normalized independently, so their absolute scale cannot
//! buy voting power. The existing stake/permit rules select eligible scorers.

use crate::*;
use alloc::collections::BTreeMap;
use sp_core::{H256, U256};
use sp_std::vec;
use substrate_fixed::types::I64F64;
use subtensor_runtime_common::MechId;

pub const MAX_NULL_UIDS: u16 = 32_768;
pub const MAX_NULL_VALIDATORS: u16 = 64;
const ROW_UNIT: u128 = 1u128 << 64;

impl<T: Config> Pallet<T> {
    pub fn pow_register_weight(_netuid: NetUid) -> frame_support::weights::Weight {
        use crate::weights::WeightInfo;
        // Account for the larger per-neuron vectors using the existing reference
        // registration bound. No locally invented time/proof measurements.
        // State independent: an earlier utility batch item can enable the mode.
        <T as Config>::WeightInfo::register()
            .saturating_mul(u64::from(MAX_NULL_UIDS) / 4096)
            .saturating_add(T::DbWeight::get().reads(1))
    }

    /// Select the epoch algorithm without rewriting the subnet's hyperparameters.
    /// AdminUtils authorizes the owner/root and applies the normal admin guards.
    pub fn do_set_null_consensus(netuid: NetUid, enabled: bool) -> DispatchResult {
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        ensure!(!netuid.is_root(), Error::<T>::NullConsensusOnRoot);
        if NullConsensus::<T>::get(netuid) == enabled {
            return Ok(());
        }
        if enabled {
            ensure!(
                MechanismCountCurrent::<T>::get(netuid) == MechId::from(1),
                Error::<T>::NullConsensusRequiresSingleMechanism
            );
        } else {
            // Never run Yuma's matrices at the enlarged null-consensus capacity.
            // The owner must explicitly downsize before disabling, without losing
            // miners as a side effect of changing consensus.
            let limit = DefaultMaxAllowedUids::<T>::get();
            ensure!(
                Self::get_subnetwork_n(netuid) <= limit
                    && Self::get_max_allowed_uids(netuid) <= limit,
                Error::<T>::NullConsensusYumaCapacityExceeded
            );
            // Yuma can replace or compact UIDs while this mode is off. Require
            // fresh null scores on return instead of reusing their old mapping.
            NullWeightsResetAt::<T>::insert(netuid, Self::get_current_block_as_u64());
        }
        NullConsensus::<T>::insert(netuid, enabled);
        Ok(())
    }

    /// Validate all work before writing account or subnet state. The hotkey is
    /// part of the seal, and the signer must be the supplied coldkey. Used seals
    /// cannot be replayed after a hotkey swap or on another subnet.
    pub fn do_null_pow_register(
        origin: OriginFor<T>,
        netuid: NetUid,
        block_number: u64,
        nonce: u64,
        work: Vec<u8>,
        hotkey: T::AccountId,
        coldkey: T::AccountId,
    ) -> DispatchResult {
        let signer = ensure_signed(origin)?;
        ensure!(signer == coldkey, Error::<T>::PowSignerColdkeyMismatch);
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        ensure!(
            NullConsensus::<T>::get(netuid),
            Error::<T>::NullConsensusNotEnabled
        );
        ensure!(
            Self::get_network_registration_allowed(netuid),
            Error::<T>::NullConsensusRegistrationDisabled
        );
        ensure!(
            NetworkPowRegistrationAllowed::<T>::get(netuid),
            Error::<T>::NullConsensusPowRegistrationDisabled
        );
        ensure!(
            !Uids::<T>::contains_key(netuid, &hotkey),
            Error::<T>::HotKeyAlreadyRegisteredInSubNet
        );
        ensure!(
            Self::get_subnetwork_n(netuid) < Self::get_max_allowed_uids(netuid).min(MAX_NULL_UIDS),
            Error::<T>::NullConsensusCapacityReached
        );
        ensure!(
            RegistrationsThisBlock::<T>::get(netuid) < MaxRegistrationsPerBlock::<T>::get(netuid),
            Error::<T>::TooManyRegistrationsThisBlock
        );
        let now = Self::get_current_block_as_u64();
        ensure!(
            block_number < now && now.saturating_sub(block_number) < 3,
            Error::<T>::InvalidWorkBlock
        );
        ensure!(work.len() == 32, Error::<T>::PowInvalidSealLength);
        ensure!(
            !UsedWork::<T>::contains_key(&work),
            Error::<T>::PowWorkAlreadyUsed
        );
        let hash = H256::from_slice(&work);
        ensure!(
            hash == Self::create_seal_hash(block_number, nonce, &hotkey),
            Error::<T>::InvalidSeal
        );
        ensure!(
            Self::hash_meets_difficulty(&hash, Self::get_difficulty(netuid).max(U256::one())),
            Error::<T>::InvalidDifficulty
        );
        Self::create_account_if_non_existent(&coldkey, &hotkey)?;
        ensure!(
            Self::coldkey_owns_hotkey(&coldkey, &hotkey),
            Error::<T>::NonAssociatedColdKey
        );
        let uid = Self::get_subnetwork_n(netuid);
        Self::append_neuron(netuid, &hotkey, now);
        UsedWork::<T>::insert(work, now);
        RegistrationsThisBlock::<T>::mutate(netuid, |n| *n = n.saturating_add(1));
        Self::deposit_event(Event::NeuronRegistered(netuid, uid, hotkey));
        Ok(())
    }

    pub fn do_set_weights_v2(
        origin: OriginFor<T>,
        netuid: NetUid,
        dests: Vec<u16>,
        values: Vec<u32>,
        version_key: u64,
    ) -> DispatchResult {
        let hotkey = ensure_signed(origin)?;
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        ensure!(
            NullConsensus::<T>::get(netuid),
            Error::<T>::NullConsensusNotEnabled
        );
        ensure!(
            dests.len() == values.len(),
            Error::<T>::WeightVecNotEqualSize
        );
        let n = Self::get_subnetwork_n(netuid);
        ensure!(
            dests.len() <= usize::from(n.min(MAX_NULL_UIDS)),
            Error::<T>::UidsLengthExceedUidsInSubNet
        );
        let uid = Self::get_uid_for_net_and_hotkey(netuid, &hotkey)?;
        ensure!(
            Self::check_weights_min_stake(&hotkey, netuid),
            Error::<T>::NotEnoughStakeToSetWeights
        );
        ensure!(
            Self::get_owner_uid(netuid) == Some(uid)
                || Self::get_validator_permit_for_uid(netuid, uid),
            Error::<T>::NeuronNoValidatorPermit
        );
        ensure!(
            Self::check_version_key(netuid, version_key),
            Error::<T>::IncorrectWeightVersionKey
        );
        let index = Self::get_mechanism_storage_index(netuid, MechId::MAIN);
        let now = Self::get_current_block_as_u64();
        ensure!(
            now > NullWeightsResetAt::<T>::get(netuid),
            Error::<T>::SettingWeightsTooFast
        );
        ensure!(
            NullLastUpdate::<T>::get(netuid, uid) == 0
                || now.saturating_sub(NullLastUpdate::<T>::get(netuid, uid))
                    >= Self::get_weights_set_rate_limit(netuid),
            Error::<T>::SettingWeightsTooFast
        );
        // Contiguous UIDs let validation avoid a database read per destination.
        let mut seen = vec![false; usize::from(n)];
        for &dest in &dests {
            let cell = seen
                .get_mut(usize::from(dest))
                .ok_or(Error::<T>::UidVecContainInvalidOne)?;
            ensure!(!*cell, Error::<T>::DuplicateUids);
            *cell = true;
        }
        let sum: u64 = values.iter().map(|&v| u64::from(v)).sum();
        ensure!(sum != 0, Error::<T>::NullConsensusWeightsAllZero);
        ensure!(
            values.iter().filter(|&&v| v != 0).count()
                >= usize::from(Self::get_min_allowed_weights(netuid).min(n)),
            Error::<T>::WeightVecLengthIsLow
        );
        let max = u64::from(values.iter().copied().max().unwrap_or_default());
        ensure!(
            max.saturating_mul(u64::from(u16::MAX))
                <= sum.saturating_mul(u64::from(Self::get_max_weight_limit(netuid))),
            Error::<T>::MaxWeightExceeded
        );
        let row: Vec<_> = dests
            .into_iter()
            .zip(values)
            .filter(|(_, v)| *v != 0)
            .collect();
        // Yuma may have changed permits while this mode was disabled. Bound
        // cached rows even before the next null epoch removes revoked scorers.
        if !NullWeights::<T>::contains_key(netuid, uid) {
            let row_limit = usize::from(MAX_NULL_VALIDATORS).saturating_add(1);
            ensure!(
                NullWeights::<T>::iter_key_prefix(netuid)
                    .take(row_limit)
                    .count()
                    < row_limit,
                Error::<T>::NullConsensusValidatorLimitExceeded
            );
        }
        NullWeights::<T>::insert(netuid, uid, row);
        NullLastUpdate::<T>::insert(netuid, uid, now);
        Self::deposit_event(Event::WeightsSet(index, uid));
        Ok(())
    }

    /// Linear score aggregation, with no consensus clipping or bond matrix. Each valid row
    /// contributes one unit, independent of stake or the row's integer scale.
    pub fn null_epoch(
        netuid: NetUid,
        budget: AlphaBalance,
    ) -> BTreeMap<T::AccountId, AlphaBalance> {
        let n = usize::from(Self::get_subnetwork_n(netuid));
        let owner = Self::get_owner_uid(netuid);
        let now = Self::get_current_block_as_u64();
        let cutoff = Self::get_activity_cutoff_blocks(netuid);
        let reset_at = NullWeightsResetAt::<T>::get(netuid);
        let index = Self::get_mechanism_storage_index(netuid, MechId::MAIN);
        let registered = Self::get_block_at_registration(netuid);
        let old_permits = ValidatorPermit::<T>::get(netuid);

        let (stake, _, _) = Self::get_stake_weights_for_network(netuid);
        let threshold = I64F64::saturating_from_num(Self::get_stake_threshold());
        let mut candidates: Vec<_> = stake
            .iter()
            .enumerate()
            .filter(|(_, s)| **s >= threshold && **s > I64F64::from_num(0))
            .map(|(uid, s)| (uid, *s))
            .collect();
        candidates.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let mut permits = vec![false; n];
        for (uid, _) in candidates.into_iter().take(usize::from(
            Self::get_max_allowed_validators(netuid).min(MAX_NULL_VALIDATORS),
        )) {
            if let Some(p) = permits.get_mut(uid) {
                *p = true;
            }
        }
        if let Some(p) = owner.and_then(|u| permits.get_mut(usize::from(u))) {
            *p = true;
        }

        let mut scores = vec![0u128; n];
        // There are at most MAX_NULL_VALIDATORS plus the owner live rows. Expired
        // or revoked rows are removed so historical permits cannot grow the matrix.
        for (uid, row) in NullWeights::<T>::iter_prefix(netuid) {
            let i = usize::from(uid);
            let last = NullLastUpdate::<T>::get(netuid, uid);
            let allowed = old_permits.get(i).copied().unwrap_or(false) || owner == Some(uid);
            if !allowed
                || !permits.get(i).copied().unwrap_or(false)
                || last <= reset_at
                || last.saturating_add(cutoff) < now
                || last <= registered.get(i).copied().unwrap_or_default()
            {
                NullWeights::<T>::remove(netuid, uid);
                NullLastUpdate::<T>::remove(netuid, uid);
                continue;
            }
            let valid = |dest: u16| {
                usize::from(dest) < n
                    && last
                        > registered
                            .get(usize::from(dest))
                            .copied()
                            .unwrap_or(u64::MAX)
                    && (dest != uid || owner == Some(uid))
            };
            let sum: u128 = row
                .iter()
                .filter(|(d, _)| valid(*d))
                .map(|(_, v)| u128::from(*v))
                .sum();
            if sum == 0 {
                continue;
            }
            for (dest, value) in row.into_iter().filter(|(d, _)| valid(*d)) {
                if let Some(score) = scores.get_mut(usize::from(dest)) {
                    *score = score.saturating_add(
                        u128::from(value)
                            .saturating_mul(ROW_UNIT)
                            .checked_div(sum)
                            .unwrap_or_default(),
                    );
                }
            }
        }
        let total: u128 = scores.iter().sum();
        let scale = |score: u128, units: u64| -> u64 {
            if total == 0 {
                return 0;
            }
            U256::from(score)
                .saturating_mul(U256::from(units))
                .checked_div(U256::from(total))
                .unwrap_or_default()
                .low_u64()
        };
        let emission: Vec<AlphaBalance> = scores
            .iter()
            .map(|&s| scale(s, budget.to_u64()).into())
            .collect();
        NullIncentive::<T>::insert(
            netuid,
            scores
                .iter()
                .map(|&s| scale(s, u64::from(u32::MAX)) as u32)
                .collect::<Vec<_>>(),
        );
        ValidatorPermit::<T>::insert(netuid, permits);
        Emission::<T>::insert(netuid, &emission);
        // Legacy incentive displays remain available; these values never drive payment.
        Incentive::<T>::insert(
            index,
            scores
                .iter()
                .map(|&s| sp_runtime::PerU16::from_parts(scale(s, u64::from(u16::MAX)) as u16))
                .collect::<Vec<_>>(),
        );
        Self::deposit_event(Event::IncentiveAlphaEmittedToMiners {
            netuid: index,
            emissions: emission.clone(),
        });
        Keys::<T>::iter_prefix(netuid)
            .filter_map(|(uid, key)| {
                emission
                    .get(usize::from(uid))
                    .copied()
                    .filter(|e| !e.is_zero())
                    .map(|e| (key, e))
            })
            .collect()
    }
}
