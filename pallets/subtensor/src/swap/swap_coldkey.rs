//! Coldkey identity swap: migrate economic ownership from one coldkey SS58 to another.
//!
//! Entry point: [`Pallet::perform_coldkey_swap`]. Runs inside a storage transaction so a
//! late failure (e.g. collateral index) rolls back stake, ownership, locks, and
//! identity writes together. After success, [`Pallet::record_coldkey_swap_lineage`]
//! records global root/successor continuity.
//!
//! Deliberately does **not** move stake into a destination that already has
//! [`StakingHotkeys`] entries or that is itself a registered hotkey.

use frame_support::storage::{TransactionOutcome, with_transaction};

use super::*;

impl<T: Config> Pallet<T> {
    /// Migrate all coldkey-keyed state from `old_coldkey` to `new_coldkey`.
    ///
    /// Transfers subnet ownership, auto-stake destinations, per-subnet alpha stake,
    /// miner collateral bonds, staking-hotkey indexes, owned-hotkey associations,
    /// stake locks, and remaining free TAO. Records coldkey lineage and emits
    /// [`Event::ColdkeySwapped`] on success.
    ///
    /// Rejects when `new_coldkey` already has staking associations or is a hotkey.
    pub fn perform_coldkey_swap(
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

        with_transaction(|| {
            let result = (|| -> DispatchResult {
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
                    Self::transfer_coldkey_subnet_ownership(netuid, old_coldkey, new_coldkey);
                    Self::transfer_coldkey_auto_stake_destination(netuid, old_coldkey, new_coldkey);
                    Self::transfer_coldkey_subnet_stake(netuid, old_coldkey, new_coldkey);
                    // Stake has moved; migrate the bond so unstake guards stay attached.
                    Self::transfer_coldkey_miner_collateral(netuid, old_coldkey, new_coldkey)?;
                }
                Self::transfer_coldkey_staking_hotkeys(old_coldkey, new_coldkey);
                Self::transfer_coldkey_owned_hotkeys(old_coldkey, new_coldkey)?;

                // Transfer stake locks
                Self::swap_coldkey_locks(old_coldkey, new_coldkey)?;

                // Transfer any remaining balance from old_coldkey to new_coldkey
                Self::transfer_all_tao_and_kill(old_coldkey, new_coldkey)?;

                // Owner identity continuity for indexers / coldkey-keyed policy.
                Self::record_coldkey_swap_lineage(old_coldkey, new_coldkey);

                Self::deposit_event(Event::ColdkeySwapped {
                    old_coldkey: old_coldkey.clone(),
                    new_coldkey: new_coldkey.clone(),
                });
                Ok(())
            })();

            match result {
                Ok(()) => TransactionOutcome::Commit(Ok(())),
                Err(e) => TransactionOutcome::Rollback(Err(e)),
            }
        })
    }

    /// Recycle `swap_cost` TAO from `coldkey` as the coldkey-swap fee.
    ///
    /// Maps insufficient free balance to [`Error::NotEnoughBalanceToPaySwapColdKey`].
    pub fn charge_coldkey_swap_cost(coldkey: &T::AccountId, swap_cost: TaoBalance) -> DispatchResult {
        Self::recycle_tao(coldkey, swap_cost)
            .map_err(|_| Error::<T>::NotEnoughBalanceToPaySwapColdKey)?;
        Ok(())
    }

    /// If `old_coldkey` owns `netuid`, rewrite [`SubnetOwner`] to `new_coldkey`.
    fn transfer_coldkey_subnet_ownership(
        netuid: NetUid,
        old_coldkey: &T::AccountId,
        new_coldkey: &T::AccountId,
    ) {
        let subnet_owner = SubnetOwner::<T>::get(netuid);
        if subnet_owner == *old_coldkey {
            SubnetOwner::<T>::insert(netuid, new_coldkey.clone());
        }
    }

    /// Move [`AutoStakeDestination`] / reverse index from `old_coldkey` to `new_coldkey` on `netuid`.
    fn transfer_coldkey_auto_stake_destination(
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

    /// Move every (hotkey, coldkey, netuid) alpha position for `old_coldkey` onto `new_coldkey`.
    ///
    /// Also migrates root-claimed rows and maintains the root auto-claim coldkey index.
    fn transfer_coldkey_subnet_stake(
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

    /// Merge [`StakingHotkeys`] from `old_coldkey` into `new_coldkey`, then clear the old list.
    fn transfer_coldkey_staking_hotkeys(old_coldkey: &T::AccountId, new_coldkey: &T::AccountId) {
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

    /// Reassign [`Owner`] / [`OwnedHotkeys`] so every hotkey owned by `old_coldkey` is owned by `new_coldkey`.
    fn transfer_coldkey_owned_hotkeys(
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
