use super::*;

impl<T: Config> Pallet<T> {
    pub fn conditional_encrypted_submission_weight(
        condition: &ConditionalIbeCondition,
        lifetime_blocks: u32,
    ) -> Weight {
        let lifetime = u64::from(lifetime_blocks);
        let eval_ref_time = condition
            .condition_eval_weight_ref_time()
            .saturating_mul(lifetime);
        let storage_rent_ref_time = condition
            .encoded_condition_len()
            .saturating_mul(lifetime)
            .saturating_mul(CONDITIONAL_IBE_STORAGE_RENT_WEIGHT_PER_BYTE_BLOCK);
        let committee_premium_ref_time =
            CONDITIONAL_IBE_COMMITTEE_PREMIUM_WEIGHT_REF_TIME.saturating_mul(lifetime);
        let variable_ref_time = eval_ref_time
            .saturating_add(storage_rent_ref_time)
            .saturating_add(committee_premium_ref_time);
        let base = Self::current_ibe_queue_depth_priced_weight(
            Self::ibe_encrypted_submission_base_weight(),
        );
        base.saturating_add(Weight::from_parts(variable_ref_time, 0))
    }

    pub(crate) fn reserve_conditional_ibe_submission_deposit(
        index: u32,
        who: &T::AccountId,
        amount: BalanceOf<T>,
    ) -> DispatchResult {
        if amount.is_zero() {
            return Ok(());
        }
        T::Currency::reserve(who, amount)?;
        PendingConditionalIbeSubmissionDeposits::<T>::insert(index, (who.clone(), amount));
        Self::deposit_event(Event::IbeSubmissionDepositReserved {
            index,
            who: who.clone(),
            amount,
        });
        Ok(())
    }

    pub(crate) fn refund_conditional_ibe_submission_deposit(index: u32) {
        let Some((who, amount)) = PendingConditionalIbeSubmissionDeposits::<T>::take(index) else {
            return;
        };
        if amount.is_zero() {
            return;
        }
        let _ = T::Currency::unreserve(&who, amount);
        Self::deposit_event(Event::IbeSubmissionDepositRefunded { index, who, amount });
    }

    pub(crate) fn forfeit_conditional_ibe_submission_deposit(index: u32) {
        let Some((who, amount)) = PendingConditionalIbeSubmissionDeposits::<T>::take(index) else {
            return;
        };
        if amount.is_zero() {
            return;
        }
        let (_slashed, unslashed) = T::Currency::slash_reserved(&who, amount);
        if !unslashed.is_zero() {
            let _ = T::Currency::unreserve(&who, unslashed);
        }
        Self::deposit_event(Event::IbeSubmissionDepositForfeited { index, who, amount });
    }

    pub(crate) fn remove_conditional_ibe_entry(index: u32) -> Option<PendingConditionalIbe<T>> {
        let entry = PendingConditionalIbeQueue::<T>::take(index)?;
        PendingConditionalIbeCommitments::<T>::remove(entry.commitment);
        PendingConditionalIbeBySubmitter::<T>::mutate(&entry.who, |n| *n = n.saturating_sub(1));
        Some(entry)
    }

    pub(crate) fn validate_conditional_ibe_envelope(
        envelope: &IbeEncryptedExtrinsicV1,
        condition: &ConditionalIbeCondition,
        now: u64,
        lifetime_blocks: u32,
    ) -> DispatchResult {
        ensure!(
            lifetime_blocks > 0 && lifetime_blocks <= MAX_CONDITIONAL_IBE_LIFETIME_BLOCKS,
            Error::<T>::InvalidConditionalIbeCondition
        );
        ensure!(
            envelope.version == MEV_SHIELD_IBE_VERSION,
            Error::<T>::BadIbeEnvelope
        );
        let target_block = condition.target_block();
        ensure!(
            target_block > now,
            Error::<T>::InvalidConditionalIbeCondition
        );
        let expires_at = now
            .checked_add(u64::from(lifetime_blocks))
            .ok_or(Error::<T>::InvalidConditionalIbeCondition)?;
        ensure!(
            target_block <= expires_at,
            Error::<T>::InvalidConditionalIbeCondition
        );
        ensure!(
            envelope.target_block == target_block,
            Error::<T>::InvalidIbeTargetWindow
        );
        ensure!(
            !PendingIbeCommitments::<T>::contains_key(envelope.commitment)
                && !PendingConditionalIbeCommitments::<T>::contains_key(envelope.commitment),
            Error::<T>::DuplicateIbeCommitment
        );
        let epoch_key = Self::active_ibe_key_for_target_block(target_block)
            .ok_or(Error::<T>::UnknownIbeEpoch)?;
        ensure!(
            envelope.epoch == epoch_key.epoch,
            Error::<T>::UnknownIbeEpoch
        );
        ensure!(
            epoch_key.key_id == envelope.key_id,
            Error::<T>::WrongIbeEpochKey
        );
        ensure!(
            target_block >= epoch_key.first_block && target_block <= epoch_key.last_block,
            Error::<T>::IbeEpochKeyInactive
        );
        ensure!(
            IbeBlockDecryptionKeys::<T>::get(block_key_storage_key(
                envelope.epoch,
                envelope.target_block,
                envelope.key_id,
            ))
            .is_none(),
            Error::<T>::IbeKeyAlreadyPublished
        );
        Ok(())
    }

    pub(crate) fn submit_conditional_encrypted_inner(
        who: T::AccountId,
        encrypted_call: BoundedVec<u8, MaxEncryptedCallSize>,
        condition: ConditionalIbeCondition,
        lifetime_blocks: u32,
    ) -> DispatchResult {
        ensure!(
            PendingConditionalIbeQueue::<T>::count() < MaxPendingExtrinsicsLimit::<T>::get(),
            Error::<T>::TooManyPendingExtrinsics
        );
        ensure!(
            PendingConditionalIbeBySubmitter::<T>::get(&who) < T::MaxPendingIbePerSender::get(),
            Error::<T>::TooManyPendingIbeForSender
        );
        let envelope = IbeEncryptedExtrinsicV1::decode_v2(encrypted_call.as_slice())
            .map_err(|_| Error::<T>::BadIbeEnvelope)?;
        let now_block: u64 = frame_system::Pallet::<T>::block_number().saturated_into::<u64>();
        Self::validate_conditional_ibe_envelope(&envelope, &condition, now_block, lifetime_blocks)?;
        let expires_at = now_block
            .checked_add(u64::from(lifetime_blocks))
            .ok_or(Error::<T>::InvalidConditionalIbeCondition)?;
        let index = NextPendingConditionalIbeIndex::<T>::get();
        let next_index = index
            .checked_add(1)
            .ok_or(Error::<T>::TooManyPendingExtrinsics)?;
        let deposit = Self::ibe_submission_deposit();
        Self::reserve_conditional_ibe_submission_deposit(index, &who, deposit)?;
        PendingConditionalIbeQueue::<T>::insert(
            index,
            PendingConditionalIbe::<T> {
                who: who.clone(),
                encrypted_call,
                condition,
                submitted_at: frame_system::Pallet::<T>::block_number(),
                expires_at,
                epoch: envelope.epoch,
                target_block: envelope.target_block,
                key_id: envelope.key_id,
                commitment: envelope.commitment,
            },
        );
        NextPendingConditionalIbeIndex::<T>::put(next_index);
        PendingConditionalIbeCommitments::<T>::insert(envelope.commitment, index);
        PendingConditionalIbeBySubmitter::<T>::mutate(&who, |n| *n = n.saturating_add(1));
        Self::deposit_event(Event::ConditionalIbeEncryptedSubmitted {
            index,
            who,
            target_block: envelope.target_block,
            expires_at,
            commitment: envelope.commitment,
        });
        Ok(())
    }

    pub fn has_fired_conditional_ibe() -> bool {
        let now: u64 = frame_system::Pallet::<T>::block_number().saturated_into::<u64>();
        PendingConditionalIbeQueue::<T>::iter()
            .any(|(_, entry)| now <= entry.expires_at && entry.condition.is_fired(now))
    }

    pub fn process_conditional_ibe_queue() -> Weight {
        Self::process_conditional_ibe_queue_after(Weight::zero())
    }

    pub(crate) fn process_conditional_ibe_queue_after(base_weight: Weight) -> Weight {
        let mut consumed = Weight::zero();
        let now: u64 = frame_system::Pallet::<T>::block_number().saturated_into::<u64>();
        let mut indices: Vec<u32> = PendingConditionalIbeQueue::<T>::iter_keys().collect();
        indices.sort_unstable();

        for index in indices {
            let eval_weight = Weight::from_parts(CONDITIONAL_IBE_EVAL_WEIGHT_REF_TIME, 0);
            if base_weight
                .saturating_add(consumed)
                .saturating_add(eval_weight)
                .ref_time()
                > OnInitializeWeight::<T>::get()
            {
                break;
            }
            consumed = consumed.saturating_add(eval_weight);

            let Some(entry) = PendingConditionalIbeQueue::<T>::get(index) else {
                continue;
            };
            let remove_weight = T::DbWeight::get().reads_writes(1, 2);
            if now > entry.expires_at {
                let _ = Self::remove_conditional_ibe_entry(index);
                consumed = consumed.saturating_add(remove_weight);
                Self::refund_conditional_ibe_submission_deposit(index);
                Self::deposit_event(Event::ConditionalIbeExpired { index });
                continue;
            }
            if !entry.condition.is_fired(now) {
                continue;
            }
            match T::IbeEncryptedTxDecryptor::decrypt(entry.encrypted_call.as_slice()) {
                IbeDecryptOutcome::NotReady => {
                    Self::deposit_event(Event::IbeBlockKeyUnavailable {
                        index,
                        epoch: entry.epoch,
                        target_block: entry.target_block,
                        key_id: entry.key_id,
                    });
                    Self::deposit_event(Event::ExtrinsicPostponed { index });
                    continue;
                }
                IbeDecryptOutcome::InvalidAfterKeyAvailable => {
                    let _ = Self::remove_conditional_ibe_entry(index);
                    consumed = consumed.saturating_add(remove_weight);
                    Self::forfeit_conditional_ibe_submission_deposit(index);
                    Self::deposit_event(Event::ConditionalIbeInvalid { index });
                }
                IbeDecryptOutcome::Ready(inner) => {
                    let Some(info) = T::DecryptedExtrinsicExecutor::dispatch_info(&inner) else {
                        let _ = Self::remove_conditional_ibe_entry(index);
                        consumed = consumed.saturating_add(remove_weight);
                        Self::forfeit_conditional_ibe_submission_deposit(index);
                        Self::deposit_event(Event::ConditionalIbeInvalid { index });
                        continue;
                    };
                    if info.call_weight.ref_time() > MaxExtrinsicWeight::<T>::get() {
                        let _ = Self::remove_conditional_ibe_entry(index);
                        consumed = consumed.saturating_add(remove_weight);
                        Self::forfeit_conditional_ibe_submission_deposit(index);
                        Self::deposit_event(Event::ExtrinsicWeightExceeded { index });
                        continue;
                    }

                    let dispatch_weight = T::DbWeight::get()
                        .writes(2)
                        .saturating_add(info.call_weight);
                    if base_weight
                        .saturating_add(consumed)
                        .saturating_add(dispatch_weight)
                        .ref_time()
                        > OnInitializeWeight::<T>::get()
                    {
                        Self::deposit_event(Event::ExtrinsicPostponed { index });
                        break;
                    }

                    IbeQueueDrainInProgress::<T>::put(true);
                    consumed = consumed.saturating_add(T::DbWeight::get().writes(1));
                    let applied = T::DecryptedExtrinsicExecutor::apply(inner);
                    IbeQueueDrainInProgress::<T>::kill();
                    consumed = consumed.saturating_add(T::DbWeight::get().writes(1));
                    consumed = consumed.saturating_add(applied.consumed_weight);
                    let _ = Self::remove_conditional_ibe_entry(index);
                    consumed = consumed.saturating_add(remove_weight);
                    if applied.success {
                        Self::refund_conditional_ibe_submission_deposit(index);
                    } else {
                        Self::forfeit_conditional_ibe_submission_deposit(index);
                    }
                    Self::deposit_event(Event::ConditionalIbeExecuted {
                        index,
                        success: applied.success,
                    });
                }
            }
        }
        consumed
    }
}
