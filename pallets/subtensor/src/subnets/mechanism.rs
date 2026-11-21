//! This file contains all tooling to work with sub-subnets
//!

use super::*;
use crate::epoch::run_epoch::EpochTerms;
use alloc::collections::BTreeMap;
use safe_math::*;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{AlphaBalance, MechId, NetUid, NetUidStorageIndex};

pub type LeaseId = u32;

pub type CurrencyOf<T> = <T as Config>::Currency;

pub type BalanceOf<T> =
    <CurrencyOf<T> as fungible::Inspect<<T as frame_system::Config>::AccountId>>::Balance;

/// Theoretical maximum of subnets on bittensor. This value is used in indexed
/// storage of epoch values for sub-subnets as
///
/// `storage_index = netuid + sub_id * GLOBAL_MAX_SUBNET_COUNT`
///
/// For sub_id = 0 this index results in netuid and provides backward compatibility
/// for subnets with default sub-subnet count of 1.
///
/// Changing this value will require a migration of all epoch maps.
///
pub const GLOBAL_MAX_SUBNET_COUNT: u16 = 4096;

// Theoretical maximum number of mechanisms per subnet
// GLOBAL_MAX_SUBNET_COUNT * MAX_MECHANISM_COUNT_PER_SUBNET should be 0x10000
pub const MAX_MECHANISM_COUNT_PER_SUBNET: u8 = 16;

/// UID capacity for each Null mechanism.
pub const NULL_UID_BUDGET: u16 = 2_500;
/// Total row capacity shared across a Null subnet's mechanisms. A single
/// mechanism retains the full existing Null row limit.
pub const TOTAL_NULL_WEIGHT_BUDGET: u16 = NULL_UID_BUDGET;

impl<T: Config> Pallet<T> {
    /// Queue a grant move with fixed-size storage writes. The old grant is
    /// removed immediately; a later overlapping move supersedes the old edge.
    pub fn queue_mech_weight_delegate_move(
        netuid: NetUid,
        mecid: MechId,
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
        delegate: Option<T::AccountId>,
    ) {
        let index = Self::get_mechanism_storage_index(netuid, mecid);
        if MechWeightDelegateRetirementQueue::<T>::contains_key(netuid, mecid) {
            MechWeightDelegate::<T>::remove((index, old_hotkey));
            MechWeightDelegateTargets::<T>::remove((netuid, old_hotkey, mecid));
            MechWeightDelegate::<T>::remove((index, new_hotkey));
            MechWeightDelegateTargets::<T>::remove((netuid, new_hotkey, mecid));
            MechWeightDelegatePurgeQueue::<T>::insert(netuid, old_hotkey, None::<u16>);
            MechWeightDelegatePurgeQueue::<T>::insert(netuid, new_hotkey, None::<u16>);
            return;
        }
        // Capture a live grant for an ordinary move. Compaction replaces this
        // with the pending grant below, so a historical source's independent
        // re-registration cannot supply a delegate to the old lineage.
        let mut delegate = delegate.or_else(|| MechWeightDelegate::<T>::get((index, old_hotkey)));
        let mut uid = Uids::<T>::get(netuid, new_hotkey);
        let mut source = old_hotkey.clone();
        // Compact A→B followed by B→C into A→C. The original UID proves that
        // the destination owns the same registration, even if B has been used
        // again by the time bounded idle work runs.
        if let Some((incoming, _)) =
            MechWeightDelegateMoveOrigins::<T>::iter_key_prefix((netuid, old_hotkey))
                .find(|(_, edge_mecid)| *edge_mecid == mecid)
            && let Some(pending) = MechWeightDelegateMoveQueue::<T>::get((netuid, &incoming, mecid))
            && Uids::<T>::get(netuid, new_hotkey) == Some(pending.uid)
        {
            let Some(pending) = MechWeightDelegateMoveQueue::<T>::take((netuid, &incoming, mecid))
            else {
                return;
            };
            if incoming == *new_hotkey {
                let pending_delegate = pending.delegate.or(delegate);
                let owner = if Uids::<T>::get(netuid, old_hotkey) == Some(pending.uid) {
                    old_hotkey
                } else {
                    new_hotkey
                };
                if Uids::<T>::get(netuid, owner) == Some(pending.uid) {
                    MechWeightDelegate::<T>::remove((index, owner));
                    MechWeightDelegateTargets::<T>::remove((netuid, owner, mecid));
                    if let Some(delegate) = pending_delegate {
                        MechWeightDelegate::<T>::insert((index, owner), delegate);
                        MechWeightDelegateTargets::<T>::insert((netuid, owner, mecid), ());
                    }
                } else if Uids::<T>::get(netuid, &incoming) == Some(pending.uid)
                    && pending_delegate.is_some()
                {
                    // The old source has a new, independent UID. The pending
                    // grant still belongs to the destination holding the
                    // original UID and must not be discarded by this rename.
                    MechWeightDelegate::<T>::remove((index, old_hotkey));
                    MechWeightDelegateTargets::<T>::remove((netuid, old_hotkey, mecid));
                    MechWeightDelegateMoveQueue::<T>::insert(
                        (netuid, &incoming, mecid),
                        PendingMechWeightDelegateMove::<T> {
                            uid: pending.uid,
                            destination: incoming.clone(),
                            delegate: pending_delegate,
                        },
                    );
                    MechWeightDelegateMechanismMoves::<T>::insert((netuid, mecid, &incoming), ());
                } else {
                    // The pending destination no longer owns the original UID.
                    // Drop only this mechanism's grant and metadata.
                    MechWeightDelegate::<T>::remove((index, &incoming));
                    MechWeightDelegateTargets::<T>::remove((netuid, &incoming, mecid));
                    MechWeightDelegate::<T>::remove((index, old_hotkey));
                    MechWeightDelegateTargets::<T>::remove((netuid, old_hotkey, mecid));
                    MechWeightDelegateMoveOrigins::<T>::remove((
                        netuid, old_hotkey, &incoming, mecid,
                    ));
                }
                MechWeightDelegateMoveOrigins::<T>::remove((netuid, old_hotkey, &incoming, mecid));
                MechWeightDelegateMechanismMoves::<T>::remove((netuid, mecid, &incoming));
                return;
            }
            uid = Some(pending.uid);
            delegate = pending.delegate.or(delegate);
            source = incoming;
            MechWeightDelegateMoveOrigins::<T>::remove((netuid, old_hotkey, &source, mecid));
            MechWeightDelegateMechanismMoves::<T>::remove((netuid, mecid, &source));
            let purged_uid = MechWeightDelegatePurgeQueue::<T>::get(netuid, &source).flatten();
            let current_uid = Uids::<T>::get(netuid, &source);
            let purge_current =
                purged_uid.is_none_or(|uid| current_uid.is_none_or(|current| current == uid));
            if purge_current && current_uid != Some(pending.uid) {
                MechWeightDelegate::<T>::remove((index, &source));
                MechWeightDelegateTargets::<T>::remove((netuid, &source, mecid));
            }
        }
        if let Some(previous) = MechWeightDelegateMoveQueue::<T>::take((netuid, old_hotkey, mecid))
        {
            delegate = delegate.or(previous.delegate);
            MechWeightDelegateMoveOrigins::<T>::remove((
                netuid,
                previous.destination,
                old_hotkey,
                mecid,
            ));
            MechWeightDelegateMechanismMoves::<T>::remove((netuid, mecid, old_hotkey));
        }
        if let Some(uid) = uid {
            if delegate.is_none() {
                // A mechanism with no grant has no move to schedule. In
                // particular, a reverse rename on that mechanism must not
                // cancel a valid grant on another mechanism.
                MechWeightDelegate::<T>::remove((index, old_hotkey));
                MechWeightDelegateTargets::<T>::remove((netuid, old_hotkey, mecid));
                return;
            }
            MechWeightDelegate::<T>::remove((index, old_hotkey));
            MechWeightDelegateTargets::<T>::remove((netuid, old_hotkey, mecid));
            MechWeightDelegateMoveQueue::<T>::insert(
                (netuid, &source, mecid),
                PendingMechWeightDelegateMove::<T> {
                    uid,
                    destination: new_hotkey.clone(),
                    delegate,
                },
            );
            MechWeightDelegateMoveOrigins::<T>::insert((netuid, new_hotkey, &source, mecid), ());
            MechWeightDelegateMechanismMoves::<T>::insert((netuid, mecid, &source), ());
        }
    }

    pub fn purge_mech_weight_delegates(netuid: NetUid, hotkey: &T::AccountId, uid: Option<u16>) {
        MechWeightDelegatePurgeQueue::<T>::insert(netuid, hotkey, uid);
    }

    pub fn purge_mechanism_weight_delegates(netuid: NetUid, mecid: MechId) {
        MechWeightDelegateRetirementQueue::<T>::insert(netuid, mecid, ());
    }

    pub fn mech_weight_delegate_lifecycle_pending(netuid: NetUid, hotkey: &T::AccountId) -> bool {
        MechWeightDelegatePurgeQueue::<T>::contains_key(netuid, hotkey)
            || MechWeightDelegateMoveQueue::<T>::iter_key_prefix((netuid, hotkey))
                .next()
                .is_some()
            || MechWeightDelegateMoveOrigins::<T>::iter_key_prefix((netuid, hotkey))
                .next()
                .is_some()
    }

    /// Weight of one bounded delegate move, cancellation, or deletion step.
    pub fn mech_weight_delegate_lifecycle_step_weight() -> Weight {
        // The purge admission scan checks at most one bounded queue per possible
        // mechanism, reading the queue row and its destination UID each time.
        T::DbWeight::get().reads_writes(
            4u64.saturating_add(2u64.saturating_mul(u64::from(MAX_MECHANISM_COUNT_PER_SUBNET))),
            9,
        )
    }

    /// One bounded grant move, edge cancellation, or deletion per step.
    pub fn drain_mech_weight_delegate_lifecycle(weight_meter: &mut WeightMeter) {
        let item_weight = Self::mech_weight_delegate_lifecycle_step_weight();
        for _ in 0..256u32 {
            if !weight_meter.can_consume(item_weight) {
                return;
            }
            if let Some((netuid, mecid)) =
                MechWeightDelegateRetirementQueue::<T>::iter_keys().next()
            {
                let index = Self::get_mechanism_storage_index(netuid, mecid);
                if let Some(old_hotkey) =
                    MechWeightDelegateMechanismMoves::<T>::iter_key_prefix((netuid, mecid)).next()
                {
                    if let Some(pending) =
                        MechWeightDelegateMoveQueue::<T>::take((netuid, &old_hotkey, mecid))
                    {
                        MechWeightDelegateMoveOrigins::<T>::remove((
                            netuid,
                            pending.destination,
                            &old_hotkey,
                            mecid,
                        ));
                    }
                    MechWeightDelegateTargets::<T>::remove((netuid, &old_hotkey, mecid));
                    MechWeightDelegateMechanismMoves::<T>::remove((netuid, mecid, &old_hotkey));
                    MechWeightDelegate::<T>::remove((index, &old_hotkey));
                    weight_meter.consume(item_weight);
                    continue;
                }
                if let Some(hotkey) = MechWeightDelegate::<T>::iter_key_prefix((index,)).next() {
                    MechWeightDelegate::<T>::remove((index, &hotkey));
                    MechWeightDelegateTargets::<T>::remove((netuid, &hotkey, mecid));
                    weight_meter.consume(item_weight);
                    continue;
                }
                MechWeightDelegateRetirementQueue::<T>::remove(netuid, mecid);
                weight_meter.consume(item_weight);
                continue;
            }
            if let Some((netuid, hotkey)) = MechWeightDelegatePurgeQueue::<T>::iter_keys().next() {
                let purged_uid = MechWeightDelegatePurgeQueue::<T>::get(netuid, &hotkey).flatten();
                let current_uid = Uids::<T>::get(netuid, &hotkey);
                let purge_current =
                    purged_uid.is_none_or(|uid| current_uid.is_none_or(|current| current == uid));
                let valid_move = MechWeightDelegateMoveQueue::<T>::iter_key_prefix((
                    netuid, &hotkey,
                ))
                .find_map(|mecid| {
                    let pending = MechWeightDelegateMoveQueue::<T>::get((netuid, &hotkey, mecid))?;
                    (Uids::<T>::get(netuid, &pending.destination) == Some(pending.uid))
                        .then_some((mecid, pending))
                });
                if let Some((mecid, pending)) = valid_move {
                    let index = Self::get_mechanism_storage_index(netuid, mecid);
                    let source_owns_uid =
                        purge_current && Uids::<T>::get(netuid, &hotkey) == Some(pending.uid);
                    if let Some(delegate) = pending.delegate {
                        MechWeightDelegate::<T>::insert((index, &pending.destination), delegate);
                        MechWeightDelegateTargets::<T>::insert(
                            (netuid, &pending.destination, mecid),
                            (),
                        );
                    }
                    if source_owns_uid && pending.destination != hotkey {
                        MechWeightDelegate::<T>::remove((index, &hotkey));
                        MechWeightDelegateTargets::<T>::remove((netuid, &hotkey, mecid));
                    }
                    if source_owns_uid && Uids::<T>::get(netuid, &pending.destination).is_none() {
                        MechWeightDelegate::<T>::remove((index, &pending.destination));
                        MechWeightDelegateTargets::<T>::remove((
                            netuid,
                            &pending.destination,
                            mecid,
                        ));
                    }
                    MechWeightDelegateMoveQueue::<T>::remove((netuid, &hotkey, mecid));
                    MechWeightDelegateMoveOrigins::<T>::remove((
                        netuid,
                        pending.destination,
                        &hotkey,
                        mecid,
                    ));
                    MechWeightDelegateMechanismMoves::<T>::remove((netuid, mecid, &hotkey));
                    weight_meter.consume(item_weight);
                    continue;
                }
                if let Some((old_hotkey, mecid)) =
                    MechWeightDelegateMoveOrigins::<T>::iter_key_prefix((netuid, &hotkey)).next()
                {
                    let index = Self::get_mechanism_storage_index(netuid, mecid);
                    let incoming_valid =
                        MechWeightDelegateMoveQueue::<T>::get((netuid, &old_hotkey, mecid))
                            .is_some_and(|pending| {
                                Uids::<T>::get(netuid, &hotkey) == Some(pending.uid)
                            });
                    if !purge_current && incoming_valid {
                        // This tombstone belongs to a previous registration. Its
                        // independent destination still owns the pending lineage;
                        // leave that edge for the ordinary mover and finish here.
                        MechWeightDelegatePurgeQueue::<T>::remove(netuid, &hotkey);
                        weight_meter.consume(item_weight);
                        continue;
                    }
                    if let Some(pending) =
                        MechWeightDelegateMoveQueue::<T>::take((netuid, &old_hotkey, mecid))
                    {
                        MechWeightDelegateMechanismMoves::<T>::remove((netuid, mecid, &old_hotkey));
                        // Do not replace an existing direct or newer UID-scoped
                        // purge with this older lineage.
                        if !MechWeightDelegatePurgeQueue::<T>::contains_key(netuid, &old_hotkey) {
                            MechWeightDelegatePurgeQueue::<T>::insert(
                                netuid,
                                &old_hotkey,
                                Some(pending.uid),
                            );
                        }
                        if Uids::<T>::get(netuid, &old_hotkey) != Some(pending.uid) {
                            MechWeightDelegate::<T>::remove((index, &old_hotkey));
                            MechWeightDelegateTargets::<T>::remove((netuid, &old_hotkey, mecid));
                        }
                    }
                    MechWeightDelegateMoveOrigins::<T>::remove((
                        netuid,
                        &hotkey,
                        &old_hotkey,
                        mecid,
                    ));
                    if Uids::<T>::get(netuid, &hotkey).is_none() {
                        MechWeightDelegateTargets::<T>::remove((netuid, &hotkey, mecid));
                        MechWeightDelegate::<T>::remove((index, &hotkey));
                    }
                    weight_meter.consume(item_weight);
                    continue;
                }
                if let Some(mecid) =
                    MechWeightDelegateMoveQueue::<T>::iter_key_prefix((netuid, &hotkey)).next()
                {
                    let index = Self::get_mechanism_storage_index(netuid, mecid);
                    if let Some(pending) =
                        MechWeightDelegateMoveQueue::<T>::take((netuid, &hotkey, mecid))
                    {
                        let destination_invalid =
                            Uids::<T>::get(netuid, &pending.destination) != Some(pending.uid);
                        let source_owns_uid =
                            purge_current && Uids::<T>::get(netuid, &hotkey) == Some(pending.uid);
                        if !source_owns_uid && purge_current {
                            MechWeightDelegate::<T>::remove((index, &hotkey));
                            MechWeightDelegateTargets::<T>::remove((netuid, &hotkey, mecid));
                        }
                        if Uids::<T>::get(netuid, &pending.destination).is_none() {
                            MechWeightDelegate::<T>::remove((index, &pending.destination));
                            MechWeightDelegateTargets::<T>::remove((
                                netuid,
                                &pending.destination,
                                mecid,
                            ));
                        }
                        MechWeightDelegateMoveOrigins::<T>::remove((
                            netuid,
                            &pending.destination,
                            &hotkey,
                            mecid,
                        ));
                        MechWeightDelegateMechanismMoves::<T>::remove((netuid, mecid, &hotkey));
                        if destination_invalid {
                            // Do not downgrade an existing direct purge to a
                            // lineage UID. Otherwise an independently registered
                            // destination could retain a stale grant.
                            if !MechWeightDelegatePurgeQueue::<T>::contains_key(
                                netuid,
                                &pending.destination,
                            ) {
                                MechWeightDelegatePurgeQueue::<T>::insert(
                                    netuid,
                                    &pending.destination,
                                    Some(pending.uid),
                                );
                            }
                        }
                    }
                    weight_meter.consume(item_weight);
                    continue;
                }
                if let Some(mecid) =
                    MechWeightDelegateTargets::<T>::iter_key_prefix((netuid, &hotkey)).next()
                {
                    let index = Self::get_mechanism_storage_index(netuid, mecid);
                    if purge_current {
                        MechWeightDelegate::<T>::remove((index, &hotkey));
                        MechWeightDelegateTargets::<T>::remove((netuid, &hotkey, mecid));
                    } else {
                        // This hotkey has a different, independent registration.
                        // Preserve its grant and index, but finish the obsolete
                        // purge instead of spinning on immutable target metadata.
                        MechWeightDelegatePurgeQueue::<T>::remove(netuid, &hotkey);
                    }
                    weight_meter.consume(item_weight);
                    continue;
                }
                MechWeightDelegatePurgeQueue::<T>::remove(netuid, &hotkey);
                weight_meter.consume(item_weight);
                continue;
            }
            let Some(((netuid, source, mecid), pending)) =
                MechWeightDelegateMoveQueue::<T>::iter().next()
            else {
                return;
            };
            let index = Self::get_mechanism_storage_index(netuid, mecid);
            let destination = pending.destination;
            let destination_valid = Uids::<T>::get(netuid, &destination) == Some(pending.uid);
            if destination_valid && let Some(delegate) = pending.delegate {
                MechWeightDelegate::<T>::insert((index, &destination), delegate);
                MechWeightDelegateTargets::<T>::insert((netuid, &destination, mecid), ());
            }
            // Completed moves retain no history: old hotkeys may later be
            // registered independently and must never inherit this lineage.
            let purged_uid = MechWeightDelegatePurgeQueue::<T>::get(netuid, &source).flatten();
            let current_uid = Uids::<T>::get(netuid, &source);
            let source_is_stale =
                purged_uid.is_none_or(|uid| current_uid.is_none_or(|current| current == uid));
            if source_is_stale && current_uid != Some(pending.uid) {
                MechWeightDelegateTargets::<T>::remove((netuid, &source, mecid));
                MechWeightDelegate::<T>::remove((index, &source));
            }
            if !destination_valid {
                if Uids::<T>::get(netuid, &destination).is_none() {
                    MechWeightDelegateTargets::<T>::remove((netuid, &destination, mecid));
                    MechWeightDelegate::<T>::remove((index, &destination));
                }
                // Propagate the invalidated lineage without replacing an existing
                // direct or newer purge. Its own registration determines whether
                // the destination's residual grant must be removed.
                if !MechWeightDelegatePurgeQueue::<T>::contains_key(netuid, &destination) {
                    MechWeightDelegatePurgeQueue::<T>::insert(
                        netuid,
                        &destination,
                        Some(pending.uid),
                    );
                }
            }
            MechWeightDelegateMoveOrigins::<T>::remove((netuid, destination, &source, mecid));
            MechWeightDelegateMoveQueue::<T>::remove((netuid, &source, mecid));
            MechWeightDelegateMechanismMoves::<T>::remove((netuid, mecid, &source));
            weight_meter.consume(item_weight);
        }
    }

    pub fn get_mechanism_storage_index(netuid: NetUid, sub_id: MechId) -> NetUidStorageIndex {
        u16::from(sub_id)
            .saturating_mul(GLOBAL_MAX_SUBNET_COUNT)
            .saturating_add(u16::from(netuid))
            .into()
    }

    pub fn get_netuid(netuid_index: NetUidStorageIndex) -> NetUid {
        if let Some(netuid) = u16::from(netuid_index).checked_rem(GLOBAL_MAX_SUBNET_COUNT) {
            NetUid::from(netuid)
        } else {
            // Because GLOBAL_MAX_SUBNET_COUNT is not zero, this never happens
            NetUid::ROOT
        }
    }

    pub fn get_netuid_and_subid(
        netuid_index: NetUidStorageIndex,
    ) -> Result<(NetUid, MechId), Error<T>> {
        let maybe_netuid = u16::from(netuid_index).checked_rem(GLOBAL_MAX_SUBNET_COUNT);
        if let Some(netuid_u16) = maybe_netuid {
            let netuid = NetUid::from(netuid_u16);

            // Make sure the base subnet exists
            ensure!(
                Self::if_subnet_exist(netuid),
                Error::<T>::MechanismDoesNotExist
            );

            // Extract sub_id
            let sub_id_u8 = u8::try_from(u16::from(netuid_index).safe_div(GLOBAL_MAX_SUBNET_COUNT))
                .map_err(|_| Error::<T>::MechanismDoesNotExist)?;
            let sub_id = MechId::from(sub_id_u8);

            if MechanismCountCurrent::<T>::get(netuid) > sub_id {
                Ok((netuid, sub_id))
            } else {
                Err(Error::<T>::MechanismDoesNotExist.into())
            }
        } else {
            Err(Error::<T>::MechanismDoesNotExist.into())
        }
    }

    pub fn get_current_mechanism_count(netuid: NetUid) -> MechId {
        MechanismCountCurrent::<T>::get(netuid)
    }

    pub fn ensure_mechanism_exists(netuid: NetUid, sub_id: MechId) -> DispatchResult {
        // Make sure the base subnet exists
        ensure!(
            Self::if_subnet_exist(netuid),
            Error::<T>::MechanismDoesNotExist
        );

        // Make sure the mechanism limit is not exceeded
        ensure!(
            MechanismCountCurrent::<T>::get(netuid) > sub_id,
            Error::<T>::MechanismDoesNotExist
        );
        Ok(())
    }

    pub fn ensure_max_uids_over_all_mechanisms(
        netuid: NetUid,
        max_uids: u16,
        mechanism_count: MechId,
    ) -> DispatchResult {
        let mode = Self::get_epoch_consensus(netuid);
        let budget = Self::epoch_uid_budget(mode);
        let required_uids = match mode {
            EpochConsensus::Null => u32::from(max_uids),
            EpochConsensus::Yuma => {
                u32::from(max_uids).saturating_mul(u32::from(u8::from(mechanism_count)))
            }
        };
        ensure!(
            required_uids <= u32::from(budget),
            Error::<T>::TooManyUIDsPerMechanism
        );
        Ok(())
    }

    pub fn epoch_uid_budget(mode: EpochConsensus) -> u16 {
        match mode {
            EpochConsensus::Yuma => DefaultMaxAllowedUids::<T>::get(),
            EpochConsensus::Null => NULL_UID_BUDGET,
        }
    }

    /// Switch without pruning implicitly. Clamp capacity only after every check.
    pub fn do_set_epoch_consensus(netuid: NetUid, mode: EpochConsensus) -> DispatchResult {
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        // Root uses its separate dividend/basket mechanism, not subnet epochs.
        ensure!(!netuid.is_root(), Error::<T>::InvalidValue);
        // Do not import legacy queues outside Null's work bound or move large
        // pending Null reveals into Yuma. Probe keys without decoding values.
        if mode != Self::get_epoch_consensus(netuid) {
            for mechanism in 0..MAX_MECHANISM_COUNT_PER_SUBNET {
                let index = Self::get_mechanism_storage_index(netuid, mechanism.into());
                ensure!(
                    TimelockedWeightCommits::<T>::iter_key_prefix(index)
                        .next()
                        .is_none(),
                    Error::<T>::InvalidValue
                );
            }
        }
        let ceiling = match mode {
            EpochConsensus::Null => NULL_UID_BUDGET,
            EpochConsensus::Yuma => Self::epoch_uid_budget(mode)
                .checked_div(u16::from(u8::from(Self::get_current_mechanism_count(
                    netuid,
                ))))
                .ok_or(Error::<T>::InvalidValue)?,
        };
        ensure!(
            Self::get_subnetwork_n(netuid) <= ceiling,
            Error::<T>::TooManyUIDsPerMechanism
        );
        ensure!(
            Self::get_min_allowed_uids(netuid) <= ceiling,
            Error::<T>::TooManyUIDsPerMechanism
        );
        if Self::get_max_allowed_uids(netuid) > ceiling {
            Self::set_max_allowed_uids(netuid, ceiling);
        }
        if mode == EpochConsensus::Null
            && Self::get_epoch_consensus(netuid) == EpochConsensus::Yuma
            && !LastYumaStepBlock::<T>::contains_key(netuid)
        {
            LastYumaStepBlock::<T>::insert(netuid, LastMechansimStepBlock::<T>::get(netuid));
        }
        if mode == EpochConsensus::Null && mode != Self::get_epoch_consensus(netuid) {
            SavedYumaMaxAllowedValidators::<T>::insert(
                netuid,
                Self::get_max_allowed_validators(netuid),
            );
            Self::set_max_allowed_validators(netuid, 1);
            let (stakes, _, _) = Self::get_null_stake_weights_for_network(netuid);
            let winner = Self::null_validator_winner(&stakes);
            ValidatorPermit::<T>::insert(
                netuid,
                (0..stakes.len())
                    .map(|uid| Some(uid) == winner)
                    .collect::<Vec<_>>(),
            );
        }
        if mode != Self::get_epoch_consensus(netuid) {
            NullPruningTarget::<T>::remove(netuid);
            if mode == EpochConsensus::Yuma {
                let restored = SavedYumaMaxAllowedValidators::<T>::take(netuid)
                    .unwrap_or_else(DefaultMaxAllowedValidators::<T>::get)
                    .min(Self::get_max_allowed_uids(netuid));
                MaxAllowedValidators::<T>::insert(netuid, restored);
                Self::deposit_event(Event::MaxAllowedValidatorsSet(netuid, restored));
            }
        }
        Self::set_epoch_consensus(netuid, mode);
        Ok(())
    }

    /// Set the desired value of mechanism count for a subnet identified
    /// by netuid
    pub fn do_set_mechanism_count(netuid: NetUid, mechanism_count: MechId) -> DispatchResult {
        // Make sure the subnet exists
        ensure!(
            Self::if_subnet_exist(netuid),
            Error::<T>::MechanismDoesNotExist
        );

        // Count cannot be zero
        ensure!(mechanism_count > 0.into(), Error::<T>::InvalidValue);
        ensure!(
            pallet_commitments::ChannelMechanismPurgeQueue::<T>::iter_prefix(netuid)
                .next()
                .is_none()
                && MechWeightDelegateRetirementQueue::<T>::iter_prefix(netuid)
                    .next()
                    .is_none(),
            Error::<T>::InvalidValue
        );
        if Self::get_epoch_consensus(netuid) == EpochConsensus::Null
            && mechanism_count != Self::get_current_mechanism_count(netuid)
        {
            for mechanism in 0..MAX_MECHANISM_COUNT_PER_SUBNET {
                let index = Self::get_mechanism_storage_index(netuid, mechanism.into());
                ensure!(
                    TimelockedWeightCommits::<T>::iter_key_prefix(index)
                        .next()
                        .is_none(),
                    Error::<T>::InvalidValue
                );
                // Legacy hash commits can outlive UID churn. Do not clear an
                // unbounded orphan prefix inside the mechanism-count setter.
                ensure!(
                    WeightCommits::<T>::iter_key_prefix(index).next().is_none(),
                    Error::<T>::InvalidValue
                );
            }
        }

        // Make sure we are not exceeding the max sub-subnet count
        ensure!(
            mechanism_count <= MaxMechanismCount::<T>::get(),
            Error::<T>::InvalidValue
        );

        // Prevent chain bloat: Require max UIDs to be limited
        let max_uids = MaxAllowedUids::<T>::get(netuid);
        Self::ensure_max_uids_over_all_mechanisms(netuid, max_uids, mechanism_count)?;

        // Make sure we are not allowing numbers that will break the math
        ensure!(
            mechanism_count <= MechId::from(MAX_MECHANISM_COUNT_PER_SUBNET),
            Error::<T>::InvalidValue
        );

        Self::update_mechanism_counts_if_needed(netuid, mechanism_count);

        Ok(())
    }

    /// Set the global maximum number of mechanisms per subnet
    pub fn do_set_max_mechanism_count(max_mechanism_count: MechId) -> DispatchResult {
        // Max count cannot be zero
        ensure!(max_mechanism_count > 0.into(), Error::<T>::InvalidValue);

        // Make sure we are not allowing numbers that will break the math
        ensure!(
            max_mechanism_count <= MechId::from(MAX_MECHANISM_COUNT_PER_SUBNET),
            Error::<T>::InvalidValue
        );

        MaxMechanismCount::<T>::set(max_mechanism_count);

        Ok(())
    }

    /// Update current count for a subnet identified by netuid
    /// * Cleans up all sub-subnet maps if count is reduced
    ///
    pub fn update_mechanism_counts_if_needed(netuid: NetUid, new_count: MechId) {
        let old_count = u8::from(MechanismCountCurrent::<T>::get(netuid));
        let new_count_u8 = u8::from(new_count);
        if old_count != new_count_u8 {
            if old_count > new_count_u8 {
                for mecid in new_count_u8..old_count {
                    let netuid_index =
                        Self::get_mechanism_storage_index(netuid, MechId::from(mecid));

                    // Cleanup Weights
                    let _ = Weights::<T>::clear_prefix(netuid_index, u32::MAX, None);

                    // Cleanup Incentive
                    Incentive::<T>::remove(netuid_index);

                    // Cleanup previous epoch consensus
                    ConsensusByMechanism::<T>::remove(netuid_index);

                    // Cleanup LastUpdate
                    LastUpdate::<T>::remove(netuid_index);

                    // Cleanup Bonds
                    let _ = Bonds::<T>::clear_prefix(netuid_index, u32::MAX, None);

                    // Cleanup WeightCommits
                    let _ = WeightCommits::<T>::clear_prefix(netuid_index, u32::MAX, None);

                    Self::purge_mechanism_weight_delegates(netuid, mecid.into());
                    pallet_commitments::ChannelMechanismPurgeQueue::<T>::insert(
                        netuid,
                        MechId::from(mecid),
                        (),
                    );

                    // Cleanup TimelockedWeightCommits
                    let _ =
                        TimelockedWeightCommits::<T>::clear_prefix(netuid_index, u32::MAX, None);
                }
            }

            MechanismCountCurrent::<T>::insert(netuid, MechId::from(new_count));

            // Reset split back to even
            MechanismEmissionSplit::<T>::remove(netuid);
        }
    }

    pub fn do_set_emission_split(netuid: NetUid, maybe_split: Option<Vec<u16>>) -> DispatchResult {
        // Make sure the subnet exists
        ensure!(
            Self::if_subnet_exist(netuid),
            Error::<T>::MechanismDoesNotExist
        );

        if let Some(split) = maybe_split {
            // Check the length
            ensure!(!split.is_empty(), Error::<T>::InvalidValue);
            ensure!(
                split.len() == u8::from(MechanismCountCurrent::<T>::get(netuid)) as usize,
                Error::<T>::InvalidValue
            );

            // Check that values add up to 65535
            let total: u64 = split.iter().map(|s| *s as u64).sum();
            ensure!(total == u16::MAX as u64, Error::<T>::InvalidValue);

            MechanismEmissionSplit::<T>::insert(netuid, split);
        } else {
            MechanismEmissionSplit::<T>::remove(netuid);
        }

        Ok(())
    }

    /// Split alpha emission in sub-subnet proportions
    /// stored in MechanismEmissionSplit
    ///
    pub fn split_emissions(netuid: NetUid, alpha: AlphaBalance) -> Vec<AlphaBalance> {
        let mechanism_count = u64::from(MechanismCountCurrent::<T>::get(netuid));
        let maybe_split = MechanismEmissionSplit::<T>::get(netuid)
            .filter(|split| split.len() == mechanism_count as usize);

        // Unset or legacy malformed split means even distribution
        let mut result: Vec<AlphaBalance> = if let Some(split) = maybe_split {
            split
                .iter()
                .map(|s| {
                    AlphaBalance::from(
                        (u64::from(alpha) as u128)
                            .saturating_mul(*s as u128)
                            .safe_div(u16::MAX as u128) as u64,
                    )
                })
                .collect()
        } else {
            let per_mechanism = u64::from(alpha).safe_div(mechanism_count);
            vec![AlphaBalance::from(per_mechanism); mechanism_count as usize]
        };

        // If there's any rounding error or lost due to truncation emission, credit it to mechanism 0
        let rounding_err =
            u64::from(alpha).saturating_sub(result.iter().map(|s| u64::from(*s)).sum());
        if let Some(cell) = result.first_mut() {
            *cell = cell.saturating_add(AlphaBalance::from(rounding_err));
        }
        result
    }

    fn weighted_acc_u16(existing: u16, added: u16, weight: U64F64) -> u16 {
        U64F64::saturating_from_num(existing)
            .saturating_add(U64F64::saturating_from_num(added).saturating_mul(weight))
            .saturating_to_num::<u16>()
    }

    fn weighted_acc_alpha(
        existing: AlphaBalance,
        added: AlphaBalance,
        weight: U64F64,
    ) -> AlphaBalance {
        U64F64::saturating_from_num(existing)
            .saturating_add(U64F64::saturating_from_num(added).saturating_mul(weight))
            .saturating_to_num::<u64>()
            .into()
    }

    /// Splits rao_emission between different sub-subnets using `split_emissions` function.
    ///
    /// Runs the epoch function for each sub-subnet and consolidates hotkey_emission
    /// into a single vector.
    ///
    pub fn epoch_with_mechanisms(
        netuid: NetUid,
        rao_emission: AlphaBalance,
    ) -> Vec<(T::AccountId, AlphaBalance, AlphaBalance)> {
        let dividends = AlphaBalance::from(u64::from(rao_emission) / 2);
        Self::epoch_with_mechanism_budgets(
            netuid,
            rao_emission.saturating_sub(dividends),
            dividends,
            AlphaBalance::ZERO,
        )
    }

    /// Split funded pools before rounding individual mechanisms' payouts.
    pub(crate) fn epoch_with_mechanism_budgets(
        netuid: NetUid,
        miner_budget: AlphaBalance,
        local_dividend_budget: AlphaBalance,
        root_budget: AlphaBalance,
    ) -> Vec<(T::AccountId, AlphaBalance, AlphaBalance)> {
        let rao_emission = miner_budget
            .saturating_add(local_dividend_budget)
            .saturating_add(root_budget);
        let miners = Self::split_emissions(netuid, miner_budget);
        let local_dividends = Self::split_emissions(netuid, local_dividend_budget);
        let roots = Self::split_emissions(netuid, root_budget);
        // Each mechanism's normal and fallback pools derive from these same
        // funded slices. Remainders cannot multiply the subnet's miner budget.
        let null_consensus = Self::get_epoch_consensus(netuid) == EpochConsensus::Null;
        let emissions = if null_consensus {
            miners
                .iter()
                .zip(&local_dividends)
                .zip(&roots)
                .map(|((miner, local), root)| miner.saturating_add(*local).saturating_add(*root))
                .collect()
        } else {
            Self::split_emissions(netuid, rao_emission)
        };
        let aggregated: BTreeMap<T::AccountId, EpochTerms> = emissions
            .into_iter()
            .enumerate()
            // Run epoch function for each mechanism to distribute its portion of emissions
            .flat_map(|(sub_id_usize, sub_emission)| {
                let sub_id_u8: u8 = sub_id_usize.try_into().unwrap_or_default();
                let sub_id = MechId::from(sub_id_u8);

                // Run epoch function on the mechanism emission
                let mining_budget = miners.get(sub_id_usize).copied().unwrap_or_default();
                let local = local_dividends
                    .get(sub_id_usize)
                    .copied()
                    .unwrap_or_default();
                let root = roots.get(sub_id_usize).copied().unwrap_or_default();
                let epoch_output = Self::epoch_mechanism_with_budgets(
                    netuid,
                    sub_id,
                    sub_emission,
                    mining_budget,
                    local.saturating_add(root),
                    mining_budget.saturating_add(local),
                );
                Self::persist_mechanism_epoch_terms(netuid, sub_id, epoch_output.as_map());

                // Calculate mechanism weight from the split emission (not the other way because preserving
                // emission accuracy is the priority)
                // For zero emission the first mechanism gets full weight
                let sub_weight = U64F64::saturating_from_num(sub_emission).safe_div_or(
                    U64F64::saturating_from_num(rao_emission),
                    U64F64::saturating_from_num(if sub_id_u8 == 0 { 1 } else { 0 }),
                );

                // Produce an iterator of (hotkey, (terms, sub_weight)) tuples
                epoch_output
                    .0
                    .into_iter()
                    .map(move |(hotkey, terms)| (hotkey, (terms, sub_weight)))
            })
            // Consolidate the hotkey emissions into a single BTreeMap
            .fold(BTreeMap::new(), |mut acc, (hotkey, (terms, sub_weight))| {
                acc.entry(hotkey)
                    .and_modify(|acc_terms| {
                        // Server and validator emission come from mechanism emission and need to be added up
                        acc_terms.validator_emission = acc_terms
                            .validator_emission
                            .saturating_add(terms.validator_emission);
                        acc_terms.server_emission = acc_terms
                            .server_emission
                            .saturating_add(terms.server_emission);

                        // The rest of the terms need to be aggregated as weighted sum
                        acc_terms.dividend =
                            Self::weighted_acc_u16(acc_terms.dividend, terms.dividend, sub_weight);
                        acc_terms.stake_weight = Self::weighted_acc_u16(
                            acc_terms.stake_weight,
                            terms.stake_weight,
                            sub_weight,
                        );
                        acc_terms.active |= terms.active;
                        acc_terms.emission = if null_consensus {
                            // These are already whole-unit payouts from funded slices.
                            acc_terms.emission.saturating_add(terms.emission)
                        } else {
                            Self::weighted_acc_alpha(acc_terms.emission, terms.emission, sub_weight)
                        };
                        acc_terms.consensus = Self::weighted_acc_u16(
                            acc_terms.consensus,
                            terms.consensus,
                            sub_weight,
                        );
                        acc_terms.validator_trust = Self::weighted_acc_u16(
                            acc_terms.validator_trust,
                            terms.validator_trust,
                            sub_weight,
                        );
                        acc_terms.new_validator_permit |= terms.new_validator_permit;
                        acc_terms.stake = acc_terms.stake.saturating_add(terms.stake);
                    })
                    .or_insert_with(|| {
                        // weighted insert for the first sub-subnet seen for this hotkey
                        EpochTerms {
                            uid: terms.uid,
                            dividend: Self::weighted_acc_u16(0, terms.dividend, sub_weight),
                            incentive: Self::weighted_acc_u16(0, terms.incentive, sub_weight),
                            validator_emission: terms.validator_emission,
                            server_emission: terms.server_emission,
                            stake_weight: Self::weighted_acc_u16(0, terms.stake_weight, sub_weight),
                            active: terms.active, // booleans are ORed across subs
                            emission: if null_consensus {
                                terms.emission
                            } else {
                                Self::weighted_acc_alpha(0u64.into(), terms.emission, sub_weight)
                            },
                            consensus: Self::weighted_acc_u16(0, terms.consensus, sub_weight),
                            validator_trust: Self::weighted_acc_u16(
                                0,
                                terms.validator_trust,
                                sub_weight,
                            ),
                            new_validator_permit: terms.new_validator_permit,
                            bond: Vec::new(), // aggregated map doesn't use bonds; keep empty
                            stake: terms.stake,
                        }
                    });
                acc
            });

        // State updates from epoch function
        Self::persist_netuid_epoch_terms(netuid, &aggregated);

        // Update voting power EMA for all validators on this subnet
        Self::update_voting_power_for_subnet(netuid, &aggregated);

        // Remap BTreeMap back to Vec<(T::AccountId, AlphaBalance, AlphaBalance)> format
        // for processing emissions in run_coinbase
        // Emission tuples ( hotkeys, server_emission, validator_emission )
        aggregated
            .into_iter()
            .map(|(hotkey, terms)| (hotkey, terms.server_emission, terms.validator_emission))
            .collect()
    }
}
