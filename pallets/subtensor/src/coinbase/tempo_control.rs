use super::*;
use crate::Error;
use frame_support::pallet_prelude::DispatchResult;
use sp_runtime::DispatchError;
use subtensor_runtime_common::NetUid;

use crate::system::pallet_prelude::OriginFor;
use crate::utils::rate_limiting::{Hyperparameter, TransactionType};

impl<T: Config> Pallet<T> {
    /// Owner-side `set_tempo` implementation.
    pub fn do_set_tempo(origin: OriginFor<T>, netuid: NetUid, tempo: u16) -> DispatchResult {
        let who = Self::ensure_subnet_owner(origin, netuid)?;

        ensure!(
            (MIN_TEMPO..=MAX_TEMPO).contains(&tempo),
            Error::<T>::TempoOutOfBounds
        );

        Self::ensure_admin_window_open(netuid)?;

        let tx = TransactionType::TempoUpdate;
        ensure!(
            tx.passes_rate_limit_on_subnet::<T>(&who, netuid),
            Error::<T>::TxRateLimitExceeded
        );

        let now = Self::get_current_block_as_u64();

        Self::apply_tempo_with_cycle_reset(netuid, tempo);

        tx.set_last_block_on_subnet::<T>(&who, netuid, now);
        Ok(())
    }

    /// `set_activity_cutoff_factor` implementation. Callable by the subnet owner
    /// (subject to admin freeze window + rate limit) or by root (bypasses both).
    pub fn do_set_activity_cutoff_factor(
        origin: OriginFor<T>,
        netuid: NetUid,
        factor_milli: u32,
    ) -> DispatchResult {
        let maybe_who = Self::ensure_subnet_owner_or_root(origin, netuid)?;

        ensure!(
            (MIN_ACTIVITY_CUTOFF_FACTOR_MILLI..=MAX_ACTIVITY_CUTOFF_FACTOR_MILLI)
                .contains(&factor_milli),
            Error::<T>::ActivityCutoffFactorMilliOutOfBounds
        );

        let tx = TransactionType::OwnerHyperparamUpdate(Hyperparameter::ActivityCutoffFactorMilli);

        // Admin freeze window and per-owner rate limit apply only to the subnet
        // owner. Root bypasses both as a governance override.
        if let Some(who) = maybe_who.as_ref() {
            Self::ensure_admin_window_open(netuid)?;
            ensure!(
                tx.passes_rate_limit_on_subnet::<T>(who, netuid),
                Error::<T>::TxRateLimitExceeded
            );
        }

        Self::set_activity_cutoff_factor_milli(netuid, factor_milli);

        if let Some(who) = maybe_who.as_ref() {
            let now = Self::get_current_block_as_u64();
            tx.set_last_block_on_subnet::<T>(who, netuid, now);
        }

        Ok(())
    }

    /// Owner-side `trigger_epoch` implementation.
    /// Schedules the triggered epoch to fire after `AdminFreezeWindow` blocks; that
    /// countdown engages the freeze window for the subnet via `is_in_admin_freeze_window`.
    pub fn do_trigger_epoch(origin: OriginFor<T>, netuid: NetUid) -> Result<(), DispatchError> {
        let who = Self::ensure_subnet_owner(origin, netuid)?;

        // Block triggering to avoid breaking CRv3 reveal
        ensure!(
            !Self::get_commit_reveal_weights_enabled(netuid),
            Error::<T>::DynamicTempoBlockedByCommitReveal
        );

        // No `ensure_admin_window_open` here: trigger *defines* the next epoch.
        ensure!(
            PendingEpochAt::<T>::get(netuid) == 0,
            Error::<T>::EpochTriggerAlreadyPending
        );

        let now = Self::get_current_block_as_u64();
        let window = AdminFreezeWindow::<T>::get() as u64;

        let tempo = Self::get_tempo(netuid);
        let remaining = Self::blocks_until_next_auto_epoch(netuid, tempo, now);
        ensure!(remaining >= window, Error::<T>::AutoEpochAlreadyImminent);

        let tx = TransactionType::OwnerHyperparamUpdate(Hyperparameter::TriggerEpoch);
        ensure!(
            tx.passes_rate_limit_on_subnet::<T>(&who, netuid),
            Error::<T>::TxRateLimitExceeded
        );

        let fires_at = now.saturating_add(window);

        PendingEpochAt::<T>::insert(netuid, fires_at);
        tx.set_last_block_on_subnet::<T>(&who, netuid, now);

        Self::deposit_event(Event::EpochTriggered {
            netuid,
            by: who,
            fires_at,
        });
        Ok(())
    }
}
