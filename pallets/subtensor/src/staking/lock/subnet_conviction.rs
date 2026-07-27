//! Aggregate conviction queries and subnet-king (highest-conviction) owner rotation.
use super::*;
use sp_std::collections::btree_map::BTreeMap;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::NetUid;

impl<T: Config> Pallet<T> {
    /// Returns the total conviction for a hotkey on a subnet,
    /// summed over all coldkeys that have locked to this hotkey.
    pub fn hotkey_conviction(hotkey: &T::AccountId, netuid: NetUid) -> U64F64 {
        let now = Self::get_current_block_as_u64();
        let unlock_rate = UnlockRate::<T>::get();
        let maturity_rate = MaturityRate::<T>::get();
        let perpetual_conviction = HotkeyLock::<T>::get(netuid, hotkey)
            .map(|lock| {
                ConvictionModel::roll_forward_lock(
                    lock,
                    now,
                    unlock_rate,
                    maturity_rate,
                    false,
                    true,
                )
                .0
                .conviction
            })
            .unwrap_or_else(|| U64F64::saturating_from_num(0));
        let decaying_conviction = DecayingHotkeyLock::<T>::get(netuid, hotkey)
            .map(|lock| {
                ConvictionModel::roll_forward_lock(
                    lock,
                    now,
                    unlock_rate,
                    maturity_rate,
                    false,
                    false,
                )
                .0
                .conviction
            })
            .unwrap_or_else(|| U64F64::saturating_from_num(0));
        let hotkey_conviction = perpetual_conviction.saturating_add(decaying_conviction);
        if hotkey == &SubnetOwnerHotkey::<T>::get(netuid) {
            let owner_conviction = OwnerLock::<T>::get(netuid)
                .map(|lock| {
                    ConvictionModel::roll_forward_lock(
                        lock,
                        now,
                        unlock_rate,
                        maturity_rate,
                        true,
                        true,
                    )
                    .0
                    .conviction
                })
                .unwrap_or_else(|| U64F64::saturating_from_num(0));
            let decaying_owner_conviction = DecayingOwnerLock::<T>::get(netuid)
                .map(|lock| {
                    ConvictionModel::roll_forward_lock(
                        lock,
                        now,
                        unlock_rate,
                        maturity_rate,
                        true,
                        false,
                    )
                    .0
                    .conviction
                })
                .unwrap_or_else(|| U64F64::saturating_from_num(0));
            hotkey_conviction
                .saturating_add(owner_conviction)
                .saturating_add(decaying_owner_conviction)
        } else {
            hotkey_conviction
        }
    }

    /// Returns total rolled aggregate conviction across all hotkey and owner locks on a subnet.
    pub fn get_total_conviction(netuid: NetUid) -> U64F64 {
        let now = Self::get_current_block_as_u64();
        let unlock_rate = UnlockRate::<T>::get();
        let maturity_rate = MaturityRate::<T>::get();
        let hotkey_conviction = HotkeyLock::<T>::iter_prefix(netuid)
            .map(|(_hotkey, lock)| {
                ConvictionModel::roll_forward_lock(
                    lock,
                    now,
                    unlock_rate,
                    maturity_rate,
                    false,
                    true,
                )
                .0
                .conviction
            })
            .fold(U64F64::saturating_from_num(0), |acc, conviction| {
                acc.saturating_add(conviction)
            });
        let decaying_hotkey_conviction = DecayingHotkeyLock::<T>::iter_prefix(netuid)
            .map(|(_hotkey, lock)| {
                ConvictionModel::roll_forward_lock(
                    lock,
                    now,
                    unlock_rate,
                    maturity_rate,
                    false,
                    false,
                )
                .0
                .conviction
            })
            .fold(U64F64::saturating_from_num(0), |acc, conviction| {
                acc.saturating_add(conviction)
            });
        let owner_conviction = OwnerLock::<T>::get(netuid)
            .map(|lock| {
                ConvictionModel::roll_forward_lock(
                    lock,
                    now,
                    unlock_rate,
                    maturity_rate,
                    true,
                    true,
                )
                .0
                .conviction
            })
            .unwrap_or_else(|| U64F64::saturating_from_num(0));
        let decaying_owner_conviction = DecayingOwnerLock::<T>::get(netuid)
            .map(|lock| {
                ConvictionModel::roll_forward_lock(
                    lock,
                    now,
                    unlock_rate,
                    maturity_rate,
                    true,
                    false,
                )
                .0
                .conviction
            })
            .unwrap_or_else(|| U64F64::saturating_from_num(0));

        hotkey_conviction
            .saturating_add(decaying_hotkey_conviction)
            .saturating_add(owner_conviction)
            .saturating_add(decaying_owner_conviction)
    }

    /// Finds the hotkey with the highest conviction on a given subnet.
    pub fn subnet_king(netuid: NetUid) -> Option<T::AccountId> {
        let now = Self::get_current_block_as_u64();
        let unlock_rate = UnlockRate::<T>::get();
        let maturity_rate = MaturityRate::<T>::get();
        let mut scores: BTreeMap<T::AccountId, U64F64> = BTreeMap::new();

        HotkeyLock::<T>::iter_prefix(netuid).for_each(|(hotkey, lock)| {
            let rolled = ConvictionModel::roll_forward_lock(
                lock,
                now,
                unlock_rate,
                maturity_rate,
                false,
                true,
            );
            let entry = scores
                .entry(hotkey)
                .or_insert_with(|| U64F64::saturating_from_num(0));
            *entry = entry.saturating_add(rolled.0.conviction);
        });
        DecayingHotkeyLock::<T>::iter_prefix(netuid).for_each(|(hotkey, lock)| {
            let rolled = ConvictionModel::roll_forward_lock(
                lock,
                now,
                unlock_rate,
                maturity_rate,
                false,
                false,
            );
            let entry = scores
                .entry(hotkey)
                .or_insert_with(|| U64F64::saturating_from_num(0));
            *entry = entry.saturating_add(rolled.0.conviction);
        });
        if let Some(lock) = OwnerLock::<T>::get(netuid) {
            let owner_hotkey = SubnetOwnerHotkey::<T>::get(netuid);
            let rolled = ConvictionModel::roll_forward_lock(
                lock,
                now,
                unlock_rate,
                maturity_rate,
                true,
                true,
            );
            let entry = scores
                .entry(owner_hotkey)
                .or_insert_with(|| U64F64::saturating_from_num(0));
            *entry = entry.saturating_add(rolled.0.conviction);
        }
        if let Some(lock) = DecayingOwnerLock::<T>::get(netuid) {
            let owner_hotkey = SubnetOwnerHotkey::<T>::get(netuid);
            let rolled = ConvictionModel::roll_forward_lock(
                lock,
                now,
                unlock_rate,
                maturity_rate,
                true,
                false,
            );
            let entry = scores
                .entry(owner_hotkey)
                .or_insert_with(|| U64F64::saturating_from_num(0));
            *entry = entry.saturating_add(rolled.0.conviction);
        }

        scores
            .into_iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(core::cmp::Ordering::Equal))
            .map(|(hotkey, _)| hotkey)
    }

    /// Reassigns subnet ownership to the current lock-conviction leader when the subnet
    /// is mature enough and enough conviction has accumulated.
    ///
    /// Ownership can change only after the subnet is at least [`ONE_YEAR`] old and the
    /// total rolled aggregate conviction on the subnet is at least 10% of `SubnetAlphaOut`.
    /// If those gates pass, the hotkey with the highest rolled aggregate conviction
    /// becomes the subnet owner hotkey, and that hotkey's owning coldkey becomes the
    /// subnet owner coldkey. The new owner hotkey's conviction is then progressed to
    /// its current locked mass so the new owner starts with full owner conviction.
    pub fn change_subnet_owner_if_needed(netuid: NetUid) {
        // No outstanding alpha means there is no meaningful 10% conviction threshold.
        let subnet_alpha_out = SubnetAlphaOut::<T>::get(netuid);
        if subnet_alpha_out.is_zero() {
            return;
        }

        // Ownership can only be reassigned after the subnet has aged for one year.
        let now = Self::get_current_block_as_u64();
        let registered_at = NetworkRegisteredAt::<T>::get(netuid);
        if now < registered_at.saturating_add(ONE_YEAR) {
            return;
        }

        // Require total rolled aggregate conviction to be at least 10% of subnet alpha out.
        let total_conviction = Self::get_total_conviction(netuid);
        if total_conviction.saturating_mul(U64F64::saturating_from_num(10))
            < U64F64::saturating_from_num(u64::from(subnet_alpha_out))
        {
            return;
        }

        // Pick the hotkey with the highest rolled aggregate conviction.
        let Some(king_hotkey) = Self::subnet_king(netuid) else {
            return;
        };

        // The king hotkey must resolve to a real coldkey owner.
        let new_owner_coldkey = Self::get_owning_coldkey_for_hotkey(&king_hotkey);
        if new_owner_coldkey == DefaultAccount::<T>::get() {
            return;
        }

        // If the winning hotkey already belongs to the current owner, nothing changes.
        let current_owner_coldkey = SubnetOwner::<T>::get(netuid);
        if new_owner_coldkey == current_owner_coldkey {
            return;
        }
        let old_owner_hotkey = SubnetOwnerHotkey::<T>::get(netuid);
        let unlock_rate = UnlockRate::<T>::get();
        let maturity_rate = MaturityRate::<T>::get();

        // Register new owner as a neuron if not yet registered.
        if Self::get_uid_for_net_and_hotkey(netuid, &king_hotkey).is_err()
            && Self::register_neuron(netuid, &king_hotkey).is_err()
        {
            return;
        }

        // Move aggregate buckets using the hotkey's new role.
        if let Some(owner_lock) = OwnerLock::<T>::take(netuid) {
            let moved_owner_lock = ConvictionModel::roll_forward_lock(
                owner_lock,
                now,
                unlock_rate,
                maturity_rate,
                true,
                true,
            );
            let current = HotkeyLock::<T>::get(netuid, &old_owner_hotkey)
                .map(|lock| {
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
                .unwrap_or_else(|| Self::empty_lock(now));
            Self::insert_hotkey_lock_state(
                netuid,
                &old_owner_hotkey,
                LockState {
                    locked_mass: current
                        .locked_mass
                        .saturating_add(moved_owner_lock.0.locked_mass),
                    conviction: current
                        .conviction
                        .saturating_add(moved_owner_lock.0.conviction),
                    last_update: now,
                },
            );
        }
        if let Some(owner_lock) = DecayingOwnerLock::<T>::take(netuid) {
            let moved_owner_lock = ConvictionModel::roll_forward_lock(
                owner_lock,
                now,
                unlock_rate,
                maturity_rate,
                true,
                false,
            );
            let current = DecayingHotkeyLock::<T>::get(netuid, &old_owner_hotkey)
                .map(|lock| {
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
                .unwrap_or_else(|| Self::empty_lock(now));
            Self::insert_decaying_hotkey_lock_state(
                netuid,
                &old_owner_hotkey,
                LockState {
                    locked_mass: current
                        .locked_mass
                        .saturating_add(moved_owner_lock.0.locked_mass),
                    conviction: current
                        .conviction
                        .saturating_add(moved_owner_lock.0.conviction),
                    last_update: now,
                },
            );
        }
        if let Some(king_lock) = HotkeyLock::<T>::take(netuid, &king_hotkey) {
            let moved_king_lock = ConvictionModel::roll_forward_lock(
                king_lock,
                now,
                unlock_rate,
                maturity_rate,
                false,
                true,
            );
            let current = OwnerLock::<T>::get(netuid)
                .map(|lock| {
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
                .unwrap_or_else(|| Self::empty_lock(now));
            Self::insert_owner_lock_state(
                netuid,
                ConvictionModel::roll_forward_lock(
                    LockState {
                        locked_mass: current
                            .locked_mass
                            .saturating_add(moved_king_lock.0.locked_mass),
                        conviction: current
                            .conviction
                            .saturating_add(moved_king_lock.0.conviction),
                        last_update: now,
                    },
                    now,
                    unlock_rate,
                    maturity_rate,
                    true,
                    true,
                )
                .0,
            );
        }
        if let Some(king_lock) = DecayingHotkeyLock::<T>::take(netuid, &king_hotkey) {
            let moved_king_lock = ConvictionModel::roll_forward_lock(
                king_lock,
                now,
                unlock_rate,
                maturity_rate,
                false,
                false,
            );
            let current = DecayingOwnerLock::<T>::get(netuid)
                .map(|lock| {
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
                .unwrap_or_else(|| Self::empty_lock(now));
            Self::insert_decaying_owner_lock_state(
                netuid,
                ConvictionModel::roll_forward_lock(
                    LockState {
                        locked_mass: current
                            .locked_mass
                            .saturating_add(moved_king_lock.0.locked_mass),
                        conviction: current
                            .conviction
                            .saturating_add(moved_king_lock.0.conviction),
                        last_update: now,
                    },
                    now,
                    unlock_rate,
                    maturity_rate,
                    true,
                    false,
                )
                .0,
            );
        }

        // Reassign subnet owner coldkey and owner hotkey.
        SubnetOwner::<T>::insert(netuid, new_owner_coldkey.clone());
        SubnetOwnerHotkey::<T>::insert(netuid, king_hotkey.clone());
        Self::deposit_event(Event::SubnetOwnerChanged {
            netuid,
            old_coldkey: current_owner_coldkey,
            new_coldkey: new_owner_coldkey,
        });
    }
}
