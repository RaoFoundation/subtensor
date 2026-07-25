//! Weight-metered cleanup / repair of child–parent storage inconsistencies.
use super::*;
use subtensor_runtime_common::NetUid;

impl<T: Config> Pallet<T> {
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
    /// * For each (parent, netuid) -> [(p, child)...] in ChildKeys:
    ///   keep only those (p, child) that appear in ParentKeys(child, netuid) as (p, parent).
    ///   If resulting list is empty, remove the key.
    /// * For each (child, netuid) -> [(p, parent)...] in ParentKeys:
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
