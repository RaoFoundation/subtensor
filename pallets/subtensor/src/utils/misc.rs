use super::*;
use crate::Error;
use crate::system::{ensure_signed, ensure_signed_or_root, pallet_prelude::BlockNumberFor};
use safe_math::*;
use sp_core::Get;
use sp_core::U256;
use sp_runtime::Saturating;
use substrate_fixed::types::{I32F32, I64F64, U64F64, U96F32};
use subtensor_runtime_common::{AlphaBalance, NetUid, NetUidStorageIndex, TaoBalance};

impl<T: Config> Pallet<T> {
    pub fn ensure_subnet_owner_or_root(
        o: OriginFor<T>,
        netuid: NetUid,
    ) -> Result<Option<T::AccountId>, DispatchError> {
        let coldkey = ensure_signed_or_root(o);
        match coldkey {
            Ok(Some(who)) if SubnetOwner::<T>::get(netuid) == who => Ok(Some(who)),
            Ok(Some(_)) => Err(DispatchError::BadOrigin),
            Ok(None) => Ok(None),
            Err(x) => Err(x.into()),
        }
    }

    pub fn ensure_subnet_owner(
        o: OriginFor<T>,
        netuid: NetUid,
    ) -> Result<T::AccountId, DispatchError> {
        let coldkey = ensure_signed(o);
        match coldkey {
            Ok(who) if SubnetOwner::<T>::get(netuid) == who => Ok(who),
            Ok(_) => Err(DispatchError::BadOrigin),
            Err(x) => Err(x.into()),
        }
    }

    /// Ensure owner-or-root with a set of TransactionType rate checks (owner only).
    pub fn ensure_sn_owner_or_root_with_limits(
        o: OriginFor<T>,
        netuid: NetUid,
        limits: &[crate::utils::rate_limiting::TransactionType],
    ) -> Result<Option<T::AccountId>, DispatchError> {
        let maybe_who = Self::ensure_subnet_owner_or_root(o, netuid)?;
        if let Some(who) = maybe_who.as_ref() {
            for tx in limits.iter() {
                ensure!(
                    tx.passes_rate_limit_on_subnet::<T>(who, netuid),
                    Error::<T>::TxRateLimitExceeded
                );
            }
        }
        Ok(maybe_who)
    }

    /// Returns true if the current block is within the terminal freeze window of the tempo for the
    /// given subnet. During this window, admin ops are prohibited to avoid interference with
    /// validator weight submissions. Engages immediately on a pending manual trigger (so the trigger
    /// arms the freeze for the entire countdown to `PendingEpochAt`).
    pub fn is_in_admin_freeze_window(netuid: NetUid, current_block: u64) -> bool {
        let tempo = Self::get_tempo(netuid);
        if tempo == 0 {
            return false;
        }
        let pending = PendingEpochAt::<T>::get(netuid);
        if pending > 0 && pending > current_block {
            return true;
        }
        let remaining = Self::blocks_until_next_auto_epoch(netuid, tempo, current_block);
        let window = AdminFreezeWindow::<T>::get() as u64;
        remaining < window
    }

    /// Ensures the admin freeze window is not currently active for the given subnet.
    pub fn ensure_admin_window_open(netuid: NetUid) -> Result<(), DispatchError> {
        let now = Self::get_current_block_as_u64();
        ensure!(
            !Self::is_in_admin_freeze_window(netuid, now),
            Error::<T>::AdminActionProhibitedDuringWeightsWindow
        );
        Ok(())
    }

    pub fn set_admin_freeze_window(window: u16) {
        AdminFreezeWindow::<T>::set(window);
        Self::deposit_event(Event::AdminFreezeWindowSet(window));
    }

    pub fn set_owner_hyperparam_rate_limit(epochs: u16) {
        OwnerHyperparamRateLimit::<T>::set(epochs);
        Self::deposit_event(Event::OwnerHyperparamRateLimitSet(epochs));
    }

    /// If owner is `Some`, record last-blocks for the provided `TransactionType`s.
    pub fn record_owner_rl(
        maybe_owner: Option<<T as frame_system::Config>::AccountId>,
        netuid: NetUid,
        txs: &[TransactionType],
    ) {
        if let Some(who) = maybe_owner {
            let now = Self::get_current_block_as_u64();
            for tx in txs {
                tx.set_last_block_on_subnet::<T>(&who, netuid, now);
            }
        }
    }

    // ========================
    // ==== Global Setters ====
    // ========================
    /// Unchecked tempo write used by tests, precompiles, and internal helpers.
    /// Does NOT reset `LastEpochBlock` — that is the responsibility of the owner-side
    /// `set_tempo` extrinsic and `sudo_set_tempo` (root), both of which perform the cycle
    /// reset explicitly.
    pub fn set_tempo_unchecked(netuid: NetUid, tempo: u16) {
        Tempo::<T>::insert(netuid, tempo);
        Self::deposit_event(Event::TempoSet(netuid, tempo));
    }

    /// Sets `Tempo` and resets the state-based scheduler anchor `LastEpochBlock`
    /// to the current block
    pub fn apply_tempo_with_cycle_reset(netuid: NetUid, tempo: u16) {
        Self::set_tempo_unchecked(netuid, tempo);
        let now = Self::get_current_block_as_u64();
        LastEpochBlock::<T>::insert(netuid, now);
    }

    pub fn set_last_adjustment_block(netuid: NetUid, last_adjustment_block: u64) {
        LastAdjustmentBlock::<T>::insert(netuid, last_adjustment_block);
    }
    pub fn set_blocks_since_last_step(netuid: NetUid, blocks_since_last_step: u64) {
        BlocksSinceLastStep::<T>::insert(netuid, blocks_since_last_step);
    }
    pub fn set_registrations_this_block(netuid: NetUid, registrations_this_block: u16) {
        RegistrationsThisBlock::<T>::insert(netuid, registrations_this_block);
    }
    pub fn set_last_mechanism_step_block(netuid: NetUid, last_mechanism_step_block: u64) {
        LastMechansimStepBlock::<T>::insert(netuid, last_mechanism_step_block);
    }
    pub fn set_registrations_this_interval(netuid: NetUid, registrations_this_interval: u16) {
        RegistrationsThisInterval::<T>::insert(netuid, registrations_this_interval);
    }
    pub fn set_pow_registrations_this_interval(
        netuid: NetUid,
        pow_registrations_this_interval: u16,
    ) {
        POWRegistrationsThisInterval::<T>::insert(netuid, pow_registrations_this_interval);
    }
    pub fn set_burn_registrations_this_interval(
        netuid: NetUid,
        burn_registrations_this_interval: u16,
    ) {
        BurnRegistrationsThisInterval::<T>::insert(netuid, burn_registrations_this_interval);
    }

    // ========================
    // ==== Global Getters ====
    // ========================
    pub fn get_current_block_as_u64() -> u64 {
        TryInto::try_into(<frame_system::Pallet<T>>::block_number())
            .ok()
            .expect("blockchain will not exceed 2^64 blocks; QED.")
    }

    // ==============================
    // ==== YumaConsensus params ====
    // ==============================
    /// Deprecated: Rank is no longer computed during epoch. Always returns empty.
    pub fn get_rank(_netuid: NetUid) -> Vec<u16> {
        Vec::new()
    }
    /// Deprecated: Trust is no longer computed during epoch. Always returns empty.
    pub fn get_trust(_netuid: NetUid) -> Vec<u16> {
        Vec::new()
    }
    pub fn get_active(netuid: NetUid) -> Vec<bool> {
        Active::<T>::get(netuid)
    }
    pub fn get_emission(netuid: NetUid) -> Vec<AlphaBalance> {
        Emission::<T>::get(netuid)
    }
    pub fn get_consensus(netuid: NetUid) -> Vec<u16> {
        Consensus::<T>::get(netuid)
    }
    pub fn get_incentive(netuid: NetUidStorageIndex) -> Vec<u16> {
        Incentive::<T>::get(netuid)
    }
    pub fn get_dividends(netuid: NetUid) -> Vec<u16> {
        Dividends::<T>::get(netuid)
    }
    /// Fetch LastUpdate for `netuid` and ensure its length is at least `get_subnetwork_n(netuid)`,
    /// padding with zeros if needed. Returns the (possibly padded) vector.
    pub fn get_last_update(netuid_index: NetUidStorageIndex) -> Vec<u64> {
        let netuid = Self::get_netuid(netuid_index);
        let target_len = Self::get_subnetwork_n(netuid) as usize;
        let mut v = LastUpdate::<T>::get(netuid_index);
        if v.len() < target_len {
            v.resize(target_len, 0);
        }
        v
    }
    /// Deprecated: PruningScores is no longer computed during epoch. Always returns empty.
    pub fn get_pruning_score(_netuid: NetUid) -> Vec<u16> {
        Vec::new()
    }
    pub fn get_validator_trust(netuid: NetUid) -> Vec<u16> {
        ValidatorTrust::<T>::get(netuid)
    }
    pub fn get_validator_permit(netuid: NetUid) -> Vec<bool> {
        ValidatorPermit::<T>::get(netuid)
    }

    // ==================================
    // ==== YumaConsensus UID params ====
    // ==================================
    pub fn set_last_update_for_uid(netuid: NetUidStorageIndex, uid: u16, last_update: u64) {
        let mut updated_last_update_vec = Self::get_last_update(netuid);
        let Some(updated_last_update) = updated_last_update_vec.get_mut(uid as usize) else {
            return;
        };
        *updated_last_update = last_update;
        LastUpdate::<T>::insert(netuid, updated_last_update_vec);
    }
    pub fn set_active_for_uid(netuid: NetUid, uid: u16, active: bool) {
        let mut updated_active_vec = Self::get_active(netuid);
        let Some(updated_active) = updated_active_vec.get_mut(uid as usize) else {
            return;
        };
        *updated_active = active;
        Active::<T>::insert(netuid, updated_active_vec);
    }
    pub fn set_validator_permit_for_uid(netuid: NetUid, uid: u16, validator_permit: bool) {
        let mut updated_validator_permits = Self::get_validator_permit(netuid);
        let Some(updated_validator_permit) = updated_validator_permits.get_mut(uid as usize) else {
            return;
        };
        *updated_validator_permit = validator_permit;
        ValidatorPermit::<T>::insert(netuid, updated_validator_permits);
    }
    pub fn set_stake_threshold(min_stake: u64) {
        StakeThreshold::<T>::put(min_stake);
        Self::deposit_event(Event::StakeThresholdSet(min_stake));
    }

    /// Deprecated: Rank is no longer computed. Always returns 0.
    pub fn get_rank_for_uid(_netuid: NetUid, _uid: u16) -> u16 {
        0
    }
    /// Deprecated: Trust is no longer computed. Always returns 0.
    pub fn get_trust_for_uid(_netuid: NetUid, _uid: u16) -> u16 {
        0
    }
    pub fn get_emission_for_uid(netuid: NetUid, uid: u16) -> AlphaBalance {
        let vec = Emission::<T>::get(netuid);
        vec.get(uid as usize).copied().unwrap_or_default()
    }
    pub fn get_active_for_uid(netuid: NetUid, uid: u16) -> bool {
        let vec = Active::<T>::get(netuid);
        vec.get(uid as usize).copied().unwrap_or(false)
    }
    pub fn get_consensus_for_uid(netuid: NetUid, uid: u16) -> u16 {
        let vec = Consensus::<T>::get(netuid);
        vec.get(uid as usize).copied().unwrap_or(0)
    }
    pub fn get_incentive_for_uid(netuid: NetUidStorageIndex, uid: u16) -> u16 {
        let vec = Incentive::<T>::get(netuid);
        vec.get(uid as usize).copied().unwrap_or(0)
    }
    pub fn get_dividends_for_uid(netuid: NetUid, uid: u16) -> u16 {
        let vec = Dividends::<T>::get(netuid);
        vec.get(uid as usize).copied().unwrap_or(0)
    }
    pub fn get_last_update_for_uid(netuid: NetUidStorageIndex, uid: u16) -> u64 {
        let vec = LastUpdate::<T>::get(netuid);
        vec.get(uid as usize).copied().unwrap_or(0)
    }
    /// Deprecated: PruningScores is no longer computed. Always returns u16::MAX.
    pub fn get_pruning_score_for_uid(_netuid: NetUid, _uid: u16) -> u16 {
        u16::MAX
    }
    pub fn get_validator_trust_for_uid(netuid: NetUid, uid: u16) -> u16 {
        let vec = ValidatorTrust::<T>::get(netuid);
        vec.get(uid as usize).copied().unwrap_or(0)
    }
    pub fn get_validator_permit_for_uid(netuid: NetUid, uid: u16) -> bool {
        let vec = ValidatorPermit::<T>::get(netuid);
        vec.get(uid as usize).copied().unwrap_or(false)
    }
    pub fn get_stake_threshold() -> u64 {
        StakeThreshold::<T>::get()
    }

    // ============================
    // ==== Subnetwork Getters ====
    // ============================
    pub fn get_tempo(netuid: NetUid) -> u16 {
        Tempo::<T>::get(netuid)
    }
    pub fn get_last_adjustment_block(netuid: NetUid) -> u64 {
        LastAdjustmentBlock::<T>::get(netuid)
    }
    pub fn get_blocks_since_last_step(netuid: NetUid) -> u64 {
        BlocksSinceLastStep::<T>::get(netuid)
    }
    pub fn get_difficulty(netuid: NetUid) -> U256 {
        U256::from(Self::get_difficulty_as_u64(netuid))
    }
    pub fn get_registrations_this_block(netuid: NetUid) -> u16 {
        RegistrationsThisBlock::<T>::get(netuid)
    }
    pub fn get_last_mechanism_step_block(netuid: NetUid) -> u64 {
        LastMechansimStepBlock::<T>::get(netuid)
    }
    pub fn get_registrations_this_interval(netuid: NetUid) -> u16 {
        RegistrationsThisInterval::<T>::get(netuid)
    }
    pub fn get_pow_registrations_this_interval(netuid: NetUid) -> u16 {
        POWRegistrationsThisInterval::<T>::get(netuid)
    }
    pub fn get_burn_registrations_this_interval(netuid: NetUid) -> u16 {
        BurnRegistrationsThisInterval::<T>::get(netuid)
    }
    pub fn get_neuron_block_at_registration(netuid: NetUid, neuron_uid: u16) -> u64 {
        BlockAtRegistration::<T>::get(netuid, neuron_uid)
    }
    /// Returns the minimum number of non-immortal & non-immune UIDs that must remain in a subnet.
    pub fn get_min_non_immune_uids(netuid: NetUid) -> u16 {
        MinNonImmuneUids::<T>::get(netuid)
    }

    /// Sets the minimum number of non-immortal & non-immune UIDs that must remain in a subnet.
    pub fn set_min_non_immune_uids(netuid: NetUid, min: u16) {
        MinNonImmuneUids::<T>::insert(netuid, min);
        Self::deposit_event(Event::MinNonImmuneUidsSet(netuid, min));
    }

    // ========================
    // ===== Take checks ======
    // ========================
    pub fn do_take_checks(coldkey: &T::AccountId, hotkey: &T::AccountId) -> Result<(), Error<T>> {
        // Ensure we are delegating a known key.
        ensure!(
            Self::hotkey_account_exists(hotkey),
            Error::<T>::HotKeyAccountNotExists
        );

        // Ensure that the coldkey is the owner.
        ensure!(
            Self::coldkey_owns_hotkey(coldkey, hotkey),
            Error::<T>::NonAssociatedColdKey
        );

        Ok(())
    }

    // ========================
    // === Token Management ===
    // ========================
    pub fn set_subnet_locked_balance(netuid: NetUid, amount: TaoBalance) {
        SubnetLocked::<T>::insert(netuid, amount);
    }
    pub fn get_subnet_locked_balance(netuid: NetUid) -> TaoBalance {
        SubnetLocked::<T>::get(netuid)
    }
    pub fn get_total_subnet_locked() -> TaoBalance {
        let mut total_subnet_locked: u64 = 0;
        for (_, locked) in SubnetLocked::<T>::iter() {
            total_subnet_locked.saturating_accrue(locked.into());
        }
        total_subnet_locked.into()
    }

    pub fn set_recycle_or_burn(netuid: NetUid, recycle_or_burn: RecycleOrBurnEnum) {
        RecycleOrBurn::<T>::insert(netuid, recycle_or_burn);
    }

    // ========================
    // ========= Sudo =========
    // ========================

    // Per-block epoch cap (dynamic tempo throttle)
    pub fn get_max_epochs_per_block() -> u8 {
        MaxEpochsPerBlock::<T>::get()
    }
    pub fn set_max_epochs_per_block(max_epochs_per_block: u8) {
        MaxEpochsPerBlock::<T>::put(max_epochs_per_block);
        Self::deposit_event(Event::MaxEpochsPerBlockSet(max_epochs_per_block));
    }

    // Configure tx rate limiting
    pub fn get_tx_rate_limit() -> u64 {
        TxRateLimit::<T>::get()
    }
    pub fn set_tx_rate_limit(tx_rate_limit: u64) {
        TxRateLimit::<T>::put(tx_rate_limit);
        Self::deposit_event(Event::TxRateLimitSet(tx_rate_limit));
    }
    pub fn get_tx_delegate_take_rate_limit() -> u64 {
        TxDelegateTakeRateLimit::<T>::get()
    }
    pub fn set_tx_delegate_take_rate_limit(tx_rate_limit: u64) {
        TxDelegateTakeRateLimit::<T>::put(tx_rate_limit);
        Self::deposit_event(Event::TxDelegateTakeRateLimitSet(tx_rate_limit));
    }
    pub fn set_min_delegate_take(take: u16) {
        MinDelegateTake::<T>::put(take);
        Self::deposit_event(Event::MinDelegateTakeSet(take));
    }
    pub fn set_max_delegate_take(take: u16) {
        MaxDelegateTake::<T>::put(take);
        Self::deposit_event(Event::MaxDelegateTakeSet(take));
    }
    pub fn get_min_delegate_take() -> u16 {
        MinDelegateTake::<T>::get()
    }
    pub fn get_max_delegate_take() -> u16 {
        MaxDelegateTake::<T>::get()
    }
    pub fn get_default_delegate_take() -> u16 {
        // Default to maximum
        MaxDelegateTake::<T>::get()
    }
    // get_default_childkey_take
    pub fn get_default_childkey_take() -> u16 {
        // Default to maximum
        MinChildkeyTake::<T>::get()
    }
    pub fn get_tx_childkey_take_rate_limit() -> u64 {
        TxChildkeyTakeRateLimit::<T>::get()
    }
    pub fn set_tx_childkey_take_rate_limit(tx_rate_limit: u64) {
        TxChildkeyTakeRateLimit::<T>::put(tx_rate_limit);
        Self::deposit_event(Event::TxChildKeyTakeRateLimitSet(tx_rate_limit));
    }
    pub fn set_min_childkey_take(take: u16) {
        MinChildkeyTake::<T>::put(take);
        Self::deposit_event(Event::MinChildKeyTakeSet(take));
    }
    pub fn set_min_childkey_take_for_subnet(netuid: NetUid, take: u16) {
        MinChildkeyTakePerSubnet::<T>::insert(netuid, take);
        Self::deposit_event(Event::MinChildKeyTakePerSubnetSet(netuid, take));
    }
    pub fn set_max_childkey_take(take: u16) {
        MaxChildkeyTake::<T>::put(take);
        Self::deposit_event(Event::MaxChildKeyTakeSet(take));
    }
    pub fn get_min_childkey_take() -> u16 {
        MinChildkeyTake::<T>::get()
    }
    pub fn get_min_childkey_take_for_subnet(netuid: NetUid) -> u16 {
        MinChildkeyTakePerSubnet::<T>::get(netuid)
    }
    pub fn get_effective_min_childkey_take(netuid: NetUid) -> u16 {
        Self::get_min_childkey_take().max(Self::get_min_childkey_take_for_subnet(netuid))
    }

    pub fn get_max_childkey_take() -> u16 {
        MaxChildkeyTake::<T>::get()
    }

    pub fn get_serving_rate_limit(netuid: NetUid) -> u64 {
        ServingRateLimit::<T>::get(netuid)
    }
    pub fn set_serving_rate_limit(netuid: NetUid, serving_rate_limit: u64) {
        ServingRateLimit::<T>::insert(netuid, serving_rate_limit);
        Self::deposit_event(Event::ServingRateLimitSet(netuid, serving_rate_limit));
    }

    pub fn get_min_difficulty(netuid: NetUid) -> u64 {
        MinDifficulty::<T>::get(netuid)
    }
    pub fn set_min_difficulty(netuid: NetUid, min_difficulty: u64) {
        MinDifficulty::<T>::insert(netuid, min_difficulty);
        Self::deposit_event(Event::MinDifficultySet(netuid, min_difficulty));
    }

    pub fn get_max_difficulty(netuid: NetUid) -> u64 {
        MaxDifficulty::<T>::get(netuid)
    }
    pub fn set_max_difficulty(netuid: NetUid, max_difficulty: u64) {
        MaxDifficulty::<T>::insert(netuid, max_difficulty);
        Self::deposit_event(Event::MaxDifficultySet(netuid, max_difficulty));
    }

    pub fn get_weights_version_key(netuid: NetUid) -> u64 {
        WeightsVersionKey::<T>::get(netuid)
    }
    pub fn set_weights_version_key(netuid: NetUid, weights_version_key: u64) {
        WeightsVersionKey::<T>::insert(netuid, weights_version_key);
        Self::deposit_event(Event::WeightsVersionKeySet(netuid, weights_version_key));
    }

    pub fn get_weights_set_rate_limit(netuid: NetUid) -> u64 {
        WeightsSetRateLimit::<T>::get(netuid)
    }
    pub fn set_weights_set_rate_limit(netuid: NetUid, weights_set_rate_limit: u64) {
        WeightsSetRateLimit::<T>::insert(netuid, weights_set_rate_limit);
        Self::deposit_event(Event::WeightsSetRateLimitSet(
            netuid,
            weights_set_rate_limit,
        ));
    }

    pub fn get_adjustment_interval(netuid: NetUid) -> u16 {
        AdjustmentInterval::<T>::get(netuid)
    }
    pub fn set_adjustment_interval(netuid: NetUid, adjustment_interval: u16) {
        AdjustmentInterval::<T>::insert(netuid, adjustment_interval);
        Self::deposit_event(Event::AdjustmentIntervalSet(netuid, adjustment_interval));
    }

    pub fn get_adjustment_alpha(netuid: NetUid) -> u64 {
        AdjustmentAlpha::<T>::get(netuid)
    }
    pub fn set_adjustment_alpha(netuid: NetUid, adjustment_alpha: u64) {
        AdjustmentAlpha::<T>::insert(netuid, adjustment_alpha);
        Self::deposit_event(Event::AdjustmentAlphaSet(netuid, adjustment_alpha));
    }

    pub fn set_validator_prune_len(netuid: NetUid, validator_prune_len: u64) {
        ValidatorPruneLen::<T>::insert(netuid, validator_prune_len);
        Self::deposit_event(Event::ValidatorPruneLenSet(netuid, validator_prune_len));
    }

    pub fn get_scaling_law_power(netuid: NetUid) -> u16 {
        ScalingLawPower::<T>::get(netuid)
    }
    pub fn set_scaling_law_power(netuid: NetUid, scaling_law_power: u16) {
        ScalingLawPower::<T>::insert(netuid, scaling_law_power);
        Self::deposit_event(Event::ScalingLawPowerSet(netuid, scaling_law_power));
    }

    #[inline(always)]
    pub const fn get_max_weight_limit(_netuid: NetUid) -> u16 {
        u16::MAX
    }

    pub fn get_immunity_period(netuid: NetUid) -> u16 {
        ImmunityPeriod::<T>::get(netuid)
    }
    pub fn set_immunity_period(netuid: NetUid, immunity_period: u16) {
        ImmunityPeriod::<T>::insert(netuid, immunity_period);
        Self::deposit_event(Event::ImmunityPeriodSet(netuid, immunity_period));
    }
    /// Check if a neuron is in immunity based on the current block
    pub fn get_neuron_is_immune(netuid: NetUid, uid: u16) -> bool {
        let registered_at = Self::get_neuron_block_at_registration(netuid, uid);
        let current_block = Self::get_current_block_as_u64();
        let immunity_period = Self::get_immunity_period(netuid);
        current_block.saturating_sub(registered_at) < u64::from(immunity_period)
    }

    pub fn get_min_allowed_weights(netuid: NetUid) -> u16 {
        MinAllowedWeights::<T>::get(netuid)
    }
    pub fn set_min_allowed_weights(netuid: NetUid, min_allowed_weights: u16) {
        MinAllowedWeights::<T>::insert(netuid, min_allowed_weights);
        Self::deposit_event(Event::MinAllowedWeightSet(netuid, min_allowed_weights));
    }

    pub fn get_min_allowed_uids(netuid: NetUid) -> u16 {
        MinAllowedUids::<T>::get(netuid)
    }
    pub fn set_min_allowed_uids(netuid: NetUid, min_allowed: u16) {
        MinAllowedUids::<T>::insert(netuid, min_allowed);
        Self::deposit_event(Event::MinAllowedUidsSet(netuid, min_allowed));
    }

    pub fn get_max_allowed_uids(netuid: NetUid) -> u16 {
        MaxAllowedUids::<T>::get(netuid)
    }
    pub fn set_max_allowed_uids(netuid: NetUid, max_allowed: u16) {
        MaxAllowedUids::<T>::insert(netuid, max_allowed);
        Self::deposit_event(Event::MaxAllowedUidsSet(netuid, max_allowed));
    }

    pub fn get_kappa(netuid: NetUid) -> u16 {
        Kappa::<T>::get(netuid)
    }
    pub fn set_kappa(netuid: NetUid, kappa: u16) {
        Kappa::<T>::insert(netuid, kappa);
        Self::deposit_event(Event::KappaSet(netuid, kappa));
    }
    pub fn get_commit_reveal_weights_enabled(netuid: NetUid) -> bool {
        CommitRevealWeightsEnabled::<T>::get(netuid)
    }
    pub fn set_commit_reveal_weights_enabled(netuid: NetUid, enabled: bool) {
        CommitRevealWeightsEnabled::<T>::set(netuid, enabled);
        Self::deposit_event(Event::CommitRevealEnabled(netuid, enabled));
    }
    pub fn get_commit_reveal_weights_version() -> u16 {
        CommitRevealWeightsVersion::<T>::get()
    }
    pub fn set_commit_reveal_weights_version(version: u16) {
        CommitRevealWeightsVersion::<T>::set(version);
        Self::deposit_event(Event::CommitRevealVersionSet(version));
    }
    pub fn get_rho(netuid: NetUid) -> u16 {
        Rho::<T>::get(netuid)
    }
    pub fn set_rho(netuid: NetUid, rho: u16) {
        Rho::<T>::insert(netuid, rho);
    }

    pub fn get_activity_cutoff(netuid: NetUid) -> u16 {
        ActivityCutoff::<T>::get(netuid)
    }
    pub fn set_activity_cutoff(netuid: NetUid, activity_cutoff: u16) {
        ActivityCutoff::<T>::insert(netuid, activity_cutoff);
        Self::deposit_event(Event::ActivityCutoffSet(netuid, activity_cutoff));
    }

    /// Effective activity cutoff in blocks, derived from `ActivityCutoffFactorMilli` and `Tempo`.
    /// `cutoff_blocks = (factor × tempo) / 1000`, clamped to ≥ 1.
    pub fn get_activity_cutoff_blocks(netuid: NetUid) -> u64 {
        let factor_milli = ActivityCutoffFactorMilli::<T>::get(netuid) as u64;
        let tempo = Self::get_tempo(netuid) as u64;
        factor_milli
            .saturating_mul(tempo)
            .checked_div(1000)
            .unwrap_or(0)
            .max(1)
    }

    pub fn get_activity_cutoff_factor_milli(netuid: NetUid) -> u32 {
        ActivityCutoffFactorMilli::<T>::get(netuid)
    }

    pub fn set_activity_cutoff_factor_milli(netuid: NetUid, factor_milli: u32) {
        ActivityCutoffFactorMilli::<T>::insert(netuid, factor_milli);
        Self::deposit_event(Event::ActivityCutoffFactorMilliSet {
            netuid,
            factor_milli,
        });
    }

    // Registration Toggle utils
    pub fn get_network_registration_allowed(netuid: NetUid) -> bool {
        NetworkRegistrationAllowed::<T>::get(netuid)
    }
    pub fn set_network_registration_allowed(netuid: NetUid, registration_allowed: bool) {
        NetworkRegistrationAllowed::<T>::insert(netuid, registration_allowed);
        Self::deposit_event(Event::RegistrationAllowed(netuid, registration_allowed));
    }

    pub fn get_network_pow_registration_allowed(netuid: NetUid) -> bool {
        NetworkPowRegistrationAllowed::<T>::get(netuid)
    }
    pub fn set_network_pow_registration_allowed(netuid: NetUid, registration_allowed: bool) {
        NetworkPowRegistrationAllowed::<T>::insert(netuid, registration_allowed);
        Self::deposit_event(Event::PowRegistrationAllowed(netuid, registration_allowed));
    }

    pub fn get_target_registrations_per_interval(netuid: NetUid) -> u16 {
        TargetRegistrationsPerInterval::<T>::get(netuid)
    }
    pub fn set_target_registrations_per_interval(
        netuid: NetUid,
        target_registrations_per_interval: u16,
    ) {
        TargetRegistrationsPerInterval::<T>::insert(netuid, target_registrations_per_interval);
        Self::deposit_event(Event::RegistrationPerIntervalSet(
            netuid,
            target_registrations_per_interval,
        ));
    }

    pub fn get_burn(netuid: NetUid) -> TaoBalance {
        Burn::<T>::get(netuid)
    }
    pub fn set_burn(netuid: NetUid, burn: TaoBalance) {
        Burn::<T>::insert(netuid, burn);
    }

    pub fn get_min_burn(netuid: NetUid) -> TaoBalance {
        MinBurn::<T>::get(netuid)
    }
    pub fn set_min_burn(netuid: NetUid, min_burn: TaoBalance) {
        MinBurn::<T>::insert(netuid, min_burn);
        Self::deposit_event(Event::MinBurnSet(netuid, min_burn));
    }

    pub fn get_max_burn(netuid: NetUid) -> TaoBalance {
        MaxBurn::<T>::get(netuid)
    }
    pub fn set_max_burn(netuid: NetUid, max_burn: TaoBalance) {
        MaxBurn::<T>::insert(netuid, max_burn);
        Self::deposit_event(Event::MaxBurnSet(netuid, max_burn));
    }

    pub fn get_difficulty_as_u64(netuid: NetUid) -> u64 {
        Difficulty::<T>::get(netuid)
    }
    pub fn set_difficulty(netuid: NetUid, difficulty: u64) {
        Difficulty::<T>::insert(netuid, difficulty);
        Self::deposit_event(Event::DifficultySet(netuid, difficulty));
    }

    pub fn get_max_allowed_validators(netuid: NetUid) -> u16 {
        MaxAllowedValidators::<T>::get(netuid)
    }
    pub fn set_max_allowed_validators(netuid: NetUid, max_allowed_validators: u16) {
        MaxAllowedValidators::<T>::insert(netuid, max_allowed_validators);
        Self::deposit_event(Event::MaxAllowedValidatorsSet(
            netuid,
            max_allowed_validators,
        ));
    }

    pub fn get_bonds_moving_average(netuid: NetUid) -> u64 {
        BondsMovingAverage::<T>::get(netuid)
    }
    pub fn set_bonds_moving_average(netuid: NetUid, bonds_moving_average: u64) {
        BondsMovingAverage::<T>::insert(netuid, bonds_moving_average);
        Self::deposit_event(Event::BondsMovingAverageSet(netuid, bonds_moving_average));
    }

    pub fn get_bonds_penalty(netuid: NetUid) -> u16 {
        BondsPenalty::<T>::get(netuid)
    }
    pub fn set_bonds_penalty(netuid: NetUid, bonds_penalty: u16) {
        BondsPenalty::<T>::insert(netuid, bonds_penalty);
        Self::deposit_event(Event::BondsPenaltySet(netuid, bonds_penalty));
    }

    pub fn get_bonds_reset(netuid: NetUid) -> bool {
        BondsResetOn::<T>::get(netuid)
    }
    pub fn set_bonds_reset(netuid: NetUid, bonds_reset: bool) {
        BondsResetOn::<T>::insert(netuid, bonds_reset);
        Self::deposit_event(Event::BondsResetOnSet(netuid, bonds_reset));
    }

    pub fn get_max_registrations_per_block(netuid: NetUid) -> u16 {
        MaxRegistrationsPerBlock::<T>::get(netuid)
    }
    pub fn set_max_registrations_per_block(netuid: NetUid, max_registrations_per_block: u16) {
        MaxRegistrationsPerBlock::<T>::insert(netuid, max_registrations_per_block);
        Self::deposit_event(Event::MaxRegistrationsPerBlockSet(
            netuid,
            max_registrations_per_block,
        ));
    }

    pub fn get_subnet_owner(netuid: NetUid) -> T::AccountId {
        SubnetOwner::<T>::get(netuid)
    }
    pub fn get_subnet_owner_cut() -> u16 {
        SubnetOwnerCut::<T>::get()
    }
    pub fn get_float_subnet_owner_cut() -> U96F32 {
        U96F32::saturating_from_num(SubnetOwnerCut::<T>::get())
            .safe_div(U96F32::saturating_from_num(u16::MAX))
    }
    pub fn set_subnet_owner_cut(subnet_owner_cut: u16) {
        SubnetOwnerCut::<T>::set(subnet_owner_cut);
        Self::deposit_event(Event::SubnetOwnerCutSet(subnet_owner_cut));
    }

    pub fn get_owned_hotkeys(coldkey: &T::AccountId) -> Vec<T::AccountId> {
        OwnedHotkeys::<T>::get(coldkey)
    }
    pub fn get_all_staked_hotkeys(coldkey: &T::AccountId) -> Vec<T::AccountId> {
        StakingHotkeys::<T>::get(coldkey)
    }

    pub fn get_rao_recycled(netuid: NetUid) -> TaoBalance {
        RAORecycledForRegistration::<T>::get(netuid)
    }
    pub fn set_rao_recycled(netuid: NetUid, rao_recycled: TaoBalance) {
        RAORecycledForRegistration::<T>::insert(netuid, rao_recycled);
        Self::deposit_event(Event::RAORecycledForRegistrationSet(netuid, rao_recycled));
    }
    pub fn increase_rao_recycled(netuid: NetUid, inc_rao_recycled: TaoBalance) {
        let curr_rao_recycled = Self::get_rao_recycled(netuid);
        let rao_recycled = curr_rao_recycled.saturating_add(inc_rao_recycled);
        Self::set_rao_recycled(netuid, rao_recycled);
    }

    pub fn is_subnet_owner(address: &T::AccountId) -> bool {
        SubnetOwner::<T>::iter_values().any(|owner| *address == owner)
    }

    /// The NominatorMinRequiredStake is the factor by which we multiply
    /// the DefaultMinStake to get nominator minimum stake. With DefaulMinStake
    /// of 0.001 TAO and NominatorMinRequiredStake of 100_000_000, the
    /// minimum nomination stake will be 0.1 TAO.
    pub fn get_nominator_min_required_stake() -> u64 {
        // Get the factor (It is stored in per-million format)
        let factor = NominatorMinRequiredStake::<T>::get();

        // Return the default minimum stake multiplied by factor
        // 21M * 10^9 * 10^6 fits u64, hence no need for fixed type usage here
        DefaultMinStake::<T>::get()
            .to_u64()
            .saturating_mul(factor)
            .safe_div(1_000_000)
    }

    pub fn set_nominator_min_required_stake(min_stake: u64) {
        NominatorMinRequiredStake::<T>::put(min_stake);
    }

    pub fn get_key_swap_cost() -> TaoBalance {
        T::KeySwapCost::get().into()
    }

    pub fn get_alpha_values(netuid: NetUid) -> (u16, u16) {
        AlphaValues::<T>::get(netuid)
    }

    pub fn set_alpha_values_32(netuid: NetUid, low: I32F32, high: I32F32) {
        let low =
            (low.saturating_mul(I32F32::saturating_from_num(u16::MAX))).saturating_to_num::<u16>();
        let high =
            (high.saturating_mul(I32F32::saturating_from_num(u16::MAX))).saturating_to_num::<u16>();
        AlphaValues::<T>::insert(netuid, (low, high));
    }

    pub fn get_alpha_values_32(netuid: NetUid) -> (I32F32, I32F32) {
        let (alpha_low, alpha_high): (u16, u16) = AlphaValues::<T>::get(netuid);
        let converted_low =
            I32F32::saturating_from_num(alpha_low).safe_div(I32F32::saturating_from_num(u16::MAX));
        let converted_high =
            I32F32::saturating_from_num(alpha_high).safe_div(I32F32::saturating_from_num(u16::MAX));

        (converted_low, converted_high)
    }

    pub fn set_alpha_sigmoid_steepness(netuid: NetUid, steepness: i16) {
        AlphaSigmoidSteepness::<T>::insert(netuid, steepness);
    }
    pub fn get_alpha_sigmoid_steepness(netuid: NetUid) -> I32F32 {
        let alpha = AlphaSigmoidSteepness::<T>::get(netuid);
        I32F32::saturating_from_num(alpha)
    }

    pub fn set_liquid_alpha_enabled(netuid: NetUid, enabled: bool) {
        LiquidAlphaOn::<T>::set(netuid, enabled);
    }

    pub fn get_liquid_alpha_enabled(netuid: NetUid) -> bool {
        LiquidAlphaOn::<T>::get(netuid)
    }

    pub fn set_yuma3_enabled(netuid: NetUid, enabled: bool) {
        Yuma3On::<T>::set(netuid, enabled);
    }

    pub fn get_yuma3_enabled(netuid: NetUid) -> bool {
        Yuma3On::<T>::get(netuid)
    }

    pub fn get_subtoken_enabled(netuid: NetUid) -> bool {
        SubtokenEnabled::<T>::get(netuid)
    }

    pub fn get_transfer_toggle(netuid: NetUid) -> bool {
        TransferToggle::<T>::get(netuid)
    }

    pub fn set_coldkey_swap_announcement_delay(duration: BlockNumberFor<T>) {
        ColdkeySwapAnnouncementDelay::<T>::set(duration);
        Self::deposit_event(Event::ColdkeySwapAnnouncementDelaySet(duration));
    }

    pub fn set_coldkey_swap_reannouncement_delay(duration: BlockNumberFor<T>) {
        ColdkeySwapReannouncementDelay::<T>::set(duration);
        Self::deposit_event(Event::ColdkeySwapReannouncementDelaySet(duration));
    }

    /// Set the duration for dissolve network
    ///
    /// # Arguments
    ///
    /// * `duration` - The blocks for dissolve network execution.
    ///
    /// # Effects
    ///
    /// * Update the DissolveNetworkScheduleDuration storage.
    /// * Emits a DissolveNetworkScheduleDurationSet evnet.
    pub fn set_dissolve_network_schedule_duration(duration: BlockNumberFor<T>) {
        DissolveNetworkScheduleDuration::<T>::set(duration);
        Self::deposit_event(Event::DissolveNetworkScheduleDurationSet(duration));
    }

    /// Set the owner hotkey for a subnet.
    ///
    /// # Arguments
    ///
    /// * `netuid` - The unique identifier for the subnet.
    /// * `hotkey` - The new hotkey for the subnet owner.
    ///
    /// # Effects
    ///
    /// * Update the SubnetOwnerHotkey storage.
    /// * Emits a SubnetOwnerHotkeySet event.
    pub fn set_subnet_owner_hotkey(netuid: NetUid, hotkey: &T::AccountId) -> DispatchResult {
        // Ensure that hotkey is not a special account
        ensure!(
            Self::is_subnet_account_id(hotkey).is_none(),
            Error::<T>::CannotUseSystemAccount
        );

        SubnetOwnerHotkey::<T>::insert(netuid, hotkey.clone());
        Self::deposit_event(Event::SubnetOwnerHotkeySet(netuid, hotkey.clone()));
        Ok(())
    }

    // Get the uid of the Owner Hotkey for a subnet.
    pub fn get_owner_uid(netuid: NetUid) -> Option<u16> {
        match SubnetOwnerHotkey::<T>::try_get(netuid) {
            Ok(owner_hotkey) => Uids::<T>::get(netuid, &owner_hotkey),
            Err(_) => None,
        }
    }

    /// Set the per-subnet limit (for the given `netuid`) on the number of **owner-immune**
    /// neurons (UIDs).
    ///
    /// The value must lie within the inclusive bounds defined by [`MinImmuneOwnerUidsLimit`]
    /// and [`MaxImmuneOwnerUidsLimit`]. If the bound check fails, this returns
    /// [`Error::<T>::InvalidValue`] and leaves storage unchanged.
    ///
    /// # Parameters
    /// - `netuid`: Identifier of the subnet to update.
    /// - `limit`: New inclusive upper bound for the count of owner-immune UIDs on this subnet.
    ///
    /// # Returns
    /// - `Ok(())` on success (value written to storage).
    /// - `Err(Error::<T>::InvalidValue)` if `limit` is outside `[MinImmuneOwnerUidsLimit, MaxImmuneOwnerUidsLimit]`.
    pub fn set_owner_immune_neuron_limit(netuid: NetUid, limit: u16) -> DispatchResult {
        ensure!(
            limit >= MinImmuneOwnerUidsLimit::<T>::get()
                && limit <= MaxImmuneOwnerUidsLimit::<T>::get(),
            Error::<T>::InvalidValue
        );

        ImmuneOwnerUidsLimit::<T>::insert(netuid, limit);
        Ok(())
    }

    /// Fetches the max number of subnet
    ///
    /// # Returns:
    /// * 'u16': The max number of subnet
    ///
    pub fn get_max_subnets() -> u16 {
        SubnetLimit::<T>::get()
    }

    /// Sets the max number of subnet
    pub fn set_max_subnets(limit: u16) {
        SubnetLimit::<T>::put(limit);
        Self::deposit_event(Event::SubnetLimitSet(limit));
    }

    /// Sets TAO flow cutoff value (A)
    pub fn set_tao_flow_cutoff(flow_cutoff: I64F64) {
        TaoFlowCutoff::<T>::set(flow_cutoff);
    }

    /// Sets TAO flow normalization exponent (p)
    pub fn set_tao_flow_normalization_exponent(exponent: U64F64) {
        FlowNormExponent::<T>::set(exponent);
    }

    /// Sets TAO flow smoothing factor (alpha)
    pub fn set_tao_flow_smoothing_factor(smoothing_factor: u64) {
        FlowEmaSmoothingFactor::<T>::set(smoothing_factor);
    }

    /// Enables or disables net TAO flow (protocol cost deduction from emission shares).
    pub fn set_net_tao_flow_enabled(enabled: bool) {
        NetTaoFlowEnabled::<T>::set(enabled);
    }

    /// Multiply an integer `value` by a Q32 fixed-point factor.
    ///
    /// Q32 means:
    ///   1.0 == 1 << 32
    ///   0.5 == 1 << 31
    ///
    /// Safe / non-panicking:
    /// - uses saturating u128 multiplication
    /// - clamps back into u64 range
    pub fn mul_by_q32(value: u64, factor_q32: u64) -> u64 {
        let product: u128 = (value as u128).saturating_mul(factor_q32 as u128);
        let shifted: u128 = product >> 32;
        core::cmp::min(shifted, u64::MAX as u128) as u64
    }

    /// Exponentiation-by-squaring for Q32 values.
    ///
    /// Returns `base_q32 ^ exp` in Q32.
    ///
    /// Safe / non-panicking:
    /// - uses `mul_by_q32`, which is saturating/clamped
    pub fn pow_q32(base_q32: u64, exp: u16) -> u64 {
        let mut result: u64 = 1u64 << 32; // 1.0 in Q32
        let mut factor: u64 = base_q32;
        let mut power: u32 = u32::from(exp);

        while power > 0 {
            if (power & 1) == 1 {
                result = Self::mul_by_q32(result, factor);
            }

            power >>= 1;

            if power > 0 {
                factor = Self::mul_by_q32(factor, factor);
            }
        }

        result
    }

    /// Returns the per-block decay factor `f` in Q32
    pub fn decay_factor_q32(half_life: u16) -> u64 {
        if half_life == 0 {
            return 1u64 << 32; // 1.0
        }

        let one_q32: u64 = 1u64 << 32;
        let half_q32: u64 = 1u64 << 31; // 0.5

        let mut lo: u64 = 0;
        let mut hi: u64 = one_q32;

        while lo.saturating_add(1) < hi {
            let span: u64 = hi.saturating_sub(lo);
            let mid: u64 = lo.saturating_add(span >> 1);
            let mid_pow: u64 = Self::pow_q32(mid, half_life);

            if mid_pow > half_q32 {
                hi = mid;
            } else {
                lo = mid;
            }
        }

        lo
    }
}
