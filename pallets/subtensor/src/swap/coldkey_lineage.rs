//! Global coldkey swap lineage.
//!
//! After a successful coldkey swap, owner-identity continuity is recorded so
//! indexers and policies can attribute stake/ownership to a stable root
//! without replaying archives. Unlike hotkey lineage, maps are global: a
//! coldkey swap moves economic identity across every subnet at once.
//!
//! Prefer [`Self::coldkey_root`] / [`Self::same_coldkey_lineage`] for identity
//! checks. [`Self::coldkey_lineage_tip`] is best-effort: successor edges are
//! cleared when a coldkey is written as a swap destination, but consumers
//! should still treat tip walks as advisory.

use super::*;

/// Safety bound when walking [`ColdkeySuccessor`] toward the live tip.
const MAX_COLDKEY_LINEAGE_DEPTH: u32 = 64;

impl<T: Config> Pallet<T> {
    /// Record that `old_coldkey` was renamed to `new_coldkey`.
    ///
    /// Sets the successor edge and copies the lineage root onto the new
    /// coldkey. The old coldkey's root entry is left unchanged so historical
    /// keys still resolve via [`Self::coldkey_root`]. Clears any stale
    /// outgoing successor on `new_coldkey` so it is the tip.
    pub fn record_coldkey_swap_lineage(old_coldkey: &T::AccountId, new_coldkey: &T::AccountId) {
        if old_coldkey == new_coldkey {
            return;
        }
        let root = Self::coldkey_root(old_coldkey);
        ColdkeySuccessor::<T>::remove(new_coldkey);
        ColdkeySuccessor::<T>::insert(old_coldkey, new_coldkey.clone());
        ColdkeyRoot::<T>::insert(new_coldkey, root);
    }

    /// Canonical (first) coldkey in this swap lineage for `coldkey`.
    pub fn coldkey_root(coldkey: &T::AccountId) -> T::AccountId {
        ColdkeyRoot::<T>::get(coldkey).unwrap_or_else(|| coldkey.clone())
    }

    /// Whether `a` and `b` share the same coldkey swap lineage root.
    pub fn same_coldkey_lineage(a: &T::AccountId, b: &T::AccountId) -> bool {
        Self::coldkey_root(a) == Self::coldkey_root(b)
    }

    /// Walk successors from `coldkey` to the current tip.
    ///
    /// Bounded to avoid pathological cycles; returns the last reachable key
    /// if the bound is hit. Prefer [`Self::coldkey_root`] for identity checks.
    pub fn coldkey_lineage_tip(coldkey: &T::AccountId) -> T::AccountId {
        let mut current = coldkey.clone();
        for _ in 0..MAX_COLDKEY_LINEAGE_DEPTH {
            match ColdkeySuccessor::<T>::get(&current) {
                Some(next) if next != current => current = next,
                _ => break,
            }
        }
        current
    }
}
