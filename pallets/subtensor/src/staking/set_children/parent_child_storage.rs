//! Load/persist parent–child edges and swap them across a hotkey change.
use super::*;
use sp_std::collections::{btree_map::BTreeMap, btree_set::BTreeSet};
use subtensor_runtime_common::NetUid;

impl<T: Config> Pallet<T> {
    /// Set childkeys vector making sure there are no empty vectors in the state
    pub(crate) fn set_childkeys(
        parent: T::AccountId,
        netuid: NetUid,
        childkey_vec: Vec<(u64, T::AccountId)>,
    ) {
        if childkey_vec.is_empty() {
            ChildKeys::<T>::remove(parent, netuid);
        } else {
            ChildKeys::<T>::insert(parent, netuid, childkey_vec);
        }
    }

    /// Set parentkeys vector making sure there are no empty vectors in the state
    pub(crate) fn set_parentkeys(
        child: T::AccountId,
        netuid: NetUid,
        parentkey_vec: Vec<(u64, T::AccountId)>,
    ) {
        if parentkey_vec.is_empty() {
            ParentKeys::<T>::remove(child, netuid);
        } else {
            ParentKeys::<T>::insert(child, netuid, parentkey_vec);
        }
    }

    /// Loads all records from ChildKeys and ParentKeys where (hotkey, netuid) is the key.
    /// Produces a parent->(child->prop) adjacency map that **cannot violate**
    /// the required consistency because all inserts go through `link`.
    pub(crate) fn load_child_parent_relations(
        hotkey: &T::AccountId,
        netuid: NetUid,
    ) -> Result<ParentChildRelations<T>, DispatchError> {
        let mut rel = ParentChildRelations::<T>::new(hotkey.clone());

        // Load children: (prop, child) from ChildKeys(hotkey, netuid)
        let child_links = ChildKeys::<T>::get(hotkey, netuid);
        let mut children = BTreeMap::<T::AccountId, u64>::new();
        for (prop, child) in child_links {
            // Ignore any accidental self-loop in storage
            if child != *hotkey {
                children.insert(child, prop);
            }
        }
        // Validate & set (enforce no self-loop and sum limit)
        rel.link_children(children)?;

        // Load parents: (prop, parent) from ParentKeys(hotkey, netuid)
        let parent_links = ParentKeys::<T>::get(hotkey, netuid);
        let mut parents = BTreeMap::<T::AccountId, u64>::new();
        for (prop, parent) in parent_links {
            if parent != *hotkey {
                parents.insert(parent, prop);
            }
        }
        // Keep the same validation rules for parents (no self-loop, bounded sum).
        rel.link_parents(parents)?;

        Ok(rel)
    }

    /// Build a `ParentChildRelations` for `pivot` (parent) from the `PendingChildKeys` queue,
    /// preserving the current `ParentKeys(pivot, netuid)` so `persist_child_parent_relations`
    /// won’t accidentally clear existing parents.
    ///
    /// PendingChildKeys layout:
    ///   (netuid, pivot) -> (Vec<(proportion, child)>)
    pub fn load_relations_from_pending(
        pivot: T::AccountId,
        pending_children_vec: &Vec<(u64, T::AccountId)>,
        netuid: NetUid,
    ) -> Result<ParentChildRelations<T>, DispatchError> {
        let mut rel = ParentChildRelations::<T>::new(pivot.clone());

        // Deduplicate into a BTreeMap<child, weight> (last wins if duplicates).
        let mut children: BTreeMap<T::AccountId, u64> = BTreeMap::new();
        for (prop, child) in pending_children_vec {
            if *child != pivot {
                children.insert(child.clone(), *prop);
            }
        }

        // Enforce invariants (no self-loop, total weight <= u64::MAX)
        rel.link_children(children)?;

        // Preserve the current parents of the pivot so `persist_child_parent_relations`
        // won’t clear them when we only intend to update children.
        let existing_parents_vec = ParentKeys::<T>::get(pivot.clone(), netuid);
        let mut parents: BTreeMap<T::AccountId, u64> = BTreeMap::new();
        for (w, parent) in existing_parents_vec {
            if parent != pivot {
                parents.insert(parent, w);
            }
        }
        // This uses the same basic checks (no self-loop, bounded sum).
        // If you didn't expose link_parents, inline the simple validations here.
        rel.link_parents(parents)?;

        Ok(rel)
    }

    /// Persist the `relations` around `hotkey` to storage, updating both directions:
    /// * Writes ChildKeys(hotkey, netuid) = children
    ///   and synchronizes ParentKeys(child, netuid) entries accordingly.
    /// * Writes ParentKeys(hotkey, netuid) = parents
    ///   and synchronizes ChildKeys(parent, netuid) entries accordingly.
    ///
    /// This is a **diff-based** update that only touches affected neighbors.
    pub fn persist_child_parent_relations(
        relations: ParentChildRelations<T>,
        netuid: NetUid,
        weight: &mut Weight,
    ) -> DispatchResult {
        let pivot = relations.pivot().clone();

        // ---------------------------
        // 1) Pivot -> Children side
        // ---------------------------
        let new_children_map = relations.children();
        let new_children_vec: Vec<(u64, T::AccountId)> = new_children_map
            .iter()
            .map(|(c, p)| (*p, c.clone()))
            .collect();

        let prev_children_vec = ChildKeys::<T>::get(&pivot, netuid);
        weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 0));

        // Overwrite pivot's children vector
        Self::set_childkeys(pivot.clone(), netuid, new_children_vec.clone());
        weight.saturating_accrue(T::DbWeight::get().reads_writes(0, 1));

        // Build quick-lookup sets for diffing
        let prev_children_set: BTreeSet<T::AccountId> =
            prev_children_vec.iter().map(|(_, c)| c.clone()).collect();
        let new_children_set: BTreeSet<T::AccountId> = new_children_map.keys().cloned().collect();

        // Added children = new / prev
        for added in new_children_set
            .iter()
            .filter(|c| !prev_children_set.contains(*c))
        {
            let p = match new_children_map.get(added) {
                Some(p) => *p,
                None => return Err(Error::<T>::ChildParentInconsistency.into()),
            };
            let mut pk = ParentKeys::<T>::get(added.clone(), netuid);
            ParentChildRelations::<T>::upsert_edge(&mut pk, p, &pivot);
            Self::set_parentkeys(added.clone(), netuid, pk);
            weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 1));
        }

        // Updated children = intersection where proportion changed
        for common in new_children_set.intersection(&prev_children_set) {
            let new_p = match new_children_map.get(common) {
                Some(p) => *p,
                None => return Err(Error::<T>::ChildParentInconsistency.into()),
            };
            let mut pk = ParentKeys::<T>::get(common.clone(), netuid);
            ParentChildRelations::<T>::upsert_edge(&mut pk, new_p, &pivot);
            Self::set_parentkeys(common.clone(), netuid, pk);
            weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 1));
        }

        // Removed children = prev \ new  => remove (pivot) from ParentKeys(child)
        for removed in prev_children_set
            .iter()
            .filter(|c| !new_children_set.contains(*c))
        {
            let mut pk = ParentKeys::<T>::get(removed.clone(), netuid);
            ParentChildRelations::<T>::remove_edge(&mut pk, &pivot);
            Self::set_parentkeys(removed.clone(), netuid, pk);
            weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 1));
        }

        // ---------------------------
        // 2) Parents -> Pivot side
        // ---------------------------
        let new_parents_map = relations.parents();
        let new_parents_vec: Vec<(u64, T::AccountId)> = new_parents_map
            .iter()
            .map(|(p, pr)| (*pr, p.clone()))
            .collect();

        let prev_parents_vec = ParentKeys::<T>::get(&pivot, netuid);

        // Overwrite pivot's parents vector
        Self::set_parentkeys(pivot.clone(), netuid, new_parents_vec.clone());

        let prev_parents_set: BTreeSet<T::AccountId> =
            prev_parents_vec.into_iter().map(|(_, p)| p).collect();
        let new_parents_set: BTreeSet<T::AccountId> = new_parents_map.keys().cloned().collect();

        // Added parents = new / prev  => ensure ChildKeys(parent) has (p, pivot)
        for added in new_parents_set
            .iter()
            .filter(|p| !prev_parents_set.contains(*p))
        {
            let p_val = match new_parents_map.get(added) {
                Some(p) => *p,
                None => return Err(Error::<T>::ChildParentInconsistency.into()),
            };
            let mut ck = ChildKeys::<T>::get(added.clone(), netuid);
            ParentChildRelations::<T>::upsert_edge(&mut ck, p_val, &pivot);
            Self::set_childkeys(added.clone(), netuid, ck);
            weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 1));
        }

        // Updated parents = intersection where proportion changed
        for common in new_parents_set.intersection(&prev_parents_set) {
            let new_p = new_parents_map
                .get(common)
                .ok_or(Error::<T>::ChildParentInconsistency)?;
            let mut ck = ChildKeys::<T>::get(common.clone(), netuid);
            ParentChildRelations::<T>::upsert_edge(&mut ck, *new_p, &pivot);
            Self::set_childkeys(common.clone(), netuid, ck);
            weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 1));
        }

        // Removed parents = prev \ new  => remove (pivot) from ChildKeys(parent)
        for removed in prev_parents_set
            .iter()
            .filter(|p| !new_parents_set.contains(*p))
        {
            let mut ck = ChildKeys::<T>::get(removed.clone(), netuid);
            ParentChildRelations::<T>::remove_edge(&mut ck, &pivot);
            Self::set_childkeys(removed.clone(), netuid, ck);
            weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 1));
        }

        Ok(())
    }

    /// Swap all parent/child relations from `old_hotkey` to `new_hotkey` on `netuid`.
    /// Steps:
    ///  1) Load relations around `old_hotkey`
    ///  2) Clean up storage references to `old_hotkey` (both directions)
    ///  3) Rebind pivot to `new_hotkey`
    ///  4) Persist relations around `new_hotkey`
    pub fn parent_child_swap_hotkey(
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
        netuid: NetUid,
        weight: &mut Weight,
    ) -> DispatchResult {
        // 1) Load the current relations around old_hotkey
        let mut relations = Self::load_child_parent_relations(old_hotkey, netuid)?;
        weight.saturating_accrue(T::DbWeight::get().reads_writes(2, 0));

        // 2) Clean up all storage entries that reference old_hotkey
        //    2a) For each child of old_hotkey: remove old_hotkey from ParentKeys(child, netuid)
        for (child, _) in relations.children().iter() {
            let mut pk = ParentKeys::<T>::get(child.clone(), netuid);
            ParentChildRelations::<T>::remove_edge(&mut pk, old_hotkey);
            Self::set_parentkeys(child.clone(), netuid, pk.clone());
            weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 1));
        }
        //    2b) For each parent of old_hotkey: remove old_hotkey from ChildKeys(parent, netuid)
        for (parent, _) in relations.parents().iter() {
            let mut ck = ChildKeys::<T>::get(parent.clone(), netuid);
            ParentChildRelations::<T>::remove_edge(&mut ck, old_hotkey);
            ChildKeys::<T>::insert(parent.clone(), netuid, ck);
            weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 1));
        }
        //    2c) Clear direct maps of old_hotkey
        ChildKeys::<T>::insert(
            old_hotkey.clone(),
            netuid,
            Vec::<(u64, T::AccountId)>::new(),
        );
        Self::set_parentkeys(
            old_hotkey.clone(),
            netuid,
            Vec::<(u64, T::AccountId)>::new(),
        );
        weight.saturating_accrue(T::DbWeight::get().reads_writes(0, 2));

        // 3) Rebind pivot to new_hotkey (validate no self-loop with existing maps)
        relations.rebind_pivot(new_hotkey.clone())?;

        // 4) Swap PendingChildKeys( netuid, parent ) --> Vec<(proportion,child), cool_down_block>
        // Fail if consistency breaks
        if PendingChildKeys::<T>::contains_key(netuid, old_hotkey) {
            let (children, cool_down_block) = PendingChildKeys::<T>::get(netuid, old_hotkey);
            relations.ensure_pending_consistency(&children)?;

            PendingChildKeys::<T>::remove(netuid, old_hotkey);
            PendingChildKeys::<T>::insert(netuid, new_hotkey, (children, cool_down_block));
            weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 2));
        }

        // 5) Persist relations under the new pivot (diffs vs existing state at new_hotkey)
        Self::persist_child_parent_relations(relations, netuid, weight)
    }
}
