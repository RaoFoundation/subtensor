//! Per-subnet hotkey swap lineage.
//!
//! After a successful hotkey swap, identity continuity is recorded so
//! validators and indexers can ban/score a stable root without replaying
//! archives. Maps are keyed by netuid because a swap may move a UID on one
//! subnet while the old hotkey remains registered on others.
//!
//! On-chain helpers use [`HotkeySuccessor`] (tip walk) and [`HotkeyRoot`]
//! (O(1) identity compare). Reverse edges are reconstructible off-chain from
//! Successor; they are not stored.
//!
//! Prefer [`Self::hotkey_root`] / [`Self::same_hotkey_lineage`] for ban/score.
//! [`Self::hotkey_lineage_tip`] is best-effort: successor edges are cleared when
//! a hotkey is written as a swap destination, but consumers should still treat
//! tip walks as advisory.

use frame_support::weights::Weight;

use super::*;

/// Safety bound when walking [`HotkeySuccessor`] toward the live tip.
const MAX_HOTKEY_LINEAGE_DEPTH: u32 = 64;

impl<T: Config> Pallet<T> {
    /// Record cooldown + lineage for one subnet after a successful hotkey swap.
    ///
    /// Accrues DB weight for the root read and the four storage writes
    /// (`LastHotkeySwapOnNetuid`, successor clear, successor insert, root).
    pub fn record_hotkey_swap_on_netuid(
        netuid: NetUid,
        coldkey: &T::AccountId,
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
        block: u64,
        weight: &mut Weight,
    ) {
        LastHotkeySwapOnNetuid::<T>::insert(netuid, coldkey, block);
        Self::record_hotkey_swap_lineage(netuid, old_hotkey, new_hotkey);
        weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 4));
    }

    /// Record that `old_hotkey` was renamed to `new_hotkey` on `netuid`.
    ///
    /// Sets the successor edge and copies the lineage root onto the new
    /// hotkey. The old hotkey's root entry is left unchanged so historical
    /// keys still resolve via [`Self::hotkey_root`]. Clears any stale outgoing
    /// successor on `new_hotkey` so it is the tip (breaks A→B / B→A cycles).
    pub fn record_hotkey_swap_lineage(
        netuid: NetUid,
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
    ) {
        if old_hotkey == new_hotkey {
            return;
        }
        let root = Self::hotkey_root(netuid, old_hotkey);
        // Destination is the live tip: drop any prior outgoing edge (e.g. from a
        // previous life of this key, or a reverse swap that would cycle).
        HotkeySuccessor::<T>::remove(netuid, new_hotkey);
        HotkeySuccessor::<T>::insert(netuid, old_hotkey, new_hotkey.clone());
        HotkeyRoot::<T>::insert(netuid, new_hotkey, root);
    }

    /// Canonical (first) hotkey in this subnet's swap lineage for `hotkey`.
    pub fn hotkey_root(netuid: NetUid, hotkey: &T::AccountId) -> T::AccountId {
        HotkeyRoot::<T>::get(netuid, hotkey).unwrap_or_else(|| hotkey.clone())
    }

    /// Whether `a` and `b` share the same swap lineage root on `netuid`.
    pub fn same_hotkey_lineage(netuid: NetUid, a: &T::AccountId, b: &T::AccountId) -> bool {
        Self::hotkey_root(netuid, a) == Self::hotkey_root(netuid, b)
    }

    /// Walk successors from `hotkey` to the current tip on `netuid`.
    ///
    /// Bounded to avoid pathological cycles; returns the last reachable key
    /// if the bound is hit. Prefer [`Self::hotkey_root`] for identity checks.
    pub fn hotkey_lineage_tip(netuid: NetUid, hotkey: &T::AccountId) -> T::AccountId {
        let mut current = hotkey.clone();
        for _ in 0..MAX_HOTKEY_LINEAGE_DEPTH {
            match HotkeySuccessor::<T>::get(netuid, &current) {
                Some(next) if next != current => current = next,
                _ => break,
            }
        }
        current
    }
}
