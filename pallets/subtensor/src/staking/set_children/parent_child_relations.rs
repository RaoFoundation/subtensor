//! Parent/child hotkey relation graph ([`ParentChildRelations`]).
//!
//! Maintains bipartite parent↔child edges with proportions, load/persist diffs
//! against `ChildKeys` / `ParentKeys`, and hotkey-swap rebinding.
use super::*;
use sp_std::collections::btree_map::BTreeMap;

pub struct ParentChildRelations<T: Config> {
    /// The distinguished `hotkey` this structure is built around.
    pivot: T::AccountId,
    children: BTreeMap<T::AccountId, u64>,
    parents: BTreeMap<T::AccountId, u64>,
}

impl<T: Config> ParentChildRelations<T> {
    /// Create empty relations for a given pivot.
    pub fn new(hotkey: T::AccountId) -> Self {
        Self {
            pivot: hotkey,
            children: BTreeMap::new(),
            parents: BTreeMap::new(),
        }
    }

    ////////////////////////////////////////////////////////////
    // Constraint checkers

    /// Ensures sum(proportions) <= u64::MAX
    pub fn ensure_total_proportions(children: &BTreeMap<T::AccountId, u64>) -> DispatchResult {
        let total: u128 = children
            .values()
            .fold(0u128, |acc, &w| acc.saturating_add(w as u128));
        ensure!(total <= u64::MAX as u128, Error::<T>::ProportionOverflow);
        Ok(())
    }

    /// Ensure that the number of children does not exceed 5
    pub fn ensure_childkey_count(children: &BTreeMap<T::AccountId, u64>) -> DispatchResult {
        ensure!(children.len() <= 5, Error::<T>::TooManyChildren);

        Ok(())
    }

    /// Ensures the given children or parent set doesn't contain pivot
    pub fn ensure_no_self_loop(
        pivot: &T::AccountId,
        hotkey_set: &BTreeMap<T::AccountId, u64>,
    ) -> DispatchResult {
        ensure!(!hotkey_set.contains_key(pivot), Error::<T>::InvalidChild);
        Ok(())
    }

    /// Ensures that children and parents sets do not have any overlap
    pub fn ensure_bipartite_separation(
        children: &BTreeMap<T::AccountId, u64>,
        parents: &BTreeMap<T::AccountId, u64>,
    ) -> DispatchResult {
        let has_overlap = children.keys().any(|c| parents.contains_key(c));
        ensure!(!has_overlap, Error::<T>::ChildParentInconsistency);
        Ok(())
    }

    /// Validate that applying `pending_children_vec` to `relations` (as the new
    /// pivot->children mapping) preserves all invariants.
    ///
    /// Checks:
    /// 1) No self-loop: pivot must not appear among children.
    /// 2) Sum of child proportions fits in `u64`.
    /// 3) Bipartite role separation: no child may also be a parent.
    pub fn ensure_pending_consistency(
        &self,
        pending_children_vec: &Vec<(u64, T::AccountId)>,
    ) -> DispatchResult {
        // Build a deduped children map (last proportion wins if duplicates present).
        let mut new_children: BTreeMap<T::AccountId, u64> = BTreeMap::new();
        for (prop, child) in pending_children_vec {
            new_children.insert(child.clone(), *prop);
        }

        // Check constraints
        Self::ensure_no_self_loop(&self.pivot, &new_children)?;
        Self::ensure_childkey_count(&new_children)?;
        Self::ensure_total_proportions(&new_children)?;
        Self::ensure_bipartite_separation(&new_children, &self.parents)?;

        Ok(())
    }

    ////////////////////////////////////////////////////////////
    // Getters

    #[inline]
    pub fn pivot(&self) -> &T::AccountId {
        &self.pivot
    }
    #[inline]
    pub fn children(&self) -> &BTreeMap<T::AccountId, u64> {
        &self.children
    }
    #[inline]
    pub fn parents(&self) -> &BTreeMap<T::AccountId, u64> {
        &self.parents
    }

    ////////////////////////////////////////////////////////////
    // Safe updaters

    /// Replace the pivot->children mapping after validating invariants.
    ///
    /// Invariants:
    /// * No self-loop: child != pivot
    /// * sum(proportions) fits in u64 (checked as u128 to avoid overflow mid-sum)
    pub fn link_children(&mut self, new_children: BTreeMap<T::AccountId, u64>) -> DispatchResult {
        // Check constraints
        Self::ensure_no_self_loop(&self.pivot, &new_children)?;
        Self::ensure_total_proportions(&new_children)?;
        Self::ensure_bipartite_separation(&new_children, &self.parents)?;

        self.children = new_children;
        Ok(())
    }

    pub fn link_parents(&mut self, new_parents: BTreeMap<T::AccountId, u64>) -> DispatchResult {
        // Check constraints
        Self::ensure_no_self_loop(&self.pivot, &new_parents)?;
        Self::ensure_bipartite_separation(&self.children, &new_parents)?;

        self.parents = new_parents;
        Ok(())
    }

    #[inline]
    pub(crate) fn upsert_edge(
        list: &mut Vec<(u64, T::AccountId)>,
        proportion: u64,
        id: &T::AccountId,
    ) {
        for (p, who) in list.iter_mut() {
            if who == id {
                *p = proportion;
                return;
            }
        }
        list.push((proportion, id.clone()));
    }

    #[inline]
    pub(crate) fn remove_edge(list: &mut Vec<(u64, T::AccountId)>, id: &T::AccountId) {
        list.retain(|(_, who)| who != id);
    }

    /// Change the pivot hotkey for these relations.
    /// Ensures there are no self-loops with the new pivot.
    pub fn rebind_pivot(&mut self, new_pivot: T::AccountId) -> DispatchResult {
        // No self-loop via children or parents for the new pivot.
        Self::ensure_no_self_loop(&new_pivot, &self.children)?;
        Self::ensure_no_self_loop(&new_pivot, &self.parents)?;

        self.pivot = new_pivot;
        Ok(())
    }
}
