use super::*;

impl<T: Config> Pallet<T> {
    /// Install/freeze a finalized NPoS/BABE DKG authority snapshot for `epoch`.
    ///
    /// This is a runtime-internal hook for the PoA -> NPoS transition and
    /// later session/election boundaries. It is deliberately not a
    /// dispatchable extrinsic and it is deliberately not a pallet migration:
    /// the caller must be the runtime transition/session code that already
    /// owns the finalized BABE/NPoS authority and stake snapshot for the
    /// target DKG epoch.
    ///
    /// PR #1708 should call this after BABE/session/staking has initialized
    /// the first post-transition authority set, and then call the same hook
    /// whenever the finalized future N+2 session/election output changes.
    pub fn install_npos_dkg_authority_snapshot_from_transition(
        epoch: u64,
        authorities: Vec<mev_shield_ibe_runtime_api::DkgAuthorityInfo>,
    ) -> DispatchResult {
        ensure!(
            !authorities.is_empty(),
            Error::<T>::BadIbeDkgAuthoritySnapshot
        );

        let mut seen_authorities = sp_std::collections::btree_set::BTreeSet::new();
        let mut total_stake = 0u128;
        for authority in &authorities {
            ensure!(
                authority.consensus_key_kind
                    == mev_shield_ibe_runtime_api::DkgConsensusKeyKind::BabeSr25519,
                Error::<T>::BadIbeDkgAuthoritySnapshot
            );
            ensure!(
                !authority.hotkey_account_id.is_empty() && !authority.authority_id.is_empty(),
                Error::<T>::BadIbeDkgAuthoritySnapshot
            );
            ensure!(authority.stake > 0, Error::<T>::BadIbeDkgAuthoritySnapshot);
            ensure!(
                seen_authorities.insert(authority.authority_id.clone()),
                Error::<T>::BadIbeDkgAuthoritySnapshot
            );
            total_stake = total_stake
                .checked_add(authority.stake)
                .ok_or(Error::<T>::BadIbeDkgAuthoritySnapshot)?;
        }
        ensure!(total_stake > 0, Error::<T>::BadIbeDkgAuthoritySnapshot);

        IbeNposDkgAuthoritySnapshots::<T>::insert(epoch, authorities.clone());
        IbeDkgAuthoritySnapshots::<T>::insert(epoch, authorities);
        IbeDkgConsensusSources::<T>::insert(
            epoch,
            mev_shield_ibe_runtime_api::DkgConsensusSource::PosBabeRootValidators,
        );
        Ok(())
    }

    pub fn current_ibe_epoch() -> u64 {
        let n: u64 = frame_system::Pallet::<T>::block_number().saturated_into::<u64>();
        let epoch_len = T::EpochLength::get();
        let safe_epoch_len = if epoch_len == 0 { 1 } else { epoch_len };
        n.checked_div(safe_epoch_len).unwrap_or(0)
    }

    /// Base charged weight for v2 submissions before queue-depth premium.
    ///
    /// v2 `submit_encrypted` both validates the IBE envelope and appends to the
    /// runtime queue, so it must not be charged at the cheap v1 event-only
    /// `submit_encrypted` weight.
    pub fn ibe_encrypted_submission_base_weight() -> Weight {
        store_encrypted_weight().saturating_add(T::WeightInfo::submit_encrypted())
    }

    /// Current queue-depth priced weight for a v2 encrypted submission.
    pub fn current_ibe_queue_depth_priced_weight(base: Weight) -> Weight {
        queue_depth_priced_weight(
            base,
            PendingExtrinsics::<T>::count(),
            MaxPendingExtrinsicsLimit::<T>::get(),
        )
        .saturating_add(T::DbWeight::get().reads(2_u64))
    }

    pub fn epoch_bounds(epoch: u64) -> (u64, u64) {
        let len = T::EpochLength::get().max(1);
        let first = epoch.saturating_mul(len);
        let last = first.saturating_add(len.saturating_sub(1));
        (first, last)
    }

    pub fn ensure_epoch_ahead_dkg_snapshots() -> frame_support::weights::Weight {
        let current = Self::current_ibe_epoch();
        let mut reads = 0u64;
        let mut writes = 0u64;

        for offset in 0..=IBE_DKG_EPOCHS_AHEAD {
            let epoch = current.saturating_add(offset);
            reads = reads.saturating_add(4);

            let stored_authorities = IbeDkgAuthoritySnapshots::<T>::get(epoch);
            let stored_source = IbeDkgConsensusSources::<T>::get(epoch);
            let authorities_missing = stored_authorities.is_empty();
            let source_missing = stored_source.is_none();

            // The N+2 snapshot is the handoff point. If it was frozen as PoA
            // before BABE runtime authorities were available, allow exactly that
            // future snapshot to be promoted to PoS before a DKG output exists.
            // Current and N+1 snapshots are left immutable so in-flight DKG
            // plans never drift.
            let may_refresh_future_handoff = offset == IBE_DKG_EPOCHS_AHEAD
                && matches!(
                    stored_source.as_ref(),
                    Some(DkgConsensusSource::PoaAuraRootValidators)
                )
                && !IbeEpochKeys::<T>::contains_key(epoch)
                && !PublishedDkgOutputHashes::<T>::contains_key(epoch);

            if !authorities_missing && !source_missing && !may_refresh_future_handoff {
                continue;
            }

            let provider_authorities = T::IbeDkgAuthorityProvider::authorities_for_epoch(epoch);
            let provider_source = Self::consensus_source_from_authorities(&provider_authorities)
                .unwrap_or_else(|| T::IbeDkgAuthorityProvider::consensus_source_for_epoch(epoch));

            if provider_authorities.is_empty() {
                if may_refresh_future_handoff
                    && matches!(provider_source, DkgConsensusSource::PosBabeRootValidators)
                {
                    // Handoff has begun but the full PoA cohort has not finished
                    // BABE/X25519 transport-key gossip. Remove the stale future PoA plan
                    // and store the PoS source marker so workers retry instead
                    // of producing a key for the wrong consensus authority set.
                    IbeDkgAuthoritySnapshots::<T>::remove(epoch);
                    IbeDkgConsensusSources::<T>::insert(epoch, provider_source);
                    writes = writes.saturating_add(2);
                }
                continue;
            }

            let should_promote_to_pos = may_refresh_future_handoff
                && matches!(provider_source, DkgConsensusSource::PosBabeRootValidators);

            let authorities = if authorities_missing || should_promote_to_pos {
                provider_authorities
            } else {
                stored_authorities
            };
            if authorities.is_empty() {
                continue;
            }

            let source = if source_missing || should_promote_to_pos {
                provider_source
            } else {
                stored_source.unwrap_or_else(|| {
                    T::IbeDkgAuthorityProvider::consensus_source_for_epoch(epoch)
                })
            };

            if authorities_missing || should_promote_to_pos {
                let authority_count = u32::try_from(authorities.len()).unwrap_or(u32::MAX);
                IbeDkgAuthoritySnapshots::<T>::insert(epoch, authorities);
                writes = writes.saturating_add(1);
                Self::deposit_event(Event::IbeDkgAuthoritySnapshotStored {
                    epoch,
                    authority_count,
                });
            }

            if source_missing || should_promote_to_pos {
                IbeDkgConsensusSources::<T>::insert(epoch, source);
                writes = writes.saturating_add(1);
            }
        }

        T::DbWeight::get().reads_writes(reads, writes)
    }

    pub fn dkg_authorities_for_plan(epoch: u64) -> Vec<DkgAuthorityInfo> {
        let snapshot = IbeDkgAuthoritySnapshots::<T>::get(epoch);
        if !snapshot.is_empty() {
            snapshot
        } else {
            T::IbeDkgAuthorityProvider::authorities_for_epoch(epoch)
        }
    }

    pub fn dkg_two_thirds_threshold(total_weight: u128) -> Option<u128> {
        total_weight.checked_mul(2)?.checked_add(2)?.checked_div(3)
    }

    pub fn dkg_threshold_atoms_for_active_stake(
        total_stake: u128,
        eligible_stake: u128,
        total_atoms: u128,
    ) -> Option<u128> {
        if total_stake == 0 || eligible_stake == 0 || total_atoms == 0 {
            return None;
        }
        let numerator = total_stake.checked_mul(2)?.checked_mul(total_atoms)?;
        let denominator = eligible_stake.checked_mul(3)?;
        let threshold = numerator
            .checked_add(denominator.saturating_sub(1))?
            .checked_div(denominator)?;
        if threshold == 0 || threshold > total_atoms {
            return None;
        }
        Some(threshold)
    }

    pub fn dkg_authority_is_atom_eligible(
        stake: u128,
        total_stake: u128,
        max_atoms: u32,
    ) -> Option<bool> {
        if stake == 0 || total_stake == 0 || max_atoms == 0 {
            return Some(false);
        }
        Some(
            stake
                .checked_mul(u128::from(max_atoms))?
                .checked_div(total_stake)?
                > 0,
        )
    }

    pub fn expected_dkg_atom_weights(
        authorities: &[DkgAuthorityInfo],
        max_atoms: u32,
    ) -> Option<(u128, u128)> {
        if max_atoms == 0 {
            return None;
        }
        let total_stake = authorities
            .iter()
            .filter(|a| a.stake > 0)
            .try_fold(0u128, |acc, a| acc.checked_add(a.stake))?;
        if total_stake == 0 {
            return None;
        }
        let total_atoms = u128::from(max_atoms);
        let mut eligible_stake = 0u128;
        for authority in authorities.iter().filter(|a| a.stake > 0) {
            if Self::dkg_authority_is_atom_eligible(authority.stake, total_stake, max_atoms)? {
                eligible_stake = eligible_stake.checked_add(authority.stake)?;
            }
        }
        let threshold_weight =
            Self::dkg_threshold_atoms_for_active_stake(total_stake, eligible_stake, total_atoms)?;
        Some((total_atoms, threshold_weight))
    }

    pub fn dkg_consensus_source_for_plan(epoch: u64) -> DkgConsensusSource {
        IbeDkgConsensusSources::<T>::get(epoch).unwrap_or_else(|| {
            let authorities = IbeDkgAuthoritySnapshots::<T>::get(epoch);
            Self::consensus_source_from_authorities(&authorities)
                .unwrap_or_else(|| T::IbeDkgAuthorityProvider::consensus_source_for_epoch(epoch))
        })
    }

    pub fn dkg_plan_for_epoch(epoch: u64) -> Option<EpochDkgPlan> {
        let authorities = Self::dkg_authorities_for_plan(epoch);
        if authorities.is_empty() {
            return None;
        }
        let (first_block, last_block) = Self::epoch_bounds(epoch);
        Some(EpochDkgPlan {
            epoch,
            first_block,
            last_block,
            consensus_source: Self::dkg_consensus_source_for_plan(epoch),
            max_atoms: T::MaxDkgAtoms::get(),
            authorities,
        })
    }

    pub fn update_latest_published_ibe_epoch(epoch: u64) {
        LatestPublishedIbeEpoch::<T>::mutate(|latest| {
            if latest.map_or(true, |known| epoch > known) {
                *latest = Some(epoch);
            }
        });
    }

    pub fn latest_extendable_ibe_epoch_key(current_epoch: u64) -> Option<IbeEpochPublicKey> {
        let max_lookback = IBE_DKG_EPOCHS_AHEAD.saturating_add(1);
        if let Some(epoch) = LatestPublishedIbeEpoch::<T>::get() {
            if epoch < current_epoch && current_epoch.saturating_sub(epoch) <= max_lookback {
                if let Some(key) = IbeEpochKeys::<T>::get(epoch) {
                    return Some(key);
                }
            }
        }
        let mut checked = 0u64;
        let mut epoch = current_epoch;
        while epoch > 0 && checked < max_lookback {
            epoch = epoch.saturating_sub(1);
            checked = checked.saturating_add(1);
            if let Some(key) = IbeEpochKeys::<T>::get(epoch) {
                return Some(key);
            }
        }
        None
    }

    /// Return the canonical IBE epoch key clients must use for a target block.
    ///
    /// Emergency DKG fallback extends the latest usable source epoch key in
    /// place. That means clients continue to put the source `epoch` and `key_id`
    /// returned here into the v2 envelope while `first_block..=last_block`
    /// covers the requested target block.
    pub fn active_ibe_key_for_target_block(target_block: u64) -> Option<IbeEpochPublicKey> {
        let epoch_len = T::EpochLength::get().max(1);
        let target_epoch = target_block.checked_div(epoch_len).unwrap_or(0);

        if let Some(epoch_key) = IbeEpochKeys::<T>::get(target_epoch) {
            if epoch_key.first_block <= target_block && target_block <= epoch_key.last_block {
                return Some(epoch_key);
            }
        }

        if let Some(source_epoch) = LatestPublishedIbeEpoch::<T>::get() {
            if let Some(epoch_key) = IbeEpochKeys::<T>::get(source_epoch) {
                if epoch_key.first_block <= target_block && target_block <= epoch_key.last_block {
                    return Some(epoch_key);
                }
            }
        }

        let max_lookback = IBE_DKG_EPOCHS_AHEAD.saturating_add(1);
        let mut checked = 0u64;
        let mut epoch = target_epoch;
        loop {
            if checked > max_lookback {
                break;
            }
            if let Some(epoch_key) = IbeEpochKeys::<T>::get(epoch) {
                if epoch_key.first_block <= target_block && target_block <= epoch_key.last_block {
                    return Some(epoch_key);
                }
            }
            if epoch == 0 {
                break;
            }
            epoch = epoch.saturating_sub(1);
            checked = checked.saturating_add(1);
        }

        None
    }

    /// True when the chain has enough active IBE epoch-key coverage to safely
    /// enable ordinary v2 encrypted submissions at `current_block`.
    ///
    /// The MVP bootstrap contract is intentionally conservative: validators
    /// must have already published/extended active keys for the current block
    /// and for the full B/B+1 inclusion window through B+2.  This avoids a
    /// devnet or PoA->PoS handoff accepting user ciphertext that cannot be
    /// targeted or released because epoch keys are still missing.
    pub fn ibe_v2_submission_bootstrap_ready(current_block: u64) -> bool {
        let mut offset = 0u64;
        while offset <= IBE_TARGET_LOOKAHEAD_BLOCKS {
            let Some(target) = current_block.checked_add(offset) else {
                return false;
            };
            if Self::active_ibe_key_for_target_block(target).is_none() {
                return false;
            }
            offset = offset.saturating_add(1);
        }
        true
    }

    pub fn ensure_ibe_dkg_liveness() -> frame_support::weights::Weight {
        let current = Self::current_ibe_epoch();
        let (_, current_last_block) = Self::epoch_bounds(current);
        let current_block: u64 = frame_system::Pallet::<T>::block_number().saturated_into::<u64>();
        let canonical_submission_target = current_block.saturating_add(IBE_TARGET_LOOKAHEAD_BLOCKS);
        let required_last_block = core::cmp::max(current_last_block, canonical_submission_target);

        // Reads are intentionally conservative: the readiness probes and fallback
        // lookup may inspect current/latest/nearby epoch keys. The important part is
        // that fallback covers the same +2 target the submission gate requires.
        let reads = 4u64;
        let mut writes = 0u64;

        if Self::active_ibe_key_for_target_block(current_block).is_some()
            && Self::active_ibe_key_for_target_block(canonical_submission_target).is_some()
        {
            return T::DbWeight::get().reads_writes(reads, writes);
        }

        if let Some(epoch_key) = IbeEpochKeys::<T>::get(current) {
            if epoch_key.last_block >= required_last_block {
                return T::DbWeight::get().reads_writes(reads, writes);
            }
        }

        let Some(mut fallback_key) = Self::latest_extendable_ibe_epoch_key(current) else {
            return T::DbWeight::get().reads_writes(reads, writes);
        };

        if fallback_key.last_block >= required_last_block {
            return T::DbWeight::get().reads_writes(reads, writes);
        }

        let source_epoch = fallback_key.epoch;
        let key_id = fallback_key.key_id;
        fallback_key.last_block = required_last_block;
        IbeEpochKeys::<T>::insert(source_epoch, fallback_key);
        Self::update_latest_published_ibe_epoch(source_epoch);
        writes = writes.saturating_add(2);

        let epoch_len = T::EpochLength::get().max(1);
        let extended_epoch = required_last_block
            .checked_div(epoch_len)
            .unwrap_or(current);
        Self::deposit_event(Event::IbeEpochKeyEmergencyExtended {
            source_epoch,
            extended_epoch,
            key_id,
            new_last_block: required_last_block,
        });

        T::DbWeight::get().reads_writes(reads, writes)
    }

    pub fn next_epoch_dkg_plan() -> Option<EpochDkgPlan> {
        let current = Self::current_ibe_epoch();
        let target_epoch = current.saturating_add(IBE_DKG_EPOCHS_AHEAD);
        for epoch in current..=target_epoch {
            if IbeEpochKeys::<T>::contains_key(epoch) {
                continue;
            }
            if let Some(plan) = Self::dkg_plan_for_epoch(epoch) {
                return Some(plan);
            }
        }
        None
    }

    pub fn active_epoch_dkg_plan() -> Option<EpochDkgPlan> {
        Self::dkg_plan_for_epoch(Self::current_ibe_epoch())
    }

    pub fn dkg_public_output_hash(publication: &EpochDkgPublication) -> sp_core::H256 {
        sp_core::H256::from(sp_core::hashing::blake2_256(
            &(
                b"bittensor.mev-shield.v2.dkg.public-output",
                publication.epoch,
                publication.key_id,
                publication.first_block,
                publication.last_block,
                publication.consensus_source,
                &publication.master_public_key,
                publication.total_weight,
                publication.threshold_weight,
                &publication.public_atoms,
            )
                .encode(),
        ))
    }

    pub fn dkg_attestation_payload_hash(
        epoch: u64,
        key_id: [u8; KEY_ID_LEN],
        public_output_hash: sp_core::H256,
        authority_id: &[u8],
        stake: u128,
    ) -> sp_core::H256 {
        sp_core::H256::from(sp_core::hashing::blake2_256(
            &(
                b"bittensor.mev-shield.v2.dkg.output-attestation",
                epoch,
                key_id,
                public_output_hash,
                authority_id,
                stake,
            )
                .encode(),
        ))
    }

    pub fn verify_epoch_dkg_publication(publication: &EpochDkgPublication) -> DispatchResult {
        let expected_hash = Self::dkg_public_output_hash(publication);
        ensure!(
            publication.public_output_hash == expected_hash,
            Error::<T>::BadIbeDkgPublication
        );
        let current = Self::current_ibe_epoch();
        ensure!(
            publication.epoch >= current
                && publication.epoch <= current.saturating_add(IBE_DKG_EPOCHS_AHEAD),
            Error::<T>::BadIbeDkgPublication
        );
        let plan =
            Self::dkg_plan_for_epoch(publication.epoch).ok_or(Error::<T>::BadIbeDkgPublication)?;
        ensure!(
            publication.consensus_source == plan.consensus_source,
            Error::<T>::BadIbeDkgPublication
        );
        ensure!(
            publication.first_block == plan.first_block
                && publication.last_block == plan.last_block,
            Error::<T>::BadIbeDkgPublication
        );
        let (expected_total_weight, expected_threshold_weight) =
            Self::expected_dkg_atom_weights(&plan.authorities, plan.max_atoms)
                .ok_or(Error::<T>::BadIbeDkgPublication)?;
        ensure!(
            publication.total_weight == expected_total_weight
                && publication.threshold_weight == expected_threshold_weight,
            Error::<T>::BadIbeDkgPublication
        );
        ensure!(
            publication.total_weight >= publication.threshold_weight
                && publication.threshold_weight > 0,
            Error::<T>::BadIbeDkgPublication
        );
        ensure!(
            publication.public_atoms.len() <= usize::try_from(plan.max_atoms).unwrap_or(usize::MAX),
            Error::<T>::BadIbeDkgPublication
        );
        let mut seen_atoms = sp_std::collections::btree_set::BTreeSet::<u32>::new();
        let mut atom_weight = 0u128;
        for atom in &publication.public_atoms {
            ensure!(atom.share_id > 0, Error::<T>::BadIbeDkgPublication);
            ensure!(atom.weight > 0, Error::<T>::BadIbeDkgPublication);
            ensure!(
                seen_atoms.insert(atom.share_id),
                Error::<T>::BadIbeDkgPublication
            );
            atom_weight = atom_weight
                .checked_add(atom.weight)
                .ok_or(Error::<T>::BadIbeDkgPublication)?;
        }
        ensure!(
            atom_weight == publication.total_weight,
            Error::<T>::BadIbeDkgPublication
        );
        ensure!(
            !IbeEpochKeys::<T>::contains_key(publication.epoch)
                && !PublishedDkgOutputHashes::<T>::contains_key(publication.epoch),
            Error::<T>::IbeDkgPublicationAlreadyKnown
        );
        let total_stake = plan
            .authorities
            .iter()
            .try_fold(0u128, |acc, a| acc.checked_add(a.stake))
            .ok_or(Error::<T>::BadIbeDkgPublication)?;
        let threshold_stake =
            Self::dkg_two_thirds_threshold(total_stake).ok_or(Error::<T>::BadIbeDkgPublication)?;
        let mut by_authority = sp_std::collections::btree_map::BTreeMap::<Vec<u8>, u128>::new();
        for a in &plan.authorities {
            if Self::dkg_authority_is_atom_eligible(a.stake, total_stake, plan.max_atoms)
                .ok_or(Error::<T>::BadIbeDkgPublication)?
            {
                by_authority.insert(a.authority_id.clone(), a.stake);
            }
        }
        let mut attested_stake = 0u128;
        let mut seen = sp_std::collections::btree_set::BTreeSet::<Vec<u8>>::new();
        for att in &publication.attestations {
            if !seen.insert(att.authority_id.clone()) {
                continue;
            }
            let Some(expected_stake) = by_authority.get(&att.authority_id).copied() else {
                continue;
            };
            if expected_stake != att.stake || att.public_output_hash != expected_hash {
                continue;
            }
            let payload = Self::dkg_attestation_payload_hash(
                publication.epoch,
                publication.key_id,
                expected_hash,
                &att.authority_id,
                att.stake,
            );
            if T::IbeDkgAuthorityProvider::verify_authority_signature(
                &att.authority_id,
                payload,
                &att.signature,
            ) {
                attested_stake = attested_stake.saturating_add(att.stake);
            }
        }
        ensure!(
            attested_stake >= threshold_stake,
            Error::<T>::InsufficientIbeDkgAttestationWeight
        );
        Ok(())
    }

    pub fn publish_ibe_epoch_public_key_inner(publication: EpochDkgPublication) -> DispatchResult {
        Self::verify_epoch_dkg_publication(&publication)?;
        let master_public_key: BoundedMasterPublicKey = publication
            .master_public_key
            .clone()
            .try_into()
            .map_err(|_| Error::<T>::BadIbeDkgPublication)?;
        let public_atoms: BoundedDkgPublicShareAtoms = publication
            .public_atoms
            .clone()
            .try_into()
            .map_err(|_| Error::<T>::BadIbeDkgPublication)?;
        let epoch_key = IbeEpochPublicKey {
            epoch: publication.epoch,
            key_id: publication.key_id,
            master_public_key,
            total_weight: publication.total_weight,
            threshold_weight: publication.threshold_weight,
            public_atoms,
            first_block: publication.first_block,
            last_block: publication.last_block,
        };
        IbeEpochKeys::<T>::insert(publication.epoch, epoch_key);
        PublishedDkgOutputHashes::<T>::insert(
            publication.epoch,
            Self::dkg_public_output_hash(&publication),
        );
        Self::update_latest_published_ibe_epoch(publication.epoch);
        let attested_weight = publication
            .attestations
            .iter()
            .fold(0u128, |acc, a| acc.saturating_add(a.stake));
        Self::deposit_event(Event::IbeEpochDkgPublicKeyPublished {
            epoch: publication.epoch,
            key_id: publication.key_id,
            attested_weight,
        });
        Ok(())
    }

    pub(crate) fn enqueue_ibe_encrypted(
        who: T::AccountId,
        encrypted_call: BoundedVec<u8, MaxEncryptedCallSize>,
    ) -> Result<(u32, IbeEncryptedExtrinsicV1), DispatchError> {
        let envelope = IbeEncryptedExtrinsicV1::decode_v2(encrypted_call.as_slice())
            .map_err(|_| Error::<T>::BadIbeEnvelope)?;
        Self::validate_v2_envelope_for_submission(&envelope)?;
        ensure!(
            PendingIbeBySubmitter::<T>::get(&who) < T::MaxPendingIbePerSender::get(),
            Error::<T>::TooManyPendingIbeForSender
        );

        let index = match Self::store_pending_encrypted(who.clone(), encrypted_call) {
            Ok(index) => index,
            Err(error) => return Err(error),
        };

        let submission_deposit = Self::ibe_submission_deposit();
        if let Err(error) = Self::reserve_ibe_submission_deposit(index, &who, submission_deposit) {
            Self::rollback_pending_insert(index);
            return Err(error);
        }

        if let Err(error) = Self::after_v2_pending_push(&who, index, &envelope) {
            Self::refund_ibe_submission_deposit(index);
            Self::rollback_pending_insert(index);
            return Err(error);
        }

        Ok((index, envelope))
    }

    pub fn verify_ibe_block_decryption_key_material(key: &IbeBlockDecryptionKeyV1) -> bool {
        if key.version != MEV_SHIELD_IBE_VERSION {
            return false;
        }

        let Some(epoch_key) = IbeEpochKeys::<T>::get(key.epoch) else {
            return false;
        };
        if epoch_key.key_id != key.key_id {
            return false;
        }
        if key.target_block < epoch_key.first_block || key.target_block > epoch_key.last_block {
            return false;
        }

        let expected_finalized = key.target_block.saturating_sub(1);
        if key.finalized_ordering_block_number != expected_finalized {
            return false;
        }

        let genesis_hash = frame_system::Pallet::<T>::block_hash(BlockNumberFor::<T>::zero());
        T::IbeKeyVerifier::verify_block_identity_key(
            genesis_hash,
            &epoch_key,
            key.target_block,
            key.identity_decryption_key.as_slice(),
        )
    }

    pub fn verify_ibe_block_decryption_key_release_bundle(
        bundle: &IbeBlockDecryptionKeyShareBundleV1,
    ) -> bool {
        let key = &bundle.key;
        if !Self::verify_ibe_block_decryption_key_material(key) {
            return false;
        }
        let Some(epoch_key) = IbeEpochKeys::<T>::get(key.epoch) else {
            return false;
        };
        if epoch_key.public_atoms.is_empty() || bundle.shares.is_empty() {
            return false;
        }

        let genesis_hash = frame_system::Pallet::<T>::block_hash(BlockNumberFor::<T>::zero());
        let mut public_atoms = sp_std::collections::btree_map::BTreeMap::<
            u32,
            &stp_mev_shield_ibe::IbeDkgPublicShareAtomV1,
        >::new();
        for atom in epoch_key.public_atoms.iter() {
            public_atoms.insert(atom.share_id, atom);
        }

        let mut seen = sp_std::collections::btree_set::BTreeSet::<u32>::new();
        let mut total_weight = 0u128;
        let mut verified_shares = Vec::new();
        for share in &bundle.shares {
            if share.version != MEV_SHIELD_IBE_VERSION
                || share.epoch != key.epoch
                || share.target_block != key.target_block
                || share.key_id != key.key_id
                || share.finalized_ordering_block_number != key.finalized_ordering_block_number
                || share.finalized_ordering_block_hash != key.finalized_ordering_block_hash
            {
                return false;
            }
            if !seen.insert(share.share_id) {
                continue;
            }
            let Some(atom) = public_atoms.get(&share.share_id).copied() else {
                return false;
            };
            if share.weight != atom.weight
                || share.public_share.as_slice() != atom.public_share.as_slice()
            {
                return false;
            }
            if !T::IbeKeyVerifier::verify_partial_identity_key(genesis_hash, &epoch_key, share) {
                return false;
            }
            let Some(next_weight) = total_weight.checked_add(share.weight) else {
                return false;
            };
            total_weight = next_weight;
            verified_shares.push(share.clone());
        }

        if total_weight < epoch_key.threshold_weight {
            return false;
        }
        let Some(combined_key) =
            T::IbeKeyVerifier::combine_partial_identity_key_shares(&epoch_key, &verified_shares)
        else {
            return false;
        };
        combined_key.as_slice() == key.identity_decryption_key.as_slice()
    }

    pub fn validate_ibe_block_decryption_key_release_bundle(
        bundle: &IbeBlockDecryptionKeyShareBundleV1,
    ) -> Result<(), Error<T>> {
        let key = &bundle.key;
        let expected_finalized_ordering_block_number = key
            .target_block
            .checked_sub(1)
            .ok_or(Error::<T>::InvalidIbeFinalityPoint)?;
        ensure!(
            key.finalized_ordering_block_number == expected_finalized_ordering_block_number,
            Error::<T>::InvalidIbeFinalityPoint
        );
        let current_block_u64: u64 =
            frame_system::Pallet::<T>::block_number().saturated_into::<u64>();
        ensure!(
            current_block_u64 >= key.target_block,
            Error::<T>::IbeKeyTooEarly
        );
        ensure!(
            Self::verify_ibe_block_decryption_key_release_bundle(bundle),
            Error::<T>::InvalidIbeBlockDecryptionKey
        );
        ensure!(
            IbeBlockDecryptionKeys::<T>::get(block_key_storage_key(
                key.epoch,
                key.target_block,
                key.key_id,
            ))
            .is_none(),
            Error::<T>::IbeKeyAlreadyPublished
        );
        Ok(())
    }

    pub fn store_ibe_block_decryption_key_bundle_from_preruntime_digest(
        bundle: IbeBlockDecryptionKeyShareBundleV1,
    ) -> Result<bool, Error<T>> {
        let key = bundle.key.clone();
        let storage_key = block_key_storage_key(key.epoch, key.target_block, key.key_id);
        if IbeBlockDecryptionKeys::<T>::contains_key(storage_key) {
            return Ok(false);
        }
        Self::validate_ibe_block_decryption_key_release_bundle(&bundle)?;
        IbeBlockDecryptionKeys::<T>::insert(storage_key, key.clone());
        Self::deposit_event(Event::IbeBlockDecryptionKeySubmitted {
            epoch: key.epoch,
            target_block: key.target_block,
            key_id: key.key_id,
        });
        Ok(true)
    }

    pub fn validate_ibe_block_decryption_key_for_runtime_api(
        key: &IbeBlockDecryptionKeyV1,
    ) -> bool {
        Self::verify_ibe_block_decryption_key_material(key)
    }

    pub fn validate_ibe_block_decryption_key_for_submission(
        key: &IbeBlockDecryptionKeyV1,
    ) -> DispatchResult {
        ensure!(
            key.version == MEV_SHIELD_IBE_VERSION,
            Error::<T>::InvalidIbeBlockDecryptionKey
        );
        let epoch_key = IbeEpochKeys::<T>::get(key.epoch).ok_or(Error::<T>::UnknownIbeEpoch)?;
        ensure!(epoch_key.key_id == key.key_id, Error::<T>::WrongIbeEpochKey);
        ensure!(
            key.target_block >= epoch_key.first_block && key.target_block <= epoch_key.last_block,
            Error::<T>::IbeEpochKeyInactive
        );
        let current_block_u64: u64 =
            frame_system::Pallet::<T>::block_number().saturated_into::<u64>();
        let expected_finalized_ordering_block_number = key
            .target_block
            .checked_sub(1)
            .ok_or(Error::<T>::InvalidIbeFinalityPoint)?;
        ensure!(
            current_block_u64 >= expected_finalized_ordering_block_number,
            Error::<T>::IbeKeyTooEarly
        );
        ensure!(
            key.finalized_ordering_block_number == expected_finalized_ordering_block_number,
            Error::<T>::InvalidIbeFinalityPoint
        );
        ensure!(
            IbeBlockDecryptionKeys::<T>::get(block_key_storage_key(
                key.epoch,
                key.target_block,
                key.key_id,
            ))
            .is_none(),
            Error::<T>::IbeKeyAlreadyPublished
        );
        let genesis_hash = frame_system::Pallet::<T>::block_hash(BlockNumberFor::<T>::zero());
        ensure!(
            T::IbeKeyVerifier::verify_block_identity_key(
                genesis_hash,
                &epoch_key,
                key.target_block,
                key.identity_decryption_key.as_slice(),
            ),
            Error::<T>::InvalidIbeBlockDecryptionKey
        );
        Ok(())
    }

    /// Import threshold-IBE block-key release bundles from pre-runtime digests.
    ///
    /// This is the spec-compatible delivery path: the block author puts
    /// threshold-share release bundles in the header, so `on_initialize` can
    /// verify/store keys before draining the encrypted queue.
    pub(crate) fn ingest_ibe_block_key_preruntime_digests() -> Weight {
        let mut imported = 0u64;
        let now: u64 = frame_system::Pallet::<T>::block_number().saturated_into();

        for log_item in frame_system::Pallet::<T>::digest().logs().iter() {
            let sp_runtime::DigestItem::PreRuntime(engine_id, payload) = log_item else {
                continue;
            };

            if engine_id != &IBE_BLOCK_DECRYPTION_KEYS_ENGINE_ID {
                continue;
            }

            let Ok(data) = IbeBlockDecryptionKeyPreRuntimeDigestData::decode(&mut &payload[..])
            else {
                log::debug!(target: LOG_TARGET, "ignoring malformed IBE block-key pre-runtime digest");
                continue;
            };

            for bundle in data.share_bundles {
                if bundle.key.target_block > now {
                    continue;
                }
                match Self::store_ibe_block_decryption_key_bundle_from_preruntime_digest(bundle) {
                    Ok(true) => imported = imported.saturating_add(1),
                    Ok(false) => {}
                    Err(error) => {
                        log::debug!(
                            target: LOG_TARGET,
                            "ignoring invalid IBE block-key pre-runtime release bundle: {:?}",
                            error,
                        );
                    }
                }
            }
        }

        T::DbWeight::get().reads_writes(1u64, imported)
    }

    pub(crate) fn process_pending_ibe_extrinsic(
        index: u32,
        pending: PendingExtrinsic<T>,
        mut weight: Weight,
        remove_weight: Weight,
    ) -> PendingProcess {
        let outcome = T::IbeEncryptedTxDecryptor::decrypt(pending.encrypted_call.as_slice());
        let inner = match outcome {
            IbeDecryptOutcome::NotReady => {
                // Keep the entry at the FIFO head. A missing or not-yet-imported
                // block key must not let a block author consume/drop encrypted
                // work and then execute plaintext non-operational calls.
                if let Some(meta) = PendingIbeMetadata::<T>::get(index) {
                    weight = weight.saturating_add(T::DbWeight::get().reads(1));
                    Self::deposit_event(Event::IbeBlockKeyUnavailable {
                        index,
                        epoch: meta.epoch,
                        target_block: meta.target_block,
                        key_id: meta.key_id,
                    });
                }
                Self::deposit_event(Event::ExtrinsicPostponed { index });
                return PendingProcess::Break(weight);
            }
            IbeDecryptOutcome::InvalidAfterKeyAvailable => {
                Self::forfeit_ibe_submission_deposit(index);
                Self::remove_pending_index(index);
                weight = weight.saturating_add(remove_weight);
                Self::deposit_event(Event::IbeEncryptedExtrinsicInvalid { index });
                return PendingProcess::Continue(weight);
            }
            IbeDecryptOutcome::Ready(inner) => inner,
        };

        let Some(info) = T::DecryptedExtrinsicExecutor::dispatch_info(&inner) else {
            Self::forfeit_ibe_submission_deposit(index);
            Self::remove_pending_index(index);
            weight = weight.saturating_add(remove_weight);
            Self::deposit_event(Event::IbeEncryptedExtrinsicInvalid { index });
            return PendingProcess::Continue(weight);
        };

        let dispatch_weight = T::DbWeight::get()
            .writes(2)
            .saturating_add(info.call_weight);
        // MaxExtrinsicWeight is a ref_time-only storage limit. Do not compare
        // against Weight::from_parts(limit, 0), because that creates a zero
        // proof-size ceiling and rejects ordinary calls with non-zero proof_size.
        if info.call_weight.ref_time() > MaxExtrinsicWeight::<T>::get() {
            Self::forfeit_ibe_submission_deposit(index);
            Self::remove_pending_index(index);
            weight = weight.saturating_add(remove_weight);
            Self::deposit_event(Event::ExtrinsicWeightExceeded { index });
            Self::deposit_event(Event::IbeEncryptedExtrinsicExecuted {
                index,
                success: false,
            });
            return PendingProcess::Continue(weight);
        }

        // OnInitializeWeight is also stored as a ref_time-only budget.
        if weight.saturating_add(dispatch_weight).ref_time() > OnInitializeWeight::<T>::get() {
            Self::deposit_event(Event::ExtrinsicPostponed { index });
            return PendingProcess::Break(weight);
        }

        Self::remove_pending_index(index);
        weight = weight.saturating_add(remove_weight);

        IbeQueueDrainInProgress::<T>::put(true);
        weight = weight.saturating_add(T::DbWeight::get().writes(1));
        let applied = T::DecryptedExtrinsicExecutor::apply(inner);

        IbeQueueDrainInProgress::<T>::kill();
        weight = weight.saturating_add(T::DbWeight::get().writes(1));
        weight = weight.saturating_add(applied.consumed_weight);
        if applied.success {
            Self::refund_ibe_submission_deposit(index);
        } else {
            Self::forfeit_ibe_submission_deposit(index);
        }
        Self::deposit_event(Event::IbeEncryptedExtrinsicExecuted {
            index,
            success: applied.success,
        });
        PendingProcess::Continue(weight)
    }

    pub fn ibe_block_decryption_key(
        epoch: u64,
        target_block: u64,
        key_id: [u8; KEY_ID_LEN],
    ) -> Option<IbeBlockDecryptionKeyV1> {
        IbeBlockDecryptionKeys::<T>::get(block_key_storage_key(epoch, target_block, key_id))
    }

    pub fn pending_ibe_identities(limit: u32) -> Vec<IbePendingIdentity> {
        let limit_usize = if limit == 0 {
            usize::MAX
        } else {
            usize::try_from(limit).unwrap_or(usize::MAX)
        };
        let next_index = NextPendingExtrinsicIndex::<T>::get();
        let count: u32 = PendingExtrinsics::<T>::count();
        let start_index = next_index.saturating_sub(count);
        let mut identities = sp_std::collections::btree_map::BTreeMap::<
            (u64, u64, [u8; KEY_ID_LEN]),
            (u32, u32),
        >::new();

        for index in start_index..next_index {
            if identities.len() >= limit_usize {
                break;
            }
            let Some(meta) = PendingIbeMetadata::<T>::get(index) else {
                continue;
            };
            let key = (meta.epoch, meta.target_block, meta.key_id);
            identities
                .entry(key)
                .and_modify(|range| {
                    range.0 = range.0.min(index);
                    range.1 = range.1.max(index);
                })
                .or_insert((index, index));
        }

        // Section 8 conditional entries use the same block-identity key release
        // path once their public predicate has fired. This makes finality-gated
        // share release and the pre-runtime digest proposer discover fired
        // conditional identities, not only regular FIFO queue identities.
        let now: u64 = frame_system::Pallet::<T>::block_number().saturated_into::<u64>();
        for (index, entry) in PendingConditionalIbeQueue::<T>::iter() {
            if identities.len() >= limit_usize {
                break;
            }
            if now > entry.expires_at || !entry.condition.is_fired(now) {
                continue;
            }
            let key = (entry.epoch, entry.target_block, entry.key_id);
            identities
                .entry(key)
                .and_modify(|range| {
                    range.0 = range.0.min(index);
                    range.1 = range.1.max(index);
                })
                .or_insert((index, index));
        }

        identities
            .into_iter()
            .map(
                |((epoch, target_block, key_id), (first_queue_index, last_queue_index))| {
                    IbePendingIdentity {
                        epoch,
                        target_block,
                        key_id,
                        first_queue_index,
                        last_queue_index,
                    }
                },
            )
            .collect()
    }

    /// Whether the runtime is currently applying decrypted threshold-IBE queue entries.
    pub fn is_ibe_queue_drain_in_progress() -> bool {
        IbeQueueDrainInProgress::<T>::get()
    }

    /// Returns true when the canonical queue head is a MEV Shield v2 entry whose
    /// target identity is due at the current block.
    ///
    /// This is the runtime-enforced no-preemption guard: if on_initialize cannot
    /// fully drain due encrypted work because a key is missing, an entry is not
    /// ready, or the configured weight budget is exhausted, ordinary
    /// non-operational plaintext extrinsics must not execute later in the same
    /// block.
    pub fn has_due_ibe_queue_head() -> bool {
        let current_block: u64 = frame_system::Pallet::<T>::block_number().saturated_into();
        Self::has_due_ibe_queue_head_at(current_block)
    }

    /// Same as `has_due_ibe_queue_head`, but evaluated at an explicit block number.
    pub fn has_due_ibe_queue_head_at(block_number: u64) -> bool {
        Self::due_ibe_queue_head_at(block_number).is_some()
    }

    /// Return the canonical threshold-IBE queue head when it is due at
    /// `block_number`. This is stricter than scanning for any due identity:
    /// queue order is load-bearing for MEV Shield, so block import must make
    /// key-release liveness decisions from the actual queue head only.
    pub fn due_ibe_queue_head_at(block_number: u64) -> Option<IbePendingIdentity> {
        let next_index = NextPendingExtrinsicIndex::<T>::get();
        let count: u32 = PendingExtrinsics::<T>::count();
        if count == 0 {
            return None;
        }

        let start_index = next_index.saturating_sub(count);
        for index in start_index..next_index {
            if !PendingExtrinsics::<T>::contains_key(index) {
                continue;
            }

            let Some(meta) = PendingIbeMetadata::<T>::get(index) else {
                // Legacy/non-v2 entries do not carry IBE metadata. For the v2 MVP
                // key-liveness/import guard, scan past them so an obsolete entry
                // cannot hide a due threshold-IBE head behind it.
                continue;
            };

            if meta.target_block > block_number {
                return None;
            }

            return Some(IbePendingIdentity {
                epoch: meta.epoch,
                target_block: meta.target_block,
                key_id: meta.key_id,
                first_queue_index: index,
                last_queue_index: index,
            });
        }

        None
    }

    pub fn has_ibe_block_key(epoch: u64, target_block: u64, key_id: [u8; KEY_ID_LEN]) -> bool {
        Self::ibe_block_decryption_key(epoch, target_block, key_id).is_some()
    }
}
