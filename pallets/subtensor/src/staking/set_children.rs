use super::*;

use sp_std::collections::{btree_map::BTreeMap, btree_set::BTreeSet};
use subtensor_runtime_common::NetUid;

pub struct PCRelations<T: Config> {
    /// The distinguished `hotkey` this structure is built around.
    pivot: T::AccountId,
    children: BTreeMap<T::AccountId, u64>,
    parents: BTreeMap<T::AccountId, u64>,
}

impl<T: Config> PCRelations<T> {
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
    /// - No self-loop: child != pivot
    /// - sum(proportions) fits in u64 (checked as u128 to avoid overflow mid-sum)
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
    fn upsert_edge(list: &mut Vec<(u64, T::AccountId)>, proportion: u64, id: &T::AccountId) {
        for (p, who) in list.iter_mut() {
            if who == id {
                *p = proportion;
                return;
            }
        }
        list.push((proportion, id.clone()));
    }

    #[inline]
    fn remove_edge(list: &mut Vec<(u64, T::AccountId)>, id: &T::AccountId) {
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

impl<T: Config> Pallet<T> {
    /// Set childkeys vector making sure there are no empty vectors in the state
    fn set_childkeys(parent: T::AccountId, netuid: NetUid, childkey_vec: Vec<(u64, T::AccountId)>) {
        if childkey_vec.is_empty() {
            ChildKeys::<T>::remove(parent, netuid);
        } else {
            ChildKeys::<T>::insert(parent, netuid, childkey_vec);
        }
    }

    /// Set parentkeys vector making sure there are no empty vectors in the state
    fn set_parentkeys(
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
    fn load_child_parent_relations(
        hotkey: &T::AccountId,
        netuid: NetUid,
    ) -> Result<PCRelations<T>, DispatchError> {
        let mut rel = PCRelations::<T>::new(hotkey.clone());

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

    /// Build a `PCRelations` for `pivot` (parent) from the `PendingChildKeys` queue,
    /// preserving the current `ParentKeys(pivot, netuid)` so `persist_child_parent_relations`
    /// won’t accidentally clear existing parents.
    ///
    /// PendingChildKeys layout:
    ///   (netuid, pivot) -> (Vec<(proportion, child)>)
    pub fn load_relations_from_pending(
        pivot: T::AccountId,
        pending_children_vec: &Vec<(u64, T::AccountId)>,
        netuid: NetUid,
    ) -> Result<PCRelations<T>, DispatchError> {
        let mut rel = PCRelations::<T>::new(pivot.clone());

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
    /// - Writes ChildKeys(hotkey, netuid) = children
    ///   and synchronizes ParentKeys(child, netuid) entries accordingly.
    /// - Writes ParentKeys(hotkey, netuid) = parents
    ///   and synchronizes ChildKeys(parent, netuid) entries accordingly.
    ///
    /// This is a **diff-based** update that only touches affected neighbors.
    pub fn persist_child_parent_relations(
        relations: PCRelations<T>,
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
            PCRelations::<T>::upsert_edge(&mut pk, p, &pivot);
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
            PCRelations::<T>::upsert_edge(&mut pk, new_p, &pivot);
            Self::set_parentkeys(common.clone(), netuid, pk);
            weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 1));
        }

        // Removed children = prev \ new  => remove (pivot) from ParentKeys(child)
        for removed in prev_children_set
            .iter()
            .filter(|c| !new_children_set.contains(*c))
        {
            let mut pk = ParentKeys::<T>::get(removed.clone(), netuid);
            PCRelations::<T>::remove_edge(&mut pk, &pivot);
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
            PCRelations::<T>::upsert_edge(&mut ck, p_val, &pivot);
            Self::set_childkeys(added.clone(), netuid, ck);
            weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 1));
        }

        // Updated parents = intersection where proportion changed
        for common in new_parents_set.intersection(&prev_parents_set) {
            let new_p = new_parents_map
                .get(common)
                .ok_or(Error::<T>::ChildParentInconsistency)?;
            let mut ck = ChildKeys::<T>::get(common.clone(), netuid);
            PCRelations::<T>::upsert_edge(&mut ck, *new_p, &pivot);
            Self::set_childkeys(common.clone(), netuid, ck);
            weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 1));
        }

        // Removed parents = prev \ new  => remove (pivot) from ChildKeys(parent)
        for removed in prev_parents_set
            .iter()
            .filter(|p| !new_parents_set.contains(*p))
        {
            let mut ck = ChildKeys::<T>::get(removed.clone(), netuid);
            PCRelations::<T>::remove_edge(&mut ck, &pivot);
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
            PCRelations::<T>::remove_edge(&mut pk, old_hotkey);
            Self::set_parentkeys(child.clone(), netuid, pk.clone());
            weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 1));
        }
        //    2b) For each parent of old_hotkey: remove old_hotkey from ChildKeys(parent, netuid)
        for (parent, _) in relations.parents().iter() {
            let mut ck = ChildKeys::<T>::get(parent.clone(), netuid);
            PCRelations::<T>::remove_edge(&mut ck, old_hotkey);
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

    /// ---- The implementation for the extrinsic do_set_child_singular: Sets a single child.
    /// This function allows a coldkey to set children keys.
    ///
    /// Adds a childkey vector to the PendingChildKeys map and performs a few checks:
    ///    **Signature Verification**: Ensures that the caller has signed the transaction, verifying the coldkey.
    ///    **Root Network Check**: Ensures that the delegation is not on the root network, as child hotkeys are not valid on the root.
    ///    **Network Existence Check**: Ensures that the specified network exists.
    ///    **Ownership Verification**: Ensures that the coldkey owns the hotkey.
    ///    **Hotkey Account Existence Check**: Ensures that the hotkey account already exists.
    ///    **Child count**: Only allow to add up to 5 children per parent
    ///    **Child-Hotkey Distinction**: Ensures that the child is not the same as the hotkey.
    ///    **Minimum stake**: Ensures that the parent key has at least the minimum stake.
    ///    **Proportion check**: Ensure that the sum of the proportions does not exceed u64::MAX.
    ///    **Duplicate check**: Ensure there are no duplicates in the list of children.
    ///
    /// # Events:
    /// * `SetChildrenScheduled`:
    ///     - If all checks pass and setting the childkeys is scheduled.
    ///
    /// # Errors:
    /// * `MechanismDoesNotExist`:
    ///     - Attempting to register to a non-existent network.
    /// * `RegistrationNotPermittedOnRootSubnet`:
    ///     - Attempting to register a child on the root network.
    /// * `NonAssociatedColdKey`:
    ///     - The coldkey does not own the hotkey or the child is the same as the hotkey.
    /// * `HotKeyAccountNotExists`:
    ///     - The hotkey account does not exist.
    /// * `TooManyChildren`:
    ///     - Too many children in request
    ///
    pub fn do_schedule_children(
        origin: OriginFor<T>,
        hotkey: T::AccountId,
        netuid: NetUid,
        children: Vec<(u64, T::AccountId)>,
    ) -> DispatchResult {
        // Check that the caller has signed the transaction. (the coldkey of the pairing)
        let coldkey = ensure_signed(origin)?;
        log::trace!(
            "do_set_children( coldkey:{coldkey:?} hotkey:{netuid:?} netuid:{hotkey:?} children:{children:?} )"
        );

        // Ensure the hotkey passes the rate limit.
        ensure!(
            TransactionType::SetChildren.passes_rate_limit_on_subnet::<T>(
                &hotkey, // Specific to a hotkey.
                netuid,  // Specific to a subnet.
            ),
            Error::<T>::TxRateLimitExceeded
        );

        // Check that this delegation is not on the root network. Child hotkeys are not valid on root.
        ensure!(
            !netuid.is_root(),
            Error::<T>::RegistrationNotPermittedOnRootSubnet
        );

        // Check that the network we are trying to create the child on exists.
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);

        // Check that the coldkey owns the hotkey.
        ensure!(
            Self::coldkey_owns_hotkey(&coldkey, &hotkey),
            Error::<T>::NonAssociatedColdKey
        );

        // Ensure there are no duplicates in the list of children.
        let mut unique_children = Vec::new();
        for (_, child_i) in &children {
            ensure!(
                !unique_children.contains(child_i),
                Error::<T>::DuplicateChild
            );
            unique_children.push(child_i.clone());
        }

        // Ensure we don't break consistency when these new childkeys are set:
        //  - Ensure that the number of children does not exceed 5
        //  - Each child is not the hotkey.
        //  - The sum of the proportions does not exceed u64::MAX.
        //  - Bipartite separation (no A <-> B relations)
        let relations = Self::load_child_parent_relations(&hotkey, netuid)?;
        relations.ensure_pending_consistency(&children)?;

        // Check that the parent key has at least the minimum own stake
        // if children vector is not empty
        // (checking with check_weights_min_stake wouldn't work because it considers
        // grandparent stake in this case)
        ensure!(
            children.is_empty()
                || Self::get_total_stake_for_hotkey(&hotkey) >= StakeThreshold::<T>::get().into()
                || SubnetOwnerHotkey::<T>::try_get(netuid)
                    .is_ok_and(|owner_hotkey| owner_hotkey.eq(&hotkey)),
            Error::<T>::NotEnoughStakeToSetChildkeys
        );

        // Set last transaction block
        let current_block = Self::get_current_block_as_u64();
        TransactionType::SetChildren.set_last_block_on_subnet::<T>(&hotkey, netuid, current_block);

        // Schedule or immediately apply CK
        Self::schedule_or_apply_ck(netuid, hotkey, children)
    }

    /// If the start call occured, schedule children, otherwise,
    /// apply immediately
    fn schedule_or_apply_ck(
        netuid: NetUid,
        hotkey: T::AccountId,
        children: Vec<(u64, T::AccountId)>,
    ) -> DispatchResult {
        if !SubtokenEnabled::<T>::get(netuid) {
            Self::persist_pending_chidren_ok(netuid, &hotkey, &children);
            return Ok(());
        }

        // Calculate cool-down block
        let cooldown_block =
            Self::get_current_block_as_u64().saturating_add(PendingChildKeyCooldown::<T>::get());

        // Insert or update PendingChildKeys
        PendingChildKeys::<T>::insert(netuid, hotkey.clone(), (children.clone(), cooldown_block));

        // Log and return.
        log::trace!(
            "SetChildrenScheduled( netuid:{:?}, cooldown_block:{:?}, hotkey:{:?}, children:{:?} )",
            cooldown_block,
            hotkey,
            netuid,
            children.clone()
        );
        Self::deposit_event(Event::SetChildrenScheduled(
            hotkey,
            netuid,
            cooldown_block,
            children,
        ));

        // Ok and return.
        Ok(())
    }

    /// This function executes setting children keys when called during hotkey draining.
    ///
    /// * `netuid` (u16):
    ///     - The u16 network identifier where the child keys will exist.
    ///
    /// # Events:
    /// * `SetChildren`:
    ///     - On successfully registering children to a hotkey.
    ///
    /// # Errors:
    /// * `MechanismDoesNotExist`:
    ///     - Attempting to register to a non-existent network.
    /// * `RegistrationNotPermittedOnRootSubnet`:
    ///     - Attempting to register a child on the root network.
    /// * `NonAssociatedColdKey`:
    ///     - The coldkey does not own the hotkey or the child is the same as the hotkey.
    /// * `HotKeyAccountNotExists`:
    ///     - The hotkey account does not exist.
    ///
    /// # Detailed Explanation of actions:
    /// 1. **Old Children Cleanup**: Removes the hotkey from the parent list of its old children.
    /// 2. **New Children Assignment**: Assigns the new child to the hotkey and updates the parent list for the new child.
    ///
    pub fn do_set_pending_children(netuid: NetUid) {
        let current_block = Self::get_current_block_as_u64();

        // If the childkey cools down before the subnet start call + PendingChildKeyCooldown:
        //   - If Start call happened: Normal track
        //   - If Start call didn't happen: Apply immediately
        // TODO: This check may be removed after all ck are applied after the runtime upgrade
        let start_call_occured = SubtokenEnabled::<T>::get(netuid);

        // Iterate over all pending children of this subnet and set as needed
        let mut to_remove: Vec<T::AccountId> = Vec::new();

        PendingChildKeys::<T>::iter_prefix(netuid).for_each(
            |(hotkey, (children, cool_down_block))| {
                if (cool_down_block < current_block) || !start_call_occured {
                    Self::persist_pending_chidren_ok(netuid, &hotkey, &children);
                    to_remove.push(hotkey);
                }
            },
        );

        for hotkey in to_remove {
            PendingChildKeys::<T>::remove(netuid, hotkey);
        }
    }

    // If child-parent consistency is broken, fail setting new children silently
    fn persist_pending_chidren_ok(
        netuid: NetUid,
        hotkey: &T::AccountId,
        children: &Vec<(u64, T::AccountId)>,
    ) {
        let maybe_relations = Self::load_relations_from_pending(hotkey.clone(), children, netuid);
        if let Ok(relations) = maybe_relations {
            let mut _weight: Weight = T::DbWeight::get().reads(0);
            if let Ok(()) = Self::persist_child_parent_relations(relations, netuid, &mut _weight) {
                // Log and emit event.
                log::trace!(
                    "SetChildren( netuid:{:?}, hotkey:{:?}, children:{:?} )",
                    hotkey,
                    netuid,
                    children.clone()
                );
                Self::deposit_event(Event::SetChildren(hotkey.clone(), netuid, children.clone()));
            }
        }
    }

    /* Retrieves the list of children for a given hotkey and network.
    ///
    /// # Arguments
    /// * `hotkey` - The hotkey whose children are to be retrieved.
    /// * `netuid` - The network identifier.
    ///
    /// # Returns
    /// * `Vec<(u64, T::AccountId)>` - A vector of tuples containing the proportion and child account ID.
    ///
    /// # Example
    /// ```
    /// let children = SubtensorModule::get_children(&hotkey, netuid);
     */
    pub fn get_children(hotkey: &T::AccountId, netuid: NetUid) -> Vec<(u64, T::AccountId)> {
        ChildKeys::<T>::get(hotkey, netuid)
    }

    /* Retrieves the list of parents for a given child and network.
    ///
    /// # Arguments
    /// * `child` - The child whose parents are to be retrieved.
    /// * `netuid` - The network identifier.
    ///
    /// # Returns
    /// * `Vec<(u64, T::AccountId)>` - A vector of tuples containing the proportion and parent account ID.
    ///
    /// # Example
    /// ```
    /// let parents = SubtensorModule::get_parents(&child, netuid);
     */
    pub fn get_parents(child: &T::AccountId, netuid: NetUid) -> Vec<(u64, T::AccountId)> {
        ParentKeys::<T>::get(child, netuid)
    }

    /// Sets the childkey take for a given hotkey.
    ///
    /// This function allows a coldkey to set the childkey take for a given hotkey.
    /// The childkey take determines the proportion of stake that the hotkey keeps for itself
    /// when distributing stake to its children.
    ///
    /// # Arguments:
    /// * `coldkey` (T::AccountId):
    ///     - The coldkey that owns the hotkey.
    ///
    /// * `hotkey` (T::AccountId):
    ///     - The hotkey for which the childkey take will be set.
    ///
    /// * `take` (u16):
    ///     - The new childkey take value. This is a percentage represented as a value between 0 and 10000,
    ///       where 10000 represents 100%.
    ///
    /// # Returns:
    /// * `DispatchResult` - The result of the operation.
    ///
    /// # Errors:
    /// * `NonAssociatedColdKey`:
    ///     - The coldkey does not own the hotkey.
    /// * `InvalidChildkeyTake`:
    ///     - The provided take value is invalid (greater than the maximum allowed take).
    /// * `TxChildkeyTakeRateLimitExceeded`:
    ///     - The rate limit for changing childkey take has been exceeded.
    pub fn do_set_childkey_take(
        coldkey: T::AccountId,
        hotkey: T::AccountId,
        netuid: NetUid,
        take: u16,
    ) -> DispatchResult {
        // Ensure the coldkey owns the hotkey
        ensure!(
            Self::coldkey_owns_hotkey(&coldkey, &hotkey),
            Error::<T>::NonAssociatedColdKey
        );

        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);

        // Ensure the take value is valid
        ensure!(
            take >= Self::get_effective_min_childkey_take(netuid)
                && take <= Self::get_max_childkey_take(),
            Error::<T>::InvalidChildkeyTake
        );

        let current_take = Self::get_childkey_take(&hotkey, netuid);
        // Check the rate limit for increasing childkey take case
        if take > current_take {
            // Ensure the hotkey passes the rate limit.
            ensure!(
                TransactionType::SetChildkeyTake.passes_rate_limit_on_subnet::<T>(
                    &hotkey, // Specific to a hotkey.
                    netuid,  // Specific to a subnet.
                ),
                Error::<T>::TxChildkeyTakeRateLimitExceeded
            );
        }

        // Set last transaction block
        let current_block = Self::get_current_block_as_u64();
        TransactionType::SetChildkeyTake.set_last_block_on_subnet::<T>(
            &hotkey,
            netuid,
            current_block,
        );

        // Set the new childkey take value for the given hotkey and network
        ChildkeyTake::<T>::insert(hotkey.clone(), netuid, take);

        // Update the last transaction block
        TransactionType::SetChildkeyTake.set_last_block_on_subnet::<T>(
            &hotkey,
            netuid,
            current_block,
        );

        // Emit the event
        Self::deposit_event(Event::ChildKeyTakeSet(hotkey.clone(), take));
        log::debug!("Childkey take set for hotkey: {hotkey:?} and take: {take:?}");
        Ok(())
    }

    /// Gets the childkey take for a given hotkey.
    ///
    /// This function retrieves the current childkey take value for a specified hotkey.
    /// If no specific take value has been set, it returns the default childkey take.
    ///
    /// # Arguments:
    /// * `hotkey` (&T::AccountId): The hotkey for which to retrieve the childkey take.
    ///
    /// # Returns:
    /// * `u16`
    ///     - The childkey take value. This is a percentage represented as a value between 0
    ///       and 10000, where 10000 represents 100%.
    pub fn get_childkey_take(hotkey: &T::AccountId, netuid: NetUid) -> u16 {
        ChildkeyTake::<T>::get(hotkey, netuid).max(Self::get_effective_min_childkey_take(netuid))
    }

    pub fn get_auto_parent_delegation_enabled(root_validator_hotkey: &T::AccountId) -> bool {
        AutoParentDelegationEnabled::<T>::get(root_validator_hotkey)
    }

    ////////////////////////////////////////////////////////////
    // State cleaners (for use in migration)
    // TODO: Deprecate when the state is clean for a while

    /// Establishes parent-child relationships between all root validators and
    /// a subnet owner's hotkey on the specified subnet.
    ///
    /// For each validator on the root network (netuid 0), this function calls
    /// `do_schedule_children` to schedule the subnet owner hotkey as a child
    /// of that root validator on the given subnet, with full proportion (u64::MAX).
    ///
    /// # Arguments
    /// * `netuid` - The subnet on which to establish relationships.
    ///
    /// # Returns
    /// * `DispatchResult` - Ok if at least the setup completes; individual
    ///   scheduling failures per validator are logged but do not abort the loop.
    pub fn do_set_root_validators_for_subnet(netuid: NetUid) -> DispatchResult {
        // Cannot set children on root network itself.
        ensure!(
            !netuid.is_root(),
            Error::<T>::RegistrationNotPermittedOnRootSubnet
        );

        // Subnet must exist.
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);

        // Get the subnet owner hotkey.
        let subnet_owner_hotkey =
            SubnetOwnerHotkey::<T>::try_get(netuid).map_err(|_| Error::<T>::SubnetNotExists)?;

        // Iterate over all root validators and schedule each one as a parent
        // of the subnet owner hotkey.
        for (_uid, root_validator_hotkey) in Keys::<T>::iter_prefix(NetUid::ROOT) {
            // Skip if the root validator is the subnet owner hotkey itself
            // (cannot be both parent and child).
            if root_validator_hotkey == subnet_owner_hotkey {
                continue;
            }

            // Skip if root validator disabled auto parent delegation via AutoParentDelegationEnabled flag
            if !Self::get_auto_parent_delegation_enabled(&root_validator_hotkey) {
                continue;
            }

            // Look up the coldkey that owns this root validator hotkey.
            let coldkey = Self::get_owning_coldkey_for_hotkey(&root_validator_hotkey);

            // Build a signed origin from the coldkey.
            let origin: <T as frame_system::Config>::RuntimeOrigin =
                frame_system::RawOrigin::Signed(coldkey).into();

            // Schedule the subnet owner hotkey as a child with full proportion.
            let children = vec![(u64::MAX, subnet_owner_hotkey.clone())];

            if let Err(e) =
                Self::do_schedule_children(origin, root_validator_hotkey.clone(), netuid, children)
            {
                log::warn!(
                    "Failed to schedule children for root validator {:?} on netuid {:?}: {:?}",
                    root_validator_hotkey,
                    netuid,
                    e
                );
            }
        }

        Ok(())
    }

    pub fn clean_zero_childkey_vectors(weight: &mut Weight) {
        // Collect keys to delete first to avoid mutating while iterating.
        let mut to_remove: Vec<(T::AccountId, NetUid)> = Vec::new();

        for (parent, netuid, children) in ChildKeys::<T>::iter() {
            // Account for the read
            *weight = weight.saturating_add(T::DbWeight::get().reads(1));

            if children.is_empty() {
                to_remove.push((parent, netuid));
            }
        }

        // Remove all empty entries
        for (parent, netuid) in &to_remove {
            ChildKeys::<T>::remove(parent, netuid);
            // Account for the write
            *weight = weight.saturating_add(T::DbWeight::get().writes(1));
        }
        log::info!(
            target: "runtime",
            "Removed {} empty childkey vectors.",
            to_remove.len()
        );
    }

    /// Remove self-loops in `ChildKeys` and `ParentKeys`.
    /// If, after removal, a value-vector becomes empty, the storage key is removed.
    pub fn clean_self_loops(weight: &mut Weight) {
        // -------------------------------
        // 1) ChildKeys: (parent, netuid) -> Vec<(w, child)>
        //    Remove any entries where child == parent.
        // -------------------------------
        let mut to_update_ck: Vec<((T::AccountId, NetUid), Vec<(u64, T::AccountId)>)> = Vec::new();
        let mut to_remove_ck: Vec<(T::AccountId, NetUid)> = Vec::new();

        for (parent, netuid, children) in ChildKeys::<T>::iter() {
            *weight = weight.saturating_add(T::DbWeight::get().reads(1));

            // Filter out self-loops
            let filtered: Vec<(u64, T::AccountId)> = children
                .clone()
                .into_iter()
                .filter(|(_, c)| *c != parent)
                .collect();

            // If nothing changed, skip
            // (we can detect by comparing lengths; safer is to re-check if any removed existed)
            // For simplicity, just compare lengths:
            // If len unchanged and the previous vector had no self-loop, skip.
            // If there *was* a self-loop and filtered is empty, we'll remove the key.
            if filtered.len() == children.len() {
                // No change -> continue
                continue;
            }

            if filtered.is_empty() {
                to_remove_ck.push((parent, netuid));
            } else {
                to_update_ck.push(((parent, netuid), filtered));
            }
        }

        // Apply ChildKeys updates/removals
        for ((parent, netuid), new_vec) in &to_update_ck {
            Self::set_childkeys(parent.clone(), *netuid, new_vec.clone());
            *weight = weight.saturating_add(T::DbWeight::get().writes(1));
        }
        for (parent, netuid) in &to_remove_ck {
            ChildKeys::<T>::remove(parent, netuid);
            *weight = weight.saturating_add(T::DbWeight::get().writes(1));
        }
        log::info!(
            target: "runtime",
            "Removed {} self-looping childkeys.",
            to_update_ck.len().saturating_add(to_remove_ck.len())
        );

        // -------------------------------
        // 2) ParentKeys: (child, netuid) -> Vec<(w, parent)>
        //    Remove any entries where parent == child.
        // -------------------------------
        let mut to_update_pk: Vec<((T::AccountId, NetUid), Vec<(u64, T::AccountId)>)> = Vec::new();
        let mut to_remove_pk: Vec<(T::AccountId, NetUid)> = Vec::new();

        for (child, netuid, parents) in ParentKeys::<T>::iter() {
            *weight = weight.saturating_add(T::DbWeight::get().reads(1));

            // Filter out self-loops
            let filtered: Vec<(u64, T::AccountId)> = parents
                .clone()
                .into_iter()
                .filter(|(_, p)| *p != child)
                .collect();

            // If unchanged, skip
            if filtered.len() == parents.len() {
                continue;
            }

            if filtered.is_empty() {
                to_remove_pk.push((child, netuid));
            } else {
                to_update_pk.push(((child, netuid), filtered));
            }
        }

        // Apply ParentKeys updates/removals
        for ((child, netuid), new_vec) in &to_update_pk {
            Self::set_parentkeys(child.clone(), *netuid, new_vec.clone());
            *weight = weight.saturating_add(T::DbWeight::get().writes(1));
        }
        for (child, netuid) in &to_remove_pk {
            ParentKeys::<T>::remove(child, netuid);
            *weight = weight.saturating_add(T::DbWeight::get().writes(1));
        }
        log::info!(
            target: "runtime",
            "Removed {} self-looping parentkeys.",
            to_update_pk.len().saturating_add(to_remove_pk.len())
        );
    }

    pub fn clean_zero_parentkey_vectors(weight: &mut Weight) {
        // Collect keys to delete first to avoid mutating while iterating.
        let mut to_remove: Vec<(T::AccountId, NetUid)> = Vec::new();

        for (parent, netuid, children) in ParentKeys::<T>::iter() {
            // Account for the read
            *weight = weight.saturating_add(T::DbWeight::get().reads(1));

            if children.is_empty() {
                to_remove.push((parent, netuid));
            }
        }

        // Remove all empty entries
        for (parent, netuid) in &to_remove {
            ParentKeys::<T>::remove(parent, netuid);
            // Account for the write
            *weight = weight.saturating_add(T::DbWeight::get().writes(1));
        }
        log::info!(
            target: "runtime",
            "Removed {} empty parentkey vectors.",
            to_remove.len()
        );
    }

    /// Make ChildKeys and ParentKeys bidirectionally consistent by
    /// **removing** entries that don't have a matching counterpart.
    /// A match means the exact tuple `(p, other_id)` is present on the opposite map.
    ///
    /// Rules:
    /// - For each (parent, netuid) -> [(p, child)...] in ChildKeys:
    ///   keep only those (p, child) that appear in ParentKeys(child, netuid) as (p, parent).
    ///   If resulting list is empty, remove the key.
    /// - For each (child, netuid) -> [(p, parent)...] in ParentKeys:
    ///   keep only those (p, parent) that appear in ChildKeys(parent, netuid) as (p, child).
    ///   If resulting list is empty, remove the key.
    pub fn repair_child_parent_consistency(weight: &mut Weight) {
        // -------------------------------
        // 1) Prune ChildKeys by checking ParentKeys
        // -------------------------------
        let mut ck_updates: Vec<((T::AccountId, NetUid), Vec<(u64, T::AccountId)>)> = Vec::new();
        let mut ck_removes: Vec<(T::AccountId, NetUid)> = Vec::new();

        for (parent, netuid, children) in ChildKeys::<T>::iter() {
            *weight = weight.saturating_add(T::DbWeight::get().reads(1));

            // Keep (p, child) only if ParentKeys(child, netuid) contains (p, parent)
            let mut filtered: Vec<(u64, T::AccountId)> = Vec::with_capacity(children.len());
            for (p, child) in children.clone().into_iter() {
                let rev = ParentKeys::<T>::get(&child, netuid);
                *weight = weight.saturating_add(T::DbWeight::get().reads(1));
                let has_match = rev.iter().any(|(pr, pa)| *pr == p && *pa == parent);
                if has_match {
                    filtered.push((p, child));
                }
            }

            if filtered.is_empty() {
                ck_removes.push((parent, netuid));
            } else {
                // Only write if changed
                if children != filtered {
                    ck_updates.push(((parent, netuid), filtered));
                }
            }
        }

        for ((parent, netuid), new_vec) in &ck_updates {
            Self::set_childkeys(parent.clone(), *netuid, new_vec.clone());
            *weight = weight.saturating_add(T::DbWeight::get().writes(1));
        }
        for (parent, netuid) in &ck_removes {
            ChildKeys::<T>::remove(parent, netuid);
            *weight = weight.saturating_add(T::DbWeight::get().writes(1));
        }
        log::info!(
            target: "runtime",
            "Updated {} childkey inconsistent records.",
            ck_updates.len()
        );
        log::info!(
            target: "runtime",
            "Removed {} childkey inconsistent records.",
            ck_removes.len()
        );

        // -------------------------------
        // 2) Prune ParentKeys by checking ChildKeys
        // -------------------------------
        let mut pk_updates: Vec<((T::AccountId, NetUid), Vec<(u64, T::AccountId)>)> = Vec::new();
        let mut pk_removes: Vec<(T::AccountId, NetUid)> = Vec::new();

        for (child, netuid, parents) in ParentKeys::<T>::iter() {
            *weight = weight.saturating_add(T::DbWeight::get().reads(1));

            // Keep (p, parent) only if ChildKeys(parent, netuid) contains (p, child)
            let mut filtered: Vec<(u64, T::AccountId)> = Vec::with_capacity(parents.len());
            for (p, parent) in parents.clone().into_iter() {
                let fwd = ChildKeys::<T>::get(&parent, netuid);
                *weight = weight.saturating_add(T::DbWeight::get().reads(1));
                let has_match = fwd.iter().any(|(pr, ch)| *pr == p && *ch == child);
                if has_match {
                    filtered.push((p, parent));
                }
            }

            if filtered.is_empty() {
                pk_removes.push((child, netuid));
            } else {
                // Only write if changed
                if parents != filtered {
                    pk_updates.push(((child, netuid), filtered));
                }
            }
        }

        for ((child, netuid), new_vec) in &pk_updates {
            Self::set_parentkeys(child.clone(), *netuid, new_vec.clone());
            *weight = weight.saturating_add(T::DbWeight::get().writes(1));
        }
        for (child, netuid) in &pk_removes {
            ParentKeys::<T>::remove(child, netuid);
            *weight = weight.saturating_add(T::DbWeight::get().writes(1));
        }
        log::info!(
            target: "runtime",
            "Updated {} parentkey inconsistent records.",
            pk_updates.len()
        );
        log::info!(
            target: "runtime",
            "Removed {} parentkey inconsistent records.",
            pk_removes.len()
        );
    }
}
