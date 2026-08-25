use super::*;
use subtensor_runtime_common::NetUid;

impl<T: Config> Pallet<T> {
    /// Protocol-initiated childkey from a root validator to one subnet owner.
    ///
    /// Skips user-extrinsic guards (rate limit, min stake, cooldown) so
    /// registration and new-subnet hooks can establish the link even
    /// before the validator has staked. Applies immediately via
    /// `persist_pending_chidren_ok`. Leaves an existing or pending child
    /// set untouched so a validator who already chose children is not
    /// overwritten.
    fn apply_auto_parent_to_owner(
        parent_hotkey: &T::AccountId,
        netuid: NetUid,
        owner_hotkey: T::AccountId,
    ) {
        if netuid.is_root() || !Self::if_subnet_exist(netuid) {
            return;
        }
        if *parent_hotkey == owner_hotkey {
            return;
        }
        if !Self::get_auto_parent_delegation_enabled(parent_hotkey) {
            return;
        }
        if !ChildKeys::<T>::get(parent_hotkey, netuid).is_empty()
            || PendingChildKeys::<T>::contains_key(netuid, parent_hotkey)
        {
            return;
        }

        Self::persist_pending_chidren_ok(netuid, parent_hotkey, &vec![(u64::MAX, owner_hotkey)]);
    }

    /// True when this parent still has the protocol auto-parent edge
    /// this module writes: one child, full proportion. The child may be
    /// a former owner after lock takeover; matching only the current
    /// `SubnetOwnerHotkey` would skip those leftovers. A validator who
    /// set a different child set (other proportion or more than one
    /// child) is left alone.
    fn is_protocol_auto_parent_edge(parent_hotkey: &T::AccountId, netuid: NetUid) -> bool {
        matches!(
            ChildKeys::<T>::get(parent_hotkey, netuid).as_slice(),
            [(u64::MAX, _)]
        )
    }

    /// Drop protocol auto-parent edges after a hotkey leaves root.
    ///
    /// `replace_neuron` on root does not walk other subnets. Without this
    /// cleanup, each churned validator would leave a `ParentKeys` row on
    /// every owner, and the epoch inherited-stake walk would grow without
    /// bound.
    pub fn clear_auto_parent_for_root_validator(root_validator_hotkey: &T::AccountId) {
        for netuid in Self::get_all_subnet_netuids() {
            if netuid.is_root() {
                continue;
            }
            if !Self::is_protocol_auto_parent_edge(root_validator_hotkey, netuid) {
                continue;
            }
            Self::persist_pending_chidren_ok(netuid, root_validator_hotkey, &Vec::new());
        }
    }

    /// Move protocol auto-parent edges from `old_owner` to `new_owner`.
    ///
    /// Lock takeover and `set_subnet_owner_hotkey` change the owner
    /// without rewriting childkeys. Until this runs, every root
    /// validator still parents the former owner, who keeps inheriting
    /// stake. Custom child sets are left alone.
    pub fn retarget_auto_parent_on_owner_change(
        netuid: NetUid,
        old_owner: &T::AccountId,
        new_owner: &T::AccountId,
    ) {
        if netuid.is_root() || old_owner == new_owner {
            return;
        }

        for (_uid, parent_hotkey) in Keys::<T>::iter_prefix(NetUid::ROOT) {
            if ChildKeys::<T>::get(&parent_hotkey, netuid) != vec![(u64::MAX, old_owner.clone())] {
                continue;
            }
            if parent_hotkey == *new_owner {
                Self::persist_pending_chidren_ok(netuid, &parent_hotkey, &Vec::new());
            } else {
                Self::persist_pending_chidren_ok(
                    netuid,
                    &parent_hotkey,
                    &vec![(u64::MAX, new_owner.clone())],
                );
            }
        }
    }

    /// Childkey every root validator to the owner of `netuid`.
    ///
    /// Used when a new subnet is registered. Setup fails if `netuid` is
    /// root or has no owner. Per-validator apply never fails the loop.
    pub fn do_set_root_validators_for_subnet(netuid: NetUid) -> DispatchResult {
        ensure!(
            !netuid.is_root(),
            Error::<T>::RegistrationNotPermittedOnRootSubnet
        );
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        let subnet_owner_hotkey =
            SubnetOwnerHotkey::<T>::try_get(netuid).map_err(|_| Error::<T>::SubnetNotExists)?;

        for (_uid, root_validator_hotkey) in Keys::<T>::iter_prefix(NetUid::ROOT) {
            Self::apply_auto_parent_to_owner(
                &root_validator_hotkey,
                netuid,
                subnet_owner_hotkey.clone(),
            );
        }
        Ok(())
    }

    /// Childkey one root validator to every existing subnet owner.
    ///
    /// Used after `root_register`. Respects `AutoParentDelegationEnabled`
    /// (default true). Never fails the registration.
    pub fn do_set_subnet_owners_for_root_validator(root_validator_hotkey: &T::AccountId) {
        if !Self::get_auto_parent_delegation_enabled(root_validator_hotkey) {
            return;
        }

        for netuid in Self::get_all_subnet_netuids() {
            let Ok(subnet_owner_hotkey) = SubnetOwnerHotkey::<T>::try_get(netuid) else {
                continue;
            };
            Self::apply_auto_parent_to_owner(root_validator_hotkey, netuid, subnet_owner_hotkey);
        }
    }
}
