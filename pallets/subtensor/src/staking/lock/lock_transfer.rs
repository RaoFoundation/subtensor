//! Move or transfer locks between hotkeys, auto-lock owner cuts, and wipe locks on network removal.
use super::*;
use frame_support::weights::WeightMeter;
use safe_math::FixedExt;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::NetUid;

impl<T: Config> Pallet<T> {
    /// Moves lock from one hotkey to another and clears conviction
    ///
    /// The lock is rolled forward to the current block before switching the
    /// associated hotkey so that the lock stays mathematically correct and
    /// preserves current decayed locked mass.
    ///
    /// The conviction is reset to zero if the destination and source hotkeys
    /// are owned by different coldkeys, otherwise it is preserved.
    pub fn do_move_lock(
        coldkey: &T::AccountId,
        destination_hotkey: &T::AccountId,
        netuid: NetUid,
    ) -> DispatchResult {
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        ensure!(
            Self::hotkey_account_exists(destination_hotkey),
            Error::<T>::HotKeyAccountNotExists
        );
        let now = Self::get_current_block_as_u64();

        match Self::read_conviction_model(coldkey, netuid, now) {
            Some((origin_hotkey, mut model)) => {
                let unlock_rate = UnlockRate::<T>::get();
                let maturity_rate = MaturityRate::<T>::get();
                model.roll_forward(now, unlock_rate, maturity_rate);
                let mut lock = model.individual_lock().clone();
                let removed = lock.clone();

                if !Self::conviction_survives_hotkey_change(&origin_hotkey, destination_hotkey) {
                    lock.conviction = U64F64::saturating_from_num(0);
                }
                lock = ConvictionModel::roll_forward_lock(
                    lock,
                    now,
                    unlock_rate,
                    maturity_rate,
                    Self::is_subnet_owner_hotkey(netuid, destination_hotkey),
                    Self::is_perpetual_lock(coldkey, netuid),
                )
                .0;

                Lock::<T>::remove((coldkey.clone(), netuid, origin_hotkey.clone()));
                Self::maybe_remove_locking_coldkey(&origin_hotkey, netuid, coldkey);
                Self::insert_lock_state(coldkey, netuid, destination_hotkey, lock.clone());
                Self::reduce_aggregate_lock(
                    coldkey,
                    &origin_hotkey,
                    netuid,
                    removed.locked_mass,
                    removed.conviction,
                );
                Self::add_aggregate_lock(coldkey, destination_hotkey, netuid, lock);

                Self::deposit_event(Event::LockMoved {
                    coldkey: coldkey.clone(),
                    origin_hotkey,
                    destination_hotkey: destination_hotkey.clone(),
                    netuid,
                });
                Ok(())
            }
            None => Err(Error::<T>::NoExistingLock.into()),
        }
    }

    pub fn auto_lock_owner_cut(netuid: NetUid, amount: AlphaBalance) {
        if !OwnerCutAutoLockEnabled::<T>::get(netuid) {
            return;
        }

        let subnet_owner_coldkey = Self::get_subnet_owner(netuid);

        // Determine the lock hotkey. If no locks exist, assign subnet owner's hotkey, otherwise
        // auto-lock to existing lock hotkey
        let lock_hotkey = if let Some((existing_hotkey, _model)) = Self::read_conviction_model(
            &subnet_owner_coldkey,
            netuid,
            Self::get_current_block_as_u64(),
        ) {
            existing_hotkey
        } else {
            SubnetOwnerHotkey::<T>::get(netuid)
        };

        // Ignore the result. It may only fail if amount is zero, which is OK to ignore because nothing
        // needs to happen in that case
        let _ = Self::do_lock_stake(&subnet_owner_coldkey, netuid, &lock_hotkey, amount);
    }

    /// When locked stake is transfered, the lock should follow the stake
    ///
    /// First, this function rolls the lock forward and checks if amount is over available
    /// stake and if it is, the stake that's over the available amount on the destination
    /// coldkey is locked in the same way as the original stake: the lock follows the stake
    /// to `destination_hotkey` (which, for plain stake transfers, is the same hotkey the
    /// stake was locked to). Conviction is moved proportionally to the moved locked amount
    /// of alpha. For example, if 20% of locked alpha is moved, then also 20% of conviction
    /// is moved. If the source and destination hotkeys are owned by different coldkeys,
    /// the moved conviction is reset to zero, mirroring `do_move_lock`.
    pub fn transfer_lock(
        origin_coldkey: &T::AccountId,
        destination_coldkey: &T::AccountId,
        destination_hotkey: &T::AccountId,
        netuid: NetUid,
        amount: AlphaBalance,
    ) -> DispatchResult {
        let now = Self::get_current_block_as_u64();

        // If no actual transfer happens, this is ok
        if origin_coldkey == destination_coldkey || amount.is_zero() {
            return Ok(());
        }

        // Read total alpha of the coldkey on this netuid. Do not check if total alpha is
        // lower than amount transferred, this is responsibility of a higher level, this
        // function needs to act protectively.
        let total_alpha = Self::total_coldkey_alpha_on_subnet(origin_coldkey, netuid);
        let mut remaining_to_transfer = amount;

        // Read the locks for source and destination coldkey (if exist) and roll forward
        let Some((source_hotkey, mut source_model)) =
            Self::read_conviction_model(origin_coldkey, netuid, now)
        else {
            return Ok(());
        };

        let unlock_rate = UnlockRate::<T>::get();
        let maturity_rate = MaturityRate::<T>::get();
        source_model.roll_forward(now, unlock_rate, maturity_rate);
        let mut source_lock = source_model.individual_lock().clone();
        let maybe_destination_lock = Self::read_conviction_model(destination_coldkey, netuid, now)
            .map(|(hotkey, mut model)| {
                model.roll_forward(now, unlock_rate, maturity_rate);
                (hotkey, model.individual_lock().clone())
            });

        let destination_lock_hotkey = maybe_destination_lock
            .as_ref()
            .map(|(hotkey, _)| hotkey.clone())
            .unwrap_or_else(|| destination_hotkey.clone());
        let mut destination_lock = maybe_destination_lock
            .as_ref()
            .map(|(_, lock)| lock.clone())
            .unwrap_or(LockState {
                locked_mass: AlphaBalance::ZERO,
                conviction: U64F64::saturating_from_num(0),
                last_update: now,
            });

        // Calculate available stake by subtracting locked_mass from total alpha.
        let unavailable = source_lock.locked_mass;
        let available_stake = total_alpha.saturating_sub(unavailable);

        // Reduce remaining_to_transfer by min(remaining_to_transfer, available stake)
        let available_transfer = remaining_to_transfer.min(available_stake);
        remaining_to_transfer = remaining_to_transfer.saturating_sub(available_transfer);

        // If result is non-zero, check the hotkey match between source and destination coldkey locks
        // (if destination coldkey lock exists). If no match, error out with LockHotkeyMismatch, otherwise,
        // reduce remaining_to_transfer by min(remaining_to_transfer, locked_mass), reduce locked_mass on
        // the source coldkey by the same amount, increase locked_mass on the destination coldkey by the
        // same amount, reduce conviction on the source coldkey proportionally, and increase conviction
        // on the destination coldkey proportionally.
        let mut locked_transfer = AlphaBalance::ZERO;
        let mut conviction_transfer = U64F64::saturating_from_num(0);
        let mut received_conviction = U64F64::saturating_from_num(0);
        if !remaining_to_transfer.is_zero() {
            if let Some((existing_hotkey, _)) = maybe_destination_lock.as_ref() {
                ensure!(
                    existing_hotkey == destination_hotkey,
                    Error::<T>::LockHotkeyMismatch
                );
            }

            locked_transfer = remaining_to_transfer.min(source_lock.locked_mass);
            conviction_transfer = if locked_transfer.is_zero() || source_lock.locked_mass.is_zero()
            {
                U64F64::saturating_from_num(0)
            } else {
                let locked_transfer = U64F64::saturating_from_num(locked_transfer.to_u64());
                let source_locked = U64F64::saturating_from_num(source_lock.locked_mass.to_u64());
                let transferred_proportion = locked_transfer.safe_div(source_locked);
                source_lock
                    .conviction
                    .saturating_mul(transferred_proportion)
            };

            // Conviction only follows the lock when the destination hotkey is owned
            // by the same coldkey as the source hotkey; otherwise it is forfeited,
            // mirroring `do_move_lock`.
            received_conviction = if Self::conviction_survives_hotkey_change(
                &source_hotkey,
                &destination_lock_hotkey,
            ) {
                conviction_transfer
            } else {
                U64F64::saturating_from_num(0)
            };

            source_lock.locked_mass = source_lock.locked_mass.saturating_sub(locked_transfer);
            source_lock.conviction = source_lock.conviction.saturating_sub(conviction_transfer);
            destination_lock.locked_mass =
                destination_lock.locked_mass.saturating_add(locked_transfer);
            destination_lock.conviction = destination_lock
                .conviction
                .saturating_add(received_conviction);
        }
        Self::ensure_can_receive_locked_alpha(destination_coldkey, locked_transfer)?;

        source_lock = ConvictionModel::roll_forward_lock(
            source_lock,
            now,
            unlock_rate,
            maturity_rate,
            Self::is_subnet_owner_hotkey(netuid, &source_hotkey),
            Self::is_perpetual_lock(origin_coldkey, netuid),
        )
        .0;
        destination_lock = ConvictionModel::roll_forward_lock(
            destination_lock,
            now,
            unlock_rate,
            maturity_rate,
            Self::is_subnet_owner_hotkey(netuid, &destination_lock_hotkey),
            Self::is_perpetual_lock(destination_coldkey, netuid),
        )
        .0;

        // Upsert updated locks (only once per this fn) even if there were no updates because
        // of roll-forward
        Self::insert_lock_state(origin_coldkey, netuid, &source_hotkey, source_lock);
        Self::insert_lock_state(
            destination_coldkey,
            netuid,
            &destination_lock_hotkey,
            destination_lock,
        );
        if !locked_transfer.is_zero() {
            Self::reduce_aggregate_lock(
                origin_coldkey,
                &source_hotkey,
                netuid,
                locked_transfer,
                conviction_transfer,
            );
            Self::add_aggregate_lock(
                destination_coldkey,
                &destination_lock_hotkey,
                netuid,
                LockState {
                    locked_mass: locked_transfer,
                    conviction: received_conviction,
                    last_update: now,
                },
            );
        }

        Ok(())
    }

    /// Removes `Lock` entries for `netuid`, resuming from `LastKeptRawKey` when weight is limited.
    pub fn remove_network_lock(
        netuid: NetUid,
        weight_meter: &mut WeightMeter,
        last_key: Option<Vec<u8>>,
    ) -> (bool, Option<Vec<u8>>) {
        let iter = match last_key {
            Some(key) => Lock::<T>::iter_from(key),
            None => Lock::<T>::iter(),
        };

        let (read_all, last_item) = Self::remove_storage_entries_for_netuid(
            weight_meter,
            iter,
            |((_, this_netuid, _), _)| *this_netuid == netuid,
            |((coldkey, _this_netuid, hotkey), _)| (coldkey, hotkey),
            |(coldkey, hotkey)| Lock::<T>::remove((coldkey.clone(), netuid, hotkey.clone())),
            1,
        );

        (
            read_all,
            last_item.map(|((coldkey, _, hotkey), _)| {
                Lock::<T>::hashed_key_for((&coldkey, netuid, &hotkey))
            }),
        )
    }

    /// Removes `DecayingLock` entries for `netuid`, resuming from `LastKeptRawKey` when weight is limited.
    pub fn remove_network_decaying_lock(
        netuid: NetUid,
        weight_meter: &mut WeightMeter,
        last_key: Option<Vec<u8>>,
    ) -> (bool, Option<Vec<u8>>) {
        let iter = match last_key {
            Some(raw_key) => DecayingLock::<T>::iter_from(raw_key),
            None => DecayingLock::<T>::iter(),
        };

        let (read_all, last_item) = Self::remove_storage_entries_for_netuid(
            weight_meter,
            iter,
            |(_, nu, _)| *nu == netuid,
            |(cold, nu, _)| (cold, nu),
            |(cold, netuid)| DecayingLock::<T>::remove(cold, netuid),
            1,
        );

        (
            read_all,
            last_item.map(|(cold, nu, _)| DecayingLock::<T>::hashed_key_for(&cold, nu)),
        )
    }
}
