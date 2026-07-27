//! Migrate conviction locks across coldkey or hotkey swaps.
use super::*;
use subtensor_runtime_common::NetUid;

impl<T: Config> Pallet<T> {
    /// Ensure the coldkey does not have an active lock on any subnets.
    pub fn ensure_no_active_locks(coldkey: &T::AccountId) -> Result<(), Error<T>> {
        let now = Self::get_current_block_as_u64();
        let unlock_rate = UnlockRate::<T>::get();
        let maturity_rate = MaturityRate::<T>::get();

        for ((netuid, hotkey), lock) in Lock::<T>::iter_prefix((coldkey,)) {
            let rolled = ConvictionModel::roll_forward_lock(
                lock,
                now,
                unlock_rate,
                maturity_rate,
                Self::is_subnet_owner_hotkey(netuid, &hotkey),
                Self::is_perpetual_lock(coldkey, netuid),
            );
            if rolled.0.locked_mass > AlphaBalance::ZERO {
                return Err(Error::<T>::ActiveLockExists);
            }
        }

        Ok(())
    }

    /// Transfers the lock from one coldkey to another for all subnets. This is used when a
    /// user swaps their coldkey and we want to preserve their locks.
    ///
    /// The hotkey and netuid remain the same, only the coldkey changes.
    ///
    /// The new coldkey must have no active locks, so we can transfer the locks
    /// "as is" without rolling them forward and the
    /// HotkeyLock map does not change (because it only contains totals, not individual coldkey locks).
    pub fn swap_coldkey_locks(
        old_coldkey: &T::AccountId,
        new_coldkey: &T::AccountId,
    ) -> DispatchResult {
        Self::ensure_no_active_locks(new_coldkey)?;

        let mut locks_to_transfer: Vec<(NetUid, T::AccountId, LockState)> = Vec::new();
        let now = Self::get_current_block_as_u64();
        let unlock_rate = UnlockRate::<T>::get();
        let maturity_rate = MaturityRate::<T>::get();
        let new_coldkey_rejects_locked_alpha = Self::account_rejects_locked_alpha(new_coldkey);
        let decaying_locks_to_transfer: Vec<(NetUid, bool)> =
            DecayingLock::<T>::iter_prefix(old_coldkey).collect();

        // Gather locks for old coldkey
        for ((netuid, hotkey), lock) in Lock::<T>::iter_prefix((old_coldkey,)) {
            locks_to_transfer.push((netuid, hotkey, lock));
        }

        let mut rolled_locks_to_transfer: Vec<(NetUid, T::AccountId, LockState, bool)> = Vec::new();
        for (netuid, hotkey, lock) in locks_to_transfer {
            let perpetual_lock = decaying_locks_to_transfer
                .iter()
                .any(|(decaying_netuid, decaying)| *decaying_netuid == netuid && !*decaying);
            let (old_lock, _) = ConvictionModel::roll_forward_lock(
                lock,
                now,
                unlock_rate,
                maturity_rate,
                Self::is_subnet_owner_hotkey(netuid, &hotkey),
                perpetual_lock,
            );
            Self::ensure_can_receive_locked_alpha_with_flag(
                new_coldkey_rejects_locked_alpha,
                old_lock.locked_mass,
            )?;
            rolled_locks_to_transfer.push((netuid, hotkey, old_lock, perpetual_lock));
        }

        // Remove old locks and reduce old aggregate buckets before moving the
        // perpetual-lock flags; aggregate selection depends on the old flag.
        for (netuid, hotkey, old_lock, _) in rolled_locks_to_transfer.iter() {
            Lock::<T>::remove((old_coldkey.clone(), *netuid, hotkey.clone()));
            Self::maybe_remove_locking_coldkey(hotkey, *netuid, old_coldkey);
            Self::reduce_aggregate_lock(
                old_coldkey,
                hotkey,
                *netuid,
                old_lock.locked_mass,
                old_lock.conviction,
            );
        }

        for (netuid, _) in decaying_locks_to_transfer {
            if let Some(decaying) = DecayingLock::<T>::take(old_coldkey, netuid) {
                DecayingLock::<T>::insert(new_coldkey, netuid, decaying);
            }
        }

        let flags = AccountFlags::<T>::get(old_coldkey);
        AccountFlags::<T>::remove(old_coldkey);
        if flags != 0 {
            AccountFlags::<T>::insert(new_coldkey, flags);
        } else {
            AccountFlags::<T>::remove(new_coldkey);
        }

        // Insert locks for the new coldkey and add to the destination aggregate
        // buckets after the flags have moved.
        for (netuid, hotkey, old_lock, perpetual_lock) in rolled_locks_to_transfer {
            let new_lock = ConvictionModel::roll_forward_lock(
                old_lock.clone(),
                now,
                unlock_rate,
                maturity_rate,
                Self::is_subnet_owner_hotkey(netuid, &hotkey),
                perpetual_lock,
            )
            .0;
            Self::insert_lock_state(new_coldkey, netuid, &hotkey, new_lock.clone());
            Self::add_aggregate_lock(new_coldkey, &hotkey, netuid, new_lock);
        }

        Ok(())
    }

    /// Swap all locks made to the old_hotkey to new_hotkey on all netuids
    ///
    /// There is no need to roll the locks, they can be just copied "as is":
    /// The lock relation between coldkeys and hotkey is 1:1, so if old hotkey has a
    /// coldkey locking to it, then the same coldkey cannot lock to the new hotkey.
    /// And in reverse: If a coldkey is locking to the new hotkey, it will not appear
    /// in the transfer list because it does not lock to the old hotkey.
    ///
    /// Conviction is not reset because the hotkey ownership does not change, it's still
    /// the same hotkey owner who will own the new hotkey.
    pub fn swap_hotkey_locks(old_hotkey: &T::AccountId, new_hotkey: &T::AccountId) -> (u64, u64) {
        Self::swap_hotkey_locks_for_netuids(old_hotkey, new_hotkey, Self::get_all_subnet_netuids())
    }

    /// Swap locks made to the old_hotkey to new_hotkey on one netuid.
    pub fn swap_hotkey_locks_on_subnet(
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
        netuid: NetUid,
    ) -> (u64, u64) {
        Self::swap_hotkey_locks_for_netuids(old_hotkey, new_hotkey, vec![netuid])
    }

    pub(crate) fn swap_hotkey_locks_for_netuids(
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
        netuids: Vec<NetUid>,
    ) -> (u64, u64) {
        let mut locks_to_transfer: Vec<(T::AccountId, NetUid, LockState)> = Vec::new();
        let mut netuids_to_transfer: Vec<(NetUid, bool, bool)> = Vec::new();
        let mut reads: u64 = 0;
        let mut writes: u64 = 0;

        for netuid in netuids.iter().copied() {
            let old_is_owner_hotkey = Self::is_subnet_owner_hotkey(netuid, old_hotkey);
            let new_is_owner_hotkey = Self::is_subnet_owner_hotkey(netuid, new_hotkey);
            let has_hotkey_lock = HotkeyLock::<T>::contains_key(netuid, old_hotkey);
            let has_decaying_hotkey_lock =
                DecayingHotkeyLock::<T>::contains_key(netuid, old_hotkey);
            let has_owner_lock = old_is_owner_hotkey && OwnerLock::<T>::contains_key(netuid);
            let has_decaying_owner_lock =
                old_is_owner_hotkey && DecayingOwnerLock::<T>::contains_key(netuid);

            if old_is_owner_hotkey
                || new_is_owner_hotkey
                || has_hotkey_lock
                || has_decaying_hotkey_lock
                || has_owner_lock
                || has_decaying_owner_lock
            {
                netuids_to_transfer.push((
                    netuid,
                    old_is_owner_hotkey,
                    old_is_owner_hotkey || new_is_owner_hotkey,
                ));
            }
            reads = reads.saturating_add(5);
        }

        // Build a concrete transfer list from the hotkey-to-coldkey index.
        // The index can contain stale coldkeys, so only locks that still exist
        // are carried forward; missing locks are pruned from the index.
        for (netuid, _, _) in &netuids_to_transfer {
            for (coldkey, _) in LockingColdkeys::<T>::iter_prefix((*netuid, old_hotkey)) {
                if let Some(lock) = Lock::<T>::get((coldkey.clone(), *netuid, old_hotkey.clone())) {
                    locks_to_transfer.push((coldkey, *netuid, lock));
                } else {
                    Self::maybe_remove_locking_coldkey(old_hotkey, *netuid, &coldkey);
                    writes = writes.saturating_add(1);
                }
                reads = reads.saturating_add(1);
            }
        }

        for (coldkey, netuid, lock) in locks_to_transfer {
            let now = Self::get_current_block_as_u64();
            let unlock_rate = UnlockRate::<T>::get();
            let maturity_rate = MaturityRate::<T>::get();
            let old_owner_lock = netuids_to_transfer
                .iter()
                .any(|(rebuild_netuid, is_owner, _)| *rebuild_netuid == netuid && *is_owner);
            let new_owner_lock = netuids_to_transfer
                .iter()
                .any(|(rebuild_netuid, _, is_owner)| *rebuild_netuid == netuid && *is_owner);
            let perpetual_lock = Self::is_perpetual_lock(&coldkey, netuid);
            let rolled = ConvictionModel::roll_forward_lock(
                lock,
                now,
                unlock_rate,
                maturity_rate,
                old_owner_lock,
                perpetual_lock,
            )
            .0;
            let moved = ConvictionModel::roll_forward_lock(
                rolled,
                now,
                unlock_rate,
                maturity_rate,
                new_owner_lock,
                perpetual_lock,
            )
            .0;
            Lock::<T>::remove((coldkey.clone(), netuid, old_hotkey.clone()));
            Self::maybe_remove_locking_coldkey(old_hotkey, netuid, &coldkey);
            Self::insert_lock_state(&coldkey, netuid, new_hotkey, moved);
            writes = writes.saturating_add(2);
        }

        for (netuid, old_was_owner, new_is_owner) in netuids_to_transfer {
            let now = Self::get_current_block_as_u64();
            let unlock_rate = UnlockRate::<T>::get();
            let maturity_rate = MaturityRate::<T>::get();
            let moved_perpetual_lock = if old_was_owner {
                OwnerLock::<T>::take(netuid).map(|lock| {
                    ConvictionModel::roll_forward_lock(
                        lock,
                        now,
                        unlock_rate,
                        maturity_rate,
                        true,
                        true,
                    )
                    .0
                })
            } else {
                HotkeyLock::<T>::take(netuid, old_hotkey).map(|lock| {
                    ConvictionModel::roll_forward_lock(
                        lock,
                        now,
                        unlock_rate,
                        maturity_rate,
                        false,
                        true,
                    )
                    .0
                })
            };
            let moved_decaying_lock = if old_was_owner {
                DecayingOwnerLock::<T>::take(netuid).map(|lock| {
                    ConvictionModel::roll_forward_lock(
                        lock,
                        now,
                        unlock_rate,
                        maturity_rate,
                        true,
                        false,
                    )
                    .0
                })
            } else {
                DecayingHotkeyLock::<T>::take(netuid, old_hotkey).map(|lock| {
                    ConvictionModel::roll_forward_lock(
                        lock,
                        now,
                        unlock_rate,
                        maturity_rate,
                        false,
                        false,
                    )
                    .0
                })
            };

            if let Some(lock) = moved_perpetual_lock {
                if new_is_owner {
                    Self::insert_owner_lock_state(
                        netuid,
                        ConvictionModel::roll_forward_lock(
                            lock,
                            now,
                            unlock_rate,
                            maturity_rate,
                            true,
                            true,
                        )
                        .0,
                    );
                } else {
                    Self::insert_hotkey_lock_state(
                        netuid,
                        new_hotkey,
                        ConvictionModel::roll_forward_lock(
                            lock,
                            now,
                            unlock_rate,
                            maturity_rate,
                            false,
                            true,
                        )
                        .0,
                    );
                }
            }
            if let Some(lock) = moved_decaying_lock {
                if new_is_owner {
                    Self::insert_decaying_owner_lock_state(
                        netuid,
                        ConvictionModel::roll_forward_lock(
                            lock,
                            now,
                            unlock_rate,
                            maturity_rate,
                            true,
                            false,
                        )
                        .0,
                    );
                } else {
                    Self::insert_decaying_hotkey_lock_state(
                        netuid,
                        new_hotkey,
                        ConvictionModel::roll_forward_lock(
                            lock,
                            now,
                            unlock_rate,
                            maturity_rate,
                            false,
                            false,
                        )
                        .0,
                    );
                }
            }
            writes = writes.saturating_add(6);
        }
        (reads, writes)
    }

    /// Conviction is only preserved when a lock moves between hotkeys owned by
    /// the same coldkey; moving it to a differently owned hotkey forfeits it.
    /// Shared by `do_move_lock` and `transfer_lock`.
    pub(crate) fn conviction_survives_hotkey_change(
        source_hotkey: &T::AccountId,
        destination_hotkey: &T::AccountId,
    ) -> bool {
        Self::get_owning_coldkey_for_hotkey(source_hotkey)
            == Self::get_owning_coldkey_for_hotkey(destination_hotkey)
    }
}
