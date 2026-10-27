use frame_support::storage::{TransactionOutcome, with_transaction};
use frame_support::traits::Currency;
use frame_support::weights::Weight;

use super::*;
use crate::migrations::migrate_alpha_v2::retired::Alpha;
use crate::subnets::leasing::LeaseId;
use crate::weights::WeightInfo;
use sp_core::Get;
use sp_runtime::{Saturating, traits::Hash};
use sp_std::collections::btree_set::BTreeSet;
use subtensor_runtime_common::TaoBalance;

/// Stake work a coldkey swap moves: `StakingHotkeys` / basket-credit entries visited
/// (combined across source and destination), positions moved from the source,
/// destination lock rows scanned/purged, and network-registration queue work
/// (total entries scanned plus owned rows rewritten).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ColdkeySwapWork {
    pub hotkeys: u32,
    pub positions: u32,
    pub pending_registrations: u32,
    /// Destination `Lock` rows scanned at admission (and purged when exhausted
    /// during execute). Independent of source stake positions.
    pub dest_locks: u32,
    /// Total `NetworkRegistrationQueue` entries visited (not only owned rows).
    pub registration_queue_scan: u32,
}

/// How an authenticated coldkey-swap announcement decides whether the source's
/// auto-stake policy may move to an otherwise-unconfigured destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ColdkeySwapPolicyImportMode {
    /// Apply the complete destination-presence check at execution.
    CurrentDestination,
    /// The destination already had presence at announcement; never import.
    Reject,
    /// The original blank-destination reservation is still live, so ignore only
    /// unsolicited balance while enforcing every non-balance protection.
    IgnoreDestinationBalance,
}

impl<T: Config> Pallet<T> {
    /// Weight of a coldkey swap over `base` (the benchmarked fixed part): one `transfer_stake`
    /// per position moved plus the per-hotkey bookkeeping reads (watermark, unlock age,
    /// root stake) for every admitted hotkey in the combined source∪destination set.
    fn coldkey_swap_weight(base: Weight, work: ColdkeySwapWork) -> Weight {
        base.saturating_add(
            <T as crate::pallet::Config>::WeightInfo::transfer_stake()
                .saturating_mul(u64::from(work.positions)),
        )
        .saturating_add(T::DbWeight::get().reads(u64::from(work.hotkeys).saturating_mul(3)))
        // Live reservation check at admission/execute (maps + TTL).
        .saturating_add(T::DbWeight::get().reads(3))
        // Destination lock path per exhausted row (Skeptic fid:bcf7b5a5):
        // admission walk, execute re-check in `swap_coldkey_locks`, and purge
        // scan each read Lock + SubnetOwnerHotkey + DecayingLock (3×3); cleanup
        // then Lock+LockingColdkeys removes plus `reduce_aggregate_lock`
        // (`read_conviction_model_for_hotkey` ≈7 reads, up to 4 aggregate writes).
        .saturating_add(T::DbWeight::get().reads_writes(
            u64::from(work.dest_locks).saturating_mul(16),
            u64::from(work.dest_locks).saturating_mul(6),
        ))
        // Queue value read, per-entry visit cost, then get+insert per owned
        // pending registration and one queue rewrite.
        .saturating_add(
            T::DbWeight::get().reads(u64::from(work.registration_queue_scan).saturating_add(1)),
        )
        .saturating_add(T::DbWeight::get().reads_writes(
            u64::from(work.pending_registrations),
            u64::from(work.pending_registrations).saturating_add(1),
        ))
    }

    /// The largest swap one call admits, used as the pre-dispatch envelope.
    fn coldkey_swap_max_work() -> ColdkeySwapWork {
        ColdkeySwapWork {
            hotkeys: crate::MAX_COLDKEY_SWAP_HOTKEYS,
            positions: crate::MAX_COLDKEY_SWAP_POSITIONS,
            pending_registrations: crate::MAX_COLDKEY_SWAP_PENDING_REGISTRATIONS,
            dest_locks: crate::MAX_COLDKEY_SWAP_DEST_LOCKS,
            registration_queue_scan: crate::MAX_COLDKEY_SWAP_REGISTRATION_QUEUE_SCAN,
        }
    }

    /// Pre-dispatch weight of `announce_coldkey_swap_v2`, covering destination validation
    /// (including the combined hotkey/position admission scan) before the fee is charged.
    ///
    /// Admission walks stop one entry past the cap so a failing oversize scan is still
    /// covered by this envelope (`max + 1` sentinel reads per axis).
    /// Admission / reservation DB envelope on top of the benchmarked V2 base.
    pub fn announce_coldkey_swap_v2_admission_weight() -> Weight {
        let max = Self::coldkey_swap_max_work();
        let hotkey_reads = u64::from(max.hotkeys).saturating_add(1).saturating_mul(2);
        let position_reads = u64::from(max.positions).saturating_add(1);
        // Dest lock walk at admission: Lock + SubnetOwnerHotkey + DecayingLock
        // per row (sentinel past the cap), plus source lock walk when an
        // in-use dest rejects locked alpha (position sentinel, one read/row).
        let lock_reads = u64::from(max.dest_locks)
            .saturating_add(1)
            .saturating_mul(3)
            .saturating_add(u64::from(max.positions).saturating_add(1));
        // Opt-out growth check re-reads dest staking + up to sentinel source
        // associations (contains_key per hotkey) after the bounded work scan.
        let opt_out_reads = u64::from(max.hotkeys)
            .saturating_add(1)
            .saturating_mul(2)
            .saturating_add(4);
        // Destination reservation: conflict check + prior-release + dual-map write.
        let reservation = T::DbWeight::get()
            .reads(3)
            .saturating_add(T::DbWeight::get().writes(4));
        // Snapshot whether the destination already carries state that rejects
        // policy import: staking/basket/ownership, flags, lineage, balance, proxy.
        let policy_snapshot = T::DbWeight::get().reads_writes(7, 1);
        // Pending-registration discovery: one queue read + total-scan sentinel
        // (not only owned rows — unrelated entries are still visited).
        let pending_reads = u64::from(max.registration_queue_scan).saturating_add(1);
        T::DbWeight::get()
            .reads(4)
            .saturating_add(
                T::DbWeight::get().reads(
                    hotkey_reads
                        .saturating_add(position_reads)
                        .saturating_add(lock_reads)
                        .saturating_add(opt_out_reads)
                        .saturating_add(pending_reads),
                ),
            )
            .saturating_add(reservation)
            .saturating_add(policy_snapshot)
    }

    /// Weight of destination-authorized reservation release (dual-map cleanup).
    pub fn release_own_coldkey_swap_destination_reservation_weight() -> Weight {
        <T as crate::pallet::Config>::WeightInfo::release_own_coldkey_swap_destination_reservation()
    }

    /// Extra DB work clear/reset pay for releasing a destination reservation.
    pub fn coldkey_swap_reservation_release_weight() -> Weight {
        T::DbWeight::get()
            .reads(2)
            .saturating_add(T::DbWeight::get().writes(2))
    }

    /// Whether `announcer`'s reservation is still within its bounded lifetime.
    ///
    /// Live while the announcement exists and
    /// `now <= when + ColdkeySwapReannouncementDelay`. Past that window the
    /// destination can be claimed again (prevents indefinite squatting).
    fn coldkey_swap_reservation_is_live(announcer: &T::AccountId) -> bool {
        let Some((when, _)) = ColdkeySwapAnnouncements::<T>::get(announcer) else {
            return false;
        };
        let now = <frame_system::Pallet<T>>::block_number();
        let ttl = ColdkeySwapReannouncementDelay::<T>::get();
        now <= when.saturating_add(ttl)
    }

    /// Drop a stale or orphaned reservation on `dest`, returning whether a live
    /// reservation by someone other than `by` still blocks it.
    fn purge_stale_coldkey_swap_destination_reservation(
        dest: &T::AccountId,
        by: Option<&T::AccountId>,
    ) -> bool {
        let Some(announcer) = ColdkeySwapDestinationReservations::<T>::get(dest) else {
            return false;
        };
        if Self::coldkey_swap_reservation_is_live(&announcer) {
            return match by {
                None => true,
                Some(who) => *who != announcer,
            };
        }
        // Stale / orphaned: free both maps.
        ColdkeySwapDestinationReservations::<T>::remove(dest);
        if ColdkeySwapReservedDestination::<T>::get(&announcer).as_ref() == Some(dest) {
            ColdkeySwapReservedDestination::<T>::remove(&announcer);
        }
        false
    }

    /// Refuse if `account` is currently reserved as a coldkey-swap destination.
    ///
    /// Used by hotkey registration / ownership paths so a revealed announce dest
    /// cannot be claimed as a hotkey (via `create_account` or `swap_hotkey`),
    /// and by cross-coldkey stake transitions so third parties cannot grow the
    /// destination's associations / locks during the delay.
    pub fn ensure_account_not_reserved_as_coldkey_swap_destination(
        account: &T::AccountId,
    ) -> Result<(), Error<T>> {
        if Self::purge_stale_coldkey_swap_destination_reservation(account, None) {
            return Err(Error::<T>::ColdkeySwapDestinationReserved);
        }
        Ok(())
    }

    /// Refuse if `dest` is already reserved by a different announcer.
    ///
    /// Called before the announce fee so a conflict cannot burn the swap cost.
    pub(crate) fn ensure_coldkey_swap_destination_reservable(
        announcer: &T::AccountId,
        dest: &T::AccountId,
    ) -> Result<(), Error<T>> {
        if Self::purge_stale_coldkey_swap_destination_reservation(dest, Some(announcer)) {
            return Err(Error::<T>::ColdkeySwapDestinationReserved);
        }
        Ok(())
    }

    /// Reserve `dest` for `announcer`'s in-flight coldkey-swap announcement.
    ///
    /// Frees any prior reservation held by `announcer` (reannounce to a new dest).
    /// Refuses if another announcer already reserved `dest`.
    pub(crate) fn reserve_coldkey_swap_destination(
        announcer: &T::AccountId,
        dest: &T::AccountId,
    ) -> Result<(), Error<T>> {
        Self::ensure_coldkey_swap_destination_reservable(announcer, dest)?;
        Self::release_coldkey_swap_destination_reservation(announcer);
        ColdkeySwapDestinationReservations::<T>::insert(dest, announcer.clone());
        ColdkeySwapReservedDestination::<T>::insert(announcer, dest.clone());
        Ok(())
    }

    /// Drop the destination reservation held by `announcer`, if any.
    pub(crate) fn release_coldkey_swap_destination_reservation(announcer: &T::AccountId) {
        if let Some(dest) = ColdkeySwapReservedDestination::<T>::take(announcer)
            && ColdkeySwapDestinationReservations::<T>::get(&dest).as_ref() == Some(announcer)
        {
            ColdkeySwapDestinationReservations::<T>::remove(&dest);
        }
    }

    /// Destination-authorized release: the reserved account itself can free the
    /// reservation so exclusive holds cannot squat someone else's coldkey forever.
    pub fn do_release_own_coldkey_swap_destination_reservation(
        dest: &T::AccountId,
    ) -> Result<(), Error<T>> {
        let Some(announcer) = ColdkeySwapDestinationReservations::<T>::get(dest) else {
            return Err(Error::<T>::ColdkeySwapDestinationReservationNotFound);
        };
        // Only the reserved destination may authorize this release.
        ensure!(
            ColdkeySwapReservedDestination::<T>::get(&announcer).as_ref() == Some(dest),
            Error::<T>::ColdkeySwapDestinationReservationNotFound
        );
        Self::release_coldkey_swap_destination_reservation(&announcer);
        Ok(())
    }

    /// Pre-dispatch weight of `swap_coldkey`, refunded to the actual work post-dispatch.
    pub fn swap_coldkey_declared_weight() -> Weight {
        Self::coldkey_swap_weight(
            <T as crate::pallet::Config>::WeightInfo::swap_coldkey(),
            Self::coldkey_swap_max_work(),
        )
        .saturating_add(T::DbWeight::get().writes(1))
    }

    /// Post-dispatch weight of `swap_coldkey`.
    pub fn swap_coldkey_actual_weight(work: ColdkeySwapWork) -> Weight {
        Self::coldkey_swap_weight(
            <T as crate::pallet::Config>::WeightInfo::swap_coldkey(),
            work,
        )
        .saturating_add(T::DbWeight::get().writes(1))
    }

    /// Pre-dispatch weight of `swap_coldkey_announced`, refunded post-dispatch.
    pub fn swap_coldkey_announced_declared_weight() -> Weight {
        Self::coldkey_swap_weight(
            <T as crate::pallet::Config>::WeightInfo::swap_coldkey_announced(),
            Self::coldkey_swap_max_work(),
        )
        // V2 commitment recovery reads the announcement-time snapshot.
        .saturating_add(T::DbWeight::get().reads(1))
        // Successful execution removes the announcement and its V2 snapshot.
        .saturating_add(T::DbWeight::get().writes(2))
    }

    /// Post-dispatch weight of `swap_coldkey_announced`.
    pub fn swap_coldkey_announced_actual_weight(work: ColdkeySwapWork) -> Weight {
        Self::coldkey_swap_weight(
            <T as crate::pallet::Config>::WeightInfo::swap_coldkey_announced(),
            work,
        )
        .saturating_add(T::DbWeight::get().writes(2))
    }

    /// Stake positions the swap has to move, plus the combined hotkey set of source and
    /// destination. Positions only leave the source; hotkeys count the union so a merge
    /// into an already-used coldkey cannot under-price bookkeeping or exceed the list cap.
    ///
    /// Stops one entry past each admission limit so oversized accounts fail closed
    /// without scanning unbounded storage during announce/execute prechecks.
    pub fn coldkey_swap_work(
        old_coldkey: &T::AccountId,
        new_coldkey: &T::AccountId,
    ) -> ColdkeySwapWork {
        let hotkey_limit = crate::MAX_COLDKEY_SWAP_HOTKEYS.saturating_add(1) as usize;
        let position_limit = crate::MAX_COLDKEY_SWAP_POSITIONS.saturating_add(1);

        // Scan each side up to the full sentinel independently so overlapping
        // associations cannot shrink the discovery budget and under-count the union.
        let old_hotkeys = Self::coldkey_swap_hotkeys_bounded(old_coldkey, hotkey_limit);
        if old_hotkeys.len() >= hotkey_limit {
            return ColdkeySwapWork {
                hotkeys: hotkey_limit as u32,
                ..Default::default()
            };
        }
        let mut combined = old_hotkeys.clone();
        if old_coldkey != new_coldkey {
            let new_hotkeys = Self::coldkey_swap_hotkeys_bounded(new_coldkey, hotkey_limit);
            if new_hotkeys.len() >= hotkey_limit {
                return ColdkeySwapWork {
                    hotkeys: hotkey_limit as u32,
                    ..Default::default()
                };
            }
            for hotkey in new_hotkeys {
                combined.insert(hotkey);
                if combined.len() >= hotkey_limit {
                    return ColdkeySwapWork {
                        hotkeys: hotkey_limit as u32,
                        ..Default::default()
                    };
                }
            }
        }

        let mut work = ColdkeySwapWork {
            hotkeys: combined.len() as u32,
            ..Default::default()
        };

        for hotkey in &old_hotkeys {
            // Walk legacy + v2 share maps directly so we can stop at the admission
            // sentinel without `alpha_iter_prefix`'s full-prefix materialization.
            let mut seen: BTreeSet<NetUid> = BTreeSet::new();
            for (netuid, share) in Alpha::<T>::iter_prefix((hotkey, old_coldkey)) {
                if share == substrate_fixed::types::U64F64::from_num(0) || !seen.insert(netuid) {
                    continue;
                }
                work.positions = work.positions.saturating_add(1);
                if work.positions >= position_limit {
                    return work;
                }
            }
            for (netuid, share) in AlphaV2::<T>::iter_prefix((hotkey, old_coldkey)) {
                if share.is_zero() || !seen.insert(netuid) {
                    continue;
                }
                work.positions = work.positions.saturating_add(1);
                if work.positions >= position_limit {
                    return work;
                }
            }
        }
        work
    }

    /// Include claim-only root-swap credits without appending them to the staking list.
    fn coldkey_swap_hotkeys(coldkey: &T::AccountId) -> BTreeSet<T::AccountId> {
        Self::coldkey_swap_hotkeys_bounded(
            coldkey,
            crate::MAX_COLDKEY_SWAP_HOTKEYS.saturating_add(1) as usize,
        )
    }

    /// Bounded hotkey discovery for admission / transfer walks.
    fn coldkey_swap_hotkeys_bounded(
        coldkey: &T::AccountId,
        limit: usize,
    ) -> BTreeSet<T::AccountId> {
        if limit == 0 {
            return BTreeSet::new();
        }
        let mut hotkeys: BTreeSet<_> = StakingHotkeys::<T>::get(coldkey)
            .into_iter()
            .take(limit)
            .collect();
        if hotkeys.len() >= limit {
            return hotkeys;
        }
        let remaining = limit.saturating_sub(hotkeys.len());
        hotkeys.extend(HotkeySwapBasketCredits::<T>::iter_key_prefix(coldkey).take(remaining));
        hotkeys
    }

    /// Whether `coldkey` already holds coldkey-keyed economic associations that make
    /// a swap into it a merge for staking / lock-transfer purposes. Fresh keys
    /// still get the temporary `accept_locked_alpha` override so an owner can move
    /// their own locks onto a new unused coldkey; staking-associated destinations
    /// honor their AccountFlags opt-outs instead.
    pub(crate) fn coldkey_already_in_use(coldkey: &T::AccountId) -> bool {
        !StakingHotkeys::<T>::get(coldkey).is_empty()
            || HotkeySwapBasketCredits::<T>::iter_key_prefix(coldkey)
                .next()
                .is_some()
            || !OwnedHotkeys::<T>::get(coldkey).is_empty()
    }

    /// Destination already carries a recorded swap-lineage root that an
    /// unsolicited merge must not overwrite (Auditor fid:9411d39b).
    ///
    /// Only `ColdkeyRoot` counts: a prior root may still hold an outgoing
    /// `ColdkeySuccessor` edge after rotation, and a reverse rename onto that
    /// root must be allowed to clear/repoint the tip.
    fn coldkey_has_established_lineage(coldkey: &T::AccountId) -> bool {
        ColdkeyRoot::<T>::contains_key(coldkey)
    }

    /// Destination already has non-balance state that must always protect it
    /// from an unsolicited policy import, even if a V2 announcement originally
    /// targeted a blank account. Balance is deliberately separate: anyone can
    /// add dust without the destination's authorization.
    fn coldkey_has_non_balance_policy_protection(coldkey: &T::AccountId) -> bool {
        Self::coldkey_already_in_use(coldkey)
            || Self::coldkey_has_configured_account_flags(coldkey)
            || Self::coldkey_has_established_lineage(coldkey)
            || T::ProxyInterface::has_delegator_proxies(coldkey)
    }

    /// Destination already has funds, proxy grants, flags, lineage, or staking
    /// associations — so a source-only swap must not import the source's
    /// auto-stake policy without destination authorization (Skeptic
    /// fid:93e26a58, fid:a2c90852, fid:c135ad77). Blank rename targets return
    /// false.
    ///
    /// Balance uses total account funds (`Inspect::balance`), including
    /// reserved/held deposits — not reducible/transferable only. A pending
    /// multisig (or similar) deposit with zero free balance is still prior
    /// presence.
    pub(crate) fn coldkey_rejects_unsolicited_policy_import(coldkey: &T::AccountId) -> bool {
        Self::coldkey_has_non_balance_policy_protection(coldkey)
            || !Self::coldkey_total_balance(coldkey).is_zero()
    }

    /// V2 commits to whether the destination was blank when it was announced.
    ///
    /// Binding the stored snapshot into the announcement hash prevents balance
    /// transfers during the delay from changing the source auto-stake policy
    /// outcome or a legacy caller from selecting V2 semantics.
    pub(crate) fn coldkey_swap_v2_announcement_hash(
        new_coldkey: &T::AccountId,
        import_source_policies: bool,
    ) -> T::Hash {
        T::Hashing::hash_of(&(
            b"coldkey-swap-v2-policy",
            new_coldkey,
            import_source_policies,
        ))
    }

    /// Recover and authenticate a V2 announcement's policy snapshot. Legacy
    /// callers may submit arbitrary hashes, so the dedicated V2 storage marker
    /// must exist before a matching tuple hash grants snapshot semantics.
    ///
    /// A `true` snapshot only permits ignoring unsolicited balance added while
    /// this exact destination reservation remains live. After release or expiry,
    /// it falls back to the complete current destination-presence check. A
    /// committed `false` remains a rejection even if the destination is later
    /// cleared.
    pub(crate) fn coldkey_swap_v2_policy_snapshot(
        announcer: &T::AccountId,
        new_coldkey: &T::AccountId,
        announcement_hash: &T::Hash,
        when: frame_system::pallet_prelude::BlockNumberFor<T>,
    ) -> Option<ColdkeySwapPolicyImportMode> {
        let import_source_policies = ColdkeySwapV2PolicySnapshots::<T>::get(announcer)?;
        if Self::coldkey_swap_v2_announcement_hash(new_coldkey, import_source_policies)
            != *announcement_hash
        {
            return None;
        }
        if !import_source_policies {
            return Some(ColdkeySwapPolicyImportMode::Reject);
        }

        let now = <frame_system::Pallet<T>>::block_number();
        let ttl = ColdkeySwapReannouncementDelay::<T>::get();
        let reservation_is_live = now <= when.saturating_add(ttl)
            && ColdkeySwapReservedDestination::<T>::get(announcer).as_ref() == Some(new_coldkey)
            && ColdkeySwapDestinationReservations::<T>::get(new_coldkey).as_ref()
                == Some(announcer);
        Some(if reservation_is_live {
            ColdkeySwapPolicyImportMode::IgnoreDestinationBalance
        } else {
            ColdkeySwapPolicyImportMode::CurrentDestination
        })
    }

    /// Free + reserved TAO on `coldkey`. Distinct from
    /// [`Self::get_coldkey_balance`] (reducible only) and from
    /// `Inspect::balance` (free only).
    fn coldkey_total_balance(coldkey: &T::AccountId) -> TaoBalance {
        <T as Config>::Currency::total_balance(coldkey)
    }

    /// Destination has explicitly configured `AccountFlags` that must be
    /// preserved on merge even without staking associations.
    fn coldkey_has_configured_account_flags(coldkey: &T::AccountId) -> bool {
        AccountFlags::<T>::contains_key(coldkey)
    }

    /// Count owned pending network-registration rows and total queue entries
    /// visited for `coldkey`.
    ///
    /// Stops one past the owned-row cap **or** the total-queue scan cap so
    /// work cannot grow with unrelated accounts' registrations. Returns
    /// `(owned, queue_entries_scanned)`.
    ///
    /// Uses [`StorageValue::decode_len`] before `get()` so an oversized queue
    /// fails closed without SCALE-decoding every entry (Auditor fid:185b04ac).
    fn coldkey_pending_registrations_bounded(coldkey: &T::AccountId) -> (u32, u32) {
        let owned_limit = crate::MAX_COLDKEY_SWAP_PENDING_REGISTRATIONS.saturating_add(1);
        let scan_limit = crate::MAX_COLDKEY_SWAP_REGISTRATION_QUEUE_SCAN.saturating_add(1);
        // Length prefix only — do not allocate/decode the Vec when oversize.
        let encoded_len = NetworkRegistrationQueue::<T>::decode_len().unwrap_or(0) as u32;
        if encoded_len >= scan_limit {
            return (0, scan_limit);
        }
        let mut owned = 0u32;
        let mut scanned = 0u32;
        for info in NetworkRegistrationQueue::<T>::get() {
            scanned = scanned.saturating_add(1);
            if scanned >= scan_limit {
                // Sentinel: either the queue itself is oversize, or we stopped
                // before finishing — both fail closed at admission.
                return (owned, scanned);
            }
            if info.coldkey != *coldkey {
                continue;
            }
            owned = owned.saturating_add(1);
            if owned >= owned_limit {
                return (owned, scanned);
            }
        }
        (owned, scanned)
    }

    /// Destination checks shared by cleartext announce (before the fee) and execution.
    ///
    /// Still rejects: destination is itself a hotkey; destination has an active lock;
    /// combined work exceeds the one-call bound; destination already in use that
    /// rejects locked alpha while the source has active locks; destination that
    /// rejects staking-hotkey growth while the source would introduce new hotkeys.
    /// Destination may already hold stake, owned hotkeys, basket credits, identity,
    /// or subnet ownership — those merge when the opt-outs allow.
    ///
    /// On failure, also returns the weight of the admission / opt-out scans already
    /// performed so dispatchers charge that work instead of refunding it.
    pub fn ensure_coldkey_swap_destination(
        old_coldkey: &T::AccountId,
        new_coldkey: &T::AccountId,
    ) -> Result<ColdkeySwapWork, (Weight, Error<T>)> {
        let precheck = || Self::coldkey_swap_failed_weight(None, false);
        // Reservation lookup (maps + TTL) charged on every admission path.
        let reservation_weight = T::DbWeight::get().reads(3);
        Self::ensure_beta_basket_seed_idle().map_err(|e| (precheck(), e))?;
        if Self::hotkey_account_exists(new_coldkey) {
            return Err((precheck(), Error::<T>::NewColdKeyIsHotkey));
        }
        // Execute (and announce) must honor a live V2 reservation: only the
        // reserving announcer may proceed. A competing legacy hash announce
        // must not be able to merge stake/locks into the reserved destination
        // and grief the victim with ActiveLockExists (Skeptic fid:fb099e4c).
        Self::ensure_coldkey_swap_destination_reservable(old_coldkey, new_coldkey)
            .map_err(|e| (precheck().saturating_add(reservation_weight), e))?;
        // Blocking locks on the destination (locked mass or residual conviction
        // above dust) cannot be merged safely with source locks, and announce
        // admission must stay read-only w.r.t. the destination — a third party
        // can name any coldkey here. Bound the walk to the announce/execute
        // weight envelope. Fully exhausted dust rows are purged later inside
        // `swap_coldkey_locks` only. Retain the scanned count on every path so
        // later admission failures still charge this work.
        let dest_locks = match Self::ensure_no_active_locks_for_swap(new_coldkey) {
            Ok(scanned) => scanned,
            Err(error) => {
                let scanned = ColdkeySwapWork {
                    dest_locks: crate::MAX_COLDKEY_SWAP_DEST_LOCKS.saturating_add(1),
                    ..Default::default()
                };
                return Err((
                    Self::coldkey_swap_failed_weight(Some(scanned), false),
                    error,
                ));
            }
        };

        // Bound the combined work first so opt-out walks never scan past the
        // admission envelope (even when the destination already contains every
        // source association and would not early-return).
        let mut work = Self::coldkey_swap_work(old_coldkey, new_coldkey);
        work.dest_locks = dest_locks;
        if work.dest_locks > crate::MAX_COLDKEY_SWAP_DEST_LOCKS
            || work.hotkeys > crate::MAX_COLDKEY_SWAP_HOTKEYS
            || work.positions > crate::MAX_COLDKEY_SWAP_POSITIONS
        {
            // Retain this scan — do not re-walk storage for the failure weight.
            return Err((
                Self::coldkey_swap_failed_weight(Some(work), false),
                Error::<T>::ColdkeySwapTooHeavy,
            ));
        }

        let (owned_regs, queue_scan) = Self::coldkey_pending_registrations_bounded(old_coldkey);
        work.pending_registrations = owned_regs;
        work.registration_queue_scan = queue_scan;
        if work.pending_registrations > crate::MAX_COLDKEY_SWAP_PENDING_REGISTRATIONS
            || work.registration_queue_scan > crate::MAX_COLDKEY_SWAP_REGISTRATION_QUEUE_SCAN
        {
            return Err((
                Self::coldkey_swap_failed_weight(Some(work), false),
                Error::<T>::ColdkeySwapTooHeavy,
            ));
        }

        // Honor destination opt-outs over the admitted (bounded) associations.
        // Fresh unused destinations still allow an owner to move their own locks
        // (temporary accept override in execute). Destinations with staking
        // associations *or* configured AccountFlags that reject locked alpha
        // keep that preference (Auditor fid:9411d39b).
        if (Self::coldkey_already_in_use(new_coldkey)
            || Self::coldkey_has_configured_account_flags(new_coldkey))
            && Self::account_rejects_locked_alpha(new_coldkey)
            && let Err(e) = Self::ensure_no_active_locks_for_swap(old_coldkey)
        {
            let error = match e {
                Error::<T>::ActiveLockExists => Error::<T>::AccountRejectsLockedAlpha,
                other => other,
            };
            return Err((
                Self::coldkey_swap_failed_weight(Some(work), true).saturating_add(
                    T::DbWeight::get()
                        .reads(u64::from(crate::MAX_COLDKEY_SWAP_POSITIONS).saturating_add(1)),
                ),
                error,
            ));
        }
        if Self::account_rejects_staking_hotkey_growth(new_coldkey) {
            let hotkey_limit = crate::MAX_COLDKEY_SWAP_HOTKEYS.saturating_add(1) as usize;
            let dest_staking: BTreeSet<_> = StakingHotkeys::<T>::get(new_coldkey)
                .into_iter()
                .take(hotkey_limit)
                .collect();
            let opt_out_reads = T::DbWeight::get()
                .reads(u64::from(work.hotkeys).saturating_add(1).saturating_mul(2));
            for hotkey in Self::coldkey_swap_hotkeys_bounded(old_coldkey, hotkey_limit) {
                if dest_staking.contains(&hotkey) {
                    continue;
                }
                if HotkeySwapBasketCredits::<T>::contains_key(new_coldkey, &hotkey) {
                    continue;
                }
                return Err((
                    Self::coldkey_swap_failed_weight(Some(work), true)
                        .saturating_add(opt_out_reads),
                    Error::<T>::AccountRejectsStakingHotkeyGrowth,
                ));
            }
        }

        Ok(work)
    }

    /// Transfer all assets, stakes, subnet ownerships, and hotkey associations from `old_coldkey` to
    /// to `new_coldkey`. Returns the stake work moved so the dispatch can report its weight.
    pub fn do_swap_coldkey(
        old_coldkey: &T::AccountId,
        new_coldkey: &T::AccountId,
    ) -> Result<ColdkeySwapWork, DispatchError> {
        Self::do_swap_coldkey_tracked(old_coldkey, new_coldkey).map_err(|(_, err)| err)
    }

    /// Weight of the stake work a failed coldkey swap did, on top of the benchmarked base
    /// the dispatch adds: nothing but the pre-check reads when refused before the scan,
    /// the admission scan (hotkeys, positions, dest locks, registration queue) when
    /// refused before commit, and the admitted work when a later step rolled it back.
    fn coldkey_swap_failed_weight(work: Option<ColdkeySwapWork>, admitted: bool) -> Weight {
        // seed idle + hotkey-exists + active-locks + (optional) work scan bookkeeping.
        let precheck = T::DbWeight::get().reads(4);
        match work {
            None => precheck,
            // Non-admitted failures still performed dest-lock and/or queue scans —
            // charge those axes so repeated announce/execute attempts cannot
            // refund the recorded work (Skeptic fid:0ea40e68). Dest admission
            // reads Lock + SubnetOwnerHotkey + DecayingLock per row.
            Some(work) if !admitted => precheck.saturating_add(
                T::DbWeight::get().reads(
                    u64::from(work.hotkeys)
                        .saturating_add(u64::from(work.positions))
                        .saturating_add(u64::from(work.dest_locks).saturating_mul(3))
                        .saturating_add(u64::from(work.registration_queue_scan))
                        .saturating_add(1),
                ),
            ),
            Some(work) => precheck.saturating_add(Self::coldkey_swap_weight(Weight::zero(), work)),
        }
    }

    /// [`Self::do_swap_coldkey`] that, on failure, also returns the weight of the work
    /// done (without the call's benchmarked base) so the dispatcher charges that instead
    /// of the declared envelope of `MAX_COLDKEY_SWAP_POSITIONS` stake transfers.
    pub fn do_swap_coldkey_tracked(
        old_coldkey: &T::AccountId,
        new_coldkey: &T::AccountId,
    ) -> Result<ColdkeySwapWork, (Weight, DispatchError)> {
        Self::do_swap_coldkey_tracked_with_policy_snapshot(
            old_coldkey,
            new_coldkey,
            ColdkeySwapPolicyImportMode::CurrentDestination,
        )
    }

    /// Announced V2 swaps pass the destination-presence snapshot recorded at
    /// announcement. Direct and legacy swaps pass `None` and evaluate the
    /// destination at execution as before.
    pub(crate) fn do_swap_coldkey_tracked_with_policy_snapshot(
        old_coldkey: &T::AccountId,
        new_coldkey: &T::AccountId,
        policy_import_mode: ColdkeySwapPolicyImportMode,
    ) -> Result<ColdkeySwapWork, (Weight, DispatchError)> {
        let work = match Self::ensure_coldkey_swap_destination(old_coldkey, new_coldkey) {
            Ok(work) => work,
            Err((done, error)) => return Err((done, error.into())),
        };

        Self::execute_swap_coldkey(old_coldkey, new_coldkey, work, policy_import_mode)
            .map_err(|error| (Self::coldkey_swap_failed_weight(Some(work), true), error))
    }

    /// The transactional body of a coldkey swap whose `work` was admitted.
    fn execute_swap_coldkey(
        old_coldkey: &T::AccountId,
        new_coldkey: &T::AccountId,
        work: ColdkeySwapWork,
        policy_import_mode: ColdkeySwapPolicyImportMode,
    ) -> Result<ColdkeySwapWork, DispatchError> {
        // A self-swap moves nothing; running the merge body against one account
        // would drop state that the helpers take before re-checking the destination.
        if old_coldkey == new_coldkey {
            Self::deposit_event(Event::ColdkeySwapped {
                old_coldkey: old_coldkey.clone(),
                new_coldkey: new_coldkey.clone(),
            });
            return Ok(work);
        }
        with_transaction(|| {
            let result = (|| -> DispatchResult {
                // Identity: destination wins. Always clear the source so a drained
                // coldkey does not keep an orphaned identity row (`take` runs before
                // the `&&` condition, so the source row is removed either way).
                if let Some(identity) = IdentitiesV2::<T>::take(old_coldkey)
                    && !IdentitiesV2::<T>::contains_key(new_coldkey)
                {
                    IdentitiesV2::<T>::insert(new_coldkey.clone(), identity);
                }

                // AccountFlags / lineage: staking associations, configured flags,
                // and established lineage are separate predicates (Auditor
                // fid:9411d39b). Staking-in-use still gates the temporary
                // accept_locked_alpha override; flags and lineage are preserved
                // whenever the destination already carries them — even with no
                // hotkey associations (rotated/drained or identity-only keys).
                let saved_dest_flags = AccountFlags::<T>::get(new_coldkey);
                let saved_source_flags = AccountFlags::<T>::get(old_coldkey);
                let dest_staking_in_use = Self::coldkey_already_in_use(new_coldkey);
                let preserve_dest_flags =
                    dest_staking_in_use || Self::coldkey_has_configured_account_flags(new_coldkey);
                let record_lineage =
                    !dest_staking_in_use && !Self::coldkey_has_established_lineage(new_coldkey);
                // Snapshot before any balance/stake moves: source-only swaps must
                // not import auto-stake onto a destination that already has funds,
                // grants, flags, lineage, or staking associations (Skeptic
                // fid:93e26a58 / fid:a2c90852). Only a blank rename target imports
                // the source policy.
                let import_source_policies = match policy_import_mode {
                    ColdkeySwapPolicyImportMode::CurrentDestination => {
                        !Self::coldkey_rejects_unsolicited_policy_import(new_coldkey)
                    }
                    ColdkeySwapPolicyImportMode::Reject => false,
                    ColdkeySwapPolicyImportMode::IgnoreDestinationBalance => {
                        !Self::coldkey_has_non_balance_policy_protection(new_coldkey)
                    }
                };
                // Temporary accept only on a true rename onto an empty key.
                // Destinations that already carry flags (including reject-locked)
                // must not have those flags flipped for the lock hand-off.
                if !preserve_dest_flags {
                    Self::set_accept_locked_alpha(new_coldkey, true);
                }

                // Lease rows outlive `NetworksAdded` while a dissolved subnet awaits cleanup,
                // so discover them through the lease index rather than the active-subnet list.
                for (_, lease_id) in SubnetUidToLeaseId::<T>::iter() {
                    Self::transfer_coldkey_lease(lease_id, old_coldkey, new_coldkey)?;
                }

                // Move stake by the coldkey's actual positions (not every subnet × every
                // hotkey), then the per-subnet ownership, auto-stake and collateral state.
                Self::transfer_coldkey_positions(old_coldkey, new_coldkey)?;
                for netuid in Self::get_all_subnet_netuids() {
                    Self::transfer_subnet_ownership(netuid, old_coldkey, new_coldkey);
                    Self::transfer_auto_stake_destination(
                        netuid,
                        old_coldkey,
                        new_coldkey,
                        import_source_policies,
                    );
                    // Stake has moved; migrate the bond so unstake guards stay attached.
                    Self::transfer_coldkey_miner_collateral(netuid, old_coldkey, new_coldkey)?;
                    Self::transfer_last_hotkey_swap_on_netuid(netuid, old_coldkey, new_coldkey);
                }
                Self::transfer_pending_network_registrations(old_coldkey, new_coldkey)?;
                Self::transfer_coldkey_rate_limits(old_coldkey, new_coldkey);
                Self::transfer_staking_hotkeys(old_coldkey, new_coldkey);
                Self::transfer_hotkeys_ownership(old_coldkey, new_coldkey)?;

                // Transfer stake locks. Destination must have no active locks (checked
                // above); decaying-lock flags keep the destination's value on conflict.
                Self::swap_coldkey_locks(old_coldkey, new_coldkey)?;

                // Clear source; restore the winning flags (dest when preserved).
                AccountFlags::<T>::remove(old_coldkey);
                let final_flags = if preserve_dest_flags {
                    saved_dest_flags
                } else {
                    saved_source_flags
                };
                if final_flags != 0 {
                    AccountFlags::<T>::insert(new_coldkey, final_flags);
                } else {
                    AccountFlags::<T>::remove(new_coldkey);
                }

                // Proxy grants, announcements, and RealPaysFee stay keyed by the old
                // coldkey: a source-only swap never moves delegation authority.
                Self::transfer_all_tao_and_kill(old_coldkey, new_coldkey)?;

                // Owner identity continuity for indexers / coldkey-keyed policy.
                // Only on a rename onto a key with no staking associations and no
                // established lineage: recording onto a dest that already has a
                // root/successor would forge same_coldkey_lineage without auth.
                if record_lineage {
                    Self::record_coldkey_swap_lineage(old_coldkey, new_coldkey);
                }

                Self::deposit_event(Event::ColdkeySwapped {
                    old_coldkey: old_coldkey.clone(),
                    new_coldkey: new_coldkey.clone(),
                });
                Ok(())
            })();

            match result {
                Ok(()) => TransactionOutcome::Commit(Ok(work)),
                Err(e) => TransactionOutcome::Rollback(Err(e)),
            }
        })
    }

    /// Move lease entitlements alongside the subnet's other coldkey-owned state.
    /// Called inside the swap transaction so proxy failures roll back the swap.
    fn transfer_coldkey_lease(
        lease_id: LeaseId,
        old_coldkey: &T::AccountId,
        new_coldkey: &T::AccountId,
    ) -> DispatchResult {
        if SubnetLeaseShares::<T>::contains_key(lease_id, old_coldkey) {
            let share = SubnetLeaseShares::<T>::take(lease_id, old_coldkey);
            SubnetLeaseShares::<T>::try_mutate(lease_id, new_coldkey, |destination| {
                *destination = destination
                    .checked_add(share)
                    .ok_or(sp_runtime::ArithmeticError::Overflow)?;
                Ok::<(), DispatchError>(())
            })?;
        }
        if SubnetLeaseUnpaidDividends::<T>::contains_key(lease_id, old_coldkey) {
            let unpaid = SubnetLeaseUnpaidDividends::<T>::take(lease_id, old_coldkey);
            SubnetLeaseUnpaidDividends::<T>::try_mutate(lease_id, new_coldkey, |destination| {
                let sum = u64::from(*destination)
                    .checked_add(u64::from(unpaid))
                    .ok_or(sp_runtime::ArithmeticError::Overflow)?;
                *destination = AlphaBalance::from(sum);
                Ok::<(), DispatchError>(())
            })?;
        }

        let mut lease = SubnetLeases::<T>::get(lease_id).ok_or(Error::<T>::LeaseDoesNotExist)?;
        if lease.beneficiary != *old_coldkey || old_coldkey == new_coldkey {
            return Ok(());
        }

        // A lease already handed over (terminated with deferred dividends still owed) has
        // no beneficiary proxy any more; only the record's beneficiary follows the coldkey.
        let handed_over = SubnetOwner::<T>::get(lease.netuid) == lease.beneficiary;
        if !handed_over {
            T::ProxyInterface::remove_lease_beneficiary_proxy(&lease.coldkey, old_coldkey)?;
            T::ProxyInterface::add_lease_beneficiary_proxy(&lease.coldkey, new_coldkey)?;
        }
        lease.beneficiary = new_coldkey.clone();
        SubnetLeases::<T>::insert(lease_id, lease);
        Ok(())
    }

    /// Charges the swap cost from the coldkey's account and recycles the tokens.
    pub fn charge_swap_cost(coldkey: &T::AccountId, swap_cost: TaoBalance) -> DispatchResult {
        Self::recycle_tao(coldkey, swap_cost)
            .map_err(|_| Error::<T>::NotEnoughBalanceToPaySwapColdKey)?;
        Ok(())
    }

    /// Transfer the ownership of the subnet to the new coldkey if it is owned by the old coldkey.
    fn transfer_subnet_ownership(
        netuid: NetUid,
        old_coldkey: &T::AccountId,
        new_coldkey: &T::AccountId,
    ) {
        let subnet_owner = SubnetOwner::<T>::get(netuid);
        if subnet_owner == *old_coldkey {
            SubnetOwner::<T>::insert(netuid, new_coldkey.clone());
        }
    }

    /// Transfer the auto stake destination from the old coldkey to the new coldkey if it is set.
    ///
    /// Destination wins when it already has an explicit setting. On an unsolicited
    /// merge into a destination that already has prior presence
    /// (`import_source_policy == false`), an unset destination keeps its default
    /// reward route — the source setting is dropped, not imported (Skeptic
    /// fid:5f22d507 / fid:a2c90852). Blank rename targets still carry the source
    /// policy forward.
    fn transfer_auto_stake_destination(
        netuid: NetUid,
        old_coldkey: &T::AccountId,
        new_coldkey: &T::AccountId,
        import_source_policy: bool,
    ) {
        let Some(old_auto_stake_hotkey) = AutoStakeDestination::<T>::take(old_coldkey, netuid)
        else {
            return;
        };

        // Drop the source from the reverse index regardless of who keeps the setting.
        AutoStakeDestinationColdkeys::<T>::mutate(&old_auto_stake_hotkey, netuid, |v| {
            v.retain(|c| *c != *old_coldkey);
        });

        if AutoStakeDestination::<T>::contains_key(new_coldkey, netuid) {
            // Destination already configured — leave it alone.
            return;
        }
        if !import_source_policy {
            // Merge into used dest with unset/default policy: do not import.
            return;
        }

        AutoStakeDestination::<T>::insert(new_coldkey, netuid, old_auto_stake_hotkey.clone());
        AutoStakeDestinationColdkeys::<T>::mutate(old_auto_stake_hotkey, netuid, |v| {
            v.retain(|c| *c != *new_coldkey);
            v.push(new_coldkey.clone());
        });
    }

    /// Preserve the later of the two hotkey-swap cooldown stamps so a merge cannot
    /// reset the destination's recovery window (and the source stamp is not orphaned).
    fn transfer_last_hotkey_swap_on_netuid(
        netuid: NetUid,
        old_coldkey: &T::AccountId,
        new_coldkey: &T::AccountId,
    ) {
        if !LastHotkeySwapOnNetuid::<T>::contains_key(netuid, old_coldkey) {
            return;
        }
        let old_block = LastHotkeySwapOnNetuid::<T>::take(netuid, old_coldkey);
        let new_block = LastHotkeySwapOnNetuid::<T>::get(netuid, new_coldkey);
        LastHotkeySwapOnNetuid::<T>::insert(netuid, new_coldkey, old_block.max(new_block));
    }

    /// Rewrite pending network-registration queue / escrow rows so a queued subnet
    /// is not owned by a drained coldkey after the swap.
    ///
    /// Escrow is updated only through queue `lock_id`s (one get+insert per owned
    /// row). Never walks the global escrow map — that cost grows with unrelated
    /// accounts' registrations and sits outside the swap weight envelope.
    ///
    /// Fails closed when the total queue exceeds
    /// [`crate::MAX_COLDKEY_SWAP_REGISTRATION_QUEUE_SCAN`] (admission already
    /// enforces this; re-check so execute cannot silently under-charge).
    fn transfer_pending_network_registrations(
        old_coldkey: &T::AccountId,
        new_coldkey: &T::AccountId,
    ) -> DispatchResult {
        if old_coldkey == new_coldkey {
            return Ok(());
        }
        // Refuse before `try_mutate` loads/decodes the Vec when the length
        // prefix alone already exceeds the scan cap (Auditor fid:185b04ac).
        let encoded_len = NetworkRegistrationQueue::<T>::decode_len().unwrap_or(0) as u32;
        ensure!(
            encoded_len <= crate::MAX_COLDKEY_SWAP_REGISTRATION_QUEUE_SCAN,
            Error::<T>::ColdkeySwapTooHeavy
        );
        NetworkRegistrationQueue::<T>::try_mutate(|queue| -> DispatchResult {
            ensure!(
                (queue.len() as u32) <= crate::MAX_COLDKEY_SWAP_REGISTRATION_QUEUE_SCAN,
                Error::<T>::ColdkeySwapTooHeavy
            );
            for info in queue.iter_mut() {
                if info.coldkey != *old_coldkey {
                    continue;
                }
                if let Some((_, amount)) = NetworkRegistrationEscrow::<T>::get(info.lock_id) {
                    NetworkRegistrationEscrow::<T>::insert(
                        info.lock_id,
                        (new_coldkey.clone(), amount),
                    );
                }
                info.coldkey = new_coldkey.clone();
            }
            Ok(())
        })
    }

    /// Move per-coldkey rate-limit clocks to the destination, keeping the later stamp
    /// so a merge cannot bypass a limit the destination already accrued.
    fn transfer_coldkey_rate_limits(old_coldkey: &T::AccountId, new_coldkey: &T::AccountId) {
        if old_coldkey == new_coldkey {
            return;
        }
        let keys = [
            RateLimitKey::LastTxBlock(old_coldkey.clone()),
            RateLimitKey::LastTxBlockChildKeyTake(old_coldkey.clone()),
            RateLimitKey::LastTxBlockDelegateTake(old_coldkey.clone()),
        ];
        let dest_keys = [
            RateLimitKey::LastTxBlock(new_coldkey.clone()),
            RateLimitKey::LastTxBlockChildKeyTake(new_coldkey.clone()),
            RateLimitKey::LastTxBlockDelegateTake(new_coldkey.clone()),
        ];
        for (old_key, new_key) in keys.into_iter().zip(dest_keys.into_iter()) {
            if !LastRateLimitedBlock::<T>::contains_key(&old_key) {
                continue;
            }
            let old_block = LastRateLimitedBlock::<T>::take(&old_key);
            let new_block = LastRateLimitedBlock::<T>::get(&new_key);
            LastRateLimitedBlock::<T>::insert(new_key, old_block.max(new_block));
        }
    }

    /// Move every stake position of `old_coldkey` to `new_coldkey`, one `(hotkey, netuid)`
    /// row at a time. Walking the coldkey's own rows (instead of every subnet for every
    /// hotkey) keeps the work proportional to what actually moves. Root bookkeeping —
    /// basket watermark and unlock age — is moved for every staking hotkey, stake or not:
    /// the signed watermark can be negative with zero stake (claim-then-unstake) and must
    /// follow the coldkey rather than be orphaned on the dead key.
    ///
    /// When the destination already holds a position on the same `(hotkey, netuid)`,
    /// `increase_stake_*` sums shares; `transfer_basket_claimed_for_new_coldkey` sums
    /// watermarks (anti double-claim, see GHSA-2026-010).
    fn transfer_coldkey_positions(
        old_coldkey: &T::AccountId,
        new_coldkey: &T::AccountId,
    ) -> DispatchResult {
        for hotkey in Self::coldkey_swap_hotkeys(old_coldkey) {
            let netuids: BTreeSet<NetUid> = Self::alpha_iter_prefix((&hotkey, old_coldkey))
                .map(|(netuid, _)| netuid)
                .collect();
            for netuid in netuids {
                let alpha_old =
                    Self::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, old_coldkey, netuid);
                if alpha_old.is_zero() {
                    continue;
                }
                // Credit the new coldkey with exactly what left the old one.
                let alpha_moved = Self::decrease_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey,
                    old_coldkey,
                    netuid,
                    alpha_old,
                );
                Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey,
                    new_coldkey,
                    netuid,
                    alpha_moved,
                );
            }

            Self::transfer_basket_claimed_for_new_coldkey(&hotkey, old_coldkey, new_coldkey)?;
            Self::migrate_root_stake_age(old_coldkey, &hotkey, new_coldkey, &hotkey);
            if !Self::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, new_coldkey, NetUid::ROOT)
                .is_zero()
            {
                Self::maybe_add_coldkey_index(new_coldkey);
            }
        }

        // All root stake has left the old coldkey; drop its auto-claim index entry.
        Self::maybe_remove_coldkey_index(old_coldkey);
        Ok(())
    }

    /// Transfer staking hotkeys from the old coldkey to the new coldkey (union).
    /// Drop claim-only basket credits for any hotkey now present in the staking list.
    fn transfer_staking_hotkeys(old_coldkey: &T::AccountId, new_coldkey: &T::AccountId) {
        let old_staking_hotkeys: Vec<T::AccountId> = StakingHotkeys::<T>::get(old_coldkey);
        let mut new_staking_hotkeys: Vec<T::AccountId> = StakingHotkeys::<T>::get(new_coldkey);
        for hotkey in old_staking_hotkeys {
            // If the hotkey is not already in the new coldkey, add it.
            if !new_staking_hotkeys.contains(&hotkey) {
                new_staking_hotkeys.push(hotkey);
            }
        }

        StakingHotkeys::<T>::remove(old_coldkey);
        if new_staking_hotkeys.is_empty() {
            StakingHotkeys::<T>::remove(new_coldkey);
        } else {
            StakingHotkeys::<T>::insert(new_coldkey, new_staking_hotkeys.clone());
        }

        // Credits are claim-only associations for hotkeys NOT on StakingHotkeys.
        // After the union, drop any credit that is now redundant.
        for hotkey in &new_staking_hotkeys {
            HotkeySwapBasketCredits::<T>::remove(new_coldkey, hotkey);
            HotkeySwapBasketCredits::<T>::remove(old_coldkey, hotkey);
        }
        // Move any remaining source-only credits onto the destination.
        let leftover_credits: Vec<T::AccountId> =
            HotkeySwapBasketCredits::<T>::iter_key_prefix(old_coldkey).collect();
        for hotkey in leftover_credits {
            HotkeySwapBasketCredits::<T>::remove(old_coldkey, &hotkey);
            HotkeySwapBasketCredits::<T>::insert(new_coldkey, hotkey, ());
        }
    }

    /// Transfer the ownership of the hotkeys owned by the old coldkey to the new coldkey.
    fn transfer_hotkeys_ownership(
        old_coldkey: &T::AccountId,
        new_coldkey: &T::AccountId,
    ) -> DispatchResult {
        let old_owned_hotkeys: Vec<T::AccountId> = OwnedHotkeys::<T>::get(old_coldkey);
        let mut new_owned_hotkeys: Vec<T::AccountId> = OwnedHotkeys::<T>::get(new_coldkey);
        for owned_hotkey in old_owned_hotkeys.iter() {
            // Remove the hotkey from the old coldkey.
            Owner::<T>::remove(owned_hotkey);
            // Add the hotkey to the new coldkey.
            Self::set_hotkey_owner(new_coldkey, owned_hotkey)?;
            // Add the owned hotkey to the new set of owned hotkeys.
            if !new_owned_hotkeys.contains(owned_hotkey) {
                new_owned_hotkeys.push(owned_hotkey.clone());
            }
        }
        OwnedHotkeys::<T>::remove(old_coldkey);
        OwnedHotkeys::<T>::insert(new_coldkey, new_owned_hotkeys);
        Ok(())
    }
}
