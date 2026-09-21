use frame_support::storage::{TransactionOutcome, with_transaction};
use frame_support::weights::Weight;

use super::*;
use crate::subnets::leasing::LeaseId;
use crate::weights::WeightInfo;
use sp_core::Get;
use sp_std::collections::btree_set::BTreeSet;

/// Stake work a coldkey swap moves: `StakingHotkeys` entries visited and positions moved.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ColdkeySwapWork {
    pub hotkeys: u32,
    pub positions: u32,
}

impl<T: Config> Pallet<T> {
    /// Weight of a coldkey swap over `base` (the benchmarked fixed part): one `transfer_stake`
    /// per position moved plus the per-hotkey bookkeeping reads (watermark, unlock age,
    /// root stake) for every `StakingHotkeys` entry.
    fn coldkey_swap_weight(base: Weight, work: ColdkeySwapWork) -> Weight {
        base.saturating_add(
            <T as crate::pallet::Config>::WeightInfo::transfer_stake()
                .saturating_mul(u64::from(work.positions)),
        )
        .saturating_add(T::DbWeight::get().reads(u64::from(work.hotkeys).saturating_mul(3)))
    }

    /// The largest swap one call admits, used as the pre-dispatch envelope.
    fn coldkey_swap_max_work() -> ColdkeySwapWork {
        ColdkeySwapWork {
            hotkeys: crate::MAX_COLDKEY_SWAP_HOTKEYS,
            positions: crate::MAX_COLDKEY_SWAP_POSITIONS,
        }
    }

    /// Pre-dispatch weight of `swap_coldkey`, refunded to the actual work post-dispatch.
    pub fn swap_coldkey_declared_weight() -> Weight {
        Self::coldkey_swap_weight(
            <T as crate::pallet::Config>::WeightInfo::swap_coldkey(),
            Self::coldkey_swap_max_work(),
        )
    }

    /// Post-dispatch weight of `swap_coldkey`.
    pub fn swap_coldkey_actual_weight(work: ColdkeySwapWork) -> Weight {
        Self::coldkey_swap_weight(
            <T as crate::pallet::Config>::WeightInfo::swap_coldkey(),
            work,
        )
    }

    /// Pre-dispatch weight of `swap_coldkey_announced`, refunded post-dispatch.
    pub fn swap_coldkey_announced_declared_weight() -> Weight {
        Self::coldkey_swap_weight(
            <T as crate::pallet::Config>::WeightInfo::swap_coldkey_announced(),
            Self::coldkey_swap_max_work(),
        )
    }

    /// Post-dispatch weight of `swap_coldkey_announced`.
    pub fn swap_coldkey_announced_actual_weight(work: ColdkeySwapWork) -> Weight {
        Self::coldkey_swap_weight(
            <T as crate::pallet::Config>::WeightInfo::swap_coldkey_announced(),
            work,
        )
    }

    /// Stake positions the swap has to move, read before any mutation so the caller can
    /// refuse an oversized swap and price the work it admits.
    pub fn coldkey_swap_work(old_coldkey: &T::AccountId) -> ColdkeySwapWork {
        let mut work = ColdkeySwapWork::default();
        for hotkey in StakingHotkeys::<T>::get(old_coldkey) {
            work.hotkeys = work.hotkeys.saturating_add(1);
            let netuids: BTreeSet<NetUid> = Self::alpha_iter_prefix((&hotkey, old_coldkey))
                .map(|(netuid, _)| netuid)
                .collect();
            work.positions = work.positions.saturating_add(netuids.len() as u32);
        }
        work
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
    /// the admission scan (one `StakingHotkeys` read plus one read per position) when
    /// refused as too heavy, and the admitted work when a later step rolled it back.
    fn coldkey_swap_failed_weight(work: Option<ColdkeySwapWork>, admitted: bool) -> Weight {
        let precheck = T::DbWeight::get().reads(4);
        match work {
            None => precheck,
            Some(work) if !admitted => precheck.saturating_add(
                T::DbWeight::get().reads(
                    u64::from(work.hotkeys)
                        .saturating_add(u64::from(work.positions))
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
        let refused =
            |error: Error<T>| (Self::coldkey_swap_failed_weight(None, false), error.into());
        // The multi-block seed may still hold `RootClaimed[(netuid, hotkey, old_coldkey)]`
        // rows and mid-hotkey `BasketClaimed` writes. Moving root stake + only the new
        // watermark would leave legacy claims on the dead coldkey.
        Self::ensure_beta_basket_seed_idle().map_err(refused)?;
        if !StakingHotkeys::<T>::get(new_coldkey).is_empty() {
            return Err(refused(Error::<T>::ColdKeyAlreadyAssociated));
        }
        if Self::hotkey_account_exists(new_coldkey) {
            return Err(refused(Error::<T>::NewColdKeyIsHotkey));
        }
        // Admission: the stake move is one `transfer_stake` per position and the declared
        // weight reserves a fixed number of them, so refuse (before any write) a coldkey
        // whose list or position count exceeds what one call is priced for.
        let work = Self::coldkey_swap_work(old_coldkey);
        if work.hotkeys > crate::MAX_COLDKEY_SWAP_HOTKEYS
            || work.positions > crate::MAX_COLDKEY_SWAP_POSITIONS
        {
            return Err((
                Self::coldkey_swap_failed_weight(Some(work), false),
                Error::<T>::ColdkeySwapTooHeavy.into(),
            ));
        }

        Self::execute_swap_coldkey(old_coldkey, new_coldkey, work)
            .map_err(|error| (Self::coldkey_swap_failed_weight(Some(work), true), error))
    }

    /// The transactional body of a coldkey swap whose `work` was admitted.
    fn execute_swap_coldkey(
        old_coldkey: &T::AccountId,
        new_coldkey: &T::AccountId,
        work: ColdkeySwapWork,
    ) -> Result<ColdkeySwapWork, DispatchError> {
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

                // Lease rows outlive `NetworksAdded` while a dissolved subnet awaits cleanup,
                // so discover them through the lease index rather than the active-subnet list.
                for (_, lease_id) in SubnetUidToLeaseId::<T>::iter() {
                    Self::transfer_coldkey_lease(lease_id, old_coldkey, new_coldkey)?;
                }

                // Move stake by the coldkey's actual positions (not every subnet × every
                // hotkey), then the per-subnet ownership, auto-stake and collateral state.
                Self::transfer_coldkey_positions(old_coldkey, new_coldkey);
                for netuid in Self::get_all_subnet_netuids() {
                    Self::transfer_subnet_ownership(netuid, old_coldkey, new_coldkey);
                    Self::transfer_auto_stake_destination(netuid, old_coldkey, new_coldkey);
                    // Stake has moved; migrate the bond so unstake guards stay attached.
                    Self::transfer_coldkey_miner_collateral(netuid, old_coldkey, new_coldkey)?;
                }
                Self::transfer_staking_hotkeys(old_coldkey, new_coldkey);
                Self::transfer_hotkeys_ownership(old_coldkey, new_coldkey)?;

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
            SubnetLeaseUnpaidDividends::<T>::mutate(lease_id, new_coldkey, |destination| {
                *destination = destination.saturating_add(unpaid);
            });
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

    /// Move every stake position of `old_coldkey` to `new_coldkey`, one `(hotkey, netuid)`
    /// row at a time. Walking the coldkey's own rows (instead of every subnet for every
    /// hotkey) keeps the work proportional to what actually moves. Root bookkeeping —
    /// basket watermark and unlock age — is moved for every staking hotkey, stake or not:
    /// the signed watermark can be negative with zero stake (claim-then-unstake) and must
    /// follow the coldkey rather than be orphaned on the dead key.
    fn transfer_coldkey_positions(old_coldkey: &T::AccountId, new_coldkey: &T::AccountId) {
        for hotkey in StakingHotkeys::<T>::get(old_coldkey) {
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

            Self::transfer_basket_claimed_for_new_coldkey(&hotkey, old_coldkey, new_coldkey);
            Self::migrate_root_stake_age(old_coldkey, &hotkey, new_coldkey, &hotkey);
            if !Self::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, new_coldkey, NetUid::ROOT)
                .is_zero()
            {
                Self::maybe_add_coldkey_index(new_coldkey);
            }
        }

        // All root stake has left the old coldkey; drop its auto-claim index entry.
        Self::maybe_remove_coldkey_index(old_coldkey);
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
        if new_staking_hotkeys.is_empty() {
            StakingHotkeys::<T>::remove(new_coldkey);
        } else {
            StakingHotkeys::<T>::insert(new_coldkey, new_staking_hotkeys);
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
