use super::*;

impl<T: Config> Pallet<T> {
    /// Transfer all assets, stakes, subnet ownerships, and hotkey associations from `old_coldkey` to
    /// to `new_coldkey`.
    pub fn do_swap_coldkey(
        old_coldkey: &T::AccountId,
        new_coldkey: &T::AccountId,
    ) -> DispatchResult {
        ensure!(
            StakingHotkeys::<T>::get(new_coldkey).is_empty(),
            Error::<T>::ColdKeyAlreadyAssociated
        );
        ensure!(
            !Self::hotkey_account_exists(new_coldkey),
            Error::<T>::NewColdKeyIsHotkey
        );

        // Swap the identity if the old coldkey has one and the new coldkey doesn't
        if IdentitiesV2::<T>::get(new_coldkey).is_none()
            && let Some(identity) = IdentitiesV2::<T>::take(old_coldkey)
        {
            IdentitiesV2::<T>::insert(new_coldkey.clone(), identity);
        }

        // Temporarily allow the destination coldkey to receive this stake even if some of it is
        // locked; swap_coldkey_locks will copy the source AccountFlags over afterward.
        Self::set_accept_locked_alpha(new_coldkey, true);

        for netuid in Self::get_all_subnet_netuids() {
            Self::transfer_subnet_ownership(netuid, old_coldkey, new_coldkey);
            Self::transfer_auto_stake_destination(netuid, old_coldkey, new_coldkey);
            Self::transfer_coldkey_stake(netuid, old_coldkey, new_coldkey);
        }
        Self::transfer_staking_hotkeys(old_coldkey, new_coldkey);
        Self::transfer_hotkeys_ownership(old_coldkey, new_coldkey)?;

        // Transfer stake locks
        Self::swap_coldkey_locks(old_coldkey, new_coldkey)?;

        // Transfer any remaining balance from old_coldkey to new_coldkey
        Self::transfer_all_tao_and_kill(old_coldkey, new_coldkey)?;

        Self::deposit_event(Event::ColdkeySwapped {
            old_coldkey: old_coldkey.clone(),
            new_coldkey: new_coldkey.clone(),
        });
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
    fn transfer_auto_stake_destination(
        netuid: NetUid,
        old_coldkey: &T::AccountId,
        new_coldkey: &T::AccountId,
    ) {
        if let Some(old_auto_stake_hotkey) = AutoStakeDestination::<T>::get(old_coldkey, netuid) {
            AutoStakeDestination::<T>::remove(old_coldkey, netuid);
            AutoStakeDestination::<T>::insert(new_coldkey, netuid, old_auto_stake_hotkey.clone());
            AutoStakeDestinationColdkeys::<T>::mutate(old_auto_stake_hotkey, netuid, |v| {
                // Remove old/new coldkeys (avoid duplicates), then add the new one.
                v.retain(|c| *c != *old_coldkey && *c != *new_coldkey);
                v.push(new_coldkey.clone());
            });
        }
    }

    /// Transfer the stake of all staking hotkeys linked to the old coldkey to the new coldkey.
    fn transfer_coldkey_stake(
        netuid: NetUid,
        old_coldkey: &T::AccountId,
        new_coldkey: &T::AccountId,
    ) {
        for hotkey in StakingHotkeys::<T>::get(old_coldkey) {
            // Swap
            let alpha_old =
                Self::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, old_coldkey, netuid);
            Self::decrease_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                old_coldkey,
                netuid,
                alpha_old,
            );
            Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                new_coldkey,
                netuid,
                alpha_old,
            );
            let new_dest_alpha =
                Self::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, new_coldkey, netuid);

            if !new_dest_alpha.is_zero() {
                Self::transfer_root_claimed_for_new_keys(
                    netuid,
                    &hotkey,
                    &hotkey,
                    old_coldkey,
                    new_coldkey,
                );

                if netuid == NetUid::ROOT {
                    // Register new coldkey with root stake
                    Self::maybe_add_coldkey_index(new_coldkey);
                }
            }
        }

        // All of the old coldkey's root stake for this subnet has been moved to the new
        // coldkey, so the old coldkey no longer holds any root stake. Remove its stale
        // entry from the auto-claim staking-coldkey index (it is added for new_coldkey
        // above) so swaps do not orphan dead entries.
        if netuid == NetUid::ROOT {
            Self::maybe_remove_coldkey_index(old_coldkey);
        }
    }

    /// Transfer staking hotkeys from the old coldkey to the new coldkey.
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
        StakingHotkeys::<T>::insert(new_coldkey, new_staking_hotkeys);
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
            // Addd the owned hotkey to the new set of owned hotkeys.
            if !new_owned_hotkeys.contains(owned_hotkey) {
                new_owned_hotkeys.push(owned_hotkey.clone());
            }
        }
        OwnedHotkeys::<T>::remove(old_coldkey);
        OwnedHotkeys::<T>::insert(new_coldkey, new_owned_hotkeys);
        Ok(())
    }
}
