//! Subnet-owner / root origin checks, admin freeze window, and owner rate-limit recording.
use super::*;
use crate::Error;
use crate::system::{ensure_signed, ensure_signed_or_root};
use subtensor_runtime_common::NetUid;

impl<T: Config> Pallet<T> {
    /// Allow root (`None`) or the [`SubnetOwner`] coldkey (`Some`) for `netuid`.
    pub fn ensure_subnet_owner_or_root(
        origin: OriginFor<T>,
        netuid: NetUid,
    ) -> Result<Option<T::AccountId>, DispatchError> {
        let coldkey = ensure_signed_or_root(origin);
        match coldkey {
            Ok(Some(who)) if SubnetOwner::<T>::get(netuid) == who => Ok(Some(who)),
            Ok(Some(_)) => Err(DispatchError::BadOrigin),
            Ok(None) => Ok(None),
            Err(x) => Err(x.into()),
        }
    }

    /// Require the signed origin to be [`SubnetOwner`] for `netuid`.
    pub fn ensure_subnet_owner(
        origin: OriginFor<T>,
        netuid: NetUid,
    ) -> Result<T::AccountId, DispatchError> {
        let coldkey = ensure_signed(origin);
        match coldkey {
            Ok(who) if SubnetOwner::<T>::get(netuid) == who => Ok(who),
            Ok(_) => Err(DispatchError::BadOrigin),
            Err(x) => Err(x.into()),
        }
    }

    /// Owner-or-root gate that also enforces each [`TransactionType`] rate limit for the owner.
    ///
    /// Root bypasses rate checks. Prefer renaming to `ensure_subnet_owner_or_root_with_limits`
    /// (see `refactor/rename-proposals.md`).
    pub fn ensure_subnet_owner_or_root_with_limits(
        origin: OriginFor<T>,
        netuid: NetUid,
        limits: &[crate::utils::rate_limiting::TransactionType],
    ) -> Result<Option<T::AccountId>, DispatchError> {
        let maybe_who = Self::ensure_subnet_owner_or_root(origin, netuid)?;
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

    /// Set the global admin-freeze window length in blocks (weights-submission quiet period).
    pub fn set_admin_freeze_window(window: u16) {
        AdminFreezeWindow::<T>::set(window);
        Self::deposit_event(Event::AdminFreezeWindowSet(window));
    }

    /// Set how many tempos an owner must wait between hyperparameter updates.
    pub fn set_owner_hyperparam_rate_limit(epochs: u16) {
        OwnerHyperparamRateLimit::<T>::set(epochs);
        Self::deposit_event(Event::OwnerHyperparamRateLimitSet(epochs));
    }

    /// If `maybe_owner` is `Some`, stamp `txs` last-block markers on `netuid` at the current block.
    ///
    /// Prefer renaming to `record_owner_rate_limits` (see `refactor/rename-proposals.md`).
    pub fn record_owner_rate_limits(
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
}
